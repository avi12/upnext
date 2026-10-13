//! The AI providers' list prices, read from their own pricing pages, with
//! the date they were read. A call's tokens times its model's price is what
//! it costs at list prices: money spent with an API key or a hosted
//! provider, and on a subscription what the same call would cost on the
//! API. Anthropic's prices fall back to a list built in here when the page
//! can't be read; the others have no fallback, so their calls show no money.
//!
//! Anthropic and OpenAI publish their pricing pages as markdown (the page's
//! address with `.md`), with a table per kind of price. OpenRouter lists
//! every model's price in its models API. OpenRouter also lists other
//! providers' models, but its prices for them can differ from the
//! provider's own page (on 2026-09-30 it listed `openai/gpt-5.6-sol` at
//! half OpenAI's price), so it is used only for calls made through it.

use crate::db::Db;
use crate::llm::Usage;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Whose prices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Anthropic,
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(rename = "openrouter")]
    OpenRouter,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
            Self::OpenRouter => "openrouter",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Anthropic => "Anthropic",
            Self::OpenAi => "OpenAI",
            Self::OpenRouter => "OpenRouter",
        }
    }

    /// The official page, linked wherever a price shows.
    pub fn page_url(self) -> &'static str {
        match self {
            Self::Anthropic => "https://platform.claude.com/docs/en/about-claude/pricing",
            Self::OpenAi => "https://developers.openai.com/api/docs/pricing",
            Self::OpenRouter => "https://openrouter.ai/models",
        }
    }

    /// What UpNext reads: the same page as markdown, or OpenRouter's models API.
    fn source_url(self) -> &'static str {
        match self {
            Self::Anthropic => "https://platform.claude.com/docs/en/about-claude/pricing.md",
            Self::OpenAi => "https://developers.openai.com/api/docs/pricing.md",
            Self::OpenRouter => "https://openrouter.ai/api/v1/models",
        }
    }

    /// The standard prices, and the Flex ones (OpenAI's only).
    fn parse(self, body: &str) -> Result<(BTreeMap<String, Price>, BTreeMap<String, Price>)> {
        match self {
            Self::Anthropic => Ok((parse_anthropic(body)?, BTreeMap::new())),
            Self::OpenAi => Ok((parse_openai(body)?, parse_openai_flex(body))),
            Self::OpenRouter => Ok((parse_openrouter(body)?, BTreeMap::new())),
        }
    }

    /// The provider an OpenAI-style server at `base_url` charges for, when
    /// UpNext can read its prices.
    pub fn for_base_url(base_url: &str) -> Option<Provider> {
        let host = reqwest::Url::parse(base_url).ok()?.host_str()?.to_ascii_lowercase();
        match host.as_str() {
            "api.openai.com" => Some(Self::OpenAi),
            "openrouter.ai" => Some(Self::OpenRouter),
            _ => None,
        }
    }
}

/// US dollars per million tokens.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    /// Input read from the cache. None when the page lists none: it then
    /// costs `input`.
    pub cache_read: Option<f64>,
    /// Input written to the cache for five minutes, and for an hour. None
    /// when the page lists none: Anthropic's rule, 1.25 and 2 times `input`.
    pub cache_write_5m: Option<f64>,
    pub cache_write_1h: Option<f64>,
}

impl Price {
    /// What `u` costs: each kind of token at its own rate.
    pub fn cost(&self, u: &Usage) -> f64 {
        let write_1h = u.cache_write_1h_tokens.min(u.cache_write_tokens);
        let write_5m = u.cache_write_tokens - write_1h;
        let uncached = u.input_tokens.saturating_sub(u.cache_read_tokens + u.cache_write_tokens);
        (uncached as f64 * self.input
            + u.cache_read_tokens as f64 * self.cache_read.unwrap_or(self.input)
            + write_5m as f64 * self.cache_write_5m.unwrap_or(self.input * 1.25)
            + write_1h as f64 * self.cache_write_1h.unwrap_or(self.input * 2.0)
            + u.output_tokens as f64 * self.output)
            / 1e6
    }
}

