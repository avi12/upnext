//! Signing in through an identity provider: a way to
//! pair a browser and find or make its player, beside the six-digit code
//! and the tap on the computer. Players stay in the local database; the
//! provider's account (`iss` and `sub`) only says which one.
//!
//! - `GET /oidc/login` sends the browser to the provider with a state, a
//!   nonce and a PKCE verifier kept here for ten minutes, tied to the
//!   browser by a cookie as Steam's sign-in is.
//! - `GET /oidc/callback` is where the provider sends it back: the code is
//!   traded for the ID token, which is checked (`crate::oidc`). A linked
//!   account is its player. While "Link the next sign-in" waits for a
//!   player, a new account waits too, to be confirmed on this computer as
//!   that player (`POST /players/{id}/oidc/confirm`, with their PIN), and
//!   signs in again once it is. Otherwise a new account makes a new player
//!   when auto-provisioning is on. The browser gets a device token bound to
//!   its player, of full scope, in the cookie pairing sets.
//! - `POST /oidc/logout` revokes that browser's token; it doesn't sign out
//!   of the provider.
//!
//! The redirect address is `public_url` plus `/api/v1/oidc/callback`, so
//! `public_url` must be set; the rule for other devices holds, HTTPS unless the browser
//! is on a private address (`access`). Everything here is off while
//! `oidc_issuer` or `oidc_client_id` is unset.

use super::access::{Caller, Edge, Https};
use super::players::{PlayerView, views};
use super::{ApiError, ApiResult, AppState, pairing};
use crate::db::{PlayerId, Scope, now};
use crate::oidc::{self as rp, Provider};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{delete, get, post};
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Where the provider sends the browser back, after `public_url`.
pub const CALLBACK: &str = "/api/v1/oidc/callback";
/// How long a sign-in, and an armed link, wait.
const TTL: Duration = Duration::from_secs(600);
/// The most sign-ins waiting at once. When that many wait, a new one is
/// refused rather than pushing out someone else's.
const MAX_WAITING: usize = 32;
/// The most sign-ins waiting from one address (an IPv6 /64): a new one
/// pushes out that address's oldest, never another device's.
const MAX_PER_NETWORK: usize = 3;
/// How long discovery's answer is used before asking again.
const DISCOVERY_FOR: Duration = Duration::from_secs(300);
/// Ties a sign-in to the browser that started it: `SameSite=Lax`, which the
/// navigation back from the provider carries. It is no credential.
const BROWSER_COOKIE: &str = "upnext_oidc_signin";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/oidc", get(status))
        .route("/oidc/login", get(login))
        .route("/oidc/callback", get(callback))
        .route("/oidc/logout", post(logout))
        .route("/players/{id}/oidc/link", post(arm).delete(disarm))
        .route("/players/{id}/oidc/confirm", post(confirm))
        .route("/players/{id}/oidc", delete(unlink))
}

/// What the server keeps in memory: sign-ins on their way, the provider
/// as discovery found it, and the player "Link the next sign-in" waits for.
#[derive(Default)]
pub struct SignIns {
    waiting: Mutex<HashMap<String, Waiting>>,
    provider: Mutex<Option<(Instant, String, Provider)>>,
    link: Mutex<Option<Armed>>,
}

/// "Link the next sign-in": the player, since when, and the new account
/// that signed in since, waiting to be confirmed as them on this computer.
/// It is never linked without that: whoever signs in first might not be
/// the person meant.
struct Armed {
    player: PlayerId,
    at: Instant,
    account: Option<Account>,
}

/// A new account that signed in while a link was armed.
#[derive(Clone)]
struct Account {
    issuer: String,
    id: rp::Identity,
}

/// The account waiting to be confirmed as a player, as Settings shows it.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct OidcWaiting {
    /// The provider's id for the account, which the confirmation names.
    pub subject: String,
    /// The account's `name`, `preferred_username` and `email` claims, as
    /// the provider gave them: to tell who it is, never matched.
    pub name: Option<String>,
    pub username: Option<String>,
    pub email: Option<String>,
}

