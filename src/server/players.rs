//! Who's playing: each request's player, PINs, the browser's
//! signed choice, and the players, "Hide this game" and Hidden by me
//! endpoints.
//!
//! The guard (`access::guard`) works out each API request's player after
//! its caller, in this order:
//! 1. A token bound to a player (a paired phone, a script made for one
//!    person) is that player, always; a header naming anyone else is a 403.
//! 2. The `UpNext-Player` header names a player by id: unknown is a 400, a
//!    player with a PIN a 403 (a script acts for them with a bound token).
//! 3. The browser's cookie, `upnext_player_<port>`, set by `PUT
//!    /players/current`: the player's id and a nonce, signed with a random
//!    key kept in `meta`, so a player with a PIN can't be chosen by writing
//!    the cookie by hand. A bad or stale one is cleared and falls through.
//! 4. The default player (`default_player`, else the lowest id). When that
//!    player has a PIN, per-player routes answer 403 "choose who's playing"
//!    until someone is chosen; household routes still answer.
//!
//! A PIN keeps the household from switching to each other here. It isn't
//! security: anyone with the database file can read everything, and a
//! short PIN can be guessed from its hash offline.

use super::access::{Caller, Https};
use super::{ApiError, ApiResult, AppState};
use crate::db::{HiddenTitle, HideScope, PlayerId};
use axum::extract::{FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The header a script names its player with.
pub const PLAYER_HEADER: &str = "upnext-player";
/// What a per-player route answers while nobody has been chosen.
pub const CHOOSE: &str = "choose who's playing: the default player has a PIN";
/// The `meta` key of the cookie's signing key.
pub const COOKIE_KEY: &str = crate::db::PLAYER_COOKIE_KEY;
/// About 400 days, what browsers allow at most.
const COOKIE_MAX_AGE: i64 = 400 * 24 * 3600;
/// Wrong PINs before a player's PIN is locked.
const MAX_WRONG: u32 = 5;
/// The first lock; each one after doubles it.
const FIRST_LOCK: Duration = Duration::from_secs(60);
/// How long the Hidden by me list stays open after the PIN.
const UNLOCK_FOR: Duration = Duration::from_secs(600);

/// A request's player, in its extensions beside the `Caller`.
#[derive(Debug, Clone)]
pub struct Who {
    /// None while the default player has a PIN and nobody has been chosen.
    pub player: Option<PlayerId>,
    /// The token it came with is bound to the player.
    pub bound: bool,
    /// What the Hidden by me unlock is tied to: the browser's choice or
    /// the token.
    pub holder: Option<String>,
}

/// The request's player, for a per-player route: refused with 403 while
/// nobody has been chosen. Handlers take it right after their state, so
/// that refusal comes before anything else is read.
pub struct Me(pub PlayerId);

impl<S: Send + Sync> FromRequestParts<S> for Me {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, ApiError> {
        match parts.extensions.get::<Who>() {
            Some(Who { player: Some(p), .. }) => Ok(Me(*p)),
            Some(_) => Err(ApiError::forbidden(CHOOSE)),
            // Only a request the guard didn't see, which a router always runs.
            None => Ok(Me(PlayerId::FIRST)),
        }
    }
}

/// Each other player's sign-in clients, by player (`AppState::gog_for`).
#[derive(Default)]
pub struct SignIns {
    pub gog: std::sync::Mutex<HashMap<PlayerId, std::sync::Arc<crate::gog::GogClient>>>,
    pub epic: std::sync::Mutex<HashMap<PlayerId, std::sync::Arc<crate::epic::EpicClient>>>,
}

/// What the server keeps in memory about PINs: each player's wrong tries
/// and lock, and the browsers that opened a Hidden by me list.
#[derive(Default)]
pub struct Pins {
    locks: Mutex<HashMap<PlayerId, Lock>>,
    unlocked: Mutex<HashMap<(PlayerId, String), Instant>>,
}

#[derive(Default)]
struct Lock {
    wrong: u32,
    until: Option<Instant>,
    next: Option<Duration>,
}

impl Pins {
    /// Check `pin` for player `p`, counting a wrong one: five lock the
    /// player's PIN for a minute, then two, four and so on.
    ///
    /// The try is counted before the PIN is checked and the lock is let go
    /// while Argon2 runs, so other players' PINs don't wait on this one and
    /// tries sent at once can't get past five between them.
    pub fn check(&self, state: &AppState, p: PlayerId, pin: Option<&str>) -> Result<(), ApiError> {
        let locked_now = {
            let mut locks = self.locks.lock().unwrap_or_else(|e| e.into_inner());
            let lock = locks.entry(p).or_default();
            if let Some(until) = lock.until {
                let left = until.saturating_duration_since(Instant::now());
                if !left.is_zero() {
                    return Err(ApiError(
                        StatusCode::TOO_MANY_REQUESTS,
                        format!("too many wrong PINs: try again in {}", crate::enrich::plural(left.as_secs().max(1), "second", "seconds")),
                    ));
                }
                lock.until = None;
            }
            if pin.map(str::trim).is_none_or(str::is_empty) {
                return Err(ApiError::forbidden("this player has a PIN: enter it"));
            }
            lock.wrong += 1;
            (lock.wrong >= MAX_WRONG).then(|| {
                let wait = lock.next.unwrap_or(FIRST_LOCK);
                lock.until = Some(Instant::now() + wait);
                lock.next = Some(wait * 2);
                lock.wrong = 0;
                wait
            })
        };
        let pin = pin.map(str::trim).unwrap_or_default();
        if state.db.pin_matches(p, pin)? {
            self.locks.lock().unwrap_or_else(|e| e.into_inner()).remove(&p);
            return Ok(());
        }
        match locked_now {
            Some(wait) => Err(ApiError(StatusCode::TOO_MANY_REQUESTS, format!("wrong PIN: locked for {} seconds", wait.as_secs()))),
            None => Err(ApiError::forbidden("wrong PIN")),
        }
    }

    fn unlock(&self, p: PlayerId, holder: &str) {
        let mut u = self.unlocked.lock().unwrap_or_else(|e| e.into_inner());
        u.retain(|_, at| at.elapsed() < UNLOCK_FOR);
        u.insert((p, holder.to_string()), Instant::now());
    }

    fn is_unlocked(&self, p: PlayerId, holder: Option<&str>) -> bool {
        let Some(holder) = holder else { return false };
        let u = self.unlocked.lock().unwrap_or_else(|e| e.into_inner());
        u.get(&(p, holder.to_string())).is_some_and(|at| at.elapsed() < UNLOCK_FOR)
    }

    /// Close Hidden by me again for this browser or token, before its ten
    /// minutes are up.
    fn lock(&self, p: PlayerId, holder: &str) {
        self.unlocked.lock().unwrap_or_else(|e| e.into_inner()).remove(&(p, holder.to_string()));
    }

    /// Forget a removed player.
    fn forget(&self, p: PlayerId) {
        self.locks.lock().unwrap_or_else(|e| e.into_inner()).remove(&p);
        self.unlocked.lock().unwrap_or_else(|e| e.into_inner()).retain(|(q, _), _| *q != p);
    }
}

/// Whether the player's own hidden games are open to this request: they
/// have no PIN, or entered it in this browser (or with this token) in the
/// last ten minutes.
pub fn hidden_open(state: &AppState, p: PlayerId, who: Option<&Who>) -> Result<bool, ApiError> {
    let has_pin = state.db.player(p)?.is_some_and(|x| x.has_pin);
    Ok(!has_pin || state.remote.pins.is_unlocked(p, who.and_then(|w| w.holder.as_deref())))
}

// ---------------------------------------------------------------------------
// The browser's choice
// ---------------------------------------------------------------------------

/// The cookie's name: with the port, since browsers share cookies across
/// ports on one host and two UpNext servers would read each other's. It's
/// the port UpNext listens on, which in a container is 8787 whatever host
/// port it's published on, so a container and another UpNext on 8787 on
/// one host still share the name.
pub fn cookie_name(state: &AppState) -> String {
    format!("upnext_player_{}", state.base_cfg.port)
}

/// HMAC-SHA256 (RFC 2104).
fn hmac(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let inner = Sha256::new().chain_update(k.map(|b| b ^ 0x36)).chain_update(msg).finalize();
    Sha256::new().chain_update(k.map(|b| b ^ 0x5c)).chain_update(inner).finalize().into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The cookie's signing key, made the first time it's needed.
fn cookie_key(state: &AppState) -> anyhow::Result<String> {
    state.db.player_cookie_key()
}

/// `id.nonce.signature`
fn signed(state: &AppState, p: PlayerId) -> anyhow::Result<String> {
    let mut n = [0u8; 12];
    getrandom::fill(&mut n).map_err(|e| anyhow::anyhow!("no randomness for the player cookie: {e}"))?;
    let body = format!("{}.{}", p.0, hex(&n));
    let sig = hmac(cookie_key(state)?.as_bytes(), body.as_bytes());
    Ok(format!("{body}.{}", hex(&sig)))
}

/// The player and nonce a cookie value names, when its signature is ours.
fn verify(state: &AppState, value: &str) -> Option<(PlayerId, String)> {
    let (body, sig) = value.rsplit_once('.')?;
    let (id, nonce) = body.split_once('.')?;
    let key = state.db.meta_get(COOKIE_KEY).ok()??;
    let want = hex(&hmac(key.as_bytes(), body.as_bytes()));
    let same = want.len() == sig.len() && want.bytes().zip(sig.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0;
    same.then(|| Some((PlayerId(id.parse().ok()?), nonce.to_string())))?
}

fn set_cookie(state: &AppState, value: &str, https: bool) -> HeaderValue {
    let secure = if https { "; Secure" } else { "" };
    let v = format!("{}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={COOKIE_MAX_AGE}{secure}", cookie_name(state));
    HeaderValue::from_str(&v).unwrap_or_else(|_| HeaderValue::from_static(""))
}

/// Ends a player cookie that no longer names anyone.
pub fn cookie_cleared(state: &AppState) -> HeaderValue {
    HeaderValue::from_str(&format!("{}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0", cookie_name(state)))
        .unwrap_or_else(|_| HeaderValue::from_static(""))
}

fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get_all(header::COOKIE).iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(';')).find_map(|pair| {
        let (k, v) = pair.trim().split_once('=')?;
        (k == name && !v.is_empty()).then_some(v)
    })
}

/// A request's player, and whether its player cookie should be cleared.
pub struct Resolved {
    pub who: Who,
    pub clear_cookie: bool,
}

/// Work out a request's player (the module's comment says how).
pub fn resolve(state: &AppState, headers: &HeaderMap, caller: &Caller) -> Result<Resolved, (StatusCode, String)> {
    let internal = |e: anyhow::Error| {
        tracing::error!("working out the player: {e:#}");
        (StatusCode::INTERNAL_SERVER_ERROR, "couldn't work out who's playing".to_string())
    };
    let named: Option<&str> = headers.get(PLAYER_HEADER).map(|v| v.to_str().unwrap_or("?").trim());
    let named = match named {
        None | Some("") => None,
        Some(v) => Some(PlayerId(
            v.parse().map_err(|_| (StatusCode::BAD_REQUEST, format!("the UpNext-Player header names a player by id, not {v:?}")))?,
        )),
    };
    if let Caller::Token(t) = caller
        && let Some(p) = t.player_id
    {
        if named.is_some_and(|n| n != p) {
            return Err((StatusCode::FORBIDDEN, "this token is bound to another player, so it can't act for anyone else".into()));
        }
        let who = Who { player: Some(p), bound: true, holder: Some(format!("token:{}", t.id)) };
        return Ok(Resolved { who, clear_cookie: false });
    }
    let holder = match caller {
        Caller::Token(t) => Some(format!("token:{}", t.id)),
        _ => None,
    };
    if let Some(p) = named {
        let player = state.db.player(p).map_err(internal)?.ok_or((StatusCode::BAD_REQUEST, format!("no player {p}")))?;
        if player.has_pin {
            return Err((
                StatusCode::FORBIDDEN,
                format!("{} has a PIN, so a script acts for them with a token bound to them, not the UpNext-Player header", player.name),
            ));
        }
        return Ok(Resolved { who: Who { player: Some(p), bound: false, holder }, clear_cookie: false });
    }
    let mut clear_cookie = false;
    if let Some(value) = cookie_value(headers, &cookie_name(state)) {
        match verify(state, value) {
            Some((p, nonce)) if state.db.player(p).map_err(internal)?.is_some() => {
                return Ok(Resolved { who: Who { player: Some(p), bound: false, holder: Some(nonce) }, clear_cookie: false });
            }
            _ => clear_cookie = true,
        }
    }
    let p = state.db.default_player().map_err(internal)?;
    let has_pin = state.db.player(p).map_err(internal)?.is_some_and(|x| x.has_pin);
    Ok(Resolved { who: Who { player: (!has_pin).then_some(p), bound: false, holder }, clear_cookie })
}

/// Binding a token or a device to a player who has a PIN asks for it, as
/// switching to them does: the device then acts as them with no PIN at
/// all. A wrong one counts against their lockout and a missing one doesn't,
/// as in `Pins::check`. Shared is always fine.
pub fn binding_allowed(state: &AppState, p: Option<PlayerId>, pin: Option<&str>) -> Result<(), ApiError> {
    let Some(p) = p else { return Ok(()) };
    if state.db.player(p)?.is_some_and(|x| x.has_pin) {
        state.remote.pins.check(state, p, pin)?;
    }
    Ok(())
}

/// Whose device a pairing code makes: `asked` is a player's id or
/// `shared`; without it, the player making the code when there are two or
/// more, and shared with one, so a one-player install pairs as it did.
pub fn pairing_player(state: &AppState, making: Option<PlayerId>, asked: Option<&str>) -> Result<Option<PlayerId>, ApiError> {
    match asked.map(str::trim).filter(|s| !s.is_empty()) {
        Some("shared") => Ok(None),
        Some(id) => {
            let id: i64 = id.parse().map_err(|_| ApiError::bad(format!("player is an id or \"shared\", not {id:?}")))?;
            match state.db.player(PlayerId(id))? {
                Some(p) => Ok(Some(p.id)),
                None => Err(ApiError::bad(format!("there's no player {id}"))),
            }
        }
        None if state.db.players()?.len() > 1 => Ok(making),
        None => Ok(None),
    }
}

/// Why a token bound to a player can't make this request, if it can't:
/// it can't read the export, which is the whole database with every
/// player's marks, hidden games and history; it can't switch players, add,
/// rename or remove them (setting its own player's PIN aside), or change
/// what is the household's (the schedules, and setup). `PUT /api/setup`
/// itself is let through: it also ends the player's own setup and tour
///, and refuses the household's parts there. `path` is
/// without the version.
pub fn bound_refusal(method: &axum::http::Method, path: &str) -> Option<&'static str> {
    use axum::http::Method;
    // Each prefix is matched as itself or with a `/` after it, so
    // `/api/setup` is refused as well as `/api/setup/state`.
    let under = |prefix: &str| path.strip_prefix(prefix).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'));
    if under("/api/export") {
        return Some("this token is bound to one player, so it can't download the whole library with everyone's history");
    }
    let write = !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS);
    if !write {
        return None;
    }
    if under("/api/players") && !path.ends_with("/pin") {
        return Some("this token is bound to one player, so it can't switch, add, rename or remove players");
    }
    if under("/api/jobs/scheduled") || (under("/api/setup") && path != "/api/setup") {
        return Some("this token is bound to one player, so it can't change what is everyone's on this computer");
    }
    None
}

