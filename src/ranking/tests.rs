//! Picks without AI: the lineup, its reasons and its warning come from the
//! ranking, and the card's buttons and new spares make no AI call.

use crate::db::PlayerId;
use crate::db::{Db, FeedbackAction, MissReason, PickBatch, PickVerdict, ReviewRow};
use crate::lineup::{self, Feedback};
use crate::recommend::tests::{id_of, library, names, request};
use crate::recommend::{PickRequest, more, pick};
use crate::review_tags::ReviewTags;
use crate::testutil::{NoAi, ScriptedLlm};

fn without_ai(prompt: &str, count: usize) -> PickRequest {
    PickRequest { count, without_ai: true, ..request(prompt) }
}

/// Every game the lineup shows or keeps as a spare, by name.
fn shown(b: &PickBatch) -> Vec<&str> {
    names(&b.picks).into_iter().chain(names(&b.spares)).collect()
}

fn not_tonight(db: &Db, b: &PickBatch) -> PickBatch {
    let f = Feedback { title_id: b.picks[0].id, action: FeedbackAction::NotTonight, text: None, reasons: None };
    lineup::feedback(db, PlayerId::FIRST, &b.batch_id, f).unwrap()
}

#[tokio::test]
async fn with_no_ai_set_up_a_pick_comes_from_the_ranking() {
    let (_dir, db, cfg) = library();
    let batch = pick(&db, &cfg, &NoAi, None, request("")).await.unwrap();
    assert_eq!((batch.model.as_deref(), batch.attempts, batch.usage), (None, Some(0), None));
    assert_eq!((batch.shown, batch.picks.len(), batch.spares.len()), (3, 3, 6), "twice as many spares, as the AI's");
    assert_eq!(batch.picks.iter().map(|p| p.slot).collect::<Vec<_>>(), [Some(0), Some(1), Some(2)]);
    // Eleven games are offered; one per series and one second chance leave nine.
    let all = shown(&batch);
    assert_eq!(all.iter().filter(|n| n.contains("Tomb Raider")).count(), 1, "{all:?}");
    assert_eq!(all.iter().filter(|n| ["Dishonored", "Prey"].contains(n)).count(), 1, "{all:?}");
    assert_eq!(all.len(), 9);
    assert!(batch.picks.iter().all(|p| !p.reason.is_empty()));

    let saved = db.pick_batch(PlayerId::FIRST, &batch.batch_id).unwrap().unwrap();
    assert_eq!(saved.ranked_by, batch.ranked_by, "kept with the lineup, for History and a reload");
    assert!(db.ai_usage_rows(0).unwrap().is_empty(), "no AI call was made");
}

#[tokio::test]
async fn pick_without_ai_makes_no_call_with_an_ai_set_up() {
    let (_dir, db, cfg) = library();
    // An empty script: any call would panic.
    let llm = ScriptedLlm::new(vec![]);
    let batch = pick(&db, &cfg, &llm, None, without_ai("", 3)).await.unwrap();
    assert!(batch.ranked_by.is_some());
    assert!(llm.asked.lock().unwrap().is_empty());
}

