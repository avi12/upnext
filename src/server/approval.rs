//! Approving a device with one tap on the computer, in
//! place of typing the six-digit pairing code, which stays as the fallback.
//!
//! 1. The device asks: `POST /api/pair/request` with its name and kind. The
//!    server keeps the request for two minutes and answers with an id, a
//!    secret for collecting the answer, and a four-digit match number the
//!    device shows. Only from a private address: the home network, or a
//!    client on one that a proxy listed in `trusted_proxies` names (a
//!    tailnet's, through `tailscale serve`). Through a proxy that isn't
//!    listed nobody may ask, since anyone could be behind it (Tailscale
//!    Funnel, a proxy facing the internet). One waiting request per address,
//!    two per IPv6 /64 and three at most.
//! 2. The computer shows it: every UpNext window or browser on the computer
//!    itself asks "A phone called "Pixel 9" wants to use UpNext. It shows
//!    4821." with Allow and Deny (`GET` and `POST /api/pairing/requests`,
//!    local-only like making codes). Without a screen, `upnext pair
//!    requests` and `upnext pair approve 4821`, and a line in the log.
//! 3. The device collects the answer: `GET /api/pair/request/{id}` with the
//!    secret in `UpNext-Pair-Secret`, so no other device can collect a
//!    token meant for it. Allowed, a browser or WebView gets the same
//!    paired-browser cookie as pairing with a code, and the app window or
//!    the Android app a token it keeps. Three denials from one address
//!    make it wait ten minutes.
//!
//! Nothing is allowed without a person at the computer, and the match
//! number keeps them from allowing a neighbour's request by mistake. The
//! request and its answer need no token, like `POST /api/pair`.

use super::access::{self, Edge, Https};
use super::pairing;
use super::{ApiError, AppState};
use crate::db::{PlayerId, Scope, now};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::{Arc, Mutex, OnceLock};

/// How long a request waits for an answer, in seconds.
pub const REQUEST_TTL: i64 = 120;
/// How long an allowed request waits to be collected, at least.
const COLLECT_TTL: i64 = 60;
/// Requests waiting at once, from every address.
pub const MAX_WAITING: usize = 3;
/// Requests waiting at once from one IPv6 /64 (`network`).
pub const MAX_PER_NETWORK: usize = 2;
/// Denials from one address before it has to wait.
pub const MAX_DENIALS: u8 = 3;
/// How long it waits then, in seconds.
pub const DENIED_WAIT: i64 = 600;
/// Seconds before an address may ask again: a device that asks over and
/// over (a loop, or someone at a keyboard) can't keep replacing its
/// request, and the window is asked for attention at most this often.
pub const ASK_EVERY: i64 = 10;
/// The header a device collects its answer with.
pub const SECRET_HEADER: &str = "upnext-pair-secret";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/pair/request", post(ask))
        .route("/pair/request/{id}", get(collect))
        .route("/pair/open", get(open))
        .route("/pairing/requests", get(waiting))
        .route("/pairing/requests/{id}", post(answer))
}

/// What kind of device asks: how the computer's dialog names it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Phone,
    Tablet,
    Tv,
    Computer,
}

impl DeviceKind {
    /// "a phone", for the log and the command line.
    pub fn with_article(self) -> &'static str {
        match self {
            Self::Phone => "a phone",
            Self::Tablet => "a tablet",
            Self::Tv => "a TV",
            Self::Computer => "a computer",
        }
    }

    /// From the browser, when the device didn't say.
    fn from_browser(headers: &HeaderMap) -> Self {
        let ua = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or_default();
        if ua.contains("iPad") || (ua.contains("Android") && !ua.contains("Mobile")) {
            Self::Tablet
        } else if ua.contains("iPhone") || ua.contains("Android") {
            Self::Phone
        } else {
            Self::Computer
        }
    }
}

/// How the device takes its access once allowed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Wants {
    /// A browser or WebView: the paired-browser cookie, as with a code.
    #[default]
    Cookie,
    /// The app window or the Android app: the token itself, to keep.
    Token,
}

#[derive(Debug, Clone, PartialEq)]
enum Answer {
    Waiting,
    /// With whose device it is: None for one shared by the household.
    Allowed(Option<PlayerId>),
    Denied,
}

