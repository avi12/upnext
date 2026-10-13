//! Pairing a browser on another device: Settings, "Add a device",
//! on the computer UpNext runs on shows a six-digit code for five minutes,
//! with a QR code of the address. The phone opens the address, the UI asks
//! for the code (or reads it from the QR code's `#pair=` fragment), and the server trades it for a device token of `full`
//! scope, set as a cookie (`HttpOnly`, `SameSite=Strict`, `Secure` over
//! HTTPS). The device is then in the token list and can be revoked.
//!
//! A code works once; five wrong tries end it. One code at a time: making
//! another ends the last.

use super::access::{self, Https};
use super::approval::Wants;
use super::lan::lan_address;
use super::{ApiError, ApiResult, AppState};
use crate::db::{Scope, now};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// How long a code lasts, in seconds.
pub const CODE_TTL: i64 = 300;
/// How long the code a container prints at startup lasts: an
/// hour, since its log may be read a while after `docker compose up`.
pub const STARTUP_CODE_TTL: i64 = 3600;
/// Wrong tries that end a code.
pub const MAX_TRIES: u8 = 5;
/// The paired browser's cookie as it was named before it took the port
/// (B-AUTH-4). Still taken, since the Android app sets it itself in its
/// WebView, but one this server doesn't know is ignored rather than
/// refused and cleared: it may be another UpNext's on the same host.
pub const OLD_COOKIE: &str = "upnext_device";
/// About 400 days, what browsers allow at most.
const COOKIE_MAX_AGE: i64 = 400 * 24 * 3600;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/pairing", get(current).post(start).delete(cancel))
        .route("/pairing/outcome", get(outcome))
        .route("/pair", post(pair))
}

struct Code {
    code: String,
    expires_at: i64,
    wrong: u8,
    /// The page the QR code opens on the new device, after the address.
    then: Then,
    /// Whose device it is: its token is bound to them. None for
    /// a device everyone shares.
    player: Option<crate::db::PlayerId>,
}

/// Where the QR code takes the new device once it's paired.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Then {
    /// The UI's first page.
    #[default]
    Home,
    /// The setup wizard: big screen's first run hands the steps that need
    /// typing (an API key, a sign-in) to a phone or computer this way.
    Setup,
}

impl Then {
    fn path(self) -> &'static str {
        match self {
            Self::Home => "/",
            Self::Setup => "/setup",
        }
    }
}

/// The code waiting to be typed on a new device, if there is one, and the
/// last code that paired one, so the page that showed it can tell a code
/// that was used from one that ran out.
#[derive(Default)]
pub struct Codes {
    waiting: Mutex<Option<Code>>,
    used: Mutex<Option<(String, Paired)>>,
}

impl Codes {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Code>> {
        self.waiting.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn used(&self) -> std::sync::MutexGuard<'_, Option<(String, Paired)>> {
        self.used.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Five minutes gone by, for the tests.
    #[cfg(test)]
    pub fn expire(&self) {
        if let Some(c) = self.lock().as_mut() {
            c.expires_at = now() - 1;
        }
    }
}

/// What "Add a device" shows.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Pairing {
    /// Six digits, typed on the new device.
    pub code: String,
    /// When it stops working (Unix seconds).
    pub expires_at: i64,
    /// The address the new device opens: `public_url`, else this computer's
    /// address on its network. None while UpNext only listens on this computer.
    pub address: Option<String>,
    /// A QR code of `address` with the code in its fragment
    /// (`…/#pair=482913`), as an SVG image: the pair page fills the code in,
    /// and a fragment never reaches a server or a proxy's log.
    pub qr_svg: Option<String>,
}

/// A six-digit code from the system's randomness.
fn new_code() -> anyhow::Result<String> {
    loop {
        let mut b = [0u8; 4];
        getrandom::fill(&mut b).map_err(|e| anyhow::anyhow!("no randomness for a pairing code: {e}"))?;
        let n = u32::from_le_bytes(b);
        // Below the last whole million, so every code is as likely.
        if n < u32::MAX - u32::MAX % 1_000_000 {
            return Ok(format!("{:06}", n % 1_000_000));
        }
    }
}

/// The address a new device opens, for the QR code. In a container,
/// `public_url` or nothing: the container's own address isn't one the host
/// or the network can open.
fn address(state: &AppState) -> Option<String> {
    if let Some(p) = access::public_url(state) {
        return Some(p.origin());
    }
    if state.base_cfg.container {
        return None;
    }
    let listening = state.listening();
    if !listening.beyond_loopback() {
        return None;
    }
    let scheme = if listening.https { "https" } else { "http" };
    let ip = match listening.host.parse::<std::net::IpAddr>() {
        Ok(ip) if !ip.is_unspecified() => ip,
        _ => lan_address()?,
    };
    let host = match ip {
        std::net::IpAddr::V6(v6) => format!("[{v6}]"),
        v4 => v4.to_string(),
    };
    Some(format!("{scheme}://{host}:{}", listening.port))
}

