//! `sync`, `enrich` and `backup`, with a progress bar. They run as jobs the
//! UI can see and cancel, like the ones it starts itself, and count as the
//! scheduled task whose work they do.

use super::format::{green, red, trunc};
use super::open_library_db;
use crate::config::Config;
use crate::db::{Db, JobSource, JobStatus, LaneProgress, LaneState, Level};
use crate::enrich::{self, Clients, EnrichOptions, Progress, ProgressSink, SyncOptions};
use crate::jobs::{self, BackupOptions, JobKind, LiveJob, tasks};
use crate::llm;
use anyhow::{Context, Result, bail};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

struct BarSink {
    bar: ProgressBar,
    total: Mutex<usize>,
    /// What a lane is waiting on, shown after the message.
    waiting: Mutex<Option<String>>,
    message: Mutex<String>,
}

impl BarSink {
    fn new() -> Self {
        let bar = ProgressBar::new(0);
        bar.set_style(
            ProgressStyle::with_template("{spinner:.green} [{bar:30.cyan/blue}] {pos}/{len} {msg}").unwrap().progress_chars("=> "),
        );
        bar.enable_steady_tick(std::time::Duration::from_millis(120));
        Self { bar, total: Mutex::new(0), waiting: Mutex::new(None), message: Mutex::new(String::new()) }
    }

    fn show(&self) {
        let msg = self.message.lock().unwrap().clone();
        match self.waiting.lock().unwrap().as_deref() {
            Some(w) => self.bar.set_message(format!("{msg} · {w}")),
            None => self.bar.set_message(msg),
        }
    }
}

impl ProgressSink for BarSink {
    fn update(&self, p: &Progress) {
        let mut t = self.total.lock().unwrap();
        if *t != p.total {
            self.bar.set_length(p.total as u64);
            *t = p.total;
        }
        self.bar.set_position(p.done as u64);
        *self.message.lock().unwrap() = match &p.current {
            Some(c) => format!("{} — {}", trunc(c, 40), p.message),
            None => p.message.clone(),
        };
        self.show();
    }
    fn log(&self, line: String) {
        if self.bar.is_hidden() {
            eprintln!("  {line}");
        } else {
            self.bar.println(format!("  {line}"));
        }
    }
    fn lanes(&self, lanes: &[LaneProgress]) {
        let waiting: Vec<String> = lanes.iter().filter(|l| l.state == LaneState::Waiting).map(|l| format!("{} waiting", l.name)).collect();
        *self.waiting.lock().unwrap() = (!waiting.is_empty()).then(|| waiting.join(", "));
        self.show();
    }
}

/// Claim the job slot for this terminal, with a progress bar. Refused, with
/// what's running, when the server or another terminal runs a job.
pub(super) fn begin(db: &Arc<Db>, cfg: &Config, kind: &JobKind) -> Result<(Arc<LiveJob>, ProgressBar)> {
    match try_begin(db, cfg, kind)? {
        Ok(started) => Ok(started),
        Err(running) => bail!("{}. Wait for it to finish, or cancel it in Settings.", upper_first(&jobs::busy_text(&running))),
    }
}

/// Claim the job slot, or say which job holds it.
fn try_begin(db: &Arc<Db>, cfg: &Config, kind: &JobKind) -> Result<std::result::Result<(Arc<LiveJob>, ProgressBar), crate::db::Job>> {
    let sink = BarSink::new();
    let bar = sink.bar.clone();
    let task = kind.task(db, cfg);
    match LiveJob::begin(db.clone(), kind.name(), task.map(jobs::Task::id), JobSource::Terminal, kind.options(cfg), Some(Box::new(sink)))? {
        Ok(job) => {
            stop_on_ctrl_c(&job);
            Ok(Ok((job, bar)))
        }
        Err(running) => {
            bar.finish_and_clear();
            Ok(Err(running))
        }
    }
}

/// How often a sync that waits looks again.
const WAIT_POLL: std::time::Duration = std::time::Duration::from_secs(2);

