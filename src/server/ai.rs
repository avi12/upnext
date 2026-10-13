//! The AI in Settings and on the AI usage page: the models each backend
//! offers, Settings' Test, what the AI used and would take, and the list
//! prices it is priced at.

use super::players::Me;
use super::{ApiResult, AppState};
use crate::db::PlayerId;
use axum::extract::{Query, State};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route("/llm/test", post(llm_test))
        .route("/llm/models", get(llm_models))
        .route("/llm/codex", get(llm_codex))
        .route("/llm/openai", get(llm_openai))
        .route("/ai/usage", get(ai_usage_get))
        .route("/ai/estimates", get(ai_estimates_get))
        .route("/ai/plan", get(ai_plan_get))
        .route("/ai/prices", get(ai_prices_get))
        .route("/ai/prices/refresh", post(ai_prices_refresh))
}

/// Models offered by the configured OpenAI-compatible server (Settings suggestions).
#[utoipa::path(get, path = "/llm/models", tag = "ai",
    description = "Models offered by the configured OpenAI-compatible server, for Settings' suggestions.",
    responses((status = 200, body = Object, description = "`{ok: true, models: [string]}`, or `{ok: false, models: [], error}`")))]
async fn llm_models(State(state): State<AppState>, Me(me): Me) -> Json<Value> {
    let cfg = state.cfg_for(me);
    let backend = crate::llm::openai_compat::OpenAiCompatBackend::new(cfg.openai_base_url.clone(), None, cfg.openai_api_key.clone());
    match backend.list_models().await {
        Ok(models) => Json(json!({ "ok": true, "models": models })),
        Err(e) => Json(json!({ "ok": false, "models": [], "error": e.to_string() })),
    }
}

/// The Codex CLI for Settings: installed, signed in, and the models the
/// account can use (Codex's catalog, or a built-in list when it can't be read).
#[utoipa::path(get, path = "/llm/codex", tag = "ai",
    description = "The Codex CLI for Settings: installed, signed in, and the models the account can use.",
    responses((status = 200, body = Object, description = "`{installed, version, login, error, families: [{id, model}], \
        models, models_from: \"codex\" | \"built_in\", models_error}`")))]
async fn llm_codex(State(state): State<AppState>, Me(me): Me) -> Json<Value> {
    use crate::llm::codex_cli;
    let bin = state.cfg_for(me).codex_program();
    let (version, login, models) = tokio::join!(codex_cli::version(&bin), codex_cli::login(&bin), codex_cli::models(&bin));
    let (models, models_from, models_error) = match models {
        Ok(m) => (m, "codex", None),
        Err(e) => (codex_cli::built_in_models(), "built_in", Some(e)),
    };
    let (login, error) = match login {
        Ok(l) => (Some(l), None),
        Err(e) => (None, Some(e)),
    };
    // "luna" and the other families, with the model each means today.
    let families: Vec<Value> = crate::llm::families::families(&models).into_iter().map(|(f, m)| json!({"id": f, "model": m.id})).collect();
    Json(json!({
        "installed": version.is_some(),
        "version": version,
        "login": login,
        "error": error,
        "families": families,
        "models": models,
        "models_from": models_from,
        "models_error": models_error,
    }))
}

/// The OpenAI API key's models for Settings: the GPT models the key can
/// use, or a built-in list when there's no key or OpenAI can't be reached.
#[utoipa::path(get, path = "/llm/openai", tag = "ai",
    description = "The OpenAI API key's models for Settings, or a built-in list when there's no key or OpenAI can't be reached.",
    responses((status = 200, body = Object, description = "`{key_set, families: [{id, model}], models, \
        models_from: \"openai\" | \"built_in\", models_error}`")))]
async fn llm_openai(State(state): State<AppState>, Me(me): Me) -> Json<Value> {
    use crate::llm::openai_key;
    let cfg = state.cfg_for(me);
    let backend = openai_key::OpenAiKeyBackend::new(cfg.openai_key_url.clone(), cfg.openai_key.clone(), None);
    let (models, models_from, models_error) = match backend.list_models().await {
        Ok(m) => (m, "openai", None),
        Err(e) => (openai_key::built_in_models(), "built_in", Some(e.reason())),
    };
    // "luna" and the other families, with the model each means today.
    let families: Vec<Value> = crate::llm::families::families(&models).into_iter().map(|(f, m)| json!({"id": f, "model": m})).collect();
    Json(json!({
        "key_set": cfg.openai_key.is_some(),
        "families": families,
        "models": models,
        "models_from": models_from,
        "models_error": models_error,
    }))
}

/// Which AI Settings' Test tries.
#[derive(Debug, Default, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LlmTestQuery {
    /// "summaries" tries the summaries' backend and model; anything
    /// else, or none, the picks'.
    purpose: Option<String>,
}

/// Tiny round trip through the configured AI backend (Settings "Test").
#[utoipa::path(post, path = "/llm/test", tag = "ai",
    description = "A tiny round trip through the configured AI backend (Settings' Test): the picks' one, or with \
        `purpose=summaries` the summaries' one and its model. It's recorded as AI usage.",
    params(LlmTestQuery),
    responses((status = 200, body = Object, description = "`{ok: true, backend, model, duration_ms, usage, cost_usd, api_usd}`, \
        or `{ok: false, backend, error}`")))]
