//! The play diary: when you play, hour by hour, only while you
//! have it on. Once an hour UpNext asks Steam what you played in the last two
//! weeks (`GetRecentlyPlayedGames`) and notes how much each game's playtime
//! grew since the last reading, in the hour it was played. The Play button
//! adds exact start times. Heroic's Epic and Amazon games report no
//! playtime, so only their launches from UpNext show.
//!
//! From the diary come habits, on the profile page and in About me where you
//! haven't stated them: how long your weeknight and weekend sessions usually
//! run, the times of day you play, and when multiplayer games get played.

use super::{Subject, When, features};
use crate::config::Config;
use crate::db::PlayerId;
use crate::db::{Db, DiaryRow, GameView, NewDiaryRow, SyncConnection};
use crate::enrich::Clients;
use anyhow::{Result, bail};
use chrono::Timelike;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

const HOUR: i64 = 3600;
const DAY: i64 = 86_400;
/// A reading this long after the last can't say when the time between them
/// was played, so it only starts the count again.
pub const MAX_GAP: i64 = 3 * HOUR;
/// Readings are an hour apart, less a minute, so a scheduler that looks
/// every 30 seconds doesn't skip one.
pub const EVERY: i64 = HOUR - 60;
/// Habits are read from this many days of the diary.
pub const WINDOW_DAYS: i64 = 90;
/// Sessions it takes before the diary says how long yours usually are.
pub const MIN_SESSIONS: usize = 3;
/// Play it takes before the diary says when you play, in minutes.
const MIN_MINUTES_FOR_TIMES: i64 = 5 * 60;
/// A time of day shows when it has at least this share of your play.
const TIME_SHARE: f64 = 0.2;
/// Tags that make a game multiplayer for the diary.
const MULTIPLAYER: [&str; 7] = ["Multiplayer", "Co-op", "PvP", "Massively Multiplayer", "MMORPG", "MOBA", "Battle Royale"];

const READING: &str = "diary_reading";
const TRIED: &str = "diary_tried";
const ERROR: &str = "diary_error";

/// Steam's playtime for the games played in the last two weeks, at a time.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Reading {
    pub at: i64,
    pub games: BTreeMap<u32, Played>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Played {
    pub name: String,
    /// Total minutes.
    pub forever: i64,
    /// Minutes in the last two weeks.
    pub two_weeks: i64,
}

/// Minutes a game was played between two readings, and the hour they go in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub appid: u32,
    pub hour: i64,
    pub minutes: i64,
}

/// What each game was played between two readings, in the hour halfway
/// between them. Nothing for the first reading, or when the last one is
/// more than `MAX_GAP` old: then there's no saying when it was played. A
/// game not in the last reading wasn't played in the two weeks before it,
/// so its two-week minutes are all new. No game counts more minutes than
/// passed.
pub fn changes(prev: Option<&Reading>, next: &Reading) -> Vec<Change> {
    let Some(prev) = prev else { return vec![] };
    let gap = next.at - prev.at;
    if gap <= 0 || gap > MAX_GAP {
        return vec![];
    }
    let cap = (gap + 59) / 60;
    let hour = ((prev.at + next.at) / 2).div_euclid(HOUR) * HOUR;
    next.games
        .iter()
        .filter_map(|(appid, now)| {
            let minutes = match prev.games.get(appid) {
                Some(before) => now.forever - before.forever,
                None => now.two_weeks,
            }
            .min(cap);
            (minutes > 0).then_some(Change { appid: *appid, hour, minutes })
        })
        .collect()
}

/// Whether a reading is due: an hour after the last try.
pub fn due(db: &Db, now: i64) -> Result<bool> {
    let tried: Option<i64> = db.meta_get(TRIED)?.and_then(|v| v.parse().ok());
    Ok(tried.is_none_or(|t| now - t >= EVERY))
}

/// What a reading did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Polled {
    /// Games Steam says were played in the last two weeks.
    pub games: usize,
    /// Diary rows written.
    pub rows: usize,
    /// The first reading since the diary was turned on (or a long gap):
    /// it only starts the count.
    pub first: bool,
}

