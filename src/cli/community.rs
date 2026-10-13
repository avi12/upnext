//! `verdicts sync|apply`: other players' backlog verdicts from
//! the terminal. `sync` runs the daily job now; `apply` puts a
//! downloaded snapshot in place, for an install that can't reach the
//! service, and turns receiving on, since asking for one is choosing it.

use super::format::{accent, green};
use super::open_db;
use super::sync::{begin, stopped};
use crate::config::Config;
use crate::db::{PlayerId, VERDICTS_RECEIVE};
use crate::jobs::JobKind;
use crate::verdicts::receive;
use anyhow::Result;
use std::sync::Arc;

/// Run the "Backlog verdicts" job now.
pub async fn sync(cfg: &Config) -> Result<()> {
    let (db, cfg) = open_db(cfg)?;
    let db = Arc::new(db);
    let http = crate::http::client()?;
    let (job, bar) = begin(&db, &cfg, &JobKind::Verdicts)?;
    let result = job.scope(receive::run_job(&db, &cfg, &http, &*job)).await;
    let status = job.finish(result.as_ref().map(String::clone));
    bar.finish_and_clear();
    println!("{} {}", green("✓"), stopped(status, result)?);
    Ok(())
}

/// Put a snapshot from a file, a folder or an address in place. One no
/// newer than the kept one is skipped with a warning, unless `force`.
pub async fn apply(cfg: &Config, from: &str, force: bool) -> Result<()> {
    let (db, _) = open_db(cfg)?;
    let http = crate::http::client()?;
    let (summary, _) = receive::load(&http, from).await?;
    db.setting_set(PlayerId::FIRST, VERDICTS_RECEIVE, Some("true"))?;
    let (kept, _) = db.community_verdicts_kept()?;
    if !force && !receive::newer(kept.as_deref(), &summary.header.date) {
        eprintln!(
            "{} Skipped: this snapshot is of {}, and the one kept is of {}. `--force` puts it in place anyway.",
            accent("!"),
            summary.header.date,
            kept.as_deref().unwrap_or("?")
        );
        return Ok(());
    }
    let done = receive::apply(&db, &summary, from, None)?;
    println!("{} {}", green("✓"), done.summary());
    Ok(())
}
