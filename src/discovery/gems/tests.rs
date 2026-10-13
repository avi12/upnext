//! Hidden gems: the pool, the job's one AI call, and the shelf.

use super::*;
use crate::db::PlayerId;
use crate::db::{NewStatement, Status};
use crate::discovery::fixture::{Spec, game, id_of, library};
use crate::discovery::{Lens, Mode};
use crate::enrich::{EnrichOptions, Progress, ProgressSink, run_enrich};
use crate::recommend::{PickRequest, pick};
use crate::testutil::{NoAi, ScriptedLlm};

const LIBRARY: &[Spec] = &[
    // In the pool.
    Spec {
        tags: &["Puzzle", "Cozy", "Relaxing"],
        reviews: (97, 20_000),
        hours: Some(3.5),
        developer: "Witch Beam",
        publisher: "Humble Games",
        ..game("Unpacking", 1)
    },
    Spec {
        tags: &["Exploration", "Cute", "Adventure"],
        reviews: (97, 9_000),
        hours: Some(2.5),
        developer: "adamgryu",
        publisher: "adamgryu",
        ..game("A Short Hike", 2)
    },
    Spec { tags: &["Puzzle", "Exploration"], reviews: (95, 1_200), hours: Some(4.0), ..game("Viewfinder", 3) },
    Spec { tags: &["Comedy", "Trivia"], reviews: (96, 3_000), hours: Some(1.0), ..game("Party Pack", 4) },
    // Out: a low score, too few reviews, too long, long by its tags, endless, played, a hard no.
    Spec { tags: &["Puzzle"], reviews: (90, 50_000), hours: Some(3.0), ..game("Mostly Liked", 10) },
    Spec { tags: &["Puzzle"], reviews: (99, 300), hours: Some(3.0), ..game("Tiny Following", 11) },
    Spec { tags: &["Adventure"], reviews: (97, 9_000), hours: Some(12.0), ..game("Long Story", 12) },
    Spec { tags: &["Open World", "Adventure"], reviews: (97, 9_000), hours: None, ..game("No Time", 13) },
    Spec { tags: &["Sandbox"], reviews: (97, 9_000), hours: Some(3.0), endless: true, ..game("Endless Sandbox", 14) },
    Spec { tags: &["Puzzle"], reviews: (97, 9_000), hours: Some(3.0), minutes: 600, ..game("Played Gem", 15) },
    Spec { tags: &["Horror", "Puzzle"], reviews: (97, 9_000), hours: Some(3.0), ..game("Scary Gem", 16) },
];

fn hard_no_horror(db: &Db) {
    db.taste_state(
        PlayerId::FIRST,
        &NewStatement { kind: "hard_no".into(), title_id: None, about: Some("tag:Horror".into()), why: None, note: None },
    )
    .unwrap();
}

fn names<'a>(games: impl IntoIterator<Item = &'a GameView>) -> Vec<&'a str> {
    let mut v: Vec<&str> = games.into_iter().map(|g| g.name.as_str()).collect();
    v.sort();
    v
}

