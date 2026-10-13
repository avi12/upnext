//! Epic Games without Heroic: the owned library through the Epic
//! sign-in, and the Epic Games Launcher's installs. Either is enough. Each
//! player's Epic sign-in is a connection of its own.

use super::{Library, Outcomes, SyncRun, with_uninstalled};
use crate::config::Config;
use crate::db::{ImportedGame, PlayerId, SyncConnection, SyncSummary};
use crate::enrich::{Clients, Progress};
use crate::epic;
use crate::signin::DbSignIn;
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;

pub struct Epic;

#[async_trait]
impl Library for Epic {
    fn id(&self) -> &'static str {
        "epic"
    }
    fn label(&self) -> &'static str {
        "Epic Games"
    }
    fn blocked(&self, cfg: &Config, clients: &Clients) -> Option<&'static str> {
        let here = epic::find_manifests_dir(cfg.epic_manifests.as_deref()).is_some();
        (clients.epic.load_token().is_none() && !here).then_some("not signed in, and no Epic Games Launcher here")
    }
    fn ready(&self, run: &SyncRun<'_>) -> bool {
        self.blocked(run.cfg, run.clients).is_none() || run.db.signed_in("epic").unwrap_or(false)
    }

    async fn sync(&self, run: &SyncRun<'_>, progress: &mut Progress) -> Result<Vec<SyncSummary>> {
        let household = run.db.own_sync_connection(PlayerId::FIRST, "epic")?.id;
        let mut outcomes = Outcomes::default();
        for mut conn in run.connections("epic")? {
            let result = sync_connection(run, progress, &mut conn, household).await;
            outcomes.add(run, "Epic", &conn, result)?;
        }
        outcomes.finish()
    }
}

/// One Epic connection: its sign-in's owned library, and the Epic Games
/// Launcher's installs where it reads them. Player 1's first reads
/// this computer's launcher (or the `epic_manifests` folder); another only
/// a folder it names.
async fn sync_connection(
    run: &SyncRun<'_>,
    progress: &mut Progress,
    conn: &mut SyncConnection,
    household: Option<i64>,
) -> Result<Vec<SyncSummary>> {
    let (db, sink) = (run.db, run.sink);
    let whose = run.whose(conn);
    let client = match conn.id {
        Some(id) if conn.id != household => {
            Arc::new(run.clients.epic.with_store(Arc::new(DbSignIn::connection(db.path().to_path_buf(), "epic", id))))
        }
        _ => run.clients.epic.clone(),
    };
    let manifests = match conn.setting("epic_manifests").map(PathBuf::from) {
        Some(own) if conn.first => epic::find_manifests_dir(Some(&own)),
        Some(own) => own.is_dir().then_some(own),
        None if conn.first => epic::find_manifests_dir(run.cfg.epic_manifests.as_deref()),
        None => None,
    };
    let installed = match manifests {
        Some(dir) => match epic::read_manifests(&dir) {
            Ok(found) => {
                sink.log(format!("Epic Games Launcher{whose}: {} installed in {}", found.len(), dir.display()));
                found
            }
            Err(e) => {
                sink.log(format!("Epic Games Launcher's installs weren't read{whose}: {e:#}"));
                vec![]
            }
        },
        None => vec![],
    };
    let mut games: Option<Vec<ImportedGame>> = None;
    if client.load_token().is_some() {
        progress.message = "Reading your Epic library".into();
        sink.update(progress);
        match client.owned_library().await {
            Ok(lib) => {
                let through: Vec<String> = ["EA app", "Ubisoft Connect"]
                    .iter()
                    .map(|store| (store, lib.games.iter().filter(|g| g.third_party.as_deref() == Some(store)).count()))
                    .filter(|(_, n)| *n > 0)
                    .map(|(store, n)| format!("{n} play through the {store}"))
                    .collect();
                sink.log(format!(
                    "Epic account{whose}: {}{}, {} (DLC, add-ons, demos) left out{}{}",
                    crate::enrich::plural(lib.games.len(), "game", "games"),
                    if through.is_empty() { String::new() } else { format!(" ({})", through.join(", ")) },
                    crate::enrich::plural(lib.skipped, "other library entry", "other library entries"),
                    if lib.unread.is_empty() {
                        String::new()
                    } else {
                        format!(
                            "; {} not read from Epic's catalog this time (kept as before)",
                            crate::enrich::plural(lib.unread.len(), "game", "games")
                        )
                    },
                    if lib.playtime.is_none() { "; Epic didn't give playtime this time" } else { "" }
                ));
                let mut copies = epic::library_copies(&lib, &installed);
                // A game whose catalog entry wasn't read keeps the copy it
                // has, rather than being removed until the catalog answers.
                if !lib.unread.is_empty() {
                    let have: std::collections::HashSet<String> = copies.iter().map(|c| c.source_id.clone()).collect();
                    let kept = db.launcher_copies(conn.owner, "epic", "epic")?;
                    copies.extend(kept.into_iter().filter(|c| lib.unread.contains(&c.source_id) && !have.contains(&c.source_id)));
                }
                games = Some(copies);
            }
            // The installs still come in, and nothing read before goes.
            Err(e) => sink.log(format!("Epic account sync failed{whose} (non-fatal): {e:#}")),
        }
    }
    let games = match games {
        Some(g) => g,
        None if installed.is_empty() && db.launcher_copies(conn.owner, "epic", "epic")?.is_empty() => return Ok(vec![]),
        None => with_uninstalled(db, conn.owner, "epic", "epic", installed.iter().map(epic::installed_copy).collect())?,
    };
    let s = db.sync_into(conn, "epic", &games)?;
    sink.log(format!(
        "Epic{whose}: {} ({} new, {} updated, {} removed)",
        crate::enrich::plural(s.total, "game", "games"),
        s.added,
        s.updated,
        s.removed
    ));
    Ok(vec![s])
}
