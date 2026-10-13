//! The recommended wishlist: games you don't own,
//! suggested from the series you've played (Collections' missing entries)
//! or read from your Steam wishlist, with the store's data fetched for them
//! the way enrich fetches it for the library. Never a game you own, one
//! Steam's store has no page for, one you said Not interested to, or a hard
//! no. The table is `db::wishlist`, the job the `wishlist` pass
//! (`enrich::sources::wishlist`), the API `server::wishlist`. The
//! `wishlist` setting switches all of it.

use crate::db::PlayerId;
use crate::db::{Db, GameView, NewWish, Status, StoreFacts, WishRow, WishSource, WishState, edition_of, loose_key};
use crate::enrich::Clients;
use crate::series::Collection;
use crate::steam::WishlistItem;
use crate::taste::Profile;
use crate::wikidata::SeriesEntry;
use anyhow::Result;
use serde::Serialize;
use std::collections::HashMap;

/// Hours played in a series that make it mean something to you.
pub const SERIES_HOURS: i64 = 10;
/// A good rating for a suggestion: Steam's "Very Positive" starts at 80%,
/// from at least this many reviews (very few reviews aren't trusted).
pub const GOOD_PCT: i64 = 80;
pub const GOOD_REVIEWS: i64 = 50;
/// Suggestions waiting for an answer, at most, from one series and in all.
pub const PER_SERIES: usize = 3;
pub const MAX_SUGGESTED: usize = 30;

/// The library title a game you don't own would be: the same Steam appid,
/// or the same name on any copy, editions included ("Dishonored -
/// Definitive Edition" is the Dishonored you have, and "The Witcher 3:
/// Complete Edition" the "The Witcher 3: Wild Hunt - Complete Edition").
pub fn owned_as<'a>(games: &'a [GameView], appid: Option<u32>, name: &str) -> Option<&'a GameView> {
    if let Some(a) = appid
        && let Some(g) = games.iter().find(|g| g.appid == Some(a))
    {
        return Some(g);
    }
    let loose = loose_key(name);
    if loose.is_empty() {
        return None;
    }
    let same = |n: &str| loose_key(n) == loose;
    if let Some(g) = games.iter().find(|g| same(&g.name) || g.copies.iter().any(|c| same(&c.name))) {
        return Some(g);
    }
    edition_of(name)?;
    let prefix = format!("{loose} ");
    games.iter().find(|g| loose_key(&g.name).starts_with(&prefix))
}

/// What a series means to you: the games you played in it, how long, and
/// which you beat or call favourites.
#[derive(Debug)]
pub struct History<'a> {
    /// Members you've played, oldest first.
    pub played: Vec<&'a GameView>,
    pub minutes: i64,
    pub beaten: Vec<&'a GameView>,
    pub favourites: Vec<&'a GameView>,
}

impl History<'_> {
    /// At least `SERIES_HOURS` played, or an entry beaten or a favourite.
    pub fn means_something(&self) -> bool {
        self.minutes >= SERIES_HOURS * 60 || !self.beaten.is_empty() || !self.favourites.is_empty()
    }
}

pub fn history<'a>(c: &'a Collection, profile: Option<&Profile>) -> History<'a> {
    let year = |g: &GameView| c.years.get(&g.id).copied().unwrap_or(9999);
    let mut played: Vec<&GameView> = c.members.iter().filter(|g| g.playtime_minutes > 0).collect();
    played.sort_by_key(|g| year(g));
    let mut beaten: Vec<&GameView> = c.members.iter().filter(|g| g.status == Status::Done).collect();
    beaten.sort_by_key(|g| year(g));
    let favourites = c.members.iter().filter(|g| profile.is_some_and(|p| p.is_favourite(g.id))).collect();
    History { played, minutes: c.members.iter().map(|g| g.playtime_minutes).sum(), beaten, favourites }
}

