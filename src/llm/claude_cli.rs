//! `claude -p` subprocess backend. Uses the user's Claude Code subscription
//! through the official CLI; no tokens are read or reused here. An AI
//! account's own token (`claude setup-token`) is passed to the CLI it starts.

use super::{LlmBackend, LlmError, LlmRequest, LlmResponse, Pricing, Usage, strip_fences, truncate};
use crate::process::{FeedError, Launch, RunError};
use async_trait::async_trait;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::Command;

pub struct ClaudeCliBackend {
    bin: String,
    workdir: Option<PathBuf>,
    timeout: Duration,
    /// An AI account's own sign-in, from `claude setup-token`,
    /// passed as `CLAUDE_CODE_OAUTH_TOKEN`; never read from or written to
    /// Claude Code's own files.
    token: Option<String>,
    /// An AI account other than this computer's, which must not fall back
    /// to the CLI's own sign-in here.
    apart: bool,
}

/// Why an AI account other than this computer's can't use the Claude CLI.
const NEEDS_OWN_SIGN_IN: &str = "this AI account uses the Claude Code CLI but has no sign-in of its own: run     `claude setup-token` and add the token it prints to the account in Settings, so it doesn't use this computer's";

impl ClaudeCliBackend {
    pub fn new(bin: String, workdir: Option<PathBuf>) -> Self {
        Self { bin, workdir, timeout: Duration::from_secs(300), token: None, apart: false }
    }

    /// Run the CLI with an AI account's own token; `apart` says the account
    /// isn't this computer's, so without a token it doesn't run at all.
    pub fn with_sign_in(mut self, token: Option<String>, apart: bool) -> Self {
        self.token = token.filter(|t| !t.trim().is_empty());
        self.apart = apart;
        self
    }

    fn command(&self) -> Command {
        let mut cmd = command(&Launch::of(&self.bin), self.workdir.as_deref());
        if let Some(t) = &self.token {
            cmd.env("CLAUDE_CODE_OAUTH_TOKEN", t);
        }
        cmd
    }

    fn signed_out(&self) -> bool {
        self.apart && self.token.is_none()
    }
}

/// `claude`, started as `process::Launch` says (on Windows an npm install's
/// `claude.cmd` is replaced by the `claude.exe` it starts), with no console
/// window.
fn command(launch: &Launch, workdir: Option<&Path>) -> Command {
    command_in(launch, workdir, crate::flatpak::get())
}

/// [`command`], in `sandbox` when UpNext runs in a Flatpak: there Claude Code
/// gets the app's own folder as its home, where `upnext install-claude` put
/// it, so its sign-in, settings and updates are written there; the real home
/// folder is out of the sandbox's reach.
fn command_in(launch: &Launch, workdir: Option<&Path>, sandbox: Option<&crate::flatpak::Flatpak>) -> Command {
    let mut cmd = launch.command();
    if let Some(f) = sandbox {
        cmd.env("HOME", &f.home);
    }
    // A neutral cwd keeps project CLAUDE.md files out of the context.
    if let Some(d) = workdir {
        let _ = std::fs::create_dir_all(d);
        cmd.current_dir(d);
    }
    // Allow running from inside another Claude Code session.
    cmd.env_remove("CLAUDECODE");
    cmd.env_remove("CLAUDE_CODE_ENTRYPOINT");
    cmd
}

/// What every `claude -p` UpNext runs is given.
const BASE_ARGS: [&str; 9] = [
    "-p",
    "--output-format",
    "json",
    "--tools",
    "",
    // No MCP servers from the user's config: this call only needs to write JSON.
    "--strict-mcp-config",
    "--no-session-persistence",
    // No hooks from the user's settings either. A SessionStart hook would
    // otherwise run on every call, and parallel calls would run it at once.
    "--settings",
    r#"{"disableAllHooks":true}"#,
];