/// The meta key for the last reading of a Steam connection: `diary_reading`
/// before the first sync has made one, `diary_reading.<id>` after.
fn reading_key(conn: &SyncConnection) -> String {
    match conn.id {
        Some(id) => format!("{READING}.{id}"),
        None => READING.to_string(),
    }
}

/// Take a reading from each Steam account and write what changed since its
/// last one (each account's readings are its own, and count for
/// its owner and whoever plays on it). Player 1's first account uses
/// the SteamID in Settings; another needs its own.
pub async fn poll(db: &Db, cfg: &Config, clients: &Clients, now: i64) -> Result<Polled> {
    if !cfg.play_diary {
        bail!("the play diary is off");
    }
    db.meta_set(TRIED, &now.to_string())?;
    let mut total = Polled::default();
    let mut read_any = false;
    let mut failed: Option<anyhow::Error> = None;
    for conn in db.sync_connections("steam")? {
        let creds = if conn.first && conn.setting("steam_id").is_none_or(|id| Some(id) == cfg.steam_id.as_deref()) {
            cfg.steam_credentials().map(|(k, id)| (k.to_string(), id.to_string()))
        } else {
            match (&cfg.steam_api_key, conn.setting("steam_id")) {
                (Some(k), Some(id)) => Ok((k.clone(), id.to_string())),
                _ => continue,
            }
        };
        let result = async {
            let (key, id) = creds?;
            let steam = crate::steam::SteamClient::new(clients.http.clone(), Some(key), Some(id), clients.base_url.as_deref());
            steam.recently_played().await
        }
        .await;
        match result {
            Ok(played) => {
                let polled = record(db, &conn, played, now)?;
                total.games += polled.games;
                total.rows += polled.rows;
                total.first |= polled.first;
                read_any = true;
            }
            Err(e) => {
                failed.get_or_insert(e);
            }
        }
    }
    match failed {
        Some(e) => {
            db.meta_set(ERROR, &serde_json::json!({ "at": now, "error": format!("{e:#}") }).to_string())?;
            if !read_any {
                return Err(e);
            }
        }
        None => db.meta_delete(&[ERROR])?,
    }
    Ok(total)
}

/// Write what one Steam account's reading changed since its last one.
fn record(db: &Db, conn: &SyncConnection, played: Vec<crate::steam::OwnedGame>, now: i64) -> Result<Polled> {
    let reading = Reading {
        at: now,
        games: played
            .into_iter()
            .map(|g| (g.appid, Played { name: g.name, forever: g.playtime_forever, two_weeks: g.playtime_2weeks }))
            .collect(),
    };
    let key = reading_key(conn);
    let mut prev_text = db.meta_get(&key)?;
    // A reading taken before the first sync made player 1's Steam
    // connection is that connection's, so the count carries on.
    let adopt = prev_text.is_none() && conn.first && conn.id.is_some();
    if adopt {
        prev_text = db.meta_get(READING)?;
    }
    let prev: Option<Reading> = prev_text.and_then(|v| serde_json::from_str(&v).ok());
    let first = prev.as_ref().is_none_or(|p| now - p.at > MAX_GAP);
    // Readings belong to the Steam account they came from, so everyone who
    // plays on it sees them.
    let account = db.connection_account(conn, "steam")?;
    let mut rows = vec![];
    for c in changes(prev.as_ref(), &reading) {
        let name = reading.games.get(&c.appid).map(|p| p.name.clone()).unwrap_or_default();
        rows.push(NewDiaryRow {
            hour: c.hour,
            at: now,
            title_id: db.title_for_appid(c.appid as i64)?,
            appid: Some(c.appid as i64),
            name: if name.is_empty() { format!("App {}", c.appid) } else { name },
            minutes: c.minutes,
            source: "steam",
            // Before the first library sync there is no account yet, so the
            // reading stays with the account's owner.
            player: account.is_none().then_some(conn.owner),
            account,
        });
    }
    let written = db.diary_add(&rows)?;
    db.meta_set(&key, &serde_json::to_string(&reading)?)?;
    if adopt {
        db.meta_delete(&[READING])?;
    }
    Ok(Polled { games: reading.games.len(), rows: written, first })
}

