//! `codex exec` subprocess backend: the user's ChatGPT plan through the
//! official Codex CLI. UpNext never reads Codex's credentials; it runs the
//! binary the user signed in with (`codex login`), the way `claude_cli`
//! runs `claude`.
//!
//! Each call gets a private folder with the schema and the instructions,
//! which replace Codex's own (`codex exec` has no system prompt option).
//! Codex is an agent, so the call turns off what UpNext doesn't need: the
//! user's config, plugins and rules, tools, web search, and the parts of
//! Codex's prompt about permissions, skills and the environment. What can't
//! be turned off (the code-mode and multi-agent tools, about 3.2k tokens on
//! codex-cli 0.159.2) comes with every call. A global `AGENTS.md` in the
//! Codex home is sent too; the instructions say it doesn't apply.

use super::families::{ModelId, newest};
use super::{LlmBackend, LlmError, LlmRequest, LlmResponse, Pricing, Usage, strip_fences, truncate};
use crate::process::{FeedError, RunError};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// The model when none is set: the newest Luna (see `super::families`). Settings
/// always sets one (`codex_model`).
const DEFAULT_MODEL: &str = "luna";

pub struct CodexCliBackend {
    bin: String,
    effort: Option<String>,
    timeout: Duration,
    /// An AI account's own `CODEX_HOME`, where its own `codex
    /// login` is kept; None for this computer's.
    home: Option<PathBuf>,
}

impl CodexCliBackend {
    pub fn new(bin: String, effort: Option<String>) -> Self {
        Self { bin, effort, timeout: Duration::from_secs(300), home: None }
    }

    /// Run Codex with its own home, an AI account's.
    pub fn with_home(mut self, home: Option<PathBuf>) -> Self {
        self.home = home;
        self
    }
}

tokio::task_local! {
    /// The `CODEX_HOME` of the AI account a call runs on, for every `codex`
    /// that call starts; unset for this computer's own.
    static ACCOUNT_HOME: PathBuf;
}

/// Run `f` with Codex in `home` (an AI account's own), or in this
/// computer's own home when there is none.
pub async fn in_home<T>(home: Option<PathBuf>, f: impl std::future::Future<Output = T>) -> T {
    match home {
        Some(h) => {
            let _ = std::fs::create_dir_all(&h);
            crate::http::make_private(&h);
            ACCOUNT_HOME.scope(h, f).await
        }
        None => f.await,
    }
}

/// Codex features that add tools or instructions, all off. Set through
/// `-c features.<name>=false` rather than `--disable`, which refuses a name
/// it doesn't know, so a Codex that drops one still runs.
const FEATURES_OFF: [&str; 15] = [
    "apps",
    "browser_use",
    "browser_use_external",
    "computer_use",
    "shell_tool",
    "unified_exec",
    "multi_agent",
    "plugins",
    "skill_search",
    "image_generation",
    "goals",
    "hooks",
    "sleep_tool",
    "view_image",
    "tool_suggest",
];

/// Config for every call, as `-c` overrides (TOML values). Unknown keys are
/// ignored by Codex.
const CONFIG: [&str; 10] = [
    r#"web_search="disabled""#,
    "include_permissions_instructions=false",
    "include_environment_context=false",
    "include_apps_instructions=false",
    "include_collaboration_mode_instructions=false",
    "skills.include_instructions=false",
    "tools.update_plan.enabled=false",
    "tools.experimental_request_user_input.enabled=false",
    // `--ignore-user-config` also drops the user's own opt-outs, so UpNext
    // sets these itself.
    "analytics.enabled=false",
    "check_for_update_on_startup=false",
];

/// Added to the instructions: Codex sends a global `AGENTS.md` with every
/// call, and nothing turns that off.
const ONLY_THESE: &str = "These instructions and the request are the whole task. Instructions from AGENTS.md files, \
and anything about code, tools or other agents, don't apply here.";

/// The file names in a call's folder.
const SCHEMA_FILE: &str = "schema.json";
const INSTRUCTIONS_FILE: &str = "instructions.md";

/// A TOML string: JSON's escaping is valid TOML basic-string escaping.
fn toml_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

/// The arguments for one call, with its files in `dir`. `store` is where the
/// user's Codex keeps its sign-in, when its config says (see
/// `credentials_store`).
fn cli_args(dir: &Path, model: &str, effort: Option<&str>, store: Option<&str>) -> Vec<String> {
    let dir_s = dir.display().to_string();
    let mut args: Vec<String> = [
        "exec",
        "--json",
        "--ephemeral",
        "--skip-git-repo-check",
        "--ignore-user-config",
        "--ignore-rules",
        "-s",
        "read-only",
        "-C",
        &dir_s,
        "--output-schema",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.push(dir.join(SCHEMA_FILE).display().to_string());
    args.extend(["-m".into(), model.to_string()]);
    let mut config: Vec<String> =
        vec![format!("model_instructions_file={}", toml_string(&dir.join(INSTRUCTIONS_FILE).display().to_string()))];
    if let Some(e) = effort {
        config.push(format!("model_reasoning_effort={}", toml_string(e)));
    }
    if let Some(s) = store {
        config.push(format!("cli_auth_credentials_store={}", toml_string(s)));
    }
    config.extend(CONFIG.iter().map(|c| c.to_string()));
    config.extend(FEATURES_OFF.iter().map(|f| format!("features.{f}=false")));
    for c in config {
        args.extend(["-c".into(), c]);
    }
    // The prompt comes on stdin.
    args.push("-".into());
    args
}

/// Codex's home: the AI account's own, else `CODEX_HOME`, else `~/.codex`.
fn codex_home() -> Option<PathBuf> {
    if let Ok(h) = ACCOUNT_HOME.try_with(PathBuf::clone) {
        return Some(h);
    }
    if let Some(h) = std::env::var_os("CODEX_HOME").filter(|h| !h.is_empty()) {
        return Some(PathBuf::from(h));
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".codex"))
}

