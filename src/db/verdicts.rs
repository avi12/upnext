//! A player's own backlog verdicts (`my_verdicts`), and what the
//! player keeps for sending them: their key and the consent they gave.
//!
//! The key and the consent are rows of `player_settings` that aren't in
//! `players::PLAYER_KEYS`, so the settings API never reads or writes them:
//! only the functions here do, and the key never leaves the install except
//! in the `Authorization` header to the verdicts service.

use super::{Db, PlayerId, now};
use crate::verdicts::verdict::Verdict;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

/// The player's key for the verdicts service (`upnext_v_…`), made the first
/// time they send or report. In backups; left out of a copy without keys.
pub const KEY_SETTING: &str = "verdicts_key";
/// The consent text's version the player accepted.
pub const CONSENT_SETTING: &str = "verdicts_consent";
/// The day they accepted it, `YYYY-MM-DD`.
pub const CONSENT_AT_SETTING: &str = "verdicts_consent_at";

/// Where one of a player's verdicts stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum VerdictState {
    /// Saved on this computer, never sent.
    Draft,
    /// Sent when the service takes it: it was busy or out of reach.
    Queued,
    /// Taken by the service, and published with its next snapshot.
    Sent,
    /// Taken, and held by the service until a moderator looks or a week
    /// passes (the game was getting a rush of verdicts).
    Held,
    /// Taken down by the player.
    Withdrawn,
    /// Taken down by the service's moderators.
    Removed,
}

impl VerdictState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Queued => "queued",
            Self::Sent => "sent",
            Self::Held => "held",
            Self::Withdrawn => "withdrawn",
            Self::Removed => "removed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Draft, Self::Queued, Self::Sent, Self::Held, Self::Withdrawn, Self::Removed].into_iter().find(|v| v.as_str() == s)
    }

    /// The service has it, live or held.
    pub fn is_out(self) -> bool {
        matches!(self, Self::Sent | Self::Held)
    }
}

/// One of a player's verdicts as this install keeps it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MyVerdict {
    pub title_id: i64,
    /// The ids and answers as last saved or sent. `client` is this build's.
    pub verdict: Verdict,
    /// The service's id for it, once sent: the id the snapshot publishes it under.
    pub remote_id: Option<String>,
    pub state: VerdictState,
    /// When a queued verdict is tried again (Unix seconds).
    pub retry_at: Option<i64>,
    /// Why the last try didn't go through.
    pub error: Option<String>,
    /// When the service last took it.
    pub sent_at: Option<i64>,
    pub updated_at: i64,
}

impl MyVerdict {
    /// A new row for a title, not sent.
    pub fn draft(title_id: i64, verdict: Verdict) -> Self {
        MyVerdict {
            title_id,
            verdict,
            remote_id: None,
            state: VerdictState::Draft,
            retry_at: None,
            error: None,
            sent_at: None,
            updated_at: now(),
        }
    }
}

const COLUMNS: &str = "title_id, steam_appid, gog_id, outcome, worth, hours, clicked, sessions, limited_time, factors, stop_point, \
     stop_note, stopped_for, text, vocab, licence, consent, remote_id, state, retry_at, error, sent_at, updated_at";

fn list(text: String) -> Vec<String> {
    serde_json::from_str(&text).unwrap_or_default()
}

fn read(r: &Row) -> rusqlite::Result<MyVerdict> {
    let verdict = Verdict {
        steam_appid: r.get(1)?,
        gog_id: r.get(2)?,
        outcome: r.get(3)?,
        worth: r.get(4)?,
        hours: r.get(5)?,
        clicked: r.get(6)?,
        sessions: r.get(7)?,
        limited_time: r.get(8)?,
        factors: list(r.get(9)?),
        stop_point: r.get(10)?,
        stop_note: r.get(11)?,
        stopped_for: list(r.get(12)?),
        text: r.get(13)?,
        vocab: r.get(14)?,
        licence: r.get(15)?,
        consent: r.get(16)?,
        client: env!("CARGO_PKG_VERSION").into(),
    };
    let state: String = r.get(18)?;
    Ok(MyVerdict {
        title_id: r.get(0)?,
        verdict,
        remote_id: r.get(17)?,
        state: VerdictState::parse(&state).unwrap_or(VerdictState::Draft),
        retry_at: r.get(19)?,
        error: r.get(20)?,
        sent_at: r.get(21)?,
        updated_at: r.get(22)?,
    })
}

