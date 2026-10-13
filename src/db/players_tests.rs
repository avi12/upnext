//! Two players on one database: which copies each sees, whose
//! hours count, what each hides, and that marks, lists, routines and
//! settings stay with the player who made them.

use super::players::{HideScope, Sharing};
use super::*;

const PORTAL: u32 = 620;
const HADES: u32 = 1145360;
const CONTROL: u32 = 870780;
const CELESTE: u32 = 504230;

fn steam(appid: u32, name: &str, minutes: i64) -> ImportedGame {
    ImportedGame {
        source: "steam".into(),
        source_id: appid.to_string(),
        launcher: "steam".into(),
        name: name.into(),
        appid: Some(appid),
        playtime_minutes: Some(minutes),
        ..Default::default()
    }
}

/// Alex (player 1) has Portal 2, Hades and Control on Steam; Sam has their
/// own Steam account with Portal 2 and Celeste. Both accounts are shared
/// with everyone, as new accounts are.
struct House {
    _dir: tempfile::TempDir,
    db: Db,
    alex: PlayerId,
    sam: PlayerId,
}

impl House {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("upnext.db"), 120).unwrap();
        let alex = PlayerId::FIRST;
        db.sync_source(alex, "steam", "steam", &[steam(PORTAL, "Portal 2", 600), steam(HADES, "Hades", 300), steam(CONTROL, "Control", 0)])
            .unwrap();
        let sam = db.add_player("Sam").unwrap();
        db.sync_source(sam, "steam", "steam", &[steam(PORTAL, "Portal 2", 60), steam(CELESTE, "Celeste", 120)]).unwrap();
        House { _dir: dir, db, alex, sam }
    }

    fn id(&self, appid: u32) -> i64 {
        self.db.title_for_appid(appid as i64).unwrap().unwrap()
    }

    fn account(&self, p: PlayerId) -> i64 {
        self.db.own_store_account(p, "steam").unwrap().unwrap()
    }

    /// What `p` sees: (name, copies, minutes played), sorted by name.
    fn library(&self, p: PlayerId) -> Vec<(String, usize, i64)> {
        let mut out: Vec<_> = self.db.all_games(p).unwrap().into_iter().map(|g| (g.name, g.copies.len(), g.playtime_minutes)).collect();
        out.sort();
        out
    }

    fn sees(&self, p: PlayerId, appid: u32) -> bool {
        self.db.game(p, self.id(appid)).unwrap().is_some()
    }
}

fn row(name: &str, copies: usize, minutes: i64) -> (String, usize, i64) {
    (name.to_string(), copies, minutes)
}

