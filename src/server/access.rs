//! Who may use the API. This computer needs nothing: a request from
//! a loopback peer address, addressed to a loopback name, works as it always
//! has. Anything else needs a token: `Authorization: Bearer upnext_…`, or a
//! paired browser's cookie (`pairing`). That's another device, a script
//! elsewhere, a proxy in front, a container's host. Over plain HTTP, a token
//! is taken only from a private address (`listen::is_private`).
//!
//! A proxy on this computer connects from loopback, and one that rewrites
//! `Host` to its upstream's address (nginx's default) would make every
//! visitor this computer. So a request carrying a proxy's headers
//! (`PROXY_HEADERS`), or speaking HTTP/1.0 as nginx does by default, is
//! never this computer's, whatever its `Host`. A proxy that speaks HTTP/1.1
//! and sends none of those headers can't be told from a local browser.
//!
//! What needs no token from anyone who can reach the server: the UI's own
//! files (a new device pairs on them), `GET /api/health`, `POST /api/pair`
//! and Steam's sign-in callback, which checks its own one-time state.
//!
//! The older guards against other websites in the same browser stay:
//! - DNS rebinding: only API requests addressed to a loopback name or to the
//!   `public_url` setting are answered (evil.example resolving to 127.0.0.1
//!   sends `Host: evil.example`). With no `public_url` and the server
//!   listening beyond loopback, a request with a valid token may name any host.
//! - Cross-site writes: a POST, PUT or DELETE must come from a page of
//!   UpNext's own (`Origin`, on loopback the same port too), and not one the
//!   browser flags as cross-site. With a paired browser's cookie, a write
//!   must say where it came from.
//! - Cross-site reads: an API request the browser flags as coming from
//!   another site (`Sec-Fetch-Site: cross-site` or `same-site`), or whose
//!   `Origin` isn't UpNext's own, a GET included, is refused, so another
//!   page can't make the server act with an image, a link or a fetch.
//!   Steam's way back from its sign-in is the exception.
//!
//! - Framing: no page may show UpNext in a frame, where it would work as
//!   this computer and a click could be stolen (`security_headers`).
//!
//! Trusted proxies (`proxies`): from an address listed
//! in `trusted_proxies`, the forwarding headers are believed for the
//! client's address, HTTPS and the host, and with "The proxy does the
//! login" on, a request needs no token and is a [`Caller::Proxy`]. From any
//! other address they mean what they meant before: another device, which
//! needs a token. A request with no forwarding headers is judged as before.
//!
//! Then the per-token limits (`limits`).

use super::AppState;
use super::limits::{AI_RETRY_AFTER, AiCall, Key};
use super::listen::{Peer, is_private};
use super::listen::{host_name, is_loopback_host};
use super::pairing;
use super::players;
use super::proxies;
use crate::config::PublicUrl;
use crate::db::PlayerId;
use crate::db::{ApiToken, Scope};
use axum::Json;
use axum::extract::connect_info::MockConnectInfo;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderValue, Method, StatusCode, Version, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use std::net::{IpAddr, SocketAddr};

/// Who's asking, in each request's extensions for the handlers that answer
/// differently (`Extension<Caller>`).
#[derive(Debug, Clone)]
pub enum Caller {
    /// This computer, without a token.
    Local,
    Token(ApiToken),
    /// Through a listed proxy with "The proxy does the login" on,
    /// without a token: the client's address as the proxy gives
    /// it. It chooses its player as a browser on this computer does, with
    /// PINs, but can do nothing that only this computer may do, and no
    /// more than a full token may.
    Proxy(IpAddr),
    /// Anyone who can reach the server, for what needs no token: the UI's
    /// files (the page a new device pairs on), `/api/health` and `/api/pair`.
    Anyone,
}

impl Caller {
    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local)
    }
}

/// The request came over HTTPS, here or through a proxy at an `https://`
/// `public_url` (in each request's extensions, beside the [`Caller`]).
#[derive(Debug, Clone, Copy)]
pub struct Https(pub bool);

/// The client a listed proxy forwarded the request for, in the answer's
/// extensions for its log line (`request_log`).
#[derive(Debug, Clone, Copy)]
pub struct ForwardedFor(pub IpAddr);