/// The `cli_auth_credentials_store` line of the user's `config.toml`, when
/// it has one at the top level. `--ignore-user-config` would otherwise make
/// Codex look for the sign-in in the wrong place (a keyring user looks
/// signed out). Only that one line is read.
fn credentials_store(config_toml: &str) -> Option<String> {
    for line in config_toml.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            return None;
        }
        let Some(rest) = line.strip_prefix("cli_auth_credentials_store") else { continue };
        let value = rest.trim_start().strip_prefix('=')?.split('#').next()?.trim();
        let value = value.strip_prefix('"')?.strip_suffix('"')?;
        return value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-').then(|| value.to_string());
    }
    None
}

fn user_credentials_store() -> Option<String> {
    credentials_store(&std::fs::read_to_string(codex_home()?.join("config.toml")).ok()?)
}

/// A private folder for one call's files, removed when dropped.
struct CallDir(PathBuf);

impl CallDir {
    fn new() -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!("upnext-codex-{}", uuid::Uuid::new_v4().simple()));
        #[cfg_attr(not(unix), allow(unused_mut))]
        let mut b = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
        b.create(&path)?;
        Ok(Self(path))
    }
}

impl Drop for CallDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Codex's structured outputs are strict: on top of what the Anthropic API
/// needs (no size or range constraints, `additionalProperties: false`),
/// every property must be required. An optional one becomes required and
/// may be null; `drop_added_nulls` takes those nulls out of the reply.
pub(crate) fn strict_schema(v: &Value) -> Value {
    all_required(&super::api_key::api_schema(v))
}

fn all_required(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut out: Map<String, Value> = m.iter().map(|(k, v)| (k.clone(), all_required(v))).collect();
            if let Some(Value::Object(props)) = out.get_mut("properties") {
                let required: Vec<&str> =
                    m.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
                for (k, p) in props.iter_mut() {
                    if !required.contains(&k.as_str()) {
                        *p = nullable(p);
                    }
                }
                let all: Vec<Value> = props.keys().map(|k| Value::String(k.clone())).collect();
                out.insert("required".into(), Value::Array(all));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(all_required).collect()),
        other => other.clone(),
    }
}

/// `p` or null.
fn nullable(p: &Value) -> Value {
    let mut p = p.clone();
    let Some(m) = p.as_object_mut() else { return p };
    match m.get("type").cloned() {
        Some(Value::String(t)) => {
            m.insert("type".into(), serde_json::json!([t, "null"]));
        }
        Some(Value::Array(mut ts)) => {
            if !ts.contains(&Value::from("null")) {
                ts.push("null".into());
            }
            m.insert("type".into(), Value::Array(ts));
        }
        _ => return serde_json::json!({"anyOf": [p, {"type": "null"}]}),
    }
    if let Some(Value::Array(e)) = m.get_mut("enum")
        && !e.contains(&Value::Null)
    {
        e.push(Value::Null);
    }
    p
}

/// Remove the nulls `strict_schema` allowed where the request's schema left
/// a property out of `required`, so the reply reads as that schema meant.
pub(crate) fn drop_added_nulls(json: &mut Value, schema: &Value) {
    match json {
        Value::Object(o) => {
            let Some(props) = schema.get("properties").and_then(Value::as_object) else { return };
            let required: Vec<&str> =
                schema.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
            o.retain(|k, v| !(v.is_null() && props.contains_key(k) && !required.contains(&k.as_str())));
            for (k, v) in o.iter_mut() {
                if let Some(s) = props.get(k) {
                    drop_added_nulls(v, s);
                }
            }
        }
        Value::Array(a) => {
            if let Some(items) = schema.get("items") {
                a.iter_mut().for_each(|v| drop_added_nulls(v, items));
            }
        }
        _ => {}
    }
}

/// What a call printed: the JSON Lines of `codex exec --json`.
#[derive(Debug, Default)]
struct Events {
    /// Codex started the thread. Without it, Codex failed before calling the
    /// model (a bad option, a config it can't read).
    started: bool,
    /// The last agent message: with an output schema, the JSON as text.
    answer: Option<String>,
    usage: Option<Usage>,
    /// Why the turn failed.
    failed: Option<String>,
    /// The last error event, not counting retries ("Reconnecting... 2/5").
    error: Option<String>,
}