// ---------------------------------------------------------------------------
// The endpoints
// ---------------------------------------------------------------------------

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/players", get(list).post(add))
        .route("/players/current", get(current_get).put(current_put))
        .route("/players/{id}", put(change).delete(remove))
        .route("/players/{id}/pin", put(pin_put))
        .route("/hidden", get(hidden_list))
        .route("/hidden/unlock", post(hidden_unlock).delete(hidden_lock))
        .route("/games/{id}/hide", put(hide).delete(unhide))
}

/// A player as the API shows them.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct PlayerView {
    pub id: i64,
    pub name: String,
    pub colour: String,
    pub has_pin: bool,
    pub created_at: i64,
    /// The player a request that names nobody acts for.
    pub default: bool,
    /// The identity provider whose account signs in as them, when one is
    /// linked.
    pub oidc: Option<String>,
    /// While "Link the next sign-in" waits for them: until when (Unix seconds).
    pub oidc_link_until: Option<i64>,
    /// The new account waiting to be confirmed as this player, while "Link
    /// the next sign-in" waits. Only the computer UpNext
    /// runs on can confirm it, so only it is shown who that is.
    pub oidc_waiting: Option<super::oidc::OidcWaiting>,
}

/// Every player as `caller` sees them: the account waiting for a link names
/// someone at the provider, so only this computer, which confirms it, sees it.
pub(super) fn views(state: &AppState, caller: Option<&Caller>) -> anyhow::Result<Vec<PlayerView>> {
    let local = caller.is_some_and(Caller::is_local);
    let default = state.db.default_player()?;
    Ok(state
        .db
        .players()?
        .into_iter()
        .map(|p| PlayerView {
            id: p.id.0,
            name: p.name,
            colour: p.colour,
            has_pin: p.has_pin,
            created_at: p.created_at,
            default: p.id == default,
            oidc_link_until: state.remote.oidc.link_until(p.id),
            oidc_waiting: state.remote.oidc.waiting_for(p.id).filter(|_| local),
            oidc: p.oidc_issuer,
        })
        .collect())
}