/// Where a request came from, worked out once, first, by the guard (R-AUTH-2):
/// the connection, a proxy's marks, what a listed proxy says about the
/// client, HTTPS and the host. In each request's extensions for the
/// handlers that answer by address (`approval`, the sign-in's limits).
#[derive(Debug, Clone)]
pub struct Edge {
    /// The connection's peer: the proxy's, when there is one in front. None
    /// in a router test that sets none.
    pub peer: Option<Peer>,
    /// What marks the request as a proxy's, if anything: one of its
    /// headers, or HTTP/1.0 (`proxy_sign`).
    pub proxied: Option<&'static str>,
    /// Through a proxy listed in `trusted_proxies`, with a proxy's marks.
    pub listed: Option<Listed>,
    /// The client is on a private network: the peer, or the client a listed
    /// proxy names. A client the proxy can't name is a public one.
    pub private: bool,
    /// It came over TLS: to this server, or to a listed proxy as it says.
    tls: bool,
    /// Over HTTPS: TLS, or addressed to an `https://` `public_url`.
    https: bool,
    /// The host it's addressed to, as a listed proxy says.
    host: String,
    /// The peer is loopback (with none, whether the server listens on loopback only).
    local_peer: bool,
    /// This computer: a loopback peer naming a loopback host, with no proxy's marks.
    here: bool,
    /// The server listens on loopback only.
    loopback_only: bool,
    public: Option<PublicUrl>,
    /// `host` names the `public_url`.
    public_host: bool,
}

/// What a proxy listed in `trusted_proxies` says about a request.
#[derive(Debug, Clone, Copy)]
pub struct Listed {
    pub proxy: IpAddr,
    /// The client it names; None when it can't name one.
    pub client: Option<IpAddr>,
    /// "The proxy does the login" is on.
    pub login: bool,
}

impl Edge {
    /// The client's address as far as it's known: the one a listed proxy
    /// names, else the peer's.
    pub fn client(&self) -> Option<IpAddr> {
        self.listed.and_then(|l| l.client).or(self.peer.map(|p| p.addr.ip().to_canonical()))
    }

    /// Where the request came from. A listed proxy that says two different
    /// things (its two kinds of headers disagree) is refused.
    fn of(state: &AppState, req: &Request, peer: Option<Peer>) -> Result<Self, Refusal> {
        let header = |name: header::HeaderName| req.headers().get(name).and_then(|v| v.to_str().ok());
        let loopback_only = !state.listening().beyond_loopback();
        let local_peer = peer.map_or(loopback_only, |p| p.addr.ip().to_canonical().is_loopback());
        let mut private = peer.is_none_or(|p| is_private(p.addr.ip()));
        let mut tls = peer.is_some_and(|p| p.tls);
        let mut host = header(header::HOST).or_else(|| req.uri().authority().map(|a| a.as_str())).unwrap_or_default().to_string();
        // This computer: a loopback peer naming a loopback host, not through a proxy.
        let proxied = proxy_sign(req);
        let here = local_peer && is_loopback_host(host_name(&host)) && proxied.is_none();

        // A listed proxy: what it says about the client,
        // HTTPS and the host is believed. A request without a proxy's headers,
        // this computer's own among them, is judged exactly as before.
        let mut listed = None;
        if proxied.is_some()
            && let Some(proxy) = peer.map(|p| p.addr.ip().to_canonical())
        {
            let (list, login) = proxies::trusted(state);
            if list.contains(proxy) {
                let Ok(said) = proxies::read(req.headers(), &list) else {
                    return Err(Refusal::new(StatusCode::BAD_REQUEST, DISAGREE));
                };
                if let Some(h) = said.host {
                    host = h;
                }
                tls = said.https.unwrap_or(tls);
                // A client the proxy can't name is a public one, never the proxy.
                private = said.client.is_some_and(is_private);
                listed = Some(Listed { proxy, client: said.client, login });
            }
        }
        let loopback_host = is_loopback_host(host_name(&host));
        let public = public_url(state);
        let public_host = public.as_ref().is_some_and(|p| names_public(p, &host));
        let https = tls || (public_host && public.as_ref().is_some_and(|p| p.https));
        if let Some(h) = proxied
            && local_peer
            && loopback_host
            && listed.is_none()
        {
            warn_proxy_rewrites_host(h);
        }
        Ok(Self { peer, proxied, listed, private, tls, https, host, local_peer, here, loopback_only, public, public_host })
    }
}

/// The `public_url` setting: the app's value, else the environment's.
pub fn public_url(state: &AppState) -> Option<PublicUrl> {
    let saved = state.db.setting_get(PlayerId::FIRST, "public_url").ok().flatten();
    saved.or_else(|| state.base_cfg.public_url.clone()).as_deref().and_then(PublicUrl::parse)
}

/// Whether a `Host` header names the `public_url`'s host and port.
pub fn names_public(public: &PublicUrl, host: &str) -> bool {
    let scheme = if public.https { "https" } else { "http" };
    PublicUrl::parse(&format!("{scheme}://{host}")).is_some_and(|h| h.authority == public.authority)
}