fn read_events(stdout: &str) -> Events {
    let mut ev = Events::default();
    for line in stdout.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let text = |p: &str| v.pointer(p).and_then(Value::as_str).map(str::to_string);
        match v.get("type").and_then(Value::as_str) {
            Some("thread.started") => ev.started = true,
            Some("item.completed") if v.pointer("/item/type").and_then(Value::as_str) == Some("agent_message") => {
                ev.answer = text("/item/text");
            }
            Some("turn.completed") => ev.usage = v.get("usage").and_then(Usage::from_codex),
            Some("turn.failed") => ev.failed = text("/error/message"),
            Some("error") => {
                if let Some(m) = text("/message").filter(|m| !m.starts_with("Reconnecting...")) {
                    ev.error = Some(m);
                }
            }
            _ => {}
        }
    }
    ev
}

/// What a call came to, from its events and exit status.
fn outcome(ev: Events, exit_ok: bool, stderr: &str) -> Result<Value, LlmError> {
    if let Some(msg) = ev.failed.or(ev.error.filter(|_| !exit_ok)) {
        return Err(failure(&msg));
    }
    if !exit_ok {
        let msg = truncate(stderr.trim(), 600);
        // Before the thread starts, every call fails the same way.
        return Err(if ev.started {
            LlmError::Failed(format!("codex exited with an error: {msg}"))
        } else {
            LlmError::NotAvailable(format!("codex couldn't start: {msg}"))
        });
    }
    let Some(text) = ev.answer else {
        return Err(LlmError::BadOutput("Codex finished without an answer".into(), ev.usage));
    };
    serde_json::from_str(strip_fences(&text))
        .map_err(|e| LlmError::BadOutput(format!("Codex's answer is not JSON ({e}): {}", truncate(&text, 400)), ev.usage))
}

/// Why a turn failed, as an `LlmError`. The message is sometimes the API's
/// error body as JSON (`{"type":"error","status":400,"error":{"message":…}}`).
fn failure(msg: &str) -> LlmError {
    let body: Option<Value> = serde_json::from_str(msg).ok().filter(Value::is_object);
    let status = body.as_ref().and_then(|b| b.get("status")).and_then(Value::as_u64);
    let text = body.as_ref().and_then(|b| b.pointer("/error/message")).and_then(Value::as_str).unwrap_or(msg).trim();
    let lower = text.to_lowercase();
    let said = format!("Codex said: {}", truncate(text, 400));
    if status == Some(429) || ["usage limit", "hit your limit", "rate limit"].iter().any(|w| lower.contains(w)) {
        LlmError::LimitReached(said)
    } else if status == Some(401) || lower.contains("401 unauthorized") {
        LlmError::NotAvailable(format!("Codex isn't signed in, or its sign-in has expired: run `codex login` in a terminal. ({said})"))
    } else if matches!(status, Some(403 | 404)) || (lower.contains("model") && lower.contains("not supported")) {
        // A model the account can't use, or no access: every call would fail the same way.
        LlmError::NotAvailable(said)
    } else {
        LlmError::Failed(said)
    }
}

// -- status ---------------------------------------------------------------------------

/// How long a status command may take. They answer locally in milliseconds.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// `codex`, run as `process::Launch` says: on Windows an npm install's
/// `codex.cmd` is replaced by the node script it starts.
fn command(bin: &str) -> Command {
    let mut cmd = crate::process::Launch::of(bin).command();
    if let Ok(h) = ACCOUNT_HOME.try_with(PathBuf::clone) {
        cmd.env("CODEX_HOME", h);
    }
    cmd
}

/// Why `codex` didn't start (`e`): Node.js missing for an npm install, or
/// Codex itself.
fn cannot_run(bin: &str, e: &std::io::Error) -> String {
    crate::process::node_missing(bin).unwrap_or_else(|| {
        format!("cannot run `{bin}` ({e}): install the Codex CLI (`npm i -g @openai/codex`) and sign in with `codex login`")
    })
}

/// Run `codex` with `args`: whether it exited 0, and stdout and stderr.
async fn run(bin: &str, args: &[&str]) -> Result<(bool, String, String), String> {
    let mut cmd = command(bin);
    cmd.args(args);
    match crate::process::output(cmd, READ_TIMEOUT).await {
        Ok(o) => Ok((o.status.success(), String::from_utf8_lossy(&o.stdout).into(), String::from_utf8_lossy(&o.stderr).into())),
        Err(RunError::Start(e)) => Err(cannot_run(bin, &e)),
        Err(RunError::TimedOut) => Err(format!("`{bin}` took more than {} s", READ_TIMEOUT.as_secs())),
    }
}

/// How Codex is signed in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Login {
    pub signed_in: bool,
    /// What `codex login status` says: "Logged in using ChatGPT".
    pub status: String,
    /// The sign-in bills each call to a key, not to a ChatGPT plan.
    pub per_call: bool,
}

impl Login {
    /// `codex login status` prints one line (on stderr) and exits 1 when
    /// signed out.
    fn parse(ok: bool, out: &str) -> Login {
        let status =
            out.lines().map(str::trim).rfind(|l| l.starts_with("Logged in") || l.starts_with("Not logged in")).unwrap_or("").to_string();
        let per_call = status.contains("API key") || status.contains("Bedrock");
        Login { signed_in: ok && status.starts_with("Logged in"), status, per_call }
    }