/// "Half-Life", "Half-Life and Half-Life 2", "Half-Life, Half-Life 2 and 2 more".
fn names(list: &[&GameView]) -> String {
    match list {
        [] => String::new(),
        [a] => a.name.clone(),
        [a, b] => format!("{} and {}", a.name, b.name),
        [a, b, rest @ ..] => format!("{}, {} and {} more", a.name, b.name, rest.len()),
    }
}

fn hours_text(minutes: i64) -> String {
    match minutes {
        m if m < 60 => "under an hour".into(),
        m if m < 90 => "an hour".into(),
        m => format!("{} hours", (m + 30) / 60),
    }
}

/// Why a missing entry is suggested, from your history with its series:
/// "You played Half-Life and Half-Life 2 for 40 hours, and beat Half-Life
/// 2. This one came after them, in 2007."
pub fn reason(c: &Collection, h: &History<'_>, entry: &SeriesEntry) -> String {
    let mut out = if !h.played.is_empty() {
        let mut s = format!("You played {} for {}", names(&h.played), hours_text(h.minutes));
        if !h.beaten.is_empty() {
            s.push_str(&format!(", and beat {}", names(&h.beaten)));
        }
        s.push('.');
        s
    } else if !h.beaten.is_empty() {
        format!("You beat {}.", names(&h.beaten))
    } else {
        format!("You own {} of the {} series.", c.members.len(), c.name)
    };
    match h.favourites.as_slice() {
        [] => {}
        [f] => out.push_str(&format!(" {} is one of your favourites.", f.name)),
        fs => out.push_str(&format!(" {} are among your favourites.", names(fs))),
    }
    let mine: Vec<&GameView> = if h.played.is_empty() { h.beaten.clone() } else { h.played.clone() };
    let years: Vec<i32> = mine.iter().filter_map(|g| c.years.get(&g.id).copied()).collect();
    let them = if mine.len() == 1 { "it" } else { "them" };
    match (entry.year, years.iter().min(), years.iter().max()) {
        (Some(y), Some(_), Some(&hi)) if y > hi => out.push_str(&format!(" This one came after {them}, in {y}.")),
        (Some(y), Some(&lo), Some(_)) if y < lo => out.push_str(&format!(" This one came before {them}, in {y}.")),
        (Some(y), _, _) => out.push_str(&format!(" This one is from {y}.")),
        _ => out.push_str(&format!(" It's another {} game.", c.name)),
    }
    out
}

/// A missing entry of a series that means something to you, not owned and
/// not on the wishlist table in any state, that could be suggested.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub appid: u32,
    pub name: String,
    pub series_key: String,
    pub reason: String,
    /// Minutes played in its series: the most-played series go first.
    pub minutes: i64,
    pub year: Option<i32>,
}

/// Every candidate from the collections, most-played series first, then
/// oldest first within a series. Entries without a Steam appid on Wikidata
/// are left out: they can't be checked against the store or rated.
pub fn candidates(collections: &[Collection], games: &[GameView], profile: Option<&Profile>, rows: &[WishRow]) -> Vec<Candidate> {
    let known: std::collections::HashSet<u32> = rows.iter().map(|r| r.appid).collect();
    let mut out: Vec<Candidate> = vec![];
    for c in collections {
        let h = history(c, profile);
        if !h.means_something() {
            continue;
        }
        for e in &c.missing {
            let Some(appid) = e.steam_appid else { continue };
            if known.contains(&appid) || out.iter().any(|x| x.appid == appid) || owned_as(games, Some(appid), &e.name).is_some() {
                continue;
            }
            out.push(Candidate {
                appid,
                name: e.name.clone(),
                series_key: c.key.clone(),
                reason: reason(c, &h, e),
                minutes: h.minutes,
                year: e.year,
            });
        }
    }
    out.sort_by(|a, b| {
        b.minutes.cmp(&a.minutes).then(a.series_key.cmp(&b.series_key)).then(a.year.unwrap_or(9999).cmp(&b.year.unwrap_or(9999)))
    });
    out
}