/// The connection's peer: the listener's [`Peer`], or what a router test
/// set with `MockConnectInfo` (a `Peer` or a bare address). None without.
pub fn peer(parts: &Parts) -> Option<Peer> {
    if let Some(ConnectInfo(p)) = parts.extensions.get::<ConnectInfo<Peer>>() {
        return Some(*p);
    }
    if let Some(ConnectInfo(a)) = parts.extensions.get::<ConnectInfo<SocketAddr>>() {
        return Some(Peer::plain(*a));
    }
    let mock = |m: &MockConnectInfo<SocketAddr>| Peer::plain(m.0);
    parts.extensions.get::<MockConnectInfo<Peer>>().map(|m| m.0).or_else(|| parts.extensions.get::<MockConnectInfo<SocketAddr>>().map(mock))
}

/// A path with the version taken out: `/api/v1/games` is `/api/games`.
pub fn unversioned(path: &str) -> std::borrow::Cow<'_, str> {
    match path.strip_prefix("/api/v1") {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("/api{rest}").into(),
        _ => path.into(),
    }
}

pub async fn guard(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let (parts, body) = req.into_parts();
    let peer = peer(&parts);
    let mut req = Request::from_parts(parts, body);
    let path = unversioned(req.uri().path()).into_owned();
    let api = path == "/api" || path.starts_with("/api/");
    let edge = match Edge::of(&state, &req, peer) {
        Ok(e) => e,
        Err(refusal) => return refusal.into_response(api),
    };
    let forwarded_for = edge.listed.and_then(|l| l.client);
    let caller = match caller(&state, &req, &path, &edge) {
        Ok(c) => c,
        Err(refusal) => return refusal.into_response(api).with_client(forwarded_for),
    };
    // Then whose request it is, for the API.
    let mut clear_player = false;
    if api && !matches!(caller, Caller::Anyone) {
        match players::resolve(&state, req.headers(), &caller) {
            Ok(r) => {
                if r.who.bound
                    && let Some(why) = players::bound_refusal(req.method(), &path)
                {
                    return Refusal::new(StatusCode::FORBIDDEN, why).into_response(api);
                }
                clear_player = r.clear_cookie;
                req.extensions_mut().insert(r.who);
            }
            Err((status, why)) => return Refusal::new(status, why).into_response(api),
        }
    }
    // The limits, for tokens and for clients behind a proxy that does the
    // login: this computer has none.
    let mut _busy = None;
    let limited = match &caller {
        Caller::Token(t) => Some((Key::Token(t.id), "this token")),
        Caller::Proxy(client) => Some((Key::Address(*client), "this address")),
        Caller::Local | Caller::Anyone => None,
    };
    if let Some((key, who)) = limited {
        if let Err(wait) = state.remote.limits.request(key) {
            let secs = wait.as_secs_f64().ceil().max(1.0) as u64;
            return Refusal::new(StatusCode::TOO_MANY_REQUESTS, format!("too many requests from {who}: about 10 a second at most"))
                .retry_after(secs)
                .into_response(api);
        }
        if let Some(call) = AiCall::of(req.method(), &path) {
            match state.remote.limits.ai_call(key, call) {
                Some(busy) => _busy = Some(busy),
                None => {
                    let m = format!("{who} already has {} running: one at a time", call.label());
                    return Refusal::new(StatusCode::TOO_MANY_REQUESTS, m).retry_after(AI_RETRY_AFTER).into_response(api);
                }
            }
        }
    }
    req.extensions_mut().insert(caller);
    req.extensions_mut().insert(Https(edge.https));
    req.extensions_mut().insert(edge);
    let mut res = next.run(req).await;
    // A player cookie that names nobody now: cleared, so the UI asks who's playing.
    if clear_player && !res.headers().contains_key(header::SET_COOKIE) {
        res.headers_mut().append(header::SET_COOKIE, players::cookie_cleared(&state));
    }
    res.with_client(forwarded_for)
}

/// The answer, with the client a listed proxy named for its log line.
trait WithClient {
    fn with_client(self, client: Option<IpAddr>) -> Self;
}

impl WithClient for Response {
    fn with_client(mut self, client: Option<IpAddr>) -> Self {
        if let Some(c) = client {
            self.extensions_mut().insert(ForwardedFor(c));
        }
        self
    }
}