#[tokio::test]
async fn the_warning_lists_only_what_counted_and_reasons_come_from_the_data() {
    let (_dir, db, cfg) = library();
    // No reviews, no budget, no words matched, nothing installed or shown before.
    let first = pick(&db, &cfg, &NoAi, None, request("")).await.unwrap();
    assert_eq!(
        first.ranked_by.unwrap(),
        [
            "staples and endless games you've played, kept lower since you didn't ask for something familiar",
            "a little chance, so each lineup differs"
        ]
    );

    let reviews = ReviewRow {
        review_score: Some(9),
        review_score_desc: Some("Very Positive".into()),
        total_positive: Some(77_600),
        total_negative: Some(2_400),
        total_reviews: Some(80_000),
    };
    db.put_reviews(504230, Some(&reviews)).unwrap();
    let relaxed = ReviewTags { scales: [("pace".to_string(), "relaxed".to_string())].into(), ..Default::default() };
    db.put_digest(id_of(&db, "Outer Wilds"), "Praised for its mystery.", "", &relaxed, "test", 4).unwrap();

    let second = pick(&db, &cfg, &NoAi, None, request("2 hours, something relaxing")).await.unwrap();
    assert_eq!(
        second.ranked_by.clone().unwrap(),
        [
            "Steam review scores, and how many reviews",
            "HowLongToBeat's main-story times, for 2 hours",
            "your words against tags, genres and review tags, then names, blurbs and review summaries",
            "staples and endless games you've played, kept lower since you didn't ask for something familiar",
            "games from your last few lineups, kept lower",
            "a little chance, so each lineup differs",
        ]
    );
    let reason = |name: &str| {
        second
            .picks
            .iter()
            .chain(&second.spares)
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("{name} isn't in the lineup"))
            .reason
            .clone()
    };
    assert_eq!(reason("Celeste"), "97% of 80k Steam reviews positive");
    assert_eq!(reason("Inside"), "3.5h main story, about 2 sittings of 2 hours");
    assert_eq!(reason("Outer Wilds"), "17h main story, long for 2 hours · relaxed pace, as asked");
    assert_eq!(reason("Hades"), "22h main story, long for 2 hours");
    assert_eq!(second.picks[0].name, "Celeste", "the reviews put it first");
}

#[tokio::test]
async fn a_second_chance_says_why_it_stopped() {
    let (_dir, db, cfg) = library();
    let prey = id_of(&db, "Prey");
    let batch = pick(&db, &cfg, &NoAi, None, PickRequest { exclude_ids: vec![prey], ..request("") }).await.unwrap();
    let dishonored = batch.picks.iter().chain(&batch.spares).find(|p| p.name == "Dishonored").expect("the only second chance left");
    assert_eq!(dishonored.reason, "You stopped: bugs or performance");
}

#[tokio::test]
async fn a_reroll_steers_away_from_what_the_lineup_was_turned_down_for() {
    let (_dir, db, cfg) = library();
    let hades = id_of(&db, "Hades");
    let req =
        PickRequest { exclude_ids: vec![hades], avoid: vec!["Too long".into(), "I just finished a long RPG".into()], ..without_ai("", 1) };
    let batch = pick(&db, &cfg, &NoAi, None, req).await.unwrap();
    // Hollow Knight (27h) is no shorter than Hades (22h): held back below
    // the seven untouched games and second chance left.
    assert!(!shown(&batch).contains(&"Hollow Knight"), "{:?}", shown(&batch));
    assert!(!shown(&batch).contains(&"Hades"), "turned down");
    let by = batch.ranked_by.unwrap();
    assert!(by.contains(&"what you turned games down for: \"too long\"".to_string()), "{by:?}");
    assert_eq!(by.last().unwrap(), "not \"I just finished a long RPG\": only the AI reads reasons in your own words");
}

#[tokio::test]
async fn new_spares_for_a_lineup_picked_without_ai_come_from_the_ranking() {
    let (_dir, db, cfg) = library();
    let batch = pick(&db, &cfg, &NoAi, None, without_ai("", 1)).await.unwrap();
    assert_eq!((batch.picks.len(), batch.spares.len()), (1, 2));
    let held = batch.all_ids();
    let mut b = batch.clone();
    for _ in 0..3 {
        b = not_tonight(&db, &b);
    }
    assert_eq!(b.empty_slots(), [0], "the pick and both spares were turned down");

    // A lineup picked without AI goes on without it, even with an AI set up.
    let llm = ScriptedLlm::new(vec![]);
    let after = more(&db, &cfg, &llm, &batch.batch_id, None, None).await.unwrap();
    assert!(llm.asked.lock().unwrap().is_empty());
    assert_eq!(after.picks.len(), 1);
    assert_eq!(after.picks[0].slot, Some(0));
    assert_eq!(after.spares.len(), 2);
    assert!(after.picks.iter().chain(&after.spares).all(|p| !held.contains(&p.id)), "nothing the lineup held comes back");
    assert_eq!(after.ranked_by, batch.ranked_by);
}

