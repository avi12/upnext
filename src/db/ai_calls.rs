//! AI calls and the tokens they used. Each call is recorded when it comes
//! back (`ai_usage::call`), whether it worked or not, and kept for a year. A
//! job's tokens and a pick batch's are sums over their calls.

use super::{Db, Job, JobStatus, PlayerId, now};
use crate::llm::Usage;
use anyhow::Result;
use rusqlite::{Connection, Row, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// AI call records older than this are deleted.
pub const KEEP_AI_CALLS_SECS: i64 = 365 * 86_400;

/// What an AI call was for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AiKind {
    /// Picks from the library. A Curate again is another pick.
    Pick,
    /// Help me decide, on Versus.
    Decide,
    /// "It's too ___" on a pick: a spare without the problem.
    Too,
    /// People say, Vibes and review tags, a batch of titles per call.
    ReviewSummaries,
    /// Series guides, a batch of series per call.
    SeriesGuides,
    /// Second Chance's "Now": what recent reviews say about why abandoned
    /// games were put down, a few games per call.
    SecondChances,
    /// Hidden gems: how unusual unplayed, highly rated, short
    /// games are, many games per call.
    HiddenGems,
    /// The Settings "Test" button.
    Test,
}

impl AiKind {
    pub const ALL: [AiKind; 8] =
        [Self::Pick, Self::Decide, Self::Too, Self::ReviewSummaries, Self::SeriesGuides, Self::SecondChances, Self::HiddenGems, Self::Test];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pick => "pick",
            Self::Decide => "decide",
            Self::Too => "too",
            Self::ReviewSummaries => "review_summaries",
            Self::SeriesGuides => "series_guides",
            Self::SecondChances => "second_chances",
            Self::HiddenGems => "hidden_gems",
            Self::Test => "test",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// The enrich lane its calls run in, for the kinds that run in jobs.
    pub fn lane(self) -> Option<&'static str> {
        match self {
            Self::ReviewSummaries => Some("AI review summaries"),
            Self::SeriesGuides => Some("AI series guides"),
            Self::SecondChances => Some("AI second chances"),
            Self::HiddenGems => Some("AI hidden gems"),
            _ => None,
        }
    }
}

/// One AI call, as recorded.
#[derive(Debug, Clone, PartialEq)]
pub struct AiCall {
    /// When it came back (Unix seconds).
    pub at: i64,
    pub kind: AiKind,
    /// `claude-cli`, `api-key` or `openai`.
    pub backend: String,
    pub model: Option<String>,
    /// Why it failed; None when it worked.
    pub error: Option<String>,
    pub duration_ms: u64,
    /// None when the backend didn't say.
    pub usage: Option<Usage>,
    /// Charged, at list prices: an API key or a hosted provider.
    pub cost_usd: Option<f64>,
    /// At the API's list prices, charged or not; None when the model has no
    /// known price (a local model).
    pub api_usd: Option<f64>,
    /// What it would have cost on the API (the Claude CLI's figure); not a charge.
    pub would_cost_usd: Option<f64>,
    /// How many titles (review summaries) or series (guides) it covered.
    pub items: Option<usize>,
    /// The pick batch it was for.
    pub batch_id: Option<String>,
    /// The job it ran in.
    pub job_id: Option<i64>,
    /// Whose request it answered; None for the library's own work.
    pub player_id: Option<PlayerId>,
    /// The AI account that ran it.
    pub ai_account_id: Option<i64>,
}

/// AI calls added up.
#[derive(Debug, Clone, Default, PartialEq, Serialize, utoipa::ToSchema)]
pub struct AiTotals {
    pub calls: u64,
    /// Of `calls`, the ones that failed.
    pub failed: u64,
    #[serde(flatten)]
    pub usage: Usage,
    /// Charged (an API key or a hosted provider); None when nothing was.
    pub cost_usd: Option<f64>,
    /// All of them at the API's list prices, charged or not; None when no
    /// call had a known price. What wasn't charged is this less `cost_usd`.
    pub api_usd: Option<f64>,
    /// What the Claude CLI's calls would have cost on the API, as it said.
    /// Not a charge, and not shown: `api_usd` is UpNext's own figure.
    pub would_cost_usd: Option<f64>,
    /// Titles or series the calls that worked covered.
    pub items: u64,
    pub duration_ms: u64,
}

