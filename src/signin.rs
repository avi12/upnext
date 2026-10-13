//! Where a store sign-in is kept: GOG's and Epic's tokens live on
//! their connection in the database (`store_connections.secret`), so each
//! player can link an account of their own and a copy made without keys
//! leaves them out. Before that each was a file next to the database
//! (`gog_token.json`, `epic_token.json`); the first time player 1's sign-in
//! is read, such a file moves onto their connection and is deleted.

use crate::db::PlayerId;
use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::path::PathBuf;

/// A sign-in's stored text (the token's JSON), wherever it is kept.
pub trait SignInStore: Send + Sync {
    fn load(&self) -> Option<String>;
    fn save(&self, text: &str) -> Result<()>;
    fn clear(&self) -> Result<()>;
}

/// A file of its own, readable only by you. The clients' own tests use it.
#[cfg(test)]
pub struct FileSignIn(PathBuf);

#[cfg(test)]
impl FileSignIn {
    pub fn new(path: PathBuf) -> Self {
        if path.exists() {
            crate::http::make_private(&path);
        }
        Self(path)
    }
}

#[cfg(test)]
impl SignInStore for FileSignIn {
    fn load(&self) -> Option<String> {
        std::fs::read_to_string(&self.0).ok()
    }

    fn save(&self, text: &str) -> Result<()> {
        crate::http::write_private(&self.0, text.as_bytes()).with_context(|| format!("writing {}", self.0.display()))
    }

    fn clear(&self) -> Result<()> {
        if self.0.exists() {
            std::fs::remove_file(&self.0)?;
        }
        Ok(())
    }
}

/// Whose sign-in it is.
#[derive(Debug, Clone, Copy)]
enum At {
    /// The player's first connection of the kind, made by the first save.
    Own(PlayerId),
    /// One connection, as a sync of it reads it.
    Connection(i64),
}

/// A connection's `secret` in the database at `db`. Each call opens the
/// database on its own, so a client made before the database is open (or
/// one that outlives the request that made it) still reads the current
/// value; a database that isn't there yet is never created.
pub struct DbSignIn {
    db: PathBuf,
    kind: &'static str,
    at: At,
    /// The file the sign-in was kept in before, moved in on first read.
    legacy: Option<PathBuf>,
}

impl DbSignIn {
    /// `player`'s sign-in of `kind` (gog, epic): their first connection of
    /// that kind. `legacy` is the file it was kept in before, if any.
    pub fn own(db: PathBuf, kind: &'static str, player: PlayerId, legacy: Option<PathBuf>) -> Self {
        Self { db, kind, at: At::Own(player), legacy }
    }

    /// Connection `id`'s sign-in.
    pub fn connection(db: PathBuf, kind: &'static str, id: i64) -> Self {
        Self { db, kind, at: At::Connection(id), legacy: None }
    }

    fn open(&self) -> Result<Connection> {
        let c = Connection::open_with_flags(&self.db, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX)
            .with_context(|| format!("opening {}", self.db.display()))?;
        c.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(c)
    }

    /// The connection it is kept on; with `make`, the player's is made when
    /// they have none.
    fn target(&self, c: &Connection, make: bool) -> Result<Option<i64>> {
        match self.at {
            At::Connection(id) => Ok(Some(id)),
            At::Own(p) => {
                let found: Option<i64> = c
                    .query_row(
                        "SELECT id FROM store_connections WHERE owner_id=?1 AND kind=?2 ORDER BY id LIMIT 1",
                        params![p, self.kind],
                        |r| r.get(0),
                    )
                    .optional()?;
                if found.is_some() || !make {
                    return Ok(found);
                }
                c.execute(
                    "INSERT INTO store_connections(kind, owner_id, created_at) VALUES (?1, ?2, ?3)",
                    params![self.kind, p, crate::db::now()],
                )?;
                Ok(Some(c.last_insert_rowid()))
            }
        }
    }

    fn stored(&self) -> Result<Option<String>> {
        let c = self.open()?;
        let Some(id) = self.target(&c, false)? else { return Ok(None) };
        Ok(c.query_row("SELECT secret FROM store_connections WHERE id=?1", params![id], |r| r.get::<_, Option<String>>(0))
            .optional()?
            .flatten()
            .filter(|s| !s.trim().is_empty()))
    }

