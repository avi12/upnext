//! Title grouping, merge, split and Steam-match behaviour, against a real
//! (temporary) database. Several cases are regressions found while grouping was built.

use super::*;
use crate::db::PlayerId;

fn open() -> (tempfile::TempDir, Db) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("upnext.db"), 120).unwrap();
    (dir, db)
}

fn g(source: &str, id: &str, name: &str, appid: Option<u32>) -> ImportedGame {
    let launcher = if source == "steam" { "steam" } else { "heroic" };
    ImportedGame { source: source.into(), source_id: id.into(), launcher: launcher.into(), name: name.into(), appid, ..Default::default() }
}

/// Library as (title name, sorted "source:id" copies), sorted.
fn titles(db: &Db) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = db
        .all_games(PlayerId::FIRST)
        .unwrap()
        .into_iter()
        .map(|v| {
            let mut copies: Vec<String> = v.copies.iter().map(|c| format!("{}:{}", c.source, c.source_id)).collect();
            copies.sort();
            (v.name, copies)
        })
        .collect();
    out.sort();
    out
}

fn view_of(db: &Db, copy: &str) -> GameView {
    db.all_games(PlayerId::FIRST)
        .unwrap()
        .into_iter()
        .find(|v| v.copies.iter().any(|c| format!("{}:{}", c.source, c.source_id) == copy))
        .unwrap_or_else(|| panic!("no title holds {copy}"))
}

fn game_id(db: &Db, copy: &str) -> i64 {
    view_of(db, copy).copies.iter().find(|c| format!("{}:{}", c.source, c.source_id) == copy).unwrap().game_id
}

fn names(v: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
    v.iter().map(|(n, c)| (n.to_string(), c.iter().map(|s| s.to_string()).collect())).collect()
}

#[test]
fn copies_group_by_appid_exact_name_and_edition() {
    let (_d, db) = open();
    db.sync_source(
        PlayerId::FIRST,
        "steam",
        "steam",
        &[g("steam", "620", "Portal 2", Some(620)), g("steam", "870780", "Control", Some(870780))],
    )
    .unwrap();
    db.sync_source(
        PlayerId::FIRST,
        "heroic",
        "epic",
        &[g("epic", "e1", "Portal 2", None), g("epic", "e2", "Control Ultimate Edition", None), g("epic", "e3", "Witcher 3, The", None)],
    )
    .unwrap();
    db.sync_source(PlayerId::FIRST, "heroic", "gog", &[g("gog", "g1", "The Witcher 3", None)]).unwrap();
    assert_eq!(
        titles(&db),
        names(&[
            ("Control", &["epic:e2", "steam:870780"]),
            ("Portal 2", &["epic:e1", "steam:620"]),
            ("The Witcher 3", &["epic:e3", "gog:g1"]),
        ])
    );
    let control = view_of(&db, "epic:e2");
    assert_eq!(control.appid, Some(870780), "the group's appid reaches the Epic copy");
    let edition = control.copies.iter().find(|c| c.source == "epic").unwrap().edition.clone();
    assert_eq!(edition.as_deref(), Some("Ultimate Edition"));
}

/// A remaster is an edition of the same title: its copy groups with the
/// original's, the title takes the plain name whichever copy came first, and
/// the title lists the edition.
#[test]
fn a_remaster_is_an_edition_of_the_original_title() {
    let (_d, db) = open();
    db.sync_source(PlayerId::FIRST, "heroic", "gog", &[g("gog", "g1", "BioShock 2 Remastered", None)]).unwrap();
    db.sync_source(PlayerId::FIRST, "heroic", "epic", &[g("epic", "e1", "BioShock 2", None)]).unwrap();
    assert_eq!(titles(&db), names(&[("BioShock 2", &["epic:e1", "gog:g1"])]));
    let v = view_of(&db, "gog:g1");
    assert_eq!(v.editions, vec!["Remastered".to_string()]);
    let plain = v.copies.iter().find(|c| c.source == "epic").unwrap();
    assert_eq!(plain.edition, None);
}

