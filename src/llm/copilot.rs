//! Microsoft Copilot on Windows: the chat on copilot.com, as the signed-in
//! Microsoft account Windows already has. No key, and no program of its own
//! beyond Microsoft's `msalruntime.dll`.
//!
//! 1. A token for the account comes from `msalruntime.dll`, silently, through
//!    a PowerShell helper (`copilot_token.ps1`): UpNext forbids `unsafe`, so it
//!    doesn't call the DLL itself. Git for Windows ships the DLL, or
//!    `UPNEXT_MSALRUNTIME` names one.
//! 2. The chat is a SignalR socket on `substrate.office.com`, the one the
//!    copilot.com page opens: a handshake, then one `chat` call whose reply
//!    comes back as the call's result.
//!
//! Copilot takes no JSON schema, so the schema goes in the prompt and the
//! reply is read as text. It reports no tokens. The free allowance is 50
//! messages a day, which every call counts against, and each call leaves a
//! chat in the account's Copilot history.

use super::{LlmBackend, LlmError, LlmRequest, LlmResponse, strip_fences, truncate};
use async_trait::async_trait;
use base64::Engine;
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::{self, Message, client::IntoClientRequest};

const HUB: &str = "wss://substrate.office.com";
/// The chat page's own origin, which the hub checks.
const ORIGIN: &str = "https://copilot.com";
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/155.0.0.0 Safari/537.36 Edg/155.0.0.0";
/// What the hub expects after each SignalR message.
const RECORD_END: char = '\u{1e}';

/// The helper that prints a token, run by PowerShell.
const TOKEN_SCRIPT: &str = include_str!("copilot_token.ps1");

/// Why Copilot can't run: the DLL its token needs isn't on this computer.
const NEEDS_MSAL_RUNTIME: &str =
    "Copilot needs Microsoft's msalruntime.dll, which Git for Windows ships: install it, or set UPNEXT_MSALRUNTIME to the DLL's path";

pub struct CopilotBackend {
    /// The hub's address; a test's fake one has its own.
    hub: String,
    /// A token to use instead of asking Windows for one, for a test.
    token: Option<String>,
    timeout: Duration,
}

impl CopilotBackend {
    pub fn new() -> Self {
        Self { hub: HUB.into(), token: None, timeout: Duration::from_secs(180) }
    }
}

/// `msalruntime.dll`: the one `UPNEXT_MSALRUNTIME` names, else the copy in
/// Git for Windows.
fn msal_runtime() -> Option<PathBuf> {
    if let Some(named) = std::env::var_os("UPNEXT_MSALRUNTIME") {
        return Some(PathBuf::from(named));
    }
    let git_dll = |root: PathBuf| root.join("Git").join("mingw64").join("bin").join("msalruntime.dll");
    ["ProgramFiles", "ProgramFiles(x86)"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(|root| git_dll(root.into()))
        .chain(std::env::var_os("LOCALAPPDATA").map(|root| git_dll(PathBuf::from(root).join("Programs"))))
        .find(|dll| dll.is_file())
}

/// A token for the Microsoft account Windows is signed in to.
async fn windows_token() -> Result<String, LlmError> {
    let dll = msal_runtime().ok_or_else(|| LlmError::NotAvailable(NEEDS_MSAL_RUNTIME.into()))?;
    let script: Vec<u8> = TOKEN_SCRIPT.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut cmd = crate::process::command("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand"])
        .arg(base64::engine::general_purpose::STANDARD.encode(script))
        .env("UPNEXT_MSALRUNTIME", dll);
    let out = crate::process::output(cmd, Duration::from_secs(40))
        .await
        .map_err(|e| LlmError::NotAvailable(format!("cannot run PowerShell: {e:?}")))?;
    let printed = String::from_utf8_lossy(&out.stdout).trim().to_string();
    match out.status.success() && !printed.is_empty() {
        true => Ok(printed),
        false => {
            Err(LlmError::NotAvailable(format!("Copilot has no token: {}", truncate(String::from_utf8_lossy(&out.stderr).trim(), 300))))
        }
    }
}

/// What to send: the system prompt, the shape of the answer, then the request.
fn prompt_of(req: &LlmRequest) -> String {
    format!(
        "{}\n\nAnswer with only a JSON value that matches this JSON Schema, with no code fence and nothing around it:\n{}\n\n{}",
        super::guarded_system(&req.system),
        req.schema,
        req.full_prompt()
    )
}

/// One SignalR message: its JSON and the record separator.
fn record(v: &Value) -> Message {
    Message::text(format!("{v}{RECORD_END}"))
}

