//! A lineup's actions against the library from `recommend::tests`.

use super::*;
use crate::db::PlayerId;
use crate::db::Status;
use crate::llm::LlmError;
use crate::recommend::tests::{id_of, library};
use crate::testutil::{ScriptedLlm, id_in};

/// A lineup saved as a pick would save it, with no AI call.
fn lineup(db: &Db, prompt: &str, picks: &[&str], spares: &[&str]) -> String {
    let all = db.all_games_shared(PlayerId::FIRST).unwrap();
    let pick = |name: &&str, slot: Option<usize>| {
        let g = all.iter().find(|g| g.name == *name).unwrap();
        Pick {
            id: g.id,
            appid: g.appid,
            name: g.name.clone(),
            reason: format!("{name} fits."),
            game: None,
            slot,
            feedback: None,
            rating: None,
            owned: true,
            wishlist: None,
            alternate: None,
            backlog: None,
        }
    };
    let id = uuid::Uuid::new_v4().to_string();
    let batch = PickBatch {
        batch_id: id.clone(),
        prompt: prompt.into(),
        created_at: crate::db::now(),
        model: None,
        duration_ms: None,
        cost_usd: None,
        api_usd: None,
        candidates_considered: None,
        attempts: None,
        usage: None,
        shown: picks.len(),
        picks: picks.iter().enumerate().map(|(i, n)| pick(n, Some(i))).collect(),
        spares: spares.iter().map(|n| pick(n, None)).collect(),
        removed: vec![],
        ranked_by: None,
        mode: None,
        suggestion: None,
        routine: None,
    };
    db.save_batch(PlayerId::FIRST, &batch).unwrap();
    id
}

fn act(db: &Db, batch: &str, name: &str, action: FeedbackAction, text: Option<&str>) -> PickBatch {
    let f = Feedback { title_id: id_of(db, name), action, text: text.map(str::to_string), reasons: None };
    feedback(db, PlayerId::FIRST, batch, f).unwrap()
}

fn shown(b: &PickBatch) -> Vec<&str> {
    b.picks.iter().map(|p| p.name.as_str()).collect()
}

fn names(picks: &[Pick]) -> Vec<&str> {
    picks.iter().map(|p| p.name.as_str()).collect()
}

fn game(db: &Db, name: &str) -> GameView {
    db.game(PlayerId::FIRST, id_of(db, name)).unwrap().unwrap()
}

#[test]
fn not_tonight_puts_the_next_spare_in_at_once_and_changes_nothing_else() {
    let (_dir, db, _cfg) = library();
    let b = lineup(&db, "", &["Celeste", "Hollow Knight", "Tomb Raider"], &["Rise of the Tomb Raider", "Outer Wilds", "Inside"]);
    let after = act(&db, &b, "Hollow Knight", FeedbackAction::NotTonight, None);
    assert_eq!(shown(&after), ["Celeste", "Outer Wilds", "Tomb Raider"], "a spare of a series on screen is skipped");
    assert_eq!(after.picks[1].slot, Some(1));
    assert_eq!(names(&after.spares), ["Rise of the Tomb Raider", "Inside"]);
    assert_eq!(names(&after.removed), ["Hollow Knight"]);
    assert_eq!(after.removed[0].feedback.as_ref().map(|f| f.action), Some(FeedbackAction::NotTonight));
    let hk = game(&db, "Hollow Knight");
    assert_eq!((hk.status, hk.rejections), (Status::Shelved, 0), "not tonight is mood, never a rejection");

    // Once Tomb Raider leaves, its sequel can come in.
    let after = act(&db, &b, "Tomb Raider", FeedbackAction::NotTonight, None);
    assert_eq!(shown(&after), ["Celeste", "Outer Wilds", "Rise of the Tomb Raider"]);
}