#[test]
fn sequels_and_unrelated_games_stay_apart() {
    let (_d, db) = open();
    db.sync_source(PlayerId::FIRST, "steam", "steam", &[g("steam", "400", "Portal", Some(400)), g("steam", "620", "Portal 2", Some(620))])
        .unwrap();
    db.sync_source(PlayerId::FIRST, "heroic", "epic", &[g("epic", "e1", "Portal 2", None), g("epic", "e2", "Hades", None)]).unwrap();
    assert_eq!(titles(&db), names(&[("Hades", &["epic:e2"]), ("Portal", &["steam:400"]), ("Portal 2", &["epic:e1", "steam:620"])]));
}

#[test]
fn removed_copies_leave_the_library() {
    let (_d, db) = open();
    db.sync_source(PlayerId::FIRST, "heroic", "epic", &[g("epic", "a", "Alpha", None), g("epic", "b", "Beta", None)]).unwrap();
    // Same second as the first sync: removal must not depend on the clock.
    let s = db.sync_source(PlayerId::FIRST, "heroic", "epic", &[g("epic", "a", "Alpha", None)]).unwrap();
    assert_eq!(s.removed, 1);
    assert_eq!(titles(&db), names(&[("Alpha", &["epic:a"])]));
}

#[test]
fn a_secondary_launcher_only_adds_to_a_native_copy() {
    // Heroic owns this GOG copy; the GOG account sync must not rename it or
    // claim it, only fill in playtime.
    let (_d, db) = open();
    let mut heroic = g("gog", "1", "The Witcher", None);
    heroic.installed = true;
    db.sync_source(PlayerId::FIRST, "heroic", "gog", &[heroic]).unwrap();
    let mut account = g("gog", "1", "The Witcher: Enhanced Edition", None);
    account.launcher = "gog".into();
    account.playtime_minutes = Some(120);
    db.sync_source(PlayerId::FIRST, "gog", "gog", &[account]).unwrap();
    let c = view_of(&db, "gog:1").copies[0].clone();
    assert_eq!((c.name.as_str(), c.launcher.as_str()), ("The Witcher", "heroic"));
    assert_eq!((c.playtime_minutes, c.playtime_known, c.installed), (120, true, true));
    // An empty GOG account sync removes only copies the GOG launcher owns.
    db.sync_source(PlayerId::FIRST, "gog", "gog", &[]).unwrap();
    assert_eq!(titles(&db).len(), 1);
}

#[test]
fn merge_survives_resync_and_carries_title_data() {
    let (_d, db) = open();
    let steam = [g("steam", "1145360", "Hades", Some(1145360))];
    let epic = [g("epic", "e1", "Hades: Battle Out of Hell", None)];
    db.sync_source(PlayerId::FIRST, "steam", "steam", &steam).unwrap();
    db.sync_source(PlayerId::FIRST, "heroic", "epic", &epic).unwrap();
    assert_eq!(titles(&db).len(), 2);
    let (a, b) = (view_of(&db, "steam:1145360").id, view_of(&db, "epic:e1").id);

    db.set_override(PlayerId::FIRST, b, Some(Override::Done), Some("beat it")).unwrap();
    // Both titles with a digest used to make the merge fail on a UNIQUE/FK error.
    db.put_digest(a, "digest a", "", &Default::default(), "claude-sonnet-5-5", 2).unwrap();
    db.put_digest(b, "digest b", "", &Default::default(), "claude-sonnet-5-5", 2).unwrap();
    let pick = Pick {
        id: b,
        appid: None,
        name: "Hades".into(),
        reason: "r".into(),
        game: None,
        slot: Some(0),
        feedback: None,
        rating: None,
        owned: true,
        wishlist: None,
        alternate: None,
        backlog: None,
    };
    let batch = PickBatch {
        batch_id: "b1".into(),
        prompt: "p".into(),
        created_at: 1,
        model: None,
        duration_ms: None,
        cost_usd: None,
        api_usd: None,
        candidates_considered: None,
        attempts: None,
        usage: None,
        shown: 1,
        picks: vec![pick],
        spares: vec![],
        removed: vec![],
        ranked_by: None,
        mode: None,
        suggestion: None,
        routine: None,
    };
    db.save_batch(PlayerId::FIRST, &batch).unwrap();
    db.set_game_rating(PlayerId::FIRST, b, Some(4)).unwrap();
    assert!(db.set_pick_rating("b1", b, Some((PickVerdict::Good, None))).unwrap());

    let survivor = db.merge_titles(&[a, b]).unwrap();
    assert_eq!(titles(&db), names(&[("Hades", &["epic:e1", "steam:1145360"])]));
    let v = db.game(PlayerId::FIRST, survivor).unwrap().unwrap();
    assert_eq!((v.override_, v.override_note.as_deref()), (Some(Override::Done), Some("beat it")), "override follows the merge");
    assert_eq!(db.pick_history(PlayerId::FIRST, 10).unwrap()[0].picks[0].id, survivor, "picks follow the merge");
    assert_eq!(v.my_rating, Some(4), "your rating follows the merge");
    let rating = db.pick_history(PlayerId::FIRST, 10).unwrap()[0].picks[0].rating.clone();
    assert_eq!(rating.map(|r| r.verdict), Some(PickVerdict::Good), "a pick's rating follows the merge");

    db.sync_source(PlayerId::FIRST, "steam", "steam", &steam).unwrap();
    db.sync_source(PlayerId::FIRST, "heroic", "epic", &epic).unwrap();
    assert_eq!(titles(&db).len(), 1, "a manual merge outlives re-syncs");
    assert!(db.merge_titles(&[survivor]).is_err());
}

