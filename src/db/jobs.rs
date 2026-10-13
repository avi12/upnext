//! Background jobs (sync, enrich, backup), their logs and their failures. Whichever
//! process runs a job (the server or the command line) writes its progress
//! here every second or so, and both read it, so a job started from a
//! terminal shows in the UI and can be cancelled from there.

use super::{Db, PlayerId, now};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A running job whose heartbeat is older than this has died with its
/// process (closed, killed or crashed).
pub const STALE_SECS: i64 = 30;
/// Jobs kept, with their logs; older ones are deleted when a job ends,
/// except each task's last run and last run that ended done, which a
/// schedule needs however long ago they were.
pub const KEEP_JOBS: usize = 50;

/// Where a job was started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum JobSource {
    /// By hand in the web UI.
    Ui,
    /// By the server's scheduler (Settings › Jobs).
    Schedule,
    /// `upnext sync` or `upnext enrich` in a terminal.
    Terminal,
}

impl JobSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ui => "ui",
            Self::Schedule => "schedule",
            Self::Terminal => "terminal",
        }
    }

    /// "from a terminal", for sentences about a job.
    pub fn started_from(self) -> &'static str {
        match self {
            Self::Ui => "from the UI",
            Self::Schedule => "by its schedule",
            Self::Terminal => "from a terminal",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "schedule" => Self::Schedule,
            "terminal" => Self::Terminal,
            _ => Self::Ui,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Running,
    Done,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "done" => Self::Done,
            "cancelled" => Self::Cancelled,
            _ => Self::Failed,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum LaneState {
    #[default]
    Running,
    /// A service asked it to slow down, or didn't answer; it tries again at `retry_at`.
    Waiting,
    /// It gave up for the rest of the run (a rejected key, too many failures, a usage limit).
    Stopped,
    Done,
}

/// One lane of an enrich run: a rate-limited service taking every title in
/// turn, or a pass over the whole library (Wikidata, the AI summaries).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct LaneProgress {
    pub name: String,
    pub done: usize,
    pub total: usize,
    pub failed: usize,
    pub state: LaneState,
    /// The title it's on.
    pub current: Option<String>,
    /// What it's waiting on or why it stopped, said plainly: "Steam asked us to slow down".
    pub note: Option<String>,
    /// While waiting: when it tries again (Unix seconds).
    pub retry_at: Option<i64>,
}

/// How far a running job has got, as the process running it last wrote it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JobProgress {
    pub total: usize,
    pub done: usize,
    pub current: Option<String>,
    pub message: String,
    pub lanes: Vec<LaneProgress>,
    /// Titles (or games) per minute over the last few minutes.
    pub per_minute: Option<f64>,
    /// Time left at that rate; None until there are enough samples.
    pub eta_secs: Option<i64>,
    /// Failures so far.
    pub failed: usize,
}

/// A job as the API shows it.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct Job {
    pub id: i64,
    pub kind: String,
    /// The scheduled task it ran as ("sync", "reviews"), or None for work
    /// that isn't one (a retry of a few titles, a single source).
    pub task: Option<String>,
    pub source: JobSource,
    pub status: JobStatus,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub heartbeat_at: i64,
    pub total: usize,
    pub done: usize,
    pub current: Option<String>,
    pub message: String,
    pub per_minute: Option<f64>,
    pub eta_secs: Option<i64>,
    pub lanes: Vec<LaneProgress>,
    pub failed: usize,
    /// What it did, once it's done: "2,000 games, 3 new".
    pub summary: Option<String>,
    pub error: Option<String>,
    pub options: Value,
    pub cancel_requested: bool,
    /// Its AI calls' tokens, when it made any.
    pub ai_usage: Option<super::JobAiUsage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "info" => Some(Self::Info),
            "warn" | "warning" => Some(Self::Warn),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

/// What a log line says in place of a game hidden from the player reading it.
pub const HIDDEN_GAME: &str = "a hidden game";

/// The names to mask for one reader ([`Db::hidden_names`]): every name of
/// a game hidden from them, longest first, and the names of games they see
/// that hold one of those, which stay as they are.
#[derive(Debug, Default)]
pub struct HiddenNames {
    find: Option<(regex::Regex, std::collections::HashSet<String>)>,
    /// When the names don't fit in one pattern: each, longest first.
    plain: Vec<String>,
}

