//! Epic Games, read without Heroic: the owned library through the
//! person's own Epic sign-in, and the games the Epic Games Launcher has
//! installed, from its manifests on Windows.
//!
//! The sign-in is Epic's authorization-code flow with the Epic Games
//! Launcher's own public client, the one Legendary, Heroic and Playnite use:
//! the person signs in on Epic's page in their browser and pastes the code
//! it shows. UpNext never sees the password. The token lives in a local
//! JSON file next to the database, as GOG's does. The library, catalog and
//! playtime endpoints are the launcher's, unofficial and undocumented.
//!
//! Without a sign-in, the launcher's manifests list what is installed on
//! this computer, and a game uninstalled later stays in the library.

use crate::db::ImportedGame;
use crate::http::{Backoff, HttpStatusError, RateLimiter, base_url, send_with_backoff, text_capped};
#[cfg(test)]
use crate::signin::FileSignIn;
use crate::signin::SignInStore;
use anyhow::{Context, Result, anyhow, bail};
use reqwest::Client;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::Mutex;

// The Epic Games Launcher's own public OAuth client, which every launcher
// install ships and Legendary, Heroic and Playnite use too: not a secret of
// this project, and nothing it grants is tied to UpNext. `.gitleaks.toml`
// allowlists it for scanners.
const CLIENT_ID: &str = "34a02cf8f4414e29b15921876da36f9a";
const CLIENT_SECRET: &str = "daafbccc737745039dffe53d94fc76cf";

const OAUTH: &str = "https://account-public-service-prod03.ol.epicgames.com";
const LIBRARY: &str = "https://library-service.live.use1a.on.epicgames.com";
const CATALOG: &str = "https://catalog-public-service-prod06.ol.epicgames.com";
/// Tries of a token request on throttling, a timeout or an error at Epic.
const TOKEN_TRIES: u32 = 3;
/// The most of one answer read: a library page or a catalog entry is a
/// few hundred KB at most.
const MAX_ANSWER: usize = 16 << 20;