#[test]
fn the_pool_is_unplayed_highly_rated_and_short() {
    let (_dir, db, cfg) = library(LIBRARY);
    hard_no_horror(&db);
    let games = db.all_games(PlayerId::FIRST).unwrap();
    let profile = Profile::load(&db, PlayerId::FIRST, &games).unwrap();
    let rules = Rules::from_config(&cfg);
    assert_eq!(rules, Rules::default(), "the settings' defaults");
    assert_eq!(
        names(pool(&games, &rules, Some(&profile))),
        ["A Short Hike", "Party Pack", "Unpacking", "Viewfinder"],
        "{:?}",
        games.iter().map(|g| (&g.name, g.status, g.review_pct, g.review_total, g.hltb_main)).collect::<Vec<_>>()
    );
    // Looser settings let more in.
    let loose = Rules { min_positive: 90, min_reviews: 200, max_hours: 15.0, count: 12 };
    let mut more = names(pool(&games, &loose, Some(&profile)));
    more.retain(|n| !["A Short Hike", "Party Pack", "Unpacking", "Viewfinder"].contains(n));
    assert_eq!(more, ["Long Story", "Mostly Liked", "Tiny Following"]);
    // GOG's rating stands in when Steam has no reviews.
    let mut gog = games.iter().find(|g| g.name == "Viewfinder").unwrap().clone();
    (gog.review_pct, gog.review_total, gog.gog_rating, gog.gog_rating_count) = (None, None, Some(4.6), Some(250));
    assert!(trusted(&gog, &rules));
    gog.gog_rating_count = Some(150);
    assert!(!trusted(&gog, &rules));
    // With no HowLongToBeat time (it's off by default), the tags decide:
    // an open world is long, Steam's "Short" tag is short, and a game nothing
    // speaks for stays in the pool; a HowLongToBeat time still wins.
    let mut g = games.iter().find(|g| g.name == "No Time").unwrap().clone();
    assert_eq!(length(&g, &rules), Length::Long);
    assert!(!in_pool(&g, &rules, Some(&profile)), "an open world");
    for tags in [&mut g.tags, g.tags_by_source.entry("steam".into()).or_default()] {
        *tags = vec!["Adventure".into(), "Story Rich".into()];
    }
    assert_eq!(length(&g, &rules), Length::Unknown);
    assert!(in_pool(&g, &rules, Some(&profile)), "nothing says how long it is");
    g.tags_by_source.get_mut("steam").unwrap().push("Short".into());
    assert_eq!(length(&g, &rules), Length::Short);
    g.hltb_main = Some(12.0);
    assert!(!in_pool(&g, &rules, Some(&profile)));
    // Players saying it wants long sittings put it out too.
    g.hltb_main = None;
    g.tags_by_source.get_mut("steam").unwrap().pop();
    g.review_tags = serde_json::from_value(json!({"scales": {"sessions": "long"}})).ok();
    assert!(g.review_tags.is_some());
    assert_eq!(length(&g, &rules), Length::Long);
}

/// A library shaped like a real one: Steam tags and review scores,
/// no HowLongToBeat time and no review tags. "A hidden gem" still has games
/// to pick, those Steam calls short first, and RPGs and open worlds stay out.
#[test]
fn a_library_without_lengths_still_has_gems() {
    const REAL: &[Spec] = &[
        Spec { tags: &["Puzzle", "Atmospheric", "Short", "Indie"], reviews: (96, 30_000), hours: None, ..game("Short Puzzle", 1) },
        Spec { tags: &["Walking Simulator", "Story Rich", "Atmospheric"], reviews: (97, 60_000), hours: None, ..game("Story Walk", 2) },
        Spec { tags: &["Card Game", "Deckbuilding", "Indie"], reviews: (97, 80_000), hours: None, ..game("Card Crawl", 3) },
        Spec { tags: &["Platformer", "Pixel Graphics", "Indie"], reviews: (95, 20_000), hours: None, ..game("Pixel Jump", 4) },
        Spec { tags: &["Open World", "RPG", "Fantasy"], reviews: (96, 500_000), hours: None, ..game("Big World", 5) },
        Spec { tags: &["Colony Sim", "Base Building"], reviews: (97, 150_000), hours: None, ..game("Colony", 6) },
        Spec { tags: &["Puzzle", "Indie"], reviews: (88, 9_000), hours: None, ..game("Just Fine", 7) },
    ];
    let (_dir, db, cfg) = library(REAL);
    let games = db.all_games(PlayerId::FIRST).unwrap();
    let rules = Rules::from_config(&cfg);
    assert!(games.iter().all(|g| g.hltb_main.is_none() && g.review_tags.is_none()), "no lengths at all");
    assert_eq!(names(pool(&games, &rules, None)), ["Card Crawl", "Pixel Jump", "Short Puzzle", "Story Walk"]);
    let lens = Lens::build(&db, &cfg, Mode::Gems, &games, None).unwrap();
    assert_eq!(lens.members.len(), 4, "enough for a lineup of three");
    let member = |name: &str| lens.member(id_of(&games, name)).unwrap();
    assert!(member("Short Puzzle").points > member("Story Walk").points, "the one Steam calls short comes first");
    assert!(member("Story Walk").note.ends_with("its length isn't known"), "{}", member("Story Walk").note);
}

