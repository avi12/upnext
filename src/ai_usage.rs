//! What the AI uses: every call is recorded with its tokens (`call`), and
//! the estimates for each AI feature on this library are made from those
//! records, or from rates measured by hand until there are enough of them.

use crate::config::Config;
use crate::db::{AiCall, AiKind, AiTotals, AiUsageRow, Db};
use crate::discovery::gems;
use crate::llm::{LlmBackend, LlmError, LlmRequest, LlmResponse};
use anyhow::Result;
use serde::Serialize;
use std::time::Instant;

/// A token count, short: "310", "27.4k", "1.9M".
pub fn tokens(n: u64) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..999_950 => format!("{:.1}k", n as f64 / 1e3),
        _ => format!("{:.1}M", n as f64 / 1e6),
    }
}

/// "3 calls used 98.4k tokens in and 12.1k out", for a job's log.
pub fn used_line(t: &crate::db::AiTotals) -> String {
    let calls = if t.calls == 1 { "1 call".to_string() } else { format!("{} calls", t.calls) };
    let mut line = format!("{calls} used {} tokens in and {} out", tokens(t.usage.input_tokens), tokens(t.usage.output_tokens));
    if t.failed > 0 {
        line.push_str(&format!(" ({} failed)", t.failed));
    }
    if let Some(cost) = t.cost_usd {
        line.push_str(&format!(", US${cost:.2}"));
    }
    line
}

/// What a call is for, recorded with it.
#[derive(Debug, Clone)]
pub struct Purpose {
    pub kind: AiKind,
    /// The pick batch it is for.
    pub batch_id: Option<String>,
    /// How many titles or series it covers.
    pub items: Option<usize>,
}

impl Purpose {
    pub fn new(kind: AiKind) -> Self {
        Self { kind, batch_id: None, items: None }
    }

    pub fn batch(mut self, id: &str) -> Self {
        self.batch_id = Some(id.to_string());
        self
    }

    pub fn items(mut self, n: usize) -> Self {
        self.items = Some(n);
        self
    }
}

/// Make one AI call and record it in `ai_calls`, with its tokens, whether it
/// worked or not; inside a job, with the job's id. Every AI call goes
/// through here. A failure to record is logged, never the call's failure.
pub async fn call(db: &Db, llm: &dyn LlmBackend, purpose: &Purpose, req: LlmRequest) -> Result<LlmResponse, LlmError> {
    let requested = req.model.clone();
    let meter = llm.plan_meter();
    if let Some(m) = &meter {
        crate::ai_plan::before_call(db, m);
    }
    let started = Instant::now();
    let mut result = llm.complete_json(req).await;
    let duration_ms = started.elapsed().as_millis() as u64;
    let mut record = AiCall {
        at: crate::db::now(),
        kind: purpose.kind,
        backend: llm.name().to_string(),
        model: requested,
        error: None,
        duration_ms,
        usage: None,
        cost_usd: None,
        api_usd: None,
        would_cost_usd: None,
        items: purpose.items,
        batch_id: purpose.batch_id.clone(),
        job_id: crate::jobs::current_job(),
        player_id: llm.whose().player,
        ai_account_id: llm.whose().ai_account,
    };
    match &result {
        Ok(r) => {
            record.model = r.model.clone().or(record.model);
            record.usage = r.usage;
            record.would_cost_usd = r.would_cost_usd;
        }
        Err(e) => {
            record.error = Some(e.to_string());
            record.usage = e.usage();
            record.model = llm.model_sent(record.model.as_deref());
        }
    }
    // Priced from the list in use: what it costs at list prices, and what
    // was charged when the backend charges per call.
    if let (Some(pricing), Some(usage)) = (llm.pricing(), &record.usage) {
        let prices = crate::prices::current(db, pricing.provider);
        let model = record.model.as_deref();
        record.api_usd = if result.as_ref().is_ok_and(|r| r.flex) { prices.cost_flex(model, usage) } else { prices.cost(model, usage) };
        record.cost_usd = record.api_usd.filter(|_| pricing.charged);
    }
    if let Ok(r) = &mut result {
        (r.cost_usd, r.api_usd) = (record.cost_usd, record.api_usd);
    }
    if let Err(e) = db.ai_call_add(&record) {
        tracing::warn!("could not record an AI call: {e:#}");
    }
    if let Some(m) = &meter {
        crate::ai_plan::after_call(db, m);
    }
    result
}

// -- estimates ----------------------------------------------------------------

/// How far back the history estimates are made from goes.
const HISTORY_DAYS: i64 = 90;
/// Calls of a kind needed before its history replaces the built-in rate.
const MIN_CALLS: u64 = 3;
/// A library younger than this has too little history to say how many
/// games it gains or how often picks are made; the assumptions stand in.
const MIN_AGE_DAYS: i64 = 14;
/// Assumed until there's history: about five picks a week, rerolls included,
const PICKS_A_MONTH: f64 = 20.0;
/// a few Help me decides, two "It's too ___" a week,
const DECIDES_A_MONTH: f64 = 4.0;
const TOOS_A_MONTH: f64 = 8.0;
/// and a library that grows by 1% a month, at least five games.
const NEW_TITLES_SHARE: f64 = 0.01;
const MIN_NEW_TITLES: f64 = 5.0;

