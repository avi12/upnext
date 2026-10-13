//! A subscription's usage bars, and what UpNext's AI takes of them.
//!
//! UpNext doesn't know a subscription's limits: plans don't say how many
//! tokens they allow, and the CLIs only report each limit as a whole
//! percent used ("Current session: 20% used · resets 6:30pm" from the
//! Claude CLI, `usedPercent: 20` from the Codex CLI). Reading them spends
//! nothing (`claude -p /usage` answers locally, `codex app-server` reads
//! the account's usage without calling a model), so UpNext reads them
//! around its own calls and measures what its calls take:
//!
//! - A reading is taken before an AI call when the last one is a minute
//!   old, and after each call, in the background. During a job's parallel
//!   calls one reading runs at a time, and one more follows when calls came
//!   back while it ran. Each reading records the newest call before it.
//! - A whole-percent bar can be almost a point below the true value, so a
//!   measurement runs from one tick of a bar to the next: the calls
//!   between the reading where it ticked up and the reading where it ticks
//!   again used that many points. A reset in between (the percent drops,
//!   or the reset time moves on) throws the span away. The CLI's reset time
//!   wobbles by a minute between reads ("11:29pm", then "11:30pm"), so it
//!   has to move by more than an hour to count.
//! - Other use (claude.ai, Claude Code) moves the same bars, which makes
//!   UpNext's calls look dearer, never cheaper. So a span counts only when
//!   it's short and UpNext's calls fill it, and the figure is the largest
//!   per point of the recent spans, or the median of the top three.
//! - Spans are worked out from the readings and the calls whenever the
//!   figure is asked for (`spans` is a pure function), so they survive
//!   restarts and get better with use. A change of plan starts over.

use crate::config::Config;
use crate::db::{AiKind, CallUse, Db, PlanReading, PlanWindow};
use crate::llm::Usage;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// A reading is taken before a call when the last one is older than this.
const BEFORE_SECS: i64 = 60;
/// The AI usage view reads again when the last reading is older than this.
const FRESH_SECS: i64 = 30;
/// A span counts only when UpNext's calls fill it: no gap between them
/// longer than this,
const GAP_MAX_SECS: i64 = 10 * 60;
/// and it lasts no longer than this.
const SPAN_MAX_SECS: i64 = 3 * 3600;
/// The figure is made from this many of the newest spans, at most,
const RECENT_SPANS: usize = 12;
/// over the readings of this many days.
const HISTORY_DAYS: i64 = 90;
/// A window's reset time must move by more than this to be a reset.
const RESET_MOVE_MINUTES: i64 = 60;
/// Spans needed before the steadier of the two measures is chosen.
const SPANS_TO_CHOOSE: usize = 4;
/// The week's bar must have moved this many points before its figure is
/// worked out from the session's.
const MIN_WEEK_MOVES: i64 = 2;

// -- reading ----------------------------------------------------------------------

/// How to read a subscription's usage bars, and the AI account they are
/// read for: each account's readings are its own, so two players
/// on two subscriptions each see their own bars. Its Debug never shows
/// the token.
#[derive(Clone)]
pub enum Meter {
    /// The Claude CLI's `/usage`, with an AI account's own token when it has one.
    Claude { bin: String, workdir: Option<PathBuf>, token: Option<String>, account: Option<i64> },
    /// The Codex CLI's app-server, `account/rateLimits/read`, in an AI
    /// account's own `CODEX_HOME` when it has one.
    Codex { bin: String, home: Option<PathBuf>, account: Option<i64> },
}

impl std::fmt::Debug for Meter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Meter::Claude { bin, workdir, token, account } => f
                .debug_struct("Claude")
                .field("bin", bin)
                .field("workdir", workdir)
                .field("token", &token.as_ref().map(|_| "<set>"))
                .field("account", account)
                .finish(),
            Meter::Codex { bin, home, account } => {
                f.debug_struct("Codex").field("bin", bin).field("home", home).field("account", account).finish()
            }
        }
    }
}

