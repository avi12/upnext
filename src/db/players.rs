//! Players: the people who play on this computer, the store and
//! AI accounts each one owns and offers to the others, the games they hide,
//! and their own settings.
//!
//! Every per-player function in the data layer takes a [`PlayerId`], so the
//! compiler finds each call that has to say whose it is. Until the server
//! works out each request's player, everything outside
//! the data layer acts for [`PlayerId::FIRST`].

use super::{Db, now};
use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

/// A player's id (`players.id`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlayerId(pub i64);

impl PlayerId {
    /// The player every database starts with, who owns everything a
    /// database held before it had players.
    pub const FIRST: PlayerId = PlayerId(1);
}

impl rusqlite::ToSql for PlayerId {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        self.0.to_sql()
    }
}

impl std::fmt::Display for PlayerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// The colours new players are given, in order. The picker shows each
/// player's card in theirs.
pub const COLOURS: [&str; 8] = ["#4f8cff", "#f2994a", "#27ae60", "#bb6bd9", "#eb5757", "#2bb3c0", "#e2b93b", "#9b8579"];

/// Settings that are each player's own: how UpNext looks to them and how
/// their play is read (`docs/players.md`).
pub const PLAYER_KEYS: &[&str] = &[
    "theme",
    "theme_follow_system",
    "sampled_minutes",
    "shelve_after_days",
    "abandon_after_days",
    "staple_hours",
    "burnout_days",
    "pattern_half_life_days",
    "pattern_min_games",
    "wishlist",
    // Whether to offer a backlog verdict after a mark.
    "verdicts_ask",
];

/// Settings kept on a store connection, with the connection's kind.
pub const CONNECTION_KEYS: &[(&str, &str)] = &[
    ("steam_id", "steam"),
    ("steam_include_hidden", "steam"),
    ("galaxy_db", "galaxy"),
    ("galaxy_skip", "galaxy"),
    ("heroic_dir", "heroic"),
    ("epic_manifests", "epic"),
];

/// The setting kept in an AI account's `backend` column.
const AI_BACKEND_KEY: &str = "llm_backend";

/// AI settings kept in an AI account's `settings`.
pub const AI_KEYS: &[&str] = &[
    "model",
    "digest_model",
    "codex_model",
    "codex_digest_model",
    "codex_effort",
    "openai_key_model",
    "openai_key_digest_model",
    "openai_key_effort",
    "openai_base_url",
    "openai_model",
    "openai_thinking",
    "openai_digest_model",
];

/// AI keys, kept in an AI account's `secret` and never shown.
pub const AI_SECRET_KEYS: &[&str] = &["anthropic_api_key", "openai_key", "openai_api_key", "claude_oauth_token"];

/// The player setting naming the AI account a player uses.
const AI_ACCOUNT: &str = "ai_account";

/// The household setting naming the AI account the library's own AI work
/// (summaries, series guides, hidden gems) runs on.
pub const LIBRARY_AI: &str = "library_ai";

/// Where a setting is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Home {
    /// The `settings` table: the same for everyone on this computer.
    Household,
    /// The player's own row in `player_settings`.
    Player,
    /// The player's first connection of this kind.
    Connection(&'static str),
    /// The AI account the player uses: its backend, settings or secret.
    AiBackend,
    Ai,
    AiSecret,
}

/// Whether a setting is the household's, the same for every player (plan
/// 21): a token bound to one player can't change those.
pub fn household_setting(key: &str) -> bool {
    home(key) == Home::Household
}

fn home(key: &str) -> Home {
    if PLAYER_KEYS.contains(&key) {
        Home::Player
    } else if let Some((_, kind)) = CONNECTION_KEYS.iter().find(|(k, _)| *k == key) {
        Home::Connection(kind)
    } else if key == AI_BACKEND_KEY {
        Home::AiBackend
    } else if AI_KEYS.contains(&key) {
        Home::Ai
    } else if AI_SECRET_KEYS.contains(&key) {
        Home::AiSecret
    } else {
        Home::Household
    }
}

/// Who an account is offered to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sharing {
    Everyone,
    /// The players the owner ticked.
    Chosen,
    /// Only its owner.
    Owner,
}

impl Sharing {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Everyone => "everyone",
            Self::Chosen => "chosen",
            Self::Owner => "owner",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Everyone, Self::Chosen, Self::Owner].into_iter().find(|x| x.as_str() == s)
    }
}

/// "Hide this game": from whom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HideScope {
    /// From this player's own views; nobody else is affected.
    Me,
    /// This player's own copies, from every other player.
    Others,
    /// Both.
    Everyone,
}

