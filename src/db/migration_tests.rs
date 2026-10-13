//! Opening databases. `tests/fixtures/schema/vN.sql` is the schema of a
//! database at version N (no rows), for every version from BASELINE_VERSION
//! on: each upgrade's test starts from the version before it. A database
//! from before BASELINE_VERSION is refused (`src/db/schema.rs` says why).

use super::*;
use crate::db::PlayerId;

/// Rows every schema from the baseline on can hold: Portal 2 on Steam and
/// Epic (one title) given up on, Hades on Epic marked done, with a pick and
/// HLTB times. From v32 the copies sit in player 1's accounts and the marks
/// and theme are player 1's; `seed` writes those parts for the version at hand.
const SEED: &str = r#"
INSERT INTO titles(id, key, name, appid) VALUES (1, 'appid:620', 'Portal 2', 620), (2, 'name:hades', 'Hades', NULL);
INSERT INTO games(id, source, source_id, launcher, name, appid, steam_matched, first_seen_at, last_synced_at, title_id{ACCOUNT}) VALUES
    (1, 'steam', '620', 'steam', 'Portal 2', 620, 0, 1, 1, 1{STEAM}),
    (2, 'epic', 'e1', 'heroic', 'Portal 2', 620, 1, 1, 1, 1{EPIC}),
    (3, 'epic', 'e2', 'heroic', 'Hades', NULL, 0, 1, 1, 2{EPIC});
INSERT INTO details(appid, fetched_at, success, type) VALUES (620, 1, 1, 'game');
INSERT INTO overrides(title_id, status, note, updated_at{PLAYER}) VALUES (2, 'done', 'beat it', 1{ONE}), (1, 'abandoned', NULL, 1{ONE});
INSERT INTO hltb(title_id, fetched_at, success, main_story) VALUES (1, 1, 1, 8.5);
INSERT INTO pick_batches(id, created_at, prompt, shown) VALUES ('b1', 1, 'something short', 1);
INSERT INTO picks(batch_id, position, title_id, name, reason, slot) VALUES ('b1', 0, 2, 'Hades', 'fits', 0);
INSERT INTO {THEME} VALUES ({ONE_}'theme', 'daylight');
"#;

/// Player 1 and their Steam and Heroic accounts, as v32 made them.
const PLAYERS_SEED: &str = r#"
INSERT INTO players(id, name, colour, created_at) VALUES (1, 'Player 1', '#e07a5f', 1);
INSERT INTO store_connections(id, kind, owner_id, created_at) VALUES (1, 'steam', 1, 1), (2, 'heroic', 1, 1);
INSERT INTO store_accounts(id, connection_id, store, name, created_at) VALUES (1, 1, 'steam', 'Steam', 1), (2, 2, 'epic', 'Epic Games', 1);
INSERT INTO ai_accounts(id, owner_id, created_at) VALUES (1, 1, 1);
INSERT INTO player_settings(player_id, key, value) VALUES (1, 'ai_account', '1');
INSERT INTO settings(key, value) VALUES ('library_ai', '1');
"#;

/// SEED for a database at `version`.
fn seed(version: i64) -> String {
    let players = version >= 32;
    let fill = |with: &'static str| if players { with } else { "" };
    let rows = SEED
        .replace("{ACCOUNT}", fill(", account_id"))
        .replace("{STEAM}", fill(", 1"))
        .replace("{EPIC}", fill(", 2"))
        .replace("{PLAYER}", fill(", player_id"))
        .replace("{ONE}", fill(", 1"))
        .replace("{THEME}", if players { "player_settings(player_id, key, value)" } else { "settings(key, value)" })
        .replace("{ONE_}", fill("1, "));
    if players { format!("{PLAYERS_SEED}{rows}") } else { rows }
}

/// A database made from `fixture` with SEED's rows, not opened by UpNext yet.
fn seeded(fixture: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("upnext.db");
    let c = Connection::open(&path).unwrap();
    c.execute_batch(&crate::testutil::fixture(fixture)).unwrap();
    c.execute_batch(&seed(user_version(&c))).unwrap();
    (dir, path)
}