struct Request {
    id: String,
    secret: String,
    match_code: String,
    name: String,
    kind: DeviceKind,
    wants: Wants,
    from: IpAddr,
    /// The listed proxy it came through, if it did.
    via: Option<IpAddr>,
    /// The network it asked from, for the flood limits (`network`).
    network: IpAddr,
    expires_at: i64,
    answer: Answer,
}

#[derive(Default)]
struct Inner {
    requests: Vec<Request>,
    /// Denials from each address, and until when it waits once it had too many.
    denials: HashMap<IpAddr, (u8, i64)>,
    /// When each network (`network`) last asked, while that's under `ASK_EVERY` ago.
    asked: HashMap<IpAddr, i64>,
    /// When the window was last asked for attention.
    attended: i64,
}

/// Run when a device asks, by the app window: it gets the person's
/// attention (`desktop`).
pub type OnRequest = Arc<dyn Fn() + Send + Sync>;

/// The requests waiting for an answer.
#[derive(Default)]
pub struct Requests {
    inner: Mutex<Inner>,
    on_request: OnceLock<OnRequest>,
}

impl Requests {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let t = now();
        inner.requests.retain(|r| r.expires_at > t);
        inner.denials.retain(|_, (count, until)| *count > 0 || *until > t);
        inner.asked.retain(|_, at| t - *at < ASK_EVERY);
        inner
    }

    /// What the app window does when a device asks.
    #[cfg_attr(not(feature = "desktop"), allow(dead_code))]
    pub fn on_request(&self, f: OnRequest) {
        let _ = self.on_request.set(f);
    }

    /// Two minutes gone by, for the tests.
    #[cfg(test)]
    pub fn expire(&self) {
        let mut inner = self.inner.lock().unwrap();
        for r in &mut inner.requests {
            r.expires_at = now() - 1;
        }
        inner.asked.clear();
        inner.attended = 0;
    }

    /// `ASK_EVERY` gone by, for the tests: an address may ask again.
    #[cfg(test)]
    pub fn a_moment_later(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.asked.clear();
        inner.attended = 0;
    }
}

