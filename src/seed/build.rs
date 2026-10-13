//! Building UpNext's shared data: the 10,000 most-owned
//! games on Steam, their reviews, the summaries written from them, and the
//! export to the release assets the apply side reads.
//!
//! The build runs in a database of its own (`UPNEXT_DATA_DIR` set to an
//! empty folder), never in a library: the list comes from SteamSpy's
//! catalogue, so nothing in the data says what anyone owns. Its titles are
//! copies of a "catalogue" source with their Steam appids, so the usual
//! enrich runs on them unchanged:
//!
//! 1. [`build`]: SteamSpy's catalogue, most owned first, and the Steam
//!    store's type for each title, until the list holds enough games (no
//!    DLC, soundtracks or tools). About five hours of store requests.
//! 2. [`fetch`]: the reviews (about nine hours) and the GOG ids, from
//!    Wikidata and, by the store page's slug, from GOG's catalogue, so GOG
//!    copies match too.
//! 3. [`generate`]: the summaries, on the AI the build database's settings
//!    name, then once more for any that copied a review word for word.
//! 4. [`export`]: `upnext-seed-<date>.jsonl.gz` and `upnext-seed.json`.
//!
//! Every step is resumable: it stores as it goes and skips what is done, so
//! a stopped run is carried on by running the same command again.

use super::verbatim::{COPIED_WORDS, longest_copied};
use super::{Entry, Manifest, is_date, manifest, read, write};
use crate::config::Config;
use crate::db::PlayerId;
use crate::db::{Db, GameView, ImportedGame};
use crate::enrich::{self, Clients, EnrichOptions, Progress, ProgressSink};
use crate::llm::LlmBackend;
use crate::steamspy::Listed;
use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// The launcher and the source of the build's titles: SteamSpy's catalogue,
/// not anyone's library.
pub const CATALOGUE: &str = "catalogue";
/// How many games the shared data covers (decided 2026-09-30).
pub const DEFAULT_COUNT: usize = 10_000;
/// Marks a database as a build of the shared data, with the day it started.
const MARK: &str = "seed_build";
/// Apps on a page of SteamSpy's catalogue.
const PAGE: usize = 1000;
/// What a summary that copied a review is asked again with.
pub const OWN_WORDS: &str = "One more firm rule: the last summaries written for these games repeated a review word for \
word. Never copy a run of words from a review, not even a short phrase: say everything in your own words.";

/// The folder beside the build's database that keeps what isn't in it:
/// SteamSpy's pages, the GOG ids, and the titles asked again.
pub fn work_dir(cfg: &Config) -> PathBuf {
    cfg.db_dir().join("seed-build")
}

fn today() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

/// A day from Unix seconds, YYYY-MM-DD.
fn ymd(at: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(at, 0).map(|d| d.format("%Y-%m-%d").to_string())
}

/// Check that the database is a build of the shared data and never a
/// library. With `start`, an empty database in a folder chosen for it
/// becomes one.
pub fn check(db: &Db, cfg: &Config, start: bool) -> Result<()> {
    let library = db.copies_not_from(CATALOGUE)?;
    if library > 0 {
        bail!(
            "this database has a library in it ({} from launchers), and the shared data is never built from a library. \
             Set UPNEXT_DATA_DIR to a folder of its own for the build, e.g. UPNEXT_DATA_DIR=~/upnext-seed",
            crate::enrich::plural(library, "copy", "copies")
        );
    }
    if db.meta_get(MARK)?.is_some() {
        return Ok(());
    }
    if !start {
        bail!(
            "this database isn't a build of the shared data. `upnext seed build` starts one, with UPNEXT_DATA_DIR set to a folder of its own"
        );
    }
    if cfg.source("data_dir") == "default" && cfg.source("db_path") == "default" {
        bail!(
            "the shared data is built in a database of its own, not UpNext's usual one: set UPNEXT_DATA_DIR to an empty folder for it, \
             e.g. UPNEXT_DATA_DIR=~/upnext-seed upnext seed build"
        );
    }
    mark(db)
}