#[test]
fn a_shared_account_shows_its_games_but_hours_come_from_your_own() {
    let h = House::new();
    assert_eq!(h.db.players().unwrap().iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Player 1", "Sam"]);
    // One record per game: the Portal 2 they both own is one title with two
    // copies, and each sees the other's games as never played.
    assert_eq!(h.library(h.alex), [row("Celeste", 1, 0), row("Control", 1, 0), row("Hades", 1, 300), row("Portal 2", 2, 600)]);
    assert_eq!(h.library(h.sam), [row("Celeste", 1, 120), row("Control", 1, 0), row("Hades", 1, 0), row("Portal 2", 2, 60)]);
    let hades = h.db.game(h.sam, h.id(HADES)).unwrap().unwrap();
    assert_eq!((hades.status, hades.playtime_known), (Status::Untouched, true), "someone else's hours aren't Sam's");

    // Ticking "also plays on it" brings Alex's hours to Sam.
    h.db.set_account_player(h.account(h.alex), h.sam, false, true, true).unwrap();
    assert_eq!(h.library(h.sam), [row("Celeste", 1, 120), row("Control", 1, 0), row("Hades", 1, 300), row("Portal 2", 2, 600)]);

    // Sam can stop using an account offered to them.
    h.db.set_account_player(h.account(h.alex), h.sam, false, false, false).unwrap();
    assert_eq!(h.library(h.sam), [row("Celeste", 1, 120), row("Portal 2", 1, 60)]);
}

#[test]
fn an_account_kept_to_its_owner_or_offered_to_some_shows_only_to_them() {
    let h = House::new();
    h.db.set_account_sharing(h.account(h.sam), Sharing::Owner).unwrap();
    assert_eq!(h.library(h.alex), [row("Control", 1, 0), row("Hades", 1, 300), row("Portal 2", 1, 600)]);
    assert_eq!(h.library(h.sam).len(), 4, "the owner still sees everything shared with them");

    h.db.set_account_sharing(h.account(h.sam), Sharing::Chosen).unwrap();
    assert!(!h.sees(h.alex, CELESTE), "chosen, and Alex isn't ticked");
    h.db.set_account_player(h.account(h.sam), h.alex, true, true, false).unwrap();
    assert!(h.sees(h.alex, CELESTE));
    assert_eq!(h.db.stats(h.alex).unwrap().total, 4);
}

#[test]
fn each_hide_scope_hides_from_whom_it_says() {
    let h = House::new();
    // Just from me: Alex stops seeing Control; Sam still does.
    h.db.set_hidden(h.alex, h.id(CONTROL), Some(HideScope::Me)).unwrap();
    assert!(!h.sees(h.alex, CONTROL) && h.sees(h.sam, CONTROL));
    assert_eq!(h.db.hidden_by(h.alex).unwrap().iter().map(|t| t.title_id).collect::<Vec<_>>(), [h.id(CONTROL)]);
    assert!(h.db.hidden_by(h.sam).unwrap().is_empty(), "only Alex's own list shows it");

    // From everyone else: Alex still sees Hades, Sam doesn't.
    h.db.set_hidden(h.alex, h.id(HADES), Some(HideScope::Others)).unwrap();
    assert!(h.sees(h.alex, HADES) && !h.sees(h.sam, HADES));

    // It hides Alex's copies only: Sam's own Portal 2 stays.
    h.db.set_hidden(h.alex, h.id(PORTAL), Some(HideScope::Others)).unwrap();
    assert_eq!(h.db.game(h.sam, h.id(PORTAL)).unwrap().unwrap().copies.len(), 1);
    assert_eq!(h.db.game(h.alex, h.id(PORTAL)).unwrap().unwrap().copies.len(), 2);

    // From everyone: neither sees Celeste.
    h.db.set_hidden(h.sam, h.id(CELESTE), Some(HideScope::Everyone)).unwrap();
    assert!(!h.sees(h.alex, CELESTE) && !h.sees(h.sam, CELESTE));

    // Hiding from others needs a copy of your own.
    assert!(h.db.set_hidden(h.sam, h.id(CONTROL), Some(HideScope::Others)).is_err());

    // Showing it again undoes it.
    h.db.set_hidden(h.alex, h.id(HADES), None).unwrap();
    h.db.set_hidden(h.sam, h.id(CELESTE), None).unwrap();
    assert!(h.sees(h.sam, HADES) && h.sees(h.alex, CELESTE) && h.sees(h.sam, CELESTE));
}

#[test]
fn a_game_says_how_its_player_hid_it_and_whether_they_have_a_copy() {
    let h = House::new();
    let (hades, control, celeste) = (h.id(HADES), h.id(CONTROL), h.id(CELESTE));
    // Control is Alex's copy: Alex can hide it from the others, Sam only from themself.
    let alex_control = h.db.game(h.alex, control).unwrap().unwrap();
    assert!(alex_control.own_copy && alex_control.hidden_scope.is_none());
    assert!(!h.db.game(h.sam, control).unwrap().unwrap().own_copy);
    assert!(h.db.game(h.sam, celeste).unwrap().unwrap().own_copy);

    h.db.set_hidden(h.alex, hades, Some(HideScope::Others)).unwrap();
    assert_eq!(h.db.game(h.alex, hades).unwrap().unwrap().hidden_scope, Some(HideScope::Others));
    // Another player's mark is never theirs to read.
    h.db.set_hidden(h.sam, control, Some(HideScope::Me)).unwrap();
    assert_eq!(h.db.game(h.alex, control).unwrap().unwrap().hidden_scope, None);
    assert_eq!(h.db.game_unhidden(h.sam, control).unwrap().unwrap().hidden_scope, Some(HideScope::Me));
    let listed = h.db.all_games(h.alex).unwrap();
    assert_eq!(listed.iter().find(|g| g.id == hades).unwrap().hidden_scope, Some(HideScope::Others), "the library list says it too");
}

#[test]
fn hidden_from_the_others_it_says_who_still_sees_it_and_why() {
    let h = House::new();
    let kit = h.db.add_player("Kit").unwrap();
    let portal = h.id(PORTAL);
    // Hidden only from Alex: nobody else's view changed, so nobody is named.
    assert_eq!(h.db.still_seen_by(h.alex, portal, HideScope::Me).unwrap(), []);
    // From the others: Sam has their own copy, and Kit, who has no account,
    // still sees it through Sam's shared one.
    h.db.set_hidden(h.alex, portal, Some(HideScope::Others)).unwrap();
    let still = h.db.still_seen_by(h.alex, portal, HideScope::Others).unwrap();
    let seen: Vec<(&str, bool)> = still.iter().map(|s| (s.name.as_str(), s.own_copy)).collect();
    assert_eq!(seen, [("Sam", true), ("Kit", false)]);
    assert!(h.db.game(kit, portal).unwrap().is_some());
    // Hades is Alex's alone: hidden from the others, nobody sees it.
    h.db.set_hidden(h.alex, h.id(HADES), Some(HideScope::Everyone)).unwrap();
    assert_eq!(h.db.still_seen_by(h.alex, h.id(HADES), HideScope::Everyone).unwrap(), []);
    // The Hidden by me list says whether each is Alex's own copy.
    h.db.set_hidden(h.alex, h.id(CELESTE), Some(HideScope::Me)).unwrap();
    let mine: Vec<(String, bool)> = h.db.hidden_by(h.alex).unwrap().into_iter().map(|t| (t.name, t.own_copy)).collect();
    assert!(mine.contains(&("Celeste".to_string(), false)) && mine.contains(&("Portal 2".to_string(), true)), "{mine:?}");
}

#[test]
fn marks_lists_and_settings_stay_with_the_player_who_made_them() {
    let h = House::new();
    let (alex, sam, portal, hades) = (h.alex, h.sam, h.id(PORTAL), h.id(HADES));
    h.db.set_override(alex, portal, Some(Override::Done), Some("beat it")).unwrap();
    h.db.add_stop_reasons(alex, portal, &["grind".to_string()], None, "menu").unwrap();
    h.db.set_play_later(alex, hades, true).unwrap();
    h.db.set_game_rating(alex, hades, Some(5)).unwrap();
    let favourite = taste::NewStatement { kind: "favourite".into(), title_id: Some(portal), about: None, why: None, note: None };
    h.db.taste_state(alex, &favourite).unwrap();
    let wish = NewWish {
        appid: 10,
        name: "Ten".into(),
        source: WishSource::Manual,
        reason: None,
        state: WishState::Wishlisted,
        on_steam: false,
        series_key: None,
        batch_id: None,
    };
    h.db.wishlist_add(alex, &wish).unwrap();
    h.db.diary_add(&[NewDiaryRow {
        hour: 0,
        at: 1,
        title_id: Some(hades),
        appid: Some(HADES as i64),
        name: "Hades".into(),
        minutes: 0,
        source: "launch",
        player: Some(alex),
        account: None,
    }])
    .unwrap();
    h.db.setting_set(sam, "theme", Some("ember")).unwrap();

    let a = h.db.game(alex, portal).unwrap().unwrap();
    assert_eq!((a.override_, a.override_note.as_deref()), (Some(Override::Done), Some("beat it")));
    let s = h.db.game(sam, portal).unwrap().unwrap();
    assert_eq!((s.override_, s.stop_reasons.is_some()), (None, false), "Sam's Portal 2 has no mark");
    let s = h.db.game(sam, hades).unwrap().unwrap();
    assert_eq!((s.saved_at, s.my_rating), (None, None));
    assert_eq!(h.db.game(alex, hades).unwrap().unwrap().my_rating, Some(5));
    assert_eq!(h.db.taste_statements(alex).unwrap().len(), 1);
    assert!(h.db.taste_statements(sam).unwrap().is_empty());
    assert_eq!((h.db.wishlist_rows(alex).unwrap().len(), h.db.wishlist_rows(sam).unwrap().len()), (1, 0));
    assert_eq!((h.db.diary_rows(alex, 0).unwrap().len(), h.db.diary_rows(sam, 0).unwrap().len()), (1, 0));
    // The same wish on Sam's list is a row of its own.
    assert!(h.db.wishlist_add(sam, &wish).unwrap().1);

    // Routines are their maker's: no one else lists, changes or deletes them.
    let couch = crate::taste::routine::RoutineBody { name: "Couch".into(), ..Default::default() };
    let r = h.db.add_routine(sam, &couch).unwrap();
    assert!(h.db.routines(alex).unwrap().is_empty());
    assert_eq!(h.db.set_routine(alex, r.id, &couch).unwrap(), None);
    assert!(!h.db.delete_routine(alex, r.id).unwrap());
    assert_eq!(h.db.routines(sam).unwrap(), [r]);

    // Settings: a player's own, their store connection's, and the household's.
    assert_eq!(h.db.setting_get(alex, "theme").unwrap(), None);
    assert_eq!(h.db.setting_get(sam, "theme").unwrap().as_deref(), Some("ember"));
    h.db.setting_set(sam, "steam_id", Some("76561197960287930")).unwrap();
    assert_eq!(h.db.setting_get(alex, "steam_id").unwrap(), None, "Sam's SteamID is on Sam's connection");
    h.db.setting_set(alex, "scan_hour", Some("4")).unwrap();
    assert_eq!(h.db.setting_get(sam, "scan_hour").unwrap().as_deref(), Some("4"), "the household's settings are everyone's");
}

#[test]
fn picks_and_their_ratings_are_the_players_who_asked() {
    let h = House::new();
    let portal = h.db.game(h.alex, h.id(PORTAL)).unwrap().unwrap();
    let batch = PickBatch {
        batch_id: "b1".into(),
        prompt: "something short".into(),
        created_at: 1,
        model: None,
        duration_ms: None,
        cost_usd: None,
        api_usd: None,
        candidates_considered: None,
        attempts: None,
        usage: None,
        shown: 1,
        picks: vec![Pick { slot: Some(0), ..Pick::of(&portal, "fits".into()) }],
        spares: vec![],
        removed: vec![],
        ranked_by: None,
        mode: None,
        suggestion: None,
        routine: None,
    };
    h.db.save_batch(h.alex, &batch).unwrap();
    assert_eq!(h.db.pick_history(h.alex, 5).unwrap().len(), 1);
    assert!(h.db.pick_history(h.sam, 5).unwrap().is_empty());
    assert!(h.db.pick_batch(h.sam, "b1").unwrap().is_none(), "Sam can't open Alex's lineup");
    assert!(h.db.set_pick_rating("b1", portal.id, Some((PickVerdict::Good, None))).unwrap());
    assert!(h.db.game(h.alex, portal.id).unwrap().unwrap().pick_rating.is_some());
    assert!(h.db.game(h.sam, portal.id).unwrap().unwrap().pick_rating.is_none());

    // Recent picks are each player's own, and lineups made in the same
    // second count newest first.
    let hades = h.db.game(h.alex, h.id(HADES)).unwrap().unwrap();
    let later =
        PickBatch { batch_id: "b2".into(), picks: vec![Pick { slot: Some(0), ..Pick::of(&hades, "fits".into()) }], ..batch.clone() };
    h.db.save_batch(h.alex, &later).unwrap();
    assert_eq!(h.db.recent_pick_ids(h.alex, 1).unwrap(), [hades.id]);
    assert_eq!(h.db.recent_pick_ids(h.alex, 2).unwrap().len(), 2);
    assert!(h.db.recent_pick_ids(h.sam, 3).unwrap().is_empty());
}

// -- sync per connection ---------------------------------

fn copy(source: &str, id: &str, launcher: &str, name: &str) -> ImportedGame {
    ImportedGame {
        source: source.into(),
        source_id: id.into(),
        launcher: launcher.into(),
        name: name.into(),
        playtime_minutes: Some(10),
        ..Default::default()
    }
}

/// Each account as "connection kind/store/owner", with its copies' names.
fn accounts(db: &Db) -> Vec<String> {
    db.with(|c| {
        let mut st = c.prepare(
            "SELECT k.id || ':' || k.kind || '/' || a.store || '/' || k.owner_id || ' ' ||
                    COALESCE((SELECT group_concat(name, ',') FROM (SELECT name FROM games WHERE account_id = a.id AND removed = 0 ORDER BY name)), '')
             FROM store_accounts a JOIN store_connections k ON k.id = a.connection_id ORDER BY a.id",
        )?;
        Ok(st.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<_>>()?)
    })
    .unwrap()
}

#[test]
fn a_second_steam_account_has_a_library_of_its_own() {
    let h = House::new();
    // Alex links a second Steam account: a connection of its own.
    let second_id = h.db.with(|c| {
        c.execute("INSERT INTO store_connections(kind, owner_id, settings, created_at) VALUES ('steam', 1, '{\"steam_id\":\"76561197960287931\"}', 1)", [])?;
        Ok(c.last_insert_rowid())
    })
    .unwrap();
    let mut second = h.db.sync_connections("steam").unwrap().into_iter().find(|c| c.id == Some(second_id)).unwrap();
    assert_eq!((second.owner, second.first, second.setting("steam_id")), (h.alex, false, Some("76561197960287931")));
    h.db.sync_into(&mut second, "steam", &[steam(CELESTE, "Celeste", 45)]).unwrap();
    let all = accounts(&h.db);
    assert_eq!(all.len(), 3, "{all:?}");
    assert!(all[2].starts_with(&format!("{second_id}:steam/steam/1 Celeste")), "{all:?}");
    assert_eq!(
        h.db.with(|c| Ok(c.query_row(
            "SELECT external_id FROM store_accounts WHERE id = (SELECT MAX(id) FROM store_accounts)",
            [],
            |r| r.get::<_, Option<String>>(0)
        )?))
        .unwrap()
        .as_deref(),
        Some("76561197960287931"),
        "its SteamID says whose it is"
    );
    // The first account's games are left as they were, and Celeste's hours
    // from both of Alex's accounts are theirs.
    assert!(h.library(h.alex).contains(&row("Portal 2", 2, 600)));
    assert!(h.library(h.alex).contains(&row("Celeste", 2, 45)));
}

#[test]
fn a_store_sign_in_takes_over_the_account_another_reader_made() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("upnext.db"), 120).unwrap();
    let me = PlayerId::FIRST;
    // Heroic reads a GOG game first, then the GOG sign-in reports it too:
    // one copy, now in the account under the GOG sign-in.
    db.sync_source(me, "heroic", "gog", &[copy("gog", "1", "heroic", "Witcher")]).unwrap();
    db.sync_source(me, "gog", "gog", &[copy("gog", "1", "gog", "Witcher"), copy("gog", "2", "gog", "Gwent")]).unwrap();
    assert_eq!(accounts(&db), ["2:gog/gog/1 Gwent,Witcher"]);
    // Heroic's next sync adds to that account rather than making another.
    db.sync_source(me, "heroic", "gog", &[copy("gog", "1", "heroic", "Witcher")]).unwrap();
    assert_eq!(accounts(&db), ["2:gog/gog/1 Gwent,Witcher"]);
    assert_eq!(db.all_games(me).unwrap().len(), 2);
}

