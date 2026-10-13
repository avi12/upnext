//! The wishlist in picks. A wishlisted game standing in
//! for one pick, with the owned game to switch to; the AI's suggestion of a
//! game you don't own, checked against a mock of Steam's store; and the
//! switch that turns it all off.

use super::tests::{answer, id_of, library, names, request};
use super::*;
use crate::db::PlayerId;
use crate::db::{DetailsRow, NewStatement, NewWish, ReviewRow, WishSource, WishState};
use crate::testutil::{ScriptedLlm, id_in};
use serde_json::json;
use wiremock::matchers::{path, path_regex};
use wiremock::{Mock, MockServer, Request as MockRequest, ResponseTemplate};

/// A game you don't own on the wishlist table, with its store data.
fn wish(db: &Db, appid: u32, name: &str, state: WishState, tags: &[&str]) -> i64 {
    let new = NewWish {
        appid,
        name: name.into(),
        source: WishSource::Manual,
        reason: None,
        state,
        on_steam: false,
        series_key: None,
        batch_id: None,
    };
    let (id, _) = db.wishlist_add(PlayerId::FIRST, &new).unwrap();
    let details = DetailsRow {
        name: Some(name.into()),
        type_: Some("game".into()),
        short_description: Some(format!("{name}, a test game.")),
        genres: vec!["Action".into()],
        release_date: Some("Sep 25, 2025".into()),
        ..Default::default()
    };
    db.put_details(appid, Some(&details)).unwrap();
    let reviews = ReviewRow { total_positive: Some(1_900), total_reviews: Some(2_000), ..Default::default() };
    db.put_reviews(appid, Some(&reviews)).unwrap();
    let tags: Vec<(String, i64)> = tags.iter().map(|t| (t.to_string(), 100)).collect();
    db.put_tags(appid, Some(&tags)).unwrap();
    id
}

/// An answer with a wishlisted game in place of one of its picks.
fn with_wished(
    picks: &'static [&'static str],
    spares: &'static [&'static str],
    wished: i64,
    instead_of: &'static str,
) -> crate::testutil::Answer {
    let base = answer(picks, spares);
    Box::new(move |req: &LlmRequest| {
        let mut v = base(req)?;
        let p = req.full_prompt();
        v["wishlisted"] = json!({"id": format!("w{wished}"), "name": "", "reason": "It's the one you've waited for.", "instead_of": id_in(&p, instead_of)});
        Ok(v)
    })
}

/// An answer with a suggestion of a game you don't own.
fn with_suggestion(name: &'static str) -> crate::testutil::Answer {
    let base = answer(&["Celeste", "Hades", "Inside"], &["Limbo"]);
    Box::new(move |req: &LlmRequest| {
        let mut v = base(req)?;
        v["suggestion"] = json!({"name": name, "reason": format!("{name} is right down your alley.")});
        Ok(v)
    })
}

