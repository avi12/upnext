//! Backlog verdicts, sending side (docs/api/verdicts.md and
//! verdicts-sending.md): the form for a game, drafts, the consent screen, sending,
//! taking down and reporting. The service's address is the `verdicts_url`
//! setting; with none, nothing is sent.

use super::players::Me;
use super::{ApiError, AppState};
use crate::verdicts::client::RemoteProblem;
use crate::verdicts::mine::{
    self, MyVerdictView, Refused, VerdictAnswers, VerdictConsent, VerdictForm, VerdictSent, VerdictsOverview, VerdictsRefreshed,
    VerdictsWithdrawnAll,
};
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/verdicts", get(overview))
        .route("/verdicts/vocab", get(vocab))
        .route("/verdicts/consent", get(consent_get).post(consent_post))
        .route("/verdicts/draft/{title_id}", get(form))
        .route("/verdicts/withdraw-all", post(withdraw_all))
        .route("/verdicts/refresh", post(refresh))
        .route("/verdicts/reports", post(report))
        .route("/verdicts/{title_id}", put(save).delete(withdraw))
        .route("/verdicts/{title_id}/send", post(send))
        // A verdict is a few hundred bytes; a draft is kept as sent, so
        // nothing much bigger is taken.
        .layer(DefaultBodyLimit::max(VERDICT_BODY_MAX))
}

/// The largest request body the verdict routes take.
const VERDICT_BODY_MAX: usize = 16 * 1024;

/// An error from a verdict action: a refusal says what the form shows, with
/// what's wrong field by field.
pub struct VerdictError(anyhow::Error);

impl From<anyhow::Error> for VerdictError {
    fn from(e: anyhow::Error) -> Self {
        VerdictError(e)
    }
}

impl IntoResponse for VerdictError {
    fn into_response(self) -> Response {
        match self.0.downcast::<Refused>() {
            Ok(r) => {
                let status = StatusCode::from_u16(r.status).unwrap_or(StatusCode::BAD_GATEWAY);
                let mut body = json!({ "error": r.error });
                if !r.problems.is_empty() {
                    body["problems"] = json!(r.problems);
                }
                (status, Json(body)).into_response()
            }
            Err(e) => ApiError::from(e).into_response(),
        }
    }
}

type VerdictResult<T> = Result<Json<T>, VerdictError>;

/// A refused verdict's body: the error, and what's wrong field by field.
#[derive(Serialize, utoipa::ToSchema)]
#[allow(dead_code)]
pub struct VerdictRefusal {
    error: String,
    problems: Vec<RemoteProblem>,
}

#[utoipa::path(get, path = "/verdicts", tag = "verdicts",
    description = "Your backlog verdicts as this computer keeps them, newest change first, with whether a verdicts service is \
        set, whether you have a key (made the first time you send or report; never shown) and the consent screen.",
    responses((status = 200, body = VerdictsOverview)))]
pub(super) async fn overview(State(state): State<AppState>, Me(me): Me) -> VerdictResult<VerdictsOverview> {
    Ok(Json(mine::overview(&state.db, &state.cfg_for(me), me)?))
}

#[utoipa::path(get, path = "/verdicts/vocab", tag = "verdicts",
    description = "The verdicts' contract, as `docs/verdicts-contract.json` holds it: the questions and their answers with \
        their words, the limits, the text checks (patterns the form runs as you type), the consent text and the report reasons.",
    responses((status = 200, body = Object, description = "The contract")))]
pub(super) async fn vocab() -> Json<Value> {
    Json(crate::verdicts::contract())
}

#[utoipa::path(get, path = "/verdicts/consent", tag = "verdicts",
    description = "The consent screen shown before your first verdict is sent, and whether you accepted its current version.",
    responses((status = 200, body = VerdictConsent)))]
pub(super) async fn consent_get(State(state): State<AppState>, Me(me): Me) -> VerdictResult<VerdictConsent> {
    Ok(Json(mine::consent_view(&state.db, &state.cfg_for(me), me)?))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ConsentBody {
    /// The consent text's version you read and accept.
    version: i64,
}

#[utoipa::path(post, path = "/verdicts/consent", tag = "verdicts",
    description = "Accept the consent screen (\"Publish my verdicts under CC0\"): everything you send is public under CC0 1.0. \
        Sends nothing. The version must be the current one.",
    request_body = ConsentBody,
    responses((status = 200, body = VerdictConsent), (status = 400, body = VerdictRefusal, description = "Not the current version"),
        (status = 413, description = "A body over 16 KB")))]