/// One provider's prices, by model id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceList {
    pub provider: Provider,
    /// When they were read from the provider's page. None for the list
    /// built in here, which was checked on `BUILT_IN_CHECKED`.
    pub fetched_at: Option<i64>,
    pub models: BTreeMap<String, Price>,
    /// OpenAI's Flex prices, for calls that ran at Flex (half the standard
    /// ones for the GPT-6 models). Empty for the other providers.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub flex: BTreeMap<String, Price>,
}

/// A model's price in `prices`: its own id, or the longest listed id it
/// extends ("claude-sonnet-5-5-20261001" is priced as "claude-sonnet-5-5").
fn lookup(prices: &BTreeMap<String, Price>, model: &str) -> Option<Price> {
    if let Some(p) = prices.get(model) {
        return Some(*p);
    }
    prices
        .iter()
        .filter(|(id, _)| model.strip_prefix(id.as_str()).is_some_and(|rest| rest.starts_with('-')))
        .max_by_key(|(id, _)| id.len())
        .map(|(_, p)| *p)
}

impl PriceList {
    /// A model's standard price.
    pub fn get(&self, model: &str) -> Option<Price> {
        lookup(&self.models, model)
    }

    /// A model's price at Flex, or its standard price when Flex lists none.
    pub fn get_flex(&self, model: &str) -> Option<Price> {
        lookup(&self.flex, model).or_else(|| self.get(model))
    }

    /// What a call costs at these prices, when its model is listed.
    pub fn cost(&self, model: Option<&str>, usage: &Usage) -> Option<f64> {
        self.get(model?).map(|p| p.cost(usage))
    }

    /// The same for a call that ran at Flex.
    pub fn cost_flex(&self, model: Option<&str>, usage: &Usage) -> Option<f64> {
        self.get_flex(model?).map(|p| p.cost(usage))
    }
}

/// When the built-in list was checked against Anthropic's page.
pub const BUILT_IN_CHECKED: &str = "2026-10-09";

/// Anthropic's list prices on `BUILT_IN_CHECKED`, for when the page can't
/// be read: (id, input, output, cache read). Cache writes follow the rule.
/// Claude Haiku 5.5 at its price for prompts up to 100,000 tokens.
const BUILT_IN: [(&str, f64, f64, f64); 16] = [
    ("claude-fable-5-1", 10.0, 50.0, 0.25),
    ("claude-mythos-5-1", 10.0, 50.0, 0.25),
    ("claude-fable-5", 10.0, 50.0, 1.0),
    ("claude-mythos-5", 10.0, 50.0, 1.0),
    ("claude-opus-5-5", 4.0, 20.0, 0.20),
    ("claude-opus-5", 5.0, 25.0, 0.50),
    ("claude-opus-4-8", 5.0, 25.0, 0.50),
    ("claude-opus-4-7", 5.0, 25.0, 0.50),
    ("claude-opus-4-6", 5.0, 25.0, 0.50),
    ("claude-opus-4-5", 5.0, 25.0, 0.50),
    ("claude-sonnet-5-5", 2.0, 10.0, 0.10),
    ("claude-sonnet-5", 2.0, 10.0, 0.20),
    ("claude-sonnet-4-6", 3.0, 15.0, 0.30),
    ("claude-sonnet-4-5", 3.0, 15.0, 0.30),
    ("claude-haiku-5-5", 0.10, 0.50, 0.01),
    ("claude-haiku-4-5", 1.0, 5.0, 0.10),
];

fn built_in(provider: Provider) -> PriceList {
    let models = match provider {
        Provider::Anthropic => BUILT_IN
            .iter()
            .map(|&(id, input, output, read)| {
                let price = Price { input, output, cache_read: Some(read), cache_write_5m: None, cache_write_1h: None };
                (id.to_string(), price)
            })
            .collect(),
        Provider::OpenAi | Provider::OpenRouter => BTreeMap::new(),
    };
    PriceList { provider, fetched_at: None, models, flex: BTreeMap::new() }
}

// -- reading the pages ----------------------------------------------------------

