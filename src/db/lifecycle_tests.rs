//! The lifecycle statuses as the library reads them: what's inferred from a real
//! (temporary) database's playtime, last played, HowLongToBeat and tags, and
//! that the settings, marks and merges carry through. `lifecycle.rs` tests
//! the rules themselves.

use super::*;
use crate::db::PlayerId;
use crate::lifecycle::{Kind, StatusRules};

const DAY: i64 = 86_400;

fn steam(appid: u32, name: &str, hours: f64, days_ago: i64) -> ImportedGame {
    ImportedGame {
        source: "steam".into(),
        source_id: appid.to_string(),
        launcher: "steam".into(),
        name: name.into(),
        appid: Some(appid),
        playtime_minutes: Some((hours * 60.0) as i64),
        last_played: if days_ago < 0 { 0 } else { now() - days_ago * DAY },
        ..Default::default()
    }
}

/// A library like a real one: a story game put down 3 months ago, one played
/// last month, GTA V far past its story, a Battlefield played a little, an
/// RTS played a lot, and an Epic game Heroic tracks no playtime for.
fn library() -> (tempfile::TempDir, Db) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("upnext.db"), 120).unwrap();
    db.sync_source(
        PlayerId::FIRST,
        "steam",
        "steam",
        &[
            steam(1593500, "God of War", 14.0, 90),
            steam(1145360, "Hades", 14.0, 30),
            steam(271590, "Grand Theft Auto V", 150.0, 400),
            steam(1238810, "Battlefield V", 4.0, 300),
            steam(287450, "Rise of Nations", 140.0, 6),
            steam(220, "Half-Life 2", 6.0, 1000),
        ],
    )
    .unwrap();
    let epic = ImportedGame {
        source: "epic".into(),
        source_id: "e1".into(),
        launcher: "heroic".into(),
        name: "Control".into(),
        ..Default::default()
    };
    db.sync_source(PlayerId::FIRST, "heroic", "epic", &[epic]).unwrap();
    let tag = |appid: u32, tags: &[&str], categories: &[&str]| {
        let d =
            DetailsRow { type_: Some("game".into()), categories: categories.iter().map(|s| s.to_string()).collect(), ..Default::default() };
        db.put_details(appid, Some(&d)).unwrap();
        let t: Vec<(String, i64)> = tags.iter().enumerate().map(|(i, s)| (s.to_string(), 100 - i as i64)).collect();
        db.put_tags(appid, Some(&t)).unwrap();
    };
    let sp_mp = ["Single-player", "Multi-player", "PvP"];
    tag(1593500, &["Action", "Singleplayer", "Story Rich"], &["Single-player"]);
    tag(271590, &["Open World", "Action", "Multiplayer", "Crime"], &sp_mp);
    tag(
        1238810,
        &["FPS", "Multiplayer", "Singleplayer", "World War II", "Shooter", "War", "Military", "First-Person", "Massively Multiplayer"],
        &sp_mp,
    );
    tag(287450, &["Strategy", "RTS", "Classic"], &sp_mp);
    let all = db.all_games(PlayerId::FIRST).unwrap();
    let id = |name: &str| all.iter().find(|g| g.name == name).unwrap().id;
    let hltb = |h: f64| HltbRow { main_story: Some(h), ..Default::default() };
    db.put_hltb(id("God of War"), Some(&hltb(20.6))).unwrap();
    db.put_hltb(id("Hades"), Some(&hltb(22.0))).unwrap();
    db.put_hltb(id("Grand Theft Auto V"), Some(&hltb(32.1))).unwrap();
    db.put_hltb(id("Battlefield V"), Some(&hltb(6.3))).unwrap();
    db.put_hltb(id("Rise of Nations"), Some(&hltb(19.8))).unwrap();
    db.put_hltb(id("Half-Life 2"), Some(&hltb(12.5))).unwrap();
    (dir, db)
}

fn game(db: &Db, name: &str) -> GameView {
    db.all_games(PlayerId::FIRST).unwrap().into_iter().find(|g| g.name == name).unwrap_or_else(|| panic!("{name} missing"))
}