fn qr_svg(text: &str) -> Option<String> {
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    Some(code.render::<qrcode::render::svg::Color>().min_dimensions(200, 200).quiet_zone(true).build())
}

/// What the QR code opens: the address and the page, with the code in the
/// fragment.
fn qr_target(address: &str, then: Then, code: &str) -> String {
    format!("{}{}#pair={code}", address.trim_end_matches('/'), then.path())
}

fn view(state: &AppState, c: &Code) -> Pairing {
    let address = address(state);
    let qr_svg = address.as_deref().and_then(|a| qr_svg(&qr_target(a, c.then, &c.code)));
    Pairing { code: c.code.clone(), expires_at: c.expires_at, address, qr_svg }
}

/// The code in effect, if there's one.
#[utoipa::path(get, path = "/pairing", tag = "devices", operation_id = "pairing_get",
    description = "The pairing code waiting to be typed on a new device, or null.",
    responses((status = 200, body = Option<Pairing>)))]
async fn current(State(state): State<AppState>) -> ApiResult<Option<Pairing>> {
    let codes = state.remote.pairing.lock();
    Ok(Json(codes.as_ref().filter(|c| c.expires_at > now()).map(|c| view(&state, c))))
}

/// What became of a code.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PairingOutcome {
    /// `waiting`: it can still be typed. `paired`: a device used it.
    /// `ended`: it ran out, took five wrong tries, or was ended or replaced.
    #[schema(value_type = String, example = "paired")]
    outcome: &'static str,
    /// The device it paired, when it did.
    device: Option<Paired>,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
struct OutcomeQuery {
    /// The six digits the code was shown with.
    code: String,
}

/// Whether a code paired a device: the page that showed it waits on this,
/// since a used code and one that ran out both leave no code waiting.
#[utoipa::path(get, path = "/pairing/outcome", tag = "devices", operation_id = "pairing_outcome",
    description = "What became of a pairing code: still waiting, used by a device (with the device), or ended without \
        one (run out, five wrong tries, ended, or replaced by a newer code). Only on the computer UpNext runs on.",
    params(OutcomeQuery),
    responses((status = 200, body = PairingOutcome)))]
async fn outcome(State(state): State<AppState>, Query(q): Query<OutcomeQuery>) -> ApiResult<PairingOutcome> {
    // Under the waiting code's lock, which `pair` holds until the device is
    // made, so a code is never seen between waiting and paired.
    let waiting = state.remote.pairing.lock();
    if waiting.as_ref().is_some_and(|c| c.code == q.code && c.expires_at > now()) {
        return Ok(Json(PairingOutcome { outcome: "waiting", device: None }));
    }
    let used = state.remote.pairing.used();
    Ok(Json(match used.as_ref().filter(|(code, _)| *code == q.code) {
        Some((_, device)) => PairingOutcome { outcome: "paired", device: Some(device.clone()) },
        None => PairingOutcome { outcome: "ended", device: None },
    }))
}

#[derive(Debug, Default, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
struct StartQuery {
    /// The page the QR code opens on the new device: `home` (the default)
    /// or `setup`, the setup wizard.
    #[param(inline)]
    then: Option<Then>,
    /// Whose device it is: a player's id, or `shared` for one
    /// everyone uses. By default, with two or more players, the player
    /// making the code; with one, shared, as before there were players.
    player: Option<String>,
}

/// The PIN of the player the device will belong to, when they have one: in
/// the body rather than the query, so it stays out of logs.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[schema(as = PairingStartBody)]
struct StartBody {
    #[serde(default)]
    pin: Option<String>,
}

/// "Add a device": a new code, valid for five minutes. Any earlier code ends.
#[utoipa::path(post, path = "/pairing", tag = "devices", operation_id = "pairing_start",
    description = "Make a pairing code (six digits, five minutes, one use). Any earlier code ends. With `then=setup`, the \
        QR code opens the setup wizard on the new device once it's paired.",
    params(StartQuery),
    request_body(content = Option<StartBody>, description = "`{pin}` when the device's player has a PIN"),
    responses((status = 201, body = Pairing), (status = 400, description = "A `then` other than home or setup"),
        (status = 403, description = "The device's player has a PIN and it's missing or wrong"),
        (status = 429, description = "Too many wrong PINs for that player: try again later")))]