impl HideScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Me => "me",
            Self::Others => "others",
            Self::Everyone => "everyone",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Me, Self::Others, Self::Everyone].into_iter().find(|x| x.as_str() == s)
    }

    /// Whether it hides the game from the player who chose it.
    fn hides_from_self(self) -> bool {
        matches!(self, Self::Me | Self::Everyone)
    }

    /// Whether it hides the player's own copies from the others.
    fn hides_from_others(self) -> bool {
        matches!(self, Self::Others | Self::Everyone)
    }
}

/// One player, as stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Player {
    pub id: PlayerId,
    pub name: String,
    pub colour: String,
    /// Whether they set a PIN; the hash never leaves the data layer.
    pub has_pin: bool,
    pub created_at: i64,
    /// The identity provider whose account signs in as them,
    /// when one is linked.
    pub oidc_issuer: Option<String>,
}

/// A game a player hid, for their Hidden by me list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct HiddenTitle {
    pub title_id: i64,
    pub name: String,
    /// `me`, `others` or `everyone`.
    #[schema(value_type = String)]
    pub scope: HideScope,
    pub at: i64,
    /// Whether the player has a copy in an account they own, which hiding
    /// it from the others needs.
    #[serde(default)]
    pub own_copy: bool,
}

/// Someone who still sees a game after it was hidden from the others: with
/// a copy of their own, or through another player's shared account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StillSeen {
    pub name: String,
    pub own_copy: bool,
}

/// A name UpNext made up rather than one somebody chose: "Player 1" for
/// the first player of a new database, "Player N" for one a copy without
/// keys renamed or an identity provider gave no name, and "Player N 2" when
/// that was taken. The wizard and Settings offer to change it.
pub fn placeholder_name(name: &str) -> bool {
    let mut words = name.split(' ');
    let number = |w: Option<&str>| w.is_some_and(|w| !w.is_empty() && w.bytes().all(|b| b.is_ascii_digit()));
    words.next() == Some("Player") && number(words.next()) && {
        let rest: Vec<&str> = words.collect();
        rest.is_empty() || (rest.len() == 1 && number(Some(rest[0])))
    }
}