/// A rate measured by hand, used until there are `MIN_CALLS` of a kind here.
struct BuiltIn {
    kind: AiKind,
    /// Per title or series for the batch kinds, per call for the others.
    input: u64,
    output: u64,
    items_per_call: f64,
    /// Through the Claude CLI or the API. A local model's speed is unknown
    /// until it has made a few calls.
    seconds_per_call: f64,
}

/// Measured on 2026-09-30 with Sonnet 5.5 through the Claude CLI on a copy
/// of a library of about 2,000 titles: 3 picks, 3 Help me
/// decides, 24 random titles' summaries in 4 calls and 16 series guides in 2.
/// Picks again on 2026-10-01, once they ranked six
/// spares after three picks: a pick and a call for new spares, about 360
/// more tokens out than without spares, and three "It's too ___" calls
/// (3.4k to 6.7k tokens in: the Claude CLI read 3.1k of its own from its
/// cache on one; 336 to 792 out for an answer of a few ids).
const BUILT_IN: [BuiltIn; 7] = [
    BuiltIn { kind: AiKind::Pick, input: 29_700, output: 660, items_per_call: 1.0, seconds_per_call: 5.9 },
    BuiltIn { kind: AiKind::Decide, input: 1_500, output: 235, items_per_call: 1.0, seconds_per_call: 3.3 },
    BuiltIn { kind: AiKind::Too, input: 4_500, output: 530, items_per_call: 1.0, seconds_per_call: 5.4 },
    BuiltIn { kind: AiKind::ReviewSummaries, input: 4_060, output: 520, items_per_call: 6.0, seconds_per_call: 21.4 },
    BuiltIn { kind: AiKind::SeriesGuides, input: 430, output: 90, items_per_call: 8.0, seconds_per_call: 6.3 },
    SECOND_CHANCES,
    HIDDEN_GEMS,
];

/// Second Chance's "Now", per game: three runs over a copy of the same
/// library on 2026-10-02, 15 or 16 games in 4 calls
/// each, 44k to 48k tokens in and 2.4k to 2.9k out, 6 to 7.5 s a call.
const SECOND_CHANCES: BuiltIn =
    BuiltIn { kind: AiKind::SecondChances, input: 3_000, output: 170, items_per_call: 4.0, seconds_per_call: 6.6 };

/// Hidden gems, per game: not measured yet. The discovery plan's guess for its pool of
/// about 80 games was 15k tokens in, so about 190 a game with the rules
/// shared by 40; about 45 tokens out a game (three ratings and a line).
const HIDDEN_GEMS: BuiltIn =
    BuiltIn { kind: AiKind::HiddenGems, input: 200, output: 45, items_per_call: gems::BATCH as f64, seconds_per_call: 30.0 };

/// The same through the Codex CLI, with GPT-6-Luna at Codex's default
/// effort, on 2026-10-01 on a copy of the same library:
/// 8 picks, 3 Help me decides, the same 24 titles' summaries in 4 calls and
/// 16 series guides in 2. Codex adds about 3.2k tokens of its own to every
/// call, and its tokenizer isn't Claude's. Not measured since picks rank
/// spares: a pick's output is scaled from three reasons to nine, and
/// "It's too ___" is about 2.5k of UpNext's own tokens plus Codex's, with
/// half the Claude CLI's output, as Help me decide has.
const CODEX_BUILT_IN: [BuiltIn; 7] = [
    BuiltIn { kind: AiKind::Pick, input: 20_860, output: 555, items_per_call: 1.0, seconds_per_call: 7.9 },
    BuiltIn { kind: AiKind::Decide, input: 3_720, output: 120, items_per_call: 1.0, seconds_per_call: 6.3 },
    BuiltIn { kind: AiKind::Too, input: 5_700, output: 200, items_per_call: 1.0, seconds_per_call: 6.3 },
    BuiltIn { kind: AiKind::ReviewSummaries, input: 2_280, output: 230, items_per_call: 6.0, seconds_per_call: 51.2 },
    BuiltIn { kind: AiKind::SeriesGuides, input: 640, output: 130, items_per_call: 8.0, seconds_per_call: 26.0 },
    // Not measured: the Claude CLI's rate, with Codex's 3.2k a call shared by 4 games.
    BuiltIn { kind: AiKind::SecondChances, input: 3_800, output: 170, items_per_call: 4.0, seconds_per_call: 26.0 },
    // Not measured: the Claude CLI's guess, with Codex's 3.2k a call shared by 40 games.
    BuiltIn { kind: AiKind::HiddenGems, input: 280, output: 45, items_per_call: gems::BATCH as f64, seconds_per_call: 60.0 },
];