fn not_found(id: i64) -> ApiError {
    ApiError::not_found(format!("no player {id}"))
}

#[utoipa::path(get, path = "/players", tag = "players",
    description = "Every player, and who this request acts for (`current`, null while the default player has a PIN and \
        nobody has been chosen). `bound`: the token it came with is bound to that player.",
    responses((status = 200, body = Object, description = "`{players: [PlayerView], current: id | null, bound}`")))]
async fn list(State(state): State<AppState>, caller: Option<Extension<Caller>>, who: Option<Extension<Who>>) -> ApiResult<Value> {
    let who = who.map(|w| w.0);
    Ok(Json(json!({
        "players": views(&state, caller.as_deref())?,
        "current": who.as_ref().and_then(|w| w.player),
        "bound": who.is_some_and(|w| w.bound),
    })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct AddBody {
    name: String,
    /// `#rrggbb`; the next of the built-in colours when left out.
    colour: Option<String>,
}

#[utoipa::path(post, path = "/players", tag = "players",
    description = "Add a player. They see what is shared with everyone at once, without anyone else's hours.",
    request_body = AddBody,
    responses((status = 201, body = PlayerView), (status = 400, description = "No name, a taken one, or a bad colour"),
        (status = 403, description = "A token bound to a player")))]
async fn add(
    State(state): State<AppState>,
    caller: Option<Extension<Caller>>,
    Json(body): Json<AddBody>,
) -> Result<(StatusCode, Json<PlayerView>), ApiError> {
    let bad = |e: anyhow::Error| ApiError::bad(e.to_string());
    let p = state.db.add_player(&crate::db::clean_name(&body.name)).map_err(bad)?;
    if let Some(c) = &body.colour {
        state.db.set_player_colour(p, c).map_err(bad)?;
    }
    let made = views(&state, caller.as_deref())?.into_iter().find(|v| v.id == p.0).ok_or_else(|| not_found(p.0))?;
    Ok((StatusCode::CREATED, Json(made)))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct ChangeBody {
    name: Option<String>,
    colour: Option<String>,
    /// true makes them the default player.
    default: Option<bool>,
}

#[utoipa::path(put, path = "/players/{id}", tag = "players",
    description = "Rename or recolour a player, or make them the default player.",
    params(("id" = i64, Path, description = "Player id")),
    request_body = ChangeBody,
    responses((status = 200, body = PlayerView), (status = 400, description = "A taken name or a bad colour"),
        (status = 403, description = "A token bound to a player"), (status = 404, description = "No such player")))]