#[test]
fn split_survives_resync_and_can_be_merged_back() {
    let (_d, db) = open();
    let steam = [g("steam", "620", "Portal 2", Some(620))];
    let epic = [g("epic", "e1", "Portal 2", None)];
    db.sync_source(PlayerId::FIRST, "steam", "steam", &steam).unwrap();
    db.sync_source(PlayerId::FIRST, "heroic", "epic", &epic).unwrap();
    assert_eq!(titles(&db).len(), 1);

    let split = db.split_copy(game_id(&db, "epic:e1")).unwrap();
    assert_eq!(titles(&db), names(&[("Portal 2", &["epic:e1"]), ("Portal 2", &["steam:620"])]));
    assert_eq!(db.game(PlayerId::FIRST, split).unwrap().unwrap().appid, None, "a split non-Steam copy drops the group's appid");

    db.sync_source(PlayerId::FIRST, "steam", "steam", &steam).unwrap();
    db.sync_source(PlayerId::FIRST, "heroic", "epic", &epic).unwrap();
    assert_eq!(titles(&db).len(), 2, "a split outlives re-syncs");
    assert!(db.split_copy(game_id(&db, "epic:e1")).is_err(), "a single copy has nothing to split from");

    db.merge_titles(&[view_of(&db, "steam:620").id, split]).unwrap();
    db.sync_source(PlayerId::FIRST, "heroic", "epic", &epic).unwrap();
    assert_eq!(titles(&db), names(&[("Portal 2", &["epic:e1", "steam:620"])]));
}

#[test]
fn splitting_a_merge_anchor_keeps_the_merge_with_its_title() {
    let (_d, db) = open();
    db.sync_source(PlayerId::FIRST, "steam", "steam", &[g("steam", "100", "Foo", Some(100))]).unwrap();
    db.sync_source(PlayerId::FIRST, "heroic", "epic", &[g("epic", "e1", "Foo", None)]).unwrap();
    db.sync_source(PlayerId::FIRST, "heroic", "gog", &[g("gog", "g1", "Bar Baz", None)]).unwrap();
    // The merge anchors on the title's first copy, the Steam one.
    db.merge_titles(&[view_of(&db, "steam:100").id, view_of(&db, "gog:g1").id]).unwrap();
    assert_eq!(titles(&db).len(), 1);

    db.split_copy(game_id(&db, "steam:100")).unwrap();
    let mut groups: Vec<Vec<String>> = titles(&db).into_iter().map(|(_, c)| c).collect();
    groups.sort();
    assert_eq!(groups, [vec!["epic:e1".to_string(), "gog:g1".to_string()], vec!["steam:100".to_string()]]);
}

