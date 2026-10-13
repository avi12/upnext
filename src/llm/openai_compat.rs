//! OpenAI-compatible chat completions backend: local servers (Ollama,
//! LM Studio, llama.cpp, vLLM, LocalAI) or any hosted endpoint that speaks
//! `POST {base}/chat/completions`. Asks for schema-constrained JSON
//! (`response_format: json_schema`) and falls back to plain JSON mode with the
//! schema in the prompt for servers that reject it.

use super::{LlmBackend, LlmError, LlmRequest, LlmResponse, Pricing, Usage};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// The most of an answer read from the server: far more than a reply or a
/// model list needs. Whoever runs the server chooses what it answers, and a
/// full token can choose the server.
const MAX_ANSWER: usize = 16 << 20;

pub struct OpenAiCompatBackend {
    base_url: String,
    model: Option<String>,
    key: Option<String>,
    /// `Some(false)` asks the model not to think first (`Config::openai_thinking`).
    thinking: Option<bool>,
    http: reqwest::Client,
}

impl OpenAiCompatBackend {
    pub fn new(base_url: String, model: Option<String>, key: Option<String>) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(crate::http::USER_AGENT)
            // Local models can take minutes on a long candidate list.
            .timeout(Duration::from_secs(900))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("reqwest client");
        Self { base_url: base_url.trim_end_matches('/').to_string(), model, key, thinking: None, http }
    }

    /// Ask the model to think first or not; None leaves its default.
    pub fn with_thinking(mut self, thinking: Option<bool>) -> Self {
        self.thinking = thinking;
        self
    }

    /// The request's fields beyond the chat: the thinking switch, when set.
    fn extra(&self, body: &mut Value) {
        if let Some(on) = self.thinking {
            body["chat_template_kwargs"] = json!({ "enable_thinking": on });
        }
    }

    async fn post(&self, body: &Value) -> Result<(reqwest::StatusCode, String), LlmError> {
        let mut rb = self.http.post(format!("{}/chat/completions", self.base_url)).json(body);
        if let Some(k) = &self.key {
            rb = rb.bearer_auth(k);
        }
        let r = rb.send().await.map_err(|e| {
            if e.is_connect() {
                LlmError::NotAvailable(format!("cannot reach {} ({e}); is the server running?", self.base_url))
            } else {
                LlmError::Failed(format!("request to {} failed: {e}", self.base_url))
            }
        })?;
        let status = r.status();
        let text = crate::http::text_capped(r, MAX_ANSWER).await.map_err(|e| LlmError::Failed(format!("reading the response: {e:#}")))?;
        Ok((status, text))
    }
}

impl OpenAiCompatBackend {
    /// Model ids the server offers (`GET {base}/models`), sorted.
    pub async fn list_models(&self) -> Result<Vec<String>, LlmError> {
        let mut rb = self.http.get(format!("{}/models", self.base_url)).timeout(Duration::from_secs(15));
        if let Some(k) = &self.key {
            rb = rb.bearer_auth(k);
        }
        let r = rb.send().await.map_err(|e| LlmError::NotAvailable(format!("cannot reach {}: {}", self.base_url, e.without_url())))?;
        let status = r.status();
        let text = crate::http::text_capped(r, MAX_ANSWER).await.unwrap_or_default();
        if !status.is_success() {
            return Err(LlmError::Failed(format!("{} returned {status}: {}", self.base_url, error_message(&text))));
        }
        let v: Value = serde_json::from_str(&text).map_err(|e| LlmError::BadOutput(format!("model list is not JSON ({e})"), None))?;
        let mut ids: Vec<String> = v
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|m| m.get("id").and_then(Value::as_str))
            // Gemini lists "models/<id>"; the chat endpoint wants the bare id.
            .map(|id| id.trim_start_matches("models/").to_string())
            .collect();
        ids.sort();
        ids.dedup();
        Ok(ids)
    }
}