impl AiTotals {
    pub fn add(&mut self, o: &AiTotals) {
        let sum =
            |a: Option<f64>, b: Option<f64>| if a.is_none() && b.is_none() { None } else { Some(a.unwrap_or(0.0) + b.unwrap_or(0.0)) };
        self.calls += o.calls;
        self.failed += o.failed;
        self.usage += o.usage;
        self.cost_usd = sum(self.cost_usd, o.cost_usd);
        self.api_usd = sum(self.api_usd, o.api_usd);
        self.would_cost_usd = sum(self.would_cost_usd, o.would_cost_usd);
        self.items += o.items;
        self.duration_ms += o.duration_ms;
    }
}

/// The calls of one local day, kind, backend and model, added up.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AiUsageRow {
    /// `2026-09-30`, in this computer's time zone.
    pub day: String,
    pub kind: AiKind,
    pub backend: String,
    pub model: Option<String>,
    #[serde(flatten)]
    pub totals: AiTotals,
}

/// One AI account's calls, added up (`Db::ai_usage_by_account`).
#[derive(Debug, Clone, PartialEq)]
pub struct AccountTotals {
    /// None only for calls from before there was any AI account.
    pub ai_account_id: Option<i64>,
    /// Its owner, and its backend (None for the built-in one); both None
    /// once the account is removed.
    pub owner_id: Option<PlayerId>,
    pub backend: Option<String>,
    /// The first account, whose are this computer's CLIs and keys.
    pub this_computer: bool,
    pub totals: AiTotals,
}

/// What a job's AI calls used.
#[derive(Debug, Clone, Default, PartialEq, Serialize, utoipa::ToSchema)]
pub struct JobAiUsage {
    #[serde(flatten)]
    pub totals: AiTotals,
    /// While it runs: the tokens, in and out, the whole job should take, from
    /// its own average per title (or series) so far. None until a call has
    /// come back for each AI pass with work left.
    pub estimated_tokens: Option<u64>,
}

/// The aggregate columns `totals_at` reads, in order.
/// The calls player `?2` sees in Usage: theirs, and those on the AI accounts
/// they own. A call from before there were AI accounts on calls was the
/// first account's, and one with no player the library's.
const SEEN_BY: &str = "(player_id = ?2
     OR COALESCE(ai_account_id, (SELECT MIN(id) FROM ai_accounts)) IN (SELECT id FROM ai_accounts WHERE owner_id = ?2)
     OR (?2 = 1 AND ai_account_id IS NULL AND player_id IS NULL AND NOT EXISTS (SELECT 1 FROM ai_accounts)))";

const SUMS: &str = "COUNT(*), SUM(error IS NOT NULL), SUM(input_tokens), SUM(output_tokens), SUM(cache_read_tokens), \
    SUM(cache_write_tokens), SUM(cost_usd), SUM(would_cost_usd), SUM(CASE WHEN error IS NULL THEN items END), SUM(duration_ms), \
    SUM(cache_write_1h_tokens), SUM(api_usd)";

fn totals_at(r: &Row<'_>, i: usize) -> rusqlite::Result<AiTotals> {
    let n = |k: usize| -> rusqlite::Result<u64> { Ok(r.get::<_, Option<i64>>(i + k)?.unwrap_or(0).max(0) as u64) };
    Ok(AiTotals {
        calls: n(0)?,
        failed: n(1)?,
        usage: Usage {
            input_tokens: n(2)?,
            output_tokens: n(3)?,
            cache_read_tokens: n(4)?,
            cache_write_tokens: n(5)?,
            cache_write_1h_tokens: n(10)?,
        },
        cost_usd: r.get(i + 6)?,
        api_usd: r.get(i + 11)?,
        would_cost_usd: r.get(i + 7)?,
        items: n(8)?,
        duration_ms: n(9)?,
    })
}

