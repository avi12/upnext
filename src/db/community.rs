//! Backlog verdicts from other players: the last
//! snapshot's games, kept whole in `community_verdicts`, and what the
//! library reads of them.
//!
//! Rows are kept by the ids the snapshot publishes, not by title, and
//! matched when the library is read: by Steam appid, then by the GOG id of
//! a GOG copy, as the shared game data is (`seed::apply`). A game synced
//! after the download gets its verdicts at once, and a snapshot never
//! points at a title that's gone.
//!
//! Nothing shows unless "Backlog verdicts from other players" is on
//! (`verdicts_receive`); turning it off hides the snapshot without
//! deleting it, so turning it on again shows the last one at once.

use super::{Db, GameView, now};
use crate::verdicts::community::Backlog;
use crate::verdicts::format::{GameCounts, GameSummary, ShownText};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::collections::HashMap;

/// The meta rows that describe the snapshot kept.
pub const DATE_META: &str = "verdicts_date";
pub const SOURCE_META: &str = "verdicts_source";
pub const ETAG_META: &str = "verdicts_etag";
pub const APPLIED_META: &str = "verdicts_applied_at";
pub const CHECKED_META: &str = "verdicts_checked_at";

/// The setting that turns receiving on.
pub const RECEIVE_SETTING: &str = "verdicts_receive";

/// The snapshot this install keeps, for Settings and the job.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct CommunitySnapshot {
    /// The snapshot's day (YYYY-MM-DD); None before the first.
    pub date: Option<String>,
    /// Where it came from: the service's address, or a file.
    pub source: Option<String>,
    /// When it was applied, and when the service was last asked for a newer one (Unix seconds).
    pub applied_at: Option<i64>,
    pub checked_at: Option<i64>,
    /// Games in it, and verdicts they count.
    pub games: usize,
    pub verdicts: u64,
    /// Of its games, the ones in this library.
    pub matched: usize,
}

fn receiving(c: &Connection) -> Result<bool> {
    let v: Option<String> = c.query_row("SELECT value FROM settings WHERE key=?1", params![RECEIVE_SETTING], |r| r.get(0)).optional()?;
    Ok(matches!(v.as_deref(), Some("true" | "1" | "on")))
}

fn meta(c: &Connection, key: &str) -> Result<Option<String>> {
    Ok(c.query_row("SELECT value FROM meta WHERE key=?1", params![key], |r| r.get(0)).optional()?)
}

