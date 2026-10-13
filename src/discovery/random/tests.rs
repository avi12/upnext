//! Rolling the dice on the pick tests' library: what's off your
//! radar, that every such game can come up, the lineup's rules, and that
//! no AI is asked even with one set up.

use super::*;
use crate::db::FeedbackAction;
use crate::db::PlayerId;
use crate::discovery::{Lens, Mode};
use crate::lineup::{Feedback, feedback};
use crate::recommend::tests::{id_of, library, names};
use crate::recommend::{PickRequest, more, pick};
use crate::testutil::ScriptedLlm;
use std::collections::BTreeSet;

/// Every game in the library that's off the radar: the seven not started,
/// and the two abandoned years ago. Hollow Knight was shelved only eight
/// months ago, Rise of Nations is a staple, Portal 2 is likely finished and
/// Doom is being played.
const OFF_RADAR: [&str; 9] =
    ["Celeste", "Dishonored", "Hades", "Inside", "Limbo", "Outer Wilds", "Prey", "Rise of the Tomb Raider", "Tomb Raider"];

fn roll(count: usize, exclude_ids: Vec<i64>) -> PickRequest {
    PickRequest {
        prompt: String::new(),
        count,
        exclude_ids,
        sources: None,
        avoid: vec![],
        without_ai: false,
        mode: Some(Mode::Random),
        routine_id: None,
        input: None,
    }
}

#[test]
fn off_the_radar_is_not_started_or_put_down_a_year_ago() {
    let (_dir, db, _cfg) = library();
    let games = db.all_games_shared(PlayerId::FIRST).unwrap();
    let now = crate::db::now();
    let off: BTreeSet<&str> = games.iter().filter(|g| off_radar(g, now).is_some()).map(|g| g.name.as_str()).collect();
    assert_eq!(off, OFF_RADAR.into_iter().collect());
    let reason = |name: &str| off_radar(games.iter().find(|g| g.name == name).unwrap(), now).unwrap();
    assert_eq!(reason("Celeste"), "Drawn at random from the games you haven't started.");
    // A game only tried is off the radar once it's been left a year.
    let mut tried = games.iter().find(|g| g.name == "Celeste").unwrap().clone();
    (tried.status, tried.playtime_known, tried.playtime_minutes, tried.last_played) = (Status::Sampled, true, 25, now - 800 * 86_400);
    assert_eq!(off_radar(&tried, now).unwrap(), "Drawn at random: you tried it for 25m 2 years ago and haven't been back.");
    tried.last_played = now - 30 * 86_400;
    assert_eq!(off_radar(&tried, now), None, "tried last month: still on the radar");
    let dishonored = reason("Dishonored");
    assert!(dishonored.starts_with("Drawn at random: you put it down ") && dishonored.ends_with(" years ago."), "{dishonored}");
}

/// Every game off the radar can be drawn, the lineup's rules hold for
/// every draw, and games ProtonDB says don't run stay out on Linux.
#[test]
fn every_game_off_the_radar_can_come_up() {
    let (_dir, db, cfg) = library();
    let mut games = db.all_games(PlayerId::FIRST).unwrap();
    games.iter_mut().find(|g| g.name == "Limbo").unwrap().linux = Some("borked".into());
    let series = crate::series::of_titles(&games, &db.catalog().unwrap());
    let lens = Lens::build(&db, &cfg, Mode::Random, &games, None).unwrap();
    let mut first = BTreeSet::new();
    for seed in 1..=300 {
        for linux in [false, true] {
            let input = PromptInput {
                games: &games,
                request: "",
                count: 1,
                spares: 4,
                exclude_ids: &[],
                avoid: &[],
                on_screen: &[],
                series: &series,
                now: crate::db::now(),
                sources: None,
                recent_ids: &[],
                max_candidates: 120,
                sampled_minutes: 120,
                linux,
                seed,
                review_feel: true,
                verdicts: false,
                profile: None,
                lens: Some(&lens),
                wishlist: None,
                routine: None,
                asked_input: None,
            };
            let drawn = draw(&input, Screen::new(&series, input.second_chances()));
            assert_eq!((drawn.picks.len(), drawn.spares.len()), (1, 4));
            let all: Vec<&str> = names(&drawn.picks).into_iter().chain(names(&drawn.spares)).collect();
            assert!(all.iter().all(|n| OFF_RADAR.contains(n)), "{all:?}");
            assert!(all.iter().filter(|n| n.contains("Tomb Raider")).count() <= 1, "one game per series: {all:?}");
            assert!(all.iter().filter(|n| ["Dishonored", "Prey"].contains(n)).count() <= 1, "one second chance: {all:?}");
            if linux {
                assert!(!all.contains(&"Limbo"), "it doesn't run on Linux: {all:?}");
            } else {
                first.insert(drawn.picks[0].name.clone());
            }
        }
    }
    assert_eq!(first, OFF_RADAR.into_iter().map(String::from).collect(), "each game can come first");
}

