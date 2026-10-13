//! Background jobs (sync, enrich, backup, restore, import, the taste profile). One runs at a
//! time across processes: the server's runner and `upnext sync`/`enrich`/
//! `backup` in a terminal all claim the slot in the `jobs` table and record
//! their progress there (`LiveJob`). The scheduled tasks and their schedules
//! are in `tasks`.

pub mod backup;
mod context;
mod live;
mod log_layer;
pub mod tasks;

pub use context::{current_job, in_lane, stop_asked, waiting};
pub use live::{LiveJob, Wait};
pub use log_layer::{ServerLine, layer as log_layer, recent as recent_log};

use crate::config::Config;
use crate::db::{Db, Failure, Job, JobSource};
use crate::enrich::{self, Clients, EnrichOptions, SyncOptions};
use crate::llm::LlmBackend;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
pub use tasks::Task;

pub enum JobKind {
    Sync(SyncOptions),
    Enrich(EnrichOptions),
    Backup(BackupOptions),
    /// Put a backup from the folder in place of the database (the server only).
    Restore(RestoreOptions),
    /// Put an uploaded database in place of it (the server only).
    Import(ImportOptions),
    /// Count the taste profile's patterns again (`taste::update`).
    Taste,
    /// Download other players' backlog verdicts (`verdicts::receive`).
    Verdicts,
}

/// A backup of the database (`backup.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupOptions {
    /// This job's backups kept; older ones are removed.
    #[serde(default = "default_keep")]
    pub keep: usize,
}

fn default_keep() -> usize {
    tasks::DEFAULT_KEEP
}

/// A restore (`backup::restore`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreOptions {
    /// The backup's file name in the backups folder.
    pub name: String,
}

/// An import (`backup::import`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportOptions {
    /// The uploaded file's name, as the computer it came from called it.
    pub name: String,
    /// Where its checked copy waits, in the backups folder.
    pub staged: String,
}

impl JobKind {
    /// Every kind's [`name`](Self::name), for whatever takes a kind by name.
    pub const NAMES: [&'static str; 7] = ["sync", "enrich", "backup", "restore", "import", "taste", "verdicts"];

    pub fn name(&self) -> &'static str {
        match self {
            Self::Sync(_) => "sync",
            Self::Enrich(_) => "enrich",
            Self::Backup(_) => "backup",
            Self::Restore(_) => "restore",
            Self::Import(_) => "import",
            Self::Taste => "taste",
            Self::Verdicts => "verdicts",
        }
    }

    /// Every option spelled out, as the job records them. A source these
    /// settings block (ProtonDB with Linux off) is recorded off, since the
    /// run leaves it out.
    pub fn options(&self, cfg: &Config) -> Value {
        match self {
            Self::Sync(o) => serde_json::to_value(o.resolved()),
            Self::Enrich(o) => {
                let mut o = o.resolved();
                for opt in enrich::sources::options().iter().filter(|opt| (opt.blocked)(cfg).is_some()) {
                    o.sources.insert(opt.id.to_string(), false);
                }
                serde_json::to_value(o)
            }
            Self::Backup(o) => serde_json::to_value(o),
            Self::Restore(o) => serde_json::to_value(o),
            Self::Import(o) => serde_json::to_value(o),
            Self::Taste | Self::Verdicts => Ok(serde_json::json!({})),
        }
        .unwrap_or(Value::Null)
    }

    /// The task it counts as, when not started as one (`tasks::task_for`).
    pub fn task(&self, db: &Db, cfg: &Config) -> Option<Task> {
        let setups = tasks::setups(db).unwrap_or_default();
        tasks::task_for(self, &setups, cfg)
    }
}

/// "2,000 games, 3 new": what a sync did. A sync where an account, or every
/// account of a store, failed while the rest synced says it's partial and
/// which failed: "Partial: 2,000 games, 3 new; Steam (Sam) failed".
pub fn sync_summary(report: &enrich::SyncReport) -> String {
    let counts = enrich::sync_counts_text(&report.summaries);
    if report.failed.is_empty() { counts } else { format!("Partial: {counts}; {} failed", report.failed.join(", ")) }
}

/// Says what's running, for a refusal: "an enrich started from the
/// terminal 5 min ago is running (1,040 of 1,600)".
pub fn busy_text(job: &Job) -> String {
    let from = job.source.started_from();
    let mins = (crate::db::now() - job.started_at).max(0) / 60;
    let ago = if mins == 0 { "just now".to_string() } else { format!("{mins} min ago") };
    let article = if job.kind.starts_with(['a', 'e', 'i', 'o', 'u']) { "an" } else { "a" };
    let mut text = format!("{article} {} started {from} {ago} is running", job.kind);
    if job.total > 0 {
        text.push_str(&format!(" ({} of {})", enrich::thousands(job.done), enrich::thousands(job.total)));
    }
    text
}

