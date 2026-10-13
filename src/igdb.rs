//! IGDB (via Twitch client credentials): genres, themes, keywords,
//! perspectives, game modes, summary and ratings for any game by name.
//! Optional: only runs when a client id/secret is configured.

use crate::http::{Backoff, RateLimiter, base_url, send_with_backoff};
use anyhow::{Context, Result, anyhow, bail};
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Default)]
pub struct IgdbGame {
    pub igdb_id: i64,
    pub name: String,
    pub genres: Vec<String>,
    pub themes: Vec<String>,
    pub keywords: Vec<String>,
    pub perspectives: Vec<String>,
    pub modes: Vec<String>,
    pub summary: Option<String>,
    pub rating: Option<f64>,
    pub rating_count: Option<i64>,
    pub similarity: f64,
}

const API: &str = "https://api.igdb.com";
const TWITCH: &str = "https://id.twitch.tv";

pub struct IgdbClient {
    http: Client,
    api: String,
    twitch: String,
    client_id: String,
    client_secret: String,
    limiter: RateLimiter,
    token: Mutex<Option<(String, i64)>>,
}

fn names(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.get("name").and_then(Value::as_str)).map(str::to_string).collect())
        .unwrap_or_default()
}

impl IgdbClient {
    /// `base` stands in for IGDB and Twitch's token host (tests pass a local
    /// mock server); None means the real ones.
    pub fn new(http: Client, client_id: String, client_secret: String, base: Option<&str>) -> Self {
        Self {
            http,
            api: base_url(API, base),
            twitch: base_url(TWITCH, base),
            client_id,
            client_secret,
            limiter: RateLimiter::new("IGDB", Duration::from_millis(300)),
            token: Mutex::new(None),
        }
    }

    async fn token(&self) -> Result<String> {
        let mut t = self.token.lock().await;
        if let Some((tok, exp)) = t.as_ref()
            && *exp > crate::db::now() + 60
        {
            return Ok(tok.clone());
        }
        let v: Value = self
            .http
            .post(format!("{}/oauth2/token", self.twitch))
            // In the body, not the URL, so the secret never shows up in logs or errors.
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("grant_type", "client_credentials"),
            ])
            .send()
            .await
            .map_err(|e| e.without_url())?
            .json()
            .await
            .context("Twitch token response was not JSON")?;
        let tok = v.get("access_token").and_then(Value::as_str).ok_or_else(|| {
            anyhow!(
                "Twitch token request failed: {}",
                v.get("message").and_then(Value::as_str).unwrap_or("check the IGDB client id/secret")
            )
        })?;
        let exp = crate::db::now() + v.get("expires_in").and_then(Value::as_i64).unwrap_or(3600);
        *t = Some((tok.to_string(), exp));
        Ok(tok.to_string())
    }

    pub async fn check(&self) -> Result<()> {
        self.token().await.map(|_| ())
    }

    /// YouTube trailer id (preferring a video named like a trailer) and first
    /// release year for IGDB game ids, up to 500 per call.
    /// Cover art URLs by IGDB slug (up to 500 slugs per call).
    pub async fn covers_by_slug(&self, slugs: &[String]) -> Result<std::collections::HashMap<String, String>> {
        if slugs.is_empty() {
            return Ok(Default::default());
        }
        let token = self.token().await?;
        let list: Vec<String> = slugs.iter().map(|s| format!("\"{}\"", s.replace('"', ""))).collect();
        let body = format!("fields slug,cover.image_id; where slug = ({}); limit 500;", list.join(","));
        let resp = send_with_backoff(&self.limiter, &Backoff { attempts: 3, ..Default::default() }, || {
            self.http
                .post(format!("{}/v4/games", self.api))
                .header("Client-ID", &self.client_id)
                .bearer_auth(&token)
                .header("Accept", "application/json")
                .body(body.clone())
        })
        .await?;
        let v: Value = resp.json().await.context("IGDB returned non-JSON")?;
        Ok(parse_covers(&v))
    }

    pub async fn trailers(&self, ids: &[i64]) -> Result<std::collections::HashMap<i64, Trailer>> {
        if ids.is_empty() {
            return Ok(Default::default());
        }
        let token = self.token().await?;
        let list: Vec<String> = ids.iter().map(|i| i.to_string()).collect();
        let body = format!(
            "fields id,videos.video_id,videos.name,first_release_date,screenshots.image_id; where id = ({}); limit 500;",
            list.join(",")
        );
        let resp = send_with_backoff(&self.limiter, &Backoff { attempts: 3, ..Default::default() }, || {
            self.http
                .post(format!("{}/v4/games", self.api))
                .header("Client-ID", &self.client_id)
                .bearer_auth(&token)
                .header("Accept", "application/json")
                .body(body.clone())
        })
        .await?;
        let status = resp.status();
        let v: Value = resp.json().await.context("IGDB returned non-JSON")?;
        if !status.is_success() {
            bail!("IGDB HTTP {status}: {}", v.to_string().chars().take(200).collect::<String>());
        }
        Ok(parse_trailers(&v))
    }

    /// Best match by name, or `Ok(None)` when nothing is close enough.
    pub async fn lookup(&self, name: &str, min_similarity: f64) -> Result<Option<IgdbGame>> {
        let token = self.token().await?;
        let clean = crate::names::normalize_name(name).replace('"', "");
        if clean.is_empty() {
            return Ok(None);
        }
        let body = format!(
            "search \"{clean}\"; fields name,genres.name,themes.name,keywords.name,player_perspectives.name,game_modes.name,summary,total_rating,total_rating_count,category,version_parent; limit 8;"
        );
        let resp = send_with_backoff(&self.limiter, &Backoff { attempts: 3, ..Default::default() }, || {
            self.http
                .post(format!("{}/v4/games", self.api))
                .header("Client-ID", &self.client_id)
                .bearer_auth(&token)
                .header("Accept", "application/json")
                .body(body.clone())
        })
        .await?;
        let status = resp.status();
        let v: Value = resp.json().await.context("IGDB returned non-JSON")?;
        if !status.is_success() {
            bail!("IGDB HTTP {status}: {}", v.to_string().chars().take(200).collect::<String>());
        }
        Ok(best_match(name, &v, min_similarity))
    }
}