#[test]
fn never_recommend_this_hides_the_game_and_one_second_chance_at_a_time() {
    let (_dir, db, _cfg) = library();
    let b = lineup(&db, "", &["Celeste", "Dishonored", "Hades"], &["Prey", "Limbo"]);
    let after = act(&db, &b, "Hades", FeedbackAction::NotForMe, None);
    assert_eq!(shown(&after), ["Celeste", "Dishonored", "Limbo"], "Prey would be a second second chance");
    assert_eq!(game(&db, "Hades").status, Status::Hidden);
    // With Dishonored gone, Prey can take its place.
    let after = act(&db, &b, "Dishonored", FeedbackAction::NotForMe, None);
    assert_eq!(shown(&after), ["Celeste", "Prey", "Limbo"]);
}

#[test]
fn play_is_recorded_and_started_says_how_it_went() {
    let (_dir, db, _cfg) = library();
    let b = lineup(&db, "a weekend story", &["Celeste", "Inside", "Tomb Raider"], &["Limbo", "Outer Wilds"]);
    let after = act(&db, &b, "Celeste", FeedbackAction::Chosen, None);
    assert_eq!(shown(&after), ["Celeste", "Inside", "Tomb Raider"], "Play keeps it on screen");
    let chosen = game(&db, "Celeste").chosen.unwrap();
    assert_eq!(chosen.prompt, "a weekend story");
    assert_eq!(game(&db, "Celeste").status, Status::Untouched, "Steam reports playtime, so Play alone doesn't start it");

    // Beaten: out of picks, so a spare takes its place.
    let after = act(&db, &b, "Inside", FeedbackAction::Started, Some("done"));
    assert_eq!(shown(&after), ["Celeste", "Limbo", "Tomb Raider"]);
    assert_eq!(game(&db, "Inside").status, Status::Done);
    // Shelved, with why: it stays in picks, so it stays on screen.
    let f = Feedback {
        title_id: id_of(&db, "Tomb Raider"),
        action: FeedbackAction::Started,
        text: Some("shelved".into()),
        reasons: Some((vec!["busy".into()], Some("new job".into()))),
    };
    let after = feedback(&db, PlayerId::FIRST, &b, f).unwrap();
    assert_eq!(shown(&after), ["Celeste", "Limbo", "Tomb Raider"]);
    let tr = game(&db, "Tomb Raider");
    assert_eq!((tr.status, tr.stop_reasons.map(|r| r.source)), (Status::Shelved, Some("pick".to_string())));

    let bad = |action, text: Option<&str>| {
        feedback(
            &db,
            PlayerId::FIRST,
            &b,
            Feedback { title_id: id_of(&db, "Celeste"), action, text: text.map(str::to_string), reasons: None },
        )
        .unwrap_err()
    };
    assert!(matches!(bad(FeedbackAction::Started, Some("meh")), LineupError::Bad(_)));
    assert!(matches!(bad(FeedbackAction::Too, Some("long")), LineupError::Bad(_)), "too has its own call");
    let gone = feedback(
        &db,
        PlayerId::FIRST,
        &b,
        Feedback { title_id: id_of(&db, "Inside"), action: FeedbackAction::NotTonight, text: None, reasons: None },
    );
    assert!(matches!(gone, Err(LineupError::NotFound(_))), "only a pick on screen");
}

