//! Steam Web API (owned games, player summary) and Steam Store (appdetails,
//! review summary) clients.

use crate::db::{DetailsRow, Profile, ReviewRow};

/// One entry from GetOwnedGames.
#[derive(Debug, Clone)]
pub struct OwnedGame {
    pub appid: u32,
    pub name: String,
    pub playtime_forever: i64,
    pub playtime_2weeks: i64,
    pub rtime_last_played: i64,
}
use crate::http::{Backoff, RateLimiter, base_url, send_with_backoff};
use crate::steam_local::Vdf;
use anyhow::{Context, Result, anyhow, bail};
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

const API: &str = "https://api.steampowered.com";
const STORE: &str = "https://store.steampowered.com";
const COMMUNITY: &str = "https://steamcommunity.com";

/// The Web API's backoff. It answers a key it doesn't accept with 403
/// ("Retrying will not help"), so a 403 there is final, unlike on the store
/// and community hosts, which use it to ask for a slower pace.
fn api_backoff() -> Backoff {
    Backoff { retry_403: false, ..Default::default() }
}

/// Whether the Web API turned the key down (401 or 403), as opposed to
/// being down or slow.
pub fn key_refused(e: &anyhow::Error) -> bool {
    e.downcast_ref::<crate::http::HttpStatusError>()
        .is_some_and(|h| matches!(h.status, reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN))
}

pub struct SteamClient {
    http: Client,
    api_key: Option<String>,
    steam_id: Option<String>,
    api: String,
    store: String,
    community: String,
    api_limiter: RateLimiter,
    store_limiter: RateLimiter,
    /// `/appreviews` is throttled separately from `/api/appdetails`, so it gets
    /// its own pace and can run alongside the store lookups.
    reviews_limiter: RateLimiter,
}

impl SteamClient {
    /// `base` stands in for the Web API, store and community hosts (tests
    /// pass a local mock server); None means the real ones.
    pub fn new(http: Client, api_key: Option<String>, steam_id: Option<String>, base: Option<&str>) -> Self {
        Self {
            http,
            api_key,
            steam_id,
            api: base_url(API, base),
            store: base_url(STORE, base),
            community: base_url(COMMUNITY, base),
            api_limiter: RateLimiter::new("Steam", Duration::from_millis(500)),
            // The store API is unofficial and throttles at roughly 200 req / 5 min.
            store_limiter: RateLimiter::new("Steam", Duration::from_millis(1600)),
            // `/appreviews` answers 429 for minutes at a time when a long run
            // keeps up one request a second; the store's pace holds.
            reviews_limiter: RateLimiter::new("Steam", Duration::from_millis(1600)),
        }
    }

    fn creds(&self) -> Result<(&str, &str)> {
        match (&self.api_key, &self.steam_id) {
            (Some(k), Some(s)) => Ok((k, s)),
            _ => bail!("STEAM_API_KEY / STEAM_ID not configured"),
        }
    }

    /// IPlayerService/GetOwnedGames with app info and played free games.
    pub async fn owned_games(&self) -> Result<Vec<OwnedGame>> {
        let (key, sid) = self.creds()?;
        let resp = send_with_backoff(&self.api_limiter, &api_backoff(), || {
            self.http.get(format!("{}/IPlayerService/GetOwnedGames/v1/", self.api)).query(&[
                ("key", key),
                ("steamid", sid),
                ("include_appinfo", "1"),
                ("include_played_free_games", "1"),
                ("include_free_sub", "1"),
                ("format", "json"),
            ])
        })
        .await
        .context("GetOwnedGames request failed (is STEAM_API_KEY valid?)")?;
        let body: Value = crate::http::json(resp).await.context("GetOwnedGames returned non-JSON")?;
        parse_owned_games(&body)
    }

    /// IPlayerService/GetRecentlyPlayedGames: the games played in the last
    /// two weeks, with their total and two-week playtime (the play diary).
    pub async fn recently_played(&self) -> Result<Vec<OwnedGame>> {
        let (key, sid) = self.creds()?;
        let resp = send_with_backoff(&self.api_limiter, &api_backoff(), || {
            self.http.get(format!("{}/IPlayerService/GetRecentlyPlayedGames/v1/", self.api)).query(&[
                ("key", key),
                ("steamid", sid),
                ("format", "json"),
            ])
        })
        .await
        .context("GetRecentlyPlayedGames request failed")?;
        let body: Value = crate::http::json(resp).await.context("GetRecentlyPlayedGames returned non-JSON")?;
        parse_recently_played(&body)
    }

    /// ISteamUser/GetPlayerSummaries for the configured account.
    pub async fn player_summary(&self) -> Result<Option<Profile>> {
        let (key, sid) = self.creds()?;
        let resp = send_with_backoff(&self.api_limiter, &api_backoff(), || {
            self.http.get(format!("{}/ISteamUser/GetPlayerSummaries/v2/", self.api)).query(&[
                ("key", key),
                ("steamids", sid),
                ("format", "json"),
            ])
        })
        .await?;
        let body: Value = crate::http::json(resp).await.context("GetPlayerSummaries returned non-JSON")?;
        Ok(parse_player_summary(&body, sid))
    }