/// Each job's calls, added up per kind.
fn by_job(c: &Connection, ids: &[i64]) -> Result<HashMap<i64, Vec<(AiKind, AiTotals)>>> {
    let mut stmt = c.prepare_cached(&format!(
        "SELECT job_id, kind, {SUMS} FROM ai_calls WHERE job_id IN (SELECT value FROM json_each(?1)) GROUP BY job_id, kind"
    ))?;
    let rows = stmt.query_map(params![super::to_json(&ids)], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, totals_at(r, 2)?)))?;
    let mut out: HashMap<i64, Vec<(AiKind, AiTotals)>> = HashMap::new();
    for row in rows {
        let (job, kind, totals) = row?;
        if let Some(kind) = AiKind::parse(&kind) {
            out.entry(job).or_default().push((kind, totals));
        }
    }
    Ok(out)
}

/// A job's AI usage from its calls per kind, and while it runs, an estimate
/// for the whole job: each AI lane's tokens so far, plus its average per
/// item for the items it hasn't done.
fn job_usage(job: &Job, kinds: &[(AiKind, AiTotals)]) -> JobAiUsage {
    let mut totals = AiTotals::default();
    let mut estimate = Some(0u64);
    for (kind, t) in kinds {
        totals.add(t);
        let left =
            kind.lane().and_then(|name| job.lanes.iter().find(|l| l.name == name)).map_or(0, |l| l.total.saturating_sub(l.done)) as u64;
        estimate = match (estimate, left, t.items) {
            (Some(e), 0, _) => Some(e + t.usage.total()),
            (Some(e), left, items) if items > 0 => Some(e + t.usage.total() + t.usage.total() * left / items),
            _ => None,
        };
    }
    let running = job.status == JobStatus::Running;
    JobAiUsage { totals, estimated_tokens: estimate.filter(|_| running) }
}

impl Db {
    /// Record a call, and forget the ones older than a year.
    pub fn ai_call_add(&self, a: &AiCall) -> Result<()> {
        let tokens = |f: fn(&Usage) -> u64| a.usage.as_ref().map(|u| f(u) as i64);
        self.with(|c| {
            c.execute(
                "INSERT INTO ai_calls(at, kind, backend, model, error, duration_ms, input_tokens, output_tokens, cache_read_tokens,
                     cache_write_tokens, cost_usd, would_cost_usd, items, batch_id, job_id, cache_write_1h_tokens, api_usd,
                     player_id, ai_account_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
                params![
                    a.at,
                    a.kind.as_str(),
                    a.backend,
                    a.model,
                    a.error,
                    a.duration_ms as i64,
                    tokens(|u| u.input_tokens),
                    tokens(|u| u.output_tokens),
                    tokens(|u| u.cache_read_tokens),
                    tokens(|u| u.cache_write_tokens),
                    a.cost_usd,
                    a.would_cost_usd,
                    a.items.map(|n| n as i64),
                    a.batch_id,
                    a.job_id,
                    tokens(|u| u.cache_write_1h_tokens),
                    a.api_usd,
                    a.player_id,
                    a.ai_account_id
                ],
            )?;
            c.execute("DELETE FROM ai_calls WHERE at < ?1", params![now() - KEEP_AI_CALLS_SECS])?;
            Ok(())
        })
    }

    /// Calls since `since`, added up per local day, kind, backend and model,
    /// oldest day first.
    pub fn ai_usage_rows(&self, since: i64) -> Result<Vec<AiUsageRow>> {
        self.usage_rows(since, None)
    }