/// Make player 1 in a new database, with an AI account of their own that
/// they use and the library's AI runs on, as the upgrade to v32 does for an
/// existing one. The account starts empty, so the environment's settings or
/// the built-in ones apply until Settings changes them.
pub(super) fn make_first_player(c: &Connection) -> Result<()> {
    let at = now();
    c.execute("INSERT INTO players(id, name, colour, created_at) VALUES (1, 'Player 1', ?1, ?2)", params![COLOURS[0], at])?;
    c.execute("INSERT INTO ai_accounts(owner_id, created_at) VALUES (1, ?1)", params![at])?;
    let ai = c.last_insert_rowid().to_string();
    c.execute("INSERT INTO player_settings(player_id, key, value) VALUES (1, ?1, ?2)", params![AI_ACCOUNT, ai])?;
    c.execute("INSERT INTO settings(key, value) VALUES (?1, ?2)", params![LIBRARY_AI, ai])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Settings, read in layers
// ---------------------------------------------------------------------------

pub(super) fn json_map(text: Option<String>) -> Map<String, Value> {
    match text.and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

fn strings(m: Map<String, Value>) -> impl Iterator<Item = (String, String)> {
    m.into_iter().filter_map(|(k, v)| match v {
        Value::String(s) => Some((k, s)),
        _ => None,
    })
}

/// The player's first connection of `kind`.
pub(super) fn own_connection(c: &Connection, p: PlayerId, kind: &str) -> Result<Option<i64>> {
    Ok(c.query_row("SELECT id FROM store_connections WHERE owner_id=?1 AND kind=?2 ORDER BY id LIMIT 1", params![p, kind], |r| r.get(0))
        .optional()?)
}

/// Whether AI account `id` is the player's or offered to them.
pub(super) fn ai_offered(c: &Connection, p: PlayerId, id: i64) -> Result<bool> {
    Ok(c.query_row(
        "SELECT 1 FROM ai_accounts a WHERE a.id=?2 AND (a.owner_id=?1 OR a.sharing='everyone'
            OR (a.sharing='chosen' AND EXISTS (SELECT 1 FROM ai_account_players x WHERE x.account_id=a.id AND x.player_id=?1)))",
        params![p, id],
        |_| Ok(()),
    )
    .optional()?
    .is_some())
}

/// The AI account the player uses: the one they chose, while it is still
/// theirs or offered to them.
pub(super) fn ai_account_in_use(c: &Connection, p: PlayerId) -> Result<Option<i64>> {
    let chosen: Option<i64> = c
        .query_row("SELECT value FROM player_settings WHERE player_id=?1 AND key=?2", params![p, AI_ACCOUNT], |r| r.get::<_, String>(0))
        .optional()?
        .and_then(|v| v.parse().ok());
    match chosen {
        Some(id) if ai_offered(c, p, id)? => Ok(Some(id)),
        _ => Ok(None),
    }
}

/// The AI account a setting the player changes is written to: the one they
/// use when it is their own, else their first own one, else a new one,
/// which they then use (and the library's AI, when it has none).
fn ai_account_to_write(c: &Connection, p: PlayerId) -> Result<i64> {
    if let Some(id) = ai_account_in_use(c, p)?
        && c.query_row("SELECT owner_id FROM ai_accounts WHERE id=?1", params![id], |r| r.get::<_, i64>(0))? == p.0
    {
        return Ok(id);
    }
    let id = match c
        .query_row("SELECT id FROM ai_accounts WHERE owner_id=?1 ORDER BY id LIMIT 1", params![p], |r| r.get::<_, i64>(0))
        .optional()?
    {
        Some(id) => id,
        None => {
            c.execute("INSERT INTO ai_accounts(owner_id, created_at) VALUES (?1, ?2)", params![p, now()])?;
            c.last_insert_rowid()
        }
    };
    c.execute(
        "INSERT INTO player_settings(player_id, key, value) VALUES (?1, ?2, ?3)
         ON CONFLICT(player_id, key) DO UPDATE SET value=excluded.value",
        params![p, AI_ACCOUNT, id.to_string()],
    )?;
    c.execute("INSERT OR IGNORE INTO settings(key, value) VALUES (?1, ?2)", params![LIBRARY_AI, id.to_string()])?;
    Ok(id)
}

/// Every stored setting as the player reads it: the household's, their own,
/// their connections', and the AI account they use (its keys included,
/// since this is what UpNext runs with; the API never shows them).
pub fn settings_in(c: &Connection, p: PlayerId) -> Result<HashMap<String, String>> {
    settings_with_ai(c, p, ai_account_in_use(c, p)?)
}

/// The AI account made first: the one the environment's AI settings
/// (`ANTHROPIC_API_KEY` and the rest) and this computer's signed-in CLIs
/// belong to. Every other account has only what is set on it.
pub(super) fn first_ai_account(c: &Connection) -> Result<Option<i64>> {
    Ok(c.query_row("SELECT MIN(id) FROM ai_accounts", [], |r| r.get(0))?)
}

/// The AI account the library's own AI work runs on (`library_ai`), while
/// it still exists.
pub(super) fn library_ai_account(c: &Connection) -> Result<Option<i64>> {
    let id: Option<i64> = c
        .query_row("SELECT value FROM settings WHERE key=?1", params![LIBRARY_AI], |r| r.get::<_, String>(0))
        .optional()?
        .and_then(|v| v.parse().ok());
    match id {
        Some(id) => Ok(c.query_row("SELECT id FROM ai_accounts WHERE id=?1", params![id], |r| r.get(0)).optional()?),
        None => Ok(None),
    }
}

/// [`settings_in`], with the AI settings of account `ai` (none: no AI).
pub(super) fn settings_with_ai(c: &Connection, p: PlayerId, ai: Option<i64>) -> Result<HashMap<String, String>> {
    let mut out: HashMap<String, String> = HashMap::new();
    let mut st = c.prepare("SELECT key, value FROM settings")?;
    for r in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (k, v) = r?;
        // The library's AI account is an id the accounts page manages, not
        // a setting anyone types.
        if home(&k) == Home::Household && k != LIBRARY_AI {
            out.insert(k, v);
        }
    }
    let mut st = c.prepare("SELECT key, value FROM player_settings WHERE player_id=?1")?;
    for r in st.query_map(params![p], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (k, v) = r?;
        if home(&k) == Home::Player {
            out.insert(k, v);
        }
    }
    let mut kinds: Vec<&str> = CONNECTION_KEYS.iter().map(|(_, k)| *k).collect();
    kinds.dedup();
    for kind in kinds {
        if let Some(id) = own_connection(c, p, kind)? {
            let settings = c.query_row("SELECT settings FROM store_connections WHERE id=?1", params![id], |r| r.get(0))?;
            out.extend(strings(json_map(Some(settings))).filter(|(k, _)| home(k) == Home::Connection(kind)));
        }
    }
    if let Some(id) = ai {
        let (backend, settings, secret): (Option<String>, String, Option<String>) =
            c.query_row("SELECT backend, settings, secret FROM ai_accounts WHERE id=?1", params![id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?;
        if let Some(b) = backend {
            out.insert(AI_BACKEND_KEY.into(), b);
        }
        out.extend(strings(json_map(Some(settings))).filter(|(k, _)| home(k) == Home::Ai));
        out.extend(strings(json_map(secret)).filter(|(k, _)| home(k) == Home::AiSecret));
    }
    Ok(out)
}

/// One setting as the player reads it.
pub fn setting_get_in(c: &Connection, p: PlayerId, key: &str) -> Result<Option<String>> {
    let from_json = |text: Option<String>| strings(json_map(text)).find(|(k, _)| k == key).map(|(_, v)| v);
    Ok(match home(key) {
        Home::Household => c.query_row("SELECT value FROM settings WHERE key=?1", params![key], |r| r.get(0)).optional()?,
        Home::Player => {
            c.query_row("SELECT value FROM player_settings WHERE player_id=?1 AND key=?2", params![p, key], |r| r.get(0)).optional()?
        }
        Home::Connection(kind) => match own_connection(c, p, kind)? {
            Some(id) => from_json(Some(c.query_row("SELECT settings FROM store_connections WHERE id=?1", params![id], |r| r.get(0))?)),
            None => None,
        },
        Home::AiBackend | Home::Ai | Home::AiSecret => match ai_account_in_use(c, p)? {
            Some(id) => {
                let column = match home(key) {
                    Home::AiBackend => "backend",
                    Home::Ai => "settings",
                    _ => "secret",
                };
                let text: Option<String> =
                    c.query_row(&format!("SELECT {column} FROM ai_accounts WHERE id=?1"), params![id], |r| r.get(0))?;
                if home(key) == Home::AiBackend { text } else { from_json(text) }
            }
            None => None,
        },
    })
}

/// Change one setting for the player, where it is kept: `None` or empty
/// clears it, so the environment's or the built-in value applies again. A
/// connection or AI setting with nowhere to go yet makes the player's own
/// connection or AI account for it.
pub fn setting_set_in(c: &Connection, p: PlayerId, key: &str, value: Option<&str>) -> Result<()> {
    let value = value.map(str::trim).filter(|v| !v.is_empty());
    let set_json = |text: Option<String>, value: Option<&str>| -> Option<String> {
        let mut m = json_map(text);
        match value {
            Some(v) => m.insert(key.to_string(), Value::String(v.to_string())),
            None => m.remove(key),
        };
        (!m.is_empty()).then(|| Value::Object(m).to_string())
    };
    match home(key) {
        Home::Household => match value {
            Some(v) => {
                c.execute(
                    "INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    params![key, v],
                )?;
            }
            None => {
                c.execute("DELETE FROM settings WHERE key=?1", params![key])?;
            }
        },
        Home::Player => match value {
            Some(v) => {
                c.execute(
                    "INSERT INTO player_settings(player_id, key, value) VALUES (?1, ?2, ?3)
                     ON CONFLICT(player_id, key) DO UPDATE SET value=excluded.value",
                    params![p, key, v],
                )?;
            }
            None => {
                c.execute("DELETE FROM player_settings WHERE player_id=?1 AND key=?2", params![p, key])?;
            }
        },
        Home::Connection(kind) => {
            let id = match (own_connection(c, p, kind)?, value) {
                (Some(id), _) => id,
                (None, None) => return Ok(()),
                (None, Some(_)) => {
                    c.execute("INSERT INTO store_connections(kind, owner_id, created_at) VALUES (?1, ?2, ?3)", params![kind, p, now()])?;
                    c.last_insert_rowid()
                }
            };
            let text: String = c.query_row("SELECT settings FROM store_connections WHERE id=?1", params![id], |r| r.get(0))?;
            let new = set_json(Some(text), value).unwrap_or_else(|| "{}".into());
            c.execute("UPDATE store_connections SET settings=?2 WHERE id=?1", params![id, new])?;
        }
        Home::AiBackend => {
            if value.is_none() && ai_account_in_use(c, p)?.is_none() {
                return Ok(());
            }
            let id = ai_account_to_write(c, p)?;
            c.execute("UPDATE ai_accounts SET backend=?2 WHERE id=?1", params![id, value])?;
        }
        Home::Ai | Home::AiSecret => {
            if value.is_none() && ai_account_in_use(c, p)?.is_none() {
                return Ok(());
            }
            let id = ai_account_to_write(c, p)?;
            if home(key) == Home::Ai {
                let text: String = c.query_row("SELECT settings FROM ai_accounts WHERE id=?1", params![id], |r| r.get(0))?;
                let new = set_json(Some(text), value).unwrap_or_else(|| "{}".into());
                c.execute("UPDATE ai_accounts SET settings=?2 WHERE id=?1", params![id, new])?;
            } else {
                let text: Option<String> = c.query_row("SELECT secret FROM ai_accounts WHERE id=?1", params![id], |r| r.get(0))?;
                c.execute("UPDATE ai_accounts SET secret=?2 WHERE id=?1", params![id, set_json(text, value)])?;
            }
        }
    }
    Ok(())
}

/// Take the keys and sign-ins out of a database being copied without them
/// (`backups::strip_keys`): the AI accounts' and connections' secrets, the
/// SteamIDs, which name the accounts, the connections' files and folders,
/// which name the computer's user, the AI accounts' own servers, the
/// store accounts' own names, the players' names and PINs, and the
/// identity provider accounts linked to them (an account id is often an
/// email address).
pub(crate) fn strip_account_keys(c: &Connection) -> Result<()> {
    c.execute("UPDATE ai_accounts SET secret=NULL", [])?;
    // An AI server of the household's own names one of its computers.
    c.execute("UPDATE ai_accounts SET settings=json_remove(settings, '$.openai_base_url') WHERE settings LIKE '%openai_base_url%'", [])?;
    c.execute("UPDATE store_connections SET secret=NULL", [])?;
    let mut st = c.prepare("SELECT id, settings FROM store_connections")?;
    let rows: Vec<(i64, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    for (id, text) in rows {
        let mut m = json_map(Some(text));
        let before = m.len();
        for key in ["steam_id"].into_iter().chain(crate::config::path_keys()) {
            m.remove(key);
        }
        if m.len() != before {
            c.execute("UPDATE store_connections SET settings=?2 WHERE id=?1", params![id, Value::Object(m).to_string()])?;
        }
    }
    c.execute("UPDATE store_accounts SET external_id=NULL", [])?;
    // A store account's name is the store's until someone renames it ("Sam's Steam").
    let mut st = c.prepare("SELECT DISTINCT store FROM store_accounts")?;
    let stores: Vec<String> = st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    for store in stores {
        c.execute("UPDATE store_accounts SET name=?2 WHERE store=?1", params![store, crate::galaxy::platform_label(&store)])?;
    }
    // From v37: the identity provider's account each player signs in as.
    let linked: bool = c.query_row("SELECT COUNT(*) > 0 FROM pragma_table_info('players') WHERE name='oidc_subject'", [], |r| r.get(0))?;
    if linked {
        c.execute("UPDATE players SET oidc_issuer=NULL, oidc_subject=NULL", [])?;
    }
    // The key that signs each browser's choice of player.
    c.execute("DELETE FROM meta WHERE key='player_cookie_key'", [])?;
    // Players' names can be people's names, and v32 named player 1 after
    // the Steam persona; their PINs are secrets too. The first pass clears
    // the names so the second can't collide with one not renamed yet.
    c.execute_batch(
        "UPDATE players SET name = 'player ' || id || ' (renaming)', pin_hash = NULL;
         UPDATE players SET name = 'Player ' || id;",
    )?;
    Ok(())
}

/// The connection kinds whose `secret` is a store sign-in (GOG's or
/// Epic's token), kept on this install when a copy without them comes in.
const SIGN_IN_KINDS: [(&str, &str); 2] = [("gog", "GOG sign-in"), ("epic", "Epic sign-in")];

/// A player's sign-in to a store, as `secret` keeps it on their first
/// connection of that kind.
#[derive(Debug, Clone, PartialEq)]
pub struct SignIn {
    pub player: PlayerId,
    pub kind: String,
    pub secret: String,
}

impl Db {
    /// Every player's GOG and Epic sign-ins.
    pub fn sign_ins(&self) -> Result<Vec<SignIn>> {
        self.with(|c| {
            let mut out = vec![];
            for (kind, _) in SIGN_IN_KINDS {
                let mut st = c.prepare(
                    "SELECT owner_id, secret FROM store_connections k WHERE kind=?1
                     AND id = (SELECT MIN(id) FROM store_connections WHERE owner_id=k.owner_id AND kind=?1)
                     AND secret IS NOT NULL AND TRIM(secret) <> '' ORDER BY owner_id",
                )?;
                let rows =
                    st.query_map(params![kind], |r| Ok(SignIn { player: PlayerId(r.get(0)?), kind: kind.to_string(), secret: r.get(1)? }))?;
                out.extend(rows.collect::<rusqlite::Result<Vec<_>>>()?);
            }
            Ok(out)
        })
    }
}

/// Keep `sign_ins` in a database being restored or imported, for each
/// player it has who isn't signed in to that store there (a copy without
/// keys, or one from before they signed in). What was kept, as the restore
/// names it.
pub(crate) fn keep_sign_ins(c: &Connection, sign_ins: &[SignIn]) -> Result<Vec<&'static str>> {
    let mut kept = vec![];
    for s in sign_ins {
        let Some((kind, label)) = SIGN_IN_KINDS.into_iter().find(|(k, _)| *k == s.kind) else { continue };
        let exists: bool = c.query_row("SELECT COUNT(*) FROM players WHERE id=?1", params![s.player], |r| r.get::<_, i64>(0))? > 0;
        let signed_in: bool = c.query_row(
            "SELECT COUNT(*) FROM store_connections WHERE owner_id=?1 AND kind=?2 AND secret IS NOT NULL AND TRIM(secret) <> ''",
            params![s.player, kind],
            |r| r.get::<_, i64>(0),
        )? > 0;
        if !exists || signed_in {
            continue;
        }
        let id = match own_connection(c, s.player, kind)? {
            Some(id) => id,
            None => {
                c.execute(
                    "INSERT INTO store_connections(kind, owner_id, created_at) VALUES (?1, ?2, ?3)",
                    params![kind, s.player, super::now()],
                )?;
                c.last_insert_rowid()
            }
        };
        c.execute("UPDATE store_connections SET secret=?2 WHERE id=?1", params![id, s.secret])?;
        if !kept.contains(&label) {
            kept.push(label);
        }
    }
    Ok(kept)
}

// ---------------------------------------------------------------------------
// What a player sees
// ---------------------------------------------------------------------------

/// Which copies a player sees and whose hours count for them: the accounts
/// they own, those offered to them that they use, those they play on, and
/// what is hidden from them.
#[derive(Debug, Default)]
pub(super) struct Sight {
    player: i64,
    /// Accounts whose games they see.
    sees: HashSet<i64>,
    /// Accounts whose playtime counts for them.
    plays: HashSet<i64>,
    /// Each account's owner.
    owner: HashMap<i64, i64>,
    /// Titles they hid from themself.
    hidden_self: HashSet<i64>,
    /// Every title they hid, with its scope.
    own_scopes: HashMap<i64, HideScope>,
    /// (player, title): a player's own copies another hid from everyone else.
    hidden_by: HashSet<(i64, i64)>,
}

impl Sight {
    pub(super) fn of(c: &Connection, p: PlayerId) -> Result<Self> {
        let mut s = Sight { player: p.0, ..Default::default() };
        let mut st = c.prepare(
            "SELECT a.id, k.owner_id, a.sharing, x.offered, x.uses, x.plays_on
             FROM store_accounts a JOIN store_connections k ON k.id = a.connection_id
             LEFT JOIN store_account_players x ON x.account_id = a.id AND x.player_id = ?1",
        )?;
        let rows = st.query_map(params![p], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, Option<i64>>(4)?,
                r.get::<_, Option<i64>>(5)?,
            ))
        })?;
        for row in rows {
            let (account, owner, sharing, offered, uses, plays_on) = row?;
            s.owner.insert(account, owner);
            if owner == p.0 {
                s.sees.insert(account);
                s.plays.insert(account);
                continue;
            }
            let offered = match Sharing::parse(&sharing) {
                Some(Sharing::Everyone) => true,
                Some(Sharing::Chosen) => offered == Some(1),
                _ => false,
            };
            if offered && uses.unwrap_or(1) != 0 {
                s.sees.insert(account);
                if plays_on == Some(1) {
                    s.plays.insert(account);
                }
            }
        }
        let mut st = c.prepare("SELECT player_id, title_id, scope FROM hidden_titles")?;
        for row in st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)))? {
            let (who, title, scope) = row?;
            let Some(scope) = HideScope::parse(&scope) else { continue };
            if who == p.0 {
                s.own_scopes.insert(title, scope);
                if scope.hides_from_self() {
                    s.hidden_self.insert(title);
                }
            }
            if who != p.0 && scope.hides_from_others() {
                s.hidden_by.insert((who, title));
            }
        }
        Ok(s)
    }

    /// Whether they see the copy of `title` in `account`.
    pub(super) fn sees(&self, account: i64, title: i64) -> bool {
        self.sees.contains(&account)
            && match self.owner.get(&account) {
                Some(&owner) if owner != self.player => !self.hidden_by.contains(&(owner, title)),
                _ => true,
            }
    }

    /// Whether they see the games in `account` at all.
    pub(super) fn sees_account(&self, account: i64) -> bool {
        self.sees.contains(&account)
    }

    /// Whether the copy's playtime counts for them.
    pub(super) fn plays(&self, account: i64) -> bool {
        self.plays.contains(&account)
    }

    /// Whether the account is one of their own.
    pub(super) fn owns(&self, account: i64) -> bool {
        self.owner.get(&account) == Some(&self.player)
    }

    /// How they hid the title, when they did.
    pub(super) fn hid_as(&self, title: i64) -> Option<HideScope> {
        self.own_scopes.get(&title).copied()
    }

    /// Whether they hid the title from themself.
    pub(super) fn hid(&self, title: i64) -> bool {
        self.hidden_self.contains(&title)
    }

    /// See a title they hid from themself after all (their hidden games'
    /// pages, while their Hidden by me list is open).
    pub(super) fn unhide(&mut self, title: i64) {
        self.hidden_self.remove(&title);
    }
}

