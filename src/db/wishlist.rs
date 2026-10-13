//! The recommended wishlist: games you don't own, keyed by Steam
//! appid, and what the store says about them. The rules for what goes in
//! are `crate::wishlist`'s.

use super::{Db, PlayerId, now};
use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Where a game on the wishlist came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WishSource {
    /// A missing entry of a series you've played (Collections).
    Collection,
    /// Suggested with a lineup.
    Pick,
    /// Your Steam wishlist.
    Steam,
    /// Added by name.
    Manual,
}

/// Where a game stands on the wishlist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WishState {
    Suggested,
    Wishlisted,
    /// Not interested: never suggested again.
    Dismissed,
    /// I own it elsewhere: counts as owned from then on.
    OwnedElsewhere,
    /// A sync found it in the library; it left the wishlist.
    InLibrary,
}

impl WishSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Collection => "collection",
            Self::Pick => "pick",
            Self::Steam => "steam",
            Self::Manual => "manual",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "collection" => Self::Collection,
            "pick" => Self::Pick,
            "steam" => Self::Steam,
            _ => Self::Manual,
        }
    }
}

impl WishState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Suggested => "suggested",
            Self::Wishlisted => "wishlisted",
            Self::Dismissed => "dismissed",
            Self::OwnedElsewhere => "owned_elsewhere",
            Self::InLibrary => "in_library",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Suggested, Self::Wishlisted, Self::Dismissed, Self::OwnedElsewhere, Self::InLibrary].into_iter().find(|x| x.as_str() == s)
    }
}

/// One row of the wishlist table.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WishRow {
    pub id: i64,
    pub appid: u32,
    pub name: String,
    pub source: WishSource,
    pub reason: Option<String>,
    pub state: WishState,
    /// On your Steam wishlist now.
    pub on_steam: bool,
    pub series_key: Option<String>,
    pub batch_id: Option<String>,
    /// The library title it became, once a sync found it there.
    pub title_id: Option<i64>,
    pub hltb_main: Option<f64>,
    #[serde(skip)]
    pub hltb_at: Option<i64>,
    pub added_at: i64,
    pub updated_at: i64,
    /// When it went on the wishlist (here or on Steam), if it ever did.
    pub wishlisted_at: Option<i64>,
}

/// A game to put on the wishlist table.
#[derive(Debug, Clone)]
pub struct NewWish {
    pub appid: u32,
    pub name: String,
    pub source: WishSource,
    pub reason: Option<String>,
    pub state: WishState,
    pub on_steam: bool,
    pub series_key: Option<String>,
    pub batch_id: Option<String>,
}

/// What the store says about a game you don't own, from the appid-keyed
/// tables enrich fills for the library.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct StoreFacts {
    /// Steam's app type ("game", "dlc"); None until details are fetched.
    #[serde(rename = "type")]
    pub type_: Option<String>,
    /// Store details were looked up (found or not).
    #[serde(skip)]
    pub details_checked: bool,
    pub short_description: Option<String>,
    pub genres: Vec<String>,
    pub developers: Vec<String>,
    pub release_date: Option<String>,
    pub header_image: Option<String>,
    /// The library capsule (2:3), when the store has one.
    pub portrait: Option<String>,
    #[serde(skip)]
    pub art_checked: bool,
    pub review_pct: Option<i64>,
    pub review_total: Option<i64>,
    pub review_desc: Option<String>,
    #[serde(skip)]
    pub reviews_checked: bool,
    /// Steam's tags, strongest first.
    pub tags: Vec<String>,
    #[serde(skip)]
    pub tags_checked: bool,
}

/// A game you don't own as picks show it: a wishlisted
/// pick, or the suggestion under a lineup. Its row and what the store says.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct WishGame {
    /// Its wishlist entry (`/api/wishlist/{id}`).
    pub id: i64,
    pub appid: u32,
    pub name: String,
    pub source: WishSource,
    /// Where it stands now: a suggestion answered since shows its answer.
    pub state: WishState,
    /// On your Steam wishlist.
    pub on_steam: bool,
    pub short_description: Option<String>,
    pub genres: Vec<String>,
    /// Steam's tags, strongest first (up to 8).
    pub tags: Vec<String>,
    pub release_date: Option<String>,
    pub header_image: Option<String>,
    /// The library capsule (2:3), when the store has one.
    pub portrait: Option<String>,
    pub review_pct: Option<i64>,
    pub review_total: Option<i64>,
    pub review_desc: Option<String>,
    /// HowLongToBeat's main story, in hours.
    pub hltb_main: Option<f64>,
}

