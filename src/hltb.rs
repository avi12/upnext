//! HowLongToBeat lookup. Unofficial: the search endpoint path and an auth
//! token are scraped from the site's JS bundle at runtime, following the same
//! approach as howlongtobeatpy. Everything here is best-effort; callers must
//! treat failures as non-fatal.

use crate::db::HltbRow;
use crate::http::{Backoff, RateLimiter, base_url, send_with_backoff};
use crate::names::normalize_name;
use anyhow::{Context, Result, anyhow};
use regex::Regex;
use reqwest::Client;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::Mutex;

const BASE: &str = "https://howlongtobeat.com/";
const BROWSER_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36";

#[derive(Debug, Clone)]
struct Session {
    /// e.g. "api/s" or "api/search/site"
    search_path: String,
    token: Option<String>,
    key: Option<String>,
    val: Option<String>,
}

pub struct HltbClient {
    http: Client,
    /// The site root, ending in `/`. Referer and Origin always name the real site.
    site: String,
    limiter: RateLimiter,
    session: Mutex<Option<Session>>,
    pub min_similarity: f64,
}

impl HltbClient {
    /// `base` stands in for howlongtobeat.com (tests pass a local mock server).
    pub fn new(http: Client, base: Option<&str>) -> Self {
        Self {
            http,
            site: format!("{}/", base_url(BASE, base)),
            limiter: RateLimiter::new("HowLongToBeat", Duration::from_millis(1500)),
            session: Mutex::new(None),
            min_similarity: 0.55,
        }
    }

    async fn get_text(&self, url: &str) -> Result<String> {
        let resp = send_with_backoff(&self.limiter, &Backoff { attempts: 3, ..Default::default() }, || {
            self.http.get(url).header("User-Agent", BROWSER_UA).header("Referer", BASE)
        })
        .await?;
        Ok(resp.text().await?)
    }

