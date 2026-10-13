//! Ratings (docs/api/ratings.md): a pick's thumbs on its card,
//! and your own rating of a game on its page.

use super::players::Me;
use super::{ApiError, ApiResult, AppState, game_or_404};
use crate::db::{GameView, MissReason, PickBatch, PickVerdict};
use axum::extract::{Path, State};
use axum::{Json, Router};
use serde::Deserialize;

pub fn routes() -> Router<AppState> {
    use axum::routing::put;
    Router::new().route("/picks/{batch_id}/rating", put(pick_rating)).route("/games/{id}/rating", put(game_rating))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct PickRatingBody {
    /// The pick's title id (minus its wishlist entry's id for a wishlisted pick).
    title_id: i64,
    /// good ("Good pick") or not_a_fit ("Not a fit"); null takes the rating back.
    verdict: Option<String>,
    /// With not_a_fit, why: wrong_length, wrong_mood, wrong_genre or played_out.
    reason: Option<String>,
}

/// How well a pick fit its lineup's request: the thumbs on its card.
#[utoipa::path(put, path = "/picks/{batch_id}/rating", tag = "picks",
    description = "Rate how well a pick fit its lineup's request: `good` (\"Good pick\") or `not_a_fit` (\"Not a fit\", with an \
        optional reason), or take the rating back with a null verdict. A rating is not a card action: the pick stays where it is. \
        Any pick of the lineup can be rated, spares and picks taken off included. Answers with the lineup.",
    params(("batch_id" = String, Path, description = "The lineup's id")),
    request_body = PickRatingBody,
    responses((status = 200, body = PickBatch),
        (status = 400, description = "An unknown verdict or reason, or a reason with a good pick"),
        (status = 404, description = "No such lineup, or the game isn't in it")))]
async fn pick_rating(
    State(state): State<AppState>,
    Me(me): Me,
    Path(batch_id): Path<String>,
    Json(body): Json<PickRatingBody>,
) -> ApiResult<PickBatch> {
    let verdict = match body.verdict.as_deref() {
        None => None,
        Some(v) => Some(PickVerdict::parse(v).ok_or_else(|| ApiError::bad(format!("unknown verdict {v:?}: good or not_a_fit")))?),
    };
    let reason = match body.reason.as_deref() {
        None => None,
        Some(r) => Some(
            MissReason::parse(r)
                .ok_or_else(|| ApiError::bad(format!("unknown reason {r:?}: wrong_length, wrong_mood, wrong_genre or played_out")))?,
        ),
    };
    if reason.is_some() && verdict != Some(PickVerdict::NotAFit) {
        return Err(ApiError::bad("a reason goes with not_a_fit"));
    }
    let not_found = || ApiError::not_found(format!("no game {} in lineup {batch_id}", body.title_id));
    if state.db.pick_batch(me, &batch_id)?.is_none() || !state.db.set_pick_rating(&batch_id, body.title_id, verdict.map(|v| (v, reason)))? {
        return Err(not_found());
    }
    Ok(Json(state.db.pick_batch(me, &batch_id)?.ok_or_else(not_found)?))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct GameRatingBody {
    /// 1 ("Disliked it") to 5 ("Loved it"); null clears it.
    #[schema(minimum = 1, maximum = 5)]
    rating: Option<i64>,
}

/// Your own rating of a game, 1 to 5 stars, on its page.
#[utoipa::path(put, path = "/games/{id}/rating", tag = "games",
    description = "Your own rating of a game, 1 (\"Disliked it\") to 5 (\"Loved it\") stars, or null to clear it. Picks read it as \
        part of your taste profile.",
    params(("id" = i64, Path, description = "Title id")),
    request_body = GameRatingBody,
    responses((status = 200, body = GameView),
        (status = 400, description = "A rating outside 1 to 5"),
        (status = 404, description = "No such title")))]
async fn game_rating(
    State(state): State<AppState>,
    Me(me): Me,
    Path(id): Path<i64>,
    Json(body): Json<GameRatingBody>,
) -> ApiResult<GameView> {
    let stars = match body.rating {
        None => None,
        Some(n @ 1..=5) => Some(n as u8),
        Some(n) => return Err(ApiError::bad(format!("a rating is 1 to 5 stars, not {n}"))),
    };
    game_or_404(&state, me, id)?;
    state.db.set_game_rating(me, id, stars)?;
    Ok(Json(state.db.game(me, id)?.expect("just checked")))
}
