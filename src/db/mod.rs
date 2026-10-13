//! SQLite storage.
//!
//! Model: a **title** is one game (what the user sees and picks); a **copy**
//! (`games` row) is one store's ownership of it, keyed by `(source, source_id)`.
//! Copies are grouped into titles by Steam appid when known, otherwise by
//! normalized name. Status, overrides, HLTB and picks are per title; Steam
//! store metadata (details/reviews/tags) is per appid.

mod accounts;
mod ai_calls;
mod community;
mod diary;
mod enrich;
mod filter;
mod gems;
mod identities;
mod jobs;
mod model;
mod picks;
mod plan_readings;
mod players;
mod ratings;
mod reset;
mod routines;
mod schema;
mod second_chances;
mod seed;
mod sync;
mod taste;
mod titles;
mod tokens;
mod verdicts;
mod views;
mod wishlist;

pub use accounts::{AiAccount, SettingsView, StoreAccount, StoreConnection, pin_ok};
pub use ai_calls::*;
pub use community::{CommunitySnapshot, RECEIVE_SETTING as VERDICTS_RECEIVE, SOURCE_META as VERDICTS_SOURCE_META};
pub use diary::{DiaryRow, NewDiaryRow};
pub use gems::GemRow;
pub use jobs::*;
pub use model::*;
pub use picks::{HistoryFind, HistoryOutcome, LineupChange, LineupChanged};
pub use plan_readings::*;
pub use players::{
    AI_KEYS, AI_SECRET_KEYS, CONNECTION_KEYS, HiddenTitle, HideScope, Player, PlayerId, Sharing, SignIn, household_setting,
    placeholder_name, setting_get_in, setting_set_in, settings_in,
};
pub(crate) use players::{keep_sign_ins, strip_account_keys};
pub use reset::ResetParts;

/// The meta row that holds this install's "Send to UpNext" key.
pub const GOG_BOOKMARK_KEY: &str = "gog_bookmark_key";
/// The meta row that holds the key the player cookie is signed with.
pub const PLAYER_COOKIE_KEY: &str = "player_cookie_key";
/// This install's own secrets in `meta`: a copy without keys leaves them
/// out, and a restore or an import keeps this install's in place of the
/// copy's, so its bookmarks and player cookies keep working.
pub const INSTALL_SECRETS: [&str; 2] = [GOG_BOOKMARK_KEY, PLAYER_COOKIE_KEY];
pub(crate) use schema::too_old;
pub use schema::{BASELINE_VERSION, SCHEMA_VERSION};
pub use second_chances::NowRow;
pub use seed::{SeedApply, SeededDigest};
pub use sync::SyncConnection;
pub use taste::{NewStatement, PatternChanges, PatternRow, ReactionCounts, StatementRow};
pub use titles::*;
#[cfg(test)]
pub use tokens::token_hash;
pub use tokens::{ApiToken, NewToken, Scope, StoredToken, clean_name, hidden_char};
pub use verdicts::{KEY_SETTING as VERDICTS_KEY, MyVerdict, VerdictState};
pub(crate) use verdicts::{keep_verdicts_keys, strip_verdicts_keys};
pub use wishlist::{FromWishlist, NewWish, StoreFacts, WishGame, WishRow, WishSource, WishState, settable_state};

use crate::lifecycle::StatusRules;
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

// ---------------------------------------------------------------------------
// Db
// ---------------------------------------------------------------------------

/// A change UpNext won't make, and why, said plainly: the asker's mistake,
/// not a fault. The API answers it with a 400 and the reason; any other
/// error from the same call is the server's, a 500 (R-CORE-2).
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Refused(pub String);

pub struct Db {
    conn: Mutex<Connection>,
    path: std::path::PathBuf,
    /// The settings statuses follow (`Config::status_rules`).
    rules: Mutex<StatusRules>,
    /// Every title as the household has it, every copy included, built once
    /// per database change (see `all_games_shared`).
    cache: Mutex<Option<Cached>>,
    /// Each player's library, laid over the household's.
    player_cache: Mutex<HashMap<PlayerId, Cached>>,
    /// Plan readings taken in the background for this database, shared
    /// with the connections `reopen` makes for them.
    pub(crate) readings: Arc<ReadingsInFlight>,
}