fn user_version(c: &Connection) -> i64 {
    c.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap()
}

/// Everything in the schema, as stored, and every row of meta and settings.
fn snapshot(path: &std::path::Path) -> (i64, Vec<String>, Vec<(String, String)>) {
    let c = Connection::open(path).unwrap();
    let all = |sql: &str| -> Vec<(String, String)> {
        let mut s = c.prepare(sql).unwrap();
        s.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<std::result::Result<_, _>>().unwrap()
    };
    let schema =
        all("SELECT name, COALESCE(sql, '') FROM sqlite_master ORDER BY rowid").into_iter().map(|(n, s)| format!("{n}: {s}")).collect();
    let rows = all("SELECT 'meta.' || key, value FROM meta UNION ALL SELECT 'settings.' || key, value FROM settings ORDER BY 1");
    (user_version(&c), schema, rows)
}

/// The library SEED made, as UpNext reads it.
fn check_seeded_rows(db: &Db, what: &str) {
    let all = db.all_games(PlayerId::FIRST).unwrap_or_else(|e| panic!("{what}: {e:#}"));
    let portal = all.iter().find(|g| g.name == "Portal 2").unwrap_or_else(|| panic!("{what}: Portal 2 missing"));
    assert_eq!((portal.copies.len(), portal.hltb_main, portal.override_), (2, Some(8.5), Some(Override::Abandoned)), "{what}");
    let hades = all.iter().find(|g| g.name == "Hades").unwrap();
    assert_eq!((hades.override_, hades.override_note.as_deref()), (Some(Override::Done), Some("beat it")), "{what}");
    let lineup = &db.pick_history(PlayerId::FIRST, 5).unwrap()[0];
    assert_eq!((lineup.picks[0].id, lineup.picks[0].slot, lineup.shown), (hades.id, Some(0), 1), "{what}");
    assert_eq!(db.setting_get(PlayerId::FIRST, "theme").unwrap().as_deref(), Some("daylight"), "{what}");
}

fn fresh_shape() -> String {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("upnext.db"), 120).unwrap();
    db.with(|c| Ok(schema_shape(c))).unwrap()
}

#[test]
fn a_new_database_is_current_and_opens_again_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("upnext.db");
    drop(Db::open(&path, 120).unwrap());
    let made = snapshot(&path);
    assert_eq!(made.0, SCHEMA_VERSION);
    assert!(made.2.iter().any(|(k, _)| k == "meta.schedules_since"), "schedules count from when it was made: {:?}", made.2);
    drop(Db::open(&path, 120).unwrap());
    assert_eq!(snapshot(&path), made);
    assert!(!dir.path().join("backups").exists(), "a new database has nothing to back up");
}

/// From each version since the baseline, the upgrade ends with the tables,
/// columns, constraints and indexes of a new database, and keeps the rows.
/// A database already at SCHEMA_VERSION is left exactly as it is, with no
/// backup; an older one is backed up first.
#[test]
fn every_supported_schema_opens_with_a_new_databases_shape_and_keeps_its_rows() {
    let fresh = fresh_shape();
    for v in BASELINE_VERSION..=SCHEMA_VERSION {
        let what = format!("v{v}");
        let (dir, path) = seeded(&format!("schema/v{v}.sql"));
        assert_eq!(user_version(&Connection::open(&path).unwrap()), v, "{what}: the fixture's version");
        let before = snapshot(&path);
        let db = Db::open(&path, 120).unwrap_or_else(|e| panic!("{what}: {e:#}"));
        assert_eq!(db.with(|c| Ok((user_version(c), schema_shape(c)))).unwrap(), (SCHEMA_VERSION, fresh.clone()), "{what}");
        check_seeded_rows(&db, &what);
        let backups = crate::backups::list(&dir.path().join("backups")).unwrap_or_default();
        if v == SCHEMA_VERSION {
            drop(db);
            assert_eq!(snapshot(&path), before, "{what}: a current database opens unchanged");
            assert!(backups.is_empty(), "{what}: {backups:?}");
        } else {
            assert_eq!(
                backups.iter().map(|b| (b.kind, b.schema)).collect::<Vec<_>>(),
                [(crate::backups::Kind::Upgrade, Some(v))],
                "{what}"
            );
        }
    }
}