/// Claim the job slot for a sync, waiting for a job that holds it rather
/// than giving up. A details run (or another enrich) its schedule
/// started is asked to stop: it starts again after this sync, from where it
/// got to. Anything else is waited for, saying what it is.
async fn begin_sync(db: &Arc<Db>, cfg: &Config, kind: &JobKind) -> Result<(Arc<LiveJob>, ProgressBar)> {
    let mut asked: Option<i64> = None;
    let mut said: Option<i64> = None;
    let mut waiting: Option<ProgressBar> = None;
    loop {
        match try_begin(db, cfg, kind)? {
            Ok(started) => {
                if let Some(w) = waiting {
                    w.finish_and_clear();
                }
                return Ok(started);
            }
            Err(running) => {
                let w = waiting.get_or_insert_with(|| {
                    let w = ProgressBar::new_spinner();
                    w.enable_steady_tick(std::time::Duration::from_millis(120));
                    w
                });
                if yields_to_sync(&running) && asked != Some(running.id) {
                    asked = Some(running.id);
                    if db.job_request_cancel_of(running.id)? {
                        let again =
                            if runs_after_sync(db, &running) { "it carries on after the sync" } else { "it carries on at its next run" };
                        w.println(format!("  {}. Stopping it so this sync can run; {again}.", upper_first(&jobs::busy_text(&running))));
                    }
                } else if said != Some(running.id) && asked != Some(running.id) {
                    w.println(format!(
                        "  {}. This sync starts when it ends (Ctrl-C to stop waiting).",
                        upper_first(&jobs::busy_text(&running))
                    ));
                }
                said = Some(running.id);
                w.set_message(format!("waiting: {}", jobs::busy_text(&running)));
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(WAIT_POLL) => {}
            _ = tokio::signal::ctrl_c() => {
                if let Some(w) = waiting {
                    w.finish_and_clear();
                }
                bail!("stopped waiting; nothing was synced.");
            }
        }
    }
}

/// Whether a running job gives way to a sync from a terminal: an enrich
/// that its schedule started. Its schedule runs it again after the sync,
/// and what it fetched so far is kept.
fn yields_to_sync(job: &crate::db::Job) -> bool {
    job.source == JobSource::Schedule && job.kind == "enrich"
}

/// Whether the job's task follows a sync, so the sync that stops it starts it again.
fn runs_after_sync(db: &Db, job: &crate::db::Job) -> bool {
    let Some(task) = job.task.as_deref().and_then(jobs::Task::parse) else { return false };
    tasks::setups(db).is_ok_and(|all| all.iter().any(|s| s.task == task && s.schedule == tasks::Schedule::After(jobs::Task::Sync)))
}

fn upper_first(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// The first Ctrl-C stops the job at its next check, so it's recorded as
/// cancelled; a second one quits at once.
fn stop_on_ctrl_c(job: &Arc<LiveJob>) {
    let job = Arc::downgrade(job);
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_err() {
            return;
        }
        if let Some(job) = job.upgrade() {
            job.cancel_now();
            job.log_at(Level::Info, "stopping (Ctrl-C again to quit at once)".into());
        }
        if tokio::signal::ctrl_c().await.is_ok() {
            std::process::exit(130);
        }
    });
}

/// A cancelled run says so plainly instead of "Error: cancelled".
pub(super) fn stopped<T>(status: JobStatus, result: Result<T>) -> Result<T> {
    match result {
        Err(_) if status == JobStatus::Cancelled => bail!("stopped before it finished. Run it again to carry on where it stopped."),
        other => other,
    }
}

/// `opts.galaxy_db` falls back to the `galaxy_db` setting when not given
/// (and Galaxy isn't skipped).
pub async fn sync(cfg: &Config, mut opts: SyncOptions) -> Result<()> {
    let (db, cfg) = open_library_db(cfg)?;
    let db = Arc::new(db);
    let clients = Clients::new(&cfg)?;
    if opts.on("galaxy") {
        opts.galaxy_db = opts.galaxy_db.or_else(|| cfg.galaxy_db.as_ref().map(|p| p.display().to_string()));
    }
    let (job, bar) = begin_sync(&db, &cfg, &JobKind::Sync(opts.clone())).await?;
    let result = job.scope(enrich::run_sync(&db, &cfg, &clients, &opts, &*job)).await;
    let status = job.finish(result.as_ref().map(jobs::sync_summary));
    bar.finish_and_clear();
    let report = stopped(status, result)?;
    let total: usize = report.summaries.iter().map(|s| s.total).sum();
    let added: usize = report.summaries.iter().map(|s| s.added).sum();
    if report.failed.is_empty() {
        println!("{} synced {} ({added} new). Next: `upnext enrich`.", green("✓"), crate::enrich::plural(total, "game", "games"));
    } else {
        println!(
            "{} partly synced: {} ({added} new); {} failed, see the log above.",
            red("!"),
            crate::enrich::plural(total, "game", "games"),
            report.failed.join(", ")
        );
    }
    Ok(())
}

