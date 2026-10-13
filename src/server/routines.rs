//! Routines (docs/api/routines.md): named sessions on the Profile,
//! offered on the Pick page with the one that fits the time now.

use super::players::Me;
use super::{ApiError, ApiResult, AppState};
use crate::taste::routine::{self, Routine, RoutineBody, Routines};
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Json, Router};

pub fn routes() -> Router<AppState> {
    use axum::routing::{get, put};
    Router::new().route("/routines", get(list).post(create)).route("/routines/{id}", put(replace).delete(delete))
}

fn bad(e: anyhow::Error) -> ApiError {
    ApiError::bad(format!("{e:#}"))
}

/// A body it can't read (a day or a "who" it doesn't know, a number where a
/// word goes, JSON that doesn't parse) is a routine it can't take, so it's
/// the same 400 with the reason as JSON, not the extractor's 422 in plain
/// text. A body that isn't sent as JSON keeps the extractor's 415.
fn unreadable(e: JsonRejection) -> ApiError {
    let status = match e {
        JsonRejection::MissingJsonContentType(_) => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        _ => StatusCode::BAD_REQUEST,
    };
    ApiError(status, e.body_text())
}

fn not_found(id: i64) -> ApiError {
    ApiError::not_found(format!("no routine {id}"))
}

/// Every routine, and the one that fits now.
#[utoipa::path(get, path = "/routines", tag = "taste", operation_id = "routines_list",
    description = "Your routines, in the order they were made, with `now`: the id of the one that fits the day and time of day \
        now (the narrowest when several do; the small hours count as the night before), or null.",
    responses((status = 200, body = Routines)))]
async fn list(State(state): State<AppState>, Me(me): Me) -> ApiResult<Routines> {
    let routines = state.db.routines(me)?;
    let now = routine::for_now(&routines, crate::db::now()).map(|r| r.id);
    Ok(Json(Routines { routines, now }))
}

/// Make a routine.
#[utoipa::path(post, path = "/routines", tag = "taste", operation_id = "routines_create",
    description = "Make a routine: a name of its own (up to 40 characters), and any of days, times of day, who with, where, \
        what with, a length (15 to 720 minutes) and a note. At most 20.",
    request_body = RoutineBody,
    responses((status = 201, body = Routine), (status = 400, description = "A routine it can't take, or a body that isn't one, saying why"),
        (status = 415, description = "A body not sent as `application/json`")))]
async fn create(
    State(state): State<AppState>,
    Me(me): Me,
    body: Result<Json<RoutineBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Routine>), ApiError> {
    let Json(body) = body.map_err(unreadable)?;
    let body = routine::check(body, &state.db.routines(me)?).map_err(bad)?;
    Ok((StatusCode::CREATED, Json(state.db.add_routine(me, &body)?)))
}

/// Replace a routine.
#[utoipa::path(put, path = "/routines/{id}", tag = "taste", operation_id = "routines_replace",
    description = "Replace a routine with the one sent, which is checked as a new one is. Lineups already picked for it keep the \
        routine as it was.",
    params(("id" = i64, Path, description = "The routine's id")),
    request_body = RoutineBody,
    responses((status = 200, body = Routine), (status = 400, description = "A routine it can't take, or a body that isn't one, saying why"),
        (status = 415, description = "A body not sent as `application/json`"),
        (status = 404, description = "No such routine")))]
async fn replace(
    State(state): State<AppState>,
    Me(me): Me,
    Path(id): Path<i64>,
    body: Result<Json<RoutineBody>, JsonRejection>,
) -> ApiResult<Routine> {
    let Json(body) = body.map_err(unreadable)?;
    let (others, this): (Vec<Routine>, Vec<Routine>) = state.db.routines(me)?.into_iter().partition(|r| r.id != id);
    if this.is_empty() {
        return Err(not_found(id));
    }
    let body = routine::check(body, &others).map_err(bad)?;
    Ok(Json(state.db.set_routine(me, id, &body)?.ok_or_else(|| not_found(id))?))
}

/// Delete a routine.
#[utoipa::path(delete, path = "/routines/{id}", tag = "taste", operation_id = "routines_delete",
    description = "Delete a routine. Lineups already picked for it keep the routine as it was.",
    params(("id" = i64, Path, description = "The routine's id")),
    responses((status = 204, description = "Deleted"), (status = 404, description = "No such routine")))]
async fn delete(State(state): State<AppState>, Me(me): Me, Path(id): Path<i64>) -> Result<StatusCode, ApiError> {
    if state.db.delete_routine(me, id)? { Ok(StatusCode::NO_CONTENT) } else { Err(not_found(id)) }
}
