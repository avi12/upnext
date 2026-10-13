//! Update the wishlist: what a sync bought leaves it, the Steam
//! wishlist is read, the series you've played suggest their missing
//! entries, and games you don't own get their store data (`crate::wishlist`
//! has the rules). No AI. It runs as its own task, after Fetch details
//! (whose series lists it reads); other enrich runs leave it out unless
//! asked for, and it does nothing at all while the `wishlist` setting is off.

use super::{Opt, Pass};
use crate::db::PlayerId;
use crate::db::{Failure, WishSource, WishState};
use crate::enrich::{Progress, Run};
use crate::wishlist::{self, MAX_SUGGESTED, PER_SERIES};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;

const OPTIONS: &[Opt] = &[Opt::new("wishlist", "Wishlist suggestions and data").off()];

pub struct Wishlist;

impl Wishlist {
    fn fail(&self, run: &Run<'_>, name: &str, error: String) {
        run.fail(Failure {
            lane: self.lane().into(),
            source: self.name().into(),
            options: run.options_on(OPTIONS),
            title_id: None,
            name: Some(name.to_string()),
            error,
            at: 0,
        });
    }
}

#[async_trait]
impl Pass for Wishlist {
    fn options(&self) -> &'static [Opt] {
        OPTIONS
    }
    fn name(&self) -> &'static str {
        "wishlist"
    }
    fn lane(&self) -> &'static str {
        "Wishlist"
    }
    fn wanted(&self, run: &Run<'_>) -> bool {
        run.on("wishlist")
    }

    async fn run(&self, run: &Run<'_>) -> Result<()> {
        let db = run.db;
        // Each player's own wishlist, for the players who have it on.
        let players = db.players()?;
        let mut on: Vec<PlayerId> = vec![];
        for p in &players {
            if wishlist::on_for(db, run.cfg, p.id)? {
                on.push(p.id);
            }
        }
        if on.is_empty() {
            run.log("The wishlist is turned off (Settings › Picks), so nothing was read or fetched".into());
            return Ok(());
        }
        let whose = |p: PlayerId| match players.iter().find(|x| x.id == p) {
            Some(x) if players.len() > 1 => format!(" ({})", x.name),
            _ => String::new(),
        };
        let mut libraries: HashMap<PlayerId, Vec<crate::db::GameView>> = HashMap::new();
        for p in &on {
            let games = db.all_games(*p)?;
            let bought = wishlist::settle(db, *p, &games)?;
            if !bought.is_empty() {
                run.log(format!("In your library now, so off the wishlist{}: {}", whose(*p), bought.join(", ")));
            }
            libraries.insert(*p, games);
        }

        // Each Steam account's wishlist, read-only, into its owner's
        //. Player 1's first falls back to the SteamID this
        // computer's Steam client is signed in with.
        let mut any = false;
        for conn in db.sync_connections("steam")? {
            let sid = conn.setting("steam_id").map(str::to_string).or_else(|| if conn.first { run.cfg.steam_id.clone() } else { None });
            let Some(sid) = sid else { continue };
            any = true;
            if !wishlist::on_for(db, run.cfg, conn.owner)? {
                continue;
            }
            let label = whose(conn.owner);
            let Some(games) = libraries.get(&conn.owner) else { continue };
            match run.clients.steam.wishlist(&sid).await {
                Ok(Some(items)) => {
                    let known: Vec<u32> = db.wishlist_rows(conn.owner)?.iter().map(|r| r.appid).collect();
                    let new: Vec<u32> = items.iter().map(|i| i.appid).filter(|a| !known.contains(a)).collect();
                    let mut names = HashMap::new();
                    for chunk in new.chunks(100) {
                        match crate::steam::store_names(&run.clients.steam, chunk).await {
                            Ok(n) => names.extend(n),
                            Err(e) => self.fail(run, "Steam wishlist names", format!("{e:#}")),
                        }
                    }
                    let changes = wishlist::mirror_steam(db, conn.owner, games, &items, &names)?;
                    run.log(format!(
                        "Steam wishlist{label}: {} ({} new here, {} already, {} taken off since the last read)",
                        crate::enrich::plural(items.len(), "game", "games"),
                        changes.added,
                        crate::enrich::plural(changes.joined, "suggestion", "suggestions"),
                        changes.left
                    ));
                }
                Ok(None) => run.log(format!("Steam showed no wishlist{label} (it may be private); the copy here is kept as it was")),
                Err(e) => self.fail(run, "Steam wishlist", format!("{e:#}")),
            }
            run.check_cancelled()?;
        }
        if !any {
            run.log("No Steam account is set, so there's no Steam wishlist to read".into());
        }
        run.check_cancelled()?;

        // For each player with the wishlist on: suggestions from the series
        // they played, and store data for what their page shows.
        let mut shown_all: Vec<crate::db::WishRow> = vec![];
        let (mut lacking_all, mut no_art_all) = (0, 0);
        for p in on.iter().copied() {
            let games = &libraries[&p];
            // Suggestions from the series you've played.
            let profile = crate::taste::Profile::load(db, p, games)?;
            let collections = crate::series::collections(games, &db.series_guides()?, &db.catalog()?);
            let rows = db.wishlist_rows(p)?;
            let candidates = wishlist::candidates(&collections, games, Some(&profile), &rows);
            let mut waiting: HashMap<String, usize> = HashMap::new();
            for r in rows.iter().filter(|r| r.state == WishState::Suggested && r.source == WishSource::Collection) {
                *waiting.entry(r.series_key.clone().unwrap_or_default()).or_default() += 1;
            }
            let mut total_waiting: usize = waiting.values().sum();
            let mut progress = Progress {
                total: candidates.len(),
                done: 0,
                current: None,
                message: format!("Checking {} from your series", crate::enrich::plural(candidates.len(), "game", "games")),
            };
            run.sink.update(&progress);
            let (mut suggested, mut passed) = (0, 0);
            let mut why_not: HashMap<String, usize> = HashMap::new();
            for c in &candidates {
                run.check_cancelled()?;
                progress.done += 1;
                if total_waiting >= MAX_SUGGESTED {
                    break;
                }
                if waiting.get(&c.series_key).copied().unwrap_or(0) >= PER_SERIES {
                    continue;
                }
                progress.current = Some(c.name.clone());
                run.sink.update(&progress);
                let facts = match wishlist::fetch_facts(db, p, run.clients, c.appid, true).await {
                    Ok(f) => f,
                    Err(e) => {
                        self.fail(run, &c.name, format!("{e:#}"));
                        continue;
                    }
                };
                match wishlist::judge(&facts, Some(&profile)) {
                    Ok(()) => {
                        let (_, new) = db.wishlist_add(
                            p,
                            &crate::db::NewWish {
                                appid: c.appid,
                                name: c.name.clone(),
                                source: WishSource::Collection,
                                reason: Some(c.reason.clone()),
                                state: WishState::Suggested,
                                on_steam: false,
                                series_key: Some(c.series_key.clone()),
                                batch_id: None,
                            },
                        )?;
                        if new {
                            suggested += 1;
                            total_waiting += 1;
                            *waiting.entry(c.series_key.clone()).or_default() += 1;
                        }
                    }
                    Err(why) => {
                        passed += 1;
                        let short = why.split(';').next().unwrap_or(&why).to_string();
                        let key = if short.contains("% of") { "rated too low or by too few".to_string() } else { short };
                        *why_not.entry(key).or_default() += 1;
                    }
                }
            }
            run.add("wishlist", suggested);
            let mut reasons: Vec<String> = why_not.into_iter().map(|(k, n)| format!("{n} {k}")).collect();
            reasons.sort();
            run.log(format!(
                "Your series{}: {} you don't own could be suggested; {}, {passed} passed over{}",
                whose(p),
                crate::enrich::plural(candidates.len(), "game", "games"),
                crate::enrich::plural(suggested, "new suggestion", "new suggestions"),
                if reasons.is_empty() { String::new() } else { format!(" ({})", reasons.join(", ")) }
            ));

            // Store data for what the page shows: the wishlist and the suggestions.
            let shown: Vec<_> =
                db.wishlist_rows(p)?.into_iter().filter(|r| matches!(r.state, WishState::Suggested | WishState::Wishlisted)).collect();
            let appids: Vec<u32> = shown.iter().map(|r| r.appid).collect();
            let facts = db.store_facts(&appids)?;
            let lacking: Vec<&crate::db::WishRow> = shown
                .iter()
                .filter(|r| facts.get(&r.appid).is_none_or(|f| !f.reviews_checked || !f.details_checked || !f.tags_checked))
                .collect();
            let mut progress =
                Progress { total: lacking.len(), done: 0, current: None, message: "Fetching store data for games you don't own".into() };
            run.sink.update(&progress);
            for r in &lacking {
                run.check_cancelled()?;
                progress.current = Some(r.name.clone());
                run.sink.update(&progress);
                if let Err(e) = wishlist::fetch_facts(db, p, run.clients, r.appid, false).await {
                    self.fail(run, &r.name, format!("{e:#}"));
                }
                progress.done += 1;
            }
            let no_art: Vec<u32> = shown.iter().filter(|r| facts.get(&r.appid).is_none_or(|f| !f.art_checked)).map(|r| r.appid).collect();
            if let Err(e) = wishlist::fetch_art(db, run.clients, &no_art).await {
                self.fail(run, "Steam art", format!("{e:#}"));
            }
            lacking_all += lacking.len();
            no_art_all += no_art.len();
            shown_all.extend(shown);
        }
        let shown = shown_all;
        // HowLongToBeat, by name: its table is per library title, so the time
        // is kept on the wishlist row. Only when HowLongToBeat is turned on
        // (its job has a schedule) or this run asks for it: it's scraped.
        let hltb_on = run.on("hltb") || crate::jobs::tasks::hltb_scheduled(db)?;
        let no_time: Vec<_> = if hltb_on { shown.iter().filter(|r| r.hltb_at.is_none() || run.refresh).collect() } else { vec![] };
        let mut timed = 0;
        let mut misses = 0;
        for r in no_time {
            run.check_cancelled()?;
            match run.clients.hltb.lookup(&r.name).await {
                Ok(found) => {
                    let main = found.and_then(|h| h.main_story);
                    db.wishlist_set_hltb(r.id, main)?;
                    if main.is_some() {
                        timed += 1;
                    }
                    misses = 0;
                }
                Err(e) => {
                    self.fail(run, &r.name, format!("HowLongToBeat: {e:#}"));
                    misses += 1;
                    // The site's search is scraped; when it breaks, it breaks for every game.
                    if misses >= 3 {
                        run.log("HowLongToBeat failed three times in a row; the rest wait for the next run".into());
                        break;
                    }
                }
            }
        }
        run.log(format!(
            "On the page: {} and {} wishlisted; store data looked up for {}, art for {}, {}",
            crate::enrich::plural(shown.iter().filter(|r| r.state == WishState::Suggested).count(), "suggestion", "suggestions"),
            shown.iter().filter(|r| r.state == WishState::Wishlisted).count(),
            lacking_all,
            no_art_all,
            if hltb_on { format!("HowLongToBeat times found for {timed}") } else { "HowLongToBeat is off".to_string() }
        ));
        Ok(())
    }
}
