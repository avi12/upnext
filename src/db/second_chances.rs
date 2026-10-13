//! The Second Chance page's rows: "Now" for each abandoned game
//! and the page's answer. Read back with the library, in `views.rs`.

use super::{Db, Override, PlayerId, now};
use crate::second_chance::{Answer, Verdict};
use anyhow::Result;
use rusqlite::{OptionalExtension, params};

/// A "Now" as the AI wrote it.
pub struct NowRow<'a> {
    pub now: &'a str,
    pub verdict: Verdict,
    pub line: &'a str,
    /// What it was written from (`second_chance::inputs`).
    pub inputs: &'a str,
    pub model: &'a str,
}

impl Db {
    /// Store a game's "Now", keeping its answer.
    pub fn put_second_chance_now(&self, p: PlayerId, title_id: i64, row: &NowRow) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO second_chances(player_id, title_id, now, verdict, line, inputs, written_at, model)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(player_id, title_id) DO UPDATE SET now=excluded.now, verdict=excluded.verdict, line=excluded.line,
                    inputs=excluded.inputs, written_at=excluded.written_at, model=excluded.model",
                params![p, title_id, row.now, row.verdict.as_str(), row.line, row.inputs, now(), row.model],
            )?;
            Ok(())
        })
    }

    /// The page's answer, or None to take it back. Willing to try again
    /// marks the game Shelved and Not for me marks it Not for me, at the
    /// same moment as the answer, which is how `classify` knows the Shelved
    /// mark came from here. Taking an answer back puts back the mark the
    /// game had before it, as it was.
    pub fn answer_second_chance(&self, p: PlayerId, title_id: i64, answer: Option<Answer>) -> Result<()> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            let exists = tx.query_row("SELECT 1 FROM titles WHERE id=?1", params![title_id], |_| Ok(())).optional()?.is_some();
            if !exists {
                anyhow::bail!("game id {title_id} is not in your library");
            }
            let mark_now: Option<(String, i64)> = tx
                .query_row("SELECT status, updated_at FROM overrides WHERE player_id=?1 AND title_id=?2", params![p, title_id], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .optional()?;
            // The answer standing now, and the mark from before it.
            let standing: Option<(Option<String>, Option<String>, Option<i64>)> = tx
                .query_row(
                    "SELECT answer, prev_status, prev_at FROM second_chances WHERE player_id=?1 AND title_id=?2",
                    params![p, title_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            let (old_answer, prev_status, prev_at) = standing.unwrap_or_default();
            let set_mark = |status: Option<&str>, at: Option<i64>| -> Result<()> {
                match (status, at) {
                    (Some(s), Some(at)) => tx.execute(
                        "INSERT INTO overrides(player_id, title_id, status, updated_at) VALUES (?1, ?2, ?3, ?4)
                         ON CONFLICT(player_id, title_id) DO UPDATE SET status=excluded.status, updated_at=excluded.updated_at",
                        params![p, title_id, s, at],
                    )?,
                    _ => tx.execute("DELETE FROM overrides WHERE player_id=?1 AND title_id=?2", params![p, title_id])?,
                };
                Ok(())
            };
            // An answer that set a mark: put the one from before it back first.
            let marked = |a: Option<&str>| a.and_then(Answer::parse).is_some_and(Answer::marks);
            let (before_status, before_at) = if marked(old_answer.as_deref()) {
                set_mark(prev_status.as_deref(), prev_at)?;
                (prev_status, prev_at)
            } else {
                mark_now.map(|(s, at)| (Some(s), Some(at))).unwrap_or_default()
            };
            let at = now();
            match answer {
                Some(a) => {
                    tx.execute(
                        "INSERT INTO second_chances(player_id, title_id, answer, answered_at, prev_status, prev_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                         ON CONFLICT(player_id, title_id) DO UPDATE SET answer=excluded.answer, answered_at=excluded.answered_at,
                            prev_status=excluded.prev_status, prev_at=excluded.prev_at",
                        params![p, title_id, a.as_str(), at, before_status, before_at],
                    )?;
                    let mark = match a {
                        Answer::Willing => Some(Override::Shelved),
                        Answer::NotForMe => Some(Override::Hidden),
                        Answer::StillNo => None,
                    };
                    if let Some(m) = mark {
                        set_mark(Some(m.as_str()), Some(at))?;
                    }
                }
                None => {
                    tx.execute(
                        "UPDATE second_chances SET answer=NULL, answered_at=NULL, prev_status=NULL, prev_at=NULL
                         WHERE player_id=?1 AND title_id=?2",
                        params![p, title_id],
                    )?;
                }
            }
            tx.commit()?;
            Ok(())
        })
    }

    /// When a Steam app's player reviews were last fetched, if they were.
    pub fn review_texts_fetched_at(&self, appid: u32) -> Result<Option<i64>> {
        self.with(|c| {
            Ok(c.query_row("SELECT fetched_at FROM review_texts WHERE appid=?1 AND success=1", params![appid], |r| r.get(0)).optional()?)
        })
    }

    /// When a GOG game's reviews were last fetched, if they were.
    pub fn gog_reviews_fetched_at(&self, gog_id: &str) -> Result<Option<i64>> {
        self.with(|c| {
            Ok(c.query_row("SELECT fetched_at FROM gog_reviews WHERE gog_id=?1 AND success=1", params![gog_id], |r| r.get(0)).optional()?)
        })
    }
}