fn random_hex(bytes: usize) -> anyhow::Result<String> {
    let mut b = vec![0u8; bytes];
    getrandom::fill(&mut b).map_err(|e| anyhow::anyhow!("no randomness for a pairing request: {e}"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// Four digits no other waiting request shows.
fn match_code(taken: &[String]) -> anyhow::Result<String> {
    loop {
        let mut b = [0u8; 2];
        getrandom::fill(&mut b).map_err(|e| anyhow::anyhow!("no randomness for a match number: {e}"))?;
        let n = u16::from_le_bytes(b);
        // Below the last whole ten thousand, so every number is as likely.
        if n >= u16::MAX - u16::MAX % 10_000 {
            continue;
        }
        let code = format!("{:04}", n % 10_000);
        if !taken.contains(&code) {
            return Ok(code);
        }
    }
}

#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
pub struct AskBody {
    /// What to call the device, such as "Pixel 9"; worked out from the
    /// browser when left out ("Android phone").
    #[serde(default)]
    name: Option<String>,
    /// phone, tablet, tv or computer; from the browser when left out.
    #[serde(default)]
    kind: Option<DeviceKind>,
    /// `cookie` (the default) for a browser or WebView, `token` for an app
    /// that keeps the token itself.
    #[serde(default)]
    wants: Option<Wants>,
}

/// The request, as the device that made it sees it.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Asked {
    pub id: String,
    /// Sent back in `UpNext-Pair-Secret` to collect the answer.
    pub secret: String,
    /// The four digits the device shows, which the computer's dialog shows too.
    pub match_code: String,
    /// When it stops waiting (Unix seconds).
    pub expires_at: i64,
    /// The computer's name on the network, for "Allow it on den".
    pub computer: String,
    /// Its install id, as it announces it: an app that keeps the computer
    /// finds it again by this after its address changes.
    pub computer_id: String,
}

/// Where a request came from, as the flood limits count it: an IPv4
/// address as it is, an IPv6 address by its /64. A device picks new
/// addresses in its /64 as it likes (privacy addresses), so counted one by
/// one it could fill every waiting place and ask again and again. Every
/// device on a home network shares that /64 too, so the request itself,
/// what replaces it and the denials stay with the full address: two phones
/// never replace each other's request or inherit each other's denials.
pub(super) fn network(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => ip,
        IpAddr::V6(v6) => IpAddr::V6(Ipv6Addr::from(u128::from(v6) & !(u128::from(u64::MAX)))),
    }
}

/// Who is asking, when that's someone on the home network (B-AUTH-2): the
/// client a listed proxy names, else the peer (this computer in a router
/// test with none), and the listed proxy it came through. Through a proxy
/// that isn't listed, the peer is the proxy's address, often loopback, and
/// the asker could be anyone the proxy lets in, so nobody may ask that way.
fn asker(edge: Option<Extension<Edge>>) -> Result<(IpAddr, Option<IpAddr>), ApiError> {
    let Some(Extension(edge)) = edge else { return Ok((IpAddr::V4(Ipv4Addr::LOCALHOST), None)) };
    if edge.proxied.is_some() && edge.listed.is_none() {
        return Err(ApiError::forbidden(THROUGH_A_PROXY));
    }
    if !edge.private {
        return Err(ApiError::forbidden(FROM_AFAR));
    }
    let from = edge.client().unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    Ok((from, edge.listed.map(|l| l.proxy)))
}

const FROM_AFAR: &str = "devices ask to be allowed only from the home network: from elsewhere, pair with a code \
    (Settings › Devices and API, Add a device, on the computer UpNext runs on)";

const THROUGH_A_PROXY: &str = "this request came through a proxy that isn't in trusted_proxies, so UpNext can't tell \
    who is asking: pair with a code (Settings › Devices and API, Add a device, on the computer UpNext runs on), or list \
    the proxy there so its home-network clients can ask";

fn too_many(m: String, secs: i64) -> Response {
    let mut res = ApiError(StatusCode::TOO_MANY_REQUESTS, m).into_response();
    res.headers_mut().insert(header::RETRY_AFTER, HeaderValue::from(secs.max(1)));
    res
}

/// A device asks to be allowed.
#[utoipa::path(post, path = "/pair/request", tag = "devices", security(()),
    description = "Ask to use UpNext: the computer it runs on shows the request, with the match number, until someone \
        allows or denies it there (two minutes). Needs no token. Only from a private address: the client's, as a proxy \
        listed in trusted_proxies names it; never through a proxy that isn't listed. One waiting request per \
        address (a new one replaces it), two per IPv6 /64, and three at most; one ask every ten seconds per address or /64; after three denials an address waits ten minutes.",
    request_body = AskBody,
    responses((status = 201, body = Asked), (status = 403, description = "From a public address, or through a proxy that isn't listed"),
        (status = 429, description = "Three devices are already waiting (two from this IPv6 /64), a device here asked under ten seconds ago, or this address was denied three times; Retry-After says when to ask again")))]
async fn ask(State(state): State<AppState>, edge: Option<Extension<Edge>>, headers: HeaderMap, body: Option<Json<AskBody>>) -> Response {
    let (from, via) = match asker(edge) {
        Ok(a) => a,
        Err(e) => return e.into_response(),
    };
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let name = body.name.as_deref().map(crate::db::clean_name).filter(|n| !n.is_empty()).unwrap_or_else(|| pairing::device_name(&headers));
    let name: String = name.chars().take(50).collect();
    let kind = body.kind.unwrap_or_else(|| DeviceKind::from_browser(&headers));
    let net = network(from);
    let made = {
        let mut inner = state.remote.approvals.lock();
        let t = now();
        if let Some((_, until)) = inner.denials.get(&from).filter(|(_, until)| *until > t) {
            return too_many("this device was denied three times: ask again in a few minutes, or pair with a code".into(), until - t);
        }
        if let Some(at) = inner.asked.get(&net) {
            return too_many("a device here asked a moment ago: wait a few seconds before asking again".into(), ASK_EVERY - (t - at));
        }
        // A device asking again (its app restarted) replaces its request,
        // which the window already asked about. The total stays at
        // `MAX_WAITING` whatever the addresses.
        let replacing = inner.requests.iter().any(|r| r.from == from && r.answer == Answer::Waiting);
        inner.requests.retain(|r| r.from != from);
        if inner.requests.iter().filter(|r| r.network == net).count() >= MAX_PER_NETWORK {
            return too_many(
                format!(
                    "{MAX_PER_NETWORK} devices on this network are already waiting to be allowed: answer them on the computer UpNext runs on first"
                ),
                30,
            );
        }
        if inner.requests.len() >= MAX_WAITING {
            return too_many(
                format!("{MAX_WAITING} devices are already waiting to be allowed: answer them on the computer UpNext runs on first"),
                30,
            );
        }
        let taken: Vec<String> = inner.requests.iter().map(|r| r.match_code.clone()).collect();
        let made = (|| -> anyhow::Result<Request> {
            Ok(Request {
                id: random_hex(8)?,
                secret: random_hex(24)?,
                match_code: match_code(&taken)?,
                name,
                kind,
                wants: body.wants.unwrap_or_default(),
                from,
                via,
                network: net,
                expires_at: t + REQUEST_TTL,
                answer: Answer::Waiting,
            })
        })();
        let made = match made {
            Ok(r) => r,
            Err(e) => return ApiError::from(e).into_response(),
        };
        let asked = Asked {
            id: made.id.clone(),
            secret: made.secret.clone(),
            match_code: made.match_code.clone(),
            expires_at: made.expires_at,
            computer: super::network::network_name(&state.cfg()),
            computer_id: match super::network::network_id(&state) {
                Ok(id) => id,
                Err(e) => return ApiError::from(e).into_response(),
            },
        };
        // To standard output, as a container's first pairing code is, so
        // `docker logs` and a service's journal show it whatever the log level.
        println!("{}", ask_line(&state, &made));
        inner.requests.push(made);
        inner.asked.insert(net, t);
        // The window's attention once in a while, however many ask: the
        // dialog shows each of them anyway.
        let attend = !replacing && t - inner.attended >= ASK_EVERY;
        if attend {
            inner.attended = t;
        }
        (asked, attend)
    };
    let (made, attend) = made;
    if let Some(f) = state.remote.approvals.on_request.get().filter(|_| attend) {
        f();
    }
    (StatusCode::CREATED, Json(made)).into_response()
}

