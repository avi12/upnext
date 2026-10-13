//! UpNext on the home network: what this server
//! announces (`announce`), and, for the app window on this computer, using
//! another computer's UpNext instead of its own ("Use the library on another
//! computer", `upnext app --connect`).
//!
//! The window's connect page finds the computers that announce themselves
//! (`GET /api/network/computers`), or takes an address; this server then
//! asks that computer to allow it (`POST /api/network/connect`, which makes
//! the request of `approval` there) and collects the answer
//! (`GET /api/network/connect`). Allowed, the token goes into
//! `computers.json` in the config folder, and the window opens on that
//! computer (`desktop`). All of it is local-only: another device has no
//! business making this server call out, or reading the tokens it keeps.

use super::{ApiError, AppState};
use crate::announce::{self, Announcement, Found};
use crate::computers::{Computer, Computers};
use crate::config::Config;
use crate::db::now;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Mutex;
use std::time::Duration;

/// How long the connect page listens for announcements.
#[cfg(not(test))]
const BROWSE: Duration = Duration::from_secs(2);
/// UpNext's port, when an address names none.
const DEFAULT_PORT: u16 = 8787;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/network/computers", get(computers))
        .route("/network/computers/{id}", delete(forget))
        .route("/network/connect", get(connect_status).post(connect).delete(connect_cancel))
}

/// This install's id on the network, made the first time it's asked for.
pub fn network_id(state: &AppState) -> anyhow::Result<String> {
    if let Some(id) = state.db.meta_get("network_id")? {
        return Ok(id);
    }
    let id = announce::new_id()?;
    state.db.meta_set("network_id", &id)?;
    Ok(id)
}

/// The computer's name on the network: the setting, else its host name.
pub fn network_name(cfg: &Config) -> String {
    cfg.network_name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(announce::host_name)
}

/// What to announce now: nothing unless other devices may use UpNext
/// (listening beyond loopback) and "Announce on the network" is on.
pub fn announcement(state: &AppState) -> Option<Announcement> {
    let cfg = state.cfg();
    let listening = state.listening();
    if !cfg.announce || !listening.beyond_loopback() {
        return None;
    }
    let id = network_id(state).inspect_err(|e| tracing::warn!("no install id to announce: {e:#}")).ok()?;
    let only = listening.host.parse::<std::net::IpAddr>().ok().filter(|ip| !ip.is_unspecified());
    Some(Announcement { id, name: network_name(&cfg), port: listening.port, tls: listening.https, only })
}

/// Bring the announcement in line with the settings and the listener.
pub fn announce_now(state: &AppState) {
    state.remote.announcer.set(announcement(state));
}

/// A request this computer made of another one, waiting for its answer.
struct Outgoing {
    address: String,
    id: String,
    secret: String,
    match_code: String,
    computer: String,
    computer_id: String,
    expires_at: i64,
}

/// The connect page's request, while it waits.
#[derive(Default)]
pub struct Connecting {
    now: Mutex<Option<Outgoing>>,
    /// What browsing finds, in the router tests: they have no network.
    #[cfg(test)]
    pub found: Mutex<Vec<Found>>,
}

impl Connecting {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Outgoing>> {
        self.now.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// This computer, as the connect page names it.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ThisComputer {
    pub id: String,
    pub name: String,
}

/// A computer the window has been allowed on.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Saved {
    pub id: String,
    pub name: String,
    pub address: String,
    /// The window opens on it.
    pub open: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Nearby {
    pub this: ThisComputer,
    /// The computers announcing UpNext on the network now, without this one.
    pub found: Vec<Found>,
    /// The ones allowed before, kept in the config folder.
    pub saved: Vec<Saved>,
}

async fn browse(state: &AppState, own: String) -> Vec<Found> {
    #[cfg(test)]
    {
        let _ = own;
        state.remote.connecting.found.lock().unwrap().clone()
    }
    #[cfg(not(test))]
    {
        let _ = state;
        match tokio::task::spawn_blocking(move || announce::browse(BROWSE, Some(&own))).await {
            Ok(Ok(found)) => found,
            Ok(Err(e)) => {
                tracing::warn!("looking for UpNext on the network: {e:#}");
                vec![]
            }
            Err(e) => {
                tracing::warn!("looking for UpNext on the network: {e}");
                vec![]
            }
        }
    }
}

/// The computers to choose from.
#[utoipa::path(get, path = "/network/computers", tag = "devices", operation_id = "network_computers",
    description = "The other computers announcing UpNext on the network (it listens for about two seconds), and the ones \
        this computer was allowed on before. Only on the computer UpNext runs on.",
    responses((status = 200, body = Nearby), (status = 403, description = "With a token")))]
