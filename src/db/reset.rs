//! What "Restart the setup wizard" can clear before the wizard starts:
//! sign-ins and keys, the AI choice, the taste profile, statuses and history,
//! or the whole library. `server::setup` takes a backup first.
//!
//! Every table is named in exactly one of the lists below, so a new table
//! has to say which part it belongs to (`every_table_has_a_part`).
//!
//! With two or more players, the taste profile and the history are
//! the asking player's own: only their rows go, and the others keep theirs.
//! The whole library is everyone's, so it still takes every player's.

use super::Db;
use anyhow::Result;
use rusqlite::{Connection, params};

/// Never cleared: the settings (cleared key by key), what the app keeps
/// about itself (meta, cleared key by key), the API tokens, the jobs and
/// their logs, and the record of AI calls and plan readings, which say what
/// was spent. Only the test reads it; it's the list a new table is checked
/// against.
#[cfg(test)]
const KEPT: &[&str] = &[
    "settings",
    "meta",
    "api_tokens",
    "jobs",
    "job_log",
    "job_failures",
    "ai_calls",
    "plan_readings",
    // The players and their accounts: sign-ins and keys in them
    // are cleared key by key, like the settings.
    "players",
    "player_settings",
    "store_connections",
    "store_accounts",
    "store_account_players",
    "ai_accounts",
    "ai_account_players",
];

/// The taste profile: what you told it, your routines, the patterns found,
/// and the play diary.
const TASTE: &[&str] = &["profile_statements", "routines", "play_diary"];

/// Statuses and history: the statuses and ratings you set, why you
/// stopped, Play later, and every lineup with what you did with its picks
/// and how you rated them, and your backlog verdicts as this computer
/// keeps them (the service still has the sent ones, and the key, a setting,
/// can list them again). Second Chance answers and "This game can be
/// played endlessly" are columns, cleared on their own.
const HISTORY: &[&str] = &[
    "overrides",
    "game_ratings",
    "stop_reasons",
    "play_later",
    "pick_batches",
    "picks",
    "pick_feedback",
    "pick_ratings",
    "hidden_titles",
    "my_verdicts",
    // What each player had on a title before a merge, for a split to give back.
    "merge_snapshot_marks",
];

/// The rest of the library: the games, how they're grouped, everything
/// fetched or written about them, and the wishlist.
const LIBRARY: &[&str] = &[
    "titles",
    "games",
    "merge_rules",
    "split_rules",
    "merge_snapshots",
    "steam_match",
    "source_sync",
    "details",
    "reviews",
    "tags",
    "review_texts",
    "gog_meta",
    "gog_reviews",
    "igdb_meta",
    "xref",
    "rawg_meta",
    "pcgw_meta",
    "protondb",
    "hltb",
    "title_series",
    "series_keys",
    "series_catalog",
    "series_guides",
    "steam_art",
    "trailers",
    "digests",
    "seed_applies",
    "community_verdicts",
    "second_chances",
    "hidden_gems",
    "wishlist",
];

/// The meta keys the taste profile keeps: the diary's last reading and its
/// last try, and when the patterns were last worked out.
const TASTE_META: &[&str] = &["diary_reading", "diary_tried", "diary_error", "taste_updated_at"];

/// Which parts to clear. The whole library takes the taste profile and the
/// history with it, since both are about its games.
#[derive(Debug, Clone)]
pub struct ResetParts {
    /// Settings to remove, so the environment's or the built-in value
    /// applies; a player's, a connection's or an AI account's as `player`
    /// reads them.
    pub settings: Vec<&'static str>,
    pub player: super::PlayerId,
    /// The Steam profile (name and picture), which goes with the SteamID.
    pub steam_profile: bool,
    pub taste: bool,
    pub history: bool,
    pub library: bool,
}