    /// Store appdetails. `Ok(None)` means the store reported `success: false`
    /// (delisted / region-locked / not a store app).
    pub async fn app_details(&self, appid: u32) -> Result<Option<DetailsRow>> {
        let resp = send_with_backoff(&self.store_limiter, &Backoff::default(), || {
            self.http.get(format!("{}/api/appdetails", self.store)).query(&[
                ("appids", appid.to_string()),
                ("cc", "us".into()),
                ("l", "english".into()),
            ])
        })
        .await?;
        let body: Value = resp.json().await.context("appdetails returned non-JSON")?;
        parse_app_details(&body, appid)
    }

    /// IWishlistService/GetWishlist: the games on `steam_id`'s Steam
    /// wishlist, read-only. A public wishlist needs no key; the
    /// key goes along when there is one. `Ok(None)`: Steam showed no
    /// wishlist (a private one answers with an empty `response`).
    pub async fn wishlist(&self, steam_id: &str) -> Result<Option<Vec<WishlistItem>>> {
        let resp = send_with_backoff(&self.api_limiter, &api_backoff(), || {
            let mut q = vec![("steamid", steam_id)];
            if let Some(k) = &self.api_key {
                q.push(("key", k.as_str()));
            }
            self.http.get(format!("{}/IWishlistService/GetWishlist/v1/", self.api)).query(&q)
        })
        .await
        .context("GetWishlist request failed")?;
        let body: Value = crate::http::json(resp).await.context("GetWishlist returned non-JSON")?;
        Ok(parse_wishlist(&body))
    }

    /// The review summary alone, in one request: a game you don't own needs
    /// its score and count, not its review texts. `Ok(None)`: the store has
    /// no reviews page for it.
    pub async fn review_summary(&self, appid: u32) -> Result<Option<ReviewRow>> {
        let resp = send_with_backoff(&self.reviews_limiter, &Backoff::default(), || {
            self.http.get(format!("{}/appreviews/{appid}", self.store)).query(&[
                ("json", "1"),
                ("language", "all"),
                ("purchase_type", "all"),
                ("num_per_page", "0"),
            ])
        })
        .await?;
        let body: Value = resp.json().await.context("appreviews returned non-JSON")?;
        Ok(parse_reviews(&body, 0).0)
    }

    /// Ask Steam whether an OpenID assertion is genuine (`check_authentication`).
    pub async fn openid_is_valid(&self, form: &[(String, String)]) -> Result<bool> {
        let resp = self.http.post(format!("{}/openid/login", self.community)).form(form).send().await?;
        Ok(resp.text().await.map(|t| t.lines().any(|l| l.trim() == "is_valid:true")).unwrap_or(false))
    }
}

/// One game on a Steam wishlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WishlistItem {
    pub appid: u32,
    /// When it was put on the wishlist (unix time), when Steam says.
    pub added_at: Option<i64>,
}

fn parse_wishlist(body: &Value) -> Option<Vec<WishlistItem>> {
    let items = body.pointer("/response/items")?.as_array()?;
    Some(
        items
            .iter()
            .filter_map(|i| {
                Some(WishlistItem {
                    appid: i.get("appid").and_then(Value::as_u64)? as u32,
                    added_at: i.get("date_added").and_then(Value::as_i64).filter(|t| *t > 0),
                })
            })
            .collect(),
    )
}

fn parse_owned_games(body: &Value) -> Result<Vec<OwnedGame>> {
    let response = body.get("response").ok_or_else(|| anyhow!("GetOwnedGames: missing 'response'"))?;
    let games = match response.get("games").and_then(Value::as_array) {
        Some(g) => g,
        None => bail!(
            "GetOwnedGames returned no games. Check STEAM_ID, and set Steam privacy 'Game details' to Public \
             (Profile > Edit Profile > Privacy Settings)."
        ),
    };
    let mut out = Vec::with_capacity(games.len());
    for g in games {
        let appid = g.get("appid").and_then(Value::as_u64).ok_or_else(|| anyhow!("game without appid"))? as u32;
        out.push(OwnedGame {
            appid,
            name: g.get("name").and_then(Value::as_str).unwrap_or("").trim().to_string(),
            playtime_forever: g.get("playtime_forever").and_then(Value::as_i64).unwrap_or(0),
            playtime_2weeks: g.get("playtime_2weeks").and_then(Value::as_i64).unwrap_or(0),
            rtime_last_played: g.get("rtime_last_played").and_then(Value::as_i64).unwrap_or(0),
        });
    }
    // Names can be empty for delisted apps; keep a placeholder so rows are usable.
    for g in out.iter_mut() {
        if g.name.is_empty() {
            g.name = format!("App {}", g.appid);
        }
    }
    Ok(out)
}

/// A private profile answers with an empty `response`; one with nothing
/// played in two weeks says `total_count: 0` and has no `games`.
fn parse_recently_played(body: &Value) -> Result<Vec<OwnedGame>> {
    let response = body.get("response").ok_or_else(|| anyhow!("GetRecentlyPlayedGames: missing 'response'"))?;
    let Some(games) = response.get("games").and_then(Value::as_array) else {
        if response.get("total_count").is_some() {
            return Ok(vec![]);
        }
        bail!("Steam didn't say what you played: set Steam privacy 'Game details' to Public (Profile > Edit Profile > Privacy Settings)");
    };
    Ok(games
        .iter()
        .filter_map(|g| {
            Some(OwnedGame {
                appid: g.get("appid").and_then(Value::as_u64)? as u32,
                name: g.get("name").and_then(Value::as_str).unwrap_or("").trim().to_string(),
                playtime_forever: g.get("playtime_forever").and_then(Value::as_i64).unwrap_or(0),
                playtime_2weeks: g.get("playtime_2weeks").and_then(Value::as_i64).unwrap_or(0),
                rtime_last_played: 0,
            })
        })
        .collect())
}

