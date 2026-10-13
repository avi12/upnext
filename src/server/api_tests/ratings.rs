//! Ratings: a pick's thumbs on its card and a game's stars on
//! its page.

use super::*;

/// A rating stays with its pick, which stays in the lineup; it can be
/// changed, given a reason with Not a fit, and taken back. Play still
/// works on a rated pick.
#[tokio::test]
async fn a_pick_is_rated_on_its_card_and_stays_in_the_lineup() {
    let app = test_app().await;
    let (status, batch) = app.call("POST", "/api/picks", Some(json!({ "prompt": "something short", "count": 1 }))).await;
    assert_eq!(status, StatusCode::OK, "{batch}");
    let id = batch["batch_id"].as_str().unwrap().to_string();
    let pick = batch["picks"][0]["id"].as_i64().unwrap();
    let spare = batch["spares"][0]["id"].as_i64().unwrap();
    let url = format!("/api/v1/picks/{id}/rating");
    assert_eq!(batch["picks"][0]["rating"], Value::Null);

    let (status, after) = app.call("PUT", &url, Some(json!({ "title_id": pick, "verdict": "not_a_fit", "reason": "wrong_mood" }))).await;
    assert_eq!(status, StatusCode::OK, "{after}");
    assert_eq!((after["picks"][0]["id"].as_i64(), after["removed"].as_array().map(Vec::len)), (Some(pick), Some(0)), "{after}");
    let rating = &after["picks"][0]["rating"];
    assert_eq!((rating["verdict"].as_str(), rating["reason"].as_str()), (Some("not_a_fit"), Some("wrong_mood")));
    let game = app.get(&format!("/api/v1/games/{pick}")).await;
    assert_eq!(
        (game["pick_rating"]["verdict"].as_str(), game["pick_rating"]["prompt"].as_str()),
        (Some("not_a_fit"), Some("something short"))
    );

    let (_, after) = app.call("PUT", &url, Some(json!({ "title_id": pick, "verdict": "good" }))).await;
    assert_eq!((after["picks"][0]["rating"]["verdict"].as_str(), &after["picks"][0]["rating"]["reason"]), (Some("good"), &Value::Null));
    let (status, after) =
        app.call("POST", &format!("/api/v1/picks/{id}/feedback"), Some(json!({ "title_id": pick, "action": "chosen" }))).await;
    assert_eq!(status, StatusCode::OK, "{after}");
    assert_eq!(
        (after["picks"][0]["feedback"]["action"].as_str(), after["picks"][0]["rating"]["verdict"].as_str()),
        (Some("chosen"), Some("good"))
    );
    // Any pick of the lineup can be rated, a spare too.
    let (status, _) = app.call("PUT", &url, Some(json!({ "title_id": spare, "verdict": "good" }))).await;
    assert_eq!(status, StatusCode::OK);

    let (_, after) = app.call("PUT", &url, Some(json!({ "title_id": pick, "verdict": null }))).await;
    assert_eq!(after["picks"][0]["rating"], Value::Null, "taken back");

    for (body, want) in [
        (json!({ "title_id": pick, "verdict": "meh" }), StatusCode::BAD_REQUEST),
        (json!({ "title_id": pick, "verdict": "not_a_fit", "reason": "too_loud" }), StatusCode::BAD_REQUEST),
        (json!({ "title_id": pick, "verdict": "good", "reason": "wrong_mood" }), StatusCode::BAD_REQUEST),
        (json!({ "title_id": 999_999, "verdict": "good" }), StatusCode::NOT_FOUND),
    ] {
        let (status, v) = app.call("PUT", &url, Some(body.clone())).await;
        assert_eq!(status, want, "{body}: {v}");
    }
    let (status, _) = app.call("PUT", "/api/v1/picks/nope/rating", Some(json!({ "title_id": pick, "verdict": "good" }))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_game_is_rated_one_to_five_and_cleared() {
    let app = test_app().await;
    let portal = app.id_of("Portal 2").await;
    let url = format!("/api/v1/games/{portal}/rating");
    let (status, g) = app.call("PUT", &url, Some(json!({ "rating": 5 }))).await;
    assert_eq!((status, g["my_rating"].as_i64()), (StatusCode::OK, Some(5)), "{g}");
    assert!(g["rated_at"].as_i64().is_some());
    let listed = app.get("/api/v1/games?q=Portal").await;
    assert_eq!(listed["items"][0]["my_rating"].as_i64(), Some(5), "the library lists it");

    let (_, g) = app.call("PUT", &url, Some(json!({ "rating": null }))).await;
    assert_eq!((&g["my_rating"], &g["rated_at"]), (&Value::Null, &Value::Null));
    for bad in [0, 6, -1] {
        let (status, v) = app.call("PUT", &url, Some(json!({ "rating": bad }))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}: {v}");
    }
    let (status, _) = app.call("PUT", "/api/v1/games/999999/rating", Some(json!({ "rating": 3 }))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// The Profile reads both ratings: pick ratings with
/// your reactions, counted by verdict for the hit rate, and game ratings as
/// a source of their own, listed, exported and deleted on their own.
#[tokio::test]
async fn the_profile_lists_counts_exports_and_deletes_ratings() {
    let app = test_app().await;
    let (_, batch) = app.call("POST", "/api/picks", Some(json!({ "prompt": "something short", "count": 2 }))).await;
    let id = batch["batch_id"].as_str().unwrap().to_string();
    let url = format!("/api/v1/picks/{id}/rating");
    let first = batch["picks"][0]["id"].as_i64().unwrap();
    let second = batch["picks"][1]["id"].as_i64().unwrap();
    app.call("PUT", &url, Some(json!({ "title_id": first, "verdict": "good" }))).await;
    app.call("PUT", &url, Some(json!({ "title_id": second, "verdict": "not_a_fit", "reason": "wrong_length" }))).await;
    let (status, _) = app.call("PUT", &format!("/api/v1/games/{first}/rating"), Some(json!({ "rating": 4 }))).await;
    assert_eq!(status, StatusCode::OK);

    let s = app.get("/api/v1/taste/sources").await;
    assert_eq!(s["reactions"]["pick_ratings"], json!({ "good": 1, "not_a_fit": 1 }));
    assert_eq!(s["ratings"], json!({ "games": 1, "by": { "4": 1 } }));
    let rows = app.get("/api/v1/taste/sources/ratings").await;
    assert_eq!((rows[0]["title_id"].as_i64(), rows[0]["stars"].as_i64()), (Some(first), Some(4)), "{rows}");
    let reactions = app.get("/api/v1/taste/sources/reactions").await;
    let whats: Vec<(&str, Option<&str>)> =
        reactions.as_array().unwrap().iter().map(|r| (r["what"].as_str().unwrap(), r["text"].as_str())).collect();
    assert!(whats.contains(&("good_pick", None)) && whats.contains(&("not_a_fit", Some("wrong_length"))), "{reactions}");
    let (_, _, bytes) = app.raw("/api/v1/taste/export").await;
    let export: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(export["ratings"][0]["stars"], 4, "{export}");

    let (status, v) = app.call("DELETE", "/api/v1/taste/sources/ratings", None).await;
    assert_eq!((status, &v["deleted"]), (StatusCode::OK, &json!(1)));
    let s = app.get("/api/v1/taste/sources").await;
    assert_eq!((s["ratings"]["games"].as_i64(), &s["reactions"]["pick_ratings"]["good"]), (Some(0), &json!(1)), "pick ratings stay");
    assert_eq!(app.get(&format!("/api/v1/games/{first}")).await["my_rating"], Value::Null);
}

/// The Library sorts and filters by your rating, and
/// its counts say how many games you rated.
#[tokio::test]
async fn the_library_sorts_and_filters_by_your_rating() {
    let app = test_app().await;
    let all = app.get("/api/v1/games?sort=name").await;
    let ids: Vec<i64> = all["items"].as_array().unwrap().iter().take(3).map(|g| g["id"].as_i64().unwrap()).collect();
    for (id, stars) in [(ids[0], 2), (ids[1], 5), (ids[2], 4)] {
        let (status, _) = app.call("PUT", &format!("/api/v1/games/{id}/rating"), Some(json!({ "rating": stars }))).await;
        assert_eq!(status, StatusCode::OK);
    }
    let names = |v: Value| v["items"].as_array().unwrap().iter().map(|g| g["id"].as_i64().unwrap()).collect::<Vec<_>>();
    assert_eq!(names(app.get("/api/v1/games?rating=rated&sort=rating").await), vec![ids[1], ids[2], ids[0]]);
    assert_eq!(names(app.get("/api/v1/games?rating=rated&sort=rating&order=asc").await), vec![ids[0], ids[2], ids[1]]);
    assert_eq!(names(app.get("/api/v1/games?rating=high&sort=rating").await), vec![ids[1], ids[2]]);
    assert_eq!(names(app.get("/api/v1/games?rating=low").await), vec![ids[0]]);
    let unrated = app.get("/api/v1/games?rating=unrated&limit=1000").await;
    assert_eq!(unrated["total"].as_u64(), all["total"].as_u64().map(|n| n - 3));
    // Unrated games come after rated ones whichever way the rating sorts.
    let sorted = names(app.get("/api/v1/games?sort=rating&order=asc&limit=1000").await);
    assert_eq!(&sorted[..3], &[ids[0], ids[2], ids[1]]);
    assert_eq!(app.get("/api/v1/stats").await["rated"], 3);
    let (status, v) = app.call("GET", "/api/v1/games?rating=foo", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
}