/// A trailer's YouTube id, the first-release year, and screenshot image ids.
pub type Trailer = (Option<String>, Option<i32>, Vec<String>);

fn parse_covers(v: &Value) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for g in v.as_array().into_iter().flatten() {
        if let (Some(slug), Some(img)) = (g.get("slug").and_then(Value::as_str), g.pointer("/cover/image_id").and_then(Value::as_str)) {
            out.insert(slug.to_string(), format!("https://images.igdb.com/igdb/image/upload/t_cover_big_2x/{img}.jpg"));
        }
    }
    out
}

/// Per IGDB id: the best trailer's YouTube id, the first-release year and up to 12 screenshot ids.
fn parse_trailers(v: &Value) -> std::collections::HashMap<i64, Trailer> {
    let mut out = std::collections::HashMap::new();
    for g in v.as_array().into_iter().flatten() {
        let Some(id) = g.get("id").and_then(Value::as_i64) else { continue };
        let videos: Vec<(&str, &str)> = g
            .get("videos")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|x| Some((x.get("video_id")?.as_str()?, x.get("name").and_then(Value::as_str).unwrap_or(""))))
            .collect();
        let rank = |name: &str| {
            let n = name.to_lowercase();
            if n.contains("launch trailer") {
                0
            } else if n.contains("release trailer") {
                1
            } else if n == "trailer" {
                2
            } else if n.contains("trailer") {
                3
            } else {
                9
            }
        };
        let video = videos.iter().min_by_key(|(_, name)| rank(name)).map(|(v, _)| v.to_string());
        let year = g
            .get("first_release_date")
            .and_then(Value::as_i64)
            .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
            .map(|d| chrono::Datelike::year(&d));
        let shots: Vec<String> = g
            .get("screenshots")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|s| s.get("image_id").and_then(Value::as_str).map(str::to_string))
            .take(12)
            .collect();
        out.insert(id, (video, year, shots));
    }
    out
}

