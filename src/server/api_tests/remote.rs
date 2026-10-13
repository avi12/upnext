//! Pairing a browser, plain HTTP only from private
//! addresses, the per-token limits, `/api/v1`, "Allow other devices" and
//! HTTPS on the same port.

use super::*;
use crate::db::PlayerId;
use crate::server::listen::Peer;

const HERE: [u8; 4] = [127, 0, 0, 1];
const LAN: [u8; 4] = [192, 168, 0, 20];
const TAILNET: [u8; 4] = [100, 101, 102, 103];
const PUBLIC: [u8; 4] = [203, 0, 113, 9];

/// A request from `peer` (`tls`: over HTTPS), addressed to `host`.
#[derive(Default)]
struct Req<'a> {
    method: &'a str,
    uri: &'a str,
    host: &'a str,
    bearer: Option<&'a str>,
    cookie: Option<&'a str>,
    origin: Option<&'a str>,
    fetch_site: Option<&'a str>,
    body: Option<Value>,
    tls: bool,
}

fn req<'a>(method: &'a str, uri: &'a str) -> Req<'a> {
    Req { method, uri, host: "den.lan:8787", ..Default::default() }
}

struct Answer {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Value,
}

impl TestApp {
    async fn ask(&self, peer: [u8; 4], r: Req<'_>) -> Answer {
        let mut b = Request::builder().method(r.method).uri(r.uri).header(header::HOST, r.host);
        if let Some(t) = r.bearer {
            b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        if let Some(c) = r.cookie {
            b = b.header(header::COOKIE, format!("theme=arcade; {c}"));
        }
        if let Some(o) = r.origin {
            b = b.header(header::ORIGIN, o);
        }
        if let Some(s) = r.fetch_site {
            b = b.header("sec-fetch-site", s);
        }
        let body = match r.body {
            Some(v) => {
                b = b.header(header::CONTENT_TYPE, "application/json");
                Body::from(v.to_string())
            }
            None => Body::empty(),
        };
        let peer = Peer { addr: std::net::SocketAddr::from((peer, 50000)), tls: r.tls };
        let resp =
            self.router.clone().layer(axum::extract::connect_info::MockConnectInfo(peer)).oneshot(b.body(body).unwrap()).await.unwrap();
        let (status, headers) = (resp.status(), resp.headers().clone());
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let body = serde_json::from_slice(&bytes).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        Answer { status, headers, body }
    }

    /// A pairing code, made on this computer.
    async fn pairing_code(&self) -> Value {
        let a = self.ask(HERE, Req { host: "127.0.0.1:8787", ..req("POST", "/api/pairing") }).await;
        assert_eq!(a.status, StatusCode::CREATED, "{}", a.body);
        a.body
    }

    async fn pair(&self, peer: [u8; 4], code: &str, tls: bool) -> Answer {
        self.ask(peer, Req { body: Some(json!({ "code": code })), tls, ..req("POST", "/api/pair") }).await
    }
}

fn on_the_network() -> impl FnOnce(&mut Config) {
    |c| c.host = "0.0.0.0".into()
}

/// The token in a `Set-Cookie`, and the cookie's attributes.
fn set_cookie(headers: &axum::http::HeaderMap) -> (String, String) {
    let c = headers[header::SET_COOKIE].to_str().unwrap().to_string();
    let token = c.split(';').next().unwrap().strip_prefix(&format!("{DEVICE_COOKIE}=")).unwrap().to_string();
    (token, c)
}

#[tokio::test]
async fn a_paired_browser_uses_the_api() {
    let app = reconfigured(test_app().await, on_the_network());
    // The phone's browser has nothing yet: the UI loads, the API asks for pairing.
    assert_eq!(app.ask(LAN, req("GET", "/")).await.status, StatusCode::OK);
    let notices = app.ask(LAN, req("GET", "/third-party-notices.txt")).await;
    assert_eq!(notices.status, StatusCode::OK, "Settings › About's link, like the UI's own files");
    assert!(notices.body.as_str().is_some_and(|t| t.starts_with("UpNext: third-party notices")));
    let refused = app.ask(LAN, req("GET", "/api/v1/stats")).await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    assert!(refused.body["error"].as_str().unwrap().contains("Add a device"), "{}", refused.body);
    assert_eq!(app.ask(LAN, req("POST", "/api/pairing")).await.status, StatusCode::UNAUTHORIZED, "codes are made on this computer");

    // No code yet; then Settings, Add a device.
    assert_eq!(app.pair(LAN, "123456", false).await.status, StatusCode::NOT_FOUND);
    let shown = app.pairing_code().await;
    let code = shown["code"].as_str().unwrap().to_string();
    assert!(code.len() == 6 && code.chars().all(|c| c.is_ascii_digit()), "{shown}");
    assert!(shown["expires_at"].as_i64().unwrap() > crate::db::now() + 290);
    assert!(shown["qr_svg"].as_str().is_some_and(|s| s.contains("<svg")) || shown["address"].is_null(), "{shown}");
    let current = app.ask(HERE, Req { host: "127.0.0.1:8787", ..req("GET", "/api/v1/pairing") }).await;
    assert_eq!(current.body["code"], code.as_str());

    // A wrong code costs a try; the right one pairs.
    let wrong = if code == "000000" { "000001" } else { "000000" };
    let a = app.pair(LAN, wrong, false).await;
    assert_eq!(a.status, StatusCode::UNAUTHORIZED);
    assert!(a.body["error"].as_str().unwrap().contains("4 tries left"), "{}", a.body);
    let a = app.ask(LAN, Req { body: Some(json!({ "code": format!(" {code} ") })), ..req("POST", "/api/pair") }).await;
    assert_eq!(a.status, StatusCode::CREATED, "{}", a.body);
    let (token, attrs) = set_cookie(&a.headers);
    assert!(token.starts_with("upnext_"), "{attrs}");
    assert!(attrs.contains("HttpOnly") && attrs.contains("SameSite=Strict") && attrs.contains("Path=/"), "{attrs}");
    assert!(!attrs.contains("Secure"), "plain HTTP on the LAN: {attrs}");
    assert_eq!(app.pair(LAN, &code, false).await.status, StatusCode::NOT_FOUND, "a code works once");

    // The cookie does what a full token does, on /api/v1 and the old paths.
    let cookie = format!("{DEVICE_COOKIE}={token}");
    let with_cookie = |r: Req<'static>| Req { cookie: Some(cookie.as_str()), ..r };
    assert_eq!(app.ask(LAN, with_cookie(req("GET", "/api/v1/stats"))).await.status, StatusCode::OK);
    assert_eq!(app.ask(LAN, with_cookie(req("GET", "/api/stats"))).await.status, StatusCode::OK);
    let list = app.ask(HERE, Req { host: "127.0.0.1:8787", ..req("GET", "/api/tokens") }).await.body;
    let device = &list["tokens"][0];
    assert_eq!((device["kind"].as_str(), device["scope"].as_str()), (Some("device"), Some("full")), "{list}");
    assert_eq!(device["name"], "Browser", "named from the browser");

    // A write with the cookie must say where it came from.
    let later = |origin: Option<&'static str>, site: Option<&'static str>| {
        with_cookie(Req { origin, fetch_site: site, ..req("PUT", "/api/games/1/later") })
    };
    assert_eq!(app.ask(LAN, later(Some("http://den.lan:8787"), None)).await.status, StatusCode::OK);
    assert_eq!(app.ask(LAN, later(None, Some("same-origin"))).await.status, StatusCode::OK);
    assert_eq!(app.ask(LAN, later(None, None)).await.status, StatusCode::FORBIDDEN, "no origin at all");
    assert_eq!(app.ask(LAN, later(Some("http://evil.example"), None)).await.status, StatusCode::FORBIDDEN);

    // Neither the phone nor a full token can hand out more access: new
    // tokens and pairing codes are made on this computer only.
    let full = app.db.token_create("script", crate::db::Scope::Full, "token").unwrap().token;
    let site = Some("same-origin");
    for uri in ["/api/v1/pairing", "/api/v1/tokens", "/api/pairing", "/api/tokens"] {
        let body = Some(json!({ "name": "more", "scope": "full" }));
        let by_cookie = app.ask(LAN, with_cookie(Req { fetch_site: site, body: body.clone(), ..req("POST", uri) })).await;
        assert_eq!(by_cookie.status, StatusCode::FORBIDDEN, "{uri} with the cookie: {}", by_cookie.body);
        assert!(by_cookie.body["error"].as_str().unwrap().contains("only on the computer"), "{}", by_cookie.body);
        let by_token = app.ask(LAN, Req { bearer: Some(&full), body, ..req("POST", uri) }).await;
        assert_eq!(by_token.status, StatusCode::FORBIDDEN, "{uri} with a full token: {}", by_token.body);
    }
    // Settings knows, so it doesn't offer them there.
    let settings = app.ask(LAN, with_cookie(req("GET", "/api/v1/settings"))).await.body;
    assert_eq!(settings["listen"]["this_computer"], false, "{settings}");
    let settings = app.ask(HERE, Req { host: "127.0.0.1:8787", ..req("GET", "/api/v1/settings") }).await.body;
    assert_eq!(settings["listen"]["this_computer"], true, "{settings}");

    // Revoked: refused at once, and the cookie is cleared so the UI asks again.
    let uri = format!("/api/v1/tokens/{}", device["id"].as_i64().unwrap());
    let revoke = app.ask(HERE, Req { host: "127.0.0.1:8787", ..req("DELETE", &uri) }).await;
    assert_eq!(revoke.status, StatusCode::NO_CONTENT);
    let a = app.ask(LAN, with_cookie(req("GET", "/api/v1/stats"))).await;
    assert_eq!(a.status, StatusCode::UNAUTHORIZED);
    assert!(a.headers[header::SET_COOKIE].to_str().unwrap().contains("Max-Age=0"));
    assert!(a.body["error"].as_str().unwrap().contains("pair it again"), "{}", a.body);
}

/// Security review L7: the name a pairing browser sends keeps no escape
/// sequences, since `upnext token list` prints it in a terminal.
#[tokio::test]
async fn a_paired_device_name_keeps_no_control_characters() {
    let app = reconfigured(test_app().await, on_the_network());
    let code = app.pairing_code().await["code"].as_str().unwrap().to_string();
    let name = "evil\u{1b}]0;pwned\u{7}\u{1b}[2J";
    let a = app.ask(LAN, Req { body: Some(json!({ "code": code, "name": name })), ..req("POST", "/api/pair") }).await;
    assert_eq!(a.status, StatusCode::CREATED, "{}", a.body);
    assert_eq!(a.body["name"], "evil]0;pwned[2J");
    let list = app.ask(HERE, Req { host: "127.0.0.1:8787", ..req("GET", "/api/tokens") }).await.body;
    assert_eq!(list["tokens"][0]["name"], "evil]0;pwned[2J", "{list}");

    // Nothing left but control characters: named from the browser instead.
    let code = app.pairing_code().await["code"].as_str().unwrap().to_string();
    let a = app.ask(LAN, Req { body: Some(json!({ "code": code, "name": "\u{1b}\u{7}" })), ..req("POST", "/api/pair") }).await;
    assert_eq!((a.status, a.body["name"].as_str()), (StatusCode::CREATED, Some("Browser")), "{}", a.body);
}

/// The Android app pairs with a code too (from a QR code
/// or typed), and keeps the token itself instead of a cookie.
#[tokio::test]
async fn an_app_pairing_with_a_code_gets_the_token() {
    let app = reconfigured(test_app().await, on_the_network());
    let code = app.pairing_code().await["code"].as_str().unwrap().to_string();
    let body = json!({ "code": code, "name": "Pixel 9", "wants": "token" });
    let a = app.ask(LAN, Req { body: Some(body), ..req("POST", "/api/v1/pair") }).await;
    assert_eq!(a.status, StatusCode::CREATED, "{}", a.body);
    assert!(a.headers.get(header::SET_COOKIE).is_none(), "the app keeps the token, so no cookie");
    let token = a.body["token"].as_str().unwrap().to_string();
    assert!(token.starts_with("upnext_"), "{}", a.body);
    assert_eq!(a.body["name"], "Pixel 9");
    assert!(a.body["address"].is_null(), "no public_url: {}", a.body);
    let id = a.body["computer_id"].as_str().unwrap();
    assert!(!id.is_empty() && a.body["computer"].as_str().is_some_and(|n| !n.is_empty()), "{}", a.body);
    assert_eq!(app.ask(LAN, Req { bearer: Some(&token), ..req("GET", "/api/v1/stats") }).await.status, StatusCode::OK);
    // The same id the announcement and a request carry.
    let asked = app.ask(LAN, Req { body: Some(json!({ "wants": "token" })), ..req("POST", "/api/v1/pair/request") }).await;
    assert_eq!(asked.body["computer_id"], id, "{}", asked.body);

    // A browser's answer is as it was: the cookie, and no token in the body.
    let code = app.pairing_code().await["code"].as_str().unwrap().to_string();
    let a = app.pair(LAN, &code, false).await;
    assert_eq!(a.status, StatusCode::CREATED, "{}", a.body);
    assert!(a.headers.get(header::SET_COOKIE).is_some());
    let fields: Vec<&String> = a.body.as_object().unwrap().keys().collect();
    assert_eq!(fields, ["id", "name"], "{}", a.body);

    // With public_url, the app is told the name to use.
    let app = reconfigured(app, |c| c.public_url = Some("https://den.tailnet.ts.net".into()));
    let code = app.pairing_code().await["code"].as_str().unwrap().to_string();
    let body = json!({ "code": code, "wants": "token" });
    let a = app.ask(LAN, Req { host: "den.tailnet.ts.net", body: Some(body), ..req("POST", "/api/v1/pair") }).await;
    assert_eq!(a.status, StatusCode::CREATED, "{}", a.body);
    assert_eq!(a.body["address"], "https://den.tailnet.ts.net");
}

#[tokio::test]
async fn an_app_pairing_that_fails_leaves_the_code() {
    let app = reconfigured(test_app().await, on_the_network());
    let code = app.pairing_code().await["code"].as_str().unwrap().to_string();
    // The install id can't be saved, so the app's answer can't be made.
    let c = rusqlite::Connection::open(app.db.path()).unwrap();
    c.execute_batch(
        "DELETE FROM meta WHERE key = 'network_id';
         CREATE TRIGGER no_id BEFORE INSERT ON meta WHEN NEW.key = 'network_id' BEGIN SELECT RAISE(ABORT, 'no'); END;",
    )
    .unwrap();
    let body = json!({ "code": code, "name": "Pixel 9", "wants": "token" });
    let a = app.ask(LAN, Req { body: Some(body), ..req("POST", "/api/v1/pair") }).await;
    assert_eq!(a.status, StatusCode::INTERNAL_SERVER_ERROR, "{}", a.body);
    let listed = app.ask(HERE, Req { host: "127.0.0.1:8787", ..req("GET", "/api/tokens") }).await;
    assert!(!listed.body.to_string().contains("Pixel 9"), "no device was made: {}", listed.body);
    // The code is still waiting.
    let a = app.pair(LAN, &code, false).await;
    assert_eq!(a.status, StatusCode::CREATED, "{}", a.body);
}

#[tokio::test]
async fn codes_end_after_five_wrong_tries_or_five_minutes() {
    let app = reconfigured(test_app().await, on_the_network());
    let code = app.pairing_code().await["code"].as_str().unwrap().to_string();
    let wrong = if code == "999999" { "999998" } else { "999999" };
    for left in (0..5).rev() {
        let a = app.pair(LAN, wrong, false).await;
        assert_eq!(a.status, StatusCode::UNAUTHORIZED);
        let said = a.body["error"].as_str().unwrap().to_string();
        assert!(if left == 0 { said.contains("last try") } else { said.contains(&format!("{left} tr")) }, "{said}");
    }
    assert_eq!(app.pair(LAN, &code, false).await.status, StatusCode::NOT_FOUND, "the code is gone");

    let code = app.pairing_code().await["code"].as_str().unwrap().to_string();
    app.state.remote.pairing.expire();
    assert_eq!(app.pair(LAN, &code, false).await.status, StatusCode::NOT_FOUND, "five minutes on");
    let current = app.ask(HERE, Req { host: "127.0.0.1:8787", ..req("GET", "/api/pairing") }).await;
    assert_eq!(current.body, Value::Null);

    // A new code ends the last; a cancelled one is gone.
    let first = app.pairing_code().await["code"].as_str().unwrap().to_string();
    let second = app.pairing_code().await["code"].as_str().unwrap().to_string();
    if first != second {
        assert_eq!(app.pair(LAN, &first, false).await.status, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(app.ask(HERE, Req { host: "127.0.0.1:8787", ..req("DELETE", "/api/pairing") }).await.status, StatusCode::NO_CONTENT);
    assert_eq!(app.pair(LAN, &second, false).await.status, StatusCode::NOT_FOUND);

    // A read token can't see or make codes.
    let read = app.db.token_create("widget", crate::db::Scope::Read, "token").unwrap().token;
    assert_eq!(app.ask(LAN, Req { bearer: Some(&read), ..req("GET", "/api/pairing") }).await.status, StatusCode::FORBIDDEN);
    assert_eq!(app.ask(LAN, Req { bearer: Some(&read), ..req("POST", "/api/pairing") }).await.status, StatusCode::FORBIDDEN);
    // Nor a full token: a lost phone that read a waiting code could pair a
    // second device, one that outlives revoking the phone, or end it.
    let full = app.db.token_create("old phone", crate::db::Scope::Full, "device").unwrap().token;
    let code = app.pairing_code().await["code"].as_str().unwrap().to_string();
    for method in ["GET", "DELETE"] {
        let a = app.ask(LAN, Req { bearer: Some(&full), ..req(method, "/api/v1/pairing") }).await;
        assert_eq!(a.status, StatusCode::FORBIDDEN, "{method}: {}", a.body);
        assert!(!a.body.to_string().contains(&code), "{}", a.body);
    }
    let current = app.ask(HERE, Req { host: "127.0.0.1:8787", ..req("GET", "/api/pairing") }).await;
    assert_eq!(current.body["code"], code.as_str(), "still waiting");
}

#[tokio::test]
async fn plain_http_only_from_private_addresses() {
    let app = reconfigured(test_app().await, on_the_network());
    let full = app.db.token_create("script", crate::db::Scope::Full, "token").unwrap().token;
    let stats = |tls: bool| Req { bearer: Some(&full), tls, ..req("GET", "/api/v1/stats") };
    for peer in [LAN, TAILNET, HERE] {
        assert_eq!(app.ask(peer, stats(false)).await.status, StatusCode::OK, "{peer:?}");
    }
    let a = app.ask(PUBLIC, stats(false)).await;
    assert_eq!(a.status, StatusCode::FORBIDDEN);
    assert!(a.body["error"].as_str().unwrap().contains("plain HTTP"), "{}", a.body);
    assert_eq!(app.ask(PUBLIC, stats(true)).await.status, StatusCode::OK, "over HTTPS");

    // Pairing from a public address too, and its cookie is Secure over HTTPS.
    let code = app.pairing_code().await["code"].as_str().unwrap().to_string();
    assert_eq!(app.pair(PUBLIC, &code, false).await.status, StatusCode::FORBIDDEN);
    let a = app.pair(PUBLIC, &code, true).await;
    assert_eq!(a.status, StatusCode::CREATED, "{}", a.body);
    assert!(set_cookie(&a.headers).1.ends_with("; Secure"));
    // Health needs nothing, from anywhere.
    assert_eq!(app.ask(PUBLIC, req("GET", "/api/v1/health")).await.status, StatusCode::OK);
}

#[tokio::test]
async fn tokens_are_limited_this_computer_is_not() {
    let app = reconfigured(test_app().await, on_the_network());
    let made = app.db.token_create("script", crate::db::Scope::Full, "token").unwrap();
    let full = made.token.clone();

    // A second pick while one runs: 429, with when to try again.
    let held =
        app.state.remote.limits.ai_call(crate::server::limits::Key::Token(made.info.id), crate::server::limits::AiCall::Pick).unwrap();
    let pick = || Req { bearer: Some(&full), body: Some(json!({ "prompt": "short", "count": 1 })), ..req("POST", "/api/v1/picks") };
    let a = app.ask(LAN, pick()).await;
    assert_eq!(a.status, StatusCode::TOO_MANY_REQUESTS, "{}", a.body);
    assert!(a.body["error"].as_str().unwrap().contains("a pick running"), "{}", a.body);
    assert!(a.headers[header::RETRY_AFTER].to_str().unwrap().parse::<u64>().unwrap() >= 1);
    let local = app
        .ask(HERE, Req { host: "127.0.0.1:8787", body: Some(json!({ "prompt": "short", "count": 1 })), ..req("POST", "/api/picks") })
        .await;
    assert_eq!(local.status, StatusCode::OK, "this computer has no limit: {}", local.body);
    drop(held);
    assert_eq!(app.ask(LAN, pick()).await.status, StatusCode::OK, "free once the first is answered");

    // About ten a second, in bursts of twenty.
    let read = app.db.token_create("dashboard", crate::db::Scope::Read, "token").unwrap().token;
    let mut statuses = vec![];
    for _ in 0..40 {
        statuses.push(app.ask(LAN, Req { bearer: Some(&read), ..req("GET", "/api/v1/health") }).await.status);
    }
    // Health is open to anyone, so it isn't counted against the token…
    assert!(statuses.iter().all(|s| *s == StatusCode::OK));
    let mut limited = None;
    for i in 0..40 {
        let a = app.ask(LAN, Req { bearer: Some(&read), ..req("GET", "/api/v1/stats") }).await;
        if a.status == StatusCode::TOO_MANY_REQUESTS {
            limited = Some((i, a.headers[header::RETRY_AFTER].clone()));
            break;
        }
    }
    let (at, retry) = limited.expect("…but the API is");
    assert!((19..=30).contains(&at), "after a burst of about 20: {at}");
    assert_eq!(retry, "1");
}

#[tokio::test]
async fn v1_and_the_old_paths_answer_the_same() {
    let app = test_app().await;
    for path in ["stats", "games?limit=2", "settings", "jobs", "picks/history"] {
        assert_eq!(app.get(&format!("/api/v1/{path}")).await, app.get(&format!("/api/{path}")).await, "{path}");
    }
    let (status, _) = app.call("POST", "/api/v1/gog/window", None).await;
    assert_ne!(status, StatusCode::NOT_FOUND, "routes are the same");
    // Local-only rules see through the version.
    let full = app.db.token_create("x", crate::db::Scope::Full, "token").unwrap().token;
    let app = reconfigured(app, on_the_network());
    assert_eq!(app.ask(LAN, Req { bearer: Some(&full), ..req("POST", "/api/v1/gog/window") }).await.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn loopback_origins_must_be_this_port() {
    let app = test_app().await;
    let merge = |origin: &'static str| Req {
        host: "127.0.0.1:8787",
        origin: Some(origin),
        body: Some(json!({ "ids": [1] })),
        ..req("POST", "/api/titles/merge")
    };
    // Another program's page on this computer is another site.
    for origin in ["http://localhost:3000", "http://127.0.0.1:8788", "http://localhost"] {
        assert_eq!(app.ask(HERE, merge(origin)).await.status, StatusCode::FORBIDDEN, "{origin}");
    }
    for origin in ["http://localhost:8787", "http://127.0.0.1:8787", "http://[::1]:8787"] {
        assert_eq!(app.ask(HERE, merge(origin)).await.status, StatusCode::BAD_REQUEST, "{origin} reaches the handler");
    }
    // The Vite dev server's proxy keeps the browser's Host, so ports match.
    let dev = Req { host: "localhost:5173", ..merge("http://localhost:5173") };
    assert_eq!(app.ask(HERE, dev).await.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn other_devices_switch_and_server_files() {
    let app = test_app().await;
    let settings = app.get("/api/v1/settings").await;
    assert_eq!(settings["allow_other_devices"], false);
    assert_eq!(settings["listen"]["other_devices"], false);
    assert_eq!(settings["listen"]["forced"], false);

    let (status, v) = app.call("PUT", "/api/v1/settings", Some(json!({ "allow_other_devices": true }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!((v["listen"]["host"].as_str(), v["listen"]["other_devices"].as_bool()), (Some("0.0.0.0"), Some(true)));

    // A paired phone can turn it off but can't name files on the server.
    let full = app.db.token_create("phone", crate::db::Scope::Full, "device").unwrap().token;
    let put =
        |body: Value| Req { bearer: Some(&full), origin: Some("http://den.lan:8787"), body: Some(body), ..req("PUT", "/api/v1/settings") };
    for key in ["tls_cert", "tls_key", "galaxy_db"] {
        let a = app.ask(LAN, put(Value::Object([(key.to_string(), json!("/tmp/x.pem"))].into_iter().collect()))).await;
        assert_eq!(a.status, StatusCode::FORBIDDEN, "{key}: {}", a.body);
    }
    // Nor change how UpNext runs on it: the Run key, and staying in the tray.
    for key in ["start_with_windows", "keep_running"] {
        let a = app.ask(LAN, put(Value::Object([(key.to_string(), json!(false))].into_iter().collect()))).await;
        assert_eq!(a.status, StatusCode::FORBIDDEN, "{key}: {}", a.body);
    }
    let body = json!({ "keep_running": false });
    let a = app.ask(HERE, Req { host: "127.0.0.1:8787", body: Some(body), ..req("PUT", "/api/v1/settings") }).await;
    assert_eq!((a.status, &a.body["keep_running"]), (StatusCode::OK, &json!(false)), "this computer can: {}", a.body);
    let body = json!({ "tls_cert": "/tmp/c.pem", "tls_key": "/tmp/k.pem" });
    let a = app.ask(HERE, Req { host: "127.0.0.1:8787", body: Some(body), ..req("PUT", "/api/v1/settings") }).await;
    assert_eq!(a.status, StatusCode::OK, "this computer can: {}", a.body);
    assert_eq!(a.body["tls_cert"], "/tmp/c.pem");
    let a = app.ask(LAN, put(json!({ "allow_other_devices": false }))).await;
    assert_eq!(a.status, StatusCode::OK, "{}", a.body);
    assert_eq!(a.body["listen"]["other_devices"], false);

    // A container's UPNEXT_HOST listens on the network whatever the switch says.
    let app = reconfigured(app, on_the_network());
    let v = app.ask(HERE, Req { host: "127.0.0.1:8787", ..req("GET", "/api/settings") }).await.body;
    assert_eq!((v["listen"]["other_devices"].as_bool(), v["listen"]["forced"].as_bool()), (Some(true), Some(true)));
}

/// The real listener: HTTPS and plain HTTP on one port, and the switch
/// moving it to every address and back while it runs.
#[tokio::test]
async fn https_and_http_share_the_port_and_the_switch_moves_it() {
    let dir = tempfile::tempdir().unwrap();
    let made = rcgen::generate_simple_self_signed(vec!["localhost".to_string(), "127.0.0.1".to_string()]).unwrap();
    let (cert, key) = (dir.path().join("cert.pem"), dir.path().join("key.pem"));
    std::fs::write(&cert, made.cert.pem()).unwrap();
    std::fs::write(&key, made.signing_key.serialize_pem()).unwrap();

    let app = test_app().await;
    app.db.setting_set(PlayerId::FIRST, "tls_cert", Some(&cert.display().to_string())).unwrap();
    app.db.setting_set(PlayerId::FIRST, "tls_key", Some(&key.display().to_string())).unwrap();
    let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = socket.local_addr().unwrap().port();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_until(socket, app.state.clone(), async {
        let _ = stopped.await;
    }));

    let client = reqwest::Client::builder()
        .add_root_certificate(reqwest::Certificate::from_pem(made.cert.pem().as_bytes()).unwrap())
        .no_proxy()
        .build()
        .unwrap();
    let get = |url: String| {
        let client = client.clone();
        async move {
            let mut tries = 0;
            loop {
                match client.get(&url).send().await {
                    Ok(r) => return r,
                    Err(e) if tries < 50 => {
                        tries += 1;
                        let _ = e;
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    }
                    Err(e) => panic!("{url}: {e:#}"),
                }
            }
        }
    };
    let https = get(format!("https://localhost:{port}/api/v1/settings")).await;
    assert_eq!(https.status(), 200, "this computer over HTTPS needs no token");
    let settings: Value = https.json().await.unwrap();
    assert_eq!(settings["listen"]["https"], true, "{}", settings["listen"]);
    assert_eq!(settings["listen"]["error"], Value::Null);
    assert_eq!(get(format!("http://127.0.0.1:{port}/api/health")).await.status(), 200, "plain HTTP on the same port");

    // A paired browser over HTTPS gets a Secure cookie.
    let code: Value = client.post(format!("https://localhost:{port}/api/pairing")).send().await.unwrap().json().await.unwrap();
    let paired = client.post(format!("https://localhost:{port}/api/pair")).json(&json!({ "code": code["code"] })).send().await.unwrap();
    assert_eq!(paired.status(), 201);
    assert!(paired.headers()[header::SET_COOKIE].to_str().unwrap().ends_with("; Secure"));

    // "Allow other devices": the same port, on every address; then back.
    let put = |on: bool| client.put(format!("https://localhost:{port}/api/v1/settings")).json(&json!({ "allow_other_devices": on })).send();
    let v: Value = put(true).await.unwrap().json().await.unwrap();
    assert_eq!(v["listen"]["host"], "0.0.0.0", "{}", v["listen"]);
    assert_eq!(v["listen"]["port"], port);
    let v: Value = get(format!("http://127.0.0.1:{port}/api/settings")).await.json().await.unwrap();
    assert_eq!(v["listen"]["other_devices"], true);
    let v: Value = client
        .put(format!("http://127.0.0.1:{port}/api/v1/settings"))
        .json(&json!({ "allow_other_devices": false }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["listen"]["host"], "127.0.0.1", "{}", v["listen"]);

    // A certificate that doesn't load says why, and plain HTTP carries on.
    app.db.setting_set(PlayerId::FIRST, "tls_key", None).unwrap();
    let v: Value = client
        .put(format!("http://127.0.0.1:{port}/api/v1/settings"))
        .json(&json!({ "tls_cert": cert.display().to_string() }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["listen"]["https"], false);
    assert!(v["listen"]["error"].as_str().unwrap().contains("both"), "{}", v["listen"]);

    drop(client);
    let _ = stop.send(());
    tokio::time::timeout(std::time::Duration::from_secs(30), server).await.unwrap().unwrap().unwrap();
}

/// In UpNext's container image nobody is this computer. The host's
/// browser arrives from the Docker bridge's address, naming localhost. The
/// server's startup greeting makes an hour-long code it pairs with; once a
/// browser is paired, the greeting only says how to get another code.
#[tokio::test]
async fn a_container_logs_its_first_pairing_code() {
    const BRIDGE: [u8; 4] = [172, 17, 0, 1];
    let app = reconfigured(test_app().await, |c| {
        c.host = "0.0.0.0".into();
        c.container = true;
    });
    fn from_host(r: Req<'_>) -> Req<'_> {
        Req { host: "localhost:8787", ..r }
    }
    let health = app.ask(BRIDGE, from_host(req("GET", "/api/v1/health"))).await;
    assert_eq!((health.status, &health.body["container"]), (StatusCode::OK, &json!(true)));
    let refused = app.ask(BRIDGE, from_host(req("GET", "/api/v1/stats"))).await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    assert!(refused.body["error"].as_str().unwrap().contains("docker logs upnext"), "{}", refused.body);
    assert_eq!(app.ask(BRIDGE, from_host(req("POST", "/api/v1/pairing"))).await.status, StatusCode::UNAUTHORIZED);

    let greeting = crate::server::pairing::container_greeting(&app.state).unwrap();
    let found = regex::Regex::new(r"Open http://localhost:8787 and enter (\d{3}) (\d{3}) to pair").unwrap();
    let caps = found.captures(&greeting).unwrap_or_else(|| panic!("{greeting}"));
    let code = format!("{} {}", &caps[1], &caps[2]);
    assert!(greeting.contains("within the next 60 minutes") && greeting.contains("docker exec upnext upnext pair"), "{greeting}");
    let current = app.ask(HERE, Req { host: "127.0.0.1:8787", ..req("GET", "/api/v1/pairing") }).await.body;
    assert!(current["address"].is_null(), "the container's own address is no use outside it: {current}");
    assert!(current["expires_at"].as_i64().unwrap() > crate::db::now() + 3500, "{current}");

    // The host's browser types it as shown, with the space.
    let body = Some(json!({ "code": code }));
    let a = app.ask(BRIDGE, from_host(Req { body, origin: Some("http://localhost:8787"), ..req("POST", "/api/v1/pair") })).await;
    assert_eq!(a.status, StatusCode::CREATED, "{}", a.body);
    let (token, _) = set_cookie(&a.headers);
    let cookie = format!("{DEVICE_COOKIE}={token}");
    let paired = |r: Req<'static>| from_host(Req { cookie: Some(cookie.as_str()), ..r });
    assert_eq!(app.ask(BRIDGE, paired(req("GET", "/api/v1/stats"))).await.status, StatusCode::OK);
    let write = paired(Req { origin: Some("http://localhost:8787"), ..req("PUT", "/api/v1/games/1/later") });
    assert_eq!(app.ask(BRIDGE, write).await.status, StatusCode::OK);
    let settings = app.ask(BRIDGE, paired(req("GET", "/api/v1/settings"))).await.body;
    assert_eq!((&settings["listen"]["container"], &settings["listen"]["this_computer"]), (&json!(true), &json!(false)), "{settings}");
    assert!(settings["heroic_dir"].is_null() && settings["steam_local_dir"].is_null(), "nothing mounted: {settings}");

    // Paired: the next start makes no code, and says how to get one.
    let again = crate::server::pairing::container_greeting(&app.state).unwrap();
    assert!(!found.is_match(&again) && again.contains("docker exec upnext upnext pair"), "{again}");
    let current = app.ask(HERE, Req { host: "127.0.0.1:8787", ..req("GET", "/api/v1/pairing") }).await.body;
    assert!(current.is_null(), "{current}");
}

/// B-AUTH-4 (code health review 2026-10-11): browsers send a host's cookies
/// to every port on it, so the paired-browser cookie takes the server's
/// port, as the player cookie does. Another UpNext's cookie under the old
/// name, `upnext_device`, is left alone, not refused and cleared: a phone
/// paired with two UpNexts on den.lan keeps both. The old name still works
/// for a token this server made, as the Android app sets it.
#[tokio::test]
async fn two_upnexts_on_one_host_keep_their_own_paired_browsers() {
    let app = reconfigured(test_app().await, on_the_network());
    let code = app.pairing_code().await["code"].as_str().unwrap().to_string();
    let paired = app.pair(LAN, &code, false).await;
    assert_eq!(paired.status, StatusCode::CREATED, "{}", paired.body);
    let (token, attrs) = set_cookie(&paired.headers);
    assert!(attrs.starts_with(&format!("{DEVICE_COOKIE}=upnext_")), "{attrs}");
    let stats = |cookie: String| {
        let app = &app;
        async move { app.ask(LAN, Req { cookie: Some(&cookie), ..req("GET", "/api/v1/stats") }).await }
    };

    // The other UpNext's cookies, under its port's name and the old one.
    let theirs = "upnext_0123456789abcdef0123456789abcdef01234567";
    for cookie in [format!("upnext_device_8788={theirs}"), format!("upnext_device={theirs}")] {
        let both = format!("{cookie}; {DEVICE_COOKIE}={token}");
        assert_eq!(stats(both).await.status, StatusCode::OK, "{cookie}: this server's own cookie is read");
        let alone = stats(cookie.clone()).await;
        assert_eq!(alone.status, StatusCode::UNAUTHORIZED, "{cookie}: {}", alone.body);
        assert!(alone.headers.get(header::SET_COOKIE).is_none(), "{cookie}: not cleared, it isn't ours");
        assert!(alone.body["error"].as_str().unwrap().contains("Add a device"), "{}", alone.body);
    }
    // On this computer, another UpNext's old cookie stops nothing.
    let here = app
        .ask(HERE, Req { host: "127.0.0.1:8787", cookie: Some(&format!("upnext_device={theirs}")), ..req("GET", "/api/v1/stats") })
        .await;
    assert_eq!(here.status, StatusCode::OK, "{}", here.body);

    // The old name with this server's token works, as the Android app sends it.
    assert_eq!(stats(format!("upnext_device={token}")).await.status, StatusCode::OK);
    // Under this server's own name, an unknown token is refused and cleared.
    let revoked = stats(format!("{DEVICE_COOKIE}={theirs}")).await;
    assert_eq!(revoked.status, StatusCode::UNAUTHORIZED);
    let cleared = revoked.headers[header::SET_COOKIE].to_str().unwrap();
    assert!(cleared.starts_with(&format!("{DEVICE_COOKIE}=;")) && cleared.contains("Max-Age=0"), "{cleared}");
}
