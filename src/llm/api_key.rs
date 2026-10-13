//! Direct Anthropic Messages API backend (ANTHROPIC_API_KEY or the key set in
//! the app). Uses structured outputs (`output_config.format`) so the reply is
//! schema-constrained JSON; there is no official Rust SDK, so this is raw HTTP.

use super::{LlmBackend, LlmError, LlmRequest, LlmResponse, Pricing, Usage};
use async_trait::async_trait;
use serde_json::{Map, Value, json};
use std::time::{Duration, Instant};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
/// A request that names no model. The settings name one per feature
/// (`config::API_KEY_PICK_MODEL`, `API_KEY_SUMMARY_MODEL`), so this is
/// only a fallback, the same as the picks' default.
const DEFAULT_MODEL: &str = "claude-sonnet-5-5";
const MAX_TOKENS: u32 = 16_000;
const MAX_ATTEMPTS: u32 = 3;

pub struct ApiKeyBackend {
    key: Option<String>,
    base_url: String,
    http: reqwest::Client,
}

impl ApiKeyBackend {
    pub fn new(key: Option<String>) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(crate::http::USER_AGENT)
            .timeout(Duration::from_secs(600))
            .connect_timeout(Duration::from_secs(15))
            .build()
            .expect("reqwest client");
        // UPNEXT_ANTHROPIC_URL exists for testing against a local mock.
        let base_url = std::env::var("UPNEXT_ANTHROPIC_URL").unwrap_or_else(|_| API_URL.to_string());
        Self { key, base_url, http }
    }
}

/// Map the CLI-style aliases used in settings ("opus", "sonnet", "haiku")
/// to API model ids, the same models the Claude CLI's aliases mean; full
/// ids pass through. No model, or "default", is `DEFAULT_MODEL`.
pub fn resolve_model(m: Option<&str>) -> String {
    match m {
        None | Some("default") | Some("cli") => DEFAULT_MODEL.to_string(),
        Some("sonnet") => "claude-sonnet-5-5".to_string(),
        Some("opus") => "claude-opus-5-5".to_string(),
        Some("haiku") => "claude-haiku-5-5".to_string(),
        Some(id) => id.to_string(),
    }
}

/// Structured outputs reject numeric, string-length and array-size
/// constraints, and require `additionalProperties: false` on every object.
/// Callers validate those constraints themselves (see `recommend.rs`).
pub(super) fn api_schema(v: &Value) -> Value {
    const UNSUPPORTED: [&str; 9] =
        ["minItems", "maxItems", "minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum", "multipleOf", "minLength", "maxLength"];
    match v {
        Value::Object(m) => {
            let mut out: Map<String, Value> =
                m.iter().filter(|(k, _)| !UNSUPPORTED.contains(&k.as_str())).map(|(k, v)| (k.clone(), api_schema(v))).collect();
            if out.get("type").and_then(Value::as_str) == Some("object") {
                out.insert("additionalProperties".into(), Value::Bool(false));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(api_schema).collect()),
        other => other.clone(),
    }
}

/// The user turn. A cached prefix goes first with a cache breakpoint, so a
/// reroll within five minutes reads it at a tenth of the price.
fn user_content(req: &LlmRequest) -> Value {
    match &req.cached_prefix {
        Some(prefix) => json!([
            { "type": "text", "text": prefix, "cache_control": { "type": "ephemeral" } },
            { "type": "text", "text": req.prompt },
        ]),
        None => Value::String(req.prompt.clone()),
    }
}

fn api_error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.pointer("/error/message").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| body.chars().take(400).collect())
}

#[async_trait]
impl LlmBackend for ApiKeyBackend {
    fn name(&self) -> &str {
        "api-key"
    }

    fn pricing(&self) -> Option<Pricing> {
        Some(Pricing { provider: crate::prices::Provider::Anthropic, charged: true })
    }

    fn model_sent(&self, requested: Option<&str>) -> Option<String> {
        Some(resolve_model(requested))
    }

    async fn available(&self) -> Result<(), LlmError> {
        match &self.key {
            Some(_) => Ok(()),
            None => Err(LlmError::NotAvailable(
                "no Anthropic API key: add one on the Settings page, with `upnext config set anthropic_api_key`, or set ANTHROPIC_API_KEY"
                    .into(),
            )),
        }
    }