/// The arguments for one call. The system prompt goes on the command line,
/// or in `system_file` when the CLI is a batch file, which can't be given an
/// argument with a line break (`process::Launch::batch`).
fn cli_args(req: &LlmRequest, system_file: Option<&Path>) -> Vec<String> {
    let mut args: Vec<String> = BASE_ARGS.iter().map(|s| s.to_string()).collect();
    match system_file {
        Some(f) => args.extend(["--system-prompt-file".into(), f.display().to_string()]),
        None => args.extend(["--system-prompt".into(), super::guarded_system(&req.system)]),
    }
    args.extend(["--json-schema".into(), req.schema.to_string()]);
    if let Some(m) = &req.model {
        args.extend(["--model".into(), m.clone()]);
    }
    args
}

/// The system prompt in a file of its own, for a batch-file CLI; removed
/// when dropped, after the call.
struct SystemFile(PathBuf);

impl SystemFile {
    fn write(workdir: Option<&Path>, system: &str) -> std::io::Result<SystemFile> {
        let dir = workdir.map(Path::to_path_buf).unwrap_or_else(std::env::temp_dir);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("upnext-system-{}.txt", uuid::Uuid::new_v4().simple()));
        std::fs::write(&path, system)?;
        Ok(SystemFile(path))
    }
}

impl Drop for SystemFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// How long a `/usage` read may take. It answers locally in about a second.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Why `claude` didn't start (`e`): Node.js missing for an npm install, or
/// the program itself.
fn cannot_run(bin: &str, e: &std::io::Error) -> String {
    crate::process::node_missing(bin).unwrap_or_else(|| format!("cannot run `{bin}`: {e}"))
}

/// Run `claude` with `args` and return what it printed, or why it couldn't;
/// signed in with an AI account's own `token` when it has one.
async fn run(bin: &str, workdir: Option<&Path>, token: Option<&str>, args: &[&str]) -> Result<String, String> {
    let mut cmd = command(&Launch::of(bin), workdir);
    if let Some(t) = token {
        cmd.env("CLAUDE_CODE_OAUTH_TOKEN", t);
    }
    cmd.args(args);
    let out = match crate::process::output(cmd, READ_TIMEOUT).await {
        Ok(o) => o,
        Err(RunError::Start(e)) => return Err(cannot_run(bin, &e)),
        Err(RunError::TimedOut) => return Err(format!("`{bin}` took more than {} s", READ_TIMEOUT.as_secs())),
    };
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() && stdout.is_empty() {
        return Err(format!("`{bin}` exited with {}: {}", out.status, truncate(String::from_utf8_lossy(&out.stderr).trim(), 300)));
    }
    Ok(stdout)
}

/// The envelope of `claude -p /usage`: the interactive `/usage` text,
/// answered by the CLI itself without asking the model, so it spends
/// nothing. `ai_plan` checks the envelope says so before trusting it. An
/// AI account's own `token` reads that account's plan.
pub async fn usage_envelope(bin: &str, workdir: Option<&std::path::Path>, token: Option<&str>) -> Result<Value, String> {
    let mut args = vec!["-p", "/usage"];
    args.extend(&BASE_ARGS[1..]);
    let out = run(bin, workdir, token, &args).await?;
    serde_json::from_str(&out)
        .map(result_envelope)
        .map_err(|e| format!("`claude -p /usage` didn't print JSON ({e}): {}", truncate(&out, 300)))
}

/// The envelope of a `--output-format json` call. Newer CLIs print the whole
/// message stream as an array and put the envelope last, as its `result`
/// entry; older ones print the envelope alone.
fn result_envelope(printed: Value) -> Value {
    match printed {
        Value::Array(items) => {
            items.into_iter().rev().find(|m| m.get("type").and_then(Value::as_str) == Some("result")).unwrap_or(Value::Null)
        }
        other => other,
    }
}

/// The kind of plan the CLI is signed in with ("max", "pro"), from `claude
/// auth status`. It doesn't say the tier (Max 5x or 20x).
pub async fn auth_plan(bin: &str, workdir: Option<&std::path::Path>, token: Option<&str>) -> Option<String> {
    let out = run(bin, workdir, token, &["auth", "status"]).await.ok()?;
    let v: Value = serde_json::from_str(&out).ok()?;
    v.get("subscriptionType").and_then(Value::as_str).map(str::to_string)
}

/// `claude --version`, to tell when the CLI has been updated.
pub async fn version(bin: &str) -> Option<String> {
    run(bin, None, None, &["--version"]).await.ok().filter(|v| !v.is_empty())
}