#[test]
fn galaxys_games_go_to_their_owners_accounts() {
    let h = House::new();
    // Sam's Galaxy reports Steam and Epic games: the Steam ones join Sam's
    // own Steam account, the Epic ones get an account under the Galaxy
    // connection; Alex's accounts are untouched.
    let mut galaxy = h.db.own_sync_connection(h.sam, "galaxy").unwrap();
    assert!(galaxy.id.is_none() && !galaxy.first, "only player 1's Galaxy reads this computer's");
    h.db.sync_into(&mut galaxy, "steam", &[copy("steam", &CONTROL.to_string(), "galaxy", "Control")]).unwrap();
    h.db.sync_into(&mut galaxy, "epic", &[copy("epic", "fortnite", "galaxy", "Fortnite")]).unwrap();
    h.db.record_galaxy_sync(&mut galaxy, 2).unwrap();
    let all = accounts(&h.db);
    assert!(all[0].ends_with("/steam/1 Control,Hades,Portal 2"), "{all:?}");
    assert!(all[1].ends_with("/steam/2 Celeste,Control,Portal 2"), "Galaxy's Steam copy is in Sam's Steam account: {all:?}");
    assert!(all[2].contains(":galaxy/epic/2 Fortnite"), "{all:?}");
    // Each player's launcher copies are their own.
    assert_eq!(h.db.launcher_sources(h.sam, "galaxy").unwrap(), [("epic".to_string(), 1), ("steam".to_string(), 1)]);
    assert!(h.db.launcher_sources(h.alex, "galaxy").unwrap().is_empty());
    assert_eq!(h.db.remove_launcher_source(h.alex, "galaxy", "epic").unwrap(), 0, "Alex can't remove Sam's");
    assert_eq!(h.db.remove_launcher_source(h.sam, "galaxy", "epic").unwrap(), 1);
}