pub async fn enrich(cfg: &Config, opts: EnrichOptions) -> Result<()> {
    let (db, cfg) = open_library_db(cfg)?;
    let db = Arc::new(db);
    let clients = Clients::new(&cfg)?;
    let llm = llm::backend_from_config(&cfg);
    let (job, bar) = begin(&db, &cfg, &JobKind::Enrich(opts.clone()))?;
    let result = job.scope(enrich::run_enrich(&db, &cfg, &clients, Some(llm.as_ref()), &opts, &*job)).await;
    let status = job.finish(result.as_ref().map(|s| s.summary_text()));
    bar.finish_and_clear();
    let s = stopped(status, result)?;
    println!("{} {} processed: {}", green("✓"), crate::enrich::plural(s.processed, "title", "titles"), s.counts_text());
    if s.errors > 0 {
        println!("  {} failed; see them with the job's log in Settings, or run them again from there.", s.errors);
    }
    Ok(())
}

/// Back up the database, as the backup job does. `keep` defaults to the
/// job's own setting. With a note, the backup is one kept until it's
/// deleted instead, as Settings › Backups, "Back up now" makes.
pub async fn backup(cfg: &Config, keep: Option<usize>, note: Option<String>) -> Result<()> {
    let (db, cfg) = open_library_db(cfg)?;
    if let Some(note) = note {
        drop(db);
        let made = jobs::backup::manual(&cfg, Some(&note))?;
        let dir = jobs::backup::backups_dir(&cfg);
        println!("{} {}, {}, kept until you delete it, in {}", green("✓"), made.name, crate::backups::megabytes(made.bytes), dir.display());
        return Ok(());
    }
    let db = Arc::new(db);
    let keep = match keep {
        Some(k) => k,
        None => tasks::setups(&db)?.iter().find(|s| s.task == jobs::Task::Backup).map_or(tasks::DEFAULT_KEEP, |s| s.keep),
    };
    let opts = BackupOptions { keep };
    let (job, bar) = begin(&db, &cfg, &JobKind::Backup(opts))?;
    let result = job.scope(jobs::backup::run(&cfg, job.id, &opts, &*job)).await;
    let status = job.finish(result.as_ref().map(String::clone));
    bar.finish_and_clear();
    let summary = stopped(status, result)?;
    println!("{} {summary} in {}", green("✓"), jobs::backup::backups_dir(&cfg).display());
    Ok(())
}

/// `upnext export`: a copy of the database (or of `from`) without keys and
/// sign-ins, at `out`. The source is only read, never opened as UpNext's
/// database, so a running server or an older schema is left as it is.
pub fn export(cfg: &Config, out: &Path, from: Option<&Path>, force: bool) -> Result<()> {
    let source = from.unwrap_or(&cfg.db_path);
    if !source.is_file() {
        bail!("there's no database at {}", source.display());
    }
    if out.exists() || out.is_symlink() {
        let same = std::fs::canonicalize(out).ok() == std::fs::canonicalize(source).ok();
        if same {
            bail!("{} is the database being copied", out.display());
        }
        if !force {
            bail!("{} already exists (--force replaces it)", out.display());
        }
        std::fs::remove_file(out).with_context(|| format!("removing {}", out.display()))?;
    }
    // A copy that was opened left its own log files; they'd belong to the old file.
    for extra in ["-wal", "-shm", "-journal"] {
        let mut p = out.as_os_str().to_owned();
        p.push(extra);
        let p = PathBuf::from(p);
        if p.exists() {
            if !force {
                bail!("{} already exists (--force replaces it)", p.display());
            }
            std::fs::remove_file(&p).with_context(|| format!("removing {}", p.display()))?;
        }
    }
    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("making {}", dir.display()))?;
    }
    crate::backups::keyless_copy(source, out)?;
    let bytes = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
    println!("{} {} without keys and sign-ins, {}, from {}", green("✓"), out.display(), crate::backups::megabytes(bytes), source.display());
    Ok(())
}

/// Every enrich source's option id, for `--only`, `--skip` and `--with`.
pub fn source_ids() -> Vec<&'static str> {
    enrich::sources::ids()
}

