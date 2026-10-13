//! Players, their store connections and accounts, and their AI accounts as
//! the API manages them: who sees what, who may change
//! it, PINs, and removing a player. `players.rs` holds the types and the
//! rules for what a player sees; this file holds the changes.

use super::players::{
    AI_KEYS, AI_SECRET_KEYS, COLOURS, CONNECTION_KEYS, LIBRARY_AI, Player, PlayerId, Sharing, ai_offered, first_ai_account, json_map,
    library_ai_account, settings_with_ai,
};
use super::{Db, now};
use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::HashMap;

/// The household setting naming the player a request that names nobody
/// gets: the lowest id when unset.
pub const DEFAULT_PLAYER: &str = "default_player";

/// The kinds of connection a player can add ("What an account
/// is"): a Steam account, a GOG or Epic sign-in, a Heroic folder, a Galaxy
/// database. EA, Ubisoft and games added by hand come with the player's
/// first connection of their kind.
pub const CONNECTION_KINDS: &[&str] = &["steam", "gog", "epic", "heroic", "galaxy"];

/// A PIN: four to eight digits.
pub fn pin_ok(pin: &str) -> bool {
    (4..=8).contains(&pin.len()) && pin.bytes().all(|b| b.is_ascii_digit())
}

/// A PIN's Argon2id hash with a random salt, in the PHC string form.
fn pin_hash(pin: &str) -> Result<String> {
    use argon2::password_hash::PasswordHasher;
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|e| anyhow::anyhow!("no randomness for a PIN: {e}"))?;
    let hash = argon2::Argon2::default().hash_password_with_salt(pin.as_bytes(), &salt);
    Ok(hash.map_err(|e| anyhow::anyhow!("hashing a PIN: {e}"))?.to_string())
}

/// Whether `pin` is the one `hash` was made from. A hash that doesn't parse
/// matches nothing.
fn pin_matches(hash: &str, pin: &str) -> bool {
    use argon2::password_hash::PasswordVerifier;
    argon2::Argon2::default().verify_password(pin.as_bytes(), hash).is_ok()
}

/// What a player's settings are read from (`Config::with_db_for`): every
/// stored value as they read it, the AI account those AI values are from,
/// and whether that is the first AI account, the one the environment's AI
/// settings and this computer's signed-in CLIs belong to.
#[derive(Debug, Clone, Default)]
pub struct SettingsView {
    pub stored: HashMap<String, String>,
    pub ai_account: Option<i64>,
    pub machine_ai: bool,
}

/// One store account as a player sees it in Settings.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct StoreAccount {
    pub id: i64,
    pub connection_id: i64,
    /// The connection's kind: steam, gog, epic, heroic, galaxy, origin, uplay or manual.
    pub kind: String,
    /// The store its games are from (a copy's source).
    pub store: String,
    pub name: String,
    #[schema(value_type = i64)]
    pub owner_id: PlayerId,
    pub owner: String,
    #[schema(value_type = String)]
    pub sharing: Sharing,
    /// Copies in it, not counting removed ones.
    pub games: i64,
    /// Whether the asker owns it.
    pub mine: bool,
    /// Whether the asker sees its games: their own, or offered and switched on.
    pub uses: bool,
    /// Whether its playtime counts for the asker.
    pub plays_on: bool,
    /// For its owner: each other player's part in it.
    pub players: Vec<AccountPlayer>,
}

/// Another player's part in an account, for its owner.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct AccountPlayer {
    #[schema(value_type = i64)]
    pub player_id: PlayerId,
    pub name: String,
    /// With chosen sharing: whether it is offered to them.
    pub offered: bool,
    /// Their own switch.
    pub uses: bool,
    /// The owner's tick: its playtime counts for them too.
    pub plays_on: bool,
}

/// A store connection, for its owner. Its sign-in never comes back.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct StoreConnection {
    pub id: i64,
    pub kind: String,
    /// Its settings: the SteamID, a folder, a database and its skip list.
    #[schema(value_type = Object)]
    pub settings: Map<String, Value>,
    /// Whether it holds a sign-in (GOG, Epic).
    pub signed_in: bool,
    pub last_sync: Option<i64>,
    pub count: Option<i64>,
}

