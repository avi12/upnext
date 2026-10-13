//! API tokens: what lets another device or a script use the API.
//! Only a token's SHA-256 is stored; the token itself is shown once, when
//! it's made, and never again.

use super::{Db, PlayerId, now};
use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use sha2::{Digest, Sha256};

/// Every token starts with this, so people and secret scanners know it.
pub const TOKEN_PREFIX: &str = "upnext_";
/// Characters after the prefix: about 238 bits of randomness.
const TOKEN_CHARS: usize = 40;
/// How often a token's last use is written, at most.
const LAST_USED_EVERY: i64 = 60;

/// What a token may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Look, change nothing: GET and HEAD requests, but not the export.
    Read,
    /// Everything the UI can do, picks (which cost AI) and settings included.
    Full,
}

impl Scope {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "read" => Some(Self::Read),
            "full" => Some(Self::Full),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Full => "full",
        }
    }
}

/// A token as listed: never the token itself.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ApiToken {
    pub id: i64,
    pub name: String,
    pub scope: Scope,
    /// "token" (made in Settings or with `upnext token create`), "device"
    /// (a paired browser), or "oidc" (a browser signed in with the identity
    /// provider).
    pub kind: String,
    /// The first characters of the token, to tell tokens apart ("upnext_Ab12").
    pub hint: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
    /// The player it is bound to: it always acts for them. Null
    /// for a token that uses the `UpNext-Player` header or the default player.
    #[schema(value_type = Option<i64>)]
    pub player_id: Option<super::PlayerId>,
}

/// A token just made: the only time the token is seen.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct NewToken {
    #[serde(flatten)]
    pub info: ApiToken,
    pub token: String,
}