struct Waiting {
    started: Instant,
    /// The address it started from, as the flood limits count it.
    from: IpAddr,
    /// The value of the browser's [`BROWSER_COOKIE`].
    browser: String,
    nonce: String,
    verifier: String,
}

impl SignIns {
    /// Until when "Link the next sign-in" waits for player `p`, when it does.
    pub fn link_until(&self, p: PlayerId) -> Option<i64> {
        let link = self.link.lock().unwrap_or_else(|e| e.into_inner());
        let a = link.as_ref().filter(|a| a.player == p)?;
        let left = TTL.checked_sub(a.at.elapsed())?;
        Some(now() + left.as_secs() as i64)
    }

    /// The account waiting to be confirmed as player `p`.
    pub fn waiting_for(&self, p: PlayerId) -> Option<OidcWaiting> {
        let link = self.link.lock().unwrap_or_else(|e| e.into_inner());
        let a = link.as_ref().filter(|a| a.player == p && a.at.elapsed() < TTL)?;
        let id = &a.account.as_ref()?.id;
        Some(OidcWaiting { subject: id.subject.clone(), name: id.name.clone(), username: id.username.clone(), email: id.email.clone() })
    }

    /// While a link is armed, a new account waits for it, in place of any
    /// that waited before: the armed player, when one is.
    fn hold(&self, issuer: &str, id: &rp::Identity) -> Option<PlayerId> {
        let mut link = self.link.lock().unwrap_or_else(|e| e.into_inner());
        let a = link.as_mut().filter(|a| a.at.elapsed() < TTL)?;
        a.account = Some(Account { issuer: issuer.to_string(), id: id.clone() });
        Some(a.player)
    }

    /// Player `p`'s waiting account, taken when it is `subject`: it is
    /// confirmed once.
    fn take_account(&self, p: PlayerId, subject: &str) -> Option<Account> {
        let mut link = self.link.lock().unwrap_or_else(|e| e.into_inner());
        let fits = link
            .as_ref()
            .is_some_and(|a| a.player == p && a.at.elapsed() < TTL && a.account.as_ref().is_some_and(|w| w.id.subject == subject));
        if !fits {
            return None;
        }
        link.take()?.account
    }

    /// End "Link the next sign-in" for player `p`, with any account waiting.
    fn disarm(&self, p: PlayerId) {
        let mut link = self.link.lock().unwrap_or_else(|e| e.into_inner());
        if link.as_ref().is_some_and(|a| a.player == p) {
            *link = None;
        }
    }

    /// Ten minutes gone by, for the tests.
    #[cfg(test)]
    pub fn expire(&self) {
        for w in self.waiting.lock().unwrap_or_else(|e| e.into_inner()).values_mut() {
            w.started -= TTL;
        }
        if let Some(a) = self.link.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            a.at -= TTL;
        }
    }
}

/// The household's sign-in settings, complete.
struct Settings {
    issuer: String,
    client_id: String,
    secret: Option<String>,
    name: String,
    auto_provision: bool,
    groups: Vec<String>,
    redirect_uri: String,
}

/// The provider's name for the button: `oidc_name`, else the issuer's host.
fn provider_name(cfg: &crate::Config) -> Option<String> {
    let issuer = cfg.oidc_issuer.as_deref()?;
    cfg.oidc_name.clone().or_else(|| reqwest::Url::parse(issuer).ok()?.host_str().map(str::to_string))
}