async fn computers(State(state): State<AppState>) -> Result<Json<Nearby>, ApiError> {
    let id = network_id(&state)?;
    let this = ThisComputer { id: id.clone(), name: network_name(&state.cfg()) };
    let found = browse(&state, id).await;
    let kept = Computers::load(&state.base_cfg.config_dir)?;
    let saved = kept
        .computers
        .iter()
        .map(|c| Saved { id: c.id.clone(), name: c.name.clone(), address: c.address.clone(), open: kept.open.as_deref() == Some(&c.id) })
        .collect();
    Ok(Json(Nearby { this, found, saved }))
}

/// Forget a computer: its token is dropped here. It stays in that
/// computer's own device list until revoked there.
#[utoipa::path(delete, path = "/network/computers/{id}", tag = "devices", operation_id = "network_forget",
    description = "Forget a computer this one was allowed on, and the token it gave. Revoke it in that computer's \
        device list too. Only on the computer UpNext runs on.",
    params(("id" = String, Path, description = "The computer's install id")),
    responses((status = 204), (status = 403, description = "With a token"), (status = 404, description = "Not one of the saved computers")))]
async fn forget(State(state): State<AppState>, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    let dir = &state.base_cfg.config_dir;
    let mut kept = Computers::load(dir)?;
    if !kept.remove(&id) {
        return Err(ApiError::not_found("that computer isn't saved here"));
    }
    kept.save(dir)?;
    Ok(StatusCode::NO_CONTENT)
}

/// The address to open, from what was found or typed: `den.lan`,
/// `192.168.0.42:8787`, `https://den.tailnet.ts.net`. Without a scheme it's
/// plain HTTP, and without a port UpNext's.
pub fn address_of(typed: &str) -> Option<String> {
    let typed = typed.trim().trim_end_matches('/');
    if typed.is_empty() || typed.contains(char::is_whitespace) {
        return None;
    }
    let with_scheme = if typed.contains("://") { typed.to_string() } else { format!("http://{typed}") };
    let mut url = reqwest::Url::parse(&with_scheme).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() || url.path() != "/" || url.query().is_some() {
        return None;
    }
    if !typed.contains("://") && url.port().is_none() {
        url.set_port(Some(DEFAULT_PORT)).ok()?;
    }
    Some(url.origin().ascii_serialization())
}

fn client() -> anyhow::Result<reqwest::Client> {
    // Straight to the computer on the network, whatever proxy the
    // environment names.
    Ok(reqwest::Client::builder().no_proxy().user_agent(crate::http::USER_AGENT).timeout(Duration::from_secs(8)).build()?)
}