/// Make this database a build of the shared data.
fn mark(db: &Db) -> Result<()> {
    if !db.all_games(PlayerId::FIRST)?.is_empty() {
        bail!("this database already has games in it; the shared data is built in an empty one");
    }
    db.meta_set(MARK, &today())
}

/// Enrich options with only these sources on. The Steam store step runs
/// whatever the options say.
pub fn only(ids: &[&str]) -> EnrichOptions {
    EnrichOptions {
        sources: enrich::sources::ids().into_iter().map(|id| (id.to_string(), ids.contains(&id))).collect(),
        ..Default::default()
    }
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("couldn't make {}", dir.display()))?;
    }
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    std::fs::write(&part, bytes).with_context(|| format!("couldn't write {}", part.display()))?;
    std::fs::rename(&part, path).with_context(|| format!("couldn't write {}", path.display()))
}

fn read_json<T: serde::de::DeserializeOwned + Default>(path: &Path) -> Result<T> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).with_context(|| format!("couldn't read {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e).with_context(|| format!("couldn't read {}", path.display())),
    }
}

fn cancelled(sink: &dyn ProgressSink) -> Result<()> {
    if sink.cancelled() {
        bail!("cancelled");
    }
    Ok(())
}

// -- the list ---------------------------------------------------------------------

/// Most owned first. SteamSpy gives owners in bands ("2,000,000 ..
/// 5,000,000"), so within a band the game with more Steam reviews goes
/// first, then the lower appid.
fn rank(mut apps: Vec<Listed>) -> Vec<Listed> {
    apps.sort_by(|a, b| b.owners.cmp(&a.owners).then(b.reviews.cmp(&a.reviews)).then(a.appid.cmp(&b.appid)));
    let mut seen = HashSet::new();
    apps.retain(|a| seen.insert(a.appid));
    apps
}

/// SteamSpy's catalogue as far as `pages` pages, ranked. Pages fetched
/// before are read from the work folder; a page past the end is kept too,
/// empty, and nothing after it is asked for.
async fn catalogue(clients: &Clients, dir: &Path, pages: usize, sink: &dyn ProgressSink) -> Result<Vec<Listed>> {
    let mut all = vec![];
    for page in 0..pages {
        let file = dir.join("steamspy").join(format!("page-{page:03}.json"));
        let apps: Vec<Listed> = if file.is_file() {
            read_json(&file)?
        } else {
            cancelled(sink)?;
            sink.log(format!("SteamSpy's catalogue, page {} of {pages} (SteamSpy allows one a minute)", page + 1));
            let apps = clients.spy.all_page(page as u32).await.context("SteamSpy's catalogue")?;
            write_file(&file, &serde_json::to_vec(&apps)?)?;
            apps
        };
        if apps.is_empty() {
            break;
        }
        all.extend(apps);
    }
    Ok(rank(all))
}

/// Make the build's titles exactly these apps. Titles that drop out of the
/// list are marked removed, and their store details stay for a later round.
fn place(db: &Db, apps: &[&Listed]) -> Result<()> {
    let games: Vec<ImportedGame> = apps
        .iter()
        .map(|a| ImportedGame {
            source: CATALOGUE.into(),
            source_id: a.appid.to_string(),
            launcher: CATALOGUE.into(),
            name: a.name.clone(),
            appid: Some(a.appid),
            ..Default::default()
        })
        .collect();
    db.sync_source(PlayerId::FIRST, CATALOGUE, CATALOGUE, &games)?;
    Ok(())
}

/// What the Steam store says each title is, by appid: `Some("game")`,
/// `Some("dlc")` and so on, or None when the store has no page for it.
/// Titles not asked about yet are missing.
fn store_types(db: &Db) -> Result<HashMap<u32, Option<String>>> {
    Ok(db.all_games(PlayerId::FIRST)?.into_iter().filter(|g| g.has_details).filter_map(|g| Some((g.appid?, g.type_))).collect())
}

