//! Steam in Settings: checking the Web API key, and signing in with Steam
//! (OpenID) to set the Steam ID.

use super::players::Me;
use super::{ApiError, ApiResult, AppState, access, accounts};
use crate::db::PlayerId;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route("/steam/test", post(steam_test))
        .route("/steam/login", get(steam_login))
        .route("/steam/callback", get(steam_callback))
}

#[utoipa::path(post, path = "/steam/test", tag = "accounts",
    description = "Check the Steam Web API key and Steam ID: reads the owned games and the profile, and saves the profile.",
    responses((status = 200, body = Object, description = "`{ok: true, persona_name, avatar, game_count}`"),
        (status = 400, description = "No key or ID, or Steam refused them")))]
async fn steam_test(State(state): State<AppState>, Me(me): Me) -> ApiResult<Value> {
    let cfg = state.cfg_for(me);
    let (key, id) = cfg.steam_credentials().map_err(|e| ApiError::bad(e.to_string()))?;
    let steam = crate::steam::SteamClient::new(
        state.clients.http.clone(),
        Some(key.to_string()),
        Some(id.to_string()),
        state.clients.base_url.as_deref(),
    );
    // Someone is waiting on "Test API key", so each request is tried once.
    let owned = crate::http::quick(steam.owned_games()).await.map_err(|e| ApiError::bad(format!("{e:#}")))?;
    let summary = crate::http::quick(steam.player_summary()).await.map_err(|e| ApiError::bad(format!("{e:#}")))?;
    if let Some(p) = &summary {
        state.db.set_profile(p)?;
    }
    Ok(Json(json!({
        "ok": true,
        "persona_name": summary.as_ref().and_then(|p| p.persona_name.clone()),
        "avatar": summary.as_ref().and_then(|p| p.avatar.clone()),
        "game_count": owned.len(),
    })))
}

pub(super) const OPENID_NS: &str = "http://specs.openid.net/auth/2.0";
const OPENID_SELECT: &str = "http://specs.openid.net/auth/2.0/identifier_select";

pub(super) fn urlenc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Where Steam sends the browser back to: `public_url` when the request was
/// addressed to it (with its scheme, so a proxy's HTTPS is kept), else the
/// host the request named, over HTTPS when the request came over the
/// built-in HTTPS: the sign-in's cookie is `Secure` then, and a browser
/// sent back over plain HTTP wouldn't send it.
fn origin_from(headers: &axum::http::HeaderMap, state: &AppState, https: bool) -> String {
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}:{}", state.base_cfg.host, state.base_cfg.port));
    match access::public_url(state) {
        Some(p) if access::names_public(&p, &host) => p.origin(),
        _ => format!("{}://{host}", if https { "https" } else { "http" }),
    }
}

/// A Steam sign-in started here: when, the browser that started it, and
/// the page it goes back to. The callback only accepts a token it has one
/// of, once, within ten minutes, from the browser it was given to, so
/// another page can't hand us a Steam assertion of its own and a state seen
/// elsewhere is no use there.
#[derive(Clone)]
pub struct SteamSignIn {
    started: std::time::Instant,
    /// The value of the browser's [`STEAM_BROWSER_COOKIE`].
    browser: String,
    /// Where the browser goes back to: Settings, the setup wizard, or a
    /// player's own setup.
    back: &'static str,
    /// Whose Steam account it signs in: the callback comes back
    /// from Steam's site without the player cookie.
    player: PlayerId,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
struct SteamLoginQuery {
    /// `setup` to come back to the setup wizard instead of Settings, or
    /// `player` to a player's own setup.
    then: Option<String>,
}

/// Where a Steam sign-in started with `then` comes back to.
fn steam_back(then: Option<&str>) -> &'static str {
    match then {
        Some("setup") => "/setup",
        Some("player") => "/setup/player",
        _ => "/settings",
    }
}

