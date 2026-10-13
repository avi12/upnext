//! Each player's store connections and accounts, and AI accounts, with who
//! they are offered to (`docs/players.md`, "Offering and choosing accounts"). Only
//! an account's owner changes its sharing, renames or removes it; each
//! player turns on the ones offered to them.

use super::access::Caller;
use super::players::{Me, Who};
use super::{ApiError, ApiResult, AppState};
use crate::db::{AiAccount, PlayerId, Sharing, StoreAccount, StoreConnection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Extension, Json, Router};
use serde::Deserialize;
use serde_json::{Map, Value, json};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/accounts", get(accounts_get))
        .route("/accounts/{id}", put(account_put))
        .route("/accounts/{id}/use", put(account_use))
        .route("/connections", post(connection_add))
        .route("/connections/{id}", put(connection_put).delete(connection_delete))
        .route("/ai-accounts", get(ai_get).post(ai_add))
        .route("/ai-accounts/{id}", put(ai_put).delete(ai_delete))
        .route("/ai-accounts/{id}/use", put(ai_use))
}

fn bad(e: anyhow::Error) -> ApiError {
    ApiError::bad(e.to_string())
}

/// Refuse a SteamID another player's Steam connection already names, the
/// first player's own reading this computer's when it names none: an
/// account is whoever linked it, so a second player can't take
/// someone else's library and hours by typing their SteamID.
pub(crate) fn steam_id_free(state: &AppState, me: PlayerId, steam_id: &str) -> Result<(), ApiError> {
    let Ok(wanted) = crate::config::validate_setting("steam_id", steam_id) else {
        // A bad value is refused where it's saved, with that check's reason.
        return Ok(());
    };
    let first = state.cfg_for(PlayerId::FIRST);
    for conn in state.db.sync_connections("steam")? {
        if conn.owner == me {
            continue;
        }
        // The first player's first connection with no SteamID reads the
        // environment's, else the Steam client's most recent account.
        let theirs = conn.setting("steam_id").map(str::to_string).or_else(|| {
            conn.first.then(|| first.steam_id.clone()).flatten().or_else(|| {
                conn.first
                    .then(|| crate::steam::detect_steam_id(crate::steam_local::find_steam_dir(first.steam_dir.as_deref()).as_deref()))
                    .flatten()
            })
        });
        if theirs.as_deref() == Some(wanted.as_str()) {
            // Removing a player removes their connections, so this is a
            // guard: a connection with no player still holds the account.
            let name = state.db.players()?.into_iter().find(|p| p.id == conn.owner).map_or_else(|| "another player".into(), |p| p.name);
            return Err(ApiError::bad(format!(
                "that Steam account is {name}'s already: an account belongs to whoever linked it, and they can offer it to you"
            )));
        }
    }
    Ok(())
}

/// [`steam_id_free`] for a connection's settings.
fn steam_id_free_in(state: &AppState, me: PlayerId, settings: &Map<String, Value>) -> Result<(), ApiError> {
    match settings.get("steam_id").and_then(Value::as_str) {
        Some(id) if !id.trim().is_empty() => steam_id_free(state, me, id),
        _ => Ok(()),
    }
}

fn not_yours(what: &str) -> ApiError {
    ApiError::forbidden(format!("only the player who owns this {what} can change it"))
}

/// Settings that name a file or folder on the computer UpNext runs on, set
/// only from there, as `PUT /settings` keeps them.
const LOCAL_KEYS: [&str; 3] = ["galaxy_db", "heroic_dir", "epic_manifests"];

fn check_local(caller: &Option<Extension<Caller>>, settings: &Map<String, Value>) -> Result<(), ApiError> {
    let local = caller.as_ref().is_some_and(|c| c.is_local());
    match settings.keys().find(|k| LOCAL_KEYS.contains(&k.as_str())) {
        Some(k) if !local => Err(ApiError::forbidden(format!("{k} names a file, so it's set only on the computer UpNext runs on"))),
        _ => Ok(()),
    }
}

#[utoipa::path(get, path = "/accounts", tag = "accounts",
    description = "This player's store connections (never their sign-ins) and every store account they own or are \
        offered, with whether they use each and whether its playtime counts for them. For accounts they own, \
        `players` says each other player's part.",
    responses((status = 200, body = Object, description = "`{connections: [StoreConnection], accounts: [StoreAccount]}`")))]
