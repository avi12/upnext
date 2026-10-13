//! The OpenAI API with the user's own key, through Chat Completions: replies
//! held to the request's schema (strict structured outputs), the newest
//! model of a family ("luna"), a reasoning effort, prompt caching, and Flex
//! for the calls nobody waits on. Every call is billed to the key.
//!
//! Written from OpenAI's documentation on 2026-10-01 without a key to try it
//! with: the replies and errors in the tests have the documented shapes.

use super::codex_cli::{drop_added_nulls, strict_schema};
use super::families::newest;
use super::{LlmBackend, LlmError, LlmRequest, LlmResponse, Pricing, Usage};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tokio::sync::OnceCell;

/// The model when none is set: the newest Luna. Settings always sets one
/// (`openai_key_model`).
const DEFAULT_MODEL: &str = "luna";
/// Tries for a call that meets a rate limit, a server error or no network.
const MAX_ATTEMPTS: u32 = 3;
/// Tries at Flex while it has no room, before the call goes at the
/// standard price.
const FLEX_ATTEMPTS: u32 = 3;

/// The GPT-6 generation on OpenAI's pricing page on 2026-10-01, newest
/// first: the models when the key's own list can't be read.
const BUILT_IN_MODELS: [&str; 7] =
    ["gpt-6.1-sol", "gpt-6-astra", "gpt-6-luna", "gpt-6-sol", "gpt-5.6-luna", "gpt-5.6-sol", "gpt-5.6-terra"];

pub struct OpenAiKeyBackend {
    base_url: String,
    key: Option<String>,
    effort: Option<String>,
    http: reqwest::Client,
    /// The first wait before trying again, doubled each time.
    backoff: Duration,
    /// The models the key can use, read once.
    models: OnceCell<Vec<String>>,
}

impl OpenAiKeyBackend {
    /// `base_url` is OpenAI's (`config::DEFAULT_OPENAI_KEY_URL`), or a mock's.
    pub fn new(base_url: String, key: Option<String>, effort: Option<String>) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(crate::http::USER_AGENT)
            // Flex answers slower; OpenAI suggests a 15-minute timeout for it.
            .timeout(Duration::from_secs(900))
            .connect_timeout(Duration::from_secs(15))
            .build()
            .expect("reqwest client");
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            key,
            effort,
            http,
            backoff: Duration::from_secs(2),
            models: OnceCell::new(),
        }
    }

    fn key(&self) -> Result<&str, LlmError> {
        self.key.as_deref().ok_or_else(|| {
            LlmError::NotAvailable(
                "no OpenAI API key: add one on the Settings page, with `upnext config set openai_key`, or set OPENAI_API_KEY".into(),
            )
        })
    }

    /// The models the key can use (`GET /models`), newest first.
    pub async fn list_models(&self) -> Result<Vec<String>, LlmError> {
        let r = self
            .http
            .get(format!("{}/models", self.base_url))
            .bearer_auth(self.key()?)
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|e| LlmError::Failed(format!("cannot reach OpenAI: {}", e.without_url())))?;
        let status = r.status().as_u16();
        let text = r.text().await.unwrap_or_default();
        if !(200..300).contains(&status) {
            return Err(failure(&ApiError::parse(status, &text), "the model list"));
        }
        let list: Value =
            serde_json::from_str(&text).map_err(|e| LlmError::BadOutput(format!("OpenAI's model list is not JSON ({e})"), None))?;
        let models = chat_models(&list);
        if models.is_empty() {
            return Err(LlmError::Failed("OpenAI listed no GPT models for this key".into()));
        }
        Ok(models)
    }

    /// The key's models, or the built-in list when they can't be read.
    async fn models(&self) -> &[String] {
        self.models
            .get_or_init(|| async {
                self.list_models().await.unwrap_or_else(|e| {
                    tracing::warn!("couldn't read the OpenAI key's models, using UpNext's list: {}", e.reason());
                    built_in_models()
                })
            })
            .await
    }

    async fn wait(&self, tries: u32, retry_after: Option<Duration>) {
        let wait = retry_after.unwrap_or(self.backoff * 2u32.pow(tries.saturating_sub(1))).min(Duration::from_secs(60));
        tokio::time::sleep(wait).await;
    }
}