impl Db {
    // -- players --------------------------------------------------------------

    /// Every player, by id.
    pub fn players(&self) -> Result<Vec<Player>> {
        self.with(|c| {
            let mut st = c.prepare(&format!("SELECT {} FROM players ORDER BY id", super::accounts::PLAYER_COLUMNS))?;
            let rows = st.query_map([], super::accounts::player_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// A row of the player's own `player_settings` that the settings API
    /// never reads or writes, since `PLAYER_KEYS` leaves it out: what a part
    /// of UpNext keeps about the player for itself, such as where their own
    /// setup got to.
    pub fn private_setting(&self, p: PlayerId, key: &str) -> Result<Option<String>> {
        debug_assert!(!PLAYER_KEYS.contains(&key));
        self.with(|c| {
            Ok(c.query_row("SELECT value FROM player_settings WHERE player_id=?1 AND key=?2", params![p, key], |r| r.get(0)).optional()?)
        })
    }

    /// Change or (with `None`) remove one of the player's private rows.
    pub fn set_private_setting(&self, p: PlayerId, key: &str, value: Option<&str>) -> Result<()> {
        debug_assert!(!PLAYER_KEYS.contains(&key));
        self.with(|c| {
            match value {
                Some(v) => c.execute(
                    "INSERT INTO player_settings(player_id, key, value) VALUES (?1, ?2, ?3)
                     ON CONFLICT(player_id, key) DO UPDATE SET value=excluded.value",
                    params![p, key, v],
                )?,
                None => c.execute("DELETE FROM player_settings WHERE player_id=?1 AND key=?2", params![p, key])?,
            };
            Ok(())
        })
    }

    /// Add a player with the next colour. Their library starts as whatever
    /// is shared with everyone.
    pub fn add_player(&self, name: &str) -> Result<PlayerId> {
        let name = name.trim();
        if name.is_empty() {
            bail!("a player needs a name");
        }
        self.with(|c| {
            let taken: bool = c.query_row("SELECT 1 FROM players WHERE name=?1", params![name], |_| Ok(())).optional()?.is_some();
            if taken {
                bail!("there is already a player called {name}");
            }
            let n: i64 = c.query_row("SELECT COUNT(*) FROM players", [], |r| r.get(0))?;
            c.execute(
                "INSERT INTO players(name, colour, created_at) VALUES (?1, ?2, ?3)",
                params![name, COLOURS[n as usize % COLOURS.len()], now()],
            )?;
            Ok(PlayerId(c.last_insert_rowid()))
        })
    }

    // -- store accounts -------------------------------------------------------

    /// The account the player's connection of kind `launcher` (made when
    /// they have none) keeps `store` in, made when there is none yet.
    pub fn store_account(&self, p: PlayerId, launcher: &str, store: &str) -> Result<i64> {
        self.with(|c| store_account_in(c, p, launcher, store))
    }

    /// The player's own account for `store`, when they have one.
    pub fn own_store_account(&self, p: PlayerId, store: &str) -> Result<Option<i64>> {
        self.with(|c| {
            Ok(c.query_row(
                "SELECT a.id FROM store_accounts a JOIN store_connections k ON k.id = a.connection_id
                 WHERE k.owner_id=?1 AND a.store=?2 ORDER BY a.id LIMIT 1",
                params![p, store],
                |r| r.get(0),
            )
            .optional()?)
        })
    }

    /// Who an account is offered to.
    pub fn set_account_sharing(&self, account: i64, sharing: Sharing) -> Result<()> {
        self.with(|c| {
            c.execute("UPDATE store_accounts SET sharing=?2 WHERE id=?1", params![account, sharing.as_str()])?;
            Ok(())
        })
    }

    /// Another player's part in an account: whether it is offered to them
    /// (for chosen sharing), whether they use it, and whether its playtime
    /// counts for them.
    pub fn set_account_player(&self, account: i64, p: PlayerId, offered: bool, uses: bool, plays_on: bool) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO store_account_players(account_id, player_id, offered, uses, plays_on) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(account_id, player_id) DO UPDATE SET offered=excluded.offered, uses=excluded.uses, plays_on=excluded.plays_on",
                params![account, p, offered, uses, plays_on],
            )?;
            Ok(())
        })
    }