/// The sources, one per line, for `enrich --help`.
pub fn sources_help() -> String {
    let lines: Vec<String> = enrich::sources::options()
        .iter()
        .map(|o| {
            let off = if o.default_on { "" } else { " (off)" };
            let note = o.note.map(|n| format!(", {n}")).unwrap_or_default();
            format!("  {:<14}{}{off}{note}", o.id, o.label)
        })
        .collect();
    format!("Sources:\n{}", lines.join("\n"))
}

/// Every library's id, for `sync --only` and `--skip`.
pub fn library_ids() -> Vec<&'static str> {
    enrich::sync_libraries().iter().map(|l| l.id()).collect()
}

/// The libraries, one per line, for `sync --help`.
pub fn libraries_help() -> String {
    let lines: Vec<String> = enrich::sync_libraries().iter().map(|l| format!("  {:<8}{}", l.id(), l.label())).collect();
    format!("Libraries:\n{}", lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::PlayerId;

    /// A sync from a terminal waits for the job slot instead of failing,
    /// and a details run its schedule started stops to let it in.
    #[tokio::test]
    async fn a_sync_stops_a_scheduled_enrich_and_waits_for_others() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = crate::testutil::offline_config(dir.path());
        let db = Arc::new(Db::open(&cfg.db_path, 120).unwrap());
        let kind = || JobKind::Sync(SyncOptions::default());

        let details =
            LiveJob::begin(db.clone(), "enrich", Some("details"), JobSource::Schedule, serde_json::json!({}), None).unwrap().unwrap();
        let sync = tokio::spawn({
            let (db, cfg) = (db.clone(), cfg.clone());
            async move { begin_sync(&db, &cfg, &kind()).await.map(|(job, _)| job) }
        });
        // The details run sees the request at its next heartbeat.
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while !details.cancelled() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the scheduled run was asked to stop");
        assert!(!sync.is_finished(), "the sync waits while it stops");
        assert_eq!(details.finish(Err(&anyhow::anyhow!("cancelled"))), JobStatus::Cancelled);
        let job = sync.await.unwrap().unwrap();
        assert_eq!(db.job_running().unwrap().map(|j| (j.id, j.kind)), Some((job.id, "sync".to_string())));
        job.finish(Ok("0 games".into()));

        // A run someone started by hand isn't stopped: the sync waits for it.
        let mine = LiveJob::begin(db.clone(), "enrich", Some("details"), JobSource::Ui, serde_json::json!({}), None).unwrap().unwrap();
        let sync = tokio::spawn({
            let (db, cfg) = (db.clone(), cfg.clone());
            async move { begin_sync(&db, &cfg, &kind()).await.map(|(job, _)| job) }
        });
        tokio::time::sleep(WAIT_POLL + std::time::Duration::from_millis(500)).await;
        assert!(!mine.cancelled() && !sync.is_finished(), "still waiting, and nothing was stopped");
        mine.finish(Ok("done".into()));
        let job = tokio::time::timeout(WAIT_POLL * 3, sync).await.expect("it starts once the slot is free").unwrap().unwrap();
        job.finish(Ok("0 games".into()));
    }

    #[test]
    fn export_leaves_existing_files_and_the_source_alone() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::built_in();
        cfg.db_path = dir.path().join("upnext.db");
        let db = Db::open(&cfg.db_path, 120).unwrap();
        db.setting_set(PlayerId::FIRST, "steam_api_key", Some("ABCDEF")).unwrap();
        let out = dir.path().join("scratch/copy.db");
        export(&cfg, &out, None, false).unwrap();
        assert!(out.is_file(), "its folder is made");
        assert!(export(&cfg, &out, None, false).is_err(), "no overwrite without --force");
        export(&cfg, &out, None, true).unwrap();
        assert!(export(&cfg, &cfg.db_path, None, true).is_err(), "never onto the database itself");

        let wal = dir.path().join("scratch/copy.db-wal");
        std::fs::remove_file(&out).unwrap();
        std::fs::write(&wal, b"old").unwrap();
        assert!(export(&cfg, &out, None, false).is_err(), "a stale log would be read with the new copy");
        export(&cfg, &out, None, true).unwrap();
        assert!(!wal.exists());

        assert!(export(&cfg, &out, Some(&dir.path().join("nope.db")), true).is_err());
        assert_eq!(
            db.settings_all(PlayerId::FIRST).unwrap().get("steam_api_key").map(String::as_str),
            Some("ABCDEF"),
            "the source keeps its key"
        );
    }
}