pub fn built_in_models() -> Vec<String> {
    BUILT_IN_MODELS.iter().map(|m| m.to_string()).collect()
}

/// The version of a GPT model named `gpt-<version>` or
/// `gpt-<version>-<family>`. Dated snapshots ("gpt-6-luna-2026-08-01") and
/// the audio, image and embedding models don't have that shape.
fn gpt_version(id: &str) -> Option<Vec<u32>> {
    let rest = id.strip_prefix("gpt-")?;
    let (version, family) = rest.split_once('-').unwrap_or((rest, ""));
    if !family.chars().all(|c| c.is_ascii_lowercase()) {
        return None;
    }
    version.split('.').map(|p| p.parse().ok()).collect()
}

/// The GPT models in a `GET /models` reply, newest first.
fn chat_models(list: &Value) -> Vec<String> {
    let mut models: Vec<(Vec<u32>, String)> = list
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| m.get("id").and_then(Value::as_str))
        .filter_map(|id| Some((gpt_version(id)?, id.to_string())))
        .collect();
    models.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    models.dedup();
    models.into_iter().map(|(_, id)| id).collect()
}

/// The model to call for a setting: an id on the key's list, or one that
/// isn't a family, stays as it is; a family becomes its newest model there,
/// or in the built-in list when the key's has none.
pub fn resolve(models: &[String], wanted: &str) -> String {
    if models.iter().any(|m| m == wanted) {
        return wanted.to_string();
    }
    let built_in = built_in_models();
    newest(models, wanted).or_else(|| newest(&built_in, wanted)).map_or_else(|| wanted.to_string(), |m| m.clone())
}