/// Whether the store data lets a game be suggested: a game on Steam's store,
/// rated well by enough players, and not a hard no. Err says why not.
pub fn judge(f: &StoreFacts, profile: Option<&Profile>) -> std::result::Result<(), String> {
    if f.details_checked && f.type_.is_none() {
        return Err("Steam's store has no page for it".into());
    }
    if let Some(t) = f.type_.as_deref().filter(|t| *t != "game") {
        return Err(format!("Steam lists it as {t}, not a game"));
    }
    match (f.review_pct, f.review_total) {
        (Some(p), Some(n)) if p >= GOOD_PCT && n >= GOOD_REVIEWS => {}
        (Some(p), Some(n)) => {
            return Err(format!(
                "{p}% of {} {} positive; a suggestion needs {GOOD_PCT}% of {GOOD_REVIEWS} or more",
                crate::enrich::plural(n, "review", "reviews"),
                if n == 1 { "is" } else { "are" }
            ));
        }
        _ => return Err("it has no Steam reviews to go by".into()),
    }
    if let Some(no) = profile.and_then(|p| p.hard_no_in_store(&f.tags, &f.genres)) {
        return Err(format!("it's {}, a hard no", no.subject.label()));
    }
    Ok(())
}

/// What a sync found: games on the wishlist table that are in the library
/// now. Each leaves the wishlist; the game page says "From your wishlist"
/// when it was on it. Returns the names.
pub fn settle(db: &Db, p: PlayerId, games: &[GameView]) -> Result<Vec<String>> {
    let mut found = vec![];
    for r in db.wishlist_rows(p)?.into_iter().filter(|r| r.state != WishState::InLibrary) {
        if let Some(g) = owned_as(games, Some(r.appid), &r.name) {
            db.wishlist_found_in_library(r.id, g.id)?;
            if r.state == WishState::Wishlisted {
                found.push(g.name.clone());
            }
        }
    }
    Ok(found)
}

/// Whether player `p` has the wishlist on: player 1's is `cfg`'s, another
/// player's their own setting, on unless they turned it off.
pub fn on_for(db: &Db, cfg: &crate::config::Config, p: PlayerId) -> Result<bool> {
    if p == PlayerId::FIRST {
        return Ok(cfg.wishlist);
    }
    Ok(db.setting_get(p, "wishlist")?.is_none_or(|v| !matches!(v.trim().to_ascii_lowercase().as_str(), "false" | "0" | "no" | "off")))
}

/// What reading the Steam wishlist changed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SteamChanges {
    /// On it and new to the table.
    pub added: usize,
    /// Already on the table (a suggestion), now on the Steam wishlist too.
    pub joined: usize,
    /// Off the Steam wishlist since the last read.
    pub left: usize,
}

/// Mirrors the Steam wishlist into the table, read-only: a game on it is on
/// the wishlist (a suggestion of the same game becomes it, so it's never
/// suggested), and one taken off it that only came from Steam is dropped
/// as Not interested. Games you own are left out. `names` are the store's
/// names for appids new to the table. A Steam account's wishlist is its
/// owner's, `p`, and `games` their library.
pub fn mirror_steam(
    db: &Db,
    p: PlayerId,
    games: &[GameView],
    items: &[WishlistItem],
    names: &HashMap<u32, String>,
) -> Result<SteamChanges> {
    let rows = db.wishlist_rows(p)?;
    let by_appid: HashMap<u32, &WishRow> = rows.iter().map(|r| (r.appid, r)).collect();
    let mut out = SteamChanges::default();
    for item in items {
        match by_appid.get(&item.appid) {
            Some(r) => {
                if !r.on_steam {
                    db.wishlist_set_on_steam(r.id, true)?;
                    if matches!(r.state, WishState::Suggested | WishState::Dismissed) {
                        db.wishlist_set_state(r.id, WishState::Wishlisted)?;
                        out.joined += 1;
                    }
                }
            }
            None => {
                let name = names.get(&item.appid).cloned().unwrap_or_else(|| format!("App {}", item.appid));
                if owned_as(games, Some(item.appid), &name).is_some() {
                    continue;
                }
                db.wishlist_add(
                    p,
                    &NewWish {
                        appid: item.appid,
                        name,
                        source: WishSource::Steam,
                        reason: None,
                        state: WishState::Wishlisted,
                        on_steam: true,
                        series_key: None,
                        batch_id: None,
                    },
                )?;
                out.added += 1;
            }
        }
    }
    for r in rows.iter().filter(|r| r.on_steam && !items.iter().any(|i| i.appid == r.appid)) {
        db.wishlist_set_on_steam(r.id, false)?;
        if r.source == WishSource::Steam && r.state == WishState::Wishlisted {
            db.wishlist_set_state(r.id, WishState::Dismissed)?;
        }
        out.left += 1;
    }
    Ok(out)
}