/// The enrich options that run a finished job's failures again: the sources
/// that failed, for the titles they failed on, refreshing if the job did.
/// None when nothing can be run again (no titles, or only failures of
/// passes that can't be narrowed to titles).
pub fn retry_options(job: &Job, failures: &[Failure]) -> Option<EnrichOptions> {
    let mut titles: Vec<i64> = failures.iter().filter_map(|f| f.title_id).collect();
    titles.sort_unstable();
    titles.dedup();
    if job.kind != "enrich" || titles.is_empty() {
        return None;
    }
    let mut sources: BTreeMap<String, bool> = enrich::sources::ids().into_iter().map(|id| (id.to_string(), false)).collect();
    for f in failures.iter().filter(|f| f.title_id.is_some()) {
        for o in &f.options {
            sources.insert(o.clone(), true);
        }
    }
    let refresh = job.options.get("refresh").and_then(Value::as_bool).unwrap_or(false);
    Some(EnrichOptions { sources, refresh, limit: None, titles })
}

/// The server's side: starts jobs in the background and cancels them. What's
/// running, here or in a terminal, is read from the database.
#[derive(Default)]
pub struct JobRunner {
    /// The job this process is running, to cancel it at once.
    own: Mutex<Option<Arc<LiveJob>>>,
    /// The server is stopping: no new jobs.
    closing: AtomicBool,
}