/// The sign-in's settings, or what's missing.
fn settings(state: &AppState) -> Result<Settings, String> {
    let cfg = state.cfg();
    let (Some(issuer), Some(client_id)) = (cfg.oidc_issuer.clone(), cfg.oidc_client_id.clone()) else {
        return Err("signing in with an identity provider isn't set up here".into());
    };
    let Some(public) = super::access::public_url(state) else {
        return Err("signing in with an identity provider needs the address other devices use (public_url) set".into());
    };
    let groups = cfg.oidc_allowed_groups.as_deref().unwrap_or_default().split(',').map(str::trim).filter(|g| !g.is_empty());
    Ok(Settings {
        name: provider_name(&cfg).unwrap_or_else(|| issuer.clone()),
        issuer,
        client_id,
        secret: cfg.oidc_client_secret.clone(),
        auto_provision: cfg.oidc_auto_provision,
        groups: groups.map(str::to_string).collect(),
        redirect_uri: format!("{}{CALLBACK}", public.origin()),
    })
}

/// The provider, from discovery or what it said in the last five minutes.
async fn provider(state: &AppState, s: &Settings) -> anyhow::Result<Provider> {
    {
        let cached = state.remote.oidc.provider.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, issuer, p)) = cached.as_ref()
            && *issuer == s.issuer
            && at.elapsed() < DISCOVERY_FOR
        {
            return Ok(p.clone());
        }
    }
    let p = rp::discover(&state.clients.http, &s.issuer).await?;
    *state.remote.oidc.provider.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), s.issuer.clone(), p.clone()));
    Ok(p)
}

/// Back to the UI's first page with what went wrong, in the fragment, which
/// never reaches a server or a proxy's log.
fn failed(why: &str) -> Redirect {
    Redirect::to(&format!("/#signin_error={}", crate::oidc::form_encode(why)))
}

/// What became of a sign-in that went through.
enum SignedIn {
    /// The browser's new token, for its player.
    Token(String, PlayerId),
    /// A new account waits to be confirmed as the player a link was armed for.
    Waiting(PlayerId),
}

/// Whether the sign-in is offered here, and what to call the provider.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct OidcStatus {
    /// An issuer, a client id and `public_url` are all set.
    pub enabled: bool,
    /// What the "Sign in with" button calls the provider.
    pub name: Option<String>,
}

#[utoipa::path(get, path = "/oidc", tag = "devices", operation_id = "oidc_status", security(()),
    description = "Whether signing in with an identity provider is offered here, and the provider's name for the \
        \"Sign in with\" button. Needs no token.",
    responses((status = 200, body = OidcStatus)))]
async fn status(State(state): State<AppState>) -> Json<OidcStatus> {
    let enabled = settings(&state).is_ok();
    Json(OidcStatus { enabled, name: enabled.then(|| provider_name(&state.cfg())).flatten() })
}

#[utoipa::path(get, path = "/oidc/login", tag = "devices", operation_id = "oidc_login", security(()),
    description = "Sign this browser in with the identity provider: redirects to the provider's sign-in page, which \
        sends it back to /api/v1/oidc/callback. The sign-in is tied to this browser with a cookie \
        (`upnext_oidc_signin`). Three sign-ins wait at most from one address (an IPv6 /64; a fourth pushes out its \
        oldest) and 32 in all (then a new one is refused). Needs no token; not over plain HTTP from a public address. \
        When the sign-in isn't set up, too many wait, or the provider can't be reached, redirects to \
        `/#signin_error=…`.",
    responses((status = 303, description = "To the provider's sign-in page, or back with the reason", headers(("Location" = String), ("Set-Cookie" = String)))))]