/// Fetches what the store tables lack for one game you don't own: the
/// review summary, store details and Steam's tags (SteamSpy), in that
/// order, stopping after the reviews when `rated_only` and they don't make
/// a suggestion. Art is fetched in batches (`fetch_art`). Returns the facts
/// as stored after.
pub async fn fetch_facts(db: &Db, p: PlayerId, clients: &Clients, appid: u32, rated_only: bool) -> Result<StoreFacts> {
    let have = db.store_facts(&[appid])?.remove(&appid).unwrap_or_default();
    if !have.reviews_checked {
        let r = clients.steam.review_summary(appid).await?;
        db.put_reviews(appid, r.as_ref())?;
    }
    if rated_only {
        let now = db.store_facts(&[appid])?.remove(&appid).unwrap_or_default();
        if !matches!((now.review_pct, now.review_total), (Some(p), Some(n)) if p >= GOOD_PCT && n >= GOOD_REVIEWS) {
            return Ok(now);
        }
    }
    if !have.details_checked {
        let d = clients.steam.app_details(appid).await?;
        db.put_details(appid, d.as_ref())?;
        if let Some(name) = d.and_then(|d| d.name)
            && let Some(r) = db.wishlist_by_appid(p, appid)?
            && r.name == format!("App {appid}")
        {
            db.wishlist_set_name(r.id, &name)?;
        }
    }
    if !have.tags_checked {
        let t = clients.spy.tags(appid).await?;
        db.put_tags(appid, t.as_deref())?;
    }
    Ok(db.store_facts(&[appid])?.remove(&appid).unwrap_or_default())
}

/// Cover art and screenshots for games you don't own, 100 per request.
/// An appid the store has no art for is stored empty, so it isn't asked again.
pub async fn fetch_art(db: &Db, clients: &Clients, appids: &[u32]) -> Result<usize> {
    let mut found = 0;
    for chunk in appids.chunks(100) {
        let art = crate::steam::store_art(&clients.steam, chunk).await?;
        for a in chunk {
            match art.get(a) {
                Some((portrait, hero, shots)) => {
                    db.put_steam_art(*a, portrait.as_deref(), hero.as_deref(), shots)?;
                    found += 1;
                }
                None => db.put_steam_art(*a, None, None, &[])?,
            }
        }
    }
    Ok(found)
}

/// One game on the wishlist as the API shows it: the row and what the store says.
#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    #[serde(flatten)]
    pub row: WishRow,
    #[serde(flatten)]
    pub facts: StoreFacts,
    /// The series it's from, by name.
    pub series: Option<String>,
}

/// `GET /api/wishlist`: suggestions waiting for an answer, best first, and
/// the wishlist itself (yours and Steam's), newest first.
#[derive(Debug, Clone, Serialize)]
pub struct Lists {
    pub suggested: Vec<Entry>,
    pub wishlist: Vec<Entry>,
    /// Said Not interested to, or owned elsewhere: never suggested again.
    pub dismissed: usize,
    pub owned_elsewhere: usize,
    /// Left the wishlist when a sync found them in the library.
    pub in_library: usize,
}