    async fn complete_json(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        self.available().await?;
        let key = self.key.as_deref().unwrap_or_default();
        let model = resolve_model(req.model.as_deref());
        let mut body = json!({
            "model": model,
            "max_tokens": MAX_TOKENS,
            "system": super::guarded_system(&req.system),
            "messages": [{ "role": "user", "content": user_content(&req) }],
            "output_config": { "format": { "type": "json_schema", "schema": api_schema(&req.schema) } },
        });
        if let Some(effort) = req.effort.filter(|_| takes_effort(&model)) {
            body["output_config"]["effort"] = json!(effort);
        }
        // Server-side refusal fallbacks, routed by refusal category.
        let fallbacks = ["claude-opus-5-5", "claude-opus-5", "claude-sonnet-5-5", "claude-fable-5-1"].contains(&model.as_str());
        if fallbacks {
            body["fallbacks"] = json!("default");
        }

        let started = Instant::now();
        let mut attempt = 0;
        let resp: Value = loop {
            attempt += 1;
            let mut rb = self.http.post(&self.base_url).header("x-api-key", key).header("anthropic-version", "2023-06-01").json(&body);
            if fallbacks {
                rb = rb.header("anthropic-beta", "server-side-fallback-2026-07-01");
            }
            let r = match rb.send().await {
                Ok(r) => r,
                // Only a request that never reached the API is sent again. One
                // that timed out or lost its connection may have been answered,
                // and charged, already; sending it again could charge it twice,
                // and only the call that came back would be recorded.
                Err(e) if e.is_connect() && attempt < MAX_ATTEMPTS => {
                    tracing::warn!("Anthropic API transport error (attempt {attempt}): {e}");
                    tokio::time::sleep(Duration::from_secs(2u64.pow(attempt))).await;
                    continue;
                }
                Err(e) => return Err(LlmError::Failed(format!("request to the Anthropic API failed: {e}"))),
            };
            let status = r.status();
            let retry_after = r.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
            let text = r.text().await.map_err(|e| LlmError::Failed(format!("reading the API response: {e}")))?;
            if status.is_success() {
                break serde_json::from_str(&text).map_err(|e| LlmError::BadOutput(format!("API response is not JSON ({e})"), None))?;
            }
            let retryable = status.as_u16() == 429 || status.as_u16() == 529 || status.is_server_error();
            if retryable && attempt < MAX_ATTEMPTS {
                let wait = retry_after.unwrap_or(2u64.pow(attempt)).min(60);
                tracing::warn!("Anthropic API returned {status} (attempt {attempt}), retrying in {wait}s");
                tokio::time::sleep(Duration::from_secs(wait)).await;
                continue;
            }
            let msg = api_error_message(&text);
            return Err(match status.as_u16() {
                401 | 403 => LlmError::NotAvailable(format!("the Anthropic API rejected the key ({status}): {msg}")),
                429 => LlmError::LimitReached(format!("the Anthropic API is still rate-limiting after {attempt} tries: {msg}")),
                _ if msg.to_lowercase().contains("credit balance") => {
                    LlmError::LimitReached(format!("the Anthropic API account is out of credit: {msg}"))
                }
                _ => LlmError::Failed(format!("Anthropic API returned {status}: {msg}")),
            });
        };
        let mut out = reply(&resp, model)?;
        out.duration_ms = started.elapsed().as_millis() as u64;
        Ok(out)
    }
}

/// Whether a request's `effort` goes to `model`: Haiku 5.5, which thinks by
/// default and ran out of `max_tokens` on summary batches at its default
/// effort. Sonnet 5.5 didn't think on those calls, and older models
/// refuse the field, so it isn't sent to them.
fn takes_effort(model: &str) -> bool {
    model.starts_with("claude-haiku-5-5")
}