async fn login(State(state): State<AppState>, https: Option<Extension<Https>>, edge: Option<Extension<Edge>>) -> Response {
    let https = https.is_some_and(|Extension(Https(on))| on);
    // The client behind a listed proxy, else the peer, by its /64.
    let from = edge.and_then(|Extension(e)| e.client());
    let from = super::approval::network(from.unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST)));
    let s = match settings(&state) {
        Ok(s) => s,
        Err(why) => return failed(&why).into_response(),
    };
    let p = match provider(&state, &s).await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("signing in with {}: {e:#}", s.issuer);
            return failed(&format!("couldn't reach {}: {e:#}", s.name)).into_response();
        }
    };
    let (Ok(sign_in), Ok(nonce), Ok(verifier), Ok(browser)) = (rp::random(), rp::random(), rp::random(), rp::random()) else {
        return failed("no randomness for a sign-in").into_response();
    };
    {
        let mut waiting = state.remote.oidc.waiting.lock().unwrap_or_else(|e| e.into_inner());
        waiting.retain(|_, w| w.started.elapsed() < TTL);
        // An address's own oldest goes when it starts too many.
        while waiting.values().filter(|w| w.from == from).count() >= MAX_PER_NETWORK {
            let Some(oldest) = waiting.iter().filter(|(_, w)| w.from == from).min_by_key(|(_, w)| w.started).map(|(k, _)| k.clone()) else {
                break;
            };
            waiting.remove(&oldest);
        }
        if waiting.len() >= MAX_WAITING {
            return failed("too many sign-ins are under way here; try again in a few minutes").into_response();
        }
        let w = Waiting { started: Instant::now(), from, browser: browser.clone(), nonce: nonce.clone(), verifier: verifier.clone() };
        waiting.insert(sign_in.clone(), w);
    }
    let r = rp::Request {
        client_id: &s.client_id,
        redirect_uri: &s.redirect_uri,
        state: &sign_in,
        nonce: &nonce,
        verifier: &verifier,
        groups: !s.groups.is_empty(),
    };
    let secure = if https { "; Secure" } else { "" };
    let cookie = format!("{BROWSER_COOKIE}={browser}; Path={CALLBACK}; HttpOnly; SameSite=Lax; Max-Age={}{secure}", TTL.as_secs());
    ([(header::SET_COOKIE, cookie)], Redirect::to(&rp::authorize_url(&p, &r))).into_response()
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
struct CallbackQuery {
    /// The sign-in's one-time state, from /oidc/login.
    state: Option<String>,
    /// The provider's code, traded for the ID token.
    code: Option<String>,
    /// The provider's error, when it didn't sign the browser in.
    error: Option<String>,
    error_description: Option<String>,
}

#[utoipa::path(get, path = "/oidc/callback", tag = "devices", operation_id = "oidc_callback", security(()),
    description = "Where the identity provider sends the browser back. The code is traded for the ID token, which is \
        checked (signature, issuer, audience, expiry, nonce). The account's player is the one linked to it, else the \
        player armed with \"Link the next sign-in\", else a new one when `oidc_auto_provision` is on; with \
        `oidc_allowed_groups` set, the token's `groups` must hold one of them. The browser gets a device token bound to \
        that player (kind `oidc`, full scope) in the cookie pairing sets, and goes to `/`; on any failure, to \
        `/#signin_error=…`.",
    params(CallbackQuery),
    responses((status = 303, description = "Into UpNext, or back with the reason", headers(("Location" = String), ("Set-Cookie" = String)))))]
async fn callback(
    State(state): State<AppState>,
    https: Option<Extension<Https>>,
    headers: HeaderMap,
    Query(q): Query<CallbackQuery>,
) -> Response {
    let https = https.is_some_and(|Extension(Https(on))| on);
    let cleared = format!("{BROWSER_COOKIE}=; Path={CALLBACK}; HttpOnly; SameSite=Lax; Max-Age=0");
    let mut res = match signed_in(&state, &headers, &q).await {
        Ok(SignedIn::Token(token, player)) => {
            tracing::info!("player {player} signed in a browser with the identity provider");
            let mut res = Redirect::to("/").into_response();
            res.headers_mut().append(header::SET_COOKIE, pairing::cookie(&state, &token, https));
            res
        }
        Ok(SignedIn::Waiting(player)) => {
            tracing::info!("a new account signed in with the identity provider and waits to be confirmed as player {player}");
            let say = "this account waits to be confirmed: on the computer UpNext runs on, Settings › Players, confirm it's \
                the player Link the next sign-in is waiting for, then sign in again here";
            Redirect::to(&format!("/#signin_waiting={}", crate::oidc::form_encode(say))).into_response()
        }
        Err(why) => failed(&why).into_response(),
    };
    if let Ok(v) = HeaderValue::from_str(&cleared) {
        res.headers_mut().append(header::SET_COOKIE, v);
    }
    res
}

