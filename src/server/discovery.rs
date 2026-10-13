//! Discovery (docs/api/discovery.md and gems.md): the Hidden gems shelf,
//! and a game's ways in for New to you. Picks in either mode go through
//! `POST /api/picks`.

use super::jobs::task_json;
use super::players::Me;
use super::{ApiError, ApiResult, AppState};
use crate::discovery::{gems, new_to_you};
use crate::jobs::Task;
use axum::extract::{Path, Query, State};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    use axum::routing::get;
    Router::new().route("/gems", get(gems_get)).route("/new-to-you/{id}", get(ways_get))
}

#[derive(Debug, Default, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
struct GemsQuery {
    /// Every game in the pool, ranked, for tuning what "unique" means.
    #[serde(default)]
    all: bool,
}

/// The Hidden gems shelf, with the pool behind it, the job that rates it
/// and whether the AI can run it.
#[utoipa::path(get, path = "/gems", tag = "games", operation_id = "gems_get",
    description = "The Hidden gems shelf, with the pool behind it, the job that rates it and whether the AI can run it.",
    params(GemsQuery),
    responses((status = 200, body = Object, description = "The shelf, plus `ai {available, reason}`, `task` and `rules {min_positive, min_reviews, max_hours, count}`")))]
async fn gems_get(State(state): State<AppState>, Me(me): Me, Query(q): Query<GemsQuery>) -> ApiResult<Value> {
    let cfg = state.cfg_for(me);
    let games = state.db.all_games_shared(me)?;
    let profile = crate::taste::Profile::load(&state.db, me, &games)?;
    let rules = gems::Rules::from_config(&cfg);
    let shelf = gems::shelf(&games, Some(&profile), &rules, &state.db.hidden_gems()?, q.all);
    let mut v = serde_json::to_value(shelf).map_err(anyhow::Error::from)?;
    v["ai"] = match state.summary_llm().available().await {
        Ok(()) => json!({ "available": true, "reason": null }),
        Err(e) => json!({ "available": false, "reason": e.reason() }),
    };
    v["task"] = task_json(&state, Task::HiddenGems)?;
    v["rules"] = json!({
        "min_positive": rules.min_positive,
        "min_reviews": rules.min_reviews,
        "max_hours": rules.max_hours,
        "count": rules.count,
    });
    Ok(Json(v))
}

/// The genres and mechanics a game is a way into for you now: what "Not
/// for me" on a New to you pick offers to make a hard no.
#[utoipa::path(get, path = "/new-to-you/{id}", tag = "games", operation_id = "new_to_you_ways",
    description = "The genres and mechanics a game is a way into for you now: what \"Not for me\" on a New to you pick offers to make a hard no.",
    params(("id" = i64, Path, description = "Title id")),
    responses((status = 200, body = Object, description = "`{ways: [{about, tag, phrase, kind}]}`"), (status = 404, description = "Not in the library")))]
async fn ways_get(State(state): State<AppState>, Me(me): Me, Path(id): Path<i64>) -> ApiResult<Value> {
    let games = state.db.all_games_shared(me)?;
    if !games.iter().any(|g| g.id == id) {
        return Err(ApiError::not_found(format!("game id {id} is not in your library")));
    }
    let profile = crate::taste::Profile::load(&state.db, me, &games)?;
    let rules = new_to_you::Rules::from_config(&state.cfg_for(me));
    let ways: Vec<Value> = new_to_you::ways_for(&games, id, Some(&profile), &rules)
        .into_iter()
        .map(|w| json!({ "about": w.about(), "tag": w.tag, "phrase": w.phrase, "kind": w.kind }))
        .collect();
    Ok(Json(json!({ "ways": ways })))
}