/// "$12.50 / MTok", "$0.25 / MTok<sup>1</sup>", "$2.00", "-".
fn dollars(cell: &str) -> Option<f64> {
    let cell = cell.split("<sup>").next()?.trim();
    let n: f64 = cell.strip_prefix('$')?.split_whitespace().next()?.replace(',', "").parse().ok()?;
    (n.is_finite() && (0.0..10_000.0).contains(&n)).then_some(n)
}

/// A markdown table's rows of cells, from its header line to the first
/// line that isn't part of it.
fn table_after<'a>(lines: &mut impl Iterator<Item = &'a str>) -> Option<(Vec<String>, Vec<Vec<String>>)> {
    let cells = |l: &str| l.trim().trim_matches('|').split('|').map(|c| c.trim().to_string()).collect::<Vec<_>>();
    let header = lines.find(|l| l.trim_start().starts_with('|'))?;
    let mut rows = vec![];
    for line in lines.by_ref() {
        if !line.trim_start().starts_with('|') {
            break;
        }
        let row = cells(line);
        if row.iter().all(|c| c.chars().all(|ch| matches!(ch, '-' | ':' | ' '))) {
            continue;
        }
        rows.push(row);
    }
    Some((cells(header), rows))
}

/// The cell under the first header containing `name` (any case).
fn column(header: &[String], name: &str) -> Option<usize> {
    header.iter().position(|h| h.to_lowercase().contains(name))
}

/// A model cell without links or notes: "Claude Mythos 5.1 ([limited availability](…))" is
/// "Claude Mythos 5.1", "gpt-5.5 (<272K context length)" is "gpt-5.5".
fn model_name(cell: &str) -> &str {
    cell.split(" (").next().unwrap_or(cell).trim()
}

/// Anthropic's page: the first table under "Model pricing", one row per
/// model by its name ("Claude Sonnet 5.5"), which becomes its id
/// ("claude-sonnet-5-5"). A model priced by prompt length has a row per
/// length, the shortest first (Claude Haiku 5.5: up to 100,000 tokens, then
/// over); the first is kept, since UpNext's prompts are far shorter.
fn parse_anthropic(md: &str) -> Result<BTreeMap<String, Price>> {
    let mut lines = md.lines().skip_while(|l| l.trim() != "## Model pricing");
    lines.next().context("no \"Model pricing\" section")?;
    let (header, rows) = table_after(&mut lines).context("no table under \"Model pricing\"")?;
    let col = |name: &str| column(&header, name).with_context(|| format!("no \"{name}\" column"));
    let (input, w5, w1, read, output) = (col("base input")?, col("5m cache")?, col("1h cache")?, col("cache hits")?, col("output")?);
    let mut out = BTreeMap::new();
    for row in rows {
        let name = model_name(&row[0]);
        let Some(rest) = name.strip_prefix("Claude ") else { continue };
        let id = format!("claude-{}", rest.to_lowercase().replace([' ', '.'], "-"));
        let cell = |i: usize| row.get(i).and_then(|c| dollars(c));
        if let (Some(input), Some(output)) = (cell(input), cell(output)) {
            out.entry(id).or_insert(Price { input, output, cache_read: cell(read), cache_write_5m: cell(w5), cache_write_1h: cell(w1) });
        }
    }
    Ok(out)
}

/// OpenAI's page: the table under "Standard pricing data", one row per
/// model id, at the short-context prices.
fn parse_openai(md: &str) -> Result<BTreeMap<String, Price>> {
    openai_table(md, "Standard")
}

/// The Flex prices, from the table under "Flex pricing data". A page
/// without one leaves Flex calls at the standard prices.
fn parse_openai_flex(md: &str) -> BTreeMap<String, Price> {
    openai_table(md, "Flex").unwrap_or_else(|e| {
        tracing::warn!("OpenAI's Flex prices: {e:#}");
        BTreeMap::new()
    })
}