/// The log's line for a request: what it is, and how to answer it without
/// a screen (a container's, from the host).
fn ask_line(state: &AppState, r: &Request) -> String {
    let cmd = if state.base_cfg.container { "docker exec upnext upnext pair" } else { "upnext pair" };
    format!(
        "{} called {:?} wants to use UpNext from {}; it shows {}. Allow it in UpNext on this computer, or with `{cmd} approve {}` (`{cmd} deny {}` to refuse).",
        capitalized(r.kind.with_article()),
        r.name,
        where_from(r),
        r.match_code,
        r.match_code,
        r.match_code
    )
}

/// "192.168.0.42", or "100.64.0.7 through the proxy at 127.0.0.1".
fn where_from(r: &Request) -> String {
    match r.via {
        Some(proxy) => format!("{} through the proxy at {proxy}", r.from),
        None => r.from.to_string(),
    }
}

fn capitalized(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnswerStatus {
    Waiting,
    Allowed,
    Denied,
}

/// The computer's answer, as the device collects it.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Collected {
    pub status: AnswerStatus,
    /// Allowed: the device's entry in the token list.
    pub id: Option<i64>,
    pub name: Option<String>,
    /// Allowed, for a request that wanted one: the token, the only time it's seen.
    pub token: Option<String>,
    /// Allowed: the address to use from now on when the computer names one
    /// (`public_url`): a token works only at that name then.
    pub address: Option<String>,
}

impl Collected {
    fn bare(status: AnswerStatus) -> Self {
        Self { status, id: None, name: None, token: None, address: None }
    }
}

/// The device collects the answer.
#[utoipa::path(get, path = "/pair/request/{id}", tag = "devices", security(()),
    description = "The computer's answer to a request, collected with its secret in the UpNext-Pair-Secret header. \
        Allowed, a request that wanted a cookie gets the paired-browser cookie (as POST /pair does), and one that \
        wanted a token gets it in the answer; either way the request is then gone. Denied, it's gone too.",
    params(("id" = String, Path, description = "The request's id"),
        ("UpNext-Pair-Secret" = String, Header, description = "The secret the request was answered with")),
    responses((status = 200, body = Collected),
        (status = 403, description = "From a public address, or through a proxy that isn't listed"),
        (status = 404, description = "No such request, a secret that doesn't match, or it expired")))]