async fn start(
    State(state): State<AppState>,
    who: Option<Extension<super::players::Who>>,
    Query(q): Query<StartQuery>,
    body: Option<Json<StartBody>>,
) -> Result<(StatusCode, Json<Pairing>), ApiError> {
    let player = super::players::pairing_player(&state, who.and_then(|w| w.player), q.player.as_deref())?;
    super::players::binding_allowed(&state, player, body.as_ref().and_then(|b| b.pin.as_deref()))?;
    Ok((StatusCode::CREATED, Json(new_pairing_to(&state, CODE_TTL, q.then.unwrap_or_default(), player)?)))
}

/// A new code that lasts `ttl` seconds. Any earlier code ends.
pub fn new_pairing(state: &AppState, ttl: i64) -> anyhow::Result<Pairing> {
    new_pairing_to(state, ttl, Then::Home, None)
}

/// A new code whose QR code opens `then`.
fn new_pairing_to(state: &AppState, ttl: i64, then: Then, player: Option<crate::db::PlayerId>) -> anyhow::Result<Pairing> {
    let code = Code { code: new_code()?, expires_at: now() + ttl, wrong: 0, then, player };
    let shown = view(state, &code);
    *state.remote.pairing.lock() = Some(code);
    Ok(shown)
}

/// A code as people read and type it: `482 913`.
pub fn spaced(code: &str) -> String {
    if code.len() == 6 { format!("{} {}", &code[..3], &code[3..]) } else { code.to_string() }
}

/// What to do with a code, for the container's log and `upnext pair`: open
/// the code's address, else localhost on this port (where the compose file
/// publishes it), and type the code.
pub fn how_to_use(p: &Pairing, port: u16) -> String {
    let address = p.address.clone().unwrap_or_else(|| format!("http://localhost:{port}"));
    let minutes = ((p.expires_at - now() + 30) / 60).max(1);
    let within = if minutes == 1 { "the next minute".to_string() } else { format!("the next {minutes} minutes") };
    format!("Open {address} and enter {} to pair your browser (it works once, within {within}).", spaced(&p.code))
}

/// What a container's server prints when it starts. Inside a
/// container no browser is "this computer", so until a browser has been
/// paired or a token made, a code goes to the log, like Jupyter's startup
/// link; after that, how to get another.
pub fn container_greeting(state: &AppState) -> anyhow::Result<String> {
    const TOKEN: &str = "A token for a script: `docker exec upnext upnext token create --name NAME --scope read|full`.";
    if state.db.tokens()?.is_empty() {
        let p = new_pairing(state, STARTUP_CODE_TTL)?;
        Ok(format!("{}\nAnother code: `docker exec upnext upnext pair`. {TOKEN}", how_to_use(&p, state.base_cfg.port)))
    } else {
        Ok(format!("To pair another browser, `docker exec upnext upnext pair` prints a code. {TOKEN}"))
    }
}

#[utoipa::path(delete, path = "/pairing", tag = "devices", operation_id = "pairing_cancel", description = "End the pairing code now.",
    responses((status = 204)))]
async fn cancel(State(state): State<AppState>) -> StatusCode {
    *state.remote.pairing.lock() = None;
    StatusCode::NO_CONTENT
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct PairBody {
    /// The six digits shown in Settings.
    code: String,
    /// What to call this device in the token list; worked out from the
    /// browser when left out ("Android phone").
    #[serde(default)]
    name: Option<String>,
    /// `cookie` (the default) for a browser, `token` for an app that keeps
    /// the token itself, such as the Android app.
    #[serde(default)]
    wants: Option<Wants>,
}

/// The device a pairing made.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct Paired {
    pub id: i64,
    pub name: String,
}

/// What `POST /pair` answers: the device, and for an app that asked for
/// it, the token and what it needs to find this computer again.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PairAnswer {
    pub id: i64,
    pub name: String,
    /// The token, only when the body asked for it (`wants: token`); no
    /// cookie is set then. This is the only time it's seen.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// `public_url` when it's set and the token was asked for: the token is
    /// taken only at that name then.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    /// The computer's name on the network, when the token was asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub computer: Option<String>,
    /// Its install id, as it announces it, when the token was asked for: the
    /// app finds the computer again by this after its address changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub computer_id: Option<String>,
}

