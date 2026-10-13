//! Who a request is for (a bound token, the
//! `UpNext-Player` header, the signed cookie, the default player), what a
//! bound token or device can't do, PINs and their lockout, removing a
//! player, and each player's own marks, jobs and AI usage.

use super::*;
use crate::db::{Failure, JobProgress, JobSource, Level, LogLine};
use crate::server::listen::Peer;

pub(super) const HERE: [u8; 4] = [127, 0, 0, 1];
pub(super) const LAN: [u8; 4] = [192, 168, 0, 20];

/// A request from `peer`, with a bearer token, a cookie and other headers.
#[derive(Default)]
pub(super) struct Req<'a> {
    pub(super) method: &'a str,
    pub(super) uri: &'a str,
    pub(super) bearer: Option<&'a str>,
    pub(super) cookie: Option<&'a str>,
    pub(super) player: Option<&'a str>,
    pub(super) body: Option<Value>,
}

pub(super) fn req<'a>(method: &'a str, uri: &'a str) -> Req<'a> {
    Req { method, uri, ..Default::default() }
}

pub(super) struct Answer {
    pub(super) status: StatusCode,
    pub(super) headers: axum::http::HeaderMap,
    pub(super) body: Value,
}

pub(super) async fn send(app: &TestApp, peer: [u8; 4], r: Req<'_>) -> Answer {
    let host = if peer == HERE { "127.0.0.1:8787" } else { "den.lan:8787" };
    let mut b = Request::builder().method(r.method).uri(r.uri).header(header::HOST, host);
    if let Some(t) = r.bearer {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    if let Some(c) = r.cookie {
        b = b.header(header::COOKIE, c);
    }
    if let Some(p) = r.player {
        b = b.header("UpNext-Player", p);
    }
    let body = match r.body {
        Some(v) => {
            b = b.header(header::CONTENT_TYPE, "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let peer = Peer { addr: std::net::SocketAddr::from((peer, 50000)), tls: false };
    let resp = app.router.clone().layer(axum::extract::connect_info::MockConnectInfo(peer)).oneshot(b.body(body).unwrap()).await.unwrap();
    let (status, headers) = (resp.status(), resp.headers().clone());
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let body = serde_json::from_slice(&bytes).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    Answer { status, headers, body }
}

/// A GET from this computer that must answer 200.
async fn here(app: &TestApp, r: Req<'_>) -> Value {
    let uri = r.uri.to_string();
    let a = send(app, HERE, Req { method: "GET", ..r }).await;
    assert_eq!(a.status, StatusCode::OK, "GET {uri}: {}", a.body);
    a.body
}

/// A title's id, by its name.
async fn title(app: &TestApp, name: &str) -> i64 {
    let uri = format!("/api/games?q={}", name.replace(' ', "+"));
    here(app, req("GET", &uri)).await["items"][0]["id"].as_i64().unwrap()
}

/// The app on the network, with a second player, Sam.
pub(super) async fn household() -> (TestApp, i64) {
    let app = reconfigured(test_app().await, |c| c.host = "0.0.0.0".into());
    let a = send(&app, HERE, Req { body: Some(json!({ "name": "Sam" })), ..req("POST", "/api/players") }).await;
    assert_eq!(a.status, StatusCode::CREATED, "{}", a.body);
    let sam = a.body["id"].as_i64().unwrap();
    (app, sam)
}

/// Who `GET /players/current` says the request is for, and whether bound.
async fn current(app: &TestApp, peer: [u8; 4], r: Req<'_>) -> (StatusCode, Option<i64>, bool) {
    let a = send(app, peer, Req { method: "GET", uri: "/api/players/current", ..r }).await;
    (a.status, a.body["player"]["id"].as_i64(), a.body["bound"] == true)
}

pub(super) async fn token(app: &TestApp, scope: &str, player: Option<i64>) -> String {
    let body = json!({ "name": format!("script {scope} {player:?}"), "scope": scope, "player_id": player });
    let a = send(app, HERE, Req { body: Some(body), ..req("POST", "/api/tokens") }).await;
    assert_eq!(a.status, StatusCode::CREATED, "{}", a.body);
    a.body["token"].as_str().unwrap().to_string()
}

/// The `name=value` a response's `Set-Cookie` sets, and the whole header.
fn cookie(headers: &axum::http::HeaderMap) -> (String, String) {
    let c = headers[header::SET_COOKIE].to_str().unwrap().to_string();
    (c.split(';').next().unwrap().to_string(), c)
}

#[tokio::test]
async fn a_binding_beats_the_header_which_beats_the_cookie_which_beats_the_default() {
    let (app, sam) = household().await;
    let me = || req("GET", "/api/players/current");
    assert_eq!(current(&app, HERE, me()).await, (StatusCode::OK, Some(1), false), "the default player");
    let sam_s = sam.to_string();
    assert_eq!(current(&app, HERE, Req { player: Some(&sam_s), ..me() }).await.1, Some(sam), "the header");
    for bad in ["sam", "999"] {
        let a = send(&app, HERE, Req { player: Some(bad), ..me() }).await;
        assert_eq!(a.status, StatusCode::BAD_REQUEST, "{bad}: {}", a.body);
    }

    // Choosing Sam in this browser sets the signed cookie.
    let chose = send(&app, HERE, Req { body: Some(json!({ "player_id": sam })), ..req("PUT", "/api/players/current") }).await;
    assert_eq!(chose.status, StatusCode::OK, "{}", chose.body);
    let (sam_cookie, whole) = cookie(&chose.headers);
    assert!(whole.contains("HttpOnly") && whole.contains("SameSite=Strict"), "{whole}");
    assert_eq!(current(&app, HERE, Req { cookie: Some(&sam_cookie), ..me() }).await.1, Some(sam), "the cookie");
    assert_eq!(current(&app, HERE, Req { cookie: Some(&sam_cookie), player: Some("1"), ..me() }).await.1, Some(1), "the header first");

    // A cookie that isn't ours, or names a removed player, is cleared.
    let (name, _) = sam_cookie.split_once('=').unwrap();
    let forged = format!("{name}={sam}.00ff00ff00ff00ff00ff00ff.{}", "0".repeat(64));
    let a = send(&app, HERE, Req { cookie: Some(&forged), ..me() }).await;
    assert_eq!(a.body["player"]["id"], 1, "{}", a.body);
    assert!(cookie(&a.headers).1.contains("Max-Age=0"), "the forged cookie is ended");

    // A token bound to Sam acts only for Sam.
    let bound = token(&app, "full", Some(sam)).await;
    assert_eq!(current(&app, LAN, Req { bearer: Some(&bound), ..me() }).await, (StatusCode::OK, Some(sam), true));
    assert_eq!(current(&app, LAN, Req { bearer: Some(&bound), player: Some(&sam_s), ..me() }).await.1, Some(sam));
    let other = send(&app, LAN, Req { bearer: Some(&bound), player: Some("1"), ..me() }).await;
    assert_eq!(other.status, StatusCode::FORBIDDEN, "{}", other.body);
    // An unbound token follows the header, as a browser does.
    let free = token(&app, "read", None).await;
    assert_eq!(current(&app, LAN, Req { bearer: Some(&free), player: Some(&sam_s), ..me() }).await.1, Some(sam));
    assert_eq!(current(&app, LAN, Req { bearer: Some(&free), ..me() }).await.1, Some(1));
}

#[tokio::test]
async fn a_bound_token_keeps_to_its_own_player() {
    let (app, sam) = household().await;
    let bound = token(&app, "full", Some(sam)).await;
    let as_sam = |method, uri, body| Req { bearer: Some(&bound), body, ..req(method, uri) };
    for (method, uri, body) in [
        ("POST", "/api/players", json!({ "name": "Kit" })),
        ("PUT", "/api/players/1", json!({ "name": "Alex" })),
        ("PUT", "/api/players/current", json!({ "player_id": 1 })),
        ("PUT", "/api/jobs/scheduled/wishlist", json!({ "enabled": true })),
        ("PUT", "/api/settings", json!({ "auto_sync": false })),
        ("PUT", "/api/setup", json!({ "setup": "done" })),
        ("GET", "/api/export", json!({})),
    ] {
        let a = send(&app, LAN, as_sam(method, uri, Some(body))).await;
        assert_eq!(a.status, StatusCode::FORBIDDEN, "{method} {uri}: {}", a.body);
    }
    // Sam's own settings and PIN are Sam's to change.
    let own = send(&app, LAN, as_sam("PUT", "/api/settings", Some(json!({ "sampled_minutes": "45" })))).await;
    assert_eq!(own.status, StatusCode::OK, "{}", own.body);
    let uri = format!("/api/players/{sam}/pin");
    let pin = send(&app, LAN, as_sam("PUT", &uri, Some(json!({ "new_pin": "2468" })))).await;
    assert_eq!(pin.status, StatusCode::NO_CONTENT, "{}", pin.body);
    // Nobody else's.
    let theirs = send(&app, LAN, as_sam("PUT", "/api/players/1/pin", Some(json!({ "new_pin": "1357" })))).await;
    assert_eq!(theirs.status, StatusCode::FORBIDDEN, "{}", theirs.body);
    assert_eq!(send(&app, LAN, as_sam("GET", "/api/settings", None)).await.body["sampled_minutes"], 45);
    assert_ne!(here(&app, req("GET", "/api/settings")).await["sampled_minutes"], 45, "player 1's is untouched");
}

#[tokio::test]
async fn pins_lock_after_five_wrong_tries_and_a_pinned_default_needs_choosing() {
    let (app, sam) = household().await;
    let sam_s = sam.to_string();
    let set = send(
        &app,
        HERE,
        Req { player: Some(&sam_s), body: Some(json!({ "new_pin": "2468" })), ..req("PUT", &format!("/api/players/{sam}/pin")) },
    )
    .await;
    assert_eq!(set.status, StatusCode::NO_CONTENT, "{}", set.body);
    // The header can't act for a player with a PIN.
    assert_eq!(send(&app, HERE, Req { player: Some(&sam_s), ..req("GET", "/api/games") }).await.status, StatusCode::FORBIDDEN);

    let choose = |pin: &str| Req { body: Some(json!({ "player_id": sam, "pin": pin })), ..req("PUT", "/api/players/current") };
    for _ in 0..4 {
        assert_eq!(send(&app, HERE, choose("0000")).await.status, StatusCode::FORBIDDEN);
    }
    assert_eq!(send(&app, HERE, choose("0000")).await.status, StatusCode::TOO_MANY_REQUESTS, "the fifth wrong one locks it");
    let locked = send(&app, HERE, choose("2468")).await;
    assert_eq!(locked.status, StatusCode::TOO_MANY_REQUESTS, "even the right PIN waits: {}", locked.body);

    // With a PIN on the default player, a request that names nobody can't
    // act for anyone until someone is chosen.
    let pin1 = send(&app, HERE, Req { body: Some(json!({ "new_pin": "1357" })), ..req("PUT", "/api/players/1/pin") }).await;
    assert_eq!(pin1.status, StatusCode::NO_CONTENT, "{}", pin1.body);
    let nobody = send(&app, HERE, req("GET", "/api/games")).await;
    assert_eq!((nobody.status, nobody.body["error"].as_str()), (StatusCode::FORBIDDEN, Some(crate::server::players::CHOOSE)));
    assert_eq!(current(&app, HERE, req("GET", "")).await.1, None);
    let chose = send(&app, HERE, Req { body: Some(json!({ "player_id": 1, "pin": "1357" })), ..req("PUT", "/api/players/current") }).await;
    assert_eq!(chose.status, StatusCode::OK, "{}", chose.body);
    let (c, _) = cookie(&chose.headers);
    assert_eq!(send(&app, HERE, Req { cookie: Some(&c), ..req("GET", "/api/games") }).await.status, StatusCode::OK);
}

/// `n` copies of one request from this computer, sent at once on a
/// runtime with several threads, and their answers.
async fn at_once(app: &Arc<TestApp>, n: usize, method: &'static str, uri: &'static str, body: Value) -> Vec<Answer> {
    let barrier = Arc::new(tokio::sync::Barrier::new(n));
    let tasks: Vec<_> = (0..n)
        .map(|_| {
            let (app, barrier, body) = (app.clone(), barrier.clone(), body.clone());
            tokio::spawn(async move {
                barrier.wait().await;
                send(&app, HERE, Req { body: Some(body), ..req(method, uri) }).await
            })
        })
        .collect();
    let mut out = vec![];
    for t in tasks {
        out.push(t.await.unwrap());
    }
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn wrong_pins_sent_at_once_still_lock_at_five() {
    let (app, sam) = household().await;
    let (app, sam_s) = (Arc::new(app), sam.to_string());
    let uri = format!("/api/players/{sam}/pin");
    let set = send(&app, HERE, Req { player: Some(&sam_s), body: Some(json!({ "new_pin": "2468" })), ..req("PUT", &uri) }).await;
    assert_eq!(set.status, StatusCode::NO_CONTENT, "{}", set.body);
    let answers = at_once(&app, 30, "PUT", "/api/players/current", json!({ "player_id": sam, "pin": "0000" })).await;
    let count = |status| answers.iter().filter(|a| a.status == status).count();
    assert_eq!(
        (count(StatusCode::FORBIDDEN), count(StatusCode::TOO_MANY_REQUESTS)),
        (4, 26),
        "four wrong PINs, then the lock: {:?}",
        answers.iter().map(|a| a.status).collect::<Vec<_>>()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn first_player_cookies_made_at_once_all_hold() {
    let (app, sam) = household().await;
    let app = Arc::new(app);
    assert_eq!(app.db.meta_get(crate::db::PLAYER_COOKIE_KEY).unwrap(), None, "no cookie has been signed yet");
    let answers = at_once(&app, 16, "PUT", "/api/players/current", json!({ "player_id": sam })).await;
    for a in &answers {
        assert_eq!(a.status, StatusCode::OK, "{}", a.body);
        let (c, _) = cookie(&a.headers);
        let (status, player, _) = current(&app, HERE, Req { cookie: Some(&c), ..req("GET", "") }).await;
        assert_eq!((status, player), (StatusCode::OK, Some(sam)), "a cookie signed with a key that lost the race");
    }
}

#[tokio::test]
async fn a_bound_token_sees_and_revokes_only_itself() {
    let (app, sam) = household().await;
    let household_token = token(&app, "full", None).await;
    let bound = token(&app, "full", Some(sam)).await;
    let as_sam = |method, uri| Req { bearer: Some(&bound), ..req(method, uri) };
    let list = send(&app, LAN, as_sam("GET", "/api/tokens")).await;
    assert_eq!(list.status, StatusCode::OK, "{}", list.body);
    let tokens = list.body["tokens"].as_array().unwrap();
    assert_eq!(tokens.len(), 1, "only itself: {}", list.body);
    assert_eq!(tokens[0]["id"], list.body["current"]);
    let own = list.body["current"].as_i64().unwrap();
    // The household's token, and every token, as this computer lists them.
    let all = here(&app, req("GET", "/api/tokens")).await;
    assert_eq!(all["tokens"].as_array().unwrap().len(), 2);
    let other = all["tokens"].as_array().unwrap().iter().find(|t| t["id"] != own).unwrap()["id"].as_i64().unwrap();
    let (revoke_other, revoke_own) = (format!("/api/tokens/{other}"), format!("/api/tokens/{own}"));
    let refused = send(&app, LAN, as_sam("DELETE", &revoke_other)).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
    let unbound = Req { bearer: Some(&household_token), ..req("GET", "/api/games") };
    assert_eq!(send(&app, LAN, unbound).await.status, StatusCode::OK, "the household's token still works");
    assert_eq!(send(&app, LAN, as_sam("DELETE", &revoke_own)).await.status, StatusCode::NO_CONTENT, "it can sign itself out");
    assert_eq!(send(&app, LAN, as_sam("GET", "/api/games")).await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn removing_a_player_is_this_computers_and_backs_up_first() {
    let (app, sam) = household().await;
    let full = token(&app, "full", None).await;
    let uri = format!("/api/players/{sam}");
    let away = send(&app, LAN, Req { bearer: Some(&full), body: Some(json!({})), ..req("DELETE", &uri) }).await;
    assert_eq!(away.status, StatusCode::FORBIDDEN, "{}", away.body);
    let bound = token(&app, "full", Some(sam)).await;

    let gone = send(&app, HERE, Req { body: Some(json!({})), ..req("DELETE", &uri) }).await;
    assert_eq!(gone.status, StatusCode::OK, "{}", gone.body);
    assert_eq!(gone.body["removed"], "Sam");
    let backup = gone.body["backup"].as_str().unwrap();
    let backups = here(&app, req("GET", "/api/backups")).await.to_string();
    assert!(backups.contains(backup), "{backup} isn't among {backups}");
    assert_eq!(here(&app, req("GET", "/api/players")).await["players"].as_array().unwrap().len(), 1);
    // Sam's token went with them.
    assert_eq!(send(&app, LAN, Req { bearer: Some(&bound), ..req("GET", "/api/games") }).await.status, StatusCode::UNAUTHORIZED);
    // And the last player stays.
    let last = send(&app, HERE, Req { body: Some(json!({})), ..req("DELETE", "/api/players/1") }).await;
    assert_eq!(last.status, StatusCode::BAD_REQUEST, "{}", last.body);
}

#[tokio::test]
async fn each_player_has_their_own_marks() {
    let (app, sam) = household().await;
    let sam_s = sam.to_string();
    let celeste = title(&app, "Celeste").await;
    let as_sam = |method, uri| Req { player: Some(&sam_s), ..req(method, uri) };
    let uri = format!("/api/games/{celeste}/later");
    let added = send(&app, HERE, as_sam("PUT", &uri)).await;
    assert!(added.status.is_success(), "{}", added.body);
    assert!(send(&app, HERE, as_sam("GET", "/api/play-later")).await.body.to_string().contains("Celeste"));
    let firsts = send(&app, HERE, req("GET", "/api/play-later")).await.body.to_string();
    assert!(!firsts.contains("Celeste"), "player 1's Play later is their own: {firsts}");

    // Hiding a game isn't Not for me: what the taste profile knows stays as it was.
    // (The library's count drops by the game, as it should.)
    let sources = || async {
        let mut v = send(&app, HERE, Req { player: Some(&sam_s), ..req("GET", "/api/taste/sources") }).await.body;
        v.as_object_mut().unwrap().remove("library");
        v
    };
    let before = sources().await;
    let hid = send(
        &app,
        HERE,
        Req { player: Some(&sam_s), body: Some(json!({ "scope": "me" })), ..req("PUT", &format!("/api/games/{}/hide", celeste)) },
    )
    .await;
    assert_eq!(hid.status, StatusCode::OK, "{}", hid.body);
    assert_eq!(sources().await, before, "hiding teaches the profile nothing");
}

#[tokio::test]
async fn a_game_hidden_from_a_player_is_never_named_in_their_jobs_or_logs() {
    let (app, sam) = household().await;
    let sam_s = sam.to_string();
    let celeste = title(&app, "Celeste").await;
    // Sam sees Celeste through player 1's shared account, until player 1
    // keeps it from everyone else.
    let sam_games =
        || async { send(&app, HERE, Req { player: Some(&sam_s), ..req("GET", "/api/games?q=celeste") }).await.body["total"].clone() };
    assert_eq!(sam_games().await, 1);
    let hid = send(&app, HERE, Req { body: Some(json!({ "scope": "others" })), ..req("PUT", &format!("/api/games/{celeste}/hide")) }).await;
    assert_eq!(hid.status, StatusCode::OK, "{}", hid.body);
    assert_eq!(sam_games().await, 0);

    // A job that names Celeste everywhere it can.
    let job = app.db.job_begin("enrich", None, JobSource::Ui, &json!({})).unwrap().unwrap();
    let progress = JobProgress {
        total: 4,
        done: 1,
        current: Some("Celeste".into()),
        message: "Fetching Celeste".into(),
        lanes: vec![crate::db::LaneProgress {
            name: "HLTB".into(),
            done: 1,
            total: 4,
            failed: 1,
            state: Default::default(),
            current: Some("Celeste".into()),
            note: Some("waiting on Celeste".into()),
            retry_at: None,
        }],
        ..Default::default()
    };
    let line = |message: &str, title_id: Option<i64>| LogLine { at: 1, level: Level::Warn, lane: None, message: message.into(), title_id };
    let failure = Failure {
        lane: "HLTB".into(),
        source: "hltb".into(),
        options: vec![],
        title_id: Some(celeste),
        name: Some("Celeste".into()),
        error: "no answer for Celeste".into(),
        at: 1,
    };
    app.db
        .job_pulse(job, &progress, &[line("HLTB: no match for Celeste", Some(celeste)), line("Celeste again, untagged", None)], &[failure])
        .unwrap();
    // And a server warning outside any job.
    {
        use tracing_subscriber::layer::SubscriberExt;
        let _log = tracing::subscriber::set_default(tracing_subscriber::registry().with(crate::log_layer()));
        tracing::warn!("Steam had no store page for Celeste");
    }

    let uris =
        ["/api/jobs".to_string(), format!("/api/jobs/{job}"), format!("/api/jobs/{job}/log.txt"), "/api/logs?level=warn".to_string()];
    for uri in &uris {
        let theirs = send(&app, HERE, Req { player: Some(&sam_s), ..req("GET", uri) }).await;
        assert_eq!(theirs.status, StatusCode::OK, "{uri}: {}", theirs.body);
        let text = theirs.body.to_string();
        assert!(!text.contains("Celeste"), "{uri} names Celeste to Sam: {text}");
        assert!(text.contains("a hidden game"), "{uri}: {text}");
        let own = send(&app, HERE, req("GET", uri)).await.body.to_string();
        assert!(own.contains("Celeste"), "{uri} names it to player 1, who sees it: {own}");
    }
    // Stopping the job answers with it, masked as the list is.
    let stopped = send(&app, HERE, Req { player: Some(&sam_s), ..req("POST", "/api/jobs/current/cancel") }).await;
    assert_eq!(stopped.status, StatusCode::OK, "{}", stopped.body);
    let text = stopped.body.to_string();
    assert!(!text.contains("Celeste") && text.contains("a hidden game"), "cancel names Celeste to Sam: {text}");
}

#[tokio::test]
async fn usage_is_each_players_own() {
    let (app, sam) = household().await;
    let sam_s = sam.to_string();
    let call = |player_id: Option<PlayerId>| crate::db::AiCall {
        at: crate::db::now(),
        kind: crate::db::AiKind::Pick,
        backend: "openai".into(),
        model: Some("fake-model".into()),
        error: None,
        duration_ms: 1_000,
        usage: None,
        cost_usd: None,
        api_usd: None,
        would_cost_usd: None,
        items: None,
        batch_id: None,
        job_id: None,
        player_id,
        ai_account_id: None,
    };
    app.db.ai_call_add(&call(Some(PlayerId::FIRST))).unwrap();
    app.db.ai_call_add(&call(Some(PlayerId(sam)))).unwrap();
    app.db.ai_call_add(&call(Some(PlayerId(sam)))).unwrap();
    let by_player = |v: &Value| -> Vec<(String, u64)> {
        v["by_player"].as_array().unwrap().iter().map(|p| (p["name"].as_str().unwrap().to_string(), p["calls"].as_u64().unwrap())).collect()
    };
    let sams = send(&app, HERE, Req { player: Some(&sam_s), ..req("GET", "/api/ai/usage") }).await.body;
    assert_eq!(by_player(&sams), [("Sam".to_string(), 2)], "{sams}");
    assert_eq!(sams["total"]["calls"], 2);
    // Player 1 owns the AI everyone uses here, so they see every call on it.
    let firsts = here(&app, req("GET", "/api/ai/usage")).await;
    assert_eq!(firsts["total"]["calls"].as_u64(), Some(3), "{firsts}");
}

/// Usage per AI account: each account's calls in a row of its
/// own, the reader's own accounts first; another player's account shows
/// only the reader's own requests on it.
#[tokio::test]
async fn usage_is_broken_out_by_ai_account() {
    let (app, sam) = household().await;
    let sam_s = sam.to_string();
    let shared = app.db.ai_accounts_for(PlayerId::FIRST).unwrap()[0].id;
    let own = app.db.add_ai_account(PlayerId(sam), Some("api-key"), &[], crate::db::Sharing::Owner).unwrap();
    let call = |player_id: PlayerId, account: i64| crate::db::AiCall {
        at: crate::db::now(),
        kind: crate::db::AiKind::Pick,
        backend: "openai".into(),
        model: Some("fake-model".into()),
        error: None,
        duration_ms: 1_000,
        usage: None,
        cost_usd: None,
        api_usd: None,
        would_cost_usd: None,
        items: None,
        batch_id: None,
        job_id: None,
        player_id: Some(player_id),
        ai_account_id: Some(account),
    };
    app.db.ai_call_add(&call(PlayerId::FIRST, shared)).unwrap();
    app.db.ai_call_add(&call(PlayerId(sam), shared)).unwrap();
    app.db.ai_call_add(&call(PlayerId(sam), own)).unwrap();
    app.db.ai_call_add(&call(PlayerId(sam), own)).unwrap();
    let by_account = |v: &Value| -> Vec<(i64, bool, u64)> {
        v["by_account"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| (a["ai_account_id"].as_i64().unwrap(), a["mine"].as_bool().unwrap(), a["calls"].as_u64().unwrap()))
            .collect()
    };
    let sams = send(&app, HERE, Req { player: Some(&sam_s), ..req("GET", "/api/ai/usage") }).await.body;
    assert_eq!(by_account(&sams), [(own, true, 2), (shared, false, 1)], "{sams}");
    let theirs = &sams["by_account"][1];
    assert_eq!(theirs["owner"], "Player 1", "{sams}");
    assert_eq!(theirs["this_computer"], true, "{sams}");
    assert_eq!(sams["by_account"][0]["backend"], "api-key", "{sams}");
    // Player 1 sees every call on the account they own, and nothing of Sam's own.
    let firsts = here(&app, req("GET", "/api/ai/usage")).await;
    assert_eq!(by_account(&firsts), [(shared, true, 2)], "{firsts}");
}

#[tokio::test]
async fn binding_a_token_or_a_device_to_a_player_with_a_pin_takes_their_pin() {
    let (app, sam) = household().await;
    app.db.set_pin(crate::db::PlayerId(sam), Some("2468")).unwrap();
    let make = |pin: Option<&str>| json!({ "name": format!("script {pin:?}"), "scope": "full", "player_id": sam, "pin": pin });
    for pin in [None, Some("1111")] {
        let a = send(&app, HERE, Req { body: Some(make(pin)), ..req("POST", "/api/tokens") }).await;
        assert_eq!(a.status, StatusCode::FORBIDDEN, "{pin:?}: {}", a.body);
    }
    let made = send(&app, HERE, Req { body: Some(make(Some("2468"))), ..req("POST", "/api/tokens") }).await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    // Unbound, or bound to a player without a PIN, needs none.
    token(&app, "full", None).await;
    token(&app, "full", Some(1)).await;

    let uri = format!("/api/pairing?player={sam}");
    // Leaving the PIN out is refused but never counted: five such don't lock it.
    for _ in 0..5 {
        let bare = send(&app, HERE, req("POST", &uri)).await;
        assert_eq!(bare.status, StatusCode::FORBIDDEN, "{}", bare.body);
    }
    let code = send(&app, HERE, Req { body: Some(json!({ "pin": "2468" })), ..req("POST", &uri) }).await;
    assert_eq!(code.status, StatusCode::CREATED, "{}", code.body);
    assert_eq!(send(&app, HERE, req("POST", "/api/pairing?player=shared")).await.status, StatusCode::CREATED);

    // Wrong tries count against Sam's lockout, as the picker's do: five
    // lock it, and then even the right PIN waits.
    for _ in 0..5 {
        send(&app, HERE, Req { body: Some(json!({ "pin": "0000" })), ..req("POST", &uri) }).await;
    }
    let again = json!({ "name": "another script", "scope": "full", "player_id": sam, "pin": "2468" });
    let locked = send(&app, HERE, Req { body: Some(again), ..req("POST", "/api/tokens") }).await;
    assert_eq!(locked.status, StatusCode::TOO_MANY_REQUESTS, "{}", locked.body);
}

#[tokio::test]
async fn hidden_by_me_opens_with_the_pin_and_locks_again() {
    let (app, sam) = household().await;
    app.db.set_pin(crate::db::PlayerId(sam), Some("2468")).unwrap();
    let chose =
        send(&app, HERE, Req { body: Some(json!({ "player_id": sam, "pin": "2468" })), ..req("PUT", "/api/players/current") }).await;
    assert_eq!(chose.status, StatusCode::OK, "{}", chose.body);
    let (sams, _) = cookie(&chose.headers);
    let as_sam = |method, uri, body| Req { cookie: Some(&sams), body, ..req(method, uri) };
    assert_eq!(send(&app, HERE, as_sam("GET", "/api/hidden", None)).await.status, StatusCode::FORBIDDEN, "locked at first");
    let open = send(&app, HERE, as_sam("POST", "/api/hidden/unlock", Some(json!({ "pin": "2468" })))).await;
    assert_eq!(open.status, StatusCode::NO_CONTENT, "{}", open.body);
    assert_eq!(send(&app, HERE, as_sam("GET", "/api/hidden", None)).await.status, StatusCode::OK);
    // Lock: closed again in this browser before the ten minutes are up.
    assert_eq!(send(&app, HERE, as_sam("DELETE", "/api/hidden/unlock", None)).await.status, StatusCode::NO_CONTENT);
    assert_eq!(send(&app, HERE, as_sam("GET", "/api/hidden", None)).await.status, StatusCode::FORBIDDEN, "locked again");
}

#[tokio::test]
async fn a_paired_device_belongs_to_whoever_it_was_paired_for() {
    let (app, sam) = household().await;
    let app_ref = &app;
    let pair = |player: Option<String>| async move {
        let uri = match &player {
            Some(p) => format!("/api/pairing?player={p}"),
            None => "/api/pairing".into(),
        };
        let code = send(app_ref, HERE, req("POST", &uri)).await;
        assert_eq!(code.status, StatusCode::CREATED, "{}", code.body);
        let paired = send(app_ref, LAN, Req { body: Some(json!({ "code": code.body["code"] })), ..req("POST", "/api/pair") }).await;
        assert!(paired.status.is_success(), "{}", paired.body);
        cookie(&paired.headers).0
    };
    let sams = pair(Some(sam.to_string())).await;
    assert_eq!(current(&app, LAN, Req { cookie: Some(&sams), ..req("GET", "") }).await, (StatusCode::OK, Some(sam), true));
    let shared = pair(Some("shared".into())).await;
    assert_eq!(current(&app, LAN, Req { cookie: Some(&shared), ..req("GET", "") }).await, (StatusCode::OK, Some(1), false));
    // Without asking, the player making the code: here, the default.
    let default = pair(None).await;
    assert_eq!(current(&app, LAN, Req { cookie: Some(&default), ..req("GET", "") }).await, (StatusCode::OK, Some(1), true));
    let unknown = send(&app, HERE, req("POST", "/api/pairing?player=999")).await;
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST, "{}", unknown.body);
}

/// B-CORE-1 (code health review 2026-10-11): a merge changes everyone's
/// library, so a player can merge only titles they see. Player 1 keeps
/// Celeste from everyone else; Sam's merge of Hades and Celeste is a 404
/// that changes nothing, and Sam can still merge two titles they see.
#[tokio::test]
async fn a_player_merges_only_titles_they_see() {
    let (app, sam) = household().await;
    let sam_s = sam.to_string();
    let (celeste, hades, stardew) = (title(&app, "Celeste").await, title(&app, "Hades").await, title(&app, "Stardew Valley").await);
    let hid = send(&app, HERE, Req { body: Some(json!({ "scope": "others" })), ..req("PUT", &format!("/api/games/{celeste}/hide")) }).await;
    assert_eq!(hid.status, StatusCode::OK, "{}", hid.body);
    let as_sam = |method, uri: String, body| {
        let (app, sam_s) = (&app, &sam_s);
        async move { send(app, HERE, Req { player: Some(sam_s), body, ..req(method, &uri) }).await }
    };
    assert_eq!(as_sam("GET", format!("/api/games/{celeste}"), None).await.status, StatusCode::NOT_FOUND, "Sam doesn't see Celeste");

    for ids in [json!([hades, celeste]), json!([celeste, hades])] {
        let a = as_sam("POST", "/api/titles/merge".into(), Some(json!({ "ids": ids }))).await;
        assert_eq!(a.status, StatusCode::NOT_FOUND, "{ids}: {}", a.body);
        assert_eq!(a.body["error"], format!("title {celeste} not found"), "the same answer as for a title that isn't there");
    }
    let mine = here(&app, req("GET", &format!("/api/games/{celeste}"))).await;
    assert_eq!(mine["copies"].as_array().unwrap().len(), 1, "player 1's Celeste is untouched: {mine}");
    assert_eq!(as_sam("GET", format!("/api/games/{hades}"), None).await.status, StatusCode::OK, "Hades is still Sam's");

    let a = as_sam("POST", "/api/titles/merge".into(), Some(json!({ "ids": [hades, 999_999] }))).await;
    assert_eq!(a.status, StatusCode::NOT_FOUND, "{}", a.body);
    let a = as_sam("POST", "/api/titles/merge".into(), Some(json!({ "ids": [hades, stardew] }))).await;
    assert_eq!(a.status, StatusCode::OK, "{}", a.body);
    assert_eq!(a.body["copies"].as_array().unwrap().len(), 2);
}
