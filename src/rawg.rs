//! RAWG (rawg.io): user tags, genres, rating and Metacritic score for almost
//! any PC game, including store exclusives. Needs a free per-user API key
//! (https://rawg.io/apidocs; 20,000 requests/month, attribution required).
//! Looked up by the exact slug from Wikidata when known, otherwise by name.

use crate::http::{Backoff, HttpStatusError, RateLimiter, base_url, send_with_backoff};
use anyhow::{Context, Result, anyhow};
use reqwest::{Client, StatusCode};
use serde_json::Value;
use std::time::Duration;

const API: &str = "https://api.rawg.io";

#[derive(Debug, Clone, Default)]
pub struct RawgGame {
    pub rawg_id: i64,
    pub slug: String,
    pub name: String,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub rating: Option<f64>,
    pub ratings_count: Option<i64>,
    pub metacritic: Option<i64>,
    pub developers: Vec<String>,
    pub similarity: f64,
}

/// RAWG turned the key down (HTTP 401), so no request with it will work.
#[derive(Debug, thiserror::Error)]
#[error("RAWG rejected the API key (401); check it on the Settings page")]
pub struct KeyRejected;

pub struct RawgClient {
    http: Client,
    api: String,
    key: String,
    limiter: RateLimiter,
}

/// RAWG mixes store features and non-English tags in with player tags.
fn keep_tag(t: &Value) -> Option<String> {
    let lang = t.get("language").and_then(Value::as_str).unwrap_or("eng");
    if lang != "eng" {
        return None;
    }
    let name = t.get("name").and_then(Value::as_str)?.trim();
    let l = name.to_lowercase();
    const NOISE: [&str; 14] = [
        "steam",
        "achievements",
        "controller",
        "cloud",
        "trading cards",
        "stats",
        "valve anti-cheat",
        "leaderboards",
        "remote play",
        "family sharing",
        "overlay",
        "workshop",
        "3d vision",
        "early access",
    ];
    if name.is_empty() || NOISE.iter().any(|n| l.contains(n)) || l == "exclusive" || l == "true exclusive" {
        return None;
    }
    Some(name.to_string())
}

fn names(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.get("name").and_then(Value::as_str)).map(str::to_string).collect())
        .unwrap_or_default()
}

fn parse_game(v: &Value, similarity: f64) -> Option<RawgGame> {
    Some(RawgGame {
        rawg_id: v.get("id").and_then(Value::as_i64)?,
        slug: v.get("slug").and_then(Value::as_str)?.to_string(),
        name: v.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
        genres: names(v.get("genres")),
        tags: v.get("tags").and_then(Value::as_array).map(|a| a.iter().filter_map(keep_tag).take(25).collect()).unwrap_or_default(),
        rating: v.get("rating").and_then(Value::as_f64).filter(|r| *r > 0.0),
        ratings_count: v.get("ratings_count").and_then(Value::as_i64),
        metacritic: v.get("metacritic").and_then(Value::as_i64),
        developers: names(v.get("developers")),
        similarity,
    })
}

impl RawgClient {
    /// `base` stands in for api.rawg.io (tests pass a local mock server).
    pub fn new(http: Client, key: String, base: Option<&str>) -> Self {
        Self { http, api: base_url(API, base), key, limiter: RateLimiter::new("RAWG", Duration::from_millis(400)) }
    }

    /// Errors carry the request URL; never let the key reach the logs.
    fn scrub(&self, e: anyhow::Error) -> anyhow::Error {
        anyhow!("{}", format!("{e:#}").replace(&self.key, "***"))
    }

    async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Option<Value>> {
        let url = format!("{}/api{path}", self.api);
        let backoff = Backoff { attempts: 3, base: Duration::from_secs(3), max: Duration::from_secs(30), ..Default::default() };
        let resp =
            send_with_backoff(&self.limiter, &backoff, || self.http.get(&url).query(&[("key", self.key.as_str())]).query(query)).await;
        match resp {
            Ok(r) => Ok(Some(crate::http::json(r).await.context("RAWG returned non-JSON")?)),
            Err(e) if crate::http::is_not_found(&e) => Ok(None),
            Err(e) if e.downcast_ref::<HttpStatusError>().is_some_and(|h| h.status == StatusCode::UNAUTHORIZED) => Err(KeyRejected.into()),
            Err(e) => Err(self.scrub(e)),
        }
    }

    /// Cheap request to validate the key.
    pub async fn check(&self) -> Result<()> {
        self.get("/genres", &[("page_size", "1")]).await.map(|_| ())
    }