fn is_game(types: &HashMap<u32, Option<String>>, appid: u32) -> bool {
    types.get(&appid).is_some_and(|t| t.as_deref() == Some("game"))
}

/// What [`build`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Built {
    /// Games in the build now.
    pub games: usize,
    /// Apps of the catalogue read so far.
    pub listed: usize,
    /// Apps above the last game chosen that aren't games (DLC, soundtracks,
    /// tools) or have no store page.
    pub skipped: usize,
}

/// Put the `count` most-owned games of SteamSpy's catalogue in the build:
/// rank the catalogue, ask the Steam store what the top ones are, and take
/// more of the list until enough of them are games.
pub async fn build(db: &Db, cfg: &Config, clients: &Clients, count: usize, sink: &dyn ProgressSink) -> Result<Built> {
    let dir = work_dir(cfg);
    // About one app in ten at the top of the catalogue isn't a game.
    let mut pages = (count * 13 / 10).div_ceil(PAGE).max(1);
    let mut list = catalogue(clients, &dir, pages, sink).await?;
    let mut take = count.min(list.len());
    let mut unanswered = usize::MAX;
    loop {
        place(db, &list[..take].iter().collect::<Vec<_>>())?;
        sink.log(format!(
            "{take} of SteamSpy's most-owned apps in the build; asking the Steam store what {}",
            if take == 1 { "it is" } else { "they are" }
        ));
        enrich::run_enrich(db, cfg, clients, None, &only(&[]), sink).await?;
        let types = store_types(db)?;
        let placed = &list[..take];
        let unknown = placed.iter().filter(|a| !types.contains_key(&a.appid)).count();
        if unknown > 0 {
            // Store requests that failed: once more, then stop.
            if unknown >= unanswered {
                bail!(
                    "the Steam store didn't answer for {}. Run `upnext seed build` again later: it carries on where it stopped",
                    crate::enrich::plural(unknown, "title", "titles")
                );
            }
            unanswered = unknown;
            continue;
        }
        unanswered = usize::MAX;
        let games = placed.iter().filter(|a| is_game(&types, a.appid)).count();
        if games >= count {
            break;
        }
        // More of the list, in the proportion of games so far, and a few over.
        let want = take + ((count - games) * take).div_ceil(games.max(1)) + 10;
        if want > list.len() && want.div_ceil(PAGE) > pages {
            pages = want.div_ceil(PAGE);
            list = catalogue(clients, &dir, pages, sink).await?;
        }
        let next = want.min(list.len());
        if next <= take {
            sink.log(format!("SteamSpy's catalogue has no more apps: the build has {}", crate::enrich::plural(games, "game", "games")));
            break;
        }
        take = next;
    }
    let types = store_types(db)?;
    let chosen: Vec<&Listed> = list[..take].iter().filter(|a| is_game(&types, a.appid)).take(count).collect();
    let last = chosen.last().and_then(|c| list.iter().position(|a| a.appid == c.appid)).map_or(0, |at| at + 1);
    place(db, &chosen)?;
    Ok(Built { games: chosen.len(), listed: list.len(), skipped: last - chosen.len() })
}

// -- the fetch ----------------------------------------------------------------------

/// The build's games: every title whose store page says it's a game.
fn games(db: &Db) -> Result<Vec<GameView>> {
    Ok(db
        .all_games(PlayerId::FIRST)?
        .into_iter()
        .filter(|g| g.type_.as_deref() == Some("game") && g.copies.iter().any(|c| c.launcher == CATALOGUE))
        .collect())
}

/// The sources [`fetch`] runs: Steam's review score and texts, which come
/// in the same requests.
pub const FETCH: &[&str] = &["reviews", "review_texts"];

/// What [`fetch`] did.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Fetched {
    /// Titles that got their reviews now.
    pub reviews: usize,
    /// Games with a GOG id, in all.
    pub gog_ids: usize,
    /// Request failures, which the next run tries again.
    pub errors: usize,
}

