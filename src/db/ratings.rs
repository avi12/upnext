//! Ratings: how well a pick fit its lineup's request, and your
//! own rating of a game.

use super::model::{MissReason, PickRating, PickVerdict};
use super::{Db, PlayerId, now};
use anyhow::Result;
use rusqlite::{Connection, params};
use serde::Serialize;
use std::collections::HashMap;

impl Db {
    /// Rate a pick of a lineup, or take its rating back (None). A reason
    /// only goes with Not a fit. Returns false when the lineup has no such
    /// pick, spares and picks taken off included.
    pub fn set_pick_rating(&self, batch_id: &str, title_id: i64, rating: Option<(PickVerdict, Option<MissReason>)>) -> Result<bool> {
        self.with(|c| {
            let found: i64 =
                c.query_row("SELECT COUNT(*) FROM picks WHERE batch_id=?1 AND title_id=?2", params![batch_id, title_id], |r| r.get(0))?;
            if found == 0 {
                return Ok(false);
            }
            match rating {
                Some((verdict, reason)) => {
                    let reason = reason.filter(|_| verdict == PickVerdict::NotAFit);
                    c.execute(
                        "INSERT INTO pick_ratings(batch_id, title_id, verdict, reason, at) VALUES (?1, ?2, ?3, ?4, ?5)
                         ON CONFLICT(batch_id, title_id) DO UPDATE SET verdict=excluded.verdict, reason=excluded.reason, at=excluded.at",
                        params![batch_id, title_id, verdict.as_str(), reason.map(MissReason::as_str), now()],
                    )?;
                }
                None => {
                    c.execute("DELETE FROM pick_ratings WHERE batch_id=?1 AND title_id=?2", params![batch_id, title_id])?;
                }
            }
            Ok(true)
        })
    }

    /// Rate a game 1 to 5 stars, or clear its rating (None).
    pub fn set_game_rating(&self, p: PlayerId, title_id: i64, stars: Option<u8>) -> Result<()> {
        self.with(|c| {
            match stars {
                Some(s) => {
                    anyhow::ensure!((1..=5).contains(&s), "a rating is 1 to 5 stars, not {s}");
                    c.execute(
                        "INSERT INTO game_ratings(player_id, title_id, stars, at) VALUES (?1, ?2, ?3, ?4)
                         ON CONFLICT(player_id, title_id) DO UPDATE SET stars=excluded.stars, at=excluded.at",
                        params![p, title_id, s, now()],
                    )?;
                }
                None => {
                    c.execute("DELETE FROM game_ratings WHERE player_id=?1 AND title_id=?2", params![p, title_id])?;
                }
            }
            Ok(())
        })
    }
}

/// One of your ratings of a game, for the Profile's "See it" and its export.
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
pub struct GameRatingRow {
    pub title_id: i64,
    pub name: String,
    /// 1 ("Disliked it") to 5 ("Loved it").
    pub stars: u8,
    pub at: i64,
}

impl Db {
    /// Every rating you gave a game, newest first.
    pub fn game_ratings(&self, p: PlayerId) -> Result<Vec<GameRatingRow>> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT r.title_id, COALESCE(t.name, ''), r.stars, r.at FROM game_ratings r LEFT JOIN titles t ON t.id = r.title_id
                 WHERE r.player_id = ?1 ORDER BY r.at DESC, r.title_id",
            )?;
            let rows =
                st.query_map(params![p], |r| Ok(GameRatingRow { title_id: r.get(0)?, name: r.get(1)?, stars: r.get(2)?, at: r.get(3)? }))?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    /// Forget every rating you gave a game; ratings of picks stay, with your
    /// reactions. Returns how many went.
    pub fn delete_game_ratings(&self, p: PlayerId) -> Result<usize> {
        self.with(|c| Ok(c.execute("DELETE FROM game_ratings WHERE player_id=?1", params![p])?))
    }
}

/// Each of a lineup's ratings, by title id.
pub(super) fn batch_ratings(c: &Connection, batch_id: &str) -> Result<HashMap<i64, PickRating>> {
    let mut stmt = c.prepare("SELECT title_id, verdict, reason, at FROM pick_ratings WHERE batch_id=?1")?;
    let rows = stmt.query_map(params![batch_id], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, i64>(3)?))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (id, verdict, reason, at) = row?;
        if let Some(verdict) = PickVerdict::parse(&verdict) {
            out.insert(id, PickRating { verdict, reason: reason.as_deref().and_then(MissReason::parse), at });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forgetting_reactions_forgets_pick_ratings_and_keeps_game_ratings() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("upnext.db"), 120).unwrap();
        db.with(|c| {
            c.execute_batch(
                "INSERT INTO titles(id, key, name) VALUES (1, 'name:hades', 'Hades');
                 INSERT INTO pick_batches(id, created_at, prompt, shown) VALUES ('b1', 1, 'short', 1);
                 INSERT INTO picks(batch_id, position, title_id, name, reason, slot) VALUES ('b1', 0, 1, 'Hades', 'fits', 0);",
            )?;
            Ok(())
        })
        .unwrap();
        assert!(db.set_pick_rating("b1", 1, Some((PickVerdict::Good, None))).unwrap());
        db.set_game_rating(PlayerId::FIRST, 1, Some(3)).unwrap();
        assert_eq!(db.taste_delete_reactions(PlayerId::FIRST).unwrap().picks, 1);
        let left =
            |table: &str| db.with(|c| Ok(c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get::<_, i64>(0))?)).unwrap();
        assert_eq!((left("pick_ratings"), left("game_ratings")), (0, 1));
    }
}