/// One of the page's tables ("Standard", "Batch", "Flex", ...).
fn openai_table(md: &str, tier: &str) -> Result<BTreeMap<String, Price>> {
    let title = format!("### {tier} pricing data");
    let mut lines = md.lines().skip_while(|l| !l.trim().eq_ignore_ascii_case(&title));
    lines.next().with_context(|| format!("no \"{tier} pricing data\" section"))?;
    let (header, rows) = table_after(&mut lines).with_context(|| format!("no table under \"{tier} pricing data\""))?;
    let col = |name: &str| column(&header, name).with_context(|| format!("no \"{name}\" column"));
    let (input, read, write, output) =
        (col("short context input")?, col("short context cached input")?, col("short context cache writes")?, col("short context output")?);
    let mut out = BTreeMap::new();
    for row in rows {
        let id = model_name(&row[0]).to_string();
        let cell = |i: usize| row.get(i).and_then(|c| dollars(c));
        if let (Some(input), Some(output)) = (cell(input), cell(output))
            && !id.contains(' ')
        {
            out.insert(id, Price { input, output, cache_read: cell(read), cache_write_5m: cell(write), cache_write_1h: None });
        }
    }
    Ok(out)
}

/// OpenRouter's models API: dollars per token, as strings, by its model ids
/// ("anthropic/claude-sonnet-5.5").
fn parse_openrouter(body: &str) -> Result<BTreeMap<String, Price>> {
    let v: Value = serde_json::from_str(body).context("not JSON")?;
    let models = v.get("data").and_then(Value::as_array).context("no \"data\" list")?;
    let per_million = |p: &Value, k: &str| -> Option<f64> {
        let n: f64 = p.get(k)?.as_str()?.parse().ok()?;
        (n.is_finite() && n >= 0.0).then_some(n * 1e6)
    };
    let mut out = BTreeMap::new();
    for m in models {
        let (Some(id), Some(p)) = (m.get("id").and_then(Value::as_str), m.get("pricing")) else { continue };
        if let (Some(input), Some(output)) = (per_million(p, "prompt"), per_million(p, "completion")) {
            let price = Price {
                input,
                output,
                cache_read: per_million(p, "input_cache_read"),
                cache_write_5m: per_million(p, "input_cache_write"),
                cache_write_1h: per_million(p, "input_cache_write_1h"),
            };
            out.insert(id.to_string(), price);
        }
    }
    Ok(out)
}

// -- stored and fetched -----------------------------------------------------------

/// Prices are read again when they are older than this,
const STALE_SECS: i64 = 7 * 86_400;
/// but no sooner than this after a try that failed.
const RETRY_SECS: i64 = 86_400;
/// Fewer models than this means the page changed, not the prices.
const MIN_MODELS: usize = 3;

fn key(provider: Provider) -> String {
    format!("prices.{}", provider.as_str())
}

/// The last try to read a provider's prices.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Tried {
    at: i64,
    error: Option<String>,
}

fn tried(db: &Db, provider: Provider) -> Option<Tried> {
    db.meta_get(&format!("{}.tried", key(provider))).ok().flatten().and_then(|s| serde_json::from_str(&s).ok())
}

/// The prices in use: the last ones read, else the built-in list (empty
/// for providers without one).
pub fn current(db: &Db, provider: Provider) -> PriceList {
    db.meta_get(&key(provider))
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str::<PriceList>(&s).ok())
        .filter(|l| l.provider == provider)
        .unwrap_or_else(|| built_in(provider))
}

/// Read a provider's prices from its page and keep them. When that fails,
/// the prices in use stay as they were and the error is kept to show.
/// `base` stands in for the providers' hosts (`Clients::base_url`): the
/// page is then `{base}/prices/{provider}`.
pub async fn refresh(db: &Db, http: &reqwest::Client, provider: Provider, base: Option<&str>) -> Result<PriceList> {
    let url = match base {
        Some(b) => format!("{}/prices/{}", b.trim_end_matches('/'), provider.as_str()),
        None => provider.source_url().to_string(),
    };
    refresh_from(db, http, provider, &url).await
}

