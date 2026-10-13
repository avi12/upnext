//! New to you against a library played in shooters and RPGs, with
//! deckbuilders, puzzle games and more unplayed.

use super::*;
use crate::db::PlayerId;
use crate::db::{Db, NewStatement};
use crate::discovery::fixture::{Spec, game, id_of, library};
use crate::discovery::{Lens, Mode};
use crate::recommend::{PickRequest, pick};
use crate::testutil::{Answer, NoAi, ScriptedLlm, id_in};
use serde_json::json;

const LIBRARY: &[Spec] = &[
    // Played: shooters and RPGs.
    Spec { minutes: 3000, genres: &["Action"], tags: &["FPS", "Shooter", "Action"], ..game("Doom", 1) },
    Spec { minutes: 2400, genres: &["Action"], tags: &["FPS", "Shooter", "Multiplayer"], ..game("Halo", 2) },
    Spec { minutes: 6000, genres: &["RPG"], tags: &["RPG", "Open World", "Fantasy"], ..game("Skyrim", 3) },
    Spec { minutes: 3000, genres: &["RPG", "Action"], tags: &["RPG", "Shooter", "Story Rich"], ..game("Mass Effect", 4) },
    // Sampled for half an hour: roguelites are tried, if not played.
    Spec { minutes: 30, genres: &["Action"], tags: &["Roguelite", "Hack and Slash", "Action"], hours: Some(22.0), ..game("Hades", 5) },
    // Unplayed, and gentle ways into something new.
    Spec { genres: &["Strategy"], tags: &["Deckbuilding", "Roguelike", "Card Game"], hours: Some(6.0), ..game("Slay the Spire", 10) },
    Spec { genres: &["Strategy"], tags: &["Deckbuilding", "Roguelite", "Strategy"], hours: Some(6.0), ..game("Monster Train", 11) },
    Spec { genres: &["Casual"], tags: &["Puzzle", "Casual", "Relaxing"], hours: Some(3.0), ..game("Unpacking", 12) },
    Spec { tags: &["Puzzle", "Relaxing"], hours: Some(2.0), ..game("Monument Valley", 15) },
    Spec {
        genres: &["Strategy"],
        tags: &["City Builder", "Strategy", "Relaxing"],
        endless: true,
        scales: &[("learning", "gentle")],
        ..game("Dorfromantik", 13)
    },
    Spec {
        genres: &["Casual"],
        tags: &["Visual Novel", "Casual", "Story Rich"],
        hours: None,
        scales: &[("sessions", "short")],
        ..game("Florence", 14)
    },
    // Unplayed, and not gentle: hard, steep, deep, long, endless and hard to learn.
    Spec { tags: &["Puzzle", "Indie"], scales: &[("difficulty", "hard")], ..game("Baba Is You", 20) },
    Spec { tags: &["Precision Platformer", "Platformer", "Difficult"], ..game("Celeste", 21) },
    Spec { tags: &["Puzzle", "Exploration"], hours: Some(20.0), ..game("The Witness", 22) },
    Spec { tags: &["Grand Strategy", "Strategy"], hours: None, scales: &[("complexity", "deep")], ..game("Crusader Kings", 23) },
    Spec { tags: &["City Builder", "Survival"], scales: &[("learning", "steep")], ..game("Frostpunk", 24) },
    Spec { tags: &["Automation", "Base Building"], endless: true, ..game("Factorio", 25) },
    Spec { tags: &["Rhythm", "Music"], hours: None, ..game("Thumper", 26) },
    // Unplayed, but nothing new: another shooter.
    Spec { genres: &["Action"], tags: &["FPS", "Shooter"], ..game("Halo 2", 27) },
    // Unplayed and new, but horror: a hard no below.
    Spec { tags: &["Deckbuilding", "Card Game", "Horror", "Puzzle"], ..game("Inscryption", 28) },
];

fn hard_no(db: &Db, about: &str) {
    db.taste_state(
        PlayerId::FIRST,
        &NewStatement { kind: "hard_no".into(), title_id: None, about: Some(about.into()), why: None, note: None },
    )
    .unwrap();
}

fn profile(db: &Db, games: &[GameView]) -> Profile {
    Profile::load(db, PlayerId::FIRST, games).unwrap()
}

fn named(games: &[GameView], name: &str) -> GameView {
    games.iter().find(|g| g.name == name).unwrap().clone()
}