/// A reply of the shape `schema()` asks for.
fn reply(games: &[GameView]) -> Value {
    let id = |name: &str| id_of(games, name);
    json!({"games": [
        {"id": id("Unpacking"), "studio": 2, "mechanics": 2, "play": 2, "party": false, "line": "You unpack a life, box by box,   and it tells you who they were."},
        {"id": id("A Short Hike"), "studio": 3, "mechanics": 1, "play": 9, "party": false, "line": "A mountain, a phone signal, and a hike you'll take slowly."},
        {"id": id("Party Pack"), "studio": 1, "mechanics": 2, "play": 2, "party": true, "line": "Shouting at friends."},
        {"id": id("Viewfinder"), "studio": 2, "mechanics": 3, "play": 3, "party": false, "line": ""},
        {"id": id("Long Story"), "studio": 3, "mechanics": 3, "play": 3, "party": false, "line": "Not asked about."},
        {"id": "#bad", "studio": 3, "mechanics": 3, "play": 3, "party": false, "line": "No id."},
    ]})
}

struct Quiet;

impl ProgressSink for Quiet {
    fn update(&self, _: &Progress) {}
    fn log(&self, _: String) {}
}

/// The job: one call for the pool, every other source off.
fn only_gems(refresh: bool) -> EnrichOptions {
    let sources = crate::enrich::sources::options().iter().map(|o| (o.id.to_string(), o.id == "hidden_gems")).collect();
    EnrichOptions { sources, refresh, ..Default::default() }
}