    /// [`Db::ai_usage_rows`], of the calls player `p` sees in Usage (plan
    /// 21): their own requests, and every call on an AI account they own,
    /// whoever asked (the library's jobs included).
    pub fn ai_usage_rows_for(&self, since: i64, p: PlayerId) -> Result<Vec<AiUsageRow>> {
        self.usage_rows(since, Some(p))
    }

    /// Those calls added up per player who asked (None: the library's jobs).
    pub fn ai_usage_by_player(&self, since: i64, p: PlayerId) -> Result<Vec<(Option<PlayerId>, AiTotals)>> {
        self.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT player_id, {SUMS} FROM ai_calls WHERE at >= ?1 AND {SEEN_BY} GROUP BY player_id ORDER BY player_id IS NULL, player_id"
            ))?;
            let rows = stmt.query_map(params![since, p], |r| Ok((r.get::<_, Option<i64>>(0)?.map(PlayerId), totals_at(r, 1)?)))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Those calls added up per AI account that ran them, with
    /// each account's owner and backend while it still exists. A call from
    /// before there were AI accounts on calls was the first account's.
    pub fn ai_usage_by_account(&self, since: i64, p: PlayerId) -> Result<Vec<AccountTotals>> {
        self.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT x.account, a.owner_id, a.backend, x.account = (SELECT MIN(id) FROM ai_accounts), x.* FROM
                   (SELECT COALESCE(ai_account_id, (SELECT MIN(id) FROM ai_accounts)) AS account, {SUMS} FROM ai_calls
                    WHERE at >= ?1 AND {SEEN_BY} GROUP BY account) x
                 LEFT JOIN ai_accounts a ON a.id = x.account
                 ORDER BY a.owner_id IS NOT ?2, x.account IS NULL, x.account"
            ))?;
            let rows = stmt.query_map(params![since, p], |r| {
                Ok(AccountTotals {
                    ai_account_id: r.get(0)?,
                    owner_id: r.get::<_, Option<i64>>(1)?.map(PlayerId),
                    backend: r.get(2)?,
                    this_computer: r.get::<_, Option<bool>>(3)?.unwrap_or(false),
                    totals: totals_at(r, 5)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    fn usage_rows(&self, since: i64, p: Option<PlayerId>) -> Result<Vec<AiUsageRow>> {
        self.with(|c| {
            let seen = if p.is_some() { SEEN_BY } else { "?2 IS NULL" };
            let mut stmt = c.prepare(&format!(
                "SELECT date(at, 'unixepoch', 'localtime') AS day, kind, backend, model, {SUMS} FROM ai_calls
                 WHERE at >= ?1 AND {seen} GROUP BY day, kind, backend, model ORDER BY day, kind, backend, model"
            ))?;
            let rows = stmt.query_map(params![since, p], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get(2)?, r.get(3)?, totals_at(r, 4)?))
            })?;
            let mut out = vec![];
            for row in rows {
                let (day, kind, backend, model, totals) = row?;
                if let Some(kind) = AiKind::parse(&kind) {
                    out.push(AiUsageRow { day, kind, backend, model, totals });
                }
            }
            Ok(out)
        })
    }

    /// The calls that worked since `since`, added up per kind and backend:
    /// the history estimates are made from.
    pub fn ai_rates(&self, since: i64) -> Result<Vec<(AiKind, String, AiTotals)>> {
        self.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT kind, backend, {SUMS} FROM ai_calls WHERE at >= ?1 AND error IS NULL AND input_tokens IS NOT NULL
                 GROUP BY kind, backend"
            ))?;
            let rows = stmt.query_map(params![since], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, totals_at(r, 2)?)))?;
            let mut out = vec![];
            for row in rows {
                let (kind, backend, totals) = row?;
                if let Some(kind) = AiKind::parse(&kind) {
                    out.push((kind, backend, totals));
                }
            }
            Ok(out)
        })
    }

    /// A job's calls of one kind, added up.
    pub fn ai_job_totals(&self, job: i64, kind: AiKind) -> Result<AiTotals> {
        self.with(|c| {
            Ok(c.query_row(&format!("SELECT {SUMS} FROM ai_calls WHERE job_id=?1 AND kind=?2"), params![job, kind.as_str()], |r| {
                totals_at(r, 0)
            })?)
        })
    }

    /// When the first AI call on record came back.
    pub fn ai_calls_began(&self) -> Result<Option<i64>> {
        self.with(|c| Ok(c.query_row("SELECT MIN(at) FROM ai_calls", [], |r| r.get(0))?))
    }

    /// How many calls of `kind` came back since `since`.
    pub fn ai_call_count(&self, kind: AiKind, since: i64) -> Result<u64> {
        self.with(|c| {
            Ok(c.query_row("SELECT COUNT(*) FROM ai_calls WHERE kind=?1 AND at >= ?2", params![kind.as_str(), since], |r| {
                r.get::<_, i64>(0)
            })?
            .max(0) as u64)
        })
    }

    /// Each pick batch's calls, added up (a retried pick made two). Batches
    /// whose calls reported no tokens are left out.
    pub(super) fn ai_usage_by_batch(&self, ids: &[String]) -> Result<HashMap<String, AiTotals>> {
        self.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT batch_id, {SUMS} FROM ai_calls WHERE batch_id IN (SELECT value FROM json_each(?1)) AND input_tokens IS NOT NULL
                 GROUP BY batch_id"
            ))?;
            let rows = stmt.query_map(params![super::to_json(&ids)], |r| Ok((r.get::<_, String>(0)?, totals_at(r, 1)?)))?;
            let mut out = HashMap::new();
            for row in rows {
                let (id, totals) = row?;
                out.insert(id, totals);
            }
            Ok(out)
        })
    }
}