#[test]
fn steam_match_joins_the_steam_copy() {
    let (_d, db) = open();
    db.sync_source(PlayerId::FIRST, "steam", "steam", &[g("steam", "620", "Portal 2", Some(620))]).unwrap();
    db.sync_source(PlayerId::FIRST, "heroic", "epic", &[g("epic", "e1", "P2 Store Name", None)]).unwrap();
    assert_eq!(titles(&db).len(), 2);
    let survivor = db.put_steam_match(view_of(&db, "epic:e1").id, Some((620, "Portal 2", 0.9))).unwrap();
    assert_eq!(titles(&db), names(&[("Portal 2", &["epic:e1", "steam:620"])]));
    assert_eq!(survivor, view_of(&db, "steam:620").id);
}

#[test]
fn prune_drops_lax_matches_only() {
    let (_d, db) = open();
    db.sync_source(
        PlayerId::FIRST,
        "heroic",
        "epic",
        &[g("epic", "e1", "Hades II", None), g("epic", "e2", "Control Ultimate Edition", None)],
    )
    .unwrap();
    db.put_steam_match(view_of(&db, "epic:e1").id, Some((1145360, "Hades", 0.87))).unwrap();
    db.put_steam_match(view_of(&db, "epic:e2").id, Some((870780, "Control", 0.9))).unwrap();
    assert_eq!(db.prune_bad_matches(0.9).unwrap(), 1);
    assert_eq!(view_of(&db, "epic:e1").appid, None, "\"Hades II\" is not \"Hades\"");
    assert_eq!(view_of(&db, "epic:e2").appid, Some(870780), "an edition of the same game keeps its match");
}

/// The code-health review's B-STORES-3, the prune part: a Steam match kept from before
/// is judged with the score the store lookup uses, so "Hades" matched to
/// "Hades 2" (0.87) or "Final Fantasy VII" to VIII (0.94) is dropped.
#[test]
fn prune_judges_known_pairs_as_the_lookup_does() {
    for (query, listed, same) in crate::names::KNOWN_PAIRS {
        let (_d, db) = open();
        db.sync_source(PlayerId::FIRST, "heroic", "epic", &[g("epic", "e1", query, None)]).unwrap();
        db.put_steam_match(view_of(&db, "epic:e1").id, Some((10, listed, 0.9))).unwrap();
        db.prune_bad_matches(0.86).unwrap();
        assert_eq!(view_of(&db, "epic:e1").appid.is_some(), *same, "{query} matched to {listed}");
    }
}

/// The first title named in a merge survives with
/// its own name, appid, status, note, ending switch and Play later, even
/// when the other title has more copies, and a split afterwards leaves it so.
#[test]
fn the_first_title_wins_a_merge() {
    let (_d, db) = open();
    let steam = [
        g("steam", "1222690", "Dragon Age: Inquisition", Some(1222690)),
        g("steam", "812140", "Assassin's Creed Odyssey", Some(812140)),
        g("steam", "582010", "MONSTER HUNTER: WORLD", Some(582010)),
    ];
    db.sync_source(PlayerId::FIRST, "steam", "steam", &steam).unwrap();
    let (dai, aco, mhw) = (view_of(&db, "steam:1222690").id, view_of(&db, "steam:812140").id, view_of(&db, "steam:582010").id);
    db.set_override(PlayerId::FIRST, aco, Some(Override::Abandoned), Some("too long")).unwrap();

    // Two unlike games: the first keeps its name and appid, and takes the
    // other's status only because it had none of its own.
    let kept = db.merge_titles(&[dai, aco]).unwrap();
    assert_eq!(kept, dai);
    let v = db.game(PlayerId::FIRST, kept).unwrap().unwrap();
    assert_eq!((v.name.as_str(), v.appid, v.copies.len()), ("Dragon Age: Inquisition", Some(1222690), 2));
    assert_eq!((v.override_, v.override_note.as_deref()), (Some(Override::Abandoned), Some("too long")));

    db.set_override(PlayerId::FIRST, mhw, Some(Override::Done), Some("the one I meant")).unwrap();
    db.set_no_ending(mhw, Some(false)).unwrap();
    db.set_play_later(PlayerId::FIRST, mhw, true).unwrap();
    // The bigger title comes second; the first still wins.
    let kept = db.merge_titles(&[mhw, dai]).unwrap();
    assert_eq!(kept, mhw);
    let check = |db: &Db, copies: usize| {
        let v = db.game(PlayerId::FIRST, mhw).unwrap().unwrap();
        assert_eq!((v.name.as_str(), v.appid, v.copies.len()), ("MONSTER HUNTER: WORLD", Some(582010), copies));
        assert_eq!((v.override_, v.override_note.as_deref()), (Some(Override::Done), Some("the one I meant")));
        assert_eq!(v.no_ending, Some(false));
        assert!(v.saved_at.is_some(), "Play later stays");
    };
    check(&db, 3);
    assert!(db.game(PlayerId::FIRST, dai).unwrap().is_none() && db.game(PlayerId::FIRST, aco).unwrap().is_none());

    // A re-sync regroups from the rules and must not undo any of it.
    db.sync_source(PlayerId::FIRST, "steam", "steam", &steam).unwrap();
    check(&db, 3);

    // Splitting a merged-in copy back out leaves the survivor as it was.
    let split = db.split_copy(game_id(&db, "steam:812140")).unwrap();
    check(&db, 2);
    let detached = db.game(PlayerId::FIRST, split).unwrap().unwrap();
    assert_eq!((detached.name.as_str(), detached.appid), ("Assassin's Creed Odyssey", Some(812140)));
    // Splitting the anchor itself moves the merge to a sibling: the rest
    // keeps the title, and the anchor goes off on its own.
    db.split_copy(game_id(&db, "steam:582010")).unwrap();
    let rest = view_of(&db, "steam:1222690");
    assert_eq!(rest.id, mhw, "the title stays with the copies still merged");
}

