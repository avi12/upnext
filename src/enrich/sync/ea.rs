//! The EA app's installed games, read from Windows' Apps list. Its
//! owned library has no sign-in UpNext may use: GOG Galaxy or the games
//! added by hand cover the rest. What is installed on this computer goes to
//! player 1's EA app connection, since this computer's installs
//! are theirs as the other machine defaults are.

use super::{Library, SyncRun, with_uninstalled};
use crate::config::Config;
use crate::db::SyncSummary;
use crate::ea;
use crate::enrich::{Clients, Progress};
use anyhow::Result;
use async_trait::async_trait;

pub struct EaApp;

#[async_trait]
impl Library for EaApp {
    fn id(&self) -> &'static str {
        "ea"
    }
    fn label(&self) -> &'static str {
        "EA app"
    }
    fn blocked(&self, _cfg: &Config, _clients: &Clients) -> Option<&'static str> {
        ea::installed(&crate::installs::uninstall_entries()).is_empty().then_some("no EA app games installed here")
    }
    /// It also runs with nothing installed when it brought games in before,
    /// so the ones uninstalled since are marked so.
    fn ready(&self, run: &SyncRun<'_>) -> bool {
        self.blocked(run.cfg, run.clients).is_none()
            || run
                .db
                .sync_connections(ea::SOURCE)
                .is_ok_and(|c| run.db.launcher_copies(c[0].owner, ea::SOURCE, ea::SOURCE).is_ok_and(|c| !c.is_empty()))
    }

    async fn sync(&self, run: &SyncRun<'_>, _progress: &mut Progress) -> Result<Vec<SyncSummary>> {
        let mut conn = run.db.sync_connections(ea::SOURCE)?.remove(0);
        let whose = run.whose(&conn);
        let found = ea::installed(&crate::installs::uninstall_entries());
        run.sink.log(format!("EA app{whose}: {} installed", found.len()));
        let games = with_uninstalled(run.db, conn.owner, ea::SOURCE, ea::SOURCE, found.iter().map(ea::copy).collect())?;
        let s = run.db.sync_into(&mut conn, ea::SOURCE, &games)?;
        run.sink.log(format!(
            "EA app{whose}: {} ({} new, {} updated, {} removed)",
            crate::enrich::plural(s.total, "game", "games"),
            s.added,
            s.updated,
            s.removed
        ));
        Ok(vec![s])
    }
}