/// Headers on every answer, pages, API and refusals alike:
/// - no framing (`X-Frame-Options` for older browsers, CSP `frame-ancestors`
///   for the rest): another site could load UpNext in a hidden frame, where
///   it works as this computer, and steer clicks onto Restore or Revoke. The
///   app window and other devices load it as the page itself.
/// - `X-Content-Type-Options: nosniff`: a file is only ever what its
///   `Content-Type` says.
/// - `Referrer-Policy: same-origin`: UpNext's addresses stay off other sites.
///   Not `no-referrer`: with it, browsers send `Origin: null` on the UI's own
///   writes, which the cross-site check refuses.
pub async fn security_headers(mut res: Response) -> Response {
    let headers = res.headers_mut();
    for (name, value) in [
        (header::X_FRAME_OPTIONS, "DENY"),
        (header::CONTENT_SECURITY_POLICY, "frame-ancestors 'none'"),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::REFERRER_POLICY, "same-origin"),
    ] {
        if !headers.contains_key(&name) {
            headers.insert(name, HeaderValue::from_static(value));
        }
    }
    res
}

/// A refusal: JSON for the API, plain text for a page.
struct Refusal {
    status: StatusCode,
    message: String,
    retry_after: Option<u64>,
    /// A paired browser's cookie that no longer works, cleared, so its UI
    /// asks for a new code.
    clear_cookie: Option<HeaderValue>,
}

impl Refusal {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self { status, message: message.into(), retry_after: None, clear_cookie: None }
    }

    fn retry_after(mut self, secs: u64) -> Self {
        self.retry_after = Some(secs);
        self
    }

    fn into_response(self, api: bool) -> Response {
        let mut res = if api {
            (self.status, Json(json!({ "error": self.message }))).into_response()
        } else {
            (self.status, format!("UpNext: {}\n", self.message)).into_response()
        };
        let headers = res.headers_mut();
        if self.status == StatusCode::UNAUTHORIZED {
            headers.insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer realm=\"UpNext\""));
        }
        if let Some(secs) = self.retry_after {
            headers.insert(header::RETRY_AFTER, HeaderValue::from(secs));
        }
        if let Some(cleared) = self.clear_cookie {
            headers.insert(header::SET_COOKIE, cleared);
        }
        res
    }
}

const NEEDS_TOKEN: &str = "requests from another device need a token: pair this browser (Settings › Devices and API, \
    Add a device, on the computer UpNext runs on), or send Authorization: Bearer upnext_… (make one there, or with \
    `upnext token create`)";

/// In UpNext's container nobody is the computer it runs on.
const NEEDS_TOKEN_IN_CONTAINER: &str = "requests need a token: pair this browser with the code in the container's log \
    (`docker logs upnext`; `docker exec upnext upnext pair` makes a new one), or send Authorization: Bearer upnext_… \
    (`docker exec upnext upnext token create`)";

const PLAIN_HTTP: &str = "over plain HTTP from a public address, anyone on the way could copy the token: use https:// \
    (a certificate in Settings › Devices and API, or a proxy such as `tailscale serve`)";

/// What a request needs no token for: the health check, pairing, and
/// Steam's way back from its sign-in. Steam sends the browser back from its
/// own site, so a paired browser's cookie (`SameSite=Strict`) doesn't come
/// with it; the sign-in's one-time state, which only a caller who may
/// change settings was given, stands in for it (`steam_callback`). Signing
/// in through the identity provider too: it is a way
/// to pair, and its way back checks its own one-time state (`oidc`).
pub(super) fn open_to_anyone(method: &Method, path: &str) -> bool {
    // Asking to be allowed with a tap, and collecting the answer, which its
    // own secret guards (`approval`).
    let asking = (method == Method::POST && path == "/api/pair/request")
        || (method == Method::GET && path.strip_prefix("/api/pair/request/").is_some_and(|id| !id.is_empty() && !id.contains('/')));
    asking
        || matches!(
            (method, path),
            (&Method::GET | &Method::HEAD, "/api/health")
                | (&Method::POST, "/api/pair")
                | (&Method::GET, "/api/steam/callback")
                | (&Method::GET, "/api/oidc")
                | (&Method::GET, "/api/oidc/login")
                | (&Method::GET, "/api/oidc/callback")
        )
}

/// What hands a browser a token without one: pairing, collecting an
/// allowed request, and the identity provider's sign-in, which are kept
/// off plain HTTP from a public address.
fn hands_out_a_token(path: &str) -> bool {
    path == "/api/pair" || path.starts_with("/api/pair/request/") || path == "/api/oidc/login" || path == "/api/oidc/callback"
}

