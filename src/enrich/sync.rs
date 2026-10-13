//! Sync: pull each launcher's library into the database. The libraries are
//! listed in `libraries()`, one file each under `sync/`; the CLI's
//! `--only`/`--skip`, the jobs API's options and the Settings page
//! (`GET /api/sync/sources`) all read that list.

mod ea;
mod epic;
pub(crate) mod galaxy;
mod gog;
mod heroic;
mod steam;
mod ubisoft;

use super::{Clients, Progress, ProgressSink};
use crate::config::Config;
use crate::db::PlayerId;
use crate::db::{Db, Failure, ImportedGame, SyncConnection, SyncSummary};
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// What a sync reads. In JSON each library is a key of its own next to
/// `steam_local` and `galaxy_db`: `{"heroic": false, "galaxy_db": null}`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncOptions {
    /// Libraries on or off by id (`libraries()`); all are on unless turned
    /// off. An unknown id is ignored.
    #[serde(flatten)]
    pub libraries: BTreeMap<String, bool>,
    /// Force the local Steam client path even when an API key is configured.
    #[serde(default)]
    pub steam_local: bool,
    /// The Galaxy database to read; Galaxy is skipped without one.
    #[serde(default)]
    pub galaxy_db: Option<String>,
    /// Only this connection (`upnext sync --account`), by id; every
    /// connection without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<i64>,
}

impl SyncOptions {
    pub fn on(&self, id: &str) -> bool {
        self.libraries.get(id).copied().unwrap_or(true)
    }

    /// Every library spelled out and unknown ids dropped: what a job reports.
    pub fn resolved(&self) -> Self {
        let libraries = libraries().iter().map(|l| (l.id().to_string(), self.on(l.id()))).collect();
        Self { libraries, steam_local: self.steam_local, galaxy_db: self.galaxy_db.clone(), connection: self.connection }
    }
}

/// One sync: what every library reads.
pub struct SyncRun<'a> {
    pub db: &'a Db,
    pub cfg: &'a Config,
    pub clients: &'a Clients,
    pub opts: &'a SyncOptions,
    pub sink: &'a dyn ProgressSink,
    /// Each player's name, once there are two or more, for the log.
    pub names: BTreeMap<PlayerId, String>,
    /// The connections that failed, "Steam (Sam)", each with why, and any
    /// library that failed before reaching one. The sync goes on past them
    /// and is reported as partial; only when no library was read at all
    /// does the sync fail, naming them all.
    failed: Mutex<Vec<(String, String)>>,
}

/// What a sync did: each library's counts, and the connections that failed
/// while the rest synced. A sync with failures is partial, never a plain
/// success: the job's summary says so and records each as a failure.
#[derive(Debug, Default)]
pub struct SyncReport {
    pub summaries: Vec<SyncSummary>,
    /// "Steam (Sam)", in the order they failed.
    pub failed: Vec<String>,
}

impl SyncRun<'_> {
    /// " (Sam)", after a library's name in the log, when there are two or
    /// more players; nothing for one, so a single player's log reads as it
    /// always has.
    pub fn whose(&self, conn: &SyncConnection) -> String {
        self.names.get(&conn.owner).map(|n| format!(" ({n})")).unwrap_or_default()
    }

    /// Record that `source` ("Steam (Sam)", or a library's label) failed:
    /// a failure of the job, in its log, and a name for the summary.
    fn fail(&self, source: String, error: String) {
        let failure = Failure {
            lane: "sync".into(),
            source: source.clone(),
            options: vec![],
            title_id: None,
            name: None,
            error,
            at: crate::db::now(),
        };
        self.sink.failure(&failure);
        self.failed.lock().unwrap_or_else(|e| e.into_inner()).push((source, failure.error));
    }

    /// The connections of `kind` this run syncs: every one, or the one
    /// `--account` names.
    pub fn connections(&self, kind: &str) -> Result<Vec<SyncConnection>> {
        let mut all = self.db.sync_connections(kind)?;
        if let Some(id) = self.opts.connection {
            all.retain(|c| c.id == Some(id));
        }
        Ok(all)
    }
}

