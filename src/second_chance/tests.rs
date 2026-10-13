//! Second Chance against the library from `recommend::tests`: Dishonored
//! (abandoned in 2022 after 6 of 12 hours, over bugs) and Prey (abandoned
//! in 2021, no reason given).

use super::*;
use crate::db::Override;
use crate::db::PlayerId;
use crate::lifecycle::Offer;
use crate::llm::LlmRequest;
use crate::recommend::tests::{id_of, library};
use crate::testutil::ScriptedLlm;

/// 2020, before either was put down, and 2025, after.
const BEFORE_T: i64 = 1_600_000_000;
const SINCE_T: i64 = 1_740_000_000;

fn review(text: &str, posted: i64, positive: bool) -> ReviewText {
    ReviewText {
        source: "steam".into(),
        positive: Some(positive),
        rating: None,
        votes: 3,
        hours: Some(20),
        title: None,
        text: text.into(),
        posted: Some(posted),
    }
}

fn with_reviews(db: &Db) {
    db.put_review_texts(
        205100,
        Some(&[
            review("Crashes every hour on my machine and the saves corrupt; unplayable for now.", BEFORE_T, false),
            review("Since the latest patch it runs smoothly and I haven't had a single crash in 30 hours.", SINCE_T, true),
            review("great game", SINCE_T, true),
        ]),
    )
    .unwrap();
    db.put_review_texts(480490, Some(&[review("A slow start, then one of the best space stations to get lost in.", BEFORE_T, true)]))
        .unwrap();
}

fn game(db: &Db, name: &str) -> GameView {
    db.game(PlayerId::FIRST, id_of(db, name)).unwrap().unwrap()
}

/// The id a batch lists a game under ("### #12 Dishonored").
fn id_in(prompt: &str, name: &str) -> i64 {
    let re = regex::Regex::new(&format!(r"(?m)^### #(\d+) {}$", regex::escape(name))).unwrap();
    re.captures(prompt).unwrap_or_else(|| panic!("{name} isn't in the prompt:\n{prompt}"))[1].parse().unwrap()
}

/// A reply giving each named game this verdict.
fn reply(games: &'static [(&'static str, &'static str)]) -> crate::testutil::Answer {
    Box::new(move |req: &LlmRequest| {
        let items: Vec<Value> = games
            .iter()
            .map(|(name, verdict)| {
                json!({"id": id_in(&req.prompt, name), "now": format!("What reviews say about {name}."), "verdict": verdict,
                    "line": format!("{name}: {verdict}")})
            })
            .collect();
        Ok(json!({ "games": items }))
    })
}

#[test]
fn the_page_lists_abandoned_games_with_news_first() {
    let (_dir, db, _cfg) = library();
    with_reviews(&db);
    let all = db.all_games(PlayerId::FIRST).unwrap();
    let page = page(&all);
    let names: Vec<&str> = page.iter().map(|e| e.game.name.as_str()).collect();
    assert_eq!(names, ["Dishonored", "Prey"], "most recently put down first");
    assert_eq!((page[0].reviews_since, page[0].due, page[0].stopped_at), (1, true, Some(1_646_092_800)), "\"great game\" is too short");
    assert_eq!((page[1].reviews_since, page[1].due), (0, false), "nothing written since to check against");
    assert!(page[0].game.review_texts.is_empty(), "slim, like other lists");
}