impl Meter {
    /// The backend whose calls it measures.
    pub fn backend(&self) -> &'static str {
        match self {
            Meter::Claude { .. } => "claude-cli",
            Meter::Codex { .. } => "codex-cli",
        }
    }

    /// The AI account whose bars it reads; None outside the players' accounts.
    pub fn account(&self) -> Option<i64> {
        match self {
            Meter::Claude { account, .. } | Meter::Codex { account, .. } => *account,
        }
    }

    /// The same meter, for `id`'s readings.
    pub fn for_account(mut self, id: Option<i64>) -> Self {
        match &mut self {
            Meter::Claude { account, .. } | Meter::Codex { account, .. } => *account = id,
        }
        self
    }

    /// The limits, and the plan they're on ("max", "plus") when the CLI says.
    async fn read(&self) -> Result<(Vec<PlanWindow>, Option<String>), ReadError> {
        match self {
            Meter::Claude { bin, workdir, token, .. } => {
                let (envelope, plan) = tokio::join!(
                    crate::llm::claude_cli::usage_envelope(bin, workdir.as_deref(), token.as_deref()),
                    crate::llm::claude_cli::auth_plan(bin, workdir.as_deref(), token.as_deref())
                );
                Ok((parse_claude(&envelope.map_err(ReadError::Failed)?)?, plan))
            }
            Meter::Codex { bin, home, .. } => {
                let limits = crate::llm::codex_cli::in_home(home.clone(), crate::llm::codex_cli::rate_limits(bin)).await;
                parse_codex(&limits.map_err(ReadError::Failed)?)
            }
        }
    }

    async fn version(&self) -> Option<String> {
        match self {
            Meter::Claude { bin, .. } => crate::llm::claude_cli::version(bin).await,
            Meter::Codex { bin, .. } => crate::llm::codex_cli::version(bin).await,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    /// The CLI couldn't be run, or failed.
    #[error("{0}")]
    Failed(String),
    /// It answered, but not in a way UpNext can read.
    #[error("{0}")]
    Unreadable(String),
    /// It sent `/usage` to the model instead of answering it, which spends
    /// usage. UpNext stops reading until the CLI changes.
    #[error("{0}")]
    Spent(String),
    /// Reading was stopped after `Spent`, for this version of the CLI.
    #[error("{0}")]
    Stopped(String),
}

/// What a `/usage` envelope says. It must be the CLI's own answer, which
/// costs nothing: `local_command` "usage" and no turns with the model.
pub fn parse_claude(envelope: &Value) -> Result<Vec<PlanWindow>, ReadError> {
    let local = envelope.get("local_command").and_then(Value::as_str) == Some("usage");
    let turns = envelope.get("num_turns").and_then(Value::as_u64);
    if !local || turns != Some(0) {
        return Err(ReadError::Spent(format!(
            "the Claude CLI sent \"/usage\" to the model instead of answering it itself ({} turns), which spends usage",
            turns.map_or("unknown".into(), |t| t.to_string())
        )));
    }
    let text = envelope.get("result").and_then(Value::as_str).unwrap_or_default();
    if envelope.get("is_error").and_then(Value::as_bool) == Some(true) {
        return Err(ReadError::Failed(format!("the Claude CLI said: {}", text.trim())));
    }
    let windows = parse_windows(text);
    if windows.is_empty() {
        let first = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("nothing");
        return Err(ReadError::Unreadable(format!("couldn't read your plan's usage: the Claude CLI said \"{first}\"")));
    }
    Ok(windows)
}

/// The limit lines of `/usage`: "Current session: 20% used · resets 6:30pm
/// (America/New_York)". A line that counts down ("80% left", "remaining") is
/// turned into percent used. A line whose direction can't be told is
/// dropped, not guessed: before the conversion, every tick would look like
/// a reset.
pub fn parse_windows(text: &str) -> Vec<PlanWindow> {
    let mut out: Vec<PlanWindow> = vec![];
    for line in text.lines().map(str::trim) {
        let Some((label, rest)) = line.split_once(": ") else { continue };
        let Some(name) = label.strip_prefix("Current ") else { continue };
        let Some((number, after)) = rest.split_once('%') else { continue };
        let Ok(n) = number.trim().parse::<f64>() else { continue };
        let after = after.trim_start().to_lowercase();
        let used = if after.starts_with("used") {
            n
        } else if after.starts_with("left") || after.starts_with("remaining") {
            100.0 - n
        } else {
            continue;
        };
        let resets = rest.split_once("resets ").map(|(_, r)| r.trim().to_string()).filter(|r| !r.is_empty());
        let id = match name.split_once(" (") {
            Some((base, inner)) => {
                let inner = inner.trim_end_matches(')');
                if inner == "all models" { base.to_lowercase() } else { format!("{}:{inner}", base.to_lowercase()) }
            }
            None => name.to_lowercase(),
        };
        if !out.iter().any(|w| w.id == id) {
            out.push(PlanWindow { id, label: label.to_string(), used: used.clamp(0.0, 100.0), resets, resets_at: None });
        }
    }
    out
}

/// What `account/rateLimits/read` says: the limits and the plan ("plus").
/// Each bucket (`codex`, and any other, such as one model's own) has up
/// to two windows, told apart by their length, not by slot: 300 minutes is
/// the session, 10,080 the week. A slot that is null is a limit the plan
/// doesn't have. Another bucket's windows get its name after a colon, as
/// the Claude CLI's per-model week does ("week:GPT-6-Astra").
pub fn parse_codex(result: &Value) -> Result<(Vec<PlanWindow>, Option<String>), ReadError> {
    let unreadable = |why: &str| ReadError::Unreadable(format!("couldn't read your plan's usage: `codex app-server` {why}"));
    let single = result.get("rateLimits").filter(|v| v.is_object()).ok_or_else(|| unreadable("answered without rateLimits"))?;
    let mut buckets: Vec<(&str, &Value)> = match result.get("rateLimitsByLimitId").and_then(Value::as_object) {
        Some(by_id) if !by_id.is_empty() => by_id.iter().map(|(k, v)| (k.as_str(), v)).collect(),
        _ => vec![("codex", single)],
    };
    buckets.sort_by_key(|(id, _)| (*id != "codex", *id));
    let mut out: Vec<PlanWindow> = vec![];
    for (bucket_id, bucket) in buckets {
        if bucket.get("primary").is_none() && bucket.get("secondary").is_none() {
            return Err(unreadable(&format!("sent a limit without windows ({bucket_id})")));
        }
        let text = |k: &str| bucket.get(k).and_then(Value::as_str).filter(|s| !s.is_empty());
        let name = (bucket_id != "codex").then(|| text("limitName").or(text("normalModelSlug")).unwrap_or(bucket_id));
        for slot in ["primary", "secondary"] {
            let Some(w) = bucket.get(slot).filter(|w| !w.is_null()) else { continue };
            let used = w
                .get("usedPercent")
                .and_then(Value::as_f64)
                .ok_or_else(|| unreadable(&format!("sent a window without usedPercent ({bucket_id})")))?;
            let mins = w.get("windowDurationMins").and_then(Value::as_i64);
            let (id, label) = match mins {
                Some(300) => ("session".to_string(), "5-hour limit".to_string()),
                Some(10_080) => ("week".to_string(), "Weekly limit".to_string()),
                Some(m) if m > 0 && m % 1440 == 0 => (format!("{m}m"), format!("{}-day limit", m / 1440)),
                Some(m) if m > 0 && m % 60 == 0 => (format!("{m}m"), format!("{}-hour limit", m / 60)),
                Some(m) => (format!("{m}m"), format!("{m}-minute limit")),
                None => (slot.to_string(), "Usage limit".to_string()),
            };
            let (id, label) = match name {
                Some(n) => (format!("{id}:{n}"), format!("{label} ({n})")),
                None => (id, label),
            };
            if !out.iter().any(|x| x.id == id) {
                let resets_at = w.get("resetsAt").and_then(Value::as_i64);
                out.push(PlanWindow { id, label, used: used.clamp(0.0, 100.0), resets: None, resets_at });
            }
        }
    }
    let plan = single
        .get("planType")
        .and_then(Value::as_str)
        .or_else(|| result.pointer("/rateLimitsByLimitId/codex/planType").and_then(Value::as_str))
        .filter(|p| !p.is_empty() && *p != "unknown")
        .map(str::to_string);
    Ok((out, plan))
}

/// Kept when reading had to stop: why, and for which CLI version.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Stopped {
    version: Option<String>,
    why: String,
    at: i64,
}

const STOPPED_KEY: &str = "plan_usage.stopped";
const ERROR_KEY: &str = "plan_usage.error";

/// The meta key for `base` (`STOPPED_KEY`, `ERROR_KEY`) of one meter: its
/// backend and AI account, as its readings are kept. With one key for every
/// meter, a Codex reading compared Codex's version with the Claude CLI's
/// stop, found them different and cleared it, so the Claude CLI was asked
/// for `/usage` again and spent usage on it.
fn meter_key(base: &str, meter: &Meter) -> String {
    match meter.account() {
        Some(account) => format!("{base}.{}.{account}", meter.backend()),
        None => format!("{base}.{}", meter.backend()),
    }
}