/// One library's connections as a sync goes through them: a
/// connection that fails (a folder that can't be read, an account Steam
/// turns down) is recorded as the job's failure and the next goes on, which
/// makes the sync partial. The library fails only when none of its
/// connections brought anything in, as it did with one; the sync then goes
/// on to the next library. A cancelled sync stops at once.
#[derive(Default)]
pub struct Outcomes {
    summaries: Vec<SyncSummary>,
    synced: bool,
    failed: Option<anyhow::Error>,
}

impl Outcomes {
    /// What connection `conn` of `label` ("Steam") did.
    pub fn add(&mut self, run: &SyncRun<'_>, label: &str, conn: &SyncConnection, result: Result<Vec<SyncSummary>>) -> Result<()> {
        match result {
            Ok(summaries) => {
                self.synced |= !summaries.is_empty();
                self.summaries.extend(summaries);
            }
            Err(e) if run.sink.cancelled() => return Err(e),
            Err(e) => {
                run.fail(format!("{label}{}", run.whose(conn)), format!("{e:#}"));
                self.failed.get_or_insert(e);
            }
        }
        Ok(())
    }

    pub fn finish(self) -> Result<Vec<SyncSummary>> {
        match self.failed {
            Some(e) if !self.synced => Err(e),
            _ => Ok(self.summaries),
        }
    }
}

/// A launcher or store whose library `sync` reads.
#[async_trait]
pub trait Library: Send + Sync {
    /// The option id: `sync --skip heroic`, `{"heroic": false}` in the jobs API.
    fn id(&self) -> &'static str;
    fn label(&self) -> &'static str;
    /// Why these settings keep it from syncing ("not connected"), for the
    /// Settings page.
    fn blocked(&self, _cfg: &Config, _clients: &Clients) -> Option<&'static str> {
        None
    }
    /// Whether it runs this time, when its option is on.
    fn ready(&self, _run: &SyncRun<'_>) -> bool {
        true
    }
    /// Read its games and write them, logging what it did. `progress` is the
    /// sync's; set its message and update the sink as work goes.
    async fn sync(&self, run: &SyncRun<'_>, progress: &mut Progress) -> Result<Vec<SyncSummary>>;
}

/// The libraries, in the order a sync reads them.
pub fn libraries() -> Vec<Box<dyn Library>> {
    vec![
        Box::new(steam::Steam),
        Box::new(heroic::Heroic),
        Box::new(gog::GogAccount),
        Box::new(epic::Epic),
        Box::new(ea::EaApp),
        Box::new(ubisoft::UbisoftConnect),
        Box::new(galaxy::Galaxy),
    ]
}

/// For a launcher whose files list installed games only (the Epic Games
/// Launcher without a sign-in, the EA app, Ubisoft Connect): `installed`,
/// with the games it brought in before and doesn't list now added back as
/// not installed. Uninstalling a game doesn't give it back to the store, so
/// it stays in the library; hiding it is the way to take it out.
pub(crate) fn with_uninstalled(
    db: &Db,
    owner: PlayerId,
    launcher: &str,
    source: &str,
    mut installed: Vec<ImportedGame>,
) -> Result<Vec<ImportedGame>> {
    for before in db.launcher_copies(owner, launcher, source)? {
        if !installed.iter().any(|g| g.source_id == before.source_id) {
            installed.push(before);
        }
    }
    Ok(installed)
}

