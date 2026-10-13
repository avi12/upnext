//! The verdicts service's endpoint as UpNext calls it (`docs/VERDICTS.md`,
//! "The endpoint"): send or replace a verdict, withdraw one or all, list a
//! key's own, and report a published text. Every request carries the
//! player's key as `Authorization: Bearer upnext_v_…` and nothing else that
//! names anyone: no cookies, no account, and the body is the allowlisted
//! [`Verdict`] or a report.
//!
//! The base is the `verdicts_url` setting, so tests point it at `wiremock`
//! and a self-run service is one setting.

use super::verdict::{KEY_PREFIX, Verdict};
use anyhow::Result;
use reqwest::{Client, Method, RequestBuilder, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The most a reply is read, as the service's bodies are a few kilobytes
/// and `GET mine` a few hundred verdicts.
const MAX_REPLY: usize = 4 << 20;

/// How long to wait before trying a busy service again when it doesn't say.
pub const DEFAULT_RETRY_SECS: i64 = 3600;

/// The longest wait a `Retry-After` is taken for: a day. The service's
/// limits are daily, and a larger number would only push the retry past
/// the queue's week (or overflow the time it's added to).
pub const MAX_RETRY_SECS: i64 = 86_400;

/// One thing the service said is wrong with a verdict or a report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RemoteProblem {
    pub field: String,
    pub why: String,
}

/// Why a call to the service didn't go through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceError {
    /// It answered and refused: a 400 (with what's wrong), 401, 403 (the
    /// key is blocked), 404, 413 or 415.
    Refused { status: u16, error: String, problems: Vec<RemoteProblem> },
    /// A limit is reached (429); try again after this many seconds.
    Limited { retry_after: i64 },
    /// It couldn't be reached, or failed on its side (5xx, or an answer
    /// that isn't the contract's).
    Down(String),
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused { error, .. } => write!(f, "the verdicts service refused it: {error}"),
            Self::Limited { retry_after } => {
                write!(f, "the verdicts service is taking no more for now; try again in {}", wait_words(*retry_after))
            }
            Self::Down(why) => write!(f, "the verdicts service couldn't be reached: {why}"),
        }
    }
}

impl std::error::Error for ServiceError {}

/// "an hour", "5 minutes", "2 hours"
pub fn wait_words(secs: i64) -> String {
    match secs {
        ..=60 => "a minute".into(),
        61..=3599 => format!("{} minutes", (secs + 59) / 60),
        3600..=5400 => "an hour".into(),
        _ => format!("{} hours", (secs + 1799) / 3600),
    }
}

/// What the service said when it took a verdict.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Taken {
    /// The verdict's id, the one the snapshot publishes it under.
    pub id: String,
    /// `live`, or `held` while its game is getting a rush of verdicts.
    pub state: String,
    /// It replaced the key's earlier verdict about the game.
    pub replaced: bool,
}

/// One of the key's verdicts as `GET /v1/verdicts/mine` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Listed {
    pub id: String,
    /// `live`, `held`, `withdrawn` or `removed`.
    pub state: String,
    pub created_day: String,
    pub updated_day: String,
    #[serde(default)]
    pub steam_appid: Option<u32>,
    #[serde(default)]
    pub gog_id: Option<String>,
    #[serde(default)]
    pub outcome: Option<String>,
    #[serde(default)]
    pub worth: Option<String>,
    #[serde(default)]
    pub hours: Option<String>,
    #[serde(default)]
    pub clicked: Option<String>,
    #[serde(default)]
    pub sessions: Option<String>,
    #[serde(default)]
    pub limited_time: Option<String>,
    #[serde(default)]
    pub factors: Vec<String>,
    #[serde(default)]
    pub stop_point: Option<String>,
    #[serde(default)]
    pub stop_note: Option<String>,
    #[serde(default)]
    pub stopped_for: Vec<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub vocab: Option<i64>,
    #[serde(default)]
    pub licence: Option<String>,
    #[serde(default)]
    pub consent: Option<i64>,
}

impl Listed {
    /// The verdict it lists, as this build would hold it.
    pub fn verdict(&self) -> Verdict {
        let mut v = Verdict::new(self.steam_appid, self.gog_id.clone());
        v.outcome = self.outcome.clone();
        v.worth = self.worth.clone();
        v.hours = self.hours.clone();
        v.clicked = self.clicked.clone();
        v.sessions = self.sessions.clone();
        v.limited_time = self.limited_time.clone();
        v.factors = self.factors.clone();
        v.stop_point = self.stop_point.clone();
        v.stop_note = self.stop_note.clone();
        v.stopped_for = self.stopped_for.clone();
        v.text = self.text.clone();
        if let Some(n) = self.vocab {
            v.vocab = n;
        }
        if let Some(l) = &self.licence {
            v.licence = l.clone();
        }
        if let Some(n) = self.consent {
            v.consent = n;
        }
        v
    }
}

#[derive(Deserialize)]
struct MineReply {
    verdicts: Vec<Listed>,
}

#[derive(Deserialize)]
struct WithdrawnReply {
    withdrawn: i64,
}