#[tokio::test]
async fn its_too_without_ai_takes_the_next_spare_the_word_allows() {
    let (_dir, db, cfg) = library();
    let batch = pick(&db, &cfg, &NoAi, None, without_ai("", 3)).await.unwrap();
    let hours = |id: i64| db.game(PlayerId::FIRST, id).unwrap().unwrap().hltb_main.unwrap();
    let target = batch.picks.iter().max_by(|a, b| hours(a.id).total_cmp(&hours(b.id))).unwrap();
    let llm = ScriptedLlm::new(vec![]);
    let out = lineup::too(&db, &cfg, &llm, &batch.batch_id, target.id, "long").await.unwrap();
    assert!(llm.asked.lock().unwrap().is_empty(), "no call for a lineup picked without AI");
    assert_eq!(out.ai_error, None, "nothing failed");
    assert_eq!(out.swaps.len(), 1, "only its own slot");
    if let Some(into) = out.swaps[0].into {
        assert!(hours(into) < hours(target.id), "a shorter spare");
    }

    // A lineup the AI picked, with the AI gone since: no call, no failure.
    let llm = ScriptedLlm::new(vec![crate::recommend::tests::answer(&["Celeste", "Hades", "Inside"], &["Limbo", "Outer Wilds"])]);
    let ai = pick(&db, &cfg, &llm, None, request("")).await.unwrap();
    let hades = id_of(&db, "Hades");
    let out = lineup::too(&db, &cfg, &NoAi, &ai.batch_id, hades, "long").await.unwrap();
    assert_eq!((out.ai_error, out.swaps[0].into), (None, Some(id_of(&db, "Limbo"))));
}

/// The mock library, with its profile: favourites Civilization VI, Hades
/// and Firewatch, likes story-driven and puzzle games, a dislike of heavy
/// grind, a hard no on horror, and the usual sessions.
fn mock() -> (tempfile::TempDir, Db, crate::config::Config) {
    let dir = tempfile::tempdir().unwrap();
    let cfg = crate::testutil::offline_config(dir.path());
    let db = Db::open(&cfg.db_path, cfg.sampled_minutes).unwrap();
    crate::mock::seed(&db).unwrap();
    (dir, db, cfg)
}

#[tokio::test]
async fn picks_without_ai_read_the_profile() {
    let (_dir, db, cfg) = mock();
    let games = db.all_games(PlayerId::FIRST).unwrap();
    let profile = crate::taste::Profile::load(&db, PlayerId::FIRST, &games).unwrap();
    let horror: Vec<&str> = games.iter().filter(|g| profile.hard_no(g).is_some()).map(|g| g.name.as_str()).collect();
    assert!(horror.contains(&"Resident Evil 2") && horror.contains(&"Phasmophobia"), "{horror:?}");
    for _ in 0..5 {
        let batch = pick(&db, &cfg, &NoAi, None, request("")).await.unwrap();
        assert!(shown(&batch).iter().all(|n| !horror.contains(n)), "a hard no came up: {:?}", shown(&batch));
        let by = batch.ranked_by.unwrap();
        for line in ["your hard nos, left out", "what you like, on your profile", "likeness to your favourites"] {
            assert!(by.iter().any(|l| l == line), "{line:?} in {by:?}");
        }
        assert!(by.iter().any(|l| l.starts_with("HowLongToBeat's main-story times, for your usual")), "{by:?}");
    }
    // Asked for by name, a hard no is still never offered.
    let batch = pick(&db, &cfg, &NoAi, None, request("something scary, horror")).await.unwrap();
    assert!(shown(&batch).iter().all(|n| !horror.contains(n)), "{:?}", shown(&batch));
}

#[tokio::test]
async fn the_ai_gets_about_me_and_never_a_hard_no() {
    let (_dir, db, cfg) = mock();
    let llm = ScriptedLlm::new(vec![crate::recommend::tests::answer(&["Slay the Spire", "Baba Is You", "A Short Hike"], &[])]);
    pick(&db, &cfg, &llm, None, request("anything")).await.unwrap();
    let asked = llm.asked.lock().unwrap();
    let prefix = asked[0].cached_prefix.as_deref().unwrap();
    assert!(prefix.starts_with("## About me\nFavourites, and why I love them:\n"), "{prefix}");
    assert!(prefix.contains("- Hades: the combat, high replay value, \"one more run, every time\""));
    assert!(prefix.contains("Hard nos, never pick these: horror games"));
    assert!(!prefix.contains("Resident Evil 2") && !prefix.contains("Amnesia"), "hard nos aren't candidates");
    assert!(asked[0].prompt.contains("anything\n(A vague request: lean on About me, with one change of pace.)"));
    assert!(asked[0].system.contains("make the last pick a change of pace instead"));
}

