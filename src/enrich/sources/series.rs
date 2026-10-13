//! Series from Wikidata: which series each title is in, the full list of
//! games in each series (so Collections can show the ones you don't own),
//! and covers for those.

use super::{Opt, Pass};
use crate::db::Status;
use crate::enrich::{Progress, Run};
use crate::steam;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};

const OPTIONS: &[Opt] = &[Opt::new("series", "Series")];

/// Wikidata series (P179) and original release year for titles that have a
/// Wikidata item but no series lookup yet.
pub struct Membership;

#[async_trait]
impl Pass for Membership {
    fn options(&self) -> &'static [Opt] {
        OPTIONS
    }
    fn name(&self) -> &'static str {
        "Wikidata series lookup"
    }
    fn lane(&self) -> &'static str {
        "Wikidata"
    }
    fn wanted(&self, run: &Run<'_>) -> bool {
        run.on("series")
    }
    fn errors_are_fatal(&self) -> bool {
        false
    }

    async fn run(&self, run: &Run<'_>) -> Result<()> {
        let db = run.db;
        let pending: Vec<(i64, String)> = db
            .library_games()?
            .into_iter()
            .filter(|g| g.status != Status::NotGame && (!g.has_title_series || run.refresh))
            .filter_map(|g| g.xref.as_ref().map(|x| (g.id, x.qid.clone())))
            .filter(|(_, q)| !q.is_empty())
            .collect();
        if pending.is_empty() {
            return Ok(());
        }
        run.log(format!("{} a Wikidata series lookup", crate::enrich::plural(pending.len(), "title needs", "titles need")));
        let mut progress = Progress { total: pending.len(), done: 0, current: None, message: "Looking up series on Wikidata".into() };
        run.sink.update(&progress);
        let mut in_series = 0;
        for chunk in pending.chunks(150) {
            run.check_cancelled()?;
            let qids: Vec<String> = chunk.iter().map(|(_, q)| q.clone()).collect();
            let found = crate::wikidata::series_for(&run.clients.wikidata, &qids).await?;
            for (id, qid) in chunk {
                let (labels, year) = found.get(qid).cloned().unwrap_or_default();
                db.put_title_series(*id, &labels, year)?;
                if !labels.is_empty() {
                    run.count("series");
                    in_series += 1;
                }
            }
            progress.done += chunk.len();
            run.sink.update(&progress);
        }
        run.log(format!(
            "Wikidata: {in_series} of {} {} part of a series",
            crate::enrich::plural(pending.len(), "title", "titles"),
            if in_series == 1 { "is" } else { "are" }
        ));
        Ok(())
    }
}

/// The full game list of every series an owned game belongs to, 20 series
/// per query.
pub struct Lists;

#[async_trait]
impl Pass for Lists {
    fn name(&self) -> &'static str {
        "Wikidata series lists"
    }
    fn lane(&self) -> &'static str {
        "Wikidata"
    }
    fn wanted(&self, run: &Run<'_>) -> bool {
        run.on("series")
    }
    fn errors_are_fatal(&self) -> bool {
        false
    }

    async fn run(&self, run: &Run<'_>) -> Result<()> {
        let db = run.db;
        // Series named by PCGamingWiki but not Wikidata: find the series item by name.
        let tried = if run.refresh { Default::default() } else { db.series_keys_tried()? };
        let mut by_label: HashMap<String, String> = HashMap::new(); // label -> key
        for g in db.library_games()?.iter().filter(|g| g.status != Status::NotGame) {
            for label in &g.series {
                let key = crate::series::series_key(label);
                if !key.is_empty() && !g.series_qids.contains_key(&key) && !tried.contains(&key) {
                    by_label.entry(crate::series::decode_entities(label).trim().to_string()).or_insert(key);
                }
            }
        }
        // Keys that some game does know the QID of don't need a name lookup.
        let known: HashSet<String> = db.library_games()?.iter().flat_map(|g| g.series_qids.keys().cloned()).collect();
        by_label.retain(|_, k| !known.contains(k));
        if !by_label.is_empty() {
            let labels: Vec<String> = by_label.keys().cloned().collect();
            for chunk in labels.chunks(50) {
                run.check_cancelled()?;
                let found = crate::wikidata::series_by_labels(&run.clients.wikidata, chunk).await?;
                for l in chunk {
                    db.put_series_key(&by_label[l], found.get(l).map(String::as_str))?;
                }
            }
        }
        let pending = db.series_without_catalog(run.refresh)?;
        if pending.is_empty() {
            return Ok(());
        }
        run.log(format!("{} their full game list", crate::enrich::plural(pending.len(), "series needs", "series need")));
        let mut progress = Progress { total: pending.len(), done: 0, current: None, message: "Listing every game in each series".into() };
        run.sink.update(&progress);
        for chunk in pending.chunks(20) {
            run.check_cancelled()?;
            let found = crate::wikidata::series_members(&run.clients.wikidata, chunk).await?;
            for q in chunk {
                db.put_series_catalog(q, found.get(q).map(Vec::as_slice).unwrap_or_default())?;
            }
            progress.done += chunk.len();
            run.sink.update(&progress);
        }
        Ok(())
    }
}