impl WishGame {
    pub fn new(r: &WishRow, f: &StoreFacts) -> Self {
        WishGame {
            id: r.id,
            appid: r.appid,
            name: r.name.clone(),
            source: r.source,
            state: r.state,
            on_steam: r.on_steam,
            short_description: f.short_description.clone(),
            genres: f.genres.clone(),
            tags: f.tags.iter().take(8).cloned().collect(),
            release_date: f.release_date.clone(),
            header_image: f.header_image.clone(),
            portrait: f.portrait.clone(),
            review_pct: f.review_pct,
            review_total: f.review_total,
            review_desc: f.review_desc.clone(),
            hltb_main: r.hltb_main,
        }
    }

    /// A wishlisted pick's game after its wishlist entry was deleted (one
    /// row, or the whole wishlist from the profile): the pick's own name
    /// and appid, with what the store tables still hold, as it was picked.
    pub fn kept(id: i64, appid: u32, name: &str, f: &StoreFacts) -> Self {
        WishGame {
            id,
            appid,
            name: name.to_string(),
            source: WishSource::Manual,
            state: WishState::Wishlisted,
            on_steam: false,
            short_description: f.short_description.clone(),
            genres: f.genres.clone(),
            tags: f.tags.iter().take(8).cloned().collect(),
            release_date: f.release_date.clone(),
            header_image: f.header_image.clone(),
            portrait: f.portrait.clone(),
            review_pct: f.review_pct,
            review_total: f.review_total,
            review_desc: f.review_desc.clone(),
            hltb_main: None,
        }
    }
}

/// What a game page says about a game that came from the wishlist.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FromWishlist {
    pub reason: Option<String>,
    pub source: WishSource,
    pub wishlisted_at: i64,
}

const COLUMNS: &str = "id, appid, name, source, reason, state, on_steam, series_key, batch_id, title_id, hltb_main, hltb_at, added_at, updated_at, wishlisted_at";

fn row(r: &Row<'_>) -> rusqlite::Result<WishRow> {
    Ok(WishRow {
        id: r.get(0)?,
        appid: r.get::<_, i64>(1)? as u32,
        name: r.get(2)?,
        source: WishSource::parse(&r.get::<_, String>(3)?),
        reason: r.get(4)?,
        state: WishState::parse(&r.get::<_, String>(5)?).unwrap_or(WishState::Dismissed),
        on_steam: r.get::<_, i64>(6)? != 0,
        series_key: r.get(7)?,
        batch_id: r.get(8)?,
        title_id: r.get(9)?,
        hltb_main: r.get(10)?,
        hltb_at: r.get(11)?,
        added_at: r.get(12)?,
        updated_at: r.get(13)?,
        wishlisted_at: r.get(14)?,
    })
}

impl Db {
    /// Every row of player `p`'s, oldest first.
    pub fn wishlist_rows(&self, p: PlayerId) -> Result<Vec<WishRow>> {
        self.with(|c| {
            let mut stmt = c.prepare(&format!("SELECT {COLUMNS} FROM wishlist WHERE player_id=?1 ORDER BY id"))?;
            Ok(stmt.query_map(params![p], row)?.collect::<std::result::Result<_, _>>()?)
        })
    }

    /// One of player `p`'s rows.
    pub fn wishlist_row(&self, p: PlayerId, id: i64) -> Result<Option<WishRow>> {
        self.with(|c| {
            Ok(c.query_row(&format!("SELECT {COLUMNS} FROM wishlist WHERE id=?1 AND player_id=?2"), params![id, p], row).optional()?)
        })
    }