/// Check the provider's answer and make the browser's token, or hold a new
/// account for a link to confirm; or why not.
async fn signed_in(state: &AppState, headers: &HeaderMap, q: &CallbackQuery) -> Result<SignedIn, String> {
    // Only a sign-in started here, only once, within ten minutes, and only
    // in the browser it started in.
    let id = q.state.clone().unwrap_or_default();
    let waiting = state.remote.oidc.waiting.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
    let Some(w) = waiting.filter(|w| w.started.elapsed() < TTL) else {
        return Err("that sign-in expired or didn't start here; try again".into());
    };
    let browser = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|pair| pair.trim().strip_prefix(BROWSER_COOKIE)?.strip_prefix('='));
    if browser != Some(w.browser.as_str()) {
        return Err("that sign-in started in another browser, or a newer one replaced it; start it again here".into());
    }
    let s = settings(state)?;
    if let Some(e) = &q.error {
        let why = q.error_description.as_deref().unwrap_or(e);
        return Err(format!("{} didn't sign you in: {}", s.name, why.chars().take(200).collect::<String>()));
    }
    let code = q.code.as_deref().filter(|c| !c.is_empty()).ok_or("the provider sent no code back")?;
    let check = async {
        let p = provider(state, &s).await?;
        let token = rp::exchange(&state.clients.http, &p, &s.client_id, s.secret.as_deref(), code, &s.redirect_uri, &w.verifier).await?;
        let keys = rp::keys(&state.clients.http, &p).await?;
        let id = rp::verify(&token, &keys, &p, &s.client_id, &w.nonce, now())?;
        anyhow::Ok((p.issuer, id))
    };
    let (issuer, id) = check.await.map_err(|e| {
        tracing::warn!("a sign-in with {}: {e:#}", s.issuer);
        format!("{e:#}")
    })?;
    if !s.groups.is_empty() && !id.groups.iter().any(|g| s.groups.contains(g)) {
        return Err(format!("your {} account isn't in a group allowed to use UpNext here", s.name));
    }
    let internal = |e: anyhow::Error| {
        tracing::error!("a sign-in: {e:#}");
        "couldn't finish signing in".to_string()
    };
    let player = match state.db.oidc_player(&issuer, &id.subject).map_err(internal)? {
        Some(p) => p,
        None => match state.remote.oidc.hold(&issuer, &id) {
            Some(p) => return Ok(SignedIn::Waiting(p)),
            None if s.auto_provision => state.db.provision_oidc_player(id.name.as_deref(), &issuer, &id.subject).map_err(internal)?.0,
            None => {
                return Err(format!(
                    "your {} account isn't linked to a player here yet: on the computer UpNext runs on, Settings › Players, \
                     choose Link the next sign-in for your player, then sign in again",
                    s.name
                ));
            }
        },
    };
    let name = pairing::free_name(state, &format!("{} ({})", pairing::device_name(headers), s.name)).map_err(internal)?;
    let made = state.db.token_create_for(&name, Scope::Full, "oidc", Some(player)).map_err(internal)?;
    Ok(SignedIn::Token(made.token, player))
}

#[utoipa::path(post, path = "/oidc/logout", tag = "devices", operation_id = "oidc_logout",
    description = "Sign this browser out: revokes the device token its identity provider sign-in made and clears its \
        cookie. It doesn't sign out of the provider.",
    responses((status = 204, description = "Signed out", headers(("Set-Cookie" = String))),
        (status = 400, description = "This browser didn't sign in with the identity provider")))]
