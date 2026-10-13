//! Writing what a launcher reports during sync.

use super::model::{ImportedGame, SyncSummary, is_native_launcher};
use super::players::{json_map, own_connection};
use super::{Db, PlayerId, now, to_json};
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

/// A connection a sync reads: a Steam account, a GOG or Epic
/// sign-in, a Heroic folder, a Galaxy database, this computer's EA app or
/// Ubisoft Connect, or a player's games added by hand. It belongs to one
/// player, and what it reads goes into that player's store accounts.
#[derive(Debug, Clone, PartialEq)]
pub struct SyncConnection {
    /// `store_connections.id`; None until the first sync that writes for
    /// a player with no connection of the kind makes it.
    pub id: Option<i64>,
    /// The launcher it reads as (`games.launcher`): steam, gog, heroic,
    /// galaxy, epic, origin, uplay or manual.
    pub kind: String,
    pub owner: PlayerId,
    /// Player 1's first connection of its kind. Only it falls back to
    /// this computer's own defaults (the environment's folders, the Steam
    /// client's signed-in user, the Epic Games Launcher's installs), so a
    /// second player's connection never reads the first player's files.
    pub first: bool,
    settings: serde_json::Map<String, Value>,
}

impl SyncConnection {
    /// One of its settings (a SteamID, a folder), when set.
    pub fn setting(&self, key: &str) -> Option<&str> {
        self.settings.get(key).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty())
    }

    /// A switch among its settings, when set.
    pub fn flag(&self, key: &str) -> Option<bool> {
        self.setting(key).map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
    }
}

/// The connection `id` is, or the owner's first of `kind` (made when they
/// have none).
fn connection_id(c: &Connection, conn: &SyncConnection) -> Result<i64> {
    if let Some(id) = conn.id {
        return Ok(id);
    }
    if let Some(id) = own_connection(c, conn.owner, &conn.kind)? {
        return Ok(id);
    }
    c.execute("INSERT INTO store_connections(kind, owner_id, created_at) VALUES (?1, ?2, ?3)", params![conn.kind, conn.owner, now()])?;
    Ok(c.last_insert_rowid())
}

