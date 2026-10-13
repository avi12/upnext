//! The taste profile (docs/api/taste.md). `/api/profile`
//! is the Steam profile, so these live under `/api/taste`.

use super::jobs::task_json;
use super::players::Me;
use super::{ApiError, ApiResult, AppState};
use crate::jobs::Task;
use crate::taste::{self, EditBody, StatementBody};
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route("/taste", get(page))
        .route("/taste/statements", post(create))
        .route("/taste/statements/{id}", axum::routing::put(edit).delete(delete))
        .route("/taste/sources", get(sources))
        .route("/taste/sources/{source}", get(source_rows).delete(delete_source))
        .route("/taste/export", get(export))
        .route("/taste/questions", get(questions))
        .route("/taste/diary", axum::routing::put(diary_switch))
        .route("/taste/diary/launch", post(diary_launch))
}

fn bad(e: anyhow::Error) -> ApiError {
    ApiError::bad(format!("{e:#}"))
}

fn not_found(id: i64) -> ApiError {
    ApiError::not_found(format!("nothing in your profile has id {id}"))
}

/// The profile, with the job that counts the patterns.
#[utoipa::path(get, path = "/taste", tag = "taste", operation_id = "taste_get",
    description = "The taste profile: what you said, the patterns UpNext counted, suggestions, the play diary, with \
        `task` (the job that counts the patterns) and `rules` (`half_life_days`, `min_games`).",
    responses((status = 200, body = Object, description = "The profile page, as docs/api/taste.md describes it")))]
async fn page(State(state): State<AppState>, Me(me): Me) -> ApiResult<Value> {
    let mut v = serde_json::to_value(taste::page(&state.db, me)?).map_err(anyhow::Error::from)?;
    v["task"] = task_json(&state, Task::Taste)?;
    let cfg = state.cfg_for(me);
    v["rules"] = json!({ "half_life_days": cfg.pattern_half_life_days, "min_games": cfg.pattern_min_games });
    Ok(Json(v))
}

/// Something you said: a favourite, a like, dislike or hard no, or a habit.
#[utoipa::path(post, path = "/taste/statements", tag = "taste", operation_id = "taste_statement_create",
    description = "Something you said: a favourite, a like, dislike or hard no, or a habit.",
    request_body = crate::taste::StatementBody,
    responses((status = 201, body = Object, description = "The statement row"), (status = 400, description = "A statement it can't take")))]
async fn create(State(state): State<AppState>, Me(me): Me, Json(body): Json<StatementBody>) -> Result<(StatusCode, Json<Value>), ApiError> {
    let new = taste::check_statement(&state.db, me, &body).map_err(bad)?;
    let id = state.db.taste_state(me, &new)?;
    taste::update(&state.db, me, &state.cfg_for(me).taste_rules())?;
    let row = state.db.taste_statement(me, id)?.ok_or_else(|| not_found(id))?;
    Ok((StatusCode::CREATED, Json(serde_json::to_value(row).map_err(anyhow::Error::from)?)))
}

#[utoipa::path(put, path = "/taste/statements/{id}", tag = "taste", operation_id = "taste_statement_edit",
    description = "Edit a statement, or hide or show a pattern.",
    params(("id" = i64, Path, description = "Statement id")),
    request_body = crate::taste::EditBody,
    responses((status = 200, body = Object, description = "The statement row"),
        (status = 400, description = "A change it can't take"), (status = 404, description = "No such statement")))]
async fn edit(State(state): State<AppState>, Me(me): Me, Path(id): Path<i64>, Json(body): Json<EditBody>) -> ApiResult<Value> {
    let row = taste::edit(&state.db, me, &state.cfg_for(me).taste_rules(), id, &body).map_err(bad)?.ok_or_else(|| not_found(id))?;
    Ok(Json(serde_json::to_value(row).map_err(anyhow::Error::from)?))
}

#[utoipa::path(delete, path = "/taste/statements/{id}", tag = "taste", operation_id = "taste_statement_delete",
    description = "Delete a statement.",
    params(("id" = i64, Path, description = "Statement id")),
    responses((status = 204, description = "Deleted"), (status = 400, description = "One that can't be deleted"),
        (status = 404, description = "No such statement")))]
async fn delete(State(state): State<AppState>, Me(me): Me, Path(id): Path<i64>) -> Result<StatusCode, ApiError> {
    taste::delete(&state.db, me, &state.cfg_for(me).taste_rules(), id).map_err(bad)?.ok_or_else(|| not_found(id))?;
    Ok(StatusCode::NO_CONTENT)
}

/// What UpNext knows about you, source by source.
#[utoipa::path(get, path = "/taste/sources", tag = "taste", operation_id = "taste_sources",
    description = "What UpNext knows about you, source by source.",
    responses((status = 200, body = crate::taste::Sources)))]
async fn sources(State(state): State<AppState>, Me(me): Me) -> ApiResult<taste::Sources> {
    Ok(Json(taste::sources(&state.db, &state.cfg_for(me))?))
}