fn parse_player_summary(body: &Value, steam_id: &str) -> Option<Profile> {
    let p = body.pointer("/response/players/0")?;
    Some(Profile {
        steam_id: steam_id.to_string(),
        persona_name: p.get("personaname").and_then(Value::as_str).map(str::to_string),
        avatar: p.get("avatarfull").and_then(Value::as_str).map(str::to_string),
        profile_url: p.get("profileurl").and_then(Value::as_str).map(str::to_string),
        last_sync: None,
        game_count: 0,
        copy_count: 0,
        sources: vec![],
    })
}

/// Parse a store appdetails response. `Ok(None)` means `success: false`
/// (delisted / region-locked / not a store app).
fn parse_app_details(body: &Value, appid: u32) -> Result<Option<DetailsRow>> {
    // The store occasionally keys the response by a related id (e.g. a
    // DLC id) instead of the requested appid; there is only ever one entry.
    let entry = body
        .get(appid.to_string())
        .or_else(|| body.as_object().and_then(|o| o.values().next()))
        .ok_or_else(|| anyhow!("appdetails: empty response for {appid}"))?;
    if entry.get("success").and_then(Value::as_bool) != Some(true) {
        return Ok(None);
    }
    let d = entry.get("data").ok_or_else(|| anyhow!("appdetails: missing data for {appid}"))?;
    let names = |key: &str, field: &str| -> Vec<String> {
        let mut out: Vec<String> = vec![];
        for x in d.get(key).and_then(Value::as_array).into_iter().flatten() {
            if let Some(n) = x.get(field).and_then(Value::as_str)
                && !out.iter().any(|e| e == n)
            {
                out.push(n.to_string());
            }
        }
        out
    };
    let strs = |key: &str| -> Vec<String> {
        d.get(key).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
    };
    let platforms = d
        .get("platforms")
        .and_then(Value::as_object)
        .map(|o| o.iter().filter(|(_, v)| v.as_bool() == Some(true)).map(|(k, _)| k.clone()).collect())
        .unwrap_or_default();
    Ok(Some(DetailsRow {
        name: d.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).map(str::to_string),
        type_: d.get("type").and_then(Value::as_str).map(str::to_string),
        short_description: d.get("short_description").and_then(Value::as_str).map(|s| html_unescape(s.trim())),
        genres: names("genres", "description"),
        categories: names("categories", "description"),
        header_image: d.get("header_image").and_then(Value::as_str).map(str::to_string),
        metacritic: d.pointer("/metacritic/score").and_then(Value::as_i64),
        release_date: d.pointer("/release_date/date").and_then(Value::as_str).map(str::to_string).filter(|s| !s.is_empty()),
        developers: strs("developers"),
        publishers: strs("publishers"),
        is_free: d.get("is_free").and_then(Value::as_bool),
        controller_support: d.get("controller_support").and_then(Value::as_str).map(str::to_string),
        platforms,
    }))
}

/// Portrait (library capsule) and hero art URLs from the store's asset list.
/// Newer apps keep art under hashed paths, so the plain
/// `/apps/{appid}/library_600x900.jpg` 404s for them. Up to 100 appids per call.
/// Art for one app: portrait, hero, and screenshot paths (relative to
/// `store_item_assets/`, in the store's order).
pub type StoreArt = (Option<String>, Option<String>, Vec<String>);

pub async fn store_art(client: &SteamClient, appids: &[u32]) -> Result<std::collections::HashMap<u32, StoreArt>> {
    if appids.is_empty() {
        return Ok(Default::default());
    }
    Ok(parse_store_art(&store_items(client, appids).await?))
}

/// Each app's name on the store (what a Steam wishlist leaves out), from
/// the same request as its art. Up to 100 appids per call.
pub async fn store_names(client: &SteamClient, appids: &[u32]) -> Result<std::collections::HashMap<u32, String>> {
    if appids.is_empty() {
        return Ok(Default::default());
    }
    Ok(parse_store_names(&store_items(client, appids).await?))
}

fn parse_store_names(v: &Value) -> std::collections::HashMap<u32, String> {
    v.pointer("/response/store_items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let appid = item.get("appid").and_then(Value::as_u64)? as u32;
            let name = item.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty())?;
            Some((appid, name.to_string()))
        })
        .collect()
}

/// `IStoreBrowseService/GetItems` with assets and screenshots. It's a Web
/// API call, so it takes the Web API's pace and backs off like the others.
async fn store_items(client: &SteamClient, appids: &[u32]) -> Result<Value> {
    let ids: Vec<Value> = appids.iter().map(|a| serde_json::json!({ "appid": a })).collect();
    let input = serde_json::json!({
        "ids": ids,
        "context": { "language": "english", "country_code": "US" },
        "data_request": { "include_assets": true, "include_screenshots": true }
    })
    .to_string();
    let resp = send_with_backoff(&client.api_limiter, &Backoff::default(), || {
        client.http.get(format!("{}/IStoreBrowseService/GetItems/v1/", client.api)).query(&[("input_json", input.as_str())])
    })
    .await
    .context("Steam GetItems request failed")?;
    resp.json().await.context("Steam GetItems returned non-JSON")
}