impl HiddenNames {
    fn new(hidden: &[String], kept: &[String]) -> Self {
        if hidden.is_empty() {
            return Self::default();
        }
        let hide: std::collections::HashSet<String> = hidden.iter().map(|n| n.to_lowercase()).collect();
        let mut all: Vec<&String> = hidden.iter().chain(kept).collect();
        // The longest first: at any place, the alternation takes the first
        // that matches, so "Portal 2" wins over "Portal".
        all.sort_by_key(|n| std::cmp::Reverse(n.len()));
        all.dedup();
        let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        let alternatives: Vec<String> = all
            .iter()
            .map(|n| {
                let (start, end) = (word(n.chars().next()), word(n.chars().last()));
                format!("{}{}{}", if start { r"\b" } else { "" }, regex::escape(n), if end { r"\b" } else { "" })
            })
            .collect();
        match regex::RegexBuilder::new(&alternatives.join("|")).case_insensitive(true).size_limit(1 << 26).build() {
            Ok(re) => Self { find: Some((re, hide)), plain: vec![] },
            Err(_) => {
                let mut plain = hidden.to_vec();
                plain.sort_by_key(|n| std::cmp::Reverse(n.len()));
                Self { find: None, plain }
            }
        }
    }

    /// Whether there's anything to mask.
    pub fn is_empty(&self) -> bool {
        self.find.is_none() && self.plain.is_empty()
    }

    /// `text` with each hidden game's name said as "a hidden game".
    pub fn mask(&self, text: &mut String) {
        for n in &self.plain {
            *text = text.replace(n.as_str(), HIDDEN_GAME);
        }
        let Some((re, hide)) = &self.find else { return };
        if !re.is_match(text) {
            return;
        }
        *text = re
            .replace_all(text, |m: &regex::Captures<'_>| {
                let found = &m[0];
                if hide.contains(&found.to_lowercase()) { HIDDEN_GAME.to_string() } else { found.to_string() }
            })
            .into_owned();
    }

    pub fn lines(&self, lines: &mut [LoggedLine]) {
        for l in lines {
            self.mask(&mut l.line.message);
        }
    }

    pub fn failures(&self, failures: &mut [Failure]) {
        for f in failures {
            if let Some(name) = f.name.as_mut() {
                self.mask(name);
            }
            self.mask(&mut f.error);
        }
    }

    /// Everything a job says: its message, the title it's on, each lane's,
    /// its summary and its error.
    pub fn job(&self, job: &mut Job) {
        if self.is_empty() {
            return;
        }
        self.mask(&mut job.message);
        for text in [&mut job.current, &mut job.summary, &mut job.error].into_iter().flatten() {
            self.mask(text);
        }
        for lane in &mut job.lanes {
            for text in [&mut lane.current, &mut lane.note].into_iter().flatten() {
                self.mask(text);
            }
        }
    }
}

/// One line of a job's log.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LogLine {
    pub at: i64,
    pub level: Level,
    /// The lane it came from, when it came from one.
    pub lane: Option<String>,
    pub message: String,
    /// The game the line names, when it names one: a player it
    /// is hidden from reads "a hidden game" in its place.
    #[serde(skip)]
    pub title_id: Option<i64>,
}

/// A log line as stored, with its place in the log.
#[derive(Debug, Clone, Serialize)]
pub struct LoggedLine {
    pub id: i64,
    #[serde(flatten)]
    pub line: LogLine,
}

/// Something that failed for one title (or, for a pass, for the library).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Failure {
    pub lane: String,
    /// What failed: "Steam reviews", "store search", "review digests".
    pub source: String,
    /// The enrich options that fetch it, to run it again: `["reviews", "review_texts"]`.
    /// Empty for the Steam store step, which always runs.
    pub options: Vec<String>,
    pub title_id: Option<i64>,
    /// The title's name, or what else it was about (a series).
    pub name: Option<String>,
    pub error: String,
    #[serde(default)]
    pub at: i64,
}

/// How a job ended.
#[derive(Debug, Clone, Copy)]
pub struct JobEnd<'a> {
    pub status: JobStatus,
    /// What it did: "2,000 games, 3 new".
    pub summary: Option<&'a str>,
    pub error: Option<&'a str>,
}

/// What a heartbeat learned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pulse {
    /// The row is still running. False when another process declared it
    /// dead (its heartbeat went stale), so this one should stop.
    pub alive: bool,
    /// Someone asked it to stop.
    pub cancel: bool,
}

const JOB_COLUMNS: &str = "id, kind, source, status, started_at, finished_at, heartbeat_at, total, done, current, message, \
     per_minute, eta_secs, lanes, failed, summary, error, options, cancel, task";