async fn logout(State(state): State<AppState>, caller: Option<Extension<Caller>>) -> Result<Response, ApiError> {
    let Some(Caller::Token(t)) = caller.as_deref().filter(|c| matches!(c, Caller::Token(t) if t.kind == "oidc")) else {
        return Err(ApiError::bad("this browser didn't sign in with the identity provider"));
    };
    state.db.token_revoke(t.id)?;
    state.remote.limits.forget(t.id);
    // Both names: a browser signed in before the cookie took the port has the old one.
    let mut res = StatusCode::NO_CONTENT.into_response();
    res.headers_mut().append(header::SET_COOKIE, pairing::cookie_cleared(&state));
    res.headers_mut().append(header::SET_COOKIE, pairing::old_cookie_cleared());
    Ok(res)
}

/// Linking is done on the computer UpNext runs on, like removing a player:
/// it decides which account at the provider becomes someone.
fn this_computer(caller: Option<Extension<Caller>>) -> Result<(), ApiError> {
    if caller.is_some_and(|c| c.is_local()) {
        Ok(())
    } else {
        Err(ApiError::forbidden("sign-ins are linked to players only on the computer UpNext runs on"))
    }
}

/// The player as this computer sees them: every route that answers with
/// one is this computer's alone.
fn view_of(state: &AppState, id: i64) -> ApiResult<PlayerView> {
    let found = views(state, Some(&Caller::Local))?.into_iter().find(|v| v.id == id);
    Ok(Json(found.ok_or_else(|| ApiError::not_found(format!("no player {id}")))?))
}

#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[schema(as = OidcLinkBody)]
struct LinkBody {
    /// The player's PIN, when they have one.
    #[serde(default)]
    pin: Option<String>,
}

#[utoipa::path(post, path = "/players/{id}/oidc/link", tag = "players", operation_id = "oidc_link",
    description = "\"Link the next sign-in\": for ten minutes, an account at the identity provider that no player is \
        linked to, signing in, waits to be confirmed as this player (`POST /players/{id}/oidc/confirm`) instead of \
        making a new one; a newer one replaces it. One player at a time. Only on the computer UpNext runs on, with the \
        player's PIN when they have one.",
    params(("id" = i64, Path, description = "Player id")),
    request_body(content = Option<LinkBody>, description = "`{pin}` when the player has one"),
    responses((status = 200, body = PlayerView, description = "The player, with `oidc_link_until`"),
        (status = 400, description = "Signing in with an identity provider isn't set up"),
        (status = 403, description = "From another device, or a wrong PIN"), (status = 404, description = "No such player"),
        (status = 429, description = "Too many wrong PINs for that player: try again later")))]
async fn arm(
    State(state): State<AppState>,
    caller: Option<Extension<Caller>>,
    Path(id): Path<i64>,
    body: Option<Json<LinkBody>>,
) -> ApiResult<PlayerView> {
    this_computer(caller)?;
    let p = PlayerId(id);
    state.db.player(p)?.ok_or_else(|| ApiError::not_found(format!("no player {id}")))?;
    if state.cfg().oidc_issuer.is_none() {
        return Err(ApiError::bad("set up signing in with an identity provider first"));
    }
    super::players::binding_allowed(&state, Some(p), body.as_ref().and_then(|b| b.pin.as_deref()))?;
    *state.remote.oidc.link.lock().unwrap_or_else(|e| e.into_inner()) = Some(Armed { player: p, at: Instant::now(), account: None });
    view_of(&state, id)
}

#[utoipa::path(delete, path = "/players/{id}/oidc/link", tag = "players", operation_id = "oidc_disarm",
    description = "End \"Link the next sign-in\" for this player, with any account waiting to be confirmed. Only on the \
        computer UpNext runs on.",
    params(("id" = i64, Path, description = "Player id")),
    responses((status = 200, body = PlayerView), (status = 403, description = "From another device"),
        (status = 404, description = "No such player")))]