impl Db {
    /// Clear `parts` in one transaction.
    pub fn reset(&self, parts: &ResetParts) -> Result<()> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            // Tables that point at each other are cleared together; the keys
            // are checked at the commit.
            tx.execute_batch("PRAGMA defer_foreign_keys=ON;")?;
            for key in &parts.settings {
                super::setting_set_in(&tx, parts.player, key, None)?;
            }
            if parts.steam_profile {
                tx.execute("DELETE FROM meta WHERE key = 'profile'", [])?;
            }
            let players: i64 = tx.query_row("SELECT COUNT(*) FROM players", [], |r| r.get(0))?;
            if players > 1 && !parts.library {
                clear_for(&tx, parts)?;
                tx.commit()?;
                return Ok(());
            }
            if parts.taste || parts.library {
                clear(&tx, TASTE)?;
                for key in TASTE_META {
                    tx.execute("DELETE FROM meta WHERE key = ?1", params![key])?;
                }
                // Each Steam account's last reading.
                tx.execute("DELETE FROM meta WHERE key LIKE 'diary\\_reading.%' ESCAPE '\\'", [])?;
            }
            if parts.history || parts.library {
                clear(&tx, HISTORY)?;
                tx.execute("UPDATE second_chances SET answer=NULL, answered_at=NULL, prev_status=NULL, prev_at=NULL", [])?;
                tx.execute("UPDATE titles SET no_ending = NULL", [])?;
                // A merged-away title's ending switch too, so a split later
                // doesn't bring back what was cleared.
                tx.execute("UPDATE merge_snapshots SET no_ending = NULL", [])?;
            }
            if parts.library {
                clear(&tx, LIBRARY)?;
                // A library with no sync yet: no daily sync until the first.
                tx.execute("DELETE FROM meta WHERE key = 'last_sync'", [])?;
                // Without its games, the verdicts' snapshot is downloaded again.
                tx.execute("DELETE FROM meta WHERE key LIKE 'verdicts\\_%' ESCAPE '\\'", [])?;
            }
            tx.commit()?;
            Ok(())
        })
    }
}

fn clear(c: &Connection, tables: &[&str]) -> Result<()> {
    for t in tables {
        c.execute(&format!("DELETE FROM {t}"), [])?;
    }
    Ok(())
}

/// The tables of `TASTE` and `HISTORY` that are a player's through their
/// lineup rather than a column of their own.
const BY_LINEUP: &[&str] = &["picks", "pick_feedback", "pick_ratings"];