fn parse_store_art(v: &Value) -> std::collections::HashMap<u32, StoreArt> {
    let mut out = std::collections::HashMap::new();
    for item in v.pointer("/response/store_items").and_then(Value::as_array).into_iter().flatten() {
        let Some(appid) = item.get("appid").and_then(Value::as_u64).map(|a| a as u32) else { continue };
        let assets = item.get("assets");
        let format = assets.and_then(|a| a.get("asset_url_format")).and_then(Value::as_str);
        let url = |key: &str| -> Option<String> {
            let file = assets?.get(key)?.as_str()?;
            let path = format?.replace("${FILENAME}", file);
            Some(format!("https://shared.akamai.steamstatic.com/store_item_assets/{path}"))
        };
        let mut shots: Vec<(i64, String)> = item
            .pointer("/screenshots/all_ages_screenshots")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|s| Some((s.get("ordinal").and_then(Value::as_i64).unwrap_or(0), s.get("filename")?.as_str()?.to_string())))
            .collect();
        shots.sort_by_key(|(o, _)| *o);
        let shots = shots.into_iter().map(|(_, f)| f).take(12).collect();
        out.insert(appid, (url("library_capsule_2x").or_else(|| url("library_capsule")), url("library_hero"), shots));
    }
    out
}

/// Review summary and the most helpful English review texts in one request:
/// the first page carries the all-language summary counts, and its most
/// helpful reviews (any language) are filtered to English here.
/// Steam reviews are at most 8,000 characters; they are stored whole.
const REVIEW_MAX_CHARS: usize = 8000;
/// Of the reviews kept per title, how many come from the all-time ranking;
/// the rest are the last month's, so a game's current state shows too.
const ALL_TIME_REVIEWS: usize = 6;
/// `day_range` for the all-time ranking. Steam documents a maximum of 365,
/// and the all-time ranking takes the largest value; 365 is the fallback.
const ALL_TIME: &str = "9223372036854775807";
const PAST_YEAR: &str = "365";

/// The review scores plus up to `n` English reviews: the all-time most helpful
/// first, then the most helpful of the last month. Two requests.
pub async fn app_reviews_with_texts(
    client: &SteamClient,
    appid: u32,
    n: usize,
) -> Result<(Option<ReviewRow>, Option<Vec<crate::db::ReviewText>>)> {
    let get = |extra: Vec<(&'static str, &'static str)>| async move {
        let resp = send_with_backoff(&client.reviews_limiter, &Backoff::default(), || {
            let mut q = vec![("json", "1"), ("filter", "all"), ("review_type", "all"), ("purchase_type", "all"), ("num_per_page", "60")];
            q.extend(extra.iter().copied());
            client.http.get(format!("{}/appreviews/{appid}", client.store)).query(&q)
        })
        .await?;
        resp.json::<Value>().await.context("appreviews returned non-JSON")
    };
    // Every language, for the scores; without a day range, this ranks the last month.
    let (summary, recent) = parse_reviews(&get(vec![("language", "all")]).await?, n);
    if summary.is_none() {
        return Ok((None, None));
    }
    let mut top = parse_reviews(&get(vec![("language", "english"), ("day_range", ALL_TIME)]).await?, n).1.unwrap_or_default();
    if top.is_empty() {
        top = parse_reviews(&get(vec![("language", "english"), ("day_range", PAST_YEAR)]).await?, n).1.unwrap_or_default();
    }
    Ok((summary, Some(merge_reviews(top, recent.unwrap_or_default(), n))))
}

/// Up to `ALL_TIME_REVIEWS` of `top`, then recent ones not already in, up to
/// `n`; either list fills in when the other runs short.
fn merge_reviews(top: Vec<crate::db::ReviewText>, recent: Vec<crate::db::ReviewText>, n: usize) -> Vec<crate::db::ReviewText> {
    let recent: Vec<_> = recent.into_iter().filter(|r| !top.iter().any(|t| t.text == r.text)).collect();
    let from_top = top.len().min(ALL_TIME_REVIEWS.max(n.saturating_sub(recent.len())));
    let mut out: Vec<_> = top.into_iter().take(from_top).collect();
    out.extend(recent.into_iter().take(n - out.len().min(n)));
    out.truncate(n);
    out
}

type ReviewsParsed = (Option<ReviewRow>, Option<Vec<crate::db::ReviewText>>);

/// Summary counts plus up to `n` English review texts of 20+ characters.
fn parse_reviews(body: &Value, n: usize) -> ReviewsParsed {
    if body.get("success").and_then(Value::as_i64) != Some(1) {
        return (None, None);
    }
    let summary = body.get("query_summary").map(|q| ReviewRow {
        review_score: q.get("review_score").and_then(Value::as_i64),
        review_score_desc: q.get("review_score_desc").and_then(Value::as_str).map(str::to_string),
        total_positive: q.get("total_positive").and_then(Value::as_i64),
        total_negative: q.get("total_negative").and_then(Value::as_i64),
        total_reviews: q.get("total_reviews").and_then(Value::as_i64),
    });
    let mut texts = vec![];
    for r in body.get("reviews").and_then(Value::as_array).into_iter().flatten() {
        if r.get("language").and_then(Value::as_str).is_some_and(|l| l != "english") {
            continue;
        }
        let text = r.get("review").and_then(Value::as_str).unwrap_or("").trim();
        if text.len() < 20 {
            continue;
        }
        texts.push(crate::db::ReviewText {
            source: "steam".into(),
            positive: r.get("voted_up").and_then(Value::as_bool),
            rating: None,
            votes: r.get("votes_up").and_then(Value::as_i64).unwrap_or(0),
            hours: r.pointer("/author/playtime_forever").and_then(Value::as_i64).map(|m| m / 60),
            title: None,
            text: text.chars().take(REVIEW_MAX_CHARS).collect(),
            posted: r.get("timestamp_created").and_then(Value::as_i64),
        });
        if texts.len() >= n {
            break;
        }
    }
    (summary, Some(texts))
}