async fn collect(
    State(state): State<AppState>,
    Path(id): Path<String>,
    edge: Option<Extension<Edge>>,
    https: Option<Extension<Https>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    asker(edge)?;
    let secret = headers.get(SECRET_HEADER).and_then(|v| v.to_str().ok()).unwrap_or_default();
    let (answer, name, wants) = {
        let mut inner = state.remote.approvals.lock();
        let at = inner.requests.iter().position(|r| r.id == id && pairing::constant_time_eq(r.secret.as_bytes(), secret.as_bytes()));
        let Some(at) = at else {
            return Err(ApiError::not_found("no such request: it expired, or was already answered and collected; ask again"));
        };
        let r = &inner.requests[at];
        if r.answer == Answer::Waiting {
            return Ok(Json(Collected::bare(AnswerStatus::Waiting)).into_response());
        }
        let r = inner.requests.remove(at);
        (r.answer, r.name, r.wants)
    };
    let player = match answer {
        Answer::Allowed(player) => player,
        _ => return Ok(Json(Collected::bare(AnswerStatus::Denied)).into_response()),
    };
    let name = pairing::free_name(&state, &name)?;
    let made = state.db.token_create_for(&name, Scope::Full, "device", player)?;
    tracing::info!("allowed a new device: {name}");
    let address = access::public_url(&state).map(|p| p.origin());
    let token = (wants == Wants::Token).then(|| made.token.clone());
    let mut res =
        Json(Collected { status: AnswerStatus::Allowed, id: Some(made.info.id), name: Some(name), token, address }).into_response();
    if wants == Wants::Cookie {
        let secure = https.is_some_and(|Extension(Https(on))| on);
        res.headers_mut().insert(header::SET_COOKIE, pairing::cookie(&state, &made.token, secure));
    }
    Ok(res)
}

/// Open the UI with a token: the app window on another computer loads this
/// first, with its token in `Authorization`, and from then on the page's
/// own requests carry it as the paired-browser cookie. A webview takes a
/// header only for the first page it loads.
#[utoipa::path(get, path = "/pair/open", tag = "devices",
    description = "Set the paired-browser cookie from the token in the Authorization header, then go to the UI (303 to /). \
        What the app window on another computer opens first, since a webview sends a header only with the first page. \
        On this computer, with no token, it only goes to the UI.",
    responses((status = 303, description = "To the UI, with the cookie set")))]
async fn open(State(state): State<AppState>, https: Option<Extension<Https>>, headers: HeaderMap) -> Response {
    let mut res = (StatusCode::SEE_OTHER, [(header::LOCATION, "/")]).into_response();
    // The guard let the request through, so a token it carries is valid here.
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .map(|(_, t)| t.trim());
    if let Some(token) = token {
        let secure = https.is_some_and(|Extension(Https(on))| on);
        res.headers_mut().insert(header::SET_COOKIE, pairing::cookie(&state, token, secure));
    }
    res
}

/// A request, as the computer's dialog shows it.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct WaitingDevice {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub match_code: String,
    /// The address it asked from: the client a listed proxy names, when it
    /// came through one.
    pub from: String,
    /// The listed proxy it came through, if it did.
    pub via: Option<String>,
    pub expires_at: i64,
}

/// A player the dialog offers as the device's owner.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Owner {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Waiting {
    /// Oldest first.
    pub requests: Vec<WaitingDevice>,
    /// Whose device it can be, with two or more players; the
    /// first is the one chosen unless another is. Empty with one player:
    /// the device is then everyone's, as a paired one is.
    pub players: Vec<Owner>,
}

/// The requests waiting, for the computer's dialog.
#[utoipa::path(get, path = "/pairing/requests", tag = "devices", operation_id = "pairing_requests",
    description = "Devices waiting to be allowed, oldest first, with the players one can belong to. Only on the computer UpNext runs on.",
    responses((status = 200, body = Waiting), (status = 403, description = "With a token")))]