pub fn lists(db: &Db, p: PlayerId, collections: &[Collection]) -> Result<Lists> {
    let rows = db.wishlist_rows(p)?;
    let shown: Vec<u32> =
        rows.iter().filter(|r| matches!(r.state, WishState::Suggested | WishState::Wishlisted)).map(|r| r.appid).collect();
    let facts = db.store_facts(&shown)?;
    let series_name: HashMap<&str, &str> = collections.iter().map(|c| (c.key.as_str(), c.name.as_str())).collect();
    let entry = |r: &WishRow| Entry {
        row: r.clone(),
        facts: facts.get(&r.appid).cloned().unwrap_or_default(),
        series: r.series_key.as_deref().and_then(|k| series_name.get(k)).map(|n| n.to_string()),
    };
    let count = |s: WishState| rows.iter().filter(|r| r.state == s).count();
    let mut suggested: Vec<Entry> = rows.iter().filter(|r| r.state == WishState::Suggested).map(entry).collect();
    suggested.sort_by(|a, b| b.facts.review_pct.cmp(&a.facts.review_pct).then(a.row.id.cmp(&b.row.id)));
    let mut wishlist: Vec<Entry> = rows.iter().filter(|r| r.state == WishState::Wishlisted).map(entry).collect();
    wishlist.sort_by(|a, b| b.row.wishlisted_at.cmp(&a.row.wishlisted_at).then(b.row.id.cmp(&a.row.id)));
    Ok(Lists {
        suggested,
        wishlist,
        dismissed: count(WishState::Dismissed),
        owned_elsewhere: count(WishState::OwnedElsewhere),
        in_library: count(WishState::InLibrary),
    })
}

/// Why a game couldn't be added by name.
#[derive(Debug, thiserror::Error)]
pub enum AddError {
    /// No name, no such game on the store, or one you own.
    #[error("{0}")]
    Bad(String),
    /// Steam's store didn't answer the one quick try.
    #[error("Steam's store didn't answer ({0:#}), so {1:?} couldn't be looked up; try again in a minute")]
    Unreachable(anyhow::Error, String),
    #[error("{0:#}")]
    Other(#[from] anyhow::Error),
}

/// A game added by name (`POST /api/wishlist` with `name`): found on
/// Steam's store, checked against the library and put on the wishlist,
/// with its store data fetched (not HowLongToBeat; the job does that).
/// Someone is waiting, so each request is tried once (`http::quick`): when
/// Steam doesn't answer, they hear so at once, and the background job
/// keeps the long backoff.
pub async fn add_by_name(db: &Db, p: PlayerId, clients: &Clients, games: &[GameView], name: &str) -> std::result::Result<i64, AddError> {
    crate::http::quick(add_by_name_now(db, p, clients, games, name)).await
}

async fn add_by_name_now(db: &Db, p: PlayerId, clients: &Clients, games: &[GameView], name: &str) -> std::result::Result<i64, AddError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AddError::Bad("name a game to add".into()));
    }
    let found = crate::steam::store_search(&clients.steam, name).await.map_err(|e| AddError::Unreachable(e, name.to_string()))?;
    let Some((appid, found, _)) = found else {
        return Err(AddError::Bad(format!("Steam's store has no game called {name:?}")));
    };
    if let Some(g) = owned_as(games, Some(appid), &found) {
        return Err(AddError::Bad(format!("{found} is in your library already, as {}", g.name)));
    }
    let id = match db.wishlist_by_appid(p, appid)? {
        Some(r) => {
            db.wishlist_set_state(r.id, WishState::Wishlisted)?;
            r.id
        }
        None => {
            let new = NewWish {
                appid,
                name: found,
                source: WishSource::Manual,
                reason: None,
                state: WishState::Wishlisted,
                on_steam: false,
                series_key: None,
                batch_id: None,
            };
            db.wishlist_add(p, &new)?.0
        }
    };
    // The page shows what's there; the next job fills in the rest.
    if let Err(e) = fetch_facts(db, p, clients, appid, false).await {
        tracing::warn!("store data for {appid} on the wishlist: {e:#}");
    }
    if let Err(e) = fetch_art(db, clients, &[appid]).await {
        tracing::warn!("art for {appid} on the wishlist: {e:#}");
    }
    Ok(id)
}

pub mod picks;

#[cfg(test)]
mod tests;