/// Search the store by name and return the best `game`-looking match as
/// `(appid, matched_name, similarity)`. Strict threshold: a wrong match is
/// worse than none.
pub async fn store_search(client: &SteamClient, name: &str) -> Result<Option<(u32, String, f64)>> {
    let clean = crate::names::normalize_name(name);
    if clean.is_empty() {
        return Ok(None);
    }
    let resp = send_with_backoff(&client.store_limiter, &Backoff::default(), || {
        client.http.get(format!("{}/api/storesearch/", client.store)).query(&[("term", clean.as_str()), ("l", "english"), ("cc", "us")])
    })
    .await?;
    let body: Value = resp.json().await.context("storesearch returned non-JSON")?;
    let items = body.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
    Ok(best_store_match(name, &items))
}

/// The search hit that is the same game as `name`, if any. An exact name
/// beats another edition of it: "X Remastered" takes "X Remastered" over
/// "X", and "X" only when the store lists no other.
fn best_store_match(name: &str, items: &[Value]) -> Option<(u32, String, f64)> {
    let mut best: Option<(u32, String, f64)> = None;
    for it in items {
        let (Some(id), Some(n)) = (it.get("id").and_then(Value::as_u64), it.get("name").and_then(Value::as_str)) else { continue };
        if crate::heroic::is_demo_title(n) && !crate::heroic::is_demo_title(name) {
            continue;
        }
        let sim = crate::names::score(name, n);
        if best.as_ref().is_none_or(|b| sim > b.2) {
            best = Some((id as u32, n.to_string(), sim));
        }
    }
    best.filter(|b| b.2 >= 0.86)
}

/// Persona name of the most recently used account in the local Steam
/// client, looked for as `detect_steam_id` does.
pub fn detect_persona_name(steam_dir: Option<&std::path::Path>) -> Option<String> {
    most_recent_login(steam_dir)?.persona
}

/// SteamID64 of the most recently used account in the local Steam client,
/// looked for in `steam_dir` first, then where Steam usually is
/// (`steam_local::steam_dir_candidates`; on Windows, the registry).
pub fn detect_steam_id(steam_dir: Option<&std::path::Path>) -> Option<String> {
    most_recent_login(steam_dir).map(|u| u.steam_id)
}

fn most_recent_login(steam_dir: Option<&std::path::Path>) -> Option<LocalUser> {
    crate::steam_local::steam_dir_candidates(steam_dir)
        .iter()
        .filter_map(|dir| std::fs::read_to_string(dir.join("config/loginusers.vdf")).ok())
        .find_map(|text| parse_login_users(&text))
}

/// An account signed in on the local Steam client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalUser {
    pub steam_id: String,
    pub persona: Option<String>,
}

/// Every account signed in on the local Steam client (its loginusers.vdf,
/// looked for as `detect_steam_id` does), the most recent first. Players
/// other than the first find their own Steam account here: a
/// family that shares one computer user often signs in to Steam in turn.
pub fn local_users(steam_dir: Option<&std::path::Path>) -> Vec<LocalUser> {
    crate::steam_local::steam_dir_candidates(steam_dir)
        .iter()
        .filter_map(|dir| std::fs::read_to_string(dir.join("config/loginusers.vdf")).ok())
        .map(|text| parse_all_login_users(&text))
        .find(|users| !users.is_empty())
        .unwrap_or_default()
}

/// The accounts in `steam_dir`'s own loginusers.vdf, the most recent first;
/// none when it can't be read.
pub fn login_users_in(steam_dir: &std::path::Path) -> Vec<LocalUser> {
    std::fs::read_to_string(steam_dir.join("config/loginusers.vdf")).map(|text| parse_all_login_users(&text)).unwrap_or_default()
}

/// The accounts in a loginusers.vdf, the one marked MostRecent first and the
/// rest as listed. It is read with the VDF parser, so a persona name with a
/// brace or an escaped quote in it can't end an account's block early.
fn parse_all_login_users(text: &str) -> Vec<LocalUser> {
    let Ok(vdf) = crate::steam_local::parse_text_vdf(text) else { return vec![] };
    let Some(users) = vdf.get("users") else { return vec![] };
    let mut users: Vec<(bool, LocalUser)> = users
        .entries()
        .iter()
        .filter(|(id, v)| id.len() == 17 && id.starts_with("7656") && id.bytes().all(|b| b.is_ascii_digit()) && matches!(v, Vdf::Map(_)))
        .map(|(id, v)| {
            let recent = v.get("MostRecent").and_then(Vdf::as_str) == Some("1");
            let persona = v.get("PersonaName").and_then(Vdf::as_str).map(str::to_string);
            (recent, LocalUser { steam_id: id.clone(), persona })
        })
        .collect();
    users.sort_by_key(|(recent, _)| !recent);
    users.into_iter().map(|(_, u)| u).collect()
}