#[tokio::test]
async fn a_wishlisted_game_stands_in_for_one_pick_and_switches_back() {
    let (_dir, db, cfg) = library();
    let silksong = wish(&db, 1030300, "Hollow Knight: Silksong", WishState::Wishlisted, &["Metroidvania", "Souls-like"]);
    // Owned already (a sync found it), answered, or only suggested: never listed.
    wish(&db, 504230, "Celeste", WishState::Wishlisted, &[]);
    wish(&db, 1145350, "Hades II", WishState::Suggested, &[]);
    wish(&db, 2000, "Not For Me", WishState::Dismissed, &[]);

    let llm = ScriptedLlm::new(vec![with_wished(&["Celeste", "Hades", "Inside"], &["Limbo", "Outer Wilds"], silksong, "Hades")]);
    let batch = pick(&db, &cfg, &llm, None, request("something tough")).await.unwrap();
    let asked = llm.asked.lock().unwrap();
    let p = &asked[0].prompt;
    assert!(p.contains("## On my wishlist (games I don't own)"), "{p}");
    assert!(
        p.contains(&format!(
            "\nw{silksong} Hollow Knight: Silksong | Action, Metroidvania, Souls-like | ?h | 95%/2.0k | 2025 | says: Hollow Knight: Silksong, a test game.\n"
        )),
        "{p}"
    );
    assert_eq!(p.lines().filter(|l| l.starts_with('w')).count(), 1, "only the wishlisted game you don't own: {p}");
    assert!(asked[0].system.contains("\"On my wishlist\" lists games they want but don't own"));
    assert!(asked[0].schema["properties"]["wishlisted"].is_object() && asked[0].schema["properties"]["suggestion"].is_object());
    assert_eq!(asked[0].schema["required"], json!(["picks", "spares"]), "both are optional");
    drop(asked);

    let hades = id_of(&db, "Hades");
    let check = |b: &PickBatch| {
        assert_eq!(names(&b.picks), ["Celeste", "Hollow Knight: Silksong", "Inside"]);
        let w = &b.picks[1];
        assert_eq!((w.id, w.owned, w.slot, w.game.is_none()), (-silksong, false, Some(1), true));
        assert_eq!(w.reason, "It's the one you've waited for.");
        assert_eq!(w.wishlist.as_ref().map(|g| (g.appid, g.review_pct, g.state)), Some((1030300, Some(95), WishState::Wishlisted)));
        let alt = w.alternate.as_ref().unwrap();
        assert_eq!((alt.id, alt.name.as_str(), alt.reason.as_str(), alt.game.is_some()), (hades, "Hades", "Hades fits.", true));
        assert!(b.picks.iter().filter(|p| p.id != w.id).all(|p| p.owned && p.alternate.is_none()));
        assert_eq!(names(&b.spares), ["Limbo", "Outer Wilds"]);
    };
    check(&batch);
    check(&db.pick_batch(PlayerId::FIRST, &batch.batch_id).unwrap().unwrap());
    assert_eq!(
        db.wishlist_by_appid(PlayerId::FIRST, 504230).unwrap().unwrap().state,
        WishState::InLibrary,
        "a purchase leaves the wishlist first"
    );

    // Its only action is the switch.
    let f = crate::lineup::Feedback { title_id: -silksong, action: FeedbackAction::NotTonight, text: None, reasons: None };
    assert!(matches!(crate::lineup::feedback(&db, PlayerId::FIRST, &batch.batch_id, f), Err(crate::lineup::LineupError::Bad(_))));
    assert!(crate::lineup::switch(&db, PlayerId::FIRST, &batch.batch_id, hades).is_err(), "Hades isn't on screen");
    let celeste = id_of(&db, "Celeste");
    assert!(
        matches!(crate::lineup::switch(&db, PlayerId::FIRST, &batch.batch_id, celeste), Err(crate::lineup::LineupError::Bad(_))),
        "nothing to switch"
    );

    let after = crate::lineup::switch(&db, PlayerId::FIRST, &batch.batch_id, -silksong).unwrap();
    assert_eq!(names(&after.picks), ["Celeste", "Hades", "Inside"]);
    assert_eq!((after.picks[1].slot, after.picks[1].owned, after.picks[1].reason.as_str()), (Some(1), true, "Hades fits."));
    assert_eq!(names(&after.spares), ["Limbo", "Outer Wilds"], "the spares are kept for later");
    assert_eq!(after.removed.iter().map(|p| (p.id, p.owned)).collect::<Vec<_>>(), [(-silksong, false)]);
}