/// The same requests share a key: the same instructions and the same cached
/// part (a pick's candidate list, new each day). OpenAI routes a key's
/// requests to the same cache, so a reroll reads the list from it. FNV-1a,
/// so the key stays the same across builds.
fn cache_key(req: &LlmRequest) -> String {
    let prefix = req.cached_prefix.as_deref().unwrap_or("");
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in req.system.bytes().chain([0]).chain(prefix.bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("upnext-{h:016x}")
}

/// The Chat Completions request.
fn body(req: &LlmRequest, model: &str, effort: Option<&str>, flex: bool) -> Value {
    let mut body = json!({
        "model": model,
        "messages": [
            { "role": "developer", "content": super::guarded_system(&req.system) },
            { "role": "user", "content": req.full_prompt() },
        ],
        "response_format": {
            "type": "json_schema",
            "json_schema": { "name": "upnext_response", "schema": strict_schema(&req.schema), "strict": true }
        },
        "prompt_cache_key": cache_key(req),
        // Not kept by OpenAI for its evals or distillation products.
        "store": false,
    });
    if let Some(e) = effort {
        body["reasoning_effort"] = e.into();
    }
    if flex {
        body["service_tier"] = "flex".into();
    }
    body
}

/// An error reply: `{"error": {"message", "type", "code", "param"}}`.
#[derive(Debug, Default)]
struct ApiError {
    status: u16,
    message: String,
    kind: String,
    code: String,
    param: String,
}

impl ApiError {
    fn parse(status: u16, body: &str) -> ApiError {
        let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
        let s = |p: &str| v.pointer(p).and_then(Value::as_str).unwrap_or("").to_string();
        let message = Some(s("/error/message")).filter(|m| !m.is_empty()).unwrap_or_else(|| body.chars().take(400).collect());
        ApiError { status, message, kind: s("/error/type"), code: s("/error/code"), param: s("/error/param") }
    }

    /// Out of credit or over a spend limit: waiting won't help.
    fn out_of_quota(&self) -> bool {
        self.kind == "insufficient_quota"
            || self.code == "insufficient_quota"
            || self.code == "credit_balance_exhausted"
            || self.code.ends_with("_spend_limit_exceeded")
            || self.code.ends_with("_usage_limit_exceeded")
    }

    /// Flex has no room for the call now. It isn't charged.
    fn flex_busy(&self) -> bool {
        self.status == 429
            && (self.code == "resource_unavailable"
                || self.kind == "resource_unavailable"
                || self.message.to_lowercase().contains("resource unavailable"))
    }
}

/// What to do after an error reply.
#[derive(Debug, PartialEq)]
enum Next {
    /// Flex has no room: wait and try it again, then the standard price.
    FlexBusy,
    /// Flex isn't allowed for this project or model: the standard price.
    NoFlex,
    /// A rate limit or a server error: wait and try again.
    Retry,
    Stop,
}

fn next(e: &ApiError, flex: bool) -> Next {
    if flex && e.flex_busy() {
        Next::FlexBusy
    } else if flex && e.status == 400 && e.param == "service_tier" {
        Next::NoFlex
    } else if (e.status == 429 && !e.out_of_quota()) || e.status >= 500 {
        Next::Retry
    } else {
        Next::Stop
    }
}

/// The error a call ends with. A rejected key, a model the key can't use and
/// a setting the model refuses fail every call the same way, so they're
/// `NotAvailable` and a batch stops at once; so is a reply format OpenAI
/// refuses, which is a bug to fix rather than a reason to try each batch.
fn failure(e: &ApiError, model: &str) -> LlmError {
    let msg = &e.message;
    match e.status {
        401 => LlmError::NotAvailable(format!("OpenAI rejected the API key (401): {msg}")),
        403 => LlmError::NotAvailable(format!("OpenAI refused the call (403): {msg}")),
        404 => LlmError::NotAvailable(format!("the OpenAI key can't use {model} (404): {msg}")),
        429 if e.code.ends_with("_limit_exceeded") && e.out_of_quota() => {
            LlmError::LimitReached(format!("the OpenAI key reached its spend limit: {msg}"))
        }
        429 if e.out_of_quota() => LlmError::LimitReached(format!(
            "the OpenAI key's credit has run out: {msg} (add credit at https://platform.openai.com/settings/organization/billing)"
        )),
        429 => LlmError::LimitReached(format!("OpenAI is still rate-limiting after {MAX_ATTEMPTS} tries: {msg}")),
        400 if e.code == "model_not_found" || e.param == "model" => {
            LlmError::NotAvailable(format!("the OpenAI key can't use {model}: {msg}"))
        }
        400 if e.param == "reasoning_effort" => LlmError::NotAvailable(format!(
            "{model} doesn't take this reasoning effort: {msg} (change Reasoning on the Settings page, Picks)"
        )),
        400 if e.param.starts_with("response_format") => {
            LlmError::NotAvailable(format!("OpenAI refused UpNext's reply format for {model}: {msg}"))
        }
        status => LlmError::Failed(format!("OpenAI returned {status}: {msg}")),
    }
}

/// A completion held to `schema`: its JSON, without the nulls the strict
/// schema added; the model asked for (the reply may name a dated snapshot,
/// which would split AI usage's rows); its tokens; whether it ran at Flex.
fn reply(resp: &Value, model: &str, schema: &Value) -> Result<LlmResponse, LlmError> {
    let usage = resp.get("usage").and_then(Usage::from_openai);
    let message = resp.pointer("/choices/0/message").ok_or_else(|| LlmError::BadOutput("the reply has no choices".into(), usage))?;
    if let Some(refusal) = message.get("refusal").and_then(Value::as_str).filter(|r| !r.is_empty()) {
        return Err(LlmError::Refused(format!("the model declined the request: {refusal}"), usage));
    }
    match resp.pointer("/choices/0/finish_reason").and_then(Value::as_str) {
        Some("length") => return Err(LlmError::BadOutput("the model ran out of output tokens before finishing".into(), usage)),
        Some("content_filter") => return Err(LlmError::Refused("OpenAI's content filter stopped the reply".into(), usage)),
        _ => {}
    }
    let content = message.get("content").and_then(Value::as_str).unwrap_or_default();
    let mut json: Value = serde_json::from_str(content.trim()).map_err(|e| {
        LlmError::BadOutput(format!("the reply is not JSON ({e}): {}", content.chars().take(400).collect::<String>()), usage)
    })?;
    drop_added_nulls(&mut json, schema);
    Ok(LlmResponse {
        json,
        model: Some(model.to_string()),
        usage,
        cost_usd: None,
        api_usd: None,
        would_cost_usd: None,
        duration_ms: 0,
        flex: resp.get("service_tier").and_then(Value::as_str) == Some("flex"),
    })
}

/// How long OpenAI asks to wait before trying again, at most a minute (as
/// `wait` waits): a larger value, whoever sent it, would otherwise be too
/// large for a `Duration` and panic the call.
fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<f64>().ok());
    header("retry-after-ms")
        .map(|ms| ms / 1000.0)
        .or_else(|| header("retry-after"))
        .filter(|s| s.is_finite() && *s >= 0.0)
        .map(|s| Duration::from_secs_f64(s.min(60.0)))
}

