//! Settings (docs/api/settings.md): every in-app setting with where it
//! comes from, changing them, what the server runs with, and the libraries
//! and sources a sync and enrich can read.

use super::listen::is_loopback_host;
use super::players::{self, Me};
use super::{ApiError, ApiResult, AppState, access, accounts, network, oidc};
use crate::config::Config;
use crate::db::PlayerId;
use axum::extract::State;
use axum::{Json, Router};
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    use axum::routing::get;
    Router::new()
        .route("/config", get(config))
        .route("/settings", get(settings_get).put(settings_put))
        .route("/enrich/sources", get(enrich_sources))
        .route("/sync/sources", get(sync_sources))
}

#[utoipa::path(get, path = "/config", tag = "settings",
    description = "What the server runs with: `model`, `sampled_minutes`, `max_candidates`, `db_path`, `claude_available` \
        (the AI in use can run), `llm_error` (why not, or null), `llm_backend`, `steam_configured`, `steam_api_configured`, \
        `steam_local_dir`, `steam_id_source`, `gog_connected`, `epic_connected`, `heroic_config_dir`, `galaxy_db`, `linux_active` and \
        `rawg_configured`.",
    responses((status = 200, body = Object, description = "The fields listed above")))]
async fn config(State(state): State<AppState>, Me(me): Me) -> ApiResult<Value> {
    let cfg = state.cfg_for(me);
    let llm_error = state.llm_for(me).available().await.err().map(|e| e.reason());
    Ok(Json(json!({
        "model": cfg.model,
        "sampled_minutes": cfg.sampled_minutes,
        "max_candidates": cfg.max_candidates,
        "db_path": cfg.db_path,
        "claude_available": llm_error.is_none(),
        "llm_error": llm_error,
        "llm_backend": cfg.llm_backend,
        "steam_configured": cfg.steam_credentials().is_ok() || crate::steam_local::find_steam_dir(cfg.steam_dir.as_deref()).is_some(),
        "steam_api_configured": cfg.steam_credentials().is_ok(),
        "steam_local_dir": crate::steam_local::find_steam_dir(cfg.steam_dir.as_deref()),
        "steam_id_source": cfg.source("steam_id"),
        "gog_connected": state.gog_for(me).load_token().is_some(),
        "epic_connected": state.epic_for(me).load_token().is_some(),
        "heroic_config_dir": crate::heroic::find_config_dir(cfg.heroic_dir.as_deref()),
        "galaxy_db": cfg.galaxy_db,
        "linux_active": cfg.linux(),
        "rawg_configured": cfg.rawg_api_key.is_some(),
    })))
}

/// The Claude model setting `key` as Settings shows it: "default" when
/// unset, and with an Anthropic API key also when never chosen, since the
/// key then uses its own default for the feature, not the CLI's "sonnet".
/// `backend` is the one the setting is for: the picks' for `model`, the
/// summaries' for `digest_model`.
fn shown_model(cfg: &Config, backend: &str, key: &str, value: Option<String>) -> String {
    let value = if backend == "api-key" && cfg.source(key) == "default" { None } else { value };
    value.unwrap_or_else(|| "default".into())
}

/// The Claude models Settings offers for a backend: Haiku only with an API
/// key, where it is the summaries' default.
fn claude_models(backend: &str) -> Value {
    if backend == "api-key" { json!(["opus", "sonnet", "haiku", "default"]) } else { json!(["opus", "sonnet", "default"]) }
}