#[tokio::test]
async fn now_is_written_from_reviews_since_and_kept_until_they_change() {
    let (_dir, db, _cfg) = library();
    with_reviews(&db);
    let pending: Vec<GameView> = db.all_games(PlayerId::FIRST).unwrap().into_iter().filter(|g| needs_now(g, false)).collect();
    let llm = ScriptedLlm::new(vec![reply(&[("Dishonored", "better")])]);
    assert_eq!(generate(&db, PlayerId::FIRST, &llm, None, &pending).await.unwrap(), Written { written: 1, missing: vec![] });
    let req = llm.asked.lock().unwrap()[0].clone();
    let p = &req.prompt;
    assert!(p.contains("### #") && p.contains(" Dishonored\nPlayed 6.0h of a 12.0h main story; stopped in March 2022.\n"), "{p}");
    assert!(p.contains("Why they stopped, about the game: Bugs or performance.\n"), "{p}");
    assert!(p.contains("  [+, 20h, 2025, since] Since the latest patch"), "{p}");
    assert!(p.contains("  [-, 20h, 2020] Crashes every hour"), "{p}");
    assert!(!p.contains("great game"), "{p}");
    assert!(req.background && req.system.contains("Use only what the reviews say"));

    let g = game(&db, "Dishonored");
    let note = g.second_chance.clone().unwrap();
    assert_eq!((note.verdict, note.outdated, note.line.as_deref()), (Some(Verdict::Better), false, Some("Dishonored: better")));
    assert!(!needs_now(&g, false) && needs_now(&g, true), "up to date, unless refreshed");
    // Picks read the gist with the game's history.
    assert_eq!(
        crate::prompt::lines::history_segment(&g, crate::db::now()).as_deref(),
        Some("second chance: abandoned 2022 after 6.0h; stopped: bugs or performance; reviews since: Dishonored: better")
    );

    // New reasons put it out of date, and picks stop quoting it.
    db.add_stop_reasons(PlayerId::FIRST, g.id, &["grind".to_string()], Some("the stealth got samey"), "second_chance").unwrap();
    let g = game(&db, "Dishonored");
    assert!(g.second_chance.as_ref().unwrap().outdated && needs_now(&g, false));
    assert_eq!(g.reviews_since_line(), None);
    // So do new reviews.
    let llm = ScriptedLlm::new(vec![reply(&[("Dishonored", "partly")])]);
    generate(&db, PlayerId::FIRST, &llm, None, &[g]).await.unwrap();
    assert!(!needs_now(&game(&db, "Dishonored"), false));
    db.put_review_texts(
        205100,
        Some(&[review("Runs fine now, but the stealth is the same few tricks over and over.", SINCE_T + 99, false)]),
    )
    .unwrap();
    assert!(needs_now(&game(&db, "Dishonored"), false));
}

#[tokio::test]
async fn reviews_saying_its_the_same_keep_a_second_chance_out_of_picks() {
    let (_dir, db, _cfg) = library();
    with_reviews(&db);
    db.put_review_texts(480490, Some(&[review("Still a slow start, then one of the best space stations around.", SINCE_T, true)])).unwrap();
    let pending: Vec<GameView> = db.all_games(PlayerId::FIRST).unwrap().into_iter().filter(|g| needs_now(g, false)).collect();
    assert_eq!(pending.len(), 2);
    let llm = ScriptedLlm::new(vec![reply(&[("Dishonored", "same"), ("Prey", "same")])]);
    generate(&db, PlayerId::FIRST, &llm, None, &pending).await.unwrap();

    // Dishonored stopped over bugs that reviews say are still there.
    let dishonored = game(&db, "Dishonored");
    assert_eq!((dishonored.second_chance.as_ref().unwrap().verdict, dishonored.offer()), (Some(Verdict::Same), None));
    // Nobody said why Prey stopped, so the reviews can't say it's the same.
    let prey = game(&db, "Prey");
    assert_eq!((prey.second_chance.as_ref().unwrap().verdict, prey.offer()), (Some(Verdict::Unclear), Some(Offer::SecondChance)));
    // Both stay on the page, the news-less one first.
    let names: Vec<String> = page(&db.all_games(PlayerId::FIRST).unwrap()).into_iter().map(|e| e.game.name).collect();
    assert_eq!(names, ["Prey", "Dishonored"]);
}