/// The Play button: an exact start time, when the diary is on. Whether it
/// was written.
pub fn launch(db: &Db, cfg: &Config, title_id: i64, now: i64) -> Result<bool> {
    if !cfg.play_diary {
        return Ok(false);
    }
    let Some(g) = db.game(cfg.whose(), title_id)? else { bail!("game id {title_id} is not in your library") };
    db.diary_add(&[NewDiaryRow {
        hour: now.div_euclid(HOUR) * HOUR,
        at: now,
        title_id: Some(g.id),
        appid: g.appid.map(i64::from),
        name: g.name.clone(),
        minutes: 0,
        source: "launch",
        player: Some(cfg.whose()),
        account: None,
    }])?;
    Ok(true)
}

/// Turn the diary on or off. Off forgets the last reading, so turning it on
/// again starts the count afresh; what's recorded stays until deleted.
pub fn switch(db: &Db, p: PlayerId, on: bool) -> Result<()> {
    db.setting_set(p, "play_diary", Some(if on { "true" } else { "false" }))?;
    if !on {
        let readings: Vec<String> = db.meta_family(READING)?.into_iter().map(|(k, _)| k).collect();
        let keys: Vec<&str> = readings.iter().map(String::as_str).chain([TRIED, ERROR]).collect();
        db.meta_delete(&keys)?;
    }
    Ok(())
}

/// The diary's state, for the profile page.
#[derive(Debug, Clone, Default, Serialize, utoipa::ToSchema)]
#[schema(as = DiaryState)]
pub struct State {
    pub on: bool,
    /// Hours with play recorded.
    pub rows: usize,
    pub days: usize,
    pub launches: usize,
    /// The first hour it holds.
    pub since: Option<i64>,
    /// The last reading from Steam.
    pub last_reading: Option<i64>,
    /// The last reading's error, when it failed.
    pub error: Option<String>,
    /// Why it's on but not reading: no Steam Web API key, or schedules off.
    pub paused: Option<String>,
}

pub fn state(db: &Db, cfg: &Config) -> Result<State> {
    let counts = db.diary_counts(cfg.whose())?;
    let last_reading =
        db.meta_family(READING)?.into_iter().filter_map(|(_, v)| serde_json::from_str::<Reading>(&v).ok()).map(|r| r.at).max();
    let error = db
        .meta_get(ERROR)?
        .and_then(|v| serde_json::from_str::<serde_json::Value>(&v).ok())
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string));
    let paused = if !cfg.play_diary {
        None
    } else if cfg.steam_credentials().is_err() {
        Some("it needs your Steam Web API key and Steam ID (Settings › Accounts) to read your playtime".to_string())
    } else if !cfg.auto_sync {
        Some("schedules are off (Settings › Jobs), so nothing reads Steam by itself".to_string())
    } else {
        None
    };
    Ok(State {
        on: cfg.play_diary,
        rows: counts.hours,
        days: counts.days,
        launches: counts.launches,
        since: counts.first,
        last_reading,
        error,
        paused,
    })
}

// ---------------------------------------------------------------------------
// Habits from the diary
// ---------------------------------------------------------------------------

/// How long your sessions usually run at one part of the week.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Sessions {
    /// The median, to the quarter hour.
    pub minutes: i64,
    pub count: usize,
}

/// When multiplayer games get played.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Multiplayer {
    pub minutes: i64,
    /// Share of their minutes at the weekend.
    pub weekend_share: f64,
    /// The time of day most of them are played at.
    pub time: Option<String>,
}

/// What the diary shows about your habits.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Habits {
    /// Days with play in the window.
    pub days: usize,
    /// Minutes played in the window.
    pub minutes: i64,
    pub weeknight: Option<Sessions>,
    pub weekend: Option<Sessions>,
    /// Times of day with a fair share of your play, most first, as the
    /// `times` habit's choices.
    pub times: Vec<String>,
    pub multiplayer: Option<Multiplayer>,
}

impl Habits {
    pub fn is_empty(&self) -> bool {
        self.weeknight.is_none() && self.weekend.is_none() && self.times.is_empty() && self.multiplayer.is_none()
    }
}

/// The `times` habit's choice for a local hour.
pub fn time_of_day(hour: u32) -> &'static str {
    match hour {
        5..=11 => "mornings",
        12..=16 => "afternoons",
        17..=21 => "evenings",
        _ => "late_nights",
    }
}