/// The reviews of every game in the build (the usual enrich: score and
/// texts in the same requests), then their GOG ids.
pub async fn fetch(db: &Db, cfg: &Config, clients: &Clients, sink: &dyn ProgressSink) -> Result<Fetched> {
    let s = enrich::run_enrich(db, cfg, clients, None, &only(FETCH), sink).await?;
    let (gog_ids, failed) = gog_ids(db, cfg, clients, sink).await?;
    Ok(Fetched { reviews: s.count("review_texts"), gog_ids, errors: s.errors + failed })
}

/// Named for the property asked: the first builds asked for P2725, the
/// store page's slug, and kept "none" for every game in `gog-ids.json`, so
/// a new name has the next run ask again instead of trusting that file.
fn gog_ids_file(cfg: &Config) -> PathBuf {
    work_dir(cfg).join("gog-product-ids.json")
}

/// The slug of each game's one GOG store page on Wikidata, by Steam appid:
/// None where it has none, or two.
fn gog_pages_file(cfg: &Config) -> PathBuf {
    work_dir(cfg).join("gog-store-pages.json")
}

/// What GOG's catalogue answered for each store page's slug: its product
/// id, or None when the catalogue lists no product with that slug. A
/// "none" is kept for good: deleting the file has the next run ask every
/// slug again.
fn gog_slug_ids_file(cfg: &Config) -> PathBuf {
    work_dir(cfg).join("gog-ids-by-slug.json")
}

/// GOG product ids by Steam appid, kept in the work folder: None where
/// there is none. Wikidata gives a product id for next to no game, but the
/// slug of the GOG store page for many, so the ids it lacks are looked up by
/// that slug in GOG's own catalogue. Each appid is asked of Wikidata once,
/// and each slug of GOG once. A failed request is logged and stops that
/// step for this run, leaving what was found before it: how many games have
/// an id, and the failed steps.
async fn gog_ids(db: &Db, cfg: &Config, clients: &Clients, sink: &dyn ProgressSink) -> Result<(usize, usize)> {
    let path = gog_ids_file(cfg);
    let mut known: BTreeMap<u32, Option<String>> = read_json(&path)?;
    let mut pages: BTreeMap<u32, Option<String>> = read_json(&gog_pages_file(cfg))?;
    let appids: Vec<u32> = games(db)?.iter().filter_map(|g| g.appid).collect();
    let mut failed = wikidata_gog_ids(cfg, clients, sink, &appids, &mut known, &mut pages).await?;
    failed += catalogue_gog_ids(cfg, clients, sink, &mut known, &pages).await?;
    Ok((known.values().filter(|g| g.is_some()).count(), failed))
}