async fn change(
    State(state): State<AppState>,
    caller: Option<Extension<Caller>>,
    Path(id): Path<i64>,
    Json(body): Json<ChangeBody>,
) -> ApiResult<PlayerView> {
    let p = PlayerId(id);
    state.db.player(p)?.ok_or_else(|| not_found(id))?;
    let bad = |e: anyhow::Error| ApiError::bad(e.to_string());
    if let Some(n) = &body.name {
        state.db.rename_player(p, n).map_err(bad)?;
    }
    if let Some(c) = &body.colour {
        state.db.set_player_colour(p, c).map_err(bad)?;
    }
    if body.default == Some(true) {
        state.db.set_default_player(p)?;
    }
    Ok(Json(views(&state, caller.as_deref())?.into_iter().find(|v| v.id == id).ok_or_else(|| not_found(id))?))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct PinBody {
    /// The PIN now, when they have one.
    pin: Option<String>,
    /// The new PIN, four to eight digits; null removes it.
    new_pin: Option<String>,
}

#[utoipa::path(put, path = "/players/{id}/pin", tag = "players",
    description = "Set, change or remove a player's PIN: only as that player, and with the PIN now when they have one. \
        Five wrong PINs lock it for a minute, then two, four and so on.",
    params(("id" = i64, Path, description = "Player id")),
    request_body = PinBody,
    responses((status = 204, description = "Changed"), (status = 400, description = "Not four to eight digits"),
        (status = 403, description = "Another player's, or a wrong PIN"), (status = 429, description = "Locked after wrong PINs")))]