/// The search hit that best matches `name`, if it clears `min_similarity`.
fn best_match(name: &str, v: &Value, min_similarity: f64) -> Option<IgdbGame> {
    let mut best: Option<(f64, &Value)> = None;
    for g in v.as_array().into_iter().flatten() {
        let n = g.get("name").and_then(Value::as_str).unwrap_or("");
        // "Hollow" must not take Hollow Knight's metadata, nor "Hades II" Hades'.
        let mut sim = crate::names::score(name, n);
        // Prefer main games over DLC/expansions/bundles (category 0 = main game).
        if g.get("category").and_then(Value::as_i64).is_some_and(|c| c != 0 && c != 8 && c != 9) {
            sim -= 0.1;
        }
        if best.is_none_or(|(b, _)| sim > b) {
            best = Some((sim, g));
        }
    }
    let (sim, g) = best?;
    if sim < min_similarity {
        return None;
    }
    Some(IgdbGame {
        igdb_id: g.get("id").and_then(Value::as_i64).unwrap_or(0),
        name: g.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
        genres: names(g.get("genres")),
        themes: names(g.get("themes")),
        keywords: names(g.get("keywords")).into_iter().filter(|k| !is_catalogue_keyword(k)).take(20).collect(),
        perspectives: names(g.get("player_perspectives")),
        modes: names(g.get("game_modes")),
        summary: g.get("summary").and_then(Value::as_str).map(|s| s.chars().take(600).collect()),
        rating: g.get("total_rating").and_then(Value::as_f64).map(|r| (r * 10.0).round() / 10.0),
        rating_count: g.get("total_rating_count").and_then(Value::as_i64),
        similarity: (sim * 1000.0).round() / 1000.0,
    })
}