/// A random lineup asks no AI, even with one set up (the scripted model has
/// no answers, so a call would panic), and leaves out the games turned
/// down and the last lineup's picks.
#[tokio::test]
async fn rolling_the_dice_draws_one_game_with_no_ai_call() {
    let (_dir, db, cfg) = library();
    let llm = ScriptedLlm::new(vec![]);
    let turned_down: Vec<i64> = ["Celeste", "Hades", "Inside"].iter().map(|n| id_of(&db, n)).collect();
    let batch = pick(&db, &cfg, &llm, None, roll(1, turned_down.clone())).await.unwrap();
    assert_eq!(batch.mode, Some(Mode::Random));
    assert_eq!((batch.picks.len(), batch.shown, batch.spares.len()), (1, 1, 2));
    assert!(batch.ranked_by.as_ref().unwrap()[0].starts_with("chance alone: drawn at random from 6 games off your radar"));
    assert_eq!(batch.attempts, Some(0));
    let first = batch.picks[0].clone();
    assert!(OFF_RADAR.contains(&first.name.as_str()) && !turned_down.contains(&first.id), "{}", first.name);
    assert!(first.reason.starts_with("Drawn at random"), "{}", first.reason);
    assert!(db.pick_batch(PlayerId::FIRST, &batch.batch_id).unwrap().is_some(), "saved, for History and a reload");
    // Another: the pick stays out while its lineup is one of the last three
    // (`recent_pick_ids`), which in the same second are the newest rows.
    for _ in 0..2 {
        let next = pick(&db, &cfg, &llm, None, roll(1, vec![])).await.unwrap();
        assert_ne!(next.picks[0].id, first.id);
    }
    assert!(llm.asked.lock().unwrap().is_empty());
}

/// When the spares run out, the lineup goes on at random, still with no AI.
#[tokio::test]
async fn a_random_lineup_gets_more_at_random() {
    let (_dir, db, cfg) = library();
    let llm = ScriptedLlm::new(vec![]);
    let batch = pick(&db, &cfg, &llm, None, roll(1, vec![])).await.unwrap();
    let mut held = vec![batch.picks[0].id];
    let mut now = batch;
    while let Some(p) = now.picks.iter().find(|p| p.slot.is_some()).cloned() {
        let f = Feedback { title_id: p.id, action: FeedbackAction::NotTonight, text: None, reasons: None };
        now = feedback(&db, PlayerId::FIRST, &now.batch_id, f).unwrap();
        held.extend(now.picks.iter().map(|p| p.id));
    }
    assert!(now.spares.is_empty() && now.empty_slots() == vec![0]);
    let after = more(&db, &cfg, &llm, &now.batch_id, None, None).await.unwrap();
    let fresh = after.picks.iter().find(|p| p.slot == Some(0)).unwrap();
    assert!(OFF_RADAR.contains(&fresh.name.as_str()) && !held.contains(&fresh.id), "{}", fresh.name);
    assert!(fresh.reason.starts_with("Drawn at random"), "{}", fresh.reason);
    assert!(llm.asked.lock().unwrap().is_empty());
}

#[tokio::test]
async fn with_nothing_off_the_radar_it_says_what_counts() {
    let (_dir, db, cfg) = library();
    let all: Vec<i64> = OFF_RADAR.iter().map(|n| id_of(&db, n)).collect();
    let e = pick(&db, &cfg, &ScriptedLlm::new(vec![]), None, roll(1, all)).await.unwrap_err();
    assert_eq!(
        e.to_string(),
        "no candidates: none of your games is off your radar yet (games you haven't started, or last played over a year ago, that you \
         haven't turned down)"
    );
}