/// A vague request's lineup has one change of pace, a game outside your
/// favourites, likes and the patterns you stick with (decided 2026-10-02,
/// after the live A/B); a request that says what it wants doesn't.
#[tokio::test]
async fn a_vague_lineup_has_a_change_of_pace() {
    let (_dir, db, cfg) = mock();
    let games = db.all_games(PlayerId::FIRST).unwrap();
    let profile = crate::taste::Profile::load(&db, PlayerId::FIRST, &games).unwrap();
    let lane = |name: &str| profile.in_your_lane(games.iter().find(|g| g.name == name).unwrap());
    for ask in ["", "anything", "surprise me"] {
        for _ in 0..3 {
            let batch = pick(&db, &cfg, &NoAi, None, request(ask)).await.unwrap();
            let outside: Vec<&str> = names(&batch.picks).into_iter().filter(|n| !lane(n)).collect();
            assert!(!outside.is_empty(), "{ask:?}: {:?}", names(&batch.picks));
            assert!(batch.ranked_by.unwrap().iter().any(|l| l.starts_with("one change of pace")), "{ask:?}");
            assert_eq!(batch.picks.len(), 3);
        }
    }
    let batch = pick(&db, &cfg, &NoAi, None, request("a puzzle game")).await.unwrap();
    assert!(!batch.ranked_by.unwrap().iter().any(|l| l.starts_with("one change of pace")));
    assert!(batch.picks.iter().all(|p| !p.reason.starts_with("A change of pace")));
    // Two picks are too few to give one up.
    let batch = pick(&db, &cfg, &NoAi, None, PickRequest { count: 2, ..request("anything") }).await.unwrap();
    assert!(!batch.ranked_by.unwrap().iter().any(|l| l.starts_with("one change of pace")));
}

/// Your ratings in a lineup picked without AI: a game you rated
/// says so in its reason, and "ranked by" names the ratings that counted;
/// a pick you said wasn't a fit is held back, and the AI is told how
/// recent picks landed.
#[tokio::test]
async fn ratings_reach_picks_without_ai_and_the_ai() {
    let (_dir, db, cfg) = library();
    let outer = id_of(&db, "Outer Wilds");
    db.set_game_rating(PlayerId::FIRST, outer, Some(5)).unwrap();
    let batch = pick(&db, &cfg, &NoAi, None, request("")).await.unwrap();
    let rated = batch.picks.iter().chain(&batch.spares).find(|p| p.id == outer).expect("a 5 is offered");
    assert!(rated.reason.to_lowercase().contains("you rated it 5/5"), "{}", rated.reason);
    assert!(batch.ranked_by.as_ref().unwrap().iter().any(|l| l == "your own ratings of games"), "{:?}", batch.ranked_by);

    let first = &batch.picks[0];
    assert!(db.set_pick_rating(&batch.batch_id, first.id, Some((PickVerdict::NotAFit, Some(MissReason::WrongLength)))).unwrap());
    let next = pick(&db, &cfg, &NoAi, None, request("")).await.unwrap();
    let by = next.ranked_by.unwrap();
    assert!(by.iter().any(|l| l == "picks you said weren't a fit in the last 90 days, kept a little lower"), "{by:?}");

    let llm = ScriptedLlm::new(vec![crate::recommend::tests::answer(&["Inside", "Limbo", "Hades"], &[])]);
    pick(&db, &cfg, &llm, None, request("anything")).await.unwrap();
    let asked = llm.asked.lock().unwrap();
    assert!(asked[0].prompt.contains("## How recent picks landed\n"), "{}", asked[0].prompt);
    let line = format!("- {}, for no particular request: not a fit (wrong length)\n", first.name);
    assert!(asked[0].prompt.contains(&line), "{}", asked[0].prompt);
    assert!(asked[0].cached_prefix.as_deref().unwrap().contains("rated 5/5 by me"));
}