/// A device's name from its browser, for the token list.
pub(super) fn device_name(headers: &HeaderMap) -> String {
    let ua = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or_default();
    let kind = if ua.contains("iPad") {
        "iPad"
    } else if ua.contains("iPhone") {
        "iPhone"
    } else if ua.contains("Android") {
        if ua.contains("Mobile") { "Android phone" } else { "Android tablet" }
    } else if ua.contains("Windows") {
        "Windows browser"
    } else if ua.contains("Mac OS") {
        "Mac browser"
    } else if ua.contains("Linux") {
        "Linux browser"
    } else {
        "Browser"
    };
    kind.to_string()
}

/// `name`, or `name 2`, `name 3`… when it's taken.
pub(super) fn free_name(state: &AppState, name: &str) -> anyhow::Result<String> {
    let taken: Vec<String> = state.db.tokens()?.into_iter().map(|t| t.name.to_lowercase()).collect();
    let mut n = 1;
    loop {
        let candidate = if n == 1 { name.to_string() } else { format!("{name} {n}") };
        if !taken.contains(&candidate.to_lowercase()) {
            return Ok(candidate);
        }
        n += 1;
    }
}

/// The paired browser's cookie's name: with the port, as the player
/// cookie's is, since browsers send a host's cookies to every port on it,
/// and two UpNexts on one host would read, refuse and clear each other's.
/// It's the port UpNext listens on, which in a container is 8787 whatever
/// host port it's published on, so a container and another UpNext on 8787
/// on one host still share the name.
pub fn cookie_name(state: &AppState) -> String {
    format!("{OLD_COOKIE}_{}", state.base_cfg.port)
}

/// The browser's cookie for a device token.
pub fn cookie(state: &AppState, token: &str, secure: bool) -> HeaderValue {
    named_cookie(&cookie_name(state), token, secure)
}

fn named_cookie(name: &str, token: &str, secure: bool) -> HeaderValue {
    let secure = if secure { "; Secure" } else { "" };
    let v = format!("{name}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={COOKIE_MAX_AGE}{secure}");
    HeaderValue::from_str(&v).unwrap_or_else(|_| HeaderValue::from_static(""))
}

/// Ends a cookie the server no longer accepts.
pub fn cookie_cleared(state: &AppState) -> HeaderValue {
    ended(&cookie_name(state))
}

/// Ends the cookie as it was named before it took the port.
pub fn old_cookie_cleared() -> HeaderValue {
    ended(OLD_COOKIE)
}

fn ended(name: &str) -> HeaderValue {
    HeaderValue::from_str(&format!("{name}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0"))
        .unwrap_or_else(|_| HeaderValue::from_static(""))
}

/// The new device: trade the code for a device token in a cookie. Anyone
/// who can reach the server may try; plain HTTP only from a private address.
#[utoipa::path(post, path = "/pair", tag = "devices", security(()),
    description = "Pair this browser: trade the six-digit code for a device token, set as an HttpOnly, \
        SameSite=Strict cookie. Needs no token. Five wrong tries end the code. An app that keeps the token itself \
        (wants: token) gets it in the answer instead of the cookie.",
    request_body = PairBody,
    responses((status = 201, body = PairAnswer), (status = 401, description = "Wrong code"), (status = 404, description = "No code is waiting")))]
async fn pair(
    State(state): State<AppState>,
    https: Option<Extension<Https>>,
    headers: HeaderMap,
    Json(body): Json<PairBody>,
) -> Result<Response, ApiError> {
    let refused = |status: StatusCode, m: String| ApiError(status, m);
    // The code stays waiting until the device is made, so `outcome` never
    // sees it gone before it's paired.
    let mut codes = state.remote.pairing.lock();
    let (code, player) = {
        let Some(c) = codes.as_mut().filter(|c| c.expires_at > now()) else {
            *codes = None;
            return Err(refused(
                StatusCode::NOT_FOUND,
                "no pairing code is waiting: in Settings › Devices and API, on the computer UpNext runs on, choose Add a device".into(),
            ));
        };
        let typed: String = body.code.chars().filter(|ch| !ch.is_whitespace()).collect();
        if !constant_time_eq(typed.as_bytes(), c.code.as_bytes()) {
            c.wrong += 1;
            let left = MAX_TRIES - c.wrong;
            if left == 0 {
                *codes = None;
                return Err(refused(
                    StatusCode::UNAUTHORIZED,
                    "that's not the code, and that was the last try: make a new one in Settings".into(),
                ));
            }
            let tries = if left == 1 { "1 try".to_string() } else { format!("{left} tries") };
            return Err(refused(StatusCode::UNAUTHORIZED, format!("that's not the code: {tries} left")));
        }
        (c.code.clone(), c.player)
    };
    // An app's answer carries the install id: read it before the code is
    // used, so a failure here leaves the code to try again.
    let computer_id = if body.wants == Some(Wants::Token) { Some(super::network::network_id(&state)?) } else { None };
    let wanted = body.name.as_deref().map(crate::db::clean_name).filter(|n| !n.is_empty()).unwrap_or_else(|| device_name(&headers));
    let name = free_name(&state, &wanted.chars().take(50).collect::<String>())?;
    let made = state.db.token_create_for(&name, Scope::Full, "device", player)?;
    let paired = Paired { id: made.info.id, name };
    *state.remote.pairing.used() = Some((code, paired.clone()));
    *codes = None;
    drop(codes);
    tracing::info!("paired a new device: {}", paired.name);
    let mut answer = PairAnswer { id: paired.id, name: paired.name, token: None, address: None, computer: None, computer_id: None };
    if body.wants == Some(Wants::Token) {
        answer.token = Some(made.token);
        answer.address = access::public_url(&state).map(|p| p.origin());
        answer.computer = Some(super::network::network_name(&state.cfg()));
        answer.computer_id = computer_id;
        return Ok((StatusCode::CREATED, Json(answer)).into_response());
    }
    let secure = https.is_some_and(|Extension(Https(on))| on);
    let mut res = (StatusCode::CREATED, Json(answer)).into_response();
    res.headers_mut().insert(header::SET_COOKIE, cookie(&state, &made.token, secure));
    Ok(res)
}