#[async_trait]
impl LlmBackend for OpenAiKeyBackend {
    fn name(&self) -> &str {
        "openai-key"
    }

    fn pricing(&self) -> Option<Pricing> {
        Some(Pricing { provider: crate::prices::Provider::OpenAi, charged: true })
    }

    async fn available(&self) -> Result<(), LlmError> {
        self.key().map(|_| ())
    }

    fn model_sent(&self, requested: Option<&str>) -> Option<String> {
        // The key's list when a call has read it; never read here, since a
        // call that failed may have failed to reach OpenAI at all.
        let wanted = requested.filter(|m| !m.is_empty()).unwrap_or(DEFAULT_MODEL);
        Some(resolve(self.models.get().map(Vec::as_slice).unwrap_or_default(), wanted))
    }

    async fn complete_json(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        let key = self.key()?;
        let wanted = req.model.clone().filter(|m| !m.is_empty()).unwrap_or_else(|| DEFAULT_MODEL.to_string());
        let model = resolve(self.models().await, &wanted);
        let started = Instant::now();
        let (mut flex, mut tries, mut flex_tries) = (req.background, 0, 0);
        let resp: Value = loop {
            let sent = self
                .http
                .post(format!("{}/chat/completions", self.base_url))
                .bearer_auth(key)
                .json(&body(&req, &model, self.effort.as_deref(), flex))
                .send()
                .await;
            let r = match sent {
                Ok(r) => r,
                Err(e) => {
                    tries += 1;
                    // Only a request that never reached OpenAI is sent again: one
                    // that timed out or lost its connection may have been
                    // answered, and charged, already.
                    if e.is_connect() && tries < MAX_ATTEMPTS {
                        tracing::warn!("OpenAI request failed (try {tries}): {e}");
                        self.wait(tries, None).await;
                        continue;
                    }
                    return Err(LlmError::Failed(format!("request to OpenAI failed: {}", e.without_url())));
                }
            };
            let status = r.status().as_u16();
            let wait = retry_after(r.headers());
            let text = r.text().await.map_err(|e| LlmError::Failed(format!("reading OpenAI's reply: {e}")))?;
            if (200..300).contains(&status) {
                break serde_json::from_str(&text).map_err(|e| LlmError::BadOutput(format!("OpenAI's reply is not JSON ({e})"), None))?;
            }
            let e = ApiError::parse(status, &text);
            match next(&e, flex) {
                Next::FlexBusy => {
                    flex_tries += 1;
                    if flex_tries < FLEX_ATTEMPTS {
                        self.wait(flex_tries, None).await;
                    } else {
                        tracing::info!("Flex still has no room after {flex_tries} tries; sending at the standard price");
                        flex = false;
                    }
                }
                Next::NoFlex => {
                    tracing::info!("OpenAI won't take Flex here ({}); sending at the standard price", e.message);
                    flex = false;
                }
                Next::Retry if tries + 1 < MAX_ATTEMPTS => {
                    tries += 1;
                    tracing::warn!("OpenAI returned {status} (try {tries}): {}", e.message);
                    self.wait(tries, wait).await;
                }
                Next::Retry | Next::Stop => return Err(failure(&e, &model)),
            }
        };
        let mut out = reply(&resp, &model, &req.schema)?;
        out.duration_ms = started.elapsed().as_millis() as u64;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    fn request(background: bool) -> LlmRequest {
        LlmRequest {
            system: "Pick games.".into(),
            cached_prefix: Some("#1 Celeste\n#2 Hades".into()),
            prompt: "Something short.".into(),
            schema: json!({"type": "object", "properties": {
                "picks": {"type": "array", "minItems": 1, "items": {"type": "object",
                    "properties": {"id": {"type": "integer"}, "note": {"type": "string"}}, "required": ["id"]}}},
                "required": ["picks"]}),
            model: Some("luna".into()),
            background,
            effort: None,
        }
    }

    fn backend(server: &MockServer) -> OpenAiKeyBackend {
        let mut b = OpenAiKeyBackend::new(format!("{}/v1", server.uri()), Some("sk-test-0123456789abcdef".into()), Some("low".into()));
        b.backoff = Duration::from_millis(1);
        b
    }

    async fn serve_models(server: &MockServer) {
        let list = crate::testutil::fixture_json("llm/openai_models.json");
        Mock::given(method("GET")).and(path("/v1/models")).respond_with(ResponseTemplate::new(200).set_body_json(list)).mount(server).await;
    }

    fn error(status: u16, kind: &str, code: &str, param: &str, message: &str) -> ResponseTemplate {
        ResponseTemplate::new(status).set_body_json(json!({"error": {"message": message, "type": kind, "code": code, "param": param}}))
    }

    fn success() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(crate::testutil::fixture_json("llm/openai_key_success.json"))
    }