async fn pin_put(
    State(state): State<AppState>,
    Me(me): Me,
    Path(id): Path<i64>,
    Json(body): Json<PinBody>,
) -> Result<StatusCode, ApiError> {
    let p = PlayerId(id);
    let player = state.db.player(p)?.ok_or_else(|| not_found(id))?;
    if p != me {
        return Err(ApiError::forbidden("only a player can change their own PIN: switch to them first"));
    }
    if player.has_pin {
        state.remote.pins.check(&state, p, body.pin.as_deref())?;
    }
    let new = body.new_pin.as_deref().map(str::trim).filter(|n| !n.is_empty());
    if new.is_some_and(|n| !crate::db::pin_ok(n)) {
        return Err(ApiError::bad("a PIN is four to eight digits"));
    }
    state.db.set_pin(p, new)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
struct RemoveBody {
    /// Their PIN, when they have one.
    pin: Option<String>,
}

#[utoipa::path(delete, path = "/players/{id}", tag = "players",
    description = "Remove a player, only on the computer UpNext runs on: a backup first (`Before removing <name>`), then \
        their marks, history, taste profile, lists, store connections with their accounts and games, AI accounts and bound \
        tokens. The others lose the games and AI that came only from those accounts. Asks for their PIN when they have one, \
        and refuses the last player.",
    params(("id" = i64, Path, description = "Player id")),
    request_body(content = RemoveBody, description = "`{pin}` when they have one, else `{}`"),
    responses((status = 200, body = Object, description = "`{backup, removed}`: the backup's file name and the player's name"),
        (status = 400, description = "The last player"), (status = 403, description = "From another device, or a wrong PIN"),
        (status = 404, description = "No such player")))]