/// GPT-6-Luna with an OpenAI key, worked out from `CODEX_BUILT_IN` on
/// 2026-10-01 without Codex's 3,226 tokens of its own a call, as
/// no call has been made with a key yet. The times are Codex's.
const OPENAI_KEY_BUILT_IN: [BuiltIn; 7] = [
    BuiltIn { kind: AiKind::Pick, input: 17_630, output: 555, items_per_call: 1.0, seconds_per_call: 7.9 },
    BuiltIn { kind: AiKind::Decide, input: 500, output: 120, items_per_call: 1.0, seconds_per_call: 6.3 },
    BuiltIn { kind: AiKind::Too, input: 2_500, output: 200, items_per_call: 1.0, seconds_per_call: 6.3 },
    BuiltIn { kind: AiKind::ReviewSummaries, input: 1_740, output: 230, items_per_call: 6.0, seconds_per_call: 51.2 },
    BuiltIn { kind: AiKind::SeriesGuides, input: 240, output: 130, items_per_call: 8.0, seconds_per_call: 26.0 },
    BuiltIn { kind: AiKind::SecondChances, input: 3_000, output: 170, items_per_call: 4.0, seconds_per_call: 26.0 },
    BuiltIn { kind: AiKind::HiddenGems, input: 200, output: 45, items_per_call: gems::BATCH as f64, seconds_per_call: 60.0 },
];

/// Where a number comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    /// Counted in this library.
    Library,
    /// This install's own calls, syncs or picks.
    History,
    /// Measured by hand on another library.
    BuiltIn,
    /// A guess, until there's history.
    Assumed,
}

/// What one unit of an AI feature takes: a title of review summaries, a
/// series guide, a pick, a Help me decide.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, utoipa::ToSchema)]
pub struct Rate {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Titles or series per call; 1 for picks and Help me decide.
    pub items_per_call: f64,
    /// For the backend in use. None when it isn't known yet: a local model
    /// before its first few calls.
    pub seconds_per_call: Option<f64>,
    /// At the API's list prices, from this install's calls; None for a
    /// built-in rate, or when its calls had no known price.
    pub api_usd: Option<f64>,
    pub basis: Basis,
    /// The calls a history rate is from.
    pub calls: u64,
}

/// An amount of AI work: what it covers, and what it takes.
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
pub struct Amount {
    /// Titles, series, picks or decisions.
    pub items: u64,
    pub calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// With a backend charged per call, at list prices.
    pub cost_usd: Option<f64>,
    /// Waiting time, with the batch passes' calls running a few at once.
    pub minutes: Option<f64>,
    /// Where `items` comes from.
    pub basis: Basis,
}

#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
pub struct FeatureEstimate {
    pub kind: AiKind,
    /// The backend its calls go to: the picks' one, or for summaries,
    /// second chances and hidden gems the summaries' one.
    pub backend: String,
    /// The model it uses, as set ("sonnet"), or the id the API key resolves it to.
    pub model: Option<String>,
    pub rate: Rate,
    /// The run for everything that doesn't have it yet. None for what runs
    /// only when asked (picks, Help me decide, "It's too ___").
    pub one_time: Option<Amount>,
    pub monthly: Amount,
}

#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
pub struct Estimates {
    /// The picks' backend; each feature names its own.
    pub backend: String,
    /// Whether amounts carry money: a backend in use is charged per call
    /// (an API key, a hosted provider whose prices UpNext reads).
    pub priced: bool,
    pub features: Vec<FeatureEstimate>,
}

/// Each kind's rate for `backend`: this install's calls through it when
/// there are enough, else the built-in one. Backends count tokens
/// differently (the Codex CLI adds about 3.2k of its own to every call, a
/// local model has its own tokenizer), so one backend's calls never stand
/// in for another's.
fn rates(history: &[(AiKind, String, AiTotals)], backend: &str) -> Vec<(AiKind, Rate)> {
    let built_in: &[BuiltIn] = match backend {
        "codex-cli" => &CODEX_BUILT_IN,
        "openai-key" => &OPENAI_KEY_BUILT_IN,
        _ => &BUILT_IN,
    };
    built_in
        .iter()
        .map(|b| {
            let mut here = AiTotals::default();
            for (_, _, t) in history.iter().filter(|(k, from, _)| *k == b.kind && from == backend) {
                here.add(t);
            }
            let rate = if here.calls >= MIN_CALLS {
                let n = if here.items > 0 { here.items } else { here.calls };
                Rate {
                    input_tokens: here.usage.input_tokens / n,
                    output_tokens: here.usage.output_tokens / n,
                    items_per_call: n as f64 / here.calls as f64,
                    seconds_per_call: Some(here.duration_ms as f64 / here.calls as f64 / 1000.0),
                    api_usd: here.api_usd.map(|u| u / n as f64),
                    basis: Basis::History,
                    calls: here.calls,
                }
            } else {
                Rate {
                    input_tokens: b.input,
                    output_tokens: b.output,
                    items_per_call: b.items_per_call,
                    // A local model's speed is unknown until it has made a few calls.
                    seconds_per_call: (backend != "openai").then_some(b.seconds_per_call),
                    api_usd: None,
                    basis: Basis::BuiltIn,
                    calls: 0,
                }
            };
            (b.kind, rate)
        })
        .collect()
}

