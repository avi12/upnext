//! Every API error is `{"error": "…"}` (docs/API.md). Handlers answer with
//! [`ApiError`], but axum's own refusals don't: a path, query or body its
//! extractors can't read, a method the route doesn't take, or a body over
//! the limit answer in plain text, or with no body at all. This layer, on
//! the `/api` routes only, puts those in the same shape and keeps their
//! status and headers (B-CORE-3, R-CORE-3).
//!
//! It also keeps an internal error's text on this computer (R-CORE-2): a
//! 500's message is the whole error chain, which can name files and folders
//! on the server and carry SQL. Another device gets a plain sentence in its
//! place; the chain is in the server's log, where `From<anyhow::Error>` put
//! it.

use super::access::Caller;
use axum::Json;
use axum::body::{Body, to_bytes};
use axum::extract::Request;
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// An extractor's message is a line or two; anything longer isn't one.
const MESSAGE_MAX: usize = 16 * 1024;

/// What another device reads in place of an internal error.
pub const INTERNAL_ELSEWHERE: &str = "Something went wrong on the server. UpNext's log on the computer it runs on says what.";

/// Marks an answer whose message is an internal error's own text.
#[derive(Debug, Clone, Copy)]
pub struct Internal;

pub struct ApiError(pub(super) StatusCode, pub(super) String);

/// The statuses handlers answer most, each with its message (R-CORE-2).
/// A status only a handler or two use is still written out in full.
impl ApiError {
    /// A 400: the request asks for something the server can't do as asked.
    pub(super) fn bad(message: impl Into<String>) -> Self {
        ApiError(StatusCode::BAD_REQUEST, message.into())
    }
    /// A 404: what the request names isn't there, or isn't this player's to see.
    pub(super) fn not_found(message: impl Into<String>) -> Self {
        ApiError(StatusCode::NOT_FOUND, message.into())
    }
    /// A 403: the caller may not do this.
    pub(super) fn forbidden(message: impl Into<String>) -> Self {
        ApiError(StatusCode::FORBIDDEN, message.into())
    }
    /// A 409: the request clashes with what is there or running now.
    pub(super) fn conflict(message: impl Into<String>) -> Self {
        ApiError(StatusCode::CONFLICT, message.into())
    }
    /// A 502: a service outside UpNext failed, and the message says how.
    pub(super) fn bad_gateway(message: impl Into<String>) -> Self {
        ApiError(StatusCode::BAD_GATEWAY, message.into())
    }
}

/// The game `id` as player `me` sees it, or a 404 when there is none or it
/// is hidden from them.
pub(super) fn game_or_404(state: &super::AppState, me: crate::db::PlayerId, id: i64) -> Result<crate::db::GameView, ApiError> {
    state.db.game(me, id)?.ok_or_else(|| ApiError::not_found(format!("game {id} not found")))
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut res = (self.0, Json(json!({ "error": self.1 }))).into_response();
        // Its text may name files and SQL; `json_errors` keeps it to
        // this computer (R-CORE-2).
        if self.0 == StatusCode::INTERNAL_SERVER_ERROR {
            res.extensions_mut().insert(Internal);
        }
        res
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!("{e:#}");
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))
    }
}

/// A 400 with the reason for a change the call refused
/// ([`crate::db::Refused`]); any other error is the server's (R-CORE-2).
pub(super) fn refused(e: anyhow::Error) -> ApiError {
    match e.downcast::<crate::db::Refused>() {
        Ok(r) => ApiError::bad(r.0),
        Err(e) => e.into(),
    }
}

pub(super) type ApiResult<T> = Result<Json<T>, ApiError>;

pub async fn json_errors(req: Request, next: Next) -> Response {
    // The access guard runs first and always says who's asking.
    let here = req.extensions().get::<Caller>().is_some_and(Caller::is_local);
    let res = next.run(req).await;
    let status = res.status();
    if res.extensions().get::<Internal>().is_some() && !here {
        let (mut parts, _) = res.into_parts();
        parts.headers.remove(header::CONTENT_LENGTH);
        return (parts, Json(json!({ "error": INTERNAL_ELSEWHERE }))).into_response();
    }
    if !(status.is_client_error() || status.is_server_error()) || is_json(res.headers()) || !is_plain(res.headers()) {
        return res;
    }
    let (mut parts, body) = res.into_parts();
    let text = to_bytes(body, MESSAGE_MAX).await.ok().map(|b| String::from_utf8_lossy(&b).trim().to_string()).unwrap_or_default();
    let error = if text.is_empty() { status.canonical_reason().unwrap_or("request failed").to_lowercase() } else { text };
    let json = Json(json!({ "error": error })).into_response();
    parts.headers.remove(header::CONTENT_LENGTH);
    parts.headers.insert(header::CONTENT_TYPE, json.headers()[header::CONTENT_TYPE].clone());
    Response::from_parts(parts, Body::new(json.into_body()))
}

fn is_json(headers: &HeaderMap) -> bool {
    headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("application/json"))
}

/// Plain text, or no body type at all: what axum's refusals send. Any other
/// type, such as a page, is left as it is.
fn is_plain(headers: &HeaderMap) -> bool {
    match headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()) {
        None => true,
        Some(v) => v.starts_with("text/plain"),
    }
}