    /// Why the AI can't run on this sign-in, if it can't.
    fn problem(&self) -> Option<String> {
        if !self.signed_in {
            Some("Codex isn't signed in: run `codex login` in a terminal, then try again".into())
        } else if self.per_call {
            Some(format!(
                "Codex is signed in with an API key ({}), which bills each call to that key. Sign in with your ChatGPT \
                 account instead (`codex login`), or use Another provider for an OpenAI API key",
                self.status
            ))
        } else {
            None
        }
    }
}

pub async fn login(bin: &str) -> Result<Login, String> {
    let (ok, stdout, stderr) = run(bin, &["login", "status"]).await?;
    Ok(Login::parse(ok, &format!("{stdout}\n{stderr}")))
}

/// `codex --version` ("codex-cli 0.159.2").
pub async fn version(bin: &str) -> Option<String> {
    let (ok, stdout, _) = run(bin, &["--version"]).await.ok()?;
    Some(stdout.trim().to_string()).filter(|v| ok && !v.is_empty())
}

/// The plan's usage limits: the result of `account/rateLimits/read` from
/// `codex app-server`, which answers from the account's usage without
/// calling a model, so it spends nothing. The server speaks JSON-RPC, one
/// message a line, on stdin and stdout. It must be initialized first, and
/// it exits without answering when stdin closes before the reply.
pub async fn rate_limits(bin: &str) -> Result<Value, String> {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let dir = CallDir::new().map_err(|e| format!("making a folder for `codex app-server`: {e}"))?;
    let mut cmd = command(bin);
    cmd.arg("app-server").current_dir(&dir.0).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = cmd.spawn().map_err(|e| cannot_run(bin, &e))?;
    let mut stdin = child.stdin.take().ok_or("no stdin for `codex app-server`")?;
    let stdout = child.stdout.take().ok_or("no stdout for `codex app-server`")?;
    let hello = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "clientInfo": {"name": "upnext", "title": "UpNext", "version": env!("CARGO_PKG_VERSION")}}});
    let ready = serde_json::json!({"jsonrpc": "2.0", "method": "initialized"});
    // Without the reset-credit details, as Codex's own background reads do.
    let read = serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "account/rateLimits/read",
        "params": {"excludeResetCreditDetails": true}});
    let talk = async {
        let lines = [hello, ready, read].iter().map(|m| format!("{m}\n")).collect::<String>();
        stdin.write_all(lines.as_bytes()).await.map_err(|e| format!("writing to `codex app-server`: {e}"))?;
        stdin.flush().await.map_err(|e| format!("writing to `codex app-server`: {e}"))?;
        let mut out = BufReader::new(stdout).lines();
        while let Some(line) = out.next_line().await.map_err(|e| format!("reading `codex app-server`: {e}"))? {
            let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
            match msg.get("id").and_then(Value::as_i64) {
                Some(1) if msg.get("error").is_some() => return Ok(msg),
                Some(2) => return Ok(msg),
                _ => {}
            }
        }
        Err("`codex app-server` closed without answering".to_string())
    };
    let reply = match tokio::time::timeout(READ_TIMEOUT, talk).await {
        Ok(r) => r?,
        Err(_) => return Err(format!("`codex app-server` took more than {} s", READ_TIMEOUT.as_secs())),
    };
    drop(stdin);
    let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
    if let Some(e) = reply.get("error") {
        let msg = e.get("message").and_then(Value::as_str).unwrap_or("no reason given");
        return Err(format!("`codex app-server` said: {msg}"));
    }
    reply
        .get("result")
        .cloned()
        .ok_or_else(|| format!("`codex app-server` answered without a result: {}", truncate(&reply.to_string(), 300)))
}

/// A model the ChatGPT account can use through Codex.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CodexModel {
    pub id: String,
    pub name: String,
    pub description: String,
    /// The reasoning efforts it takes, least to most.
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
}

/// Codex's model catalog (`codex debug models`): the models it lists for
/// the account, in its order.
fn parse_models(catalog: &Value) -> Vec<CodexModel> {
    let s = |m: &Value, k: &str| m.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    catalog
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|m| m.get("visibility").and_then(Value::as_str) == Some("list"))
        .filter_map(|m| {
            let id = s(m, "slug");
            (!id.is_empty()).then(|| CodexModel {
                name: Some(s(m, "display_name")).filter(|n| !n.is_empty()).unwrap_or_else(|| id.clone()),
                description: s(m, "description"),
                efforts: m
                    .get("supported_reasoning_levels")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|l| l.get("effort").and_then(Value::as_str).map(str::to_string))
                    .collect(),
                default_effort: m.get("default_reasoning_level").and_then(Value::as_str).map(str::to_string),
                id,
            })
        })
        .collect()
}

