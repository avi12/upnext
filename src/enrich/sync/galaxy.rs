//! A copy of GOG Galaxy's `galaxy-2.0.db`: every store Galaxy knows about.
//! Runs when a database path is given for the sync, or set on a Galaxy
//! connection. Each database is one connection, its player's.

use super::{Library, Outcomes, SyncRun};
use crate::config::Config;
use crate::db::{Db, ImportedGame, PlayerId, SyncConnection, SyncSummary};
use crate::enrich::{Clients, Progress};
use crate::galaxy;
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub struct Galaxy;

#[async_trait]
impl Library for Galaxy {
    fn id(&self) -> &'static str {
        "galaxy"
    }
    fn label(&self) -> &'static str {
        "Galaxy database"
    }
    fn blocked(&self, cfg: &Config, _clients: &Clients) -> Option<&'static str> {
        cfg.galaxy_db.is_none().then_some("no path set")
    }
    fn ready(&self, run: &SyncRun<'_>) -> bool {
        run.opts.galaxy_db.is_some()
            || run.db.sync_connections("galaxy").is_ok_and(|all| all.iter().any(|c| c.setting("galaxy_db").is_some()))
    }

    async fn sync(&self, run: &SyncRun<'_>, progress: &mut Progress) -> Result<Vec<SyncSummary>> {
        let mut outcomes = Outcomes::default();
        for mut conn in run.connections("galaxy")? {
            // The sync's own path is player 1's first Galaxy
            // database's; another player's is the one set on their connection.
            let path = match conn.first {
                true => run.opts.galaxy_db.clone().or_else(|| conn.setting("galaxy_db").map(str::to_string)),
                false => conn.setting("galaxy_db").map(str::to_string),
            };
            let Some(path) = path else { continue };
            let result = sync_database(run, progress, &mut conn, &PathBuf::from(path)).await;
            outcomes.add(run, "Galaxy", &conn, result)?;
        }
        outcomes.finish()
    }
}

/// One Galaxy database, into its owner's accounts: Galaxy's Steam games go
/// to their Steam account (their first, when they have two; Galaxy's
/// database doesn't say which Steam account it is linked to), its GOG games
/// to their GOG account, and so on.
async fn sync_database(run: &SyncRun<'_>, progress: &mut Progress, conn: &mut SyncConnection, path: &Path) -> Result<Vec<SyncSummary>> {
    let (db, sink) = (run.db, run.sink);
    let whose = run.whose(conn);
    let mut summaries = vec![];
    progress.message = "Reading Galaxy database".into();
    sink.update(progress);
    let skip_setting = match conn.setting("galaxy_skip") {
        Some(own) => Some(own.to_string()),
        None if conn.first => run.cfg.galaxy_skip.clone(),
        None => None,
    };
    let skip = galaxy::skip_list(skip_setting.as_deref());
    let lib = galaxy::read_library(path, &skip).with_context(|| format!("Galaxy import from {}", path.display()))?;
    for i in &lib.integrations {
        sink.log(format!("Galaxy{whose}: {}", galaxy::integration_line(i)));
    }
    if lib.not_owned + lib.non_games > 0 {
        sink.log(format!(
            "Galaxy{whose}: left out {} not owned (family shared or a subscription) and {}",
            lib.not_owned,
            crate::enrich::plural(lib.non_games, "beta, test server or demo", "betas, test servers and demos")
        ));
    }
    let before = db.launcher_sources(conn.owner, "galaxy")?;
    let mut by_source: std::collections::BTreeMap<String, Vec<ImportedGame>> = Default::default();
    for g in lib.games {
        by_source.entry(g.source.clone()).or_default().push(g);
    }
    let mut total = 0;
    for (source, games) in by_source {
        let s = db.sync_into(conn, &source, &games)?;
        sink.log(format!(
            "Galaxy{whose}/{source}: {} ({} new, {} updated)",
            crate::enrich::plural(s.total, "game", "games"),
            s.added,
            s.updated
        ));
        total += s.total;
        summaries.push(s);
    }
    for (source, copies) in before {
        let label = galaxy::platform_label(&source);
        match lib.integrations.iter().find(|i| i.platform == source) {
            // Left out by choice: its copies go.
            Some(i) if i.left_out => {
                let n = db.remove_launcher_source(conn.owner, "galaxy", &source)?;
                let removed = crate::enrich::plural(n, &format!("{label} copy"), &format!("{label} copies"));
                sink.log(format!("Galaxy{whose}: removed {removed}, left out in Settings"));
            }
            Some(_) => {}
            // Galaxy's other stores come through plugins that can
            // disappear: absence isn't a reason to drop what was read.
            None => sink.log(format!(
                "Galaxy{whose}: no {label} games in Galaxy now; {} read before {} kept (leave {source} out in Settings to remove them)",
                if copies == 1 { "the 1 copy".to_string() } else { format!("the {copies} copies") },
                if copies == 1 { "is" } else { "are" }
            )),
        }
    }
    remember_integrations(db, conn.owner, &lib.integrations)?;
    db.record_galaxy_sync(conn, total)?;
    Ok(summaries)
}

/// The meta key holding every integration player 1's Galaxy syncs have
/// seen; another player's are under `galaxy_integrations.<player id>`.
pub const SEEN_KEY: &str = "galaxy_integrations";

fn seen_key(p: PlayerId) -> String {
    if p == PlayerId::FIRST { SEEN_KEY.to_string() } else { format!("{SEEN_KEY}.{}", p.0) }
}

/// An integration as Settings lists it: what the last sync that saw it said.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SeenIntegration {
    #[serde(flatten)]
    pub integration: galaxy::Integration,
    /// Its name as people know it: "EA app".
    pub label: String,
    /// When a sync last found games from it (Unix seconds).
    pub seen_at: i64,
}

/// Keep the integrations this sync found, and the ones earlier syncs found
/// that are gone now, so Settings can still offer to leave them out.
fn remember_integrations(db: &Db, p: PlayerId, found: &[galaxy::Integration]) -> Result<()> {
    let mut all = seen_integrations_of(db, p);
    let at = crate::db::now();
    for i in found {
        let seen = SeenIntegration { integration: i.clone(), label: galaxy::platform_label(&i.platform), seen_at: at };
        match all.iter_mut().find(|s| s.integration.platform == i.platform) {
            Some(s) => *s = seen,
            None => all.push(seen),
        }
    }
    db.meta_set(&seen_key(p), &serde_json::to_string(&all)?)
}

/// Every integration player 1's Galaxy syncs have seen, GOG's own first,
/// as Settings lists them.
pub fn seen_integrations(db: &Db) -> Vec<SeenIntegration> {
    seen_integrations_of(db, PlayerId::FIRST)
}

/// Every integration player `p`'s Galaxy syncs have seen, GOG's own first.
pub fn seen_integrations_of(db: &Db, p: PlayerId) -> Vec<SeenIntegration> {
    let mut all: Vec<SeenIntegration> =
        db.meta_get(&seen_key(p)).ok().flatten().and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default();
    all.sort_by_key(|s| (s.integration.platform != "gog", std::cmp::Reverse(s.integration.games)));
    all
}