async fn accounts_get(State(state): State<AppState>, Me(me): Me) -> ApiResult<Value> {
    let connections: Vec<StoreConnection> = state.db.connections_of(me)?;
    let accounts: Vec<StoreAccount> = state.db.store_accounts_for(me)?;
    Ok(Json(json!({ "connections": connections, "accounts": accounts })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct OfferBody {
    player_id: i64,
    /// With chosen sharing: offer it to them.
    offered: Option<bool>,
    /// Count its playtime for them too ("also plays on this account").
    plays_on: Option<bool>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct AccountBody {
    name: Option<String>,
    /// `everyone`, `chosen` or `owner` (only me).
    sharing: Option<String>,
    /// Each other player's part, for the ones to change.
    #[serde(default)]
    players: Vec<OfferBody>,
}

#[utoipa::path(put, path = "/accounts/{id}", tag = "accounts",
    description = "The owner's changes to a store account: its name, who it is offered to (`everyone`, `chosen`, `owner`), \
        and for each other player whether it is offered to them and whether its playtime counts for them.",
    params(("id" = i64, Path, description = "Store account id")),
    request_body = AccountBody,
    responses((status = 200, body = StoreAccount), (status = 400, description = "A bad name, sharing or player"),
        (status = 403, description = "Not the owner"), (status = 404, description = "No such account")))]
async fn account_put(
    State(state): State<AppState>,
    Me(me): Me,
    Path(id): Path<i64>,
    Json(body): Json<AccountBody>,
) -> ApiResult<StoreAccount> {
    let owner = state.db.account_owner(id)?.ok_or_else(|| ApiError::not_found(format!("no account {id}")))?;
    if owner != me {
        return Err(not_yours("account"));
    }
    let sharing = match body.sharing.as_deref() {
        Some(s) => Some(Sharing::parse(s).ok_or_else(|| ApiError::bad(format!("sharing is everyone, chosen or owner, not {s:?}")))?),
        None => None,
    };
    let players = state.db.players()?;
    for o in &body.players {
        if o.player_id == me.0 || !players.iter().any(|p| p.id.0 == o.player_id) {
            return Err(ApiError::bad(format!("no other player {}", o.player_id)));
        }
    }
    if let Some(n) = &body.name {
        state.db.rename_account(id, n).map_err(bad)?;
    }
    if let Some(s) = sharing {
        state.db.set_account_sharing(id, s)?;
    }
    for o in &body.players {
        state.db.set_account_offer(id, PlayerId(o.player_id), o.offered, o.plays_on)?;
    }
    found_account(&state, me, id)
}

fn found_account(state: &AppState, me: PlayerId, id: i64) -> ApiResult<StoreAccount> {
    let a = state.db.store_accounts_for(me)?.into_iter().find(|a| a.id == id);
    a.map(Json).ok_or_else(|| ApiError::not_found(format!("no account {id}")))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct UseBody {
    uses: bool,
}

#[utoipa::path(put, path = "/accounts/{id}/use", tag = "accounts",
    description = "This player's own switch for an account offered to them (\"Use these games\"). Turning it off takes its \
        games out of their library and keeps their marks and history on them.",
    params(("id" = i64, Path, description = "Store account id")),
    request_body = UseBody,
    responses((status = 200, body = StoreAccount), (status = 400, description = "Their own account, which is always theirs"),
        (status = 403, description = "Not offered to this player"),
        (status = 404, description = "No such account")))]
async fn account_use(State(state): State<AppState>, Me(me): Me, Path(id): Path<i64>, Json(body): Json<UseBody>) -> ApiResult<StoreAccount> {
    let owner = state.db.account_owner(id)?.ok_or_else(|| ApiError::not_found(format!("no account {id}")))?;
    if owner == me {
        return Err(ApiError::bad("your own accounts are always yours: change their sharing instead"));
    }
    state.db.set_account_use(id, me, body.uses).map_err(|e| ApiError::forbidden(e.to_string()))?;
    found_account(&state, me, id)
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct ConnectionBody {
    /// `steam`, `gog`, `epic`, `heroic` or `galaxy`.
    kind: String,
    /// Its settings: `steam_id` and `steam_include_hidden` for Steam, `heroic_dir` for Heroic, `galaxy_db` and
    /// `galaxy_skip` for Galaxy, `epic_manifests` for Epic.
    #[serde(default)]
    #[schema(value_type = Object)]
    settings: Map<String, Value>,
}

#[utoipa::path(post, path = "/connections", tag = "accounts",
    description = "Link a library for this player: another Steam account (a SteamID whose game details are public, or \
        whose user is signed in to Steam on this computer), a GOG or Epic sign-in (then `/gog/login` or the Epic sign-in for \
        it), a Heroic folder or a Galaxy database. Folders and databases are set only on the computer UpNext runs on. \
        The next sync fills it.",
    request_body = ConnectionBody,
    responses((status = 201, body = StoreConnection), (status = 400, description = "An unknown kind or setting, a bad value, or another player's SteamID"),
        (status = 403, description = "A folder or database from another device")))]