#[test]
fn games_added_by_hand_are_each_players_own_list() {
    let h = House::new();
    crate::manual::save(&h.db, h.alex, "origin", &["Dead Space".into()]).unwrap();
    crate::manual::save(&h.db, h.sam, "origin", &["Battlefield 1".into()]).unwrap();
    assert_eq!(crate::manual::lists(&h.db, h.alex).unwrap()["origin"], ["Dead Space"]);
    assert_eq!(crate::manual::lists(&h.db, h.sam).unwrap()["origin"], ["Battlefield 1"]);
    // Saving Sam's list again leaves Alex's alone.
    crate::manual::save(&h.db, h.sam, "origin", &[]).unwrap();
    assert_eq!(crate::manual::lists(&h.db, h.alex).unwrap()["origin"], ["Dead Space"]);
    assert!(crate::manual::lists(&h.db, h.sam).unwrap()["origin"].is_empty());
}

#[test]
fn a_log_line_names_a_hidden_game_only_to_whoever_can_see_it() {
    let h = House::new();
    let celeste = h.id(CELESTE);
    h.db.set_hidden(h.sam, celeste, Some(HideScope::Everyone)).unwrap();
    let job = h.db.job_begin("enrich", None, JobSource::Ui, &serde_json::json!({})).unwrap().unwrap();
    let line = |message: &str, title_id: Option<i64>| LogLine { at: 1, level: Level::Info, lane: None, message: message.into(), title_id };
    let failure = Failure {
        lane: "HLTB".into(),
        source: "hltb".into(),
        options: vec![],
        title_id: Some(celeste),
        name: Some("Celeste".into()),
        error: "no answer for Celeste".into(),
        at: 1,
    };
    h.db.job_pulse(
        job,
        &JobProgress::default(),
        &[line("HLTB: no confident match for Celeste", Some(celeste)), line("Celeste is a word here too", None)],
        std::slice::from_ref(&failure),
    )
    .unwrap();
    let read = |p: PlayerId| {
        let mut lines = h.db.job_log(job, 0, Level::Info, 10).unwrap();
        let mut failures = h.db.job_failures(job).unwrap();
        h.db.mask_hidden(p, &mut lines, &mut failures).unwrap();
        (lines.into_iter().map(|l| l.line.message).collect::<Vec<_>>(), failures[0].name.clone().unwrap(), failures[0].error.clone())
    };
    assert_eq!(
        read(h.alex),
        (
            vec!["HLTB: no confident match for a hidden game".to_string(), "a hidden game is a word here too".to_string()],
            "a hidden game".to_string(),
            "no answer for a hidden game".to_string()
        ),
        "a line that doesn't say which game it names is masked by the name"
    );
    assert_eq!(read(h.sam).0[0], "HLTB: no confident match for Celeste", "Sam hid it, so it is named for Sam");
    h.db.set_hidden(h.sam, celeste, Some(HideScope::Others)).unwrap();
    assert_eq!(read(h.sam).0[0], "HLTB: no confident match for Celeste");
    // Hidden by someone, but Alex has a copy of their own: named.
    let portal = h.id(PORTAL);
    h.db.set_hidden(h.sam, portal, Some(HideScope::Others)).unwrap();
    h.db.job_pulse(job, &JobProgress::default(), &[line("Portal 2 done", Some(portal))], &[]).unwrap();
    assert_eq!(read(h.alex).0[2], "Portal 2 done");
    // A game Alex sees whose name holds a hidden one keeps its name.
    h.db.sync_source(h.sam, "steam", "steam", &[steam(PORTAL, "Portal 2", 60), steam(CELESTE, "Celeste", 120), steam(400, "Portal", 5)])
        .unwrap();
    let first = h.db.title_for_appid(400).unwrap().unwrap();
    h.db.set_hidden(h.sam, first, Some(HideScope::Others)).unwrap();
    h.db.job_pulse(job, &JobProgress::default(), &[line("Portal, then Portal 2", None)], &[]).unwrap();
    assert_eq!(read(h.alex).0[3], "a hidden game, then Portal 2");
    assert_eq!(read(h.sam).0[3], "Portal, then Portal 2");
}