async fn remove(
    State(state): State<AppState>,
    caller: Option<Extension<Caller>>,
    Path(id): Path<i64>,
    Json(body): Json<RemoveBody>,
) -> ApiResult<Value> {
    if !caller.is_some_and(|c| c.is_local()) {
        return Err(ApiError::forbidden("players are removed only on the computer UpNext runs on"));
    }
    let p = PlayerId(id);
    let player = state.db.player(p)?.ok_or_else(|| not_found(id))?;
    if state.db.players()?.len() <= 1 {
        return Err(ApiError::bad("the last player can't be removed"));
    }
    if player.has_pin {
        state.remote.pins.check(&state, p, body.pin.as_deref())?;
    }
    let cfg = state.cfg();
    let reason = format!("Before removing {}", player.name);
    let new = crate::backups::New {
        kind: crate::backups::Kind::Manual,
        about: crate::backups::About { note: None, reason: Some(reason) },
        keep: None,
        job: None,
    };
    let (path, dir) = (cfg.db_path.clone(), crate::jobs::backup::backups_dir(&cfg));
    let made = tokio::task::spawn_blocking(move || crate::backups::write(&path, &dir, &crate::backups::Kind::Manual.stem(), &new))
        .await
        .map_err(anyhow::Error::from)?
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("backing up first, so nothing was removed: {e:#}")))?;
    let bound: Vec<i64> = state.db.tokens()?.into_iter().filter(|t| t.player_id == Some(p)).map(|t| t.id).collect();
    state.db.remove_player(p).map_err(|e| ApiError::bad(e.to_string()))?;
    for t in bound {
        state.remote.limits.forget(t);
    }
    state.remote.pins.forget(p);
    tracing::info!("removed a player, after backing up to {}", made.name);
    Ok(Json(json!({ "backup": made.name, "removed": player.name })))
}

#[utoipa::path(get, path = "/players/current", tag = "players",
    description = "Who this request acts for: `player` (null while the default player has a PIN and nobody has been chosen), \
        and `bound` when the token it came with is bound to them.",
    responses((status = 200, body = Object, description = "`{player: PlayerView | null, bound}`")))]