fn set_meta(c: &Connection, key: &str, value: Option<&str>) -> Result<()> {
    match value {
        Some(v) => {
            c.execute("INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key, v])?
        }
        None => c.execute("DELETE FROM meta WHERE key=?1", params![key])?,
    };
    Ok(())
}

/// A stored game as read: its appid, its GOG id, its count, and its
/// counts and texts as JSON.
type StoredRow = (Option<u32>, Option<String>, u32, String, String);

/// Every GOG copy's id, by title.
fn gog_ids(g: &GameView) -> impl Iterator<Item = &str> {
    g.copies.iter().filter(|c| c.source == "gog").map(|c| c.source_id.as_str())
}

/// Put each title's verdicts on the views, when receiving is on: the
/// snapshot's game with its Steam appid, else with a GOG id of its copies.
pub(super) fn attach(c: &Connection, views: &mut [GameView]) -> Result<()> {
    if views.is_empty() || !receiving(c)? {
        return Ok(());
    }
    // Only the snapshot's games in this library are read: the snapshot
    // holds every game anyone wrote about.
    let mut st = c.prepare(
        "SELECT steam_appid, gog_id, n, counts FROM community_verdicts
         WHERE steam_appid IN (SELECT appid FROM titles WHERE appid IS NOT NULL)
            OR gog_id IN (SELECT source_id FROM games WHERE source='gog' AND removed=0)",
    )?;
    let mut by_appid: HashMap<u32, Backlog> = HashMap::new();
    let mut by_gog: HashMap<String, Backlog> = HashMap::new();
    for r in st
        .query_map([], |r| Ok((r.get::<_, Option<u32>>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, u32>(2)?, r.get::<_, String>(3)?)))?
    {
        let (appid, gog, n, counts) = r?;
        let Some(b) = serde_json::from_str::<GameCounts>(&counts).ok().and_then(|counts| Backlog::of(n, counts)) else { continue };
        if let Some(g) = gog {
            by_gog.entry(g).or_insert_with(|| b.clone());
        }
        if let Some(a) = appid {
            by_appid.insert(a, b);
        }
    }
    for g in views.iter_mut() {
        let found = g.appid.and_then(|a| by_appid.get(&a)).or_else(|| gog_ids(g).find_map(|id| by_gog.get(id)));
        g.backlog = found.cloned();
    }
    Ok(())
}

impl Db {
    /// Put a snapshot in place of the one kept, in one transaction.
    pub fn replace_community_verdicts(&self, date: &str, source: &str, etag: Option<&str>, games: &[GameSummary]) -> Result<()> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            tx.execute("DELETE FROM community_verdicts", [])?;
            {
                let mut st =
                    tx.prepare("INSERT INTO community_verdicts(steam_appid, gog_id, n, counts, texts) VALUES (?1, ?2, ?3, ?4, ?5)")?;
                for g in games {
                    st.execute(params![g.steam_appid, g.gog_id, g.n, serde_json::to_string(&g.counts)?, serde_json::to_string(&g.texts)?])?;
                }
            }
            let at = now().to_string();
            set_meta(&tx, DATE_META, Some(date))?;
            set_meta(&tx, SOURCE_META, Some(source))?;
            set_meta(&tx, ETAG_META, etag)?;
            set_meta(&tx, APPLIED_META, Some(&at))?;
            set_meta(&tx, CHECKED_META, Some(&at))?;
            tx.commit()?;
            Ok(())
        })
    }

    /// Note that the service was asked and had nothing newer, with the
    /// manifest's ETag for next time.
    pub fn community_verdicts_checked(&self, etag: Option<&str>) -> Result<()> {
        self.with(|c| {
            if etag.is_some() {
                set_meta(c, ETAG_META, etag)?;
            }
            set_meta(c, CHECKED_META, Some(&now().to_string()))
        })
    }

    /// The kept snapshot's date and its manifest's ETag, for asking for a newer one.
    pub fn community_verdicts_kept(&self) -> Result<(Option<String>, Option<String>)> {
        self.with(|c| Ok((meta(c, DATE_META)?, meta(c, ETAG_META)?)))
    }

    /// What the kept snapshot is, and how much of it is about this library.
    pub fn community_snapshot(&self) -> Result<CommunitySnapshot> {
        self.with(|c| {
            let (games, verdicts): (i64, i64) =
                c.query_row("SELECT COUNT(*), COALESCE(SUM(n), 0) FROM community_verdicts", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
            let matched: i64 = c.query_row(
                "SELECT COUNT(*) FROM community_verdicts
                 WHERE steam_appid IN (SELECT appid FROM titles WHERE appid IS NOT NULL)
                    OR (gog_id IN (SELECT source_id FROM games WHERE source='gog' AND removed=0)
                        AND (steam_appid IS NULL OR steam_appid NOT IN (SELECT appid FROM titles WHERE appid IS NOT NULL)))",
                [],
                |r| r.get(0),
            )?;
            let time = |key: &str| -> Result<Option<i64>> { Ok(meta(c, key)?.and_then(|v| v.parse().ok())) };
            Ok(CommunitySnapshot {
                date: meta(c, DATE_META)?,
                source: meta(c, SOURCE_META)?,
                applied_at: time(APPLIED_META)?,
                checked_at: time(CHECKED_META)?,
                games: games as usize,
                verdicts: verdicts as u64,
                matched: matched as usize,
            })
        })
    }

    /// One title's verdicts as the snapshot has them, texts and all, when
    /// receiving is on.
    pub fn community_verdicts_for(&self, title_id: i64) -> Result<Option<GameSummary>> {
        self.with(|c| {
            if !receiving(c)? {
                return Ok(None);
            }
            let appid: Option<u32> =
                c.query_row("SELECT appid FROM titles WHERE id=?1", params![title_id], |r| r.get(0)).optional()?.flatten();
            let read = |r: &rusqlite::Row| -> rusqlite::Result<StoredRow> { Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)) };
            const COLUMNS: &str = "SELECT steam_appid, gog_id, n, counts, texts FROM community_verdicts";
            let mut row = match appid {
                Some(a) => c.query_row(&format!("{COLUMNS} WHERE steam_appid=?1"), params![a], read).optional()?,
                None => None,
            };
            if row.is_none() {
                row = c
                    .query_row(
                        &format!(
                            "{COLUMNS} WHERE gog_id IN (SELECT source_id FROM games WHERE title_id=?1 AND source='gog' AND removed=0)
                             ORDER BY id LIMIT 1"
                        ),
                        params![title_id],
                        read,
                    )
                    .optional()?;
            }
            let Some((steam_appid, gog_id, n, counts, texts)) = row else { return Ok(None) };
            let counts: GameCounts = serde_json::from_str(&counts)?;
            let texts: Vec<ShownText> = serde_json::from_str(&texts)?;
            Ok(Some(GameSummary { steam_appid, gog_id, n, counts, texts }))
        })
    }
}
