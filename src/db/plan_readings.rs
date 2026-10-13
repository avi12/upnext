//! A subscription's usage bars as its CLI reported them (`ai_plan`), read
//! around UpNext's own AI calls and kept for a year, like the calls.

use super::{AiKind, Db, now};
use crate::llm::Usage;
use anyhow::Result;
use rusqlite::{Row, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

/// Readings older than this are deleted.
const KEEP_SECS: i64 = super::KEEP_AI_CALLS_SECS;

/// One of a plan's limits, as read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PlanWindow {
    /// "session", "week", or "week:Fable" for a model's own weekly limit.
    pub id: String,
    /// As the CLI says it: "Current week (all models)".
    pub label: String,
    /// Percent used, never left: a count-down reading is turned round
    /// when it is read.
    pub used: f64,
    /// When it resets, as the Claude CLI says it: "Oct 4, 3pm (America/New_York)".
    pub resets: Option<String>,
    /// When it resets, in Unix seconds, when the CLI gives it as a number
    /// (the Codex CLI does).
    #[serde(default)]
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlanReading {
    pub id: i64,
    /// When the reading began.
    pub at: i64,
    pub backend: String,
    /// The AI account whose subscription it read.
    pub ai_account_id: Option<i64>,
    /// The plan it was read on ("max"), when the CLI said.
    pub plan: Option<String>,
    /// The newest AI call recorded when it began: calls up to this one are
    /// in the reading, later ones are not.
    pub last_call: i64,
    /// The limits the plan has. One it doesn't have is absent.
    pub windows: Vec<PlanWindow>,
}

impl PlanReading {
    pub fn window(&self, id: &str) -> Option<&PlanWindow> {
        self.windows.iter().find(|w| w.id == id)
    }
}

/// An AI call's tokens, as the measuring needs them.
#[derive(Debug, Clone, PartialEq)]
pub struct CallUse {
    pub id: i64,
    /// When it came back.
    pub at: i64,
    pub duration_ms: u64,
    pub kind: AiKind,
    pub model: Option<String>,
    pub usage: Usage,
    pub api_usd: Option<f64>,
}

/// Which subscription a reading is of: the backend and its AI account.
pub type ReadingKey = (&'static str, Option<i64>);

/// For each subscription, whether a background reading is running for a
/// database, and whether another call came back while it ran, so one more
/// is due when it ends. Each AI account's subscription is read on its own
///: a call on one doesn't wait for, or ask again of, another's.
#[derive(Debug, Default)]
pub struct ReadingsInFlight(Mutex<HashMap<ReadingKey, (bool, bool)>>);

impl ReadingsInFlight {
    /// Start a reading, unless one is running: then ask it to go again.
    pub fn start(&self, key: ReadingKey) -> bool {
        let mut all = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let s = all.entry(key).or_default();
        if s.0 {
            s.1 = true;
            false
        } else {
            *s = (true, false);
            true
        }
    }

    /// A reading ended: true when another is due.
    pub fn again(&self, key: ReadingKey) -> bool {
        let mut all = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let s = all.entry(key).or_default();
        if s.1 {
            s.1 = false;
            true
        } else {
            all.remove(&key);
            false
        }
    }

    /// Whether any reading is running.
    pub fn busy(&self) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).values().any(|s| s.0)
    }
}

fn reading(r: &Row<'_>) -> rusqlite::Result<PlanReading> {
    let windows: String = r.get(5)?;
    Ok(PlanReading {
        id: r.get(0)?,
        at: r.get(1)?,
        backend: r.get(2)?,
        plan: r.get(3)?,
        last_call: r.get(4)?,
        windows: serde_json::from_str(&windows).unwrap_or_default(),
        ai_account_id: r.get(6)?,
    })
}

const COLUMNS: &str = "id, at, backend, plan, last_call, windows, ai_account_id";

impl Db {
    /// Keep a reading, and forget the ones older than a year.
    pub fn plan_reading_add(&self, r: &PlanReading) -> Result<i64> {
        self.with(|c| {
            c.execute(
                "INSERT INTO plan_readings(at, backend, plan, last_call, windows, ai_account_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![r.at, r.backend, r.plan, r.last_call, super::to_json(&r.windows), r.ai_account_id],
            )?;
            let id = c.last_insert_rowid();
            c.execute("DELETE FROM plan_readings WHERE at < ?1", params![now() - KEEP_SECS])?;
            Ok(id)
        })
    }