fn settings_json(state: &AppState, me: PlayerId) -> Result<Value, ApiError> {
    let cfg = state.cfg_for(me);
    let profile = state.db.profile(me, cfg.steam_id.as_deref())?;
    let galaxy_exists = cfg.galaxy_db.as_ref().is_some_and(|p| p.is_file());
    let mut v = json!({
        "steam_api_key_set": cfg.steam_api_key.is_some(),
        "steam_api_key_source": if cfg.steam_api_key.is_some() { Some(cfg.source("steam_api_key")) } else { None },
        "steam_id": cfg.steam_id,
        "steam_id_source": if cfg.steam_id.is_some() { Some(cfg.source("steam_id")) } else { None },
        "steam_persona": profile.persona_name,
        "steam_avatar": profile.avatar,
        "model": shown_model(&cfg, &cfg.llm_backend, "model", cfg.model.clone()),
        "model_source": cfg.source("model"),
        "digest_model": shown_model(&cfg, cfg.summary_backend(), "digest_model", cfg.digest_model.clone()),
        "openai_digest_model": cfg.openai_digest_model,
        "igdb_client_id": cfg.igdb_client_id,
        "igdb_secret_set": cfg.igdb_client_secret.is_some(),
        "models": claude_models(&cfg.llm_backend),
        "llm_backend": cfg.llm_backend,
        "llm_backends": crate::config::LLM_BACKENDS,
        "rawg_api_key_set": cfg.rawg_api_key.is_some(),
        "openai_base_url": cfg.openai_base_url,
        "openai_model": cfg.openai_model,
        "openai_api_key_set": cfg.openai_api_key.is_some(),
        "theme": cfg.theme,
        "themes": crate::config::THEMES,
        "theme_follow_system": cfg.theme_follow_system,
        "auto_sync": cfg.auto_sync,
        "last_sync": state.db.meta_get("last_sync").ok().flatten().and_then(|v| v.parse::<i64>().ok()),
        "linux_mode": cfg.linux_mode,
        "linux_modes": crate::config::LINUX_MODES,
        "linux_detected": crate::config::running_on_linux(),
        "linux_active": cfg.linux(),
        "anthropic_api_key_set": cfg.anthropic_api_key.is_some(),
        "anthropic_api_key_source": if cfg.anthropic_api_key.is_some() { Some(cfg.source("anthropic_api_key")) } else { None },
        "sampled_minutes": cfg.sampled_minutes,
        "max_candidates": cfg.max_candidates,
        "galaxy_db": cfg.galaxy_db,
        "galaxy_db_exists": galaxy_exists,
        "heroic_dir": crate::heroic::find_config_dir(cfg.heroic_dir.as_deref()),
        "steam_local_dir": crate::steam_local::find_steam_dir(cfg.steam_dir.as_deref()),
        "gog": state.gog_for(me).status(),
        "epic": state.epic_for(me).status(),
        "epic_launcher_dir": crate::epic::find_manifests_dir(cfg.epic_manifests.as_deref()),
        "env_file": std::path::Path::new(".env").is_file(),
    });
    // Apart, since one json! this long passes the macro's recursion limit.
    v["codex_model"] = json!(cfg.codex_model);
    // "linux", "windows" or "macos": which folders and launchers the hints name.
    v["os"] = json!(std::env::consts::OS);
    v["codex_digest_model"] = json!(cfg.codex_digest_model);
    // Summaries on a backend of their own: "same", or one of `llm_backends`.
    v["digest_backend"] = json!(cfg.digest_backend);
    v["digest_backends"] = json!(crate::config::DIGEST_BACKENDS);
    v["digest_models"] = claude_models(cfg.summary_backend());
    v["codex_effort"] = json!(cfg.codex_effort);
    v["codex_efforts"] = json!(crate::config::CODEX_EFFORTS);
    v["shelve_after_days"] = json!(cfg.shelve_after_days);
    v["abandon_after_days"] = json!(cfg.abandon_after_days);
    v["staple_hours"] = json!(cfg.staple_hours);
    v["burnout_days"] = json!(cfg.burnout_days);
    v["pattern_half_life_days"] = json!(cfg.pattern_half_life_days);
    v["pattern_min_games"] = json!(cfg.pattern_min_games);
    v["play_diary"] = json!(cfg.play_diary);
    v["wishlist"] = json!(cfg.wishlist);
    v["steam_include_hidden"] = json!(cfg.steam_include_hidden);
    v["openai_thinking"] = json!(cfg.openai_thinking);
    v["galaxy_skip"] = json!(crate::galaxy::skip_list(cfg.galaxy_skip.as_deref()));
    v["galaxy_integrations"] = json!(crate::enrich::sync::galaxy::seen_integrations(&state.db));
    v["openai_key_set"] = json!(cfg.openai_key.is_some());
    v["openai_key_source"] = json!(cfg.openai_key.as_ref().map(|_| cfg.source("openai_key")));
    v["openai_key_model"] = json!(cfg.openai_key_model);
    v["openai_key_digest_model"] = json!(cfg.openai_key_digest_model);
    v["openai_key_effort"] = json!(cfg.openai_key_effort);
    v["openai_key_efforts"] = json!(crate::config::OPENAI_EFFORTS);
    // Devices and API.
    v["public_url"] = json!(cfg.public_url);
    v["public_url_source"] = json!(cfg.public_url.as_ref().map(|_| cfg.source("public_url")));
    let listening = state.remote.listen.now(&cfg);
    v["listen"] = json!({
        "host": listening.host,
        "port": listening.port,
        "other_devices": listening.beyond_loopback(),
        "https": listening.https,
        "error": listening.error,
        // A host beyond loopback from the environment (a container's
        // UPNEXT_HOST) listens there whatever the switch says.
        "forced": !is_loopback_host(&state.base_cfg.host),
        // UpNext's container image: codes and tokens come from
        // `docker exec upnext upnext pair` and `token create`.
        "container": state.base_cfg.container,
    });
    v["allow_other_devices"] = json!(cfg.allow_other_devices);
    // Signing in with an identity provider and trusted proxies:
    // set only on this computer or in the environment, each
    // with where it came from. The client secret only as set or not.
    v["oidc_issuer"] = json!(cfg.oidc_issuer);
    v["oidc_client_id"] = json!(cfg.oidc_client_id);
    v["oidc_client_secret_set"] = json!(cfg.oidc_client_secret.is_some());
    v["oidc_name"] = json!(cfg.oidc_name);
    v["oidc_auto_provision"] = json!(cfg.oidc_auto_provision);
    v["oidc_allowed_groups"] = json!(cfg.oidc_allowed_groups);
    v["oidc_redirect_uri"] = json!(access::public_url(state).map(|p| format!("{}{}", p.origin(), oidc::CALLBACK)));
    v["trusted_proxies"] = json!(cfg.trusted_proxies);
    v["trusted_proxy_login"] = json!(cfg.trusted_proxy_login);
    for key in crate::config::SIGN_IN_KEYS {
        v[format!("{key}_source")] = json!(cfg.source(key));
    }
    // Found on the network: whether UpNext announces itself, and
    // the name it uses (`network_name` unset is the host name, shown as
    // `network_name_shown`).
    v["announce"] = json!(cfg.announce);
    v["network_name"] = json!(cfg.network_name);
    v["network_name_shown"] = json!(network::network_name(&cfg));
    v["tls_cert"] = json!(cfg.tls_cert);
    v["tls_key"] = json!(cfg.tls_key);
    // The app window's own: it reads them when it opens and saves them when it closes.
    v["window_size"] = json!(cfg.window_size);
    v["window_maximized"] = json!(cfg.window_maximized);
    v["window_zoom"] = json!(cfg.window_zoom);
    // Windows only: what closing the window does, and the Run key.
    v["keep_running"] = json!(cfg.keep_running);
    v["start_with_windows"] = json!(cfg.start_with_windows);
    // Backlog verdicts: the service's address (none until one is
    // set) and whether to offer a verdict after a mark.
    v["verdicts_url"] = json!(cfg.verdicts_url);
    v["verdicts_url_source"] = json!(cfg.verdicts_url.as_ref().map(|_| cfg.source("verdicts_url")));
    v["verdicts_ask"] = json!(cfg.verdicts_ask);
    // Which of these are the player's own: their preferences,
    // their connections' and their AI account's. The rest are everyone's.
    let own: Vec<&str> =
        crate::config::SETTINGS.iter().filter(|s| s.in_app && !crate::db::household_setting(s.key)).map(|s| s.key).collect();
    v["player_keys"] = json!(own);
    // Other players' verdicts: whether they're
    // downloaded and shown ("app" once someone chose, else "default"), and
    // whether picks' prompt gets their lines.
    v["verdicts_receive"] = json!(cfg.verdicts_receive);
    v["verdicts_receive_source"] = json!(cfg.source("verdicts_receive"));
    v["verdicts_prompt"] = json!(cfg.verdicts_prompt);
    Ok(v)
}