async fn connection_add(
    State(state): State<AppState>,
    Me(me): Me,
    caller: Option<Extension<Caller>>,
    Json(body): Json<ConnectionBody>,
) -> Result<(StatusCode, Json<StoreConnection>), ApiError> {
    check_local(&caller, &body.settings)?;
    steam_id_free_in(&state, me, &body.settings)?;
    let id = state.db.add_connection(me, &body.kind, &body.settings).map_err(bad)?;
    let made =
        state.db.connections_of(me)?.into_iter().find(|c| c.id == id).ok_or_else(|| ApiError::not_found("the connection vanished"))?;
    Ok((StatusCode::CREATED, Json(made)))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct ConnectionSettingsBody {
    /// The settings to change; null or "" clears one.
    #[schema(value_type = Object)]
    settings: Map<String, Value>,
}

#[utoipa::path(put, path = "/connections/{id}", tag = "accounts",
    description = "Change a connection's settings (its owner only).",
    params(("id" = i64, Path, description = "Connection id")),
    request_body = ConnectionSettingsBody,
    responses((status = 200, body = StoreConnection), (status = 400, description = "A setting it doesn't have, a bad value, or another player's SteamID"),
        (status = 403, description = "Not the owner, or a folder from another device"), (status = 404, description = "No such connection")))]
async fn connection_put(
    State(state): State<AppState>,
    Me(me): Me,
    caller: Option<Extension<Caller>>,
    Path(id): Path<i64>,
    Json(body): Json<ConnectionSettingsBody>,
) -> ApiResult<StoreConnection> {
    let owner = state.db.connection_owner(id)?.ok_or_else(|| ApiError::not_found(format!("no connection {id}")))?;
    if owner != me {
        return Err(not_yours("connection"));
    }
    check_local(&caller, &body.settings)?;
    steam_id_free_in(&state, me, &body.settings)?;
    state.db.set_connection_settings(id, &body.settings).map_err(bad)?;
    let c = state.db.connections_of(me)?.into_iter().find(|c| c.id == id);
    c.map(Json).ok_or_else(|| ApiError::not_found(format!("no connection {id}")))
}

#[utoipa::path(delete, path = "/connections/{id}", tag = "accounts",
    description = "Remove a connection (its owner only), with its accounts and their games. Everyone's marks and history \
        on those games stay, so linking it again brings them back.",
    params(("id" = i64, Path, description = "Connection id")),
    responses((status = 204, description = "Removed"), (status = 403, description = "Not the owner"),
        (status = 404, description = "No such connection")))]
async fn connection_delete(State(state): State<AppState>, Me(me): Me, Path(id): Path<i64>) -> Result<StatusCode, ApiError> {
    let owner = state.db.connection_owner(id)?.ok_or_else(|| ApiError::not_found(format!("no connection {id}")))?;
    if owner != me {
        return Err(not_yours("connection"));
    }
    state.db.remove_connection(id)?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/ai-accounts", tag = "ai",
    description = "Every AI account this player owns or is offered, never its keys (`keys_set` names the ones it holds), \
        with the one they use (`in_use`), the library's (`library`) and this computer's (`this_computer`, the one the \
        environment's AI settings and the signed-in CLIs are).",
    responses((status = 200, body = Vec<AiAccount>)))]