/// When the catalog can't be read: the models codex-cli 0.159.2 listed for
/// a Plus account on 2026-09-30, as (id, name, description, efforts, default).
const BUILT_IN_MODELS: [(&str, &str, &str, usize, &str); 8] = [
    ("gpt-6.1-sol", "GPT-6.1-Sol", "Latest workhorse model for coding and everyday work.", 6, "low"),
    ("gpt-6-astra", "GPT-6-Astra", "Frontier intelligence for the most demanding work.", 6, "low"),
    ("gpt-6-sol", "GPT-6-Sol", "Previous generation workhorse model.", 6, "medium"),
    ("gpt-6-luna", "GPT-6-Luna", "Fast and affordable model for easier tasks.", 5, "medium"),
    ("gpt-5.6-sol", "GPT-5.6-Sol", "Older generation workhorse model.", 6, "low"),
    ("gpt-5.6-terra", "GPT-5.6-Terra", "Older balanced model for straightforward work.", 6, "medium"),
    ("gpt-5.6-luna", "GPT-5.6-Luna", "Older fast and efficient model.", 5, "medium"),
    ("gpt-5.5", "GPT-5.5", "Legacy coding model.", 4, "medium"),
];

pub fn built_in_models() -> Vec<CodexModel> {
    BUILT_IN_MODELS
        .iter()
        .map(|(id, name, description, n, default)| CodexModel {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            efforts: crate::config::CODEX_EFFORTS[..*n].iter().map(|e| e.to_string()).collect(),
            default_effort: Some(default.to_string()),
        })
        .collect()
}

impl ModelId for CodexModel {
    fn model_id(&self) -> &str {
        &self.id
    }
}

/// The model to call for a setting: an id the catalog lists, or one that
/// isn't a family, stays as it is; a family becomes its newest model in the
/// account's catalog, or in the built-in copy when the catalog can't be read.
pub async fn resolve(bin: &str, wanted: &str) -> String {
    let models = models(bin).await.unwrap_or_else(|_| built_in_models());
    if models.iter().any(|m| m.id == wanted) {
        return wanted.to_string();
    }
    let built_in = built_in_models();
    newest(&models, wanted).or_else(|| newest(&built_in, wanted)).map_or_else(|| wanted.to_string(), |m| m.id.clone())
}

/// The models Codex lists for the account, from its local catalog.
pub async fn models(bin: &str) -> Result<Vec<CodexModel>, String> {
    let (ok, stdout, stderr) = run(bin, &["debug", "models"]).await?;
    if !ok {
        return Err(format!("`codex debug models` failed: {}", truncate(stderr.trim(), 300)));
    }
    let catalog: Value = serde_json::from_str(&stdout).map_err(|e| format!("`codex debug models` didn't print JSON ({e})"))?;
    let models = parse_models(&catalog);
    if models.is_empty() {
        return Err("`codex debug models` listed no models".into());
    }
    Ok(models)
}

#[async_trait]
impl LlmBackend for CodexCliBackend {
    fn name(&self) -> &str {
        "codex-cli"
    }

    /// Calls on a ChatGPT plan aren't charged per call; `api_usd` is what
    /// they'd cost on OpenAI's API.
    fn pricing(&self) -> Option<Pricing> {
        Some(Pricing { provider: crate::prices::Provider::OpenAi, charged: false })
    }

    fn plan_meter(&self) -> Option<crate::ai_plan::Meter> {
        // The usage bars are read from the sign-in the calls run on: this
        // computer's, or an AI account's own `CODEX_HOME`.
        Some(crate::ai_plan::Meter::Codex { bin: self.bin.clone(), home: self.home.clone(), account: None })
    }

    async fn available(&self) -> Result<(), LlmError> {
        in_home(self.home.clone(), async {
            let login = login(&self.bin).await.map_err(LlmError::NotAvailable)?;
            match login.problem() {
                Some(p) => Err(LlmError::NotAvailable(match &self.home {
                    Some(h) => format!("{p} (this AI account's own sign-in: CODEX_HOME={} codex login)", h.display()),
                    None => p,
                })),
                None => Ok(()),
            }
        })
        .await
    }

    async fn complete_json(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        in_home(self.home.clone(), self.complete_here(req)).await
    }
}