async fn llm_test(State(state): State<AppState>, Me(me): Me, Query(q): Query<LlmTestQuery>) -> Json<Value> {
    let cfg = state.cfg_for(me);
    let summaries = q.purpose.as_deref() == Some("summaries");
    let (llm, model) = if summaries { (state.summary_llm(), cfg.summary_model()) } else { (state.llm_for(me), cfg.pick_model()) };
    let req = crate::llm::LlmRequest {
        system: "You check that an API connection works. Reply with JSON only.".into(),
        cached_prefix: None,
        prompt: "Reply with {\"ok\": true}.".into(),
        schema: json!({"type": "object", "properties": {"ok": {"type": "boolean"}}, "required": ["ok"], "additionalProperties": false}),
        model,
        background: false,
        effort: None,
    };
    match crate::ai_usage::call(&state.db, llm.as_ref(), &crate::ai_usage::Purpose::new(crate::db::AiKind::Test), req).await {
        Ok(r) => Json(json!({
            "ok": true, "backend": llm.name(), "model": r.model, "duration_ms": r.duration_ms, "usage": r.usage,
            "cost_usd": r.cost_usd, "api_usd": r.api_usd,
        })),
        Err(e) => Json(json!({ "ok": false, "backend": llm.name(), "error": e.reason() })),
    }
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct UsageQuery {
    /// The period, in days: 30 unless asked (1 to 365).
    days: Option<i64>,
}

/// What the AI used: today, the last 7 days, and the last `days` (30 by
/// default) by day, kind and model.
#[utoipa::path(get, path = "/ai/usage", tag = "ai",
    description = "What the AI used: today, the last 7 days, and the last `days` by day, kind and model.",
    params(UsageQuery),
    responses((status = 200, body = crate::ai_usage::UsageSummary)))]
async fn ai_usage_get(State(state): State<AppState>, Me(me): Me, Query(q): Query<UsageQuery>) -> ApiResult<crate::ai_usage::UsageSummary> {
    Ok(Json(crate::ai_usage::usage_summary(&state.db, &state.cfg_for(me), q.days.unwrap_or(30))?))
}

/// What each AI feature would take on this library.
#[utoipa::path(get, path = "/ai/estimates", tag = "ai",
    description = "What each AI feature would take on this library: once, and each month.",
    responses((status = 200, body = crate::ai_usage::Estimates)))]
async fn ai_estimates_get(State(state): State<AppState>, Me(me): Me) -> ApiResult<crate::ai_usage::Estimates> {
    Ok(Json(crate::ai_usage::estimates(&state.db, &state.cfg_for(me))?))
}

/// On a subscription: the plan's usage bars, read now (unless a reading is
/// under 30 s old), and what UpNext's AI takes of them.
#[utoipa::path(get, path = "/ai/plan", tag = "ai",
    description = "On a subscription: the plan's usage bars, read now (unless a reading is under 30 s old), and what \
        UpNext's AI takes of them.",
    responses((status = 200, body = crate::ai_plan::PlanSummary)))]
async fn ai_plan_get(State(state): State<AppState>, Me(me): Me) -> ApiResult<crate::ai_plan::PlanSummary> {
    Ok(Json(crate::ai_plan::summary(&state.db, &state.cfg_for(me)).await?))
}

/// The list prices the AI in use is priced at, with when they were read.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PricesView {
    backend: String,
    /// Whether its calls are charged (an API key, a hosted provider) or
    /// shown at API prices (a subscription).
    charged: bool,
    /// None for a local model, or a provider whose prices UpNext can't read.
    prices: Option<crate::prices::PriceInfo>,
}

fn prices_view(state: &AppState, me: PlayerId) -> PricesView {
    let llm = state.llm_for(me);
    let pricing = llm.pricing();
    PricesView {
        backend: llm.name().to_string(),
        charged: pricing.is_some_and(|p| p.charged),
        prices: pricing.map(|p| crate::prices::info(&state.db, p.provider)),
    }
}

/// Reads them first when they're a week old.
#[utoipa::path(get, path = "/ai/prices", tag = "ai",
    description = "The list prices the AI in use is priced at, with when they were read. Reads them first when they're a week old.",
    responses((status = 200, body = PricesView)))]
async fn ai_prices_get(State(state): State<AppState>, Me(me): Me) -> ApiResult<PricesView> {
    for p in state.priced_providers(me) {
        crate::prices::refresh_if_stale(&state.db, &state.clients.http, p, state.clients.base_url.as_deref()).await;
    }
    Ok(Json(prices_view(&state, me)))
}

/// Read them again now. A failure is in `prices.error`, and the prices in
/// use stay as they were.
#[utoipa::path(post, path = "/ai/prices/refresh", tag = "ai",
    description = "Read the prices again now. A failure is in `prices.error`, and the prices in use stay as they were.",
    responses((status = 200, body = PricesView)))]
async fn ai_prices_refresh(State(state): State<AppState>, Me(me): Me) -> ApiResult<PricesView> {
    for p in state.priced_providers(me) {
        if let Err(e) = crate::prices::refresh(&state.db, &state.clients.http, p, state.clients.base_url.as_deref()).await {
            tracing::warn!("could not read {}'s prices: {e:#}", p.label());
        }
    }
    Ok(Json(prices_view(&state, me)))
}