/// A lane still running or waiting when its run ends, however it ends,
/// stops there, saying why.
pub fn stop_unfinished_lanes(lanes: &mut [LaneProgress], why: &str) {
    for l in lanes.iter_mut().filter(|l| matches!(l.state, LaneState::Running | LaneState::Waiting)) {
        l.state = LaneState::Stopped;
        l.note = Some(why.to_string());
        l.retry_at = None;
        l.current = None;
    }
}

fn job_from_row(r: &Row<'_>) -> rusqlite::Result<Job> {
    let lanes: Option<String> = r.get(13)?;
    let options: String = r.get(17)?;
    Ok(Job {
        id: r.get(0)?,
        kind: r.get(1)?,
        task: r.get(19)?,
        source: JobSource::parse(&r.get::<_, String>(2)?),
        status: JobStatus::parse(&r.get::<_, String>(3)?),
        started_at: r.get(4)?,
        finished_at: r.get(5)?,
        heartbeat_at: r.get(6)?,
        total: r.get::<_, i64>(7)?.max(0) as usize,
        done: r.get::<_, i64>(8)?.max(0) as usize,
        current: r.get(9)?,
        message: r.get(10)?,
        per_minute: r.get(11)?,
        eta_secs: r.get(12)?,
        lanes: {
            let mut lanes: Vec<LaneProgress> = lanes.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
            // A run that ended without saying so (closed, killed) left its
            // lanes as they were; none of them runs any more.
            if r.get::<_, String>(3)? != "running" {
                stop_unfinished_lanes(&mut lanes, "the run ended before it finished");
            }
            lanes
        },
        failed: r.get::<_, i64>(14)?.max(0) as usize,
        summary: r.get(15)?,
        error: r.get(16)?,
        options: serde_json::from_str(&options).unwrap_or(Value::Null),
        cancel_requested: r.get::<_, i64>(18)? != 0,
        ai_usage: None,
    })
}

/// Running jobs whose process stopped writing are failed, ended at their last heartbeat.
fn reap_stale(c: &Connection) -> Result<()> {
    c.execute(
        "UPDATE jobs SET status='failed', finished_at=heartbeat_at, current=NULL,
             error='Stopped without finishing: the process running it ended (closed, killed or crashed).'
         WHERE status='running' AND heartbeat_at < ?1",
        params![now() - STALE_SECS],
    )?;
    Ok(())
}

fn running_in(c: &Connection) -> Result<Option<Job>> {
    Ok(c.query_row(&format!("SELECT {JOB_COLUMNS} FROM jobs WHERE status='running' ORDER BY id DESC LIMIT 1"), [], job_from_row)
        .optional()?)
}

/// Write progress and the heartbeat, if the row is still running.
fn write_progress(c: &Connection, id: i64, p: &JobProgress) -> Result<bool> {
    let n = c.execute(
        "UPDATE jobs SET heartbeat_at=?2, total=?3, done=?4, current=?5, message=?6, lanes=?7, per_minute=?8, eta_secs=?9, failed=?10
         WHERE id=?1 AND status='running'",
        params![
            id,
            now(),
            p.total as i64,
            p.done as i64,
            p.current,
            p.message,
            (!p.lanes.is_empty()).then(|| super::to_json(&p.lanes)),
            p.per_minute,
            p.eta_secs,
            p.failed as i64
        ],
    )?;
    Ok(n > 0)
}

fn append(c: &Connection, id: i64, lines: &[LogLine], failures: &[Failure]) -> Result<()> {
    let mut log = c.prepare_cached("INSERT INTO job_log(job_id, at, level, lane, message, title_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")?;
    for l in lines {
        log.execute(params![id, l.at, l.level.as_str(), l.lane, l.message, l.title_id])?;
    }
    let mut fail = c.prepare_cached(
        "INSERT INTO job_failures(job_id, at, lane, source, options, title_id, name, error) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    for f in failures {
        fail.execute(params![id, f.at, f.lane, f.source, super::to_json(&f.options), f.title_id, f.name, f.error])?;
    }
    Ok(())
}

fn with_ai_usage(c: &Connection, job: Option<Job>) -> Result<Option<Job>> {
    let mut jobs: Vec<Job> = job.into_iter().collect();
    super::ai_calls::attach_ai_usage(c, &mut jobs)?;
    Ok(jobs.pop())
}

