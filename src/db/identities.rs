//! The identity provider's accounts linked to players.
//! An account at the provider is its issuer and subject (`iss`, `sub`),
//! which only the provider gives out: players are never matched by name or
//! email, so an account that calls itself "Sam" doesn't become Sam.

use super::{Db, PlayerId, now};
use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension, params};

/// The player an account at the provider signs in as, when one is linked.
fn linked(c: &Connection, issuer: &str, subject: &str) -> Result<Option<PlayerId>> {
    Ok(c.query_row("SELECT id FROM players WHERE oidc_issuer=?1 AND oidc_subject=?2", params![issuer, subject], |r| r.get(0))
        .optional()?
        .map(PlayerId))
}

/// A name nobody has yet: `wanted`, else "Player N", with " 2", " 3" and so
/// on added while it's taken.
fn free_name(c: &Connection, wanted: Option<&str>) -> Result<String> {
    let taken = |name: &str| -> Result<bool> {
        Ok(c.query_row("SELECT 1 FROM players WHERE name=?1", params![name], |_| Ok(())).optional()?.is_some())
    };
    let wanted = wanted.map(super::clean_name).filter(|n| !n.is_empty()).map(|n| n.chars().take(36).collect::<String>());
    let base = match wanted {
        Some(n) => n,
        None => {
            let n: i64 = c.query_row("SELECT COUNT(*) FROM players", [], |r| r.get(0))?;
            format!("Player {}", n + 1)
        }
    };
    if !taken(&base)? {
        return Ok(base);
    }
    for n in 2.. {
        let name = format!("{base} {n}");
        if !taken(&name)? {
            return Ok(name);
        }
    }
    unreachable!("some number is free")
}

impl Db {
    /// The player an account at the provider signs in as, when one is linked.
    pub fn oidc_player(&self, issuer: &str, subject: &str) -> Result<Option<PlayerId>> {
        self.with(|c| linked(c, issuer, subject))
    }

    /// Link an account at the provider to player `p`, in place of any they
    /// had. Refused when another player has it.
    pub fn link_oidc(&self, p: PlayerId, issuer: &str, subject: &str) -> Result<()> {
        self.with(|c| {
            if let Some(other) = linked(c, issuer, subject)?
                && other != p
            {
                bail!("that account already signs in as another player");
            }
            if c.execute("UPDATE players SET oidc_issuer=?2, oidc_subject=?3 WHERE id=?1", params![p, issuer, subject])? == 0 {
                bail!("no player {p}");
            }
            Ok(())
        })
    }

    /// Forget the account at the provider linked to player `p`.
    pub fn unlink_oidc(&self, p: PlayerId) -> Result<()> {
        self.with(|c| {
            c.execute("UPDATE players SET oidc_issuer=NULL, oidc_subject=NULL WHERE id=?1", params![p])?;
            Ok(())
        })
    }

    /// A new player for an account at the provider that signed in for the
    /// first time (auto-provisioning): named `name` when it's free, with a
    /// number added when it isn't, else "Player N". Made and linked in one
    /// step, so two first sign-ins of one account at once make one player.
    pub fn provision_oidc_player(&self, name: Option<&str>, issuer: &str, subject: &str) -> Result<(PlayerId, bool)> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            if let Some(p) = linked(&tx, issuer, subject)? {
                return Ok((p, false));
            }
            let name = free_name(&tx, name)?;
            let n: i64 = tx.query_row("SELECT COUNT(*) FROM players", [], |r| r.get(0))?;
            tx.execute(
                "INSERT INTO players(name, colour, created_at, oidc_issuer, oidc_subject) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![name, super::players::COLOURS[n as usize % super::players::COLOURS.len()], now(), issuer, subject],
            )?;
            let p = PlayerId(tx.last_insert_rowid());
            tx.commit()?;
            Ok((p, true))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ISS: &str = "https://sso.den.example.org";

    #[test]
    fn provisioning_names_and_links_once() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("upnext.db"), 120).unwrap();
        let first = db.players().unwrap()[0].name.clone();
        let (sam, made) = db.provision_oidc_player(Some("Sam"), ISS, "u-1").unwrap();
        assert!(made);
        assert_eq!(db.oidc_player(ISS, "u-1").unwrap(), Some(sam));
        assert_eq!(db.provision_oidc_player(Some("Someone else"), ISS, "u-1").unwrap(), (sam, false), "a known subject");
        // The same name from another account gets a number, never Sam's player.
        let (other, _) = db.provision_oidc_player(Some("sam"), ISS, "u-2").unwrap();
        assert_ne!(other, sam);
        assert_eq!(db.player(other).unwrap().unwrap().name, "sam 2");
        let (_, _) = db.provision_oidc_player(Some(&first), ISS, "u-3").unwrap();
        let (nameless, _) = db.provision_oidc_player(None, ISS, "u-4").unwrap();
        assert_eq!(db.player(nameless).unwrap().unwrap().name, "Player 5");
        assert_eq!(db.oidc_player("https://other.example.org", "u-1").unwrap(), None, "another issuer");
    }

    #[test]
    fn linking_is_one_account_one_player() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("upnext.db"), 120).unwrap();
        let alex = db.add_player("Alex").unwrap();
        db.link_oidc(PlayerId::FIRST, ISS, "u-1").unwrap();
        assert!(db.link_oidc(alex, ISS, "u-1").is_err(), "someone else's");
        assert_eq!(db.player(PlayerId::FIRST).unwrap().unwrap().oidc_issuer.as_deref(), Some(ISS));
        db.unlink_oidc(PlayerId::FIRST).unwrap();
        assert_eq!(db.oidc_player(ISS, "u-1").unwrap(), None);
        db.link_oidc(alex, ISS, "u-1").unwrap();
        assert_eq!(db.oidc_player(ISS, "u-1").unwrap(), Some(alex));
    }
}