/// The `chat` call, with the least the hub accepts: no web search, no
/// plugins, and a new conversation every time.
fn chat_call(id: &str, prompt: &str) -> Value {
    json!({
        "arguments": [{
            "source": "officeweb",
            "clientCorrelationId": id,
            "sessionId": id,
            "requestId": id,
            "traceId": id,
            "isStartOfSession": true,
            "optionsSets": ["enable_msa_user"],
            "streamingMode": "ConciseWithPadding",
            "options": {},
            "extraExtensionParameters": {},
            "allowedMessageTypes": ["Chat", "Progress", "EndOfRequest"],
            "sliceIds": [],
            "threadLevelGptId": {},
            "message": {
                "author": "user",
                "inputMethod": "Keyboard",
                "text": prompt,
                "requestId": id,
                "locale": "en-us",
                "messageType": "Chat",
                "experienceType": "Default",
                "adaptiveCards": [],
            },
            "plugins": [],
            "tone": "Magic",
            "disconnectBehavior": "continue",
        }],
        "invocationId": "0",
        "target": "chat",
        "type": 4,
    })
}

/// The reply in a finished `chat` call (SignalR's type 2 message), or why
/// there isn't one.
fn reply_of(call: &Value) -> Result<String, LlmError> {
    if let Some(e) = call.get("error").and_then(Value::as_str) {
        return Err(LlmError::Failed(format!("Copilot: {}", truncate(e, 300))));
    }
    let item = &call["item"];
    let meter = item.pointer("/result/meteringInformation/0");
    if meter.is_some_and(|m| m["hasAccess"] == false || m["remainingAllowance"].as_f64() == Some(0.0)) {
        let resets = meter.and_then(|m| m["resetAt"].as_str()).unwrap_or("later");
        return Err(LlmError::LimitReached(format!("Copilot's free messages for today are used up; they reset at {resets}")));
    }
    if let Some(value) = item.pointer("/result/value").and_then(Value::as_str).filter(|v| *v != "Success") {
        let why = item.pointer("/result/message").and_then(Value::as_str).unwrap_or("");
        return Err(LlmError::Failed(format!("Copilot answered {value}: {}", truncate(why, 300))));
    }
    // The chat's own answer is the last bot message that has text and isn't a
    // progress note, hint or other typed message.
    item["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["author"] == "bot" && m.get("messageType").is_none())
        .filter_map(|m| m["text"].as_str())
        .next_back()
        .map(str::to_string)
        .ok_or_else(|| LlmError::Failed("Copilot sent no answer".into()))
}

/// One chat: the reply's text.
async fn chat(hub: &str, token: &str, prompt: &str) -> Result<String, LlmError> {
    let ws_err = |e: tungstenite::Error| LlmError::Failed(format!("Copilot's chat: {e}"));
    let id = uuid::Uuid::new_v4().simple().to_string();
    let url = format!(
        "{hub}/m365Copilot/Chathub/?chatsessionid={id}&clientrequestid={id}&X-SessionId={}&ConversationId={}&access_token={token}\
         &source=%22officeweb%22&product=Office&agentHost=Bizchat.FullScreen&licenseType=Free&isEdu=false&agent=web&scenario=OfficeWebFreeConsumerCopilot",
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4()
    );
    let mut request = url.into_client_request().map_err(ws_err)?;
    request.headers_mut().insert("origin", ORIGIN.parse().expect("a header value"));
    request.headers_mut().insert("user-agent", USER_AGENT.parse().expect("a header value"));
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.map_err(|e| match e {
        // The hub turns a token it doesn't take away with a 401.
        tungstenite::Error::Http(r) if r.status() == 401 => LlmError::NotAvailable("Copilot refused the account's token (HTTP 401)".into()),
        e => ws_err(e),
    })?;
    socket.send(record(&json!({"protocol": "json", "version": 1}))).await.map_err(ws_err)?;

    let mut sent = false;
    while let Some(message) = socket.next().await {
        let Message::Text(text) = message.map_err(ws_err)? else { continue };
        for part in text.split(RECORD_END).filter(|p| !p.is_empty()) {
            let Ok(frame) = serde_json::from_str::<Value>(part) else { continue };
            // `{}` answers the handshake: now the chat can be asked.
            if !sent && frame.as_object().is_some_and(|f| f.is_empty()) {
                sent = true;
                socket.send(record(&chat_call(&id, prompt))).await.map_err(ws_err)?;
                continue;
            }
            match frame["type"].as_u64() {
                Some(2) => {
                    let _ = socket.close(None).await;
                    return reply_of(&frame);
                }
                Some(3) if frame.get("error").is_some() => return reply_of(&frame),
                Some(7) => return Err(LlmError::Failed(format!("Copilot closed the chat: {}", truncate(part, 300)))),
                _ => {}
            }
        }
    }
    Err(LlmError::Failed("Copilot's chat ended with no answer".into()))
}

#[async_trait]
impl LlmBackend for CopilotBackend {
    fn name(&self) -> &str {
        "copilot"
    }

    /// Whether the DLL the token needs is here. Whether Windows has an
    /// account to give a token to is only known by asking for one, which every
    /// pick would pay for.
    async fn available(&self) -> Result<(), LlmError> {
        match self.token.is_some() || msal_runtime().is_some() {
            true => Ok(()),
            false => Err(LlmError::NotAvailable(NEEDS_MSAL_RUNTIME.into())),
        }
    }

    async fn complete_json(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        let started = Instant::now();
        let token = match &self.token {
            Some(t) => t.clone(),
            None => windows_token().await?,
        };
        let reply = tokio::time::timeout(self.timeout, chat(&self.hub, &token, &prompt_of(&req)))
            .await
            .map_err(|_| LlmError::Failed(format!("Copilot timed out after {}s", self.timeout.as_secs())))??;
        let json = serde_json::from_str(strip_fences(&reply))
            .map_err(|e| LlmError::BadOutput(format!("reply is not JSON ({e}): {}", truncate(&reply, 400)), None))?;
        Ok(LlmResponse {
            json,
            model: None,
            usage: None,
            cost_usd: None,
            api_usd: None,
            would_cost_usd: None,
            duration_ms: started.elapsed().as_millis() as u64,
            flex: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> LlmRequest {
        LlmRequest {
            system: "Pick games.".into(),
            cached_prefix: None,
            prompt: "Something short".into(),
            schema: json!({"type": "object"}),
            model: None,
            background: false,
            effort: None,
        }
    }

    /// A fake hub on a local port: it answers the handshake, checks the
    /// `chat` call and the token, and replies the way the real one does, with
    /// a hint and the answer in a code fence. Returns its address.
    #[allow(clippy::result_large_err)]
    async fn fake_hub() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("ws://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut seen_url = String::new();
            let callback = |request: &tungstenite::handshake::server::Request, response| {
                seen_url = request.uri().to_string();
                Ok(response)
            };
            let mut socket = tokio_tungstenite::accept_hdr_async(stream, callback).await.unwrap();
            assert!(seen_url.starts_with("/m365Copilot/Chathub/?") && seen_url.contains("access_token=a-token"), "{seen_url}");
            while let Some(Ok(Message::Text(text))) = socket.next().await {
                let frame: Value = serde_json::from_str(text.trim_end_matches(RECORD_END)).unwrap();
                if frame["protocol"] == "json" {
                    socket.send(record(&json!({}))).await.unwrap();
                } else if frame["target"] == "chat" {
                    let sent = &frame["arguments"][0]["message"];
                    assert!(sent["text"].as_str().unwrap().starts_with("Pick games."), "{frame}");
                    let done = json!({"type": 2, "invocationId": "0", "item": {
                        "messages": [
                            {"author": "user", "text": "Pick games."},
                            {"author": "bot", "messageType": "HintInvocation", "invocation": "respond_language()"},
                            {"author": "bot", "text": "```json\n{\"pick\": 7}\n```"},
                        ],
                        "result": {"value": "Success", "meteringInformation": [{"hasAccess": true, "remainingAllowance": 40}]},
                    }});
                    socket.send(record(&json!({"type": 1, "target": "update"}))).await.unwrap();
                    socket.send(record(&done)).await.unwrap();
                }
            }
        });
        address
    }

    #[tokio::test]
    async fn a_chat_answers_with_the_reply_as_json() {
        let backend = CopilotBackend { hub: fake_hub().await, token: Some("a-token".into()), timeout: Duration::from_secs(10) };
        let resp = backend.complete_json(request()).await.unwrap();
        assert_eq!(resp.json, json!({"pick": 7}));
    }

    #[test]
    fn the_reply_is_the_last_answer_that_is_not_a_typed_message() {
        let call = json!({"type": 2, "item": {"messages": [
            {"author": "bot", "text": "first"},
            {"author": "bot", "messageType": "Progress", "text": "Just a sec…"},
            {"author": "bot", "text": "last"},
            {"author": "user", "text": "ignored"},
        ], "result": {"value": "Success"}}});
        assert_eq!(reply_of(&call).unwrap(), "last");
    }

    #[test]
    fn a_spent_allowance_is_a_limit_and_another_refusal_is_a_failure() {
        let spent = json!({"item": {"result": {"value": "Success", "meteringInformation": [
            {"hasAccess": false, "remainingAllowance": 0, "resetAt": "2026-10-22T00:00:00+00:00"}]}}});
        let LlmError::LimitReached(msg) = reply_of(&spent).unwrap_err() else { panic!("not a limit") };
        assert!(msg.contains("2026-10-22"), "{msg}");

        let refused = json!({"item": {"result": {"value": "Throttled", "message": "Too many"}}});
        let LlmError::Failed(msg) = reply_of(&refused).unwrap_err() else { panic!("not a failure") };
        assert!(msg.contains("Throttled") && msg.contains("Too many"), "{msg}");
        assert!(matches!(reply_of(&json!({"error": "bad"})), Err(LlmError::Failed(_))));
    }

    #[test]
    fn the_prompt_carries_the_schema_and_the_request() {
        let req = LlmRequest { cached_prefix: Some("Candidates".into()), ..request() };
        let prompt = prompt_of(&req);
        assert!(prompt.starts_with("Pick games."), "{prompt}");
        assert!(prompt.contains(r#"{"type":"object"}"#) && prompt.ends_with("Candidates\nSomething short"), "{prompt}");
    }
}
