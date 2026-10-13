//! Picks: making a lineup, its cards' actions, past lineups, and Versus'
//! Help me decide, with the statuses their errors answer with.

use super::games::ReasonsBody;
use super::players::Me;
use super::{ApiError, ApiResult, AppState};
use crate::lineup::{self, LineupError};
use crate::llm::LlmError;
use crate::recommend::{self, PickError, PickRequest};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route("/versus/decide", post(versus_decide))
        .route("/picks", post(picks))
        .route("/picks/history", get(history))
        .route("/picks/{batch_id}/feedback", post(pick_feedback))
        .route("/picks/{batch_id}/too", post(pick_too))
        .route("/picks/{batch_id}/more", post(pick_more))
        .route("/picks/{batch_id}/switch", post(pick_switch))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct DecideBody {
    ids: Vec<i64>,
    #[serde(default)]
    prompt: Option<String>,
}

/// Versus "Help me decide": the AI picks one of two games for this person.
#[utoipa::path(post, path = "/versus/decide", tag = "picks",
    description = "Versus' \"Help me decide\": the AI picks one of two games and says why.",
    request_body = DecideBody,
    responses((status = 200, body = Object, description = "`{pick_id, reason, model, usage, cost_usd, api_usd}`"),
        (status = 400, description = "Not exactly two different game ids, or a `prompt` over 2,000 characters"),
        (status = 404, description = "No such title"),
        (status = 502, description = "The AI failed or gave no usable answer"),
        (status = 503, description = "No AI is set up")))]
async fn versus_decide(State(state): State<AppState>, Me(me): Me, Json(body): Json<DecideBody>) -> ApiResult<Value> {
    let cfg = state.cfg_for(me);
    let llm = state.llm_for(me);
    let d = crate::decide::decide(&state.db, &cfg, llm.as_ref(), me, &body.ids, body.prompt.as_deref()).await?;
    Ok(Json(json!({
        "pick_id": d.pick_id, "reason": d.reason, "model": d.model, "usage": d.usage,
        "cost_usd": d.cost_usd, "api_usd": d.api_usd,
    })))
}

impl From<crate::decide::DecideError> for ApiError {
    fn from(e: crate::decide::DecideError) -> Self {
        use crate::decide::DecideError;
        match e {
            DecideError::Bad(m) => ApiError::bad(m),
            DecideError::NotFound(m) => ApiError::not_found(m),
            DecideError::NotAvailable(m) => ApiError(StatusCode::SERVICE_UNAVAILABLE, m),
            DecideError::Failed(m) => ApiError::bad_gateway(m),
            DecideError::Other(e) => e.into(),
        }
    }
}

impl From<PickError> for ApiError {
    fn from(e: PickError) -> Self {
        match e {
            PickError::NoCandidates(m) | PickError::TooLarge(m) => ApiError::bad(m),
            PickError::NotFound(m) => ApiError::not_found(m),
            PickError::Llm(LlmError::NotAvailable(m)) => ApiError(StatusCode::SERVICE_UNAVAILABLE, m),
            PickError::Llm(LlmError::LimitReached(m)) => ApiError(StatusCode::TOO_MANY_REQUESTS, m),
            e @ (PickError::Llm(_) | PickError::Invalid(_)) => ApiError::bad_gateway(e.to_string()),
            PickError::Other(e) => e.into(),
        }
    }
}

impl From<LineupError> for ApiError {
    fn from(e: LineupError) -> Self {
        match e {
            LineupError::NotFound(m) => ApiError::not_found(m),
            LineupError::Bad(m) => ApiError::bad(m),
            LineupError::Conflict(m) => ApiError::conflict(m),
            LineupError::Other(e) => e.into(),
        }
    }
}