/// Epic's sign-in page, which ends on a page of JSON holding the code to paste.
pub fn auth_url() -> String {
    let redirect = format!("https://www.epicgames.com/id/api/redirect?clientId={CLIENT_ID}&responseType=code");
    format!("https://www.epicgames.com/id/login?redirectUrl={}", crate::heroic::urlencode(&redirect))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpicToken {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds.
    pub expires_at: i64,
    /// When the refresh token runs out (Unix seconds): a sync before then
    /// renews it, so a sign-in lasts while UpNext syncs now and then.
    pub refresh_expires_at: i64,
    pub account_id: String,
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct EpicStatus {
    pub connected: bool,
    /// The Epic display name signed in.
    pub display_name: Option<String>,
    /// Epic's sign-in page: it ends on a page showing `authorizationCode`.
    pub auth_url: String,
    /// Why the sign-in ended, when Epic turned it down at a sync.
    pub error: Option<String>,
}

/// Epic turned a token request down (as opposed to a network problem).
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct TokenRejected(String);

/// One game the account owns, as the library and catalog describe it.
#[derive(Debug, Clone, PartialEq)]
pub struct OwnedGame {
    pub app_name: String,
    pub namespace: String,
    pub catalog_item_id: String,
    pub title: String,
    pub description: Option<String>,
    pub developer: Option<String>,
    pub art_cover: Option<String>,
    pub art_square: Option<String>,
    /// Bought on Epic and run by another store's launcher: "EA app" or
    /// "Ubisoft Connect".
    pub third_party: Option<String>,
}

/// The owned library: games, with playtime when Epic gave it.
#[derive(Debug, Default)]
pub struct OwnedLibrary {
    pub games: Vec<OwnedGame>,
    /// Minutes played, by app name. None when Epic didn't answer; an owned
    /// game missing from it hasn't been played.
    pub playtime: Option<HashMap<String, i64>>,
    /// Library entries that aren't games: DLC, add-ons, Unreal Engine assets,
    /// software, demos.
    pub skipped: usize,
    /// App names whose catalog entry couldn't be read this time. The sync
    /// keeps the copies it already has of them, and the next one asks again.
    pub unread: Vec<String>,
}

/// Catalog lookups that fail in a row before the rest of a library read
/// stops asking: each has waited out its retries, and with the catalog down
/// a library of hundreds would take hours.
const CATALOG_GIVE_UP_AFTER: u32 = 3;

/// What the catalog says about an item, kept between syncs (catalog
/// entries hardly change, and a library has hundreds).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct CatalogEntry {
    title: String,
    /// A game, rather than DLC, an add-on, a plugin or software.
    game: bool,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    developer: Option<String>,
    #[serde(default)]
    art_cover: Option<String>,
    #[serde(default)]
    art_square: Option<String>,
    #[serde(default)]
    third_party: Option<String>,
}

pub struct EpicClient {
    http: Client,
    oauth: String,
    library: String,
    catalog: String,
    limiter: RateLimiter,
    /// Where the sign-in is kept: a connection in the database, or a file.
    store: std::sync::Arc<dyn SignInStore>,
    catalog_path: PathBuf,
    cache: Mutex<Option<EpicToken>>,
    /// Why the stored sign-in was dropped, until the next one.
    error: std::sync::Mutex<Option<String>>,
}

impl EpicClient {
    /// `base` stands in for Epic's account, library and catalog hosts (tests
    /// pass a local mock server); None means the real ones. The catalog's
    /// entries are kept in `catalog_path`, the sign-in in the file `token_path`.
    #[cfg(test)]
    pub fn new(http: Client, token_path: PathBuf, catalog_path: PathBuf, base: Option<&str>) -> Self {
        Self::stored_in(http, std::sync::Arc::new(FileSignIn::new(token_path)), catalog_path, base)
    }

    /// A client whose sign-in is kept in `store`.
    pub fn stored_in(http: Client, store: std::sync::Arc<dyn SignInStore>, catalog_path: PathBuf, base: Option<&str>) -> Self {
        Self {
            http,
            oauth: base_url(OAUTH, base),
            library: base_url(LIBRARY, base),
            catalog: base_url(CATALOG, base),
            limiter: RateLimiter::new("Epic", Duration::from_millis(200)),
            store,
            catalog_path,
            cache: Mutex::new(None),
            error: Default::default(),
        }
    }

    /// The same client for another sign-in: another player's Epic account,
    /// read by a sync of their connection.
    pub fn with_store(&self, store: std::sync::Arc<dyn SignInStore>) -> Self {
        Self {
            http: self.http.clone(),
            oauth: self.oauth.clone(),
            library: self.library.clone(),
            catalog: self.catalog.clone(),
            limiter: RateLimiter::new("Epic", Duration::from_millis(200)),
            store,
            catalog_path: self.catalog_path.clone(),
            cache: Mutex::new(None),
            error: Default::default(),
        }
    }

    pub fn load_token(&self) -> Option<EpicToken> {
        serde_json::from_str(&self.store.load()?).ok()
    }

    fn save_token(&self, t: &EpicToken) -> Result<()> {
        self.store.save(&serde_json::to_string_pretty(t)?).context("keeping the Epic sign-in")
    }

    pub fn status(&self) -> EpicStatus {
        let error = self.error.lock().ok().and_then(|e| e.clone());
        match self.load_token() {
            Some(t) => EpicStatus { connected: true, display_name: t.display_name, auth_url: auth_url(), error: None },
            None => EpicStatus { connected: false, display_name: None, auth_url: auth_url(), error },
        }
    }

    pub async fn disconnect(&self) -> Result<()> {
        *self.cache.lock().await = None;
        self.store.clear()
    }

    /// One request for a token. Only Epic saying no ends a sign-in: a 400
    /// or 401, or an error code naming a bad grant or token. Throttling, a
    /// timeout, a CDN's 403 or Epic being down is tried again a few times,
    /// then fails with the sign-in kept. This isn't `send_with_backoff`,
    /// which drops the answer of a refused request, and with it Epic's
    /// reason.
    async fn token_request(&self, params: &[(&str, &str)]) -> Result<EpicToken> {
        let mut tries = 0u32;
        let resp = loop {
            tries += 1;
            if !self.limiter.wait().await {
                return Err(crate::http::stopped());
            }
            let sent = self
                .http
                .post(format!("{}/account/api/oauth/token", self.oauth))
                .basic_auth(CLIENT_ID, Some(CLIENT_SECRET))
                .form(params)
                .send()
                .await
                .map_err(|e| e.without_url());
            let again = match &sent {
                Ok(r) => worth_retrying(r.status()),
                Err(_) => true,
            };
            if !again || tries >= TOKEN_TRIES {
                break sent.context("Epic's sign-in didn't answer")?;
            }
            // Tests' mock servers don't need the wait.
            tokio::time::sleep(Duration::from_millis(if cfg!(test) { 5 } else { 2_000 }) * tries).await;
        };
        let status = resp.status();
        let body: Value = text_capped(resp, MAX_ANSWER).await.ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
        let code = body.get("errorCode").and_then(Value::as_str).unwrap_or_default();
        if !status.is_success() || !code.is_empty() {
            let msg = body.get("errorMessage").and_then(Value::as_str).filter(|m| !m.is_empty()).unwrap_or(if code.is_empty() {
                "no reason given"
            } else {
                code
            });
            let refused = matches!(status, StatusCode::BAD_REQUEST | StatusCode::UNAUTHORIZED)
                || ["invalid_grant", "invalid_token", "invalid_refresh_token", "auth_token.invalid"].iter().any(|c| code.contains(c));
            if refused {
                return Err(TokenRejected(format!("Epic turned the sign-in down ({status}): {msg}")).into());
            }
            bail!("Epic's sign-in failed ({status}): {msg}");
        }
        token_from(&body, crate::db::now())
    }

    /// Exchange the code from Epic's page (the code, the page's JSON, or an
    /// address carrying `code=`) for a token.
    pub async fn login(&self, pasted: &str) -> Result<EpicStatus> {
        let code = auth_code(pasted).ok_or_else(|| anyhow!("no code found: paste the authorizationCode from Epic's page"))?;
        let token = self.token_request(&[("grant_type", "authorization_code"), ("code", &code), ("token_type", "eg1")]).await?;
        self.save_token(&token)?;
        *self.cache.lock().await = Some(token);
        if let Ok(mut e) = self.error.lock() {
            *e = None;
        }
        Ok(self.status())
    }

    /// A token good for at least another minute, renewed when needed.
    async fn valid_token(&self) -> Result<EpicToken> {
        let mut cache = self.cache.lock().await;
        if cache.is_none() {
            *cache = self.load_token();
        }
        let Some(mut t) = cache.clone() else { bail!("Epic isn't signed in") };
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
        match self.token_request(&[("grant_type", "refresh_token"), ("refresh_token", &t.refresh_token), ("token_type", "eg1")]).await {
            Ok(fresh) => {
                self.save_token(&fresh)?;
                *cache = Some(fresh.clone());
                Ok(fresh)
            }
            Err(e) => {
                // Epic refused the refresh token: the sign-in is over. Say so
                // instead of showing "connected" and failing every sync.
                if e.downcast_ref::<TokenRejected>().is_some() {
                    *cache = None;
                    let _ = self.store.clear();
                    if let Ok(mut err) = self.error.lock() {
                        *err = Some("Your Epic sign-in expired. Sign in again.".into());
                    }
                }
                Err(e)
            }
        }
    }

    async fn get_json(&self, url: &str, token: &EpicToken, query: &[(&str, &str)]) -> Result<Value> {
        let resp =
            send_with_backoff(&self.limiter, &Backoff::default(), || self.http.get(url).bearer_auth(&token.access_token).query(query))
                .await
                .map_err(quiet)?;
        json_capped(resp).await
    }

    /// Every game the account owns, with playtime.
    pub async fn owned_library(&self) -> Result<OwnedLibrary> {
        let token = self.valid_token().await?;
        let mut records = vec![];
        let mut cursor: Option<String> = None;
        loop {
            let mut query = vec![("includeMetadata", "true")];
            if let Some(c) = &cursor {
                query.push(("cursor", c.as_str()));
            }
            let page =
                self.get_json(&format!("{}/library/api/public/items", self.library), &token, &query).await.context("Epic's library")?;
            records.extend(page.get("records").and_then(Value::as_array).cloned().unwrap_or_default());
            match page.pointer("/responseMetadata/nextCursor").and_then(Value::as_str).filter(|c| !c.is_empty()) {
                // A cursor seen before would loop forever.
                Some(next) if cursor.as_deref() != Some(next) => cursor = Some(next.to_string()),
                _ => break,
            }
        }

        let mut catalog = self.load_catalog();
        let mut out = OwnedLibrary::default();
        let mut seen = std::collections::HashSet::new();
        let mut failures = 0;
        for r in &records {
            let field = |k: &str| r.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
            let (namespace, item, app_name) = (field("namespace"), field("catalogItemId"), field("appName"));
            // Unreal Engine marketplace assets, private sandboxes (builds a
            // developer was given) and entries with no app aren't games.
            if namespace == "ue" || field("sandboxType") == "PRIVATE" || app_name.is_empty() || item.is_empty() {
                out.skipped += 1;
                continue;
            }
            if !seen.insert(app_name.clone()) {
                continue;
            }
            let key = format!("{namespace}:{item}");
            let entry = match catalog.get(&key) {
                Some(e) => e.clone(),
                None if failures >= CATALOG_GIVE_UP_AFTER => {
                    out.unread.push(app_name);
                    continue;
                }
                None => {
                    let url = format!("{}/catalog/api/shared/namespace/{}/bulk/items", self.catalog, path_part(&namespace));
                    let query = [
                        ("id", item.as_str()),
                        ("includeDLCDetails", "true"),
                        ("includeMainGameDetails", "true"),
                        ("country", "US"),
                        ("locale", "en"),
                    ];
                    // One item that fails doesn't fail the library: the
                    // others still come in, and what was read is kept.
                    let found = match self.get_json(&url, &token, &query).await {
                        Ok(found) => found,
                        Err(e) => {
                            tracing::warn!("Epic's catalog for {app_name} wasn't read: {e:#}");
                            failures += 1;
                            out.unread.push(app_name);
                            continue;
                        }
                    };
                    failures = 0;
                    let Some(v) = found.get(&item) else {
                        out.skipped += 1;
                        continue;
                    };
                    let e = catalog_entry(v);
                    catalog.insert(key, e.clone());
                    e
                }
            };
            if !entry.game
                || entry.title.is_empty()
                || crate::heroic::is_demo_title(&entry.title)
                || crate::steam_local::is_test_app(&entry.title)
            {
                out.skipped += 1;
                continue;
            }
            out.games.push(OwnedGame {
                app_name,
                namespace,
                catalog_item_id: item,
                title: entry.title,
                description: entry.description,
                developer: entry.developer,
                art_cover: entry.art_cover,
                art_square: entry.art_square,
                third_party: entry.third_party,
            });
        }
        self.save_catalog(&catalog);

        out.playtime = match self.playtime(&token).await {
            Ok(v) => Some(playtime_minutes(&v)),
            Err(e) => {
                tracing::warn!("Epic's playtime wasn't read: {e:#}");
                None
            }
        };
        Ok(out)
    }

    /// Epic's playtime for the account, asked once: it's read softly, so a
    /// failure isn't retried, and its address (with the account id in it)
    /// stays out of the errors, the log and the sync's lines.
    async fn playtime(&self, token: &EpicToken) -> Result<Value> {
        let url = format!("{}/library/api/public/playtime/account/{}/all", self.library, path_part(&token.account_id));
        if !self.limiter.wait().await {
            return Err(crate::http::stopped());
        }
        let resp = self.http.get(url).bearer_auth(&token.access_token).send().await.map_err(|e| e.without_url())?;
        if !resp.status().is_success() {
            bail!("HTTP {}", resp.status());
        }
        json_capped(resp).await
    }

    fn load_catalog(&self) -> BTreeMap<String, CatalogEntry> {
        std::fs::read_to_string(&self.catalog_path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    fn save_catalog(&self, catalog: &BTreeMap<String, CatalogEntry>) {
        let write = || -> Result<()> {
            if let Some(dir) = self.catalog_path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&self.catalog_path, serde_json::to_vec(catalog)?)?;
            Ok(())
        };
        if let Err(e) = write() {
            tracing::warn!("Epic's catalog entries weren't kept in {}: {e:#}", self.catalog_path.display());
        }
    }
}

/// One part of a URL's path, from text a service sent: anything but
/// letters, digits and `-_.~` is percent-encoded, so it can't add path
/// segments. RAWG's slugs use it too.
pub(crate) fn path_part(s: &str) -> String {
    // `urlencode` writes a space as `+`, which a path takes literally.
    crate::heroic::urlencode(s).replace('+', "%20")
}

/// Statuses a token request is tried again on (GOG's too).
pub(crate) fn worth_retrying(status: StatusCode) -> bool {
    matches!(status, StatusCode::TOO_MANY_REQUESTS | StatusCode::REQUEST_TIMEOUT | StatusCode::FORBIDDEN) || status.is_server_error()
}

/// An error from one of Epic's services without its address, which can carry
/// ids.
fn quiet(e: anyhow::Error) -> anyhow::Error {
    if let Some(h) = e.downcast_ref::<HttpStatusError>() {
        return anyhow!("HTTP {}", h.status);
    }
    match e.downcast::<reqwest::Error>() {
        Ok(r) => r.without_url().into(),
        Err(e) => e,
    }
}

/// An answer as JSON, read with a cap so a runaway one can't fill memory.
async fn json_capped(resp: reqwest::Response) -> Result<Value> {
    let text = text_capped(resp, MAX_ANSWER).await.map_err(quiet)?;
    serde_json::from_str(&text).context("Epic's answer isn't JSON")
}

/// A token from Epic's answer. `expires_at` is an RFC 3339 time; `expires_in`
/// (seconds) is the fallback.
fn token_from(body: &Value, now: i64) -> Result<EpicToken> {
    let text = |k: &str| body.get(k).and_then(Value::as_str).map(str::to_string);
    let at = |at: &str, within: &str, default: i64| {
        text(at)
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(&t).ok())
            .map(|d| d.timestamp())
            .unwrap_or_else(|| now + body.get(within).and_then(Value::as_i64).unwrap_or(default))
    };
    Ok(EpicToken {
        access_token: text("access_token").ok_or_else(|| anyhow!("no access_token in Epic's answer"))?,
        refresh_token: text("refresh_token").ok_or_else(|| anyhow!("no refresh_token in Epic's answer"))?,
        expires_at: at("expires_at", "expires_in", 7200),
        refresh_expires_at: at("refresh_expires_at", "refresh_expires", 30 * 86_400),
        account_id: text("account_id").ok_or_else(|| anyhow!("no account_id in Epic's answer"))?,
        display_name: text("displayName"),
    })
}

/// The code in what was pasted: Epic's page of JSON (`authorizationCode`),
/// an address with `code=`, or the code itself (32 hex digits).
pub fn auth_code(pasted: &str) -> Option<String> {
    let t = pasted.trim();
    if t.starts_with('{') {
        let v: Value = serde_json::from_str(t).ok()?;
        return v.get("authorizationCode").and_then(Value::as_str).filter(|c| !c.is_empty()).map(str::to_string);
    }
    if let Some((_, rest)) = t.split_once("code=") {
        let code: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        return (!code.is_empty()).then_some(code);
    }
    let code = t.trim_matches('"');
    (code.len() >= 16 && code.len() <= 64 && code.chars().all(|c| c.is_ascii_alphanumeric())).then(|| code.to_string())
}

/// An item from the catalog, as UpNext keeps it. DLC, add-ons, plugins,
/// mods, software and digital extras aren't games, as Playnite and Legendary
/// read them; a launchable add-on is.
fn catalog_entry(v: &Value) -> CatalogEntry {
    let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let categories: Vec<&str> = v
        .get("categories")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|c| c.get("path").and_then(Value::as_str)).collect())
        .unwrap_or_default();
    let has = |c: &str| categories.contains(&c);
    let addon = v.get("mainGameItem").is_some_and(|m| !m.is_null()) && !has("addons/launchable");
    let other = ["digitalextras", "plugins", "plugins/engine", "mods", "software", "engines"].iter().any(|c| has(c));
    let attribute = |k: &str| v.pointer(&format!("/customAttributes/{k}/value")).and_then(Value::as_str).map(str::to_lowercase);
    let third_party =
        attribute("ThirdPartyManagedApp").or_else(|| attribute("ThirdPartyManagedProvider")).or_else(|| attribute("partnerLinkType"));
    let third_party = third_party.and_then(|p| match p.as_str() {
        "origin" | "the ea app" | "ea app" | "ea" => Some("EA app".to_string()),
        "uplay" | "ubisoftconnect" | "ubisoft connect" | "ubisoft" => Some("Ubisoft Connect".to_string()),
        _ => None,
    });
    let image = |kinds: &[&str]| {
        kinds.iter().find_map(|kind| {
            v.get("keyImages")?
                .as_array()?
                .iter()
                .find(|i| i.get("type").and_then(Value::as_str) == Some(kind))
                .and_then(|i| i.get("url")?.as_str())
                .map(str::to_string)
        })
    };
    CatalogEntry {
        title: text("title").unwrap_or_default(),
        game: has("applications") && !addon && !other,
        description: text("description"),
        developer: text("developer"),
        art_cover: image(&["DieselGameBox", "OfferImageWide"]).map(|u| crate::heroic::epic_resize(&u, 640)),
        art_square: image(&["DieselGameBoxTall", "OfferImageTall", "Thumbnail"]).map(|u| crate::heroic::epic_resize(&u, 400)),
        third_party,
    }
}