pub(super) async fn consent_post(
    State(state): State<AppState>,
    Me(me): Me,
    Json(body): Json<ConsentBody>,
) -> VerdictResult<VerdictConsent> {
    Ok(Json(mine::accept_consent(&state.db, &state.cfg_for(me), me, body.version)?))
}

#[utoipa::path(get, path = "/verdicts/draft/{title_id}", tag = "verdicts",
    description = "The verdict form for a game: your saved or sent verdict, or the answers UpNext fills in from your mark, \
        playtime, play diary and stop reasons, with where each came from and what's left before it can be sent.",
    params(("title_id" = i64, Path, description = "Title id")),
    responses((status = 200, body = VerdictForm), (status = 404, body = VerdictRefusal, description = "No such title")))]
pub(super) async fn form(State(state): State<AppState>, Me(me): Me, Path(title_id): Path<i64>) -> VerdictResult<VerdictForm> {
    Ok(Json(mine::form(&state.db, &state.cfg_for(me), me, title_id)?))
}

#[utoipa::path(put, path = "/verdicts/{title_id}", tag = "verdicts",
    description = "Save your answers about a game as a draft, on this computer only. A sent verdict is replaced by sending \
        it again, not by saving (409).",
    params(("title_id" = i64, Path, description = "Title id")),
    request_body = VerdictAnswers,
    responses((status = 200, body = MyVerdictView),
        (status = 400, body = VerdictRefusal, description = "The game has neither a Steam appid nor a GOG id"),
        (status = 404, body = VerdictRefusal, description = "No such title"),
        (status = 409, body = VerdictRefusal, description = "The verdict is sent"),
        (status = 413, description = "A body over 16 KB")))]
pub(super) async fn save(
    State(state): State<AppState>,
    Me(me): Me,
    Path(title_id): Path<i64>,
    Json(body): Json<VerdictAnswers>,
) -> VerdictResult<MyVerdictView> {
    Ok(Json(mine::save_draft(&state.db, &state.cfg_for(me), me, title_id, body)?))
}

#[utoipa::path(post, path = "/verdicts/{title_id}/send", tag = "verdicts",
    description = "Send your verdict about a game to the verdicts service, replacing the one you sent before. It's published \
        with the next daily snapshot, under CC0. `200` when the service took it (`state` sent, or held while the game gets a \
        rush of verdicts); `202` when the service is busy or out of reach, so it's queued and sent again while UpNext runs. \
        Needs the consent accepted first (409) and a service address (503).",
    params(("title_id" = i64, Path, description = "Title id")),
    request_body = VerdictAnswers,
    responses((status = 200, body = VerdictSent), (status = 202, body = VerdictSent, description = "Queued"),
        (status = 400, body = VerdictRefusal, description = "What's wrong with it, field by field"),
        (status = 403, body = VerdictRefusal, description = "The service blocked this player's key"),
        (status = 404, body = VerdictRefusal, description = "No such title"),
        (status = 409, body = VerdictRefusal, description = "The consent isn't accepted"),
        (status = 502, body = VerdictRefusal, description = "The service refused it for a reason the form can't fix"),
        (status = 503, body = VerdictRefusal, description = "No verdicts service is set"),
        (status = 413, description = "A body over 16 KB")))]
pub(super) async fn send(
    State(state): State<AppState>,
    Me(me): Me,
    Path(title_id): Path<i64>,
    Json(body): Json<VerdictAnswers>,
) -> Result<(StatusCode, Json<VerdictSent>), VerdictError> {
    let sent = mine::send(&state.db, &state.clients.http, &state.cfg_for(me), me, title_id, body).await?;
    let status = if sent.verdict.state.is_out() { StatusCode::OK } else { StatusCode::ACCEPTED };
    Ok((status, Json(sent)))
}