/// `refresh`, reading `url` in place of the provider's page.
async fn refresh_from(db: &Db, http: &reqwest::Client, provider: Provider, url: &str) -> Result<PriceList> {
    let result = fetch(http, provider, url).await;
    let now = crate::db::now();
    let error = result.as_ref().err().map(|e| format!("{e:#}"));
    db.meta_set(&format!("{}.tried", key(provider)), &serde_json::to_string(&Tried { at: now, error })?)?;
    let (models, flex) = result?;
    let list = PriceList { provider, fetched_at: Some(now), models, flex };
    db.meta_set(&key(provider), &serde_json::to_string(&list)?)?;
    Ok(list)
}

async fn fetch(http: &reqwest::Client, provider: Provider, url: &str) -> Result<(BTreeMap<String, Price>, BTreeMap<String, Price>)> {
    let resp = http.get(url).timeout(std::time::Duration::from_secs(20)).send().await.with_context(|| format!("reading {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        bail!("{url} answered {status}");
    }
    let body = resp.text().await?;
    let (models, flex) = provider.parse(&body).with_context(|| format!("{}'s pricing page has changed", provider.label()))?;
    if models.len() < MIN_MODELS {
        bail!("{}'s pricing page has changed: it listed {}", provider.label(), crate::enrich::plural(models.len(), "model", "models"));
    }
    Ok((models, flex))
}

/// Read the prices again when they are a week old, or were never read,
/// unless a try failed in the last day. Errors are kept, not returned.
pub async fn refresh_if_stale(db: &Db, http: &reqwest::Client, provider: Provider, base: Option<&str>) {
    let now = crate::db::now();
    let fetched = current(db, provider).fetched_at;
    let failed_lately = tried(db, provider).is_some_and(|t| t.error.is_some() && now - t.at < RETRY_SECS);
    if fetched.is_none_or(|at| now - at >= STALE_SECS)
        && !failed_lately
        && let Err(e) = refresh(db, http, provider, base).await
    {
        tracing::warn!("could not read {}'s prices: {e:#}", provider.label());
    }
}

/// A provider's prices as the API shows them.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct PriceInfo {
    pub provider: Provider,
    pub label: &'static str,
    /// The official page, to link to.
    pub page_url: &'static str,
    /// When they were read. None: UpNext's built-in list, checked on
    /// `built_in_checked`, or no prices at all for this provider.
    pub fetched_at: Option<i64>,
    pub built_in_checked: Option<&'static str>,
    /// Why the last try to read them failed, and when it was.
    pub error: Option<String>,
    pub tried_at: Option<i64>,
    pub models: BTreeMap<String, Price>,
    /// OpenAI's Flex prices; left out when there are none.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub flex: BTreeMap<String, Price>,
}

