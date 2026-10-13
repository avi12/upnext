//! The GOG account: whether it's connected, signing in with the code from
//! GOG's redirect or in the native window, and disconnecting. A browser on
//! another device signs in through `gog_link` instead.

use super::players::Me;
use super::{ApiError, ApiResult, AppState};
use axum::extract::State;
use axum::{Json, Router};
use serde::Deserialize;

pub fn routes() -> Router<AppState> {
    use axum::routing::{get, post};
    Router::new().route("/gog", get(gog_get).delete(gog_logout)).route("/gog/login", post(gog_login)).route("/gog/window", post(gog_window))
}

#[utoipa::path(get, path = "/gog", tag = "accounts",
    description = "The GOG account: connected or not, and the sign-in address.",
    responses((status = 200, body = crate::gog::GogStatus)))]
async fn gog_get(State(state): State<AppState>, Me(me): Me) -> ApiResult<crate::gog::GogStatus> {
    Ok(Json(state.gog_for(me).status()))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct GogLoginBody {
    code: String,
}

#[utoipa::path(post, path = "/gog/login", tag = "accounts",
    description = "Connect GOG with the code from the sign-in redirect (or the whole redirect URL).",
    request_body = GogLoginBody,
    responses((status = 200, body = crate::gog::GogStatus), (status = 400, description = "The code is invalid or expired")))]
async fn gog_login(State(state): State<AppState>, Me(me): Me, Json(body): Json<GogLoginBody>) -> ApiResult<crate::gog::GogStatus> {
    match state.gog_for(me).login(&body.code).await {
        Ok(s) => Ok(Json(s)),
        Err(e) => Err(ApiError::bad(format!("{e:#}"))),
    }
}

/// Open the native GOG login window; the UI polls `/api/gog` for the result.
#[utoipa::path(post, path = "/gog/window", tag = "accounts",
    description = "Open the native GOG sign-in window on the computer UpNext runs on; poll GET /gog for the result.",
    responses((status = 200, body = crate::gog::GogStatus),
        (status = 409, description = "A window is already open, this build has no windows, or the computer has no desktop")))]
async fn gog_window(State(state): State<AppState>, Me(me): Me) -> ApiResult<crate::gog::GogStatus> {
    let desktop = crate::display::detected(state.base_cfg.container, state.base_cfg.run_mode()).await;
    match state.gog_for(me).start_window_login(&desktop) {
        Ok(()) => Ok(Json(state.gog_for(me).status())),
        Err(e) => Err(ApiError::conflict(format!("{e:#}"))),
    }
}

#[utoipa::path(delete, path = "/gog", tag = "accounts",
    description = "Disconnect GOG: the stored sign-in is deleted.",
    responses((status = 200, body = crate::gog::GogStatus)))]
async fn gog_logout(State(state): State<AppState>, Me(me): Me) -> ApiResult<crate::gog::GogStatus> {
    state.gog_for(me).disconnect().await?;
    Ok(Json(state.gog_for(me).status()))
}