/// GOG product ids (P12727) and store pages' slugs (P2725) from Wikidata,
/// for the appids not asked yet: a build from before the slugs were kept
/// asks again for all of them, keeping the ids it has. 1 when a query failed.
async fn wikidata_gog_ids(
    cfg: &Config,
    clients: &Clients,
    sink: &dyn ProgressSink,
    appids: &[u32],
    known: &mut BTreeMap<u32, Option<String>>,
    pages: &mut BTreeMap<u32, Option<String>>,
) -> Result<usize> {
    let ask: Vec<u32> = appids.iter().copied().filter(|a| !known.contains_key(a) || !pages.contains_key(a)).collect();
    if ask.is_empty() {
        return Ok(0);
    }
    sink.log(format!("GOG ids from Wikidata for {}", crate::enrich::plural(ask.len(), "game", "games")));
    let mut progress = Progress { total: ask.len(), done: 0, current: None, message: "GOG ids from Wikidata".into() };
    let (mut found_now, mut page_only, mut ambiguous, mut failed) = (0, 0, 0, 0);
    for chunk in ask.chunks(150) {
        cancelled(sink)?;
        let found = match clients.wikidata.gog_ids_by_steam_appids(chunk).await {
            Ok(found) => found,
            Err(e) => {
                sink.log(format!("GOG ids from Wikidata failed: {e:#}"));
                failed = 1;
                break;
            }
        };
        for a in chunk {
            // An id found in GOG's catalogue on an earlier run is kept.
            match found.ids.get(a) {
                Some(id) => _ = known.insert(*a, Some(id.clone())),
                None => _ = known.entry(*a).or_insert(None),
            }
            pages.insert(*a, found.pages.get(a).cloned());
        }
        (found_now, page_only, ambiguous) = (found_now + found.ids.len(), page_only + found.page_only, ambiguous + found.ambiguous);
        write_file(&gog_ids_file(cfg), &serde_json::to_vec(&known)?)?;
        write_file(&gog_pages_file(cfg), &serde_json::to_vec(&pages)?)?;
        progress.done += chunk.len();
        sink.update(&progress);
    }
    sink.log(format!(
        "GOG ids from Wikidata: {found_now} of {} asked {} one; {page_only} more are on GOG with no product id there, and {ambiguous} with two were left out",
        crate::enrich::plural(progress.done, "game", "games"),
        if found_now == 1 { "has" } else { "have" }
    ));
    Ok(failed)
}

/// Looks up in GOG's catalogue the store page of every game Wikidata gives
/// a page and no product id, one search per slug not asked before, and
/// gives each such game its page's id. An id that would go to two games
/// (two Steam apps sharing a store page) is left out rather than guessed,
/// since the shared data finds a GOG copy's entry by it. A search GOG
/// refuses as a request (HTTP 400 for "breach and clear", every time) is a
/// miss for that slug, logged and kept like one not in the catalogue, and
/// the run carries on; 1 when a lookup failed otherwise (no answer, too
/// many requests, any other HTTP error, or GOG's own trouble), which stops
/// the lookups and leaves that slug to ask again next run.
async fn catalogue_gog_ids(
    cfg: &Config,
    clients: &Clients,
    sink: &dyn ProgressSink,
    known: &mut BTreeMap<u32, Option<String>>,
    pages: &BTreeMap<u32, Option<String>>,
) -> Result<usize> {
    let path = gog_slug_ids_file(cfg);
    let mut by_slug: BTreeMap<String, Option<String>> = read_json(&path)?;
    let wanted = |known: &BTreeMap<u32, Option<String>>| -> Vec<(u32, String)> {
        known.iter().filter(|(_, id)| id.is_none()).filter_map(|(a, _)| Some((*a, pages.get(a)?.clone()?))).collect()
    };
    let ask: BTreeSet<String> = wanted(known).into_iter().map(|(_, slug)| slug).filter(|s| !by_slug.contains_key(s)).collect();
    let mut failed = 0;
    if !ask.is_empty() {
        sink.log(format!("GOG ids from GOG's catalogue for {}", crate::enrich::plural(ask.len(), "store page", "store pages")));
        let mut progress = Progress { total: ask.len(), done: 0, current: None, message: "GOG ids from GOG's catalogue".into() };
        let (mut found, mut unlisted, mut refused) = (0, 0, 0);
        for slug in &ask {
            // Stopped: what was found so far is kept, as after a failure.
            if sink.cancelled() {
                write_file(&path, &serde_json::to_vec(&by_slug)?)?;
                bail!("cancelled");
            }
            match clients.gogmeta.id_by_slug(slug).await {
                Ok(id) => {
                    if id.is_some() {
                        found += 1
                    } else {
                        unlisted += 1
                    }
                    by_slug.insert(slug.clone(), id);
                }
                Err(e) if refused_search(&e) => {
                    sink.log(format!("GOG's catalogue refused the search for {slug}, which is left without an id: {e:#}"));
                    refused += 1;
                    by_slug.insert(slug.clone(), None);
                }
                Err(e) => {
                    sink.log(format!("GOG ids from GOG's catalogue failed: {e:#}"));
                    failed = 1;
                    break;
                }
            }
            progress.done += 1;
            // Kept every so often, so a stopped run asks little again.
            if progress.done.is_multiple_of(50) {
                write_file(&path, &serde_json::to_vec(&by_slug)?)?;
            }
            sink.update(&progress);
        }
        write_file(&path, &serde_json::to_vec(&by_slug)?)?;
        sink.log(format!(
            "GOG ids from GOG's catalogue: {found} of {} found; {unlisted} not in the catalogue (off sale, renamed, or sold only in an edition){}",
            crate::enrich::plural(progress.done, "store page", "store pages"),
            if refused > 0 { format!("; {} by GOG", crate::enrich::plural(refused, "search refused", "searches refused")) } else { String::new() }
        ));
    }
    let found: Vec<(u32, String)> = wanted(known).into_iter().filter_map(|(a, slug)| Some((a, by_slug.get(&slug)?.clone()?))).collect();
    let mut games_per_id: HashMap<String, usize> = HashMap::new();
    for id in known.values().flatten().chain(found.iter().map(|(_, id)| id)) {
        *games_per_id.entry(id.clone()).or_default() += 1;
    }
    let (fill, shared): (Vec<_>, Vec<_>) = found.into_iter().partition(|(_, id)| games_per_id[id] == 1);
    if !shared.is_empty() {
        sink.log(format!(
            "{} left without a GOG id: their store page's id is another game's too",
            crate::enrich::plural(shared.len(), "game", "games")
        ));
    }
    if !fill.is_empty() {
        for (a, id) in fill {
            known.insert(a, Some(id));
        }
        write_file(&gog_ids_file(cfg), &serde_json::to_vec(&known)?)?;
    }
    Ok(failed)
}

