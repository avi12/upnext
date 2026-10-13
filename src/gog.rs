//! GOG account access: OAuth login (the same public Galaxy client that
//! Heroic, gogdl and lgogdownloader use), owned-games listing, and per-game
//! playtime from GOG's cloud session log. The token lives in a local JSON
//! file under the data directory.

use crate::db::ImportedGame;
use crate::http::{Backoff, RateLimiter, base_url, send_with_backoff};
#[cfg(test)]
use crate::signin::FileSignIn;
use crate::signin::SignInStore;
use anyhow::{Context, Result, anyhow, bail};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tokio::sync::Mutex;

// GOG Galaxy's own public OAuth client, which every Galaxy install ships and
// Heroic, Lutris and gogdl use too: not a secret of this project, and nothing
// it grants is tied to UpNext. `.gitleaks.toml` allowlists it for scanners.
const CLIENT_ID: &str = "46899977096215655";
const CLIENT_SECRET: &str = "9d85c43b1482497dbbce61f6e4aa173a433796eeae2ca8c5f6129f2dc4de46d9";
const REDIRECT_URI: &str = "https://embed.gog.com/on_login_success?origin=client";

/// GOG only accepts its own landing page as the redirect URI, so the code is
/// read from there: by the login window, or pasted by hand as a fallback.
pub fn auth_url() -> String {
    format!("https://auth.gog.com/auth?client_id={CLIENT_ID}&redirect_uri={}&response_type=code&layout=client2", urlencode(REDIRECT_URI))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GogToken {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub user_id: String,
    #[serde(default)]
    pub username: Option<String>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct GogStatus {
    pub connected: bool,
    pub user_id: Option<String>,
    pub username: Option<String>,
    pub auth_url: String,
    /// A login window is open and UpNext is waiting for it.
    pub login_pending: bool,
    /// Why the last login window attempt failed, if it did.
    pub login_error: Option<String>,
}

/// GOG turned a token request down (as opposed to a network problem).
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct TokenRejected(String);

/// A sign-in in a browser that isn't the server's (Docker, another device):
/// the "Send to UpNext" bookmark brings GOG's code back with this sign-in's
/// state, issued here. Kept in memory: a restart ends every one.
#[derive(Debug, Clone)]
struct BrowserLogin {
    at: i64,
    /// Who started it (`server::gog_link`): this computer, or a token's id.
    owner: String,
}

/// How long a browser sign-in's state lasts, like Steam's.
pub const BROWSER_LOGIN_TTL: i64 = 600;

/// The most browser sign-ins waiting at once; the oldest goes first.
const BROWSER_LOGINS_MAX: usize = 16;

/// Why a browser sign-in's code wasn't taken.
#[derive(Debug, thiserror::Error)]
pub enum BrowserLoginError {
    /// The state wasn't issued here, was used already, has expired, or was
    /// issued to another device.
    #[error("that GOG sign-in didn't start here, was used already or expired: start it again from UpNext")]
    NotOurs,
    #[error("{0:#}")]
    Exchange(anyhow::Error),
}

/// State of the native login window flow.
#[derive(Debug, Default)]
struct WindowLogin {
    pending: bool,
    error: Option<String>,
}

const AUTH: &str = "https://auth.gog.com";
/// Tries for a token request that meets throttling or an outage.
const TOKEN_TRIES: u32 = 3;
const EMBED: &str = "https://embed.gog.com";
const GAMEPLAY: &str = "https://gameplay.gog.com";

pub struct GogClient {
    http: Client,
    auth: String,
    embed: String,
    gameplay: String,
    limiter: RateLimiter,
    /// Where the sign-in is kept: a connection in the database, or a file.
    store: std::sync::Arc<dyn SignInStore>,
    cache: Mutex<Option<GogToken>>,
    window: std::sync::Mutex<WindowLogin>,
    browser: std::sync::Mutex<std::collections::HashMap<String, BrowserLogin>>,
}

impl GogClient {
    /// `base` stands in for GOG's auth, account and gameplay hosts (tests
    /// pass a local mock server); None means the real ones. The sign-in is
    /// kept in the file `token_path`.
    #[cfg(test)]
    pub fn new(http: Client, token_path: std::path::PathBuf, base: Option<&str>) -> Self {
        Self::stored_in(http, std::sync::Arc::new(FileSignIn::new(token_path)), base)
    }

    /// A client whose sign-in is kept in `store`.
    pub fn stored_in(http: Client, store: std::sync::Arc<dyn SignInStore>, base: Option<&str>) -> Self {
        Self {
            http,
            auth: base_url(AUTH, base),
            embed: base_url(EMBED, base),
            gameplay: base_url(GAMEPLAY, base),
            limiter: RateLimiter::new("GOG", Duration::from_millis(350)),
            store,
            cache: Mutex::new(None),
            window: Default::default(),
            browser: Default::default(),
        }
    }

    /// The same client for another sign-in: another player's GOG account,
    /// read by a sync of their connection.
    pub fn with_store(&self, store: std::sync::Arc<dyn SignInStore>) -> Self {
        Self {
            http: self.http.clone(),
            auth: self.auth.clone(),
            embed: self.embed.clone(),
            gameplay: self.gameplay.clone(),
            limiter: RateLimiter::new("GOG", Duration::from_millis(350)),
            store,
            cache: Mutex::new(None),
            window: Default::default(),
            browser: Default::default(),
        }
    }

    pub fn load_token(&self) -> Option<GogToken> {
        serde_json::from_str(&self.store.load()?).ok()
    }

    fn save_token(&self, t: &GogToken) -> Result<()> {
        self.store.save(&serde_json::to_string_pretty(t)?).context("keeping the GOG sign-in")?;
        if let Ok(mut w) = self.window.lock() {
            w.error = None;
        }
        Ok(())
    }

    pub fn status(&self) -> GogStatus {
        let (login_pending, login_error) = self.window.lock().map(|w| (w.pending, w.error.clone())).unwrap_or((false, None));
        match self.load_token() {
            Some(t) => GogStatus {
                connected: true,
                user_id: Some(t.user_id),
                username: t.username,
                auth_url: auth_url(),
                login_pending,
                login_error: None,
            },
            None => GogStatus { connected: false, user_id: None, username: None, auth_url: auth_url(), login_pending, login_error },
        }
    }

    pub async fn disconnect(&self) -> Result<()> {
        *self.cache.lock().await = None;
        self.store.clear()
    }

    /// One request for a token. Only GOG saying no ends a sign-in: a 400 or
    /// 401, or an OAuth error naming a bad grant or token. Throttling, a
    /// timeout, a CDN's 403 page or GOG being down is tried again a few
    /// times, then fails with the sign-in kept, as Epic's does.
    async fn token_request(&self, params: &[(&str, &str)]) -> Result<GogToken> {
        let mut tries = 0u32;
        let resp = loop {
            tries += 1;
            let sent = self
                .http
                .get(format!("{}/token", self.auth))
                .query(&[("client_id", CLIENT_ID), ("client_secret", CLIENT_SECRET)])
                .query(params)
                .send()
                .await
                // The URL carries the refresh token or login code; keep it out of errors.
                .map_err(|e| e.without_url());
            let again = match &sent {
                Ok(r) => crate::epic::worth_retrying(r.status()),
                Err(_) => true,
            };
            if !again || tries >= TOKEN_TRIES {
                break sent?;
            }
            // Tests' mock servers don't need the wait.
            tokio::time::sleep(std::time::Duration::from_millis(if cfg!(test) { 5 } else { 2_000 }) * tries).await;
        };
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            let msg = body.get("error_description").or(body.get("error")).and_then(Value::as_str).unwrap_or("unknown error");
            let code = body.get("error").and_then(Value::as_str).unwrap_or_default();
            if matches!(status.as_u16(), 400 | 401) || ["invalid_grant", "invalid_token"].contains(&code) {
                return Err(TokenRejected(format!("GOG token request failed ({status}): {msg}")).into());
            }
            bail!("GOG token request failed ({status}): {msg}");
        }
        let get = |k: &str| body.get(k).and_then(Value::as_str).map(str::to_string);
        let expires_in = body.get("expires_in").and_then(Value::as_i64).unwrap_or(3600);
        Ok(GogToken {
            access_token: get("access_token").ok_or_else(|| anyhow!("no access_token in GOG response"))?,
            refresh_token: get("refresh_token").ok_or_else(|| anyhow!("no refresh_token in GOG response"))?,
            expires_at: crate::db::now() + expires_in,
            user_id: get("user_id").ok_or_else(|| anyhow!("no user_id in GOG response"))?,
            username: None,
        })
    }

    /// Open the native login window (`upnext gog-window`) and finish the login
    /// in the background when it reports a code. Poll `status()` for the outcome.
    /// `desktop` is where it opens (`display::detect`).
    pub fn start_window_login(self: &std::sync::Arc<Self>, desktop: &crate::display::Desktop) -> Result<()> {
        if !cfg!(feature = "desktop") {
            bail!("this UpNext was built without its windows (the `desktop` feature); sign in with the link and paste where it lands");
        }
        if !desktop.available {
            bail!("the computer UpNext runs on has no desktop to open GOG's window on; sign in with the link and paste where it lands");
        }
        {
            let mut w = self.window.lock().map_err(|_| anyhow!("login state poisoned"))?;
            if w.pending {
                bail!("a GOG login window is already open");
            }
            *w = WindowLogin { pending: true, error: None };
        }
        let exe = std::env::current_exe().context("locating the upnext binary")?;
        let child = crate::process::command(exe)
            .arg("gog-window")
            .envs(desktop.env.iter().map(|(k, v)| (k, v)))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                self.finish_window(Some(format!("could not open the login window: {e}")));
                return Err(anyhow!("could not open the login window: {e}"));
            }
        };
        let me = self.clone();
        tokio::spawn(async move {
            let out = tokio::time::timeout(Duration::from_secs(15 * 60), async {
                let stdout = child.stdout.take();
                let mut line = String::new();
                if let Some(s) = stdout {
                    use tokio::io::AsyncBufReadExt;
                    let _ = tokio::io::BufReader::new(s).read_line(&mut line).await;
                }
                let status = child.wait().await;
                (line, status)
            })
            .await;
            let err = match out {
                Err(_) => Some("the login window timed out after 15 minutes".to_string()),
                Ok((line, status)) => match line.trim().strip_prefix("CODE ") {
                    Some(code) => me.login(code).await.err().map(|e| format!("{e:#}")),
                    None if line.trim() == "CLOSED" => Some("the login window was closed before finishing".into()),
                    None => Some(format!(
                        "the login window could not start ({}); use the manual login instead",
                        status.map(|s| s.to_string()).unwrap_or_else(|e| e.to_string())
                    )),
                },
            };
            me.finish_window(err);
        });
        Ok(())
    }

    fn finish_window(&self, error: Option<String>) {
        if let Ok(mut w) = self.window.lock() {
            *w = WindowLogin { pending: false, error };
        }
    }

    /// Exchange the code from the redirect (or the whole redirect URL) for a token.
    pub async fn login(&self, code_or_url: &str) -> Result<GogStatus> {
        let code = extract_code(code_or_url).ok_or_else(|| anyhow!("no code found; paste the URL you landed on after logging in"))?;
        let mut token =
            self.token_request(&[("grant_type", "authorization_code"), ("code", &code), ("redirect_uri", REDIRECT_URI)]).await?;
        token.username = self.fetch_username(&token).await.ok();
        self.save_token(&token)?;
        *self.cache.lock().await = Some(token);
        Ok(self.status())
    }

    /// Start a sign-in in a browser that isn't the server's: a one-time state
    /// for `owner`, good for [`BROWSER_LOGIN_TTL`] seconds.
    pub fn start_browser_login(&self, owner: &str) -> String {
        let state = uuid::Uuid::new_v4().simple().to_string();
        let now = crate::db::now();
        let mut logins = self.browser.lock().unwrap_or_else(|e| e.into_inner());
        logins.retain(|_, l| now - l.at < BROWSER_LOGIN_TTL);
        while logins.len() >= BROWSER_LOGINS_MAX {
            let Some(oldest) = logins.iter().min_by_key(|(_, l)| l.at).map(|(k, _)| k.clone()) else { break };
            logins.remove(&oldest);
        }
        logins.insert(state.clone(), BrowserLogin { at: now, owner: owner.to_string() });
        state
    }

    /// Finish a browser sign-in: the state must be one issued here to
    /// `owner`, within its time, and it's used up whatever happens next, so
    /// a link made elsewhere can't attach another GOG account. The code is
    /// never logged.
    pub async fn finish_browser_login(&self, owner: &str, state: &str, code_or_url: &str) -> Result<GogStatus, BrowserLoginError> {
        let issued = self.browser.lock().unwrap_or_else(|e| e.into_inner()).remove(state);
        if !issued.is_some_and(|l| l.owner == owner && crate::db::now() - l.at < BROWSER_LOGIN_TTL) {
            return Err(BrowserLoginError::NotOurs);
        }
        self.login(code_or_url).await.map_err(BrowserLoginError::Exchange)
    }

    async fn fetch_username(&self, t: &GogToken) -> Result<String> {
        let v: Value = self.http.get(format!("{}/userData.json", self.embed)).bearer_auth(&t.access_token).send().await?.json().await?;
        v.get("username").and_then(Value::as_str).map(str::to_string).ok_or_else(|| anyhow!("no username"))
    }

    /// A token that is valid for at least another minute, refreshing if needed.
    async fn valid_token(&self) -> Result<GogToken> {
        let mut cache = self.cache.lock().await;
        if cache.is_none() {
            *cache = self.load_token();
        }
        let Some(mut t) = cache.clone() else { bail!("GOG account not connected (run `upnext gog-login`)") };
        if t.expires_at - crate::db::now() > 60 {
            return Ok(t);
        }
        // Another client for the same sign-in (a sync's, or another
        // process's) may have renewed it already, with a new refresh token.
        if let Some(stored) = self.load_token()
            && stored.expires_at > t.expires_at
        {
            *cache = Some(stored.clone());
            if stored.expires_at - crate::db::now() > 60 {
                return Ok(stored);
            }
            t = stored;
        }
        let mut fresh = match self.token_request(&[("grant_type", "refresh_token"), ("refresh_token", &t.refresh_token)]).await {
            Ok(f) => f,
            Err(e) => {
                // GOG refused the refresh token: the sign-in is over. Say so
                // instead of showing "connected" and failing every sync.
                if e.downcast_ref::<TokenRejected>().is_some() {
                    *cache = None;
                    let _ = self.store.clear();
                    if let Ok(mut w) = self.window.lock() {
                        w.error = Some("Your GOG sign-in expired. Connect again.".into());
                    }
                }
                return Err(e);
            }
        };
        fresh.username = t.username.clone();
        self.save_token(&fresh)?;
        *cache = Some(fresh.clone());
        Ok(fresh)
    }

    /// Owned GOG games (base games only) with store metadata.
    pub async fn owned_games(&self) -> Result<Vec<ImportedGame>> {
        let token = self.valid_token().await?;
        let mut out = vec![];
        let mut page = 1;
        loop {
            let resp = send_with_backoff(&self.limiter, &Backoff::default(), || {
                self.http
                    .get(format!("{}/account/getFilteredProducts", self.embed))
                    .query(&[("mediaType", "1"), ("page", &page.to_string()), ("sortBy", "title")])
                    .bearer_auth(&token.access_token)
            })
            .await?;
            let body: Value = resp.json().await.context("GOG getFilteredProducts returned non-JSON")?;
            let (games, total_pages) = parse_products_page(&body);
            out.extend(games);
            if page >= total_pages {
                break;
            }
            page += 1;
        }
        Ok(out)
    }

    /// Minutes played and last session end for one game, from GOG's cloud.
    pub async fn playtime(&self, game_id: &str) -> Result<Option<(i64, i64)>> {
        let token = self.valid_token().await?;
        let url = format!("{}/games/{game_id}/users/{}/sessions", self.gameplay, token.user_id);
        if !self.limiter.wait().await {
            return Err(crate::http::stopped());
        }
        let resp = self.http.get(&url).bearer_auth(&token.access_token).send().await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            self.limiter.penalize(Duration::from_secs(10)).await;
            bail!("GOG rate limited");
        }
        if !resp.status().is_success() {
            bail!("GOG sessions HTTP {}", resp.status());
        }
        let v: Value = resp.json().await?;
        Ok(Some(parse_sessions(&v)))
    }
}

