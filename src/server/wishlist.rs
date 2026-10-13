//! The recommended wishlist (docs/api/wishlist.md): the
//! suggestions and the wishlist, adding a game, answering a suggestion and
//! forgetting one. With the `wishlist` setting off it shows nothing and
//! changes nothing, except that a row can still be deleted.

use super::jobs::task_json;
use super::players::Me;
use super::{ApiError, ApiResult, AppState};
use crate::db::{WishState, settable_state};
use crate::jobs::Task;
use crate::wishlist::AddError;
use axum::extract::{Path, State};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    use axum::routing::{get, put};
    Router::new()
        .route("/wishlist", get(wishlist_get).post(wishlist_post))
        .route("/wishlist/{id}", put(wishlist_put).delete(wishlist_delete))
}

fn turned_off() -> ApiError {
    ApiError::conflict("the wishlist is turned off (Settings › Picks)")
}

/// Suggestions and the wishlist, with what's behind them.
#[utoipa::path(get, path = "/wishlist", tag = "library", operation_id = "wishlist_get",
    description = "Games you don't own: suggestions from your series waiting for an answer, and the wishlist (yours and your Steam wishlist's). With the `wishlist` setting off, `enabled` is false and the lists are empty.",
    responses((status = 200, body = Object, description = "`{enabled, suggested: [entry], wishlist: [entry], dismissed, owned_elsewhere, in_library, steam_account, task, rules {series_hours, min_positive, min_reviews, per_series}}`")))]
async fn wishlist_get(State(state): State<AppState>, Me(me): Me) -> ApiResult<Value> {
    let cfg = state.cfg_for(me);
    if !cfg.wishlist {
        return Ok(Json(json!({
            "enabled": false,
            "suggested": [],
            "wishlist": [],
            "dismissed": 0,
            "owned_elsewhere": 0,
            "in_library": 0,
        })));
    }
    let games = state.db.all_games_shared(me)?;
    crate::wishlist::settle(&state.db, me, &games)?;
    let collections = crate::series::collections(&games, &state.db.series_guides()?, &state.db.catalog()?);
    let lists = crate::wishlist::lists(&state.db, me, &collections)?;
    let mut v = serde_json::to_value(lists).map_err(anyhow::Error::from)?;
    v["enabled"] = json!(true);
    v["steam_account"] = json!(cfg.steam_id.is_some());
    v["task"] = task_json(&state, Task::Wishlist)?;
    v["rules"] = json!({
        "series_hours": crate::wishlist::SERIES_HOURS,
        "min_positive": crate::wishlist::GOOD_PCT,
        "min_reviews": crate::wishlist::GOOD_REVIEWS,
        "per_series": crate::wishlist::PER_SERIES,
    });
    Ok(Json(v))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct AddBody {
    /// A suggestion (or a game said no to before) to put on the wishlist.
    id: Option<i64>,
    /// A game to find on Steam's store by name and add.
    name: Option<String>,
}

/// Puts a suggestion on the wishlist, or a game found by name. Answers with
/// the wishlist as `GET` does.
#[utoipa::path(post, path = "/wishlist", tag = "library", operation_id = "wishlist_add",
    description = "Puts a suggestion on the wishlist (`id`), or finds a game on Steam's store by name and adds it (`name`). Answers as `GET /wishlist`.",
    request_body = AddBody,
    responses((status = 200, body = Object, description = "The wishlist, as `GET /wishlist`"),
        (status = 400, description = "Neither, a name Steam's store doesn't know, or a game you own"),
        (status = 404, description = "No such entry"),
        (status = 409, description = "In your library now, or the wishlist is turned off"),
        (status = 502, description = "Steam's store didn't answer the one quick try to look the name up")))]
async fn wishlist_post(State(state): State<AppState>, Me(me): Me, Json(body): Json<AddBody>) -> ApiResult<Value> {
    if !state.cfg_for(me).wishlist {
        return Err(turned_off());
    }
    match (body.id, body.name) {
        (Some(id), None) => {
            let row = state.db.wishlist_row(me, id)?.ok_or_else(|| ApiError::not_found(format!("wishlist entry {id} not found")))?;
            if row.state == WishState::InLibrary {
                return Err(ApiError::conflict(format!("{} is in your library now", row.name)));
            }
            state.db.wishlist_set_state(id, WishState::Wishlisted)?;
        }
        (None, Some(name)) => {
            let games = state.db.all_games_shared(me)?;
            crate::wishlist::add_by_name(&state.db, me, &state.clients, &games, &name).await.map_err(|e| match e {
                AddError::Bad(m) => ApiError::bad(m),
                e @ AddError::Unreachable(..) => ApiError::bad_gateway(e.to_string()),
                AddError::Other(e) => e.into(),
            })?;
        }
        _ => return Err(ApiError::bad("send an id or a name")),
    }
    wishlist_get(State(state), Me(me)).await
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct StateBody {
    /// suggested, wishlisted, dismissed (Not interested) or owned_elsewhere.
    state: String,
}

/// Answers a suggestion, or changes a wishlisted game's state.
#[utoipa::path(put, path = "/wishlist/{id}", tag = "library", operation_id = "wishlist_set",
    description = "Answers a suggestion (Add to wishlist, Not interested, I own it elsewhere) or changes a wishlisted game's state. Answers as `GET /wishlist`.",
    params(("id" = i64, Path, description = "Wishlist entry id")),
    request_body = StateBody,
    responses((status = 200, body = Object, description = "The wishlist, as `GET /wishlist`"),
        (status = 400, description = "A state that can't be set"),
        (status = 404, description = "No such entry"),
        (status = 409, description = "In your library now, or the wishlist is turned off")))]
async fn wishlist_put(State(state): State<AppState>, Me(me): Me, Path(id): Path<i64>, Json(body): Json<StateBody>) -> ApiResult<Value> {
    if !state.cfg_for(me).wishlist {
        return Err(turned_off());
    }
    let next = settable_state(&body.state).map_err(|e| ApiError::bad(e.to_string()))?;
    let row = state.db.wishlist_row(me, id)?.ok_or_else(|| ApiError::not_found(format!("wishlist entry {id} not found")))?;
    if row.state == WishState::InLibrary {
        return Err(ApiError::conflict(format!("{} is in your library now", row.name)));
    }
    state.db.wishlist_set_state(id, next)?;
    wishlist_get(State(state), Me(me)).await
}

/// Forgets one entry entirely: a collection may suggest it again.
#[utoipa::path(delete, path = "/wishlist/{id}", tag = "library", operation_id = "wishlist_forget",
    description = "Forgets one entry entirely, so a series may suggest it again. Works with the wishlist turned off. Answers as `GET /wishlist`.",
    params(("id" = i64, Path, description = "Wishlist entry id")),
    responses((status = 200, body = Object, description = "The wishlist, as `GET /wishlist`"),
        (status = 404, description = "No such entry")))]
async fn wishlist_delete(State(state): State<AppState>, Me(me): Me, Path(id): Path<i64>) -> ApiResult<Value> {
    // Only the player's own entries: the ids count across players.
    if state.db.wishlist_row(me, id)?.is_none() || !state.db.wishlist_delete(id)? {
        return Err(ApiError::not_found(format!("wishlist entry {id} not found")));
    }
    wishlist_get(State(state), Me(me)).await
}