/// A search GOG refused as a request, 400 or 422: about this one search.
/// Any other client error (401, 404, 410: the endpoint moved or wants a
/// sign-in now) stops the lookups like a server's, rather than marking
/// every slug left as a miss for good.
fn refused_search(e: &anyhow::Error) -> bool {
    e.downcast_ref::<crate::http::HttpStatusError>().is_some_and(|h| matches!(h.status.as_u16(), 400 | 422))
}

// -- the summaries ------------------------------------------------------------------

/// Whether a title's People say or Vibes copies a run of
/// [`COPIED_WORDS`] words from one of its reviews.
fn copies(g: &GameView) -> bool {
    let reviews = || g.review_texts.iter().map(|r| r.text.as_str());
    [g.digest.as_deref(), g.vibes.as_deref()].into_iter().flatten().any(|t| longest_copied(t, reviews()) >= COPIED_WORDS)
}

/// Games with reviews to summarize and no summary yet.
fn unwritten(db: &Db) -> Result<usize> {
    Ok(games(db)?.iter().filter(|g| crate::digest::wanted(g) && crate::digest::outdated(g)).count())
}

fn asked_again_file(cfg: &Config) -> PathBuf {
    work_dir(cfg).join("asked-again.json")
}

/// What [`generate`] did.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Generated {
    /// Summaries written by the summaries pass.
    pub written: usize,
    /// Summaries that copied a review, written again in the model's own words.
    pub rewritten: usize,
    /// Games with reviews still without a summary.
    pub unwritten: usize,
    /// Summaries that still copy a review, which the export leaves out.
    pub copying: usize,
}