fn amount(items: u64, calls: u64, rate: &Rate, price: Option<crate::prices::Price>, parallel: usize, basis: Basis) -> Amount {
    let (input_tokens, output_tokens) = (items * rate.input_tokens, items * rate.output_tokens);
    Amount {
        items,
        calls,
        input_tokens,
        output_tokens,
        cost_usd: price.map(|p| (input_tokens as f64 * p.input + output_tokens as f64 * p.output) / 1e6),
        minutes: rate.seconds_per_call.map(|s| (calls as f64 / parallel as f64).ceil() * s / 60.0),
        basis,
    }
}

/// How many of something a month, from `count` over the last `days` days
/// of history, or `assumed` when there are too few days.
fn per_month(count: u64, days: i64, assumed: f64) -> (f64, Basis) {
    if days >= MIN_AGE_DAYS { (count as f64 * 30.0 / days.min(30) as f64, Basis::History) } else { (assumed, Basis::Assumed) }
}

/// New titles a month: those first seen after the library's first day
/// (which brought the whole library), over the last 90 days.
fn new_titles_a_month(first_seen: &[i64], now: i64) -> (f64, Basis) {
    let assumed = (first_seen.len() as f64 * NEW_TITLES_SHARE).max(MIN_NEW_TITLES).round();
    let Some(&first) = first_seen.iter().min() else { return (assumed, Basis::Assumed) };
    let age = (now - first) / 86_400;
    if age < MIN_AGE_DAYS {
        return (assumed, Basis::Assumed);
    }
    let from = (first + 86_400).max(now - HISTORY_DAYS * 86_400);
    let days = ((now - from) as f64 / 86_400.0).max(1.0);
    let new = first_seen.iter().filter(|&&t| t >= from).count();
    (new as f64 * 30.0 / days, Basis::History)
}

/// Whether a kind of work goes to the summaries' backend: what is
/// written with `Config::summary_model`. Series guides reason like picks
/// do, so they stay with the picks' backend.
pub fn summary_kind(kind: AiKind) -> bool {
    matches!(kind, AiKind::ReviewSummaries | AiKind::SecondChances | AiKind::HiddenGems)
}

/// One backend's part of the estimates: its name, the prices it is charged
/// at (None when its calls aren't charged), and its rates.
struct Side {
    backend: String,
    prices: Option<crate::prices::PriceList>,
    rates: Vec<(AiKind, Rate)>,
}

impl Side {
    fn new(db: &Db, llm: &dyn crate::llm::LlmBackend, history: &[(AiKind, String, AiTotals)]) -> Self {
        let backend = llm.name().to_string();
        Self {
            prices: llm.pricing().filter(|p| p.charged).map(|p| crate::prices::current(db, p.provider)),
            rates: rates(history, &backend),
            backend,
        }
    }
}