#[tokio::test]
async fn new_spares_hold_back_the_owned_game_a_switch_would_bring() {
    let (_dir, db, cfg) = library();
    let silksong = wish(&db, 1030300, "Hollow Knight: Silksong", WishState::Wishlisted, &["Metroidvania"]);
    let llm = ScriptedLlm::new(vec![with_wished(&["Celeste", "Hades", "Inside"], &[], silksong, "Hades")]);
    let batch = pick(&db, &cfg, &llm, None, request("")).await.unwrap();
    let f = crate::lineup::Feedback { title_id: id_of(&db, "Celeste"), action: FeedbackAction::NotTonight, text: None, reasons: None };
    assert_eq!(crate::lineup::feedback(&db, PlayerId::FIRST, &batch.batch_id, f).unwrap().empty_slots(), [0]);
    let llm = ScriptedLlm::new(vec![answer(&["Limbo"], &[])]);
    let more = more(&db, &cfg, &llm, &batch.batch_id, None, None).await.unwrap();
    assert_eq!(names(&more.picks), ["Limbo", "Hollow Knight: Silksong", "Inside"]);
    let asked = llm.asked.lock().unwrap();
    let hades = id_of(&db, "Hades");
    assert!(asked[0].prompt.contains("## Still in my lineup"), "{}", asked[0].prompt);
    assert!(asked[0].prompt.contains(&format!("#{hades} Hades")), "the switch's game is held: {}", asked[0].prompt);
    assert!(
        asked[0].prompt.contains(&format!("I turned down: #{} Celeste. Don't", id_of(&db, "Celeste"))),
        "only Celeste was turned down: {}",
        asked[0].prompt
    );
    assert!(!asked[0].prompt.contains("wishlist") && !asked[0].system.contains("suggestion"), "new spares leave the wishlist out");
}

#[tokio::test]
async fn a_switch_whose_owned_game_cant_show_takes_the_next_spare() {
    let (_dir, db, cfg) = library();
    let silksong = wish(&db, 1030300, "Hollow Knight: Silksong", WishState::Wishlisted, &["Metroidvania"]);
    let llm = ScriptedLlm::new(vec![with_wished(&["Celeste", "Hades", "Inside"], &["Limbo", "Outer Wilds"], silksong, "Inside")]);
    let batch = pick(&db, &cfg, &llm, None, request("")).await.unwrap();
    assert_eq!(names(&batch.picks), ["Celeste", "Hades", "Hollow Knight: Silksong"]);
    // Marked done since, so it's out of picks.
    db.set_override(PlayerId::FIRST, id_of(&db, "Inside"), Some(crate::db::Override::Done), None).unwrap();
    let after = crate::lineup::switch(&db, PlayerId::FIRST, &batch.batch_id, -silksong).unwrap();
    assert_eq!(names(&after.picks), ["Celeste", "Hades", "Limbo"]);
    assert_eq!(names(&after.spares), ["Outer Wilds"]);
}

#[tokio::test]
async fn a_wishlisted_pick_that_doesnt_hold_up_is_dropped_and_the_lineup_stands() {
    let (_dir, db, cfg) = library();
    let silksong = wish(&db, 1030300, "Hollow Knight: Silksong", WishState::Wishlisted, &[]);
    let dismissed = wish(&db, 2000, "Not For Me", WishState::Dismissed, &[]);
    for (id, instead_of) in [(silksong, "Limbo"), (dismissed, "Hades")] {
        // A spare isn't a pick, and a game not listed can't join.
        let llm = ScriptedLlm::new(vec![with_wished(&["Celeste", "Hades", "Inside"], &["Limbo"], id, instead_of)]);
        let batch = pick(&db, &cfg, &llm, None, request("")).await.unwrap();
        assert_eq!(names(&batch.picks), ["Celeste", "Hades", "Inside"]);
        assert!(batch.picks.iter().all(|p| p.owned));
        assert_eq!(batch.attempts, Some(1), "no second call over it");
    }
}

