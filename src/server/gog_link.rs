//! GOG's sign-in from a browser that isn't on the server's computer:
//! UpNext in Docker, or another device. GOG only sends its code to
//! its own page, so the "Send to UpNext" bookmark, clicked on that page, brings
//! the code to UpNext's page `/gog/callback`, which posts it here with the
//! state this browser got when it started the sign-in.
//!
//! The state is what stops a forged link: it's made here, for the device that
//! asked (this computer, or one token), lasts ten minutes and works once.
//! The code goes in a POST's body, never a URL, and nothing logs it.
//!
//! The bookmark also carries this install's own key (security review L10).
//! The callback page finds the state in the browser's storage, so another
//! site could send the browser there with its own GOG code while a sign-in
//! waits and link the wrong GOG account; that site can't know the key, and a
//! code without it is refused.

use super::access::Caller;
use super::players::Me;
use super::{ApiError, ApiResult, AppState};
use crate::gog::{BROWSER_LOGIN_TTL, BrowserLoginError, GogStatus};
use axum::extract::State;
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};

pub fn routes() -> Router<AppState> {
    use axum::routing::{get, post};
    Router::new().route("/gog/browser", post(browser_start)).route("/gog/callback", post(callback)).route("/gog/bookmark", get(bookmark))
}

/// Who a sign-in belongs to: this computer, or the token (a paired browser's
/// included) that started it. None when the guard didn't say.
fn owner(caller: Option<Extension<Caller>>) -> Option<String> {
    match caller.map(|Extension(c)| c) {
        Some(Caller::Local) => Some("local".into()),
        Some(Caller::Token(t)) => Some(format!("token:{}", t.id)),
        Some(Caller::Proxy(client)) => Some(format!("proxy:{client}")),
        Some(Caller::Anyone) | None => None,
    }
}

fn no_owner() -> ApiError {
    ApiError::forbidden("a GOG sign-in needs this computer, a token or a paired browser")
}

/// A GOG sign-in started for the bookmark.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GogBrowserLogin {
    /// The one-time state to send back with the code (`POST /gog/callback`).
    pub state: String,
    /// GOG's sign-in page, which ends on its own success page with the code.
    pub auth_url: String,
    /// When the state stops working (Unix seconds).
    pub expires_at: i64,
}

#[utoipa::path(post, path = "/gog/browser", tag = "accounts",
    description = "Start a GOG sign-in in a browser that isn't on the computer UpNext runs on (Docker, another \
        device): a one-time state for this device, good for ten minutes, and GOG's sign-in address. After signing \
        in, the \"Send to UpNext\" bookmark on GOG's page brings the code to UpNext's /gog/callback page, which \
        sends it with the state to POST /gog/callback.",
    responses((status = 200, body = GogBrowserLogin)))]
async fn browser_start(State(state): State<AppState>, Me(me): Me, caller: Option<Extension<Caller>>) -> ApiResult<GogBrowserLogin> {
    let owner = owner(caller).ok_or_else(no_owner)?;
    let issued = state.gog_for(me).start_browser_login(&owner);
    Ok(Json(GogBrowserLogin { state: issued, auth_url: crate::gog::auth_url(), expires_at: crate::db::now() + BROWSER_LOGIN_TTL }))
}

/// What the UI puts in the "Send to UpNext" bookmark besides its address.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GogBookmark {
    /// This install's key, made once: the bookmark brings it back with the
    /// code, and a code without it is refused.
    pub key: String,
}

#[utoipa::path(get, path = "/gog/bookmark", tag = "accounts",
    description = "What the UI puts in the \"Send to UpNext\" bookmark besides this UpNext's address: this install's own \
        key, made the first time it's asked for. POST /gog/callback takes a code only with it, so another site can't \
        send this browser to /gog/callback with a code of its own while a sign-in waits.",
    responses((status = 200, body = GogBookmark)))]
async fn bookmark(State(state): State<AppState>) -> ApiResult<GogBookmark> {
    Ok(Json(GogBookmark { key: state.db.gog_bookmark_key()? }))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct GogCallbackBody {
    /// The state from POST /gog/browser.
    state: String,
    /// The code from GOG's success page, or that page's whole address.
    code: String,
    /// The key the bookmark brought, from GET /gog/bookmark.
    #[serde(default)]
    key: Option<String>,
}

/// Whether `key` is this install's bookmark key, compared in constant time.
fn our_key(state: &AppState, key: Option<&str>) -> anyhow::Result<bool> {
    let Some(key) = key.map(str::trim) else { return Ok(false) };
    let ours = state.db.gog_bookmark_key()?;
    Ok(ours.len() == key.len() && ours.bytes().zip(key.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0)
}

#[utoipa::path(post, path = "/gog/callback", tag = "accounts",
    description = "Finish a GOG sign-in started with POST /gog/browser: the code from GOG's success page with the \
        state and the bookmark's `key` (GET /gog/bookmark). The state must be one issued to this device in the last \
        ten minutes, and it works once, whatever the outcome. A code without this install's key is refused before \
        the state is looked at, so the sign-in still waits for the right one.",
    request_body = GogCallbackBody,
    responses((status = 200, body = GogStatus),
        (status = 400, description = "GOG turned the code down, or it had expired"),
        (status = 403, description = "No key or another install's (a bookmark from before the key: drag it again), or the \
            state wasn't issued to this device, was used already, or expired")))]
async fn callback(
    State(state): State<AppState>,
    Me(me): Me,
    caller: Option<Extension<Caller>>,
    Json(body): Json<GogCallbackBody>,
) -> ApiResult<GogStatus> {
    let owner = owner(caller).ok_or_else(no_owner)?;
    if !our_key(&state, body.key.as_deref())? {
        // Never the code: it would sign in whoever reads the log.
        tracing::warn!("a GOG code came without this install's \"Send to UpNext\" key, so it was refused");
        return Err(ApiError::forbidden(
            "this code didn't come from this UpNext's \"Send to UpNext\" bookmark. A bookmark saved before it had a key of \
             its own doesn't work any more: drag Send to UpNext to your bookmarks bar again, then use the new one",
        ));
    }
    match state.gog_for(me).finish_browser_login(&owner, &body.state, &body.code).await {
        Ok(s) => Ok(Json(s)),
        Err(e @ BrowserLoginError::NotOurs) => Err(ApiError::forbidden(e.to_string())),
        Err(e @ BrowserLoginError::Exchange(_)) => Err(ApiError::bad(e.to_string())),
    }
}