#[test]
fn the_pool_is_only_what_you_have_not_played_and_gentle() {
    let (_dir, db, _cfg) = library(LIBRARY);
    hard_no(&db, "tag:Horror");
    let games = db.all_games(PlayerId::FIRST).unwrap();
    let profile = profile(&db, &games);
    let rules = Rules::default();

    let familiar = Familiar::of(&games, &rules);
    assert!(!familiar.is_new("FPS", &rules) && !familiar.is_new("RPG", &rules), "two played games each");
    assert!(!familiar.is_new("Roguelite", &rules), "Hades was sampled: tried, so not new");
    assert!(familiar.is_new("Deckbuilding", &rules) && familiar.is_new("Puzzle", &rules));
    assert!(familiar.is_new("Hack and Slash", &rules), "a genre with a sampled half hour is still rare");

    let why = |name: &str| too_much(&named(&games, name), &rules);
    assert_eq!(why("Baba Is You"), Some("players say it's hard"));
    assert_eq!(why("Celeste"), Some("tagged as difficult"));
    assert_eq!(why("The Witness"), Some("longer than a short main story"));
    assert_eq!(why("Crusader Kings"), Some("deep systems"));
    assert_eq!(why("Frostpunk"), Some("a steep learning curve"));
    assert_eq!(why("Factorio"), Some("endless, and not known to be easy to learn"));
    assert_eq!(why("Thumper"), Some("its length isn't known"));
    for gentle in ["Slay the Spire", "Monster Train", "Unpacking", "Monument Valley", "Dorfromantik", "Florence"] {
        assert_eq!(why(gentle), None, "{gentle}");
    }

    let members = members(&games, Some(&profile), &rules);
    let mut names: Vec<&str> = members.keys().map(|id| games.iter().find(|g| g.id == *id).unwrap().name.as_str()).collect();
    names.sort();
    assert_eq!(
        names,
        ["Dorfromantik", "Florence", "Monster Train", "Monument Valley", "Slay the Spire", "Unpacking"],
        "no played game, nothing familiar (Halo 2), nothing too much, no hard no (Inscryption)"
    );
    let slay = &members[&id_of(&games, "Slay the Spire")];
    assert_eq!(slay.ways, ["deckbuilders", "roguelikes", "card games", "strategy games"], "mechanics first; store genres count");
    assert_eq!(
        slay.note,
        "new to you: deckbuilders (a mechanic you haven't tried), roguelikes (a mechanic you haven't tried), \
         card games (a genre you rarely play), strategy games (a genre you rarely play)"
    );
    assert_eq!(slay.reason, "New to you: deckbuilders, roguelikes, card games, strategy games");
    assert_eq!(members[&id_of(&games, "Monster Train")].ways, ["deckbuilders", "strategy games"], "roguelites were tried");
}

#[test]
fn a_dislike_or_hard_no_is_never_a_way_in() {
    let (_dir, db, _cfg) = library(LIBRARY);
    hard_no(&db, "tag:Horror");
    db.taste_state(
        PlayerId::FIRST,
        &NewStatement { kind: "dislike".into(), title_id: None, about: Some("tag:Card Game".into()), why: None, note: None },
    )
    .unwrap();
    let games = db.all_games(PlayerId::FIRST).unwrap();
    let profile = profile(&db, &games);
    let rules = Rules::default();
    let ways = |name: &str| -> Vec<String> {
        ways_for(&games, id_of(&games, name), Some(&profile), &rules).iter().map(|w| w.tag.clone()).collect()
    };
    assert_eq!(ways("Slay the Spire"), ["Deckbuilding", "Roguelike", "Strategy"], "card games are disliked");
    assert_eq!(ways("Inscryption"), ["Deckbuilding", "Puzzle"], "horror's a hard no, so never offered");
    assert_eq!(ways("Halo 2"), Vec::<String>::new());
    let w = &ways_for(&games, id_of(&games, "Unpacking"), Some(&profile), &rules)[0];
    assert_eq!((w.about(), w.phrase, w.kind), ("tag:Puzzle".to_string(), "puzzle games", WayKind::Genre));
    // Played in two games, a genre is familiar, however few the hours; the
    // game itself doesn't count.
    let mut played = games.clone();
    for g in played.iter_mut().filter(|g| ["Baba Is You", "The Witness"].contains(&g.name.as_str())) {
        g.status = Status::Played;
        g.playtime_minutes = 120;
    }
    let puzzle = |name: &str| ways_for(&played, id_of(&played, name), Some(&profile), &rules).iter().any(|w| w.tag == "Puzzle");
    assert!(!puzzle("Unpacking"), "two puzzle games played");
    assert!(puzzle("Baba Is You"), "only one other puzzle game played");
}

