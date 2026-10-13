use super::*;
use crate::db::PlayerId;
use crate::db::{ImportedGame, NewStatement, Override};
use crate::discovery::fixture::{Spec, game, id_of, library};

fn entry(qid: &str, name: &str, year: i32, appid: Option<u32>) -> SeriesEntry {
    SeriesEntry {
        qid: qid.into(),
        name: name.into(),
        year: Some(year),
        steam_appid: appid,
        igdb_slug: None,
        gog_id: None,
        gog_slug: None,
        cover: None,
        cover_checked: false,
    }
}

/// Half-Life played 25 hours over two games; Portal 30 minutes; Thief
/// Gold never played but beaten. Each series has entries you don't own.
fn series_library() -> (tempfile::TempDir, Db) {
    let specs = [
        Spec { minutes: 600, ..game("Half-Life", 70) },
        Spec { minutes: 900, ..game("Half-Life 2", 220) },
        Spec { minutes: 30, ..game("Portal", 400) },
        game("Thief Gold", 211600),
    ];
    let (dir, db, _cfg) = library(&specs);
    let all = db.all_games(PlayerId::FIRST).unwrap();
    for (name, series, qid, year) in [
        ("Half-Life", "Half-Life", "Q1", 1998),
        ("Half-Life 2", "Half-Life", "Q1", 2004),
        ("Portal", "Portal", "Q2", 2007),
        ("Thief Gold", "Thief", "Q3", 1998),
    ] {
        db.put_title_series(id_of(&all, name), &[(series.into(), qid.into())], Some(year)).unwrap();
    }
    db.set_override(PlayerId::FIRST, id_of(&all, "Thief Gold"), Some(Override::Done), None).unwrap();
    db.put_series_catalog(
        "Q1",
        &[
            entry("Q10", "Half-Life", 1998, Some(70)),
            entry("Q11", "Half-Life 2", 2004, Some(220)),
            entry("Q12", "Half-Life 2: Episode Two", 2007, Some(420)),
            entry("Q13", "Half-Life: Opposing Force", 1999, Some(50)),
            // Not on Steam: it can't be checked or rated, so it's never suggested.
            entry("Q14", "Half-Life: Decay", 2001, None),
        ],
    )
    .unwrap();
    db.put_series_catalog("Q2", &[entry("Q20", "Portal", 2007, Some(400)), entry("Q21", "Portal 2", 2011, Some(620))]).unwrap();
    db.put_series_catalog(
        "Q3",
        &[entry("Q30", "Thief Gold", 1998, Some(211600)), entry("Q31", "Thief II: The Metal Age", 2000, Some(211740))],
    )
    .unwrap();
    (dir, db)
}

fn collections(db: &Db) -> Vec<Collection> {
    crate::series::collections(&db.all_games(PlayerId::FIRST).unwrap(), &db.series_guides().unwrap(), &db.catalog().unwrap())
}

fn wish(appid: u32, name: &str, source: WishSource, state: WishState, on_steam: bool) -> NewWish {
    NewWish { appid, name: name.into(), source, reason: None, state, on_steam, series_key: None, batch_id: None }
}

/// The plan's test: collection entries are kept only where the series means
/// something to you (10 hours, a game beaten or a favourite), and the
/// reason names your history with it.
#[test]
fn collection_entries_are_scored_by_series_history() {
    let (_dir, db) = series_library();
    let games = db.all_games(PlayerId::FIRST).unwrap();
    let found = candidates(&collections(&db), &games, None, &[]);
    let got: Vec<(&str, u32, &str)> = found.iter().map(|c| (c.name.as_str(), c.appid, c.reason.as_str())).collect();
    assert_eq!(
        got,
        vec![
            ("Half-Life: Opposing Force", 50, "You played Half-Life and Half-Life 2 for 25 hours. This one is from 1999."),
            ("Half-Life 2: Episode Two", 420, "You played Half-Life and Half-Life 2 for 25 hours. This one came after them, in 2007."),
            ("Thief II: The Metal Age", 211740, "You beat Thief Gold. This one came after it, in 2000."),
        ],
        "Portal (30 minutes) means nothing yet, and Half-Life: Decay has no Steam id"
    );

    // A favourite makes a series mean something.
    let portal = id_of(&games, "Portal");
    let fav = NewStatement { kind: "favourite".into(), title_id: Some(portal), about: None, why: None, note: None };
    db.taste_state(PlayerId::FIRST, &fav).unwrap();
    let profile = crate::taste::Profile::load(&db, PlayerId::FIRST, &games).unwrap();
    let found = candidates(&collections(&db), &games, Some(&profile), &[]);
    let portal2 = found.iter().find(|c| c.appid == 620).expect("Portal 2 is suggested once Portal is a favourite");
    assert_eq!(portal2.reason, "You played Portal for under an hour. Portal is one of your favourites. This one came after it, in 2011.");

    // Anything already on the table, in any state, isn't a candidate again.
    db.wishlist_add(PlayerId::FIRST, &wish(420, "Half-Life 2: Episode Two", WishSource::Collection, WishState::Dismissed, false)).unwrap();
    let found = candidates(&collections(&db), &games, Some(&profile), &db.wishlist_rows(PlayerId::FIRST).unwrap());
    assert!(!found.iter().any(|c| c.appid == 420), "Not interested means never again");
}

