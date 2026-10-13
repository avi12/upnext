//! The GOG account: owned games, and playtime from GOG's cloud (which covers
//! games played through Galaxy on Windows too). Needs `gog-login` first.
//! Each player's GOG sign-in is a connection of its own.

use super::{Library, Outcomes, SyncRun};
use crate::config::Config;
use crate::db::{PlayerId, SyncConnection, SyncSummary};
use crate::enrich::{Clients, Progress};
use crate::gog::GogClient;
use crate::signin::DbSignIn;
use anyhow::{Result, bail};
use async_trait::async_trait;
use std::sync::Arc;

pub struct GogAccount;

/// The client for a connection's sign-in: the shared one for player 1's
/// own first (which also moves an old sign-in file in), else one of its own.
fn client_for(run: &SyncRun<'_>, conn: &SyncConnection, household: Option<i64>) -> Arc<GogClient> {
    match conn.id {
        Some(id) if conn.id != household => {
            Arc::new(run.clients.gog.with_store(Arc::new(DbSignIn::connection(run.db.path().to_path_buf(), "gog", id))))
        }
        _ => run.clients.gog.clone(),
    }
}

#[async_trait]
impl Library for GogAccount {
    fn id(&self) -> &'static str {
        "gog"
    }
    fn label(&self) -> &'static str {
        "GOG account"
    }
    fn blocked(&self, _cfg: &Config, clients: &Clients) -> Option<&'static str> {
        clients.gog.load_token().is_none().then_some("not connected")
    }
    fn ready(&self, run: &SyncRun<'_>) -> bool {
        self.blocked(run.cfg, run.clients).is_none() || run.db.signed_in("gog").unwrap_or(false)
    }

    async fn sync(&self, run: &SyncRun<'_>, progress: &mut Progress) -> Result<Vec<SyncSummary>> {
        let household = run.db.own_sync_connection(PlayerId::FIRST, "gog")?.id;
        let mut outcomes = Outcomes::default();
        for mut conn in run.connections("gog")? {
            let gog = client_for(run, &conn, household);
            if gog.load_token().is_none() {
                continue;
            }
            let result = sync_connection(run, progress, &gog, &mut conn).await;
            outcomes.add(run, "GOG account", &conn, result)?;
        }
        outcomes.finish()
    }
}

/// One GOG sign-in: its owned games, each with its playtime. A game whose
/// playtime can't be read comes in without it; owned games GOG won't list
/// fail the connection, as any other store's account does.
async fn sync_connection(
    run: &SyncRun<'_>,
    progress: &mut Progress,
    gog: &GogClient,
    conn: &mut SyncConnection,
) -> Result<Vec<SyncSummary>> {
    let (db, sink) = (run.db, run.sink);
    let whose = run.whose(conn);
    progress.message = "Fetching owned games from GOG".into();
    sink.update(progress);
    let mut games = gog.owned_games().await?;
    sink.log(format!("GOG account{whose}: {}; fetching playtime", crate::enrich::plural(games.len(), "game", "games")));
    let n = games.len();
    for (i, g) in games.iter_mut().enumerate() {
        if sink.cancelled() {
            bail!("cancelled");
        }
        progress.message = format!("GOG playtime {}/{n}: {}", i + 1, g.name);
        sink.update(progress);
        match gog.playtime(&g.source_id).await {
            Ok(Some((minutes, last))) => {
                g.playtime_minutes = Some(minutes);
                g.last_played = last;
            }
            Ok(None) => g.playtime_minutes = Some(0),
            Err(e) => sink.log(format!("GOG playtime failed for {}: {e}", g.name)),
        }
    }
    let s = db.sync_into(conn, "gog", &games)?;
    let played = games.iter().filter(|g| g.playtime_minutes.unwrap_or(0) > 0).count();
    sink.log(format!(
        "GOG account{whose}: {} ({} new, {} updated, {played} with playtime)",
        crate::enrich::plural(s.total, "game", "games"),
        s.added,
        s.updated
    ));
    Ok(vec![s])
}
