//! HTTP API (docs/API.md) plus the web UI (`ui`). This file puts the
//! routes together; each area's handlers and their routes are in a module
//! beside it, and `serve` runs the server.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};
use tower_http::services::{ServeDir, ServeFile};

use errors::{ApiError, ApiResult, game_or_404, refused};
pub(crate) use setup::own_setup_done;
pub use state::AppState;

/// `GET /api/health`: which program this is, its version and build (the
/// short commit, or null). `upnext app` asks it to tell a running UpNext from
/// another program on the port, and an UpNext of another build from its own
///, and `upnext healthcheck` (the container's health check)
/// asks it too.
/// `container` tells an unpaired browser where its pairing code comes from.
/// `id` is the install id the network announcement carries, so a
/// computer that kept this one's token checks it's still this one before
/// sending it.
#[utoipa::path(get, path = "/health", tag = "system",
    description = "Which program this is, its version and its build (the short commit it was built from, or null), for \
        `upnext app` and `upnext healthcheck` (the container's health check), whether it runs in UpNext's container \
        image, for the pairing page, and its install id, as the network announcement gives it. Needs no token.",
    responses((status = 200, body = Object, description = "`{app: \"upnext\", version, build, container, id}`")))]
async fn health(State(state): State<AppState>) -> Json<Value> {
    let id = network::network_id(&state).inspect_err(|e| tracing::warn!("no install id: {e:#}")).ok();
    Json(json!({
        "app": "upnext",
        "version": crate::build_id::VERSION,
        "build": crate::build_id::BUILD,
        "container": state.base_cfg.container,
        "id": id,
    }))
}

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .route("/openapi.json", get(openapi::document))
        .merge(backups::routes())
        .merge(taste::routes())
        .merge(tokens::routes())
        .merge(pairing::routes())
        .merge(approval::routes())
        .merge(network::routes())
        .merge(discovery::routes())
        .merge(wishlist::routes())
        .merge(setup::routes())
        .merge(gog_link::routes())
        .merge(seed::routes())
        .merge(ratings::routes())
        .merge(stores::routes())
        .merge(routines::routes())
        .merge(steam_shortcut::routes())
        .merge(verdicts::routes())
        .merge(players::routes())
        .merge(oidc::routes())
        .merge(accounts::routes())
        .merge(community_verdicts::routes())
        .merge(settings::routes())
        .merge(steam::routes())
        .merge(ai::routes())
        .merge(gog::routes())
        .merge(jobs::routes())
        .merge(library::routes())
        .merge(games::routes())
        .merge(picks::routes());

    let static_dir = state.base_cfg.static_dir.clone();
    let guard_state = state.clone();
    // `/api/v1` is the contract; `/api` answers the same until the
    // first public release.
    // An `/api` path that matches no route is a JSON 404, not the UI's page,
    // and axum's own refusals are JSON like every other error.
    let api = api.fallback(api_not_found).layer(axum::middleware::from_fn(errors::json_errors));
    let mut app =
        Router::new().route("/third-party-notices.txt", get(ui::notices)).nest("/api/v1", api.clone()).nest("/api", api).with_state(state);
    app = match static_dir {
        Some(dir) if dir.join("index.html").is_file() => {
            // A missing file under assets/ is a 404, as in `ui::embedded_ui`.
            let index = dir.join("index.html");
            app.nest_service("/assets", ServeDir::new(dir.join("assets")))
                .fallback_service(ServeDir::new(&dir).fallback(ServeFile::new(index)))
        }
        Some(dir) => {
            tracing::warn!("web UI not found at {}; serving a placeholder page", dir.display());
            app.fallback(ui::missing_ui)
        }
        None if ui::embedded() => app.fallback(ui::embedded_ui),
        None => {
            tracing::warn!("this build has no web UI (web/dist wasn't built); serving a placeholder page");
            app.fallback(ui::missing_ui)
        }
    };
    // No CORS: the UI is same-origin (and the Vite dev server proxies /api).
    app.layer(axum::middleware::from_fn_with_state(guard_state, access::guard))
        .layer(axum::middleware::map_response(access::security_headers))
        .layer(axum::middleware::from_fn(request_log::log))
}

async fn api_not_found(axum::extract::OriginalUri(uri): axum::extract::OriginalUri) -> ApiError {
    ApiError::not_found(format!("no such API route: {}", uri.path()))
}

/// Whether this binary has the web UI in it (`upnext doctor`).
pub fn ui_built_in() -> bool {
    ui::embedded()
}

mod access;
mod accounts;
mod ai;
pub mod approval;
mod backups;
mod community_verdicts;
mod discovery;
mod errors;
mod games;
mod gog;
mod gog_link;
mod jobs;
pub mod lan;
mod library;
mod limits;
pub mod listen;
pub mod network;
mod oidc;
mod openapi;
pub mod pairing;
mod picks;
mod players;
pub mod proxies;
mod ratings;
mod request_log;
mod routines;
pub(crate) mod scheduler;
mod seed;
pub(crate) mod serve;
mod settings;
mod setup;
mod setup_sync;
mod state;
mod steam;
mod steam_shortcut;
mod stores;
mod taste;
mod tokens;
mod ui;
mod verdicts;
mod wishlist;

#[cfg(test)]
mod api_tests;
