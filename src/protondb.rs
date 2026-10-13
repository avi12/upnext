//! ProtonDB: community Linux/Proton compatibility tier per Steam appid
//! (platinum, gold, silver, bronze, borked). No key; unofficial JSON summary.

use crate::http::{Backoff, RateLimiter, base_url, send_with_backoff};
use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct ProtonSummary {
    pub tier: String,
    pub trending_tier: Option<String>,
    pub confidence: Option<String>,
    pub score: Option<f64>,
    pub total: Option<i64>,
}

const BASE: &str = "https://www.protondb.com";

pub struct ProtonDbClient {
    http: Client,
    base: String,
    limiter: RateLimiter,
}

/// Tiers that mean "runs fine on Linux" for filters and the pick prompt.
pub fn runs_well(tier: &str) -> bool {
    matches!(tier, "native" | "platinum" | "gold")
}

impl ProtonDbClient {
    /// `base` stands in for protondb.com (tests pass a local mock server).
    pub fn new(http: Client, base: Option<&str>) -> Self {
        Self { http, base: base_url(BASE, base), limiter: RateLimiter::new("ProtonDB", Duration::from_millis(300)) }
    }

    /// `Ok(None)` when ProtonDB has no reports for the app.
    pub async fn summary(&self, appid: u32) -> Result<Option<ProtonSummary>> {
        let url = format!("{}/api/v1/reports/summaries/{appid}.json", self.base);
        let resp = match send_with_backoff(&self.limiter, &Backoff::default(), || self.http.get(&url)).await {
            Ok(r) => r,
            Err(e) if crate::http::is_not_found(&e) => return Ok(None),
            Err(e) => return Err(e),
        };
        let v: Value = resp.json().await.context("ProtonDB returned non-JSON")?;
        Ok(parse_summary(&v))
    }
}

fn parse_summary(v: &Value) -> Option<ProtonSummary> {
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    s("tier").map(|tier| ProtonSummary {
        tier,
        trending_tier: s("trendingTier"),
        confidence: s("confidence"),
        score: v.get("score").and_then(Value::as_f64),
        total: v.get("total").and_then(Value::as_i64),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::fixture_json;

    #[test]
    fn summary() {
        let s = parse_summary(&fixture_json("protondb/summary_620.json")).unwrap();
        assert_eq!(s.tier, "platinum");
        assert_eq!(s.trending_tier.as_deref(), Some("platinum"));
        assert_eq!(s.confidence.as_deref(), Some("strong"));
        assert_eq!(s.total, Some(340));
        assert!(runs_well(&s.tier));
        assert!(!runs_well("borked"));
        assert!(parse_summary(&serde_json::json!({"total": 0})).is_none());
    }
}