/// A split Steam copy takes back the play diary's readings of its own
/// account and appid; launches name only the title, so they stay.
#[test]
fn a_split_steam_copy_takes_its_own_diary_readings() {
    let (_d, db) = open();
    let sam = PlayerId::FIRST;
    db.sync_source(sam, "heroic", "epic", &[g("epic", "a", "Alpha", None)]).unwrap();
    db.sync_source(sam, "steam", "steam", &[g("steam", "620", "Portal 2", Some(620))]).unwrap();
    let (alpha, portal) = (view_of(&db, "epic:a").id, view_of(&db, "steam:620").id);
    let account: i64 =
        db.with(|c| Ok(c.query_row("SELECT account_id FROM games WHERE source='steam' AND source_id='620'", [], |r| r.get(0))?)).unwrap();
    let row = |title: i64, source: &'static str, account: Option<i64>, appid: Option<i64>| crate::db::NewDiaryRow {
        hour: 1,
        at: 1,
        title_id: Some(title),
        appid,
        name: "Portal 2".into(),
        minutes: 30,
        source,
        player: (source == "launch").then_some(sam),
        account,
    };
    db.diary_add(&[
        row(portal, "steam", Some(account), Some(620)),
        row(portal, "launch", None, Some(620)),
        // The account's reading of another game isn't this copy's.
        row(alpha, "steam", Some(account), Some(400)),
    ])
    .unwrap();
    let diary = |db: &Db, t: i64| -> Vec<String> {
        db.with(|c| {
            let mut st = c.prepare("SELECT source || ' ' || COALESCE(appid, '-') FROM play_diary WHERE title_id=?1 ORDER BY 1")?;
            Ok(st.query_map([t], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<_>>()?)
        })
        .unwrap()
    };

    assert_eq!(db.merge_titles(&[alpha, portal]).unwrap(), alpha);
    assert_eq!(diary(&db, alpha).len(), 3, "a merge moves the diary to the survivor");
    let back = db.split_copy(game_id(&db, "steam:620")).unwrap();
    assert_eq!(diary(&db, back), ["steam 620"]);
    assert_eq!(diary(&db, alpha), ["launch 620", "steam 400"]);
}