fn local_hour(t: i64) -> u32 {
    chrono::DateTime::from_timestamp(t, 0).map(|d| d.with_timezone(&chrono::Local).hour()).unwrap_or(0)
}

fn quarter_hours(m: f64) -> i64 {
    (((m / 15.0).round() as i64) * 15).max(15)
}

fn median(mut v: Vec<i64>) -> f64 {
    v.sort_unstable();
    let n = v.len();
    if n % 2 == 1 { v[n / 2] as f64 } else { (v[n / 2 - 1] + v[n / 2]) as f64 / 2.0 }
}

/// Habits from diary rows: sessions are runs of hours in a row with play in
/// them, a weeknight or the weekend by when they start (`When`, Friday from
/// 5 pm is the weekend).
pub fn habits(rows: &[DiaryRow], games: &[GameView]) -> Habits {
    let mut by_hour: BTreeMap<i64, i64> = BTreeMap::new();
    let multiplayer: HashMap<i64, bool> = games
        .iter()
        .map(|g| (g.id, features(g).iter().any(|s| matches!(s, Subject::Tag(t) if MULTIPLAYER.contains(&t.as_str())))))
        .collect();
    let mut times: BTreeMap<&str, i64> = BTreeMap::new();
    let (mut mp_minutes, mut mp_weekend) = (0i64, 0i64);
    let mut mp_times: BTreeMap<&str, i64> = BTreeMap::new();
    let mut days = std::collections::HashSet::new();
    for r in rows.iter().filter(|r| r.source == "steam" && r.minutes > 0) {
        *by_hour.entry(r.hour).or_default() += r.minutes;
        let tod = time_of_day(local_hour(r.hour));
        *times.entry(tod).or_default() += r.minutes;
        days.insert(r.hour.div_euclid(DAY));
        if r.title_id.is_some_and(|id| multiplayer.get(&id).copied().unwrap_or(false)) {
            mp_minutes += r.minutes;
            if When::at(r.hour) == When::Weekend {
                mp_weekend += r.minutes;
            }
            *mp_times.entry(tod).or_default() += r.minutes;
        }
    }
    let total: i64 = by_hour.values().sum();

    // Sessions: hours in a row with play.
    let (mut weeknight, mut weekend): (Vec<i64>, Vec<i64>) = (vec![], vec![]);
    let mut session: Option<(i64, i64, i64)> = None; // (start, last hour, minutes)
    let mut close = |s: (i64, i64, i64)| match When::at(s.0) {
        When::Weeknight => weeknight.push(s.2),
        When::Weekend => weekend.push(s.2),
    };
    for (&hour, &minutes) in &by_hour {
        session = match session {
            Some((start, last, m)) if hour - last == HOUR => Some((start, hour, m + minutes)),
            Some(s) => {
                close(s);
                Some((hour, hour, minutes))
            }
            None => Some((hour, hour, minutes)),
        };
    }
    if let Some(s) = session {
        close(s);
    }
    let usual = |v: Vec<i64>| (v.len() >= MIN_SESSIONS).then(|| Sessions { count: v.len(), minutes: quarter_hours(median(v)) });

    let mut shares: Vec<(&str, i64)> = times.into_iter().collect();
    shares.sort_by_key(|(_, m)| std::cmp::Reverse(*m));
    let times = if total >= MIN_MINUTES_FOR_TIMES {
        shares.iter().filter(|(_, m)| *m as f64 >= TIME_SHARE * total as f64).map(|(t, _)| t.to_string()).collect()
    } else {
        vec![]
    };
    let multiplayer = (mp_minutes >= 60).then(|| Multiplayer {
        minutes: mp_minutes,
        weekend_share: mp_weekend as f64 / mp_minutes as f64,
        time: mp_times.into_iter().max_by_key(|(_, m)| *m).map(|(t, _)| t.to_string()),
    });
    Habits { days: days.len(), minutes: total, weeknight: usual(weeknight), weekend: usual(weekend), times, multiplayer }
}

/// The diary's habits from the last `WINDOW_DAYS`.
pub fn load_habits(db: &Db, p: PlayerId, games: &[GameView], now: i64) -> Result<Habits> {
    Ok(habits(&db.diary_rows(p, now - WINDOW_DAYS * DAY)?, games))
}

#[cfg(test)]
mod tests;