    pub async fn by_slug(&self, slug: &str) -> Result<Option<RawgGame>> {
        // The slug comes from Wikidata: encoded, so a `?` or `/` in it can't
        // ask for another page.
        Ok(self.get(&format!("/games/{}", crate::epic::path_part(slug)), &[]).await?.and_then(|v| parse_game(&v, 1.0)))
    }

    /// Best search hit by name, or `Ok(None)` below `min_similarity`.
    pub async fn search(&self, name: &str, min_similarity: f64) -> Result<Option<RawgGame>> {
        let clean = crate::names::normalize_name(name);
        if clean.is_empty() {
            return Ok(None);
        }
        let Some(v) = self.get("/games", &[("search", clean.as_str()), ("search_precise", "true"), ("page_size", "5")]).await? else {
            return Ok(None);
        };
        Ok(best_search_hit(&clean, &v, min_similarity))
    }
}

/// The search result closest to the normalized name `clean`, if it clears `min_similarity`.
fn best_search_hit(clean: &str, v: &Value, min_similarity: f64) -> Option<RawgGame> {
    let best = v
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let n = r.get("name").and_then(Value::as_str)?;
            let sim = crate::names::score(clean, n);
            Some((sim, r))
        })
        .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    best.filter(|(s, _)| *s >= min_similarity).and_then(|(s, r)| parse_game(r, s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tag_filter() {
        let v = json!({"id": 1, "slug": "x", "name": "X", "tags": [
            {"name": "Singleplayer", "language": "eng"}, {"name": "Steam Achievements", "language": "eng"},
            {"name": "Full controller support", "language": "eng"}, {"name": "Atmospheric", "language": "eng"},
            {"name": "атмосфера", "language": "rus"}]});
        assert_eq!(parse_game(&v, 1.0).unwrap().tags, vec!["Singleplayer", "Atmospheric"]);
    }

    /// The code-health review's B-STORES-11: a slug from Wikidata is one path segment,
    /// so a `?` or `/` in it can't ask for another page.
    #[tokio::test]
    async fn a_slug_stays_one_path_segment() {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::any};
        let s = MockServer::start().await;
        Mock::given(any()).respond_with(ResponseTemplate::new(404)).mount(&s).await;
        let rawg = RawgClient::new(crate::http::client().unwrap(), "k".into(), Some(&s.uri()));
        assert!(rawg.by_slug("portal-2/../../genres?x=1").await.unwrap().is_none());
        let asked: Vec<String> = s.received_requests().await.unwrap().iter().map(|r| r.url.path().to_string()).collect();
        assert_eq!(asked, ["/api/games/portal-2%2F..%2F..%2Fgenres%3Fx%3D1"]);
    }
}

#[cfg(test)]
mod key_tests {
    use super::*;

    /// SEC-M4: an answer that isn't JSON says so without the key, which
    /// reqwest would otherwise name with the URL.
    #[tokio::test]
    async fn an_answer_that_isnt_json_leaves_the_key_out_of_the_error() {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::any};
        let s = MockServer::start().await;
        Mock::given(any()).respond_with(ResponseTemplate::new(200).set_body_string("<html>busy</html>")).mount(&s).await;
        let key = "0123456789abcdef0123456789abcdef";
        let rawg = RawgClient::new(crate::http::client().unwrap(), key.into(), Some(&s.uri()));
        let e = rawg.by_slug("portal-2").await.unwrap_err();
        let text = format!("{e:#} {e:?}");
        assert!(text.contains("non-JSON"), "{text}");
        assert!(!text.contains(key), "the key is in the error: {text}");
    }
}

#[cfg(test)]
mod search_tests {
    use super::*;
    use crate::testutil::fixture_json;

    #[test]
    fn best_hit() {
        let v = fixture_json("rawg/search.json");
        let g = best_search_hit(&crate::names::normalize_name("Portal 2"), &v, 0.8).unwrap();
        assert_eq!((g.rawg_id, g.slug.as_str(), g.metacritic), (4200, "portal-2", Some(95)));
        assert_eq!(g.tags, ["Singleplayer"]);
        assert!(best_search_hit(&crate::names::normalize_name("Stardew Valley"), &v, 0.8).is_none());
    }

    /// The code-health review's B-STORES-3, the RAWG part: a sequel, or the game it
    /// follows, no longer takes the other's tags and scores ("Final Fantasy
    /// VII" took VIII's at 0.94), and "Hades II" finds "Hades 2".
    #[test]
    fn known_pairs() {
        for (query, listed, same) in crate::names::KNOWN_PAIRS {
            let one = serde_json::json!({"results": [{"id": 1, "slug": "x", "name": listed}]});
            assert_eq!(best_search_hit(&crate::names::normalize_name(query), &one, 0.85).is_some(), *same, "{query} and {listed}");
        }
    }
}