/// Rows changed on our connection, commits by other processes (the CLI),
/// the settings statuses follow, and the day, since statuses change with time
/// (a game untouched for 2 months is shelved).
type CacheKey = (u64, i64, StatusRules, i64);
/// A library as read at a cache key.
type Cached = (CacheKey, Arc<Vec<GameView>>);

fn json_vec(s: Option<String>) -> Vec<String> {
    s.and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok()).unwrap_or_default()
}

fn to_json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "[]".to_string())
}

impl Db {
    pub fn open(path: &Path, sampled_minutes: i64) -> Result<Self> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        // It holds API keys (Settings); only you should read it.
        crate::http::make_private(path);
        conn.execute_batch("PRAGMA busy_timeout=5000;")?;
        // Only reads until the version is one this build opens: switching to
        // WAL rewrites the file's header, and a refused file stays as it was.
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            anyhow::bail!(
                "{} is from a newer UpNext (schema v{version}); this build reads up to v{SCHEMA_VERSION}, so update UpNext",
                path.display()
            );
        }
        let library = schema::table_exists(&conn, "games")?;
        if version < schema::BASELINE_VERSION && library {
            anyhow::bail!("{} {}, so move it aside to start a new library", path.display(), schema::too_old(version));
        }
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=OFF;")?;
        // An upgrade never runs without a backup (an empty file has nothing to keep).
        if version < SCHEMA_VERSION && library {
            crate::backups::before_upgrade(path, version)?;
        }
        Self::migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            path: path.to_path_buf(),
            rules: Mutex::new(StatusRules { sampled_minutes, ..StatusRules::default() }),
            cache: Mutex::new(None),
            player_cache: Mutex::default(),
            readings: Arc::default(),
        })
    }

    /// Upgrade a database that isn't open as a `Db` (a copy being restored)
    /// to this build's schema.
    pub fn upgrade(c: &Connection) -> Result<()> {
        Self::migrate(c)
    }

    /// A second connection to the same database, for work that outlives the
    /// caller's borrow (a plan reading after an AI call). It is already
    /// migrated, so this only opens it.
    pub fn reopen(&self) -> Result<Self> {
        let conn = Connection::open(&self.path).with_context(|| format!("opening {}", self.path.display()))?;
        conn.execute_batch("PRAGMA foreign_keys=OFF; PRAGMA busy_timeout=5000;")?;
        Ok(Self {
            conn: Mutex::new(conn),
            path: self.path.clone(),
            rules: Mutex::new(self.status_rules()),
            cache: Mutex::new(None),
            player_cache: Mutex::default(),
            readings: self.readings.clone(),
        })
    }

    /// The database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        f(&conn)
    }

    pub fn set_status_rules(&self, rules: StatusRules) {
        if let Ok(mut r) = self.rules.lock() {
            *r = rules;
        }
    }

    pub fn status_rules(&self) -> StatusRules {
        self.rules.lock().map(|r| *r).unwrap_or_default()
    }

    pub fn sampled_minutes(&self) -> i64 {
        self.status_rules().sampled_minutes
    }

    // -- app settings (override .env) --------------------------------------

    /// Every stored setting as player `p` reads it: the household's, their
    /// own, and their connections' and AI account's (`players::settings_in`).
    pub fn settings_all(&self, p: PlayerId) -> Result<HashMap<String, String>> {
        self.with(|c| settings_in(c, p))
    }

    /// One app setting's stored value, as player `p` reads it.
    pub fn setting_get(&self, p: PlayerId, key: &str) -> Result<Option<String>> {
        self.with(|c| setting_get_in(c, p, key))
    }

    /// `None` / empty clears the app value so `.env` or the default applies
    /// again. A setting that is a player's, a connection's or an AI
    /// account's is changed there.
    pub fn setting_set(&self, p: PlayerId, key: &str, value: Option<&str>) -> Result<()> {
        self.with(|c| setting_set_in(c, p, key, value))
    }

    // -- meta ---------------------------------------------------------------

    pub fn meta_get(&self, key: &str) -> Result<Option<String>> {
        self.with(|c| Ok(c.query_row("SELECT value FROM meta WHERE key=?1", params![key], |r| r.get(0)).optional()?))
    }

    /// Every meta key `key`, or `key.` followed by anything (one per
    /// account, as the diary's readings are), with its value.
    pub fn meta_family(&self, key: &str) -> Result<Vec<(String, String)>> {
        self.with(|c| {
            let mut st =
                c.prepare("SELECT key, value FROM meta WHERE key = ?1 OR substr(key, 1, length(?1) + 1) = ?1 || '.' ORDER BY key")?;
            let rows = st.query_map(params![key], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// This install's key for GOG's "Send to UpNext" bookmark (security
    /// review L10), made the first time it's asked for. The bookmark carries
    /// it, so the callback can tell a code this install's own bookmark
    /// brought from one another page sent the browser with. It stays out of
    /// a copy without keys and out of the log.
    pub fn gog_bookmark_key(&self) -> Result<String> {
        self.install_key(GOG_BOOKMARK_KEY, "the GOG bookmark's key")
    }

    /// This install's key for signing player cookies, made the
    /// first time it's asked for. It stays out of a copy without keys.
    pub fn player_cookie_key(&self) -> Result<String> {
        self.install_key(PLAYER_COOKIE_KEY, "the player cookie's key")
    }

    /// One of [`INSTALL_SECRETS`]: 32 random bytes as hex, kept in `meta`.
    fn install_key(&self, key: &str, what: &str) -> Result<String> {
        if let Some(k) = self.meta_get(key)?.filter(|k| k.len() == 64) {
            return Ok(k);
        }
        let mut b = [0u8; 32];
        getrandom::fill(&mut b).map_err(|e| anyhow::anyhow!("no randomness for {what}: {e}"))?;
        let k: String = b.iter().map(|b| format!("{b:02x}")).collect();
        self.with(|c| {
            // Two first asks at once agree on one key: the first one written.
            c.execute("DELETE FROM meta WHERE key=?1 AND length(value) <> 64", params![key])?;
            c.execute("INSERT OR IGNORE INTO meta(key, value) VALUES (?1, ?2)", params![key, k])?;
            Ok(c.query_row("SELECT value FROM meta WHERE key=?1", params![key], |r| r.get(0))?)
        })
    }

    pub fn meta_set(&self, key: &str, value: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![key, value],
            )?;
            Ok(())
        })
    }

    /// Steam profile (from meta) plus per-source sync info, for the copies
    /// player `p` sees. `configured` and `note` are filled in by the caller,
    /// which knows about credentials/paths.
    pub fn profile(&self, p: PlayerId, steam_id: Option<&str>) -> Result<Profile> {
        let mut prof: Profile = self.meta_get("profile")?.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        if prof.steam_id.is_empty() {
            prof.steam_id = steam_id.unwrap_or("").to_string();
        }
        prof.last_sync = self.meta_get("last_sync")?.and_then(|s| s.parse().ok());
        let sight = self.with(|c| players::Sight::of(c, p))?;
        // (account, title, source) of every copy not removed.
        let copies: Vec<(i64, Option<i64>, String)> = self.with(|c| {
            let mut stmt = c.prepare("SELECT account_id, title_id, source FROM games WHERE removed=0")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            Ok(rows.collect::<std::result::Result<_, _>>()?)
        })?;
        let seen: Vec<&(i64, Option<i64>, String)> =
            copies.iter().filter(|(a, t, _)| t.is_some_and(|t| sight.sees(*a, t) && !sight.hid(t))).collect();
        prof.copy_count = seen.len() as i64;
        prof.game_count = seen.iter().filter_map(|(_, t, _)| *t).collect::<std::collections::HashSet<_>>().len() as i64;
        let mut counts: HashMap<String, i64> = HashMap::new();
        for (_, _, source) in &seen {
            *counts.entry(source.clone()).or_default() += 1;
        }
        // Each store's latest sync among the accounts they see, and the
        // Galaxy import of their own connection.
        let syncs: HashMap<String, (i64, i64)> = self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT s.account_id, a.store, s.last_sync, s.count FROM source_sync s JOIN store_accounts a ON a.id = s.account_id",
            )?;
            let rows =
                stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?)))?;
            let mut out: HashMap<String, (i64, i64)> = HashMap::new();
            for row in rows {
                let (account, store, last, count) = row?;
                if sight.sees_account(account) && out.get(&store).is_none_or(|(l, _)| *l < last) {
                    out.insert(store, (last, count));
                }
            }
            let galaxy: Option<(Option<i64>, Option<i64>)> = c
                .query_row(
                    "SELECT last_sync, count FROM store_connections WHERE owner_id=?1 AND kind='galaxy' ORDER BY id LIMIT 1",
                    params![p],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some((Some(last), count)) = galaxy {
                out.insert("galaxy".into(), (last, count.unwrap_or(0)));
            }
            Ok(out)
        })?;
        prof.sources = [
            ("steam", "steam", "Steam"),
            ("epic", "heroic", "Epic"),
            ("gog", "gog", "GOG"),
            ("amazon", "heroic", "Amazon (Heroic)"),
            ("galaxy", "galaxy", "GOG Galaxy import"),
            ("origin", "origin", "EA app"),
            ("uplay", "uplay", "Ubisoft Connect"),
        ]
        .iter()
        // The stores read only on Windows, or added by hand, are listed
        // once they have games, so they don't read as missing elsewhere.
        .filter(|(source, _, _)| !matches!(*source, "origin" | "uplay") || counts.contains_key(*source))
        .map(|(source, launcher, label)| SourceInfo {
            source: source.to_string(),
            launcher: launcher.to_string(),
            label: label.to_string(),
            configured: false,
            count: if *source == "galaxy" {
                syncs.get("galaxy").map(|s| s.1).unwrap_or(0)
            } else {
                counts.get(*source).copied().unwrap_or(0)
            },
            last_sync: syncs.get(*source).map(|s| s.0),
            note: None,
        })
        .collect();
        Ok(prof)
    }

    pub fn set_profile(&self, p: &Profile) -> Result<()> {
        self.meta_set("profile", &to_json(p))
    }

    // -- export and restore --------------------------------------------------

    /// Write a consistent copy of the whole database to `path` (which must
    /// not exist), without keys and sign-ins (`backups::strip_keys`), so the
    /// file is safe to share; everything else, including statuses, Play
    /// later, merges and pick history, is in it.
    pub fn export_to(&self, path: &std::path::Path) -> Result<()> {
        self.with(|c| crate::backups::keyless_copy_of(c, path))
    }

    /// Replace the whole database with the one at `from`, which is checked
    /// and at this build's schema (`backups::prepare`). SQLite's backup API
    /// copies it into this connection, so the server keeps running on it.
    ///
    /// The running job `keep` (the restore) keeps its row and its log so
    /// far under its own id; the copy's job with that id, if it has one,
    /// moves to a new id. Jobs the copy caught running are closed, since it
    /// can't know how they ended.
    pub fn replace_with(&self, from: &Path, keep: i64) -> Result<()> {
        let mut guard = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        let conn: &mut Connection = &mut guard;
        let src = Connection::open(from).with_context(|| format!("opening {}", from.display()))?;
        // A database in WAL mode only takes a copy with its own page size.
        let page_size = |c: &Connection| c.query_row("PRAGMA main.page_size", [], |r| r.get::<_, i64>(0));
        let ours = page_size(conn)?;
        if page_size(&src)? != ours {
            src.execute_batch(&format!("PRAGMA page_size={ours}; VACUUM;"))?;
        }
        conn.execute_batch(
            "DROP TABLE IF EXISTS temp.kept_job; DROP TABLE IF EXISTS temp.kept_log; DROP TABLE IF EXISTS temp.kept_failures;",
        )?;
        conn.execute("CREATE TEMP TABLE kept_job AS SELECT * FROM main.jobs WHERE id=?1", [keep])?;
        conn.execute("CREATE TEMP TABLE kept_log AS SELECT * FROM main.job_log WHERE job_id=?1", [keep])?;
        conn.execute("CREATE TEMP TABLE kept_failures AS SELECT * FROM main.job_failures WHERE job_id=?1", [keep])?;
        {
            use rusqlite::backup::{Backup, StepResult};
            let copy = Backup::new(&src, conn)?;
            let mut tries = 0;
            loop {
                match copy.step(-1)? {
                    StepResult::Done => break,
                    StepResult::More => {}
                    _ if tries < 50 => {
                        tries += 1;
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    _ => anyhow::bail!("the database stayed busy, so nothing was replaced"),
                }
            }
        }
        drop(src);
        conn.flush_prepared_statement_cache();
        let tx = conn.unchecked_transaction()?;
        // The moved job's log lines point at it; the keys are checked at the commit.
        tx.execute_batch("PRAGMA defer_foreign_keys=ON;")?;
        if tx.query_row("SELECT 1 FROM main.jobs WHERE id=?1", [keep], |_| Ok(())).optional()?.is_some() {
            let moved: i64 = tx.query_row("SELECT MAX(id) + 1 FROM main.jobs", [], |r| r.get(0))?;
            for (table, col) in [("jobs", "id"), ("job_log", "job_id"), ("job_failures", "job_id"), ("ai_calls", "job_id")] {
                tx.execute(&format!("UPDATE main.{table} SET {col}=?1 WHERE {col}=?2"), params![moved, keep])?;
            }
        }
        tx.execute(
            "UPDATE main.jobs SET status='failed', finished_at=heartbeat_at, current=NULL,
                 error='Running when this backup was made, so the backup doesn''t know how it ended.'
             WHERE status='running'",
            [],
        )?;
        for (table, kept, own_id) in [("jobs", "kept_job", true), ("job_log", "kept_log", false), ("job_failures", "kept_failures", false)]
        {
            copy_rows(&tx, table, kept, own_id)?;
        }
        tx.execute_batch("DROP TABLE temp.kept_job; DROP TABLE temp.kept_log; DROP TABLE temp.kept_failures;")?;
        tx.commit()?;
        drop(guard);
        *self.cache.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.player_cache.lock().unwrap_or_else(|e| e.into_inner()).clear();
        Ok(())
    }
}

/// Copy rows kept in a temp table back into `main.<table>`, by column name
/// (a table's columns can be in another order in another database). Log
/// lines and failures take new ids, so they can't meet the copy's own.
fn copy_rows(c: &Connection, table: &str, kept: &str, own_id: bool) -> Result<()> {
    let columns = |schema: &str, t: &str| -> Result<Vec<String>> {
        let mut stmt = c.prepare(&format!("PRAGMA {schema}.table_info({t})"))?;
        let names = stmt.query_map([], |r| r.get::<_, String>(1))?.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(names)
    };
    let theirs = columns("temp", kept)?;
    let both: Vec<String> = columns("main", table)?.into_iter().filter(|col| theirs.contains(col) && (own_id || col != "id")).collect();
    let list = both.join(", ");
    c.execute(&format!("INSERT INTO main.{table} ({list}) SELECT {list} FROM temp.{kept}"), [])?;
    Ok(())
}

#[cfg(test)]
mod grouping_tests;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod migration_tests;
#[cfg(test)]
mod players_tests;
