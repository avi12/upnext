//! Per-token limits, so a runaway script can't starve the UI or
//! run up an AI bill. This computer, without a token, has none. A request
//! that a listed proxy let in without a token counts
//! against the client address the proxy names.
//!
//! - About 10 requests a second, with bursts of 20: beyond that, `429`.
//! - One pick and one Help me decide at a time, since each is an AI call:
//!   a second while the first runs gets `429` with `Retry-After`.

use axum::http::Method;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Requests a second a token gets, on average.
const RATE: f64 = 10.0;
/// How many it can make at once before the rate applies.
const BURST: f64 = 20.0;
/// What a `429` for a busy AI call says to wait, in seconds.
pub const AI_RETRY_AFTER: u64 = 5;

/// The AI calls a token makes one at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AiCall {
    /// A pick, more picks, or "too" (picks again with a word ruled out).
    Pick,
    /// Help me decide, on the Versus page.
    Decide,
}

impl AiCall {
    /// The AI call a request makes, if it makes one (`path` without `/v1`).
    pub fn of(method: &Method, path: &str) -> Option<Self> {
        if method != Method::POST {
            return None;
        }
        let picks = path
            .strip_prefix("/api/picks")
            .is_some_and(|rest| rest.is_empty() || (rest.ends_with("/more") || rest.ends_with("/too")) && rest.matches('/').count() == 2);
        match path {
            _ if picks => Some(Self::Pick),
            "/api/versus/decide" => Some(Self::Decide),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Pick => "a pick",
            Self::Decide => "a Help me decide call",
        }
    }
}

/// Whose requests a limit counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Token(i64),
    /// A client behind a listed proxy that does the login.
    Address(std::net::IpAddr),
}

struct Bucket {
    tokens: f64,
    at: Instant,
}

#[derive(Default)]
pub struct Limits {
    buckets: Mutex<HashMap<Key, Bucket>>,
    busy: Arc<Mutex<HashSet<(Key, AiCall)>>>,
}

/// Holds a token's AI call slot until dropped.
pub struct Busy {
    held: Arc<Mutex<HashSet<(Key, AiCall)>>>,
    key: (Key, AiCall),
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.held.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.key);
    }
}

impl Limits {
    /// Count one request for `token`. `Err` with how long until it may ask
    /// again when it's over the rate.
    pub fn request(&self, token: Key) -> Result<(), Duration> {
        self.request_at(token, Instant::now())
    }

    fn request_at(&self, token: Key, now: Instant) -> Result<(), Duration> {
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        let b = buckets.entry(token).or_insert(Bucket { tokens: BURST, at: now });
        b.tokens = (b.tokens + now.saturating_duration_since(b.at).as_secs_f64() * RATE).min(BURST);
        b.at = now;
        if b.tokens >= 1.0 {
            b.tokens -= 1.0;
            Ok(())
        } else {
            Err(Duration::from_secs_f64((1.0 - b.tokens) / RATE))
        }
    }

    /// Take `token`'s slot for `call`, or None while another holds it.
    pub fn ai_call(&self, token: Key, call: AiCall) -> Option<Busy> {
        let key = (token, call);
        let taken = self.busy.lock().unwrap_or_else(|e| e.into_inner()).insert(key);
        taken.then(|| Busy { held: self.busy.clone(), key })
    }

    /// Forget a revoked token's bucket.
    pub fn forget(&self, token: i64) {
        self.buckets.lock().unwrap_or_else(|e| e.into_inner()).remove(&Key::Token(token));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_a_second_with_bursts() {
        let l = Limits::default();
        let t0 = Instant::now();
        let (one, two) = (Key::Token(1), Key::Token(2));
        for i in 0..20 {
            assert!(l.request_at(one, t0).is_ok(), "burst {i}");
        }
        let wait = l.request_at(one, t0).unwrap_err();
        assert!(wait <= Duration::from_millis(100), "{wait:?}");
        assert!(l.request_at(two, t0).is_ok(), "another token has its own");
        assert!(l.request_at(Key::Address("192.168.0.42".parse().unwrap()), t0).is_ok(), "a client behind a proxy has its own");
        assert!(l.request_at(one, t0 + Duration::from_millis(100)).is_ok(), "one back after a tenth of a second");
        assert!(l.request_at(one, t0 + Duration::from_millis(100)).is_err());
    }

    #[test]
    fn one_ai_call_of_each_kind_at_a_time() {
        let l = Limits::default();
        let (one, two) = (Key::Token(1), Key::Token(2));
        let first = l.ai_call(one, AiCall::Pick).unwrap();
        assert!(l.ai_call(one, AiCall::Pick).is_none());
        assert!(l.ai_call(one, AiCall::Decide).is_some(), "a decide alongside a pick");
        assert!(l.ai_call(two, AiCall::Pick).is_some(), "another token");
        drop(first);
        assert!(l.ai_call(one, AiCall::Pick).is_some(), "free again once the first is answered");
    }

    #[test]
    fn which_requests_are_ai_calls() {
        let post = Method::POST;
        assert_eq!(AiCall::of(&post, "/api/picks"), Some(AiCall::Pick));
        assert_eq!(AiCall::of(&post, "/api/picks/12/more"), Some(AiCall::Pick));
        assert_eq!(AiCall::of(&post, "/api/picks/12/too"), Some(AiCall::Pick));
        assert_eq!(AiCall::of(&post, "/api/picks/12/feedback"), None);
        assert_eq!(AiCall::of(&Method::GET, "/api/picks/history"), None);
        assert_eq!(AiCall::of(&post, "/api/versus/decide"), Some(AiCall::Decide));
    }
}