#[tokio::test]
async fn with_the_wishlist_off_or_in_a_mode_picks_have_no_trace_of_it() {
    let (_dir, db, mut cfg) = library();
    let silksong = wish(&db, 1030300, "Hollow Knight: Silksong", WishState::Wishlisted, &[]);
    cfg.wishlist = false;
    let a = with_wished(&["Celeste", "Hades", "Inside"], &["Limbo"], silksong, "Hades");
    let s = with_suggestion("Hades II");
    // Both extras in one answer, as a model might still send them.
    let both: crate::testutil::Answer = Box::new(move |req: &LlmRequest| {
        let mut v = a(req)?;
        v["suggestion"] = s(req)?["suggestion"].clone();
        Ok(v)
    });
    let llm = ScriptedLlm::new(vec![both]);
    let batch = pick(&db, &cfg, &llm, None, request("")).await.unwrap();
    assert!(batch.picks.iter().all(|p| p.owned) && batch.suggestion.is_none());
    let asked = llm.asked.lock().unwrap();
    assert!(!asked[0].prompt.contains("wishlist") && !asked[0].prompt.contains("suggestion"), "{}", asked[0].prompt);
    assert!(!asked[0].system.contains("wishlist") && !asked[0].system.contains("suggestion"), "{}", asked[0].system);
    assert_eq!(asked[0].schema, crate::prompt::schema(3, 6));
}