    /// One AI account's readings of a backend since `since`, oldest first.
    pub fn plan_readings(&self, backend: &str, account: Option<i64>, since: i64) -> Result<Vec<PlanReading>> {
        self.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM plan_readings WHERE backend=?1 AND ai_account_id IS ?2 AND at >= ?3 ORDER BY id"
            ))?;
            let rows = stmt.query_map(params![backend, account, since], reading)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    /// One AI account's newest reading of a backend.
    pub fn plan_reading_latest(&self, backend: &str, account: Option<i64>) -> Result<Option<PlanReading>> {
        use rusqlite::OptionalExtension;
        self.with(|c| {
            Ok(c.query_row(
                &format!("SELECT {COLUMNS} FROM plan_readings WHERE backend=?1 AND ai_account_id IS ?2 ORDER BY id DESC LIMIT 1"),
                params![backend, account],
                reading,
            )
            .optional()?)
        })
    }

    /// The newest AI call's id, 0 before the first.
    pub fn ai_last_call_id(&self) -> Result<i64> {
        self.with(|c| Ok(c.query_row("SELECT COALESCE(MAX(id), 0) FROM ai_calls", [], |r| r.get(0))?))
    }

    /// One AI account's calls on a backend after the call `after`, with the
    /// tokens they reported, oldest first: what moved that account's bars.
    pub fn ai_call_uses(&self, backend: &str, account: Option<i64>, after: i64) -> Result<Vec<CallUse>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, at, duration_ms, kind, model, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                        cache_write_1h_tokens, api_usd
                 FROM ai_calls WHERE backend=?1 AND ai_account_id IS ?2 AND id > ?3 AND input_tokens IS NOT NULL ORDER BY id",
            )?;
            let rows = stmt.query_map(params![backend, account, after], |r| {
                let n = |i: usize| -> rusqlite::Result<u64> { Ok(r.get::<_, Option<i64>>(i)?.unwrap_or(0).max(0) as u64) };
                Ok((
                    r.get::<_, String>(3)?,
                    CallUse {
                        id: r.get(0)?,
                        at: r.get(1)?,
                        duration_ms: n(2)?,
                        kind: AiKind::Test,
                        model: r.get(4)?,
                        usage: Usage {
                            input_tokens: n(5)?,
                            output_tokens: n(6)?,
                            cache_read_tokens: n(7)?,
                            cache_write_tokens: n(8)?,
                            cache_write_1h_tokens: n(9)?,
                        },
                        api_usd: r.get(10)?,
                    },
                ))
            })?;
            let mut out = vec![];
            for row in rows {
                let (kind, call) = row?;
                if let Some(kind) = AiKind::parse(&kind) {
                    out.push(CallUse { kind, ..call });
                }
            }
            Ok(out)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readings_are_kept_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("upnext.db"), 120).unwrap();
        let window = PlanWindow {
            id: "session".into(),
            label: "Current session".into(),
            used: 20.0,
            resets: Some("6:30pm".into()),
            resets_at: None,
        };
        let r = PlanReading {
            id: 0,
            at: now(),
            backend: "claude-cli".into(),
            ai_account_id: None,
            plan: Some("max".into()),
            last_call: 0,
            windows: vec![window],
        };
        db.plan_reading_add(&PlanReading { at: now() - KEEP_SECS - 10, ..r.clone() }).unwrap();
        db.plan_reading_add(&r).unwrap();
        db.plan_reading_add(&PlanReading { last_call: 4, ..r.clone() }).unwrap();
        let all = db.plan_readings("claude-cli", None, 0).unwrap();
        assert_eq!(all.len(), 2, "the one older than a year is gone");
        assert_eq!(all[0].windows, r.windows);
        assert_eq!(db.plan_reading_latest("claude-cli", None).unwrap().unwrap().last_call, 4);
        assert!(db.plan_reading_latest("codex", None).unwrap().is_none());
        assert_eq!(db.ai_last_call_id().unwrap(), 0);

        // A second connection sees them, and shares the in-flight state.
        let again = db.reopen().unwrap();
        assert_eq!(again.plan_readings("claude-cli", None, 0).unwrap().len(), 2);
        let key = ("claude-cli", None);
        assert!(db.readings.start(key));
        assert!(!again.readings.start(key), "one at a time");
        assert!(again.readings.again(key), "the second asked for one more");
        assert!(!db.readings.again(key));
        assert!(!db.readings.busy());
    }

    /// Each AI account's subscription has its own readings and is read on
    /// its own: one account's bars never stand for another's.
    #[test]
    fn each_ai_accounts_readings_are_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("upnext.db"), 120).unwrap();
        let reading = |account: Option<i64>, used: f64| PlanReading {
            id: 0,
            at: now(),
            backend: "claude-cli".into(),
            ai_account_id: account,
            plan: Some("max".into()),
            last_call: 0,
            windows: vec![PlanWindow { id: "session".into(), label: "Current session".into(), used, resets: None, resets_at: None }],
        };
        db.plan_reading_add(&reading(Some(1), 20.0)).unwrap();
        db.plan_reading_add(&reading(Some(2), 70.0)).unwrap();
        db.plan_reading_add(&reading(Some(1), 25.0)).unwrap();
        let used = |account| db.plan_readings("claude-cli", account, 0).unwrap().iter().map(|r| r.windows[0].used).collect::<Vec<_>>();
        assert_eq!(used(Some(1)), [20.0, 25.0]);
        assert_eq!(used(Some(2)), [70.0]);
        assert!(used(None).is_empty());
        assert_eq!(db.plan_reading_latest("claude-cli", Some(2)).unwrap().unwrap().ai_account_id, Some(2));

        // A reading of one account doesn't hold up, or stand in for, another's.
        assert!(db.readings.start(("claude-cli", Some(1))));
        assert!(db.readings.start(("claude-cli", Some(2))), "another account's reading starts too");
        assert!(!db.readings.again(("claude-cli", Some(2))));
        assert!(db.readings.busy(), "the first is still running");
        assert!(!db.readings.again(("claude-cli", Some(1))));
        assert!(!db.readings.busy());
    }
}