#[tokio::test]
async fn a_game_left_out_of_the_reply_is_missing() {
    let (_dir, db, _cfg) = library();
    with_reviews(&db);
    let pending: Vec<GameView> = db.all_games(PlayerId::FIRST).unwrap().into_iter().filter(|g| needs_now(g, false)).collect();
    let llm = ScriptedLlm::new(vec![Box::new(|_: &LlmRequest| Ok(json!({ "games": [{"id": 1, "now": "", "verdict": "x", "line": ""}] })))]);
    let written = generate(&db, PlayerId::FIRST, &llm, None, &pending).await.unwrap();
    assert_eq!(written, Written { written: 0, missing: vec![id_of(&db, "Dishonored")] });
    assert!(game(&db, "Dishonored").second_chance.is_none());
}

#[test]
fn answers_stick_and_come_undone() {
    let (_dir, db, _cfg) = library();
    let id = id_of(&db, "Dishonored");
    // Willing to try again: Shelved, on its last chance, and off the page.
    db.answer_second_chance(PlayerId::FIRST, id, Some(Answer::Willing)).unwrap();
    let g = game(&db, "Dishonored");
    assert_eq!((g.status, g.override_, g.second_try, g.offer()), (Status::Shelved, Some(Override::Shelved), true, Some(Offer::Shelved)));
    assert!(!listed(&g) && checked(&g), "off the page, but its Now is kept for picks");
    assert_eq!(
        crate::prompt::lines::history_segment(&g, crate::db::now()).as_deref(),
        Some("willing to try again: abandoned 2022 after 6.0h; stopped: bugs or performance")
    );
    // Taken back: abandoned again, unmarked, as it was.
    db.answer_second_chance(PlayerId::FIRST, id, None).unwrap();
    let g = game(&db, "Dishonored");
    assert_eq!((g.status, g.override_, g.second_try, g.offer()), (Status::Abandoned, None, false, Some(Offer::SecondChance)));
    assert!(listed(&g));
    // Willing, then Still no instead: the Shelved mark goes, and it's out of picks for good.
    db.answer_second_chance(PlayerId::FIRST, id, Some(Answer::Willing)).unwrap();
    db.answer_second_chance(PlayerId::FIRST, id, Some(Answer::StillNo)).unwrap();
    let g = game(&db, "Dishonored");
    assert_eq!((g.status, g.override_, g.offer(), listed(&g)), (Status::Abandoned, None, None, false));
    assert_eq!(g.second_chance.as_ref().and_then(|n| n.answer), Some(Answer::StillNo));

    // Not for me on a game marked Abandoned by hand; taken back, the hand mark returns as it was.
    let prey = id_of(&db, "Prey");
    db.set_override(PlayerId::FIRST, prey, Some(Override::Abandoned), None).unwrap();
    let marked_at = game(&db, "Prey").override_at;
    db.answer_second_chance(PlayerId::FIRST, prey, Some(Answer::NotForMe)).unwrap();
    assert_eq!(game(&db, "Prey").status, Status::Hidden);
    db.answer_second_chance(PlayerId::FIRST, prey, None).unwrap();
    let g = game(&db, "Prey");
    assert_eq!((g.status, g.override_, g.override_at), (Status::Abandoned, Some(Override::Abandoned), marked_at));

    // Played again since Still no (and abandoned again later): the answer no longer stands.
    let mut again = game(&db, "Dishonored");
    again.last_played = again.second_chance.as_ref().and_then(|n| n.answered_at).unwrap() + 1;
    assert!(!answered(&again) && listed(&again));
}

/// After Still no and its toast, Mark as… Shelved is the way back to picks.
#[test]
fn shelving_by_hand_after_still_no_brings_it_back() {
    let (_dir, db, _cfg) = library();
    let id = id_of(&db, "Dishonored");
    db.answer_second_chance(PlayerId::FIRST, id, Some(Answer::StillNo)).unwrap();
    assert_eq!(game(&db, "Dishonored").offer(), None);
    // Rejections count only after the mark, so one set in the same second works.
    db.set_override(PlayerId::FIRST, id, Some(Override::Shelved), None).unwrap();
    let g = game(&db, "Dishonored");
    assert_eq!((g.status, g.second_try, g.offer()), (Status::Shelved, false, Some(Offer::Shelved)));
    assert!(!listed(&g));
}