async fn current_get(State(state): State<AppState>, caller: Option<Extension<Caller>>, who: Option<Extension<Who>>) -> ApiResult<Value> {
    let who = who.map(|w| w.0);
    let current = who.as_ref().and_then(|w| w.player);
    let player = views(&state, caller.as_deref())?.into_iter().find(|v| Some(v.id) == current.map(|p| p.0));
    Ok(Json(json!({ "player": player, "bound": who.is_some_and(|w| w.bound) })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct CurrentBody {
    player_id: i64,
    /// Their PIN, when they have one.
    pin: Option<String>,
}

#[utoipa::path(put, path = "/players/current", tag = "players",
    description = "Choose who's playing in this browser: sets the `upnext_player_<port>` cookie (HttpOnly, SameSite=Strict, \
        signed). A player with a PIN needs it; five wrong ones lock it for a minute, doubling each time.",
    request_body = CurrentBody,
    responses((status = 200, body = PlayerView, headers(("Set-Cookie" = String))),
        (status = 403, description = "A wrong or missing PIN, or a token bound to a player"),
        (status = 404, description = "No such player"), (status = 429, description = "Locked after wrong PINs")))]
async fn current_put(
    State(state): State<AppState>,
    caller: Option<Extension<Caller>>,
    https: Option<Extension<Https>>,
    Json(body): Json<CurrentBody>,
) -> Result<Response, ApiError> {
    let p = PlayerId(body.player_id);
    let player = state.db.player(p)?.ok_or_else(|| not_found(body.player_id))?;
    if player.has_pin {
        state.remote.pins.check(&state, p, body.pin.as_deref())?;
    }
    let value = signed(&state, p)?;
    let https = https.is_some_and(|Extension(Https(on))| on);
    let view = views(&state, caller.as_deref())?.into_iter().find(|v| v.id == p.0).ok_or_else(|| not_found(p.0))?;
    let mut res = Json(view).into_response();
    res.headers_mut().insert(header::SET_COOKIE, set_cookie(&state, &value, https));
    Ok(res)
}

#[utoipa::path(get, path = "/hidden", tag = "players",
    description = "Hidden by me: every game this player hid, newest first, with its scope. For a player with a PIN, only \
        within ten minutes of `POST /hidden/unlock` in this browser (or with this token).",
    responses((status = 200, body = Vec<HiddenTitle>), (status = 403, description = "Locked: the player's PIN is needed")))]
async fn hidden_list(State(state): State<AppState>, Me(me): Me, who: Option<Extension<Who>>) -> ApiResult<Vec<HiddenTitle>> {
    if !hidden_open(&state, me, who.as_deref())? {
        return Err(ApiError::forbidden("enter your PIN to see the games you hid"));
    }
    Ok(Json(state.db.hidden_by(me)?))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct UnlockBody {
    pin: Option<String>,
}

#[utoipa::path(post, path = "/hidden/unlock", tag = "players",
    description = "Open this player's Hidden by me list, and their hidden games' pages, for ten minutes in this browser \
        (or with this token). A player without a PIN needs nothing.",
    request_body = UnlockBody,
    responses((status = 204, description = "Open"), (status = 403, description = "A wrong PIN"),
        (status = 429, description = "Locked after wrong PINs")))]
async fn hidden_unlock(
    State(state): State<AppState>,
    Me(me): Me,
    who: Option<Extension<Who>>,
    Json(body): Json<UnlockBody>,
) -> Result<StatusCode, ApiError> {
    if !state.db.player(me)?.is_some_and(|p| p.has_pin) {
        return Ok(StatusCode::NO_CONTENT);
    }
    let Some(holder) = who.and_then(|w| w.0.holder) else {
        return Err(ApiError::forbidden("choose who's playing in this browser first"));
    };
    state.remote.pins.check(&state, me, body.pin.as_deref())?;
    state.remote.pins.unlock(me, &holder);
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(delete, path = "/hidden/unlock", tag = "players",
    description = "Lock Hidden by me again in this browser (or with this token) before its ten minutes are up. \
        Nothing to do for a player without a PIN, or when it wasn't open.",
    responses((status = 204, description = "Locked")))]
async fn hidden_lock(Me(me): Me, State(state): State<AppState>, who: Option<Extension<Who>>) -> StatusCode {
    if let Some(holder) = who.and_then(|w| w.0.holder) {
        state.remote.pins.lock(me, &holder);
    }
    StatusCode::NO_CONTENT
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct HideBody {
    /// `me` (from your own views), `others` (your copies, from everyone else) or `everyone` (both).
    scope: String,
}

/// What hiding a game answers.
#[derive(Debug, Serialize, utoipa::ToSchema)]
struct HideAnswer {
    scope: HideScope,
    /// Hidden from the others: who still sees it. Empty for `me`.
    still_seen_by: Vec<String>,
    /// Those of them who see it only through another player's shared
    /// account, not a copy of their own.
    through_shared: Vec<String>,
}

#[utoipa::path(put, path = "/games/{id}/hide", tag = "games",
    description = "Hide this game: `me` takes it out of your own library, search, picks and statistics; `others` keeps your \
        copies out of every other player's, unless they have it through their own account or another shared one; \
        `everyone` does both. It isn't Not for me: it teaches the taste profile nothing. Hidden from the others, \
        `still_seen_by` names the players who still see it, and `through_shared` those of them who see it only through \
        another player's shared account rather than a copy of their own; both are empty for `me`.",
    params(("id" = i64, Path, description = "Title id")),
    request_body = HideBody,
    responses((status = 200, body = HideAnswer),
        (status = 400, description = "An unknown scope, or others/everyone without a copy of your own"),
        (status = 404, description = "No such title")))]