/// Steam's store, as the suggestion's checks read it: a search by name,
/// review summaries, details, SteamSpy's tags and art. Scary Game is
/// Horror; Lukewarm is rated 60%.
async fn store() -> MockServer {
    let s = MockServer::start().await;
    Mock::given(path("/api/storesearch/"))
        .respond_with(|req: &MockRequest| {
            let term = req.url.query_pairs().find(|(k, _)| k == "term").map(|(_, v)| v.to_lowercase()).unwrap_or_default();
            let items = [(1145350, "Hades II"), (504230, "Celeste"), (9999, "Scary Game"), (2000, "Not For Me"), (3000, "Lukewarm")];
            let found: Vec<_> =
                items.iter().filter(|(_, n)| term.contains(&n.to_lowercase())).map(|(id, n)| json!({"id": id, "name": n})).collect();
            ResponseTemplate::new(200).set_body_json(json!({"total": found.len(), "items": found}))
        })
        .mount(&s)
        .await;
    Mock::given(path_regex(r"^/appreviews/\d+$"))
        .respond_with(|req: &MockRequest| {
            let (positive, total) = if req.url.path().ends_with("/3000") { (600, 1_000) } else { (1_900, 2_000) };
            ResponseTemplate::new(200).set_body_json(json!({
                "success": 1,
                "query_summary": {"review_score": 8, "review_score_desc": "Very Positive", "total_positive": positive,
                    "total_negative": total - positive, "total_reviews": total},
                "reviews": []
            }))
        })
        .mount(&s)
        .await;
    Mock::given(path("/api/appdetails"))
        .respond_with(|req: &MockRequest| {
            let appid = req.url.query_pairs().find(|(k, _)| k == "appids").map(|(_, v)| v.into_owned()).unwrap_or_default();
            let data =
                json!({"type": "game", "name": "x", "short_description": "A test game.", "genres": [{"id": "1", "description": "Action"}]});
            ResponseTemplate::new(200).set_body_json(json!({appid: {"success": true, "data": data}}))
        })
        .mount(&s)
        .await;
    Mock::given(path("/api.php"))
        .respond_with(|req: &MockRequest| {
            let appid: i64 = req.url.query_pairs().find(|(k, _)| k == "appid").and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
            let tags = if appid == 9999 { json!({"Horror": 900}) } else { json!({"Roguelike": 900, "Action": 800}) };
            ResponseTemplate::new(200).set_body_json(json!({"appid": appid, "name": "x", "tags": tags}))
        })
        .mount(&s)
        .await;
    Mock::given(path("/IStoreBrowseService/GetItems/v1/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"response": {"store_items": []}})))
        .mount(&s)
        .await;
    s
}

#[tokio::test]
async fn a_suggestion_shows_only_once_steam_and_the_library_check_out() {
    let (_dir, db, cfg) = library();
    let s = store().await;
    let clients = crate::enrich::Clients::with_base_url(&cfg, Some(&s.uri())).unwrap();
    let no = NewStatement { kind: "hard_no".into(), title_id: None, about: Some("tag:Horror".into()), why: None, note: None };
    db.taste_state(PlayerId::FIRST, &no).unwrap();
    wish(&db, 2000, "Not For Me", WishState::Dismissed, &[]);

    // Made up, owned (by name, or by the store's appid), said no to, a hard
    // no, or rated too low: not shown, and the lineup stands.
    for name in ["A Game Nobody Made", "Portal 2", "Celeste Definitive Edition", "Not For Me", "Scary Game", "Lukewarm"] {
        let llm = ScriptedLlm::new(vec![with_suggestion(name)]);
        let batch = pick(&db, &cfg, &llm, Some(&clients), request("")).await.unwrap();
        assert!(batch.suggestion.is_none(), "{name}");
        assert_eq!(names(&batch.picks), ["Celeste", "Hades", "Inside"], "{name}");
    }
    assert_eq!(db.wishlist_rows(PlayerId::FIRST).unwrap().len(), 1, "none of them were put on the wishlist");

    let llm = ScriptedLlm::new(vec![with_suggestion("Hades II")]);
    let batch = pick(&db, &cfg, &llm, Some(&clients), request("")).await.unwrap();
    let sug = batch.suggestion.as_ref().expect("Hades II is shown");
    assert_eq!((sug.game.name.as_str(), sug.game.appid, sug.game.state), ("Hades II", 1145350, WishState::Suggested));
    assert_eq!(
        (sug.reason.as_str(), sug.game.review_pct, sug.game.tags.as_slice()),
        ("Hades II is right down your alley.", Some(95), &["Roguelike".to_string(), "Action".to_string()][..])
    );
    let row = db.wishlist_by_appid(PlayerId::FIRST, 1145350).unwrap().unwrap();
    assert_eq!((row.source, row.state, row.batch_id.as_deref()), (WishSource::Pick, WishState::Suggested, Some(batch.batch_id.as_str())));
    assert_eq!(db.pick_batch(PlayerId::FIRST, &batch.batch_id).unwrap().unwrap().suggestion.map(|s| s.game.id), Some(row.id));
    let system = llm.asked.lock().unwrap()[0].system.clone();
    assert!(system.contains("A suggestion is optional"), "{system}");

    // Answered on the Lists page, the lineup shows the answer.
    db.wishlist_set_state(row.id, WishState::Wishlisted).unwrap();
    assert_eq!(
        db.pick_batch(PlayerId::FIRST, &batch.batch_id).unwrap().unwrap().suggestion.map(|s| s.game.state),
        Some(WishState::Wishlisted)
    );
    // On the wishlist now, so it isn't suggested again.
    let llm = ScriptedLlm::new(vec![with_suggestion("Hades II")]);
    assert!(pick(&db, &cfg, &llm, Some(&clients), request("")).await.unwrap().suggestion.is_none());

    // Without AI there's no suggestion, and no store to check one.
    let llm = ScriptedLlm::new(vec![with_suggestion("Hades II")]);
    db.wishlist_delete(row.id).unwrap();
    assert!(pick(&db, &cfg, &llm, None, request("")).await.unwrap().suggestion.is_none());
}

#[tokio::test]
async fn a_suggestion_steam_cant_check_at_once_isnt_shown() {
    let (_dir, db, cfg) = library();
    let s = MockServer::start().await;
    Mock::given(path("/api/storesearch/")).respond_with(ResponseTemplate::new(503)).mount(&s).await;
    let clients = crate::enrich::Clients::with_base_url(&cfg, Some(&s.uri())).unwrap();
    let llm = ScriptedLlm::new(vec![with_suggestion("Hades II")]);
    let batch = pick(&db, &cfg, &llm, Some(&clients), request("")).await.unwrap();
    assert!(batch.suggestion.is_none());
    assert_eq!(names(&batch.picks), ["Celeste", "Hades", "Inside"]);
    assert_eq!(s.received_requests().await.unwrap().len(), 1, "one try: the pick doesn't wait out a backoff");
}