#[test]
fn a_library_sorts_itself() {
    let (_dir, db) = library();
    let at = |name: &str| {
        let g = game(&db, name);
        (g.status, g.kind, g.staple)
    };
    assert_eq!(at("God of War"), (Status::Shelved, Kind::Ending, false), "14 of 20 hours, 3 months ago");
    assert!(game(&db, "God of War").shelved_at.is_some_and(|t| (now() - t - 30 * DAY).abs() < 60), "shelved since it crossed 2 months");
    assert_eq!(at("Hades"), (Status::Playing, Kind::Ending, false), "played last month");
    assert_eq!(at("Grand Theft Auto V"), (Status::Played, Kind::Ending, true), "150 of 32 hours: neither shelved nor abandoned");
    assert_eq!(at("Battlefield V"), (Status::Played, Kind::Endless, false), "4 of 6 campaign hours, but endless");
    assert_eq!(at("Rise of Nations"), (Status::Playing, Kind::Endless, true), "a staple played last week");
    assert_eq!(at("Half-Life 2"), (Status::Abandoned, Kind::Ending, false), "untouched for nearly 3 years");
    assert_eq!(at("Control").0, Status::Untouched, "no playtime to go on");
    assert_eq!(db.stats(PlayerId::FIRST).unwrap().by_status["shelved"], 1);
}

#[test]
fn the_settings_move_the_lines() {
    let (_dir, db) = library();
    db.set_status_rules(StatusRules { shelve_after_days: 120, abandon_after_days: 4000, staple_hours: 10, ..StatusRules::default() });
    assert_eq!(game(&db, "God of War").status, Status::Playing, "3 months is under 120 days");
    assert_eq!(game(&db, "Half-Life 2").status, Status::Shelved, "not abandoned for 11 years");
    assert!(game(&db, "Hades").staple, "a staple at 10 hours");
    db.set_status_rules(StatusRules::default());
    assert_eq!(game(&db, "God of War").status, Status::Shelved, "the library is read again when the settings change");
}

#[test]
fn marks_switches_and_reasons() {
    let (_dir, db) = library();
    let bf = game(&db, "Battlefield V").id;
    db.set_no_ending(bf, Some(false)).unwrap();
    let g = game(&db, "Battlefield V");
    assert_eq!((g.kind, g.no_ending, g.status), (Kind::Ending, Some(false), Status::Shelved), "4 of its campaign's 6 hours");

    // Abandoned by hand, then played again: the mark lapses.
    let gow = game(&db, "God of War").id;
    db.set_override(PlayerId::FIRST, gow, Some(Override::Abandoned), None).unwrap();
    db.add_stop_reasons(PlayerId::FIRST, gow, &["busy".to_string()], None, "menu").unwrap();
    db.add_stop_reasons(PlayerId::FIRST, gow, &["grind".to_string(), "too_long".to_string()], Some("the Valkyries"), "menu").unwrap();
    // Marked a minute ago, so that a play from now on comes after it.
    db.with(|c| Ok(c.execute("UPDATE overrides SET updated_at=updated_at-60 WHERE title_id=?1", rusqlite::params![gow])?)).unwrap();
    let g = game(&db, "God of War");
    assert_eq!((g.status, g.override_), (Status::Abandoned, Some(Override::Abandoned)));
    let r = g.stop_reasons.unwrap();
    assert_eq!(
        (r.codes, r.note.as_deref(), r.source.as_str()),
        (vec!["grind".to_string(), "too_long".to_string()], Some("the Valkyries"), "menu")
    );
    db.sync_source(PlayerId::FIRST, "steam", "steam", &[steam(1593500, "God of War", 15.0, 0)]).unwrap();
    let g = game(&db, "God of War");
    assert_eq!((g.status, g.override_, g.override_at), (Status::Playing, None, None), "played since it was marked");
}

#[test]
fn merging_titles_keeps_the_switch_and_the_reasons() {
    let (_dir, db) = library();
    let epic = ImportedGame {
        source: "epic".into(),
        source_id: "e2".into(),
        launcher: "heroic".into(),
        name: "Thrones and Patriots".into(),
        ..Default::default()
    };
    db.sync_source(PlayerId::FIRST, "heroic", "epic", std::slice::from_ref(&epic)).unwrap();
    let ron = game(&db, "Rise of Nations").id;
    let extended = game(&db, "Thrones and Patriots").id;
    db.set_no_ending(extended, Some(true)).unwrap();
    db.add_stop_reasons(PlayerId::FIRST, extended, &["other_game".to_string()], None, "menu").unwrap();
    let kept = db.merge_titles(&[ron, extended]).unwrap();
    let g = db.game(PlayerId::FIRST, kept).unwrap().unwrap();
    assert_eq!((g.no_ending, g.stop_reasons.map(|r| r.codes)), (Some(true), Some(vec!["other_game".to_string()])));
}