/// What each AI feature would take on this library: the one-time run for
/// what doesn't have it yet and a typical month, in tokens, time and, with
/// a backend charged per call, money.
pub fn estimates(db: &Db, cfg: &Config) -> Result<Estimates> {
    let now = crate::db::now();
    let history = db.ai_rates(now - HISTORY_DAYS * 86_400)?;
    // Summaries' kinds may go to a backend of their own, with its own
    // prices and rates.
    let picks = Side::new(db, crate::llm::backend_from_config(cfg).as_ref(), &history);
    let summaries = Side::new(db, crate::llm::summary_backend(cfg).as_ref(), &history);
    let side = |kind: AiKind| if summary_kind(kind) { &summaries } else { &picks };
    let backend = picks.backend.clone();
    let priced = picks.prices.is_some() || summaries.prices.is_some();
    let model_for = |kind: AiKind| {
        let m = if summary_kind(kind) { cfg.summary_model() } else { cfg.pick_model() };
        let side = side(kind);
        match side.backend.as_str() {
            "api-key" => Some(crate::llm::api_key::resolve_model(m.as_deref())),
            // A family ("luna") is priced as its newest model on the price list.
            "openai-key" => {
                let listed: Vec<String> = side.prices.iter().flat_map(|p| p.models.keys().cloned()).collect();
                Some(crate::llm::openai_key::resolve(&listed, m.as_deref().unwrap_or("luna")))
            }
            _ => m,
        }
    };
    // Summaries, series guides, second chances and hidden gems go at Flex with an OpenAI key.
    let flex = |kind: AiKind| {
        side(kind).backend == "openai-key"
            && matches!(kind, AiKind::ReviewSummaries | AiKind::SeriesGuides | AiKind::SecondChances | AiKind::HiddenGems)
    };
    let price_for = |kind: AiKind, model: &Option<String>| {
        let (list, m) = (side(kind).prices.as_ref()?, model.as_deref()?);
        if flex(kind) { list.get_flex(m) } else { list.get(m) }
    };
    let rate = |kind: AiKind| side(kind).rates.iter().find(|(k, _)| *k == kind).map(|(_, r)| *r).expect("a built-in rate for each kind");

    let all = db.library_games()?;
    let titles = all.len().max(1) as f64;
    let first_seen = db.titles_first_seen()?;
    let (new_titles, new_basis) = new_titles_a_month(&first_seen, now);
    let library_days = first_seen.iter().min().map_or(0, |&f| (now - f) / 86_400);
    let parallel = crate::enrich::sources::AI_PARALLEL;
    let mut features = vec![];

    // Picks, Help me decide and "It's too ___": only when asked.
    let since = now - 30 * 86_400;
    let (picks, picks_basis) = per_month(db.pick_batches_since(since)?, library_days, PICKS_A_MONTH);
    let ai_days = db.ai_calls_began()?.map_or(0, |t| (now - t) / 86_400);
    let (decides, decides_basis) = per_month(db.ai_call_count(AiKind::Decide, since)?, ai_days, DECIDES_A_MONTH);
    let (toos, toos_basis) = per_month(db.ai_call_count(AiKind::Too, since)?, ai_days, TOOS_A_MONTH);
    for (kind, n, basis) in [(AiKind::Pick, picks, picks_basis), (AiKind::Decide, decides, decides_basis), (AiKind::Too, toos, toos_basis)]
    {
        let (model, r) = (model_for(kind), rate(kind));
        let n = n.round() as u64;
        features.push(FeatureEstimate {
            backend: side(kind).backend.clone(),
            kind,
            monthly: amount(n, n, &r, price_for(kind, &model), 1, basis),
            one_time: None,
            model,
            rate: r,
        });
    }

    // Review summaries: the titles without one (or with an old one), then new games.
    let kind = AiKind::ReviewSummaries;
    let (model, r) = (model_for(kind), rate(kind));
    let pending: Vec<usize> = all
        .iter()
        .filter(|g| crate::digest::wanted(g) && crate::digest::outdated(g))
        .map(|g| crate::digest::review_block(g).len())
        .collect();
    let calls = crate::digest::batch_sizes(pending.iter().copied()).len() as u64;
    let with_reviews = all.iter().filter(|g| crate::digest::wanted(g)).count() as f64 / titles;
    let monthly = (new_titles * with_reviews).round() as u64;
    features.push(FeatureEstimate {
        backend: side(kind).backend.clone(),
        kind,
        one_time: Some(amount(pending.len() as u64, calls, &r, price_for(kind, &model), parallel, Basis::Library)),
        monthly: amount(monthly, (monthly as f64 / r.items_per_call).ceil() as u64, &r, price_for(kind, &model), parallel, new_basis),
        model,
        rate: r,
    });

    // Series guides: the series without one (or whose games changed), then new games in a series.
    let kind = AiKind::SeriesGuides;
    let (model, r) = (model_for(kind), rate(kind));
    let groups = crate::series::groups(&all, &db.catalog()?);
    let guides = db.series_guides()?;
    let pending = groups.iter().filter(|g| crate::series::needs_guide(g, &guides)).count() as u64;
    let in_series = groups.iter().map(|g| g.members.len()).sum::<usize>() as f64 / titles;
    let monthly = (new_titles * in_series).round() as u64;
    features.push(FeatureEstimate {
        backend: side(kind).backend.clone(),
        kind,
        one_time: Some(amount(
            pending,
            pending.div_ceil(crate::series::BATCH as u64),
            &r,
            price_for(kind, &model),
            parallel,
            Basis::Library,
        )),
        monthly: amount(monthly, monthly.div_ceil(crate::series::BATCH as u64), &r, price_for(kind, &model), parallel, new_basis),
        model,
        rate: r,
    });

    // Second chances: the games it checks without an up-to-date "Now", then
    // each month the share whose reviews it reads again (every
    // `REVIEWS_FRESH_DAYS`), since new ones change what "Now" is written from.
    let kind = AiKind::SecondChances;
    let (model, r) = (model_for(kind), rate(kind));
    let batch = crate::second_chance::BATCH as u64;
    let checked: Vec<&crate::db::GameView> =
        all.iter().filter(|g| crate::second_chance::checked(g) && crate::second_chance::reviews_since(g) > 0).collect();
    let pending = checked.iter().filter(|g| crate::second_chance::needs_now(g, false)).count() as u64;
    let monthly = (checked.len() as f64 * 30.0 / crate::second_chance::REVIEWS_FRESH_DAYS as f64).ceil() as u64;
    features.push(FeatureEstimate {
        backend: side(kind).backend.clone(),
        kind,
        one_time: Some(amount(pending, pending.div_ceil(batch), &r, price_for(kind, &model), parallel, Basis::Library)),
        monthly: amount(monthly, monthly.div_ceil(batch), &r, price_for(kind, &model), parallel, Basis::Library),
        model,
        rate: r,
    });

    // Hidden gems: the games in the pool not rated yet, then each month the
    // new games that join it.
    let kind = AiKind::HiddenGems;
    let (model, r) = (model_for(kind), rate(kind));
    let rules = gems::Rules::from_config(cfg);
    let profile = crate::taste::Profile::load(db, cfg.whose(), &all)?;
    let rated = db.hidden_gems()?;
    let pool = gems::pool(&all, &rules, Some(&profile));
    let pending = pool.iter().filter(|g| gems::needs_rating(g, &rated, false)).count() as u64;
    let monthly = (new_titles * pool.len() as f64 / titles).round() as u64;
    let batch = gems::BATCH as u64;
    features.push(FeatureEstimate {
        backend: side(kind).backend.clone(),
        kind,
        one_time: Some(amount(pending, pending.div_ceil(batch), &r, price_for(kind, &model), parallel, Basis::Library)),
        monthly: amount(monthly, monthly.div_ceil(batch), &r, price_for(kind, &model), parallel, new_basis),
        model,
        rate: r,
    });

    Ok(Estimates { backend, priced, features })
}