    async fn sent(server: &MockServer) -> Vec<Value> {
        let requests: Vec<Request> = server.received_requests().await.unwrap();
        requests.iter().filter(|r| r.url.path().ends_with("/chat/completions")).map(|r| serde_json::from_slice(&r.body).unwrap()).collect()
    }

    /// The request: the newest Luna for "luna", the strict schema, the
    /// effort, the instructions as the developer's, no storing, and a cache
    /// key that's the same for a reroll and different for another list.
    #[test]
    fn the_request_is_strict_and_cacheable() {
        let req = request(false);
        let b = body(&req, "gpt-6-luna", Some("low"), false);
        assert_eq!((&b["model"], &b["reasoning_effort"], &b["store"]), (&json!("gpt-6-luna"), &json!("low"), &json!(false)));
        assert_eq!(b["messages"][0]["role"], "developer");
        assert!(b["messages"][0]["content"].as_str().unwrap().starts_with("Pick games."));
        assert_eq!(b["messages"][1]["content"], "#1 Celeste\n#2 Hades\nSomething short.", "the cached part first");
        let schema = &b["response_format"]["json_schema"];
        assert_eq!(schema["strict"], true);
        assert_eq!(schema["schema"], strict_schema(&req.schema));
        assert!(schema["schema"]["properties"]["picks"].get("minItems").is_none());
        assert!(b.get("service_tier").is_none(), "a pick goes at the standard price");
        assert_eq!(body(&request(true), "gpt-6-luna", None, true)["service_tier"], "flex");
        assert!(body(&req, "gpt-6-luna", None, false).get("reasoning_effort").is_none(), "OpenAI's default");

        let reroll = LlmRequest { prompt: "Something short, again.".into(), ..request(false) };
        assert_eq!(cache_key(&req), cache_key(&reroll), "a reroll shares the key");
        assert!(cache_key(&req).starts_with("upnext-") && cache_key(&req).len() == 23);
        let tomorrow = LlmRequest { cached_prefix: Some("#3 Portal 2".into()), ..request(false) };
        assert_ne!(cache_key(&req), cache_key(&tomorrow));
        assert_eq!(cache_key(&req), "upnext-8154afa430d14250", "the same in every build");
    }

    /// The key's list, from the documented shape: GPT models only, newest
    /// first, without dated snapshots; "luna" is the newest Luna on it.
    #[test]
    fn models_come_from_the_keys_list() {
        let models = chat_models(&crate::testutil::fixture_json("llm/openai_models.json"));
        assert_eq!(models, ["gpt-6.1-sol", "gpt-6-astra", "gpt-6-luna", "gpt-6-sol", "gpt-5.6-luna", "gpt-5.5", "gpt-5.4-mini"]);
        assert_eq!(resolve(&models, "luna"), "gpt-6-luna");
        assert_eq!(resolve(&models, "sol"), "gpt-6.1-sol");
        assert_eq!(resolve(&models, "gpt-5.6-luna"), "gpt-5.6-luna", "an exact model stays");
        assert_eq!(resolve(&[], "luna"), "gpt-6-luna", "the built-in list");
        assert_eq!(resolve(&models, "my-fine-tune"), "my-fine-tune");
        assert_eq!(gpt_version("gpt-6-luna-2026-08-01"), None);
        assert_eq!(gpt_version("gpt-5.5"), Some(vec![5, 5]));
    }