/// Fill in each job's AI usage (jobs without AI calls keep None).
pub(super) fn attach_ai_usage(c: &Connection, jobs: &mut [Job]) -> Result<()> {
    let ids: Vec<i64> = jobs.iter().map(|j| j.id).collect();
    if ids.is_empty() {
        return Ok(());
    }
    let by_job = by_job(c, &ids)?;
    for job in jobs.iter_mut() {
        if let Some(kinds) = by_job.get(&job.id) {
            job.ai_usage = Some(job_usage(job, kinds));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::PlayerId;
    use crate::db::{JobFilter, JobProgress, JobSource, LaneProgress};

    fn db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("upnext.db"), 120).unwrap();
        (dir, db)
    }

    fn call(kind: AiKind, at: i64, input: u64, output: u64) -> AiCall {
        AiCall {
            at,
            kind,
            backend: "claude-cli".into(),
            model: Some("claude-sonnet-5-5".into()),
            error: None,
            duration_ms: 2_000,
            usage: Some(Usage {
                input_tokens: input,
                output_tokens: output,
                cache_read_tokens: 0,
                cache_write_tokens: input.saturating_sub(2),
                cache_write_1h_tokens: input.saturating_sub(2),
            }),
            cost_usd: None,
            api_usd: Some(0.012),
            player_id: None,
            ai_account_id: None,
            would_cost_usd: Some(0.01),
            items: None,
            batch_id: None,
            job_id: None,
        }
    }

    #[test]
    fn calls_add_up_by_day_kind_and_model() {
        let (_dir, db) = db();
        let t = now();
        db.ai_call_add(&call(AiKind::Pick, t, 29_000, 700)).unwrap();
        db.ai_call_add(&AiCall { model: Some("claude-opus-5-5".into()), ..call(AiKind::Pick, t, 29_000, 800) }).unwrap();
        db.ai_call_add(&AiCall { items: Some(8), ..call(AiKind::ReviewSummaries, t, 40_000, 5_000) }).unwrap();
        let failed = AiCall {
            error: Some("usage limit".into()),
            usage: None,
            items: Some(8),
            would_cost_usd: None,
            ..call(AiKind::ReviewSummaries, t, 2, 0)
        };
        db.ai_call_add(&failed).unwrap();

        let rows = db.ai_usage_rows(t - 60).unwrap();
        assert_eq!(rows.len(), 3, "{rows:?}");
        let summaries = rows.iter().find(|r| r.kind == AiKind::ReviewSummaries).unwrap();
        assert_eq!((summaries.totals.calls, summaries.totals.failed), (2, 1));
        assert_eq!(summaries.totals.items, 8, "only the call that worked covered its titles");
        assert_eq!(summaries.totals.usage.input_tokens, 40_000, "the failed call reported nothing");
        assert_eq!(summaries.totals.would_cost_usd, Some(0.01));
        assert_eq!(summaries.totals.cost_usd, None, "nothing was charged");
        assert!(rows.iter().all(|r| r.day == rows[0].day));

        let rates = db.ai_rates(t - 60).unwrap();
        let summaries = rates.iter().find(|(k, ..)| *k == AiKind::ReviewSummaries).unwrap();
        assert_eq!(summaries.2.calls, 1, "rates come from calls that worked");
        assert_eq!(db.ai_call_count(AiKind::Pick, t - 60).unwrap(), 2);
        assert_eq!(db.ai_calls_began().unwrap(), Some(t));
    }

    #[test]
    fn a_year_is_kept() {
        let (_dir, db) = db();
        db.ai_call_add(&call(AiKind::Decide, now() - KEEP_AI_CALLS_SECS - 10, 2_000, 100)).unwrap();
        db.ai_call_add(&call(AiKind::Decide, now() - 300 * 86_400, 2_000, 100)).unwrap();
        db.ai_call_add(&call(AiKind::Decide, now(), 2_000, 100)).unwrap();
        assert_eq!(db.ai_call_count(AiKind::Decide, 0).unwrap(), 2, "the one older than a year is gone");
    }

    #[test]
    fn a_batch_adds_up_its_calls() {
        let (_dir, db) = db();
        for input in [29_000, 30_000] {
            db.ai_call_add(&AiCall { batch_id: Some("b1".into()), ..call(AiKind::Pick, now(), input, 500) }).unwrap();
        }
        db.ai_call_add(&AiCall { batch_id: Some("b2".into()), usage: None, ..call(AiKind::Pick, now(), 0, 0) }).unwrap();
        let by = db.ai_usage_by_batch(&["b1".into(), "b2".into(), "b3".into()]).unwrap();
        assert_eq!(by["b1"].usage.input_tokens, 59_000);
        assert_eq!(by["b1"].usage.output_tokens, 1_000);
        assert_eq!(by["b1"].usage.cache_write_1h_tokens, 58_996);
        assert!((by["b1"].api_usd.unwrap() - 0.024).abs() < 1e-12);
        assert!(!by.contains_key("b2"), "a backend that reported no tokens");
    }

    /// History gives each batch its calls' figures. A batch from before its
    /// calls were recorded keeps its old cost only as a list-price figure:
    /// the first builds stored the Claude CLI's there.
    #[test]
    fn old_batches_costs_are_not_charges() {
        let (_dir, db) = db();
        let batch = |id: &str, created_at: i64, cost_usd: Option<f64>| crate::db::PickBatch {
            batch_id: id.into(),
            prompt: "p".into(),
            created_at,
            model: None,
            duration_ms: None,
            cost_usd,
            api_usd: None,
            candidates_considered: None,
            attempts: None,
            usage: None,
            shown: 0,
            picks: vec![],
            spares: vec![],
            removed: vec![],
            ranked_by: None,
            mode: None,
            suggestion: None,
            routine: None,
        };
        db.save_batch(PlayerId::FIRST, &batch("old", 1, Some(0.41))).unwrap();
        db.save_batch(PlayerId::FIRST, &batch("new", 2, None)).unwrap();
        db.ai_call_add(&AiCall { batch_id: Some("new".into()), ..call(AiKind::Pick, now(), 29_000, 300) }).unwrap();
        let history = db.pick_history(PlayerId::FIRST, 5).unwrap();
        let of = |id: &str| history.iter().find(|b| b.batch_id == id).map(|b| (b.cost_usd, b.api_usd, b.usage.is_some())).unwrap();
        assert_eq!(of("old"), (None, Some(0.41), false));
        assert_eq!(of("new"), (None, Some(0.012), true));
    }

    /// While a job runs, its estimate is its tokens so far plus its own
    /// average per title for the titles left; once it ends, only what it used.
    #[test]
    fn a_running_job_estimates_the_rest() {
        let (_dir, db) = db();
        let id = db.job_begin("enrich", None, JobSource::Ui, &serde_json::json!({})).unwrap().unwrap();
        let lane = |name: &str, done, total| LaneProgress { name: name.into(), done, total, ..Default::default() };
        let progress = JobProgress {
            lanes: vec![lane("Steam reviews", 5, 5), lane("AI review summaries", 16, 100), lane("AI series guides", 0, 10)],
            ..Default::default()
        };
        db.job_pulse(id, &progress, &[], &[]).unwrap();
        for _ in 0..2 {
            let c = AiCall { items: Some(8), job_id: Some(id), ..call(AiKind::ReviewSummaries, now(), 40_000, 8_000) };
            db.ai_call_add(&c).unwrap();
        }
        db.ai_call_add(&call(AiKind::Pick, now(), 29_000, 700)).unwrap();

        let job = db.job_running().unwrap().unwrap();
        let used = job.ai_usage.unwrap();
        assert_eq!((used.totals.calls, used.totals.usage.total(), used.totals.items), (2, 96_000, 16), "only this job's calls");
        assert_eq!(used.estimated_tokens, Some(96_000 + 96_000 * 84 / 16), "6,000 a title for the 84 left");

        // A guide call comes back: now both passes can be estimated.
        let guide = AiCall { items: Some(4), job_id: Some(id), ..call(AiKind::SeriesGuides, now(), 3_000, 1_000) };
        db.ai_call_add(&guide).unwrap();
        let progress = JobProgress { lanes: vec![lane("AI review summaries", 16, 100), lane("AI series guides", 4, 10)], ..progress };
        db.job_pulse(id, &progress, &[], &[]).unwrap();
        let used = db.job(id).unwrap().unwrap().ai_usage.unwrap();
        assert_eq!(used.estimated_tokens, Some(96_000 + 96_000 * 84 / 16 + 4_000 + 4_000 * 6 / 4));

        let end = crate::db::JobEnd { status: JobStatus::Done, summary: None, error: None };
        db.job_end(id, &progress, &[], &[], &end).unwrap();
        let done = &db.jobs_recent(&JobFilter { limit: 5, ..Default::default() }).unwrap()[0];
        assert_eq!(done.ai_usage.as_ref().map(|u| (u.totals.calls, u.estimated_tokens)), Some((3, None)));
    }

    /// A pass with work left and no call back yet can't be estimated.
    #[test]
    fn no_estimate_before_the_first_call() {
        let (_dir, db) = db();
        let id = db.job_begin("enrich", None, JobSource::Ui, &serde_json::json!({})).unwrap().unwrap();
        let lanes = vec![LaneProgress { name: "AI review summaries".into(), done: 8, total: 100, ..Default::default() }];
        db.job_pulse(id, &JobProgress { lanes, ..Default::default() }, &[], &[]).unwrap();
        let failed = AiCall {
            error: Some("bad".into()),
            usage: None,
            items: Some(8),
            job_id: Some(id),
            ..call(AiKind::ReviewSummaries, now(), 2, 0)
        };
        db.ai_call_add(&failed).unwrap();
        let used = db.job(id).unwrap().unwrap().ai_usage.unwrap();
        assert_eq!((used.totals.failed, used.estimated_tokens), (1, None));
    }
}