pub fn info(db: &Db, provider: Provider) -> PriceInfo {
    let list = current(db, provider);
    let tried = tried(db, provider);
    PriceInfo {
        provider,
        label: provider.label(),
        page_url: provider.page_url(),
        fetched_at: list.fetched_at,
        built_in_checked: (list.fetched_at.is_none() && !list.models.is_empty()).then_some(BUILT_IN_CHECKED),
        error: tried.as_ref().and_then(|t| t.error.clone()),
        tried_at: tried.map(|t| t.at),
        models: list.models,
        flex: list.flex,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::fixture;

    fn usage(input: u64, read: u64, write: u64, write_1h: u64, output: u64) -> Usage {
        Usage {
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: read,
            cache_write_tokens: write,
            cache_write_1h_tokens: write_1h,
        }
    }

    /// A trimmed capture of the page on 2026-09-30 (with 2026-10-09's
    /// Haiku 5.5 and Sonnet 5.5 rows), with the batch table after it, which
    /// must not be read.
    #[test]
    fn anthropics_page() {
        let models = parse_anthropic(&fixture("prices/anthropic_pricing.md")).unwrap();
        assert_eq!(models.len(), 20, "{:?}", models.keys());
        let haiku = models["claude-haiku-5-5"];
        assert_eq!(
            haiku,
            Price { input: 0.10, output: 0.50, cache_read: Some(0.01), cache_write_5m: Some(0.125), cache_write_1h: Some(0.20) },
            "the price for prompts up to 100,000 tokens, not the row after it"
        );
        let opus = models["claude-opus-5-5"];
        assert_eq!(opus, Price { input: 4.0, output: 20.0, cache_read: Some(0.20), cache_write_5m: Some(5.0), cache_write_1h: Some(8.0) });
        assert_eq!(models["claude-fable-5-1"].cache_read, Some(0.25), "the footnote mark is left out");
        assert_eq!(models["claude-sonnet-5"].input, 2.0);
        assert!(models.contains_key("claude-mythos-5-1"), "the link after the name is left out");
        assert_eq!(models["claude-opus-4-1"].input, 15.0);

        // The page agrees with the built-in list on every model the list has.
        let built = built_in(Provider::Anthropic);
        for (id, p) in &built.models {
            let page = models[id];
            assert_eq!((page.input, page.output, page.cache_read), (p.input, p.output, p.cache_read), "{id}");
            assert_eq!((page.cache_write_5m, page.cache_write_1h), (Some(p.input * 1.25), Some(p.input * 2.0)), "{id}: the rule holds");
        }
    }

    #[test]
    fn openais_page() {
        let models = parse_openai(&fixture("prices/openai_pricing.md")).unwrap();
        let luna = models["gpt-5.6-luna"];
        assert_eq!(luna, Price { input: 0.20, output: 1.20, cache_read: Some(0.02), cache_write_5m: Some(0.25), cache_write_1h: None });
        assert_eq!(models["gpt-5.5"].input, 5.0, "the context-length note is left out");
        assert_eq!(models["gpt-5.5"].cache_write_5m, None, "\"-\" is no price");
        assert_eq!(models["gpt-5.6-sol"].input, 4.0, "the standard table, not the batch one after it");

        let flex = parse_openai_flex(&fixture("prices/openai_pricing.md"));
        assert_eq!(
            flex["gpt-6-luna"],
            Price { input: 0.05, output: 0.25, cache_read: Some(0.005), cache_write_5m: Some(0.0625), cache_write_1h: None }
        );
        let list = PriceList { provider: Provider::OpenAi, fetched_at: Some(1), models, flex };
        let u = usage(10_000, 2_000, 0, 0, 1_000);
        let (standard, at_flex) = (list.cost(Some("gpt-6-luna"), &u).unwrap(), list.cost_flex(Some("gpt-6-luna"), &u).unwrap());
        assert!((at_flex * 2.0 - standard).abs() < 1e-12, "half price: {at_flex} against {standard}");
        assert_eq!(list.get_flex("gpt-5.2").map(|p| p.input), Some(1.75), "not at Flex: the standard price");
        assert!(parse_openai_flex("### Standard pricing data\n").is_empty(), "a page without Flex prices");
    }

    #[test]
    fn openrouters_list() {
        let models = parse_openrouter(&fixture("prices/openrouter_models.json")).unwrap();
        let sonnet = models["anthropic/claude-sonnet-5.5"];
        assert!((sonnet.input - 2.0).abs() < 1e-9 && (sonnet.output - 10.0).abs() < 1e-9, "{sonnet:?}");
        assert!((sonnet.cache_write_1h.unwrap() - 4.0).abs() < 1e-9);
        assert_eq!(models["qwen/qwen3.8-27b:free"].input, 0.0);
        assert_eq!(models["qwen/qwen3.8-27b"].cache_write_5m, None);
    }

    #[test]
    fn a_changed_page_is_an_error() {
        assert!(parse_anthropic("# Pricing\n\nNothing here.").is_err());
        let renamed = fixture("prices/anthropic_pricing.md").replace("Base input tokens", "Input");
        assert!(parse_anthropic(&renamed).unwrap_err().to_string().contains("base input"));
        assert!(parse_openai("### Standard pricing data\n\n| Model | Input |\n| --- | --- |\n| x | $1 |").is_err());
        assert!(parse_openrouter("<html>").is_err());
    }

    /// The Claude CLI's own figure for a real call agrees with this one: the
    /// call wrote its 884 cached tokens for an hour.
    #[test]
    fn a_call_costs_what_the_cli_says() {
        let envelope = crate::testutil::fixture_json("llm/claude_cli_success.json");
        let u = Usage::from_anthropic(&envelope["usage"]).unwrap();
        assert_eq!(u.cache_write_1h_tokens, 884);
        let cost = built_in(Provider::Anthropic).cost(Some("claude-sonnet-5-5"), &u).unwrap();
        assert!((cost - envelope["total_cost_usd"].as_f64().unwrap()).abs() < 1e-9, "{cost}");

        // 1,000 uncached, 2,000 read, 3,000 written for five minutes, 4,000 for an hour, 500 out.
        let u = usage(10_000, 2_000, 7_000, 4_000, 500);
        let p = Price { input: 2.0, output: 10.0, cache_read: Some(0.2), cache_write_5m: None, cache_write_1h: None };
        let expected = (1_000.0 * 2.0 + 2_000.0 * 0.2 + 3_000.0 * 2.5 + 4_000.0 * 4.0 + 500.0 * 10.0) / 1e6;
        assert!((p.cost(&u) - expected).abs() < 1e-12);
    }

    #[test]
    fn models_are_found_by_their_id_or_a_longer_one() {
        let list = built_in(Provider::Anthropic);
        assert_eq!(list.get("claude-opus-5-5").map(|p| p.input), Some(4.0));
        assert_eq!(list.get("claude-opus-5-5-20261001").map(|p| p.input), Some(4.0), "a dated id");
        assert_eq!(list.get("claude-opus-5").map(|p| p.input), Some(5.0), "not Opus 5.5's");
        assert_eq!(list.get("claude-opus-50").map(|p| p.input), None);
        assert_eq!(list.get("sonnet"), None, "an alias isn't a model id");
        assert!(built_in(Provider::OpenAi).models.is_empty(), "no fallback for the others");
    }

    #[test]
    fn hosted_servers_by_their_address() {
        assert_eq!(Provider::for_base_url("https://api.openai.com/v1"), Some(Provider::OpenAi));
        assert_eq!(Provider::for_base_url("https://openrouter.ai/api/v1"), Some(Provider::OpenRouter));
        assert_eq!(Provider::for_base_url("http://localhost:11434/v1"), None);
        assert_eq!(Provider::for_base_url("https://api.groq.com/openai/v1"), None, "no prices UpNext can read");
    }

    /// Prices read from a mock page are kept with their date; a page that
    /// changed leaves them as they were and keeps the error.
    #[tokio::test]
    async fn read_kept_and_kept_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("upnext.db"), 120).unwrap();
        assert_eq!(current(&db, Provider::Anthropic).fetched_at, None, "the built-in list until a read");
        assert_eq!(info(&db, Provider::Anthropic).built_in_checked, Some(BUILT_IN_CHECKED));

        let server = wiremock::MockServer::start().await;
        let page = fixture("prices/anthropic_pricing.md").replace("| $4 / MTok             |", "| $3 / MTok             |");
        wiremock::Mock::given(wiremock::matchers::path("/pricing.md"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(page))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        let (http, url) = (reqwest::Client::new(), format!("{}/pricing.md", server.uri()));
        let list = refresh_from(&db, &http, Provider::Anthropic, &url).await.unwrap();
        assert!(list.fetched_at.is_some());
        assert_eq!(
            current(&db, Provider::Anthropic).get("claude-opus-5-5").map(|p| p.input),
            Some(3.0),
            "the page's price, not the built-in one"
        );

        wiremock::Mock::given(wiremock::matchers::path("/pricing.md"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string("# Pricing"))
            .mount(&server)
            .await;
        assert!(refresh_from(&db, &http, Provider::Anthropic, &url).await.is_err());
        let shown = info(&db, Provider::Anthropic);
        assert_eq!(shown.models["claude-opus-5-5"].input, 3.0, "kept from the last read");
        assert!(shown.error.as_deref().is_some_and(|e| e.contains("changed")), "{:?}", shown.error);
    }
}