/// A library kept since an early development build has the current tables
/// in another order, and an index new databases lack. It opens, and at the
/// current version, unchanged.
#[test]
fn a_database_the_development_builds_upgraded_opens() {
    let (dir, path) = seeded("schema/v29-from-v4.sql");
    let before = snapshot(&path);
    let db = Db::open(&path, 120).unwrap_or_else(|e| panic!("{e:#}"));
    check_seeded_rows(&db, "v29 from v4");
    let only = |a: &str, b: &str| a.lines().filter(|l| !b.lines().any(|m| m == *l)).map(String::from).collect::<Vec<_>>();
    let shape = db.with(|c| Ok(schema_shape(c))).unwrap();
    let fresh = fresh_shape();
    assert_eq!(
        (only(&shape, &fresh), only(&fresh, &shape)),
        (vec!["games: index games_appid: CREATE INDEX games_appid ON games(appid)".to_string()], vec![]),
    );
    if SCHEMA_VERSION == 29 {
        drop(db);
        assert_eq!(snapshot(&path), before, "opened unchanged");
        assert!(!dir.path().join("backups").exists());
    }
}

/// A database from before the first release isn't opened, backed up or
/// relabelled; one from a newer UpNext isn't either. The fixtures are in
/// SQLite's rollback-journal mode, so the file staying byte for byte the
/// same also shows it wasn't switched to WAL.
#[test]
fn databases_this_build_cant_open_are_left_as_they_are() {
    let refused = |version: i64| {
        let (dir, path) = seeded(&format!("schema/v{BASELINE_VERSION}.sql"));
        Connection::open(&path).unwrap().execute_batch(&format!("PRAGMA user_version={version};")).unwrap();
        let c = Connection::open(&path).unwrap();
        assert_eq!(c.query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0)).unwrap(), "delete");
        drop(c);
        let (before, bytes) = (snapshot(&path), std::fs::read(&path).unwrap());
        let e = format!("{:#}", Db::open(&path, 120).err().unwrap());
        assert_eq!(snapshot(&path), before, "v{version}");
        assert!(std::fs::read(&path).unwrap() == bytes, "v{version}: the file changed");
        assert!(!dir.path().join("backups").exists(), "v{version}");
        e
    };
    for old in [0, 1, 16, BASELINE_VERSION - 1] {
        let e = refused(old);
        assert!(
            e.ends_with(&format!(
                "is from a development build of UpNext made before its first release (schema v{old}); this build opens v{BASELINE_VERSION} and later and can't upgrade it, so move it aside to start a new library"
            )),
            "{e}"
        );
    }
    let e = refused(SCHEMA_VERSION + 1);
    assert!(e.contains(&format!("newer UpNext (schema v{})", SCHEMA_VERSION + 1)), "{e}");

    // A copy being restored is refused the same way.
    let (_dir, path) = seeded(&format!("schema/v{BASELINE_VERSION}.sql"));
    let c = Connection::open(&path).unwrap();
    c.execute_batch(&format!("PRAGMA user_version={};", BASELINE_VERSION - 1)).unwrap();
    let e = Db::upgrade(&c).unwrap_err().to_string();
    assert!(e.starts_with("the database is from a development build of UpNext made before its first release"), "{e}");
    assert_eq!(user_version(&c), BASELINE_VERSION - 1);
}

