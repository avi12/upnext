//! The taste profile's rows (`profile_statements`), and the
//! reactions and marks its page shows and deletes.

use super::{Db, PlayerId, now};
use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, Row, params};
use serde::Serialize;
use serde_json::Value;

/// One row of `profile_statements`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatementRow {
    pub id: i64,
    /// favourite, like, dislike, hard_no, context or pattern.
    pub kind: String,
    pub title_id: Option<i64>,
    pub about: Option<String>,
    /// A favourite's reasons, or a context's answer.
    pub why: Option<Value>,
    pub note: Option<String>,
    /// stated or derived.
    pub source: String,
    pub evidence: Option<Value>,
    pub confidence: Option<f64>,
    pub hidden: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

impl StatementRow {
    fn from_row(r: &Row) -> rusqlite::Result<Self> {
        let json = |s: Option<String>| s.and_then(|s| serde_json::from_str(&s).ok());
        Ok(Self {
            id: r.get(0)?,
            kind: r.get(1)?,
            title_id: r.get(2)?,
            about: r.get(3)?,
            why: json(r.get(4)?),
            note: r.get(5)?,
            source: r.get(6)?,
            evidence: json(r.get(7)?),
            confidence: r.get(8)?,
            hidden: r.get::<_, i64>(9)? != 0,
            created_at: r.get(10)?,
            updated_at: r.get(11)?,
        })
    }
}

/// Rows of a lineup's table (`x`) that belong to player `?1`'s lineups.
const MINE: &str = "WHERE x.batch_id IN (SELECT id FROM pick_batches WHERE player_id = ?1)";

const COLUMNS: &str = "id, kind, title_id, about, why, note, source, evidence, confidence, hidden, created_at, updated_at";

/// A statement you made, as checked by `taste::check_statement`.
#[derive(Debug, Clone, PartialEq)]
pub struct NewStatement {
    pub kind: String,
    pub title_id: Option<i64>,
    pub about: Option<String>,
    pub why: Option<Value>,
    pub note: Option<String>,
}

/// A pattern the profile job found, to store.
#[derive(Debug, Clone)]
pub struct PatternRow {
    /// "drift:facet:grind:heavy"
    pub key: String,
    pub sentence: String,
    pub evidence: Value,
    pub confidence: f64,
}

/// What storing a run's patterns changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct PatternChanges {
    pub added: usize,
    pub updated: usize,
    /// Gone from the evidence, and not hidden.
    pub removed: usize,
}

/// One reaction the profile reads, for "See it": a pick card's action or
/// rating, a set of reasons for stopping, or a Second Chance answer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReactionRow {
    pub at: i64,
    pub title_id: i64,
    pub name: String,
    /// A pick action (chosen, not_tonight, too, same_problem, not_for_me,
    /// started), a pick's rating ("good_pick" or "not_a_fit"),
    /// "reasons" or "second_chance".
    pub what: String,
    /// The word for "too", how it went for "started", why it was "not_a_fit"
    /// when you said, the reason codes (JSON list) for "reasons", the answer
    /// for "second_chance".
    pub text: Option<String>,
    /// The note given with reasons.
    pub note: Option<String>,
}

/// How many reactions of each kind there are.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct ReactionCounts {
    /// By pick action.
    pub picks: std::collections::BTreeMap<String, usize>,
    /// Picks you rated, by verdict: `good` and `not_a_fit`.
    pub pick_ratings: std::collections::BTreeMap<String, usize>,
    /// Games with reasons for stopping.
    pub stop_reasons: usize,
    pub second_chance_answers: usize,
}

/// What deleting reactions removed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ReactionsDeleted {
    pub picks: usize,
    pub stop_reasons: usize,
    pub second_chance_answers: usize,
}