/// Two Steam accounts own the same game, so their copies share an appid
/// and a title. Merged with another title and then split, the split copy
/// takes only its own account's diary readings: the account and the appid
/// together say whose a reading is, never the appid alone.
#[test]
fn a_split_steam_copy_leaves_another_accounts_readings_of_the_same_appid() {
    let (_d, db) = open();
    let sam = PlayerId::FIRST;
    let alex = db.add_player("Alex").unwrap();
    db.sync_source(sam, "heroic", "epic", &[g("epic", "a", "Alpha", None)]).unwrap();
    db.sync_source(sam, "steam", "steam", &[g("steam", "620", "Portal 2", Some(620))]).unwrap();
    db.sync_source(alex, "steam", "steam", &[g("steam", "620", "Portal 2", Some(620))]).unwrap();
    let copies: Vec<(i64, i64)> = db
        .with(|c| {
            let mut st = c.prepare("SELECT id, account_id FROM games WHERE source='steam' AND source_id='620' ORDER BY id")?;
            Ok(st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?)
        })
        .unwrap();
    let [(sams_copy, sams), (alexs_copy, alexs)] = copies[..] else { panic!("two Steam copies of 620: {copies:?}") };
    assert_ne!(sams, alexs, "each player's Steam is an account of its own");
    let title_of = |db: &Db, copy: i64| -> i64 {
        db.with(|c| Ok(c.query_row("SELECT title_id FROM games WHERE id=?1", [copy], |r| r.get(0))?)).unwrap()
    };
    let (alpha, portal) = (view_of(&db, "epic:a").id, title_of(&db, sams_copy));
    assert_eq!(title_of(&db, alexs_copy), portal, "the same appid is one title");
    let row = |account: i64| crate::db::NewDiaryRow {
        hour: 1,
        at: 1,
        title_id: Some(portal),
        appid: Some(620),
        name: "Portal 2".into(),
        minutes: 30,
        source: "steam",
        player: None,
        account: Some(account),
    };
    db.diary_add(&[row(sams), row(alexs)]).unwrap();
    let diary = |db: &Db, t: i64| -> Vec<i64> {
        db.with(|c| {
            let mut st = c.prepare("SELECT account_id FROM play_diary WHERE title_id=?1 ORDER BY 1")?;
            Ok(st.query_map([t], |r| r.get::<_, i64>(0))?.collect::<rusqlite::Result<_>>()?)
        })
        .unwrap()
    };

    assert_eq!(db.merge_titles(&[alpha, portal]).unwrap(), alpha);
    assert_eq!(diary(&db, alpha), [sams, alexs], "a merge moves both accounts' readings to the survivor");
    let back = db.split_copy(alexs_copy).unwrap();
    assert_ne!(back, alpha);
    assert_eq!(title_of(&db, sams_copy), alpha, "the other account's copy stays merged");
    assert_eq!(diary(&db, back), [alexs], "the split copy takes its own account's reading");
    assert_eq!(diary(&db, alpha), [sams], "and leaves the other account's, of the same appid");
}

