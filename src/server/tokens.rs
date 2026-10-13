//! Settings › Devices and API: the tokens other devices and scripts use
//! (docs/api/pairing.md, "Tokens"). A read token can't see these (`access`).

use super::access::Caller;
use super::{ApiError, ApiResult, AppState};
use crate::db::{NewToken, Scope};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get};
use axum::{Extension, Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    Router::new().route("/tokens", get(list).post(create)).route("/tokens/{id}", delete(revoke))
}

/// Every token, and which one this request came with (none from this computer).
#[utoipa::path(get, path = "/tokens", tag = "devices", operation_id = "tokens_list",
    description = "Every token and paired device (never the tokens themselves), and which one this request came with \
        (null from this computer). Needs a full token. A token bound to a player gets only itself.",
    responses((status = 200, body = Object, description = "`{tokens: [ApiToken], current: id | null}`")))]
async fn list(State(state): State<AppState>, caller: Option<Extension<Caller>>) -> ApiResult<Value> {
    let mut tokens = state.db.tokens()?;
    let current = match caller.as_deref() {
        Some(Caller::Token(t)) => {
            // A token bound to a player sees only itself: the list
            // names every other device and whose it is.
            if t.player_id.is_some() {
                tokens.retain(|x| x.id == t.id);
            }
            Some(t.id)
        }
        _ => None,
    };
    Ok(Json(json!({ "tokens": tokens, "current": current })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[schema(as = NewTokenBody)]
struct CreateBody {
    name: String,
    /// "read" or "full".
    scope: String,
    /// Bind it to this player: it acts only for them and can't
    /// change what is everyone's. Without one, it uses the `UpNext-Player`
    /// header or the default player, as a browser does.
    #[serde(default)]
    player_id: Option<i64>,
    /// That player's PIN, when they have one.
    #[serde(default)]
    pin: Option<String>,
}

/// Make a token: the only answer that ever has it in full.
#[utoipa::path(post, path = "/tokens", tag = "devices", operation_id = "tokens_create",
    description = "Make a token: the only answer that ever has it in full. Bound to a player who has a PIN, it needs \
        their `pin`; a wrong one counts against their lockout as the picker's does, a missing one doesn't.",
    request_body = CreateBody,
    responses((status = 201, body = NewToken), (status = 400, description = "A bad or taken name, a scope other than read or full, or an unknown player"),
        (status = 403, description = "The player has a PIN and it's missing or wrong"),
        (status = 429, description = "Too many wrong PINs for that player: try again later")))]
async fn create(State(state): State<AppState>, Json(body): Json<CreateBody>) -> Result<(StatusCode, Json<NewToken>), ApiError> {
    let scope = Scope::parse(&body.scope).ok_or_else(|| ApiError::bad(format!("a token's scope is read or full, not {:?}", body.scope)))?;
    let player = body.player_id.map(crate::db::PlayerId);
    super::players::binding_allowed(&state, player, body.pin.as_deref())?;
    let made = state.db.token_create_for(&body.name, scope, "token", player).map_err(|e| ApiError::bad(e.to_string()))?;
    Ok((StatusCode::CREATED, Json(made)))
}

/// Revoke a token: it stops working at once, this request's own included.
#[utoipa::path(delete, path = "/tokens/{id}", tag = "devices", operation_id = "tokens_revoke",
    description = "Revoke a token or a paired device: it stops working at once, this request's own included. \
        A token bound to a player can revoke only itself.",
    params(("id" = i64, Path, description = "Token id")),
    responses((status = 204, description = "Revoked"), (status = 403, description = "A bound token revoking another"),
        (status = 404, description = "No such token")))]
async fn revoke(State(state): State<AppState>, caller: Option<Extension<Caller>>, Path(id): Path<i64>) -> Result<StatusCode, ApiError> {
    // A token bound to a player can revoke itself, a lost phone signing
    // itself out, and no other.
    if let Some(Caller::Token(t)) = caller.as_deref()
        && t.player_id.is_some()
        && t.id != id
    {
        return Err(ApiError::forbidden("this token is bound to one player, so it can revoke only itself"));
    }
    if !state.db.token_revoke(id)? {
        return Err(ApiError::not_found(format!("no token {id}")));
    }
    state.remote.limits.forget(id);
    Ok(StatusCode::NO_CONTENT)
}