    /// A reply from the documented shape: the JSON without the added nulls,
    /// the tokens with the cached ones, and the tier it ran at.
    #[test]
    fn a_reply_reports_its_json_tokens_and_tier() {
        let resp = crate::testutil::fixture_json("llm/openai_key_success.json");
        let out = reply(&resp, "gpt-6-luna", &request(true).schema).unwrap();
        assert_eq!(out.json, json!({"picks": [{"id": 1}, {"id": 2, "note": "Short."}]}));
        assert_eq!(out.model.as_deref(), Some("gpt-6-luna"), "not the dated snapshot the reply names");
        assert_eq!(
            out.usage,
            Some(Usage {
                input_tokens: 2_100,
                output_tokens: 160,
                cache_read_tokens: 1_792,
                cache_write_tokens: 0,
                cache_write_1h_tokens: 0
            })
        );
        assert!(out.flex);

        let mut refused = resp.clone();
        refused["choices"][0]["message"] = json!({"role": "assistant", "content": null, "refusal": "I can't help with that."});
        assert!(matches!(reply(&refused, "gpt-6-luna", &json!({})), Err(LlmError::Refused(m, Some(_))) if m.contains("can't help")));
        let mut cut = resp.clone();
        cut["choices"][0]["finish_reason"] = "length".into();
        assert_eq!(reply(&cut, "gpt-6-luna", &json!({})).unwrap_err().usage().map(|u| u.output_tokens), Some(160));
        let mut standard = resp.clone();
        standard["service_tier"] = "default".into();
        assert!(!reply(&standard, "gpt-6-luna", &json!({})).unwrap().flex);
    }

    /// Errors with OpenAI's documented codes: which stop a batch at once,
    /// which mean the credit is gone, and which are worth another try.
    #[test]
    fn errors_say_what_to_do() {
        let e = |status: u16, kind: &str, code: &str, param: &str| ApiError {
            status,
            message: "msg".into(),
            kind: kind.into(),
            code: code.into(),
            param: param.into(),
        };
        // A real reply, to a made-up key.
        let real = ApiError::parse(401, &crate::testutil::fixture("llm/openai_key_bad_key.json"));
        assert_eq!((real.kind.as_str(), real.code.as_str(), real.param.as_str()), ("invalid_request_error", "invalid_api_key", ""));
        let bad_key = failure(&real, "gpt-6-luna");
        assert!(
            matches!(&bad_key, LlmError::NotAvailable(m) if m.starts_with("OpenAI rejected the API key (401): Incorrect API key provided"))
        );
        let no_model = failure(&e(404, "invalid_request_error", "model_not_found", "model"), "gpt-7-luna");
        assert!(matches!(&no_model, LlmError::NotAvailable(m) if m.contains("can't use gpt-7-luna")));
        let credit = e(429, "insufficient_quota", "insufficient_quota", "");
        assert_eq!(next(&credit, false), Next::Stop, "waiting won't bring credit back");
        assert!(matches!(failure(&credit, "m"), LlmError::LimitReached(m) if m.contains("credit has run out")));
        let spend = e(429, "insufficient_quota", "project_spend_limit_exceeded", "");
        assert!(matches!(failure(&spend, "m"), LlmError::LimitReached(m) if m.contains("spend limit")));
        let effort = failure(&e(400, "invalid_request_error", "unsupported_value", "reasoning_effort"), "gpt-6-astra");
        assert!(matches!(&effort, LlmError::NotAvailable(m) if m.contains("change Reasoning")));
        let format = failure(&e(400, "invalid_request_error", "invalid_schema", "response_format.json_schema"), "m");
        assert!(matches!(format, LlmError::NotAvailable(_)), "a schema OpenAI refuses is a bug, not a reason to try every batch");
        assert!(matches!(failure(&e(400, "invalid_request_error", "", "messages"), "m"), LlmError::Failed(_)));

        let rate = e(429, "rate_limit_error", "rate_limit_exceeded", "");
        assert_eq!(
            (next(&rate, false), next(&e(503, "service_unavailable_error", "server_is_overloaded", ""), false)),
            (Next::Retry, Next::Retry)
        );
        assert!(matches!(failure(&rate, "m"), LlmError::LimitReached(m) if m.contains("still rate-limiting")));
        let busy = ApiError { message: "Resource Unavailable".into(), ..e(429, "", "", "") };
        assert_eq!((next(&busy, true), next(&busy, false)), (Next::FlexBusy, Next::Retry));
        assert_eq!(next(&e(400, "invalid_request_error", "", "service_tier"), true), Next::NoFlex);
        let gave_up = crate::llm::gave_up(&anyhow::Error::new(failure(&credit, "m")).context("digest generation"));
        assert!(gave_up.is_some(), "a batch stops when the credit runs out");
    }