    // -- hiding ---------------------------------------------------------------

    /// "Hide this game" for the player, or unhide it (`None`). Hiding it from
    /// the others needs a copy in an account the player owns.
    pub fn set_hidden(&self, p: PlayerId, title_id: i64, scope: Option<HideScope>) -> Result<()> {
        self.with(|c| {
            match scope {
                Some(scope) => {
                    if scope.hides_from_others() {
                        let owns: bool = c
                            .query_row(
                                "SELECT 1 FROM games g JOIN store_accounts a ON a.id = g.account_id
                                 JOIN store_connections k ON k.id = a.connection_id
                                 WHERE g.title_id=?1 AND g.removed=0 AND k.owner_id=?2 LIMIT 1",
                                params![title_id, p],
                                |_| Ok(()),
                            )
                            .optional()?
                            .is_some();
                        if !owns {
                            bail!("only a player with their own copy of a game can hide it from the others");
                        }
                    }
                    c.execute(
                        "INSERT INTO hidden_titles(player_id, title_id, scope, created_at) VALUES (?1, ?2, ?3, ?4)
                         ON CONFLICT(player_id, title_id) DO UPDATE SET scope=excluded.scope, created_at=excluded.created_at",
                        params![p, title_id, scope.as_str(), now()],
                    )?;
                }
                None => {
                    c.execute("DELETE FROM hidden_titles WHERE player_id=?1 AND title_id=?2", params![p, title_id])?;
                }
            }
            Ok(())
        })
    }