async fn ai_get(State(state): State<AppState>, Me(me): Me) -> ApiResult<Vec<AiAccount>> {
    Ok(Json(state.db.ai_accounts_for(me)?))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct AiBody {
    /// An `llm_backend`: `claude-cli`, `codex-cli`, `api-key`, `openai-key` or `openai`; null for the built-in one.
    backend: Option<String>,
    /// AI settings and keys by name, as `PUT /settings` takes them (`model`, `anthropic_api_key`,
    /// `claude_oauth_token` and the rest); null or "" clears one.
    #[serde(default)]
    #[schema(value_type = Object)]
    settings: Map<String, Value>,
    /// `everyone`, `chosen` or `owner`.
    sharing: Option<String>,
    /// With chosen sharing: the players it is offered to.
    offered_to: Option<Vec<i64>>,
    /// true runs the library's own AI work (summaries, series guides, hidden gems) on it.
    library: Option<bool>,
}

/// AI settings from a body, each checked as `PUT /settings` checks it.
fn ai_settings(body: &Map<String, Value>) -> Result<Vec<(String, Option<String>)>, ApiError> {
    let mut out = vec![];
    for (key, value) in body {
        if !crate::db::AI_KEYS.contains(&key.as_str()) && !crate::db::AI_SECRET_KEYS.contains(&key.as_str()) {
            return Err(ApiError::bad(format!("{key} isn't a setting of an AI account")));
        }
        let text = match value {
            Value::Null => String::new(),
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        let v = if text.trim().is_empty() {
            None
        } else {
            Some(crate::config::validate_setting(key, &text).map_err(|e| ApiError::bad(e.to_string()))?)
        };
        out.push((key.clone(), v));
    }
    Ok(out)
}

fn backend(b: &Option<String>) -> Result<Option<&str>, ApiError> {
    match b.as_deref().map(str::trim).filter(|b| !b.is_empty()) {
        Some(b) if !crate::config::LLM_BACKENDS.contains(&b) => {
            Err(ApiError::bad(format!("a backend is one of {}", crate::config::LLM_BACKENDS.join(", "))))
        }
        b => Ok(b),
    }
}

fn sharing_and_players(state: &AppState, me: PlayerId, body: &AiBody) -> Result<(Option<Sharing>, Option<Vec<PlayerId>>), ApiError> {
    let sharing = match body.sharing.as_deref() {
        Some(s) => Some(Sharing::parse(s).ok_or_else(|| ApiError::bad(format!("sharing is everyone, chosen or owner, not {s:?}")))?),
        None => None,
    };
    let chosen = match &body.offered_to {
        Some(ids) => {
            let players = state.db.players()?;
            for id in ids {
                if *id == me.0 || !players.iter().any(|p| p.id.0 == *id) {
                    return Err(ApiError::bad(format!("no other player {id}")));
                }
            }
            Some(ids.iter().copied().map(PlayerId).collect())
        }
        None => None,
    };
    Ok((sharing, chosen))
}

#[utoipa::path(post, path = "/ai-accounts", tag = "ai",
    description = "Add an AI account for this player. A Claude Code CLI account other than this computer's needs its own \
        token (`claude_oauth_token`, from `claude setup-token`); a Codex CLI one signs in in its own folder \
        (`CODEX_HOME=<data folder>/codex/<id> codex login`). A player with no AI account starts using it.",
    request_body = AiBody,
    responses((status = 201, body = AiAccount),
        (status = 400, description = "An unknown backend or setting, a bad value, or a bad sharing or player"),
        (status = 403, description = "The library's AI from a bound token")))]
async fn ai_add(
    State(state): State<AppState>,
    Me(me): Me,
    who: Option<Extension<Who>>,
    Json(body): Json<AiBody>,
) -> Result<(StatusCode, Json<AiAccount>), ApiError> {
    let settings = ai_settings(&body.settings)?;
    let (sharing, chosen) = sharing_and_players(&state, me, &body)?;
    if body.library == Some(true) && who.as_ref().is_some_and(|w| w.bound) {
        return Err(ApiError::forbidden("this token is bound to one player, so it can't choose the library's AI"));
    }
    let id = state.db.add_ai_account(me, backend(&body.backend)?, &settings, sharing.unwrap_or(Sharing::Everyone))?;
    if let Some(c) = &chosen {
        state.db.set_ai_sharing(id, sharing.unwrap_or(Sharing::Chosen), Some(c))?;
    }
    if body.library == Some(true) {
        state.db.set_library_ai(id)?;
    }
    let made = state.db.ai_accounts_for(me)?.into_iter().find(|a| a.id == id);
    Ok((StatusCode::CREATED, Json(made.ok_or_else(|| ApiError::not_found("the account vanished"))?)))
}