    /// A summaries call goes at Flex. While Flex has no room it's tried
    /// again, then sent at the standard price.
    #[tokio::test]
    async fn flex_without_room_falls_back_to_the_standard_price() {
        let server = MockServer::start().await;
        serve_models(&server).await;
        let busy = error(429, "", "resource_unavailable", "", "Resource Unavailable");
        Mock::given(path("/v1/chat/completions"))
            .and(body_partial_json(json!({"service_tier": "flex"})))
            .respond_with(busy)
            .mount(&server)
            .await;
        Mock::given(path("/v1/chat/completions")).respond_with(success()).mount(&server).await;
        let out = backend(&server).complete_json(request(true)).await.unwrap();
        assert_eq!(out.model.as_deref(), Some("gpt-6-luna"));
        let sent = sent(&server).await;
        let tiers: Vec<&Value> = sent.iter().map(|b| &b["service_tier"]).collect();
        assert_eq!(tiers, [&json!("flex"), &json!("flex"), &json!("flex"), &Value::Null]);
        assert!(sent.iter().all(|b| b["model"] == "gpt-6-luna" && b["reasoning_effort"] == "low"));
    }

    /// A project without Flex sends at once at the standard price; a rate
    /// limit is tried again; a key without credit stops after one call.
    #[tokio::test]
    async fn retries_only_what_can_change() {
        let server = MockServer::start().await;
        serve_models(&server).await;
        let refused =
            error(400, "invalid_request_error", "", "service_tier", "The requested service tier is not allowed for this project.");
        Mock::given(path("/v1/chat/completions"))
            .and(body_partial_json(json!({"service_tier": "flex"})))
            .respond_with(refused)
            .mount(&server)
            .await;
        Mock::given(path("/v1/chat/completions"))
            .respond_with(error(429, "rate_limit_error", "rate_limit_exceeded", "", "Slow down."))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(path("/v1/chat/completions")).respond_with(success()).mount(&server).await;
        backend(&server).complete_json(request(true)).await.unwrap();
        let tiers: Vec<Value> = sent(&server).await.iter().map(|b| b["service_tier"].clone()).collect();
        assert_eq!(tiers, [json!("flex"), Value::Null, Value::Null]);

        let server = MockServer::start().await;
        serve_models(&server).await;
        let credit = error(429, "insufficient_quota", "insufficient_quota", "", "You exceeded your current quota.");
        Mock::given(path("/v1/chat/completions")).respond_with(credit).mount(&server).await;
        let err = backend(&server).complete_json(request(false)).await.unwrap_err();
        assert!(matches!(&err, LlmError::LimitReached(m) if m.contains("credit has run out")), "{err:?}");
        assert_eq!(sent(&server).await.len(), 1);
    }