/// A Messages API reply: the JSON the model wrote, its model and its tokens
/// (`duration_ms` is left for the caller; `ai_usage::call` prices it).
fn reply(resp: &Value, requested: String) -> Result<LlmResponse, LlmError> {
    let usage = resp.get("usage").and_then(Usage::from_anthropic);
    match resp.get("stop_reason").and_then(Value::as_str) {
        Some("refusal") => {
            let cat = resp.pointer("/stop_details/category").and_then(Value::as_str).unwrap_or("unspecified");
            return Err(LlmError::Refused(format!("the model declined the request (refusal, category {cat})"), usage));
        }
        Some("max_tokens") => {
            return Err(LlmError::BadOutput(format!("output hit max_tokens ({MAX_TOKENS}) before finishing"), usage));
        }
        _ => {}
    }
    let text: String = resp
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect()
        })
        .unwrap_or_default();
    let json: Value = serde_json::from_str(text.trim())
        .map_err(|e| LlmError::BadOutput(format!("reply is not JSON ({e}): {}", text.chars().take(400).collect::<String>()), usage))?;

    let used_model = resp.get("model").and_then(Value::as_str).map(str::to_string).unwrap_or(requested);
    Ok(LlmResponse {
        json,
        model: Some(used_model),
        usage,
        cost_usd: None,
        api_usd: None,
        would_cost_usd: None,
        duration_ms: 0,
        flex: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a review-summary batch sends: the schema's properties in the
    /// order they are written, vibes before the tags (keys out of order got
    /// "placeholder" summaries), and low effort on Haiku 5.5 only.
    #[tokio::test]
    async fn a_summary_request_keeps_its_key_order_and_effort() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        let reply = crate::testutil::fixture_json("llm/anthropic_api_success.json");
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(200).set_body_json(reply)).mount(&server).await;
        let mut backend = ApiKeyBackend::new(Some("sk-ant-test".into()));
        backend.base_url = server.uri();
        let request = |model: &str| LlmRequest {
            system: "s".into(),
            cached_prefix: None,
            prompt: "p".into(),
            schema: crate::digest::schema(),
            model: Some(model.into()),
            background: true,
            effort: Some("low"),
        };
        for model in ["haiku", "sonnet"] {
            let _ = backend.complete_json(request(model)).await;
        }
        let sent: Vec<Value> = server.received_requests().await.unwrap().iter().map(|r| serde_json::from_slice(&r.body).unwrap()).collect();
        let item = &sent[0]["output_config"]["format"]["schema"]["properties"]["digests"]["items"]["properties"];
        let keys: Vec<&str> = item.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, ["id", "vibes", "digest", "facets", "praised", "criticized", "divisive", "notes"]);
        assert_eq!(sent[0]["model"], "claude-haiku-5-5");
        assert_eq!(sent[0]["output_config"]["effort"], "low");
        assert_eq!(sent[1]["model"], "claude-sonnet-5-5");
        assert!(sent[1]["output_config"].get("effort").is_none(), "Sonnet 5.5 is left at its default");
    }

    #[test]
    fn schema_drops_unsupported_constraints() {
        let s = json!({
            "type": "object",
            "properties": { "picks": { "type": "array", "minItems": 3, "maxItems": 3,
                "items": { "type": "object", "properties": { "n": { "type": "integer", "minimum": 1 } } } } },
        });
        let out = api_schema(&s);
        assert_eq!(out["additionalProperties"], json!(false));
        assert!(out["properties"]["picks"].get("minItems").is_none());
        assert_eq!(out["properties"]["picks"]["items"]["additionalProperties"], json!(false));
        assert!(out["properties"]["picks"]["items"]["properties"]["n"].get("minimum").is_none());
    }

    #[test]
    fn aliases_resolve() {
        assert_eq!(resolve_model(None), "claude-sonnet-5-5");
        assert_eq!(resolve_model(Some("default")), "claude-sonnet-5-5");
        assert_eq!(resolve_model(Some("opus")), "claude-opus-5-5");
        assert_eq!(resolve_model(Some("sonnet")), "claude-sonnet-5-5");
        assert_eq!(resolve_model(Some("haiku")), "claude-haiku-5-5", "the summaries' default");
        assert_eq!(resolve_model(Some("claude-sonnet-5")), "claude-sonnet-5");
    }

    /// Written from the documented shape (there's no key to capture one):
    /// a reply that wrote part of its input to the cache and read the rest.
    #[test]
    fn a_reply_reports_its_tokens() {
        let resp = crate::testutil::fixture_json("llm/anthropic_api_success.json");
        let out = reply(&resp, "claude-sonnet-5-5".into()).unwrap();
        assert_eq!(out.json, json!({"ok": true}));
        assert_eq!(out.model.as_deref(), Some("claude-sonnet-5-5"));
        let usage = out.usage.unwrap();
        assert_eq!(
            usage,
            Usage {
                input_tokens: 40 + 12_000 + 20_000,
                output_tokens: 300,
                cache_read_tokens: 20_000,
                cache_write_tokens: 12_000,
                cache_write_1h_tokens: 0
            }
        );
        assert_eq!((out.cost_usd, out.would_cost_usd), (None, None), "priced by ai_usage::call, from the price list");

        let mut hour = resp.clone();
        hour["usage"]["cache_creation"] = json!({"ephemeral_5m_input_tokens": 0, "ephemeral_1h_input_tokens": 12_000});
        assert_eq!(reply(&hour, "claude-sonnet-5-5".into()).unwrap().usage.unwrap().cache_write_1h_tokens, 12_000);

        let mut cut = resp.clone();
        cut["stop_reason"] = "max_tokens".into();
        let err = reply(&cut, "claude-sonnet-5-5".into()).unwrap_err();
        assert_eq!(err.usage(), Some(usage), "a reply cut off still used its tokens");
    }

    /// A refusal is charged for the tokens it used, and says so (B-AI-4).
    #[test]
    fn a_refusal_keeps_its_tokens() {
        let mut resp = crate::testutil::fixture_json("llm/anthropic_api_success.json");
        resp["stop_reason"] = "refusal".into();
        let err = reply(&resp, "claude-sonnet-5-5".into()).unwrap_err();
        assert!(matches!(&err, LlmError::Refused(m, _) if m.contains("declined")), "{err:?}");
        assert_eq!(err.usage().map(|u| u.output_tokens), Some(300), "the refusal's tokens are lost: {err:?}");
    }

    /// A reply cut off at `max_tokens`, or a refusal, is charged: it is
    /// recorded under the model the alias was sent as ("sonnet" is
    /// claude-sonnet-5-5), with its tokens and its cost, not as a "sonnet"
    /// that has no price (B-AI-4).
    #[tokio::test]
    async fn a_failed_call_that_was_charged_is_priced() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::Db::open(&dir.path().join("upnext.db"), 120).unwrap();
        for stop in ["max_tokens", "refusal"] {
            let server = MockServer::start().await;
            let mut resp = crate::testutil::fixture_json("llm/anthropic_api_success.json");
            resp["stop_reason"] = stop.into();
            Mock::given(method("POST")).respond_with(ResponseTemplate::new(200).set_body_json(resp)).mount(&server).await;
            let mut backend = ApiKeyBackend::new(Some("sk-ant-test".into()));
            backend.base_url = server.uri();
            let req = LlmRequest {
                system: "s".into(),
                cached_prefix: None,
                prompt: "p".into(),
                schema: json!({"type": "object"}),
                model: Some(crate::config::API_KEY_PICK_MODEL.into()),
                background: false,
                effort: None,
            };
            let purpose = crate::ai_usage::Purpose::new(crate::db::AiKind::Pick);
            assert!(crate::ai_usage::call(&db, &backend, &purpose, req).await.is_err(), "{stop}");
        }
        let rows = db.ai_usage_rows(0).unwrap();
        assert_eq!(rows.len(), 1, "one model, not a row for the alias: {rows:?}");
        assert_eq!(rows[0].model.as_deref(), Some("claude-sonnet-5-5"));
        let t = &rows[0].totals;
        assert_eq!(t.usage.output_tokens, 600, "both calls' tokens are recorded");
        assert!(t.cost_usd.is_some_and(|c| c > 0.0), "charged calls are recorded as costing nothing: {t:?}");
    }

    /// A request that was sent but timed out may have been answered and
    /// charged, so it isn't sent again (B-AI-6).
    #[tokio::test]
    async fn a_request_that_timed_out_is_not_sent_again() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        let slow = ResponseTemplate::new(200)
            .set_body_json(crate::testutil::fixture_json("llm/anthropic_api_success.json"))
            .set_delay(Duration::from_secs(5));
        Mock::given(method("POST")).respond_with(slow).mount(&server).await;
        let http = reqwest::Client::builder().timeout(Duration::from_millis(300)).build().unwrap();
        let backend = ApiKeyBackend { key: Some("sk-ant-test".into()), base_url: server.uri(), http };
        let req = LlmRequest {
            system: "s".into(),
            cached_prefix: None,
            prompt: "p".into(),
            schema: json!({"type": "object"}),
            model: None,
            background: false,
            effort: None,
        };
        let err = backend.complete_json(req).await.unwrap_err();
        assert!(matches!(&err, LlmError::Failed(m) if m.contains("request to the Anthropic API failed")), "{err:?}");
        assert_eq!(server.received_requests().await.unwrap().len(), 1, "a request that may have been charged was sent again");
    }
}