/// An AI account as a player sees it.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct AiAccount {
    pub id: i64,
    #[schema(value_type = i64)]
    pub owner_id: PlayerId,
    pub owner: String,
    /// Its `llm_backend`, or null for the environment's or the built-in one.
    pub backend: Option<String>,
    /// Its models, address, effort and thinking settings (never its keys).
    #[schema(value_type = Object)]
    pub settings: Map<String, Value>,
    /// The keys it holds, by name (`anthropic_api_key`), never their values.
    pub keys_set: Vec<String>,
    #[schema(value_type = String)]
    pub sharing: Sharing,
    /// For its owner, with chosen sharing: the players it is offered to.
    #[schema(value_type = Vec<i64>)]
    pub offered_to: Vec<PlayerId>,
    pub mine: bool,
    /// Whether the asker uses it.
    pub in_use: bool,
    /// Whether the library's own AI work (summaries, series guides, hidden gems) runs on it.
    pub library: bool,
    /// The first account: the environment's AI settings and this
    /// computer's signed-in Claude and Codex CLIs are its.
    pub this_computer: bool,
}

/// Whether a store account is offered to `p` (not counting their switch).
fn account_offered(c: &Connection, p: PlayerId, account: i64) -> Result<bool> {
    Ok(c.query_row(
        "SELECT 1 FROM store_accounts a JOIN store_connections k ON k.id = a.connection_id WHERE a.id=?2 AND (k.owner_id=?1
            OR a.sharing='everyone'
            OR (a.sharing='chosen' AND EXISTS (SELECT 1 FROM store_account_players x WHERE x.account_id=a.id AND x.player_id=?1 AND x.offered=1)))",
        params![p, account],
        |_| Ok(()),
    )
    .optional()?
    .is_some())
}

/// The columns [`player_row`] reads.
pub(super) const PLAYER_COLUMNS: &str = "id, name, colour, pin_hash IS NOT NULL, created_at, oidc_issuer";

pub(super) fn player_row(r: &rusqlite::Row) -> rusqlite::Result<Player> {
    Ok(Player {
        id: PlayerId(r.get(0)?),
        name: r.get(1)?,
        colour: r.get(2)?,
        has_pin: r.get(3)?,
        created_at: r.get(4)?,
        oidc_issuer: r.get(5)?,
    })
}

/// The settings of a connection or AI account that may be shown: every
/// value that is text, keys never.
fn shown(text: String) -> Map<String, Value> {
    json_map(Some(text)).into_iter().filter(|(k, v)| v.is_string() && !AI_SECRET_KEYS.contains(&k.as_str())).collect()
}

/// A player's own value for one key in a JSON settings column, set or
/// cleared, as [`super::players::setting_set_in`] writes them.
fn put_json(text: Option<String>, key: &str, value: Option<&str>) -> Option<String> {
    let mut m = json_map(text);
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        Some(v) => m.insert(key.to_string(), Value::String(v.to_string())),
        None => m.remove(key),
    };
    (!m.is_empty()).then(|| Value::Object(m).to_string())
}

/// A store account's row: id, connection, kind, store, name, owner, sharing
/// and how many games it brings.
type AccountRow = (i64, i64, String, String, String, i64, String, i64);

/// An AI account's row: id, owner, backend, settings, secret and sharing.
type AiAccountRow = (i64, i64, Option<String>, String, Option<String>, String);

impl Db {
    // -- players --------------------------------------------------------------

    /// One player.
    pub fn player(&self, p: PlayerId) -> Result<Option<Player>> {
        self.with(|c| Ok(c.query_row(&format!("SELECT {PLAYER_COLUMNS} FROM players WHERE id=?1"), params![p], player_row).optional()?))
    }

    /// A player by id or by name, as the command line names them.
    pub fn find_player(&self, name_or_id: &str) -> Result<Option<Player>> {
        let wanted = name_or_id.trim();
        let players = self.players()?;
        if let Ok(id) = wanted.trim_start_matches('#').parse::<i64>()
            && let Some(p) = players.iter().find(|p| p.id.0 == id)
        {
            return Ok(Some(p.clone()));
        }
        Ok(players.into_iter().find(|p| p.name.eq_ignore_ascii_case(wanted)))
    }

    /// The player a request or command that names nobody acts for: the
    /// `default_player` setting while it names one, else the lowest id.
    pub fn default_player(&self) -> Result<PlayerId> {
        self.with(|c| {
            let set: Option<i64> = c
                .query_row(
                    "SELECT CAST(s.value AS INTEGER) FROM settings s JOIN players p ON p.id = CAST(s.value AS INTEGER) WHERE s.key=?1",
                    params![DEFAULT_PLAYER],
                    |r| r.get(0),
                )
                .optional()?;
            let id = match set {
                Some(id) => id,
                None => c.query_row("SELECT MIN(id) FROM players", [], |r| r.get::<_, Option<i64>>(0))?.unwrap_or(1),
            };
            Ok(PlayerId(id))
        })
    }