/// The pages the browser comes back to from another site's sign-in:
/// Steam's and the identity provider's. They check their own one-time
/// state, tied to the browser that started it.
fn way_back(path: &str) -> bool {
    path == "/api/steam/callback" || path == "/api/oidc/callback"
}

/// How a request carries its token.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Carried {
    Bearer,
    /// This server's paired-browser cookie (`pairing::cookie_name`).
    Cookie,
    /// The cookie by its name from before it took the port, which another
    /// UpNext on this host may have set.
    OldCookie,
}

/// A token the request carries: the `Authorization` header's, else a
/// paired browser's cookie, this server's own name before the old one.
fn credential<'a>(state: &AppState, req: &'a Request) -> Option<(&'a str, Carried)> {
    if let Some(t) = bearer(req) {
        return Some((t, Carried::Bearer));
    }
    let cookies = || {
        req.headers().get_all(header::COOKIE).iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(';')).filter_map(|pair| {
            let (k, v) = pair.trim().split_once('=')?;
            (!v.is_empty()).then_some((k, v))
        })
    };
    let ours = pairing::cookie_name(state);
    let named = |name: &str, carried| cookies().find(|(k, _)| *k == name).map(|(_, v)| (v, carried));
    named(&ours, Carried::Cookie).or_else(|| named(pairing::OLD_COOKIE, Carried::OldCookie))
}

/// The port a `Host` or an origin's authority names, the scheme's default
/// when it names none.
fn port_of(authority: &str, https: bool) -> Option<u16> {
    let name = host_name(authority);
    match authority[name.len()..].strip_prefix(':') {
        Some(p) => p.parse().ok(),
        None if authority.is_empty() => None,
        None => Some(if https { 443 } else { 80 }),
    }
}

/// Headers a proxy adds: nginx's `X-Real-IP` and `X-Forwarded-For` when
/// configured as most guides do, Apache, Caddy, Traefik and `tailscale serve`
/// at least `X-Forwarded-For` by default, and `Forwarded` and `Via` from the
/// standards. A browser or script on this computer sends none of them.
const PROXY_HEADERS: [&str; 6] = ["forwarded", "x-forwarded-for", "x-forwarded-host", "x-forwarded-proto", "x-real-ip", "via"];

/// What marks the request as a proxy's, if anything: one of its headers,
/// or HTTP/1.0. nginx talks HTTP/1.0 to its upstream unless told otherwise
/// (`proxy_http_version`), and a bare `proxy_pass` adds no header at all;
/// browsers, the app window and UpNext's own CLI speak HTTP/1.1 or newer.
fn proxy_sign(req: &Request) -> Option<&'static str> {
    if req.version() == Version::HTTP_10 {
        return Some("HTTP/1.0");
    }
    PROXY_HEADERS.into_iter().find(|h| req.headers().contains_key(*h))
}

/// Says once, loudly, that a proxy on this computer names a loopback host:
/// its visitors need tokens now, and the fix is in its configuration.
fn warn_proxy_rewrites_host(sign: &str) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let sign = if sign == "HTTP/1.0" { "spoke HTTP/1.0".to_string() } else { format!("carried {sign}") };
        tracing::warn!(
            "a request from this computer named a loopback host but {sign}: a proxy in front of UpNext is \
             rewriting Host to its upstream's address (nginx's default). Requests through it need a token like any \
             other device's. Pass the Host through (nginx: proxy_set_header Host $host;) and set public_url to its address."
        );
    });
}

const THROUGH_A_PROXY: &str = "this request came through a proxy (it carries a forwarding header, or speaks HTTP/1.0 as \
    nginx does to its upstream), so it needs a token \
    like any other device's; the proxy should also pass the Host header through (nginx: proxy_set_header Host $host;)";

const DISAGREE: &str = "this request's Forwarded and X-Forwarded-* headers say different things; \
    the proxy in front of UpNext should set one kind, and replace whatever a client sent of it";