#[tokio::test]
async fn find_hidden_gems_rates_the_pool_in_one_call_and_fills_the_shelf() {
    let (_dir, db, cfg) = library(LIBRARY);
    hard_no_horror(&db);
    let games = db.all_games(PlayerId::FIRST).unwrap();
    let clients = crate::enrich::Clients::new(&cfg).unwrap();
    let llm = ScriptedLlm::new(vec![Box::new({
        let games = games.clone();
        move |_| Ok(reply(&games))
    })]);
    let s = run_enrich(&db, &cfg, &clients, Some(&llm), &only_gems(false), &Quiet).await.unwrap();
    assert_eq!(s.count("hidden_gems"), 3, "Viewfinder came back without a line");
    assert_eq!(s.errors, 1);

    let asked = llm.asked.lock().unwrap()[0].clone();
    assert!(asked.background, "a job's call");
    assert_eq!(asked.schema, schema());
    for name in ["Unpacking", "A Short Hike", "Viewfinder", "Party Pack"] {
        assert!(asked.prompt.contains(&format!(" {name} (2020)\n")), "{name}:\n{}", asked.prompt);
    }
    for out in ["Long Story", "Played Gem", "Scary Gem", "Endless Sandbox"] {
        assert!(!asked.prompt.contains(out), "{out}:\n{}", asked.prompt);
    }
    assert!(asked.prompt.contains("Studio: developed by adamgryu; published by adamgryu (self-published)"), "{}", asked.prompt);
    assert!(asked.prompt.contains("Studio: developed by Witch Beam; published by Humble Games\n"));
    assert!(
        asked.prompt.contains("Rarest tags in their library: Cozy (1 of 11 games), Relaxing (1 of 11 games), Puzzle ("),
        "{}",
        asked.prompt
    );
    assert!(asked.prompt.contains("People say: Praised for Unpacking.") || asked.prompt.contains("Main story: 3.5h"));

    let rated = db.hidden_gems().unwrap();
    let hike = &rated[&id_of(&games, "A Short Hike")];
    assert_eq!((hike.studio, hike.mechanics, hike.play, hike.score()), (3, 1, 3, 7), "ratings are clamped to 0 to 3");
    assert_eq!((hike.prompt_version, hike.model.as_deref()), (PROMPT_VERSION, Some("scripted")));
    assert_eq!(rated[&id_of(&games, "Unpacking")].line, "You unpack a life, box by box, and it tells you who they were.");
    assert!(rated[&id_of(&games, "Party Pack")].party);
    assert!(!rated.contains_key(&id_of(&games, "Long Story")), "not in this call's games");

    let profile = Profile::load(&db, PlayerId::FIRST, &games).unwrap();
    let rules = Rules::from_config(&cfg);
    let s = shelf(&games, Some(&profile), &rules, &rated, true);
    assert_eq!(s.gems.iter().map(|g| g.game.name.as_str()).collect::<Vec<_>>(), ["A Short Hike", "Unpacking"], "best first, no party game");
    assert_eq!((s.pool, s.rated, s.unrated, s.party), (4, 3, 1, 1));
    let all: Vec<&str> = s.ranked.as_ref().unwrap().iter().map(|g| g.game.name.as_str()).collect();
    assert_eq!(all, ["A Short Hike", "Unpacking", "Party Pack", "Viewfinder"], "ranked, then party, then unrated");
    assert_eq!(s.gems[0].score, Some(7));

    // A second run rates only what's missing; a refresh rates them all.
    let llm = ScriptedLlm::new(vec![Box::new({
        let games = games.clone();
        move |req: &crate::llm::LlmRequest| {
            assert!(req.prompt.contains("Viewfinder") && !req.prompt.contains("Unpacking"), "{}", req.prompt);
            let id = id_of(&games, "Viewfinder");
            Ok(json!({"games": [{"id": id, "studio": 2, "mechanics": 3, "play": 3, "party": false, "line": "Photos you can walk into."}]}))
        }
    })]);
    let s = run_enrich(&db, &cfg, &clients, Some(&llm), &only_gems(false), &Quiet).await.unwrap();
    assert_eq!((s.count("hidden_gems"), s.errors), (1, 0));
    let rated = db.hidden_gems().unwrap();
    let shelf_now = shelf(&games, Some(&profile), &rules, &rated, false);
    assert_eq!(shelf_now.gems.iter().map(|g| g.game.name.as_str()).collect::<Vec<_>>(), ["Viewfinder", "A Short Hike", "Unpacking"]);
    assert!(shelf_now.ranked.is_none());

    let llm = ScriptedLlm::new(vec![Box::new({
        let games = games.clone();
        move |req: &crate::llm::LlmRequest| {
            assert!(req.prompt.contains("Viewfinder") && req.prompt.contains("Unpacking"));
            Ok(reply(&games))
        }
    })]);
    run_enrich(&db, &cfg, &clients, Some(&llm), &only_gems(true), &Quiet).await.unwrap();
    assert_eq!(llm.asked.lock().unwrap().len(), 1);

    // Off by default: a plain enrich never calls the AI for it.
    let llm = ScriptedLlm::new(vec![]);
    let mut others = only_gems(false);
    others.sources.remove("hidden_gems");
    others.sources.values_mut().for_each(|on| *on = false);
    run_enrich(&db, &cfg, &clients, Some(&llm), &others, &Quiet).await.unwrap();
    assert!(llm.asked.lock().unwrap().is_empty());
}

