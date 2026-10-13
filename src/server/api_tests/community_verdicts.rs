//! Backlog verdicts from other players: the job against
//! a wiremock stand-in for the service, a game's verdicts, the library's
//! filters and the status Settings shows.

use super::*;
use crate::testutil::fixture_path;

/// The fixture snapshot in `tests/fixtures/verdicts/`, served as the service serves it, with
/// an ETag on the manifest.
async fn service() -> MockServer {
    let dir = fixture_path("verdicts");
    let manifest = std::fs::read(dir.join("verdicts.json")).unwrap();
    let summary = std::fs::read(dir.join("verdicts-summary-2026-10-09.jsonl.gz")).unwrap();
    let service = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/verdicts.json"))
        .and(wiremock::matchers::header("if-none-match", "\"2026-10-09\""))
        .respond_with(ResponseTemplate::new(304))
        .mount(&service)
        .await;
    Mock::given(method("GET"))
        .and(path("/verdicts.json"))
        .respond_with(ResponseTemplate::new(200).insert_header("etag", "\"2026-10-09\"").set_body_bytes(manifest))
        .mount(&service)
        .await;
    Mock::given(method("GET"))
        .and(path("/verdicts-summary-2026-10-09.jsonl.gz"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(summary))
        .expect(1)
        .mount(&service)
        .await;
    service
}

async fn run_job(app: &TestApp) -> Value {
    let (status, job) = app.call("POST", "/api/jobs", Some(json!({ "task": "verdicts" }))).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    app.job_ended().await
}

/// Off, the job says why and downloads nothing, and no game carries
/// verdicts. On, it downloads the snapshot once, a game's page and the
/// library read it, and an unchanged manifest downloads nothing more.
#[tokio::test]
async fn the_job_downloads_the_snapshot_and_games_show_it_once_on() {
    let app = test_app().await;
    let service = service().await;
    app.call("PUT", "/api/settings", Some(json!({ "verdicts_url": service.uri() }))).await;
    let hades = app.id_of("Hades").await;

    let status = app.get("/api/v1/verdicts/community").await;
    assert_eq!((&status["on"], &status["chosen"], &status["service"]), (&json!(false), &json!(false), &json!(true)), "{status}");
    assert_eq!((&status["snapshot"]["date"], &status["licence"]), (&Value::Null, &json!("CC0-1.0")));
    let ended = run_job(&app).await;
    assert_eq!((&ended["status"], &ended["task"]), (&json!("done"), &json!("verdicts")), "{ended}");
    assert!(ended["summary"].as_str().unwrap().contains("are off"), "{ended}");
    assert!(service.received_requests().await.unwrap().is_empty(), "nothing is asked while it's off");

    let (code, v) = app.call("PUT", "/api/settings", Some(json!({ "verdicts_receive": true }))).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!((&v["verdicts_receive"], &v["verdicts_receive_source"]), (&json!(true), &json!("app")), "{v}");
    let ended = run_job(&app).await;
    assert_eq!(
        ended["summary"], "Backlog verdicts of 2026-10-09: 2 of its 4 games are in this library (35 verdicts read)",
        "Hades and Celeste: {ended}"
    );
    let ended = run_job(&app).await;
    assert!(ended["summary"].as_str().unwrap().starts_with("No newer backlog verdicts than 2026-10-09"), "{ended}");

    let page = app.get(&format!("/api/v1/games/{hades}/verdicts")).await;
    assert_eq!((&page["on"], &page["chosen"], &page["snapshot"]["matched"]), (&json!(true), &json!(true), &json!(2)), "{page}");
    let v = &page["verdicts"];
    assert_eq!(v["headline"], "Worth it on a backlog, say 18 of 23 players", "{v}");
    assert_eq!(v["lines"][0], "Clicks within the first hour");
    assert_eq!(v["outcomes"][0]["label"], "Finished: usually 10 to 20 h");
    assert_eq!(v["texts"].as_array().unwrap().len(), 20, "the newest 20");
    let game = app.get(&format!("/api/games/{hades}")).await;
    assert_eq!(game["backlog"]["short"], "Worth it, 18 of 23", "{}", game["backlog"]);

    // The library's filters read what most players said.
    let listed = |v: Value| names(&v["items"]);
    assert_eq!(listed(app.get("/api/v1/games?backlog=respects_time").await), ["Hades"]);
    assert_eq!(listed(app.get("/api/v1/games?backlog=clicks_fast").await), ["Hades"]);
    assert_eq!(app.call("GET", "/api/v1/games?backlog=slow", None).await.0, StatusCode::BAD_REQUEST);

    // Celeste has 3 verdicts: counts, not lines. Portal 2 has none.
    let celeste = app.id_of("Celeste").await;
    let v = app.get(&format!("/api/v1/games/{celeste}/verdicts")).await["verdicts"].clone();
    assert_eq!((&v["counts_line"], &v["lines"]), (&json!("2 say it's worth it, 1 says no"), &json!([])), "{v}");
    let portal = app.id_of("Portal 2").await;
    assert_eq!(app.get(&format!("/api/v1/games/{portal}/verdicts")).await["verdicts"], Value::Null);
    assert_eq!(app.call("GET", "/api/v1/games/999999/verdicts", None).await.0, StatusCode::NOT_FOUND);

    // Off again: nothing shows, and the snapshot is kept for next time.
    app.call("PUT", "/api/settings", Some(json!({ "verdicts_receive": false }))).await;
    assert_eq!(app.get(&format!("/api/v1/games/{hades}/verdicts")).await["verdicts"], Value::Null);
    assert_eq!(app.get(&format!("/api/games/{hades}")).await["backlog"], Value::Null);
    assert_eq!(app.get("/api/v1/verdicts/community").await["snapshot"]["date"], "2026-10-09");
}

/// Help me decide reads other players' verdicts only under the one switch,
/// with what `bv` is and how far to trust it.
#[tokio::test]
async fn help_me_decide_reads_them_only_under_the_switch() {
    let llm = MockServer::start().await;
    let app = test_app_with(Some(format!("{}/v1", llm.uri())), None).await;
    let service = service().await;
    let off = json!({ "verdicts_url": service.uri(), "verdicts_receive": true, "verdicts_prompt": false });
    app.call("PUT", "/api/settings", Some(off)).await;
    run_job(&app).await;
    let (hades, celeste) = (app.id_of("Hades").await, app.id_of("Celeste").await);
    let answer = json!({ "pick_id": hades, "reason": "Shorter." }).to_string();
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "model": "fake-model",
            "choices": [{ "finish_reason": "stop", "message": { "role": "assistant", "content": answer } }] })))
        .mount(&llm)
        .await;
    let asked = || async {
        let (status, v) = app.call("POST", "/api/versus/decide", Some(json!({ "ids": [hades, celeste] }))).await;
        assert_eq!(status, StatusCode::OK, "{v}");
        let sent = llm.received_requests().await.unwrap();
        String::from_utf8_lossy(&sent.last().unwrap().body).to_string()
    };
    let off = asked().await;
    assert!(!off.contains("bv n=") && !off.contains("bv:") && !off.contains("\\\"bv\\\""), "{off}");
    app.call("PUT", "/api/settings", Some(json!({ "verdicts_prompt": true }))).await;
    let on = asked().await;
    assert!(on.contains("| bv n=23 worth 78%"), "{on}");
    assert!(on.contains("bv: other backlog players' verdicts") && on.contains("lines are other backlog players' answers"), "{on}");
}