/// A character a name keeps out: control characters, which include a
/// terminal's escape sequences, and the marks that turn text around. A paired
/// browser chooses its own name, and `upnext token list` prints names in a
/// terminal, where an escape sequence would clear the screen or retitle it.
pub fn hidden_char(c: char) -> bool {
    c.is_control() || matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// A token's name as it's kept: without `hidden_char`s, and with each run of
/// spaces made one.
pub fn clean_name(name: &str) -> String {
    let kept: String = name.chars().filter(|c| c.is_whitespace() || !hidden_char(*c)).collect();
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The stored form of a token: its SHA-256, in hex.
pub fn token_hash(token: &str) -> String {
    Sha256::digest(token.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// A new random token: the prefix and 40 letters and digits.
fn new_token() -> Result<String> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut out = String::from(TOKEN_PREFIX);
    let mut buf = [0u8; 64];
    while out.len() < TOKEN_PREFIX.len() + TOKEN_CHARS {
        getrandom::fill(&mut buf).map_err(|e| anyhow::anyhow!("no randomness for a token: {e}"))?;
        // 248 is 4 x 62: anything above it would favour the first letters.
        for b in buf.iter().filter(|b| **b < 248) {
            if out.len() == TOKEN_PREFIX.len() + TOKEN_CHARS {
                break;
            }
            out.push(ALPHABET[(*b % 62) as usize] as char);
        }
    }
    Ok(out)
}

fn row(r: &rusqlite::Row) -> rusqlite::Result<ApiToken> {
    let scope: String = r.get(2)?;
    Ok(ApiToken {
        id: r.get(0)?,
        name: r.get(1)?,
        scope: Scope::parse(&scope).unwrap_or(Scope::Read),
        kind: r.get(3)?,
        hint: r.get(4)?,
        created_at: r.get(5)?,
        last_used_at: r.get(6)?,
        player_id: r.get::<_, Option<i64>>(7)?.map(super::PlayerId),
    })
}

const COLUMNS: &str = "id, name, scope, kind, hint, created_at, last_used_at, player_id";

/// A token as stored, hash and all: this install's, carried into a
/// restored or imported database in place of the copy's (`backups::Keys`).
#[derive(Debug, Clone)]
pub struct StoredToken {
    id: i64,
    name: String,
    scope: String,
    kind: String,
    hash: String,
    hint: String,
    created_at: i64,
    last_used_at: Option<i64>,
    player_id: Option<i64>,
}

impl StoredToken {
    /// Replace the tokens in `c`'s database with `tokens`. The list in use
    /// wins over a copy's, whatever either holds: a copy's would bring back
    /// tokens revoked since it was made (a lost phone's) and drop devices
    /// paired since. How many the copy had.
    pub fn replace_in(c: &rusqlite::Connection, tokens: &[StoredToken]) -> Result<usize> {
        let dropped = c.execute("DELETE FROM api_tokens", [])?;
        for t in tokens {
            // A token bound to a player the copy doesn't have is unbound,
            // never someone else's.
            c.execute(
                "INSERT INTO api_tokens(id, name, scope, kind, hash, hint, created_at, last_used_at, player_id) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, (SELECT id FROM players WHERE id = ?9))",
                params![t.id, t.name, t.scope, t.kind, t.hash, t.hint, t.created_at, t.last_used_at, t.player_id],
            )?;
        }
        Ok(dropped)
    }
}

impl Db {
    /// Make a token. Names are what you call the device or script ("phone",
    /// "Home Assistant"), one to 60 characters, each name used once, kept
    /// as `clean_name` leaves them.
    pub fn token_create(&self, name: &str, scope: Scope, kind: &str) -> Result<NewToken> {
        self.token_create_for(name, scope, kind, None)
    }

    /// Make a token that acts for one player, or for whoever the
    /// request names when `player` is None. A device allowed on the computer
    /// gets the player chosen in its dialog.
    pub fn token_create_for(&self, name: &str, scope: Scope, kind: &str, player: Option<PlayerId>) -> Result<NewToken> {
        let name = clean_name(name);
        let name = name.as_str();
        if name.is_empty() || name.chars().count() > 60 {
            bail!("a token's name is 1 to 60 characters, such as \"phone\" or \"Home Assistant\"");
        }
        let token = new_token()?;
        let hint: String = token.chars().take(TOKEN_PREFIX.len() + 4).collect();
        self.with(|c| {
            let taken = c.query_row("SELECT 1 FROM api_tokens WHERE name = ?1 COLLATE NOCASE", [name], |_| Ok(())).optional()?.is_some();
            if taken {
                bail!("there's already a token called {name:?}; revoke it first or pick another name");
            }
            if let Some(p) = player
                && c.query_row("SELECT 1 FROM players WHERE id = ?1", [p], |_| Ok(())).optional()?.is_none()
            {
                bail!("no player {p}");
            }
            c.execute(
                "INSERT INTO api_tokens(name, scope, kind, hash, hint, created_at, player_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![name, scope.as_str(), kind, token_hash(&token), hint, now(), player],
            )?;
            let id = c.last_insert_rowid();
            let info = c.query_row(&format!("SELECT {COLUMNS} FROM api_tokens WHERE id = ?1"), [id], row)?;
            Ok(NewToken { info, token })
        })
    }

    /// Every token, oldest first.
    pub fn tokens(&self) -> Result<Vec<ApiToken>> {
        self.with(|c| {
            let mut stmt = c.prepare(&format!("SELECT {COLUMNS} FROM api_tokens ORDER BY created_at, id"))?;
            Ok(stmt.query_map([], row)?.collect::<std::result::Result<_, _>>()?)
        })
    }

    /// Every token as stored, for `backups::Keys`.
    pub fn stored_tokens(&self) -> Result<Vec<StoredToken>> {
        self.with(|c| {
            let mut stmt =
                c.prepare("SELECT id, name, scope, kind, hash, hint, created_at, last_used_at, player_id FROM api_tokens ORDER BY id")?;
            let rows = stmt.query_map([], |r| {
                Ok(StoredToken {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    scope: r.get(2)?,
                    kind: r.get(3)?,
                    hash: r.get(4)?,
                    hint: r.get(5)?,
                    created_at: r.get(6)?,
                    last_used_at: r.get(7)?,
                    player_id: r.get(8)?,
                })
            })?;
            Ok(rows.collect::<std::result::Result<_, _>>()?)
        })
    }

    /// Revoke a token: it stops working at once. Whether there was one.
    pub fn token_revoke(&self, id: i64) -> Result<bool> {
        self.with(|c| Ok(c.execute("DELETE FROM api_tokens WHERE id = ?1", [id])? > 0))
    }

    /// The token a request carries, if it's one of ours, noting its use.
    pub fn token_check(&self, token: &str) -> Result<Option<ApiToken>> {
        if !token.starts_with(TOKEN_PREFIX) {
            return Ok(None);
        }
        let hash = token_hash(token);
        self.with(|c| {
            let found = c.query_row(&format!("SELECT {COLUMNS} FROM api_tokens WHERE hash = ?1"), [&hash], row).optional()?;
            if let Some(t) = &found {
                let t_now = now();
                if t.last_used_at.is_none_or(|at| t_now - at >= LAST_USED_EVERY) {
                    c.execute("UPDATE api_tokens SET last_used_at = ?1 WHERE id = ?2", params![t_now, t.id])?;
                }
            }
            Ok(found)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.db"), 120).unwrap();
        (dir, db)
    }

    #[test]
    fn tokens_are_random_prefixed_and_stored_as_hashes() {
        let (_dir, db) = db();
        let a = db.token_create("phone", Scope::Read, "token").unwrap();
        let b = db.token_create("script", Scope::Full, "token").unwrap();
        for t in [&a.token, &b.token] {
            assert!(t.starts_with("upnext_"), "{t}");
            assert_eq!(t.len(), 47);
            assert!(t[7..].chars().all(|c| c.is_ascii_alphanumeric()), "{t}");
        }
        assert_ne!(a.token, b.token);
        assert_eq!(a.info.hint, a.token[..11]);
        // Only the hash is stored: no column anywhere holds the token.
        let dump: Vec<String> = db
            .with(|c| {
                let mut stmt = c.prepare("SELECT name, scope, kind, hash, hint FROM api_tokens")?;
                let rows = stmt.query_map([], |r| Ok((0..5).map(|i| r.get::<_, String>(i).unwrap()).collect::<Vec<_>>().join("|")))?;
                Ok(rows.collect::<std::result::Result<_, _>>()?)
            })
            .unwrap();
        assert!(dump.iter().all(|r| !r.contains(&a.token) && !r.contains(&b.token)), "{dump:?}");
        assert!(dump[0].contains(&token_hash(&a.token)));

        assert_eq!(db.token_check(&a.token).unwrap().unwrap().scope, Scope::Read);
        assert_eq!(db.token_check(&b.token).unwrap().unwrap().scope, Scope::Full);
        assert!(db.token_check("upnext_nope").unwrap().is_none());
        assert!(db.token_check(&a.token[7..]).unwrap().is_none(), "without the prefix it isn't a token");
        assert!(db.tokens().unwrap()[0].last_used_at.is_some(), "a check notes the use");

        assert!(db.token_create("Phone", Scope::Full, "token").is_err(), "names are used once");
        assert!(db.token_create("  ", Scope::Full, "token").is_err());

        assert!(db.token_revoke(a.info.id).unwrap());
        assert!(db.token_check(&a.token).unwrap().is_none(), "revoked at once");
        assert!(!db.token_revoke(a.info.id).unwrap());
        assert_eq!(db.tokens().unwrap().len(), 1);
    }

    /// Security review L7: a name can't carry a terminal's escape sequences
    /// or turn text around.
    #[test]
    fn names_keep_no_control_characters() {
        let (_dir, db) = db();
        let made = db.token_create("evil\u{1b}]0;pwned\u{7}\u{1b}[2J\nphone", Scope::Read, "token").unwrap();
        assert_eq!(made.info.name, "evil]0;pwned[2J phone");
        let made = db.token_create("\u{202e}enohp\u{2069}  tablet\t", Scope::Read, "token").unwrap();
        assert_eq!(made.info.name, "enohp tablet");
        assert!(db.token_create("\u{1b}\u{7}", Scope::Read, "token").is_err(), "nothing left is no name");
        assert!(db.tokens().unwrap().iter().all(|t| !t.name.chars().any(hidden_char)));
    }
}
