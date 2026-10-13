//! A device asks to be allowed, the computer allows or
//! denies it with a tap, and the device collects its access; and the app
//! window on this computer asking another computer the same way.

use super::*;
use crate::server::listen::Peer;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

const HERE: [u8; 4] = [127, 0, 0, 1];
const LAN: [u8; 4] = [192, 168, 0, 42];
const KITCHEN: [u8; 4] = [192, 168, 0, 31];
const PUBLIC: [u8; 4] = [203, 0, 113, 9];

struct Answered {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Value,
}

/// A request from `peer`: from this computer it's addressed to 127.0.0.1,
/// from anywhere else to den.lan. `headers` are added as they are.
async fn from(app: &TestApp, peer: impl Into<IpAddr>, method: &str, uri: &str, headers: &[(&str, &str)], body: Option<Value>) -> Answered {
    let peer: IpAddr = peer.into();
    let host = if peer.is_loopback() { "127.0.0.1:8787" } else { "den.lan:8787" };
    let mut b = Request::builder().method(method).uri(uri).header(header::HOST, host);
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    let body = match body {
        Some(v) => {
            b = b.header(header::CONTENT_TYPE, "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let peer = Peer { addr: SocketAddr::from((peer, 50000)), tls: false };
    let res = app.router.clone().layer(axum::extract::connect_info::MockConnectInfo(peer)).oneshot(b.body(body).unwrap()).await.unwrap();
    let (status, headers) = (res.status(), res.headers().clone());
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let body = serde_json::from_slice(&bytes).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    Answered { status, headers, body }
}

fn on_the_network(app: TestApp) -> TestApp {
    reconfigured(app, |c| c.host = "0.0.0.0".into())
}

async fn ask(app: &TestApp, peer: impl Into<IpAddr>, body: Value) -> Answered {
    from(app, peer, "POST", "/api/v1/pair/request", &[], Some(body)).await
}

async fn collect(app: &TestApp, peer: impl Into<IpAddr>, asked: &Value) -> Answered {
    let uri = format!("/api/v1/pair/request/{}", asked["id"].as_str().unwrap());
    from(app, peer, "GET", &uri, &[("UpNext-Pair-Secret", asked["secret"].as_str().unwrap())], None).await
}

async fn waiting(app: &TestApp) -> Value {
    let a = from(app, HERE, "GET", "/api/v1/pairing/requests", &[], None).await;
    assert_eq!(a.status, StatusCode::OK, "{}", a.body);
    a.body
}

async fn answer(app: &TestApp, asked: &Value, body: Value) -> StatusCode {
    let uri = format!("/api/v1/pairing/requests/{}", asked["id"].as_str().unwrap());
    from(app, HERE, "POST", &uri, &[], Some(body)).await.status
}

#[tokio::test]
async fn a_phone_asks_the_computer_allows_and_the_phone_is_in() {
    let app = on_the_network(test_app().await);
    let asked = ask(&app, LAN, json!({ "name": "Pixel 9", "kind": "phone" })).await;
    assert_eq!(asked.status, StatusCode::CREATED, "{}", asked.body);
    let asked = asked.body;
    let number = asked["match_code"].as_str().unwrap();
    assert!(number.len() == 4 && number.chars().all(|c| c.is_ascii_digit()), "{asked}");
    assert!(asked["secret"].as_str().unwrap().len() >= 32);
    assert!(!asked["computer"].as_str().unwrap().is_empty() && asked["computer_id"].as_str().unwrap().len() == 16, "{asked}");
    assert!(asked["expires_at"].as_i64().unwrap() > crate::db::now() + 100);
    let health = from(&app, LAN, "GET", "/api/health", &[], None).await.body;
    assert_eq!(health["id"], asked["computer_id"], "the id a computer that keeps the token checks: {health}");

    // The computer sees it, with the number the phone shows.
    let list = waiting(&app).await;
    let shown = &list["requests"][0];
    assert_eq!(
        (shown["name"].as_str(), shown["kind"].as_str(), shown["match_code"].as_str()),
        (Some("Pixel 9"), Some("phone"), Some(number))
    );
    assert_eq!(shown["from"], "192.168.0.42");
    assert_eq!(list["players"], json!([]), "one player: nothing to choose");
    assert!(shown.get("secret").is_none(), "the secret stays with the phone: {shown}");

    // Only the secret collects, and not before an answer.
    let wrong =
        from(&app, LAN, "GET", &format!("/api/v1/pair/request/{}", asked["id"].as_str().unwrap()), &[("UpNext-Pair-Secret", "nope")], None)
            .await;
    assert_eq!(wrong.status, StatusCode::NOT_FOUND);
    let early = collect(&app, LAN, &asked).await;
    assert_eq!((early.status, early.body["status"].as_str()), (StatusCode::OK, Some("waiting")));
    assert!(early.headers.get(header::SET_COOKIE).is_none());

    assert_eq!(answer(&app, &asked, json!({ "allow": true })).await, StatusCode::NO_CONTENT);
    assert_eq!(waiting(&app).await["requests"], json!([]), "answered: off the dialog");
    let got = collect(&app, LAN, &asked).await;
    assert_eq!((got.status, got.body["status"].as_str()), (StatusCode::OK, Some("allowed")), "{}", got.body);
    assert_eq!(got.body["name"], "Pixel 9");
    assert!(got.body["token"].is_null(), "a browser gets the cookie, not the token: {}", got.body);
    let cookie = got.headers[header::SET_COOKIE].to_str().unwrap().to_string();
    assert!(cookie.starts_with(&format!("{DEVICE_COOKIE}=upnext_")) && cookie.contains("HttpOnly"), "{cookie}");
    let token = cookie.split(';').next().unwrap().to_string();
    assert_eq!(from(&app, LAN, "GET", "/api/v1/stats", &[("cookie", &token)], None).await.status, StatusCode::OK);
    assert_eq!(collect(&app, LAN, &asked).await.status, StatusCode::NOT_FOUND, "collected once");

    let tokens = from(&app, HERE, "GET", "/api/v1/tokens", &[], None).await.body;
    let device = &tokens["tokens"][0];
    assert_eq!(
        (device["name"].as_str(), device["kind"].as_str(), device["scope"].as_str()),
        (Some("Pixel 9"), Some("device"), Some("full"))
    );
}

#[tokio::test]
async fn an_app_takes_the_token_and_opens_the_ui_with_it() {
    let app = on_the_network(test_app().await);
    let asked = ask(&app, LAN, json!({ "name": "Steam Deck", "kind": "computer", "wants": "token" })).await.body;
    assert_eq!(answer(&app, &asked, json!({ "allow": true })).await, StatusCode::NO_CONTENT);
    let got = collect(&app, LAN, &asked).await;
    assert!(got.headers.get(header::SET_COOKIE).is_none());
    let token = got.body["token"].as_str().unwrap().to_string();
    assert!(token.starts_with("upnext_"), "{}", got.body);
    assert!(got.body["address"].is_null(), "no public_url: the address it used");

    // The window's first page sets the cookie from the header and goes to the UI.
    let bearer = format!("Bearer {token}");
    let opened = from(&app, LAN, "GET", "/api/v1/pair/open", &[("authorization", &bearer)], None).await;
    assert_eq!(opened.status, StatusCode::SEE_OTHER);
    assert_eq!(opened.headers[header::LOCATION], "/");
    assert!(opened.headers[header::SET_COOKIE].to_str().unwrap().starts_with(&format!("{DEVICE_COOKIE}={token};")));
    let without = from(&app, LAN, "GET", "/api/v1/pair/open", &[], None).await;
    assert_eq!(without.status, StatusCode::UNAUTHORIZED, "it needs the token");

    // With a public name, the answer says to use it.
    let app = reconfigured(app, |c| c.public_url = Some("https://den.tailnet.ts.net".into()));
    app.state.remote.approvals.a_moment_later();
    let asked = ask(&app, LAN, json!({ "wants": "token" })).await.body;
    answer(&app, &asked, json!({ "allow": true })).await;
    assert_eq!(collect(&app, LAN, &asked).await.body["address"], "https://den.tailnet.ts.net");
}

#[tokio::test]
async fn denied_and_after_three_denials_it_waits() {
    let app = on_the_network(test_app().await);
    for round in 1..=3 {
        let asked = ask(&app, LAN, json!({})).await;
        assert_eq!(asked.status, StatusCode::CREATED, "round {round}: {}", asked.body);
        assert_eq!(answer(&app, &asked.body, json!({ "allow": false })).await, StatusCode::NO_CONTENT);
        assert_eq!(answer(&app, &asked.body, json!({ "allow": true })).await, StatusCode::NOT_FOUND, "answered once");
        let got = collect(&app, LAN, &asked.body).await;
        assert_eq!(got.body["status"], "denied");
        assert!(got.body["token"].is_null() && got.headers.get(header::SET_COOKIE).is_none());
        app.state.remote.approvals.a_moment_later();
    }
    let refused = ask(&app, LAN, json!({})).await;
    assert_eq!(refused.status, StatusCode::TOO_MANY_REQUESTS, "{}", refused.body);
    let wait: i64 = refused.headers[header::RETRY_AFTER].to_str().unwrap().parse().unwrap();
    assert!((590..=600).contains(&wait), "{wait}");
    assert_eq!(ask(&app, KITCHEN, json!({})).await.status, StatusCode::CREATED, "another address isn't held up");
    assert_eq!(tokens_made(&app).await, 0);
}

async fn tokens_made(app: &TestApp) -> usize {
    from(app, HERE, "GET", "/api/v1/tokens", &[], None).await.body["tokens"].as_array().unwrap().len()
}

#[tokio::test]
async fn one_request_per_address_and_three_at_most() {
    let app = on_the_network(test_app().await);
    let first = ask(&app, LAN, json!({ "name": "first" })).await.body;
    app.state.remote.approvals.a_moment_later();
    let again = ask(&app, LAN, json!({ "name": "again" })).await.body;
    let list = waiting(&app).await;
    assert_eq!(list["requests"].as_array().unwrap().len(), 1, "{list}");
    assert_eq!(list["requests"][0]["name"], "again");
    assert_eq!(collect(&app, LAN, &first).await.status, StatusCode::NOT_FOUND, "replaced");
    assert_eq!(collect(&app, LAN, &again).await.body["status"], "waiting");

    ask(&app, KITCHEN, json!({})).await;
    ask(&app, [10, 0, 0, 5], json!({})).await;
    let full = ask(&app, [10, 0, 0, 6], json!({})).await;
    assert_eq!(full.status, StatusCode::TOO_MANY_REQUESTS, "{}", full.body);
    let numbers: std::collections::HashSet<String> =
        waiting(&app).await["requests"].as_array().unwrap().iter().map(|r| r["match_code"].as_str().unwrap().to_string()).collect();
    assert_eq!(numbers.len(), 3, "each shows its own number");

    // Two minutes on, they're gone.
    app.state.remote.approvals.expire();
    assert_eq!(waiting(&app).await["requests"], json!([]));
    assert_eq!(collect(&app, LAN, &again).await.status, StatusCode::NOT_FOUND);
    assert_eq!(ask(&app, [10, 0, 0, 6], json!({})).await.status, StatusCode::CREATED);
}

#[tokio::test]
async fn asking_over_and_over_waits_and_the_window_is_asked_now_and_then() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let app = on_the_network(test_app().await);
    let asked_for = Arc::new(AtomicUsize::new(0));
    let count = asked_for.clone();
    app.state.remote.approvals.on_request(Arc::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    }));
    let attention = || asked_for.load(Ordering::SeqCst);

    assert_eq!(ask(&app, LAN, json!({})).await.status, StatusCode::CREATED);
    assert_eq!(attention(), 1);
    for _ in 0..5 {
        let again = ask(&app, LAN, json!({})).await;
        assert_eq!(again.status, StatusCode::TOO_MANY_REQUESTS, "{}", again.body);
        let wait: i64 = again.headers[header::RETRY_AFTER].to_str().unwrap().parse().unwrap();
        assert!((1..=10).contains(&wait), "{wait}");
    }
    assert_eq!(waiting(&app).await["requests"].as_array().unwrap().len(), 1, "one request, as before");

    // Another device a moment later shows in the dialog, without asking for
    // attention again so soon.
    assert_eq!(ask(&app, KITCHEN, json!({})).await.status, StatusCode::CREATED);
    assert_eq!(attention(), 1);
    // Ten seconds on, the first device may ask again: its request is
    // replaced, and the window already asked about it.
    app.state.remote.approvals.a_moment_later();
    assert_eq!(ask(&app, LAN, json!({})).await.status, StatusCode::CREATED);
    assert_eq!(attention(), 1);
    // A new one, then, asks for attention again; whatever their addresses,
    // three wait at most.
    assert_eq!(ask(&app, [192, 168, 0, 43], json!({})).await.status, StatusCode::CREATED);
    assert_eq!(attention(), 2);
    assert_eq!(ask(&app, [192, 168, 0, 44], json!({})).await.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(waiting(&app).await["requests"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn one_ipv6_device_cannot_fill_the_dialog_from_many_addresses() {
    let app = on_the_network(test_app().await);
    let at = |s: &str| s.parse::<IpAddr>().unwrap();
    // Addresses one device could pick in its home network's /64.
    let first = ask(&app, at("fd12:3456:789a:1::10"), json!({ "name": "Pixel 9" })).await;
    assert_eq!(first.status, StatusCode::CREATED, "{}", first.body);
    let soon = ask(&app, at("fd12:3456:789a:1:abcd::20"), json!({})).await;
    assert_eq!(soon.status, StatusCode::TOO_MANY_REQUESTS, "a new address is no way round the wait: {}", soon.body);
    app.state.remote.approvals.a_moment_later();
    assert_eq!(ask(&app, at("fd12:3456:789a:1::21"), json!({})).await.status, StatusCode::CREATED);
    app.state.remote.approvals.a_moment_later();
    let third = ask(&app, at("fd12:3456:789a:1::22"), json!({})).await;
    assert_eq!(third.status, StatusCode::TOO_MANY_REQUESTS, "two at most from one /64: {}", third.body);
    assert!(third.body["error"].as_str().unwrap().contains("on this network"), "{}", third.body);
    assert_eq!(waiting(&app).await["requests"].as_array().unwrap().len(), 2);
    // So the last place is still free for another device.
    assert_eq!(ask(&app, LAN, json!({})).await.status, StatusCode::CREATED);
}

#[tokio::test]
async fn two_devices_on_one_ipv6_network_keep_their_own_requests() {
    let app = on_the_network(test_app().await);
    let (pixel, tablet) = ("fd12:3456:789a:1::10".parse::<IpAddr>().unwrap(), "fd12:3456:789a:1::20".parse::<IpAddr>().unwrap());
    let kept = ask(&app, tablet, json!({ "name": "Tablet" })).await.body;
    // The phone is denied three times; the tablet's request stays, and the
    // phone's denials aren't the tablet's.
    for _ in 0..3 {
        app.state.remote.approvals.a_moment_later();
        let asked = ask(&app, pixel, json!({ "name": "Pixel 9" })).await;
        assert_eq!(asked.status, StatusCode::CREATED, "{}", asked.body);
        let shown = waiting(&app).await;
        let names: Vec<&str> = shown["requests"].as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["Tablet", "Pixel 9"], "neither replaces the other");
        assert_eq!(answer(&app, &asked.body, json!({ "allow": false })).await, StatusCode::NO_CONTENT);
    }
    app.state.remote.approvals.a_moment_later();
    assert_eq!(ask(&app, pixel, json!({})).await.status, StatusCode::TOO_MANY_REQUESTS, "the phone waits");
    assert_eq!(collect(&app, tablet, &kept).await.body["status"], "waiting");
    assert_eq!(answer(&app, &kept, json!({ "allow": true })).await, StatusCode::NO_CONTENT);
    assert_eq!(collect(&app, tablet, &kept).await.body["status"], "allowed");
    let again = ask(&app, tablet, json!({ "name": "Tablet" })).await;
    assert_eq!(again.status, StatusCode::CREATED, "the tablet isn't held by the phone's denials: {}", again.body);
}

#[tokio::test]
async fn only_the_home_network_asks_and_only_the_computer_answers() {
    let app = on_the_network(test_app().await);
    let far = ask(&app, PUBLIC, json!({})).await;
    assert_eq!(far.status, StatusCode::FORBIDDEN);
    assert!(far.body["error"].as_str().unwrap().contains("home network"), "{}", far.body);

    let asked = ask(&app, LAN, json!({ "name": "Pixel 9" })).await.body;
    // A paired device can't see or answer requests: not with its cookie,
    // not with a full token.
    let made = app.db.token_create("script", crate::db::Scope::Full, "token").unwrap();
    let bearer = format!("Bearer {}", made.token);
    let list = from(&app, LAN, "GET", "/api/v1/pairing/requests", &[("authorization", &bearer)], None).await;
    assert_eq!(list.status, StatusCode::FORBIDDEN, "{}", list.body);
    let uri = format!("/api/v1/pairing/requests/{}", asked["id"].as_str().unwrap());
    let allow =
        from(&app, LAN, "POST", &uri, &[("authorization", &bearer), ("origin", "http://den.lan:8787")], Some(json!({ "allow": true })))
            .await;
    assert_eq!(allow.status, StatusCode::FORBIDDEN);
    let none = from(&app, LAN, "GET", "/api/v1/pairing/requests", &[], None).await;
    assert_eq!(none.status, StatusCode::UNAUTHORIZED);
    // Nor reach other computers through this one.
    let connect = from(&app, LAN, "GET", "/api/v1/network/computers", &[("authorization", &bearer)], None).await;
    assert_eq!(connect.status, StatusCode::FORBIDDEN);
    assert_eq!(collect(&app, LAN, &asked).await.body["status"], "waiting", "still waiting for the computer");
}

#[tokio::test]
async fn with_two_players_the_computer_chooses_whose_device_it_is() {
    let app = on_the_network(test_app().await);
    let sam = app.db.add_player("Sam").unwrap();
    let asked = ask(&app, LAN, json!({ "name": "Sam's phone", "wants": "token" })).await.body;
    let list = waiting(&app).await;
    let names: Vec<&str> = list["players"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(names.len(), 2, "{list}");
    assert_eq!(names[1], "Sam");
    assert_eq!(answer(&app, &asked, json!({ "allow": true, "player_id": 99 })).await, StatusCode::BAD_REQUEST);
    assert_eq!(answer(&app, &asked, json!({ "allow": true, "player_id": sam.0 })).await, StatusCode::NO_CONTENT);
    collect(&app, LAN, &asked).await;
    let bound = |name: &str| -> Option<i64> {
        let c = rusqlite::Connection::open(&app.state.base_cfg.db_path).unwrap();
        c.query_row("SELECT player_id FROM api_tokens WHERE name = ?1", [name], |r| r.get(0)).unwrap()
    };
    assert_eq!(bound("Sam's phone"), Some(sam.0));

    // Shared: no player.
    let shared = ask(&app, KITCHEN, json!({ "name": "tablet", "wants": "token" })).await.body;
    answer(&app, &shared, json!({ "allow": true, "player_id": null })).await;
    collect(&app, KITCHEN, &shared).await;
    assert_eq!(bound("tablet"), None);
}

#[tokio::test]
async fn allowing_a_device_as_a_player_with_a_pin_takes_their_pin() {
    let app = on_the_network(test_app().await);
    let sam = app.db.add_player("Sam").unwrap();
    app.db.set_pin(sam, Some("2468")).unwrap();
    let asked = ask(&app, LAN, json!({ "name": "Sam's phone", "wants": "token" })).await.body;
    assert_eq!(answer(&app, &asked, json!({ "allow": true, "player_id": sam.0 })).await, StatusCode::FORBIDDEN, "no PIN");
    assert_eq!(answer(&app, &asked, json!({ "allow": true, "player_id": sam.0, "pin": "1111" })).await, StatusCode::FORBIDDEN);
    assert_eq!(collect(&app, LAN, &asked).await.body["status"], "waiting", "a refused answer leaves it waiting");
    assert_eq!(answer(&app, &asked, json!({ "allow": true, "player_id": sam.0, "pin": "2468" })).await, StatusCode::NO_CONTENT);
    assert_eq!(collect(&app, LAN, &asked).await.body["status"], "allowed");
    // Everyone's needs no PIN, and denying never does.
    let shared = ask(&app, KITCHEN, json!({ "name": "tablet", "wants": "token" })).await.body;
    assert_eq!(answer(&app, &shared, json!({ "allow": true, "player_id": null })).await, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn the_window_asks_another_computer_and_keeps_its_token() {
    let app = test_app().await;
    let den = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "app": "upnext", "version": "0.1.0", "id": "4f2a9c01d3e85b67" })))
        .mount(&den)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/pair/request"))
        .and(body_string_contains("\"wants\":\"token\""))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "ab12", "secret": "s3cret", "match_code": "4821", "expires_at": crate::db::now() + 120,
            "computer": "den", "computer_id": "4f2a9c01d3e85b67"
        })))
        .mount(&den)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/pair/request/ab12"))
        .and(wiremock::matchers::header("upnext-pair-secret", "s3cret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "status": "waiting" })))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&den)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/pair/request/ab12"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "status": "allowed", "id": 7, "name": "this one", "token": "upnext_fromden" })),
        )
        .with_priority(2)
        .mount(&den)
        .await;

    assert_eq!(app.call("GET", "/api/v1/network/connect", None).await.0, StatusCode::NOT_FOUND, "nothing asked yet");
    let (status, bad) = app.call("POST", "/api/v1/network/connect", Some(json!({ "address": "den lan" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    // Found as another computer: the address went to this one since.
    let (status, moved) =
        app.call("POST", "/api/v1/network/connect", Some(json!({ "address": den.uri(), "id": "0123456789abcdef" }))).await;
    assert_eq!(status, StatusCode::CONFLICT, "{moved}");
    assert!(moved["error"].as_str().unwrap().contains("another UpNext"), "{moved}");
    assert_eq!(app.call("GET", "/api/v1/network/connect", None).await.0, StatusCode::NOT_FOUND, "nothing was asked");
    let (status, asked) =
        app.call("POST", "/api/v1/network/connect", Some(json!({ "address": den.uri(), "id": "4f2a9c01d3e85b67" }))).await;
    assert_eq!(status, StatusCode::CREATED, "{asked}");
    assert_eq!(
        (asked["status"].as_str(), asked["match_code"].as_str(), asked["computer"].as_str()),
        (Some("waiting"), Some("4821"), Some("den"))
    );
    let (_, now) = app.call("GET", "/api/v1/network/connect", None).await;
    assert_eq!(now["status"], "waiting", "{now}");
    let (_, now) = app.call("GET", "/api/v1/network/connect", None).await;
    assert_eq!((now["status"].as_str(), now["id"].as_str()), (Some("allowed"), Some("4f2a9c01d3e85b67")), "{now}");

    // Kept in the config folder, token and all; listed without the token.
    let kept = crate::computers::Computers::load(&app.state.base_cfg.config_dir).unwrap();
    let c = kept.get("4f2a9c01d3e85b67").unwrap();
    assert_eq!((c.name.as_str(), c.address.as_str(), c.token.as_str()), ("den", den.uri().as_str(), "upnext_fromden"));
    *app.state.remote.connecting.found.lock().unwrap() = vec![crate::announce::Found {
        id: "77".into(),
        name: "attic".into(),
        version: "0.1.0".into(),
        address: "http://192.168.0.42:8787".into(),
    }];
    let nearby = app.get("/api/v1/network/computers").await;
    assert_eq!(nearby["found"][0]["name"], "attic");
    assert_eq!(nearby["saved"][0]["name"], "den");
    assert!(nearby["saved"][0].get("token").is_none(), "{nearby}");
    assert_eq!(nearby["this"]["id"].as_str().map(str::len), Some(16));
    assert_eq!(app.call("DELETE", "/api/v1/network/computers/4f2a9c01d3e85b67", None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(app.call("DELETE", "/api/v1/network/computers/4f2a9c01d3e85b67", None).await.0, StatusCode::NOT_FOUND);

    // Something that isn't UpNext, or an UpNext too old to ask.
    let other = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/health"))
        .respond_with(ResponseTemplate::new(200).set_body_string("hello"))
        .mount(&other)
        .await;
    let (status, why) = app.call("POST", "/api/v1/network/connect", Some(json!({ "address": other.uri() }))).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{why}");
    assert!(why["error"].as_str().unwrap().contains("not UpNext"), "{why}");
}

#[tokio::test]
async fn the_name_on_the_network_is_a_setting() {
    let app = test_app().await;
    let (status, s) = app.call("PUT", "/api/v1/settings", Some(json!({ "network_name": "  Living   room PC " }))).await;
    assert_eq!(status, StatusCode::OK, "{s}");
    assert_eq!(app.state.cfg().network_name.as_deref(), Some("Living room PC"));
    let (status, _) = app.call("PUT", "/api/v1/settings", Some(json!({ "network_name": "x".repeat(64) }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = app.call("PUT", "/api/v1/settings", Some(json!({ "announce": false }))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!app.state.cfg().announce);
    // Off, or listening on this computer alone, nothing is announced.
    assert_eq!(crate::server::network::announcement(&app.state), None);
}

/// Through a listed proxy, the client is the right-most hop the proxy
/// didn't add itself: what a client writes at the left of
/// `X-Forwarded-For` doesn't change who is asking.
#[tokio::test]
async fn a_client_cant_name_itself_through_a_listed_proxy() {
    let app = on_the_network(test_app().await);
    let (status, v) = app.here("PUT", "/api/settings", Some(json!({ "trusted_proxies": "127.0.0.1" }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let uri = "/api/v1/pair/request";
    let body = json!({ "name": "Pixel 9", "kind": "phone" });
    // A public client claiming a tailnet address is still public.
    let spoofed = [("X-Forwarded-For", "100.64.0.9, 203.0.113.9"), ("X-Forwarded-Proto", "https")];
    let a = from(&app, HERE, "POST", uri, &spoofed, Some(body.clone())).await;
    assert_eq!(a.status, StatusCode::FORBIDDEN, "{}", a.body);
    assert!(a.body["error"].as_str().unwrap().contains("home network"), "{}", a.body);
    assert!(waiting(&app).await["requests"].as_array().unwrap().is_empty(), "no dialog was raised");
    // A tailnet client claiming a public address is still the tailnet one.
    let claimed = [("X-Forwarded-For", "203.0.113.9, 100.64.0.7"), ("X-Forwarded-Proto", "https")];
    let asked = from(&app, HERE, "POST", uri, &claimed, Some(body)).await;
    assert_eq!(asked.status, StatusCode::CREATED, "{}", asked.body);
    let shown = &waiting(&app).await["requests"][0];
    assert_eq!((&shown["from"], &shown["via"]), (&json!("100.64.0.7"), &json!("127.0.0.1")), "{shown}");
}

/// B-AUTH-2 (code health review 2026-10-11): asking to be allowed is for
/// the home network, judged by who is asking, not by the proxy in front.
/// Through a proxy that isn't listed (Tailscale Funnel or Caddy on this
/// computer) nobody may ask, since anyone could be behind it; through a
/// listed one, a client it names on a private network may, and the dialog
/// says which client and which proxy; a public client it names may not.
#[tokio::test]
async fn through_a_proxy_only_a_listed_ones_home_clients_ask() {
    let app = on_the_network(test_app().await);
    let from_far = [("X-Forwarded-For", "203.0.113.9"), ("X-Forwarded-Proto", "https")];
    let body = json!({ "name": "Pixel 9", "kind": "phone" });
    // An unlisted proxy on this computer, and one on the network.
    for peer in [HERE, [10, 0, 0, 2]] {
        let a = from(&app, peer, "POST", "/api/v1/pair/request", &from_far, Some(body.clone())).await;
        assert_eq!(a.status, StatusCode::FORBIDDEN, "{}", a.body);
        assert!(a.body["error"].as_str().unwrap().contains("trusted_proxies"), "{}", a.body);
        let tailnet = [("X-Forwarded-For", "100.64.0.7")];
        assert_eq!(from(&app, peer, "POST", "/api/v1/pair/request", &tailnet, Some(body.clone())).await.status, StatusCode::FORBIDDEN);
    }
    // nginx's bare proxy_pass: no header, but HTTP/1.0.
    let uri = "/api/v1/pair/request";
    let req = Request::post(uri).version(axum::http::Version::HTTP_10).header(header::HOST, "den.lan:8787");
    let peer = Peer { addr: SocketAddr::from((HERE, 50000)), tls: false };
    let res = app
        .router
        .clone()
        .layer(axum::extract::connect_info::MockConnectInfo(peer))
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert!(waiting(&app).await["requests"].as_array().unwrap().is_empty(), "no dialog was raised");

    // Listed: a public client is still from afar, a tailnet one may ask.
    let (status, v) = app.here("PUT", "/api/settings", Some(json!({ "trusted_proxies": "127.0.0.1" }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let a = from(&app, HERE, "POST", uri, &from_far, Some(body.clone())).await;
    assert_eq!(a.status, StatusCode::FORBIDDEN, "{}", a.body);
    assert!(a.body["error"].as_str().unwrap().contains("home network"), "{}", a.body);
    let tailnet = [("X-Forwarded-For", "100.64.0.7"), ("X-Forwarded-Proto", "https")];
    let asked = from(&app, HERE, "POST", uri, &tailnet, Some(body.clone())).await;
    assert_eq!(asked.status, StatusCode::CREATED, "{}", asked.body);
    let shown = &waiting(&app).await["requests"][0];
    assert_eq!((&shown["from"], &shown["via"]), (&json!("100.64.0.7"), &json!("127.0.0.1")), "{shown}");
    // Collecting it follows the same rule.
    let id = asked.body["id"].as_str().unwrap();
    let secret = asked.body["secret"].as_str().unwrap();
    let path = format!("/api/v1/pair/request/{id}");
    let far = [("X-Forwarded-For", "203.0.113.9"), ("X-Forwarded-Proto", "https"), ("UpNext-Pair-Secret", secret)];
    assert_eq!(from(&app, HERE, "GET", &path, &far, None).await.status, StatusCode::FORBIDDEN);
    let near = [("X-Forwarded-For", "100.64.0.7"), ("X-Forwarded-Proto", "https"), ("UpNext-Pair-Secret", secret)];
    let c = from(&app, HERE, "GET", &path, &near, None).await;
    assert_eq!((c.status, &c.body["status"]), (StatusCode::OK, &json!("waiting")), "{}", c.body);

    // Asked directly from the home network, there is no proxy to name.
    let direct = ask(&app, LAN, json!({ "name": "Pixel 8" })).await;
    assert_eq!(direct.status, StatusCode::CREATED, "{}", direct.body);
    let list = waiting(&app).await;
    let mine = list["requests"].as_array().unwrap().iter().find(|r| r["name"] == "Pixel 8").unwrap().clone();
    assert_eq!((&mine["from"], &mine["via"]), (&json!("192.168.0.42"), &Value::Null), "{mine}");
}