#[test]
fn owned_games_are_found_by_appid_name_or_edition() {
    let specs = [game("Dishonored - Definitive Edition", 205100), game("The Witcher 3: Wild Hunt - Complete Edition", 292030)];
    let (_dir, db, _cfg) = library(&specs);
    let games = db.all_games(PlayerId::FIRST).unwrap();
    let name = |appid: Option<u32>, n: &str| owned_as(&games, appid, n).map(|g| g.name.clone());
    assert_eq!(name(Some(205100), "Anything"), Some("Dishonored - Definitive Edition".into()), "by appid");
    assert_eq!(name(None, "Dishonored"), Some("Dishonored - Definitive Edition".into()), "an edition of the same name");
    assert_eq!(name(Some(1), "The Witcher 3: Complete Edition"), Some("The Witcher 3: Wild Hunt - Complete Edition".into()));
    assert_eq!(name(Some(1), "Dishonored 2"), None);
    assert_eq!(name(Some(1), "The Witcher 3"), None, "a shorter name without an edition is another game");
}

/// The plan's checks before anything is shown: a game on the store, rated
/// well by enough players, and not a hard no.
#[test]
fn suggestions_need_a_store_page_a_good_rating_and_no_hard_no() {
    let good = StoreFacts {
        type_: Some("game".into()),
        details_checked: true,
        review_pct: Some(92),
        review_total: Some(8_000),
        reviews_checked: true,
        tags: vec!["Puzzle".into(), "Horror".into()],
        genres: vec!["Adventure".into()],
        ..Default::default()
    };
    assert_eq!(judge(&good, None), Ok(()));
    let with = |f: &dyn Fn(&mut StoreFacts)| {
        let mut x = good.clone();
        f(&mut x);
        judge(&x, None).unwrap_err()
    };
    assert_eq!(with(&|f| f.type_ = Some("dlc".into())), "Steam lists it as dlc, not a game");
    assert_eq!(with(&|f| f.type_ = None), "Steam's store has no page for it");
    assert!(with(&|f| f.review_pct = Some(79)).starts_with("79% of 8,000 reviews are positive"));
    assert!(with(&|f| f.review_total = Some(49)).starts_with("92% of 49 reviews"));
    assert_eq!(with(&|f| f.review_total = None), "it has no Steam reviews to go by");

    let (_dir, db, _cfg) = library(&[game("Portal", 400)]);
    let no = NewStatement { kind: "hard_no".into(), title_id: None, about: Some("tag:Horror".into()), why: None, note: None };
    db.taste_state(PlayerId::FIRST, &no).unwrap();
    let profile = crate::taste::Profile::load(&db, PlayerId::FIRST, &db.all_games(PlayerId::FIRST).unwrap()).unwrap();
    assert_eq!(judge(&good, Some(&profile)).unwrap_err(), "it's Horror, a hard no");
    let mut deep = good.clone();
    deep.tags = vec!["Puzzle".into(), "Atmospheric".into(), "Indie".into(), "Story Rich".into(), "Funny".into(), "Horror".into()];
    assert_eq!(judge(&deep, Some(&profile)), Ok(()), "a hard no reaches a store's five strongest tags only");
}