const STEAM_STATE_TTL: std::time::Duration = std::time::Duration::from_secs(600);
/// The most Steam sign-ins waiting at once, as for GOG's; the oldest goes
/// first, so whoever may start one can't fill memory with them.
const STEAM_SIGN_INS_MAX: usize = 16;
/// Ties a sign-in to the browser that started it. The callback can't ask
/// for the caller's token: Steam sends the browser back from its own site,
/// so a paired browser's `SameSite=Strict` cookie stays behind and nothing
/// else names the caller. This cookie is `SameSite=Lax`, which a top-level
/// navigation back from Steam carries, and it is no credential: it only
/// says which browser a state was given to.
const STEAM_BROWSER_COOKIE: &str = "upnext_steam_signin";
/// Where the browser signs in, and what a genuine assertion names as its
/// `op_endpoint`. The check_authentication call goes through
/// `SteamClient::openid_is_valid`, whose host tests can replace.
pub(super) const STEAM_OP_ENDPOINT: &str = "https://steamcommunity.com/openid/login";

/// Redirect the browser to Steam's OpenID login.
#[utoipa::path(get, path = "/steam/login", tag = "accounts",
    description = "Sign in with Steam: redirects the browser to Steam's OpenID page, which sends it back to /api/steam/callback. \
        The sign-in is tied to this browser with a cookie (`upnext_steam_signin`), so it has to come back to the browser \
        that opened this address.",
    params(SteamLoginQuery),
    responses((status = 303, description = "To Steam's sign-in page", headers(("Location" = String), ("Set-Cookie" = String)))))]
async fn steam_login(
    State(state): State<AppState>,
    Me(me): Me,
    https: Option<axum::Extension<access::Https>>,
    headers: axum::http::HeaderMap,
    Query(q): Query<SteamLoginQuery>,
) -> impl IntoResponse {
    let https = https.is_some_and(|axum::Extension(access::Https(on))| on);
    let origin = origin_from(&headers, &state, https);
    let token = uuid::Uuid::new_v4().simple().to_string();
    let browser = uuid::Uuid::new_v4().simple().to_string();
    {
        let mut states = state.remote.steam_sign_ins.lock().unwrap_or_else(|e| e.into_inner());
        states.retain(|_, s| s.started.elapsed() < STEAM_STATE_TTL);
        while states.len() >= STEAM_SIGN_INS_MAX {
            let Some(oldest) = states.iter().min_by_key(|(_, s)| s.started).map(|(k, _)| k.clone()) else { break };
            states.remove(&oldest);
        }
        let started = std::time::Instant::now();
        states.insert(token.clone(), SteamSignIn { started, browser: browser.clone(), back: steam_back(q.then.as_deref()), player: me });
    }
    let secure = if https { "; Secure" } else { "" };
    let cookie = format!(
        "{STEAM_BROWSER_COOKIE}={browser}; Path=/api/steam/callback; HttpOnly; SameSite=Lax; Max-Age={}{secure}",
        STEAM_STATE_TTL.as_secs()
    );
    let return_to = format!("{origin}/api/steam/callback?state={token}");
    let url = format!(
        "{STEAM_OP_ENDPOINT}?openid.ns={}&openid.mode=checkid_setup&openid.return_to={}&openid.realm={}&openid.identity={}&openid.claimed_id={}",
        urlenc(OPENID_NS),
        urlenc(&return_to),
        urlenc(&origin),
        urlenc(OPENID_SELECT),
        urlenc(OPENID_SELECT)
    );
    ([(axum::http::header::SET_COOKIE, cookie)], axum::response::Redirect::to(&url))
}

/// Verify Steam's OpenID assertion with Steam, store the SteamID, bounce back to the UI.
#[utoipa::path(get, path = "/steam/callback", tag = "accounts",
    description = "Where Steam sends the browser back: the assertion is checked with Steam and the Steam ID saved. \
        Redirects to /settings?steam=ok, or /settings?steam=error&reason=…; to /setup instead when the sign-in \
        started with `then=setup`, and to /setup/player with `then=player`.",
    params(("state" = String, Query, description = "The sign-in's one-time token, from /api/steam/login"),
        ("openid.claimed_id" = Option<String>, Query, description = "Steam's OpenID fields (`openid.*`), as Steam sends them")),
    responses((status = 303, description = "Back to Settings, with the outcome in the query", headers(("Location" = String)))))]