#[utoipa::path(put, path = "/ai-accounts/{id}", tag = "ai",
    description = "The owner's changes to an AI account: its backend, settings and keys, who it is offered to, and whether \
        the library's own AI work runs on it. From another device, a new `openai_base_url` clears the account's \
        `openai_api_key` unless the same request sends one.",
    params(("id" = i64, Path, description = "AI account id")),
    request_body = AiBody,
    responses((status = 200, body = AiAccount), (status = 400, description = "An unknown backend or setting, a bad value, or a bad sharing or player"),
        (status = 403, description = "Not the owner, or the library's AI from a bound token"), (status = 404, description = "No such account")))]
async fn ai_put(
    State(state): State<AppState>,
    Me(me): Me,
    caller: Option<Extension<Caller>>,
    who: Option<Extension<Who>>,
    Path(id): Path<i64>,
    Json(body): Json<AiBody>,
) -> ApiResult<AiAccount> {
    let owner = state.db.ai_account_owner(id)?.ok_or_else(|| ApiError::not_found(format!("no AI account {id}")))?;
    if owner != me {
        return Err(not_yours("AI account"));
    }
    if body.library.is_some() && who.as_ref().is_some_and(|w| w.bound) {
        return Err(ApiError::forbidden("this token is bound to one player, so it can't choose the library's AI"));
    }
    let mut settings = ai_settings(&body.settings)?;
    // The OpenAI-compatible key goes to whatever address is set: from
    // another device, a new address takes the saved key away (`PUT /settings`).
    let local = caller.is_some_and(|c| c.is_local());
    if !local && settings.iter().any(|(k, _)| k == "openai_base_url") && !settings.iter().any(|(k, v)| k == "openai_api_key" && v.is_some())
    {
        settings.push(("openai_api_key".into(), None));
    }
    let (sharing, chosen) = sharing_and_players(&state, me, &body)?;
    let b = if body.backend.is_some() { Some(backend(&body.backend)?) } else { None };
    state.db.set_ai_account(id, b, &settings)?;
    if sharing.is_some() || chosen.is_some() {
        let now = state.db.ai_accounts_for(me)?.into_iter().find(|a| a.id == id).map(|a| a.sharing).unwrap_or(Sharing::Everyone);
        state.db.set_ai_sharing(id, sharing.unwrap_or(now), chosen.as_deref())?;
    }
    if body.library == Some(true) {
        state.db.set_library_ai(id)?;
    }
    let a = state.db.ai_accounts_for(me)?.into_iter().find(|a| a.id == id);
    a.map(Json).ok_or_else(|| ApiError::not_found(format!("no AI account {id}")))
}

#[utoipa::path(delete, path = "/ai-accounts/{id}", tag = "ai",
    description = "Remove an AI account (its owner only). Whoever used it gets picks without AI until they choose another; \
        the library's own AI work stops until one is chosen for it.",
    params(("id" = i64, Path, description = "AI account id")),
    responses((status = 204, description = "Removed"), (status = 403, description = "Not the owner"),
        (status = 404, description = "No such account")))]
async fn ai_delete(State(state): State<AppState>, Me(me): Me, Path(id): Path<i64>) -> Result<StatusCode, ApiError> {
    let owner = state.db.ai_account_owner(id)?.ok_or_else(|| ApiError::not_found(format!("no AI account {id}")))?;
    if owner != me {
        return Err(not_yours("AI account"));
    }
    state.db.remove_ai_account(id)?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(put, path = "/ai-accounts/{id}/use", tag = "ai",
    description = "Use this AI account for this player's picks, Help me decide and Second Chance: one they own or are offered.",
    params(("id" = i64, Path, description = "AI account id")),
    responses((status = 200, body = AiAccount), (status = 403, description = "Not theirs or offered to them"),
        (status = 404, description = "No such account")))]
async fn ai_use(State(state): State<AppState>, Me(me): Me, Path(id): Path<i64>) -> ApiResult<AiAccount> {
    state.db.ai_account_owner(id)?.ok_or_else(|| ApiError::not_found(format!("no AI account {id}")))?;
    state.db.use_ai_account(me, id).map_err(|e| ApiError::forbidden(e.to_string()))?;
    let a = state.db.ai_accounts_for(me)?.into_iter().find(|a| a.id == id);
    a.map(Json).ok_or_else(|| ApiError::not_found(format!("no AI account {id}")))
}