// -- usage ----------------------------------------------------------------------

/// One kind's calls, added up.
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
pub struct KindUsage {
    pub kind: AiKind,
    #[serde(flatten)]
    pub totals: AiTotals,
}

/// One model's calls, added up.
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
pub struct ModelUsage {
    pub backend: String,
    pub model: Option<String>,
    #[serde(flatten)]
    pub totals: AiTotals,
}

/// One local day's calls, added up and per kind.
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
pub struct DayUsage {
    pub day: String,
    #[serde(flatten)]
    pub totals: AiTotals,
    pub by_kind: Vec<KindUsage>,
}

/// What the AI used over the last `days` days (today included), for the
/// Settings view: headline totals, and the period by day, kind and model.
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
pub struct UsageSummary {
    pub days: i64,
    /// The backend in use now, and whether it's charged per call (an API key).
    pub backend: String,
    pub priced: bool,
    pub today: AiTotals,
    pub last_7_days: AiTotals,
    /// All of the `days`.
    pub total: AiTotals,
    /// Every day of the period, oldest first, days without calls included.
    pub by_day: Vec<DayUsage>,
    pub by_kind: Vec<KindUsage>,
    pub by_model: Vec<ModelUsage>,
    /// The period's calls by who asked: the reader's own, and on
    /// the AI accounts they own everyone's, with the library's jobs as a
    /// row of their own (`player_id` null).
    pub by_player: Vec<PlayerUsage>,
    /// The same calls by the AI account that ran them: the
    /// reader's own accounts first, then the ones others share with them.
    pub by_account: Vec<AccountUsage>,
}

/// One AI account's calls in [`UsageSummary`].
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
pub struct AccountUsage {
    /// Null only for calls from before there was any AI account.
    pub ai_account_id: Option<i64>,
    /// Its owner's name; null once the account is removed.
    pub owner: Option<String>,
    /// Whether it's the reader's own.
    pub mine: bool,
    /// Its backend now (`claude-cli`, `api-key` and the rest), or null for
    /// the built-in one or a removed account.
    pub backend: Option<String>,
    /// The first account: this computer's own CLIs and keys.
    pub this_computer: bool,
    #[serde(flatten)]
    pub totals: AiTotals,
}

/// One player's calls in [`UsageSummary`].
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
pub struct PlayerUsage {
    #[schema(value_type = Option<i64>)]
    pub player_id: Option<crate::db::PlayerId>,
    /// Their name, or "The library" for the library's jobs.
    pub name: String,
    #[serde(flatten)]
    pub totals: AiTotals,
}

/// Local midnight `days_ago` days before today, as Unix seconds.
fn local_midnight(days_ago: i64) -> (String, i64) {
    use chrono::{Local, TimeZone};
    let day = Local::now().date_naive() - chrono::Duration::days(days_ago);
    let at = Local.from_local_datetime(&day.and_hms_opt(0, 0, 0).expect("midnight")).earliest().map_or(0, |t| t.timestamp());
    (day.to_string(), at)
}

pub fn usage_summary(db: &Db, cfg: &Config, days: i64) -> Result<UsageSummary> {
    let days = days.clamp(1, 365);
    let (_, since) = local_midnight(days.max(7) - 1);
    let p = cfg.whose();
    let rows = db.ai_usage_rows_for(since, p)?;
    let llm = crate::llm::backend_from_config(cfg);
    let mut summary = summarize(rows, days, llm.name(), llm.pricing().is_some_and(|p| p.charged));
    let (_, from) = local_midnight(days - 1);
    let players = db.players()?;
    summary.by_player = db
        .ai_usage_by_player(from, p)?
        .into_iter()
        .map(|(who, totals)| PlayerUsage {
            player_id: who,
            name: match who {
                Some(id) => players.iter().find(|x| x.id == id).map_or_else(|| "A removed player".to_string(), |x| x.name.clone()),
                None => "The library".into(),
            },
            totals,
        })
        .collect();
    summary.by_account = db
        .ai_usage_by_account(from, p)?
        .into_iter()
        .map(|a| AccountUsage {
            ai_account_id: a.ai_account_id,
            owner: a.owner_id.and_then(|o| players.iter().find(|x| x.id == o)).map(|x| x.name.clone()),
            mine: a.owner_id == Some(p),
            backend: a.backend,
            this_computer: a.this_computer,
            totals: a.totals,
        })
        .collect();
    Ok(summary)
}

