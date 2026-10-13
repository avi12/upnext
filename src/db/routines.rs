//! Routines: named sessions on the Profile, chosen on the Pick page.

use super::{Db, PlayerId, now};
use crate::taste::routine::{Routine, RoutineBody};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// An enum as the word it's stored as ("keyboard_mouse").
fn word<T: Serialize>(v: &T) -> Result<String> {
    Ok(serde_json::to_value(v)?.as_str().unwrap_or_default().to_string())
}

/// A stored word read back; one this build doesn't know reads as unset.
fn parse<T: DeserializeOwned>(s: Option<String>) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(s?)).ok()
}

/// A stored JSON list read back, dropping what this build doesn't know.
fn list<T: DeserializeOwned>(s: String) -> Vec<T> {
    let words: Vec<String> = serde_json::from_str(&s).unwrap_or_default();
    words.into_iter().filter_map(|w| parse(Some(w))).collect()
}

const COLUMNS: &str = "id, name, days, times, who, place, input, minutes, note";

fn routine(r: &Row) -> rusqlite::Result<Routine> {
    Ok(Routine {
        id: r.get(0)?,
        name: r.get(1)?,
        days: list(r.get(2)?),
        times: list(r.get(3)?),
        who: parse(r.get(4)?),
        place: parse(r.get(5)?),
        input: parse(r.get(6)?),
        minutes: r.get(7)?,
        note: r.get(8)?,
    })
}

/// The body's columns after the name: days, times, who, place, input, minutes, note.
type Columns = (String, String, Option<String>, Option<String>, Option<String>, Option<i64>, Option<String>);

fn columns(b: &RoutineBody) -> Result<Columns> {
    let words = |v: &[_]| -> Result<String> { Ok(serde_json::to_string(v)?) };
    Ok((
        words(&b.days.iter().map(word).collect::<Result<Vec<_>>>()?)?,
        words(&b.times.iter().map(word).collect::<Result<Vec<_>>>()?)?,
        b.who.as_ref().map(word).transpose()?,
        b.place.as_ref().map(word).transpose()?,
        b.input.as_ref().map(word).transpose()?,
        b.minutes,
        b.note.clone(),
    ))
}

fn one(c: &Connection, p: PlayerId, id: i64) -> Result<Option<Routine>> {
    Ok(c.query_row(&format!("SELECT {COLUMNS} FROM routines WHERE id=?1 AND player_id=?2"), params![id, p], routine).optional()?)
}

impl Db {
    /// Player `p`'s routines, in the order they were made.
    pub fn routines(&self, p: PlayerId) -> Result<Vec<Routine>> {
        self.with(|c| {
            let mut st = c.prepare(&format!("SELECT {COLUMNS} FROM routines WHERE player_id=?1 ORDER BY id"))?;
            let rows = st.query_map(params![p], routine)?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    pub fn routine(&self, p: PlayerId, id: i64) -> Result<Option<Routine>> {
        self.with(|c| one(c, p, id))
    }

    /// Save a new routine, checked already (`routine::check`).
    pub fn add_routine(&self, p: PlayerId, b: &RoutineBody) -> Result<Routine> {
        let (days, times, who, place, input, minutes, note) = columns(b)?;
        self.with(|c| {
            let t = now();
            c.execute(
                "INSERT INTO routines(name, days, times, who, place, input, minutes, note, created_at, updated_at, player_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?10)",
                params![b.name, days, times, who, place, input, minutes, note, t, p],
            )?;
            one(c, p, c.last_insert_rowid())?.ok_or_else(|| anyhow::anyhow!("the routine wasn't saved"))
        })
    }

    /// Replace a routine with a checked body; None when `p` has no such routine.
    pub fn set_routine(&self, p: PlayerId, id: i64, b: &RoutineBody) -> Result<Option<Routine>> {
        let (days, times, who, place, input, minutes, note) = columns(b)?;
        self.with(|c| {
            let n = c.execute(
                "UPDATE routines SET name=?2, days=?3, times=?4, who=?5, place=?6, input=?7, minutes=?8, note=?9, updated_at=?10
                 WHERE id=?1 AND player_id=?11",
                params![id, b.name, days, times, who, place, input, minutes, note, now(), p],
            )?;
            if n == 0 { Ok(None) } else { one(c, p, id) }
        })
    }

    /// Delete a routine; false when there was none. Lineups keep their own
    /// copy of the routine they were picked for.
    pub fn delete_routine(&self, p: PlayerId, id: i64) -> Result<bool> {
        self.with(|c| Ok(c.execute("DELETE FROM routines WHERE id=?1 AND player_id=?2", params![id, p])? > 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: PlayerId = PlayerId::FIRST;
    use crate::taste::routine::{Day, Input, Place, Time, Who};

    #[test]
    fn routines_are_kept_changed_and_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("upnext.db"), 120).unwrap();
        let body = RoutineBody {
            name: "Friday with friends".into(),
            days: vec![Day::Fri],
            times: vec![Time::Evenings, Time::LateNights],
            who: Some(Who::Friends),
            place: Some(Place::Desk),
            input: Some(Input::KeyboardMouse),
            minutes: Some(120),
            note: Some("Discord on".into()),
        };
        let r = db.add_routine(P, &body).unwrap();
        assert_eq!(db.routines(P).unwrap(), vec![r.clone()]);
        assert_eq!(
            (r.name.as_str(), r.days.as_slice(), r.input, r.minutes),
            ("Friday with friends", &[Day::Fri][..], Some(Input::KeyboardMouse), Some(120))
        );
        let stored: (String, String) =
            db.with(|c| Ok(c.query_row("SELECT days, input FROM routines", [], |x| Ok((x.get(0)?, x.get(1)?)))?)).unwrap();
        assert_eq!(stored, ("[\"fri\"]".to_string(), "keyboard_mouse".to_string()), "words, not numbers");

        let couch = RoutineBody { name: "Couch".into(), place: Some(Place::Couch), ..Default::default() };
        let changed = db.set_routine(P, r.id, &couch).unwrap().unwrap();
        assert_eq!((changed.id, changed.days.len(), changed.who, changed.place), (r.id, 0, None, Some(Place::Couch)));
        assert_eq!(db.set_routine(P, r.id + 1, &couch).unwrap(), None);

        // A word a later build wrote reads as unset rather than failing.
        db.with(|c| Ok(c.execute("UPDATE routines SET who='robots', days='[\"fri\",\"someday\"]'", [])?)).unwrap();
        let read = db.routine(P, r.id).unwrap().unwrap();
        assert_eq!((read.who, read.days), (None, vec![Day::Fri]));

        assert!(db.delete_routine(P, r.id).unwrap());
        assert!(!db.delete_routine(P, r.id).unwrap());
        assert!(db.routines(P).unwrap().is_empty());
    }
}