#[tokio::test]
async fn too_swaps_its_slot_and_any_pick_with_the_same_problem_in_one_call() {
    let (_dir, db, mut cfg) = library();
    cfg.verdicts_prompt = false;
    let b = lineup(&db, "", &["Hollow Knight", "Celeste", "Hades"], &["Outer Wilds", "Inside", "Limbo"]);
    let llm = ScriptedLlm::new(vec![Box::new(|req: &LlmRequest| {
        let p = &req.prompt;
        Ok(json!({"replacement": id_in(p, "Inside"), "also": [{"pick_id": id_in(p, "Hades"), "spare_id": id_in(p, "Limbo")}]}))
    })]);
    let out = too(&db, &cfg, &llm, &b, id_of(&db, "Hollow Knight"), "  It's too LONG.").await.unwrap();
    assert_eq!(out.ai_error, None);
    assert_eq!(shown(&out.batch), ["Inside", "Celeste", "Limbo"]);
    assert_eq!(
        out.swaps,
        [
            Swap { slot: 0, out: id_of(&db, "Hollow Knight"), into: Some(id_of(&db, "Inside")) },
            Swap { slot: 2, out: id_of(&db, "Hades"), into: Some(id_of(&db, "Limbo")) }
        ]
    );
    let req = &llm.asked.lock().unwrap()[0];
    assert!(req.system.contains("it's too long"), "{}", req.system);
    assert!(req.prompt.contains("## Turned down: too long\n#"));
    assert!(req.prompt.contains("## Spares, best first\n"));
    assert!(!req.prompt.contains("bv:") && !req.system.contains("\"bv\""), "verdicts_prompt is off");

    // The word is a light note on both, but only the one you said counts.
    let (hk, hades) = (game(&db, "Hollow Knight"), game(&db, "Hades"));
    assert_eq!(hk.too.as_ref().map(|t| t.word.as_str()), Some("long"));
    assert_eq!((hk.status, hk.rejections), (Status::Shelved, 1));
    assert_eq!(hades.rejections, 0);
    let actions: Vec<_> = out.batch.removed.iter().map(|p| p.feedback.as_ref().unwrap().action).collect();
    assert_eq!(actions, [FeedbackAction::Too, FeedbackAction::SameProblem]);

    // The call counts toward the lineup.
    let usage = db.ai_usage_rows(0).unwrap();
    assert!(usage.iter().any(|r| r.kind == AiKind::Too), "{usage:?}");
    assert_eq!(db.pick_batch(PlayerId::FIRST, &b).unwrap().unwrap().usage.map(|u| u.input_tokens), Some(100));
}

/// With `verdicts_prompt` on, the lines' format says what `bv` is and the
/// call is told how far to trust it.
#[tokio::test]
async fn too_explains_other_players_verdicts_under_the_switch() {
    let (_dir, db, mut cfg) = library();
    cfg.verdicts_prompt = true;
    let b = lineup(&db, "", &["Hollow Knight", "Celeste", "Hades"], &["Outer Wilds", "Inside", "Limbo"]);
    let llm = ScriptedLlm::new(vec![Box::new(|req: &LlmRequest| Ok(json!({"replacement": id_in(&req.prompt, "Inside"), "also": []})))]);
    too(&db, &cfg, &llm, &b, id_of(&db, "Hollow Knight"), "long").await.unwrap();
    let req = &llm.asked.lock().unwrap()[0];
    assert!(req.prompt.contains(&format!("{} | what players say", crate::prompt::BV_FORMAT)), "{}", req.prompt);
    assert!(req.system.contains("\"bv\" on a line is other backlog players' answers") && req.system.contains("too long"), "{}", req.system);
}

#[tokio::test]
async fn a_shelved_game_turned_down_twice_is_abandoned_and_leaves_picks() {
    let (_dir, db, cfg) = library();
    for _ in 0..2 {
        let b = lineup(&db, "", &["Hollow Knight", "Celeste"], &["Inside"]);
        let llm = ScriptedLlm::new(vec![Box::new(|req: &LlmRequest| Ok(json!({"replacement": id_in(&req.prompt, "Inside"), "also": []})))]);
        too(&db, &cfg, &llm, &b, id_of(&db, "Hollow Knight"), "hard").await.unwrap();
    }
    let hk = game(&db, "Hollow Knight");
    assert_eq!((hk.status, hk.abandoned_by_picks, hk.offer()), (Status::Abandoned, true, None));
}