#[utoipa::path(get, path = "/settings", tag = "settings",
    description = "Every in-app setting with where it comes from (`*_source`: app, env). Keys and secrets are never \
        returned, only `*_set` booleans. Also the allowed values (`models`, `digest_models`, `llm_backends`, \
        `digest_backends`, `themes`, `linux_modes`, `codex_efforts`, `openai_key_efforts`), `gog` (the GogStatus), `listen` (`host`, `port`, `other_devices`, \
        `https`, `error`, `forced`, `this_computer`, `container`), `public_url`, `last_sync` and the paths UpNext found (`heroic_dir`, `steam_local_dir`).",
    responses((status = 200, body = Object, description = "The settings, as described above")))]
async fn settings_get(State(state): State<AppState>, Me(me): Me, caller: Option<axum::Extension<access::Caller>>) -> ApiResult<Value> {
    Ok(Json(with_caller(settings_json(&state, me)?, caller.is_some_and(|c| c.is_local()))))
}

/// Whether the asker is this computer, for what Settings offers: making
/// tokens and pairing codes, and the file settings, are this computer's.
fn with_caller(mut v: Value, local: bool) -> Value {
    v["listen"]["this_computer"] = json!(local);
    v
}

#[utoipa::path(put, path = "/settings", tag = "settings",
    description = "Change settings: an object of setting names and values; null or \"\" clears one (it falls back to \
        `.env`). Every value is checked first, so a bad one saves nothing. `galaxy_db`, `tls_cert` and `tls_key` name \
        files, `start_with_windows` and `keep_running` change how UpNext runs there, and `verdicts_url` is where each player's verdicts key goes, so only this computer can set them. From another device, a new `openai_base_url` clears the saved \
        `openai_api_key` unless the same request sends one (403 when that key is set in the environment). On Windows, \
        `start_with_windows` writes or removes UpNext's value in the Run key before it is saved.",
    request_body(content = Object, description = "Setting names and their new values"),
    responses((status = 200, body = Object, description = "The settings after the change, as GET /settings"),
        (status = 400, description = "An unknown setting or a value it doesn't take, a SteamID another player's account has, or a Run key Windows didn't let UpNext change"),
        (status = 403, description = "A setting that names a file or changes how UpNext runs on its computer, or a new \
            OpenAI-compatible address while its key is in the environment, from another device")))]
