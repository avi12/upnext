//! Backlog verdicts from other players (docs/api/community-verdicts.md):
//! a game's, as its page shows them, and the snapshot
//! this install keeps. The daily job downloads it (`POST /jobs` with the
//! `verdicts` task runs it now); `verdicts_receive` turns it on.

use super::players::Me;
use super::{ApiError, AppState};
use crate::db::CommunitySnapshot;
use crate::verdicts::community::GameVerdicts;
use crate::verdicts::consent::LICENCE;
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

pub fn routes() -> Router<AppState> {
    Router::new().route("/games/{id}/verdicts", get(game_verdicts)).route("/verdicts/community", get(community_status))
}

/// Whether other players' verdicts are on, and the snapshot kept.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CommunityStatus {
    /// "Backlog verdicts from other players" is on.
    pub on: bool,
    /// Someone chose on or off, here or in the setup wizard; until then the
    /// game page offers it once.
    pub chosen: bool,
    /// A verdicts service is set, so there's something to download.
    pub service: bool,
    pub snapshot: CommunitySnapshot,
    /// `CC0-1.0`.
    pub licence: &'static str,
}

fn status_of(state: &AppState) -> anyhow::Result<CommunityStatus> {
    let cfg = state.cfg();
    Ok(CommunityStatus {
        on: cfg.verdicts_receive,
        chosen: cfg.source("verdicts_receive") != "default",
        service: cfg.verdicts_url.is_some(),
        snapshot: state.db.community_snapshot()?,
        licence: LICENCE,
    })
}

#[utoipa::path(get, path = "/verdicts/community", tag = "verdicts",
    description = "Other players' backlog verdicts as this computer keeps them: whether they're on, whether a service is \
        set, and the snapshot kept (its date, its games and how many of them are in this library).",
    responses((status = 200, body = CommunityStatus)))]
pub(super) async fn community_status(State(state): State<AppState>) -> Result<Json<CommunityStatus>, ApiError> {
    Ok(Json(status_of(&state)?))
}

/// A game's verdicts from other players.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GameCommunity {
    #[serde(flatten)]
    pub status: CommunityStatus,
    /// The game's verdicts, when it has at least 3 and they're on.
    pub verdicts: Option<GameVerdicts>,
}

#[utoipa::path(get, path = "/games/{id}/verdicts", tag = "verdicts",
    description = "What other backlog players say about a game, from the last snapshot of their verdicts: the headline, \
        the \"most players\" lines (from 5 verdicts), how far people got and the newest texts, each with its verdict id \
        for reporting it. `verdicts` is null when the game has fewer than 3 or they're off.",
    params(("id" = i64, Path, description = "Title id")),
    responses((status = 200, body = GameCommunity), (status = 404, description = "No such title")))]
pub(super) async fn game_verdicts(State(state): State<AppState>, Me(me): Me, Path(id): Path<i64>) -> Result<Json<GameCommunity>, ApiError> {
    if state.db.game(me, id)?.is_none() {
        return Err(ApiError::not_found(format!("no title {id}")));
    }
    let verdicts = state.db.community_verdicts_for(id)?.as_ref().and_then(GameVerdicts::of);
    Ok(Json(GameCommunity { status: status_of(&state)?, verdicts }))
}