    async fn discover_search_path(&self) -> Result<String> {
        let home = self.get_text(&self.site).await.context("fetching HLTB home page")?;
        let script_re = Regex::new(r#"<script[^>]+src="([^"]*/_next/static/chunks/[^"]+)""#)?;
        let fetch_re = Regex::new(r#"fetch\s*\(\s*["']/api/([a-zA-Z0-9_/]+)[^"']*["']\s*,\s*\{[^}]*method:\s*["']POST["']"#)?;
        let mut srcs: Vec<String> = script_re.captures_iter(&home).map(|c| c[1].to_string()).collect();
        // App chunks are the likeliest place; look at them first.
        srcs.sort_by_key(|s| if s.contains("_app") || s.contains("pages/") { 0 } else { 1 });
        for src in srcs.into_iter().take(40) {
            let url = if src.starts_with("http") { src } else { format!("{}{}", self.site, src.trim_start_matches('/')) };
            let Ok(js) = self.get_text(&url).await else { continue };
            if let Some(c) = fetch_re.captures(&js) {
                return Ok(format!("api/{}", c[1].trim_end_matches('/')));
            }
        }
        Err(anyhow!("could not find HLTB search endpoint in site scripts"))
    }

    async fn session(&self) -> Result<Session> {
        let mut guard = self.session.lock().await;
        if let Some(s) = guard.as_ref() {
            return Ok(s.clone());
        }
        let search_path = match self.discover_search_path().await {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("HLTB: {e}; falling back to api/s");
                "api/s".to_string()
            }
        };
        let mut session = Session { search_path: search_path.clone(), token: None, key: None, val: None };
        let ms = chrono::Utc::now().timestamp_millis();
        let init_url = format!("{}{search_path}/init?t={ms}", self.site);
        match self.get_text(&init_url).await.and_then(|t| Ok(serde_json::from_str::<Value>(&t)?)) {
            Ok(v) => {
                session.token = v.get("token").and_then(Value::as_str).map(str::to_string);
                if let Some(o) = v.as_object() {
                    for (k, val) in o {
                        let lk = k.to_lowercase();
                        if lk.contains("key") {
                            session.key = val.as_str().map(str::to_string).or_else(|| Some(val.to_string()));
                        } else if lk.contains("val") {
                            session.val = val.as_str().map(str::to_string).or_else(|| Some(val.to_string()));
                        }
                    }
                }
            }
            Err(e) => tracing::warn!("HLTB: init token failed ({e}); searching without auth headers"),
        }
        *guard = Some(session.clone());
        Ok(session)
    }

    async fn reset_session(&self) {
        *self.session.lock().await = None;
    }

    async fn search_raw(&self, session: &Session, terms: &[String]) -> Result<Value> {
        let mut payload = json!({
            "searchType": "games",
            "searchTerms": terms,
            "searchPage": 1,
            "size": 20,
            "searchOptions": {
                "games": {
                    "userId": 0, "platform": "", "sortCategory": "popular", "rangeCategory": "main",
                    "rangeTime": {"min": 0, "max": 0},
                    "gameplay": {"perspective": "", "flow": "", "genre": "", "difficulty": ""},
                    "rangeYear": {"max": "", "min": ""},
                    "modifier": ""
                },
                "users": {"sortCategory": "postcount"},
                "lists": {"sortCategory": "follows"},
                "filter": "", "sort": 0, "randomizer": 0
            },
            "useCache": true
        });
        if let (Some(k), Some(v)) = (&session.key, &session.val) {
            payload[k.as_str()] = Value::String(v.clone());
        }
        let url = format!("{}{}", self.site, session.search_path);
        let body = payload.to_string();
        let resp = send_with_backoff(&self.limiter, &Backoff { attempts: 2, ..Default::default() }, || {
            let mut rb = self
                .http
                .post(&url)
                .header("User-Agent", BROWSER_UA)
                .header("Referer", BASE)
                .header("Origin", BASE.trim_end_matches('/'))
                .header("Accept", "*/*")
                .header("Content-Type", "application/json")
                .body(body.clone());
            if let Some(t) = &session.token {
                rb = rb.header("x-auth-token", t);
            }
            if let (Some(k), Some(v)) = (&session.key, &session.val) {
                rb = rb.header("x-hp-key", k).header("x-hp-val", v);
            }
            rb
        })
        .await?;
        resp.json().await.context("HLTB search returned non-JSON")
    }

    /// Find the best match for a Steam game name. `Ok(None)` = no confident match.
    pub async fn lookup(&self, name: &str) -> Result<Option<HltbRow>> {
        let clean = normalize_name(name);
        let terms: Vec<String> = clean.split_whitespace().map(str::to_string).collect();
        if terms.is_empty() {
            return Ok(None);
        }
        let session = self.session().await?;
        let result = match self.search_raw(&session, &terms).await {
            Ok(v) => v,
            Err(e) => {
                // A stale token/endpoint is the usual cause; rebuild once.
                tracing::warn!("HLTB search failed ({e}); refreshing session");
                self.reset_session().await;
                let session = self.session().await?;
                self.search_raw(&session, &terms).await?
            }
        };
        Ok(best_match(&clean, &result, self.min_similarity))
    }
}

/// The search hit that best matches the normalized name `clean`, if it clears `min_similarity`.
fn best_match(clean: &str, result: &Value, min_similarity: f64) -> Option<HltbRow> {
    let data = result.get("data").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut best: Option<(f64, &Value)> = None;
    for g in &data {
        let gname = g.get("game_name").and_then(Value::as_str).unwrap_or("");
        let alias = g.get("game_alias").and_then(Value::as_str).unwrap_or("");
        // "Portal" must not match "Portal 2", nor "Hollow" Hollow Knight.
        let sim = crate::names::score(clean, gname).max(crate::names::score(clean, alias));
        if best.is_none_or(|(b, _)| sim > b) {
            best = Some((sim, g));
        }
    }
    let (sim, g) = best?;
    if sim < min_similarity {
        return None;
    }
    let hours = |k: &str| g.get(k).and_then(Value::as_f64).filter(|v| *v > 0.0).map(|s| (s / 3600.0 * 10.0).round() / 10.0);
    Some(HltbRow {
        hltb_id: g.get("game_id").and_then(Value::as_i64),
        hltb_name: g.get("game_name").and_then(Value::as_str).map(str::to_string),
        similarity: Some((sim * 1000.0).round() / 1000.0),
        main_story: hours("comp_main"),
        main_extra: hours("comp_plus"),
        completionist: hours("comp_100"),
    })
}

#[cfg(test)]
mod match_tests {
    use super::*;
    use crate::testutil::fixture_json;
    use serde_json::json;