/// Clear the taste profile and the history of `parts.player` alone, with
/// other players in the household. "This game can be played endlessly" is
/// a fact about the game, so it stays for the others.
fn clear_for(c: &Connection, parts: &ResetParts) -> Result<()> {
    let p = parts.player;
    if parts.taste {
        for t in TASTE.iter().filter(|t| **t != "play_diary") {
            c.execute(&format!("DELETE FROM {t} WHERE player_id = ?1"), params![p])?;
        }
        // Their launches, and the hourly readings of the Steam accounts they own.
        c.execute(
            "DELETE FROM play_diary WHERE player_id = ?1 OR account_id IN
               (SELECT a.id FROM store_accounts a JOIN store_connections k ON k.id = a.connection_id WHERE k.owner_id = ?1)",
            params![p],
        )?;
        c.execute("DELETE FROM meta WHERE key IN (SELECT 'diary_reading.' || id FROM store_connections WHERE owner_id = ?1)", params![p])?;
    }
    if parts.history {
        for t in HISTORY.iter().filter(|t| BY_LINEUP.contains(t)) {
            c.execute(&format!("DELETE FROM {t} WHERE batch_id IN (SELECT id FROM pick_batches WHERE player_id = ?1)"), params![p])?;
        }
        for t in HISTORY.iter().filter(|t| !BY_LINEUP.contains(t)) {
            c.execute(&format!("DELETE FROM {t} WHERE player_id = ?1"), params![p])?;
        }
        c.execute(
            "UPDATE second_chances SET answer=NULL, answered_at=NULL, prev_status=NULL, prev_at=NULL WHERE player_id = ?1",
            params![p],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_table_has_a_part() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.db"), 60).unwrap();
        let mut tables: Vec<String> = db
            .with(|c| {
                let mut st = c.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?;
                Ok(st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?)
            })
            .unwrap();
        let mut named: Vec<String> = [KEPT, TASTE, HISTORY, LIBRARY].concat().iter().map(|t| t.to_string()).collect();
        tables.sort();
        named.sort();
        assert_eq!(named, tables, "each table in exactly one of KEPT, TASTE, HISTORY and LIBRARY");
    }

    #[test]
    fn with_several_players_a_reset_clears_only_the_askers_taste_and_history() {
        use crate::db::{ImportedGame, PlayerId};
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.db"), 60).unwrap();
        let portal = ImportedGame {
            source: "steam".into(),
            source_id: "620".into(),
            launcher: "steam".into(),
            name: "Portal 2".into(),
            appid: Some(620),
            playtime_minutes: Some(600),
            ..Default::default()
        };
        db.sync_source(PlayerId::FIRST, "steam", "steam", std::slice::from_ref(&portal)).unwrap();
        let sam = db.add_player("Sam").unwrap();
        db.sync_source(sam, "steam", "steam", &[portal]).unwrap();
        let title = db.title_for_appid(620).unwrap().unwrap();
        db.with(|c| {
            for p in [PlayerId::FIRST, sam] {
                c.execute("INSERT INTO overrides(player_id, title_id, status, updated_at) VALUES (?1, ?2, 'done', 1)", params![p, title])?;
                c.execute("INSERT INTO play_later(player_id, title_id, added_at) VALUES (?1, ?2, 1)", params![p, title])?;
                c.execute("INSERT INTO routines(name, created_at, updated_at, player_id) VALUES ('Friday', 1, 1, ?1)", params![p])?;
                c.execute(
                    "INSERT INTO profile_statements(kind, title_id, source, created_at, updated_at, player_id)
                     VALUES ('favourite', ?2, 'stated', 1, 1, ?1)",
                    params![p, title],
                )?;
                c.execute(
                    "INSERT INTO play_diary(hour, at, title_id, name, minutes, source, player_id) VALUES (1, 1, ?2, 'Portal 2', 30, 'launch', ?1)",
                    params![p, title],
                )?;
                let batch = format!("batch-{p}");
                c.execute("INSERT INTO pick_batches(id, created_at, prompt, player_id) VALUES (?1, 1, 'anything', ?2)", params![batch, p])?;
                c.execute("INSERT INTO pick_ratings(batch_id, title_id, verdict, at) VALUES (?1, ?2, 'good', 1)", params![batch, title])?;
            }
            c.execute("UPDATE titles SET no_ending = 1 WHERE id = ?1", params![title])?;
            Ok(())
        })
        .unwrap();
        let count = |table: &str, p: PlayerId| -> i64 {
            let sql = match table {
                "pick_ratings" => "SELECT COUNT(*) FROM pick_ratings WHERE batch_id IN (SELECT id FROM pick_batches WHERE player_id=?1)",
                t => &format!("SELECT COUNT(*) FROM {t} WHERE player_id=?1"),
            };
            db.with(|c| Ok(c.query_row(sql, params![p], |r| r.get(0))?)).unwrap()
        };
        let tables = ["overrides", "play_later", "routines", "profile_statements", "play_diary", "pick_batches", "pick_ratings"];

        db.reset(&ResetParts { settings: vec![], player: sam, steam_profile: false, taste: true, history: true, library: false }).unwrap();
        for t in tables {
            assert_eq!((count(t, sam), count(t, PlayerId::FIRST)), (0, 1), "{t}: Sam's gone, player 1's kept");
        }
        let no_ending: Option<i64> =
            db.with(|c| Ok(c.query_row("SELECT no_ending FROM titles WHERE id=?1", params![title], |r| r.get(0))?)).unwrap();
        assert_eq!(no_ending, Some(1), "a fact about the game stays for the others");

        // The whole library is everyone's.
        db.reset(&ResetParts { settings: vec![], player: sam, steam_profile: false, taste: false, history: false, library: true }).unwrap();
        for t in tables {
            assert_eq!(count(t, PlayerId::FIRST), 0, "{t}");
        }
    }

    /// Clearing history clears the ending switch a merge kept for a split,
    /// as it clears the titles' own.
    #[test]
    fn a_history_reset_clears_the_ending_switch_kept_for_a_split() {
        use crate::db::{ImportedGame, PlayerId};
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.db"), 60).unwrap();
        let game = |id: &str, name: &str| ImportedGame {
            source: "epic".into(),
            source_id: id.into(),
            launcher: "heroic".into(),
            name: name.into(),
            ..Default::default()
        };
        db.sync_source(PlayerId::FIRST, "heroic", "epic", &[game("a", "Alpha"), game("b", "Beta")]).unwrap();
        let ids = db.all_games(PlayerId::FIRST).unwrap();
        let id = |name: &str| ids.iter().find(|v| v.name == name).unwrap().id;
        let (a, b) = (id("Alpha"), id("Beta"));
        db.set_no_ending(b, Some(true)).unwrap();
        db.merge_titles(&[a, b]).unwrap();

        db.reset(&ResetParts {
            settings: vec![],
            player: PlayerId::FIRST,
            steam_profile: false,
            taste: false,
            history: true,
            library: false,
        })
        .unwrap();
        let copy: i64 = db.with(|c| Ok(c.query_row("SELECT id FROM games WHERE source_id='b'", [], |r| r.get(0))?)).unwrap();
        let back = db.split_copy(copy).unwrap();
        let no_ending: Option<i64> =
            db.with(|c| Ok(c.query_row("SELECT no_ending FROM titles WHERE id=?1", params![back], |r| r.get(0))?)).unwrap();
        assert_eq!(no_ending, None);
    }

    #[test]
    fn every_taste_and_history_table_is_someones() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.db"), 60).unwrap();
        db.with(|c| {
            for t in TASTE.iter().chain(HISTORY) {
                let mut st = c.prepare(&format!("SELECT name FROM pragma_table_info('{t}')"))?;
                let columns: Vec<String> = st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
                let column = if BY_LINEUP.contains(t) { "batch_id" } else { "player_id" };
                assert!(columns.iter().any(|c| c == column), "{t} has no {column}, so one player's reset can't find their rows");
            }
            Ok(())
        })
        .unwrap();
    }
}