#[test]
fn a_played_gem_leaves_and_the_shelf_moves_up() {
    let (_dir, db, cfg) = library(LIBRARY);
    let games = db.all_games(PlayerId::FIRST).unwrap();
    let row = |score: u8, line: &str| GemRow {
        studio: score,
        mechanics: score,
        play: score,
        party: false,
        line: line.into(),
        prompt_version: PROMPT_VERSION,
        model: Some("scripted".into()),
        written_at: 0,
    };
    for (name, score) in [("Unpacking", 3), ("A Short Hike", 2), ("Viewfinder", 1)] {
        db.put_gem(id_of(&games, name), &row(score, &format!("{name} is odd."))).unwrap();
    }
    let rules = Rules { count: 2, ..Rules::from_config(&cfg) };
    let shelf_of = |games: &[GameView]| -> Vec<String> {
        shelf(games, None, &rules, &db.hidden_gems().unwrap(), false).gems.iter().map(|g| g.game.name.clone()).collect()
    };
    assert_eq!(shelf_of(&games), ["Unpacking", "A Short Hike"], "gem_count caps the shelf");
    let mut played = games.clone();
    let unpacking = played.iter_mut().find(|g| g.name == "Unpacking").unwrap();
    (unpacking.status, unpacking.playtime_minutes) = (Status::Played, 240);
    assert_eq!(shelf_of(&played), ["A Short Hike", "Viewfinder"]);

    // An older prompt's rating counts on the shelf until the job runs again.
    let mut old = row(3, "Old.");
    old.prompt_version = PROMPT_VERSION - 1;
    db.put_gem(id_of(&games, "Viewfinder"), &old).unwrap();
    let s = shelf(&games, None, &rules, &db.hidden_gems().unwrap(), false);
    assert_eq!((s.rated, s.unrated), (2, 3), "Viewfinder's rating is old; Party Pack and Scary Gem (no hard no here) have none");

    // Its members: points for the ratings, the line in the note.
    let m = members(&games, None, &rules, &db.hidden_gems().unwrap());
    let u = &m[&id_of(&games, "Unpacking")];
    assert_eq!(u.note, "unusual: Unpacking is odd. (studio 3/3, mechanics 3/3, play 3/3)");
    assert_eq!((u.reason.as_str(), u.points), ("Unusual: Unpacking is odd", 27.0));
    assert_eq!(m[&id_of(&games, "Party Pack")].note, "hidden gem, not rated yet");
}

/// "A hidden gem" picks from the pool, best rated first, without the AI.
#[tokio::test]
async fn a_hidden_gem_without_ai_picks_from_the_pool() {
    let (_dir, db, cfg) = library(LIBRARY);
    hard_no_horror(&db);
    let games = db.all_games(PlayerId::FIRST).unwrap();
    let row = |score: u8, party: bool| GemRow {
        studio: score,
        mechanics: score,
        play: score,
        party,
        line: "Odd.".into(),
        prompt_version: PROMPT_VERSION,
        model: None,
        written_at: 0,
    };
    db.put_gem(id_of(&games, "Viewfinder"), &row(3, false)).unwrap();
    db.put_gem(id_of(&games, "Party Pack"), &row(3, true)).unwrap();
    let req = PickRequest {
        prompt: "A hidden gem".into(),
        count: 2,
        exclude_ids: vec![],
        sources: None,
        avoid: vec![],
        without_ai: true,
        mode: None,
        routine_id: None,
        input: None,
    };
    let batch = pick(&db, &cfg, &NoAi, None, req).await.unwrap();
    assert_eq!(batch.mode, Some(Mode::Gems));
    let lens = Lens::build(&db, &cfg, Mode::Gems, &games, None).unwrap();
    assert!(!lens.allows(id_of(&games, "Party Pack")), "party games are left out");
    assert_eq!(batch.picks[0].name, "Viewfinder", "the best rated first");
    assert!(batch.picks[0].reason.starts_with("Unusual: Odd"), "{}", batch.picks[0].reason);
    for p in batch.picks.iter().chain(&batch.spares) {
        assert!(["Viewfinder", "Unpacking", "A Short Hike"].contains(&p.name.as_str()), "{}", p.name);
    }
    assert!(batch.ranked_by.unwrap()[0].starts_with("hidden gems only"));
}