/// The rows behind a source ("See it"); reactions, the play diary and your ratings of games have a list here.
#[utoipa::path(get, path = "/taste/sources/{source}", tag = "taste", operation_id = "taste_source_rows",
    description = "The rows behind a source (\"See it\"): reactions, the play diary and your ratings of games have a list here.",
    params(("source" = String, Path, description = "`reactions`, `diary` or `ratings`")),
    responses((status = 200, body = Object, description = "The source's rows"), (status = 404, description = "That source has no list")))]
async fn source_rows(State(state): State<AppState>, Me(me): Me, Path(source): Path<String>) -> ApiResult<Value> {
    match taste::source_rows(&state.db, me, &source)? {
        Some(rows) => Ok(Json(rows)),
        None => Err(ApiError::not_found(format!("{source:?} has no list here; reactions, diary and ratings do"))),
    }
}

#[utoipa::path(delete, path = "/taste/sources/{source}", tag = "taste", operation_id = "taste_source_delete",
    description = "Delete what a source holds: `statuses`, `reactions` (pick ratings included), `answers`, `diary`, `wishlist` \
        or `ratings` (your ratings of games). `library` can't be deleted here.",
    params(("source" = String, Path, description = "`statuses`, `reactions`, `answers`, `diary`, `wishlist` or `ratings`")),
    responses((status = 200, body = Object, description = "`{deleted}`: how many rows went"),
        (status = 400, description = "`library`, or a source that doesn't exist")))]
async fn delete_source(State(state): State<AppState>, Me(me): Me, Path(source): Path<String>) -> ApiResult<Value> {
    let deleted = taste::delete_source(&state.db, me, &state.cfg_for(me).taste_rules(), &source).map_err(bad)?;
    Ok(Json(json!({ "deleted": deleted })))
}

/// The whole profile as a JSON file.
#[utoipa::path(get, path = "/taste/export", tag = "taste", operation_id = "taste_export",
    description = "The whole profile as a JSON file (`upnext-profile-YYYYMMDD.json`).",
    responses((status = 200, body = Object, description = "`{exported_at, statements, statuses, reactions, play_diary, ratings, routines, sources}`, \
        as an attachment")))]
async fn export(State(state): State<AppState>, Me(me): Me) -> Result<Response, ApiError> {
    let v = taste::export(&state.db, &state.cfg_for(me))?;
    let name = format!("upnext-profile-{}.json", chrono::Local::now().format("%Y%m%d"));
    let body = serde_json::to_vec_pretty(&v).map_err(anyhow::Error::from)?;
    Ok((
        [(header::CONTENT_TYPE, "application/json".to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\""))],
        body,
    )
        .into_response())
}

/// The first-run questions: most-played games, loves, hard nos and the
/// play diary's switch.
#[utoipa::path(get, path = "/taste/questions", tag = "taste", operation_id = "taste_questions",
    description = "The first-run questions: most-played games, loves, hard nos and the play diary's switch.",
    responses((status = 200, body = crate::taste::questions::Questions)))]
async fn questions(State(state): State<AppState>, Me(me): Me) -> ApiResult<taste::questions::Questions> {
    Ok(Json(taste::questions::questions(&state.db, &state.cfg_for(me))?))
}

#[derive(serde::Deserialize, utoipa::ToSchema)]
struct DiarySwitch {
    on: bool,
}

/// Turn the play diary on or off. On takes the first reading at once, so
/// the next hour's has something to count from; a failed one is in the
/// answer's `diary.error` and doesn't stop the switch.
#[utoipa::path(put, path = "/taste/diary", tag = "taste", operation_id = "taste_diary_switch",
    description = "Turn the play diary on or off. On takes the first reading at once; a failed one is in `diary.error` \
        and doesn't stop the switch.",
    request_body = DiarySwitch,
    responses((status = 200, body = Object, description = "`{diary}`, as in GET /taste/sources")))]
async fn diary_switch(State(state): State<AppState>, Me(me): Me, Json(body): Json<DiarySwitch>) -> ApiResult<Value> {
    taste::diary::switch(&state.db, me, body.on)?;
    let cfg = state.cfg_for(me);
    if body.on && cfg.steam_credentials().is_ok() {
        let _ = taste::diary::poll(&state.db, &cfg, &state.clients, crate::db::now()).await;
    }
    Ok(Json(json!({ "diary": taste::diary::state(&state.db, &cfg)? })))
}

#[derive(serde::Deserialize, utoipa::ToSchema)]
struct Launch {
    title_id: i64,
}

/// The Play button was pressed: a start time for the diary, when it's on.
#[utoipa::path(post, path = "/taste/diary/launch", tag = "taste", operation_id = "taste_diary_launch",
    description = "The Play button was pressed: a start time for the play diary, when it's on.",
    request_body = Launch,
    responses((status = 200, body = Object, description = "`{recorded}`: true when the diary is on"),
        (status = 400, description = "A game not in the library")))]
async fn diary_launch(State(state): State<AppState>, Me(me): Me, Json(body): Json<Launch>) -> ApiResult<Value> {
    let written = taste::diary::launch(&state.db, &state.cfg_for(me), body.title_id, crate::db::now()).map_err(bad)?;
    Ok(Json(json!({ "recorded": written })))
}