async fn steam_callback(
    State(state): State<AppState>,
    https: Option<axum::Extension<access::Https>>,
    headers: axum::http::HeaderMap,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let https = https.is_some_and(|axum::Extension(access::Https(on))| on);
    let cleared = format!("{STEAM_BROWSER_COOKIE}=; Path=/api/steam/callback; HttpOnly; SameSite=Lax; Max-Age=0");
    ([(axum::http::header::SET_COOKIE, cleared)], steam_sign_in_back(&state, &headers, &q, https).await)
}

/// Check Steam's reply to a sign-in started here and save the SteamID; where
/// the browser goes next.
async fn steam_sign_in_back(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    q: &std::collections::HashMap<String, String>,
    https: bool,
) -> axum::response::Redirect {
    // Only a sign-in started here, only once, and only in the browser it started in.
    let token = q.get("state").cloned().unwrap_or_default();
    let issued = state.remote.steam_sign_ins.lock().unwrap_or_else(|e| e.into_inner()).remove(&token);
    let back = issued.as_ref().map_or("/settings", |s| s.back);
    let fail = |reason: &str| axum::response::Redirect::to(&format!("{back}?steam=error&reason={}", urlenc(reason)));
    let Some(issued) = issued.filter(|s| s.started.elapsed() < STEAM_STATE_TTL) else {
        return fail("that sign-in link expired or didn't start here; try again");
    };
    let browser = headers
        .get_all(axum::http::header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|pair| pair.trim().strip_prefix(STEAM_BROWSER_COOKIE)?.strip_prefix('='));
    if browser != Some(issued.browser.as_str()) {
        return fail("that sign-in started in another browser, or a newer one replaced it; start it again here");
    }
    // The assertion must be for this callback, from Steam, and sign the fields we use.
    let expected_return = format!("{}/api/steam/callback?state={token}", origin_from(headers, state, https));
    let field = |k: &str| q.get(k).map(String::as_str).unwrap_or_default();
    let signed: Vec<&str> = field("openid.signed").split(',').collect();
    if field("openid.return_to") != expected_return
        || field("openid.op_endpoint") != STEAM_OP_ENDPOINT
        || !["claimed_id", "identity", "return_to", "response_nonce"].iter().all(|f| signed.contains(f))
    {
        return fail("the sign-in reply wasn't for this app");
    }
    let claimed = q.get("openid.claimed_id").cloned().unwrap_or_default();
    let Some(id) = regex::Regex::new(r"/openid/id/(\d{17})$").ok().and_then(|re| re.captures(&claimed).map(|c| c[1].to_string())) else {
        return fail("Steam did not return a SteamID");
    };
    let mut form: Vec<(String, String)> = q.iter().filter(|(k, _)| k.starts_with("openid.")).map(|(k, v)| (k.clone(), v.clone())).collect();
    form.retain(|(k, _)| k != "openid.mode");
    form.push(("openid.mode".into(), "check_authentication".into()));
    let verified = match state.clients.steam.openid_is_valid(&form).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("openid verification request failed: {e}");
            false
        }
    };
    if !verified {
        return fail("Steam rejected the sign-in assertion");
    }
    // Signing in proves the browser can sign in to that Steam account, not
    // whose it is here: on a shared computer that may be another player's
    // login, and an account is whoever linked it first.
    if let Err(ApiError(_, why)) = accounts::steam_id_free(state, issued.player, &id) {
        return fail(&why);
    }
    if let Err(e) = state.db.setting_set(issued.player, "steam_id", Some(&id)) {
        return fail(&format!("could not save SteamID: {e}"));
    }
    axum::response::Redirect::to(&format!("{back}?steam=ok"))
}