/// A routine in a lineup: picked without AI, the lineup leans to
/// games for playing together and says why; the lineup keeps the routine,
/// and its new spares keep to it after the routine is deleted; the AI gets
/// "This session".
#[tokio::test]
async fn a_routine_steers_the_lineup_and_stays_with_it() {
    use crate::taste::routine::Marks;
    let (_dir, db, cfg) = mock();
    let saturday = db.routines(PlayerId::FIRST).unwrap().into_iter().find(|r| r.name == "Saturday co-op").unwrap();
    let with = |prompt: &str, count: usize| PickRequest { count, routine_id: Some(saturday.id), ..without_ai(prompt, count) };
    // Whether each pick can be played together, as far as its tags and store categories say.
    let together = |b: &PickBatch| -> Vec<Option<bool>> {
        b.picks.iter().map(|p| p.game.as_ref().and_then(|g| Marks::of(g).plays_together())).collect()
    };
    for _ in 0..5 {
        let batch = pick(&db, &cfg, &NoAi, None, with("a strategy game", 3)).await.unwrap();
        assert_eq!(batch.routine.as_ref(), Some(&saturday));
        // Games that say nothing aren't held back, so it's a lean, not a filter: no
        // single-player-only game makes it, and the co-op ones say why they're there.
        let t = together(&batch);
        assert!(!t.contains(&Some(false)) && t.contains(&Some(true)), "{:?}: {t:?}", names(&batch.picks));
        for (p, t) in batch.picks.iter().zip(&t) {
            assert_eq!(p.reason.contains("co-op or multiplayer, for playing with friends"), *t == Some(true), "{}", p.reason);
        }
        let by = batch.ranked_by.unwrap();
        assert!(by.iter().any(|l| l == "co-op and multiplayer games first, as your routine \"Saturday co-op\" is with friends"), "{by:?}");
        assert!(by.iter().any(|l| l == "HowLongToBeat's main-story times, for 3 hours, your routine \"Saturday co-op\""), "{by:?}");
    }
    let plain = pick(&db, &cfg, &NoAi, None, without_ai("a strategy game", 3)).await.unwrap();
    assert_eq!(plain.routine, None);
    assert!(!plain.ranked_by.unwrap().iter().any(|l| l.contains("routine")));

    // The AI's prompt says the session.
    let llm = ScriptedLlm::new(vec![crate::recommend::tests::answer(&["Stardew Valley", "It Takes Two", "Valheim"], &[])]);
    pick(&db, &cfg, &llm, None, PickRequest { routine_id: Some(saturday.id), ..request("anything") }).await.unwrap();
    let asked = llm.asked.lock().unwrap().clone();
    assert!(
        asked[0].prompt.contains(
            "## This session\nMy routine \"Saturday co-op\" (Saturday afternoons and evenings): with friends, at a desk, \
             on keyboard and mouse, about 3 hours.\n"
        ),
        "{}",
        asked[0].prompt
    );
    assert!(asked[0].prompt.contains("- Also: \"Voice chat on, two or three of us\"\n"));

    // New spares keep to the routine the lineup was picked for, after it's gone.
    let batch = pick(&db, &cfg, &NoAi, None, with("", 1)).await.unwrap();
    let mut b = batch.clone();
    for _ in 0..3 {
        b = not_tonight(&db, &b);
    }
    assert!(db.delete_routine(PlayerId::FIRST, saturday.id).unwrap());
    let after = more(&db, &cfg, &NoAi, &batch.batch_id, None, None).await.unwrap();
    assert_eq!(after.routine.as_ref(), Some(&saturday));
    assert_ne!(together(&after), [Some(false)], "{:?}", names(&after.picks));

    // A routine that isn't there.
    let gone = pick(&db, &cfg, &NoAi, None, with("", 3)).await.unwrap_err();
    assert!(matches!(gone, crate::recommend::PickError::NotFound(_)), "{gone}");
}

/// A budget just over an hour, or a minute, reads in the singular.
#[test]
fn a_budget_of_one_reads_in_the_singular() {
    assert_eq!(super::budget_text(1.04), "an hour");
    assert_eq!(super::budget_text(1.0 / 60.0), "1 minute");
    assert_eq!(super::budget_text(0.5), "30 minutes");
    assert_eq!(super::budget_text(2.5), "2.5 hours");
}