/// The server's own error message, from an OpenAI-style error body. Never
/// the body itself: a full token may choose the address, and whatever
/// answers there (a router's page, a cloud metadata service) mustn't be
/// read back through UpNext.
fn error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.pointer("/error/message").or_else(|| v.get("error")).and_then(|m| m.as_str().map(str::to_string)))
        .map(|m| m.chars().take(400).collect())
        .unwrap_or_else(|| "its answer wasn't an API error message".to_string())
}

/// Some local models wrap JSON in code fences or add a preamble; take the
/// last whole object. A reasoning model on a server that leaves its
/// thinking in the reply writes `<think>…</think>` first, and the thinking
/// often mentions a brace ("the schema wants {picks, spares}"), so the
/// answer is looked for after it.
fn extract_json(text: &str) -> Option<Value> {
    const THOUGHT: &str = "</think>";
    let t = text.rfind(THOUGHT).map_or(text, |i| &text[i + THOUGHT.len()..]).trim();
    if let Ok(v) = serde_json::from_str(t) {
        return Some(v);
    }
    // Each `{` that starts a whole object, skipping the objects inside it.
    let (mut last, mut from) = (None, 0);
    while let Some(start) = t[from..].find('{').map(|i| from + i) {
        let mut values = serde_json::Deserializer::from_str(&t[start..]).into_iter::<Value>();
        match values.next() {
            Some(Ok(v)) => {
                from = start + values.byte_offset();
                last = Some(v);
            }
            _ => from = start + 1,
        }
    }
    last
}

#[async_trait]
impl LlmBackend for OpenAiCompatBackend {
    fn name(&self) -> &str {
        "openai"
    }

    fn pricing(&self) -> Option<Pricing> {
        crate::prices::Provider::for_base_url(&self.base_url).map(|provider| Pricing { provider, charged: true })
    }

    async fn available(&self) -> Result<(), LlmError> {
        if self.base_url.is_empty() {
            return Err(LlmError::NotAvailable("no server address set for the OpenAI-compatible backend".into()));
        }
        if self.model.is_none() {
            return Err(LlmError::NotAvailable("no model set for the OpenAI-compatible backend".into()));
        }
        Ok(())
    }

    async fn complete_json(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        self.available().await?;
        // The caller's model (e.g. a cheaper one for summaries), else the server default.
        let model = req.model.clone().or_else(|| self.model.clone()).unwrap_or_default();
        let started = Instant::now();
        let messages = json!([
            { "role": "system", "content": super::guarded_system(&req.system) },
            { "role": "user", "content": req.full_prompt() },
        ]);
        let mut body = json!({
            "model": model,
            "messages": messages,
            "response_format": {
                "type": "json_schema",
                "json_schema": { "name": "upnext_response", "schema": req.schema, "strict": true }
            },
        });
        self.extra(&mut body);
        let (mut status, mut text) = self.post(&body).await?;
        // Older servers only know JSON mode: put the schema in the prompt instead.
        if status.as_u16() == 400 || status.as_u16() == 422 {
            tracing::warn!("{} rejected json_schema ({status}); retrying in JSON mode", self.base_url);
            let system = format!(
                "{}\n\nRespond with a single JSON object that matches this JSON Schema exactly:\n{}",
                super::guarded_system(&req.system),
                req.schema
            );
            let mut fallback = json!({
                "model": model,
                "messages": [
                    { "role": "system", "content": system },
                    { "role": "user", "content": req.full_prompt() },
                ],
                "response_format": { "type": "json_object" },
            });
            self.extra(&mut fallback);
            (status, text) = self.post(&fallback).await?;
        }
        if !status.is_success() {
            let msg = error_message(&text);
            return Err(match status.as_u16() {
                401 | 403 => LlmError::NotAvailable(format!("{} rejected the API key ({status}): {msg}", self.base_url)),
                404 => LlmError::NotAvailable(format!(
                    "{} returned 404: {msg} (check the address ends in /v1 and the model name)",
                    self.base_url
                )),
                429 => LlmError::LimitReached(format!("{} is rate-limiting or out of quota ({status}): {msg}", self.base_url)),
                _ => LlmError::Failed(format!("{} returned {status}: {msg}", self.base_url)),
            });
        }
        let resp: Value = serde_json::from_str(&text).map_err(|e| LlmError::BadOutput(format!("response is not JSON ({e})"), None))?;
        let mut out = reply(&resp, model)?;
        out.duration_ms = started.elapsed().as_millis() as u64;
        Ok(out)
    }
}