/// Write the build's summaries on the AI the build database's settings name
/// (the summaries' backend, its server and model, and whether the model thinks first),
/// as the summaries pass does for a library: a second pass picks up the
/// titles a reply left out. Then the summaries that copied a review are
/// written once more with [`OWN_WORDS`]. `limit` writes at most that many
/// a pass, a night's share on a model with a usage limit.
pub async fn generate(
    db: &Db,
    cfg: &Config,
    clients: &Clients,
    llm: &dyn LlmBackend,
    limit: Option<usize>,
    sink: &dyn ProgressSink,
) -> Result<Generated> {
    let mut out = Generated::default();
    let mut opts = only(&["digests"]);
    opts.limit = limit;
    for pass in 0..2 {
        if pass > 0 {
            let left = unwritten(db)?;
            if left == 0 || limit.is_some() {
                break;
            }
            sink.log(format!("Again for the {} the first pass left out", crate::enrich::plural(left, "title", "titles")));
        }
        // `llm` is the summaries' backend, and only summaries run here.
        let s = enrich::run_enrich_with(db, cfg, clients, None, Some(llm), &opts, sink).await?;
        out.written += s.count("digests");
    }
    out.rewritten = rewrite_copies(db, cfg, llm, sink).await?;
    out.unwritten = unwritten(db)?;
    out.copying = games(db)?.iter().filter(|g| copies(g)).count();
    Ok(out)
}

/// Ask once more, in the model's own words, for the summaries that copy a
/// review; a title is asked again only once (the work folder keeps which).
/// How many were written again.
async fn rewrite_copies(db: &Db, cfg: &Config, llm: &dyn LlmBackend, sink: &dyn ProgressSink) -> Result<usize> {
    use futures::StreamExt;
    let path = asked_again_file(cfg);
    let mut asked: BTreeSet<i64> = read_json(&path)?;
    let copying: Vec<GameView> = games(db)?.into_iter().filter(|g| !asked.contains(&g.id) && copies(g)).collect();
    if copying.is_empty() {
        return Ok(0);
    }
    sink.log(format!(
        "{} {COPIED_WORDS} or more words from a review: asking again, in the model's own words",
        crate::enrich::plural(copying.len(), "summary copies", "summaries copy")
    ));
    let model = cfg.summary_model();
    let chunks = crate::digest::batches(&copying);
    let mut calls = futures::stream::iter(chunks.iter().map(|chunk| {
        let model = model.clone();
        async move { (*chunk, crate::digest::generate_with(db, llm, model, chunk, Some(OWN_WORDS)).await) }
    }))
    .buffer_unordered(enrich::sources::AI_PARALLEL);
    let mut progress = Progress { total: copying.len(), done: 0, current: None, message: "Summaries in the model's own words".into() };
    sink.update(&progress);
    let mut rewritten = 0;
    while let Some((chunk, result)) = calls.next().await {
        match result {
            Ok(w) => {
                rewritten += w.digests;
                asked.extend(chunk.iter().map(|g| g.id));
                write_file(&path, &serde_json::to_vec(&asked)?)?;
            }
            Err(e) => {
                sink.log(format!("Writing again failed for {}: {e:#}", crate::enrich::plural(chunk.len(), "title", "titles")));
                if crate::llm::gave_up(&e).is_some() {
                    break;
                }
            }
        }
        progress.done += chunk.len();
        sink.update(&progress);
        cancelled(sink)?;
    }
    Ok(rewritten)
}

// -- the export ---------------------------------------------------------------------

/// What [`export`] wrote.
#[derive(Debug, Clone)]
pub struct Exported {
    pub manifest: Manifest,
    pub data_file: PathBuf,
    pub manifest_file: PathBuf,
    /// Games without a summary, left out.
    pub unwritten: usize,
    /// Games whose summary copies a review, left out.
    pub copying: usize,
    /// Entries with a GOG id; None when `seed fetch` hasn't looked them up,
    /// so the export has none and GOG copies won't match it.
    pub gog_ids: Option<usize>,
}