#[utoipa::path(post, path = "/picks", tag = "picks",
    description = "Make a lineup of picks for a request, by the AI or, without one, by UpNext's own ranking. `mode` `random` draws games off your radar by chance instead, with no AI call.",
    request_body = PickRequest,
    responses((status = 200, body = crate::db::PickBatch),
        (status = 400, description = "No candidates (sync and enrich first), or a request over the limits: a prompt over 2,000 characters, more than 50 reasons in `avoid` or one over 300 characters, more than 500 `exclude_ids`"),
        (status = 404, description = "Something the request names wasn't found, such as its `routine_id`"),
        (status = 429, description = "The AI's provider takes no more calls for now"),
        (status = 502, description = "The AI failed or returned invalid picks twice"),
        (status = 503, description = "The AI stopped being available during the call")))]
async fn picks(State(state): State<AppState>, Me(me): Me, Json(req): Json<PickRequest>) -> ApiResult<crate::db::PickBatch> {
    let cfg = state.cfg_for(me);
    Ok(Json(recommend::pick(&state.db, &cfg, state.llm_for(me).as_ref(), Some(&state.clients), req).await?))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct FeedbackBody {
    title_id: i64,
    action: String,
    text: Option<String>,
    /// With `started` and shelved, abandoned or burnt_out: why it stopped.
    reasons: Option<ReasonsBody>,
}

/// A pick's action on its card: Play, Not tonight, Never
/// recommend this, "I've started it…".
#[utoipa::path(post, path = "/picks/{batch_id}/feedback", tag = "picks",
    description = "A pick's action on its card: chosen (Play), not_tonight, not_for_me, or started (\"I've started it…\").",
    params(("batch_id" = String, Path, description = "The lineup's id")),
    request_body = FeedbackBody,
    responses((status = 200, body = crate::db::PickBatch),
        (status = 400, description = "An unknown action or reason"),
        (status = 404, description = "No such lineup, or the game isn't in it"),
        (status = 409, description = "Another action changed the lineup first; look at it again and retry")))]
async fn pick_feedback(
    State(state): State<AppState>,
    Me(me): Me,
    Path(batch_id): Path<String>,
    Json(body): Json<FeedbackBody>,
) -> ApiResult<crate::db::PickBatch> {
    let action =
        crate::db::FeedbackAction::parse(&body.action).ok_or_else(|| ApiError::bad(format!("unknown action {:?}", body.action)))?;
    let reasons = match &body.reasons {
        Some(r) => crate::lifecycle::check_reasons(&r.codes, r.note.as_deref()).map_err(|e| ApiError::bad(e.to_string()))?,
        None => None,
    };
    let f = lineup::Feedback { title_id: body.title_id, action, text: body.text, reasons };
    Ok(Json(lineup::feedback(&state.db, me, &batch_id, f)?))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct TooBody {
    title_id: i64,
    word: String,
}

/// "It's too ___" on a pick: spares without the problem, chosen by the AI.
#[utoipa::path(post, path = "/picks/{batch_id}/too", tag = "picks",
    description = "\"It's too ___\" on a pick: spares without the problem take its place, chosen by the AI.",
    params(("batch_id" = String, Path, description = "The lineup's id")),
    request_body = TooBody,
    responses((status = 200, body = crate::lineup::TooOutcome),
        (status = 400, description = "A word it can't use"),
        (status = 404, description = "No such lineup, or the game isn't on screen"),
        (status = 409, description = "Another action changed the lineup first; look at it again and retry")))]
async fn pick_too(
    State(state): State<AppState>,
    Me(me): Me,
    Path(batch_id): Path<String>,
    Json(body): Json<TooBody>,
) -> ApiResult<lineup::TooOutcome> {
    let cfg = state.cfg_for(me);
    Ok(Json(lineup::too(&state.db, &cfg, state.llm_for(me).as_ref(), &batch_id, body.title_id, &body.word).await?))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SwitchBody {
    /// The wishlisted pick's id (minus its wishlist entry's id).
    title_id: i64,
}

/// "Switch with owned game" on a wishlisted pick.
#[utoipa::path(post, path = "/picks/{batch_id}/switch", tag = "picks",
    description = "\"Switch with owned game\" on a wishlisted pick: the owned game the pick call chose with it takes its slot, with no AI call.",
    params(("batch_id" = String, Path, description = "The lineup's id")),
    request_body = SwitchBody,
    responses((status = 200, body = crate::db::PickBatch),
        (status = 400, description = "A game you own, or one with nothing to switch to"),
        (status = 404, description = "No such lineup, or the game isn't on screen"),
        (status = 409, description = "Another action changed the lineup first; look at it again and retry")))]