/// A database's schema in a form that doesn't depend on how it got there:
/// one line per column or constraint of each table, sorted, and each index,
/// with comments, spacing, quoting and the order of columns left out. A
/// column added by `ALTER TABLE` lands at the end of its table and an old
/// table renamed into place is quoted, so two databases with the same
/// tables can differ in their text but never in this.
pub(super) fn schema_shape(c: &Connection) -> String {
    let mut stmt =
        c.prepare("SELECT type, name, tbl_name, sql FROM sqlite_master WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%'").unwrap();
    let rows: Vec<(String, String, String, String)> =
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap().collect::<std::result::Result<_, _>>().unwrap();
    let mut lines = vec![];
    for (kind, name, table, sql) in rows {
        let sql = normalize_sql(&sql);
        if kind == "table" {
            let open = sql.find('(').unwrap();
            let body = &sql[open + 1..sql.rfind(')').unwrap()];
            let mut depth = 0;
            let mut part = String::new();
            for ch in body.chars() {
                match ch {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                if ch == ',' && depth == 0 {
                    lines.push(format!("{table}: {}", part.trim()));
                    part.clear();
                } else {
                    part.push(ch);
                }
            }
            lines.push(format!("{table}: {}", part.trim()));
        } else {
            lines.push(format!("{table}: {kind} {name}: {sql}"));
        }
    }
    lines.sort();
    lines.join("\n")
}

fn normalize_sql(sql: &str) -> String {
    let stripped: Vec<&str> = sql.lines().map(|l| l.split("--").next().unwrap()).collect();
    let mut out = stripped.join(" ").replace('"', "").split_whitespace().collect::<Vec<_>>().join(" ");
    for (from, to) in [("( ", "("), (" )", ")"), (" ,", ","), (" (", "(")] {
        out = out.replace(from, to);
    }
    out
}

/// With UPNEXT_UPDATE_SCHEMA_FIXTURE=1, write the current schema's fixture
/// when there is none yet. One already written stays: it is what databases
/// at that version look like, whatever the schema says now.
#[test]
fn write_current_fixture() {
    let path = crate::testutil::fixture_path(&format!("schema/v{SCHEMA_VERSION}.sql"));
    if std::env::var_os("UPNEXT_UPDATE_SCHEMA_FIXTURE").is_none() || path.exists() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("upnext.db"), 120).unwrap();
    let sql: Vec<String> = db
        .with(|c| {
            let mut s = c.prepare("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' ORDER BY rowid")?;
            Ok(s.query_map([], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?)
        })
        .unwrap();
    let mut out = format!("-- UpNext schema v{SCHEMA_VERSION}, made by this build's schema (schema only, no rows).\n");
    for s in sql {
        out.push_str(&s);
        out.push_str(";\n");
    }
    out.push_str(&format!("PRAGMA user_version={SCHEMA_VERSION};\n"));
    std::fs::write(path, out).unwrap();
}

/// v30: a v29 library gains the two rating tables, empty, and
/// takes ratings of its picks and its games.
#[test]
fn v30_adds_ratings_of_picks_and_games() {
    let (_dir, path) = seeded("schema/v29.sql");
    let db = Db::open(&path, 120).unwrap();
    let hades = 2;
    assert_eq!(db.pick_batch(PlayerId::FIRST, "b1").unwrap().unwrap().picks[0].rating, None);
    assert_eq!(db.game(PlayerId::FIRST, hades).unwrap().unwrap().my_rating, None);

    let not_a_fit = Some((PickVerdict::NotAFit, Some(MissReason::WrongLength)));
    assert!(db.set_pick_rating("b1", hades, not_a_fit).unwrap());
    assert!(!db.set_pick_rating("b1", 1, Some((PickVerdict::Good, None))).unwrap(), "Portal 2 isn't in that lineup");
    db.set_game_rating(PlayerId::FIRST, hades, Some(5)).unwrap();

    let rating = db.pick_batch(PlayerId::FIRST, "b1").unwrap().unwrap().picks[0].rating.clone().unwrap();
    assert_eq!((rating.verdict, rating.reason), (PickVerdict::NotAFit, Some(MissReason::WrongLength)));
    let g = db.game(PlayerId::FIRST, hades).unwrap().unwrap();
    assert_eq!(g.my_rating, Some(5));
    let rated = g.pick_rating.unwrap();
    assert_eq!((rated.verdict, rated.prompt.as_str()), (PickVerdict::NotAFit, "something short"));

    // A good pick has no reason, and taking a rating back leaves none.
    assert!(db.set_pick_rating("b1", hades, Some((PickVerdict::Good, Some(MissReason::WrongMood)))).unwrap());
    let rating = db.pick_batch(PlayerId::FIRST, "b1").unwrap().unwrap().picks[0].rating.clone().unwrap();
    assert_eq!((rating.verdict, rating.reason), (PickVerdict::Good, None));
    assert!(db.set_pick_rating("b1", hades, None).unwrap());
    assert_eq!(db.pick_batch(PlayerId::FIRST, "b1").unwrap().unwrap().picks[0].rating, None);
    db.set_game_rating(PlayerId::FIRST, hades, None).unwrap();
    assert_eq!(db.game(PlayerId::FIRST, hades).unwrap().unwrap().my_rating, None);

    // The tables refuse what the API never sends.
    assert!(db.set_game_rating(PlayerId::FIRST, hades, Some(6)).is_err());
    let refused = |sql: &str| db.with(|c| Ok(c.execute(sql, [])?)).is_err();
    assert!(refused("INSERT INTO game_ratings(player_id, title_id, stars, at) VALUES (1, 2, 0, 1)"));
    assert!(refused("INSERT INTO pick_ratings(batch_id, title_id, verdict, at) VALUES ('b1', 2, 'meh', 1)"));
}

/// v31: a v30 library gains an empty routines table, and its lineups
/// a routine column that stays empty for the lineups it had.
#[test]
fn v31_adds_routines_and_the_routine_a_lineup_was_picked_for() {
    use crate::taste::routine::{Day, RoutineBody, Who};
    let (_dir, path) = seeded("schema/v30.sql");
    let db = Db::open(&path, 120).unwrap();
    assert!(db.routines(PlayerId::FIRST).unwrap().is_empty());
    assert_eq!(db.pick_batch(PlayerId::FIRST, "b1").unwrap().unwrap().routine, None, "an old lineup was picked for no routine");

    let body = RoutineBody { name: "Friday with friends".into(), days: vec![Day::Fri], who: Some(Who::Friends), ..Default::default() };
    let r = db.add_routine(PlayerId::FIRST, &body).unwrap();
    let mut lineup = db.pick_batch(PlayerId::FIRST, "b1").unwrap().unwrap();
    lineup.batch_id = "b2".into();
    lineup.routine = Some(r.clone());
    db.save_batch(PlayerId::FIRST, &lineup).unwrap();
    // A lineup keeps the routine as it was, after it changes or goes.
    db.set_routine(PlayerId::FIRST, r.id, &RoutineBody { name: "Saturday".into(), ..Default::default() }).unwrap();
    db.delete_routine(PlayerId::FIRST, r.id).unwrap();
    assert_eq!(db.pick_batch(PlayerId::FIRST, "b2").unwrap().unwrap().routine, Some(r));
}

/// v32: an existing library becomes player 1's. Their stores,
/// accounts and AI keys move out of the household settings into their
/// connections and AI account, every mark and list is theirs, and nothing
/// they see changes.
#[test]
fn v32_gives_the_library_to_player_one() {
    let (_dir, path) = seeded("schema/v31.sql");
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "INSERT INTO settings(key, value) VALUES
                ('steam_id', '76561197960287930'), ('steam_api_key', 'STEAMKEY'), ('llm_backend', 'api-key'),
                ('anthropic_api_key', 'sk-test'), ('model', 'sonnet'), ('shelve_after_days', '40'), ('scan_hour', '3');
             INSERT INTO meta(key, value) VALUES ('profile', '{\"persona_name\":\"Sam\"}');
             INSERT INTO source_sync(source, launcher, last_sync, count) VALUES ('steam', 'steam', 5, 1), ('epic', 'heroic', 6, 2);
             INSERT INTO api_tokens(name, scope, hash, hint, created_at) VALUES ('phone', 'full', 'h', 'up_1', 1);
             INSERT INTO play_later(title_id, added_at) VALUES (2, 1);
             INSERT INTO game_ratings(title_id, stars, at) VALUES (1, 4, 1);
             INSERT INTO wishlist(appid, name, source, state, added_at, updated_at) VALUES (10, 'Ten', 'manual', 'wishlisted', 1, 1);
             INSERT INTO play_diary(hour, at, title_id, appid, name, minutes, source) VALUES
                (0, 1, 1, 620, 'Portal 2', 30, 'steam'), (0, 2, 2, NULL, 'Hades', 0, 'launch');
             INSERT INTO ai_calls(at, kind, backend, duration_ms) VALUES (1, 'pick', 'api-key', 5), (1, 'digest', 'api-key', 5);",
        )
        .unwrap();
    let db = Db::open(&path, 120).unwrap();
    let me = PlayerId::FIRST;
    let players = db.players().unwrap();
    assert_eq!(players.iter().map(|p| (p.id, p.name.as_str(), p.has_pin)).collect::<Vec<_>>(), [(me, "Sam", false)]);

    // Each store's account sits in the connection that synced it, and every
    // copy is in one.
    let q = |sql: &str| -> Vec<String> {
        db.with(|c| {
            let mut s = c.prepare(sql)?;
            Ok(s.query_map([], |r| r.get::<_, String>(0))?.collect::<std::result::Result<_, _>>()?)
        })
        .unwrap()
    };
    assert_eq!(
        q("SELECT k.kind || '/' || a.store || '/' || COALESCE(a.external_id, '-') || '/' || a.sharing || '/' || k.owner_id
           FROM store_accounts a JOIN store_connections k ON k.id = a.connection_id ORDER BY 1"),
        ["heroic/epic/-/everyone/1", "steam/steam/76561197960287930/everyone/1"]
    );
    assert_eq!(
        q("SELECT g.source || ':' || a.store FROM games g JOIN store_accounts a ON a.id = g.account_id ORDER BY g.id"),
        ["steam:steam", "epic:epic", "epic:epic"]
    );
    assert_eq!(
        q(
            "SELECT a.store || ':' || s.last_sync || ':' || s.count FROM source_sync s JOIN store_accounts a ON a.id = s.account_id ORDER BY 1"
        ),
        ["epic:6:2", "steam:5:1"]
    );

    // The settings read the same, from their new homes.
    let settings = db.settings_all(me).unwrap();
    for (k, v) in [
        ("steam_id", "76561197960287930"),
        ("steam_api_key", "STEAMKEY"),
        ("llm_backend", "api-key"),
        ("anthropic_api_key", "sk-test"),
        ("model", "sonnet"),
        ("shelve_after_days", "40"),
        ("theme", "daylight"),
        ("scan_hour", "3"),
    ] {
        assert_eq!(settings.get(k).map(String::as_str), Some(v), "{k}");
    }
    assert_eq!(
        q("SELECT key FROM settings ORDER BY key"),
        // A later step's job schedule is the household's too.
        ["library_ai", "scan_hour", "schedule.verdicts", "steam_api_key"],
        "the household keeps only its own"
    );
    assert_eq!(q("SELECT key FROM player_settings ORDER BY key"), ["ai_account", "shelve_after_days", "theme"]);
    assert_eq!(
        q("SELECT backend || ' ' || settings || ' ' || secret FROM ai_accounts"),
        [r#"api-key {"model":"sonnet"} {"anthropic_api_key":"sk-test"}"#]
    );

    // Marks and lists are player 1's; a token isn't bound to anyone yet.
    let g = db.game(me, 2).unwrap().unwrap();
    assert_eq!(g.saved_at, Some(1), "Hades is still on Play later");
    assert_eq!(db.game(me, 1).unwrap().unwrap().my_rating, Some(4));
    assert_eq!(db.wishlist_rows(me).unwrap().len(), 1);
    assert_eq!(q("SELECT COALESCE(player_id, 'none') FROM api_tokens"), ["none"]);
    assert_eq!(
        q(
            "SELECT source || ':' || COALESCE(player_id, '-') || ':' || COALESCE((SELECT store FROM store_accounts WHERE id = account_id), '-') FROM play_diary ORDER BY id"
        ),
        ["steam:-:steam", "launch:1:-"]
    );
    assert_eq!(db.diary_rows(me, 0).unwrap().len(), 2, "player 1 sees both diary rows");
    assert_eq!(
        q("SELECT kind || ':' || COALESCE(player_id, '-') || ':' || COALESCE(ai_account_id, '-') FROM ai_calls ORDER BY id"),
        ["pick:1:1", "digest:-:1"]
    );
    assert!(q("SELECT 'broken: ' || \"table\" FROM pragma_foreign_key_check").is_empty());
}

/// v33: a v32 library gains an empty table of the player's own
/// backlog verdicts, which keeps a verdict and refuses an unknown state.
#[test]
fn v33_adds_the_players_own_backlog_verdicts() {
    use crate::verdicts::verdict::Verdict;
    let (_dir, path) = seeded("schema/v32.sql");
    let db = Db::open(&path, 120).unwrap();
    assert!(db.my_verdicts(PlayerId::FIRST).unwrap().is_empty());
    let mut v = Verdict::new(Some(620), None);
    v.outcome = Some("stopped".into());
    v.worth = Some("no".into());
    v.stopped_for = vec!["grind".into()];
    let m = MyVerdict::draft(1, v);
    db.save_my_verdict(PlayerId::FIRST, &m).unwrap();
    assert_eq!(db.my_verdicts(PlayerId::FIRST).unwrap(), vec![(m, "Portal 2".to_string())]);
    let refused = db
        .with(|c| {
            Ok(c.execute(
                "INSERT INTO my_verdicts(player_id, title_id, vocab, licence, consent, state, updated_at) VALUES (1, 2, 1, 'CC0-1.0', 1, 'live', 1)",
                [],
            )?)
        })
        .is_err();
    assert!(refused);
}

#[test]
fn v34_moves_the_heroic_folder_and_the_diary_reading_onto_player_ones_connections() {
    let (_dir, path) = seeded("schema/v33.sql");
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "INSERT INTO settings(key, value) VALUES ('heroic_dir', '/home/you/.config/heroic'), ('epic_manifests', '/srv/epic'),
                 ('scan_hour', '3');
             INSERT INTO meta(key, value) VALUES ('diary_reading', '{\"at\":5,\"games\":{}}');",
        )
        .unwrap();
    let db = Db::open(&path, 120).unwrap();
    let q = |sql: &str| -> Vec<String> {
        db.with(|c| {
            let mut s = c.prepare(sql)?;
            Ok(s.query_map([], |r| r.get::<_, String>(0))?.collect::<std::result::Result<_, _>>()?)
        })
        .unwrap()
    };
    // Heroic's folder goes onto the Heroic connection v32 made; Epic, with
    // no connection yet, gets one.
    assert_eq!(
        q("SELECT id || ' ' || kind || ' ' || owner_id || ' ' || settings FROM store_connections ORDER BY id"),
        ["1 steam 1 {}", r#"2 heroic 1 {"heroic_dir":"/home/you/.config/heroic"}"#, r#"3 epic 1 {"epic_manifests":"/srv/epic"}"#]
    );
    assert_eq!(q("SELECT key FROM settings WHERE key IN ('heroic_dir', 'epic_manifests', 'scan_hour')"), ["scan_hour"]);
    let settings = db.settings_all(PlayerId::FIRST).unwrap();
    assert_eq!(settings.get("heroic_dir").map(String::as_str), Some("/home/you/.config/heroic"), "read as before");
    // The Steam account's last reading is its own now.
    assert_eq!(q("SELECT key FROM meta WHERE key LIKE 'diary%'"), ["diary_reading.1"]);
    check_seeded_rows(&db, "v34");
    assert!(q("SELECT 'broken: ' || \"table\" FROM pragma_foreign_key_check").is_empty());
}

/// v35: a v34 library gains an empty table of other players'
/// verdicts, and the job that downloads them is daily from the upgrade on,
/// so it isn't due the moment the server starts.
#[test]
fn v35_adds_other_players_verdicts_and_their_job() {
    let (_dir, path) = seeded("schema/v34.sql");
    let before = now();
    let db = Db::open(&path, 120).unwrap();
    assert_eq!(db.community_snapshot().unwrap(), crate::db::CommunitySnapshot::default());
    let schedule = db.setting_get(PlayerId::FIRST, "schedule.verdicts").unwrap().unwrap();
    let v: serde_json::Value = serde_json::from_str(&schedule).unwrap();
    assert_eq!(v["every"], "daily");
    assert!(v["since"].as_i64().unwrap() >= before, "{schedule}");
}

/// v36: a v35 library gains the two empty snapshot tables, and a title
/// merged after the upgrade gets its status and note back when its copy is
/// split out again.
#[test]
fn v36_keeps_what_a_merge_folds_away_for_a_split() {
    let (_dir, path) = seeded("schema/v35.sql");
    let db = Db::open(&path, 120).unwrap();
    let count = |sql: &str| db.with(|c| Ok(c.query_row(sql, [], |r| r.get::<_, i64>(0))?)).unwrap();
    assert_eq!(count("SELECT COUNT(*) FROM merge_snapshots") + count("SELECT COUNT(*) FROM merge_snapshot_marks"), 0);
    check_seeded_rows(&db, "v36");
    // Hades (done, "beat it") into Portal 2 (abandoned), then out again.
    assert_eq!(db.merge_titles(&[1, 2]).unwrap(), 1);
    let portal = db.game(PlayerId::FIRST, 1).unwrap().unwrap();
    assert_eq!((portal.copies.len(), portal.override_), (3, Some(Override::Abandoned)));
    let hades = db.split_copy(3).unwrap();
    let hades = db.game(PlayerId::FIRST, hades).unwrap().unwrap();
    assert_eq!((hades.name.as_str(), hades.override_, hades.override_note.as_deref()), ("Hades", Some(Override::Done), Some("beat it")));
    assert_eq!(count("SELECT COUNT(*) FROM merge_snapshots"), 0);
    assert_eq!(count("SELECT COUNT(*) FROM pragma_foreign_key_check"), 0);
}

/// v37: players gain the identity provider's account,
/// one player per account, and a token can be a browser signed in through
/// it. The tokens a v36 database has are kept as they were.
#[test]
fn v37_links_identities_and_keeps_the_tokens() {
    let (_dir, path) = seeded("schema/v36.sql");
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "INSERT INTO api_tokens(id, name, scope, kind, hash, hint, created_at, last_used_at, player_id)
             VALUES (7, 'phone', 'full', 'device', 'abc', 'upnext_Ab12', 5, 6, 1);",
        )
        .unwrap();
    let db = Db::open(&path, 120).unwrap();
    check_seeded_rows(&db, "v37");
    let tokens = db.tokens().unwrap();
    assert_eq!(tokens.len(), 1);
    let t = &tokens[0];
    assert_eq!(
        (t.id, t.name.as_str(), t.kind.as_str(), t.last_used_at, t.player_id),
        (7, "phone", "device", Some(6), Some(PlayerId::FIRST))
    );
    db.token_create_for("Browser (Authelia)", crate::db::Scope::Full, "oidc", Some(PlayerId::FIRST)).unwrap();
    db.link_oidc(PlayerId::FIRST, "https://sso.den.example.org", "u-1").unwrap();
    let other = db.add_player("Sam").unwrap();
    assert!(db.link_oidc(other, "https://sso.den.example.org", "u-1").is_err(), "one player per account");
    let count = |sql: &str| db.with(|c| Ok(c.query_row(sql, [], |r| r.get::<_, i64>(0))?)).unwrap();
    assert_eq!(count("SELECT COUNT(*) FROM pragma_foreign_key_check"), 0);
}

/// v38: a v37 library keeps its rows, and while v38 is the newest, its
/// tables' text exactly as it was, since the step changes no table. Only a
/// new database gets the reworded comments.
#[test]
fn v38_leaves_a_v37_librarys_tables_as_they_were() {
    let (_dir, path) = seeded("schema/v37.sql");
    let (_, tables, rows) = snapshot(&path);
    let db = Db::open(&path, 120).unwrap();
    check_seeded_rows(&db, "v38");
    drop(db);
    if SCHEMA_VERSION == 38 {
        assert_eq!(snapshot(&path), (38, tables, rows));
    }
}