/// A new key: [`KEY_PREFIX`] and 32 random bytes in lowercase hex.
pub fn new_key() -> Result<String> {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).map_err(|e| anyhow::anyhow!("no randomness for a verdicts key: {e}"))?;
    Ok(format!("{KEY_PREFIX}{}", b.iter().map(|x| format!("{x:02x}")).collect::<String>()))
}

/// The service at one address, called with one player's key.
pub struct Service<'a> {
    http: &'a Client,
    base: String,
    key: &'a str,
}

impl<'a> Service<'a> {
    pub fn new(http: &'a Client, base: &str, key: &'a str) -> Self {
        Service { http, base: base.trim_end_matches('/').to_string(), key }
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        self.http.request(method, format!("{}{path}", self.base)).bearer_auth(self.key)
    }

    /// Send a verdict, or replace the key's earlier one about the game.
    pub async fn send(&self, v: &Verdict) -> Result<Taken, ServiceError> {
        let r = call(self.request(Method::POST, "/v1/verdicts").json(v)).await?;
        parse(r, &[StatusCode::OK, StatusCode::CREATED]).await
    }

    /// Withdraw one of the key's verdicts. False when the service has no
    /// such verdict of this key (withdrawn already, or removed).
    pub async fn withdraw(&self, id: &str) -> Result<bool, ServiceError> {
        let r = call(self.request(Method::DELETE, &format!("/v1/verdicts/{id}"))).await?;
        match r.status() {
            StatusCode::NO_CONTENT | StatusCode::OK => Ok(true),
            StatusCode::NOT_FOUND => Ok(false),
            _ => Err(failure(r).await),
        }
    }

    /// Withdraw every verdict of the key: how many there were.
    pub async fn withdraw_all(&self) -> Result<i64, ServiceError> {
        let r = call(self.request(Method::DELETE, "/v1/verdicts")).await?;
        let reply: WithdrawnReply = parse(r, &[StatusCode::OK]).await?;
        Ok(reply.withdrawn)
    }

    /// The key's verdicts as the service has them.
    pub async fn mine(&self) -> Result<Vec<Listed>, ServiceError> {
        let r = call(self.request(Method::GET, "/v1/verdicts/mine")).await?;
        let reply: MineReply = parse(r, &[StatusCode::OK]).await?;
        Ok(reply.verdicts)
    }

    /// Report a published text.
    pub async fn report(&self, verdict_id: &str, reason: &str) -> Result<(), ServiceError> {
        let r = call(self.request(Method::POST, "/v1/reports").json(&json!({"verdict_id": verdict_id, "reason": reason}))).await?;
        let _: Value = parse(r, &[StatusCode::ACCEPTED]).await?;
        Ok(())
    }
}

async fn call(req: RequestBuilder) -> Result<reqwest::Response, ServiceError> {
    req.send().await.map_err(|e| ServiceError::Down(without_url(&e)))
}

/// A transport error in a few words. The address is the service's, which
/// isn't secret, and the key is in a header, never in the address.
fn without_url(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "it didn't answer in time".into()
    } else if e.is_connect() {
        "no connection".into()
    } else {
        e.to_string()
    }
}

/// The body of a reply with one of `ok`'s statuses, or what went wrong.
async fn parse<T: serde::de::DeserializeOwned>(r: reqwest::Response, ok: &[StatusCode]) -> Result<T, ServiceError> {
    if !ok.contains(&r.status()) {
        return Err(failure(r).await);
    }
    let text = crate::http::text_capped(r, MAX_REPLY).await.map_err(|e| ServiceError::Down(e.to_string()))?;
    let text = if text.trim().is_empty() { "{}" } else { &text };
    serde_json::from_str(text).map_err(|e| ServiceError::Down(format!("its answer isn't what UpNext expects ({e})")))
}

/// The error a non-2xx reply stands for.
async fn failure(r: reqwest::Response) -> ServiceError {
    let status = r.status();
    if status == StatusCode::TOO_MANY_REQUESTS {
        let retry_after = r
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.trim().parse::<i64>().ok())
            .filter(|s| *s > 0)
            .map(|s| s.min(MAX_RETRY_SECS))
            .unwrap_or(DEFAULT_RETRY_SECS);
        return ServiceError::Limited { retry_after };
    }
    let text = crate::http::text_capped(r, MAX_REPLY).await.unwrap_or_default();
    if status.is_server_error() {
        return ServiceError::Down(format!("it answered {}", status.as_u16()));
    }
    #[derive(Deserialize)]
    struct Body {
        error: Option<String>,
        #[serde(default)]
        problems: Vec<RemoteProblem>,
    }
    let body: Option<Body> = serde_json::from_str(&text).ok();
    let (error, problems) = match body {
        Some(b) => (b.error, b.problems),
        None => (None, vec![]),
    };
    // The service's words, kept short: they're shown to the player.
    let error: String = error.unwrap_or_else(|| format!("it answered {}", status.as_u16())).chars().take(300).collect();
    ServiceError::Refused { status: status.as_u16(), error, problems }
}