/// Epic's playtime answer, `[{artifactId, totalTime}]` in seconds, as
/// minutes by app name.
fn playtime_minutes(v: &Value) -> HashMap<String, i64> {
    let mut out = HashMap::new();
    for item in v.as_array().into_iter().flatten() {
        let (Some(app), Some(seconds)) = (item.get("artifactId").and_then(Value::as_str), item.get("totalTime").and_then(Value::as_i64))
        else {
            continue;
        };
        *out.entry(app.to_string()).or_insert(0) += seconds.max(0) / 60;
    }
    out
}

// ---------------------------------------------------------------------------
// The Epic Games Launcher's installs
// ---------------------------------------------------------------------------

/// A game the Epic Games Launcher installed, from its `.item` manifest.
#[derive(Debug, Clone, PartialEq)]
pub struct Installed {
    pub app_name: String,
    pub namespace: Option<String>,
    pub catalog_item_id: Option<String>,
    pub display_name: String,
}

impl Installed {
    /// The launcher's own link that starts the game, as its desktop
    /// shortcuts do: `com.epicgames.launcher://apps/<namespace>:<item>:<app>`.
    pub fn run_url(&self) -> String {
        run_url(&self.app_name, self.namespace.as_deref(), self.catalog_item_id.as_deref())
    }
}