async fn waiting(State(state): State<AppState>) -> Result<Json<Waiting>, ApiError> {
    let requests = state
        .remote
        .approvals
        .lock()
        .requests
        .iter()
        .filter(|r| r.answer == Answer::Waiting)
        .map(|r| WaitingDevice {
            id: r.id.clone(),
            name: r.name.clone(),
            kind: r.kind,
            match_code: r.match_code.clone(),
            from: r.from.to_string(),
            via: r.via.map(|v| v.to_string()),
            expires_at: r.expires_at,
        })
        .collect();
    let players = state.db.players()?;
    let players = if players.len() < 2 { vec![] } else { players.into_iter().map(|p| Owner { id: p.id.0, name: p.name }).collect() };
    Ok(Json(Waiting { requests, players }))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AnswerBody {
    allow: bool,
    /// Whose device it is, with two or more players: one of `players`, or
    /// null for one the household shares.
    #[serde(default)]
    player_id: Option<i64>,
    /// That player's PIN, when they have one.
    #[serde(default)]
    pin: Option<String>,
}

/// Allow or deny a request.
#[utoipa::path(post, path = "/pairing/requests/{id}", tag = "devices", operation_id = "pairing_request_answer",
    description = "Allow or deny a waiting device. Allowed, it gets a device token of full scope when it collects the \
        answer, bound to `player_id` when one is given (with `pin` when that player has one). Three denials from one address make it wait ten minutes. Only on \
        the computer UpNext runs on.",
    params(("id" = String, Path, description = "The request's id")),
    request_body = AnswerBody,
    responses((status = 204), (status = 400, description = "No such player"),
        (status = 403, description = "With a token, or the player has a PIN and it's missing or wrong"),
        (status = 429, description = "Too many wrong PINs for that player: try again later"),
        (status = 404, description = "No such request waiting: it expired or was answered")))]
async fn answer(State(state): State<AppState>, Path(id): Path<String>, Json(body): Json<AnswerBody>) -> Result<StatusCode, ApiError> {
    let player = match body.player_id {
        Some(p) if body.allow => {
            if !state.db.players()?.iter().any(|x| x.id.0 == p) {
                return Err(ApiError::bad(format!("there's no player {p}")));
            }
            Some(PlayerId(p))
        }
        _ => None,
    };
    super::players::binding_allowed(&state, player, body.pin.as_deref())?;
    let mut inner = state.remote.approvals.lock();
    let Some(r) = inner.requests.iter_mut().find(|r| r.id == id && r.answer == Answer::Waiting) else {
        return Err(ApiError::not_found("that device isn't waiting any more: it gave up, or was answered already"));
    };
    if body.allow {
        r.answer = Answer::Allowed(player);
        r.expires_at = r.expires_at.max(now() + COLLECT_TTL);
        tracing::info!("allowed {:?} ({}); it collects its access next", r.name, r.match_code);
    } else {
        r.answer = Answer::Denied;
        let from = r.from;
        tracing::info!("denied {:?} ({})", r.name, r.match_code);
        let (count, until) = inner.denials.entry(from).or_insert((0, 0));
        *count += 1;
        if *count >= MAX_DENIALS {
            *count = 0;
            *until = now() + DENIED_WAIT;
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flood_limits_count_an_ipv6_address_by_its_64() {
        let v4: IpAddr = [192, 168, 0, 42].into();
        assert_eq!(network(v4), v4);
        let a: IpAddr = "fd12:3456:789a:1::1".parse().unwrap();
        let b: IpAddr = "fd12:3456:789a:1:abcd:ef01:2345:6789".parse().unwrap();
        let other: IpAddr = "fd12:3456:789a:2::1".parse().unwrap();
        assert_eq!(network(a), "fd12:3456:789a:1::".parse::<IpAddr>().unwrap());
        assert_eq!(network(a), network(b));
        assert_ne!(network(a), network(other));
    }

    #[test]
    fn match_numbers_are_four_digits_and_never_the_same_twice() {
        let mut taken = vec![];
        for _ in 0..200 {
            let c = match_code(&taken).unwrap();
            assert_eq!(c.len(), 4);
            assert!(c.chars().all(|ch| ch.is_ascii_digit()), "{c}");
            assert!(!taken.contains(&c));
            taken.push(c);
        }
    }

    #[test]
    fn kinds_from_browsers() {
        let kind = |ua: &str| {
            let mut h = HeaderMap::new();
            h.insert(header::USER_AGENT, HeaderValue::from_str(ua).unwrap());
            DeviceKind::from_browser(&h)
        };
        assert_eq!(kind("Mozilla/5.0 (Linux; Android 15; Pixel 9) Mobile Safari/537.36"), DeviceKind::Phone);
        assert_eq!(kind("Mozilla/5.0 (Linux; Android 15; Pixel Tablet) Safari/537.36"), DeviceKind::Tablet);
        assert_eq!(kind("Mozilla/5.0 (X11; Linux x86_64)"), DeviceKind::Computer);
        assert_eq!(capitalized(DeviceKind::Tv.with_article()), "A TV");
    }
}
