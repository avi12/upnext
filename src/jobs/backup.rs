//! The jobs that use the backups folder (`crate::backups`):
//! - the backup job, a consistent copy of the whole database, keys and
//!   sign-ins included, keeping the newest few of its own files
//!   (`upnext-scheduled-…`); backups made any other way are never removed
//!   here;
//! - restore, which puts a backup from the folder in place of the database;
//! - import, which does the same with a database uploaded from elsewhere.
//!
//! Restore and import are jobs so they hold the one job slot while they
//! work, and they back the database up before they replace it.

use super::{BackupOptions, ImportOptions, RestoreOptions};
use crate::backups::{self, About, Kind, New, Staged, megabytes};
use crate::config::Config;
use crate::db::Db;
use crate::enrich::{Progress, ProgressSink};
use anyhow::{Context, Result, bail};
use std::path::PathBuf;
use std::sync::Arc;

/// `…/upnext/backups` for `…/upnext/upnext.db`.
pub fn backups_dir(cfg: &Config) -> PathBuf {
    backups::dir_for(&cfg.db_path)
}

/// Write a backup and remove this job's older ones: "upnext-scheduled-20261007-030012.db, 39 MB".
pub async fn run(cfg: &Config, job: i64, opts: &BackupOptions, sink: &dyn ProgressSink) -> Result<String> {
    let (db, dir) = (cfg.db_path.clone(), backups_dir(cfg));
    let keep = opts.keep.max(1);
    sink.update(&Progress { total: 1, done: 0, current: None, message: "Writing the backup".into() });
    let new = New { kind: Kind::Scheduled, about: About::default(), keep: Some(keep), job: Some(job) };
    let made = tokio::task::spawn_blocking(move || backups::write(&db, &dir, &Kind::Scheduled.stem(), &new))
        .await
        .context("the backup crashed")??;
    sink.log(format!("wrote {} ({})", made.name, megabytes(made.bytes)));
    for r in &made.removed {
        sink.log(format!("removed {r}, to keep the newest {keep}"));
    }
    sink.update(&Progress { total: 1, done: 1, current: None, message: "Done".into() });
    let mut summary = format!("{}, {}", made.name, megabytes(made.bytes));
    match made.removed.len() {
        0 => {}
        1 => summary.push_str(", removed 1 older one"),
        n => summary.push_str(&format!(", removed {n} older ones")),
    }
    Ok(summary)
}

/// A backup kept until it's deleted (Backups, "Back up now"; `upnext backup --note`).
pub fn manual(cfg: &Config, note: Option<&str>) -> Result<backups::Made> {
    let note = note.map(str::trim).filter(|n| !n.is_empty()).map(str::to_string);
    let new = New { kind: Kind::Manual, about: About { note, reason: None }, keep: None, job: None };
    backups::write(&cfg.db_path, &backups_dir(cfg), &Kind::Manual.stem(), &new)
}

/// Put a backup from the folder in place of the database.
pub async fn restore(db: &Arc<Db>, cfg: &Config, job: i64, opts: &RestoreOptions, sink: &dyn ProgressSink) -> Result<String> {
    let dir = backups_dir(cfg);
    let source = backups::find(&dir, &opts.name)?;
    step(sink, 0, format!("Checking {}", opts.name));
    let staged = tokio::task::spawn_blocking(move || backups::stage(&source, &dir)).await.context("the check crashed")??;
    replace(db, cfg, job, staged, Kind::Restore, &opts.name, sink).await
}

/// Put an uploaded database, checked and waiting in the folder, in place of the database.
pub async fn import(db: &Arc<Db>, cfg: &Config, job: i64, opts: &ImportOptions, sink: &dyn ProgressSink) -> Result<String> {
    let dir = backups_dir(cfg);
    let path = PathBuf::from(&opts.staged);
    // Only a file this server put there (`backups::work_path`).
    if path.parent() != Some(dir.as_path())
        || !path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(backups::WORK_PREFIX))
    {
        bail!("the uploaded file isn't where uploads wait");
    }
    let schema = backups::header_version(&path).context("the uploaded file is gone; upload it again")?;
    step(sink, 0, format!("Checking {}", opts.name));
    replace(db, cfg, job, Staged { path, schema }, Kind::Import, &opts.name, sink).await
}

const STEPS: usize = 4;

fn step(sink: &dyn ProgressSink, done: usize, message: String) {
    sink.update(&Progress { total: STEPS, done, current: None, message });
}

/// Back up the database as it is, then replace it with `staged`:
/// "Restored upnext-manual-20261001-101500.db, upgraded from v29; the database before it is upnext-pre-restore-20261002-090000.db".
async fn replace(db: &Arc<Db>, cfg: &Config, job: i64, staged: Staged, kind: Kind, name: &str, sink: &dyn ProgressSink) -> Result<String> {
    let (verb, done) = if kind == Kind::Import { ("importing", "Imported") } else { ("restoring", "Restored") };
    sink.log(format!("{name} is a good UpNext database, schema v{}", staged.schema));

    step(sink, 1, "Backing up the database first".into());
    let new = New { kind, about: About { note: None, reason: Some(format!("Before {verb} {name}")) }, keep: kind.keep(), job: None };
    let (path, dir) = (cfg.db_path.clone(), backups_dir(cfg));
    let first = tokio::task::spawn_blocking(move || backups::write(&path, &dir, &kind.stem(), &new))
        .await
        .context("the backup crashed")?
        .context("backing up the database first, so nothing was replaced")?;
    sink.log(format!("backed up the database as it was to {} ({})", first.name, megabytes(first.bytes)));
    for r in &first.removed {
        sink.log(format!("removed {r}, to keep the newest {}", backups::KEEP_AUTOMATIC));
    }

    let from_schema = staged.schema;
    let upgrading = from_schema < crate::db::SCHEMA_VERSION;
    step(sink, 2, if upgrading { format!("Upgrading it from v{from_schema}") } else { "Getting it ready".into() });
    let keys = backups::Keys::of(db)?;
    let (staged, kept) = tokio::task::spawn_blocking(move || backups::prepare(&staged, &keys).map(|kept| (staged, kept)))
        .await
        .context("getting it ready crashed")??;
    if upgrading {
        sink.log(format!("upgraded it from schema v{from_schema} to v{}", crate::db::SCHEMA_VERSION));
    }
    if !kept.settings.is_empty() {
        sink.log(format!("kept this computer's {}, which {name} doesn't have", kept.settings.join(", ")));
    }
    if kept.tokens + kept.copy_tokens > 0 {
        let tokens = |n: usize| if n == 1 { "1 API token".to_string() } else { format!("{n} API tokens") };
        sink.log(format!(
            "kept this computer's {} in place of the {} in {name}, so a token revoked since stays revoked",
            tokens(kept.tokens),
            tokens(kept.copy_tokens)
        ));
    }
    if sink.cancelled() {
        bail!("stopped before anything was replaced");
    }

    step(sink, 3, format!("{} {name}", if kind == Kind::Import { "Importing" } else { "Restoring" }));
    let live = db.clone();
    tokio::task::spawn_blocking(move || live.replace_with(&staged.path, job)).await.context("replacing the database crashed")??;
    sink.log(format!("{} {name}", done.to_lowercase()));
    sink.update(&Progress { total: STEPS, done: STEPS, current: None, message: "Done".into() });
    let upgraded = if upgrading { format!(", upgraded from v{from_schema}") } else { String::new() };
    Ok(format!("{done} {name}{upgraded}; the database before it is {}", first.name))
}