/// A chat completion: the JSON the model wrote, its model and its tokens
/// (`duration_ms` is left for the caller).
fn reply(resp: &Value, requested: String) -> Result<LlmResponse, LlmError> {
    let usage = resp.get("usage").and_then(Usage::from_openai);
    let choice = resp.pointer("/choices/0").ok_or_else(|| LlmError::BadOutput("response has no choices".into(), usage))?;
    if choice.get("finish_reason").and_then(Value::as_str) == Some("length") {
        return Err(LlmError::BadOutput("the model ran out of output tokens before finishing".into(), usage));
    }
    let content = choice.pointer("/message/content").and_then(Value::as_str).unwrap_or_default();
    let json = extract_json(content)
        .ok_or_else(|| LlmError::BadOutput(format!("reply is not JSON: {}", content.chars().take(400).collect::<String>()), usage))?;
    Ok(LlmResponse {
        json,
        model: resp.get("model").and_then(Value::as_str).map(str::to_string).or(Some(requested)),
        usage,
        // Hosted providers charge: `ai_usage::call` prices the call when it can.
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

    /// A real reply from vLLM: no cached tokens reported, and the reasoning
    /// is part of `completion_tokens`.
    #[test]
    fn a_reply_reports_its_tokens() {
        let resp = crate::testutil::fixture_json("llm/openai_vllm_success.json");
        let out = reply(&resp, "qwen".into()).unwrap();
        assert_eq!(out.json, json!({"ok": true}));
        assert_eq!(out.model.as_deref(), Some("qwen3.8-27b"));
        assert_eq!(out.usage, Some(Usage { input_tokens: 35, output_tokens: 39, ..Default::default() }));
        assert_eq!((out.cost_usd, out.would_cost_usd), (None, None));

        let mut cached = resp.clone();
        cached["usage"]["prompt_tokens_details"] = json!({"cached_tokens": 30});
        assert_eq!(reply(&cached, "qwen".into()).unwrap().usage.unwrap().cache_read_tokens, 30);

        let mut bare = resp.clone();
        bare.as_object_mut().unwrap().remove("usage");
        assert_eq!(reply(&bare, "qwen".into()).unwrap().usage, None, "a server that says nothing about tokens");

        let mut cut = resp.clone();
        cut["choices"][0]["finish_reason"] = "length".into();
        assert_eq!(reply(&cut, "qwen".into()).unwrap_err().usage().map(|u| u.output_tokens), Some(39));
    }

    /// With thinking off, every request (the
    /// JSON-mode retry too) carries the switch; otherwise none does.
    #[tokio::test]
    async fn thinking_off_is_sent_with_each_request() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(wiremock::matchers::body_partial_json(json!({"response_format": {"type": "json_schema"}})))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error": {"message": "no json_schema here"}})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(crate::testutil::fixture_json("llm/openai_vllm_success.json")))
            .mount(&server)
            .await;
        let req = || LlmRequest {
            system: "s".into(),
            cached_prefix: None,
            prompt: "p".into(),
            schema: json!({"type": "object"}),
            model: None,
            background: false,
            effort: None,
        };
        async fn sent(server: &MockServer) -> Vec<Option<Value>> {
            let reqs = server.received_requests().await.unwrap();
            reqs.iter().map(|r| serde_json::from_slice::<Value>(&r.body).unwrap().get("chat_template_kwargs").cloned()).collect()
        }

        let off = OpenAiCompatBackend::new(format!("{}/v1", server.uri()), Some("qwen".into()), None).with_thinking(Some(false));
        off.complete_json(req()).await.unwrap();
        assert_eq!(sent(&server).await, [Some(json!({"enable_thinking": false})), Some(json!({"enable_thinking": false}))]);

        server.reset().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(crate::testutil::fixture_json("llm/openai_vllm_success.json")))
            .mount(&server)
            .await;
        let default = OpenAiCompatBackend::new(format!("{}/v1", server.uri()), Some("qwen".into()), None);
        default.complete_json(req()).await.unwrap();
        assert_eq!(sent(&server).await, [None], "the model's own default");
    }

    #[test]
    fn errors_say_the_servers_message_not_its_page() {
        assert_eq!(error_message(r#"{"error":{"message":"model \"x\" not found"}}"#), "model \"x\" not found");
        assert_eq!(error_message(r#"{"error":"unauthorized"}"#), "unauthorized");
        let page = error_message("<html><title>Router admin</title>secret-ish</html>");
        assert!(!page.contains("Router") && !page.contains("secret"), "{page}");
    }

    /// Security review L4: an answer that unzips to far more than any reply
    /// is given up at the cap, not read into memory whole.
    #[tokio::test]
    async fn a_huge_answer_is_cut_off_at_the_cap() {
        use std::io::Write;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        gz.write_all(&vec![b' '; MAX_ANSWER + (4 << 20)]).unwrap();
        let small = gz.finish().unwrap();
        assert!(small.len() < 1 << 20, "the bomb itself is small: {}", small.len());
        let server = wiremock::MockServer::start().await;
        let bomb = wiremock::ResponseTemplate::new(200)
            .insert_header("content-encoding", "gzip")
            .insert_header("content-type", "application/json")
            .set_body_bytes(small);
        wiremock::Mock::given(wiremock::matchers::any()).respond_with(bomb).mount(&server).await;
        let backend = OpenAiCompatBackend::new(server.uri(), Some("m".into()), None);
        let req = LlmRequest {
            system: "s".into(),
            cached_prefix: None,
            prompt: "p".into(),
            schema: json!({"type": "object"}),
            model: None,
            background: false,
            effort: None,
        };
        let Err(e) = backend.complete_json(req).await else { panic!("a reply from a bomb") };
        assert!(e.to_string().contains("over 16 MB"), "{e}");
        assert!(backend.list_models().await.is_err(), "the model list too");
    }

    #[test]
    fn json_from_wrapped_reply() {
        assert_eq!(extract_json("```json\n{\"a\":1}\n```").unwrap()["a"], 1);
        assert_eq!(extract_json("Sure! {\"a\":2} Hope that helps").unwrap()["a"], 2);
        assert!(extract_json("no json here").is_none());
        assert!(extract_json("} then {").is_none());
        assert_eq!(extract_json("Sure! {\"a\": {\"b\": 1}} Hope that helps").unwrap(), json!({"a": {"b": 1}}), "the whole object");
        assert_eq!(extract_json("e.g. {\"a\": 0}; mine: {\"a\": 3} done").unwrap()["a"], 3, "the last one");
    }

    /// A reasoning model's thinking, left in the reply, mentions braces
    /// before the answer; the answer is still read (B-AI-10).
    #[test]
    fn json_after_a_think_block_is_read() {
        let content = "<think>The schema wants {picks, spares}; pick #3 first.</think>\n{\"picks\": [], \"spares\": []}";
        assert_eq!(extract_json(content), Some(json!({"picks": [], "spares": []})));
        let fenced = "<think>{</think>\n```json\n{\"picks\": [1]}\n```";
        assert_eq!(extract_json(fenced), Some(json!({"picks": [1]})));
        let unclosed = "<think>I'll answer {picks} as asked: {\"picks\": [2]}";
        assert_eq!(extract_json(unclosed), Some(json!({"picks": [2]})), "no closing tag: the last whole object");
    }
}