    pub fn wishlist_by_appid(&self, p: PlayerId, appid: u32) -> Result<Option<WishRow>> {
        self.with(|c| {
            Ok(c.query_row(&format!("SELECT {COLUMNS} FROM wishlist WHERE appid=?1 AND player_id=?2"), params![appid, p], row)
                .optional()?)
        })
    }

    /// Adds a game to player `p`'s wishlist; one already there (by appid) is
    /// left as it is. Returns its id and whether it was new.
    pub fn wishlist_add(&self, p: PlayerId, w: &NewWish) -> Result<(i64, bool)> {
        if let Some(r) = self.wishlist_by_appid(p, w.appid)? {
            return Ok((r.id, false));
        }
        self.with(|c| {
            let t = now();
            let wishlisted = (w.state == WishState::Wishlisted).then_some(t);
            c.execute(
                "INSERT INTO wishlist(appid, name, source, reason, state, on_steam, series_key, batch_id, added_at, updated_at, wishlisted_at,
                                      player_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?10, ?11)",
                params![
                    w.appid,
                    w.name,
                    w.source.as_str(),
                    w.reason,
                    w.state.as_str(),
                    w.on_steam,
                    w.series_key,
                    w.batch_id,
                    t,
                    wishlisted,
                    p
                ],
            )?;
            Ok((c.last_insert_rowid(), true))
        })
    }

    /// Moves a game to another state. Going on the wishlist stamps when.
    pub fn wishlist_set_state(&self, id: i64, state: WishState) -> Result<bool> {
        self.with(|c| {
            let n = c.execute(
                "UPDATE wishlist SET state=?2, updated_at=?3,
                    wishlisted_at = CASE WHEN ?2 = 'wishlisted' THEN COALESCE(wishlisted_at, ?3) ELSE wishlisted_at END
                 WHERE id=?1",
                params![id, state.as_str(), now()],
            )?;
            Ok(n > 0)
        })
    }

    /// On your Steam wishlist or not, as Steam said last.
    pub fn wishlist_set_on_steam(&self, id: i64, on: bool) -> Result<()> {
        self.with(|c| {
            c.execute("UPDATE wishlist SET on_steam=?2, updated_at=?3 WHERE id=?1 AND on_steam<>?2", params![id, on, now()])?;
            Ok(())
        })
    }

    /// A sync found it in the library: it leaves the wishlist for good.
    pub fn wishlist_found_in_library(&self, id: i64, title_id: i64) -> Result<()> {
        self.with(|c| {
            c.execute("UPDATE wishlist SET state='in_library', title_id=?2, updated_at=?3 WHERE id=?1", params![id, title_id, now()])?;
            Ok(())
        })
    }

    pub fn wishlist_set_hltb(&self, id: i64, main: Option<f64>) -> Result<()> {
        self.with(|c| {
            c.execute("UPDATE wishlist SET hltb_main=?2, hltb_at=?3 WHERE id=?1", params![id, main, now()])?;
            Ok(())
        })
    }

    /// The name the store gives it, when it had none better.
    pub fn wishlist_set_name(&self, id: i64, name: &str) -> Result<()> {
        self.with(|c| {
            c.execute("UPDATE wishlist SET name=?2 WHERE id=?1", params![id, name])?;
            Ok(())
        })
    }

    pub fn wishlist_delete(&self, id: i64) -> Result<bool> {
        self.with(|c| Ok(c.execute("DELETE FROM wishlist WHERE id=?1", params![id])? > 0))
    }

    /// Forgets the whole wishlist: suggestions, answers, the Steam wishlist's
    /// copy and where library games came from. Returns how many rows went.
    pub fn wishlist_wipe(&self, p: PlayerId) -> Result<usize> {
        self.with(|c| Ok(c.execute("DELETE FROM wishlist WHERE player_id=?1", params![p])?))
    }

    /// A library title that was on player `p`'s wishlist before they got it.
    pub fn wishlist_from(&self, p: PlayerId, title_id: i64) -> Result<Option<FromWishlist>> {
        self.with(|c| {
            Ok(c.query_row(
                "SELECT reason, source, wishlisted_at FROM wishlist
                 WHERE title_id=?1 AND player_id=?2 AND state='in_library' AND wishlisted_at IS NOT NULL ORDER BY wishlisted_at LIMIT 1",
                params![title_id, p],
                |r| Ok(FromWishlist { reason: r.get(0)?, source: WishSource::parse(&r.get::<_, String>(1)?), wishlisted_at: r.get(2)? }),
            )
            .optional()?)
        })
    }

    /// These wishlist entries as picks show them, by id. Ids not found are left out.
    pub fn wish_games(&self, ids: &[i64]) -> Result<HashMap<i64, WishGame>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        // A lineup's wishlist rows are its player's, so they are read by id alone.
        let rows: Vec<WishRow> = self.with(|c| {
            let mut out = vec![];
            for id in ids {
                out.extend(c.query_row(&format!("SELECT {COLUMNS} FROM wishlist WHERE id=?1"), params![id], row).optional()?);
            }
            Ok(out)
        })?;
        let facts = self.store_facts(&rows.iter().map(|r| r.appid).collect::<Vec<_>>())?;
        Ok(rows.iter().map(|r| (r.id, WishGame::new(r, facts.get(&r.appid).unwrap_or(&StoreFacts::default())))).collect())
    }

    /// What the store tables hold for these appids.
    pub fn store_facts(&self, appids: &[u32]) -> Result<HashMap<u32, StoreFacts>> {
        let list = |s: Option<String>| -> Vec<String> { s.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default() };
        self.with(|c| {
            let mut details = c.prepare(
                "SELECT success, type, short_description, genres, developers, release_date, header_image FROM details WHERE appid=?1",
            )?;
            let mut reviews = c.prepare("SELECT success, total_positive, total_reviews, review_score_desc FROM reviews WHERE appid=?1")?;
            let mut tags = c.prepare("SELECT success, tags FROM tags WHERE appid=?1")?;
            let mut art = c.prepare("SELECT portrait FROM steam_art WHERE appid=?1")?;
            let mut out = HashMap::new();
            for &a in appids {
                let mut f = StoreFacts::default();
                type Details = (i64, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>);
                let d: Option<Details> = details
                    .query_row(params![a], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)))
                    .optional()?;
                if let Some((ok, ty, desc, genres, devs, release, header)) = d {
                    f.details_checked = true;
                    if ok != 0 {
                        f.type_ = ty;
                        f.short_description = desc;
                        f.genres = list(genres);
                        f.developers = list(devs);
                        f.release_date = release;
                        f.header_image = header;
                    }
                }
                type Reviews = (i64, Option<i64>, Option<i64>, Option<String>);
                let r: Option<Reviews> = reviews.query_row(params![a], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).optional()?;
                if let Some((ok, positive, total, desc)) = r {
                    f.reviews_checked = true;
                    if ok != 0 {
                        f.review_total = total;
                        f.review_pct = match (positive, total) {
                            (Some(p), Some(t)) if t > 0 => Some((p * 100 + t / 2) / t),
                            _ => None,
                        };
                        f.review_desc = desc;
                    }
                }
                let t: Option<(i64, Option<String>)> = tags.query_row(params![a], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
                if let Some((ok, json)) = t {
                    f.tags_checked = true;
                    if ok != 0 {
                        let pairs: Vec<(String, i64)> = json.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
                        f.tags = pairs.into_iter().map(|(n, _)| n).collect();
                    }
                }
                if let Some(p) = art.query_row(params![a], |r| r.get::<_, Option<String>>(0)).optional()? {
                    f.art_checked = true;
                    f.portrait = p;
                }
                out.insert(a, f);
            }
            Ok(out)
        })
    }
}

/// Checks a state a person may set by hand (`PUT /api/wishlist/{id}`).
pub fn settable_state(s: &str) -> Result<WishState> {
    match WishState::parse(s) {
        Some(st @ (WishState::Suggested | WishState::Wishlisted | WishState::Dismissed | WishState::OwnedElsewhere)) => Ok(st),
        Some(WishState::InLibrary) => bail!("in_library is set by a sync that finds the game in your library"),
        None => bail!("state is suggested, wishlisted, dismissed or owned_elsewhere; not {s:?}"),
    }
}