    #[test]
    fn best_hit() {
        let v = fixture_json("hltb/search.json");
        let r = best_match(&normalize_name("Portal 2"), &v, 0.8).unwrap();
        assert_eq!((r.hltb_id, r.hltb_name.as_deref()), (Some(7231), Some("Portal 2")));
        assert_eq!((r.main_story, r.main_extra, r.completionist), (Some(8.6), Some(13.5), Some(22.0)));
        // "Portal" matches Portal, not Portal 2; a zero time means unknown.
        let r = best_match(&normalize_name("Portal"), &v, 0.8).unwrap();
        assert_eq!((r.hltb_id, r.main_extra), (Some(7230), None));
        // A number in the request that no result has is a different game.
        assert!(best_match(&normalize_name("Portal 3"), &v, 0.8).is_none());
    }

    /// The code-health review's B-STORES-3, the HowLongToBeat part of the
    /// name matching: a game HowLongToBeat doesn't list no longer takes the times
    /// of a game whose name starts the same, or of the one it is a sequel
    /// to. "Hollow" (a 2017 horror game) took Hollow Knight's at 0.80.
    #[test]
    fn a_short_name_or_a_sequel_doesnt_take_another_games_times() {
        let one = |name: &str| json!({"data": [{"game_id": 1, "game_name": name, "game_alias": "", "comp_main": 97200}]});
        for (query, listed) in [
            ("Hollow", "Hollow Knight"),
            ("Hades II", "Hades"),
            ("Hades", "Hades II"),
            ("Hades 2", "Hades"),
            ("Portal", "Portal 2"),
            ("DOOM", "DOOM Eternal"),
            ("DOOM Eternal", "DOOM"),
            ("Final Fantasy VII", "Final Fantasy VII Remake"),
            ("Final Fantasy VII", "Final Fantasy VIII"),
            ("Celeste", "Celeste Classic"),
        ] {
            let m = best_match(&normalize_name(query), &one(listed), 0.55);
            assert!(m.is_none(), "{query} took {listed}'s times at {:?}", m.and_then(|m| m.similarity));
        }
        for (query, listed) in [
            ("Hollow Knight", "Hollow Knight"),
            ("The Witcher 3", "The Witcher 3: Wild Hunt"),
            ("Prey", "Prey (2017)"),
            ("Dishonored - Definitive Edition", "Dishonored"),
            ("Hades II", "Hades 2"),
            ("Grand Theft Auto V", "Grand Theft Auto V"),
        ] {
            assert!(best_match(&normalize_name(query), &one(listed), 0.55).is_some(), "{query} should match {listed}");
        }
        // An alias can still match where the name is a look-alike.
        let aliased = json!({"data": [{"game_id": 2, "game_name": "Hollow Knight", "game_alias": "Hollow", "comp_main": 3600}]});
        assert!(best_match(&normalize_name("Hollow"), &aliased, 0.55).is_some());
    }

    /// The code-health review's R-STORES-1, B-STORES-3: HowLongToBeat reads names with
    /// the one score every source uses, so it decides each known pair the
    /// way the others do.
    #[test]
    fn known_pairs() {
        for (query, listed, same) in crate::names::KNOWN_PAIRS {
            let one = json!({"data": [{"game_id": 1, "game_name": listed, "game_alias": "", "comp_main": 3600}]});
            assert_eq!(best_match(&normalize_name(query), &one, 0.55).is_some(), *same, "{query} and {listed}");
        }
    }
}