/// The account marked MostRecent in a loginusers.vdf, else the first listed.
fn parse_login_users(text: &str) -> Option<LocalUser> {
    parse_all_login_users(text).into_iter().next()
}

/// Minimal HTML entity unescape for short descriptions.
/// `&amp;` goes last, so an escaped entity (`&amp;lt;`) comes out as
/// written (`&lt;`) rather than unescaped twice.
pub fn html_unescape(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{fixture, fixture_json};
    use serde_json::json;

    #[test]
    fn owned_games() {
        let games = parse_owned_games(&fixture_json("steam/owned_games.json")).unwrap();
        assert_eq!(games.len(), 3);
        assert_eq!((games[0].appid, games[0].name.as_str(), games[0].playtime_forever), (620, "Portal 2", 1260));
        assert_eq!(games[1].name, "Portal", "names are trimmed");
        assert_eq!(games[1].playtime_2weeks, 0, "missing fields default to 0");
        assert_eq!(games[2].name, "App 12345", "delisted apps get a placeholder name");
    }

    #[test]
    fn wishlist() {
        let items = parse_wishlist(&fixture_json("steam/wishlist.json")).unwrap();
        assert_eq!(
            items,
            vec![
                WishlistItem { appid: 1145350, added_at: Some(1780000000) },
                WishlistItem { appid: 400, added_at: Some(1781000000) },
                WishlistItem { appid: 1902490, added_at: None },
            ]
        );
        assert_eq!(parse_wishlist(&json!({"response": {}})), None, "a private wishlist shows nothing");
        assert_eq!(parse_wishlist(&json!({"response": {"items": []}})), Some(vec![]));
        let items =
            json!({"response": {"store_items": [{"appid": 620, "name": "Portal 2 "}, {"appid": 1, "name": ""}, {"name": "No appid"}]}});
        assert_eq!(parse_store_names(&items), std::collections::HashMap::from([(620, "Portal 2".to_string())]));
    }

    /// GetItems (names and art) goes through the Web API's limiter and
    /// backs off on a 429 like Steam's other calls; in `http::quick` it's
    /// tried once and fails at once.
    #[tokio::test]
    async fn store_items_back_off_and_fail_fast_when_quick() {
        use wiremock::matchers::path;
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let s = MockServer::start().await;
        Mock::given(path("/IStoreBrowseService/GetItems/v1/"))
            .respond_with(ResponseTemplate::new(429))
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&s)
            .await;
        let body = json!({"response": {"store_items": [{"appid": 620, "name": "Portal 2"}]}});
        Mock::given(path("/IStoreBrowseService/GetItems/v1/")).respond_with(ResponseTemplate::new(200).set_body_json(body)).mount(&s).await;
        let client = SteamClient::new(crate::http::client().unwrap(), None, None, Some(&s.uri()));
        let names = store_names(&client, &[620]).await.unwrap();
        assert_eq!(names.get(&620).map(String::as_str), Some("Portal 2"), "the 429 was waited out");
        assert_eq!(s.received_requests().await.unwrap().len(), 2);

        Mock::given(path("/IStoreBrowseService/GetItems/v1/"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&s)
            .await;
        let err = crate::http::quick(store_names(&client, &[620])).await.unwrap_err();
        assert!(format!("{err:#}").contains("HTTP 503"), "{err:#}");
        assert_eq!(s.received_requests().await.unwrap().len(), 3, "tried once");
    }

    /// SEC-M4: reqwest names the URL, `?key=` included, when a body doesn't
    /// decode; the Web API's answers that don't must not carry the key into
    /// the log or a job's message.
    #[tokio::test]
    async fn an_answer_that_isnt_json_leaves_the_key_out_of_the_error() {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::any};
        let s = MockServer::start().await;
        Mock::given(any()).respond_with(ResponseTemplate::new(200).set_body_string("<html>busy</html>")).mount(&s).await;
        let key = "ABCDEF0123456789ABCDEF0123456789";
        let client = SteamClient::new(crate::http::client().unwrap(), Some(key.into()), Some("76561197960287930".into()), Some(&s.uri()));
        let errors = [
            client.owned_games().await.map(|_| ()).unwrap_err(),
            client.recently_played().await.map(|_| ()).unwrap_err(),
            client.player_summary().await.map(|_| ()).unwrap_err(),
            client.wishlist("76561197960287930").await.map(|_| ()).unwrap_err(),
        ];
        for e in errors {
            let text = format!("{e:#} {e:?}");
            assert!(text.contains("non-JSON"), "{text}");
            assert!(!text.contains(key), "the key is in the error: {text}");
        }
    }

    #[test]
    fn owned_games_private_profile() {
        let err = parse_owned_games(&fixture_json("steam/owned_games_private.json")).unwrap_err();
        assert!(err.to_string().contains("'Game details' to Public"), "{err}");
    }

    #[test]
    fn recently_played() {
        let body = json!({"response": {"total_count": 2, "games": [
            {"appid": 1145360, "name": "Hades ", "playtime_2weeks": 95, "playtime_forever": 3120},
            {"appid": 620, "name": "Portal 2", "playtime_2weeks": 30, "playtime_forever": 1260}
        ]}});
        let games = parse_recently_played(&body).unwrap();
        assert_eq!(
            games.iter().map(|g| (g.appid, g.name.as_str(), g.playtime_2weeks, g.playtime_forever)).collect::<Vec<_>>(),
            vec![(1145360, "Hades", 95, 3120), (620, "Portal 2", 30, 1260)]
        );
        assert!(parse_recently_played(&json!({"response": {"total_count": 0}})).unwrap().is_empty(), "nothing played");
        let err = parse_recently_played(&json!({"response": {}})).unwrap_err();
        assert!(err.to_string().contains("'Game details' to Public"), "a private profile: {err}");
    }

    #[test]
    fn player_summary() {
        let p = parse_player_summary(&fixture_json("steam/player_summary.json"), "76561198000000000").unwrap();
        assert_eq!(p.persona_name.as_deref(), Some("Player One"));
        assert!(p.avatar.unwrap().ends_with("_full.jpg"));
        assert!(parse_player_summary(&json!({"response": {"players": []}}), "1").is_none());
    }

    #[test]
    fn app_details() {
        let d = parse_app_details(&fixture_json("steam/appdetails_620.json"), 620).unwrap().unwrap();
        assert_eq!(d.type_.as_deref(), Some("game"));
        assert_eq!(d.short_description.as_deref(), Some("A co-op puzzle game & the \"sequel\" to Portal."));
        assert_eq!(d.genres, ["Action", "Adventure"]);
        assert_eq!(d.categories, ["Single-player", "Multi-player", "Co-op"], "duplicates dropped");
        assert_eq!(d.metacritic, Some(95));
        assert_eq!(d.release_date.as_deref(), Some("Apr 18, 2011"));
        assert_eq!((d.developers, d.publishers), (vec!["Valve".to_string()], vec!["Valve".to_string()]));
        assert_eq!(d.is_free, Some(false));
        assert_eq!(d.controller_support.as_deref(), Some("full"));
        let mut platforms = d.platforms;
        platforms.sort();
        assert_eq!(platforms, ["linux", "windows"], "only platforms marked true");
    }

    #[test]
    fn app_details_edge_cases() {
        assert!(parse_app_details(&fixture_json("steam/appdetails_unlisted.json"), 1234).unwrap().is_none());
        // Keyed by a related id instead of the one asked for: still read.
        let mut v = fixture_json("steam/appdetails_620.json");
        let entry = v.as_object_mut().unwrap().remove("620").unwrap();
        let rekeyed = json!({ "999": entry });
        assert!(parse_app_details(&rekeyed, 620).unwrap().is_some());
        assert!(parse_app_details(&json!({}), 620).is_err());
    }

    #[test]
    fn store_art() {
        let art = parse_store_art(&fixture_json("steam/getitems_620.json"));
        let (portrait, hero, shots) = &art[&620];
        let base = "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/620/";
        assert_eq!(portrait.as_deref(), Some(format!("{base}library_600x900_2x.jpg?t=1790187113").as_str()));
        assert!(hero.as_deref().unwrap().starts_with(base) && hero.as_deref().unwrap().contains("/library_hero.jpg"));
        assert_eq!(shots.len(), 3);
        assert!(
            shots[0].contains("325.jpg") && shots[1].contains("6f4.jpg") && shots[2].contains("164.jpg"),
            "sorted by ordinal: {shots:?}"
        );
        assert_eq!(art[&999999], (None, None, vec![]), "no assets, no art");
    }

    #[test]
    fn reviews() {
        let (summary, texts) = parse_reviews(&fixture_json("steam/appreviews_620.json"), 10);
        let s = summary.unwrap();
        assert_eq!((s.review_score, s.total_reviews), (Some(9), Some(467348)));
        assert_eq!(s.review_score_desc.as_deref(), Some("Overwhelmingly Positive"));
        let texts = texts.unwrap();
        assert_eq!(texts.len(), 2, "non-English and very short reviews are skipped");
        assert_eq!((texts[0].positive, texts[0].votes, texts[0].hours, texts[0].posted), (Some(true), 812, Some(21), Some(1318291200)));
        assert_eq!((texts[1].positive, texts[1].posted), (Some(false), None));
        assert_eq!(parse_reviews(&fixture_json("steam/appreviews_620.json"), 1).1.unwrap().len(), 1);
        assert!(matches!(parse_reviews(&json!({"success": 2}), 10), (None, None)));
    }

    #[test]
    fn all_time_reviews_come_first_and_recent_ones_fill_in() {
        let r = |text: &str| crate::db::ReviewText {
            source: "steam".into(),
            positive: Some(true),
            rating: None,
            votes: 0,
            hours: None,
            title: None,
            text: text.into(),
            posted: None,
        };
        let texts = |v: &[crate::db::ReviewText]| v.iter().map(|x| x.text.clone()).collect::<Vec<_>>();
        let top: Vec<_> = (0..10).map(|i| r(&format!("top {i}"))).collect();
        let recent: Vec<_> = ["top 0", "new a", "new b", "new c", "new d", "new e"].iter().map(|t| r(t)).collect();
        let merged = merge_reviews(top.clone(), recent.clone(), 10);
        assert_eq!(texts(&merged), ["top 0", "top 1", "top 2", "top 3", "top 4", "top 5", "new a", "new b", "new c", "new d"]);
        // Few recent reviews: more all-time ones. No all-time ones: all recent.
        assert_eq!(merge_reviews(top.clone(), vec![r("new a")], 10).len(), 10);
        assert_eq!(texts(&merge_reviews(vec![], recent, 3)), ["top 0", "new a", "new b"]);
        assert_eq!(merge_reviews(top, vec![], 4).len(), 4);
    }

    #[test]
    fn store_search_match() {
        let items = fixture_json("steam/storesearch_portal2.json")["items"].as_array().unwrap().clone();
        let exact = best_store_match("Portal 2", &items).unwrap();
        assert_eq!((exact.0, exact.2), (620, 1.0));
        // Edition wording differs: the loose fallback still finds it.
        assert_eq!(best_store_match("Portal 2 - Definitive Edition", &items).map(|m| m.0), Some(620));
        // A demo in the results never stands in for the full game.
        assert_eq!(best_store_match("Grand Emprise 2: Portals Apart", &items).map(|m| m.0), Some(2722610));
        // A different number is a different game.
        assert!(best_store_match("Portal 3", &items).is_none());
        assert!(best_store_match("Unrelated Farming Game", &items).is_none());
    }

    /// The code-health review's B-STORES-3, the Steam store part: the sequel check
    /// reads Roman numerals and works both ways, so "Hades" no longer takes
    /// "Hades 2" (0.87) nor "Final Fantasy VII" VIII (0.94), and "Hades 2"
    /// finds "Hades II".
    #[test]
    fn known_pairs() {
        for (query, listed, same) in crate::names::KNOWN_PAIRS {
            let one = [serde_json::json!({"id": 1, "name": listed})];
            assert_eq!(best_store_match(query, &one).is_some(), *same, "{query} and {listed}");
        }
    }

    #[test]
    fn login_users() {
        let text = fixture("steam/loginusers.vdf");
        let user = parse_login_users(&text).unwrap();
        assert_eq!(user.steam_id, "76561198000000002", "the MostRecent account wins");
        assert_eq!(user.persona.as_deref(), Some("Second Player"));
        let only_first = text.replace("\"MostRecent\"\t\t\"1\"", "\"MostRecent\"\t\t\"0\"");
        assert_eq!(parse_login_users(&only_first).unwrap().steam_id, "76561198000000001");
        assert!(parse_login_users("\"users\" { }").is_none());
    }

    /// The code-health review's B-STORES-4: a `}` or an escaped quote in a persona
    /// name no longer ends the account's block early, so the MostRecent
    /// account is the one read, whatever the spacing.
    #[test]
    fn a_brace_in_a_persona_name_keeps_the_right_account() {
        let text = "\"users\"\n{\n\t\"76561198000000001\"\n\t{\n\t\t\"AccountName\"\t\t\"first\"\n\t\t\"PersonaName\"\t\t\"First\"\n\t\t\"MostRecent\"\t\t\"0\"\n\t}\n\t\"76561198000000002\"\n\t{\n\t\t\"AccountName\"\t\t\"second\"\n\t\t\"PersonaName\"\t\t\"}{ero \\\"Q\\\"\"\n\t\t\"MostRecent\"   \"1\"\n\t}\n}\n";
        let user = parse_login_users(text).unwrap();
        assert_eq!((user.steam_id.as_str(), user.persona.as_deref()), ("76561198000000002", Some("}{ero \"Q\"")));
        assert_eq!(parse_all_login_users(text).len(), 2);
    }

    /// The code-health review's B-STORES-11: an escaped entity is unescaped once.
    #[test]
    fn html_is_unescaped_once() {
        assert_eq!(html_unescape("Tom &amp; Jerry &lt;3 &amp;lt;b&amp;gt; &quot;x&quot;"), "Tom & Jerry <3 &lt;b&gt; \"x\"");
    }

    /// The code-health review's B-STORES-6: Steam's Web API answers a key it doesn't
    /// accept with 403 ("Retrying will not help"); that is asked once, not
    /// five times with backoff, and the sync can tell it from an outage.
    #[tokio::test]
    async fn a_refused_web_api_key_is_asked_once() {
        use wiremock::matchers::path;
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let s = MockServer::start().await;
        Mock::given(path("/IPlayerService/GetOwnedGames/v1/"))
            .respond_with(ResponseTemplate::new(403).set_body_string("<html><body>Access is denied. Retrying will not help.</body></html>"))
            .mount(&s)
            .await;
        let client = SteamClient::new(crate::http::client().unwrap(), Some("bad".into()), Some("76561198000000000".into()), Some(&s.uri()));
        let err = client.owned_games().await.unwrap_err();
        assert!(key_refused(&err), "{err:#}");
        assert_eq!(s.received_requests().await.unwrap().len(), 1, "a refused key was asked again");
    }

    /// The `steam_dir` folder is asked first, wherever it is (on Windows,
    /// Steam can be on any drive).
    #[test]
    fn the_login_is_read_from_the_steam_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(dir.path().join("config/loginusers.vdf"), fixture("steam/loginusers.vdf")).unwrap();
        assert_eq!(detect_steam_id(Some(dir.path())).as_deref(), Some("76561198000000002"));
        assert_eq!(detect_persona_name(Some(dir.path())).as_deref(), Some("Second Player"));
        let users = local_users(Some(dir.path()));
        let ids: Vec<(&str, Option<&str>)> = users.iter().map(|u| (u.steam_id.as_str(), u.persona.as_deref())).collect();
        assert_eq!(
            ids,
            [("76561198000000002", Some("Second Player")), ("76561198000000001", Some("First Player"))],
            "every account, the most recent first"
        );
    }
}