/// Who's asking, given where the request came from.
fn caller(state: &AppState, req: &Request, path: &str, edge: &Edge) -> Result<Caller, Refusal> {
    let header = |name: header::HeaderName| req.headers().get(name).and_then(|v| v.to_str().ok());
    let Edge { proxied, private: private_peer, tls, here, local_peer, loopback_only, public_host, .. } = *edge;
    let public = edge.public.as_ref();
    let host = edge.host.as_str();
    let loopback_host = is_loopback_host(host_name(host));
    // Through a listed proxy: the client it names, else the proxy itself
    // (for the limits), and whether it does the login.
    let through_trusted = edge.listed.map(|l| (l.client.unwrap_or(l.proxy), l.login));

    // The UI's own files hold nothing private, and a new device needs them
    // to pair. The API's health check and pairing need no token either;
    // pairing, like a token, not over plain HTTP from a public address.
    let api = path == "/api" || path.starts_with("/api/");
    if !api || open_to_anyone(req.method(), path) {
        if hands_out_a_token(path) && !tls && !private_peer {
            return Err(Refusal::new(StatusCode::FORBIDDEN, PLAIN_HTTP));
        }
        if !api || !here {
            return Ok(Caller::Anyone);
        }
    }

    let (token, via_cookie) = match credential(state, req) {
        None => (None, false),
        Some((t, carried)) => match state.db.token_check(t) {
            Ok(Some(t)) => (Some(t), carried != Carried::Bearer),
            // Under its old name, possibly another UpNext's on this host:
            // left alone, as if it weren't there.
            Ok(None) if carried == Carried::OldCookie => (None, false),
            Ok(None) => {
                let mut r = Refusal::new(StatusCode::UNAUTHORIZED, "that token isn't valid here: it was revoked, or it's another UpNext's");
                if carried == Carried::Cookie {
                    r.message = "this browser's pairing was revoked: pair it again".into();
                    r.clear_cookie = Some(pairing::cookie_cleared(state));
                }
                return Err(r);
            }
            Err(e) => {
                tracing::error!("checking a token: {e:#}");
                return Err(Refusal::new(StatusCode::INTERNAL_SERVER_ERROR, "couldn't check the token"));
            }
        },
    };
    if token.is_some() && !tls && !private_peer {
        return Err(Refusal::new(StatusCode::FORBIDDEN, PLAIN_HTTP));
    }

    // Without a token: this computer, addressed by a loopback name. A
    // request from here to another name is DNS rebinding, or a proxy in
    // front, which needs a token like any other device; so does one with a
    // proxy's headers, whatever name it gives.
    // Through a listed proxy that does the login: no token, addressed to
    // this computer's name (the proxy rewrote `Host`) or the public one,
    // never another, which is DNS rebinding through the proxy's address.
    let proxy_login = match through_trusted {
        Some((client, true)) if token.is_none() && (loopback_host || public_host) => Some(client),
        _ => None,
    };
    if token.is_none() && !here && proxy_login.is_none() {
        let needs_token = if state.base_cfg.container { NEEDS_TOKEN_IN_CONTAINER } else { NEEDS_TOKEN };
        if through_trusted.is_some_and(|(_, login)| login) {
            return Err(Refusal::new(StatusCode::FORBIDDEN, host_refusal(public, true)));
        }
        return Err(if proxied.is_some() && loopback_host {
            Refusal::new(StatusCode::UNAUTHORIZED, format!("{THROUGH_A_PROXY}. {needs_token}"))
        } else if local_peer && !public_host {
            Refusal::new(StatusCode::FORBIDDEN, host_refusal(public, loopback_only))
        } else {
            Refusal::new(StatusCode::UNAUTHORIZED, needs_token)
        });
    }
    // With one: addressed to this computer, the public name, or with no
    // public name set, whatever its holder used to reach it.
    let inferred = public.is_none() && !loopback_only;
    if token.is_some() && !(loopback_host || public_host || inferred) {
        return Err(Refusal::new(StatusCode::FORBIDDEN, host_refusal(public, loopback_only)));
    }

    let cross_site = || Refusal::new(StatusCode::FORBIDDEN, "cross-site request refused");
    let fetch_site = header(header::HeaderName::from_static("sec-fetch-site"));
    // Another site's page can't make the server act with a GET either: an
    // image pointing at Steam's sign-in pushes out the one in progress, and
    // one at a fetching endpoint reaches networks only the server sees.
    // `same-site` too: another program's page on localhost:3000 is the same
    // site as localhost:8787. Steam's way back comes from its own site.
    let steam_back = way_back(path);
    if matches!(fetch_site, Some("cross-site" | "same-site")) && !steam_back {
        return Err(cross_site());
    }
    // A request that says it came from a page must come from one of
    // UpNext's own, whatever its method: a browser too old for
    // Sec-Fetch-Site still sends Origin on another site's fetch.
    if let Some(origin) = header(header::ORIGIN)
        && !steam_back
    {
        let (scheme, authority) = origin.split_once("://").unwrap_or(("", ""));
        // Same-origin, with the scheme checked where it's known. A page on
        // this computer must be UpNext's own port: another program's page
        // on localhost:3000 is another site.
        let loopback_origin = is_loopback_host(host_name(authority))
            && port_of(authority, scheme.eq_ignore_ascii_case("https")).is_some_and(|p| Some(p) == port_of(host, edge.https));
        let ours = loopback_origin
            || public.is_some_and(|p| origin.eq_ignore_ascii_case(&p.origin()))
            || (!public_host && !authority.is_empty() && !is_loopback_host(host_name(authority)) && authority.eq_ignore_ascii_case(host));
        if origin == "null" || !ours {
            return Err(cross_site());
        }
    }
    // A browser's cookie rides along on any request to here, so a write
    // with one must say where it came from. So does the proxy's own
    // sign-in cookie, through a proxy that does the login.
    let write = !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    if write && (via_cookie || proxy_login.is_some()) && header(header::ORIGIN).is_none() && fetch_site != Some("same-origin") {
        return Err(cross_site());
    }

    let caller = match (token, proxy_login) {
        (Some(t), _) => Caller::Token(t),
        (None, Some(client)) => Caller::Proxy(client),
        (None, None) => Caller::Local,
    };
    let scope = match &caller {
        Caller::Token(t) => Some(t.scope),
        Caller::Proxy(_) => Some(Scope::Full),
        Caller::Local | Caller::Anyone => None,
    };
    if let Some(why) = scope.and_then(|s| refused_for(s, req.method(), path)) {
        return Err(Refusal::new(StatusCode::FORBIDDEN, why));
    }
    Ok(caller)
}