/// The launcher's link that starts `app_name`; with its namespace and
/// catalog item when known, as the launcher's desktop shortcuts have it.
pub fn run_url(app_name: &str, namespace: Option<&str>, item: Option<&str>) -> String {
    match (namespace, item) {
        (Some(ns), Some(id)) => format!("com.epicgames.launcher://apps/{ns}%3A{id}%3A{app_name}?action=launch&silent=true"),
        _ => format!("com.epicgames.launcher://apps/{app_name}?action=launch&silent=true"),
    }
}

/// The launcher's manifests folder: the `epic_manifests` setting, else, on
/// Windows, `%ProgramData%\Epic\EpicGamesLauncher\Data\Manifests`. Tests
/// never read this machine's (as Steam's and Heroic's).
pub fn find_manifests_dir(explicit: Option<&Path>) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = explicit.map(Path::to_path_buf).into_iter().collect();
    if !cfg!(test)
        && cfg!(windows)
        && let Some(data) = std::env::var_os("ProgramData")
    {
        candidates.push(PathBuf::from(data).join(r"Epic\EpicGamesLauncher\Data\Manifests"));
    }
    candidates.into_iter().find(|p| p.is_dir())
}

/// The games installed through the launcher. DLC, Unreal Engine plugins and
/// unfinished installs are left out; a manifest that doesn't parse (the
/// launcher sometimes leaves one empty) is skipped.
pub fn read_manifests(dir: &Path) -> Result<Vec<Installed>> {
    let mut out: Vec<Installed> = vec![];
    let entries = std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?;
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("item")))
        .collect();
    paths.sort();
    for path in paths {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let Ok(m) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else {
            tracing::warn!("skipped an Epic manifest that doesn't parse: {}", path.display());
            continue;
        };
        let text = |k: &str| m.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        let list = |k: &str| -> Vec<String> {
            m.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
        };
        let Some(app_name) = text("AppName") else { continue };
        let categories = list("AppCategories");
        let has = |c: &str| categories.iter().any(|x| x == c);
        let dlc = has("addons") && !has("addons/launchable");
        let plugin = has("plugins")
            || has("plugins/engine")
            || list("CompatibleApps").iter().any(|a| a.starts_with("UE_"))
            || text("TechnicalType").is_some_and(|t| t.contains("plugins/engine"));
        let incomplete = m.get("bIsIncompleteInstall").and_then(Value::as_bool) == Some(true);
        if dlc || plugin || incomplete {
            continue;
        }
        let display_name = text("DisplayName").unwrap_or_else(|| app_name.clone());
        if out.iter().any(|g| g.app_name == app_name) {
            continue;
        }
        out.push(Installed { app_name, namespace: text("CatalogNamespace"), catalog_item_id: text("CatalogItemId"), display_name });
    }
    Ok(out)
}

