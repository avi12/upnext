//! SteamSpy: user-voted tags ("Relaxing", "Story Rich", ...), which the store
//! API does not expose, and the catalogue sorted by owners that UpNext's
//! shared data is chosen from. Unofficial, ~1 request/second, and one a
//! minute for the catalogue.

use crate::http::{Backoff, RateLimiter, base_url, send_with_backoff};
use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

const BASE: &str = "https://steamspy.com";

pub struct SteamSpyClient {
    http: Client,
    base: String,
    limiter: RateLimiter,
    /// SteamSpy asks for at most one catalogue page a minute.
    all_limiter: RateLimiter,
}

/// One app on SteamSpy's catalogue (`request=all`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listed {
    pub appid: u32,
    pub name: String,
    /// The low end of SteamSpy's owners estimate ("2,000,000 .. 5,000,000").
    pub owners: u64,
    /// Steam reviews either way, which orders games within an owners band.
    pub reviews: u64,
}

impl SteamSpyClient {
    /// `base` stands in for steamspy.com (tests pass a local mock server).
    pub fn new(http: Client, base: Option<&str>) -> Self {
        Self {
            http,
            base: base_url(BASE, base),
            limiter: RateLimiter::new("SteamSpy", Duration::from_millis(1100)),
            all_limiter: RateLimiter::new("SteamSpy", Duration::from_secs(61)),
        }
    }

    /// One page (1,000 apps) of SteamSpy's catalogue, which it sorts by
    /// owners; empty past the last page.
    pub async fn all_page(&self, page: u32) -> Result<Vec<Listed>> {
        let resp = send_with_backoff(&self.all_limiter, &Backoff::default(), || {
            self.http.get(format!("{}/api.php", self.base)).query(&[("request", "all"), ("page", &page.to_string())])
        })
        .await?;
        let body: Value = resp.json().await.context("steamspy returned non-JSON")?;
        Ok(parse_all(&body))
    }

    /// Tags sorted by votes desc. `Ok(None)` when SteamSpy does not know the app.
    pub async fn tags(&self, appid: u32) -> Result<Option<Vec<(String, i64)>>> {
        let resp = send_with_backoff(&self.limiter, &Backoff::default(), || {
            self.http.get(format!("{}/api.php", self.base)).query(&[("request", "appdetails"), ("appid", &appid.to_string())])
        })
        .await?;
        let body: Value = resp.json().await.context("steamspy returned non-JSON")?;
        Ok(parse_tags(&body))
    }
}

/// Tags from an `appdetails` response, sorted by votes desc; `None` when
/// SteamSpy does not know the app.
fn parse_tags(body: &Value) -> Option<Vec<(String, i64)>> {
    let known = body.get("name").and_then(Value::as_str).map(|n| !n.is_empty()).unwrap_or(false)
        || body.get("appid").and_then(Value::as_i64).is_some();
    if !known {
        return None;
    }
    let mut tags: Vec<(String, i64)> = body
        .get("tags")
        .and_then(Value::as_object)
        .map(|o| o.iter().filter_map(|(k, v)| v.as_i64().map(|n| (k.clone(), n))).collect())
        .unwrap_or_default();
    tags.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Some(tags)
}

/// The apps of a catalogue page: an object keyed by appid. Entries without
/// an appid or a name are skipped.
fn parse_all(body: &Value) -> Vec<Listed> {
    let Some(apps) = body.as_object() else { return vec![] };
    let number = |v: Option<&Value>| match v {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0),
        _ => 0,
    };
    apps.values()
        .filter_map(|app| {
            let appid = u32::try_from(number(app.get("appid"))).ok().filter(|&a| a > 0)?;
            let name = app.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty())?.to_string();
            let owners = app
                .get("owners")
                .and_then(Value::as_str)
                .and_then(|o| o.split("..").next())
                .and_then(|low| low.replace([',', ' '], "").parse().ok())
                .unwrap_or(0);
            let reviews = number(app.get("positive")) + number(app.get("negative"));
            Some(Listed { appid, name, owners, reviews })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::fixture_json;

    #[test]
    fn tags_sorted_by_votes() {
        let tags = parse_tags(&fixture_json("steamspy/appdetails_620.json")).unwrap();
        assert_eq!(tags.len(), 5);
        assert!(tags.windows(2).all(|w| w[0].1 >= w[1].1));
        assert_eq!(tags[0], ("Platformer".to_string(), 7477));
    }

    #[test]
    fn catalogue_page() {
        let mut apps = parse_all(&fixture_json("steamspy/all_page0.json"));
        apps.sort_by_key(|a| a.appid);
        assert_eq!(apps.len(), 6, "the entry without a name is skipped: {apps:?}");
        assert_eq!(apps[0], Listed { appid: 10, name: "Counter-Strike".into(), owners: 10_000_000, reviews: 250_000 });
        assert_eq!(parse_all(&serde_json::json!({})), vec![], "past the last page");
        assert_eq!(parse_all(&serde_json::json!([])), vec![]);
    }

    #[test]
    fn unknown_app() {
        // SteamSpy answers unknown appids with appid 0 and a null name; no tags either way.
        let tags = parse_tags(&fixture_json("steamspy/appdetails_unknown.json"));
        assert!(tags.is_none_or(|t| t.is_empty()));
        assert!(parse_tags(&serde_json::json!({})).is_none());
    }
}
