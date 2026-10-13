//! Every API error is `{"error": …}`, axum's own refusals included
//! (B-CORE-3).

use super::*;

/// A path, query or body the route can't read and a method it doesn't take
/// keep their status and headers and say why as JSON.
#[tokio::test]
async fn refusals_before_the_handler_are_json_too() {
    let app = test_app().await;
    let plain = |method: &str, uri: &str, body: &str, json_type: bool| {
        let mut b = Request::builder().method(method).uri(uri).header(header::HOST, "127.0.0.1:8787");
        if json_type {
            b = b.header(header::CONTENT_TYPE, "application/json");
        }
        b.body(Body::from(body.to_string())).unwrap()
    };
    for (req, status, says) in [
        (plain("GET", "/api/games/not-a-number", "", false), StatusCode::BAD_REQUEST, "Cannot parse `not-a-number`"),
        (plain("GET", "/api/v1/games?limit=lots", "", false), StatusCode::BAD_REQUEST, "limit"),
        (plain("POST", "/api/titles/merge", r#"{"ids":"one"}"#, true), StatusCode::UNPROCESSABLE_ENTITY, "ids"),
        (plain("POST", "/api/titles/merge", "{not json", true), StatusCode::BAD_REQUEST, "JSON"),
        (plain("POST", "/api/titles/merge", r#"{"ids":[1,2]}"#, false), StatusCode::UNSUPPORTED_MEDIA_TYPE, "Content-Type"),
        (plain("DELETE", "/api/stats", "", false), StatusCode::METHOD_NOT_ALLOWED, "method not allowed"),
    ] {
        let what = format!("{} {}", req.method(), req.uri());
        let resp = app.router.clone().oneshot(req).await.unwrap();
        let (got, headers) = (resp.status(), resp.headers().clone());
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| panic!("{what} answered {got} with {:?}, not JSON", String::from_utf8_lossy(&bytes)));
        assert_eq!(got, status, "{what}: {v}");
        assert_eq!(headers[header::CONTENT_TYPE], "application/json", "{what}");
        let error = v["error"].as_str().unwrap_or_else(|| panic!("{what}: {v}"));
        assert!(error.contains(says), "{what}: {error}");
        if got == StatusCode::METHOD_NOT_ALLOWED {
            assert!(headers.contains_key(header::ALLOW), "the route's methods are still listed: {headers:?}");
        }
    }
    // A plain-text answer that isn't an error is left as it is.
    let id = app.db.job_begin("sync", None, crate::db::JobSource::Ui, &json!({})).unwrap().unwrap();
    let resp = app.router.clone().oneshot(plain("GET", &format!("/api/jobs/{id}/log.txt"), "", false)).await.unwrap();
    assert_eq!((resp.status(), resp.headers()[header::CONTENT_TYPE].to_str().unwrap()), (StatusCode::OK, "text/plain; charset=utf-8"));
}

/// A title's id from the test library.
async fn title_id(app: &TestApp, name: &str) -> i64 {
    app.get(&format!("/api/games?q={name}")).await["items"][0]["id"].as_i64().unwrap()
}

/// An internal error's own text (here SQLite's, naming a table) is shown on
/// this computer, which has the log anyway; another device gets a plain
/// sentence, and the status stays a 500 (R-CORE-2).
#[tokio::test]
async fn an_internal_error_keeps_its_details_on_this_computer() {
    use crate::server::listen::Peer;
    let app = reconfigured(test_app().await, |c| c.host = "0.0.0.0".into());
    let full = app.db.token_create("phone", crate::db::Scope::Full, "token").unwrap().token;
    rusqlite::Connection::open(app.db.path()).unwrap().execute_batch("DROP TABLE ai_calls;").unwrap();
    let from = |ip: [u8; 4], host: &str, bearer: Option<&str>| {
        let mut b = Request::get("/api/v1/ai/usage").header(header::HOST, host);
        if let Some(t) = bearer {
            b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        let peer = Peer { addr: std::net::SocketAddr::from((ip, 50000)), tls: false };
        let router = app.router.clone().layer(axum::extract::connect_info::MockConnectInfo(peer));
        async move {
            let resp = router.oneshot(b.body(Body::empty()).unwrap()).await.unwrap();
            let status = resp.status();
            (status, serde_json::from_slice::<Value>(&to_bytes(resp.into_body(), usize::MAX).await.unwrap()).unwrap())
        }
    };

    let (status, here) = from([127, 0, 0, 1], "127.0.0.1:8787", None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{here}");
    assert!(here["error"].as_str().unwrap().contains("ai_calls"), "this computer reads the cause: {here}");

    let (status, there) = from([192, 168, 0, 42], "den.lan:8787", Some(&full)).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{there}");
    assert_eq!(there, json!({ "error": crate::server::errors::INTERNAL_ELSEWHERE }), "another device reads no table, path or SQL");
}

/// A merge, a split or a job's schedule the server fails to save is a 500,
/// not the asker's 400; what the asker got wrong is still a 400 with why.
#[tokio::test]
async fn a_change_the_server_fails_to_save_is_its_error_not_the_askers() {
    let app = test_app().await;
    let (celeste, hades, portal) = (title_id(&app, "Celeste").await, title_id(&app, "Hades").await, title_id(&app, "Portal").await);
    let copy = app.get(&format!("/api/games/{portal}")).await["copies"][1]["game_id"].as_i64().unwrap();

    // The asker's mistakes.
    let (status, v) = app.call("POST", "/api/titles/merge", Some(json!({ "ids": [celeste, hades, celeste] }))).await;
    assert_eq!(
        (status, v["error"].as_str().unwrap()),
        (StatusCode::BAD_REQUEST, format!("title #{celeste} is named twice; a title can't be merged with itself").as_str())
    );
    let (status, v) = app.call("PUT", "/api/jobs/scheduled/sync", Some(json!({ "keep": 3 }))).await;
    assert_eq!((status, v["error"].as_str().unwrap()), (StatusCode::BAD_REQUEST, "only the backup job keeps a number of files"));
    let (status, v) = app
        .call(
            "POST",
            &format!("/api/titles/{celeste}/split"),
            Some(json!({ "game_id": app.get(&format!("/api/games/{celeste}")).await["copies"][0]["game_id"] })),
        )
        .await;
    assert_eq!((status, v["error"].as_str().unwrap()), (StatusCode::BAD_REQUEST, "that title has only one copy; nothing to split"));

    // The server's own failures.
    rusqlite::Connection::open(app.db.path())
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER no_merges BEFORE INSERT ON merge_rules BEGIN SELECT RAISE(ABORT, 'disk on fire'); END;
             CREATE TRIGGER no_splits BEFORE INSERT ON split_rules BEGIN SELECT RAISE(ABORT, 'disk on fire'); END;
             CREATE TRIGGER no_schedules BEFORE INSERT ON settings WHEN NEW.key LIKE 'schedule.%' BEGIN SELECT RAISE(ABORT, 'disk on fire'); END;",
        )
        .unwrap();
    for (method, uri, body) in [
        ("POST", "/api/titles/merge".to_string(), json!({ "ids": [celeste, hades] })),
        ("POST", format!("/api/titles/{portal}/split"), json!({ "game_id": copy })),
        ("PUT", "/api/jobs/scheduled/sync".to_string(), json!({ "schedule": "daily" })),
    ] {
        let (status, v) = app.call(method, &uri, Some(body)).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{method} {uri}: {v}");
        assert!(v["error"].as_str().unwrap().contains("disk on fire"), "{method} {uri}: {v}");
    }
}