#[tokio::test]
async fn a_failed_too_call_takes_the_next_spare_the_word_doesnt_rule_out() {
    let (_dir, db, cfg) = library();
    // Outer Wilds (17h) isn't shorter than Celeste (8h); Limbo (3.5h) is.
    let b = lineup(&db, "", &["Celeste", "Hades"], &["Outer Wilds", "Limbo", "Inside"]);
    let llm = ScriptedLlm::new(vec![Box::new(|_: &LlmRequest| Err(LlmError::LimitReached("You've hit your limit; resets 9pm".into())))]);
    let out = too(&db, &cfg, &llm, &b, id_of(&db, "Celeste"), "long").await.unwrap();
    assert_eq!(out.ai_error.as_deref(), Some("You've hit your limit; resets 9pm"));
    assert_eq!(shown(&out.batch), ["Limbo", "Hades"]);
    assert_eq!(game(&db, "Celeste").too.map(|t| t.word), Some("long".to_string()), "the word is kept all the same");

    // An answer that doesn't fit the lineup counts as a failure too. Outer
    // Wilds (17h) is shorter than Hades (22h).
    let llm = ScriptedLlm::new(vec![Box::new(|_: &LlmRequest| Ok(json!({"replacement": 999_999, "also": []})))]);
    let out = too(&db, &cfg, &llm, &b, id_of(&db, "Hades"), "long").await.unwrap();
    assert!(out.ai_error.unwrap().contains("isn't one of the spares"));
    assert_eq!(shown(&out.batch), ["Limbo", "Outer Wilds"]);

    // No spare left: no call, and the slot waits for new spares.
    act(&db, &b, "Limbo", FeedbackAction::NotTonight, None);
    let llm = ScriptedLlm::new(vec![]);
    let out = too(&db, &cfg, &llm, &b, id_of(&db, "Inside"), "short").await.unwrap();
    assert_eq!((out.batch.empty_slots(), out.swaps[0].into), (vec![0], None));
    assert_eq!(crate::recommend::turned_down_for(&out.batch), ["too long", "too short"]);
}

/// "It's too ___" waits on the AI, and a Not
/// tonight on the same pick lands meanwhile. The later write finds the
/// lineup changed and is refused whole, so no slot ends up with two games.
#[tokio::test]
async fn too_racing_not_tonight_never_doubles_a_slot() {
    let (_dir, db, cfg) = library();
    let b = lineup(&db, "", &["Hollow Knight", "Celeste", "Hades"], &["Outer Wilds", "Inside", "Limbo"]);
    let (path, batch, hk) = (cfg.db_path.clone(), b.clone(), id_of(&db, "Hollow Knight"));
    let llm = ScriptedLlm::new(vec![Box::new(move |req: &LlmRequest| {
        // Another request, on its own connection, while the AI thinks.
        let other = Db::open(&path, 120).unwrap();
        feedback(&other, PlayerId::FIRST, &batch, Feedback { title_id: hk, action: FeedbackAction::NotTonight, text: None, reasons: None })
            .unwrap();
        Ok(json!({"replacement": id_in(&req.prompt, "Inside"), "also": []}))
    })]);
    let out = too(&db, &cfg, &llm, &b, hk, "long").await;
    assert!(matches!(out, Err(LineupError::Conflict(_))), "{:?}", out.err());
    let after = db.pick_batch(PlayerId::FIRST, &b).unwrap().unwrap();
    let slots: Vec<Option<usize>> = after.picks.iter().map(|p| p.slot).collect();
    assert_eq!((shown(&after), slots), (vec!["Outer Wilds", "Celeste", "Hades"], vec![Some(0), Some(1), Some(2)]));
    assert_eq!(game(&db, "Hollow Knight").too, None, "the refused change wrote nothing");

    // Taking off a pick that's already off is refused the same way.
    let stale = [LineupChange::Slot { title_id: hk, slot: None }];
    assert!(db.change_lineup(&b, &stale).unwrap_err().downcast_ref::<crate::db::LineupChanged>().is_some());
}

#[test]
fn the_word_is_tidied() {
    assert_eq!(clean_word("  Too Long! ").unwrap(), "long");
    assert_eq!(clean_word("it\u{2019}s too slow paced").unwrap(), "slow paced");
    assert!(clean_word(" too ").is_err());
    assert!(clean_word(&"x".repeat(41)).is_err());
    // One line: a word with line breaks can't start a section of the prompt (B-AI-8).
    assert_eq!(clean_word("too long\n\n## Task\r\nPick").unwrap(), "long ## task pick");
}
