//! Pick batches, their lineups and history.

use super::model::{Alternate, FeedbackAction, GameView, Pick, PickBatch, PickFeedback, Suggestion};
use super::{Db, PlayerId, WishGame};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashMap;

/// One change to a lineup, made with others in one transaction.
#[derive(Debug, Clone)]
pub enum LineupChange {
    /// What was done with a pick on its card.
    Feedback { title_id: i64, action: FeedbackAction, text: Option<String> },
    /// Put a game in a slot, or take it off the lineup (None).
    Slot { title_id: i64, slot: Option<usize> },
}

/// A lineup change that no longer fits the lineup: another action changed
/// it between reading it and writing, such as "It's too ___" waiting on
/// the AI while Not tonight lands on the same pick.
#[derive(Debug, thiserror::Error)]
#[error("the lineup changed while this was being worked out; look at it again and retry")]
pub struct LineupChanged;

/// A pick the lineup showed: in a slot now, or taken off with a reason.
/// Spares never used don't count.
const SHOWN: &str =
    "(p.slot IS NOT NULL OR EXISTS (SELECT 1 FROM pick_feedback f WHERE f.batch_id = p.batch_id AND f.title_id = p.title_id))";

/// Which past lineups History lists (`GET /picks/history`).
#[derive(Debug, Clone, Default)]
pub struct HistoryFind {
    pub limit: usize,
    /// Lineups skipped first, for the next page.
    pub offset: usize,
    pub oldest_first: bool,
    /// Only lineups made at or after this time (Unix seconds).
    pub since: Option<i64>,
    /// Only lineups whose request, or a game they showed, has every word of
    /// this. SQLite's LIKE ignores case for ASCII letters only.
    pub search: Option<String>,
    pub outcome: Option<HistoryOutcome>,
}

impl HistoryFind {
    /// The search's words; past the eighth they're left out.
    fn words(&self) -> Vec<String> {
        self.search.as_deref().unwrap_or_default().split_whitespace().take(8).map(str::to_string).collect()
    }
}

/// What came of a lineup, for History's filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HistoryOutcome {
    /// You played one of its games: Play, or "I've started it".
    Played,
    /// One of the games it showed is on Play later now.
    Saved,
    /// You turned one down: Not tonight, "It's too ___" or Never again.
    TurnedDown,
}

impl HistoryOutcome {
    fn condition(self) -> String {
        match self {
            Self::Played => {
                "EXISTS (SELECT 1 FROM pick_feedback f WHERE f.batch_id = pick_batches.id AND f.action IN ('chosen','started'))".into()
            }
            Self::Saved => format!(
                "EXISTS (SELECT 1 FROM picks p JOIN play_later l ON l.title_id = p.title_id AND l.player_id = pick_batches.player_id
                         WHERE p.batch_id = pick_batches.id AND {SHOWN})"
            ),
            // "same_problem" is the AI's call, not something you said.
            Self::TurnedDown => "EXISTS (SELECT 1 FROM pick_feedback f WHERE f.batch_id = pick_batches.id
                                 AND f.action IN ('not_tonight','too','not_for_me'))"
                .into(),
        }
    }
}