    fn legacy_text(&self) -> Option<(PathBuf, String)> {
        let path = self.legacy.clone()?;
        let text = std::fs::read_to_string(&path).ok()?;
        (!text.trim().is_empty()).then_some((path, text))
    }
}

impl SignInStore for DbSignIn {
    fn load(&self) -> Option<String> {
        match self.stored() {
            Ok(Some(text)) => Some(text),
            // Nothing on the connection yet: a sign-in kept in the old file
            // moves onto it.
            Ok(None) => {
                let (path, text) = self.legacy_text()?;
                match self.save(&text) {
                    Ok(()) => {
                        if let Err(e) = std::fs::remove_file(&path) {
                            tracing::warn!("the {} sign-in moved into the database, but {} wasn't deleted: {e}", self.kind, path.display());
                        }
                    }
                    Err(e) => tracing::warn!("the {} sign-in in {} wasn't moved into the database: {e:#}", self.kind, path.display()),
                }
                Some(text)
            }
            // A database that can't be read yet (not made, or before its
            // upgrade) still has the old file.
            Err(_) => self.legacy_text().map(|(_, text)| text),
        }
    }

    fn save(&self, text: &str) -> Result<()> {
        let c = self.open()?;
        let id = self.target(&c, true)?.context("no connection to keep the sign-in on")?;
        c.execute("UPDATE store_connections SET secret=?2 WHERE id=?1", params![id, text])?;
        Ok(())
    }

    fn clear(&self) -> Result<()> {
        if let Ok(c) = self.open()
            && let Some(id) = self.target(&c, false)?
        {
            c.execute("UPDATE store_connections SET secret=NULL WHERE id=?1", params![id])?;
        }
        // An old file left behind would come back as the sign-in.
        if let Some(path) = &self.legacy
            && path.exists()
        {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn secrets(db: &std::path::Path) -> Vec<(String, i64, Option<String>)> {
        let c = Connection::open(db).unwrap();
        let mut st = c.prepare("SELECT kind, owner_id, secret FROM store_connections ORDER BY id").unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap()
    }

    #[test]
    fn an_old_sign_in_file_moves_onto_player_ones_connection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upnext.db");
        let file = dir.path().join("gog_token.json");
        std::fs::write(&file, r#"{"access_token":"a"}"#).unwrap();
        // Before the database exists the file is still read, and kept.
        let own = DbSignIn::own(path.clone(), "gog", PlayerId::FIRST, Some(file.clone()));
        assert_eq!(own.load().as_deref(), Some(r#"{"access_token":"a"}"#));
        assert!(file.exists());
        drop(Db::open(&path, 120).unwrap());
        assert_eq!(own.load().as_deref(), Some(r#"{"access_token":"a"}"#));
        assert!(!file.exists(), "moved, so it can't come back after a sign-out");
        assert_eq!(secrets(&path), [("gog".to_string(), 1, Some(r#"{"access_token":"a"}"#.to_string()))]);
        own.clear().unwrap();
        assert_eq!(own.load(), None);
        assert_eq!(secrets(&path)[0].2, None);
    }

    #[test]
    fn each_connection_keeps_its_own_sign_in() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upnext.db");
        let db = Db::open(&path, 120).unwrap();
        let sam = db.add_player("Sam").unwrap();
        DbSignIn::own(path.clone(), "gog", PlayerId::FIRST, None).save("one").unwrap();
        DbSignIn::own(path.clone(), "gog", sam, None).save("two").unwrap();
        let all = secrets(&path);
        assert_eq!(all.iter().map(|(_, o, s)| (*o, s.clone().unwrap())).collect::<Vec<_>>(), [(1, "one".into()), (sam.0, "two".into())]);
        assert_eq!(DbSignIn::connection(path.clone(), "gog", 2).load().as_deref(), Some("two"));
        assert_eq!(DbSignIn::own(path.clone(), "epic", sam, None).load(), None, "an Epic sign-in is another connection");
    }
}