/// The entries of the build: every game with its own summary that copies no
/// review, by appid. Only the fields the format allows exist in an
/// [`Entry`]: no review, description or setting can reach the file.
fn entries(db: &Db, cfg: &Config) -> Result<(Vec<Entry>, usize, usize)> {
    let gog: BTreeMap<u32, Option<String>> = read_json(&gog_ids_file(cfg))?;
    let written = db.own_digests_written()?;
    let (mut out, mut unwritten, mut copying) = (vec![], 0, 0);
    for g in games(db)? {
        let (Some(appid), Some(people_say), Some((model, at))) =
            (g.appid, g.digest.as_deref().map(str::trim).filter(|d| !d.is_empty()), written.get(&g.id))
        else {
            unwritten += 1;
            continue;
        };
        if copies(&g) {
            copying += 1;
            continue;
        }
        out.push(Entry {
            steam_appid: Some(appid),
            gog_id: gog.get(&appid).cloned().flatten(),
            name: g.name.clone(),
            people_say: people_say.to_string(),
            vibes: g.vibes.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string),
            tags: g.review_tags.clone().unwrap_or_default(),
            prompt_version: g.digest_version.unwrap_or(1),
            model: if model.is_empty() { "unknown".into() } else { model.clone() },
            written: ymd(*at).ok_or_else(|| anyhow!("{}'s summary has no date", g.name))?,
            reviews_until: g.review_texts.iter().filter_map(|r| r.posted).max().and_then(ymd),
        });
    }
    out.sort_by_key(|e| e.steam_appid);
    Ok((out, unwritten, copying))
}

/// Write the release assets into `out`: `upnext-seed-<date>.jsonl.gz` and
/// its manifest, `upnext-seed.json`. The data is read back with the apply
/// side's own checks before anything is written.
pub fn export(db: &Db, cfg: &Config, out: &Path, date: &str) -> Result<Exported> {
    check(db, cfg, false)?;
    if !is_date(date) {
        bail!("{date:?} isn't a date (YYYY-MM-DD)");
    }
    let (entries, unwritten, copying) = entries(db, cfg)?;
    if entries.is_empty() {
        bail!("there are no summaries to export yet: `upnext seed generate` writes them");
    }
    let gog_ids = gog_ids_file(cfg).exists().then(|| entries.iter().filter(|e| e.gog_id.is_some()).count());
    let data = write(date, &entries)?;
    let seed = read(&data).map_err(|e| anyhow!("the export doesn't read back as shared data: {e}"))?;
    let manifest = manifest(&seed.header, &data);
    let data_file = out.join(&manifest.file);
    let manifest_file = out.join("upnext-seed.json");
    write_file(&data_file, &data)?;
    let mut json = serde_json::to_vec_pretty(&manifest)?;
    json.push(b'\n');
    write_file(&manifest_file, &json)?;
    Ok(Exported { manifest, data_file, manifest_file, unwritten, copying, gog_ids })
}

// -- where it stands ----------------------------------------------------------------

/// Where a build stands, for `upnext seed status`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct BuildStatus {
    /// The day the build started (YYYY-MM-DD).
    pub started: String,
    /// Games in the build.
    pub games: usize,
    /// Games whose reviews are fetched.
    pub reviews: usize,
    /// Games with reviews a summary can be written from.
    pub summarizable: usize,
    /// Games with a summary.
    pub summaries: usize,
    /// Summaries that copy a review, which the export leaves out.
    pub copying: usize,
    /// Games with a GOG id.
    pub gog_ids: usize,
}

/// Where the build stands; None when the database isn't a build.
pub fn status(db: &Db, cfg: &Config) -> Result<Option<BuildStatus>> {
    let Some(started) = db.meta_get(MARK)? else { return Ok(None) };
    let games = games(db)?;
    let gog: BTreeMap<u32, Option<String>> = read_json(&gog_ids_file(cfg))?;
    Ok(Some(BuildStatus {
        started,
        games: games.len(),
        reviews: games.iter().filter(|g| g.has_review_texts).count(),
        summarizable: games.iter().filter(|g| crate::digest::wanted(g)).count(),
        summaries: games.iter().filter(|g| g.has_digest).count(),
        copying: games.iter().filter(|g| copies(g)).count(),
        gog_ids: games.iter().filter_map(|g| g.appid).filter(|a| gog.get(a).is_some_and(Option::is_some)).count(),
    }))
}

#[cfg(test)]
#[path = "build_tests.rs"]
mod tests;