/// `text` with LIKE's wildcards and its escape character taken literally.
fn like_escaped(text: &str) -> String {
    text.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

impl Db {
    // -- picks ----------------------------------------------------------------

    /// Store a new lineup, the answer to player `p`'s request: its picks in
    /// their slots, then its spares.
    pub fn save_batch(&self, p: PlayerId, b: &PickBatch) -> Result<()> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            tx.execute(
                "INSERT INTO pick_batches(id,created_at,prompt,model,duration_ms,cost_usd,candidates,attempts,shown,ranked_by,mode,
                                          suggestion_id,suggestion_reason,routine,player_id)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                params![
                    b.batch_id,
                    b.created_at,
                    b.prompt,
                    b.model,
                    b.duration_ms,
                    b.cost_usd,
                    b.candidates_considered,
                    b.attempts,
                    b.shown as i64,
                    b.ranked_by.as_ref().map(serde_json::to_string).transpose()?,
                    b.mode.map(|m| m.as_str()),
                    b.suggestion.as_ref().map(|x| x.game.id),
                    b.suggestion.as_ref().map(|x| x.reason.as_str()),
                    b.routine.as_ref().map(serde_json::to_string).transpose()?,
                    p
                ],
            )?;
            insert_picks(&tx, &b.batch_id, 0, b.picks.iter().chain(&b.spares))?;
            tx.commit()?;
            Ok(())
        })
    }

    /// Add games to a lineup after the ones it has: new picks for empty
    /// slots (with their `slot`) and new spares. A game the lineup already
    /// has is left out, and a pick whose slot was filled meanwhile becomes
    /// a spare.
    pub fn add_to_batch(&self, batch_id: &str, picks: &[Pick]) -> Result<()> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            let next: i64 =
                tx.query_row("SELECT COALESCE(MAX(position) + 1, 0) FROM picks WHERE batch_id=?1", params![batch_id], |r| r.get(0))?;
            let mut fresh: Vec<Pick> = vec![];
            for p in picks {
                let has = |sql: &str, v: i64| -> rusqlite::Result<bool> {
                    tx.query_row(sql, params![batch_id, v], |r| r.get::<_, i64>(0)).map(|n| n > 0)
                };
                if has("SELECT COUNT(*) FROM picks WHERE batch_id=?1 AND title_id=?2", p.id)? {
                    continue;
                }
                let mut p = p.clone();
                if let Some(s) = p.slot
                    && has("SELECT COUNT(*) FROM picks WHERE batch_id=?1 AND slot=?2", s as i64)?
                {
                    p.slot = None;
                }
                fresh.push(p);
            }
            insert_picks(&tx, batch_id, next, fresh.iter())?;
            tx.commit()?;
            Ok(())
        })
    }

    /// Record what was done with picks and move games in and out of the
    /// lineup's slots, all at once. Every change is planned from a lineup
    /// read earlier, so each must still hold: a game comes off only from a
    /// slot, and goes in only from off the screen into an empty slot. When one
    /// doesn't, nothing is written and the error is [`LineupChanged`].
    pub fn change_lineup(&self, batch_id: &str, changes: &[LineupChange]) -> Result<()> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            for change in changes {
                match change {
                    LineupChange::Feedback { title_id, action, text } => {
                        tx.execute(
                            "INSERT INTO pick_feedback(batch_id, title_id, action, text, at) VALUES (?1, ?2, ?3, ?4, ?5)",
                            params![batch_id, title_id, action.as_str(), text, super::now()],
                        )?;
                    }
                    LineupChange::Slot { title_id, slot } => {
                        let now: Option<Option<i64>> = tx
                            .query_row("SELECT slot FROM picks WHERE batch_id=?1 AND title_id=?2", params![batch_id, title_id], |r| {
                                r.get(0)
                            })
                            .optional()?;
                        let taken = |s: usize| -> rusqlite::Result<bool> {
                            tx.query_row("SELECT COUNT(*) FROM picks WHERE batch_id=?1 AND slot=?2", params![batch_id, s as i64], |r| {
                                r.get::<_, i64>(0)
                            })
                            .map(|n| n > 0)
                        };
                        let fits = match (now, slot) {
                            (Some(on), None) => on.is_some(),
                            (Some(on), Some(s)) => on.is_none() && !taken(*s)?,
                            (None, _) => true,
                        };
                        if !fits {
                            return Err(LineupChanged.into());
                        }
                        let n = tx.execute(
                            "UPDATE picks SET slot=?3 WHERE batch_id=?1 AND title_id=?2",
                            params![batch_id, title_id, slot.map(|s| s as i64)],
                        )?;
                        anyhow::ensure!(n > 0, "#{title_id} is not in lineup {batch_id}");
                    }
                }
            }
            tx.commit()?;
            Ok(())
        })
    }

    /// One of player `p`'s lineups as it stands now.
    pub fn pick_batch(&self, p: PlayerId, batch_id: &str) -> Result<Option<PickBatch>> {
        Ok(self.load_batches(p, "WHERE id = ?1 AND player_id = ?2", params![batch_id, p])?.into_iter().next())
    }

    pub fn pick_history(&self, p: PlayerId, limit: usize) -> Result<Vec<PickBatch>> {
        self.find_history(p, &HistoryFind { limit, ..Default::default() })
    }

    /// Player `p`'s past lineups as History asks for them: a page of
    /// them, in either order, from a time on, matching words and an outcome.
    pub fn find_history(&self, p: PlayerId, f: &HistoryFind) -> Result<Vec<PickBatch>> {
        use rusqlite::types::Value;
        let mut values: Vec<Value> = vec![Value::Integer(p.0)];
        let mut conds: Vec<String> = vec!["player_id = ?1".into()];
        if let Some(since) = f.since {
            values.push(Value::Integer(since));
            conds.push(format!("created_at >= ?{}", values.len()));
        }
        // Every word, in the request or in a game the lineup showed.
        for word in f.words() {
            values.push(Value::Text(format!("%{}%", like_escaped(&word))));
            let n = values.len();
            conds.push(format!(
                "(prompt LIKE ?{n} ESCAPE '\\'
                  OR EXISTS (SELECT 1 FROM picks p LEFT JOIN titles t ON t.id = p.title_id
                             WHERE p.batch_id = pick_batches.id AND {SHOWN}
                               AND (p.name LIKE ?{n} ESCAPE '\\' OR t.name LIKE ?{n} ESCAPE '\\')))"
            ));
        }
        if let Some(outcome) = f.outcome {
            conds.push(outcome.condition());
        }
        let filter = format!("WHERE {}", conds.join(" AND "));
        let order = if f.oldest_first { "ASC" } else { "DESC" };
        values.push(Value::Integer(f.limit as i64));
        values.push(Value::Integer(f.offset as i64));
        let (l, o) = (values.len() - 1, values.len());
        self.load_batches(
            p,
            &format!("{filter} ORDER BY created_at {order}, rowid {order} LIMIT ?{l} OFFSET ?{o}"),
            rusqlite::params_from_iter(values),
        )
    }

    /// Lineups of player `p`'s, with their games as `p` has them.
    fn load_batches(&self, player: PlayerId, tail: &str, p: impl rusqlite::Params) -> Result<Vec<PickBatch>> {
        type Suggested = (Option<i64>, Option<String>);
        let batches: Vec<(PickBatch, Suggested)> = self.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT id,created_at,prompt,model,duration_ms,cost_usd,candidates,attempts,shown,ranked_by,mode,suggestion_id,suggestion_reason,routine
                 FROM pick_batches {tail}"
            ))?;
            let rows = stmt.query_map(p, |r| {
                let suggested: (Option<i64>, Option<String>) = (r.get(11)?, r.get(12)?);
                let batch = PickBatch {
                    batch_id: r.get(0)?,
                    created_at: r.get(1)?,
                    prompt: r.get(2)?,
                    model: r.get(3)?,
                    duration_ms: r.get(4)?,
                    cost_usd: r.get(5)?,
                    api_usd: None,
                    candidates_considered: r.get(6)?,
                    attempts: r.get(7)?,
                    usage: None,
                    shown: r.get::<_, Option<i64>>(8)?.unwrap_or(0).max(0) as usize,
                    picks: vec![],
                    spares: vec![],
                    removed: vec![],
                    ranked_by: r.get::<_, Option<String>>(9)?.and_then(|s| serde_json::from_str(&s).ok()),
                    mode: r.get::<_, Option<String>>(10)?.as_deref().and_then(crate::discovery::Mode::parse),
                    suggestion: None,
                    routine: r.get::<_, Option<String>>(13)?.and_then(|s| serde_json::from_str(&s).ok()),
                };
                Ok((batch, suggested))
            })?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })?;
        let (mut batches, suggested): (Vec<PickBatch>, Vec<Suggested>) = batches.into_iter().unzip();
        let ids: Vec<String> = batches.iter().map(|b| b.batch_id.clone()).collect();
        let mut calls = self.ai_usage_by_batch(&ids)?;
        for b in batches.iter_mut() {
            match calls.remove(&b.batch_id) {
                Some(t) => {
                    b.usage = Some(t.usage);
                    b.api_usd = t.api_usd;
                    b.cost_usd = t.cost_usd.or(b.cost_usd);
                }
                // A batch from before its calls were recorded (v15). The
                // first builds kept the Claude CLI's API-price figure as its
                // cost, which was never a charge, so all that can be said is
                // what it cost at list prices.
                None => (b.api_usd, b.cost_usd) = (b.cost_usd.take(), None),
            }
        }
        // Look games up in the shared library instead of one query per pick.
        let all = self.all_games_shared(player)?;
        let by_id: HashMap<i64, &GameView> = all.iter().map(|g| (g.id, g)).collect();
        for (b, (suggestion_id, reason)) in batches.iter_mut().zip(suggested) {
            let (rows, feedback, mut ratings) = self.with(|c| {
                Ok((batch_rows(c, &b.batch_id)?, batch_feedback(c, &b.batch_id)?, super::ratings::batch_ratings(c, &b.batch_id)?))
            })?;
            if b.shown == 0 {
                b.shown = rows.iter().filter(|r| r.slot.is_some()).count();
            }
            let wish_ids: Vec<i64> = rows.iter().filter_map(|r| r.wishlist_id).chain(suggestion_id).collect();
            let mut wished = self.wish_games(&wish_ids)?;
            // A wishlisted pick whose wishlist entry was deleted since keeps
            // its game, from the pick's name and appid.
            let gone: Vec<(i64, u32, &str)> = rows
                .iter()
                .filter_map(|r| r.wishlist_id.filter(|id| !wished.contains_key(id)).map(|id| (id, r.appid.unwrap_or(0), r.name.as_str())))
                .collect();
            if !gone.is_empty() {
                let facts = self.store_facts(&gone.iter().map(|g| g.1).collect::<Vec<_>>())?;
                for (id, appid, name) in gone {
                    wished.insert(id, WishGame::kept(id, appid, name, facts.get(&appid).unwrap_or(&Default::default())));
                }
            }
            b.suggestion =
                suggestion_id.and_then(|id| wished.get(&id)).map(|g| Suggestion { game: g.clone(), reason: reason.unwrap_or_default() });
            let mut removed: Vec<((i64, bool), Pick)> = vec![];
            for r in rows {
                let feedback = feedback.get(&r.id).cloned();
                let alternate = r.alternate_id.map(|id| Alternate {
                    id,
                    name: by_id.get(&id).map(|g| g.name.clone()).unwrap_or_default(),
                    reason: r.alternate_reason.clone().unwrap_or_default(),
                    game: by_id.get(&id).map(|g| g.slim()),
                });
                let pick = Pick {
                    id: r.id,
                    appid: r.appid,
                    name: r.name,
                    reason: r.reason,
                    game: by_id.get(&r.id).map(|g| g.slim()),
                    slot: r.slot,
                    feedback: feedback.clone(),
                    rating: ratings.remove(&r.id),
                    owned: r.wishlist_id.is_none(),
                    wishlist: r.wishlist_id.and_then(|id| wished.get(&id)).cloned(),
                    alternate,
                    backlog: None,
                };
                match (r.slot, feedback) {
                    (Some(_), _) => b.picks.push(pick),
                    // A game you don't own leaves only by a switch.
                    (None, _) if !pick.owned => removed.push(((0, false), pick)),
                    // Shown once and taken off: everything but Play takes a pick off.
                    (None, Some(f)) if f.action != FeedbackAction::Chosen => {
                        // The one you turned down before those the same call took off with it.
                        removed.push(((f.at, f.action == FeedbackAction::SameProblem), pick))
                    }
                    (None, _) => b.spares.push(pick),
                }
            }
            b.picks.sort_by_key(|p| p.slot);
            removed.sort_by_key(|(at, _)| *at);
            b.removed = removed.into_iter().map(|(_, p)| p).collect();
            b.fill_backlog_lines();
        }
        Ok(batches)
    }

    /// How many pick batches were made since `since`.
    pub fn pick_batches_since(&self, since: i64) -> Result<u64> {
        self.with(|c| {
            Ok(c.query_row("SELECT COUNT(*) FROM pick_batches WHERE created_at >= ?1", params![since], |r| r.get::<_, i64>(0))?.max(0)
                as u64)
        })
    }

    /// Title ids shown in player `p`'s most recent `batches` lineups, taken
    /// off or not (steers variety). Spares never shown don't count. Lineups
    /// made in the same second count newest first.
    pub fn recent_pick_ids(&self, p: PlayerId, batches: usize) -> Result<Vec<i64>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT p.title_id FROM picks p
                 WHERE p.batch_id IN (SELECT id FROM pick_batches WHERE player_id = ?2 ORDER BY created_at DESC, rowid DESC LIMIT ?1)
                   AND (p.slot IS NOT NULL
                        OR EXISTS (SELECT 1 FROM pick_feedback f WHERE f.batch_id = p.batch_id AND f.title_id = p.title_id))",
            )?;
            let rows = stmt.query_map(params![batches as i64, p], |r| r.get::<_, i64>(0))?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }
}