/// One page of `account/getFilteredProducts`: the games on it (base games
/// only) and the total page count.
fn parse_products_page(body: &Value) -> (Vec<ImportedGame>, i64) {
    let mut games = vec![];
    let total_pages = body.get("totalPages").and_then(Value::as_i64).unwrap_or(1);
    for p in body.get("products").and_then(Value::as_array).into_iter().flatten() {
        let Some(id) = p.get("id").and_then(Value::as_i64) else { continue };
        let title = p.get("title").and_then(Value::as_str).unwrap_or("").trim().to_string();
        if title.is_empty() || p.get("isGame").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let image = p.get("image").and_then(Value::as_str).map(|i| if i.starts_with("//") { format!("https:{i}") } else { i.to_string() });
        let url = p.get("url").and_then(Value::as_str).map(|u| format!("https://www.gog.com{u}"));
        let genre = p.get("category").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
        let release = p.get("releaseDate").and_then(release_date);
        games.push(ImportedGame {
            source: "gog".into(),
            source_id: id.to_string(),
            launcher: "gog".into(),
            name: title,
            appid: None,
            installed: false,
            playtime_minutes: None,
            playtime_2weeks: 0,
            last_played: 0,
            art_cover: image.as_ref().map(|i| format!("{i}_product_card_v2_mobile_slider_639.jpg")),
            art_square: image.as_ref().map(|i| format!("{i}_glx_vertical_cover.jpg")),
            description: None,
            genres: genre.into_iter().collect(),
            developer: None,
            release_date: release,
            store_url: url,
            run_url: Some(format!("goggalaxy://openGameView/gog_{id}")),
        });
    }
    (games, total_pages)
}