#[tracing::instrument(target = "upnext::timing", name = "sync", skip_all)]
pub async fn run_sync(db: &Db, cfg: &Config, clients: &Clients, opts: &SyncOptions, sink: &dyn ProgressSink) -> Result<SyncReport> {
    let players = db.players()?;
    let names = if players.len() > 1 { players.into_iter().map(|p| (p.id, p.name)).collect() } else { BTreeMap::new() };
    let run = SyncRun { db, cfg, clients, opts, sink, names, failed: Mutex::default() };
    let libraries = libraries();
    // With one connection named, only the library that reads its kind.
    let only = match opts.connection {
        Some(id) => Some(db.connection_kind(id)?.ok_or_else(|| anyhow::anyhow!("there's no connection {id}"))?),
        None => None,
    };
    let active: Vec<&dyn Library> = libraries
        .iter()
        .map(|l| l.as_ref())
        .filter(|l| only.as_deref().is_none_or(|k| k == l.id()) && opts.on(l.id()) && l.ready(&run))
        .collect();
    let mut progress = Progress { total: active.len(), done: 0, current: None, message: "Starting sync".into() };
    sink.update(&progress);
    let mut summaries = vec![];
    for library in &active {
        progress.current = Some(library.label().into());
        let failed_before = run.failed.lock().unwrap_or_else(|e| e.into_inner()).len();
        match library.sync(&run, &mut progress).await {
            Ok(synced) => summaries.extend(synced),
            Err(e) if sink.cancelled() => return Err(e),
            // Every connection of this library failed, or the library
            // itself did before reaching one: the sync goes on with the
            // next library and ends partial, as when one of several
            // accounts fails, so one store that's down costs only its own.
            // Each connection's failure is recorded already; a library's
            // own is recorded here.
            Err(e) => {
                let recorded = run.failed.lock().unwrap_or_else(|e| e.into_inner()).len() > failed_before;
                if !recorded {
                    run.fail(library.label().to_string(), format!("{e:#}"));
                }
            }
        }
        progress.done += 1;
        sink.update(&progress);
    }
    // Partial needs some library that was read, even if it had no games
    // (a summary). One that read nothing, such as Heroic with no folder or
    // a store with no sign-in, was skipped, not synced, so a failure with
    // nothing else read is the sync's failure: its error names each
    // account that failed and why.
    let failed = run.failed.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if !failed.is_empty() && summaries.is_empty() {
        return Err(anyhow::anyhow!(failed_text(&failed)));
    }
    // A game on a wishlist that's in that player's library now leaves it
    //, for each player with the wishlist on.
    for p in db.players()? {
        if !crate::wishlist::on_for(db, cfg, p.id)? {
            continue;
        }
        let bought = crate::wishlist::settle(db, p.id, &db.all_games(p.id)?)?;
        if !bought.is_empty() {
            let whose = run.names.get(&p.id).map(|n| format!(" ({n})")).unwrap_or_default();
            sink.log(format!("In your library now, so off the wishlist{whose}: {}", bought.join(", ")));
        }
    }
    // Games new to the library get their summaries from the kept shared
    // data, if any was applied.
    match crate::seed::apply_kept(db, cfg, "the kept shared data, after a sync") {
        Ok(Some(done)) if done.written > 0 => {
            sink.log(format!("{} from UpNext's shared data", super::plural(done.written, "summary", "summaries")))
        }
        Ok(_) => {}
        Err(e) => sink.log(format!("The kept shared data wasn't applied: {e:#}")),
    }
    progress.current = None;
    let failed: Vec<String> = run.failed.into_inner().unwrap_or_else(|e| e.into_inner()).into_iter().map(|(who, _)| who).collect();
    progress.message = if failed.is_empty() { "Sync complete".into() } else { "Sync partly complete".into() };
    sink.update(&progress);
    Ok(SyncReport { summaries, failed })
}

/// "2,000 games, 3 new": what the libraries synced.
pub(crate) fn counts_text(summaries: &[SyncSummary]) -> String {
    let total: usize = summaries.iter().map(|s| s.total).sum();
    let added: usize = summaries.iter().map(|s| s.added).sum();
    format!("{}, {} new", super::plural(total, "game", "games"), super::thousands(added))
}

/// Why a sync where nothing was read failed, in the words of its
/// failures' log lines: "Steam (Sam) failed: …; Heroic failed: …".
fn failed_text(failed: &[(String, String)]) -> String {
    failed.iter().map(|(who, why)| format!("{who} failed: {why}")).collect::<Vec<_>>().join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A failed sync no longer stops partway, so there is no "synced before
    /// it stopped" or "not synced" to say: just each failure and why.
    #[test]
    fn a_failed_sync_names_each_failure_and_why() {
        let failed = [("Steam (Sam)", "HTTP 500"), ("Steam (Alex)", "the profile is private"), ("Heroic", "no folder")];
        let failed: Vec<(String, String)> = failed.iter().map(|(w, y)| (w.to_string(), y.to_string())).collect();
        assert_eq!(
            failed_text(&failed),
            "Steam (Sam) failed: HTTP 500; Steam (Alex) failed: the profile is private; Heroic failed: no folder"
        );
    }
}