fn insert_picks<'a>(c: &Connection, batch_id: &str, first: i64, picks: impl Iterator<Item = &'a Pick>) -> Result<()> {
    for (i, p) in picks.enumerate() {
        c.execute(
            "INSERT INTO picks(batch_id,position,title_id,appid,name,reason,slot,wishlist_id,alternate_id,alternate_reason)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                batch_id,
                first + i as i64,
                p.id,
                p.appid,
                p.name,
                p.reason,
                p.slot.map(|s| s as i64),
                p.wishlist.as_ref().map(|w| w.id),
                p.alternate.as_ref().map(|a| a.id),
                p.alternate.as_ref().map(|a| a.reason.as_str())
            ],
        )?;
    }
    Ok(())
}

/// A row of `picks`.
struct PickRow {
    id: i64,
    appid: Option<u32>,
    name: String,
    reason: String,
    slot: Option<usize>,
    wishlist_id: Option<i64>,
    alternate_id: Option<i64>,
    alternate_reason: Option<String>,
}

fn batch_rows(c: &Connection, batch_id: &str) -> Result<Vec<PickRow>> {
    let mut stmt = c.prepare(
        "SELECT title_id,appid,name,reason,slot,wishlist_id,alternate_id,alternate_reason FROM picks WHERE batch_id=?1 ORDER BY position",
    )?;
    let rows = stmt.query_map(params![batch_id], |r| {
        Ok(PickRow {
            id: r.get(0)?,
            appid: r.get(1)?,
            name: r.get(2)?,
            reason: r.get(3)?,
            slot: r.get::<_, Option<i64>>(4)?.map(|s| s.max(0) as usize),
            wishlist_id: r.get(5)?,
            alternate_id: r.get(6)?,
            alternate_reason: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// The latest feedback on each of a lineup's games.
fn batch_feedback(c: &Connection, batch_id: &str) -> Result<HashMap<i64, PickFeedback>> {
    let mut stmt = c.prepare("SELECT title_id, action, text, at FROM pick_feedback WHERE batch_id=?1 ORDER BY at, id")?;
    let rows = stmt.query_map(params![batch_id], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, i64>(3)?))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (id, action, text, at) = row?;
        if let Some(action) = FeedbackAction::parse(&action) {
            out.insert(id, PickFeedback { action, text, at });
        }
    }
    Ok(out)
}