#[async_trait]
impl LlmBackend for ClaudeCliBackend {
    fn name(&self) -> &str {
        "claude-cli"
    }

    fn pricing(&self) -> Option<Pricing> {
        Some(Pricing { provider: crate::prices::Provider::Anthropic, charged: false })
    }

    fn plan_meter(&self) -> Option<crate::ai_plan::Meter> {
        // The usage bars are read from the sign-in the calls run on: this
        // computer's, or an AI account's own token. An account apart from
        // this computer's with no token has none to read.
        (!self.signed_out()).then(|| crate::ai_plan::Meter::Claude {
            bin: self.bin.clone(),
            workdir: self.workdir.clone(),
            token: self.token.clone(),
            account: None,
        })
    }

    async fn available(&self) -> Result<(), LlmError> {
        if self.signed_out() {
            return Err(LlmError::NotAvailable(NEEDS_OWN_SIGN_IN.into()));
        }
        // Every pick asks this first, so a CLI that hangs (an update or
        // sign-in prompt waiting on a terminal) must not hold the pick: it
        // gets no longer than a status read, nor than the call itself.
        let limit = READ_TIMEOUT.min(self.timeout);
        let mut cmd = self.command();
        cmd.arg("--version");
        match crate::process::output(cmd, limit).await {
            Ok(o) if o.status.success() => Ok(()),
            Ok(o) => Err(LlmError::NotAvailable(format!("`{} --version` failed: {}", self.bin, String::from_utf8_lossy(&o.stderr).trim()))),
            Err(RunError::Start(e)) => Err(LlmError::NotAvailable(cannot_run(&self.bin, &e))),
            Err(RunError::TimedOut) => {
                Err(LlmError::NotAvailable(format!("`{} --version` didn't answer within {} s", self.bin, limit.as_secs())))
            }
        }
    }

    async fn complete_json(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        if self.signed_out() {
            return Err(LlmError::NotAvailable(NEEDS_OWN_SIGN_IN.into()));
        }
        let launch = Launch::of(&self.bin);
        let system_file = match launch.batch {
            true => Some(
                SystemFile::write(self.workdir.as_deref(), &super::guarded_system(&req.system))
                    .map_err(|e| LlmError::Failed(format!("writing the system prompt for `{}`: {e}", self.bin)))?,
            ),
            false => None,
        };
        let mut cmd = command(&launch, self.workdir.as_deref());
        if let Some(t) = &self.token {
            cmd.env("CLAUDE_CODE_OAUTH_TOKEN", t);
        }
        cmd.args(cli_args(&req, system_file.as_ref().map(|f| f.0.as_path())));
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

        let started = Instant::now();
        let child = cmd.spawn().map_err(|e| LlmError::NotAvailable(cannot_run(&self.bin, &e)))?;
        let output = match crate::process::feed(child, req.full_prompt().as_bytes(), self.timeout).await {
            Ok(o) => o,
            Err(FeedError::Write(e)) => return Err(LlmError::Failed(format!("writing prompt: {e}"))),
            Err(FeedError::Wait(e)) => return Err(LlmError::Failed(format!("waiting for claude: {e}"))),
            Err(FeedError::TimedOut) => return Err(LlmError::Failed(format!("claude timed out after {}s", self.timeout.as_secs()))),
        };
        let duration_ms = started.elapsed().as_millis() as u64;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() {
            // A failed call still prints its envelope, with the reason in it.
            if let Ok(envelope) = serde_json::from_str::<Value>(stdout.trim()).map(result_envelope)
                && envelope.is_object()
            {
                return Err(envelope_error(&envelope));
            }
            let msg = if stderr.trim().is_empty() { stdout.trim().to_string() } else { stderr.trim().to_string() };
            return Err(LlmError::Failed(format!("claude exited with {}: {}", output.status, truncate(&msg, 800))));
        }

        let envelope: Value = serde_json::from_str(stdout.trim())
            .map(result_envelope)
            .map_err(|e| LlmError::BadOutput(format!("claude output is not JSON ({e}): {}", truncate(&stdout, 400)), None))?;
        if envelope.get("is_error").and_then(Value::as_bool) == Some(true)
            || envelope.get("subtype").and_then(Value::as_str).is_some_and(|s| s != "success")
        {
            return Err(envelope_error(&envelope));
        }
        let mut resp = success(&envelope, req.model.as_deref())?;
        resp.duration_ms = duration_ms;
        Ok(resp)
    }
}