async fn disarm(State(state): State<AppState>, caller: Option<Extension<Caller>>, Path(id): Path<i64>) -> ApiResult<PlayerView> {
    this_computer(caller)?;
    state.remote.oidc.disarm(PlayerId(id));
    view_of(&state, id)
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[schema(as = OidcConfirmBody)]
struct ConfirmBody {
    /// The waiting account's subject, as `oidc_waiting` showed it: the one
    /// being confirmed, so a newer sign-in can't take its place.
    subject: String,
    /// The player's PIN, when they have one.
    #[serde(default)]
    pin: Option<String>,
}

#[utoipa::path(post, path = "/players/{id}/oidc/confirm", tag = "players", operation_id = "oidc_confirm",
    description = "Confirm that the account waiting for \"Link the next sign-in\" (the player's `oidc_waiting`) is this \
        player: it is linked, the link ends, and the account signs in as them from then on. An account the player had \
        before is unlinked, and the browsers it signed in as them are signed out. Only on the computer UpNext \
        runs on, with the player's PIN again when they have one.",
    params(("id" = i64, Path, description = "Player id")),
    request_body(content = ConfirmBody, description = "`{subject, pin}`"),
    responses((status = 200, body = PlayerView, description = "The player, now with `oidc`"),
        (status = 403, description = "From another device, or a wrong PIN"), (status = 404, description = "No such player"),
        (status = 409, description = "No account waits for this player, or another replaced it"),
        (status = 429, description = "Too many wrong PINs for that player: try again later")))]
async fn confirm(
    State(state): State<AppState>,
    caller: Option<Extension<Caller>>,
    Path(id): Path<i64>,
    Json(body): Json<ConfirmBody>,
) -> ApiResult<PlayerView> {
    this_computer(caller)?;
    let p = PlayerId(id);
    let had = state.db.player(p)?.ok_or_else(|| ApiError::not_found(format!("no player {id}")))?.oidc_issuer.is_some();
    super::players::binding_allowed(&state, Some(p), body.pin.as_deref())?;
    let Some(account) = state.remote.oidc.take_account(p, &body.subject) else {
        return Err(ApiError::conflict(
            "that account isn't waiting for this player any more: the link ran out or another sign-in replaced it",
        ));
    };
    state.db.link_oidc(p, &account.issuer, &account.id.subject).map_err(|e| ApiError::conflict(format!("{e:#}")))?;
    // The account it replaces signs in as them no more, and neither do the
    // browsers it signed in: a waiting account was linked to nobody, so
    // every browser signed in as them before was the old account's.
    if had {
        sign_out_browsers(&state, p)?;
    }
    view_of(&state, id)
}

/// Revoke the device tokens the identity provider's sign-in made for `p`.
fn sign_out_browsers(state: &AppState, p: PlayerId) -> anyhow::Result<()> {
    for t in state.db.tokens()?.into_iter().filter(|t| t.kind == "oidc" && t.player_id == Some(p)) {
        state.db.token_revoke(t.id)?;
        state.remote.limits.forget(t.id);
    }
    Ok(())
}

#[utoipa::path(delete, path = "/players/{id}/oidc", tag = "players", operation_id = "oidc_unlink",
    description = "Unlink the player's account at the identity provider, end a \"Link the next sign-in\" waiting for \
        them, and sign out the browsers that account signed in as them. Only on the computer UpNext runs on.",
    params(("id" = i64, Path, description = "Player id")),
    responses((status = 200, body = PlayerView), (status = 403, description = "From another device"),
        (status = 404, description = "No such player")))]
async fn unlink(State(state): State<AppState>, caller: Option<Extension<Caller>>, Path(id): Path<i64>) -> ApiResult<PlayerView> {
    this_computer(caller)?;
    let p = PlayerId(id);
    state.db.player(p)?.ok_or_else(|| ApiError::not_found(format!("no player {id}")))?;
    state.remote.oidc.disarm(p);
    state.db.unlink_oidc(p)?;
    sign_out_browsers(&state, p)?;
    view_of(&state, id)
}