fn summarize(rows: Vec<AiUsageRow>, days: i64, backend: &str, priced: bool) -> UsageSummary {
    let (today, _) = local_midnight(0);
    let (week_from, _) = local_midnight(6);
    let (from, _) = local_midnight(days - 1);
    let mut summary = UsageSummary {
        days,
        backend: backend.to_string(),
        priced,
        today: AiTotals::default(),
        last_7_days: AiTotals::default(),
        total: AiTotals::default(),
        by_day: (0..days).rev().map(|ago| DayUsage { day: local_midnight(ago).0, totals: AiTotals::default(), by_kind: vec![] }).collect(),
        by_kind: vec![],
        by_model: vec![],
        by_player: vec![],
        by_account: vec![],
    };
    for row in rows {
        // Days compare as text: they're all yyyy-mm-dd.
        if row.day >= week_from {
            summary.last_7_days.add(&row.totals);
        }
        if row.day == today {
            summary.today.add(&row.totals);
        }
        if row.day < from {
            continue;
        }
        summary.total.add(&row.totals);
        add_kind(&mut summary.by_kind, row.kind, &row.totals);
        match summary.by_model.iter_mut().find(|m| m.backend == row.backend && m.model == row.model) {
            Some(m) => m.totals.add(&row.totals),
            None => {
                summary.by_model.push(ModelUsage { backend: row.backend.clone(), model: row.model.clone(), totals: row.totals.clone() })
            }
        }
        if let Some(day) = summary.by_day.iter_mut().find(|d| d.day == row.day) {
            day.totals.add(&row.totals);
            add_kind(&mut day.by_kind, row.kind, &row.totals);
        }
    }
    summary.by_kind.sort_by_key(|k| k.kind);
    summary.by_model.sort_by_key(|m| std::cmp::Reverse(m.totals.usage.total()));
    summary
}