/// Why the other computer said no, from its answer.
async fn their_reason(res: reqwest::Response) -> String {
    let status = res.status();
    let body: Value = res.json().await.unwrap_or_default();
    body["error"].as_str().map_or_else(|| format!("it answered {status}"), str::to_string)
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ConnectBody {
    /// A found computer's address, or one typed: `den.lan`,
    /// `192.168.0.42:8787`, `https://den.tailnet.ts.net`.
    address: String,
    /// The install id it was found or saved with: asking stops when the
    /// UpNext at `address` is another one.
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConnectStatus {
    Waiting,
    Allowed,
    Denied,
    /// Nobody answered in time, or the other computer forgot the request.
    Expired,
}

/// The request to another computer, as the connect page shows it.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Connection {
    pub status: ConnectStatus,
    /// The four digits the other computer shows beside Allow.
    pub match_code: String,
    /// The other computer's name and address.
    pub computer: String,
    pub address: String,
    /// Allowed: its install id, as saved.
    pub id: Option<String>,
}

fn view(o: &Outgoing, status: ConnectStatus) -> Connection {
    let id = (status == ConnectStatus::Allowed).then(|| o.computer_id.clone());
    Connection { status, match_code: o.match_code.clone(), computer: o.computer.clone(), address: o.address.clone(), id }
}

/// Ask another computer to allow this one.
#[utoipa::path(post, path = "/network/connect", tag = "devices", operation_id = "network_connect",
    description = "Ask the UpNext at `address` to allow this computer: that computer shows the request, with the match \
        number in the answer, until someone allows or denies it there. Any earlier request from here ends. Only on the \
        computer UpNext runs on.",
    request_body = ConnectBody,
    responses((status = 201, body = Connection), (status = 400, description = "Not an address"),
        (status = 403, description = "With a token"),
        (status = 409, description = "Another UpNext than `id` answers at the address"),
        (status = 502, description = "Nothing answered there as UpNext, or it refused the request (its reason is given)")))]
async fn connect(State(state): State<AppState>, Json(body): Json<ConnectBody>) -> Result<(StatusCode, Json<Connection>), ApiError> {
    let address = address_of(&body.address)
        .ok_or_else(|| ApiError::bad("that isn't an address: try den.lan, 192.168.0.42:8787 or https://den.tailnet.ts.net"))?;
    let http = client()?;
    let unreachable = |e: reqwest::Error| ApiError::bad_gateway(format!("UpNext doesn't answer at {address}: {e}"));
    let health: Value = http.get(format!("{address}/api/health")).send().await.map_err(unreachable)?.json().await.unwrap_or_default();
    if health["app"] != "upnext" {
        return Err(ApiError::bad_gateway(format!("something answers at {address}, but not UpNext")));
    }
    let expected = body.id.as_deref().filter(|id| !id.is_empty());
    if let Some(id) = expected.filter(|id| health["id"].as_str() != Some(id)) {
        return Err(another_upnext(&address, id));
    }
    let ask = json!({ "name": network_name(&state.cfg()), "kind": "computer", "wants": "token" });
    let res = http.post(format!("{address}/api/v1/pair/request")).json(&ask).send().await.map_err(unreachable)?;
    if !res.status().is_success() {
        let why = if res.status() == StatusCode::NOT_FOUND {
            "its UpNext is too old to allow devices with a tap: update it, or pair with a code".to_string()
        } else {
            their_reason(res).await
        };
        return Err(ApiError::bad_gateway(format!("{address} didn't take the request: {why}")));
    }
    let asked: Value = res.json().await.map_err(|e| ApiError::bad_gateway(format!("{address} answered oddly: {e}")))?;
    let text = |k: &str| asked[k].as_str().unwrap_or_default().to_string();
    let o = Outgoing {
        address: address.clone(),
        id: text("id"),
        secret: text("secret"),
        match_code: text("match_code"),
        computer: asked["computer"].as_str().map_or_else(|| address.clone(), str::to_string),
        computer_id: text("computer_id"),
        expires_at: asked["expires_at"].as_i64().unwrap_or_else(|| now() + super::approval::REQUEST_TTL),
    };
    if o.id.is_empty() || o.secret.is_empty() || o.computer_id.is_empty() {
        return Err(ApiError::bad_gateway(format!("{address} answered oddly: no request id")));
    }
    if let Some(id) = expected.filter(|id| *id != o.computer_id) {
        return Err(another_upnext(&address, id));
    }
    let shown = view(&o, ConnectStatus::Waiting);
    *state.remote.connecting.lock() = Some(o);
    Ok((StatusCode::CREATED, Json(shown)))
}

/// The computer at an address isn't the one found or saved there: its
/// address went to another computer running UpNext since.
fn another_upnext(address: &str, id: &str) -> ApiError {
    ApiError::conflict(format!(
        "another UpNext answers at {address} now, not the one with the id {id}: look for it again, or type its new address"
    ))
}

