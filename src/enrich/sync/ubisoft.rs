//! Ubisoft Connect's installed games, read from the registry. Its
//! owned library has no sign-in UpNext may use: GOG Galaxy or the games
//! added by hand cover the rest. What is installed on this computer goes to
//! player 1's Ubisoft Connect connection, since this computer's
//! installs are theirs as the other machine defaults are.

use super::{Library, SyncRun, with_uninstalled};
use crate::config::Config;
use crate::db::SyncSummary;
use crate::enrich::{Clients, Progress};
use crate::ubisoft;
use anyhow::Result;
use async_trait::async_trait;

pub struct UbisoftConnect;

fn found() -> Vec<ubisoft::UbisoftInstall> {
    let installs = crate::installs::machine_subkey_values(ubisoft::INSTALLS_KEY, "InstallDir");
    if installs.is_empty() {
        return vec![];
    }
    ubisoft::installed(&installs, &crate::installs::uninstall_entries())
}

#[async_trait]
impl Library for UbisoftConnect {
    fn id(&self) -> &'static str {
        "ubisoft"
    }
    fn label(&self) -> &'static str {
        "Ubisoft Connect"
    }
    fn blocked(&self, _cfg: &Config, _clients: &Clients) -> Option<&'static str> {
        found().is_empty().then_some("no Ubisoft Connect games installed here")
    }
    /// It also runs with nothing installed when it brought games in before,
    /// so the ones uninstalled since are marked so.
    fn ready(&self, run: &SyncRun<'_>) -> bool {
        self.blocked(run.cfg, run.clients).is_none()
            || run
                .db
                .sync_connections(ubisoft::SOURCE)
                .is_ok_and(|c| run.db.launcher_copies(c[0].owner, ubisoft::SOURCE, ubisoft::SOURCE).is_ok_and(|c| !c.is_empty()))
    }

    async fn sync(&self, run: &SyncRun<'_>, _progress: &mut Progress) -> Result<Vec<SyncSummary>> {
        let mut conn = run.db.sync_connections(ubisoft::SOURCE)?.remove(0);
        let whose = run.whose(&conn);
        let found = found();
        run.sink.log(format!("Ubisoft Connect{whose}: {} installed", found.len()));
        let games = with_uninstalled(run.db, conn.owner, ubisoft::SOURCE, ubisoft::SOURCE, found.iter().map(ubisoft::copy).collect())?;
        let s = run.db.sync_into(&mut conn, ubisoft::SOURCE, &games)?;
        run.sink.log(format!(
            "Ubisoft Connect{whose}: {} ({} new, {} updated, {} removed)",
            crate::enrich::plural(s.total, "game", "games"),
            s.added,
            s.updated,
            s.removed
        ));
        Ok(vec![s])
    }
}