#[test]
fn the_steam_wishlist_is_mirrored_read_only() {
    let (_dir, db) = series_library();
    let games = db.all_games(PlayerId::FIRST).unwrap();
    let (suggested, _) = db
        .wishlist_add(PlayerId::FIRST, &wish(420, "Half-Life 2: Episode Two", WishSource::Collection, WishState::Suggested, false))
        .unwrap();
    let (steam_only, _) =
        db.wishlist_add(PlayerId::FIRST, &wish(1, "Gone From Steam", WishSource::Steam, WishState::Wishlisted, true)).unwrap();
    let (mine, _) = db.wishlist_add(PlayerId::FIRST, &wish(2, "Added Here Too", WishSource::Manual, WishState::Wishlisted, true)).unwrap();
    let items: Vec<WishlistItem> = [420, 3, 70].into_iter().map(|appid| WishlistItem { appid, added_at: None }).collect();
    let names = HashMap::from([(3, "Brand New".to_string()), (70, "Half-Life".to_string())]);

    let changes = mirror_steam(&db, PlayerId::FIRST, &games, &items, &names).unwrap();
    assert_eq!(changes, SteamChanges { added: 1, joined: 1, left: 2 }, "Half-Life is yours already, so it's left out");
    let row = |id: i64| db.wishlist_row(PlayerId::FIRST, id).unwrap().unwrap();
    assert_eq!((row(suggested).state, row(suggested).on_steam), (WishState::Wishlisted, true), "a suggestion on Steam is wishlisted");
    assert_eq!((row(steam_only).state, row(steam_only).on_steam), (WishState::Dismissed, false), "taken off Steam's");
    assert_eq!((row(mine).state, row(mine).on_steam), (WishState::Wishlisted, false), "added here, so it stays");
    let new = db.wishlist_by_appid(PlayerId::FIRST, 3).unwrap().unwrap();
    assert_eq!((new.name.as_str(), new.source, new.state, new.on_steam), ("Brand New", WishSource::Steam, WishState::Wishlisted, true));
    assert!(new.wishlisted_at.is_some());
    assert!(db.wishlist_by_appid(PlayerId::FIRST, 70).unwrap().is_none());

    // Read again unchanged: nothing moves.
    assert_eq!(mirror_steam(&db, PlayerId::FIRST, &games, &items, &names).unwrap(), SteamChanges::default());
}

/// The plan's test: a synced purchase leaves the wishlist, and its game page
/// says where it came from.
#[test]
fn a_synced_purchase_leaves_the_wishlist() {
    let (_dir, db) = series_library();
    let reason = "You beat Thief Gold. This one came after it, in 2000.";
    let new = NewWish {
        reason: Some(reason.into()),
        ..wish(211740, "Thief II: The Metal Age", WishSource::Collection, WishState::Suggested, false)
    };
    let (id, _) = db.wishlist_add(PlayerId::FIRST, &new).unwrap();
    db.wishlist_set_state(id, WishState::Wishlisted).unwrap();
    let (other, _) = db.wishlist_add(PlayerId::FIRST, &wish(9, "Never Bought", WishSource::Manual, WishState::Wishlisted, false)).unwrap();
    assert!(settle(&db, PlayerId::FIRST, &db.all_games(PlayerId::FIRST).unwrap()).unwrap().is_empty());

    // Bought on GOG: no Steam appid, found by name.
    let copy = ImportedGame {
        source: "gog".into(),
        source_id: "1".into(),
        launcher: "heroic".into(),
        name: "Thief II: The Metal Age".into(),
        ..Default::default()
    };
    db.sync_source(PlayerId::FIRST, "heroic", "gog", &[copy]).unwrap();
    let games = db.all_games(PlayerId::FIRST).unwrap();
    assert_eq!(settle(&db, PlayerId::FIRST, &games).unwrap(), vec!["Thief II: The Metal Age".to_string()]);
    let title = id_of(&games, "Thief II: The Metal Age");
    let row = db.wishlist_row(PlayerId::FIRST, id).unwrap().unwrap();
    assert_eq!((row.state, row.title_id), (WishState::InLibrary, Some(title)));
    assert_eq!(db.wishlist_row(PlayerId::FIRST, other).unwrap().unwrap().state, WishState::Wishlisted);
    let from = db.wishlist_from(PlayerId::FIRST, title).unwrap().expect("the game page says it was on the wishlist");
    assert_eq!((from.reason.as_deref(), from.source), (Some(reason), WishSource::Collection));
    let lists = lists(&db, PlayerId::FIRST, &collections(&db)).unwrap();
    assert_eq!((lists.wishlist.len(), lists.in_library), (1, 1));
}