/// A successful call's envelope: the JSON the model wrote, its model and
/// its tokens (`duration_ms` is left for the caller).
fn success(envelope: &Value, requested: Option<&str>) -> Result<LlmResponse, LlmError> {
    let usage = envelope.get("usage").and_then(Usage::from_anthropic);
    let json = match envelope.get("structured_output") {
        Some(v) if v.is_object() || v.is_array() => v.clone(),
        _ => {
            let text = envelope.get("result").and_then(Value::as_str).unwrap_or("");
            serde_json::from_str(strip_fences(text))
                .map_err(|e| LlmError::BadOutput(format!("result is not JSON ({e}): {}", truncate(text, 400)), usage))?
        }
    };
    let model = envelope
        .get("modelUsage")
        .and_then(Value::as_object)
        .and_then(|m| answered_by(m, requested))
        .or_else(|| requested.map(str::to_string));
    Ok(LlmResponse {
        json,
        model,
        usage,
        // Subscription use is not charged per call. The envelope's
        // `total_cost_usd` is what the call would cost on the API.
        cost_usd: None,
        api_usd: None,
        would_cost_usd: envelope.get("total_cost_usd").and_then(Value::as_f64),
        duration_ms: 0,
        flex: false,
    })
}

/// The model that answered, from the envelope's `modelUsage`. The CLI can
/// make side calls on another model (a Haiku call sits beside the Sonnet one),
/// and serde_json sorts the keys, so the first key isn't the answer. The
/// requested model wins when it's a key; otherwise the one that wrote the most
/// output tokens (the first of a tie).
fn answered_by(usage: &serde_json::Map<String, Value>, requested: Option<&str>) -> Option<String> {
    if let Some(r) = requested
        && usage.contains_key(r)
    {
        return Some(r.to_string());
    }
    let output = |v: &Value| v.get("outputTokens").and_then(Value::as_u64).unwrap_or(0);
    usage
        .iter()
        .fold(None::<(&String, u64)>, |best, (k, v)| match best {
            Some((_, n)) if n >= output(v) => best,
            _ => Some((k, output(v))),
        })
        .map(|(k, _)| k.clone())
}