    /// Make `p` the default player.
    pub fn set_default_player(&self, p: PlayerId) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO settings(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![DEFAULT_PLAYER, p.0.to_string()],
            )?;
            Ok(())
        })
    }

    /// Rename a player; names are unique, ignoring case.
    pub fn rename_player(&self, p: PlayerId, name: &str) -> Result<()> {
        let name = super::clean_name(name);
        if name.is_empty() || name.chars().count() > 40 {
            bail!("a player's name is 1 to 40 characters");
        }
        self.with(|c| {
            let taken = c.query_row("SELECT 1 FROM players WHERE name=?1 AND id<>?2", params![name, p], |_| Ok(())).optional()?.is_some();
            if taken {
                bail!("there is already a player called {name}");
            }
            if c.execute("UPDATE players SET name=?2 WHERE id=?1", params![p, name])? == 0 {
                bail!("no player {p}");
            }
            Ok(())
        })
    }

    /// A player's colour: one of [`COLOURS`] or any `#rrggbb`.
    pub fn set_player_colour(&self, p: PlayerId, colour: &str) -> Result<()> {
        let colour = colour.trim();
        let hex = colour.len() == 7 && colour.starts_with('#') && colour[1..].bytes().all(|b| b.is_ascii_hexdigit());
        if !hex && !COLOURS.contains(&colour) {
            bail!("a colour is #rrggbb, such as {}", COLOURS[0]);
        }
        self.with(|c| {
            c.execute("UPDATE players SET colour=?2 WHERE id=?1", params![p, colour.to_ascii_lowercase()])?;
            Ok(())
        })
    }

    /// Set a player's PIN (four to eight digits), or clear it (`None`).
    pub fn set_pin(&self, p: PlayerId, pin: Option<&str>) -> Result<()> {
        let hash = match pin {
            Some(pin) if !pin_ok(pin) => bail!("a PIN is four to eight digits"),
            Some(pin) => Some(pin_hash(pin)?),
            None => None,
        };
        self.with(|c| {
            c.execute("UPDATE players SET pin_hash=?2 WHERE id=?1", params![p, hash])?;
            Ok(())
        })
    }

    /// Whether `pin` is player `p`'s PIN; false for a player without one.
    pub fn pin_matches(&self, p: PlayerId, pin: &str) -> Result<bool> {
        let hash: Option<String> =
            self.with(|c| Ok(c.query_row("SELECT pin_hash FROM players WHERE id=?1", params![p], |r| r.get(0)).optional()?.flatten()))?;
        Ok(hash.is_some_and(|h| pin_matches(&h, pin)))
    }

    /// Remove a player: their own rows, their connections with their
    /// accounts and copies, their AI accounts, and the tokens bound to them.
    /// The last player can't be removed. Titles left with no copy stay as
    /// a sync leaves a removed game's title, out of every library. The
    /// caller takes the backup.
    pub fn remove_player(&self, p: PlayerId) -> Result<()> {
        self.with(|c| {
            let n: i64 = c.query_row("SELECT COUNT(*) FROM players", [], |r| r.get(0))?;
            let exists = c.query_row("SELECT 1 FROM players WHERE id=?1", params![p], |_| Ok(())).optional()?.is_some();
            if !exists {
                bail!("no player {p}");
            }
            if n <= 1 {
                bail!("the last player can't be removed");
            }
            let tx = c.unchecked_transaction()?;
            tx.execute_batch("PRAGMA defer_foreign_keys=ON;")?;
            // Their connections, with everything that hangs off them.
            let connections: Vec<i64> = {
                let mut st = tx.prepare("SELECT id FROM store_connections WHERE owner_id=?1")?;
                st.query_map(params![p], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
            };
            for id in connections {
                remove_connection_in(&tx, id)?;
            }
            // Their AI accounts; whoever used one has none until they choose again.
            let library = library_ai_account(&tx)?;
            tx.execute("DELETE FROM ai_account_players WHERE account_id IN (SELECT id FROM ai_accounts WHERE owner_id=?1)", params![p])?;
            tx.execute("DELETE FROM ai_accounts WHERE owner_id=?1", params![p])?;
            if library.is_some() && library_ai_account(&tx)?.is_none() {
                tx.execute("DELETE FROM settings WHERE key=?1", params![LIBRARY_AI])?;
            }
            // Their own rows. A lineup's picks, feedback and ratings follow it.
            for sql in [
                "DELETE FROM pick_feedback WHERE batch_id IN (SELECT id FROM pick_batches WHERE player_id=?1)",
                "DELETE FROM pick_ratings WHERE batch_id IN (SELECT id FROM pick_batches WHERE player_id=?1)",
                "DELETE FROM picks WHERE batch_id IN (SELECT id FROM pick_batches WHERE player_id=?1)",
                "DELETE FROM pick_batches WHERE player_id=?1",
                "DELETE FROM overrides WHERE player_id=?1",
                "DELETE FROM stop_reasons WHERE player_id=?1",
                "DELETE FROM play_later WHERE player_id=?1",
                "DELETE FROM second_chances WHERE player_id=?1",
                "DELETE FROM profile_statements WHERE player_id=?1",
                "DELETE FROM play_diary WHERE player_id=?1",
                "DELETE FROM wishlist WHERE player_id=?1",
                "DELETE FROM game_ratings WHERE player_id=?1",
                "DELETE FROM routines WHERE player_id=?1",
                "DELETE FROM my_verdicts WHERE player_id=?1",
                "DELETE FROM hidden_titles WHERE player_id=?1",
                "DELETE FROM merge_snapshot_marks WHERE player_id=?1",
                "DELETE FROM player_settings WHERE player_id=?1",
                "DELETE FROM store_account_players WHERE player_id=?1",
                "DELETE FROM ai_account_players WHERE player_id=?1",
                "DELETE FROM api_tokens WHERE player_id=?1",
                // What they cost stays in the usage, without their name.
                "UPDATE ai_calls SET player_id=NULL WHERE player_id=?1",
                "DELETE FROM players WHERE id=?1",
            ] {
                tx.execute(sql, params![p])?;
            }
            tx.execute(
                "DELETE FROM settings WHERE key=?1 AND CAST(value AS INTEGER) NOT IN (SELECT id FROM players)",
                params![DEFAULT_PLAYER],
            )?;
            tx.commit()?;
            Ok(())
        })
    }

    /// What removing `p` takes from the others: the accounts of theirs the
    /// others use, by name, for the dialog that asks first.
    pub fn removal_takes(&self, p: PlayerId) -> Result<Vec<String>> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT a.name || ' (' || COUNT(g.id) || ' games)' FROM store_accounts a
                 JOIN store_connections k ON k.id = a.connection_id
                 LEFT JOIN games g ON g.account_id = a.id AND g.removed = 0
                 WHERE k.owner_id=?1 AND a.sharing <> 'owner' GROUP BY a.id ORDER BY a.id",
            )?;
            Ok(st.query_map(params![p], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
        })
    }

    // -- settings, as Config reads them -----------------------------------------

    /// What player `p`'s settings are read from.
    pub fn settings_view(&self, p: PlayerId) -> Result<SettingsView> {
        self.with(|c| {
            let ai = super::players::ai_account_in_use(c, p)?;
            Ok(SettingsView { stored: settings_with_ai(c, p, ai)?, ai_account: ai, machine_ai: ai.is_some() && ai == first_ai_account(c)? })
        })
    }

    /// What the library's own work reads: player 1's settings, whose first
    /// connections this computer's folders and sign-ins are, with the
    /// library's AI account in place of theirs.
    pub fn library_settings_view(&self) -> Result<SettingsView> {
        let p = PlayerId::FIRST;
        self.with(|c| {
            let ai = library_ai_account(c)?;
            Ok(SettingsView { stored: settings_with_ai(c, p, ai)?, ai_account: ai, machine_ai: ai.is_some() && ai == first_ai_account(c)? })
        })
    }

    // -- store connections and accounts -------------------------------------------

    /// Every store account `p` owns or is offered, theirs first.
    pub fn store_accounts_for(&self, p: PlayerId) -> Result<Vec<StoreAccount>> {
        self.with(|c| {
            let names: HashMap<i64, String> = {
                let mut st = c.prepare("SELECT id, name FROM players")?;
                st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
            };
            let mut st = c.prepare(
                "SELECT a.id, a.connection_id, k.kind, a.store, a.name, k.owner_id, a.sharing,
                        (SELECT COUNT(*) FROM games g WHERE g.account_id = a.id AND g.removed = 0)
                 FROM store_accounts a JOIN store_connections k ON k.id = a.connection_id
                 ORDER BY k.owner_id <> ?1, k.owner_id, a.id",
            )?;
            let rows: Vec<AccountRow> = st
                .query_map(params![p], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)))?
                .collect::<rusqlite::Result<_>>()?;
            let mut parts = c.prepare("SELECT player_id, offered, uses, plays_on FROM store_account_players WHERE account_id=?1")?;
            let mut out = vec![];
            for (id, connection_id, kind, store, name, owner, sharing, games) in rows {
                let sharing = Sharing::parse(&sharing).unwrap_or(Sharing::Owner);
                let mine = owner == p.0;
                let theirs: Vec<(i64, bool, bool, bool)> =
                    parts.query_map(params![id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<_>>()?;
                let own_part = theirs.iter().find(|t| t.0 == p.0);
                let offered = mine
                    || match sharing {
                        Sharing::Everyone => true,
                        Sharing::Chosen => own_part.is_some_and(|t| t.1),
                        Sharing::Owner => false,
                    };
                if !offered {
                    continue;
                }
                let players = if mine {
                    names
                        .iter()
                        .filter(|(pid, _)| **pid != p.0)
                        .map(|(pid, n)| {
                            let t = theirs.iter().find(|t| t.0 == *pid);
                            AccountPlayer {
                                player_id: PlayerId(*pid),
                                name: n.clone(),
                                offered: t.is_some_and(|t| t.1),
                                uses: t.is_none_or(|t| t.2),
                                plays_on: t.is_some_and(|t| t.3),
                            }
                        })
                        .collect::<Vec<_>>()
                } else {
                    vec![]
                };
                let mut players = players;
                players.sort_by_key(|x| x.player_id);
                out.push(StoreAccount {
                    id,
                    connection_id,
                    kind,
                    store,
                    name,
                    owner_id: PlayerId(owner),
                    owner: names.get(&owner).cloned().unwrap_or_default(),
                    sharing,
                    games,
                    mine,
                    uses: mine || own_part.is_none_or(|t| t.2),
                    plays_on: mine || own_part.is_some_and(|t| t.3),
                    players,
                });
            }
            Ok(out)
        })
    }

    /// Every connection `p` owns.
    pub fn connections_of(&self, p: PlayerId) -> Result<Vec<StoreConnection>> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT id, kind, settings, secret IS NOT NULL AND TRIM(secret) <> '', last_sync, count
                 FROM store_connections WHERE owner_id=?1 ORDER BY id",
            )?;
            let rows = st.query_map(params![p], |r| {
                Ok(StoreConnection {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    settings: shown(r.get(2)?),
                    signed_in: r.get(3)?,
                    last_sync: r.get(4)?,
                    count: r.get(5)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// The owner of a connection, when there is one.
    pub fn connection_owner(&self, id: i64) -> Result<Option<PlayerId>> {
        self.with(|c| {
            Ok(c.query_row("SELECT owner_id FROM store_connections WHERE id=?1", params![id], |r| r.get(0)).optional()?.map(PlayerId))
        })
    }

    /// A connection's kind (`steam`, `gog`, ...), when there is one.
    pub fn connection_kind(&self, id: i64) -> Result<Option<String>> {
        self.with(|c| Ok(c.query_row("SELECT kind FROM store_connections WHERE id=?1", params![id], |r| r.get(0)).optional()?))
    }

    /// The owner of a store account, when there is one.
    pub fn account_owner(&self, account: i64) -> Result<Option<PlayerId>> {
        self.with(|c| {
            Ok(c.query_row(
                "SELECT k.owner_id FROM store_accounts a JOIN store_connections k ON k.id = a.connection_id WHERE a.id=?1",
                params![account],
                |r| r.get(0),
            )
            .optional()?
            .map(PlayerId))
        })
    }

    /// Add a connection for `p` with `settings` (the keys
    /// [`CONNECTION_KEYS`] gives its kind). A Steam account starts with its
    /// own empty library; the next sync fills it.
    pub fn add_connection(&self, p: PlayerId, kind: &str, settings: &Map<String, Value>) -> Result<i64> {
        if !CONNECTION_KINDS.contains(&kind) {
            bail!("a connection is one of {}", CONNECTION_KINDS.join(", "));
        }
        let settings = connection_settings(kind, settings)?;
        self.with(|c| {
            c.execute(
                "INSERT INTO store_connections(kind, owner_id, settings, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![kind, p, Value::Object(settings.clone()).to_string(), now()],
            )?;
            let id = c.last_insert_rowid();
            if kind == "steam" || kind == "gog" || kind == "epic" {
                super::sync::account_for(c, id, kind)?;
            }
            Ok(id)
        })
    }

    /// Change a connection's settings: each key set, or cleared with null
    /// or "".
    pub fn set_connection_settings(&self, id: i64, settings: &Map<String, Value>) -> Result<()> {
        self.with(|c| {
            let (kind, text): (String, String) =
                c.query_row("SELECT kind, settings FROM store_connections WHERE id=?1", params![id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            let mut text = Some(text);
            for (key, value) in settings {
                if !CONNECTION_KEYS.iter().any(|(k, of)| k == key && *of == kind) {
                    bail!("{key} isn't a setting of a {kind} connection");
                }
                let v = match value {
                    Value::Null => None,
                    Value::String(s) if s.trim().is_empty() => None,
                    Value::String(s) => Some(crate::config::validate_connection_setting(key, s)?),
                    other => Some(crate::config::validate_connection_setting(key, &other.to_string())?),
                };
                text = put_json(text, key, v.as_deref());
            }
            let text = text.unwrap_or_else(|| "{}".into());
            c.execute("UPDATE store_connections SET settings=?2 WHERE id=?1", params![id, text])?;
            // A Steam account's SteamID says whose library it is.
            if kind == "steam" {
                let steam_id = json_map(Some(text)).get("steam_id").and_then(Value::as_str).map(str::to_string);
                c.execute("UPDATE store_accounts SET external_id=?2 WHERE connection_id=?1 AND store='steam'", params![id, steam_id])?;
            }
            Ok(())
        })
    }

    /// Remove a connection with its accounts and their copies, as removing
    /// its owner would.
    pub fn remove_connection(&self, id: i64) -> Result<()> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            tx.execute_batch("PRAGMA defer_foreign_keys=ON;")?;
            remove_connection_in(&tx, id)?;
            tx.commit()?;
            Ok(())
        })
    }

    /// Rename a store account.
    pub fn rename_account(&self, account: i64, name: &str) -> Result<()> {
        let name = super::clean_name(name);
        if name.is_empty() || name.chars().count() > 60 {
            bail!("an account's name is 1 to 60 characters");
        }
        self.with(|c| {
            c.execute("UPDATE store_accounts SET name=?2 WHERE id=?1", params![account, name])?;
            Ok(())
        })
    }

    /// The owner's part of another player's place in an account: whether
    /// it is offered to them (for chosen sharing) and whether they play on
    /// it. Their own switch is kept.
    pub fn set_account_offer(&self, account: i64, p: PlayerId, offered: Option<bool>, plays_on: Option<bool>) -> Result<()> {
        self.with(|c| {
            c.execute("INSERT OR IGNORE INTO store_account_players(account_id, player_id) VALUES (?1, ?2)", params![account, p])?;
            if let Some(o) = offered {
                c.execute("UPDATE store_account_players SET offered=?3 WHERE account_id=?1 AND player_id=?2", params![account, p, o])?;
            }
            if let Some(o) = plays_on {
                c.execute("UPDATE store_account_players SET plays_on=?3 WHERE account_id=?1 AND player_id=?2", params![account, p, o])?;
            }
            Ok(())
        })
    }

    /// A player's own switch for an account offered to them ("Use these
    /// games"). Refused for one that isn't offered to them.
    pub fn set_account_use(&self, account: i64, p: PlayerId, uses: bool) -> Result<()> {
        self.with(|c| {
            if !account_offered(c, p, account)? {
                bail!("that account isn't offered to you");
            }
            c.execute(
                "INSERT INTO store_account_players(account_id, player_id, uses) VALUES (?1, ?2, ?3)
                 ON CONFLICT(account_id, player_id) DO UPDATE SET uses=excluded.uses",
                params![account, p, uses],
            )?;
            Ok(())
        })
    }

    // -- AI accounts --------------------------------------------------------------

    /// Every AI account `p` owns or is offered, theirs first.
    pub fn ai_accounts_for(&self, p: PlayerId) -> Result<Vec<AiAccount>> {
        self.with(|c| {
            let names: HashMap<i64, String> = {
                let mut st = c.prepare("SELECT id, name FROM players")?;
                st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
            };
            let (in_use, library, first) = (super::players::ai_account_in_use(c, p)?, library_ai_account(c)?, first_ai_account(c)?);
            let mut st = c.prepare(
                "SELECT id, owner_id, backend, settings, secret, sharing FROM ai_accounts ORDER BY owner_id <> ?1, owner_id, id",
            )?;
            let rows: Vec<AiAccountRow> = st
                .query_map(params![p], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?
                .collect::<rusqlite::Result<_>>()?;
            let mut chosen = c.prepare("SELECT player_id FROM ai_account_players WHERE account_id=?1 ORDER BY player_id")?;
            let mut out = vec![];
            for (id, owner, backend, settings, secret, sharing) in rows {
                if !ai_offered(c, p, id)? {
                    continue;
                }
                let mine = owner == p.0;
                let offered_to: Vec<PlayerId> = if mine {
                    chosen
                        .query_map(params![id], |r| r.get(0))?
                        .collect::<rusqlite::Result<Vec<i64>>>()?
                        .into_iter()
                        .map(PlayerId)
                        .collect()
                } else {
                    vec![]
                };
                let mut keys_set: Vec<String> =
                    json_map(secret).into_iter().filter(|(_, v)| v.as_str().is_some_and(|s| !s.is_empty())).map(|(k, _)| k).collect();
                keys_set.sort();
                out.push(AiAccount {
                    id,
                    owner_id: PlayerId(owner),
                    owner: names.get(&owner).cloned().unwrap_or_default(),
                    backend,
                    settings: shown(settings).into_iter().filter(|(k, _)| AI_KEYS.contains(&k.as_str())).collect(),
                    keys_set,
                    sharing: Sharing::parse(&sharing).unwrap_or(Sharing::Owner),
                    offered_to,
                    mine,
                    in_use: in_use == Some(id),
                    library: library == Some(id),
                    this_computer: first == Some(id),
                });
            }
            Ok(out)
        })
    }

    /// The owner of an AI account, when there is one.
    pub fn ai_account_owner(&self, id: i64) -> Result<Option<PlayerId>> {
        self.with(|c| Ok(c.query_row("SELECT owner_id FROM ai_accounts WHERE id=?1", params![id], |r| r.get(0)).optional()?.map(PlayerId)))
    }

    /// Add an AI account for `p` with a backend (or none, for the built-in
    /// one) and settings, and keys from `secrets`. It isn't used by anyone
    /// until chosen, but a player with no AI account starts using it.
    pub fn add_ai_account(
        &self,
        p: PlayerId,
        backend: Option<&str>,
        settings: &[(String, Option<String>)],
        sharing: Sharing,
    ) -> Result<i64> {
        self.with(|c| {
            c.execute(
                "INSERT INTO ai_accounts(owner_id, backend, sharing, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![p, backend, sharing.as_str(), now()],
            )?;
            let id = c.last_insert_rowid();
            ai_settings_in(c, id, settings)?;
            if super::players::ai_account_in_use(c, p)?.is_none() {
                use_ai_in(c, p, id)?;
            }
            Ok(id)
        })
    }

    /// Change an AI account's backend (`Some(None)` clears it) and settings.
    pub fn set_ai_account(&self, id: i64, backend: Option<Option<&str>>, settings: &[(String, Option<String>)]) -> Result<()> {
        self.with(|c| {
            if let Some(b) = backend {
                c.execute("UPDATE ai_accounts SET backend=?2 WHERE id=?1", params![id, b])?;
            }
            ai_settings_in(c, id, settings)
        })
    }

    /// Who an AI account is offered to, and with chosen sharing, to whom.
    pub fn set_ai_sharing(&self, id: i64, sharing: Sharing, chosen: Option<&[PlayerId]>) -> Result<()> {
        self.with(|c| {
            c.execute("UPDATE ai_accounts SET sharing=?2 WHERE id=?1", params![id, sharing.as_str()])?;
            if let Some(players) = chosen {
                c.execute("DELETE FROM ai_account_players WHERE account_id=?1", params![id])?;
                for p in players {
                    c.execute("INSERT OR IGNORE INTO ai_account_players(account_id, player_id) VALUES (?1, ?2)", params![id, p])?;
                }
            }
            Ok(())
        })
    }

    /// Remove an AI account. Whoever used it has none until they choose
    /// another; the library's AI work stops until its owner offers one.
    pub fn remove_ai_account(&self, id: i64) -> Result<()> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            tx.execute("DELETE FROM ai_account_players WHERE account_id=?1", params![id])?;
            tx.execute("DELETE FROM ai_accounts WHERE id=?1", params![id])?;
            tx.execute("DELETE FROM settings WHERE key=?1 AND value=?2", params![LIBRARY_AI, id.to_string()])?;
            tx.commit()?;
            Ok(())
        })
    }

    /// Use AI account `id` for `p`'s picks: one they own or are offered.
    pub fn use_ai_account(&self, p: PlayerId, id: i64) -> Result<()> {
        self.with(|c| use_ai_in(c, p, id))
    }

    /// Run the library's own AI work on account `id`.
    pub fn set_library_ai(&self, id: i64) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO settings(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![LIBRARY_AI, id.to_string()],
            )?;
            Ok(())
        })
    }
}

