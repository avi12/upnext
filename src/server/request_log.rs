//! One log line per request: its method, path, status and how long
//! the answer took, at info level under the target `upnext::request`. In a
//! terminal `upnext serve` prints them by default (`cli::init_logging`), so
//! you can watch the window's requests arrive.
//!
//! The query string and the headers are never logged: the Steam, GOG and
//! identity provider callbacks carry sign-in codes in the query, and tokens
//! and cookies travel in the headers. No route takes a secret in its path.
//! Through a proxy listed in `trusted_proxies`, the line ends with the
//! client's address as the proxy gave it.

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;

/// The target the lines are logged under, for `RUST_LOG`.
const TARGET: &str = "upnext::request";

/// Log the request once its answer is ready (a streamed body may still be
/// on its way).
pub async fn log(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_owned();
    let started = std::time::Instant::now();
    let resp = next.run(req).await;
    let status = resp.status().as_u16();
    let ms = started.elapsed().as_millis();
    match resp.extensions().get::<super::access::ForwardedFor>() {
        Some(super::access::ForwardedFor(client)) => tracing::info!(target: TARGET, "{method} {path} {status} {ms} ms for {client}"),
        None => tracing::info!(target: TARGET, "{method} {path} {status} {ms} ms"),
    }
    resp
}