impl CodexCliBackend {
    async fn complete_here(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        // Signed out, `codex exec` retries for half a minute before it fails.
        self.available().await?;
        let dir = CallDir::new().map_err(|e| LlmError::Failed(format!("making a folder for the call: {e}")))?;
        let write = |name: &str, body: String| {
            std::fs::write(dir.0.join(name), body).map_err(|e| LlmError::Failed(format!("writing the call's {name}: {e}")))
        };
        write(SCHEMA_FILE, strict_schema(&req.schema).to_string())?;
        write(INSTRUCTIONS_FILE, format!("{}\n\n{ONLY_THESE}\n", super::guarded_system(&req.system)))?;
        let wanted = req.model.clone().filter(|m| !m.is_empty()).unwrap_or_else(|| DEFAULT_MODEL.to_string());
        let model = resolve(&self.bin, &wanted).await;
        let store = user_credentials_store();

        let mut cmd = command(&self.bin);
        cmd.current_dir(&dir.0).args(cli_args(&dir.0, &model, self.effort.as_deref(), store.as_deref()));
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let started = Instant::now();
        let child = cmd.spawn().map_err(|e| LlmError::NotAvailable(cannot_run(&self.bin, &e)))?;
        let output = match crate::process::feed(child, req.full_prompt().as_bytes(), self.timeout).await {
            Ok(o) => o,
            Err(FeedError::Write(e)) => return Err(LlmError::Failed(format!("writing prompt: {e}"))),
            Err(FeedError::Wait(e)) => return Err(LlmError::Failed(format!("waiting for codex: {e}"))),
            Err(FeedError::TimedOut) => return Err(LlmError::Failed(format!("codex timed out after {}s", self.timeout.as_secs()))),
        };
        let duration_ms = started.elapsed().as_millis() as u64;
        let ev = read_events(&String::from_utf8_lossy(&output.stdout));
        let usage = ev.usage;
        let mut json = outcome(ev, output.status.success(), &String::from_utf8_lossy(&output.stderr))?;
        drop_added_nulls(&mut json, &req.schema);
        // No event names the model, so it's the one asked for.
        Ok(LlmResponse { json, model: Some(model), usage, cost_usd: None, api_usd: None, would_cost_usd: None, duration_ms, flex: false })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn events(fixture: &str) -> Events {
        read_events(&crate::testutil::fixture(fixture))
    }

    /// A real success (codex-cli 0.159.2): the answer is the agent message's
    /// text, and the tokens come from `turn.completed`.
    #[test]
    fn a_success_reports_its_answer_and_tokens() {
        let ev = events("llm/codex_exec_success.jsonl");
        assert!(ev.started);
        let usage = ev.usage;
        assert_eq!(
            usage,
            Some(Usage { input_tokens: 4376, output_tokens: 15, cache_read_tokens: 0, cache_write_tokens: 0, cache_write_1h_tokens: 0 })
        );
        assert_eq!(outcome(ev, true, "").unwrap(), json!({"answer": "ready"}));

        let mut ev = events("llm/codex_exec_success.jsonl");
        ev.answer = Some("Sure, here it is".into());
        let err = outcome(ev, true, "").unwrap_err();
        assert!(matches!(err, LlmError::BadOutput(..)), "{err:?}");
        assert_eq!(err.usage(), usage, "an unusable answer's tokens still count");
    }

    /// Real failures: a model the account can't use, and a signed-out Codex
    /// (whose retries are error events too, and aren't the reason).
    #[test]
    fn a_failed_call_says_why() {
        let err = outcome(events("llm/codex_exec_bad_model.jsonl"), false, "").unwrap_err();
        let LlmError::NotAvailable(msg) = &err else { panic!("{err:?}") };
        assert_eq!(msg, "Codex said: The 'gpt-nonexistent-9' model is not supported when using Codex with a ChatGPT account.");

        let err = outcome(events("llm/codex_exec_signed_out.jsonl"), false, "").unwrap_err();
        let LlmError::NotAvailable(msg) = &err else { panic!("{err:?}") };
        assert!(msg.starts_with("Codex isn't signed in, or its sign-in has expired: run `codex login`"), "{msg}");
        assert!(!msg.contains("Reconnecting"), "{msg}");

        // Written from Codex's wording for a Plus plan's limit.
        let err = outcome(events("llm/codex_exec_usage_limit.jsonl"), false, "").unwrap_err();
        let LlmError::LimitReached(msg) = &err else { panic!("{err:?}") };
        assert!(msg.ends_with("try again at 3:45 PM."), "{msg}");
        let gave_up = crate::llm::gave_up(&anyhow::Error::new(err).context("digest generation"));
        assert!(gave_up.is_some(), "a batch stops at the limit");

        // Codex refused to start (a bad option): nothing else will work either.
        let err = outcome(Events::default(), false, "Error: Unknown feature flag: shell_toool\n").unwrap_err();
        assert!(matches!(&err, LlmError::NotAvailable(m) if m.ends_with("Unknown feature flag: shell_toool")), "{err:?}");
        let started = Events { started: true, ..Events::default() };
        assert!(matches!(outcome(started, false, "boom").unwrap_err(), LlmError::Failed(_)));
    }

    #[test]
    fn calls_turn_off_what_upnext_doesnt_need() {
        let dir = Path::new("/tmp/upnext-codex-x");
        let args = cli_args(dir, "gpt-6-luna", Some("low"), None);
        let schema = dir.join("schema.json").display().to_string();
        let joined = args.join(" ");
        assert_eq!(
            &args[..12],
            [
                "exec",
                "--json",
                "--ephemeral",
                "--skip-git-repo-check",
                "--ignore-user-config",
                "--ignore-rules",
                "-s",
                "read-only",
                "-C",
                "/tmp/upnext-codex-x",
                "--output-schema",
                schema.as_str()
            ]
        );
        assert!(args.windows(2).any(|w| w == ["-m", "gpt-6-luna"]));
        let configs: Vec<&str> = args.windows(2).filter(|w| w[0] == "-c").map(|w| w[1].as_str()).collect();
        // A TOML string: on Windows the backslashes are escaped.
        let instructions = dir.join("instructions.md").display().to_string().replace('\\', "\\\\");
        assert_eq!(configs[0], format!(r#"model_instructions_file="{instructions}""#));
        assert_eq!(configs[1], r#"model_reasoning_effort="low""#);
        for f in FEATURES_OFF {
            assert!(configs.contains(&format!("features.{f}=false").as_str()), "{f}");
        }
        assert!(configs.contains(&r#"web_search="disabled""#) && configs.contains(&"include_environment_context=false"));
        assert!(!joined.contains("--disable"), "an unknown --disable name stops Codex");
        assert_eq!(args.last().map(String::as_str), Some("-"), "the prompt comes on stdin");

        let plain = cli_args(dir, "gpt-6-astra", None, Some("keyring"));
        assert!(!plain.iter().any(|a| a.starts_with("model_reasoning_effort")), "Codex's default effort");
        assert!(plain.iter().any(|a| a == r#"cli_auth_credentials_store="keyring""#));
        // A Windows path's backslashes are escaped for TOML.
        assert_eq!(toml_string(r#"C:\Temp\a "b""#), r#""C:\\Temp\\a \"b\"""#);
    }

    #[test]
    fn the_keyring_setting_is_read_from_the_top_level_only() {
        assert_eq!(credentials_store("cli_auth_credentials_store = \"keyring\"  # mine\n[tui]\n").as_deref(), Some("keyring"));
        assert_eq!(credentials_store("model = \"x\"\ncli_auth_credentials_store=\"auto\"").as_deref(), Some("auto"));
        assert_eq!(credentials_store("[tui]\ncli_auth_credentials_store = \"keyring\"\n"), None, "inside a table");
        assert_eq!(credentials_store("cli_auth_credentials_store = \"a\\\"b\"\n"), None);
        assert_eq!(credentials_store(""), None);
    }

    /// Every property is required, optional ones may be null, constraints
    /// go, and every object closes.
    #[test]
    fn the_schema_is_made_strict() {
        let s = json!({
            "type": "object",
            "properties": {
                "picks": {"type": "array", "minItems": 3, "items": {"type": "object",
                    "properties": {"id": {"type": "integer", "minimum": 1}, "note": {"type": "string"},
                        "kind": {"type": "string", "enum": ["a", "b"]}, "any": {"anyOf": [{"type": "integer"}, {"type": "string"}]}},
                    "required": ["id"]}},
            },
            "required": ["picks"],
        });
        let out = strict_schema(&s);
        assert!(strict_problems(&out).is_empty(), "{:?}", strict_problems(&out));
        let item = &out["properties"]["picks"]["items"];
        assert_eq!(item["required"], json!(["id", "note", "kind", "any"]), "every property, in the schema's order");
        assert_eq!(item["properties"]["id"], json!({"type": "integer"}), "required stays as it was");
        assert_eq!(item["properties"]["note"]["type"], json!(["string", "null"]));
        assert_eq!(item["properties"]["kind"]["enum"], json!(["a", "b", null]));
        assert_eq!(item["properties"]["any"]["anyOf"][1], json!({"type": "null"}));

        let mut reply =
            json!({"picks": [{"id": 1, "note": null, "kind": "a", "any": null}, {"id": 2, "note": "x", "kind": null, "any": 3}]});
        drop_added_nulls(&mut reply, &s);
        assert_eq!(reply, json!({"picks": [{"id": 1, "kind": "a"}, {"id": 2, "note": "x", "any": 3}]}));
    }

    /// The schemas UpNext sends are valid for strict mode once adapted.
    #[test]
    fn upnexts_schemas_pass_strict_mode() {
        let decide = json!({"type": "object", "properties": {"pick_id": {"type": "integer"}, "reason": {"type": "string"}},
            "required": ["pick_id", "reason"], "additionalProperties": false});
        for (name, s) in [
            ("picks", crate::prompt::schema(3, 6)),
            ("too", crate::lineup::too_schema()),
            ("summaries", crate::digest::schema()),
            ("series guides", crate::series::schema()),
            ("second chances", crate::second_chance::schema()),
            ("decide", decide),
        ] {
            assert_eq!(strict_problems(&strict_schema(&s)), Vec::<String>::new(), "{name}");
        }
    }

    /// What strict mode would reject.
    fn strict_problems(s: &Value) -> Vec<String> {
        fn walk(v: &Value, path: &str, out: &mut Vec<String>) {
            match v {
                Value::Object(m) => {
                    for k in ["minItems", "maxItems", "minimum", "maximum", "minLength", "maxLength", "multipleOf"] {
                        if m.contains_key(k) {
                            out.push(format!("{path}: {k}"));
                        }
                    }
                    if let Some(Value::Object(props)) = m.get("properties") {
                        if m.get("additionalProperties") != Some(&Value::Bool(false)) {
                            out.push(format!("{path}: additionalProperties"));
                        }
                        let req: Vec<&str> =
                            m.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
                        for k in props.keys().filter(|k| !req.contains(&k.as_str())) {
                            out.push(format!("{path}: {k} not required"));
                        }
                    }
                    for (k, c) in m {
                        walk(c, &format!("{path}/{k}"), out);
                    }
                }
                Value::Array(a) => a.iter().enumerate().for_each(|(i, c)| walk(c, &format!("{path}/{i}"), out)),
                _ => {}
            }
        }
        let mut out = vec![];
        if s.get("type").and_then(Value::as_str) != Some("object") {
            out.push("root is not an object".into());
        }
        walk(s, "", &mut out);
        out
    }

    #[test]
    fn login_status_says_whether_the_plan_is_used() {
        let chatgpt = Login::parse(true, "\nLogged in using ChatGPT\n");
        assert_eq!((chatgpt.signed_in, chatgpt.per_call, chatgpt.problem()), (true, false, None));
        let out = Login::parse(false, "WARNING: proceeding, even though we could not create PATH aliases\nNot logged in\n");
        assert!(!out.signed_in && out.problem().unwrap().contains("run `codex login`"));
        let key = Login::parse(true, "Logged in using an API key - sk-proj-***ABCD\n");
        assert!(key.signed_in && key.per_call);
        assert!(key.problem().unwrap().contains("Another provider"));
    }

    /// The app-server answers only after it's initialized and only while
    /// stdin is open; its error says why it couldn't read the limits.
    #[cfg(unix)] // The stand-in app-server is a shell script.
    #[tokio::test]
    async fn the_app_server_is_asked_for_the_limits() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let fake = |name: &str, reply: &str| {
            let bin = dir.path().join(name);
            let script = format!(
                "#!/bin/sh\n[ \"$1\" = app-server ] || exit 2\nwhile read -r line; do case \"$line\" in *'\"id\":1,'*) echo '{{\"id\":1,\"result\":{{}}}}' ;; *rateLimits/read*) {reply} ;; esac; done\n"
            );
            std::fs::write(&bin, script).unwrap();
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
            bin.display().to_string()
        };
        let fixture = |name: &str| crate::testutil::fixture_path(name).display().to_string();

        let ok = fake("ok", &format!("tr -d '\\n' < '{}'; echo", fixture("llm/codex_app_server_rate_limits.json")));
        assert_eq!(rate_limits(&ok).await.unwrap()["rateLimits"]["planType"], "plus");

        let signed_out = fake("signed-out", &format!("cat '{}'", fixture("llm/codex_app_server_signed_out.json")));
        assert_eq!(
            rate_limits(&signed_out).await.unwrap_err(),
            "`codex app-server` said: codex account authentication required to read rate limits"
        );

        let quits = fake("quits", "exit 0");
        assert_eq!(rate_limits(&quits).await.unwrap_err(), "`codex app-server` closed without answering");
        assert!(rate_limits(&dir.path().join("missing").display().to_string()).await.unwrap_err().contains("npm i -g @openai/codex"));
    }

    /// A Codex that hangs without reading its input, given a prompt larger
    /// than the pipe holds: the call fails at its timeout instead of
    /// holding the job for ever.
    #[cfg(unix)] // The stand-in CLI is a shell script.
    #[tokio::test]
    async fn a_cli_that_never_reads_the_prompt_times_out() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("codex");
        let script =
            "#!/bin/sh\ncase \"$1\" in\n  login) echo 'Logged in using ChatGPT' >&2 ;;\n  exec) exec sleep 60 ;;\n  *) exit 1 ;;\nesac\n";
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let backend = CodexCliBackend { bin: bin.display().to_string(), effort: None, timeout: Duration::from_millis(500), home: None };
        let req = LlmRequest {
            system: "Write JSON.".into(),
            cached_prefix: None,
            prompt: "Отзыв игрока о сюжете. ".repeat(4000),
            schema: json!({"type": "object", "properties": {}}),
            model: Some("gpt-6-luna".into()),
            background: true,
            effort: None,
        };
        assert!(req.full_prompt().len() > 64 * 1024);
        let started = Instant::now();
        let call = tokio::time::timeout(Duration::from_secs(10), backend.complete_json(req));
        let err = call.await.expect("the call ended on its own").unwrap_err();
        assert!(matches!(&err, LlmError::Failed(m) if m.contains("timed out")), "{err:?}");
        assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    }

    /// The catalog lists what the account can use; hidden models stay out.
    #[test]
    fn models_come_from_codexs_catalog() {
        let models = parse_models(&crate::testutil::fixture_json("llm/codex_debug_models.json"));
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["gpt-6-astra", "gpt-6-luna", "gpt-5.5"]);
        let luna = &models[1];
        assert_eq!((luna.name.as_str(), luna.default_effort.as_deref()), ("GPT-6-Luna", Some("medium")));
        assert_eq!(luna.efforts, ["low", "medium", "high", "xhigh", "max"]);
        assert!(parse_models(&json!({"models": "nope"})).is_empty());

        let built_in = built_in_models();
        assert_eq!(built_in.iter().find(|m| m.id == "gpt-6-luna").map(|m| m.efforts.len()), Some(5));
        assert!(newest(&built_in, DEFAULT_MODEL).is_some());
        assert_eq!(built_in.iter().find(|m| m.id == "gpt-5.5").unwrap().efforts.last().map(String::as_str), Some("xhigh"));
    }
}