pub(super) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_six_digits() {
        for _ in 0..200 {
            let c = new_code().unwrap();
            assert_eq!(c.len(), 6);
            assert!(c.chars().all(|ch| ch.is_ascii_digit()), "{c}");
        }
    }

    #[test]
    fn device_names_from_browsers() {
        let named = |ua: &str| {
            let mut h = HeaderMap::new();
            h.insert(header::USER_AGENT, HeaderValue::from_str(ua).unwrap());
            device_name(&h)
        };
        assert_eq!(named("Mozilla/5.0 (Linux; Android 15; Pixel 9) AppleWebKit/537.36 Chrome/140 Mobile Safari/537.36"), "Android phone");
        assert_eq!(named("Mozilla/5.0 (iPhone; CPU iPhone OS 19_0 like Mac OS X) AppleWebKit/605.1.15 Mobile/15E148"), "iPhone");
        assert_eq!(named("curl/8.9"), "Browser");
    }

    #[test]
    fn the_cookie() {
        let c = named_cookie("upnext_device_8787", "upnext_abc", false);
        let c = c.to_str().unwrap();
        assert!(c.starts_with("upnext_device_8787=upnext_abc;"), "{c}");
        assert!(c.contains("HttpOnly") && c.contains("SameSite=Strict") && !c.contains("Secure"), "{c}");
        assert!(named_cookie("upnext_device_8787", "upnext_abc", true).to_str().unwrap().ends_with("; Secure"));
        assert_eq!(ended("upnext_device_8787").to_str().unwrap(), "upnext_device_8787=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0");
    }

    #[test]
    fn how_a_code_is_shown() {
        assert_eq!(spaced("482913"), "482 913");
        let p = |address: Option<&str>, ttl: i64| Pairing {
            code: "482913".into(),
            expires_at: now() + ttl,
            address: address.map(Into::into),
            qr_svg: None,
        };
        assert_eq!(
            how_to_use(&p(None, STARTUP_CODE_TTL), 8787),
            "Open http://localhost:8787 and enter 482 913 to pair your browser (it works once, within the next 60 minutes)."
        );
        let shown = how_to_use(&p(Some("https://upnext.example.org"), CODE_TTL), 8787);
        assert!(shown.starts_with("Open https://upnext.example.org and enter 482 913") && shown.contains("the next 5 minutes"), "{shown}");
        assert!(how_to_use(&p(None, 20), 8787).contains("within the next minute"));
    }

    #[test]
    fn qr_codes_render() {
        assert_eq!(qr_target("http://192.168.0.10:8787", Then::Home, "482913"), "http://192.168.0.10:8787/#pair=482913");
        assert_eq!(qr_target("https://upnext.example.org/", Then::Home, "000042"), "https://upnext.example.org/#pair=000042");
        assert_eq!(qr_target("http://192.168.0.10:8787", Then::Setup, "482913"), "http://192.168.0.10:8787/setup#pair=482913");
        let svg = qr_svg(&qr_target("http://192.168.0.10:8787", Then::Home, "482913")).unwrap();
        assert!(svg.starts_with("<?xml") || svg.starts_with("<svg"), "{}", &svg[..40]);
    }
}