    /// Without a key nothing is sent; with one OpenAI rejects, the model
    /// list's 401 says so.
    #[tokio::test]
    async fn a_missing_or_rejected_key_says_so() {
        let server = MockServer::start().await;
        let none = OpenAiKeyBackend::new(server.uri(), None, None);
        assert!(matches!(none.available().await, Err(LlmError::NotAvailable(m)) if m.contains("no OpenAI API key")));
        assert!(none.complete_json(request(false)).await.is_err());
        assert!(server.received_requests().await.unwrap().is_empty());

        let rejected = error(401, "invalid_request_error", "invalid_api_key", "", "Incorrect API key provided: sk-test-****cdef.");
        Mock::given(method("GET")).and(path("/v1/models")).respond_with(rejected).mount(&server).await;
        let err = backend(&server).list_models().await.unwrap_err();
        assert!(matches!(&err, LlmError::NotAvailable(m) if m.contains("Incorrect API key")), "{err:?}");
    }

    /// A call cut off or refused is charged, and recorded under the model
    /// "luna" was sent as, so it is priced (B-AI-4).
    #[tokio::test]
    async fn a_failed_call_that_was_charged_is_priced() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::Db::open(&dir.path().join("upnext.db"), 120).unwrap();
        let server = MockServer::start().await;
        serve_models(&server).await;
        // OpenAI's prices, with gpt-6-luna's, as UpNext reads them from its page.
        let page = crate::testutil::fixture("prices/openai_pricing.md");
        Mock::given(path("/prices/openai")).respond_with(ResponseTemplate::new(200).set_body_string(page)).mount(&server).await;
        crate::prices::refresh(&db, &reqwest::Client::new(), crate::prices::Provider::OpenAi, Some(&server.uri())).await.unwrap();
        let mut cut = crate::testutil::fixture_json("llm/openai_key_success.json");
        cut["choices"][0]["finish_reason"] = "length".into();
        Mock::given(path("/v1/chat/completions")).respond_with(ResponseTemplate::new(200).set_body_json(cut)).mount(&server).await;
        let backend = backend(&server);
        let purpose = crate::ai_usage::Purpose::new(crate::db::AiKind::Pick);
        assert!(crate::ai_usage::call(&db, &backend, &purpose, request(false)).await.is_err());
        let rows = db.ai_usage_rows(0).unwrap();
        assert_eq!(rows[0].model.as_deref(), Some("gpt-6-luna"), "{rows:?}");
        assert!(rows[0].totals.cost_usd.is_some_and(|c| c > 0.0), "{:?}", rows[0].totals);
        assert_eq!(backend.model_sent(None).as_deref(), Some("gpt-6-luna"), "no model is the default family");
    }

    /// A `retry-after` too large for a `Duration` is a minute, not a panic
    /// that ends the call, or the job it runs in (B-AI-1).
    #[test]
    fn a_huge_retry_after_is_a_minute() {
        let headers = |name: &'static str, value: &str| {
            let mut h = reqwest::header::HeaderMap::new();
            h.insert(name, value.parse().unwrap());
            h
        };
        assert_eq!(retry_after(&headers("retry-after", "1e30")), Some(Duration::from_secs(60)));
        assert_eq!(retry_after(&headers("retry-after-ms", "1e40")), Some(Duration::from_secs(60)));
        assert_eq!(retry_after(&headers("retry-after", "2.5")), Some(Duration::from_millis(2500)));
        assert_eq!(retry_after(&headers("retry-after", "inf")), None);
        assert_eq!(retry_after(&headers("retry-after", "-1")), None);
    }

    /// A request that was sent but timed out may have been answered and
    /// charged, so it isn't sent again (B-AI-6).
    #[tokio::test]
    async fn a_request_that_timed_out_is_not_sent_again() {
        let server = MockServer::start().await;
        serve_models(&server).await;
        Mock::given(path("/v1/chat/completions")).respond_with(success().set_delay(Duration::from_secs(5))).mount(&server).await;
        let mut b = backend(&server);
        b.http = reqwest::Client::builder().timeout(Duration::from_millis(300)).build().unwrap();
        let err = b.complete_json(request(false)).await.unwrap_err();
        assert!(matches!(&err, LlmError::Failed(m) if m.contains("request to OpenAI failed")), "{err:?}");
        assert_eq!(sent(&server).await.len(), 1, "a request that may have been charged was sent again");
    }
}