/// A pick line-up from New to you, without the AI: three games, no way in twice.
#[tokio::test]
async fn new_to_me_without_ai_spans_three_ways_in() {
    let (_dir, db, cfg) = library(LIBRARY);
    hard_no(&db, "tag:Horror");
    let req = PickRequest {
        prompt: "New to me".into(),
        count: 3,
        exclude_ids: vec![],
        sources: None,
        avoid: vec![],
        without_ai: false,
        mode: None,
        routine_id: None,
        input: None,
    };
    let batch = pick(&db, &cfg, &NoAi, None, req).await.unwrap();
    assert_eq!(batch.mode, Some(Mode::NewToYou));
    let games = db.all_games(PlayerId::FIRST).unwrap();
    let lens = Lens::build(&db, &cfg, Mode::NewToYou, &games, Some(&profile(&db, &games))).unwrap();
    assert_eq!(batch.picks.len(), 3);
    let mut ways: Vec<&Vec<String>> = vec![];
    for p in batch.picks.iter() {
        let m = lens.member(p.id).unwrap_or_else(|| panic!("{} isn't new to you", p.name));
        ways.push(&m.ways);
        assert!(p.reason.starts_with("New to you: "), "{}", p.reason);
    }
    // Each pick stands for a different genre or mechanic.
    let spans = ways[0].iter().any(|a| ways[1].iter().any(|b| b != a && ways[2].iter().any(|c| c != a && c != b)));
    assert!(spans, "{ways:?}");
    assert!(batch.spares.iter().all(|s| lens.allows(s.id)));
    let ranked_by = batch.ranked_by.as_ref().unwrap();
    assert!(ranked_by[0].starts_with("games new to you only"), "{ranked_by:?}");

    // Never a familiar game or a hard no, as a pick or a spare.
    let names: Vec<&str> = batch.picks.iter().chain(&batch.spares).map(|p| p.name.as_str()).collect();
    assert!(!names.contains(&"Halo 2") && !names.contains(&"Inscryption"), "{names:?}");
}

/// Every roll without AI keeps the spread, whatever its little chance:
/// Unpacking and Monument Valley are only ever a way into puzzle games, so
/// never both picks. The one turned away is a spare, and stands for nothing
/// until it's a pick, so Dorfromantik and Florence still fit.
#[test]
fn every_roll_without_ai_keeps_the_spread() {
    let four = ["Unpacking", "Monument Valley", "Florence", "Dorfromantik", "Inscryption"];
    let specs: Vec<Spec> = LIBRARY.iter().filter(|s| s.minutes > 0 || four.contains(&s.name)).copied().collect();
    let (_dir, db, cfg) = library(&specs);
    hard_no(&db, "tag:Horror");
    let games = db.all_games_shared(PlayerId::FIRST).unwrap();
    let profile = profile(&db, &games);
    let lens = Lens::build(&db, &cfg, Mode::NewToYou, &games, Some(&profile)).unwrap();
    let ways = lens.ways();
    assert_eq!(ways[&id_of(&games, "Unpacking")], ["puzzle games"]);
    assert_eq!(ways[&id_of(&games, "Monument Valley")], ["puzzle games"]);
    let series = std::collections::HashMap::new();
    let mut puzzle_spares = 0;
    for seed in 0..200 {
        let input = crate::prompt::PromptInput {
            games: &games,
            request: "",
            count: 3,
            spares: 6,
            exclude_ids: &[],
            avoid: &[],
            on_screen: &[],
            series: &series,
            now: crate::db::now(),
            sources: None,
            recent_ids: &[],
            max_candidates: 200,
            sampled_minutes: 120,
            linux: false,
            seed,
            review_feel: true,
            verdicts: false,
            profile: Some(&profile),
            lens: Some(&lens),
            wishlist: None,
            routine: None,
            asked_input: None,
        };
        let screen = crate::recommend::Screen::new(&series, 1).spread(Some(&ways));
        let ranked = crate::ranking::lineup(&input, &[], screen);
        let picked: Vec<&Vec<String>> = ranked.picks.iter().map(|p| &ways[&p.id]).collect();
        assert_eq!(picked.len(), 3, "seed {seed}");
        let spans = picked[0].iter().any(|a| picked[1].iter().any(|b| b != a && picked[2].iter().any(|c| c != a && c != b)));
        assert!(spans, "seed {seed}: {:?}", ranked.picks.iter().map(|p| (&p.name, &ways[&p.id])).collect::<Vec<_>>());
        puzzle_spares += ranked.spares.iter().filter(|p| ways[&p.id] == ["puzzle games"]).count();
    }
    assert!(puzzle_spares > 0, "the puzzle game turned away is kept as a spare");
}