/// The account's games as copies, marked installed (with the launcher's
/// link) where the launcher installed them, and the installed games the
/// account didn't list (a game from another account on this computer) too.
pub fn library_copies(owned: &OwnedLibrary, installed: &[Installed]) -> Vec<ImportedGame> {
    let installed_by: HashMap<&str, &Installed> = installed.iter().map(|i| (i.app_name.as_str(), i)).collect();
    let mut out: Vec<ImportedGame> = owned
        .games
        .iter()
        .map(|g| {
            let here = installed_by.get(g.app_name.as_str());
            ImportedGame {
                source: "epic".into(),
                source_id: g.app_name.clone(),
                launcher: "epic".into(),
                name: g.title.clone(),
                installed: here.is_some(),
                playtime_minutes: owned.playtime.as_ref().map(|p| p.get(&g.app_name).copied().unwrap_or(0)),
                art_cover: g.art_cover.clone(),
                art_square: g.art_square.clone(),
                description: g.description.clone().filter(|d| d != &g.title),
                developer: g.developer.clone(),
                run_url: here.map(|i| i.run_url()),
                ..Default::default()
            }
        })
        .collect();
    for i in installed {
        if !owned.games.iter().any(|g| g.app_name == i.app_name) {
            out.push(installed_copy(i));
        }
    }
    out
}