/// Read the bars now and keep the reading. A CLI that spends usage on it
/// is caught, and not asked again until it's a different version.
pub async fn take_reading(db: &Db, meter: &Meter) -> Result<PlanReading, ReadError> {
    let (stopped_key, error_key) = (meter_key(STOPPED_KEY, meter), meter_key(ERROR_KEY, meter));
    let stopped: Option<Stopped> = db.meta_get(&stopped_key).ok().flatten().and_then(|s| serde_json::from_str(&s).ok());
    if let Some(s) = stopped {
        if meter.version().await == s.version {
            return Err(ReadError::Stopped(format!("UpNext stopped reading your plan's usage: {}", s.why)));
        }
        let _ = db.meta_set(&stopped_key, "null");
    }
    let at = crate::db::now();
    let last_call = db.ai_last_call_id().map_err(|e| ReadError::Failed(format!("{e:#}")))?;
    let (windows, plan) = match meter.read().await {
        Ok(w) => w,
        Err(e) => {
            if let ReadError::Spent(why) = &e {
                tracing::warn!("{why}; not reading the plan's usage again until the CLI changes");
                let s = Stopped { version: meter.version().await, why: why.clone(), at };
                let _ = db.meta_set(&stopped_key, &serde_json::to_string(&s).unwrap_or_default());
            }
            let _ = db.meta_set(&error_key, &e.to_string());
            return Err(e);
        }
    };
    let mut reading =
        PlanReading { id: 0, at, backend: meter.backend().to_string(), ai_account_id: meter.account(), plan, last_call, windows };
    reading.id = db.plan_reading_add(&reading).map_err(|e| ReadError::Failed(format!("{e:#}")))?;
    let _ = db.meta_set(&error_key, "");
    Ok(reading)
}

/// Before an AI call: read in the background when the last reading is
/// more than a minute old, so the call's effect has a reading on each side.
pub fn before_call(db: &Db, meter: &Meter) {
    let stale = match db.plan_reading_latest(meter.backend(), meter.account()) {
        Ok(latest) => latest.is_none_or(|r| crate::db::now() - r.at >= BEFORE_SECS),
        Err(_) => false,
    };
    if stale {
        in_background(db, meter);
    }
}

/// After an AI call: read in the background. One reading runs at a time;
/// calls that come back while it runs get one more after it.
pub fn after_call(db: &Db, meter: &Meter) {
    in_background(db, meter);
}

fn in_background(db: &Db, meter: &Meter) {
    let key = (meter.backend(), meter.account());
    if !db.readings.start(key) {
        return;
    }
    let (db, meter) = match db.reopen() {
        Ok(again) => (again, meter.clone()),
        Err(e) => {
            tracing::warn!("could not read the plan's usage: {e:#}");
            db.readings.again(key);
            return;
        }
    };
    tokio::spawn(async move {
        loop {
            if let Err(e) = take_reading(&db, &meter).await
                && !matches!(e, ReadError::Stopped(_))
            {
                tracing::debug!("plan usage: {e}");
            }
            if !db.readings.again(key) {
                break;
            }
        }
    });
}

/// Wait for the background reading to finish (at most 30 s), so a
/// terminal job's last calls are read before the process exits.
pub async fn settle(db: &Db) {
    for _ in 0..150 {
        if !db.readings.busy() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

// -- measuring ----------------------------------------------------------------------

/// What a span is measured in. Limits may weigh input, output, cache reads
/// and writes, and models differently: the API price weighs them the way
/// the price does, raw tokens weigh them all the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Measure {
    /// US dollars at the API's list prices.
    ApiUsd,
    /// Tokens in and out.
    Tokens,
}

/// From one tick of a window's bar to the next, and UpNext's calls in it.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub window: String,
    /// The whole percents it ran between.
    pub from: i64,
    pub to: i64,
    /// The readings it ran between.
    pub start: i64,
    pub end: i64,
    pub calls: usize,
    pub usage: Usage,
    pub by_model: BTreeMap<String, Usage>,
    /// None when a call's model had no known price.
    pub api_usd: Option<f64>,
    pub kinds: Vec<AiKind>,
    /// Short, and filled by UpNext's calls (see the module notes).
    pub usable: bool,
}

impl Span {
    /// What one point of the window took, in `m`.
    pub fn per_point(&self, m: Measure) -> Option<f64> {
        let points = (self.to - self.from) as f64;
        match m {
            Measure::ApiUsd => self.api_usd.map(|u| u / points),
            Measure::Tokens => Some(self.usage.total() as f64 / points),
        }
    }
}

fn points(w: &PlanWindow) -> i64 {
    w.used.floor() as i64
}

/// A reset time as the CLI writes it ("Sep 30, 11:30pm (America/New_York)",
/// "Oct 4, 3pm") in minutes from the start of the year, near enough to
/// compare two readings: every month counts 31 days.
fn reset_minutes(text: &str) -> Option<i64> {
    const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let text = text.split(" (").next()?.to_lowercase();
    let mut words = text.split([' ', ',']).filter(|w| !w.is_empty());
    let month = MONTHS.iter().position(|m| words.clone().next().is_some_and(|w| w.starts_with(m)))? as i64;
    words.next();
    let day: i64 = words.next()?.parse().ok()?;
    let time = words.next()?;
    let (clock, pm) = match (time.strip_suffix("am"), time.strip_suffix("pm")) {
        (Some(c), _) => (c, false),
        (_, Some(c)) => (c, true),
        _ => return None,
    };
    let (hour, minute) = match clock.split_once(':') {
        Some((h, m)) => (h.parse::<i64>().ok()?, m.parse::<i64>().ok()?),
        None => (clock.parse::<i64>().ok()?, 0),
    };
    let hour = hour % 12 + if pm { 12 } else { 0 };
    Some(((month * 31 + day) * 24 + hour) * 60 + minute)
}

/// Whether a window reset between two readings: its percent dropped, or
/// its reset time moved on by more than the Claude CLI's wobble.
fn reset_between(before: &PlanWindow, after: &PlanWindow) -> bool {
    if points(after) < points(before) {
        return true;
    }
    match (before.resets_at, after.resets_at) {
        (Some(x), Some(y)) => return (y - x).abs() > RESET_MOVE_MINUTES * 60,
        (None, None) => {}
        _ => return true,
    }
    match (before.resets.as_deref(), after.resets.as_deref()) {
        (None, None) => false,
        (Some(a), Some(b)) => match (reset_minutes(a), reset_minutes(b)) {
            (Some(x), Some(y)) => (y - x).abs() > RESET_MOVE_MINUTES,
            _ => a != b,
        },
        _ => true,
    }
}

