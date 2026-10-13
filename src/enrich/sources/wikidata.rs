//! Wikidata cross-references: the ids other sources use (PCGamingWiki page,
//! RAWG slug, IGDB slug) and the item series come from.

use super::{Opt, Pass};
use crate::db::{GameView, Status};
use crate::enrich::{Progress, Run};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;

const OPTIONS: &[Opt] = &[Opt::new("wikidata", "Wikidata links")];

/// Batch lookups for titles without a cross-reference yet: by Steam appid
/// first, then by exact English title for the rest.
pub struct Crossref;

#[async_trait]
impl Pass for Crossref {
    fn options(&self) -> &'static [Opt] {
        OPTIONS
    }
    fn name(&self) -> &'static str {
        "Wikidata cross-referencing"
    }
    fn lane(&self) -> &'static str {
        "Wikidata"
    }
    fn wanted(&self, run: &Run<'_>) -> bool {
        run.on("wikidata")
    }
    fn errors_are_fatal(&self) -> bool {
        false
    }

    async fn run(&self, run: &Run<'_>) -> Result<()> {
        let (db, sink) = (run.db, run.sink);
        let pending: Vec<GameView> =
            db.library_games()?.into_iter().filter(|g| g.status != Status::NotGame && (!g.has_xref || run.refresh)).collect();
        if pending.is_empty() {
            return Ok(());
        }
        run.log(format!("{} Wikidata cross-references", crate::enrich::plural(pending.len(), "title needs", "titles need")));
        let mut progress = Progress { total: pending.len(), done: 0, current: None, message: "Cross-referencing with Wikidata".into() };
        sink.update(&progress);

        let mut found: HashMap<i64, crate::wikidata::Xref> = HashMap::new();
        let with_appid: Vec<&GameView> = pending.iter().filter(|g| g.appid.is_some()).collect();
        for chunk in with_appid.chunks(150) {
            run.check_cancelled()?;
            let appids: Vec<u32> = chunk.iter().filter_map(|g| g.appid).collect();
            let hits = run.clients.wikidata.by_steam_appids(&appids).await?;
            for g in chunk {
                if let Some(x) = g.appid.and_then(|a| hits.get(&a)) {
                    found.insert(g.id, x.clone());
                }
            }
            progress.done += chunk.len();
            sink.update(&progress);
        }

        let by_name: Vec<&GameView> = pending.iter().filter(|g| !found.contains_key(&g.id)).collect();
        progress.message = "Cross-referencing with Wikidata by title".into();
        progress.total = with_appid.len() + by_name.len();
        for chunk in by_name.chunks(25) {
            run.check_cancelled()?;
            let labels: Vec<(i64, Vec<String>)> = chunk.iter().map(|g| (g.id, crate::wikidata::label_candidates(&g.name))).collect();
            let mut all: Vec<String> = labels.iter().flat_map(|(_, l)| l.iter().cloned()).collect();
            all.sort();
            all.dedup();
            let hits = run.clients.wikidata.by_names(&all).await?;
            for (id, cands) in &labels {
                if let Some(x) = cands.iter().find_map(|c| hits.get(c)) {
                    found.insert(*id, x.clone());
                }
            }
            progress.done += chunk.len();
            sink.update(&progress);
        }

        for g in &pending {
            db.put_xref(g.id, found.get(&g.id))?;
        }
        run.add("wikidata", found.len());
        run.log(format!("Wikidata: {} of {} cross-referenced", found.len(), crate::enrich::plural(pending.len(), "title", "titles")));
        Ok(())
    }
}