fn use_ai_in(c: &Connection, p: PlayerId, id: i64) -> Result<()> {
    if !ai_offered(c, p, id)? {
        bail!("that AI account isn't yours or offered to you");
    }
    c.execute(
        "INSERT INTO player_settings(player_id, key, value) VALUES (?1, 'ai_account', ?2)
         ON CONFLICT(player_id, key) DO UPDATE SET value=excluded.value",
        params![p, id.to_string()],
    )?;
    Ok(())
}

/// Write AI settings and keys onto account `id`, each set or cleared.
fn ai_settings_in(c: &Connection, id: i64, settings: &[(String, Option<String>)]) -> Result<()> {
    let (mut text, mut secret): (Option<String>, Option<String>) =
        c.query_row("SELECT settings, secret FROM ai_accounts WHERE id=?1", params![id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    for (key, value) in settings {
        if AI_SECRET_KEYS.contains(&key.as_str()) {
            secret = put_json(secret, key, value.as_deref());
        } else if AI_KEYS.contains(&key.as_str()) {
            text = put_json(text, key, value.as_deref());
        } else {
            bail!("{key} isn't a setting of an AI account");
        }
    }
    c.execute("UPDATE ai_accounts SET settings=?2, secret=?3 WHERE id=?1", params![id, text.unwrap_or_else(|| "{}".into()), secret])?;
    Ok(())
}

/// A new connection's settings: only its kind's keys, each checked.
fn connection_settings(kind: &str, settings: &Map<String, Value>) -> Result<Map<String, Value>> {
    let mut out = Map::new();
    for (key, value) in settings {
        if !CONNECTION_KEYS.iter().any(|(k, of)| k == key && *of == kind) {
            bail!("{key} isn't a setting of a {kind} connection");
        }
        let text = match value {
            Value::Null => continue,
            Value::String(s) if s.trim().is_empty() => continue,
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        out.insert(key.clone(), Value::String(crate::config::validate_connection_setting(key, &text)?));
    }
    Ok(out)
}

/// Remove connection `id`, its accounts, their copies with what points at
/// them, and other players' parts in them. Foreign keys are checked at the
/// caller's commit.
fn remove_connection_in(c: &Connection, id: i64) -> Result<()> {
    let accounts = "SELECT id FROM store_accounts WHERE connection_id=?1";
    let copies = format!("SELECT id FROM games WHERE account_id IN ({accounts})");
    for sql in [
        format!("DELETE FROM merge_rules WHERE game_id IN ({copies}) OR with_game_id IN ({copies})"),
        format!("DELETE FROM split_rules WHERE game_id IN ({copies})"),
        format!("DELETE FROM merge_snapshot_marks WHERE game_id IN ({copies})"),
        format!("DELETE FROM merge_snapshots WHERE game_id IN ({copies})"),
        format!("DELETE FROM play_diary WHERE account_id IN ({accounts})"),
        format!("DELETE FROM source_sync WHERE account_id IN ({accounts})"),
        format!("DELETE FROM store_account_players WHERE account_id IN ({accounts})"),
        format!("DELETE FROM games WHERE account_id IN ({accounts})"),
        "DELETE FROM store_accounts WHERE connection_id=?1".to_string(),
        "DELETE FROM store_connections WHERE id=?1".to_string(),
    ] {
        c.execute(&sql, params![id])?;
    }
    c.execute("DELETE FROM meta WHERE key=?1", params![format!("diary_reading.{id}")])?;
    Ok(())
}