/// Every span in `readings` (one backend's, oldest first), for each window,
/// with `calls` (that backend's, oldest first).
pub fn spans(readings: &[PlanReading], calls: &[CallUse]) -> Vec<Span> {
    let mut ids: Vec<&str> = vec![];
    for w in readings.iter().flat_map(|r| &r.windows) {
        if !ids.contains(&w.id.as_str()) {
            ids.push(&w.id);
        }
    }
    let mut out = vec![];
    for id in ids {
        // The last reading with this window, and the reading where it last
        // ticked up with the one before that.
        let mut prev: Option<(usize, &PlanWindow)> = None;
        let mut anchor: Option<(usize, usize, i64)> = None;
        for (i, r) in readings.iter().enumerate() {
            if i > 0 && r.plan != readings[i - 1].plan {
                (prev, anchor) = (None, None);
            }
            let Some(w) = r.window(id) else { continue };
            let now = points(w);
            if let Some((pi, before)) = prev {
                if reset_between(before, w) {
                    anchor = None;
                } else if now > points(before) {
                    if let Some((ai, bi, from)) = anchor {
                        out.push(span(id, readings, calls, bi, ai, i, from, now));
                    }
                    anchor = Some((i, pi, now));
                }
            }
            prev = Some((i, w));
        }
    }
    out
}

/// The span from the tick at reading `ai` (the reading before it is `bi`)
/// to the tick at reading `ci`.
#[allow(clippy::too_many_arguments)]
fn span(window: &str, readings: &[PlanReading], calls: &[CallUse], bi: usize, ai: usize, ci: usize, from: i64, to: i64) -> Span {
    let (a, c) = (&readings[ai], &readings[ci]);
    let inside: Vec<&CallUse> = calls.iter().filter(|x| x.id > a.last_call && x.id <= c.last_call).collect();
    let mut usage = Usage::default();
    let mut by_model: BTreeMap<String, Usage> = BTreeMap::new();
    let mut api_usd = Some(0.0);
    let mut kinds = vec![];
    // When something happened: the ticks' readings, and each call's start and end.
    let mut times = vec![readings[bi].at, a.at, c.at];
    for x in &inside {
        usage += x.usage;
        *by_model.entry(x.model.clone().unwrap_or_default()).or_default() += x.usage;
        api_usd = api_usd.zip(x.api_usd).map(|(s, u)| s + u);
        if !kinds.contains(&x.kind) {
            kinds.push(x.kind);
        }
        times.push(x.at - (x.duration_ms / 1000) as i64);
        times.push(x.at);
    }
    times.sort_unstable();
    let gap = times.windows(2).map(|p| p[1] - p[0]).max().unwrap_or(0);
    let usable = !inside.is_empty() && gap <= GAP_MAX_SECS && c.at - a.at <= SPAN_MAX_SECS;
    Span { window: window.to_string(), from, to, start: a.at, end: c.at, calls: inside.len(), usage, by_model, api_usd, kinds, usable }
}

/// The figure from spans' values per point: the largest, which other use
/// lowers least, or the median of the top three once there are three.
fn figure(mut values: Vec<f64>) -> Option<f64> {
    values.sort_by(|a, b| b.total_cmp(a));
    match values.len() {
        0 => None,
        1 | 2 => Some(values[0]),
        _ => Some(values[1]),
    }
}

/// How much spans' values per point vary for `m`: the coefficient of variation.
fn spread(spans: &[&Span], m: Measure) -> Option<f64> {
    let v: Vec<f64> = spans.iter().map(|s| s.per_point(m)).collect::<Option<_>>()?;
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    let var = v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / v.len() as f64;
    (mean > 0.0).then(|| var.sqrt() / mean)
}

/// The measure to use: once there are a few usable spans, whichever gives
/// steadier numbers; before that, the API price when every span has one.
pub fn choose_measure(spans: &[&Span]) -> Measure {
    let priced = !spans.is_empty() && spans.iter().all(|s| s.api_usd.is_some());
    if !priced {
        return Measure::Tokens;
    }
    if spans.len() >= SPANS_TO_CHOOSE
        && let (Some(usd), Some(tokens)) = (spread(spans, Measure::ApiUsd), spread(spans, Measure::Tokens))
        && tokens < usd
    {
        return Measure::Tokens;
    }
    Measure::ApiUsd
}

/// How many points the `of` window moves for each point of `per` (the
/// session's for one of the week's), over pairs of readings where neither
/// reset. None until the `per` window has moved `MIN_WEEK_MOVES` points.
pub fn move_ratio(readings: &[PlanReading], of: &str, per: &str) -> Option<f64> {
    let (mut moved_of, mut moved_per) = (0i64, 0i64);
    for pair in readings.windows(2) {
        let (r0, r1) = (&pair[0], &pair[1]);
        if r0.plan != r1.plan {
            continue;
        }
        let (Some(a0), Some(a1), Some(b0), Some(b1)) = (r0.window(of), r1.window(of), r0.window(per), r1.window(per)) else { continue };
        if !reset_between(a0, a1) && !reset_between(b0, b1) {
            moved_of += points(a1) - points(a0);
            moved_per += points(b1) - points(b0);
        }
    }
    (moved_per >= MIN_WEEK_MOVES).then(|| moved_of as f64 / moved_per as f64)
}

// -- what the API shows ------------------------------------------------------------

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ReadingView {
    pub at: i64,
    pub windows: Vec<PlanWindow>,
}

/// What one point of a window takes, as measured.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct WindowFigure {
    pub id: String,
    pub label: String,
    /// One point of the window, in the measure.
    pub per_point: f64,
    /// The usable spans behind it; 0 when it's worked out from the session's.
    pub spans: usize,
    /// Points of the session for each point of this window, when the figure
    /// is the session's times that.
    pub from_session: Option<f64>,
}