/// Ask the other computer for its answer once.
#[utoipa::path(get, path = "/network/connect", tag = "devices", operation_id = "network_connect_status",
    description = "Whether the other computer has answered. Allowed, its token is saved in the config folder and the \
        answer carries its id, for the window to open on it. Only on the computer UpNext runs on.",
    responses((status = 200, body = Connection), (status = 403, description = "With a token"),
        (status = 404, description = "No request is waiting"),
        (status = 502, description = "The other computer didn't answer")))]
async fn connect_status(State(state): State<AppState>) -> Result<Json<Connection>, ApiError> {
    let (address, id, secret) = {
        let o = state.remote.connecting.lock();
        let o = o.as_ref().ok_or_else(|| ApiError::not_found("no request to another computer is waiting"))?;
        (o.address.clone(), o.id.clone(), o.secret.clone())
    };
    let res = client()?
        .get(format!("{address}/api/v1/pair/request/{id}"))
        .header(super::approval::SECRET_HEADER, secret)
        .send()
        .await
        .map_err(|e| ApiError::bad_gateway(format!("{address} didn't answer: {e}")))?;
    let gone = res.status() == StatusCode::NOT_FOUND;
    let answer: Value = res.json().await.unwrap_or_default();
    let mut slot = state.remote.connecting.lock();
    let Some(o) = slot.as_ref().filter(|o| o.id == id) else {
        return Err(ApiError::not_found("the request was cancelled"));
    };
    if gone {
        let shown = view(o, ConnectStatus::Expired);
        *slot = None;
        return Ok(Json(shown));
    }
    let status = match answer["status"].as_str() {
        Some("allowed") => ConnectStatus::Allowed,
        Some("denied") => ConnectStatus::Denied,
        _ if o.expires_at <= now() => ConnectStatus::Expired,
        _ => return Ok(Json(view(o, ConnectStatus::Waiting))),
    };
    if status == ConnectStatus::Allowed {
        let Some(token) = answer["token"].as_str() else {
            return Err(ApiError::bad_gateway(format!("{address} allowed this computer but sent no token")));
        };
        // A computer with a public name takes its token only there.
        let address = answer["address"].as_str().and_then(address_of).unwrap_or_else(|| o.address.clone());
        let dir = &state.base_cfg.config_dir;
        let mut kept = Computers::load(dir)?;
        kept.add(Computer { id: o.computer_id.clone(), name: o.computer.clone(), address, token: token.to_string(), added_at: now() });
        kept.save(dir)?;
        tracing::info!("{} allowed this computer to use its UpNext", o.computer);
    }
    let shown = view(o, status);
    *slot = None;
    Ok(Json(shown))
}

#[utoipa::path(delete, path = "/network/connect", tag = "devices", operation_id = "network_connect_cancel",
    description = "Stop waiting for the other computer's answer. Only on the computer UpNext runs on.",
    responses((status = 204), (status = 403, description = "With a token")))]
async fn connect_cancel(State(state): State<AppState>) -> StatusCode {
    *state.remote.connecting.lock() = None;
    StatusCode::NO_CONTENT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_people_type() {
        assert_eq!(address_of("den.lan").as_deref(), Some("http://den.lan:8787"));
        assert_eq!(address_of(" 192.168.0.42:9000/ ").as_deref(), Some("http://192.168.0.42:9000"));
        assert_eq!(address_of("https://den.tailnet.ts.net").as_deref(), Some("https://den.tailnet.ts.net"));
        assert_eq!(address_of("http://192.168.0.42:8787/").as_deref(), Some("http://192.168.0.42:8787"));
        assert_eq!(address_of("[fd00::42]").as_deref(), Some("http://[fd00::42]:8787"));
        for bad in ["", "den lan", "ftp://den.lan", "http://den.lan/games", "http://den.lan/?x=1"] {
            assert_eq!(address_of(bad), None, "{bad}");
        }
    }
}
