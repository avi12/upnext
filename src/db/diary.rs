//! The play diary's rows (`play_diary`): minutes per game per hour
//! from Steam's playtime, and launches from the Play button.

use super::players::Sight;
use super::{Db, PlayerId};
use anyhow::Result;
use rusqlite::{OptionalExtension, params};
use serde::Serialize;

/// One row of the diary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiaryRow {
    pub id: i64,
    /// Unix time the hour starts.
    pub hour: i64,
    /// When it was written: the reading, or the exact launch.
    pub at: i64,
    pub title_id: Option<i64>,
    pub appid: Option<i64>,
    pub name: String,
    pub minutes: i64,
    /// steam or launch.
    pub source: String,
}

/// A row to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDiaryRow {
    pub hour: i64,
    pub at: i64,
    pub title_id: Option<i64>,
    pub appid: Option<i64>,
    pub name: String,
    pub minutes: i64,
    pub source: &'static str,
    /// Who pressed Play, for a launch.
    pub player: Option<PlayerId>,
    /// The Steam account a reading came from.
    pub account: Option<i64>,
}

/// How much the diary holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct DiaryCounts {
    /// Hours with play in them.
    pub hours: usize,
    /// Days with anything in them.
    pub days: usize,
    pub launches: usize,
    pub first: Option<i64>,
}

impl Db {
    pub fn diary_add(&self, rows: &[NewDiaryRow]) -> Result<usize> {
        if rows.is_empty() {
            return Ok(0);
        }
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            for r in rows {
                tx.execute(
                    "INSERT INTO play_diary(hour, at, title_id, appid, name, minutes, source, player_id, account_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![r.hour, r.at, r.title_id, r.appid, r.name, r.minutes, r.source, r.player, r.account],
                )?;
            }
            tx.commit()?;
            Ok(rows.len())
        })
    }

    /// Player `p`'s rows from `since` on, oldest first: their launches, and
    /// the readings of the Steam accounts they play on.
    pub fn diary_rows(&self, p: PlayerId, since: i64) -> Result<Vec<DiaryRow>> {
        self.with(|c| {
            let sight = Sight::of(c, p)?;
            let mut st = c.prepare(
                "SELECT id, hour, at, title_id, appid, name, minutes, source, player_id, account_id FROM play_diary
                 WHERE hour >= ?1 ORDER BY hour, id",
            )?;
            let rows = st.query_map(params![since], |r| {
                let row = DiaryRow {
                    id: r.get(0)?,
                    hour: r.get(1)?,
                    at: r.get(2)?,
                    title_id: r.get(3)?,
                    appid: r.get(4)?,
                    name: r.get(5)?,
                    minutes: r.get(6)?,
                    source: r.get(7)?,
                };
                Ok((row, r.get::<_, Option<i64>>(8)?, r.get::<_, Option<i64>>(9)?))
            })?;
            let mut out = vec![];
            for row in rows {
                let (row, player, account) = row?;
                if player == Some(p.0) || account.is_some_and(|a| sight.plays(a)) {
                    out.push(row);
                }
            }
            Ok(out)
        })
    }

    /// Player `p`'s hours with play, days and launches; days by UTC date,
    /// which is close enough for a count.
    pub fn diary_counts(&self, p: PlayerId) -> Result<DiaryCounts> {
        let rows = self.diary_rows(p, i64::MIN)?;
        let distinct = |f: &dyn Fn(&DiaryRow) -> Option<i64>| rows.iter().filter_map(f).collect::<std::collections::HashSet<_>>().len();
        Ok(DiaryCounts {
            hours: distinct(&|r| (r.source == "steam" && r.minutes > 0).then_some(r.hour)),
            days: distinct(&|r| Some(r.hour.div_euclid(86_400))),
            launches: rows.iter().filter(|r| r.source == "launch").count(),
            first: rows.iter().map(|r| r.hour).min(),
        })
    }

    /// Delete player `p`'s diary: their launches and the readings of the
    /// Steam accounts they own. Those accounts' last readings go too, so the
    /// count starts afresh.
    pub fn diary_wipe(&self, p: PlayerId) -> Result<usize> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            let n = tx.execute(
                "DELETE FROM play_diary WHERE player_id = ?1 OR account_id IN (
                     SELECT a.id FROM store_accounts a JOIN store_connections k ON k.id = a.connection_id WHERE k.owner_id = ?1)",
                params![p],
            )?;
            tx.execute(
                "DELETE FROM meta WHERE key = 'diary_reading' OR key IN (
                     SELECT 'diary_reading.' || k.id FROM store_connections k WHERE k.owner_id = ?1)",
                params![p],
            )?;
            tx.commit()?;
            Ok(n)
        })
    }

    /// The title a Steam app is in the library as, if it is.
    pub fn title_for_appid(&self, appid: i64) -> Result<Option<i64>> {
        self.with(|c| {
            Ok(c.query_row(
                "SELECT title_id FROM games WHERE appid=?1 AND removed=0 AND title_id IS NOT NULL ORDER BY (source<>'steam'), id LIMIT 1",
                params![appid],
                |r| r.get(0),
            )
            .optional()?)
        })
    }

    pub fn meta_delete(&self, keys: &[&str]) -> Result<()> {
        self.with(|c| {
            for k in keys {
                c.execute("DELETE FROM meta WHERE key=?1", params![k])?;
            }
            Ok(())
        })
    }
}