/// What one of something takes of each window, in percent.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct Share {
    pub kind: AiKind,
    /// Picks, decisions, titles or series.
    pub items: u64,
    /// By window id.
    pub percent: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct Measured {
    pub measure: Measure,
    /// The usable spans behind the figures, over every window.
    pub spans: usize,
    pub windows: Vec<WindowFigure>,
    /// One pick, one Help me decide, one title of summaries, one series guide.
    pub each: Vec<Share>,
    /// The summaries still to write, when there are any.
    pub one_time: Option<Share>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct PlanSummary {
    pub backend: String,
    /// Whether the AI runs on a subscription that UpNext can read.
    pub subscription: bool,
    /// "max", "pro": what the CLI says. Not the tier.
    pub plan: Option<String>,
    pub reading: Option<ReadingView>,
    /// Why the last reading failed.
    pub error: Option<String>,
    /// What UpNext has measured; None until a span is usable.
    pub measured: Option<Measured>,
}

/// The plan's bars, read now unless a reading is under 30 s old, and
/// what UpNext's AI takes of them.
pub async fn summary(db: &Db, cfg: &Config) -> Result<PlanSummary> {
    // The picks' subscription, or the summaries' when only they are on one.
    let picks = crate::llm::backend_from_config(cfg);
    let llm = if picks.plan_meter().is_none() && crate::llm::summaries_apart(cfg) { crate::llm::summary_backend(cfg) } else { picks };
    let backend = llm.name().to_string();
    let Some(meter) = llm.plan_meter() else {
        return Ok(PlanSummary { backend, subscription: false, plan: None, reading: None, error: None, measured: None });
    };
    let account = meter.account();
    let latest = db.plan_reading_latest(meter.backend(), account)?;
    let fresh = latest.as_ref().is_some_and(|r| crate::db::now() - r.at < FRESH_SECS);
    let (reading, error) = if fresh {
        (latest, None)
    } else {
        match take_reading(db, &meter).await {
            Ok(r) => (Some(r), None),
            Err(e) => (None, Some(e.to_string())),
        }
    };
    let plan = reading.as_ref().and_then(|r| r.plan.clone()).or_else(|| latest_plan(db, meter.backend(), account));
    let measured = measured(db, cfg, meter.backend(), account, &plan)?;
    Ok(PlanSummary {
        backend,
        subscription: true,
        plan,
        reading: reading.map(|r| ReadingView { at: r.at, windows: r.windows }),
        error,
        measured,
    })
}

fn latest_plan(db: &Db, backend: &str, account: Option<i64>) -> Option<String> {
    db.plan_reading_latest(backend, account).ok().flatten().and_then(|r| r.plan)
}

/// The figures from this backend's readings on `plan` over the last 90
/// days, on one AI account's subscription with that account's calls.
fn measured(db: &Db, cfg: &Config, backend: &str, account: Option<i64>, plan: &Option<String>) -> Result<Option<Measured>> {
    let since = crate::db::now() - HISTORY_DAYS * 86_400;
    let readings: Vec<PlanReading> = db.plan_readings(backend, account, since)?.into_iter().filter(|r| &r.plan == plan).collect();
    let Some(first) = readings.first() else { return Ok(None) };
    let calls = db.ai_call_uses(backend, account, first.last_call)?;
    let all = spans(&readings, &calls);
    let mut usable: Vec<&Span> = all.iter().filter(|s| s.usable).collect();
    usable.sort_by_key(|s| std::cmp::Reverse(s.end));
    if usable.is_empty() {
        return Ok(None);
    }
    let measure = choose_measure(&usable);
    let latest = readings.last().expect("a reading");

    let mut windows: Vec<WindowFigure> = vec![];
    for w in &latest.windows {
        let mine: Vec<f64> = usable.iter().filter(|s| s.window == w.id).take(RECENT_SPANS).filter_map(|s| s.per_point(measure)).collect();
        let n = mine.len();
        if let Some(per_point) = figure(mine) {
            windows.push(WindowFigure { id: w.id.clone(), label: w.label.clone(), per_point, spans: n, from_session: None });
        }
    }
    // The week's bar moves too slowly to measure on picks: work it out from
    // the session's, by how far each moved over the same readings.
    if !windows.iter().any(|f| f.id == "week")
        && let Some(session) = windows.iter().find(|f| f.id == "session").map(|f| f.per_point)
        && let Some(week) = latest.window("week")
        && let Some(ratio) = move_ratio(&readings, "session", "week")
        && ratio > 0.0
    {
        windows.push(WindowFigure {
            id: week.id.clone(),
            label: week.label.clone(),
            per_point: session * ratio,
            spans: 0,
            from_session: Some(ratio),
        });
    }
    if windows.is_empty() {
        return Ok(None);
    }

    // What one of each takes, in the measure: this install's own average
    // per item when its calls were priced, else the token rate.
    let estimates = crate::ai_usage::estimates(db, cfg)?;
    let usd_per_token = {
        let (usd, tokens) = usable.iter().fold((0.0, 0u64), |(u, t), s| (u + s.api_usd.unwrap_or(0.0), t + s.usage.total()));
        (tokens > 0).then(|| usd / tokens as f64)
    };
    let size = |rate: &crate::ai_usage::Rate| -> Option<f64> {
        let tokens = (rate.input_tokens + rate.output_tokens) as f64;
        match measure {
            Measure::Tokens => Some(tokens),
            Measure::ApiUsd => rate.api_usd.or_else(|| usd_per_token.map(|u| u * tokens)),
        }
    };
    let share = |kind: AiKind, items: u64, one: f64| Share {
        kind,
        items,
        percent: windows.iter().map(|w| (w.id.clone(), one * items as f64 / w.per_point)).collect(),
    };
    let mut each = vec![];
    let mut one_time = None;
    for f in &estimates.features {
        let Some(one) = size(&f.rate) else { continue };
        each.push(share(f.kind, 1, one));
        if f.kind == AiKind::ReviewSummaries
            && let Some(t) = f.one_time.as_ref().filter(|t| t.items > 0)
        {
            one_time = Some(share(f.kind, t.items, one));
        }
    }
    Ok(Some(Measured { measure, spans: usable.len(), windows, each, one_time }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_meters_debug_never_shows_its_token() {
        let meter = super::Meter::Claude { bin: "claude".into(), workdir: None, token: Some("not-a-real-token".into()), account: Some(2) };
        let shown = format!("{meter:?}");
        assert!(!shown.contains("not-a-real-token"), "{shown}");
        assert!(shown.contains("<set>") && shown.contains("account: Some(2)"), "{shown}");
    }

    use super::*;
    use crate::testutil::fixture_json;

    #[test]
    fn the_usage_text_is_read() {
        let windows = parse_claude(&fixture_json("llm/claude_cli_usage_command.json")).unwrap();
        let ids: Vec<&str> = windows.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["session", "week", "week:Fable"]);
        assert_eq!(windows[0].used, 20.0);
        assert_eq!(windows[0].resets.as_deref(), Some("Sep 30, 6:30pm (America/New_York)"));
        assert_eq!(windows[1].label, "Current week (all models)");
        assert_eq!(windows[2].used, 10.0);
    }

    /// Made by hand from the capture: the same limits, counting down.
    #[test]
    fn a_count_down_is_turned_round() {
        let windows = parse_claude(&fixture_json("llm/claude_cli_usage_left.json")).unwrap();
        let used: Vec<(&str, f64)> = windows.iter().map(|w| (w.id.as_str(), w.used)).collect();
        assert_eq!(used, [("session", 20.0), ("week", 40.0)], "80% left is 20% used; a line that says neither is dropped");
        let text = "Current session: 12% · resets 4pm\nCurrent week (all models): 3% used";
        assert_eq!(parse_windows(text).len(), 1, "no direction, no window");
    }

    #[test]
    fn an_answer_from_the_model_is_caught() {
        let mut spent = fixture_json("llm/claude_cli_usage_command.json");
        spent["num_turns"] = 1.into();
        assert!(matches!(parse_claude(&spent), Err(ReadError::Spent(_))));
        let mut spent = fixture_json("llm/claude_cli_usage_command.json");
        spent.as_object_mut().unwrap().remove("local_command");
        assert!(matches!(parse_claude(&spent), Err(ReadError::Spent(_))));

        let mut changed = fixture_json("llm/claude_cli_usage_command.json");
        changed["result"] = "Usage is now shown at claude.ai/settings".into();
        let err = parse_claude(&changed).unwrap_err();
        assert!(matches!(err, ReadError::Unreadable(_)) && err.to_string().contains("couldn't read"), "{err}");
    }

    /// The app-server's reply on a Plus plan: the windows by their length,
    /// with numeric reset times.
    #[test]
    fn the_codex_limits_are_read() {
        let reply = fixture_json("llm/codex_app_server_rate_limits.json");
        let (windows, plan) = parse_codex(&reply["result"]).unwrap();
        assert_eq!(plan.as_deref(), Some("plus"));
        let got: Vec<(&str, &str, f64, Option<i64>)> =
            windows.iter().map(|w| (w.id.as_str(), w.label.as_str(), w.used, w.resets_at)).collect();
        assert_eq!(got, [("session", "5-hour limit", 1.0, Some(1_790_826_486)), ("week", "Weekly limit", 0.0, Some(1_791_413_286))]);
        assert!(windows.iter().all(|w| w.resets.is_none()));
    }

    /// Made up from the protocol's schema: the slots swapped, a plan with no
    /// 5-hour limit, and a second bucket for one model.
    #[test]
    fn codex_windows_go_by_length_not_slot() {
        let result = serde_json::json!({
            "rateLimits": {"limitId": "codex", "planType": "pro", "primary": null,
                "secondary": {"usedPercent": 12, "windowDurationMins": 10080, "resetsAt": 1_791_413_286}},
            "rateLimitsByLimitId": {
                "gpt-6-astra": {"limitId": "gpt-6-astra", "limitName": "GPT-6-Astra", "planType": "pro",
                    "primary": {"usedPercent": 40, "windowDurationMins": 10080, "resetsAt": 1}, "secondary": null},
                "codex": {"limitId": "codex", "planType": "pro", "primary": null,
                    "secondary": {"usedPercent": 12, "windowDurationMins": 10080, "resetsAt": 1_791_413_286}},
            },
        });
        let (windows, plan) = parse_codex(&result).unwrap();
        assert_eq!(plan.as_deref(), Some("pro"));
        let got: Vec<(&str, &str, f64)> = windows.iter().map(|w| (w.id.as_str(), w.label.as_str(), w.used)).collect();
        assert_eq!(got, [("week", "Weekly limit", 12.0), ("week:GPT-6-Astra", "Weekly limit (GPT-6-Astra)", 40.0)]);

        let day = serde_json::json!({"rateLimits": {"planType": "unknown", "primary": {"usedPercent": 3, "windowDurationMins": 1440}}});
        let (windows, plan) = parse_codex(&day).unwrap();
        assert_eq!((windows[0].id.as_str(), windows[0].label.as_str(), plan), ("1440m", "1-day limit", None));
    }

    #[test]
    fn an_unknown_codex_answer_is_not_read_as_zeros() {
        for result in [
            serde_json::json!({"limits": []}),
            serde_json::json!({"rateLimits": {"planType": "plus", "fiveHour": {"usedPercent": 3}}}),
            serde_json::json!({"rateLimits": {"primary": {"percentUsed": 3, "windowDurationMins": 300}}}),
        ] {
            let err = parse_codex(&result).unwrap_err();
            assert!(matches!(err, ReadError::Unreadable(_)) && err.to_string().starts_with("couldn't read"), "{result}: {err}");
        }
    }

    #[test]
    fn a_numeric_reset_time_is_compared_as_a_time() {
        let at = |resets_at: Option<i64>| PlanWindow { id: "session".into(), label: "".into(), used: 5.0, resets: None, resets_at };
        assert!(!reset_between(&at(Some(1_790_847_255)), &at(Some(1_790_847_255))));
        assert!(!reset_between(&at(Some(1_790_847_255)), &at(Some(1_790_847_255 + 30))), "a few seconds' difference");
        assert!(reset_between(&at(Some(1_790_847_255)), &at(Some(1_790_847_255 + 5 * 3600))), "the next 5-hour window");
        assert!(reset_between(&at(Some(1_790_847_255)), &at(None)), "the reset time went away");
    }

    // -- spans --

    const T0: i64 = 1_000_000;

    fn reading(at: i64, last_call: i64, session: f64, week: f64) -> PlanReading {
        let w = |id: &str, used: f64, resets: &str| PlanWindow {
            id: id.into(),
            label: id.into(),
            used,
            resets: Some(resets.into()),
            resets_at: None,
        };
        PlanReading {
            id: 0,
            at: T0 + at,
            backend: "claude-cli".into(),
            ai_account_id: None,
            plan: Some("max".into()),
            last_call,
            windows: vec![w("session", session, "6:30pm"), w("week", week, "Oct 4")],
        }
    }

    fn pick(id: i64, at: i64) -> CallUse {
        CallUse {
            id,
            at: T0 + at,
            duration_ms: 4_000,
            kind: AiKind::Pick,
            model: Some("claude-sonnet-5-5".into()),
            usage: Usage {
                input_tokens: 29_000,
                output_tokens: 300,
                cache_write_tokens: 29_000,
                cache_write_1h_tokens: 29_000,
                ..Default::default()
            },
            api_usd: Some(0.12),
        }
    }

    /// Picks a minute apart: the session ticks after pick 2 and again after
    /// pick 5, so picks 3 to 5 used one point. Pick 1 and 2 come before
    /// the first tick and count for nothing.
    #[test]
    fn a_span_runs_from_tick_to_tick() {
        let calls: Vec<CallUse> = (1..=6).map(|i| pick(i, i * 60)).collect();
        let readings: Vec<PlanReading> = (0..=6)
            .map(|i| {
                reading(
                    i * 60 + 1,
                    i,
                    if i < 2 {
                        10.0
                    } else if i < 5 {
                        11.0
                    } else {
                        12.0
                    },
                    40.0,
                )
            })
            .collect();
        let s = spans(&readings, &calls);
        assert_eq!(s.len(), 1, "{s:?}");
        let s = &s[0];
        assert_eq!((s.window.as_str(), s.from, s.to, s.calls), ("session", 11, 12, 3));
        assert!(s.usable);
        assert_eq!(s.usage.total(), 3 * 29_300);
        assert!((s.api_usd.unwrap() - 0.36).abs() < 1e-9);
        assert_eq!(s.per_point(Measure::Tokens), Some(87_900.0));
        assert_eq!(s.by_model["claude-sonnet-5-5"].input_tokens, 87_000);
    }

    #[test]
    fn a_reset_throws_the_span_away() {
        let calls: Vec<CallUse> = (1..=4).map(|i| pick(i, i * 60)).collect();
        let mut readings = vec![reading(1, 0, 10.0, 40.0), reading(61, 1, 11.0, 40.0), reading(121, 2, 11.0, 40.0)];
        // The session resets (new reset time, lower percent), then ticks.
        let mut r = reading(181, 3, 1.0, 40.0);
        r.windows[0].resets = Some("11:30pm".into());
        readings.extend([r.clone(), PlanReading { at: T0 + 241, last_call: 4, ..r }]);
        readings[4].windows[0].used = 2.0;
        assert!(spans(&readings, &calls).is_empty(), "the tick after the reset starts a span; nothing closes it");

        // Only the reset time moved on: still a reset.
        let mut readings = vec![reading(1, 0, 10.0, 40.0), reading(61, 1, 11.0, 40.0), reading(121, 2, 11.0, 40.0)];
        let mut moved = reading(181, 3, 12.0, 40.0);
        moved.windows[0].resets = Some("Oct 1, 4:30am (America/New_York)".into());
        readings.push(moved);
        assert!(spans(&readings, &calls).is_empty());
    }

    /// Seen on 2026-09-30: two reads a few minutes apart said "Sep 30,
    /// 11:29pm" and "Sep 30, 11:30pm" for the same session.
    #[test]
    fn the_reset_time_may_wobble_by_a_minute() {
        let at =
            |resets: &str| PlanWindow { id: "session".into(), label: "".into(), used: 5.0, resets: Some(resets.into()), resets_at: None };
        assert!(!reset_between(&at("Sep 30, 11:29pm (America/New_York)"), &at("Sep 30, 11:30pm (America/New_York)")));
        assert!(!reset_between(&at("Oct 4, 2:59pm (America/New_York)"), &at("Oct 4, 3pm (America/New_York)")));
        assert!(reset_between(&at("Sep 30, 11:30pm (America/New_York)"), &at("Oct 1, 4:30am (America/New_York)")), "the next session");
        assert!(reset_between(&at("Oct 4, 3pm"), &at("Oct 11, 3pm")), "the next week");
        assert!(reset_between(&at("soon"), &at("later")), "unreadable times compare as text");
        assert!(!reset_between(&at("soon"), &at("soon")));
        assert_eq!(reset_minutes("Sep 30, 12am"), reset_minutes("Sep 30, 12:00am"));
        assert_eq!(reset_minutes("Sep 30, 12pm").unwrap() - reset_minutes("Sep 30, 12am").unwrap(), 12 * 60);

        // Spans run across the wobble.
        let calls: Vec<CallUse> = (1..=3).map(|i| pick(i, i * 60)).collect();
        let mut readings =
            vec![reading(1, 0, 10.0, 40.0), reading(61, 1, 11.0, 40.0), reading(121, 2, 11.0, 40.0), reading(181, 3, 12.0, 40.0)];
        for (r, resets) in readings.iter_mut().zip(["Sep 30, 11:29pm", "Sep 30, 11:30pm", "Sep 30, 11:29pm", "Sep 30, 11:30pm"]) {
            r.windows[0].resets = Some(resets.into());
        }
        assert_eq!(spans(&readings, &calls).len(), 1);
    }

    #[test]
    fn a_span_with_a_long_gap_or_no_calls_is_not_used() {
        // A pick, then nothing for an hour, then a pick that ticks it.
        let calls = vec![pick(1, 60), pick(2, 120), pick(3, 3_720)];
        let readings =
            vec![reading(1, 0, 10.0, 40.0), reading(61, 1, 11.0, 40.0), reading(121, 2, 11.0, 40.0), reading(3_721, 3, 12.0, 40.0)];
        let s = spans(&readings, &calls);
        assert_eq!(s.len(), 1);
        assert!(!s[0].usable, "an hour in which anything else could have used the plan");

        // Other use ticks the bar twice with no call of UpNext's between.
        let readings = vec![reading(1, 0, 10.0, 40.0), reading(30, 0, 11.0, 40.0), reading(60, 0, 12.0, 40.0)];
        let s = spans(&readings, &[]);
        assert_eq!(s.len(), 1);
        assert!(!s[0].usable);
    }

    #[test]
    fn a_change_of_plan_starts_over() {
        let calls: Vec<CallUse> = (1..=3).map(|i| pick(i, i * 60)).collect();
        let mut readings =
            vec![reading(1, 0, 10.0, 40.0), reading(61, 1, 11.0, 40.0), reading(121, 2, 11.0, 40.0), reading(181, 3, 12.0, 40.0)];
        readings[3].plan = Some("pro".into());
        assert!(spans(&readings, &calls).is_empty());
    }

    #[test]
    fn the_figure_is_the_largest_or_the_median_of_the_top_three() {
        assert_eq!(figure(vec![]), None);
        assert_eq!(figure(vec![5.0, 9.0]), Some(9.0), "other use only makes a span smaller");
        assert_eq!(figure(vec![5.0, 9.0, 8.0, 30.0, 1.0]), Some(9.0), "30 is one odd span; 9 is the middle of 30, 9 and 8");
    }

    fn priced(api_usd: f64, tokens: u64) -> Span {
        Span {
            window: "session".into(),
            from: 1,
            to: 2,
            start: 0,
            end: 0,
            calls: 1,
            usage: Usage { input_tokens: tokens, ..Default::default() },
            by_model: BTreeMap::new(),
            api_usd: Some(api_usd),
            kinds: vec![],
            usable: true,
        }
    }

    #[test]
    fn the_steadier_measure_is_chosen() {
        // Tokens per point vary a lot (picks are mostly cache writes,
        // summaries mostly fresh input), dollars little: dollars.
        let a = [priced(0.40, 100_000), priced(0.42, 300_000), priced(0.39, 60_000), priced(0.41, 200_000)];
        assert_eq!(choose_measure(&a.iter().collect::<Vec<_>>()), Measure::ApiUsd);
        let b = [priced(0.40, 100_000), priced(0.90, 101_000), priced(0.20, 99_000), priced(0.60, 100_000)];
        assert_eq!(choose_measure(&b.iter().collect::<Vec<_>>()), Measure::Tokens);
        assert_eq!(choose_measure(&b[..2].iter().collect::<Vec<_>>()), Measure::ApiUsd, "too few to tell: the price");
        let unpriced = Span { api_usd: None, ..priced(0.0, 1) };
        assert_eq!(choose_measure(&[&unpriced]), Measure::Tokens);
    }

    #[test]
    fn the_week_moves_with_the_session() {
        // The session moves 9 points while the week moves 2; one pair has
        // the session reset and doesn't count.
        let mut readings =
            vec![reading(0, 0, 10.0, 40.0), reading(60, 0, 14.0, 41.0), reading(120, 0, 15.0, 41.0), reading(180, 0, 19.0, 42.0)];
        assert_eq!(move_ratio(&readings, "session", "week"), Some(4.5));
        readings.push(reading(240, 0, 2.0, 43.0));
        assert_eq!(move_ratio(&readings, "session", "week"), Some(4.5), "the reset pair is left out");
        assert_eq!(move_ratio(&readings[..2], "session", "week"), None, "one point of the week is too few");
    }

    /// Readings and calls written to a database: the summary measures them
    /// and turns a pick into a share of each window.
    #[tokio::test]
    async fn a_measured_pick() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = crate::testutil::offline_config(dir.path());
        let db = Db::open(&cfg.db_path, 120).unwrap();
        let now = crate::db::now();
        let mut last = 0;
        let add_reading = |at: i64, session: f64, week: f64, last_call: i64| {
            let r = reading(0, last_call, session, week);
            db.plan_reading_add(&PlanReading { at, ..r }).unwrap();
        };
        // Three ticks of the session three picks apart, and the week moving
        // one point for every four of the session's over earlier readings.
        add_reading(now - 7_200, 1.0, 10.0, 0);
        add_reading(now - 7_100, 5.0, 11.0, 0);
        add_reading(now - 7_000, 9.0, 12.0, 0);
        add_reading(now - 1_000, 9.0, 12.0, 0);
        for i in 0..9 {
            let c = pick(0, 0);
            db.ai_call_add(&crate::db::AiCall {
                at: now - 900 + i * 60,
                kind: AiKind::Pick,
                backend: "claude-cli".into(),
                model: c.model.clone(),
                error: None,
                duration_ms: 4_000,
                usage: Some(c.usage),
                cost_usd: None,
                api_usd: c.api_usd,
                player_id: None,
                ai_account_id: None,
                would_cost_usd: None,
                items: None,
                batch_id: None,
                job_id: None,
            })
            .unwrap();
            last += 1;
            let session = 9.0 + ((i + 1) / 3) as f64;
            add_reading(now - 900 + i * 60 + 1, session, 12.0, last);
        }
        let readings = db.plan_readings("claude-cli", None, 0).unwrap();
        let calls = db.ai_call_uses("claude-cli", None, 0).unwrap();
        let s: Vec<Span> = spans(&readings, &calls).into_iter().filter(|s| s.window == "session").collect();
        assert_eq!(s.iter().filter(|s| s.usable).count(), 2, "{s:?}");

        // Another AI account's subscription has none of these readings or calls.
        assert!(measured(&db, &cfg, "claude-cli", Some(2), &Some("max".into())).unwrap().is_none());
        assert!(db.ai_call_uses("claude-cli", Some(2), 0).unwrap().is_empty());

        let m = measured(&db, &cfg, "claude-cli", None, &Some("max".into())).unwrap().unwrap();
        assert_eq!(m.measure, Measure::ApiUsd);
        let session = m.windows.iter().find(|w| w.id == "session").unwrap();
        assert!((session.per_point - 0.36).abs() < 1e-9, "three picks a point: {}", session.per_point);
        let week = m.windows.iter().find(|w| w.id == "week").unwrap();
        assert_eq!(week.from_session, Some(11.0 / 2.0), "the session moved 11 points while the week moved 2");
        // This install's picks average US$0.12 each: a third of a point.
        let pick = m.each.iter().find(|e| e.kind == AiKind::Pick).unwrap();
        assert!((pick.percent["session"] - 0.12 / 0.36).abs() < 1e-9, "{pick:?}");
        assert!((pick.percent["week"] - 0.12 / (0.36 * 5.5)).abs() < 1e-9);
        let summaries = m.each.iter().find(|e| e.kind == AiKind::ReviewSummaries).unwrap();
        assert!(summaries.percent["session"] > 0.0, "a title of summaries, from the built-in tokens at the spans' dollars a token");
    }

    /// A Claude CLI that answers `/usage` with a model turn is caught and
    /// not asked again; a reading of another meter (summaries on Codex, or
    /// another player's account) leaves that stop alone (B-AI-2).
    #[cfg(unix)] // The stand-in CLIs are shell scripts.
    #[tokio::test]
    async fn another_meters_reading_leaves_the_claude_clis_stop() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let cfg = crate::testutil::offline_config(dir.path());
        let db = Db::open(&cfg.db_path, 120).unwrap();
        let script = |name: &str, body: &str| {
            let p = dir.path().join(name);
            std::fs::write(&p, format!("#!/bin/sh\n{body}")).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p.display().to_string()
        };
        let count = dir.path().join("usage-calls");
        let claude = script(
            "claude",
            &format!(
                "case \"$*\" in\n\
                 *--version*) echo '9.9.9 (Claude Code)';;\n\
                 *auth*) echo '{{}}';;\n\
                 *) echo x >> '{}'; echo '{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"num_turns\":1,\"result\":\"Current session: 5% used\"}}';;\n\
                 esac\n",
                count.display()
            ),
        );
        let codex = script("codex", "case \"$*\" in\n*--version*) echo 'codex-cli 0.1.0';;\n*) exit 1;;\nesac\n");
        let claude_meter = Meter::Claude { bin: claude.clone(), workdir: None, token: None, account: None };
        let codex_meter = Meter::Codex { bin: codex, home: Some(dir.path().join("codex-home")), account: None };
        let spent = || std::fs::read_to_string(&count).map(|s| s.lines().count()).unwrap_or(0);

        assert!(matches!(take_reading(&db, &claude_meter).await, Err(ReadError::Spent(_))));
        assert!(matches!(take_reading(&db, &claude_meter).await, Err(ReadError::Stopped(_))), "caught, and not asked again");
        assert_eq!(spent(), 1);

        let _ = take_reading(&db, &codex_meter).await;
        let again = take_reading(&db, &claude_meter).await;
        assert!(matches!(again, Err(ReadError::Stopped(_))), "{again:?}");
        assert_eq!(spent(), 1, "the Claude CLI was asked for /usage again after a Codex reading");

        // Another AI account's Claude CLI has its own stop, and its own error.
        let theirs = Meter::Claude { bin: claude, workdir: None, token: Some("not-a-real-token".into()), account: Some(2) };
        assert!(matches!(take_reading(&db, &theirs).await, Err(ReadError::Spent(_))));
        assert_eq!(spent(), 2);
        assert!(db.meta_get("plan_usage.error.codex-cli").unwrap().is_some_and(|e| !e.is_empty()), "Codex's own error");
        assert!(db.meta_get("plan_usage.stopped.claude-cli.2").unwrap().is_some_and(|s| s != "null"));
    }
}