async fn settings_put(
    State(state): State<AppState>,
    Me(me): Me,
    caller: Option<axum::Extension<access::Caller>>,
    who: Option<axum::Extension<players::Who>>,
    Json(body): Json<serde_json::Map<String, Value>>,
) -> ApiResult<Value> {
    // No caller means the guard didn't run: refuse what's local-only.
    let local = caller.is_some_and(|c| c.is_local());
    let bound = who.is_some_and(|w| w.bound);
    // Validate everything first so a bad value saves nothing.
    let mut writes: Vec<(&str, Option<String>)> = Vec::with_capacity(body.len());
    for (key, value) in &body {
        if crate::config::in_app_setting(key).is_err() {
            return Err(ApiError::bad(format!("unknown setting {key:?}")));
        }
        // A device or script bound to one player changes only that player's
        // own settings.
        if bound && crate::db::household_setting(key) {
            return Err(ApiError::forbidden(format!(
                "{key} is everyone's on this computer, and this token is bound to one player, so it can't change it"
            )));
        }
        // They name files on the computer UpNext runs on.
        if matches!(key.as_str(), "galaxy_db" | "tls_cert" | "tls_key") && !local {
            return Err(ApiError::forbidden(format!("{key} names a file, so it's set only on the computer UpNext runs on")));
        }
        // They change how UpNext runs on that computer: its Run key, and
        // whether it stays running there.
        if matches!(key.as_str(), "start_with_windows" | "keep_running") && !local {
            return Err(ApiError::forbidden(format!("{key} is set only on the computer UpNext runs on")));
        }
        // They decide who gets in: the identity
        // provider, and which proxies are believed.
        if crate::config::SIGN_IN_KEYS.contains(&key.as_str()) && !local {
            return Err(ApiError::forbidden(format!(
                "{key} decides who gets in, so it's set only on the computer UpNext runs on, or in its environment"
            )));
        }
        // Each player's verdicts key goes there, so another
        // device can't point it at a server of its own.
        if key == "verdicts_url" && !local {
            return Err(ApiError::forbidden("the verdicts service's address is set only on the computer UpNext runs on"));
        }
        let text = match value {
            Value::Null => String::new(),
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            other => other.to_string(),
        };
        if text.trim().is_empty() {
            writes.push((key, None));
        } else {
            let v = crate::config::validate_setting(key, &text).map_err(|e| ApiError::bad(e.to_string()))?;
            if key == "steam_id" {
                accounts::steam_id_free(&state, me, &v)?;
            }
            writes.push((key, Some(v)));
        }
    }
    if !local {
        openai_key_follows_its_address(&state, me, &mut writes)?;
    }
    // The announcement says whether HTTPS is on and the computer's name, so
    // it follows them too.
    let relisten = writes.iter().any(|(k, _)| matches!(*k, "allow_other_devices" | "tls_cert" | "tls_key" | "announce" | "network_name"));
    // Start with Windows is the Run key's value as much as a setting, so the
    // value goes first: a change Windows refused isn't saved as made.
    if let Some((_, on)) = writes.iter().find(|(k, _)| *k == "start_with_windows") {
        crate::autostart::set(on.as_deref() == Some("true"), crate::autostart::Why::Asked).map_err(|e| ApiError::bad(format!("{e:#}")))?;
    }
    for (key, v) in writes {
        state.db.setting_set(me, key, v.as_deref())?;
    }
    if relisten {
        state.remote.listen.apply().await;
    }
    Ok(Json(with_caller(settings_json(&state, me)?, local)))
}

