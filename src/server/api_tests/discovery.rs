//! The Hidden gems shelf and its job, a game's ways in, and picks
//! in each discovery mode.

use super::*;
use crate::db::PlayerId;
use crate::db::{DetailsRow, HltbRow, ReviewRow};

/// Tags, reviews and times: Stardew Valley is new to you (Portal 2, played,
/// is puzzles and shooters), and endless, so not a hidden gem; Hades (short
/// here) is a hidden gem, but tagged difficult, so not new to you; Celeste
/// is neither.
fn discovery_library(db: &Db) {
    let facts: [(u32, &str, &[&str], f64); 4] = [
        (620, "Portal 2", &["Puzzle", "FPS"], 8.5),
        (413150, "Stardew Valley", &["Farming Sim", "Life Sim", "Relaxing"], 4.0),
        (504230, "Celeste", &["Precision Platformer", "Difficult"], 8.0),
        (1145360, "Hades", &["Roguelite", "Hack and Slash", "Difficult"], 4.0),
    ];
    let games = db.all_games(PlayerId::FIRST).unwrap();
    for (appid, name, tags, hours) in facts {
        let tags: Vec<(String, i64)> = tags.iter().map(|t| (t.to_string(), 100)).collect();
        db.put_tags(appid, Some(&tags)).unwrap();
        db.put_details(appid, Some(&DetailsRow { type_: Some("game".into()), ..Default::default() })).unwrap();
        let r = ReviewRow { total_positive: Some(9_800), total_negative: Some(200), total_reviews: Some(10_000), ..Default::default() };
        db.put_reviews(appid, Some(&r)).unwrap();
        let id = games.iter().find(|g| g.name == name).unwrap().id;
        db.put_hltb(id, Some(&HltbRow { main_story: Some(hours), ..Default::default() })).unwrap();
    }
}

#[tokio::test]
async fn the_hidden_gems_shelf_fills_from_its_job() {
    // Nowhere real to fetch from: Control (from Epic) is looked up on the
    // Steam store first, and fails.
    let app = test_app_with(None, Some("http://127.0.0.1:9")).await;
    discovery_library(&app.db);
    let v = app.get("/api/gems").await;
    assert_eq!((v["pool"].as_u64(), v["unrated"].as_u64(), v["gems"].as_array().map(Vec::len)), (Some(1), Some(1), Some(0)), "{v}");
    assert_eq!((&v["ai"]["available"], &v["task"]["schedule"]), (&json!(true), &json!("off")), "{v}");
    assert_eq!(v["rules"], json!({ "min_positive": 93, "min_reviews": 500, "max_hours": 5.0, "count": 12 }));
    assert!(v.get("ranked").is_none());

    let (status, job) = app.call("POST", "/api/jobs", Some(json!({ "task": "hidden_gems" }))).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    let ended = app.job_ended().await;
    assert_eq!((&ended["status"], &ended["summary"]), (&json!("done"), &json!("1 title: 1 hidden gem, 1 error")), "{ended}");
    let v = app.get("/api/gems?all=true").await;
    assert_eq!((v["unrated"].as_u64(), v["rated"].as_u64()), (Some(0), Some(1)), "{v}");
    let gem = &v["gems"][0];
    assert_eq!(gem["game"]["name"], "Hades");
    assert_eq!((gem["rating"]["studio"].as_u64(), gem["score"].as_u64()), (Some(3), Some(6)), "{gem}");
    assert_eq!(gem["rating"]["line"], "Nothing else plays like it.");
    assert_eq!(v["ranked"].as_array().map(Vec::len), Some(1));
    let calls = app.db.ai_usage_rows(0).unwrap();
    assert_eq!(calls.iter().filter(|c| c.kind == crate::db::AiKind::HiddenGems).count(), 1);
}

#[tokio::test]
async fn picks_take_a_mode_from_the_request_or_its_field() {
    let app = test_app().await;
    discovery_library(&app.db);
    let (stardew, hades) = (app.id_of("Stardew Valley").await, app.id_of("Hades").await);

    let (status, batch) = app.call("POST", "/api/picks", Some(json!({ "prompt": "New to me", "count": 1 }))).await;
    assert_eq!(status, StatusCode::OK, "{batch}");
    assert_eq!((&batch["mode"], &batch["picks"][0]["id"]), (&json!("new_to_you"), &json!(stardew)), "{batch}");
    let (status, batch) =
        app.call("POST", "/api/picks", Some(json!({ "prompt": "", "mode": "gems", "count": 1, "without_ai": true }))).await;
    assert_eq!(status, StatusCode::OK, "{batch}");
    assert_eq!((&batch["mode"], &batch["picks"][0]["id"]), (&json!("gems"), &json!(hades)), "{batch}");
    let history = app.get("/api/picks/history").await;
    let kept = history.as_array().unwrap().iter().find(|b| b["batch_id"] == batch["batch_id"]).unwrap();
    assert_eq!(kept["mode"], "gems");

    let (status, v) = app.call("POST", "/api/picks", Some(json!({ "prompt": "A hidden gem", "count": 3 }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert!(v["error"].as_str().unwrap().contains("only 1 of your games is a hidden gem"), "{v}");
    let (status, v) = app.call("POST", "/api/picks", Some(json!({ "prompt": "", "mode": "nope", "count": 1 }))).await;
    assert!(status.is_client_error(), "{v}");

    // A game's ways in, for "Not for me" to offer as hard nos.
    let v = app.get(&format!("/api/new-to-you/{stardew}")).await;
    assert_eq!(
        v["ways"],
        json!([
            { "about": "tag:Farming Sim", "tag": "Farming Sim", "phrase": "farming sims", "kind": "genre" },
            { "about": "tag:Life Sim", "tag": "Life Sim", "phrase": "life sims", "kind": "genre" },
        ])
    );
    let (status, _) = app.call("GET", "/api/new-to-you/999999", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