/// The store account a connection's copies of `store` go into. A store's
/// own connection (a Steam account for Steam, a GOG sign-in for GOG) has an
/// account of its own, so a player with two Steam accounts has two
/// libraries; it takes over the account another of its owner's readers made
/// for that store before it was added. Every other reader (Heroic, Galaxy's
/// integrations, games added by hand) adds to its owner's account for the
/// store, the store's own connection's first, so a game Heroic and the GOG
/// sign-in both report is one copy.
pub(super) fn account_for(c: &Connection, connection: i64, store: &str) -> Result<i64> {
    let (kind, owner, settings): (String, i64, String) = c
        .query_row("SELECT kind, owner_id, settings FROM store_connections WHERE id=?1", params![connection], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .with_context(|| format!("store connection {connection}"))?;
    let found = c
        .query_row(
            "SELECT id FROM store_accounts WHERE connection_id=?1 AND store=?2 ORDER BY id LIMIT 1",
            params![connection, store],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = found {
        return Ok(id);
    }
    // The owner's account for the store under another of their connections:
    // for the store's own connection, one under another kind (to take over);
    // for any other reader, any, the store's own connection's first.
    let other: Option<i64> = c
        .query_row(
            "SELECT a.id FROM store_accounts a JOIN store_connections k ON k.id = a.connection_id
             WHERE k.owner_id=?1 AND a.store=?2 AND (?3 = 0 OR k.kind <> ?2) ORDER BY k.kind = ?2 DESC, a.id LIMIT 1",
            params![owner, store, kind == store],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = other {
        if kind == store {
            c.execute("UPDATE store_accounts SET connection_id=?2 WHERE id=?1", params![id, connection])?;
        }
        return Ok(id);
    }
    let external = if store == "steam" && kind == "steam" {
        json_map(Some(settings)).get("steam_id").and_then(Value::as_str).map(str::to_string)
    } else {
        None
    };
    c.execute(
        "INSERT INTO store_accounts(connection_id, store, external_id, name, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![connection, store, external, crate::galaxy::platform_label(store), now()],
    )?;
    Ok(c.last_insert_rowid())
}

impl Db {
    /// When each title was first seen: its oldest copy's first sync.
    pub fn titles_first_seen(&self) -> Result<Vec<i64>> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT MIN(first_seen_at) FROM games WHERE removed=0 AND title_id IS NOT NULL GROUP BY title_id")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            Ok(rows.collect::<std::result::Result<_, _>>()?)
        })
    }

    // -- sync ---------------------------------------------------------------

    /// Every connection of `kind`, player 1's first one ahead of the rest
    /// (oldest first). When player 1 has none, one for them that the first
    /// write makes still leads, since this computer's defaults are theirs.
    pub fn sync_connections(&self, kind: &str) -> Result<Vec<SyncConnection>> {
        self.with(|c| {
            let mut st = c.prepare("SELECT id, owner_id, settings FROM store_connections WHERE kind=?1 ORDER BY id")?;
            let rows = st.query_map(params![kind], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)))?;
            let mut out: Vec<SyncConnection> = vec![];
            for row in rows {
                let (id, owner, settings) = row?;
                out.push(SyncConnection {
                    id: Some(id),
                    kind: kind.to_string(),
                    owner: PlayerId(owner),
                    first: false,
                    settings: json_map(Some(settings)),
                });
            }
            match out.iter().position(|conn| conn.owner == PlayerId::FIRST) {
                Some(i) => {
                    let mut first = out.remove(i);
                    first.first = true;
                    out.insert(0, first);
                }
                None => out.insert(
                    0,
                    SyncConnection { id: None, kind: kind.to_string(), owner: PlayerId::FIRST, first: true, settings: Default::default() },
                ),
            }
            Ok(out)
        })
    }

    /// The connection a sync of `kind` for `owner` writes to: their first of
    /// that kind, or a new one made by the write.
    pub fn own_sync_connection(&self, owner: PlayerId, kind: &str) -> Result<SyncConnection> {
        let all = self.sync_connections(kind)?;
        Ok(match all.into_iter().find(|c| c.owner == owner) {
            Some(c) => c,
            None => SyncConnection { id: None, kind: kind.to_string(), owner, first: false, settings: Default::default() },
        })
    }

    /// Whether any connection of `kind` holds a sign-in (GOG, Epic).
    pub fn signed_in(&self, kind: &str) -> Result<bool> {
        self.with(|c| {
            Ok(c.query_row(
                "SELECT 1 FROM store_connections WHERE kind=?1 AND secret IS NOT NULL AND TRIM(secret) <> '' LIMIT 1",
                params![kind],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
        })
    }

    /// The store account `conn` writes its copies of `store` into, once
    /// it has one.
    pub fn connection_account(&self, conn: &SyncConnection, store: &str) -> Result<Option<i64>> {
        let Some(id) = conn.id else { return Ok(None) };
        self.with(|c| {
            Ok(c.query_row(
                "SELECT id FROM store_accounts WHERE connection_id=?1 AND store=?2 ORDER BY id LIMIT 1",
                params![id, store],
                |r| r.get(0),
            )
            .optional()?)
        })
    }

    /// Record the store's own id for the account `conn` keeps `store` in
    /// (a SteamID, which says whose library it is).
    pub fn set_account_external_id(&self, conn: &SyncConnection, store: &str, external: &str) -> Result<()> {
        let Some(id) = conn.id else { return Ok(()) };
        self.with(|c| {
            c.execute("UPDATE store_accounts SET external_id=?3 WHERE connection_id=?1 AND store=?2", params![id, store, external])?;
            Ok(())
        })
    }

    /// Upsert one launcher's view of one source into `owner`'s connection
    /// of kind `launcher` (their first, made when they have none); see
    /// [`Db::sync_into`].
    pub fn sync_source(&self, owner: PlayerId, launcher: &str, source: &str, games: &[ImportedGame]) -> Result<SyncSummary> {
        let mut conn = self.own_sync_connection(owner, launcher)?;
        self.sync_into(&mut conn, source, games)
    }

    /// Upsert what connection `conn` reports of one source into the store
    /// account it keeps that store in ([`account_for`]); a connection not
    /// made yet is made, and `conn.id` set. Rows of the same (launcher,
    /// account) missing from `games` are marked removed. A row owned by
    /// another launcher is only enriched (playtime, installed) when the
    /// incoming launcher is not native; a native launcher takes ownership back.
    pub fn sync_into(&self, conn: &mut SyncConnection, source: &str, games: &[ImportedGame]) -> Result<SyncSummary> {
        self.sync_rows(conn, source, games, true)
    }

    /// [`Db::sync_into`] for a read that sees only part of the account's
    /// library, as a Steam account other than the one signed in to the
    /// client does: what it sees is added or updated, and nothing is marked
    /// removed for being missing.
    pub fn sync_seen_into(&self, conn: &mut SyncConnection, source: &str, games: &[ImportedGame]) -> Result<SyncSummary> {
        self.sync_rows(conn, source, games, false)
    }

    fn sync_rows(&self, conn: &mut SyncConnection, source: &str, games: &[ImportedGame], whole: bool) -> Result<SyncSummary> {
        let ts = now();
        let launcher = conn.kind.clone();
        let launcher = launcher.as_str();
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            let connection = connection_id(&tx, conn)?;
            let account = account_for(&tx, connection, source)?;
            let mut summary = SyncSummary { source: source.to_string(), total: games.len(), ..Default::default() };
            // Rows this sync saw; the launcher's other rows for the source are gone.
            let mut seen: Vec<i64> = Vec::with_capacity(games.len());
            {
                let mut find = tx.prepare("SELECT id, launcher FROM games WHERE account_id=?1 AND source_id=?2")?;
                let mut insert = tx.prepare(
                    "INSERT INTO games(source,source_id,launcher,name,appid,steam_matched,installed,playtime_forever,playtime_known,
                        playtime_2weeks,rtime_last_played,art_cover,art_square,l_description,l_genres,l_developer,l_release,
                        store_url,run_url,first_seen_at,last_synced_at,removed,account_id)
                     VALUES(?1,?2,?3,?4,?5,0,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?19,0,?20)",
                )?;
                let mut update_full = tx.prepare(
                    "UPDATE games SET launcher=?2, name=?3, appid=COALESCE(?4, appid), installed=?5,
                        playtime_forever=CASE WHEN ?7 THEN ?6 ELSE playtime_forever END,
                        playtime_known=CASE WHEN ?7 THEN 1 ELSE playtime_known END,
                        playtime_2weeks=?8, rtime_last_played=CASE WHEN ?9 > 0 THEN ?9 ELSE rtime_last_played END,
                        art_cover=COALESCE(?10, art_cover), art_square=COALESCE(?11, art_square),
                        l_description=COALESCE(?12, l_description), l_genres=COALESCE(?13, l_genres),
                        l_developer=COALESCE(?14, l_developer), l_release=COALESCE(?15, l_release),
                        store_url=COALESCE(?16, store_url), run_url=COALESCE(?17, run_url),
                        last_synced_at=?18, removed=0
                     WHERE id=?1",
                )?;
                // A non-native launcher never lowers playtime the owner reported.
                let mut update_soft = tx.prepare(
                    "UPDATE games SET installed = installed OR ?2,
                        playtime_forever=CASE WHEN ?4 THEN MAX(?3, CASE WHEN playtime_known THEN playtime_forever ELSE 0 END) ELSE playtime_forever END,
                        playtime_known=CASE WHEN ?4 THEN 1 ELSE playtime_known END,
                        rtime_last_played=CASE WHEN ?5 > rtime_last_played THEN ?5 ELSE rtime_last_played END,
                        art_cover=COALESCE(art_cover, ?6), art_square=COALESCE(art_square, ?7),
                        store_url=COALESCE(store_url, ?8), l_genres=COALESCE(l_genres, ?9), l_release=COALESCE(l_release, ?10)
                     WHERE id=?1",
                )?;
                for g in games {
                    if g.name.trim().is_empty() {
                        summary.skipped += 1;
                        continue;
                    }
                    let existing: Option<(i64, String)> =
                        find.query_row(params![account, g.source_id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
                    let genres = if g.genres.is_empty() { None } else { Some(to_json(&g.genres)) };
                    match existing {
                        None => {
                            insert.execute(params![
                                g.source,
                                g.source_id,
                                g.launcher,
                                g.name,
                                g.appid,
                                g.installed as i64,
                                g.playtime_minutes.unwrap_or(0),
                                g.playtime_minutes.is_some() as i64,
                                g.playtime_2weeks,
                                g.last_played,
                                g.art_cover,
                                g.art_square,
                                g.description,
                                genres,
                                g.developer,
                                g.release_date,
                                g.store_url,
                                g.run_url,
                                ts,
                                account
                            ])?;
                            seen.push(tx.last_insert_rowid());
                            summary.added += 1;
                        }
                        Some((id, owner)) => {
                            seen.push(id);
                            let incoming_native = is_native_launcher(&g.launcher);
                            let owner_native = is_native_launcher(&owner);
                            if incoming_native || !owner_native {
                                update_full.execute(params![
                                    id,
                                    g.launcher,
                                    g.name,
                                    g.appid,
                                    g.installed as i64,
                                    g.playtime_minutes.unwrap_or(0),
                                    g.playtime_minutes.is_some() as i64,
                                    g.playtime_2weeks,
                                    g.last_played,
                                    g.art_cover,
                                    g.art_square,
                                    g.description,
                                    genres,
                                    g.developer,
                                    g.release_date,
                                    g.store_url,
                                    g.run_url,
                                    ts
                                ])?;
                            } else {
                                update_soft.execute(params![
                                    id,
                                    g.installed as i64,
                                    g.playtime_minutes.unwrap_or(0),
                                    g.playtime_minutes.is_some() as i64,
                                    g.last_played,
                                    g.art_cover,
                                    g.art_square,
                                    g.store_url,
                                    genres,
                                    g.release_date
                                ])?;
                            }
                            summary.updated += 1;
                        }
                    }
                }
            }
            // By id, not by timestamp: two syncs in the same second must still see removals.
            let count = if whole {
                summary.removed = tx.execute(
                    "UPDATE games SET removed=1 WHERE launcher=?1 AND account_id=?2 AND removed=0
                       AND id NOT IN (SELECT value FROM json_each(?3))",
                    params![launcher, account, to_json(&seen)],
                )?;
                games.len() as i64
            } else {
                // The account's count is what it has, not what this read saw.
                tx.query_row(
                    "SELECT COUNT(*) FROM games WHERE launcher=?1 AND account_id=?2 AND removed=0",
                    params![launcher, account],
                    |r| r.get::<_, i64>(0),
                )?
            };
            tx.execute(
                "INSERT INTO source_sync(account_id,launcher,last_sync,count) VALUES(?1,?2,?3,?4)
                 ON CONFLICT(account_id) DO UPDATE SET launcher=excluded.launcher, last_sync=excluded.last_sync, count=excluded.count",
                params![account, launcher, ts, count],
            )?;
            tx.execute(
                "INSERT INTO meta(key,value) VALUES('last_sync',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![ts.to_string()],
            )?;
            summary.merged_titles = Self::assign_titles_in(&tx)?;
            tx.commit()?;
            conn.id = Some(connection);
            Ok(summary)
        })
    }

    /// Copies `owner`'s connections of kind `launcher` brought in, by
    /// source, not counting removed ones.
    pub fn launcher_sources(&self, owner: PlayerId, launcher: &str) -> Result<Vec<(String, usize)>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT g.source, COUNT(*) FROM games g JOIN store_accounts a ON a.id = g.account_id
                 JOIN store_connections k ON k.id = a.connection_id
                 WHERE g.launcher=?1 AND k.owner_id=?2 AND g.removed=0 GROUP BY g.source ORDER BY g.source",
            )?;
            let rows = stmt.query_map(params![launcher, owner], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?.max(0) as usize)))?;
            Ok(rows.collect::<std::result::Result<_, _>>()?)
        })
    }

    /// The copies of `source` that `owner`'s connections of kind `launcher`
    /// brought in and that aren't removed, as a sync would write them again:
    /// name, links and art, by name. Playtime is left unknown and
    /// `installed` false, so writing one back changes neither.
    pub fn launcher_copies(&self, owner: PlayerId, launcher: &str, source: &str) -> Result<Vec<ImportedGame>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT g.source_id, g.name, g.store_url, g.run_url, g.art_cover, g.art_square FROM games g
                 JOIN store_accounts a ON a.id = g.account_id JOIN store_connections k ON k.id = a.connection_id
                 WHERE g.launcher=?1 AND g.source=?2 AND k.owner_id=?3 AND g.removed=0 ORDER BY g.name COLLATE NOCASE",
            )?;
            let rows = stmt.query_map(params![launcher, source, owner], |r| {
                Ok(ImportedGame {
                    source: source.to_string(),
                    source_id: r.get(0)?,
                    launcher: launcher.to_string(),
                    name: r.get(1)?,
                    store_url: r.get(2)?,
                    run_url: r.get(3)?,
                    art_cover: r.get(4)?,
                    art_square: r.get(5)?,
                    ..Default::default()
                })
            })?;
            Ok(rows.collect::<std::result::Result<_, _>>()?)
        })
    }

    /// Mark every copy of `source` that `owner`'s connections of kind
    /// `launcher` brought in removed, as a sync that no longer sees them
    /// would; how many there were. The store's own launcher's copies stay.
    pub fn remove_launcher_source(&self, owner: PlayerId, launcher: &str, source: &str) -> Result<usize> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            let n = tx.execute(
                "UPDATE games SET removed=1 WHERE launcher=?1 AND source=?2 AND removed=0 AND account_id IN (
                     SELECT a.id FROM store_accounts a JOIN store_connections k ON k.id = a.connection_id WHERE k.owner_id=?3)",
                params![launcher, source, owner],
            )?;
            if n > 0 {
                Self::assign_titles_in(&tx)?;
            }
            tx.commit()?;
            Ok(n)
        })
    }

    /// Record a Galaxy import as a whole (it spans several sources) on its
    /// connection, made when it isn't yet.
    pub fn record_galaxy_sync(&self, conn: &mut SyncConnection, count: usize) -> Result<()> {
        self.with(|c| {
            let id = connection_id(c, conn)?;
            c.execute("UPDATE store_connections SET last_sync=?2, count=?3 WHERE id=?1", params![id, now(), count as i64])?;
            conn.id = Some(id);
            Ok(())
        })
    }
}
