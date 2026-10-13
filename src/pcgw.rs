//! PCGamingWiki: genres, themes, perspectives, pacing, modes, art styles and
//! series from a game's infobox taxonomy. No key. The page comes from
//! Wikidata, the Steam-appid redirect, or an exact title search.

use crate::http::{Backoff, RateLimiter, base_url, send_with_backoff};
use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

const BASE: &str = "https://www.pcgamingwiki.com";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PcgwMeta {
    pub page: String,
    pub genres: Vec<String>,
    pub themes: Vec<String>,
    pub perspectives: Vec<String>,
    pub pacing: Vec<String>,
    pub modes: Vec<String>,
    pub controls: Vec<String>,
    pub art_styles: Vec<String>,
    pub series: Vec<String>,
}

pub struct PcgwClient {
    http: Client,
    base: String,
    /// Separate client that does not follow redirects (for the appid lookup).
    no_redirect: Client,
    limiter: RateLimiter,
}

/// Drop `<!-- ... -->` placeholders the page template leaves in empty fields.
fn strip_comments(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("<!--") {
        out.push_str(&rest[..i]);
        rest = rest[i..].find("-->").map(|j| &rest[i + j + 3..]).unwrap_or("");
    }
    out.push_str(rest);
    out
}

fn split_list(s: &str) -> Vec<String> {
    strip_comments(s).split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
}

/// Parse `{{Infobox game/row/taxonomy/<field> | a, b }}` rows.
pub fn parse_taxonomy(page: &str, wikitext: &str) -> PcgwMeta {
    let mut m = PcgwMeta { page: page.to_string(), ..Default::default() };
    for line in wikitext.lines() {
        let Some(rest) = line.trim().strip_prefix("{{Infobox game/row/taxonomy/") else { continue };
        let Some((field, value)) = rest.split_once('|') else { continue };
        let value = value.trim().trim_end_matches("}}").trim();
        let list = split_list(value);
        match field.trim() {
            "genres" => m.genres = list,
            "themes" => m.themes = list,
            "perspectives" => m.perspectives = list,
            "pacing" => m.pacing = list,
            "modes" => m.modes = list,
            "controls" => m.controls = list,
            "art styles" => m.art_styles = list,
            "series" => m.series = list,
            _ => {}
        }
    }
    m
}

impl PcgwClient {
    /// `base` stands in for pcgamingwiki.com (tests pass a local mock server).
    pub fn new(http: Client, base: Option<&str>) -> Self {
        let no_redirect = reqwest::Client::builder()
            .user_agent(crate::http::USER_AGENT)
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("reqwest client");
        Self { http, base: base_url(BASE, base), no_redirect, limiter: RateLimiter::new("PCGamingWiki", Duration::from_millis(1000)) }
    }

    /// Page title for a Steam appid via PCGamingWiki's redirect helper.
    pub async fn page_for_appid(&self, appid: u32) -> Result<Option<String>> {
        if !self.limiter.wait().await {
            return Err(crate::http::stopped());
        }
        let resp = self.no_redirect.get(format!("{}/api/appid.php", self.base)).query(&[("appid", appid.to_string())]).send().await?;
        let loc = resp.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default();
        Ok(page_from_location(loc))
    }

    /// Page title whose normalized name equals `name`, via opensearch.
    pub async fn page_for_name(&self, name: &str) -> Result<Option<String>> {
        let clean = crate::names::normalize_name(name);
        let resp = send_with_backoff(&self.limiter, &Backoff::default(), || {
            self.http.get(format!("{}/w/api.php", self.base)).query(&[
                ("action", "opensearch"),
                ("search", clean.as_str()),
                ("limit", "5"),
                ("namespace", "0"),
                ("format", "json"),
            ])
        })
        .await?;
        let v: Value = resp.json().await.context("PCGamingWiki opensearch returned non-JSON")?;
        Ok(exact_title(&v, &clean))
    }

    /// Taxonomy for a page; `Ok(None)` when the page does not exist.
    pub async fn meta(&self, page: &str) -> Result<Option<PcgwMeta>> {
        let page = page.replace('_', " ");
        let resp = send_with_backoff(&self.limiter, &Backoff::default(), || {
            self.http.get(format!("{}/w/api.php", self.base)).query(&[
                ("action", "parse"),
                ("page", page.as_str()),
                ("prop", "wikitext"),
                ("redirects", "1"),
                ("format", "json"),
                ("formatversion", "2"),
            ])
        })
        .await?;
        let v: Value = resp.json().await.context("PCGamingWiki returned non-JSON")?;
        Ok(parse_page(&v, &page))
    }
}

/// The page named by the appid redirect's Location header.
fn page_from_location(loc: &str) -> Option<String> {
    loc.split("/wiki/").nth(1).map(urlencoding_decode).filter(|p| !p.is_empty() && !p.starts_with("Special:"))
}