/// Too few ways in for the lineup: no lineup, rather than one that repeats.
#[tokio::test]
async fn too_few_ways_in_say_so_rather_than_repeat_one() {
    let few: Vec<Spec> =
        LIBRARY.iter().filter(|s| s.minutes > 0 || ["Unpacking", "Monument Valley", "Florence"].contains(&s.name)).copied().collect();
    let (_dir, db, cfg) = library(&few);
    for _ in 0..20 {
        let req = PickRequest {
            prompt: "new to me".into(),
            count: 3,
            exclude_ids: vec![],
            sources: None,
            avoid: vec![],
            without_ai: true,
            mode: None,
            routine_id: None,
            input: None,
        };
        let err = pick(&db, &cfg, &NoAi, None, req).await.unwrap_err();
        assert!(
            err.to_string()
                .contains("only 2 of the eligible games can share a lineup (one game per series, each a way into something different)"),
            "{err}"
        );
    }
    let req = PickRequest {
        prompt: "new to me".into(),
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
    let names: Vec<&str> = batch.picks.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"Florence") && names.len() == 2, "{names:?}");
}

/// An answer naming its picks by name.
fn answer(picks: &'static [&'static str]) -> Answer {
    Box::new(move |req: &crate::llm::LlmRequest| {
        let prompt = req.full_prompt();
        let item = |name: &&str| json!({"id": id_in(&prompt, name), "name": name, "reason": format!("{name} is a gentle start.")});
        Ok(json!({"picks": picks.iter().map(item).collect::<Vec<_>>(), "spares": []}))
    })
}

/// With the AI: only New to you's games are listed, each with what it's a
/// way into, and two picks that are only a way into one thing are asked
/// again. Slay the Spire and Monster Train are both deckbuilders, but each
/// is a way into something else too, so they may share a lineup.
#[tokio::test]
async fn new_to_me_with_ai_asks_again_for_two_of_one_way_in() {
    let (_dir, db, cfg) = library(LIBRARY);
    hard_no(&db, "tag:Horror");
    let llm = ScriptedLlm::new(vec![
        answer(&["Slay the Spire", "Unpacking", "Monument Valley"]),
        answer(&["Slay the Spire", "Monster Train", "Unpacking"]),
    ]);
    let req = PickRequest {
        prompt: "new to me, something cozy".into(),
        count: 3,
        exclude_ids: vec![],
        sources: None,
        avoid: vec![],
        without_ai: false,
        mode: None,
        routine_id: None,
        input: None,
    };
    let batch = pick(&db, &cfg, &llm, None, req).await.unwrap();
    assert_eq!(batch.attempts, Some(2));
    assert_eq!(batch.picks.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Slay the Spire", "Monster Train", "Unpacking"]);
    assert_eq!(batch.mode, Some(Mode::NewToYou));

    let asked = llm.asked.lock().unwrap();
    let first = asked[0].full_prompt();
    let line =
        |name: &str| first.lines().find(|l| l.contains(&format!(" {name} |")) || l.ends_with(&format!(" {name}"))).map(str::to_string);
    assert!(line("Slay the Spire").unwrap().contains("| new to you: deckbuilders (a mechanic you haven't tried)"), "{first}");
    for out in ["Doom", "Halo 2", "Baba Is You", "Inscryption", "The Witness"] {
        assert!(line(out).is_none(), "{out} is listed:\n{first}");
    }
    assert!(first.contains("New to me"), "{first}");
    assert!(first.contains("something cozy"), "{first}");
    assert!(asked[0].system.to_lowercase().contains("new to"), "{}", asked[0].system);
    assert!(asked[1].prompt.contains("(Monument Valley) is a way into puzzle games, which another pick already is"), "{}", asked[1].prompt);
}

/// An explicit mode works without the words, and too few games say why.
#[tokio::test]
async fn too_few_new_games_say_so() {
    let (_dir, db, cfg) = library(&LIBRARY[..6]);
    let req = PickRequest {
        prompt: String::new(),
        count: 3,
        exclude_ids: vec![],
        sources: None,
        avoid: vec![],
        without_ai: true,
        mode: Some(Mode::NewToYou),
        routine_id: None,
        input: None,
    };
    let err = pick(&db, &cfg, &NoAi, None, req).await.unwrap_err();
    assert!(err.to_string().contains("only 1 of your games is new to you"), "{err}");
}
