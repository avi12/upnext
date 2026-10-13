//! Shared game data through the API, and the game page's reviews
//! fetched when they're asked for.

use super::*;

fn fixture_bytes(rel: &str) -> Vec<u8> {
    std::fs::read(crate::testutil::fixture_path(rel)).unwrap()
}

impl TestApp {
    async fn upload_seed(&self, bytes: Vec<u8>) -> (StatusCode, Value) {
        let req = Request::post("/api/v1/seed/upload?name=C%3A%5Cdownloads%5Cupnext-seed-2026-10-01.jsonl.gz")
            .header(header::HOST, "127.0.0.1:8787")
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .body(Body::from(bytes))
            .unwrap();
        self.send(req).await
    }
}

#[tokio::test]
async fn shared_data_applies_from_a_file_or_a_url_and_comes_out_again() {
    let services = MockServer::start().await;
    Mock::given(path("/latest/upnext-seed.json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(crate::testutil::fixture("seed/upnext-seed.json")))
        .mount(&services)
        .await;
    Mock::given(path("/latest/upnext-seed-2026-10-01.jsonl.gz"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(fixture_bytes("seed/upnext-seed-2026-10-01.jsonl.gz")))
        .mount(&services)
        .await;
    let app = test_app_with(None, Some(&services.uri())).await;

    let status = app.get("/api/v1/seed").await;
    assert_eq!((status["seeded"].as_i64(), &status["kept"]), (Some(0), &Value::Null));
    assert_eq!(status["default_url"], crate::seed::DEFAULT_URL);

    // A damaged file is refused and changes nothing.
    let mut damaged = fixture_bytes("seed/upnext-seed-2026-10-01.jsonl.gz");
    damaged.truncate(damaged.len() - 30);
    let (code, v) = app.upload_seed(damaged).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{v}");
    assert!(v["error"].as_str().unwrap().contains("damaged"), "{v}");

    let (code, v) = app.upload_seed(fixture_bytes("seed/upnext-seed-2026-10-01.jsonl.gz")).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!((v["date"].as_str(), v["matched"].as_i64(), v["written"].as_i64()), (Some("2026-10-01"), Some(1), Some(1)), "Hades: {v}");
    let hades = app.get(&format!("/api/v1/games/{}", app.id_of("Hades").await)).await;
    assert_eq!(hades["seed_date"], "2026-10-01");
    assert!(hades["digest"].as_str().unwrap().starts_with("Fixture text"));
    assert_eq!(app.db.ai_usage_rows(0).unwrap().len(), 0, "no AI call");

    let status = app.get("/api/v1/seed").await;
    assert_eq!((status["seeded"].as_i64(), status["kept"].as_str()), (Some(1), Some("upnext-seed-2026-10-01.jsonl.gz")));
    assert_eq!(status["applies"][0]["source"], "upnext-seed-2026-10-01.jsonl.gz", "the file's name, without its folder");

    let (code, offer) =
        app.call("POST", "/api/v1/seed/offer", Some(json!({"url": format!("{}/latest/upnext-seed.json", services.uri())}))).await;
    assert_eq!((code, offer["count"].as_i64(), offer["date"].as_str()), (StatusCode::OK, Some(5), Some("2026-10-01")), "{offer}");
    let (code, _) = app.call("POST", "/api/v1/seed/offer", Some(json!({"url": "file:///etc/passwd"}))).await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    // A POST, so an image or a link on another site can't make the server fetch an address.
    let (code, _) = app.call("GET", &format!("/api/v1/seed/offer?url={}/latest/upnext-seed.json", services.uri()), None).await;
    assert_eq!(code, StatusCode::METHOD_NOT_ALLOWED);

    let (code, v) = app.call("POST", "/api/v1/seed", Some(json!({"url": format!("{}/latest/upnext-seed.json", services.uri())}))).await;
    assert_eq!((code, v["written"].as_i64(), v["already"].as_i64()), (StatusCode::OK, Some(0), Some(1)), "{v}");

    let (code, v) = app.call("POST", "/api/v1/seed", Some(json!({"url": "file:///etc/passwd"}))).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{v}");
    let (code, v) = app.call("POST", "/api/v1/seed", Some(json!({"url": format!("{}/missing/upnext-seed.json", services.uri())}))).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "a 404 is the address's fault: {v}");

    let (code, v) = app.call("DELETE", "/api/v1/seed", None).await;
    assert_eq!((code, v["removed"].as_i64()), (StatusCode::OK, Some(1)), "{v}");
    let hades = app.get(&format!("/api/v1/games/{}", app.id_of("Hades").await)).await;
    assert!(hades["digest"].is_null() && hades["seed_date"].is_null(), "{hades}");
    assert_eq!(app.get("/api/v1/seed").await["kept"], Value::Null);
}

/// The game page's "What players wrote" on a seeded game: one fetch from
/// Steam's mock, stored and returned with the title.
#[tokio::test]
async fn reviews_are_fetched_when_the_game_page_asks() {
    let services = MockServer::start().await;
    Mock::given(path("/appreviews/620"))
        .respond_with(ResponseTemplate::new(200).set_body_json(crate::testutil::fixture_json("steam/appreviews_620.json")))
        .mount(&services)
        .await;
    let app = test_app_with(None, Some(&services.uri())).await;
    let portal = app.id_of("Portal 2").await;
    assert_eq!(app.get(&format!("/api/v1/games/{portal}")).await["review_texts"], json!([]));

    let (code, v) = app.call("POST", &format!("/api/v1/games/{portal}/reviews"), None).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert!(!v["review_texts"].as_array().unwrap().is_empty(), "{v}");
    assert!(v["review_total"].as_i64().is_some(), "the score comes with them");
    assert!(!app.get(&format!("/api/v1/games/{portal}")).await["review_texts"].as_array().unwrap().is_empty(), "stored");

    // Control is on Epic alone, with no Steam appid or GOG copy.
    let (code, v) = app.call("POST", &format!("/api/v1/games/{}/reviews", app.id_of("Control").await), None).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{v}");
    let (code, _) = app.call("POST", "/api/v1/games/999999/reviews", None).await;
    assert_eq!(code, StatusCode::NOT_FOUND);
}

/// Someone is waiting on the game page, so a Steam that's down is tried
/// once and answers at once, not after the jobs' minute of backoff.
#[tokio::test]
async fn reviews_from_a_store_that_is_down_fail_after_one_try() {
    let services = MockServer::start().await;
    Mock::given(path("/appreviews/620")).respond_with(ResponseTemplate::new(500)).mount(&services).await;
    let app = test_app_with(None, Some(&services.uri())).await;
    let portal = app.id_of("Portal 2").await;
    let started = std::time::Instant::now();
    let (code, v) = app.call("POST", &format!("/api/v1/games/{portal}/reviews"), None).await;
    assert_eq!(code, StatusCode::BAD_GATEWAY, "{v}");
    assert!(v["error"].as_str().unwrap().starts_with("the store didn't answer: HTTP 500"), "{v}");
    assert_eq!(services.received_requests().await.unwrap().len(), 1, "tried once");
    assert!(started.elapsed() < std::time::Duration::from_secs(10));

    // Nothing listening at all.
    let app = test_app_with(None, Some("http://127.0.0.1:1")).await;
    let (code, v) = app.call("POST", &format!("/api/v1/games/{portal}/reviews"), None).await;
    assert_eq!(code, StatusCode::BAD_GATEWAY, "{v}");
    assert!(v["error"].as_str().unwrap().starts_with("the store didn't answer: "), "{v}");
}

/// The offer has the server fetch the address it's given, so a read token
/// can't ask for it: it would tell a read token what answers on the
/// server's network. Seeing what's applied stays a read.
#[tokio::test]
async fn a_read_token_cant_have_the_server_fetch_an_offer() {
    let app = reconfigured(test_app().await, |c| c.host = "0.0.0.0".into());
    let read = app.db.token_create("dashboard", crate::db::Scope::Read, "token").unwrap().token;
    let full = app.db.token_create("script", crate::db::Scope::Full, "token").unwrap().token;
    let ask = |method: &'static str, uri: &'static str, token: String| {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, "den.lan:8787")
            .header(header::AUTHORIZATION, format!("Bearer {token}"));
        let body = if method == "POST" {
            req = req.header(header::CONTENT_TYPE, "application/json");
            Body::from(json!({ "url": "http://127.0.0.1:1/upnext-seed.json" }).to_string())
        } else {
            Body::empty()
        };
        let peer = crate::server::listen::Peer { addr: std::net::SocketAddr::from(([192, 168, 0, 20], 50000)), tls: false };
        app.router.clone().layer(axum::extract::connect_info::MockConnectInfo(peer)).oneshot(req.body(body).unwrap())
    };
    assert_eq!(ask("GET", "/api/v1/seed", read.clone()).await.unwrap().status(), StatusCode::OK);
    let offer = "/api/v1/seed/offer";
    assert_eq!(ask("POST", offer, read).await.unwrap().status(), StatusCode::FORBIDDEN);
    assert_eq!(ask("POST", offer, full).await.unwrap().status(), StatusCode::BAD_GATEWAY, "a full token may: nothing answers there");
}