/// What a failed call's envelope says. The CLI puts the reason in `result`
/// ("You've hit your limit · resets 4pm"), with `terminal_reason` and, when
/// the API turned the call down, `api_error_status`. The rest of the envelope
/// (usage, session ids) says nothing about why.
fn envelope_error(envelope: &Value) -> LlmError {
    let text = |k: &str| envelope.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    let result = text("result");
    let status = envelope.get("api_error_status").and_then(Value::as_u64);
    let mut details: Vec<String> =
        [text("terminal_reason"), text("subtype").filter(|s| *s != "success")].into_iter().flatten().map(str::to_string).collect();
    details.extend(status.map(|s| format!("HTTP {s}")));
    let mut msg = format!("Claude said: {}", truncate(result.unwrap_or("nothing about why"), 400));
    if !details.is_empty() {
        msg.push_str(&format!(" ({})", details.join(", ")));
    }
    let lower = result.unwrap_or_default().to_lowercase();
    let limited = status == Some(429) || ["usage limit", "hit your limit", "rate limit"].iter().any(|w| lower.contains(w));
    if limited {
        LlmError::LimitReached(msg)
    } else if matches!(status, Some(401 | 403 | 404)) {
        // Signed out, or a model that doesn't exist: every call would fail the same way.
        LlmError::NotAvailable(msg)
    } else {
        LlmError::Failed(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_a_flatpak_claude_codes_home_is_the_apps_folder() {
        let launch = Launch::of("claude");
        let home = |cmd: &Command| cmd.as_std().get_envs().find(|(k, _)| *k == "HOME").and_then(|(_, v)| v.map(PathBuf::from));
        assert_eq!(home(&command_in(&launch, None, None)), None, "outside one, the user's own home");
        let sandbox = crate::flatpak::Flatpak::example();
        assert_eq!(home(&command_in(&launch, None, Some(&sandbox))), Some(sandbox.home.clone()));
    }

    /// Claude Code 2.1.302 prints the message stream as an array, where
    /// earlier versions printed the envelope alone.
    #[test]
    fn the_envelope_is_the_last_result_of_a_message_stream() {
        let result = serde_json::json!({"type": "result", "subtype": "success", "structured_output": {"ok": true}});
        let stream = serde_json::json!([{"type": "system", "subtype": "init"}, {"type": "assistant"}, result.clone()]);
        assert_eq!(result_envelope(stream), result);
        assert_eq!(result_envelope(result.clone()), result, "older CLIs print the envelope alone");
        let resp = success(&result_envelope(serde_json::json!([result])), None).unwrap();
        assert_eq!(resp.json, serde_json::json!({"ok": true}));
    }

    /// A real envelope from a call that failed (`--model` set to a model
    /// that doesn't exist), and one made from it for a spent usage limit.
    #[test]
    fn a_failed_call_says_why() {
        let bad_model = crate::testutil::fixture_json("llm/claude_cli_bad_model.json");
        let err = envelope_error(&bad_model);
        let LlmError::NotAvailable(msg) = &err else { panic!("{err:?}") };
        assert!(msg.starts_with("Claude said: There's an issue with the selected model"), "{msg}");
        assert!(msg.ends_with("(api_error, HTTP 404)"), "{msg}");
        assert!(!msg.contains("session_id") && !msg.contains("usage"), "none of the envelope's other fields: {msg}");

        let limit = crate::testutil::fixture_json("llm/claude_cli_usage_limit.json");
        let err = envelope_error(&limit);
        let LlmError::LimitReached(msg) = &err else { panic!("{err:?}") };
        let msg = msg.clone();
        assert!(msg.contains("hit your limit") && msg.contains("HTTP 429"), "{msg}");
        let gave_up = crate::llm::gave_up(&anyhow::Error::new(err).context("digest generation"));
        assert_eq!(gave_up.as_deref(), Some(msg.as_str()), "found through the context a batch adds");
    }

    /// A real success: nearly all of the input is under
    /// `cache_creation_input_tokens`, and `total_cost_usd` is not a charge.
    #[test]
    fn a_success_reports_its_tokens() {
        let envelope = crate::testutil::fixture_json("llm/claude_cli_success.json");
        let resp = success(&envelope, Some("sonnet")).unwrap();
        assert_eq!(resp.json, serde_json::json!({"ok": true}));
        assert_eq!(resp.model.as_deref(), Some("claude-sonnet-5-5"));
        let usage =
            Usage { input_tokens: 2 + 884, output_tokens: 52, cache_read_tokens: 0, cache_write_tokens: 884, cache_write_1h_tokens: 884 };
        assert_eq!(resp.usage, Some(usage), "the CLI writes its cache for an hour");
        assert_eq!(resp.cost_usd, None, "a subscription call costs nothing per call");
        assert_eq!(resp.would_cost_usd, Some(0.00406));

        let mut broken = envelope.clone();
        broken["structured_output"] = Value::Null;
        broken["result"] = "Sure, here you go".into();
        let err = success(&broken, None).unwrap_err();
        assert!(matches!(err, LlmError::BadOutput(..)), "{err:?}");
        assert_eq!(err.usage().map(|u| u.input_tokens), Some(886), "an unusable reply's tokens still count");
    }

    /// The CLI's side call on Haiku sorts before Sonnet in `modelUsage`; the
    /// model that answered is still the one recorded.
    #[test]
    fn the_answering_model_is_recorded_beside_a_side_call() {
        let mut envelope = crate::testutil::fixture_json("llm/claude_cli_success.json");
        let mut usage = serde_json::Map::new();
        usage.insert("claude-haiku-4-5-20251001".into(), serde_json::json!({"inputTokens": 310, "outputTokens": 12}));
        usage.extend(envelope["modelUsage"].as_object().unwrap().clone());
        envelope["modelUsage"] = usage.into();
        let keys: Vec<&String> = envelope["modelUsage"].as_object().unwrap().keys().collect();
        assert_eq!(keys[0], "claude-haiku-4-5-20251001", "the side call comes first");

        // An alias isn't a key: the most output tokens decide.
        assert_eq!(success(&envelope, Some("sonnet")).unwrap().model.as_deref(), Some("claude-sonnet-5-5"));
        assert_eq!(success(&envelope, None).unwrap().model.as_deref(), Some("claude-sonnet-5-5"));
        // A requested model that is a key wins, whatever it wrote.
        envelope["modelUsage"]["claude-haiku-4-5-20251001"]["outputTokens"] = 900.into();
        assert_eq!(success(&envelope, Some("claude-sonnet-5-5")).unwrap().model.as_deref(), Some("claude-sonnet-5-5"));
        assert_eq!(success(&envelope, Some("sonnet")).unwrap().model.as_deref(), Some("claude-haiku-4-5-20251001"));
        // No modelUsage: the requested model, as before.
        envelope["modelUsage"] = serde_json::json!({});
        assert_eq!(success(&envelope, Some("sonnet")).unwrap().model.as_deref(), Some("sonnet"));
    }

    #[test]
    fn calls_skip_the_users_hooks() {
        let req = LlmRequest {
            system: "Write JSON.".into(),
            cached_prefix: None,
            prompt: "games".into(),
            schema: serde_json::json!({"type": "object"}),
            model: Some("sonnet".into()),
            background: false,
            effort: None,
        };
        let args = cli_args(&req, None);
        let at = args.iter().position(|a| a == "--settings").expect("--settings");
        let settings: Value = serde_json::from_str(&args[at + 1]).unwrap();
        assert_eq!(settings["disableAllHooks"], true);
        assert!(args.windows(2).any(|w| w == ["--model", "sonnet"]));
        assert!(args.windows(2).any(|w| w == ["--json-schema", r#"{"type":"object"}"#]));
        let system = &args[args.iter().position(|a| a == "--system-prompt").expect("--system-prompt") + 1];
        assert!(system.contains("Write JSON.") && system.contains('\n'), "the guarded prompt, line breaks and all: {system}");
        assert!(!args.iter().any(|a| a == "--system-prompt-file"));
    }

    /// For a batch-file CLI the system prompt goes in a file, which is
    /// removed after the call, and no argument has a line break, which
    /// `cmd.exe` can't be given.
    #[test]
    fn a_batch_file_cli_reads_the_system_prompt_from_a_file() {
        let req = LlmRequest {
            system: "Write JSON.\nOnly JSON.".into(),
            cached_prefix: None,
            prompt: "games".into(),
            schema: crate::prompt::schema(3, 2),
            model: Some("sonnet".into()),
            background: false,
            effort: None,
        };
        let dir = tempfile::tempdir().unwrap();
        let file = SystemFile::write(Some(dir.path()), &super::super::guarded_system(&req.system)).unwrap();
        let path = file.0.clone();
        let args = cli_args(&req, Some(&path));
        assert!(args.windows(2).any(|w| w[0] == "--system-prompt-file" && Path::new(&w[1]) == path));
        assert!(!args.iter().any(|a| a == "--system-prompt"));
        assert!(!args.iter().any(|a| a.contains(['\n', '\r'])), "{args:?}");
        assert!(std::fs::read_to_string(&path).unwrap().contains("Only JSON."));
        drop(file);
        assert!(!path.exists(), "the file goes with the call");
    }

    /// A CLI that hangs without reading its input, given a prompt larger
    /// than the pipe holds (a digest batch of non-Latin reviews can be):
    /// the call fails at its timeout instead of holding the job for ever.
    #[cfg(unix)] // The stand-in CLI is a shell script.
    #[tokio::test]
    async fn a_cli_that_never_reads_the_prompt_times_out() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("claude");
        std::fs::write(&bin, "#!/bin/sh\nexec sleep 60\n").unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let backend = ClaudeCliBackend {
            bin: bin.display().to_string(),
            workdir: None,
            timeout: Duration::from_millis(500),
            token: None,
            apart: false,
        };
        let req = LlmRequest {
            system: "Write JSON.".into(),
            cached_prefix: None,
            prompt: "Отзыв игрока о сюжете. ".repeat(4000),
            schema: serde_json::json!({"type": "object"}),
            model: Some("claude-sonnet-5-5".into()),
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

    /// A CLI that hangs on `--version` (an update or sign-in prompt waiting
    /// on a terminal): the probe every pick makes first gives up at the
    /// backend's timeout, here half a second, and says why, instead of
    /// holding the pick for ever (B-AI-3).
    #[cfg(unix)] // The stand-in CLI is a shell script.
    #[tokio::test]
    async fn a_hung_cli_does_not_hold_the_availability_probe() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("claude");
        std::fs::write(&bin, "#!/bin/sh\nexec sleep 60\n").unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let backend = ClaudeCliBackend {
            bin: bin.display().to_string(),
            workdir: None,
            timeout: Duration::from_millis(500),
            token: None,
            apart: false,
        };
        let started = Instant::now();
        let probe = tokio::time::timeout(Duration::from_secs(5), backend.available()).await;
        let err = probe.expect("available() was still waiting on `claude --version` after 5 s").unwrap_err();
        assert!(matches!(&err, LlmError::NotAvailable(m) if m.contains("--version` didn't answer")), "{err:?}");
        assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
    }

    /// On Windows, a call through a batch file that isn't npm's shim: the
    /// system prompt reaches it in a file, which is gone after the call.
    #[cfg(windows)]
    #[tokio::test]
    async fn on_windows_a_batch_file_cli_answers() {
        // `shift` moves `%0` too, so the batch file notes its folder first.
        let dir = tempfile::tempdir().unwrap();
        let bat = dir.path().join("claude.bat");
        std::fs::write(
            &bat,
            "@echo off\r\nset \"here=%~dp0\"\r\n:next\r\nif [%1]==[] goto run\r\nif [%1]==[--system-prompt-file] copy /y %2 \"%here%seen-system.txt\" >nul\r\nshift\r\ngoto next\r\n:run\r\nfindstr \"^\" >nul\r\necho {\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"\",\"structured_output\":{\"ok\":true}}\r\nexit /b 0\r\n",
        )
        .unwrap();
        let work = dir.path().join("work");
        let backend = ClaudeCliBackend::new(bat.display().to_string(), Some(work.clone()));
        let req = LlmRequest {
            system: "Write JSON.\nOnly JSON.".into(),
            cached_prefix: None,
            prompt: "games".into(),
            schema: serde_json::json!({"type": "object"}),
            model: Some("claude-sonnet-5-5".into()),
            background: false,
            effort: None,
        };
        let resp = backend.complete_json(req).await.unwrap();
        assert_eq!(resp.json, serde_json::json!({"ok": true}));
        let seen = std::fs::read_to_string(dir.path().join("seen-system.txt")).expect("the batch file got --system-prompt-file");
        assert!(seen.contains("Only JSON."), "{seen}");
        assert_eq!(std::fs::read_dir(&work).unwrap().count(), 0, "the system prompt's file is removed");
    }

    /// `cmd.exe` takes at most 8,191 characters. With the system prompt in
    /// a file, every request UpNext makes fits, with `"` doubled as Rust
    /// escapes it for a batch file.
    #[test]
    fn batch_file_command_lines_stay_short() {
        let program = r"C:\Users\someone.with.a.long.name\AppData\Roaming\npm\claude.cmd";
        let file =
            Path::new(r"C:\Users\someone.with.a.long.name\AppData\Local\upnext\cache\upnext-system-0123456789abcdef0123456789abcdef.txt");
        let schemas = [
            ("picks", crate::prompt::schema(5, 3)),
            ("review summaries", crate::digest::schema()),
            ("series guides", crate::series::schema()),
            ("hidden gems", crate::discovery::gems::schema()),
            ("second chances", crate::second_chance::schema()),
            ("too-similar check", crate::lineup::too_schema()),
        ];
        for (what, schema) in schemas {
            let req = LlmRequest {
                system: "Write JSON.".into(),
                cached_prefix: None,
                prompt: "games".into(),
                schema,
                model: Some("claude-sonnet-5-5".into()),
                background: false,
                effort: None,
            };
            let len = program.len() + cli_args(&req, Some(file)).iter().map(|a| a.len() + a.matches('"').count() + 3).sum::<usize>();
            assert!(len < 8191, "{what}: {len} characters");
        }
    }
}