/// What taking a verdict down left.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct VerdictWithdrawn {
    /// The verdict, now withdrawn; null for one never sent, which is forgotten.
    verdict: Option<MyVerdictView>,
}

#[utoipa::path(delete, path = "/verdicts/{title_id}", tag = "verdicts",
    description = "Take your verdict about a game down: the service leaves it out of the next daily snapshot and deletes it \
        from its servers within a week. A draft or a queued verdict never sent is just forgotten.",
    params(("title_id" = i64, Path, description = "Title id")),
    responses((status = 200, body = VerdictWithdrawn),
        (status = 404, body = VerdictRefusal, description = "No verdict of yours about this game"),
        (status = 409, body = VerdictRefusal, description = "Taken down already, or the key that sent it is gone"),
        (status = 502, body = VerdictRefusal, description = "The service couldn't be reached; nothing changed"),
        (status = 503, body = VerdictRefusal, description = "No verdicts service is set")))]
pub(super) async fn withdraw(State(state): State<AppState>, Me(me): Me, Path(title_id): Path<i64>) -> VerdictResult<VerdictWithdrawn> {
    let verdict = mine::withdraw(&state.db, &state.clients.http, &state.cfg_for(me), me, title_id).await?;
    Ok(Json(VerdictWithdrawn { verdict }))
}

#[utoipa::path(post, path = "/verdicts/withdraw-all", tag = "verdicts",
    description = "Take down all your verdicts and forget your key. Queued verdicts go back to being drafts.",
    responses((status = 200, body = VerdictsWithdrawnAll),
        (status = 502, body = VerdictRefusal, description = "The service couldn't be reached; nothing changed and the key is kept")))]
pub(super) async fn withdraw_all(State(state): State<AppState>, Me(me): Me) -> VerdictResult<VerdictsWithdrawnAll> {
    Ok(Json(mine::withdraw_all(&state.db, &state.clients.http, &state.cfg_for(me), me).await?))
}

#[utoipa::path(post, path = "/verdicts/refresh", tag = "verdicts",
    description = "Ask the service which verdicts your key sent, and bring this computer's list up to date: verdicts it lost \
        come back, and held or removed ones say so.",
    responses((status = 200, body = VerdictsRefreshed),
        (status = 502, body = VerdictRefusal, description = "The service couldn't be reached"),
        (status = 503, body = VerdictRefusal, description = "No verdicts service is set")))]
pub(super) async fn refresh(State(state): State<AppState>, Me(me): Me) -> VerdictResult<VerdictsRefreshed> {
    Ok(Json(mine::refresh(&state.db, &state.clients.http, &state.cfg_for(me), me).await?))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ReportBody {
    /// The published verdict's id.
    verdict_id: String,
    /// spam, personal (personal information), abuse (abuse or hate) or off_topic (not about this game).
    reason: String,
}

#[utoipa::path(post, path = "/verdicts/reports", tag = "verdicts",
    description = "Report a published verdict's text to the service: spam, personal information, abuse or hate, or not about \
        the game. Your key counts once per verdict, so the consent screen, which tells of the key, comes first.",
    request_body = ReportBody,
    responses((status = 202, body = Object, description = "`{}`"),
        (status = 400, body = VerdictRefusal, description = "An unknown reason or not a verdict id"),
        (status = 404, body = VerdictRefusal, description = "Not a published verdict"),
        (status = 409, body = VerdictRefusal, description = "The consent isn't accepted; reports go with the player's key"),
        (status = 429, body = VerdictRefusal, description = "Today's reports are used up"),
        (status = 502, body = VerdictRefusal, description = "The service couldn't be reached"),
        (status = 503, body = VerdictRefusal, description = "No verdicts service is set"),
        (status = 413, description = "A body over 16 KB")))]
pub(super) async fn report(
    State(state): State<AppState>,
    Me(me): Me,
    Json(body): Json<ReportBody>,
) -> Result<(StatusCode, Json<Value>), VerdictError> {
    mine::report(&state.db, &state.clients.http, &state.cfg_for(me), me, &body.verdict_id, &body.reason).await?;
    Ok((StatusCode::ACCEPTED, Json(json!({}))))
}