#[test]
fn sync_account_names_one_connection_by_id() {
    let h = House::new();
    let theirs = h.db.add_connection(h.sam, "heroic", &serde_json::Map::new()).unwrap();
    // `sync --account` reads only the library of the connection's kind.
    assert_eq!(h.db.connection_kind(theirs).unwrap().as_deref(), Some("heroic"));
    assert_eq!(h.db.connection_kind(theirs + 100).unwrap(), None);
    let heroic = h.db.sync_connections("heroic").unwrap();
    assert_eq!(
        heroic.iter().map(|c| (c.id, c.owner)).collect::<Vec<_>>(),
        [(None, h.alex), (Some(theirs), h.sam)],
        "player 1's machine one first"
    );
}

#[test]
fn a_name_upnext_made_up_is_told_from_a_chosen_one() {
    for made_up in ["Player 1", "Player 12", "Player 2 2"] {
        assert!(players::placeholder_name(made_up), "{made_up}");
    }
    for chosen in ["Sam", "Player", "Player One", "Player 1 again", "player 1", "Player 1 2 3", "Player  1", "Kit 1"] {
        assert!(!players::placeholder_name(chosen), "{chosen}");
    }
}

/// A PIN set before argon2 0.6 still opens its player (R-CHECKS-4): the
/// hash is the PHC string argon2 0.5.3 wrote for PIN 2468 with a fixed
/// salt, and stored hashes are never rewritten.
#[test]
fn a_pin_hashed_by_argon2_0_5_still_matches() {
    const ARGON2_0_5: &str = "$argon2id$v=19$m=19456,t=2,p=1$BwcHBwcHBwcHBwcHBwcHBw$ZmVGhvR1OaHB8Wx5BcR3xZaiHwkCmGWqR1Vb6km9gRk";
    let h = House::new();
    h.db.with(|c| Ok(c.execute("UPDATE players SET pin_hash=?1 WHERE id=1", [ARGON2_0_5])?)).unwrap();
    assert!(h.db.pin_matches(PlayerId::FIRST, "2468").unwrap());
    assert!(!h.db.pin_matches(PlayerId::FIRST, "1357").unwrap());

    // A PIN set now is the same kind of hash, and the same PIN matches it.
    h.db.set_pin(PlayerId::FIRST, Some("2468")).unwrap();
    let new: String = h.db.with(|c| Ok(c.query_row("SELECT pin_hash FROM players WHERE id=1", [], |r| r.get(0))?)).unwrap();
    assert!(new.starts_with("$argon2id$v=19$m=19456,t=2,p=1$") && new != ARGON2_0_5, "{new}");
    assert!(h.db.pin_matches(PlayerId::FIRST, "2468").unwrap());
    assert!(!h.db.pin_matches(PlayerId::FIRST, "1357").unwrap());
    h.db.with(|c| Ok(c.execute("UPDATE players SET pin_hash='not a hash' WHERE id=1", [])?)).unwrap();
    assert!(!h.db.pin_matches(PlayerId::FIRST, "2468").unwrap(), "a hash that doesn't parse matches nothing");
}
