//! A backlog verdict pre-filled from what the install already knows about a
//! player and a game (the table's "From" column), so someone who
//! just marked a game Beat it or Shelved usually changes an answer or two
//! and presses Send.
//!
//! Only answers the install can stand behind are filled in: the mark (or
//! that the game is being played right now), the playtime as a range, the
//! play diary's median session when it holds enough of them, the game
//! reasons the player gave for stopping, and "any time" for a game with no
//! ending. Everything else is asked. Life reasons are never carried over,
//! and nothing here reads the player's rating, name, accounts or settings.

use super::verdict::{Problem, Verdict, is_gog_id};
use super::vocab::{self, STOPPED_OUTCOMES};
use crate::config::Config;
use crate::db::{Db, DiaryRow, GameView, Override, Status};
use crate::lifecycle::{Kind, StopReasons};
use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;

/// The play diary suggests a session length once it holds this many
/// sessions of the game.
pub const DIARY_SESSIONS_AT: usize = 5;

/// Where a pre-filled answer came from, for the form's "from your …" hint.
/// An answer with no source is the player's to give.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// The status the player set.
    Mark,
    /// No mark, but the game is being played now.
    Playing,
    /// The store's playtime, as a range.
    Playtime,
    /// The play diary's median session.
    Diary,
    /// The game reasons the player gave for putting it down.
    StopReasons,
    /// The game has no ending, so any time is fine to stop.
    NoEnding,
}

/// A verdict ready for the form.
#[derive(Debug, Clone, Serialize)]
pub struct Draft {
    pub title_id: i64,
    pub name: String,
    /// Why this game can't get a verdict, when it can't: it has neither a
    /// Steam appid nor a GOG id to match it across installs by.
    pub cant_send: Option<String>,
    pub verdict: Verdict,
    /// The pre-filled answers' sources, by field.
    pub from: BTreeMap<&'static str, Source>,
    /// What's left before it can be sent, usually the required answers
    /// nothing could fill in ([`Verdict::check`]).
    pub problems: Vec<Problem>,
}

/// What a draft is built from: the game as the player sees it, every time
/// they said why they put it down (newest first), and the play diary's
/// sessions of it in minutes when the diary is on.
#[derive(Debug, Clone, Copy)]
pub struct Facts<'a> {
    pub game: &'a GameView,
    pub stop_reasons: &'a [StopReasons],
    pub diary_sessions: Option<&'a [i64]>,
}

/// The outcome a mark means. Unmarked games aren't guessed at, apart from
/// one being played right now: UpNext's own shelved and abandoned are
/// inferred from playtime and dates, not something the player said.
fn outcome(g: &GameView) -> Option<(&'static str, Source)> {
    let marked = match g.override_? {
        Override::Done => "finished",
        Override::Shelved | Override::BurntOut => "shelved",
        Override::Abandoned => "stopped",
        Override::StillPlaying => "endless",
        Override::Hidden => "bounced",
    };
    Some((marked, Source::Mark))
}

pub fn build(f: &Facts) -> Draft {
    let g = f.game;
    let gog_id = g.copies.iter().filter(|c| c.source == "gog").map(|c| c.source_id.trim()).find(|id| is_gog_id(id)).map(str::to_string);
    let mut v = Verdict::new(g.appid.filter(|a| *a != 0), gog_id);
    let mut from = BTreeMap::new();
    let outcome = outcome(g).or((g.status == Status::Playing).then_some(("playing", Source::Playing)));
    if let Some((o, source)) = outcome {
        v.outcome = Some(o.into());
        from.insert(vocab::OUTCOME.name, source);
    }
    if g.playtime_known {
        v.hours = Some(vocab::hours_range(g.playtime_minutes).into());
        from.insert(vocab::HOURS.name, Source::Playtime);
    }
    if let Some(m) = f.diary_sessions.and_then(median_of_enough) {
        v.sessions = Some(vocab::sessions_for(m).into());
        from.insert(vocab::SESSIONS.name, Source::Diary);
    }
    if v.outcome.as_deref() == Some("endless") || g.kind == Kind::Endless {
        v.stop_point = Some("anytime".into());
        from.insert(vocab::STOP_POINT.name, Source::NoEnding);
    }
    if v.outcome.as_deref().is_some_and(|o| STOPPED_OUTCOMES.contains(&o)) {
        for code in f.stop_reasons.iter().flat_map(|r| &r.codes) {
            if vocab::is_stop_reason(code) && !v.stopped_for.contains(code) {
                v.stopped_for.push(code.clone());
            }
        }
        if !v.stopped_for.is_empty() {
            from.insert("stopped_for", Source::StopReasons);
        }
    }
    let cant_send = (v.steam_appid.is_none() && v.gog_id.is_none())
        .then(|| "UpNext can't match this game across installs yet: it has neither a Steam appid nor a GOG id.".to_string());
    let problems = v.check();
    Draft { title_id: g.id, name: g.name.clone(), cant_send, verdict: v, from, problems }
}

/// The median of the sessions, once there are enough of them.
fn median_of_enough(sessions: &[i64]) -> Option<i64> {
    if sessions.len() < DIARY_SESSIONS_AT {
        return None;
    }
    let mut s = sessions.to_vec();
    s.sort_unstable();
    let mid = s.len() / 2;
    Some(if s.len().is_multiple_of(2) { (s[mid - 1] + s[mid]) / 2 } else { s[mid] })
}

/// One game's sessions in the play diary, in minutes: its Steam readings
/// hour by hour, where play in back-to-back hours is one session. Launches
/// carry no minutes and don't count.
pub fn diary_sessions(rows: &[DiaryRow], title_id: i64) -> Vec<i64> {
    let mut hours: Vec<(i64, i64)> =
        rows.iter().filter(|r| r.title_id == Some(title_id) && r.source == "steam" && r.minutes > 0).map(|r| (r.hour, r.minutes)).collect();
    hours.sort_unstable();
    let mut out: Vec<i64> = vec![];
    let mut last: Option<i64> = None;
    for (hour, minutes) in hours {
        match (last, out.last_mut()) {
            (Some(prev), Some(s)) if hour - prev <= 3600 => *s += minutes,
            _ => out.push(minutes),
        }
        last = Some(hour);
    }
    out
}

/// The draft for a game in this library, or None for no such game.
pub fn for_title(db: &Db, cfg: &Config, title_id: i64) -> Result<Option<Draft>> {
    let Some(game) = db.game(cfg.whose(), title_id)? else { return Ok(None) };
    let stop_reasons = db.stop_reason_history(cfg.whose(), title_id)?;
    let diary = if cfg.play_diary { Some(diary_sessions(&db.diary_rows(cfg.whose(), 0)?, title_id)) } else { None };
    Ok(Some(build(&Facts { game: &game, stop_reasons: &stop_reasons, diary_sessions: diary.as_deref() })))
}
