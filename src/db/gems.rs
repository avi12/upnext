//! Hidden gems: the AI's ratings of the games in the gem pool,
//! and the store details the ratings are written from that games don't carry.

use super::{Db, now};
use anyhow::Result;
use rusqlite::params;
use serde::Serialize;
use std::collections::HashMap;

/// One game's ratings, as stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GemRow {
    /// How little-known its studio is, 0 to 3.
    pub studio: u8,
    /// How rare its mechanics are in the library, 0 to 3.
    pub mechanics: u8,
    /// How odd and interesting its play is, 0 to 3.
    pub play: u8,
    /// A party game: short, but meant for a group, so not on the shelf.
    pub party: bool,
    /// What makes it unusual, in one sentence.
    pub line: String,
    pub prompt_version: u32,
    pub model: Option<String>,
    pub written_at: i64,
}

impl GemRow {
    /// The three ratings together, 0 to 9.
    pub fn score(&self) -> u8 {
        self.studio + self.mechanics + self.play
    }
}

impl Db {
    pub fn put_gem(&self, title_id: i64, row: &GemRow) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO hidden_gems(title_id, studio, mechanics, play, party, line, prompt_version, model, written_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(title_id) DO UPDATE SET studio=excluded.studio, mechanics=excluded.mechanics, play=excluded.play,
                    party=excluded.party, line=excluded.line, prompt_version=excluded.prompt_version, model=excluded.model,
                    written_at=excluded.written_at",
                params![
                    title_id,
                    row.studio,
                    row.mechanics,
                    row.play,
                    row.party,
                    row.line,
                    row.prompt_version,
                    row.model,
                    if row.written_at > 0 { row.written_at } else { now() }
                ],
            )?;
            Ok(())
        })
    }

    /// Every rated game, by title.
    pub fn hidden_gems(&self) -> Result<HashMap<i64, GemRow>> {
        self.with(|c| {
            let mut stmt =
                c.prepare("SELECT title_id, studio, mechanics, play, party, line, prompt_version, model, written_at FROM hidden_gems")?;
            let rows = stmt.query_map([], |r| {
                let rating = |i: usize| -> rusqlite::Result<u8> { Ok(r.get::<_, i64>(i)?.clamp(0, 3) as u8) };
                Ok((
                    r.get::<_, i64>(0)?,
                    GemRow {
                        studio: rating(1)?,
                        mechanics: rating(2)?,
                        play: rating(3)?,
                        party: r.get::<_, i64>(4)? != 0,
                        line: r.get(5)?,
                        prompt_version: r.get::<_, i64>(6)?.max(0) as u32,
                        model: r.get(7)?,
                        written_at: r.get(8)?,
                    },
                ))
            })?;
            Ok(rows.collect::<std::result::Result<HashMap<_, _>, _>>()?)
        })
    }

    /// The publishers in Steam's store details, by appid, for the games asked about.
    pub fn publishers(&self, appids: &[u32]) -> Result<HashMap<u32, Vec<String>>> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT publishers FROM details WHERE appid=?1 AND success=1")?;
            let mut out = HashMap::new();
            for &a in appids {
                let found: Option<Option<String>> = stmt.query_row(params![a], |r| r.get(0)).ok();
                let list: Vec<String> = found.flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
                if !list.is_empty() {
                    out.insert(a, list);
                }
            }
            Ok(out)
        })
    }
}
