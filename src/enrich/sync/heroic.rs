//! Heroic's library caches: Epic, GOG and Amazon games installed through it.
//! Each Heroic folder is one connection, its player's.

use super::{Library, Outcomes, SyncRun};
use crate::db::{SyncConnection, SyncSummary};
use crate::enrich::Progress;
use anyhow::Result;
use async_trait::async_trait;
use std::path::{Path, PathBuf};

pub struct Heroic;

#[async_trait]
impl Library for Heroic {
    fn id(&self) -> &'static str {
        "heroic"
    }
    fn label(&self) -> &'static str {
        "Heroic"
    }

    async fn sync(&self, run: &SyncRun<'_>, progress: &mut Progress) -> Result<Vec<SyncSummary>> {
        let (cfg, sink) = (run.cfg, run.sink);
        let mut outcomes = Outcomes::default();
        progress.message = "Reading Heroic library caches".into();
        sink.update(progress);
        for mut conn in run.connections("heroic")? {
            let whose = run.whose(&conn);
            // Player 1's first Heroic reads this computer's own (or
            // UPNEXT_HEROIC_DIR's); another reads only the folder it names.
            let dir = match conn.setting("heroic_dir").map(PathBuf::from) {
                Some(own) if conn.first => crate::heroic::find_config_dir(Some(&own)),
                Some(own) => own.join("store_cache").is_dir().then_some(own),
                None if conn.first => crate::heroic::find_config_dir(cfg.heroic_dir.as_deref()),
                None => None,
            };
            match dir {
                Some(dir) => {
                    let result = sync_folder(run, &mut conn, &dir);
                    outcomes.add(run, "Heroic", &conn, result)?;
                }
                None if conn.first => sink.log("Heroic skipped: no config directory found (set UPNEXT_HEROIC_DIR)".into()),
                None => sink.log(format!("Heroic{whose} skipped: no Heroic folder found at the one set for it")),
            }
        }
        outcomes.finish()
    }
}

/// One Heroic folder's store caches, into its connection's accounts.
fn sync_folder(run: &SyncRun<'_>, conn: &mut SyncConnection, dir: &Path) -> Result<Vec<SyncSummary>> {
    let whose = run.whose(conn);
    let libs = crate::heroic::read_libraries(dir)?;
    if libs.stores.is_empty() {
        run.sink.log(format!("Heroic{whose}: no store caches found in {}", dir.display()));
    }
    // A store whose cache wasn't read keeps its games as they were.
    for why in &libs.unread {
        run.sink.log(format!("Heroic{whose}: a store skipped this time: {why}"));
    }
    let mut summaries = vec![];
    for (source, games) in libs.stores {
        let s = run.db.sync_into(conn, &source, &games)?;
        run.sink.log(format!(
            "Heroic{whose}/{source}: {} ({} new, {} updated, {} removed)",
            crate::enrich::plural(s.total, "game", "games"),
            s.added,
            s.updated,
            s.removed
        ));
        summaries.push(s);
    }
    Ok(summaries)
}