async fn pick_switch(
    State(state): State<AppState>,
    Me(me): Me,
    Path(batch_id): Path<String>,
    Json(body): Json<SwitchBody>,
) -> ApiResult<crate::db::PickBatch> {
    Ok(Json(lineup::switch(&state.db, me, &batch_id, body.title_id)?))
}

#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
pub struct MoreBody {
    #[serde(default)]
    sources: Option<Vec<String>>,
    /// What it's played on, as for a pick: the lineup doesn't keep it, so
    /// big screen sends `controller` again.
    #[serde(default)]
    input: Option<crate::taste::routine::Input>,
}

/// New spares for a lineup whose spares ran out, filling its empty slots.
#[utoipa::path(post, path = "/picks/{batch_id}/more", tag = "picks",
    description = "New spares for a lineup whose spares ran out, filling its empty slots.",
    params(("batch_id" = String, Path, description = "The lineup's id")),
    request_body = Option<MoreBody>,
    responses((status = 200, body = crate::db::PickBatch),
        (status = 400, description = "No candidates"),
        (status = 404, description = "No such lineup"),
        (status = 429, description = "The AI's provider takes no more calls for now"),
        (status = 502, description = "The AI failed or returned invalid picks twice"),
        (status = 503, description = "The AI stopped being available during the call")))]
async fn pick_more(
    State(state): State<AppState>,
    Me(me): Me,
    Path(batch_id): Path<String>,
    body: Option<Json<MoreBody>>,
) -> ApiResult<crate::db::PickBatch> {
    let cfg = state.cfg_for(me);
    let Json(body) = body.unwrap_or_default();
    Ok(Json(recommend::more(&state.db, &cfg, state.llm_for(me).as_ref(), &batch_id, body.sources, body.input).await?))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct HistoryQuery {
    /// Lineups listed: 20 unless asked (1 to 200).
    limit: Option<usize>,
    /// Lineups skipped first, for the next page: 0 unless asked.
    offset: Option<usize>,
    /// `newest` first (the default) or `oldest` first.
    #[param(inline)]
    order: Option<HistoryOrder>,
    /// Only lineups made at or after this time, in Unix seconds.
    since: Option<i64>,
    /// Only lineups whose request, or a game they showed, has every word of
    /// this (up to eight words; case is ignored for ASCII letters).
    q: Option<String>,
    /// Only lineups with this outcome.
    #[param(inline)]
    outcome: Option<crate::db::HistoryOutcome>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum HistoryOrder {
    Newest,
    Oldest,
}

#[utoipa::path(get, path = "/picks/history", tag = "picks",
    description = "Past lineups, newest first unless asked, a page at a time, with a time, words and an outcome to filter by. \
        A page shorter than `limit` is the last.",
    params(HistoryQuery),
    responses((status = 200, body = Vec<crate::db::PickBatch>), (status = 400, description = "A value the query can't take")))]
async fn history(State(state): State<AppState>, Me(me): Me, Query(q): Query<HistoryQuery>) -> ApiResult<Vec<crate::db::PickBatch>> {
    let find = crate::db::HistoryFind {
        limit: q.limit.unwrap_or(20).clamp(1, 200),
        offset: q.offset.unwrap_or(0),
        oldest_first: q.order == Some(HistoryOrder::Oldest),
        since: q.since,
        search: q.q,
        outcome: q.outcome,
    };
    Ok(Json(state.db.find_history(me, &find)?))
}