/// Each task's last run and last run that ended done, by job id.
const TASK_RUNS: &str = "SELECT MAX(id) FROM jobs WHERE task IS NOT NULL AND status<>'running' GROUP BY task
     UNION SELECT MAX(id) FROM jobs WHERE task IS NOT NULL AND status='done' GROUP BY task";

/// Keep the newest `keep` jobs, and each task's last run and last run that
/// ended done however old (a monthly task's would otherwise be gone before
/// it's due again); older ones go with their logs and failures.
fn trim(c: &Connection, keep: usize) -> Result<()> {
    let cutoff: Option<i64> =
        c.query_row("SELECT id FROM jobs ORDER BY id DESC LIMIT 1 OFFSET ?1", params![keep as i64 - 1], |r| r.get(0)).optional()?;
    if let Some(cutoff) = cutoff {
        let old = format!("SELECT id FROM jobs WHERE id < ?1 AND status<>'running' AND id NOT IN ({TASK_RUNS})");
        c.execute(&format!("DELETE FROM job_log WHERE job_id IN ({old})"), params![cutoff])?;
        c.execute(&format!("DELETE FROM job_failures WHERE job_id IN ({old})"), params![cutoff])?;
        c.execute(&format!("DELETE FROM jobs WHERE id IN ({old})"), params![cutoff])?;
    }
    Ok(())
}

/// The settings row that holds a task's schedule (`jobs::tasks`).
pub fn schedule_key(task: &str) -> String {
    format!("schedule.{task}")
}

/// Which finished jobs `Db::jobs_recent` lists.
#[derive(Debug, Clone, Default)]
pub struct JobFilter {
    /// "sync", "enrich" or "backup".
    pub kind: Option<String>,
    /// A task's id; `Some(None)` for jobs that weren't a task.
    pub task: Option<Option<String>>,
    /// Only jobs that failed, or finished with failures.
    pub failed: bool,
    pub limit: usize,
}

/// A task's last run, and its last run that ended done (the same job when
/// the last one did), for its schedule.
#[derive(Debug, Clone, Default)]
pub struct TaskRuns {
    pub last: Option<Job>,
    pub last_done: Option<Job>,
}

impl Db {
    /// Start a job, unless one is running here or in another process: then
    /// `Err` with that job. The check and the insert are one write
    /// transaction, so two processes can't both start one.
    pub fn job_begin(&self, kind: &str, task: Option<&str>, source: JobSource, options: &Value) -> Result<std::result::Result<i64, Job>> {
        self.with(|c| {
            let tx = rusqlite::Transaction::new_unchecked(c, TransactionBehavior::Immediate)?;
            reap_stale(&tx)?;
            let result = match running_in(&tx)? {
                Some(job) => Err(job),
                None => {
                    tx.execute(
                        "INSERT INTO jobs(kind, task, source, options, status, started_at, heartbeat_at, message)
                         VALUES (?1, ?2, ?3, ?4, 'running', ?5, ?5, 'Starting')",
                        params![kind, task, source.as_str(), options.to_string(), now()],
                    )?;
                    Ok(tx.last_insert_rowid())
                }
            };
            tx.commit()?;
            Ok(result)
        })
    }