async fn hide(
    State(state): State<AppState>,
    Me(me): Me,
    who: Option<Extension<Who>>,
    Path(id): Path<i64>,
    Json(body): Json<HideBody>,
) -> ApiResult<HideAnswer> {
    let scope =
        HideScope::parse(&body.scope).ok_or_else(|| ApiError::bad(format!("a scope is me, others or everyone, not {:?}", body.scope)))?;
    if visible_game(&state, me, who.as_deref(), id)?.is_none() {
        return Err(ApiError::not_found(format!("game {id} not found")));
    }
    state.db.set_hidden(me, id, Some(scope)).map_err(|e| ApiError::bad(e.to_string()))?;
    let still = state.db.still_seen_by(me, id, scope)?;
    let through_shared = still.iter().filter(|s| !s.own_copy).map(|s| s.name.clone()).collect();
    let still_seen_by = still.into_iter().map(|s| s.name).collect();
    Ok(Json(HideAnswer { scope, still_seen_by, through_shared }))
}

#[utoipa::path(delete, path = "/games/{id}/hide", tag = "games",
    description = "Unhide a game you hid.",
    params(("id" = i64, Path, description = "Title id")),
    responses((status = 204, description = "Unhidden, or it wasn't hidden")))]
async fn unhide(State(state): State<AppState>, Me(me): Me, Path(id): Path<i64>) -> Result<StatusCode, ApiError> {
    state.db.set_hidden(me, id, None)?;
    Ok(StatusCode::NO_CONTENT)
}

/// A game as `p` sees it, or one they hid from themself while their hidden
/// games are open to this request; None otherwise, as if it weren't there.
pub fn visible_game(state: &AppState, p: PlayerId, who: Option<&Who>, id: i64) -> Result<Option<crate::db::GameView>, ApiError> {
    if let Some(g) = state.db.game(p, id)? {
        return Ok(Some(g));
    }
    let hid = state.db.hidden_by(p)?.iter().any(|h| h.title_id == id);
    if hid && hidden_open(state, p, who)? {
        return Ok(state.db.game_unhidden(p, id)?);
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_matches_rfc_4231() {
        // Test case 2.
        let mac = hmac(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(hex(&mac), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
    }

    #[test]
    fn bound_tokens_keep_to_their_player() {
        use axum::http::Method;
        assert!(bound_refusal(&Method::POST, "/api/players").is_some());
        assert!(bound_refusal(&Method::PUT, "/api/players/current").is_some());
        assert!(bound_refusal(&Method::PUT, "/api/players/2").is_some());
        assert!(bound_refusal(&Method::DELETE, "/api/players/2").is_some());
        assert_eq!(bound_refusal(&Method::PUT, "/api/players/2/pin"), None);
        assert_eq!(bound_refusal(&Method::GET, "/api/players"), None);
        assert!(bound_refusal(&Method::PUT, "/api/jobs/scheduled/sync").is_some());
        assert!(bound_refusal(&Method::POST, "/api/setup/state").is_some());
        assert_eq!(bound_refusal(&Method::PUT, "/api/setup"), None, "its own setup and tour; the handler refuses the household's");
        assert!(bound_refusal(&Method::POST, "/api/setup/reset").is_some());
        assert_eq!(bound_refusal(&Method::GET, "/api/setup"), None);
        assert!(bound_refusal(&Method::GET, "/api/export").is_some(), "everyone's history");
        assert!(bound_refusal(&Method::HEAD, "/api/export").is_some());
        assert_eq!(bound_refusal(&Method::GET, "/api/taste/export"), None, "their own taste");
        assert_eq!(bound_refusal(&Method::PUT, "/api/setupx"), None);
        assert_eq!(bound_refusal(&Method::PUT, "/api/games/3/override"), None);
    }
}