fn setting(c: &Connection, p: PlayerId, key: &str) -> Result<Option<String>> {
    Ok(c.query_row("SELECT value FROM player_settings WHERE player_id=?1 AND key=?2", params![p, key], |r| r.get(0)).optional()?)
}

fn set_setting(c: &Connection, p: PlayerId, key: &str, value: Option<&str>) -> Result<()> {
    match value {
        Some(v) => c.execute(
            "INSERT INTO player_settings(player_id, key, value) VALUES (?1, ?2, ?3)
             ON CONFLICT(player_id, key) DO UPDATE SET value=excluded.value",
            params![p, key, v],
        )?,
        None => c.execute("DELETE FROM player_settings WHERE player_id=?1 AND key=?2", params![p, key])?,
    };
    Ok(())
}

impl Db {
    /// A player's verdict about a title, if they saved or sent one.
    pub fn my_verdict(&self, p: PlayerId, title_id: i64) -> Result<Option<MyVerdict>> {
        self.with(|c| {
            Ok(c.query_row(&format!("SELECT {COLUMNS} FROM my_verdicts WHERE player_id=?1 AND title_id=?2"), params![p, title_id], read)
                .optional()?)
        })
    }

    /// Every verdict of a player with the title's name, the newest change first.
    pub fn my_verdicts(&self, p: PlayerId) -> Result<Vec<(MyVerdict, String)>> {
        self.with(|c| {
            let cols = COLUMNS.split(", ").map(|col| format!("v.{col}")).collect::<Vec<_>>().join(", ");
            let mut st = c.prepare(&format!(
                "SELECT {cols}, COALESCE(t.name, '') FROM my_verdicts v LEFT JOIN titles t ON t.id = v.title_id
                 WHERE v.player_id=?1 ORDER BY v.updated_at DESC, v.title_id"
            ))?;
            let rows = st.query_map(params![p], |r| Ok((read(r)?, r.get::<_, String>(23)?)))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Save a player's verdict as it is now, replacing what was kept.
    pub fn save_my_verdict(&self, p: PlayerId, m: &MyVerdict) -> Result<()> {
        let v = &m.verdict;
        self.with(|c| {
            c.execute(
                &format!(
                    "INSERT OR REPLACE INTO my_verdicts(player_id, {COLUMNS})
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)"
                ),
                params![
                    p,
                    m.title_id,
                    v.steam_appid,
                    v.gog_id,
                    v.outcome,
                    v.worth,
                    v.hours,
                    v.clicked,
                    v.sessions,
                    v.limited_time,
                    serde_json::to_string(&v.factors)?,
                    v.stop_point,
                    v.stop_note,
                    serde_json::to_string(&v.stopped_for)?,
                    v.text,
                    v.vocab,
                    v.licence,
                    v.consent,
                    m.remote_id,
                    m.state.as_str(),
                    m.retry_at,
                    m.error,
                    m.sent_at,
                    m.updated_at,
                ],
            )?;
            Ok(())
        })
    }

    /// Forget a player's verdict about a title. True when there was one.
    pub fn delete_my_verdict(&self, p: PlayerId, title_id: i64) -> Result<bool> {
        self.with(|c| Ok(c.execute("DELETE FROM my_verdicts WHERE player_id=?1 AND title_id=?2", params![p, title_id])? > 0))
    }

    /// Queued verdicts whose time to try again has come, every player's.
    pub fn queued_verdicts(&self, at: i64) -> Result<Vec<(PlayerId, MyVerdict)>> {
        self.with(|c| {
            let mut st = c.prepare(&format!(
                "SELECT {COLUMNS}, player_id FROM my_verdicts WHERE state='queued' AND COALESCE(retry_at, 0) <= ?1 ORDER BY retry_at, title_id"
            ))?;
            let rows = st.query_map(params![at], |r| Ok((PlayerId(r.get(23)?), read(r)?)))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// The player's key for the verdicts service, if they have one.
    pub fn verdicts_key(&self, p: PlayerId) -> Result<Option<String>> {
        self.with(|c| setting(c, p, KEY_SETTING))
    }

    /// Keep a key for the player, or forget theirs (None).
    pub fn set_verdicts_key(&self, p: PlayerId, key: Option<&str>) -> Result<()> {
        self.with(|c| set_setting(c, p, KEY_SETTING, key))
    }

    /// The consent version the player accepted and the day they did.
    pub fn verdicts_consent(&self, p: PlayerId) -> Result<Option<(i64, Option<String>)>> {
        self.with(|c| {
            let Some(version) = setting(c, p, CONSENT_SETTING)?.and_then(|v| v.parse().ok()) else { return Ok(None) };
            Ok(Some((version, setting(c, p, CONSENT_AT_SETTING)?)))
        })
    }

    /// Record that the player accepted a consent version on a day.
    pub fn set_verdicts_consent(&self, p: PlayerId, version: i64, day: &str) -> Result<()> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            set_setting(&tx, p, CONSENT_SETTING, Some(&version.to_string()))?;
            set_setting(&tx, p, CONSENT_AT_SETTING, Some(day))?;
            tx.commit()?;
            Ok(())
        })
    }

    /// The title a verdict from the service is about: the one with its
    /// Steam appid, else the one with a GOG copy of its GOG id.
    pub fn title_for_verdict(&self, steam_appid: Option<u32>, gog_id: Option<&str>) -> Result<Option<i64>> {
        self.with(|c| {
            if let Some(appid) = steam_appid
                && let Some(id) = c.query_row("SELECT id FROM titles WHERE appid=?1", params![appid], |r| r.get(0)).optional()?
            {
                return Ok(Some(id));
            }
            let Some(gog) = gog_id else { return Ok(None) };
            Ok(c.query_row(
                "SELECT title_id FROM games WHERE source='gog' AND source_id=?1 AND removed=0 ORDER BY id LIMIT 1",
                params![gog],
                |r| r.get(0),
            )
            .optional()?)
        })
    }

    /// Every player's verdict key, for keeping them across a restore of a
    /// copy without keys.
    pub fn verdicts_keys(&self) -> Result<Vec<(PlayerId, String)>> {
        self.with(|c| {
            let mut st = c.prepare("SELECT player_id, value FROM player_settings WHERE key=?1 ORDER BY player_id")?;
            let rows = st.query_map(params![KEY_SETTING], |r| Ok((PlayerId(r.get(0)?), r.get(1)?)))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }
}

/// Keep `keys` in a database being restored, for each player it has who has
/// no key of their own there. The players whose key was kept.
pub(crate) fn keep_verdicts_keys(c: &Connection, keys: &[(PlayerId, String)]) -> Result<Vec<PlayerId>> {
    let mut kept = vec![];
    for (p, key) in keys {
        let exists: bool = c.query_row("SELECT COUNT(*) FROM players WHERE id=?1", params![p], |r| r.get::<_, i64>(0))? > 0;
        if exists && setting(c, *p, KEY_SETTING)?.is_none() {
            set_setting(c, *p, KEY_SETTING, Some(key))?;
            kept.push(*p);
        }
    }
    Ok(kept)
}

/// Leave every player's verdict key out of a copy without keys.
pub(crate) fn strip_verdicts_keys(c: &Connection) -> Result<()> {
    c.execute("DELETE FROM player_settings WHERE key=?1", params![KEY_SETTING])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.db"), 60).unwrap();
        db.with(|c| {
            c.execute_batch(
                "INSERT INTO titles(id, key, name, appid) VALUES (1, 'appid:620', 'Portal 2', 620), (2, 'name:hades', 'Hades', NULL);",
            )?;
            Ok(())
        })
        .unwrap();
        (dir, db)
    }

    #[test]
    fn a_verdict_is_kept_read_back_and_replaced_whole() {
        let (_dir, db) = db();
        let p = PlayerId::FIRST;
        assert_eq!(db.my_verdict(p, 1).unwrap(), None);
        let mut v = Verdict::new(Some(620), None);
        v.outcome = Some("finished".into());
        v.worth = Some("yes".into());
        v.limited_time = Some("mostly".into());
        v.factors = vec!["save_anywhere".into(), "recap".into()];
        v.text = Some("Short chapters.".into());
        let mut m = MyVerdict::draft(1, v);
        db.save_my_verdict(p, &m).unwrap();
        assert_eq!(db.my_verdict(p, 1).unwrap().as_ref(), Some(&m));

        m.state = VerdictState::Sent;
        m.remote_id = Some("a".repeat(26));
        m.sent_at = Some(5);
        m.verdict.text = None;
        db.save_my_verdict(p, &m).unwrap();
        assert_eq!(db.my_verdict(p, 1).unwrap().as_ref(), Some(&m));
        assert_eq!(db.my_verdicts(p).unwrap(), vec![(m.clone(), "Portal 2".to_string())]);
        assert!(db.my_verdicts(PlayerId(2)).unwrap().is_empty(), "another player's are their own");

        assert!(db.delete_my_verdict(p, 1).unwrap());
        assert!(!db.delete_my_verdict(p, 1).unwrap());
    }

    #[test]
    fn queued_verdicts_come_due_at_their_time() {
        let (_dir, db) = db();
        let mut m = MyVerdict::draft(2, Verdict::new(None, Some("1207664663".into())));
        db.save_my_verdict(PlayerId::FIRST, &m).unwrap();
        assert!(db.queued_verdicts(i64::MAX).unwrap().is_empty(), "a draft waits for the player");
        m.state = VerdictState::Queued;
        m.retry_at = Some(100);
        db.save_my_verdict(PlayerId::FIRST, &m).unwrap();
        assert!(db.queued_verdicts(99).unwrap().is_empty());
        assert_eq!(db.queued_verdicts(100).unwrap(), vec![(PlayerId::FIRST, m)]);
    }

    #[test]
    fn the_key_and_consent_are_the_players_and_out_of_their_settings() {
        let (_dir, db) = db();
        let p = PlayerId::FIRST;
        assert_eq!((db.verdicts_key(p).unwrap(), db.verdicts_consent(p).unwrap()), (None, None));
        db.set_verdicts_key(p, Some("upnext_v_00")).unwrap();
        db.set_verdicts_consent(p, 1, "2026-10-10").unwrap();
        assert_eq!(db.verdicts_key(p).unwrap().as_deref(), Some("upnext_v_00"));
        assert_eq!(db.verdicts_consent(p).unwrap(), Some((1, Some("2026-10-10".into()))));
        let settings = db.settings_all(p).unwrap();
        assert!(![KEY_SETTING, CONSENT_SETTING, CONSENT_AT_SETTING].iter().any(|k| settings.contains_key(*k)), "{settings:?}");
        assert_eq!(db.verdicts_keys().unwrap(), vec![(p, "upnext_v_00".to_string())]);
        db.set_verdicts_key(p, None).unwrap();
        assert_eq!(db.verdicts_key(p).unwrap(), None);
    }

    #[test]
    fn the_table_refuses_an_unknown_state() {
        let (_dir, db) = db();
        let bad = db.with(|c| {
            Ok(c.execute(
                "INSERT INTO my_verdicts(player_id, title_id, vocab, licence, consent, state, updated_at) VALUES (1, 1, 1, 'CC0-1.0', 1, 'live', 1)",
                [],
            )?)
        });
        assert!(bad.is_err());
    }
}

#[cfg(test)]
mod backup_tests {
    use super::*;

    /// A copy without keys has no verdicts key; restoring one onto an
    /// install keeps the install's key for a player the copy has with none.
    #[test]
    fn a_copy_without_keys_drops_the_key_and_a_restore_keeps_this_installs() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.db"), 60).unwrap();
        db.set_verdicts_key(PlayerId::FIRST, Some("upnext_v_11")).unwrap();
        let keys = db.verdicts_keys().unwrap();
        let copy = dir.path().join("copy.db");
        db.with(|c| crate::backups::keyless_copy_of(c, &copy)).unwrap();
        let c = Connection::open(&copy).unwrap();
        assert_eq!(setting(&c, PlayerId::FIRST, KEY_SETTING).unwrap(), None);
        assert_eq!(keep_verdicts_keys(&c, &keys).unwrap(), vec![PlayerId::FIRST]);
        assert_eq!(setting(&c, PlayerId::FIRST, KEY_SETTING).unwrap().as_deref(), Some("upnext_v_11"));
        assert!(keep_verdicts_keys(&c, &[(PlayerId(7), "upnext_v_22".into())]).unwrap().is_empty(), "no such player");
        set_setting(&c, PlayerId::FIRST, KEY_SETTING, Some("upnext_v_33")).unwrap();
        assert!(keep_verdicts_keys(&c, &keys).unwrap().is_empty(), "the copy's own key wins");
    }
}