fn add_kind(kinds: &mut Vec<KindUsage>, kind: AiKind, totals: &AiTotals) {
    match kinds.iter_mut().find(|k| k.kind == kind) {
        Some(k) => k.totals.add(totals),
        None => kinds.push(KindUsage { kind, totals: totals.clone() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::Usage;

    #[test]
    fn token_counts_are_short() {
        assert_eq!(tokens(310), "310");
        assert_eq!(tokens(27_400), "27.4k");
        assert_eq!(tokens(999_949), "999.9k");
        assert_eq!(tokens(999_950), "1.0M");
        assert_eq!(tokens(1_900_000), "1.9M");
    }

    fn totals(calls: u64, input: u64, output: u64, items: u64, duration_ms: u64) -> AiTotals {
        AiTotals {
            calls,
            usage: Usage { input_tokens: input, output_tokens: output, ..Default::default() },
            items,
            duration_ms,
            ..Default::default()
        }
    }

    fn rate(rates: &[(AiKind, Rate)], kind: AiKind) -> Rate {
        rates.iter().find(|(k, _)| *k == kind).unwrap().1
    }

    #[test]
    fn rates_are_built_in_until_there_are_enough_calls() {
        let r = rates(&[], "claude-cli");
        let pick = rate(&r, AiKind::Pick);
        assert_eq!((pick.basis, pick.input_tokens, pick.calls), (Basis::BuiltIn, 29_700, 0));
        assert!(pick.seconds_per_call.is_some());
        assert_eq!(rate(&rates(&[], "openai"), AiKind::Pick).seconds_per_call, None, "a local model's speed isn't known yet");

        let history = vec![
            (AiKind::ReviewSummaries, "claude-cli".to_string(), totals(3, 120_000, 18_000, 24, 180_000)),
            (AiKind::ReviewSummaries, "openai".to_string(), totals(1, 40_000, 6_000, 8, 400_000)),
            (AiKind::Pick, "claude-cli".to_string(), totals(2, 58_000, 1_400, 0, 6_000)),
        ];
        let r = rates(&history, "claude-cli");
        let summaries = rate(&r, AiKind::ReviewSummaries);
        assert_eq!(summaries.basis, Basis::History);
        assert_eq!((summaries.input_tokens, summaries.output_tokens, summaries.calls), (5_000, 750, 3), "per title");
        assert_eq!((summaries.items_per_call, summaries.seconds_per_call), (8.0, Some(60.0)));
        assert_eq!(rate(&r, AiKind::Pick).basis, Basis::BuiltIn, "two picks are too few");

        let summaries = rate(&rates(&history, "openai"), AiKind::ReviewSummaries);
        assert_eq!(
            (summaries.basis, summaries.input_tokens, summaries.seconds_per_call),
            (Basis::BuiltIn, 4_060, None),
            "one call of its own is too few, and the Claude CLI's don't count for it"
        );
    }

    /// The Codex CLI has its own built-in rates, and Claude's calls don't
    /// stand in for its own.
    #[test]
    fn codex_rates_are_its_own() {
        let history = vec![(AiKind::Pick, "claude-cli".to_string(), totals(5, 146_000, 1_500, 0, 20_000))];
        let pick = rate(&rates(&history, "codex-cli"), AiKind::Pick);
        assert_eq!((pick.basis, pick.input_tokens), (Basis::BuiltIn, CODEX_BUILT_IN[0].input));
        assert_ne!(pick.input_tokens, BUILT_IN[0].input);
        let history = vec![(AiKind::Pick, "codex-cli".to_string(), totals(3, 66_000, 600, 0, 30_000))];
        let pick = rate(&rates(&history, "codex-cli"), AiKind::Pick);
        assert_eq!((pick.basis, pick.input_tokens, pick.seconds_per_call), (Basis::History, 22_000, Some(10.0)));

        let pick = rate(&rates(&history, "openai-key"), AiKind::Pick);
        assert_eq!((pick.basis, pick.input_tokens), (Basis::BuiltIn, OPENAI_KEY_BUILT_IN[0].input), "Codex's calls aren't the key's");
        assert!(pick.seconds_per_call.is_some());
    }

    #[test]
    fn new_games_a_month() {
        let day = 86_400;
        let now = 1_000 * day;
        let first = now - 5 * day;
        let young: Vec<i64> = std::iter::repeat_n(first, 1_000).collect();
        assert_eq!(new_titles_a_month(&young, now), (10.0, Basis::Assumed), "1% of the library until there's history");
        assert_eq!(new_titles_a_month(&young[..100], now), (5.0, Basis::Assumed), "at least five");
        assert_eq!(new_titles_a_month(&[], now), (5.0, Basis::Assumed));

        // Synced 60 days ago, then 12 games since: 6 a month. The first day doesn't count.
        let first = now - 60 * day;
        let mut seen: Vec<i64> = std::iter::repeat_n(first, 500).chain(std::iter::repeat_n(first + day / 2, 30)).collect();
        seen.extend((0..12).map(|i| now - i * 4 * day));
        let (n, basis) = new_titles_a_month(&seen, now);
        assert_eq!(basis, Basis::History);
        assert!((n - 12.0 * 30.0 / 59.0).abs() < 1e-9, "{n}");

        assert_eq!(per_month(9, 20, PICKS_A_MONTH), (13.5, Basis::History), "20 days of history, scaled to 30");
        assert_eq!(per_month(9, 45, PICKS_A_MONTH), (9.0, Basis::History), "the last 30 days");
        assert_eq!(per_month(0, 3, PICKS_A_MONTH), (PICKS_A_MONTH, Basis::Assumed));
    }

    #[test]
    fn usage_adds_up_by_day_kind_and_model() {
        let today = local_midnight(0).0;
        let row = |ago: i64, kind: AiKind, model: &str, input: u64| AiUsageRow {
            day: local_midnight(ago).0,
            kind,
            backend: "claude-cli".into(),
            model: Some(model.into()),
            totals: totals(1, input, 100, 0, 1_000),
        };
        let rows = vec![
            row(40, AiKind::Pick, "claude-opus-5-5", 1),
            row(8, AiKind::ReviewSummaries, "claude-sonnet-5-5", 1_000_000),
            row(3, AiKind::Pick, "claude-opus-5-5", 29_000),
            row(0, AiKind::Pick, "claude-opus-5-5", 30_000),
            row(0, AiKind::Decide, "claude-sonnet-5-5", 2_000),
        ];
        let s = summarize(rows, 30, "claude-cli", false);
        assert!(!s.priced);
        assert_eq!((s.today.calls, s.today.usage.input_tokens), (2, 32_000));
        assert_eq!((s.last_7_days.calls, s.last_7_days.usage.input_tokens), (3, 61_000));
        assert_eq!((s.total.calls, s.total.usage.input_tokens), (4, 1_061_000), "day 40 is outside the 30");
        assert_eq!(s.by_day.len(), 30);
        assert_eq!(s.by_day.last().unwrap().day, today);
        assert_eq!(s.by_day.last().unwrap().by_kind.len(), 2);
        assert_eq!(s.by_day.iter().filter(|d| d.totals.calls > 0).count(), 3, "empty days are listed too");
        let kinds: Vec<AiKind> = s.by_kind.iter().map(|k| k.kind).collect();
        assert_eq!(kinds, [AiKind::Pick, AiKind::Decide, AiKind::ReviewSummaries]);
        assert_eq!(s.by_model[0].model.as_deref(), Some("claude-sonnet-5-5"), "most tokens first");
        assert_eq!(s.by_model[1].totals.calls, 2);
    }
}