impl JobRunner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask the running job to stop, wherever it runs. None when none is.
    pub fn cancel(&self, db: &Db) -> Result<Option<Job>> {
        let job = db.job_request_cancel()?;
        if let Some(j) = &job
            && let Some(own) = self.own.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
            && own.id == j.id
        {
            own.cancel_now();
        }
        Ok(job)
    }

    /// For a shutdown: start no more jobs, ask this process's job to stop,
    /// and wait up to `grace` for it to stop and record how far it got (the
    /// next run starts from there). Whether it stopped in time.
    pub async fn shut_down(&self, grace: Duration) -> bool {
        self.closing.store(true, Ordering::SeqCst);
        let own = self.own.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let Some(job) = own else { return true };
        job.stop_because("stopping: UpNext is shutting down");
        let deadline = tokio::time::Instant::now() + grace;
        while tokio::time::Instant::now() < deadline {
            if self.own.lock().unwrap_or_else(|e| e.into_inner()).is_none() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    /// Whether the server is shutting down.
    pub fn closing(&self) -> bool {
        self.closing.load(Ordering::SeqCst)
    }

    /// Start a job in the background, as `task` or as the task it does the
    /// work of. `Ok(Err(running))` when one is already running, here or in a
    /// terminal; an error when the server is shutting down.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        self: &Arc<Self>,
        kind: JobKind,
        task: Option<Task>,
        source: JobSource,
        db: Arc<Db>,
        cfg: Config,
        clients: Arc<Clients>,
        llm: Arc<dyn LlmBackend>,
    ) -> Result<std::result::Result<Job, Job>> {
        if self.closing() {
            anyhow::bail!("UpNext is shutting down");
        }
        let task = task.or_else(|| kind.task(&db, &cfg));
        let live = match LiveJob::begin(db.clone(), kind.name(), task.map(Task::id), source, kind.options(&cfg), None)? {
            Ok(live) => live,
            Err(running) => return Ok(Err(running)),
        };
        // Read back before it counts as this process's job: if that fails,
        // the job ends failed here, or its heartbeat would keep the row
        // running and every later job would wait on it until a restart
        // (B-CORE-6).
        let started = match db.job(live.id).and_then(|j| j.ok_or_else(|| anyhow::anyhow!("job {} vanished", live.id))) {
            Ok(j) => j,
            Err(e) => {
                live.finish(Err(&e));
                return Err(e);
            }
        };
        *self.own.lock().unwrap_or_else(|e| e.into_inner()) = Some(live.clone());
        let id = live.id;
        let runner = self.clone();
        tokio::spawn(async move {
            // The work runs in a task of its own so a panic becomes a failed
            // job instead of a job left running.
            let work = {
                let live = live.clone();
                tokio::spawn(async move {
                    let sink = &*live;
                    let work = async {
                        match kind {
                            JobKind::Sync(o) => enrich::run_sync(&db, &cfg, &clients, &o, sink).await.map(|s| sync_summary(&s)),
                            JobKind::Enrich(o) => {
                                enrich::run_enrich(&db, &cfg, &clients, Some(llm.as_ref()), &o, sink).await.map(|s| s.summary_text())
                            }
                            JobKind::Backup(o) => backup::run(&cfg, id, &o, sink).await,
                            JobKind::Restore(o) => backup::restore(&db, &cfg, id, &o, sink).await,
                            JobKind::Import(o) => backup::import(&db, &cfg, id, &o, sink).await,
                            JobKind::Taste => crate::taste::run_for_everyone(&db, &cfg, sink).await,
                            JobKind::Verdicts => crate::verdicts::receive::run_job(&db, &cfg, &clients.http, sink).await,
                        }
                    };
                    live.scope(work).await
                })
            };
            let result = match work.await {
                Ok(r) => r,
                Err(e) => Err(anyhow::anyhow!("the job crashed: {e}")),
            };
            live.finish(result.as_ref().map(String::clone));
            let mut own = runner.own.lock().unwrap_or_else(|e| e.into_inner());
            if own.as_ref().is_some_and(|o| o.id == live.id) {
                *own = None;
            }
        });
        Ok(Ok(started))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sync with an account that failed is partial, and says which.
    #[test]
    fn a_partial_sync_says_so() {
        let summary = |total, added| crate::db::SyncSummary { source: "steam".into(), total, added, ..Default::default() };
        let mut report = enrich::SyncReport { summaries: vec![summary(1500, 2), summary(500, 1)], failed: vec![] };
        assert_eq!(sync_summary(&report), "2,000 games, 3 new");
        report.failed = vec!["Steam (Sam)".into(), "Heroic (Alex)".into()];
        assert_eq!(sync_summary(&report), "Partial: 2,000 games, 3 new; Steam (Sam), Heroic (Alex) failed");
    }

    /// `NAMES` is every kind's `name()`, once each: a kind added to one and
    /// not the other fails here (a new variant also stops the match below
    /// from compiling until it's listed).
    #[test]
    fn names_lists_every_kind() {
        let kinds = [
            JobKind::Sync(SyncOptions::default()),
            JobKind::Enrich(EnrichOptions::default()),
            JobKind::Backup(BackupOptions { keep: 1 }),
            JobKind::Restore(RestoreOptions { name: "upnext-manual-20261001-101500.db".into() }),
            JobKind::Import(ImportOptions { name: "upnext.db".into(), staged: "upnext-import.db".into() }),
            JobKind::Taste,
            JobKind::Verdicts,
        ];
        for k in &kinds {
            match k {
                JobKind::Sync(_)
                | JobKind::Enrich(_)
                | JobKind::Backup(_)
                | JobKind::Restore(_)
                | JobKind::Import(_)
                | JobKind::Taste
                | JobKind::Verdicts => {}
            }
        }
        let mut names: Vec<&str> = kinds.iter().map(JobKind::name).collect();
        let mut listed = JobKind::NAMES.to_vec();
        names.sort_unstable();
        listed.sort_unstable();
        assert_eq!(names, listed);
        names.dedup();
        assert_eq!(names.len(), JobKind::NAMES.len(), "each once");
    }

    /// A job whose row can't be read back right after it's written ends
    /// failed, so the next job starts; before, its heartbeat kept the row
    /// running and every later job was refused until a restart (B-CORE-6).
    #[tokio::test]
    async fn a_job_that_fails_to_start_frees_the_slot() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = crate::testutil::offline_config(dir.path());
        let db = Arc::new(Db::open(&cfg.db_path, cfg.sampled_minutes).unwrap());
        // Each new row is made unreadable: its options become bytes, not text.
        let raw = rusqlite::Connection::open(db.path()).unwrap();
        raw.execute_batch("CREATE TRIGGER unreadable AFTER INSERT ON jobs BEGIN UPDATE jobs SET options = x'ff' WHERE id = NEW.id; END;")
            .unwrap();
        let clients = Arc::new(Clients::with_base_url(&cfg, None).unwrap());
        let llm: Arc<dyn LlmBackend> = Arc::new(crate::testutil::ScriptedLlm::new(vec![]));
        let runner = Arc::new(JobRunner::new());
        let start = || runner.start(JobKind::Taste, None, JobSource::Ui, db.clone(), cfg.clone(), clients.clone(), llm.clone());

        let e = start().unwrap_err();
        raw.execute_batch("DROP TRIGGER unreadable; UPDATE jobs SET options = '{}';").unwrap();
        let first = db.job(1).unwrap().expect("the row was written");
        assert_eq!(first.status, crate::db::JobStatus::Failed, "it ended with the error: {e:#}");
        let next = start().unwrap().expect("the slot is free, not held by the job that never started");
        assert_ne!(next.id, first.id);
    }

    /// A source these settings block is recorded off, as the run leaves it
    /// out (ProtonDB with Linux off).
    #[test]
    fn recorded_options_leave_out_blocked_sources() {
        let mut cfg = Config::built_in();
        let kind = JobKind::Enrich(EnrichOptions::default());
        cfg.linux_mode = "off".into();
        assert_eq!(kind.options(&cfg)["protondb"], false);
        assert_eq!(kind.options(&cfg)["pcgw"], true, "others as they were");
        cfg.linux_mode = "on".into();
        assert_eq!(kind.options(&cfg)["protondb"], true);
    }
}