/// An installed game as a copy, when the account isn't read.
pub fn installed_copy(i: &Installed) -> ImportedGame {
    ImportedGame {
        source: "epic".into(),
        source_id: i.app_name.clone(),
        launcher: "epic".into(),
        name: i.display_name.clone(),
        installed: true,
        run_url: Some(i.run_url()),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{basic_auth, body_string_contains, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client(dir: &Path, server: &MockServer) -> EpicClient {
        EpicClient::new(
            crate::http::client().unwrap(),
            dir.join("epic_token.json"),
            dir.join("cache/epic-catalog.json"),
            Some(&server.uri()),
        )
    }

    fn token_answer(access: &str) -> serde_json::Value {
        json!({ "access_token": access, "expires_in": 7200, "expires_at": "2099-01-01T00:00:00.000Z", "token_type": "bearer",
            "refresh_token": "refresh-1", "refresh_expires": 1_987_200, "refresh_expires_at": "2099-01-20T00:00:00.000Z",
            "account_id": "acct1", "client_id": CLIENT_ID, "displayName": "Player One" })
    }

    fn item(id: &str, title: &str, categories: &[&str], extra: serde_json::Value) -> serde_json::Value {
        let mut v = json!({ "id": id, "title": title, "description": format!("About {title}."), "developer": "Studio",
            "categories": categories.iter().map(|c| json!({ "path": c })).collect::<Vec<_>>(),
            "keyImages": [
                { "type": "DieselGameBoxTall", "url": format!("https://cdn1.epicgames.com/{id}-tall.jpg") },
                { "type": "DieselGameBox", "url": format!("https://cdn1.epicgames.com/{id}-wide.jpg") }
            ] });
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        json!({ id: v })
    }

    #[test]
    fn the_code_is_found_in_whatever_was_pasted() {
        let code = "0123456789abcdef0123456789abcdef";
        assert_eq!(auth_code(code).as_deref(), Some(code));
        assert_eq!(auth_code(&format!("  \"{code}\" ")).as_deref(), Some(code));
        let page = format!(
            r#"{{"warning":"Do not share this code","redirectUrl":"https://localhost/launcher/authorized?code={code}","authorizationCode":"{code}","sid":null}}"#
        );
        assert_eq!(auth_code(&page).as_deref(), Some(code));
        assert_eq!(auth_code(&format!("https://localhost/launcher/authorized?code={code}&x=1")).as_deref(), Some(code));
        assert_eq!(auth_code(r#"{"authorizationCode":null}"#), None);
        assert_eq!(auth_code("my password"), None);
        assert!(
            auth_url().starts_with("https://www.epicgames.com/id/login?redirectUrl=https%3A%2F%2Fwww.epicgames.com%2Fid%2Fapi%2Fredirect")
        );
    }

    /// Sign in with a pasted page, read the library through two pages with
    /// the catalog and playtime, and leave out what isn't a game. The
    /// catalog's entries are kept, so a second read asks only the library.
    #[tokio::test]
    async fn signs_in_and_reads_the_owned_library() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/account/api/oauth/token"))
            .and(body_string_contains("grant_type=authorization_code"))
            .and(body_string_contains("code=0123456789abcdef0123456789abcdef"))
            .and(basic_auth(CLIENT_ID, CLIENT_SECRET))
            .respond_with(ResponseTemplate::new(200).set_body_json(token_answer("access-1")))
            .expect(1)
            .mount(&server)
            .await;
        let lib = |cursor: Option<&str>, records: serde_json::Value, next: Option<&str>| {
            let mut m = Mock::given(method("GET")).and(path("/library/api/public/items")).and(header("authorization", "Bearer access-1"));
            if let Some(c) = cursor {
                m = m.and(query_param("cursor", c));
            }
            m.respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "records": records, "responseMetadata": { "nextCursor": next } })),
            )
        };
        let rec = |ns: &str, item: &str, app: &str| json!({ "namespace": ns, "catalogItemId": item, "appName": app, "sandboxType": "PUBLIC", "recordType": "APPLICATION" });
        lib(Some("page2"), json!([rec("ns3", "dlc1", "SpaceDlc"), rec("ns4", "ea1", "EaGame"), rec("ns5", "demo1", "DemoApp")]), None)
            .expect(2)
            .mount(&server)
            .await;
        lib(
            None,
            json!([
                rec("ns1", "space1", "SpaceApp"),
                rec("ue", "asset1", "Asset"),
                json!({ "namespace": "ns2", "catalogItemId": "x", "appName": "Private", "sandboxType": "PRIVATE" })
            ]),
            Some("page2"),
        )
        .expect(2)
        .mount(&server)
        .await;
        let catalog = |ns: &str, id: &str, body: serde_json::Value| {
            Mock::given(method("GET"))
                .and(path(format!("/catalog/api/shared/namespace/{ns}/bulk/items")))
                .and(query_param("id", id))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .expect(1)
        };
        catalog("ns1", "space1", item("space1", "Space Game™", &["public", "games", "applications"], json!({}))).mount(&server).await;
        catalog(
            "ns3",
            "dlc1",
            item("dlc1", "Space Game: Expansion", &["addons", "applications"], json!({ "mainGameItem": { "id": "space1" } })),
        )
        .mount(&server)
        .await;
        let ea = json!({ "customAttributes": { "ThirdPartyManagedApp": { "type": "STRING", "value": "The EA App" } } });
        catalog("ns4", "ea1", item("ea1", "Battlefield 2042", &["games", "applications"], ea)).mount(&server).await;
        catalog("ns5", "demo1", item("demo1", "Racer Demo", &["games", "applications"], json!({}))).mount(&server).await;
        Mock::given(method("GET"))
            .and(path("/library/api/public/playtime/account/acct1/all"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!([{ "accountId": "acct1", "artifactId": "SpaceApp", "totalTime": 7260 }])),
            )
            .expect(2)
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let epic = client(dir.path(), &server);
        assert!(!epic.status().connected);
        let page = r#"{"redirectUrl":"https://localhost/launcher/authorized?code=0123456789abcdef0123456789abcdef","authorizationCode":"0123456789abcdef0123456789abcdef"}"#;
        let status = epic.login(page).await.unwrap();
        assert_eq!((status.connected, status.display_name.as_deref()), (true, Some("Player One")));
        let saved = epic.load_token().unwrap();
        assert_eq!((saved.account_id.as_str(), saved.expires_at), ("acct1", 4_070_908_800));

        let lib = epic.owned_library().await.unwrap();
        let names: Vec<&str> = lib.games.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(names, ["Space Game™", "Battlefield 2042"]);
        assert_eq!(lib.skipped, 4, "the asset, the private build, the DLC and the demo");
        let space = &lib.games[0];
        assert_eq!(space.art_square.as_deref(), Some("https://cdn1.epicgames.com/space1-tall.jpg?resize=1&w=400"));
        assert_eq!(space.art_cover.as_deref(), Some("https://cdn1.epicgames.com/space1-wide.jpg?resize=1&w=640"));
        assert_eq!(lib.games[1].third_party.as_deref(), Some("EA app"));
        assert_eq!(lib.playtime.as_ref().unwrap().get("SpaceApp"), Some(&121));

        let installed = vec![Installed {
            app_name: "SpaceApp".into(),
            namespace: Some("ns1".into()),
            catalog_item_id: Some("space1".into()),
            display_name: "Space Game".into(),
        }];
        let copies = library_copies(&lib, &installed);
        assert_eq!((copies[0].installed, copies[0].playtime_minutes), (true, Some(121)));
        assert_eq!(copies[0].run_url.as_deref(), Some("com.epicgames.launcher://apps/ns1%3Aspace1%3ASpaceApp?action=launch&silent=true"));
        assert_eq!((copies[1].installed, copies[1].playtime_minutes, copies[1].run_url.as_deref()), (false, Some(0), None));

        // The catalog answers once per item (its mocks expect one call).
        let again = epic.owned_library().await.unwrap();
        assert_eq!(again.games, lib.games);
    }

    /// A refused refresh ends the sign-in, and Settings says why.
    fn expired() -> EpicToken {
        EpicToken {
            access_token: "old".into(),
            refresh_token: "r".into(),
            expires_at: 0,
            refresh_expires_at: 0,
            account_id: "acct/1".into(),
            display_name: None,
        }
    }

    /// Throttling and Epic's own errors are tried again, and when they last,
    /// the sign-in is kept: only Epic saying no ends it.
    #[tokio::test]
    async fn a_busy_epic_never_ends_the_sign_in() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/account/api/oauth/token"))
            .respond_with(ResponseTemplate::new(429))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/account/api/oauth/token"))
            .and(body_string_contains("grant_type=authorization_code"))
            .respond_with(ResponseTemplate::new(200).set_body_json(token_answer("access-1")))
            .expect(1)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let epic = client(dir.path(), &server);
        assert!(epic.login("0123456789abcdef0123456789abcdef").await.unwrap().connected, "signed in on the second try");

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/account/api/oauth/token"))
            .respond_with(ResponseTemplate::new(403).set_body_string("<html>Access denied</html>"))
            .expect(u64::from(TOKEN_TRIES))
            .mount(&server)
            .await;
        let epic = client(dir.path(), &server);
        epic.save_token(&expired()).unwrap();
        let e = epic.owned_library().await.unwrap_err();
        assert!(format!("{e:#}").contains("403"), "{e:#}");
        let status = epic.status();
        assert!(status.connected && status.error.is_none(), "kept: {status:?}");
        assert!(dir.path().join("epic_token.json").exists());
    }

    /// Ids Epic sends go into addresses encoded, and the playtime's address,
    /// which carries the account id, stays out of its error.
    #[tokio::test]
    async fn ids_in_addresses_are_encoded_and_kept_out_of_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/library/api/public/items"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "records": [
                { "namespace": "ns/../x y", "catalogItemId": "item1", "appName": "App1", "sandboxType": "PUBLIC" }
            ], "responseMetadata": {} })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/catalog/api/shared/namespace/ns%2F..%2Fx%20y/bulk/items"))
            .respond_with(ResponseTemplate::new(200).set_body_json(item("item1", "Space Game", &["games", "applications"], json!({}))))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/library/api/public/playtime/account/acct%2F1/all"))
            .respond_with(ResponseTemplate::new(500))
            // Once in the sync, never tried again, and once below.
            .expect(2)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let epic = client(dir.path(), &server);
        let mut token = expired();
        token.expires_at = crate::db::now() + 3600;
        epic.save_token(&token).unwrap();
        let lib = epic.owned_library().await.unwrap();
        assert_eq!((lib.games.len(), lib.playtime.is_none()), (1, true), "{lib:?}");
        let e = format!("{:#}", epic.playtime(&token).await.unwrap_err());
        assert!(e.contains("500") && !e.contains("acct"), "{e}");
    }

    #[tokio::test]
    async fn an_expired_sign_in_is_dropped_and_said() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/account/api/oauth/token"))
            .and(body_string_contains("grant_type=refresh_token"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_json(json!({ "errorCode": "errors.com.epicgames.account.auth_token.invalid_refresh_token",
                "errorMessage": "Sorry the refresh token is invalid" })),
            )
            .expect(1)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let epic = client(dir.path(), &server);
        let old = EpicToken {
            access_token: "old".into(),
            refresh_token: "r".into(),
            expires_at: 0,
            refresh_expires_at: 0,
            account_id: "a".into(),
            display_name: None,
        };
        epic.save_token(&old).unwrap();
        let e = epic.owned_library().await.unwrap_err();
        assert!(format!("{e:#}").contains("refresh token is invalid"), "{e:#}");
        let status = epic.status();
        assert!(!status.connected);
        assert_eq!(status.error.as_deref(), Some("Your Epic sign-in expired. Sign in again."));
        assert!(!dir.path().join("epic_token.json").exists());
    }

    #[test]
    fn the_launchers_manifests_list_installed_games() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, v: serde_json::Value| std::fs::write(dir.path().join(name), v.to_string()).unwrap();
        write(
            "A1.item",
            json!({ "AppName": "SpaceApp", "DisplayName": "Space Game", "CatalogNamespace": "ns1", "CatalogItemId": "space1",
            "AppCategories": ["public", "games", "applications"], "InstallLocation": "C:\\Games\\Space", "bIsIncompleteInstall": false }),
        );
        write("A2.item", json!({ "AppName": "SpaceDlc", "DisplayName": "Space DLC", "AppCategories": ["public", "addons"] }));
        write("A3.item", json!({ "AppName": "Plugin", "DisplayName": "A Plugin", "CompatibleApps": ["UE_5.3"] }));
        write("A4.item", json!({ "AppName": "Half", "DisplayName": "Half Installed", "bIsIncompleteInstall": true }));
        write(
            "A5.item",
            json!({ "AppName": "Launchable", "DisplayName": "Launchable Extra", "AppCategories": ["addons", "addons/launchable"] }),
        );
        std::fs::write(dir.path().join("A6.item"), "").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "{}").unwrap();
        let found = read_manifests(dir.path()).unwrap();
        let names: Vec<&str> = found.iter().map(|g| g.display_name.as_str()).collect();
        assert_eq!(names, ["Space Game", "Launchable Extra"]);
        assert_eq!(found[1].run_url(), "com.epicgames.launcher://apps/Launchable?action=launch&silent=true");
        assert_eq!(find_manifests_dir(Some(dir.path())).as_deref(), Some(dir.path()));
        assert_eq!(find_manifests_dir(Some(&dir.path().join("nope"))), None, "tests never look at this machine's launcher");
    }

    /// The code-health review's B-STORES-2: a catalog item that keeps failing is left
    /// unread, the others still come in, and the catalog read before it is
    /// kept. After three failures in a row the rest isn't asked for.
    #[tokio::test]
    async fn a_failing_catalog_item_is_left_unread_and_the_rest_kept() {
        let server = MockServer::start().await;
        let rec =
            |ns: &str, item: &str, app: &str| json!({ "namespace": ns, "catalogItemId": item, "appName": app, "sandboxType": "PUBLIC" });
        let mut records = vec![rec("ns1", "a1", "AppA")];
        records.extend((2..=6).map(|i| rec(&format!("bad{i}"), &format!("b{i}"), &format!("App{i}"))));
        Mock::given(path("/library/api/public/items"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "records": records, "responseMetadata": {} })))
            .mount(&server)
            .await;
        let a = json!({ "a1": { "id": "a1", "title": "Game A", "categories": [{ "path": "games" }, { "path": "applications" }] } });
        Mock::given(path("/catalog/api/shared/namespace/ns1/bulk/items"))
            .and(query_param("id", "a1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(a))
            .mount(&server)
            .await;
        Mock::given(wiremock::matchers::path_regex("^/catalog/api/shared/namespace/bad"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let epic = EpicClient::new(
            crate::http::client().unwrap(),
            dir.path().join("t.json"),
            dir.path().join("cache/epic-catalog.json"),
            Some(&server.uri()),
        );
        epic.save_token(&EpicToken {
            access_token: "x".into(),
            refresh_token: "r".into(),
            expires_at: crate::db::now() + 3600,
            refresh_expires_at: 0,
            account_id: "acct".into(),
            display_name: None,
        })
        .unwrap();
        let lib = epic.owned_library().await.unwrap();
        assert_eq!(lib.games.iter().map(|g| g.title.as_str()).collect::<Vec<_>>(), ["Game A"]);
        assert_eq!(lib.unread, ["App2", "App3", "App4", "App5", "App6"]);
        let asked: std::collections::HashSet<String> = server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|r| r.url.path().to_string())
            .filter(|p| p.contains("/namespace/bad"))
            .collect();
        assert_eq!(asked.len(), 3, "stops asking after three failures in a row: {asked:?}");
        let kept = std::fs::read_to_string(dir.path().join("cache/epic-catalog.json")).unwrap();
        assert!(kept.contains("Game A"), "{kept}");
    }
}