/// Schema v36: splitting a merged-in copy back out gives its new title what
/// its first title had before the merge (each player's status and note,
/// rating, Play later and hide, and the ending switch), while the survivor
/// keeps its own. Merging and splitting again works the same way.
#[test]
fn a_split_gives_back_what_a_merge_folded_away() {
    let (_d, db) = open();
    let sam = PlayerId::FIRST;
    let alex = db.add_player("Alex").unwrap();
    db.sync_source(sam, "heroic", "epic", &[g("epic", "a", "Alpha", None), g("epic", "b", "Beta", None)]).unwrap();
    let (a, b) = (view_of(&db, "epic:a").id, view_of(&db, "epic:b").id);
    db.set_override(sam, a, Some(Override::Shelved), Some("later")).unwrap();
    db.set_override(sam, b, Some(Override::Abandoned), Some("too long")).unwrap();
    db.set_game_rating(sam, b, Some(2)).unwrap();
    db.set_play_later(alex, b, true).unwrap();
    db.set_hidden(alex, b, Some(HideScope::Me)).unwrap();
    db.set_no_ending(b, Some(true)).unwrap();
    let marks = |db: &Db, t: i64| -> Vec<String> {
        db.with(|c| {
            let mut st = c.prepare(
                "SELECT 'override ' || player_id || ' ' || status || ' ' || COALESCE(note, '') FROM overrides WHERE title_id=?1
                 UNION ALL SELECT 'rating ' || player_id || ' ' || stars FROM game_ratings WHERE title_id=?1
                 UNION ALL SELECT 'later ' || player_id FROM play_later WHERE title_id=?1
                 UNION ALL SELECT 'hidden ' || player_id || ' ' || scope FROM hidden_titles WHERE title_id=?1
                 UNION ALL SELECT 'no_ending ' || no_ending FROM titles WHERE id=?1 AND no_ending IS NOT NULL
                 ORDER BY 1",
            )?;
            Ok(st.query_map([t], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<_>>()?)
        })
        .unwrap()
    };
    let snapshots = |db: &Db| -> i64 {
        db.with(|c| {
            Ok(c.query_row("SELECT (SELECT COUNT(*) FROM merge_snapshots) + (SELECT COUNT(*) FROM merge_snapshot_marks)", [], |r| {
                r.get(0)
            })?)
        })
        .unwrap()
    };

    assert_eq!(db.merge_titles(&[a, b]).unwrap(), a);
    // The survivor keeps its own status and takes the rest, as before v36.
    assert_eq!(marks(&db, a), ["hidden 2 me", "later 2", "no_ending 1", "override 1 shelved later", "rating 1 2"]);
    assert_eq!(snapshots(&db), 3, "one snapshot with each player's marks");

    let back = db.split_copy(game_id(&db, "epic:b")).unwrap();
    assert_ne!(back, a);
    assert_eq!(marks(&db, back), ["hidden 2 me", "later 2", "no_ending 1", "override 1 abandoned too long", "rating 1 2"]);
    assert_eq!(
        marks(&db, a),
        ["hidden 2 me", "later 2", "no_ending 1", "override 1 shelved later", "rating 1 2"],
        "the survivor keeps what it has"
    );
    assert_eq!(snapshots(&db), 0, "a snapshot is used once");
    assert_eq!(db.with(|c| Ok(c.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r.get::<_, i64>(0))?)).unwrap(), 0);

    // Merging again keeps what the split title has now.
    db.set_override(sam, back, Some(Override::Done), None).unwrap();
    assert_eq!(db.merge_titles(&[a, back]).unwrap(), a);
    let back = db.split_copy(game_id(&db, "epic:b")).unwrap();
    assert_eq!(marks(&db, back), ["hidden 2 me", "later 2", "no_ending 1", "override 1 done too long", "rating 1 2"]);

    // A title with nothing of its own leaves no snapshot, and its copy
    // comes back bare.
    db.sync_source(sam, "heroic", "epic", &[g("epic", "a", "Alpha", None), g("epic", "b", "Beta", None), g("epic", "c", "Gamma", None)])
        .unwrap();
    let c = view_of(&db, "epic:c").id;
    db.merge_titles(&[a, c]).unwrap();
    assert_eq!(snapshots(&db), 0);
    let gamma = db.split_copy(game_id(&db, "epic:c")).unwrap();
    assert!(marks(&db, gamma).is_empty());

    // Removing a player takes their marks out of a snapshot too.
    db.merge_titles(&[a, back]).unwrap();
    db.remove_player(alex).unwrap();
    assert_eq!(snapshots(&db), 3 - 1);
    let back = db.split_copy(game_id(&db, "epic:b")).unwrap();
    assert_eq!(marks(&db, back), ["no_ending 1", "override 1 done too long", "rating 1 2"]);
}

#[test]
fn a_merge_names_each_title_once() {
    let (_d, db) = open();
    db.sync_source(PlayerId::FIRST, "heroic", "epic", &[g("epic", "a", "Alpha", None), g("epic", "b", "Beta", None)]).unwrap();
    let (a, b) = (view_of(&db, "epic:a").id, view_of(&db, "epic:b").id);
    let err = db.merge_titles(&[a, a]).unwrap_err().to_string();
    assert!(err.contains("named twice"), "{err}");
    assert!(db.merge_titles(&[a, b, a]).is_err(), "a repeat anywhere is refused, not only next to itself");
    assert_eq!(titles(&db).len(), 2, "nothing was merged");
    assert_eq!(db.with(|c| Ok(c.query_row("SELECT COUNT(*) FROM merge_rules", [], |r| r.get::<_, i64>(0))?)).unwrap(), 0);
}