    /// The heartbeat: progress, new log lines and failures in one write.
    pub fn job_pulse(&self, id: i64, p: &JobProgress, lines: &[LogLine], failures: &[Failure]) -> Result<Pulse> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            let alive = write_progress(&tx, id, p)?;
            append(&tx, id, lines, failures)?;
            let cancel: bool = tx.query_row("SELECT cancel FROM jobs WHERE id=?1", params![id], |r| r.get::<_, i64>(0))? != 0;
            tx.commit()?;
            Ok(Pulse { alive, cancel })
        })
    }

    /// End a job: its last progress, lines and failures, and how it went.
    /// Old jobs beyond `KEEP_JOBS` are deleted.
    pub fn job_end(&self, id: i64, p: &JobProgress, lines: &[LogLine], failures: &[Failure], end: &JobEnd<'_>) -> Result<()> {
        let JobEnd { status, summary, error } = *end;
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            write_progress(&tx, id, p)?;
            append(&tx, id, lines, failures)?;
            tx.execute(
                "UPDATE jobs SET status=?2, finished_at=?3, current=NULL, eta_secs=NULL, summary=?4, error=?5 WHERE id=?1",
                params![id, status.as_str(), now(), summary, error],
            )?;
            trim(&tx, KEEP_JOBS)?;
            tx.commit()?;
            Ok(())
        })
    }

    pub fn job(&self, id: i64) -> Result<Option<Job>> {
        self.with(|c| {
            reap_stale(c)?;
            let job = c.query_row(&format!("SELECT {JOB_COLUMNS} FROM jobs WHERE id=?1"), params![id], job_from_row).optional()?;
            with_ai_usage(c, job)
        })
    }

    /// The job running now, in this process or another.
    pub fn job_running(&self) -> Result<Option<Job>> {
        self.with(|c| {
            reap_stale(c)?;
            with_ai_usage(c, running_in(c)?)
        })
    }

    /// Finished jobs, newest first.
    pub fn jobs_recent(&self, f: &JobFilter) -> Result<Vec<Job>> {
        self.with(|c| {
            reap_stale(c)?;
            let (task, untasked) = match &f.task {
                None => (None, false),
                Some(None) => (None, true),
                Some(Some(t)) => (Some(t.as_str()), false),
            };
            let mut stmt = c.prepare(&format!(
                "SELECT {JOB_COLUMNS} FROM jobs WHERE status<>'running'
                   AND (?1 IS NULL OR kind=?1) AND (?2 IS NULL OR task=?2) AND (?3=0 OR task IS NULL)
                   AND (?4=0 OR status='failed' OR failed>0)
                 ORDER BY id DESC LIMIT ?5"
            ))?;
            let rows = stmt.query_map(params![f.kind, task, untasked, f.failed, f.limit as i64], job_from_row)?;
            let mut jobs: Vec<Job> = rows.collect::<std::result::Result<_, _>>()?;
            super::ai_calls::attach_ai_usage(c, &mut jobs)?;
            Ok(jobs)
        })
    }

    /// Each task's last run and last run that ended done, by task id.
    /// Running jobs aren't runs yet.
    pub fn task_runs(&self) -> Result<std::collections::BTreeMap<String, TaskRuns>> {
        self.with(|c| {
            reap_stale(c)?;
            let mut stmt = c.prepare(&format!("SELECT {JOB_COLUMNS} FROM jobs WHERE id IN ({TASK_RUNS}) ORDER BY id"))?;
            let rows = stmt.query_map([], job_from_row)?;
            let mut jobs: Vec<Job> = rows.collect::<std::result::Result<_, _>>()?;
            super::ai_calls::attach_ai_usage(c, &mut jobs)?;
            let mut runs: std::collections::BTreeMap<String, TaskRuns> = Default::default();
            // Oldest first, so the newest of each ends up in place.
            for job in jobs {
                let Some(task) = job.task.clone() else { continue };
                let entry = runs.entry(task).or_default();
                if job.status == JobStatus::Done {
                    entry.last_done = Some(job.clone());
                }
                entry.last = Some(job);
            }
            Ok(runs)
        })
    }

    /// Ask the running job to stop. The process running it sees the flag at
    /// its next heartbeat. None when nothing is running.
    pub fn job_request_cancel(&self) -> Result<Option<Job>> {
        self.with(|c| {
            reap_stale(c)?;
            let Some(job) = running_in(c)? else { return Ok(None) };
            c.execute("UPDATE jobs SET cancel=1, message='Cancelling…' WHERE id=?1", params![job.id])?;
            Ok(c.query_row(&format!("SELECT {JOB_COLUMNS} FROM jobs WHERE id=?1"), params![job.id], job_from_row).optional()?)
        })
    }

    /// Ask job `id` to stop, if it is still the one running. Whether it was.
    pub fn job_request_cancel_of(&self, id: i64) -> Result<bool> {
        self.with(|c| Ok(c.execute("UPDATE jobs SET cancel=1, message='Cancelling…' WHERE id=?1 AND status='running'", params![id])? > 0))
    }

    /// A job's log from after line `after`, at `min` level or above, oldest first.
    pub fn job_log(&self, id: i64, after: i64, min: Level, limit: usize) -> Result<Vec<LoggedLine>> {
        let levels: Vec<&str> = [Level::Info, Level::Warn, Level::Error].into_iter().filter(|l| *l >= min).map(Level::as_str).collect();
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, at, level, lane, message, title_id FROM job_log
                 WHERE job_id=?1 AND id>?2 AND level IN (SELECT value FROM json_each(?3)) ORDER BY id LIMIT ?4",
            )?;
            let rows = stmt.query_map(params![id, after, super::to_json(&levels), limit as i64], |r| {
                Ok(LoggedLine {
                    id: r.get(0)?,
                    line: LogLine {
                        at: r.get(1)?,
                        level: Level::parse(&r.get::<_, String>(2)?).unwrap_or(Level::Info),
                        lane: r.get(3)?,
                        message: r.get(4)?,
                        title_id: r.get(5)?,
                    },
                })
            })?;
            Ok(rows.collect::<std::result::Result<_, _>>()?)
        })
    }

    /// Say "a hidden game" in place of the name of each game hidden from
    /// player `p` in a job's log lines and failures ("Hiding
    /// games"). [`Db::hidden_names`] says which games, and masks the rest
    /// of what a job says too.
    pub fn mask_hidden(&self, p: PlayerId, lines: &mut [LoggedLine], failures: &mut [Failure]) -> Result<()> {
        let names = self.hidden_names(p)?;
        names.lines(lines);
        names.failures(failures);
        Ok(())
    }

    /// The names of the games hidden from player `p`, to mask wherever a
    /// job says them: games somebody else hid that `p` doesn't see. Whoever
    /// hid a game knows which it is, so it's never masked for them. Every
    /// name a title goes by is masked, whichever line says it and however
    /// it was logged, since a progress message or a failure's text names
    /// games as freely as a log line does.
    ///
    /// It reads only the hidden titles' rows and the names that hold one,
    /// not the whole library: the UI polls `/jobs` while a job runs, and
    /// each of the job's writes would make a library read start over.
    pub fn hidden_names(&self, p: PlayerId) -> Result<HiddenNames> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT DISTINCT title_id FROM hidden_titles
                 WHERE title_id NOT IN (SELECT title_id FROM hidden_titles WHERE player_id=?1)",
            )?;
            let hid: Vec<i64> = st.query_map(params![p], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            if hid.is_empty() {
                return Ok(HiddenNames::default());
            }
            let sight = super::players::Sight::of(c, p)?;
            // Whether `p` sees a title: one of its copies is in an account
            // they see and isn't hidden from them, as their library has it.
            let mut accounts = c.prepare_cached("SELECT account_id FROM games WHERE title_id=?1")?;
            let mut sees = |title: i64| -> Result<bool> {
                if sight.hid(title) {
                    return Ok(false);
                }
                let mut rows = accounts.query_map(params![title], |r| r.get::<_, i64>(0))?;
                Ok(rows.try_fold(false, |seen, a| a.map(|a| seen || sight.sees(a, title)))?)
            };
            let mut hidden: Vec<String> = vec![];
            for title in hid {
                if sees(title)? {
                    continue;
                }
                let mut st = c.prepare_cached(
                    "SELECT name FROM titles WHERE id=?1 UNION SELECT name FROM games WHERE title_id=?1 UNION
                     SELECT edition FROM games WHERE title_id=?1 AND edition IS NOT NULL",
                )?;
                let all: Vec<String> = st.query_map(params![title], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
                hidden.extend(all.into_iter().map(|n| n.trim().to_string()).filter(|n| !n.is_empty()));
            }
            hidden.sort();
            hidden.dedup();
            // A game the player does see whose name holds a hidden one ("Portal
            // 2" beside a hidden "Portal") keeps its name. SQLite's `lower`
            // folds ASCII only, so a name differing past that is masked too,
            // which shows less, never more.
            let mut holding = c.prepare_cached(
                "SELECT t.id, t.name FROM titles t WHERE instr(lower(t.name), lower(?1)) > 0
                 UNION SELECT g.title_id, g.name FROM games g WHERE g.title_id IS NOT NULL AND instr(lower(g.name), lower(?1)) > 0",
            )?;
            let mut kept: Vec<String> = vec![];
            for h in &hidden {
                let rows: Vec<(i64, String)> =
                    holding.query_map(params![h], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
                for (title, name) in rows {
                    if name.to_lowercase() != h.to_lowercase() && sees(title)? {
                        kept.push(name);
                    }
                }
            }
            Ok(HiddenNames::new(&hidden, &kept))
        })
    }

    pub fn job_failures(&self, id: i64) -> Result<Vec<Failure>> {
        self.with(|c| {
            let mut stmt =
                c.prepare("SELECT lane, source, options, title_id, name, error, at FROM job_failures WHERE job_id=?1 ORDER BY id")?;
            let rows = stmt.query_map(params![id], |r| {
                Ok(Failure {
                    lane: r.get(0)?,
                    source: r.get(1)?,
                    options: super::json_vec(r.get(2)?),
                    title_id: r.get(3)?,
                    name: r.get(4)?,
                    error: r.get(5)?,
                    at: r.get(6)?,
                })
            })?;
            Ok(rows.collect::<std::result::Result<_, _>>()?)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn two_handles() -> (tempfile::TempDir, Db, Db) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upnext.db");
        let a = Db::open(&path, 120).unwrap();
        let b = Db::open(&path, 120).unwrap();
        (dir, a, b)
    }

    fn line(level: Level, message: &str) -> LogLine {
        LogLine { at: now(), level, lane: Some("Steam reviews".into()), message: message.into(), title_id: None }
    }

    /// The server and the command line are two processes with their own
    /// connections: a job one starts, the other sees, and can't start a second.
    #[test]
    fn a_job_is_seen_across_connections_and_blocks_a_second() {
        let (_dir, cli, server) = two_handles();
        let id = cli.job_begin("enrich", None, JobSource::Terminal, &json!({"hltb": true})).unwrap().unwrap();
        let progress = JobProgress { total: 1600, done: 1040, message: "in parallel".into(), ..Default::default() };
        cli.job_pulse(id, &progress, &[], &[]).unwrap();

        let seen = server.job_running().unwrap().expect("running");
        assert_eq!((seen.id, seen.source, seen.done, seen.total), (id, JobSource::Terminal, 1040, 1600));
        assert_eq!(seen.options["hltb"], true);
        let refused = server.job_begin("sync", None, JobSource::Ui, &json!({})).unwrap().unwrap_err();
        assert_eq!(refused.id, id, "the running job comes back");

        cli.job_end(id, &progress, &[], &[], &JobEnd { status: JobStatus::Done, summary: Some("1,600 titles"), error: None }).unwrap();
        assert!(server.job_running().unwrap().is_none());
        assert!(server.job_begin("sync", None, JobSource::Ui, &json!({})).unwrap().is_ok(), "free again");
    }

    #[test]
    fn a_stale_heartbeat_counts_as_dead() {
        let (_dir, cli, server) = two_handles();
        let id = cli.job_begin("enrich", None, JobSource::Terminal, &json!({})).unwrap().unwrap();
        cli.with(|c| Ok(c.execute("UPDATE jobs SET heartbeat_at=?1 WHERE id=?2", params![now() - STALE_SECS - 5, id])?)).unwrap();

        assert!(server.job_running().unwrap().is_none(), "no longer running");
        let dead = server.job(id).unwrap().unwrap();
        assert_eq!(dead.status, JobStatus::Failed);
        assert!(dead.error.unwrap().contains("Stopped without finishing"));
        assert!(server.job_begin("sync", None, JobSource::Ui, &json!({})).unwrap().is_ok(), "the slot is free");
        // The process that lost its row learns it at its next heartbeat.
        let pulse = cli.job_pulse(id, &JobProgress::default(), &[], &[]).unwrap();
        assert!(!pulse.alive);
    }

    #[test]
    fn cancelling_sets_a_flag_the_running_process_reads() {
        let (_dir, cli, server) = two_handles();
        let id = cli.job_begin("enrich", None, JobSource::Terminal, &json!({})).unwrap().unwrap();
        assert!(!cli.job_pulse(id, &JobProgress::default(), &[], &[]).unwrap().cancel);
        let asked = server.job_request_cancel().unwrap().expect("a running job");
        assert!(asked.cancel_requested && asked.status == JobStatus::Running);
        assert_eq!(cli.job_pulse(id, &JobProgress::default(), &[], &[]).unwrap(), Pulse { alive: true, cancel: true });
        cli.job_end(
            id,
            &JobProgress::default(),
            &[],
            &[],
            &JobEnd { status: JobStatus::Cancelled, summary: None, error: Some("cancelled") },
        )
        .unwrap();
        assert!(server.job_request_cancel().unwrap().is_none(), "nothing left to cancel");
    }

    #[test]
    fn jobs_and_logs_survive_a_reopen_and_the_oldest_are_trimmed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upnext.db");
        let failure = Failure {
            lane: "Steam reviews".into(),
            source: "Steam reviews".into(),
            options: vec!["reviews".into(), "review_texts".into()],
            title_id: Some(7),
            name: Some("Portal 2".into()),
            error: "HTTP 429 after 5 tries".into(),
            at: now(),
        };
        let first = {
            let db = Db::open(&path, 120).unwrap();
            let id = db.job_begin("enrich", None, JobSource::Ui, &json!({})).unwrap().unwrap();
            let lines = [line(Level::Info, "2 titles need enrichment"), line(Level::Warn, "Steam asked us to slow down")];
            db.job_pulse(id, &JobProgress::default(), &lines, &[]).unwrap();
            let last = [line(Level::Error, "Steam reviews failed for Portal 2")];
            db.job_end(
                id,
                &JobProgress::default(),
                &last,
                std::slice::from_ref(&failure),
                &JobEnd { status: JobStatus::Done, summary: Some("2 titles"), error: None },
            )
            .unwrap();
            id
        };
        let db = Db::open(&path, 120).unwrap();
        let job = db.job(first).unwrap().unwrap();
        assert_eq!((job.status, job.summary.as_deref()), (JobStatus::Done, Some("2 titles")));
        let all = db.job_log(first, 0, Level::Info, 100).unwrap();
        assert_eq!(all.len(), 3);
        let warnings = db.job_log(first, 0, Level::Warn, 100).unwrap();
        assert_eq!(
            warnings.iter().map(|l| l.line.message.as_str()).collect::<Vec<_>>(),
            ["Steam asked us to slow down", "Steam reviews failed for Portal 2"]
        );
        assert_eq!(db.job_log(first, all[1].id, Level::Info, 100).unwrap().len(), 1, "paged after a line");
        assert_eq!(db.job_failures(first).unwrap(), std::slice::from_ref(&failure));

        for _ in 0..KEEP_JOBS {
            let id = db.job_begin("sync", None, JobSource::Schedule, &json!({})).unwrap().unwrap();
            db.job_end(id, &JobProgress::default(), &[], &[], &JobEnd { status: JobStatus::Done, summary: None, error: None }).unwrap();
        }
        assert!(db.job(first).unwrap().is_none(), "the oldest went");
        assert!(db.job_log(first, 0, Level::Info, 100).unwrap().is_empty() && db.job_failures(first).unwrap().is_empty());
        assert_eq!(db.jobs_recent(&JobFilter { limit: 100, ..Default::default() }).unwrap().len(), KEEP_JOBS);
    }

    fn run(db: &Db, kind: &str, task: Option<&str>, status: JobStatus, failed: usize) -> i64 {
        let id = db.job_begin(kind, task, JobSource::Schedule, &json!({})).unwrap().unwrap();
        let progress = JobProgress { failed, ..Default::default() };
        db.job_end(id, &progress, &[], &[], &JobEnd { status, summary: None, error: None }).unwrap();
        id
    }

    /// A monthly task's last run outlives 50 daily ones, or the schedule
    /// would think it never ran and start it early.
    #[test]
    fn trimming_keeps_each_tasks_last_runs() {
        let (_dir, db, _) = two_handles();
        let done = run(&db, "enrich", Some("reviews"), JobStatus::Done, 5);
        let failed = run(&db, "enrich", Some("reviews"), JobStatus::Failed, 0);
        let untasked = run(&db, "enrich", None, JobStatus::Done, 0);
        for _ in 0..KEEP_JOBS {
            run(&db, "sync", Some("sync"), JobStatus::Done, 0);
        }
        assert!(db.job(untasked).unwrap().is_none(), "old runs that were no task's go");
        let runs = db.task_runs().unwrap();
        assert_eq!(runs["reviews"].last.as_ref().map(|j| j.id), Some(failed));
        assert_eq!(runs["reviews"].last_done.as_ref().map(|j| (j.id, j.failed)), Some((done, 5)));
        assert_eq!(runs["sync"].last.as_ref().map(|j| j.task.as_deref()), Some(Some("sync")));
        assert!(!runs.contains_key("backup"));

        // The History filters.
        let list = |f: JobFilter| db.jobs_recent(&JobFilter { limit: 200, ..f }).unwrap().iter().map(|j| j.id).collect::<Vec<_>>();
        assert_eq!(list(JobFilter { task: Some(Some("reviews".into())), ..Default::default() }), [failed, done]);
        assert_eq!(list(JobFilter { failed: true, ..Default::default() }), [failed, done], "failed, or done with failures");
        assert_eq!(list(JobFilter { kind: Some("enrich".into()), ..Default::default() }), [failed, done]);
        let other = run(&db, "enrich", None, JobStatus::Cancelled, 0);
        assert_eq!(list(JobFilter { task: Some(None), ..Default::default() }), [other]);
    }
}