impl Db {
    /// Every row of player `p`'s profile, oldest first.
    pub fn taste_statements(&self, p: PlayerId) -> Result<Vec<StatementRow>> {
        self.with(|c| {
            let mut st = c.prepare(&format!("SELECT {COLUMNS} FROM profile_statements WHERE player_id=?1 ORDER BY id"))?;
            Ok(st.query_map(params![p], StatementRow::from_row)?.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    /// One row of player `p`'s profile.
    pub fn taste_statement(&self, p: PlayerId, id: i64) -> Result<Option<StatementRow>> {
        self.with(|c| {
            Ok(c.query_row(
                &format!("SELECT {COLUMNS} FROM profile_statements WHERE id=?1 AND player_id=?2"),
                params![id, p],
                StatementRow::from_row,
            )
            .optional()?)
        })
    }

    /// Store something you said. A like, dislike or hard no about something
    /// already in one of those lists moves it to this kind; a habit already
    /// answered takes the new answer; a game already a favourite keeps its
    /// row and takes the new reasons. Returns the row's id.
    pub fn taste_state(&self, p: PlayerId, s: &NewStatement) -> Result<i64> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            let at = now();
            let why = s.why.as_ref().map(serde_json::to_string).transpose()?;
            let existing: Option<i64> = match s.kind.as_str() {
                "favourite" => tx
                    .query_row(
                        "SELECT id FROM profile_statements WHERE player_id=?1 AND kind='favourite' AND title_id=?2",
                        params![p, s.title_id],
                        |r| r.get(0),
                    )
                    .optional()?,
                "context" => tx
                    .query_row(
                        "SELECT id FROM profile_statements WHERE player_id=?1 AND kind='context' AND about=?2",
                        params![p, s.about],
                        |r| r.get(0),
                    )
                    .optional()?,
                "like" | "dislike" | "hard_no" => tx
                    .query_row(
                        "SELECT id FROM profile_statements WHERE player_id=?1 AND kind IN ('like','dislike','hard_no') AND about=?2",
                        params![p, s.about],
                        |r| r.get(0),
                    )
                    .optional()?,
                other => bail!("{other} isn't something you can state"),
            };
            let id = match existing {
                Some(id) => {
                    tx.execute(
                        "UPDATE profile_statements SET kind=?2, why=?3, note=?4, source='stated', updated_at=?5 WHERE id=?1",
                        params![id, s.kind, why, s.note, at],
                    )?;
                    id
                }
                None => {
                    tx.execute(
                        "INSERT INTO profile_statements(player_id, kind, title_id, about, why, note, source, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'stated', ?7, ?7)",
                        params![p, s.kind, s.title_id, s.about, why, s.note, at],
                    )?;
                    tx.last_insert_rowid()
                }
            };
            tx.commit()?;
            Ok(id)
        })
    }

    /// Change a stated row's kind (among like, dislike and hard no), reasons
    /// or note. Fields left None stay as they are.
    pub fn taste_edit(&self, p: PlayerId, id: i64, kind: Option<&str>, why: Option<&Value>, note: Option<Option<&str>>) -> Result<()> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            if tx
                .query_row("SELECT 1 FROM profile_statements WHERE id=?1 AND player_id=?2", params![id, p], |_| Ok(()))
                .optional()?
                .is_none()
            {
                return Ok(());
            }
            if let Some(k) = kind {
                tx.execute("UPDATE profile_statements SET kind=?2 WHERE id=?1", params![id, k])?;
            }
            if let Some(w) = why {
                tx.execute("UPDATE profile_statements SET why=?2 WHERE id=?1", params![id, serde_json::to_string(w)?])?;
            }
            if let Some(n) = note {
                tx.execute("UPDATE profile_statements SET note=?2 WHERE id=?1", params![id, n])?;
            }
            tx.execute("UPDATE profile_statements SET updated_at=?2 WHERE id=?1", params![id, now()])?;
            tx.commit()?;
            Ok(())
        })
    }

    /// "Not really" (true) or "Show again" (false) on a pattern.
    pub fn taste_hide(&self, p: PlayerId, id: i64, hidden: bool) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE profile_statements SET hidden=?2, updated_at=?3 WHERE id=?1 AND player_id=?4",
                params![id, hidden as i64, now(), p],
            )?;
            Ok(())
        })
    }

    pub fn taste_delete(&self, p: PlayerId, id: i64) -> Result<bool> {
        self.with(|c| Ok(c.execute("DELETE FROM profile_statements WHERE id=?1 AND player_id=?2", params![id, p])? > 0))
    }

    /// Store a run's patterns: new ones are added, ones found again take the
    /// new sentence and evidence, and ones no longer found are removed,
    /// except hidden ones, kept so "Not really" holds when one comes back.
    pub fn taste_put_patterns(&self, p: PlayerId, found: &[PatternRow]) -> Result<PatternChanges> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            let at = now();
            let mut changes = PatternChanges::default();
            let existing: Vec<(i64, String, bool)> = {
                let mut st = tx.prepare("SELECT id, about, hidden FROM profile_statements WHERE player_id=?1 AND kind='pattern'")?;
                st.query_map(params![p], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0)))?.collect::<rusqlite::Result<_>>()?
            };
            for pat in found {
                let evidence = serde_json::to_string(&pat.evidence)?;
                match existing.iter().find(|(_, about, _)| *about == pat.key) {
                    Some((id, ..)) => {
                        tx.execute(
                            "UPDATE profile_statements SET note=?2, evidence=?3, confidence=?4, updated_at=?5 WHERE id=?1",
                            params![id, pat.sentence, evidence, pat.confidence, at],
                        )?;
                        changes.updated += 1;
                    }
                    None => {
                        tx.execute(
                            "INSERT INTO profile_statements(player_id, kind, about, note, source, evidence, confidence, created_at, updated_at)
                             VALUES (?6, 'pattern', ?1, ?2, 'derived', ?3, ?4, ?5, ?5)",
                            params![pat.key, pat.sentence, evidence, pat.confidence, at, p],
                        )?;
                        changes.added += 1;
                    }
                }
            }
            for (id, about, hidden) in &existing {
                if !hidden && !found.iter().any(|pat| &pat.key == about) {
                    tx.execute("DELETE FROM profile_statements WHERE id=?1", params![id])?;
                    changes.removed += 1;
                }
            }
            tx.commit()?;
            Ok(changes)
        })
    }

    /// Delete everything you told the profile, and forget which patterns
    /// you said weren't right. Returns the rows deleted.
    pub fn taste_delete_answers(&self, p: PlayerId) -> Result<usize> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            let n = tx.execute("DELETE FROM profile_statements WHERE player_id=?1 AND source='stated'", params![p])?;
            let hidden = tx.execute("DELETE FROM profile_statements WHERE player_id=?1 AND kind='pattern' AND hidden=1", params![p])?;
            // Routines are what you told it too.
            let routines = tx.execute("DELETE FROM routines WHERE player_id=?1", params![p])?;
            tx.commit()?;
            Ok(n + hidden + routines)
        })
    }

    /// Every reaction of player `p`'s, newest first.
    pub fn taste_reactions(&self, p: PlayerId) -> Result<Vec<ReactionRow>> {
        self.with(|c| {
            let mut out: Vec<ReactionRow> = vec![];
            let name = "COALESCE((SELECT name FROM titles WHERE id = x.title_id), '')";
            let mut st = c.prepare(&format!("SELECT x.at, x.title_id, {name}, x.action, x.text FROM pick_feedback x {MINE}"))?;
            for r in st.query_map(params![p], |r| {
                Ok(ReactionRow { at: r.get(0)?, title_id: r.get(1)?, name: r.get(2)?, what: r.get(3)?, text: r.get(4)?, note: None })
            })? {
                out.push(r?);
            }
            let mut st = c.prepare(&format!("SELECT x.at, x.title_id, {name}, x.verdict, x.reason FROM pick_ratings x {MINE}"))?;
            for r in st.query_map(params![p], |r| {
                let verdict: String = r.get(3)?;
                Ok(ReactionRow {
                    at: r.get(0)?,
                    title_id: r.get(1)?,
                    name: r.get(2)?,
                    what: if verdict == "good" { "good_pick".into() } else { verdict },
                    text: r.get(4)?,
                    note: None,
                })
            })? {
                out.push(r?);
            }
            let mut st =
                c.prepare(&format!("SELECT x.at, x.title_id, {name}, x.reasons, x.note FROM stop_reasons x WHERE x.player_id = ?1"))?;
            for r in st.query_map(params![p], |r| {
                Ok(ReactionRow {
                    at: r.get(0)?,
                    title_id: r.get(1)?,
                    name: r.get(2)?,
                    what: "reasons".into(),
                    text: r.get(3)?,
                    note: r.get(4)?,
                })
            })? {
                out.push(r?);
            }
            let mut st = c.prepare(&format!(
                "SELECT x.answered_at, x.title_id, {name}, x.answer FROM second_chances x WHERE x.player_id = ?1 AND x.answered_at IS NOT NULL"
            ))?;
            for r in st.query_map(params![p], |r| {
                Ok(ReactionRow {
                    at: r.get(0)?,
                    title_id: r.get(1)?,
                    name: r.get(2)?,
                    what: "second_chance".into(),
                    text: r.get(3)?,
                    note: None,
                })
            })? {
                out.push(r?);
            }
            out.sort_by_key(|r| std::cmp::Reverse(r.at));
            Ok(out)
        })
    }

    pub fn taste_reaction_counts(&self, p: PlayerId) -> Result<ReactionCounts> {
        self.with(|c| {
            let mut counts = ReactionCounts::default();
            let mut st = c.prepare(&format!("SELECT x.action, COUNT(*) FROM pick_feedback x {MINE} GROUP BY x.action"))?;
            for r in st.query_map(params![p], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
                let (action, n) = r?;
                counts.picks.insert(action, n as usize);
            }
            let mut st = c.prepare(&format!("SELECT x.verdict, COUNT(*) FROM pick_ratings x {MINE} GROUP BY x.verdict"))?;
            for r in st.query_map(params![p], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
                let (verdict, n) = r?;
                counts.pick_ratings.insert(verdict, n as usize);
            }
            counts.stop_reasons = c
                .query_row("SELECT COUNT(DISTINCT title_id) FROM stop_reasons WHERE player_id=?1", params![p], |r| r.get::<_, i64>(0))?
                as usize;
            counts.second_chance_answers =
                c.query_row("SELECT COUNT(*) FROM second_chances WHERE player_id=?1 AND answered_at IS NOT NULL", params![p], |r| {
                    r.get::<_, i64>(0)
                })? as usize;
            Ok(counts)
        })
    }

    /// Forget every reaction: what you did with picks and how you rated them,
    /// why you stopped, and your Second Chance answers. The marks those
    /// answers set stay, as statuses you set.
    pub fn taste_delete_reactions(&self, p: PlayerId) -> Result<ReactionsDeleted> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            let mine = "batch_id IN (SELECT id FROM pick_batches WHERE player_id = ?1)";
            let deleted = ReactionsDeleted {
                // A pick's rating is a reaction to it too.
                picks: tx.execute(&format!("DELETE FROM pick_feedback WHERE {mine}"), params![p])?
                    + tx.execute(&format!("DELETE FROM pick_ratings WHERE {mine}"), params![p])?,
                stop_reasons: tx.execute("DELETE FROM stop_reasons WHERE player_id=?1", params![p])?,
                second_chance_answers: tx.execute(
                    "UPDATE second_chances SET answer=NULL, answered_at=NULL, prev_status=NULL, prev_at=NULL
                     WHERE player_id=?1 AND answered_at IS NOT NULL",
                    params![p],
                )?,
            };
            tx.commit()?;
            Ok(deleted)
        })
    }

    /// Clear every status you set. Returns how many games had one.
    pub fn taste_delete_marks(&self, p: PlayerId) -> Result<usize> {
        self.with(|c| Ok(c.execute("DELETE FROM overrides WHERE player_id=?1", params![p])?))
    }
}