    /// Who still sees a game `p` hid from the others: the players it is
    /// still in a library of, each with whether that's their own copy or
    /// another player's shared account. Nobody for a game hidden only from
    /// `p`, which never hid it from anyone else.
    pub fn still_seen_by(&self, p: PlayerId, title_id: i64, scope: HideScope) -> Result<Vec<StillSeen>> {
        if !scope.hides_from_others() {
            return Ok(vec![]);
        }
        let mut out = vec![];
        for other in self.players()?.into_iter().filter(|o| o.id != p) {
            if let Some(g) = self.game(other.id, title_id)? {
                out.push(StillSeen { name: other.name, own_copy: g.own_copy });
            }
        }
        Ok(out)
    }

    /// Every game the player hid, newest first: their Hidden by me list.
    pub fn hidden_by(&self, p: PlayerId) -> Result<Vec<HiddenTitle>> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT h.title_id, COALESCE(t.name, ''), h.scope, h.created_at,
                        EXISTS(SELECT 1 FROM games g JOIN store_accounts a ON a.id = g.account_id
                               JOIN store_connections k ON k.id = a.connection_id
                               WHERE g.title_id = h.title_id AND g.removed = 0 AND k.owner_id = h.player_id)
                 FROM hidden_titles h
                 LEFT JOIN titles t ON t.id = h.title_id WHERE h.player_id=?1 ORDER BY h.created_at DESC, h.title_id",
            )?;
            let rows = st.query_map(params![p], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get(3)?, r.get::<_, bool>(4)?))
            })?;
            let mut out = vec![];
            for row in rows {
                let (title_id, name, scope, at, own_copy) = row?;
                if let Some(scope) = HideScope::parse(&scope) {
                    out.push(HiddenTitle { title_id, name, scope, at, own_copy });
                }
            }
            Ok(out)
        })
    }
}

/// The player's account for `store` (see [`Db::store_account`]).
pub(super) fn store_account_in(c: &Connection, p: PlayerId, launcher: &str, store: &str) -> Result<i64> {
    let connection = match own_connection(c, p, launcher)? {
        Some(id) => id,
        None => {
            c.execute("INSERT INTO store_connections(kind, owner_id, created_at) VALUES (?1, ?2, ?3)", params![launcher, p, now()])?;
            c.last_insert_rowid()
        }
    };
    super::sync::account_for(c, connection, store)
}