/// IGDB keywords about the catalogue rather than the game: stores and
/// subscriptions it was on, platform features, trade shows, awards,
/// crowdfunding and middleware ("previously on - stadia pro", "steam
/// achievements", "pax west 2017", "bink video"). They never become tags.
pub fn is_catalogue_keyword(keyword: &str) -> bool {
    const EXACT: &[&str] = &[
        "steam",
        "digital distribution",
        "achievements",
        "trophies",
        "controller support",
        "controller recommendation",
        "bink video",
        "dolby digital",
        "digital rights management",
        "humble bundle",
        "crowdfunding",
        "indiegogo",
        "twitch",
        "twitch integration",
        "retroachievements",
        "cloud gaming",
        "pre-release public testing",
    ];
    // "steam " with the space, so "steampunk" stays.
    const PREFIXES: &[&str] = &[
        "previously on",
        "available on",
        "crowdfunding -",
        "retail games with",
        "steam ",
        "xbox ",
        "playstation ",
        "nintendo ",
        "new nintendo ",
        "wii u ",
        "gog preservation",
        "pax ",
        "gamescom",
        "tokyo game show",
        "nextfest",
        "the game awards",
        "game developers choice",
        "d.i.c.e.",
    ];
    const CONTAINS: &[&str] = &["achievement", "(the game awards)", "anti-cheat", "backwards compatibility"];
    let k = keyword.trim().to_lowercase();
    // An event and its year: "summer consumer electronics show 1990". Not
    // decades ("1980s") or future years in titles ("blade runner 2049").
    let event_year = k.rsplit_once(' ').is_some_and(|(_, y)| y.len() == 4 && y.parse::<u32>().is_ok_and(|y| (1970..=2035).contains(&y)));
    EXACT.contains(&k.as_str()) || PREFIXES.iter().any(|p| k.starts_with(p)) || CONTAINS.iter().any(|c| k.contains(c)) || event_year
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::fixture_json;

    #[test]
    fn covers() {
        let c = parse_covers(&fixture_json("igdb/covers.json"));
        assert_eq!(c.len(), 1, "entries without a cover are skipped");
        assert_eq!(c["portal-2"], "https://images.igdb.com/igdb/image/upload/t_cover_big_2x/co1rs4.jpg");
    }

    #[test]
    fn trailers() {
        let t = parse_trailers(&fixture_json("igdb/trailers.json"));
        let (video, year, shots) = &t[&72];
        assert_eq!(video.as_deref(), Some("launch1"), "a launch trailer beats a teaser");
        assert_eq!(*year, Some(2011));
        assert_eq!(shots.len(), 12, "capped at 12");
        assert_eq!(t[&74], (None, None, vec![]));
    }

    #[test]
    fn catalogue_keywords_are_not_tags() {
        for k in [
            "previously on - stadia pro",
            "available on - blacknut",
            "steam",
            "Steam Achievements",
            "steam trading cards",
            "digital distribution",
            "crowdfunding - kickstarter",
            "pax west 2017",
            "summer consumer electronics show 1990",
            "best performance: winner - 2017 (the game awards)",
            "xbox one x enhanced",
            "playstation trophies",
            "bink video",
            "valve anti-cheat",
            "online multiplayer achievements",
            "pre-release public testing",
        ] {
            assert!(is_catalogue_keyword(k), "{k}");
        }
        for k in
            ["steampunk", "1980s", "cloud strife", "blade runner 2049", "female protagonist", "exploration", "engineering", "licensed game"]
        {
            assert!(!is_catalogue_keyword(k), "{k}");
        }
    }

    #[test]
    fn lookup_match() {
        let v = fixture_json("igdb/search.json");
        let g = best_match("Portal 2", &v, 0.8).unwrap();
        assert_eq!((g.igdb_id, g.similarity), (72, 1.0));
        assert_eq!(g.genres, ["Puzzle", "Platform"]);
        assert_eq!(g.keywords.len(), 20, "keywords capped at 20");
        assert_eq!(g.summary.unwrap().chars().count(), 600, "summary capped at 600 chars");
        assert_eq!((g.rating, g.rating_count), (Some(94.3), Some(2100)));
        assert!(best_match("A Completely Different Game", &v, 0.8).is_none());
        assert!(best_match("Portal 2", &serde_json::json!([]), 0.8).is_none());
        // The code-health review's B-STORES-3, the IGDB part: a name that only starts
        // another game's, or a sequel's original, isn't that game.
        for (query, listed) in [("Hollow", "Hollow Knight"), ("Hades II", "Hades"), ("DOOM", "DOOM Eternal")] {
            let one = serde_json::json!([{"id": 1, "name": listed, "category": 0}]);
            assert!(best_match(query, &one, 0.8).is_none(), "{query} took {listed}'s metadata");
        }
        let witcher = serde_json::json!([{"id": 2, "name": "The Witcher 3: Wild Hunt", "category": 0}]);
        assert!(best_match("The Witcher 3: Wild Hunt - Complete Edition", &witcher, 0.8).is_some());
    }

    /// The code-health review's R-STORES-1: IGDB decides each known pair the way the
    /// other sources do.
    #[test]
    fn known_pairs() {
        for (query, listed, same) in crate::names::KNOWN_PAIRS {
            let one = serde_json::json!([{"id": 1, "name": listed, "category": 0}]);
            assert_eq!(best_match(query, &one, 0.8).is_some(), *same, "{query} and {listed}");
        }
    }

    // -- auth, against a local stand-in for Twitch and IGDB --------------------

    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client(server: &MockServer) -> IgdbClient {
        IgdbClient::new(crate::http::client().unwrap(), "my-id".into(), "my-secret".into(), Some(&server.uri()))
    }

    #[tokio::test]
    async fn token_is_fetched_once_and_sent_with_the_client_id() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .and(body_string_contains("client_id=my-id"))
            .and(body_string_contains("client_secret=my-secret"))
            .and(body_string_contains("grant_type=client_credentials"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"access_token": "tok", "expires_in": 5000000})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v4/games"))
            .and(header("client-id", "my-id"))
            .and(header("authorization", "Bearer tok"))
            .and(body_string_contains("search \"Portal 2\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(fixture_json("igdb/search.json")))
            .expect(2)
            .mount(&server)
            .await;
        let igdb = client(&server);
        igdb.check().await.unwrap();
        assert_eq!(igdb.lookup("Portal 2", 0.8).await.unwrap().map(|g| g.igdb_id), Some(72));
        assert!(igdb.lookup("Portal 2", 0.8).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn bad_credentials_fail_the_check_with_twitchs_reason() {
        let server = MockServer::start().await;
        Mock::given(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({"status": 403, "message": "invalid client secret"})))
            .mount(&server)
            .await;
        Mock::given(path("/v4/games")).respond_with(ResponseTemplate::new(500)).expect(0).mount(&server).await;
        let igdb = client(&server);
        let err = igdb.check().await.unwrap_err();
        assert_eq!(err.to_string(), "Twitch token request failed: invalid client secret");
        assert!(igdb.lookup("Portal 2", 0.8).await.is_err(), "no IGDB request without a token");
    }
}
