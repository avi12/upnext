//! History in either order, a page at a time, from a time on, with
//! words to find and what came of each lineup.

use super::*;

/// Three lineups of one pick each, a day apart, oldest first: "something
/// calm", "a long 50% story" and "short_and_sweet". Returns their batch ids
/// and the pick each showed. Each leaves out the picks before it: the
/// ranking's seed follows the local date, and a pick request can't set it,
/// so on some days two lineups would otherwise show the same game and "what
/// came of them" would find both.
async fn three_lineups(app: &TestApp) -> Vec<(String, i64)> {
    let mut out: Vec<(String, i64)> = vec![];
    for prompt in ["something calm", "a long 50% story", "short_and_sweet"] {
        let shown: Vec<i64> = out.iter().map(|(_, id)| *id).collect();
        let body = json!({ "prompt": prompt, "count": 1, "exclude_ids": shown });
        let (status, b) = app.call("POST", "/api/picks", Some(body)).await;
        assert_eq!(status, StatusCode::OK, "{b}");
        out.push((b["batch_id"].as_str().unwrap().to_string(), b["picks"][0]["id"].as_i64().unwrap()));
    }
    let c = rusqlite::Connection::open(&app.state.base_cfg.db_path).unwrap();
    for (i, (id, _)) in out.iter().enumerate() {
        c.execute("UPDATE pick_batches SET created_at = ?1 WHERE id = ?2", rusqlite::params![1_800_000_000 + i as i64 * 86_400, id])
            .unwrap();
    }
    out
}

/// Answer a lineup's pick on its card.
async fn feedback(app: &TestApp, (batch, title): &(String, i64), action: &str) {
    let body = json!({ "title_id": title, "action": action });
    let (status, v) = app.call("POST", &format!("/api/v1/picks/{batch}/feedback"), Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{v}");
}

/// The prompts History lists for a query.
async fn prompts(app: &TestApp, query: &str) -> Vec<String> {
    let v = app.get(&format!("/api/v1/picks/history?{query}")).await;
    v.as_array().unwrap().iter().map(|b| b["prompt"].as_str().unwrap().to_string()).collect()
}

#[tokio::test]
async fn history_sorts_pages_and_finds() {
    let app = test_app().await;
    let lineups = three_lineups(&app).await;
    let (calm, long, short) = ("something calm", "a long 50% story", "short_and_sweet");

    assert_eq!(prompts(&app, "").await, [short, long, calm], "newest first");
    assert_eq!(prompts(&app, "order=oldest").await, [calm, long, short]);
    assert_eq!(prompts(&app, "limit=2").await, [short, long]);
    assert_eq!(prompts(&app, "limit=2&offset=2").await, [calm], "a page shorter than the limit is the last");
    assert_eq!(prompts(&app, "limit=2&offset=2&order=oldest").await, [short]);
    assert_eq!(prompts(&app, &format!("since={}", 1_800_000_000 + 86_400)).await, [short, long]);

    // Words in the request, all of them, in any case; LIKE's wildcards are
    // only themselves.
    assert_eq!(prompts(&app, "q=CALM").await, [calm]);
    assert_eq!(prompts(&app, "q=long+story").await, [long]);
    assert_eq!(prompts(&app, "q=calm+story").await, Vec::<String>::new());
    assert_eq!(prompts(&app, "q=50%25").await, [long]);
    assert_eq!(prompts(&app, "q=%25").await, [long], "% matches only a %");
    assert_eq!(prompts(&app, "q=_and_").await, [short], "_ matches only an _");
    assert_eq!(prompts(&app, "q=t_e").await, Vec::<String>::new());
    // Or the name of a game a lineup showed.
    let calm_game = app.get(&format!("/api/v1/games/{}", lineups[0].1)).await;
    let name = calm_game["name"].as_str().unwrap();
    let found = prompts(&app, &format!("q={}", name.replace(' ', "+"))).await;
    assert!(found.contains(&calm.to_string()), "{name}: {found:?}");

    // What came of them.
    assert_eq!(prompts(&app, "outcome=played").await, Vec::<String>::new());
    // Saved before the second lineup is turned down: Not tonight brings on
    // that lineup's spare, which can be the game the third one showed.
    let (status, _) = app.call("PUT", &format!("/api/v1/games/{}/later", lineups[2].1), None).await;
    assert!(status.is_success(), "{status}");
    assert_eq!(prompts(&app, "outcome=saved").await, [short]);
    feedback(&app, &lineups[0], "chosen").await;
    feedback(&app, &lineups[1], "not_tonight").await;
    assert_eq!(prompts(&app, "outcome=played").await, [calm]);
    assert_eq!(prompts(&app, "outcome=turned_down").await, [long]);
    assert_eq!(prompts(&app, "outcome=played&q=story").await, Vec::<String>::new(), "every filter applies");

    for bad in ["order=sideways", "outcome=liked", "limit=x", "since=yesterday"] {
        let (status, v) = app.call("GET", &format!("/api/v1/picks/history?{bad}"), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}: {v}");
        assert!(v["error"].as_str().is_some_and(|e| e.contains("query")), "{bad}: {v}");
    }
}