/// Covers for the games in each series you don't own: the Steam capsule
/// (its real, often hashed, path), else IGDB's cover, else GOG's store
/// image. Each entry is looked up once. Runs when both series and cover art
/// are on.
pub struct Covers;

#[async_trait]
impl Pass for Covers {
    fn name(&self) -> &'static str {
        "series covers"
    }
    fn lane(&self) -> &'static str {
        "Series covers"
    }
    fn wanted(&self, run: &Run<'_>) -> bool {
        run.on("series") && run.on("art")
    }
    fn errors_are_fatal(&self) -> bool {
        false
    }

    async fn run(&self, run: &Run<'_>) -> Result<()> {
        let (db, clients, sink) = (run.db, run.clients, run.sink);
        let mut catalog = db.series_catalog()?;
        let todo: Vec<crate::wikidata::SeriesEntry> = catalog.values().flatten().filter(|e| !e.cover_checked).cloned().collect();
        if todo.is_empty() {
            return Ok(());
        }
        let mut found: HashMap<String, String> = HashMap::new(); // entry qid -> cover
        let mut progress =
            Progress { total: todo.len(), done: 0, current: None, message: "Finding covers for series games you don't own".into() };
        sink.update(&progress);

        // Steam, 100 apps per request.
        let appids: Vec<u32> = {
            let mut v: Vec<u32> = todo.iter().filter_map(|e| e.steam_appid).collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        let mut steam: HashMap<u32, String> = HashMap::new();
        for chunk in appids.chunks(100) {
            run.check_cancelled()?;
            match steam::store_art(&clients.steam, chunk).await {
                Ok(art) => {
                    for (appid, (portrait, hero, _)) in art {
                        if let Some(url) = portrait.or(hero) {
                            steam.insert(appid, url);
                        }
                    }
                }
                Err(e) => run.log(format!("Steam art for series games failed: {e:#}")),
            }
        }
        for e in &todo {
            if let Some(url) = e.steam_appid.and_then(|a| steam.get(&a)) {
                found.insert(e.qid.clone(), url.clone());
            }
        }
        progress.done = found.len();
        sink.update(&progress);

        // IGDB by slug, 500 per request (only when this run set IGDB up).
        if let Some(igdb) = run.igdb_if_started() {
            let slugs: Vec<String> = todo.iter().filter(|e| !found.contains_key(&e.qid)).filter_map(|e| e.igdb_slug.clone()).collect();
            for chunk in slugs.chunks(500) {
                match igdb.covers_by_slug(chunk).await {
                    Ok(covers) => {
                        for e in &todo {
                            if found.contains_key(&e.qid) {
                                continue;
                            }
                            if let Some(url) = e.igdb_slug.as_ref().and_then(|s| covers.get(s)) {
                                found.insert(e.qid.clone(), url.clone());
                            }
                        }
                    }
                    Err(e) => run.log(format!("IGDB covers failed: {e:#}")),
                }
            }
            progress.done = found.len();
            sink.update(&progress);
        }

        // GOG's store image, one product at a time. Wikidata gives next to
        // no entry a product id, so the store page's slug is looked up in
        // GOG's catalogue for one, until a lookup fails.
        let mut catalogue = true;
        for e in &todo {
            if found.contains_key(&e.qid) {
                continue;
            }
            let id = match (e.gog_id.as_ref().filter(|i| i.chars().all(|c| c.is_ascii_digit())), &e.gog_slug) {
                (Some(id), _) => id.clone(),
                (None, Some(slug)) if catalogue => {
                    run.check_cancelled()?;
                    match clients.gogmeta.id_by_slug(slug).await {
                        Ok(Some(id)) => id,
                        Ok(None) => continue,
                        Err(err) => {
                            run.log(format!("GOG's catalogue failed, so series games without a GOG id get no GOG cover: {err:#}"));
                            catalogue = false;
                            continue;
                        }
                    }
                }
                _ => continue,
            };
            run.check_cancelled()?;
            if let Some(img) = clients.gogmeta.product_image(&id).await {
                found.insert(e.qid.clone(), img);
            }
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }

        // Store: every entry looked at is marked, found or not.
        let mut with_cover = 0;
        for (series, entries) in catalog.iter_mut() {
            let mut changed = false;
            for e in entries.iter_mut().filter(|e| !e.cover_checked) {
                e.cover = found.get(&e.qid).cloned();
                e.cover_checked = true;
                with_cover += e.cover.is_some() as usize;
                changed = true;
            }
            if changed {
                db.put_series_catalog(series, entries)?;
            }
        }
        run.log(format!("Covers for {with_cover} of {} you don't own", crate::enrich::plural(todo.len(), "series game", "series games")));
        Ok(())
    }
}