/// A product's release date as YYYY-MM-DD. The API sends an object
/// `{date: "2008-09-16 00:00:00.000000", timezone_type, timezone}`; a Unix
/// timestamp (number or string) is also accepted.
fn release_date(r: &Value) -> Option<String> {
    if let Some(d) = r.get("date").and_then(Value::as_str) {
        return d.get(..10).filter(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok()).map(str::to_string);
    }
    let ts = r.as_i64().or_else(|| r.as_str().and_then(|s| s.parse().ok()))?;
    chrono::DateTime::from_timestamp(ts, 0).map(|d| d.format("%Y-%m-%d").to_string())
}

/// `(minutes played, last session end)` from a sessions response.
fn parse_sessions(v: &Value) -> (i64, i64) {
    let minutes = v.get("time_sum").and_then(Value::as_i64).unwrap_or(0);
    let last = v.get("last_session_date").and_then(Value::as_i64).unwrap_or(0);
    (minutes, last)
}

pub fn extract_code(s: &str) -> Option<String> {
    let s = s.trim();
    if let Some(i) = s.find("code=") {
        let rest = &s[i + 5..];
        let end = rest.find(['&', '#', ' ']).unwrap_or(rest.len());
        let code = &rest[..end];
        return if code.is_empty() { None } else { Some(code.to_string()) };
    }
    if !s.is_empty() && !s.contains("://") && !s.contains(' ') {
        return Some(s.to_string());
    }
    None
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::fixture_json;

    #[test]
    fn products_page() {
        let (games, pages) = parse_products_page(&fixture_json("gog/filtered_products.json"));
        assert_eq!(pages, 2);
        assert_eq!(games.len(), 1, "non-games and blank titles are skipped");
        let g = &games[0];
        assert_eq!((g.source.as_str(), g.source_id.as_str(), g.name.as_str()), ("gog", "1207658924", "The Witcher: Enhanced Edition"));
        assert_eq!(g.store_url.as_deref(), Some("https://www.gog.com/en/game/the_witcher"));
        assert_eq!(g.art_square.as_deref(), Some("https://images-1.gog-statics.com/abc123_glx_vertical_cover.jpg"));
        assert_eq!(g.genres, ["Role-playing"]);
        assert_eq!(g.run_url.as_deref(), Some("goggalaxy://openGameView/gog_1207658924"));
        // The live API sends {date: "2008-09-16 00:00:00.000000", timezone_type, timezone}.
        assert_eq!(g.release_date.as_deref(), Some("2008-09-16"));
    }

    #[test]
    fn release_date_forms() {
        let date = |v: serde_json::Value| {
            let body = serde_json::json!({"totalPages": 1, "products": [{"id": 1, "title": "X", "releaseDate": v}]});
            parse_products_page(&body).0[0].release_date.clone()
        };
        assert_eq!(date(serde_json::json!({"date": "2008-09-16 00:00:00.000000", "timezone": "UTC"})).as_deref(), Some("2008-09-16"));
        assert_eq!(date(serde_json::json!(1221523200)).as_deref(), Some("2008-09-16"));
        assert_eq!(date(serde_json::json!("1221523200")).as_deref(), Some("2008-09-16"));
        assert_eq!(date(serde_json::json!(null)), None);
    }

    #[test]
    fn sessions() {
        assert_eq!(parse_sessions(&fixture_json("gog/sessions.json")), (754, 1790000000));
        assert_eq!(parse_sessions(&serde_json::json!({})), (0, 0));
    }

    #[test]
    fn code_from_redirect() {
        assert_eq!(extract_code("https://embed.gog.com/on_login_success?origin=client&code=abc-123&x=1").as_deref(), Some("abc-123"));
        assert_eq!(extract_code("  abc123  ").as_deref(), Some("abc123"));
        assert_eq!(extract_code("https://embed.gog.com/on_login_success?code="), None);
        assert_eq!(extract_code("https://example.com/nothing"), None);
    }

    // -- tokens, against a local stand-in for GOG ------------------------------

    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// A client whose saved token expires at `expires_at`.
    fn signed_in(dir: &tempfile::TempDir, server: &MockServer, expires_at: i64) -> GogClient {
        let token_path = dir.path().join("gog_token.json");
        let t = GogToken {
            access_token: "old-access".into(),
            refresh_token: "old-refresh".into(),
            expires_at,
            user_id: "4800".into(),
            username: Some("tester".into()),
        };
        std::fs::write(&token_path, serde_json::to_string(&t).unwrap()).unwrap();
        GogClient::new(crate::http::client().unwrap(), token_path, Some(&server.uri()))
    }

    async fn serve_sessions(server: &MockServer, bearer: &str, times: u64) {
        Mock::given(method("GET"))
            .and(path("/games/1207658924/users/4800/sessions"))
            .and(header("authorization", format!("Bearer {bearer}").as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(fixture_json("gog/sessions.json")))
            .expect(times)
            .mount(server)
            .await;
    }

    /// The "Send to UpNext" bookmark's way in: only a state issued here, to
    /// the same device, once.
    #[tokio::test]
    async fn a_browser_sign_in_needs_its_own_state_once() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/token"))
            .and(query_param("grant_type", "authorization_code"))
            .and(query_param("code", "good-code"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "a", "refresh_token": "r", "expires_in": 3600, "user_id": "4800"
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(path("/userData.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "username": "tester" })))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let gog = GogClient::new(crate::http::client().unwrap(), dir.path().join("gog_token.json"), Some(&server.uri()));
        let not_ours = |r: Result<GogStatus, BrowserLoginError>| matches!(r, Err(BrowserLoginError::NotOurs));

        assert!(not_ours(gog.finish_browser_login("local", &"0".repeat(32), "good-code").await), "never issued");
        // Issued to another device: refused, and used up.
        let state = gog.start_browser_login("token:7");
        assert!(not_ours(gog.finish_browser_login("token:8", &state, "good-code").await));
        assert!(not_ours(gog.finish_browser_login("token:7", &state, "good-code").await), "used up by the wrong try");
        // Expired.
        let state = gog.start_browser_login("local");
        gog.browser.lock().unwrap().get_mut(&state).unwrap().at -= BROWSER_LOGIN_TTL;
        assert!(not_ours(gog.finish_browser_login("local", &state, "good-code").await));
        assert!(gog.load_token().is_none());

        let state = gog.start_browser_login("local");
        let url = "https://embed.gog.com/on_login_success?origin=client&code=good-code";
        let status = gog.finish_browser_login("local", &state, url).await.unwrap();
        assert_eq!((status.connected, status.username.as_deref()), (true, Some("tester")));
        assert!(not_ours(gog.finish_browser_login("local", &state, "good-code").await), "once");

        // At most a few wait at once.
        for _ in 0..=BROWSER_LOGINS_MAX {
            gog.start_browser_login("local");
        }
        assert_eq!(gog.browser.lock().unwrap().len(), BROWSER_LOGINS_MAX);
    }

    #[tokio::test]
    async fn valid_token_is_used_without_a_refresh() {
        let server = MockServer::start().await;
        Mock::given(path("/token")).respond_with(ResponseTemplate::new(500)).expect(0).mount(&server).await;
        serve_sessions(&server, "old-access", 1).await;
        let dir = tempfile::tempdir().unwrap();
        let gog = signed_in(&dir, &server, crate::db::now() + 3600);
        assert_eq!(gog.playtime("1207658924").await.unwrap(), Some((754, 1790000000)));
    }

    #[tokio::test]
    async fn expired_token_is_refreshed_once_and_saved() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/token"))
            .and(query_param("grant_type", "refresh_token"))
            .and(query_param("refresh_token", "old-refresh"))
            .and(query_param("client_id", CLIENT_ID))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "new-access", "refresh_token": "new-refresh", "expires_in": 3600, "user_id": "4800"
            })))
            .expect(1)
            .mount(&server)
            .await;
        serve_sessions(&server, "new-access", 2).await;
        let dir = tempfile::tempdir().unwrap();
        let gog = signed_in(&dir, &server, crate::db::now() + 30);
        gog.playtime("1207658924").await.unwrap();
        gog.playtime("1207658924").await.unwrap();

        let saved = gog.load_token().unwrap();
        assert_eq!((saved.access_token.as_str(), saved.refresh_token.as_str()), ("new-access", "new-refresh"));
        assert_eq!(saved.username.as_deref(), Some("tester"), "the username survives a refresh");
        assert!(saved.expires_at > crate::db::now() + 3000);
    }

    #[tokio::test]
    async fn refresh_turned_down_signs_out() {
        let server = MockServer::start().await;
        Mock::given(path("/token"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_json(serde_json::json!({"error": "invalid_grant", "error_description": "The refresh token is invalid."})),
            )
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let gog = signed_in(&dir, &server, crate::db::now() - 10);
        let err = gog.playtime("1207658924").await.unwrap_err();
        assert!(err.to_string().contains("The refresh token is invalid."), "{err:#}");
        assert!(gog.load_token().is_none(), "the dead token is deleted");
        let status = gog.status();
        assert!(!status.connected);
        assert_eq!(status.login_error.as_deref(), Some("Your GOG sign-in expired. Connect again."));
    }

    #[tokio::test]
    async fn refresh_failing_on_the_server_keeps_the_sign_in() {
        let server = MockServer::start().await;
        Mock::given(path("/token")).respond_with(ResponseTemplate::new(503)).mount(&server).await;
        let dir = tempfile::tempdir().unwrap();
        let gog = signed_in(&dir, &server, crate::db::now() - 10);
        assert!(gog.playtime("1207658924").await.is_err());
        assert!(gog.load_token().is_some(), "a GOG outage is not a sign-out");
        assert!(gog.status().connected);
    }

    /// The code-health review's B-STORES-5: a 403 page from in front of GOG's token
    /// endpoint (a CDN or a block) is tried again, then fails with the
    /// sign-in kept; it no longer signs the user out.
    #[tokio::test]
    async fn a_403_page_on_refresh_keeps_the_sign_in() {
        let server = MockServer::start().await;
        Mock::given(path("/token"))
            .respond_with(ResponseTemplate::new(403).set_body_string("<html>Access denied</html>"))
            .expect(u64::from(TOKEN_TRIES))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let gog = signed_in(&dir, &server, crate::db::now() - 10);
        assert!(gog.playtime("1207658924").await.is_err());
        assert!(gog.load_token().is_some(), "signed out by a 403 page: {:?}", gog.status().login_error);
        assert!(gog.status().connected);
    }

    /// A 403 ends the sign-in only when GOG names a bad grant in it: one
    /// with another OAuth error keeps the sign-in (the old rule signed out
    /// on every 403), and one saying `invalid_grant` drops it.
    #[tokio::test]
    async fn a_403_signs_out_only_when_it_names_a_bad_grant() {
        let refresh_answered = |error: &'static str| async move {
            let server = MockServer::start().await;
            Mock::given(path("/token"))
                .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({"error": error})))
                .mount(&server)
                .await;
            let dir = tempfile::tempdir().unwrap();
            let gog = signed_in(&dir, &server, crate::db::now() - 10);
            assert!(gog.playtime("1207658924").await.is_err());
            (gog.load_token().is_some(), gog.status().login_error)
        };
        assert_eq!(refresh_answered("access_denied").await, (true, None), "a 403 that names no bad grant keeps the sign-in");
        let (kept, error) = refresh_answered("invalid_grant").await;
        assert!(!kept);
        assert_eq!(error.as_deref(), Some("Your GOG sign-in expired. Connect again."));
    }
}