fn host_refusal(public: Option<&PublicUrl>, loopback_only: bool) -> String {
    match public {
        Some(p) => format!("UpNext only answers requests addressed to localhost or {}", p.authority),
        None if loopback_only => "UpNext only answers requests addressed to localhost".to_string(),
        None => format!("UpNext only answers requests addressed to localhost, or to another name with a token; {NEEDS_TOKEN}"),
    }
}

/// The token in `Authorization: Bearer …`, if there is one.
fn bearer(req: &Request) -> Option<&str> {
    let value = req.headers().get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.trim().split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then(|| token.trim())
}

/// Why a token of `scope` can't make this request, if it can't.
///
/// - Local-only: opening GOG's login window, which opens on the server's
///   desktop, and making a token or a pairing code, so a token or a paired
///   device can't hand out more access; reading or ending a waiting
///   pairing code too. "Add to Steam" (`steam_shortcut`), which reads
///   and writes Steam's files there. Backups' downloads, restores,
///   imports and deletes too: a backup with keys holds every secret and the
///   token list. Their own guard (`backups::this_computer_only`) refuses
///   them a second time.
/// - Read: GET and HEAD only, and not the export (it's the whole library),
///   the token list or Steam's sign-in (it saves the SteamID). The shared
///   data's offer is a POST, so a read token can't have the server fetch an
///   address either, which would let it probe the network.
///
/// `path` is without the version (`unversioned`).
pub fn refused_for(scope: Scope, method: &Method, path: &str) -> Option<&'static str> {
    if path == "/api/gog/window" {
        return Some("GOG's sign-in window opens on the computer UpNext runs on, so only that computer can open it");
    }
    // A token, or a paired phone, can't make more of itself. Nor read or end
    // a waiting code: a lost phone could pair a second device with it, one
    // that outlives revoking the phone.
    if *method == Method::POST && path == "/api/tokens" {
        return Some("new tokens and pairing codes are made only on the computer UpNext runs on");
    }
    // Nor allow another device that asks: a paired phone allowing
    // a second device is the lost phone's problem again.
    if path == "/api/pairing/requests" || path.starts_with("/api/pairing/requests/") {
        return Some("devices are allowed or denied only on the computer UpNext runs on");
    }
    if path == "/api/pairing" || path.starts_with("/api/pairing/") {
        return Some("pairing codes are made, shown and ended only on the computer UpNext runs on");
    }
    // They make the server call other computers, and read and keep the
    // tokens they give.
    if path.starts_with("/api/network/") {
        return Some("other computers' UpNext is used only from the computer UpNext runs on");
    }
    // It clears keys and can clear the library.
    if path == "/api/setup/reset" {
        return Some("what setup clears is chosen only on the computer UpNext runs on");
    }
    // They read and write Steam's files here, and name UpNext's own program.
    if path == "/api/steam/shortcut" {
        return Some("big screen is added to Steam only on the computer UpNext runs on");
    }
    // Only the list and backing up now are anyone's.
    if path.starts_with("/api/backups/") {
        return Some("backups are downloaded, restored, imported and deleted only on the computer UpNext runs on");
    }
    if scope == Scope::Full {
        return None;
    }
    if !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return Some("this token can only read: make a full one to change anything or to make picks");
    }
    if path == "/api/export" || path == "/api/tokens" || path.starts_with("/api/tokens/") {
        return Some("this needs a full token");
    }
    // The "Send to UpNext" key lets its holder finish a GOG sign-in (L10).
    if path == "/api/gog/bookmark" {
        return Some("the GOG bookmark's key needs a full token");
    }
    // A GET, but the sign-in it starts saves the SteamID.
    if path == "/api/steam/login" {
        return Some("signing in with Steam changes a setting: it needs a full token");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes() {
        let get = Method::GET;
        assert_eq!(refused_for(Scope::Read, &get, "/api/games"), None);
        assert_eq!(refused_for(Scope::Read, &get, "/"), None, "the UI's files");
        assert!(refused_for(Scope::Read, &Method::POST, "/api/picks").is_some());
        assert!(refused_for(Scope::Read, &Method::PUT, "/api/settings").is_some());
        assert!(refused_for(Scope::Read, &get, "/api/export").is_some());
        assert!(refused_for(Scope::Read, &get, "/api/tokens").is_some());
        assert!(refused_for(Scope::Read, &get, "/api/steam/login").is_some());
        assert!(refused_for(Scope::Read, &Method::POST, "/api/seed/offer").is_some());
        assert_eq!(refused_for(Scope::Read, &get, "/api/seed"), None);
        assert_eq!(refused_for(Scope::Full, &get, "/api/steam/login"), None);
        assert_eq!(refused_for(Scope::Full, &Method::POST, "/api/picks"), None);
        assert_eq!(refused_for(Scope::Full, &get, "/api/export"), None);
        assert!(refused_for(Scope::Full, &Method::POST, "/api/setup/reset").is_some(), "starting the setup again");
        assert!(refused_for(Scope::Full, &get, "/api/steam/shortcut").is_some(), "Add to Steam");
        assert!(refused_for(Scope::Full, &Method::POST, "/api/steam/shortcut").is_some(), "Add to Steam");
        assert!(refused_for(Scope::Full, &Method::POST, "/api/gog/window").is_some());
        assert!(refused_for(Scope::Full, &Method::POST, "/api/tokens").is_some());
        assert!(refused_for(Scope::Full, &Method::POST, "/api/pairing").is_some());
        assert_eq!(refused_for(Scope::Full, &get, "/api/tokens"), None);
        assert!(refused_for(Scope::Full, &get, "/api/pairing").is_some(), "a waiting code");
        assert!(refused_for(Scope::Full, &get, "/api/pairing/outcome").is_some(), "what became of a code");
        assert!(refused_for(Scope::Full, &Method::DELETE, "/api/pairing").is_some());
        assert_eq!(refused_for(Scope::Full, &Method::POST, "/api/pair"), None);
        assert_eq!(refused_for(Scope::Full, &Method::POST, "/api/pair/request"), None);
        assert_eq!(
            refused_for(Scope::Full, &get, "/api/pairing/requests"),
            Some("devices are allowed or denied only on the computer UpNext runs on"),
            "answering a device"
        );
        assert!(refused_for(Scope::Full, &Method::POST, "/api/pairing/requests/ab12").is_some(), "answering a device");
        assert!(refused_for(Scope::Full, &get, "/api/network/computers").is_some(), "the computers on the network");
        assert!(refused_for(Scope::Full, &Method::POST, "/api/network/connect").is_some(), "opening the window on another computer");
        assert_eq!(refused_for(Scope::Read, &get, "/api/backups"), None);
        assert_eq!(refused_for(Scope::Full, &Method::POST, "/api/backups"), None);
        for (method, path) in [
            (&get, "/api/backups/upnext-2026-10-02.db/download"),
            (&Method::POST, "/api/backups/upnext-2026-10-02.db/restore"),
            (&Method::POST, "/api/backups/import"),
            (&Method::DELETE, "/api/backups/upnext-2026-10-02.db"),
        ] {
            assert!(refused_for(Scope::Full, method, path).is_some(), "{method} {path}");
            assert!(refused_for(Scope::Read, method, path).is_some(), "{method} {path}");
        }
    }

    #[test]
    fn public_hosts() {
        let p = PublicUrl::parse("https://upnext.example.org").unwrap();
        assert!(names_public(&p, "upnext.example.org"));
        assert!(names_public(&p, "UpNext.Example.org:443"));
        assert!(!names_public(&p, "upnext.example.org:8787"));
        assert!(!names_public(&p, "evil.example"));
        let p = PublicUrl::parse("http://den.lan:8787").unwrap();
        assert!(names_public(&p, "den.lan:8787"));
        assert!(!names_public(&p, "den.lan"));
    }
}
