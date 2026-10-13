//! The taste profile against a small library: five short story games you
//! finished, five grindy ones you put down three months ago (four over the
//! grind, two since marked Abandoned), one you left only because life got
//! busy, and a few you haven't touched.

use super::*;
use crate::config::Config;
use crate::db::PlayerId;
use crate::db::{HltbRow, ImportedGame, Override};
use crate::review_tags::ReviewTags;

const DAY_S: i64 = 86_400;

fn tags(json: &str) -> ReviewTags {
    serde_json::from_str(json).unwrap()
}

/// The library, with the ids of its games by name.
fn library() -> (tempfile::TempDir, Db, Config, HashMap<String, i64>) {
    let dir = tempfile::tempdir().unwrap();
    let cfg = crate::testutil::offline_config(dir.path());
    let db = Db::open(&cfg.db_path, cfg.sampled_minutes).unwrap();
    let t = now();
    let steam = |appid: u32, name: &str, minutes: i64, days_ago: i64| ImportedGame {
        source: "steam".into(),
        source_id: appid.to_string(),
        launcher: "steam".into(),
        name: name.into(),
        appid: Some(appid),
        playtime_minutes: Some(minutes),
        last_played: if minutes > 0 { t - days_ago * DAY_S } else { 0 },
        ..Default::default()
    };
    let mut games = vec![];
    // Played through: 7 to 8 hours of an 8-hour story, three months ago.
    for (i, name) in ["Story A", "Story B", "Story C", "Story D", "Story E"].iter().enumerate() {
        games.push(steam(100 + i as u32, name, 470, 70 + 7 * i as i64));
    }
    // Put down 6 hours into a 40-hour grind three months ago: shelved.
    for (i, name) in ["Grind A", "Grind B", "Grind C", "Grind D", "Grind E", "Busy Grind"].iter().enumerate() {
        games.push(steam(200 + i as u32, name, 360, 90 + i as i64));
    }
    games.push(steam(300, "Untouched", 0, 0));
    games.push(steam(301, "Sampled", 30, 400));
    db.sync_source(PlayerId::FIRST, "steam", "steam", &games).unwrap();
    let ids: HashMap<String, i64> = db.all_games(PlayerId::FIRST).unwrap().into_iter().map(|g| (g.name.clone(), g.id)).collect();
    let hltb = |h: f64| HltbRow { main_story: Some(h), ..Default::default() };
    let story = tags(r#"{"scales": {"story": "central", "pace": "steady"}, "aspects": [{"aspect": "story", "verdict": "praised"}]}"#);
    let grind = tags(r#"{"scales": {"grind": "heavy"}, "mood": ["epic"]}"#);
    for (name, id) in &ids {
        let (h, t) = match &name[..5] {
            "Story" => (8.0, &story),
            "Grind" | "Busy " => (40.0, &grind),
            _ => continue,
        };
        db.put_hltb(*id, Some(&hltb(h))).unwrap();
        db.put_digest(*id, "People say", "", t, "test", crate::digest::DIGEST_VERSION).unwrap();
    }
    db.set_override(PlayerId::FIRST, ids["Story B"], Some(Override::Done), None).unwrap();
    for name in ["Grind A", "Grind B"] {
        db.set_override(PlayerId::FIRST, ids[name], Some(Override::Abandoned), None).unwrap();
    }
    for name in ["Grind A", "Grind B", "Grind C", "Grind E"] {
        db.add_stop_reasons(PlayerId::FIRST, ids[name], &["grind".to_string()], None, "menu").unwrap();
    }
    db.add_stop_reasons(PlayerId::FIRST, ids["Busy Grind"], &["busy".to_string()], Some("new job"), "menu").unwrap();
    (dir, db, cfg, ids)
}

fn found(db: &Db, rules: &TasteRules, keep: impl Fn(&GameView) -> bool) -> (Vec<Found>, Judged) {
    let games: Vec<GameView> = db.all_games(PlayerId::FIRST).unwrap().into_iter().filter(|g| keep(g)).collect();
    find_patterns(&games, &HashSet::new(), &HashSet::new(), rules, now())
}

fn sentences(f: &[Found]) -> Vec<&str> {
    f.iter().map(|p| p.sentence.as_str()).collect()
}

#[test]
fn beta_tail_matches_the_binomial() {
    // I_x(a, b) for whole a and b is P(Binomial(a + b - 1, x) >= a).
    assert!((beta_cdf(0.5, 2.0, 6.0) - 120.0 / 128.0).abs() < 1e-9);
    assert!((beta_cdf(0.5, 3.0, 3.0) - 0.5).abs() < 1e-9);
    assert!((beta_cdf(0.3, 1.0, 1.0) - 0.3).abs() < 1e-9);
    // Shares a prior gives, under 1.
    let small = beta_cdf(0.5, 0.4, 7.6);
    assert!(small > 0.99 && small <= 1.0, "{small}");
}

#[test]
fn each_game_counts_one_way_or_not_at_all() {
    let (_dir, db, _cfg, ids) = library();
    let all = db.all_games(PlayerId::FIRST).unwrap();
    let outcome = |name: &str| {
        let g = all.iter().find(|g| g.id == ids[name]).unwrap();
        judge(g, false).map(|(o, _)| o)
    };
    assert_eq!(outcome("Story A"), Some(Outcome::PlayedThrough));
    assert_eq!(outcome("Story B"), Some(Outcome::Beaten));
    assert_eq!(outcome("Grind A"), Some(Outcome::Abandoned));
    assert_eq!(outcome("Grind C"), Some(Outcome::Shelved));
    assert_eq!(outcome("Busy Grind"), None, "life reasons never count against a game");
    assert_eq!((outcome("Untouched"), outcome("Sampled")), (None, None));
    let sampled = all.iter().find(|g| g.name == "Sampled").unwrap();
    assert_eq!(judge(sampled, true), Some((Outcome::Favourite, None)), "a favourite counts, however little it's played");
    let shelved = all.iter().find(|g| g.name == "Grind C").unwrap();
    assert!(shelved.shelved_at.is_some());
    assert_eq!(judge(shelved, false).unwrap().1, Some(shelved.last_played), "from when you stopped, not when UpNext noticed");
    let marked = all.iter().find(|g| g.name == "Grind A").unwrap();
    assert_eq!(judge(marked, false).unwrap().1, marked.override_at, "a mark from when you set it");
}

#[test]
fn old_evidence_fades_by_the_half_life() {
    let rules = TasteRules::default();
    let t = now();
    assert!((weight(Outcome::Abandoned, Some(t), &rules, t) - 1.0).abs() < 1e-9);
    assert!((weight(Outcome::Abandoned, Some(t - 365 * DAY_S), &rules, t) - 0.5).abs() < 1e-9);
    assert!((weight(Outcome::Staple, Some(t - 730 * DAY_S), &rules, t) - 0.25).abs() < 1e-9);
    assert_eq!(weight(Outcome::Favourite, Some(t - 3650 * DAY_S), &rules, t), 1.0, "favourites never fade");
    assert_eq!(weight(Outcome::PlayedThrough, None, &rules, t), 0.5, "no date: a half-life old");

    // With a half-life of a day, the library's months-old evidence is
    // almost nothing beside the prior: no patterns.
    let (_dir, db, _cfg, _ids) = library();
    let (fresh, _) = found(&db, &rules, |_| true);
    assert!(!fresh.is_empty());
    let (faded, judged) = found(&db, &TasteRules { half_life_days: 1, ..rules }, |_| true);
    assert_eq!((faded.len(), judged.kept, judged.left), (0, 5, 5));
}

#[test]
fn story_games_kept_and_grind_left_make_two_patterns() {
    let (_dir, db, _cfg, _ids) = library();
    let (f, judged) = found(&db, &TasteRules::default(), |_| true);
    assert_eq!((judged.kept, judged.left), (5, 5));
    // Length says the same about the same games, so only the review tags show.
    assert_eq!(sentences(&f), ["You drift away from games with heavy grind.", "You stick with story-driven games."]);
    let drift = &f[0];
    assert_eq!(drift.key, "drift:facet:grind:heavy");
    assert_eq!(drift.evidence.summary, "3 shelved, 2 abandoned, 4 of them for the grind", "life reasons left out");
    assert!(drift.confidence >= MIN_CONFIDENCE && drift.evidence.share == 0.0);
    assert!(drift.evidence.games.iter().all(|g| g.name != "Busy Grind"));
    assert_eq!(drift.evidence.games.iter().filter(|g| g.reasons == ["grind"]).count(), 4);
    let stick = &f[1];
    assert_eq!(stick.evidence.summary, "4 played through, 1 beaten");
    assert_eq!(stick.evidence.games.len(), 5);
}

#[test]
fn three_games_are_not_a_pattern() {
    let (_dir, db, _cfg, _ids) = library();
    let rules = TasteRules::default();
    // Three grind games left, four story games kept.
    let some = |g: &GameView| !["Grind A", "Grind B", "Story A"].contains(&g.name.as_str());
    let (f, _) = found(&db, &rules, some);
    assert!(f.iter().all(|p| p.evidence.sense == Sense::Stick), "{:?}", sentences(&f));
    // With the setting at 3 they are.
    let (f, _) = found(&db, &TasteRules { min_games: 3, ..rules }, some);
    assert!(sentences(&f).contains(&"You drift away from games with heavy grind."), "{:?}", sentences(&f));
}

#[test]
fn nothing_to_compare_when_everything_went_one_way() {
    let (_dir, db, _cfg, _ids) = library();
    let (f, judged) = found(&db, &TasteRules::default(), |g| !g.name.starts_with("Story"));
    assert_eq!((f.len(), judged.kept, judged.left), (0, 0, 5));
}

#[test]
fn not_really_holds_after_the_patterns_are_counted_again() {
    let (_dir, db, cfg, _ids) = library();
    let rules = cfg.taste_rules();
    let first = update(&db, PlayerId::FIRST, &rules).unwrap();
    assert_eq!((first.patterns, first.changes.added), (2, 2));
    let p = page(&db, PlayerId::FIRST).unwrap();
    let drift = p.patterns.iter().find(|x| x.key == "drift:facet:grind:heavy").unwrap();
    assert!(drift.sure || drift.confidence >= MIN_CONFIDENCE);
    edit(&db, PlayerId::FIRST, &rules, drift.id, &EditBody { hidden: Some(true), ..Default::default() }).unwrap();

    let again = update(&db, PlayerId::FIRST, &rules).unwrap();
    assert_eq!(again.changes, PatternChanges { added: 0, updated: 2, removed: 0 });
    let p = page(&db, PlayerId::FIRST).unwrap();
    assert_eq!(p.patterns.iter().map(|x| x.key.as_str()).collect::<Vec<_>>(), ["stick:facet:story:central"]);
    assert_eq!(p.hidden_patterns.len(), 1);

    // When the evidence goes, a shown pattern goes; a hidden one stays hidden.
    let strict = TasteRules { min_games: 10, ..rules };
    let gone = update(&db, PlayerId::FIRST, &strict).unwrap();
    assert_eq!((gone.patterns, gone.changes.removed), (0, 1));
    let p = page(&db, PlayerId::FIRST).unwrap();
    assert_eq!((p.patterns.len(), p.hidden_patterns.len()), (0, 1));
    update(&db, PlayerId::FIRST, &rules).unwrap();
    let p = page(&db, PlayerId::FIRST).unwrap();
    assert_eq!((p.patterns.len(), p.hidden_patterns[0].key.as_str()), (1, "drift:facet:grind:heavy"));

    // Show again.
    let id = p.hidden_patterns[0].id;
    edit(&db, PlayerId::FIRST, &rules, id, &EditBody { hidden: Some(false), ..Default::default() }).unwrap();
    assert_eq!(page(&db, PlayerId::FIRST).unwrap().patterns.len(), 2);
    assert!(delete(&db, PlayerId::FIRST, &rules, id).is_err(), "patterns aren't deleted, they're hidden");
}

#[test]
fn what_you_said_wins_over_what_was_counted() {
    let (_dir, db, cfg, _ids) = library();
    let rules = cfg.taste_rules();
    update(&db, PlayerId::FIRST, &rules).unwrap();
    // That's right: the story pattern becomes a like, and goes.
    let story = page(&db, PlayerId::FIRST).unwrap().patterns.into_iter().find(|p| p.key == "stick:facet:story:central").unwrap();
    let like = edit(&db, PlayerId::FIRST, &rules, story.id, &EditBody { confirm: true, ..Default::default() }).unwrap().unwrap();
    assert_eq!((like.kind.as_str(), like.about.as_deref(), like.source.as_str()), ("like", Some("facet:story:central"), "stated"));
    let p = page(&db, PlayerId::FIRST).unwrap();
    assert!(p.patterns.iter().all(|x| x.key != "stick:facet:story:central"));
    // Length said the same about the same games; with story taken, it shows.
    assert!(p.patterns.iter().any(|x| x.sentence == "You stick with games under 10 hours."), "{:?}", p.patterns);
    let liked = p.statements.iter().find(|s| s.about == "facet:story:central").unwrap();
    assert_eq!((liked.label.as_str(), liked.matches), ("Story-driven", Some(5)));

    // A dislike stated by hand keeps the grind pattern away too.
    let new = check_statement(
        &db,
        PlayerId::FIRST,
        &StatementBody { kind: Some("dislike".into()), about: Some("facet:Grind:Heavy".into()), ..Default::default() },
    )
    .unwrap();
    assert_eq!(new.about.as_deref(), Some("facet:grind:heavy"), "spelled as the vocabulary spells it");
    db.taste_state(PlayerId::FIRST, &new).unwrap();
    update(&db, PlayerId::FIRST, &rules).unwrap();
    assert!(page(&db, PlayerId::FIRST).unwrap().patterns.iter().all(|x| x.key != "drift:facet:grind:heavy"));

    // Turned into a hard no, it's still one statement.
    let hard = check_statement(
        &db,
        PlayerId::FIRST,
        &StatementBody { kind: Some("hard_no".into()), about: Some("facet:grind:heavy".into()), ..Default::default() },
    )
    .unwrap();
    db.taste_state(PlayerId::FIRST, &hard).unwrap();
    let p = page(&db, PlayerId::FIRST).unwrap();
    let grind: Vec<&StatementView> = p.statements.iter().filter(|s| s.about == "facet:grind:heavy").collect();
    assert_eq!((grind.len(), grind[0].kind.as_str(), grind[0].matches), (1, "hard_no", Some(6)));
}

#[test]
fn favourites_count_and_carry_their_reasons() {
    let (_dir, db, cfg, ids) = library();
    let rules = cfg.taste_rules();
    let body = |why: Value| StatementBody {
        kind: Some("favourite".into()),
        title_id: Some(ids["Story A"]),
        why: Some(why),
        note: Some("  the last hour  ".into()),
        ..Default::default()
    };
    let fav = check_statement(&db, PlayerId::FIRST, &body(json!(["aspect:story", "facet:story:central", "tag:Great Soundtrack"]))).unwrap();
    assert_eq!(fav.note.as_deref(), Some("the last hour"));
    db.taste_state(PlayerId::FIRST, &fav).unwrap();
    assert!(check_statement(&db, PlayerId::FIRST, &body(json!(["aspect:bugs"]))).is_err(), "bugs aren't a reason to love a game");
    assert!(check_statement(&db, PlayerId::FIRST, &body(json!(["facet:story:loud"]))).is_err());
    assert!(
        check_statement(
            &db,
            PlayerId::FIRST,
            &StatementBody { kind: Some("favourite".into()), title_id: Some(99_999), ..Default::default() }
        )
        .is_err()
    );

    let p = page(&db, PlayerId::FIRST).unwrap();
    let f = &p.favourites[0];
    assert_eq!(f.why.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(), ["the story", "story-driven", "Great Soundtrack"]);
    assert_eq!(f.choices.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(), ["aspect:story", "facet:pace:steady", "facet:story:central"]);
    assert!(p.suggestions.iter().all(|g| g.id != ids["Story A"]), "a favourite isn't suggested again");
    assert_eq!(p.suggestions[0].name, "Story B", "the most played, recent play first");

    // Editing its reasons; a favourite stated again keeps its row.
    let id = f.id;
    edit(&db, PlayerId::FIRST, &rules, id, &EditBody { why: Some(json!(["aspect:story"])), ..Default::default() }).unwrap();
    assert_eq!(page(&db, PlayerId::FIRST).unwrap().favourites[0].why.len(), 1);
    assert_eq!(db.taste_state(PlayerId::FIRST, &fav).unwrap(), id);
    assert!(edit(&db, PlayerId::FIRST, &rules, id, &EditBody { kind: Some("like".into()), ..Default::default() }).is_err());
    delete(&db, PlayerId::FIRST, &rules, id).unwrap();
    assert!(page(&db, PlayerId::FIRST).unwrap().favourites.is_empty());
}

#[test]
fn habits_take_minutes_or_their_own_choices() {
    let (_dir, db, _cfg, _ids) = library();
    let habit = |about: &str, why: Value| {
        check_statement(
            &db,
            PlayerId::FIRST,
            &StatementBody { kind: Some("context".into()), about: Some(about.into()), why: Some(why), ..Default::default() },
        )
    };
    assert_eq!(habit("weeknight", json!(90)).unwrap().why, Some(json!(90)));
    assert!(habit("weeknight", json!(2000)).is_err());
    assert_eq!(
        habit("who", json!(["friends", "solo", "friends"])).unwrap().why,
        Some(json!(["solo", "friends"])),
        "in the list's order, once"
    );
    assert!(habit("who", json!(["dog"])).is_err());
    assert!(habit("mood", json!(1)).is_err());
    let first = db.taste_state(PlayerId::FIRST, &habit("weekend", json!(180)).unwrap()).unwrap();
    assert_eq!(db.taste_state(PlayerId::FIRST, &habit("weekend", json!(240)).unwrap()).unwrap(), first, "one answer per habit");
    assert_eq!(page(&db, PlayerId::FIRST).unwrap().habits[0].value, Some(json!(240)));
    assert!(
        check_statement(
            &db,
            PlayerId::FIRST,
            &StatementBody { kind: Some("pattern".into()), about: Some("drift:tag:Horror".into()), ..Default::default() }
        )
        .unwrap_err()
        .to_string()
        .starts_with("patterns come from how you play")
    );
}

#[test]
fn subjects_parse_spell_and_phrase() {
    assert_eq!(Subject::parse("tag:souls-like"), Some(Subject::Tag("Souls-like".into())));
    assert_eq!(Subject::parse("tag:Trains").map(|s| s.key()), Some("tag:Trains".into()), "any tag, as written");
    assert_eq!(Subject::parse("length:short").map(|s| s.label()), Some("Under 10 hours".into()));
    assert_eq!(Subject::parse("text:  games about trains ").map(|s| s.key()), Some("text:games about trains".into()));
    assert_eq!(Subject::parse("facet:mood:loud"), None);
    assert_eq!(Subject::parse("genre:RPG"), None);
    assert_eq!(Subject::parse("text: "), None);
    // Every review-tag value and tag a pattern can name is real.
    for (f, _) in vocab::FACETS {
        assert_eq!(crate::review_tags::facet_name(f).as_deref(), Some(*f), "{f}");
    }
    let mut keys: Vec<String> = vocab::TAGS.iter().map(|(t, _)| vocab::tag_key(t)).collect();
    keys.sort();
    let n = keys.len();
    keys.dedup();
    assert_eq!(keys.len(), n, "no tag twice");
}

#[test]
fn sources_say_what_is_kept_and_delete_it() {
    let (_dir, db, cfg, ids) = library();
    let rules = cfg.taste_rules();
    db.taste_state(
        PlayerId::FIRST,
        &NewStatement { kind: "like".into(), title_id: None, about: Some("tag:Horror".into()), why: None, note: None },
    )
    .unwrap();
    update(&db, PlayerId::FIRST, &rules).unwrap();
    let s = sources(&db, &cfg).unwrap();
    assert_eq!((s.library.games, s.library.played, s.library.stores.as_slice()), (13, 12, ["steam".to_string()].as_slice()));
    assert_eq!((s.statuses.games, s.statuses.by["done"], s.statuses.by["abandoned"]), (3, 1, 2));
    assert_eq!((s.reactions.stop_reasons, s.answers.likes, s.diary.on), (5, 1, false));
    assert_eq!(source_rows(&db, PlayerId::FIRST, "reactions").unwrap().unwrap().as_array().unwrap().len(), 5);

    assert!(delete_source(&db, PlayerId::FIRST, &rules, "library").is_err(), "it comes from the stores");
    assert_eq!(delete_source(&db, PlayerId::FIRST, &rules, "reactions").unwrap(), 5);
    // Without the reasons, Busy Grind counts as left too.
    let p = page(&db, PlayerId::FIRST).unwrap();
    assert_eq!(p.judged.left, 6);
    assert!(db.game(PlayerId::FIRST, ids["Busy Grind"]).unwrap().unwrap().stop_reasons.is_none());
    assert_eq!(delete_source(&db, PlayerId::FIRST, &rules, "answers").unwrap(), 1);
    assert_eq!(delete_source(&db, PlayerId::FIRST, &rules, "statuses").unwrap(), 3);
    let s = sources(&db, &cfg).unwrap();
    assert_eq!((s.statuses.games, s.reactions.stop_reasons, s.answers.likes), (0, 0, 0));
    let e = export(&db, &cfg).unwrap();
    assert!(e["statements"].as_array().unwrap().iter().all(|r| r["kind"] == "pattern"));
}

/// When you keep most games, "you stick with" is still reachable: the rule
/// is on odds, not points (a 20-point rule asked 92% here, of 5 games).
#[test]
fn stick_is_reachable_when_you_keep_most_games() {
    let (_dir, db, _cfg, _ids) = library();
    let mut games = db.all_games(PlayerId::FIRST).unwrap();
    let story = games.iter().find(|g| g.name == "Story A").unwrap().clone();
    for i in 0..10 {
        let mut g = story.clone();
        (g.id, g.name, g.review_tags, g.hltb_main) = (10_000 + i, format!("Other {i}"), None, None);
        games.push(g);
    }
    let (f, judged) = find_patterns(&games, &HashSet::new(), &HashSet::new(), &TasteRules::default(), now());
    assert_eq!((judged.kept, judged.left), (15, 5));
    assert!(sentences(&f).contains(&"You stick with story-driven games."), "{:?}", sentences(&f));
}

/// Merging titles keeps the survivor's favourite, else takes the other's;
/// the play diary's rows follow.
#[test]
fn favourites_follow_title_merges() {
    let (_dir, db, _cfg, ids) = library();
    let fav = |name: &str, why: &str| NewStatement {
        kind: "favourite".into(),
        title_id: Some(ids[name]),
        about: None,
        why: Some(json!([why])),
        note: None,
    };
    db.taste_state(PlayerId::FIRST, &fav("Story B", "aspect:story")).unwrap();
    let diary = crate::db::NewDiaryRow {
        hour: 0,
        at: 0,
        title_id: Some(ids["Story B"]),
        appid: None,
        name: "Story B".into(),
        minutes: 30,
        source: "steam",
        player: Some(PlayerId::FIRST),
        account: None,
    };
    db.diary_add(&[diary]).unwrap();
    let kept = db.merge_titles(&[ids["Story A"], ids["Story B"]]).unwrap();
    let favs: Vec<StatementRow> = db.taste_statements(PlayerId::FIRST).unwrap().into_iter().filter(|r| r.kind == "favourite").collect();
    assert_eq!((favs.len(), favs[0].title_id, favs[0].why.clone()), (1, Some(kept), Some(json!(["aspect:story"]))));
    assert_eq!(db.diary_rows(PlayerId::FIRST, 0).unwrap()[0].title_id, Some(kept), "the diary follows too");

    db.taste_state(PlayerId::FIRST, &fav("Story C", "tag:Puzzle")).unwrap();
    db.taste_state(PlayerId::FIRST, &fav("Story D", "aspect:world")).unwrap();
    let kept = db.merge_titles(&[ids["Story C"], ids["Story D"]]).unwrap();
    let favs: Vec<StatementRow> = db.taste_statements(PlayerId::FIRST).unwrap().into_iter().filter(|r| r.kind == "favourite").collect();
    let theirs: Vec<&StatementRow> = favs.iter().filter(|r| r.title_id == Some(kept)).collect();
    assert_eq!((favs.len(), theirs.len(), theirs[0].why.clone()), (2, 1, Some(json!(["tag:Puzzle"]))), "the survivor's own");
}

/// A hard no on a tag reaches the store's five strongest tags and the store
/// genres (question VIII), and never a tag only RAWG, IGDB or PCGamingWiki
/// gave the game.
#[test]
fn a_hard_no_reaches_five_store_tags() {
    let (_dir, db, _cfg, ids) = library();
    let mut g = db.game(PlayerId::FIRST, ids["Story A"]).unwrap().unwrap();
    let horror = Subject::Tag("Horror".into());
    let tags = |list: &[&str]| list.iter().map(|t| t.to_string()).collect::<Vec<_>>();
    g.genres = tags(&["Adventure"]);
    g.tags_by_source = HashMap::from([("steam".to_string(), tags(&["Adventure", "Indie", "Story Rich", "Atmospheric", "Horror"]))]);
    assert!(hard_no_matches(&horror, &g), "5th of Steam's tags");
    g.tags_by_source.insert("steam".into(), tags(&["Adventure", "Indie", "Story Rich", "Atmospheric", "Puzzle", "Horror"]));
    assert!(!hard_no_matches(&horror, &g), "6th is too far down");
    assert!(matches(&horror, &g), "a like still reaches ten");
    g.genres.push("Horror".into());
    assert!(hard_no_matches(&horror, &g), "a store genre");

    // No Steam tags: RAWG's and IGDB's don't count; a GOG copy's own do.
    g.genres = tags(&["Sports"]);
    g.tags_by_source = HashMap::from([("rawg".to_string(), tags(&["Simulation", "Horror"])), ("igdb".to_string(), tags(&["Horror"]))]);
    g.tags = tags(&["Simulation", "Horror"]);
    assert!(!hard_no_matches(&horror, &g), "a tag matched by name to maybe the wrong game");
    assert!(matches(&horror, &g), "likes and patterns still read it");
    assert!(hard_no_matches(&Subject::Tag("Sports".into()), &g), "the store genre");
    g.tags_by_source.insert("gog".into(), tags(&["Horror", "Atmospheric"]));
    assert!(hard_no_matches(&horror, &g), "GOG's own tags");
}