/// The opensearch title whose normalized form equals the normalized name `clean`.
fn exact_title(v: &Value, clean: &str) -> Option<String> {
    let want = clean.to_lowercase();
    v.get(1).and_then(Value::as_array).and_then(|titles| {
        titles.iter().filter_map(Value::as_str).find(|t| crate::names::normalize_name(t).to_lowercase() == want).map(str::to_string)
    })
}

/// Taxonomy from an `action=parse` response; `None` for a missing or empty page.
fn parse_page(v: &Value, page: &str) -> Option<PcgwMeta> {
    if v.pointer("/error/code").and_then(Value::as_str) == Some("missingtitle") {
        return None;
    }
    let title = v.pointer("/parse/title").and_then(Value::as_str).unwrap_or(page);
    let text = v.pointer("/parse/wikitext").and_then(Value::as_str).unwrap_or_default();
    if text.is_empty() {
        return None;
    }
    Some(parse_taxonomy(title, text))
}

fn urlencoding_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(b) = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(if bytes[i] == b'_' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn decode_never_splits_characters() {
        assert_eq!(super::urlencoding_decode("Caf%C3%A9_Racer"), "Café Racer");
        let _ = super::urlencoding_decode("%é1");
        let _ = super::urlencoding_decode("ab%");
    }

    use super::*;

    #[test]
    fn taxonomy() {
        let t = "{{Infobox game/row/taxonomy/modes             | Singleplayer, Multiplayer }}\n\
                 {{Infobox game/row/taxonomy/genres            | RPG, Building, Farming, Life sim}}\n\
                 {{Infobox game/row/taxonomy/themes            | }}\n\
                 {{Infobox game/row/taxonomy/art styles        | Anime, Pixel art}}\n\
                 {{Infobox game/row/taxonomy/series            | <!-- CHANGE TO THE ACTUAL SERIES NAME IF ONE EXISTS --> }}";
        let m = parse_taxonomy("Stardew Valley", t);
        assert_eq!(m.genres, vec!["RPG", "Building", "Farming", "Life sim"]);
        assert_eq!(m.modes, vec!["Singleplayer", "Multiplayer"]);
        assert!(m.themes.is_empty());
        assert_eq!(m.art_styles, vec!["Anime", "Pixel art"]);
        assert!(m.series.is_empty());
    }

    #[test]
    fn page_response() {
        use crate::testutil::fixture_json;
        let m = parse_page(&fixture_json("pcgw/parse_page.json"), "Portal_2").unwrap();
        assert_eq!(m.page, "Portal 2");
        assert_eq!(m.genres, ["Platform", "Puzzle"]);
        assert_eq!(m.themes, ["Sci-fi", "North America", "Post-apocalyptic"]);
        assert_eq!((m.perspectives, m.pacing), (vec!["First-person".to_string()], vec!["Real-time".to_string()]));
        assert_eq!((m.art_styles, m.series), (vec!["Realistic".to_string()], vec!["Portal".to_string()]));
        assert!(parse_page(&fixture_json("pcgw/parse_missing.json"), "Nope").is_none());
        assert!(parse_page(&serde_json::json!({"parse": {"title": "X", "wikitext": ""}}), "X").is_none());
    }

    #[test]
    fn search_and_redirect() {
        let v = crate::testutil::fixture_json("pcgw/opensearch_portal2.json");
        assert_eq!(exact_title(&v, "Portal 2").as_deref(), Some("Portal 2"));
        assert_eq!(exact_title(&v, "Portal 3"), None);
        assert_eq!(page_from_location("https://www.pcgamingwiki.com/wiki/Hades_II%3A_Early").as_deref(), Some("Hades II: Early"));
        assert_eq!(page_from_location("https://www.pcgamingwiki.com/wiki/Special:Search"), None);
        assert_eq!(page_from_location(""), None);
    }

    #[test]
    fn decode() {
        assert_eq!(urlencoding_decode("Hades_II%3A_Early"), "Hades II: Early");
    }

    /// A job asked to stop doesn't send the request its throttle was holding.
    #[tokio::test]
    async fn a_stop_sends_nothing() {
        let server = wiremock::MockServer::start().await;
        let pcgw = PcgwClient::new(Client::new(), Some(&server.uri()));
        pcgw.limiter.penalize(Duration::from_secs(60)).await;
        let dir = tempfile::tempdir().unwrap();
        let cfg = crate::testutil::offline_config(dir.path());
        let db = std::sync::Arc::new(crate::db::Db::open(&cfg.db_path, 120).unwrap());
        let job =
            crate::jobs::LiveJob::begin(db, "enrich", None, crate::db::JobSource::Terminal, serde_json::json!({}), None).unwrap().unwrap();
        job.cancel_now();
        let err = job.scope(pcgw.page_for_appid(620)).await.unwrap_err();
        assert_eq!(err.to_string(), "stopped waiting: the job was asked to stop");
        assert!(server.received_requests().await.unwrap().is_empty());
    }
}
