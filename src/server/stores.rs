//! The stores beyond Steam, GOG and Heroic (docs/api/more-stores.md): the
//! Epic sign-in, and lists of games added by hand for the stores whose owned
//! library UpNext can't read.

use super::players::Me;
use super::{ApiError, ApiResult, AppState};
use crate::db::PlayerId;
use axum::extract::{Path, State};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

pub fn routes() -> Router<AppState> {
    use axum::routing::{get, post, put};
    Router::new()
        .route("/epic", get(epic_get).delete(epic_logout))
        .route("/epic/login", post(epic_login))
        .route("/library/by-hand", get(by_hand_get))
        .route("/library/by-hand/{store}", put(by_hand_put))
}

#[utoipa::path(get, path = "/epic", tag = "accounts",
    description = "The Epic sign-in: whether it's there, the display name, Epic's sign-in page, and why it ended when Epic \
        turned it down at a sync.",
    responses((status = 200, body = crate::epic::EpicStatus)))]
async fn epic_get(State(state): State<AppState>, Me(me): Me) -> ApiResult<crate::epic::EpicStatus> {
    Ok(Json(state.epic_for(me).status()))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct EpicLoginBody {
    /// What Epic's page showed after signing in: its `authorizationCode`,
    /// or the whole page.
    code: String,
}

#[utoipa::path(post, path = "/epic/login", tag = "accounts",
    description = "Sign in to Epic with the code its sign-in page (`auth_url`) ends on: the `authorizationCode`, or the whole \
        page of JSON. The code works once, for a few minutes.",
    request_body = EpicLoginBody,
    responses((status = 200, body = crate::epic::EpicStatus), (status = 400, description = "No code in it, or Epic turned it down")))]
async fn epic_login(State(state): State<AppState>, Me(me): Me, Json(body): Json<EpicLoginBody>) -> ApiResult<crate::epic::EpicStatus> {
    match state.epic_for(me).login(&body.code).await {
        Ok(s) => Ok(Json(s)),
        Err(e) => Err(ApiError::bad(format!("{e:#}"))),
    }
}

#[utoipa::path(delete, path = "/epic", tag = "accounts",
    description = "Sign out of Epic: the stored sign-in is deleted. The Epic games already read stay.",
    responses((status = 200, body = crate::epic::EpicStatus)))]
async fn epic_logout(State(state): State<AppState>, Me(me): Me) -> ApiResult<crate::epic::EpicStatus> {
    state.epic_for(me).disconnect().await?;
    Ok(Json(state.epic_for(me).status()))
}

/// One store's list of games added by hand.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ByHandList {
    /// The store's source id: `origin`, `uplay`, `battlenet` or `xboxone`.
    store: String,
    /// Its name as people know it: "EA app".
    label: String,
    /// The titles listed, by name.
    titles: Vec<String>,
}

/// Every store a list can be kept for, with its list, empty when there's none.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ByHandLists {
    stores: Vec<ByHandList>,
}

fn lists(state: &AppState, me: PlayerId) -> Result<ByHandLists, ApiError> {
    let mut all = crate::manual::lists(&state.db, me)?;
    let stores = crate::manual::STORES
        .iter()
        .map(|s| ByHandList { store: s.to_string(), label: crate::galaxy::platform_label(s), titles: all.remove(*s).unwrap_or_default() })
        .collect();
    Ok(ByHandLists { stores })
}

#[utoipa::path(get, path = "/library/by-hand", tag = "library",
    description = "The games added by hand, for each store whose owned library UpNext can't read (the EA app, Ubisoft \
        Connect, Battle.net and Xbox): every such store, with its titles, an empty list where none were added.",
    responses((status = 200, body = ByHandLists)))]
async fn by_hand_get(State(state): State<AppState>, Me(me): Me) -> ApiResult<ByHandLists> {
    Ok(Json(lists(&state, me)?))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ByHandBody {
    /// The titles, one a line; an entry may hold several lines. Blank lines,
    /// bullets, trademark signs and repeats are dropped.
    titles: Vec<String>,
}

/// What saving a list did.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ByHandSaved {
    /// Titles on the list now.
    total: usize,
    added: usize,
    /// Titles that were on the list and aren't now: they leave the library.
    removed: usize,
    lists: ByHandLists,
}

#[utoipa::path(put, path = "/library/by-hand/{store}", tag = "library",
    description = "Replace a store's list of games added by hand. A title new to the list joins the library as an owned, \
        uninstalled copy of that store; one left off leaves it. An empty list takes them all out. At most 5,000 titles.",
    params(("store" = String, Path, description = "`origin`, `uplay`, `battlenet` or `xboxone`")),
    request_body = ByHandBody,
    responses((status = 200, body = ByHandSaved), (status = 400, description = "Another store, or too many titles")))]
async fn by_hand_put(
    State(state): State<AppState>,
    Me(me): Me,
    Path(store): Path<String>,
    Json(body): Json<ByHandBody>,
) -> ApiResult<ByHandSaved> {
    let saved = crate::manual::save(&state.db, me, &store, &body.titles).map_err(|e| ApiError::bad(format!("{e:#}")))?;
    Ok(Json(ByHandSaved { total: saved.total, added: saved.added, removed: saved.removed, lists: lists(&state, me)? }))
}