/// The OpenAI-compatible key goes to whatever address is set, so from
/// another device a new address clears the saved key unless the same
/// request sends one: a lost phone could otherwise point it at a server of
/// its own and collect the key with the next model list or pick. A key from
/// the environment can't be cleared here, so then the address is this
/// computer's to change.
fn openai_key_follows_its_address(state: &AppState, me: PlayerId, writes: &mut Vec<(&str, Option<String>)>) -> Result<(), ApiError> {
    let Some((_, address)) = writes.iter().find(|(k, _)| *k == "openai_base_url") else {
        return Ok(());
    };
    if writes.iter().any(|(k, v)| *k == "openai_api_key" && v.is_some()) {
        return Ok(());
    }
    let cfg = state.cfg_for(me);
    let next = address.clone().unwrap_or_else(|| state.base_cfg.openai_base_url.clone());
    if cfg.openai_api_key.is_none() || next.trim_end_matches('/') == cfg.openai_base_url.trim_end_matches('/') {
        return Ok(());
    }
    if cfg.source("openai_api_key") != "app" {
        let why = "the OpenAI-compatible server's key is set in the environment, so its address is changed only on the computer \
            UpNext runs on, or with a new key";
        return Err(ApiError::forbidden(why));
    }
    writes.retain(|(k, _)| *k != "openai_api_key");
    writes.push(("openai_api_key", None));
    Ok(())
}

/// The libraries a sync can read, for the Settings page, with why the
/// current settings block one ("not connected"), if they do.
#[utoipa::path(get, path = "/sync/sources", tag = "jobs",
    description = "The libraries a sync can read, in order: `[{id, label, blocked}]`, with `blocked` saying why the \
        current settings block one, or null.",
    responses((status = 200, body = [Object], description = "`[{id, label, blocked}]`")))]
async fn sync_sources(State(state): State<AppState>) -> Json<Value> {
    let cfg = state.cfg();
    let list: Vec<Value> = crate::enrich::sync_libraries()
        .iter()
        .map(|l| json!({ "id": l.id(), "label": l.label(), "blocked": l.blocked(&cfg, &state.clients) }))
        .collect();
    Json(Value::Array(list))
}

/// What enrich can fetch, for the Settings page: each source's option with
/// its default, a note, and why the current settings block it, if they do.
#[utoipa::path(get, path = "/enrich/sources", tag = "jobs",
    description = "What enrich can fetch, in run order: `[{id, label, default, note, blocked}]`, each option with its \
        default, a note, and why the current settings block it, or null.",
    responses((status = 200, body = [Object], description = "`[{id, label, default, note, blocked}]`")))]
async fn enrich_sources(State(state): State<AppState>) -> Json<Value> {
    let cfg = state.cfg();
    let list: Vec<Value> = crate::enrich::sources::options()
        .iter()
        .map(|o| json!({ "id": o.id, "label": o.label, "default": o.default_on, "note": o.note, "blocked": (o.blocked)(&cfg) }))
        .collect();
    Json(Value::Array(list))
}
