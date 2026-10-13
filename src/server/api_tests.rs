//! The HTTP API end to end: the real router on a temp database, with an
//! OpenAI-compatible fake standing in for the model and, where a test needs
//! them, a local mock server standing in for the outside services.

use super::scheduler::schedule_tick;
use super::serve::serve_until;
use super::steam::{OPENID_NS, STEAM_OP_ENDPOINT, urlenc};
use super::*;
use crate::config::Config;
use crate::db::Db;
use crate::db::ImportedGame;
use crate::db::PlayerId;
use crate::db::{JobSource, JobStatus};
use crate::enrich::Clients;
use crate::jobs::{JobKind, JobRunner};
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use axum::routing::post;
use std::sync::Arc;
use tower::ServiceExt;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod access;
mod accounts;
mod approval;
mod community_verdicts;
mod discovery;
mod errors;
mod history;
mod oidc;
mod player_setup;
mod players;
mod proxies;
mod ratings;
mod remote;
mod request_log;
mod routines;
mod seed;
mod setup;
mod setup_sync;
mod steam_shortcut;
mod stores;
mod verdicts;
mod wishlist;

/// A fake chat-completions server that picks the first candidates listed in
/// the prompt, then the next ones as spares, skipping any the request says
/// were turned down or are still in the lineup. Help me decide gets the
/// first game, and "It's too ___" the first spare. A request asking to
/// "answer wrong first" gets a game that isn't a candidate until the
/// correction. Hidden gems are each rated 3, 2 and 1. Every reply used
/// 1,000 tokens in (400 of them cached) and 50 out.
async fn fake_llm() -> String {
    async fn complete(Json(body): Json<Value>) -> Json<Value> {
        let prompt = body.pointer("/messages/1/content").and_then(Value::as_str).unwrap_or_default().to_string();
        let usage = json!({ "prompt_tokens": 1000, "completion_tokens": 50, "total_tokens": 1050, "prompt_tokens_details": { "cached_tokens": 400 } });
        let reply = |content: Value| {
            Json(json!({ "model": "fake-model", "usage": usage, "choices": [{ "finish_reason": "stop",
                "message": { "role": "assistant", "content": content.to_string() } }] }))
        };
        let first = regex::Regex::new(r"(?m)^#(\d+) ").unwrap().captures(&prompt).map(|c| c[1].parse::<i64>().unwrap());
        if body.pointer("/response_format/json_schema/schema/properties/pick_id").is_some() {
            return reply(json!({ "pick_id": first, "reason": "Shorter, and you're in the mood for it." }));
        }
        if body.pointer("/response_format/json_schema/schema/properties/games/items/properties/studio").is_some() {
            let games: Vec<Value> = regex::Regex::new(r"(?m)^### #(\d+) ")
                .unwrap()
                .captures_iter(&prompt)
                .map(|c| {
                    json!({ "id": c[1].parse::<i64>().unwrap(), "studio": 3, "mechanics": 2, "play": 1, "party": false,
                    "line": "Nothing else plays like it." })
                })
                .collect();
            return reply(json!({ "games": games }));
        }
        if body.pointer("/response_format/json_schema/schema/properties/games").is_some() {
            let games: Vec<Value> = regex::Regex::new(r"(?m)^### #(\d+) ")
                .unwrap()
                .captures_iter(&prompt)
                .map(|c| {
                    json!({ "id": c[1].parse::<i64>().unwrap(), "now": "Reviews since say it's gentler now.", "verdict": "better",
                    "line": "reviews since say it's gentler" })
                })
                .collect();
            return reply(json!({ "games": games }));
        }
        if body.pointer("/response_format/json_schema/schema/properties/replacement").is_some() {
            let spares = prompt.split("## Spares, best first\n").nth(1).unwrap_or_default();
            let spare = regex::Regex::new(r"(?m)^#(\d+) ").unwrap().captures(spares).map(|c| c[1].parse::<i64>().unwrap());
            return reply(json!({ "replacement": spare.unwrap_or(0), "also": [] }));
        }
        if prompt.contains("answer wrong first") && !prompt.contains("## Correction") {
            return reply(json!({ "picks": [{ "id": 999_999, "name": "Not a candidate", "reason": "?" }] }));
        }
        let max =
            |list: &str| body.pointer(&format!("/response_format/json_schema/schema/properties/{list}/maxItems")).and_then(Value::as_u64);
        let (n, spares) = (max("picks").unwrap_or(3) as usize, max("spares").unwrap_or(0) as usize);
        let listed = |heading: &str| -> Vec<String> {
            prompt
                .split(heading)
                .nth(1)
                .and_then(|rest| rest.lines().find(|l| !l.trim().is_empty()))
                .map(|l| regex::Regex::new(r"#(\d+)").unwrap().captures_iter(l).map(|c| c[1].to_string()).collect())
                .unwrap_or_default()
        };
        let mut off = listed("I turned down: ");
        off.extend(listed("## Still in my lineup\n"));
        let line = regex::Regex::new(r"^#(\d+) ([^|\[]+)").unwrap();
        let mut games = prompt
            .lines()
            .filter_map(|l| line.captures(l))
            .filter(|c| !off.contains(&c[1].to_string()))
            .map(|c| json!({ "id": c[1].parse::<i64>().unwrap(), "name": c[2].trim(), "reason": "Fits the request." }));
        let picks: Vec<Value> = games.by_ref().take(n).collect();
        let spares: Vec<Value> = games.take(spares).collect();
        reply(json!({ "picks": picks, "spares": spares }))
    }
    let app = Router::new().route("/v1/chat/completions", post(complete));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}/v1")
}

/// Offline config (see `testutil::offline_config`) with the model at `llm_url`.
fn test_config(dir: &std::path::Path, llm_url: &str) -> Config {
    let mut cfg = crate::testutil::offline_config(dir);
    cfg.llm_backend = "openai".into();
    cfg.openai_base_url = llm_url.into();
    cfg.openai_model = Some("fake-model".into());
    cfg
}

struct TestApp {
    _dir: tempfile::TempDir,
    router: Router,
    db: Arc<Db>,
    state: AppState,
}

/// `services` stands in for Steam, GOG and the rest; None leaves the real
/// hosts configured (tests that don't call out never reach them).
async fn test_app_with(llm_url: Option<String>, services: Option<&str>) -> TestApp {
    let dir = tempfile::tempdir().unwrap();
    let llm_url = match llm_url {
        Some(u) => u,
        None => fake_llm().await,
    };
    let cfg = test_config(dir.path(), &llm_url);
    let db = Arc::new(Db::open(&cfg.db_path, cfg.sampled_minutes).unwrap());
    let game = |source: &str, id: &str, name: &str, appid: Option<u32>, minutes: Option<i64>| ImportedGame {
        source: source.into(),
        source_id: id.into(),
        launcher: if source == "steam" { "steam".into() } else { "heroic".into() },
        name: name.into(),
        appid,
        playtime_minutes: minutes,
        ..Default::default()
    };
    db.sync_source(
        PlayerId::FIRST,
        "steam",
        "steam",
        &[
            game("steam", "620", "Portal 2", Some(620), Some(1260)),
            game("steam", "413150", "Stardew Valley", Some(413150), Some(0)),
            game("steam", "504230", "Celeste", Some(504230), Some(0)),
            game("steam", "1145360", "Hades", Some(1145360), Some(0)),
        ],
    )
    .unwrap();
    db.sync_source(
        PlayerId::FIRST,
        "heroic",
        "epic",
        &[game("epic", "e1", "Control", None, None), game("epic", "e2", "Portal 2", None, None)],
    )
    .unwrap();
    let clients = Arc::new(Clients::with_base_url(&cfg, services).unwrap());
    let state = AppState {
        db: db.clone(),
        base_cfg: cfg,
        clients,
        jobs: Arc::new(JobRunner::new()),
        remote: Default::default(),
        claude_install: Default::default(),
        setup_sync: Default::default(),
    };
    TestApp { _dir: dir, router: router(state.clone()), db, state }
}

/// The paired-browser cookie's name on the test server's port (B-AUTH-4).
const DEVICE_COOKIE: &str = "upnext_device_8787";

async fn test_app() -> TestApp {
    test_app_with(None, None).await
}

impl TestApp {
    async fn send(&self, req: Request<Body>) -> (StatusCode, Value) {
        let resp = self.router.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned())))
    }

    async fn call(&self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(uri).header(header::HOST, "127.0.0.1:8787");
        let body = match body {
            Some(v) => {
                req = req.header(header::CONTENT_TYPE, "application/json");
                Body::from(v.to_string())
            }
            None => Body::empty(),
        };
        self.send(req.body(body).unwrap()).await
    }

    async fn get(&self, uri: &str) -> Value {
        let (status, v) = self.call("GET", uri, None).await;
        assert_eq!(status, StatusCode::OK, "GET {uri}: {v}");
        v
    }

    /// Where a response redirects to.
    async fn redirect(&self, uri: &str) -> String {
        let req = Request::get(uri).header(header::HOST, "127.0.0.1:8787").body(Body::empty()).unwrap();
        let resp = self.router.clone().oneshot(req).await.unwrap();
        assert!(resp.status().is_redirection(), "GET {uri}: {}", resp.status());
        resp.headers()[header::LOCATION].to_str().unwrap().to_string()
    }

    /// Start a Steam sign-in and return the state token it issued, with the
    /// cookie that ties it to this browser (`name=value`).
    async fn steam_login(&self) -> (String, String) {
        let req = Request::get("/api/steam/login").header(header::HOST, "127.0.0.1:8787").body(Body::empty()).unwrap();
        let resp = self.router.clone().oneshot(req).await.unwrap();
        let to = resp.headers()[header::LOCATION].to_str().unwrap();
        assert!(to.starts_with("https://steamcommunity.com/openid/login?"), "{to}");
        let state = regex::Regex::new(r"state%3D([0-9a-f]{32})").unwrap().captures(to).unwrap()[1].to_string();
        let cookie = resp.headers()[header::SET_COOKIE].to_str().unwrap();
        assert!(
            cookie.contains("; Path=/api/steam/callback; HttpOnly; SameSite=Lax; Max-Age=600") && !cookie.contains("Secure"),
            "{cookie}"
        );
        (state, cookie.split(';').next().unwrap().to_string())
    }

    /// Come back from Steam with an assertion for SteamID 76561190000000001,
    /// in a browser with `cookie`. Returns where the UI is sent.
    async fn steam_callback(&self, state: &str, cookie: Option<&str>, return_to: &str, op_endpoint: &str) -> String {
        let claimed = "https://steamcommunity.com/openid/id/76561190000000001";
        let fields = [
            ("state", state),
            ("openid.ns", OPENID_NS),
            ("openid.mode", "id_res"),
            ("openid.op_endpoint", op_endpoint),
            ("openid.claimed_id", claimed),
            ("openid.identity", claimed),
            ("openid.return_to", return_to),
            ("openid.response_nonce", "2026-09-29T12:00:00Zc0ffee"),
            ("openid.assoc_handle", "1234567890"),
            ("openid.signed", "signed,op_endpoint,claimed_id,identity,return_to,response_nonce,assoc_handle"),
            ("openid.sig", "c2lnbmF0dXJl"),
        ];
        let query: Vec<String> = fields.iter().map(|(k, v)| format!("{k}={}", urlenc(v))).collect();
        let mut req = Request::get(format!("/api/steam/callback?{}", query.join("&"))).header(header::HOST, "127.0.0.1:8787");
        if let Some(c) = cookie {
            req = req.header(header::COOKIE, c);
        }
        let resp = self.router.clone().oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
        assert!(resp.status().is_redirection(), "{}", resp.status());
        let cleared = resp.headers()[header::SET_COOKIE].to_str().unwrap();
        assert!(cleared.starts_with("upnext_steam_signin=;") && cleared.contains("Max-Age=0"), "{cleared}");
        resp.headers()[header::LOCATION].to_str().unwrap().to_string()
    }

    /// Wait for the running job to end, and return it.
    async fn job_ended(&self) -> Value {
        for _ in 0..500 {
            let jobs = self.get("/api/jobs").await;
            if jobs["current"].is_null() {
                return jobs["recent"][0].clone();
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("the job didn't end");
    }

    async fn id_of(&self, name: &str) -> i64 {
        let v = self.get(&format!("/api/games?q={}", name.replace(' ', "+"))).await;
        v["items"].as_array().unwrap().iter().find(|g| g["name"] == name).unwrap_or_else(|| panic!("{name} not listed: {v}"))["id"]
            .as_i64()
            .unwrap()
    }
}

fn names(items: &Value) -> Vec<String> {
    items.as_array().unwrap().iter().map(|g| g["name"].as_str().unwrap().to_string()).collect()
}

/// Big screen's Controller filter: games Steam says a pad works
/// with, fully or in part, and the word itself on each game.
#[tokio::test]
async fn library_filters_on_controller_support() {
    let app = test_app().await;
    for (appid, support) in [(1145360, Some("full")), (504230, Some("partial")), (413150, None)] {
        let row = crate::db::DetailsRow { type_: Some("game".into()), controller_support: support.map(Into::into), ..Default::default() };
        app.db.put_details(appid, Some(&row)).unwrap();
    }
    let pads = app.get("/api/v1/games?controller=true").await;
    assert_eq!(names(&pads["items"]), ["Celeste", "Hades"]);
    assert_eq!(pads["items"][0]["controller_support"], "partial");
    let all = app.get("/api/v1/games?q=stardew").await;
    assert_eq!(all["items"][0]["controller_support"], Value::Null, "Steam doesn't say");
}

/// A pick asked from big screen counts as on a controller, as a
/// routine on one does: the ranking says it counted how well each game
/// plays on one, and a game Steam lists no controller support for falls
/// behind one with full support.
#[tokio::test]
async fn picks_asked_on_a_controller_count_controller_support() {
    let app = test_app().await;
    for (appid, support) in [(1145360, Some("full")), (504230, Some("full")), (413150, None)] {
        let categories = match support {
            Some(_) => vec!["Single-player".into(), "Full controller support".into()],
            None => vec!["Single-player".into()],
        };
        let row = crate::db::DetailsRow {
            type_: Some("game".into()),
            controller_support: support.map(Into::into),
            categories,
            ..Default::default()
        };
        app.db.put_details(appid, Some(&row)).unwrap();
    }
    let body = json!({ "prompt": "", "count": 5, "without_ai": true, "input": "controller" });
    let (status, v) = app.call("POST", "/api/v1/picks", Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let by: Vec<&str> = v["ranked_by"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(by.contains(&"how well it plays on a controller, by Steam's store details, as you asked on one"), "{by:?}");
    let all: Vec<&str> =
        v["picks"].as_array().unwrap().iter().chain(v["spares"].as_array().unwrap()).filter_map(|p| p["name"].as_str()).collect();
    let at = |name: &str| all.iter().position(|n| *n == name);
    assert!(at("Hades") < at("Stardew Valley") || at("Stardew Valley").is_none(), "{all:?}");
    let (status, v) =
        app.call("POST", "/api/v1/picks", Some(json!({ "prompt": "", "count": 1, "without_ai": true, "input": "mouse" }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "only a known input: {v}");
    let (_, v) = app.call("POST", "/api/v1/picks", Some(json!({ "prompt": "", "count": 1, "without_ai": true }))).await;
    assert!(!v["ranked_by"].to_string().contains("controller"), "{v}");
}

/// An `/api` path no route matches is a JSON 404; other paths still
/// get the UI's page, so its own routes load.
#[tokio::test]
async fn unknown_api_routes_are_404() {
    let app = test_app().await;
    for (method, uri) in [("GET", "/api/v1/nonexistent"), ("POST", "/api/v1/jobs/5/cancel"), ("GET", "/api/nope"), ("GET", "/api/v1/")] {
        let (status, v) = app.call(method, uri, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}: {v}");
        assert_eq!(v["error"], format!("no such API route: {uri}"), "{method} {uri}");
    }
    let (status, _) = app.call("GET", "/settings", None).await;
    assert_eq!(status, StatusCode::OK, "the UI's own routes still load");
    let (status, _) = app.call("DELETE", "/api/v1/stats", None).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "a known path with the wrong method");
}

/// A UI folder (`UPNEXT_STATIC_DIR`): its files, the page for the UI's own
/// routes, and a 404 for a script it doesn't have, which an open tab from
/// before an upgrade asks for. The page there would fail as a script and
/// leave the tab blank (code health B-WEB-1).
#[tokio::test]
async fn a_ui_folder_answers_a_missing_script_with_404() {
    let app = test_app().await;
    let ui = app._dir.path().join("ui");
    std::fs::create_dir_all(ui.join("assets")).unwrap();
    std::fs::write(ui.join("index.html"), "<!doctype html><script type=module src=/assets/index-new.js></script>").unwrap();
    std::fs::write(ui.join("assets/index-new.js"), "export {}").unwrap();
    let mut state = app.state.clone();
    state.base_cfg.static_dir = Some(ui);
    let router = router(state);
    let get = |uri: &'static str| {
        let router = router.clone();
        async move {
            let resp = router.oneshot(Request::get(uri).header(header::HOST, "127.0.0.1:8787").body(Body::empty()).unwrap()).await.unwrap();
            let status = resp.status();
            (status, String::from_utf8(to_bytes(resp.into_body(), usize::MAX).await.unwrap().to_vec()).unwrap())
        }
    };
    let (status, body) = get("/assets/index-new.js").await;
    assert_eq!((status, body.as_str()), (StatusCode::OK, "export {}"));
    for page in ["/", "/settings", "/game/4"] {
        let (status, body) = get(page).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert!(body.contains("index-new.js"), "{page}: {body}");
    }
    let (status, body) = get("/assets/SettingsPage-0ld0ld00.js").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(!body.contains("<script"), "{body}");
}

#[tokio::test]
async fn library_listing_filters_sorts_and_pages() {
    let app = test_app().await;
    let all = app.get("/api/games?limit=100").await;
    assert_eq!(all["total"], 5, "Portal 2's Steam and Epic copies are one title");
    assert_eq!(names(&all["items"]), ["Celeste", "Control", "Hades", "Portal 2", "Stardew Valley"]);

    let page = app.get("/api/games?limit=2&offset=2").await;
    assert_eq!((page["total"].as_i64(), names(&page["items"])), (Some(5), vec!["Hades".to_string(), "Portal 2".to_string()]));
    assert_eq!(names(&app.get("/api/games?source=epic").await["items"]), ["Control", "Portal 2"], "any store a title is owned on");
    assert_eq!(names(&app.get("/api/games?q=stard").await["items"]), ["Stardew Valley"]);
    assert_eq!(names(&app.get("/api/games?sort=name&order=desc&limit=1").await["items"]), ["Stardew Valley"]);
    // 21 hours, with no HowLongToBeat time and no date it was last played: shelved.
    assert_eq!(names(&app.get("/api/games?status=shelved").await["items"]), ["Portal 2"]);
    assert_eq!(names(&app.get("/api/games?status=paused").await["items"]), ["Portal 2"], "the name before v18");

    // A status or a Linux filter it doesn't know is a 400, not an empty list.
    let (status, v) = app.call("GET", "/api/games?status=shelved,bogus", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(
        v["error"],
        "unknown status \"bogus\": untouched, sampled, playing, shelved, played, done, still_playing, burnt_out, abandoned, hidden or not_game"
    );
    let (status, v) = app.call("GET", "/api/games?linux=bogus", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"], "unknown Linux filter \"bogus\": ok, native, issues or unknown");
    assert_eq!(app.get("/api/games?linux=unknown").await["total"], 5, "none of them has Linux data");
    // A store or a genre is any name, so one no game has finds none.
    assert_eq!(app.get("/api/games?source=psn").await["total"], 0);
    assert_eq!(app.get("/api/games?genre=Nothing").await["total"], 0);
}

#[tokio::test]
async fn game_detail_and_override() {
    let app = test_app().await;
    let hades = app.id_of("Hades").await;
    assert_eq!(app.get(&format!("/api/games/{hades}")).await["name"], "Hades");
    assert_eq!(app.call("GET", "/api/games/999999", None).await.0, StatusCode::NOT_FOUND);

    let uri = format!("/api/games/{hades}/override");
    let (status, v) = app.call("PUT", &uri, Some(json!({ "status": "done", "note": "beat it" }))).await;
    assert_eq!((status, &v["override"], &v["override_note"]), (StatusCode::OK, &json!("done"), &json!("beat it")));
    assert_eq!(app.call("PUT", &uri, Some(json!({ "status": "finished" }))).await.0, StatusCode::BAD_REQUEST);
    let (_, v) = app.call("PUT", &uri, Some(json!({ "status": null }))).await;
    assert_eq!(v["override"], Value::Null);
}

/// The new statuses, why a game was put down, and the switch for
/// games with no ending.
#[tokio::test]
async fn statuses_reasons_and_the_ending_switch() {
    let app = test_app().await;
    let hades = app.id_of("Hades").await;
    let uri = format!("/api/games/{hades}/override");
    let (status, v) = app
        .call("PUT", &uri, Some(json!({ "status": "shelved", "reasons": { "codes": ["grind", "busy"], "note": " the second act " } })))
        .await;
    assert_eq!((status, &v["status"], &v["override"]), (StatusCode::OK, &json!("shelved"), &json!("shelved")), "{v}");
    assert_eq!(v["stop_reasons"]["codes"], json!(["busy", "grind"]), "in the menu's order");
    assert_eq!((&v["stop_reasons"]["note"], &v["stop_reasons"]["source"]), (&json!("the second act"), &json!("menu")));
    // Marking again without reasons keeps the last ones given.
    let (_, v) = app.call("PUT", &uri, Some(json!({ "status": "abandoned" }))).await;
    assert_eq!((&v["status"], &v["stop_reasons"]["codes"]), (&json!("abandoned"), &json!(["busy", "grind"])));
    let bad = |body: Value| app.call("PUT", &uri, Some(body));
    assert_eq!(bad(json!({ "status": "shelved", "reasons": { "codes": ["meh"] } })).await.0, StatusCode::BAD_REQUEST);
    let (status, v) = bad(json!({ "status": "done", "reasons": { "codes": ["busy"] } })).await;
    assert_eq!((status, &v["error"]), (StatusCode::BAD_REQUEST, &json!("reasons go with shelved, abandoned or burnt_out")));
    // The names before v18 still work.
    assert_eq!(app.call("PUT", &uri, Some(json!({ "status": "paused" }))).await.1["override"], "shelved");

    let (status, v) = app.call("PUT", &uri, Some(json!({ "status": "burnt_out", "reasons": { "codes": [], "note": "  " } }))).await;
    assert_eq!((status, &v["status"]), (StatusCode::OK, &json!("burnt_out")), "blank reasons are none");
    let back = v["burnt_out_until"].as_i64().unwrap() - crate::db::now();
    assert!((89 * 86_400..=90 * 86_400).contains(&back), "back in 90 days: {back}");

    // The switch: set either way, then back to inferred.
    let ending = format!("/api/games/{hades}/ending");
    assert_eq!(app.get(&format!("/api/games/{hades}")).await["kind"], "ending");
    let (status, v) = app.call("PUT", &ending, Some(json!({ "no_ending": true }))).await;
    assert_eq!((status, &v["kind"], &v["no_ending"]), (StatusCode::OK, &json!("endless"), &json!(true)));
    let (_, v) = app.call("PUT", &ending, Some(json!({ "no_ending": null }))).await;
    assert_eq!((&v["kind"], &v["no_ending"]), (&json!("ending"), &Value::Null));
    assert_eq!(app.call("PUT", "/api/games/999999/ending", Some(json!({ "no_ending": true }))).await.0, StatusCode::NOT_FOUND);

    // The settings move the lines as soon as they're saved.
    let portal = format!("/api/games/{}", app.id_of("Portal 2").await);
    assert_eq!(app.get(&portal).await["staple"], false, "21 hours");
    let (status, v) = app.call("PUT", "/api/settings", Some(json!({ "staple_hours": 20 }))).await;
    assert_eq!((status, &v["staple_hours"], &v["burnout_days"]), (StatusCode::OK, &json!(20), &json!(90)));
    assert_eq!(app.get(&portal).await["staple"], true);
    assert_eq!(app.call("PUT", "/api/settings", Some(json!({ "shelve_after_days": 0 }))).await.0, StatusCode::BAD_REQUEST);

    let reasons = app.get("/api/stop-reasons").await;
    assert_eq!(reasons[0], json!({ "code": "busy", "group": "life", "label": "Got busy", "aspects": [] }));
    assert_eq!(reasons[4], json!({ "code": "grind", "group": "game", "label": "The grind", "aspects": ["grind"] }));
}

/// The Second Chance page: an abandoned game, its "Now" from the job, the
/// three answers, Undo, a reply to "Now", and the staples filter.
#[tokio::test]
async fn second_chances_answers_and_staples() {
    // The job's enrich runs the Steam store step first. This store has no
    // page for any of the games and finds nothing by name, so the run never
    // leaves the machine; anything else it asks answers 404.
    let services = MockServer::start().await;
    Mock::given(path("/api/appdetails"))
        .respond_with(|req: &wiremock::Request| {
            let appid = req.url.query_pairs().find(|(k, _)| k == "appids").map(|(_, v)| v.into_owned()).unwrap_or_default();
            ResponseTemplate::new(200).set_body_json(json!({ appid: { "success": false } }))
        })
        .mount(&services)
        .await;
    Mock::given(path("/api/storesearch/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "items": [] })))
        .mount(&services)
        .await;
    let app = test_app_with(None, Some(&services.uri())).await;
    // Portal 2, 21 hours in and last played in 2022: abandoned by UpNext.
    let steam = |id: &str, name: &str, appid: u32, minutes: i64, last_played: i64| ImportedGame {
        source: "steam".into(),
        source_id: id.into(),
        launcher: "steam".into(),
        name: name.into(),
        appid: Some(appid),
        playtime_minutes: Some(minutes),
        last_played,
        ..Default::default()
    };
    app.db
        .sync_source(
            PlayerId::FIRST,
            "steam",
            "steam",
            &[
                steam("620", "Portal 2", 620, 1260, 1_646_092_800),
                steam("413150", "Stardew Valley", 413150, 0, 0),
                steam("504230", "Celeste", 504230, 0, 0),
                steam("1145360", "Hades", 1145360, 0, 0),
            ],
        )
        .unwrap();
    let portal = app.id_of("Portal 2").await;
    let review = |text: &str, posted: i64| crate::db::ReviewText {
        source: "steam".into(),
        positive: Some(true),
        rating: None,
        votes: 1,
        hours: Some(9),
        title: None,
        text: text.into(),
        posted: Some(posted),
    };
    // Fetched just now, so the job reads them as they are.
    app.db
        .put_review_texts(
            620,
            Some(&[review("Still the funniest co-op puzzles around, and the workshop maps keep coming.", 1_740_000_000)]),
        )
        .unwrap();

    let v = app.get("/api/second-chances").await;
    assert_eq!((v["ai"]["available"].clone(), v["task"]["schedule"].clone()), (json!(true), json!("off")), "{v}");
    assert_eq!(names(&v["games"].as_array().unwrap().iter().map(|e| e["game"].clone()).collect()), ["Portal 2"]);
    let entry = &v["games"][0];
    assert_eq!(
        (entry["reviews_since"].clone(), entry["due"].clone(), entry["stopped_at"].clone()),
        (json!(1), json!(true), json!(1_646_092_800))
    );

    // The job writes "Now"; then nothing is due.
    let (status, job) = app.call("POST", "/api/jobs", Some(json!({ "task": "second_chances" }))).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    let ended = app.job_ended().await;
    assert_eq!((ended["status"].clone(), ended["task"].clone()), (json!("done"), json!("second_chances")), "{ended}");
    let entry = app.get("/api/second-chances").await["games"][0].clone();
    let note = &entry["game"]["second_chance"];
    assert_eq!(
        (note["verdict"].clone(), note["outdated"].clone(), entry["due"].clone()),
        (json!("better"), json!(false), json!(false)),
        "{entry}"
    );
    assert_eq!(note["now"], "Reviews since say it's gentler now.");
    let usage = app.get("/api/ai/usage?days=1").await;
    assert!(usage["by_kind"].as_array().unwrap().iter().any(|k| k["kind"] == "second_chances" && k["calls"] == 1), "{usage}");

    // A reply updates the reasons, which puts "Now" out of date.
    let uri = format!("/api/second-chances/{portal}");
    let (status, g) = app.call("POST", &uri, Some(json!({ "reasons": { "codes": ["stuck"], "note": "the last test chamber" } }))).await;
    assert_eq!(
        (status, g["stop_reasons"]["source"].clone(), g["stop_reasons"]["note"].clone()),
        (StatusCode::OK, json!("second_chance"), json!("the last test chamber"))
    );
    assert_eq!(g["second_chance"]["outdated"], true);
    assert_eq!(app.get("/api/second-chances").await["games"][0]["due"], true);

    // Willing to try again: Shelved, off the page; then undone.
    let (status, g) = app.call("POST", &uri, Some(json!({ "answer": "willing" }))).await;
    assert_eq!((status, g["status"].clone(), g["second_try"].clone()), (StatusCode::OK, json!("shelved"), json!(true)), "{g}");
    assert_eq!(app.get("/api/second-chances").await["games"], json!([]));
    let (status, v) = app.call("POST", &uri, Some(json!({ "answer": "still_no" }))).await;
    assert_eq!(status, StatusCode::CONFLICT, "only games waiting on the page: {v}");
    let (_, g) = app.call("POST", &uri, Some(json!({ "undo": true }))).await;
    assert_eq!((g["status"].clone(), g["override"].clone()), (json!("abandoned"), Value::Null));
    // Still no and Not for me.
    let (_, g) = app.call("POST", &uri, Some(json!({ "answer": "still_no" }))).await;
    assert_eq!((g["status"].clone(), g["second_chance"]["answer"].clone()), (json!("abandoned"), json!("still_no")));
    app.call("POST", &uri, Some(json!({ "undo": true }))).await;
    let (_, g) = app.call("POST", &uri, Some(json!({ "answer": "not_for_me" }))).await;
    assert_eq!(g["status"], "hidden");
    // Bad requests.
    let bad = |body: Value| app.call("POST", &uri, Some(body));
    assert_eq!(bad(json!({ "answer": "maybe" })).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(bad(json!({})).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(bad(json!({ "answer": "willing", "undo": true })).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(app.call("POST", "/api/second-chances/999999", Some(json!({ "undo": true }))).await.0, StatusCode::NOT_FOUND);

    // Staples: 21 hours is one once the line is 20.
    assert_eq!(names(&app.get("/api/games?staple=true").await["items"]), Vec::<String>::new());
    app.call("PUT", "/api/settings", Some(json!({ "staple_hours": 20 }))).await;
    assert_eq!(names(&app.get("/api/games?staple=true").await["items"]), ["Portal 2"]);
}

/// Review tags come with a title, and their plain labels join the merged
/// tags (right after Steam's, which these titles don't have).
#[tokio::test]
async fn review_tags_come_with_a_title() {
    let app = test_app().await;
    let stardew = app.id_of("Stardew Valley").await;
    let tags = serde_json::from_value(json!({"scales": {"sessions": "short", "pace": "relaxed"}, "mood": ["cozy"],
        "aspects": [{"aspect": "music", "verdict": "praised"}], "notes": ["slow first week"]}))
    .unwrap();
    app.db.put_digest(stardew, "Praised for its calm loop. Complaints: a slow first week.", "", &tags, "test", 2).unwrap();

    let g = app.get(&format!("/api/games/{stardew}")).await;
    assert_eq!(g["review_tags"]["scales"], json!({"pace": "relaxed", "sessions": "short"}));
    assert_eq!(g["review_tags"]["aspects"], json!([{"aspect": "music", "verdict": "praised"}]));
    let labels = json!(["Cozy", "Relaxed pace", "Short sessions"]);
    assert_eq!((&g["tags_by_source"]["reviews"], &g["tags"]), (&labels, &labels));
    // Lists keep them for the pick lineup; only the per-source split is left out.
    let list = app.get("/api/games?q=stardew").await;
    assert_eq!(list["items"][0]["review_tags"]["mood"], json!(["cozy"]));
    let control = app.id_of("Control").await;
    assert_eq!(app.get(&format!("/api/games/{control}")).await["review_tags"], Value::Null);
}

/// The library filters on review tags, and the vocabulary they come from.
#[tokio::test]
async fn review_tag_filters_and_vocabulary() {
    let app = test_app().await;
    let tag = |id: i64, v: Value| {
        app.db.put_digest(id, "A summary.", "", &serde_json::from_value(v).unwrap(), "test", 2).unwrap();
    };
    tag(
        app.id_of("Stardew Valley").await,
        json!({"scales": {"sessions": "short", "grind": "light"}, "mood": ["relaxing", "cozy"],
               "aspects": [{"aspect": "music", "verdict": "praised"}, {"aspect": "grind", "verdict": "criticized"}]}),
    );
    tag(
        app.id_of("Celeste").await,
        json!({"scales": {"difficulty": "hard", "sessions": "short"}, "skills": ["precision"],
               "aspects": [{"aspect": "music", "verdict": "praised"}, {"aspect": "story", "verdict": "divisive"}]}),
    );
    tag(app.id_of("Hades").await, json!({"scales": {"difficulty": "hard"}, "aspects": [{"aspect": "grind", "verdict": "divisive"}]}));

    async fn list(app: &TestApp, q: &str) -> Vec<String> {
        names(&app.get(&format!("/api/games?{q}")).await["items"])
    }
    assert_eq!(list(&app, "facet=sessions:short").await, ["Celeste", "Stardew Valley"]);
    assert_eq!(list(&app, "facet=sessions:short&facet=mood:relaxing").await, ["Stardew Valley"], "repeated facets all apply");
    assert_eq!(list(&app, "facet=Sessions:Short,difficulty:hard").await, ["Celeste"], "a comma list, in any case");
    assert_eq!(list(&app, "praised=music").await, ["Celeste", "Stardew Valley"]);
    assert_eq!(list(&app, "praised=story").await, Vec::<String>::new(), "divisive is not praised");
    assert_eq!(list(&app, "not_criticized=grind").await, ["Celeste"], "criticized and divisive are out, and so are untagged titles");
    let (status, v) = app.call("GET", "/api/games?facet=sessions:tiny", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(app.call("GET", "/api/games?praised=graphics", None).await.0, StatusCode::BAD_REQUEST);

    let vocab = app.get("/api/review-tags").await;
    assert_eq!(vocab["tagged"], 3);
    let difficulty = &vocab["scales"][0];
    assert_eq!((&difficulty["name"], &difficulty["title"], &difficulty["known"]), (&json!("difficulty"), &json!("Difficulty"), &json!(2)));
    let values: Vec<(&str, i64)> =
        difficulty["values"].as_array().unwrap().iter().map(|v| (v["value"].as_str().unwrap(), v["count"].as_i64().unwrap())).collect();
    assert_eq!(values, [("easy", 0), ("moderate", 0), ("hard", 2), ("punishing", 0)], "ordered low to high");
    let mood = vocab["sets"].as_array().unwrap().iter().find(|s| s["name"] == "mood").unwrap();
    assert_eq!((&mood["known"], &mood["max"]), (&json!(1), &json!(2)));
    let grind = vocab["aspects"].as_array().unwrap().iter().find(|a| a["name"] == "grind").unwrap();
    assert_eq!((&grind["praised"], &grind["criticized"], &grind["divisive"]), (&json!(0), &json!(1), &json!(1)));
}

/// IGDB keywords stored before the import filtered them (stores, trade
/// shows, platform features) stay out of the tags.
#[tokio::test]
async fn catalogue_keywords_stay_out_of_the_tags() {
    let app = test_app().await;
    let control = app.id_of("Control").await;
    let keywords = ["previously on - stadia pro", "steam achievements", "pax west 2017", "psychic powers", "steampunk"];
    let igdb = crate::igdb::IgdbGame {
        igdb_id: 1,
        name: "Control".into(),
        genres: vec!["Shooter".into()],
        themes: vec![],
        keywords: keywords.iter().map(|k| k.to_string()).collect(),
        perspectives: vec![],
        modes: vec![],
        summary: None,
        rating: None,
        rating_count: None,
        similarity: 1.0,
    };
    app.db.put_igdb(control, Some(&igdb)).unwrap();
    let g = app.get(&format!("/api/games/{control}")).await;
    assert_eq!(g["tags_by_source"]["igdb"], json!(["psychic powers", "steampunk", "Shooter"]));
}

#[tokio::test]
async fn merge_and_split_endpoints() {
    let app = test_app().await;
    let (hades, celeste) = (app.id_of("Hades").await, app.id_of("Celeste").await);
    assert_eq!(app.call("POST", "/api/titles/merge", Some(json!({ "ids": [hades] }))).await.0, StatusCode::BAD_REQUEST);
    let (status, merged) = app.call("POST", "/api/titles/merge", Some(json!({ "ids": [hades, celeste] }))).await;
    assert_eq!(status, StatusCode::OK, "{merged}");
    assert_eq!(merged["copies"].as_array().unwrap().len(), 2);
    assert_eq!(app.get("/api/games").await["total"], 4);

    let tid = merged["id"].as_i64().unwrap();
    let copy = merged["copies"].as_array().unwrap().iter().find(|c| c["name"] == "Celeste").unwrap()["game_id"].as_i64().unwrap();
    let wrong = app.call("POST", &format!("/api/titles/{celeste}9/split"), Some(json!({ "game_id": copy }))).await;
    assert_eq!(wrong.0, StatusCode::NOT_FOUND, "the copy must belong to the title in the URL");
    let (status, v) = app.call("POST", &format!("/api/titles/{tid}/split"), Some(json!({ "game_id": copy }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["detached"]["name"], "Celeste");
    assert_eq!(app.get("/api/games").await["total"], 5);
}

/// The first id survives a merge with its status
/// and note, and a split afterwards leaves them; repeats are refused.
#[tokio::test]
async fn merge_keeps_the_first_title() {
    let app = test_app().await;
    let (stardew, celeste, hades) = (app.id_of("Stardew Valley").await, app.id_of("Celeste").await, app.id_of("Hades").await);
    for ids in [json!([hades, hades]), json!([hades, celeste, hades])] {
        let (status, v) = app.call("POST", "/api/titles/merge", Some(json!({ "ids": ids }))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{ids}: {v}");
    }
    app.db.set_override(PlayerId::FIRST, celeste, Some(crate::db::Override::Abandoned), None).unwrap();
    assert_eq!(app.call("POST", "/api/titles/merge", Some(json!({ "ids": [stardew, celeste] }))).await.0, StatusCode::OK);
    app.db.set_override(PlayerId::FIRST, hades, Some(crate::db::Override::Done), Some("the one I meant")).unwrap();
    let (status, merged) = app.call("POST", "/api/titles/merge", Some(json!({ "ids": [hades, stardew] }))).await;
    assert_eq!(status, StatusCode::OK, "{merged}");
    let expect = |v: &Value, copies: usize| {
        assert_eq!(v["id"], hades);
        assert_eq!((&v["name"], &v["appid"]), (&json!("Hades"), &json!(1145360)));
        assert_eq!((&v["override"], &v["override_note"]), (&json!("done"), &json!("the one I meant")), "{v}");
        assert_eq!(v["copies"].as_array().unwrap().len(), copies);
    };
    expect(&merged, 3);
    let copy = merged["copies"].as_array().unwrap().iter().find(|c| c["name"] == "Celeste").unwrap()["game_id"].as_i64().unwrap();
    let (status, v) = app.call("POST", &format!("/api/titles/{hades}/split"), Some(json!({ "game_id": copy }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    expect(&v["title"], 2);
}

/// A pick request over the limits is refused before
/// anything is ranked or sent.
#[tokio::test]
async fn pick_requests_have_limits() {
    let app = test_app().await;
    let long = "a".repeat(crate::recommend::MAX_PROMPT_CHARS + 1);
    let many: Vec<String> = (0..=crate::recommend::MAX_AVOID).map(|i| format!("reason {i}")).collect();
    let ids: Vec<i64> = (0..=crate::recommend::MAX_EXCLUDE as i64).collect();
    for (body, says) in [
        (json!({ "prompt": long }), "2001 characters"),
        (json!({ "prompt": "x", "avoid": many }), "at most 50 reasons"),
        (json!({ "prompt": "x", "avoid": ["b".repeat(301)] }), "at most 300 characters"),
        (json!({ "prompt": "x", "exclude_ids": ids }), "at most 500 games"),
    ] {
        let (status, v) = app.call("POST", "/api/picks", Some(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
        assert!(v.to_string().contains(says), "{v}");
    }
    assert_eq!(app.get("/api/picks/history").await, json!([]), "nothing was stored");
    let (status, v) = app.call("POST", "/api/picks", Some(json!({ "prompt": "é".repeat(2000), "count": 1 }))).await;
    assert_eq!(status, StatusCode::OK, "2,000 characters, not bytes: {v}");

    // Help me decide takes what was asked to the model too, under the same limit (B-AI-7).
    let (hades, celeste) = (app.id_of("Hades").await, app.id_of("Celeste").await);
    let calls = app.db.ai_usage_rows(0).unwrap().iter().map(|r| r.totals.calls).sum::<u64>();
    let (status, v) = app.call("POST", "/api/versus/decide", Some(json!({ "ids": [hades, celeste], "prompt": long }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert!(v.to_string().contains("2001 characters"), "{v}");
    assert_eq!(app.db.ai_usage_rows(0).unwrap().iter().map(|r| r.totals.calls).sum::<u64>(), calls, "no call was made");
}

#[tokio::test]
async fn picks_round_trip_and_reroll() {
    let app = test_app().await;
    let portal = app.id_of("Portal 2").await;
    // 21 hours of an 8.5-hour story: likely finished.
    app.db.put_hltb(portal, Some(&crate::db::HltbRow { main_story: Some(8.5), ..Default::default() })).unwrap();
    let (status, batch) = app.call("POST", "/api/picks", Some(json!({ "prompt": "something short", "count": 2 }))).await;
    assert_eq!(status, StatusCode::OK, "{batch}");
    let first: Vec<i64> = batch["picks"].as_array().unwrap().iter().map(|p| p["id"].as_i64().unwrap()).collect();
    assert_eq!(first.len(), 2);
    assert!(!first.contains(&portal), "finished games are never candidates");
    assert_eq!(batch["model"], "fake-model");

    let (status, again) =
        app.call("POST", "/api/picks", Some(json!({ "prompt": "something short", "count": 2, "exclude_ids": first }))).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    let second: Vec<i64> = again["picks"].as_array().unwrap().iter().map(|p| p["id"].as_i64().unwrap()).collect();
    assert!(second.iter().all(|id| !first.contains(id)), "a reroll avoids turned-down games: {first:?} then {second:?}");

    let history = app.get("/api/picks/history").await;
    assert_eq!(history.as_array().unwrap().len(), 2);
}

/// A lineup's cards: Play is recorded, Not tonight puts the next spare in
/// at once, "It's too ___" asks the AI for a spare, and when the spares run
/// out, new ones fill the empty slot. Each answers with the lineup.
#[tokio::test]
async fn a_lineup_takes_actions_and_new_spares() {
    let app = test_app().await;
    let (status, batch) = app.call("POST", "/api/picks", Some(json!({ "prompt": "something short", "count": 1 }))).await;
    assert_eq!(status, StatusCode::OK, "{batch}");
    let ids = |b: &Value, list: &str| -> Vec<i64> { b[list].as_array().unwrap().iter().map(|p| p["id"].as_i64().unwrap()).collect() };
    let (first, spares) = (ids(&batch, "picks"), ids(&batch, "spares"));
    assert_eq!((batch["shown"].as_u64(), first.len(), spares.len()), (Some(1), 1, 2), "twice as many spares as picks");
    let id = batch["batch_id"].as_str().unwrap().to_string();
    let url = |what: &str| format!("/api/picks/{id}/{what}");

    let (status, after) = app.call("POST", &url("feedback"), Some(json!({ "title_id": first[0], "action": "chosen" }))).await;
    assert_eq!((status, after["picks"][0]["feedback"]["action"].as_str()), (StatusCode::OK, Some("chosen")), "{after}");
    let (status, after) = app.call("POST", &url("feedback"), Some(json!({ "title_id": first[0], "action": "not_tonight" }))).await;
    assert_eq!(status, StatusCode::OK, "{after}");
    assert_eq!((ids(&after, "picks"), ids(&after, "removed")), (vec![spares[0]], first.clone()));

    let (status, out) = app.call("POST", &url("too"), Some(json!({ "title_id": spares[0], "word": "long" }))).await;
    assert_eq!(status, StatusCode::OK, "{out}");
    assert_eq!((out["swaps"][0]["into"].as_i64(), &out["ai_error"]), (Some(spares[1]), &Value::Null));
    assert_eq!(ids(&out["batch"], "picks"), [spares[1]]);

    // The last spare goes; new ones fill the slot.
    let (_, after) = app.call("POST", &url("feedback"), Some(json!({ "title_id": spares[1], "action": "not_for_me" }))).await;
    assert_eq!(after["picks"], json!([]));
    let (status, more) = app.call("POST", &url("more"), Some(json!({}))).await;
    assert_eq!(status, StatusCode::OK, "{more}");
    let filled = ids(&more, "picks");
    assert_eq!(filled.len(), 1);
    assert!(!first.contains(&filled[0]) && !spares.contains(&filled[0]), "every game the lineup held is left out");
    let history = app.get("/api/picks/history?limit=1").await;
    assert_eq!(history[0]["removed"].as_array().unwrap().len(), 3);

    let (status, _) = app.call("POST", "/api/picks/nope/feedback", Some(json!({ "title_id": 1, "action": "chosen" }))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, v) = app.call("POST", &url("feedback"), Some(json!({ "title_id": filled[0], "action": "meh" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    let (status, _) = app.call("POST", &url("too"), Some(json!({ "title_id": filled[0], "word": "  " }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// With no AI set up, a roll comes from UpNext's own ranking, says what it
/// was ranked by, and keeps that with the lineup; "It's too ___" swaps with
/// no call and no failure. With an AI set up, "Pick without AI" does the same.
#[tokio::test]
async fn picks_work_without_ai() {
    let app = test_app().await;
    let (status, v) = app.call("POST", "/api/picks", Some(json!({ "prompt": "something short", "count": 1, "without_ai": true }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!((v["model"].clone(), v["attempts"].as_i64()), (Value::Null, Some(0)));
    assert!(v["ranked_by"].as_array().is_some_and(|b| !b.is_empty()), "{v}");
    assert!(app.db.ai_usage_rows(0).unwrap().is_empty(), "the fake model wasn't called");

    // An Anthropic key that isn't there: no AI set up.
    let (status, s) = app.call("PUT", "/api/settings", Some(json!({ "llm_backend": "api-key" }))).await;
    assert_eq!(status, StatusCode::OK, "{s}");
    assert_eq!(app.get("/api/config").await["claude_available"], false);
    let (status, batch) = app.call("POST", "/api/picks", Some(json!({ "prompt": "something short", "count": 1 }))).await;
    assert_eq!(status, StatusCode::OK, "{batch}");
    let by = batch["ranked_by"].as_array().unwrap();
    assert_eq!(by.last().unwrap(), "a little chance, so each lineup differs", "{by:?}");
    assert!(!batch["picks"][0]["reason"].as_str().unwrap().is_empty());
    let id = batch["batch_id"].as_str().unwrap();
    let pick = batch["picks"][0]["id"].as_i64().unwrap();
    let (status, out) = app.call("POST", &format!("/api/picks/{id}/too"), Some(json!({ "title_id": pick, "word": "long" }))).await;
    assert_eq!((status, &out["ai_error"]), (StatusCode::OK, &Value::Null), "{out}");
    let history = app.get("/api/picks/history").await;
    let kept = history.as_array().unwrap().iter().find(|b| b["batch_id"] == batch["batch_id"]).unwrap();
    assert_eq!(kept["ranked_by"], batch["ranked_by"]);
    // Help me decide stays an AI feature, and says why it can't.
    let (hades, celeste) = (app.id_of("Hades").await, app.id_of("Celeste").await);
    let (status, v) = app.call("POST", "/api/versus/decide", Some(json!({ "ids": [hades, celeste] }))).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    assert!(v["error"].as_str().unwrap().starts_with("no Anthropic API key"), "{v}");
    assert!(app.db.ai_usage_rows(0).unwrap().is_empty(), "no call was tried");
}

/// Every call is recorded with its tokens: a pick with its batch (a pick
/// whose first answer was turned down made two calls), Help me decide and
/// the Settings test. The usage endpoint adds them up.
#[tokio::test]
async fn ai_calls_are_recorded_and_added_up() {
    let app = test_app().await;
    let one =
        json!({ "input_tokens": 1000, "output_tokens": 50, "cache_read_tokens": 400, "cache_write_tokens": 0, "cache_write_1h_tokens": 0 });
    let (status, batch) = app.call("POST", "/api/picks", Some(json!({ "prompt": "something short", "count": 2 }))).await;
    assert_eq!(status, StatusCode::OK, "{batch}");
    assert_eq!(batch["usage"], one);

    let (status, retried) = app.call("POST", "/api/picks", Some(json!({ "prompt": "answer wrong first", "count": 2 }))).await;
    assert_eq!(status, StatusCode::OK, "{retried}");
    assert_eq!(retried["attempts"], 2);
    assert_eq!((retried["usage"]["input_tokens"].as_u64(), retried["usage"]["output_tokens"].as_u64()), (Some(2000), Some(100)));

    let history = app.get("/api/picks/history").await;
    let usage_of = |id: &Value| history.as_array().unwrap().iter().find(|b| &b["batch_id"] == id).unwrap()["usage"].clone();
    assert_eq!(usage_of(&batch["batch_id"]), one, "each call was recorded with its batch");
    assert_eq!(usage_of(&retried["batch_id"])["input_tokens"], 2000, "both calls of the retried pick");

    let (hades, celeste) = (app.id_of("Hades").await, app.id_of("Celeste").await);
    let (status, decided) = app.call("POST", "/api/versus/decide", Some(json!({ "ids": [hades, celeste] }))).await;
    assert_eq!(status, StatusCode::OK, "{decided}");
    assert_eq!(decided["usage"], one);
    let (_, tested) = app.call("POST", "/api/llm/test", None).await;
    assert_eq!((&tested["ok"], &tested["usage"]), (&json!(true), &one));

    let usage = app.get("/api/ai/usage?days=7").await;
    assert_eq!((&usage["backend"], &usage["priced"]), (&json!("openai"), &json!(false)));
    assert_eq!(usage["today"]["calls"], 5);
    assert_eq!(usage["today"]["input_tokens"], 5000);
    assert_eq!(usage["today"]["cache_read_tokens"], 2000);
    assert_eq!(usage["today"]["cost_usd"], Value::Null, "nothing here is charged");
    assert_eq!(usage["total"], usage["today"]);
    let kinds: Vec<(&str, u64)> =
        usage["by_kind"].as_array().unwrap().iter().map(|k| (k["kind"].as_str().unwrap(), k["calls"].as_u64().unwrap())).collect();
    assert_eq!(kinds, [("pick", 3), ("decide", 1), ("test", 1)]);
    assert_eq!(usage["by_model"][0]["model"], "fake-model");
    let days = usage["by_day"].as_array().unwrap();
    assert_eq!(days.len(), 7);
    assert_eq!(days[6]["calls"], 5, "today is last");
    assert_eq!(days[0]["calls"], 0);
}

/// With no history, the estimates use the built-in rates; with a few calls
/// of a kind, their averages. The one-time run counts only titles without
/// a current summary.
#[tokio::test]
async fn estimates_use_history_once_there_is_some() {
    let app = test_app().await;
    let review = |text: &str| crate::db::ReviewText {
        source: "steam".into(),
        positive: Some(true),
        rating: None,
        votes: 3,
        hours: Some(10),
        title: None,
        text: text.into(),
        posted: None,
    };
    let text = "A long enough review about what the game is like to play, for a summary.";
    for appid in [620, 413150, 504230, 1145360] {
        app.db.put_review_texts(appid, Some(&[review(text)])).unwrap();
    }
    let (celeste, hades) = (app.id_of("Celeste").await, app.id_of("Hades").await);
    app.db.put_digest(celeste, "Current.", "", &Default::default(), "claude-sonnet-5-5", crate::digest::DIGEST_VERSION).unwrap();
    app.db.put_digest(hades, "Old.", "", &Default::default(), "claude-sonnet-5-5", 1).unwrap();

    let e = app.get("/api/ai/estimates").await;
    assert_eq!((&e["backend"], &e["priced"]), (&json!("openai"), &json!(false)));
    let feature = |e: &Value, kind: &str| e["features"].as_array().unwrap().iter().find(|f| f["kind"] == kind).unwrap().clone();
    let summaries = feature(&e, "review_summaries");
    assert_eq!(summaries["rate"]["basis"], "built_in");
    assert_eq!(summaries["rate"]["seconds_per_call"], Value::Null, "a local model's speed isn't known before it's used");
    let one_time = &summaries["one_time"];
    assert_eq!(one_time["items"], 3, "Portal 2, Stardew Valley and Hades's old summary; not Celeste's current one");
    assert_eq!((&one_time["calls"], &one_time["basis"]), (&json!(1), &json!("library")));
    let per_title = summaries["rate"]["input_tokens"].as_u64().unwrap();
    assert_eq!(one_time["input_tokens"].as_u64(), Some(3 * per_title));
    assert_eq!(one_time["cost_usd"], Value::Null);
    assert_eq!(summaries["monthly"]["basis"], "assumed", "a library a day old has no history");
    assert_eq!(feature(&e, "pick")["one_time"], Value::Null, "picks happen when asked");
    assert_eq!(feature(&e, "pick")["monthly"]["items"], 20);

    for _ in 0..3 {
        app.db
            .ai_call_add(&crate::db::AiCall {
                at: crate::db::now(),
                kind: crate::db::AiKind::ReviewSummaries,
                backend: "openai".into(),
                model: Some("fake-model".into()),
                error: None,
                duration_ms: 90_000,
                usage: Some(crate::llm::Usage { input_tokens: 32_000, output_tokens: 4_000, ..Default::default() }),
                cost_usd: None,
                api_usd: None,
                player_id: None,
                ai_account_id: None,
                would_cost_usd: None,
                items: Some(8),
                batch_id: None,
                job_id: None,
            })
            .unwrap();
    }
    let summaries = feature(&app.get("/api/ai/estimates").await, "review_summaries");
    assert_eq!(summaries["rate"]["basis"], "history");
    assert_eq!((summaries["rate"]["input_tokens"].as_u64(), summaries["rate"]["output_tokens"].as_u64()), (Some(4_000), Some(500)));
    assert_eq!(summaries["rate"]["seconds_per_call"], 90.0);
    assert_eq!(summaries["one_time"]["input_tokens"], 12_000);
    assert_eq!(summaries["one_time"]["minutes"], 1.5, "one call of 90 seconds");
}

#[tokio::test]
async fn picks_report_an_unreachable_model() {
    // Nothing listens on this port once the listener is dropped.
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let app = test_app_with(Some(format!("http://127.0.0.1:{port}/v1")), None).await;
    let (status, v) = app.call("POST", "/api/picks", Some(json!({ "prompt": "anything" }))).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    assert!(v["error"].as_str().unwrap().contains("cannot reach"), "{v}");
}

/// The tray's settings: closing the window keeps the
/// server running unless turned off, and Start with Windows is off until
/// turned on. On Windows turning it on writes the Run key first, which needs
/// `upnext-app.exe` beside the program; the test binary has none, so the
/// change is refused and nothing is saved. Elsewhere it's only stored.
#[tokio::test]
async fn the_windows_settings_save_and_start_with_windows_needs_the_app() {
    let app = test_app().await;
    let s = app.get("/api/settings").await;
    assert_eq!((&s["keep_running"], &s["start_with_windows"]), (&json!(true), &json!(false)));
    let (status, v) = app.call("PUT", "/api/settings", Some(json!({ "keep_running": false }))).await;
    assert_eq!((status, &v["keep_running"]), (StatusCode::OK, &json!(false)), "{v}");

    let (status, v) = app.call("PUT", "/api/settings", Some(json!({ "start_with_windows": true, "keep_running": true }))).await;
    if cfg!(windows) {
        assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
        assert!(v["error"].as_str().unwrap().contains("starts upnext-app.exe, and there is none in"), "{v}");
        let s = app.get("/api/settings").await;
        assert_eq!((&s["keep_running"], &s["start_with_windows"]), (&json!(false), &json!(false)), "nothing saved");
    } else {
        assert_eq!((status, &v["start_with_windows"], &v["keep_running"]), (StatusCode::OK, &json!(true), &json!(true)), "{v}");
    }
    let (status, v) = app.call("PUT", "/api/settings", Some(json!({ "start_with_windows": false }))).await;
    assert_eq!((status, &v["start_with_windows"]), (StatusCode::OK, &json!(false)), "{v}");
    assert_eq!(app.call("PUT", "/api/settings", Some(json!({ "start_with_windows": "maybe" }))).await.0, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn secrets_never_come_back() {
    let app = test_app().await;
    let key = "0123456789abcdef0123456789abcdef";
    let (status, v) = app.call("PUT", "/api/settings", Some(json!({ "steam_api_key": key }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["steam_api_key_set"], true);
    let upper = key.to_uppercase();
    assert!(!v.to_string().contains(&upper) && !v.to_string().contains(key));
    let settings = app.get("/api/settings").await.to_string();
    assert!(!settings.contains(&upper) && !settings.contains(key));
    assert_eq!(app.call("PUT", "/api/settings", Some(json!({ "no_such_setting": "1" }))).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(app.call("PUT", "/api/settings", Some(json!({ "steam_api_key": "short" }))).await.0, StatusCode::BAD_REQUEST);

    let resp = app
        .router
        .clone()
        .oneshot(Request::get("/api/export").header(header::HOST, "localhost").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let export = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8_lossy(&export);
    assert!(!text.contains(&upper) && !text.contains(key), "the export leaves keys out");
}

#[tokio::test]
async fn foreign_requests_are_refused() {
    let app = test_app().await;
    let req = |method: &str, host: &str| Request::builder().method(method).uri("/api/games").header(header::HOST, host);

    // DNS rebinding: a foreign name that resolves to 127.0.0.1.
    assert_eq!(app.send(req("GET", "evil.example:8787").body(Body::empty()).unwrap()).await.0, StatusCode::FORBIDDEN);
    assert_eq!(app.send(req("GET", "localhost:8787").body(Body::empty()).unwrap()).await.0, StatusCode::OK);
    assert_eq!(app.send(req("GET", "[::1]:8787").body(Body::empty()).unwrap()).await.0, StatusCode::OK);

    // Cross-site writes, by Origin or by the browser's Sec-Fetch-Site.
    let merge = |origin: Option<&str>, site: Option<&str>| {
        let mut r =
            Request::post("/api/titles/merge").header(header::HOST, "127.0.0.1:8787").header(header::CONTENT_TYPE, "application/json");
        if let Some(o) = origin {
            r = r.header(header::ORIGIN, o);
        }
        if let Some(s) = site {
            r = r.header("sec-fetch-site", s);
        }
        r.body(Body::from(json!({ "ids": [1] }).to_string())).unwrap()
    };
    assert_eq!(app.send(merge(Some("https://evil.example"), None)).await.0, StatusCode::FORBIDDEN);
    assert_eq!(app.send(merge(Some("null"), None)).await.0, StatusCode::FORBIDDEN);
    assert_eq!(app.send(merge(None, Some("cross-site"))).await.0, StatusCode::FORBIDDEN);
    // Same-origin writes reach the handler (which rejects one id as a bad request).
    assert_eq!(app.send(merge(Some("http://127.0.0.1:8787"), Some("same-origin"))).await.0, StatusCode::BAD_REQUEST);
}

/// Steam's side of OpenID: answers check_authentication with `is_valid`,
/// expecting to be asked `calls` times.
async fn fake_steam(valid: bool, calls: u64) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/openid/login"))
        .and(body_string_contains("openid.mode=check_authentication"))
        .and(body_string_contains("openid.sig=c2lnbmF0dXJl"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!("ns:{OPENID_NS}\nis_valid:{valid}\n")))
        .expect(calls)
        .mount(&server)
        .await;
    server
}

fn steam_error(reason: &str) -> String {
    format!("/settings?steam=error&reason={}", urlenc(reason))
}

#[tokio::test]
async fn steam_sign_in_round_trip() {
    let steam = fake_steam(true, 1).await;
    let app = test_app_with(None, Some(&steam.uri())).await;
    let back_here = |state: &str| format!("http://127.0.0.1:8787/api/steam/callback?state={state}");

    let (state, cookie) = app.steam_login().await;
    assert_eq!(app.steam_callback(&state, Some(&cookie), &back_here(&state), STEAM_OP_ENDPOINT).await, "/settings?steam=ok");
    assert_eq!(app.get("/api/settings").await["steam_id"], "76561190000000001");

    // A state works once; a replay is turned away before Steam is asked again.
    assert_eq!(
        app.steam_callback(&state, Some(&cookie), &back_here(&state), STEAM_OP_ENDPOINT).await,
        steam_error("that sign-in link expired or didn't start here; try again")
    );
}

#[tokio::test]
async fn steam_sign_in_refusals() {
    // Asked once: only the last assertion gets as far as Steam, which calls it forged.
    let steam = fake_steam(false, 1).await;
    let app = test_app_with(None, Some(&steam.uri())).await;
    let back_here = |state: &str| format!("http://127.0.0.1:8787/api/steam/callback?state={state}");
    let not_ours = steam_error("the sign-in reply wasn't for this app");

    let never_issued = "0".repeat(32);
    assert_eq!(
        app.steam_callback(&never_issued, None, &back_here(&never_issued), STEAM_OP_ENDPOINT).await,
        steam_error("that sign-in link expired or didn't start here; try again")
    );
    let (state, cookie) = app.steam_login().await;
    let cookie = Some(cookie.as_str());
    assert_eq!(
        app.steam_callback(&state, cookie, "https://evil.example/callback", STEAM_OP_ENDPOINT).await,
        not_ours,
        "another site's return_to"
    );
    let (state, cookie) = app.steam_login().await;
    let cookie = Some(cookie.as_str());
    assert_eq!(
        app.steam_callback(&state, cookie, &back_here(&state), "https://evil.example/openid/login").await,
        not_ours,
        "not from Steam"
    );
    let (state, cookie) = app.steam_login().await;
    assert_eq!(
        app.steam_callback(&state, Some(&cookie), &back_here(&state), STEAM_OP_ENDPOINT).await,
        steam_error("Steam rejected the sign-in assertion")
    );
    assert_eq!(app.get("/api/settings").await["steam_id"], Value::Null);
}

/// Over the built-in HTTPS, Steam sends the browser
/// back to an https address, where the sign-in's `Secure` cookie is sent.
#[tokio::test]
async fn steam_sign_in_over_https_comes_back_over_https() {
    let steam = fake_steam(true, 1).await;
    let mut app = test_app_with(None, Some(&steam.uri())).await;
    let peer = crate::server::listen::Peer { addr: std::net::SocketAddr::from(([127, 0, 0, 1], 50000)), tls: true };
    app.router = app.router.clone().layer(axum::extract::connect_info::MockConnectInfo(peer));
    let req = Request::get("/api/steam/login").header(header::HOST, "127.0.0.1:8787").body(Body::empty()).unwrap();
    let resp = app.router.clone().oneshot(req).await.unwrap();
    let to = resp.headers()[header::LOCATION].to_str().unwrap();
    assert!(to.contains(&urlenc("https://127.0.0.1:8787/api/steam/callback?state=")), "{to}");
    let cookie = resp.headers()[header::SET_COOKIE].to_str().unwrap();
    assert!(cookie.ends_with("; Secure"), "{cookie}");
    let state = regex::Regex::new(r"state%3D([0-9a-f]{32})").unwrap().captures(to).unwrap()[1].to_string();
    let back = format!("https://127.0.0.1:8787/api/steam/callback?state={state}");
    let cookie = cookie.split(';').next().unwrap().to_string();
    assert_eq!(app.steam_callback(&state, Some(&cookie), &back, STEAM_OP_ENDPOINT).await, "/settings?steam=ok");
}

/// A state works only in the browser it was given to, and no more than 16
/// wait at once: the oldest goes first.
#[tokio::test]
async fn steam_sign_ins_are_tied_to_their_browser_and_capped() {
    // Asked once: only the sign-in that comes back to its own browser gets as far as Steam.
    let steam = fake_steam(true, 1).await;
    let app = test_app_with(None, Some(&steam.uri())).await;
    let back_here = |state: &str| format!("http://127.0.0.1:8787/api/steam/callback?state={state}");
    let elsewhere = steam_error("that sign-in started in another browser, or a newer one replaced it; start it again here");

    let (state, _) = app.steam_login().await;
    assert_eq!(app.steam_callback(&state, None, &back_here(&state), STEAM_OP_ENDPOINT).await, elsewhere, "no cookie");
    let (state, _) = app.steam_login().await;
    let (_, other) = app.steam_login().await;
    let other = format!("theme=dark; {other}");
    assert_eq!(
        app.steam_callback(&state, Some(&other), &back_here(&state), STEAM_OP_ENDPOINT).await,
        elsewhere,
        "another sign-in's cookie"
    );
    assert_eq!(
        app.steam_callback(&state, Some(&other), &back_here(&state), STEAM_OP_ENDPOINT).await,
        steam_error("that sign-in link expired or didn't start here; try again"),
        "used up, even from the wrong browser"
    );

    // Sixteen more push out the first of them.
    let first = app.steam_login().await;
    let mut rest = vec![];
    for _ in 0..16 {
        rest.push(app.steam_login().await);
    }
    assert_eq!(app.state.remote.steam_sign_ins.lock().unwrap().len(), 16);
    assert_eq!(
        app.steam_callback(&first.0, Some(&first.1), &back_here(&first.0), STEAM_OP_ENDPOINT).await,
        steam_error("that sign-in link expired or didn't start here; try again")
    );
    let (state, cookie) = &rest[0];
    assert_eq!(app.steam_callback(state, Some(cookie), &back_here(state), STEAM_OP_ENDPOINT).await, "/settings?steam=ok");
}

#[tokio::test]
async fn enrich_sources_come_from_the_registry() {
    let app = test_app().await;
    let list = app.get("/api/enrich/sources").await;
    let list = list.as_array().unwrap();
    let ids: Vec<&str> = list.iter().map(|o| o["id"].as_str().unwrap()).collect();
    assert_eq!(ids, crate::enrich::sources::ids());
    let by_id = |id: &str| list.iter().find(|o| o["id"] == id).unwrap().clone();
    assert_eq!((by_id("hltb")["default"].clone(), by_id("hltb")["note"].clone()), (json!(false), json!("slow")));
    assert_eq!(by_id("reviews")["label"], "Steam review scores");
    // The test config has no keys.
    assert_eq!((by_id("rawg")["blocked"].clone(), by_id("igdb")["blocked"].clone()), (json!("needs a key"), json!("needs an IGDB login")));
    assert_eq!(by_id("wikidata")["blocked"], Value::Null);

    let (status, v) = app.call("PUT", "/api/settings", Some(json!({ "rawg_api_key": "0123456789abcdef0123456789abcdef" }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let list = app.get("/api/enrich/sources").await;
    let rawg = list.as_array().unwrap().iter().find(|o| o["id"] == "rawg").unwrap().clone();
    assert_eq!(rawg["blocked"], Value::Null, "a key saved in Settings unblocks it");
}

#[tokio::test]
async fn sync_sources_say_what_blocks_them() {
    let app = test_app().await;
    let list = app.get("/api/sync/sources").await;
    let got: Vec<(String, Value)> =
        list.as_array().unwrap().iter().map(|l| (l["id"].as_str().unwrap().to_string(), l["blocked"].clone())).collect();
    let want: Vec<(String, Value)> = [
        ("steam", Value::Null),
        ("heroic", Value::Null),
        ("gog", json!("not connected")),
        ("epic", json!("not signed in, and no Epic Games Launcher here")),
        ("ea", json!("no EA app games installed here")),
        ("ubisoft", json!("no Ubisoft Connect games installed here")),
        ("galaxy", json!("no path set")),
    ]
    .into_iter()
    .map(|(id, b)| (id.to_string(), b))
    .collect();
    assert_eq!(got, want, "no GOG or Epic sign-in, no Epic Games Launcher and no Galaxy path in the test config");
}

/// A terminal runs an enrich: the UI sees it, can't start another, can stop
/// it, and afterwards reads its failures and log and runs the failures again.
#[tokio::test]
async fn jobs_from_a_terminal_show_and_can_be_stopped_and_run_again() {
    use crate::db::{Failure, JobEnd, JobProgress, JobSource, JobStatus, Level, LogLine, now};
    // Stands in for every service, answering 404, so the retry never leaves the machine.
    let services = MockServer::start().await;
    let app = test_app_with(None, Some(&services.uri())).await;
    let portal = app.id_of("Portal 2").await;
    let id = app.db.job_begin("enrich", None, JobSource::Terminal, &json!({"refresh": false})).unwrap().unwrap();
    app.db.job_pulse(id, &JobProgress { total: 1600, done: 1040, message: "in parallel".into(), ..Default::default() }, &[], &[]).unwrap();

    let jobs = app.get("/api/jobs").await;
    assert_eq!(
        (jobs["current"]["id"].clone(), jobs["current"]["source"].clone(), jobs["current"]["done"].clone()),
        (json!(id), json!("terminal"), json!(1040))
    );
    let (status, v) = app.call("POST", "/api/jobs", Some(json!({ "kind": "sync" }))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(v["error"].as_str().unwrap().starts_with("an enrich started from a terminal just now is running (1,040 of 1,600)"), "{v}");
    let (status, v) = app.call("POST", "/api/jobs/current/cancel", None).await;
    assert_eq!((status, v["cancel_requested"].clone(), v["id"].clone()), (StatusCode::OK, json!(true), json!(id)));

    // The terminal stops, with one failure.
    let failure = Failure {
        lane: "Steam reviews".into(),
        source: "Steam reviews".into(),
        options: vec!["reviews".into(), "review_texts".into()],
        title_id: Some(portal),
        name: Some("Portal 2".into()),
        error: "HTTP 429 Too Many Requests".into(),
        at: now(),
    };
    let lines = [
        LogLine { at: now(), level: Level::Info, lane: None, message: "2 titles need enrichment".into(), title_id: None },
        LogLine {
            title_id: None,
            at: now(),
            level: Level::Error,
            lane: Some("Steam reviews".into()),
            message: "Steam reviews failed for Portal 2: HTTP 429".into(),
        },
    ];
    let end = JobEnd { status: JobStatus::Cancelled, summary: None, error: Some("cancelled") };
    app.db.job_end(id, &JobProgress::default(), &lines, &[failure], &end).unwrap();
    assert_eq!(app.get("/api/jobs").await["current"], Value::Null);

    let job = app.get(&format!("/api/jobs/{id}")).await;
    assert_eq!((job["status"].clone(), job["can_retry"].clone(), job["log_more"].clone()), (json!("cancelled"), json!(true), json!(false)));
    assert_eq!((job["failures"][0]["name"].clone(), job["failures"][0]["title_id"].clone()), (json!("Portal 2"), json!(portal)));
    assert_eq!(job["log"].as_array().unwrap().len(), 2);
    let errors = app.get(&format!("/api/jobs/{id}?level=error")).await;
    assert_eq!((errors["log"].as_array().unwrap().len(), errors["log"][0]["lane"].clone()), (1, json!("Steam reviews")));
    let page = app.get(&format!("/api/jobs/{id}?limit=1")).await;
    assert_eq!((page["log"].as_array().unwrap().len(), page["log_more"].clone()), (1, json!(true)));
    let (status, text) = app.call("GET", &format!("/api/jobs/{id}/log.txt"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(text.as_str().unwrap().contains("ERROR [Steam reviews] Steam reviews failed for Portal 2: HTTP 429\n"), "{text}");
    assert_eq!(app.call("GET", "/api/jobs/999999", None).await.0, StatusCode::NOT_FOUND);

    // Try these again: Steam reviews, for Portal 2 only.
    let (status, retry) = app.call("POST", &format!("/api/jobs/{id}/retry-failed"), None).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{retry}");
    assert_eq!((retry["source"].clone(), retry["options"]["titles"].clone()), (json!("ui"), json!([portal])));
    assert_eq!(
        (retry["options"]["reviews"].clone(), retry["options"]["hltb"].clone(), retry["options"]["wikidata"].clone()),
        (json!(true), json!(false), json!(false))
    );
    for _ in 0..200 {
        if app.get("/api/jobs").await["current"].is_null() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let jobs = app.get("/api/jobs").await;
    assert_eq!((jobs["recent"][0]["id"].clone(), jobs["recent"][0]["status"].clone()), (retry["id"].clone(), json!("done")), "{jobs}");

    assert!(app.get("/api/logs?level=error").await.is_array());
    assert_eq!(app.call("GET", "/api/logs?level=loud", None).await.0, StatusCode::BAD_REQUEST);
}

/// The Jobs table: schedules, options, runs by hand, and History's filters.
#[tokio::test]
async fn scheduled_jobs_can_be_changed_and_run_by_hand() {
    let app = test_app().await;
    let v = app.get("/api/jobs/scheduled").await;
    assert_eq!((v["enabled"].clone(), v["waiting_for_first_sync"].clone()), (json!(false), json!(false)), "{v}");
    let rows = v["tasks"].as_array().unwrap();
    let ids: Vec<&str> = rows.iter().map(|r| r["task"].as_str().unwrap()).collect();
    assert_eq!(
        ids,
        [
            "sync",
            "details",
            "reviews",
            "digests",
            "series_guides",
            "second_chances",
            "hidden_gems",
            "wishlist",
            "taste",
            "hltb",
            "backup",
            "verdicts"
        ]
    );
    let row = |v: &Value, id: &str| v["tasks"].as_array().unwrap().iter().find(|r| r["task"] == id).unwrap().clone();
    let details = row(&v, "details");
    assert_eq!(
        (details["schedule"].clone(), details["next_after"].clone(), details["last_run"].clone()),
        (json!("after:sync"), json!("sync"), Value::Null)
    );
    assert!(details["options"]["tags"] == json!(true) && details["options"].get("hltb").is_none(), "{details}");
    assert_eq!(row(&v, "sync")["options"]["galaxy"], json!(true));
    assert_eq!((row(&v, "digests")["uses_ai"].clone(), row(&v, "digests")["next_run_at"].clone()), (json!(true), Value::Null));
    assert_eq!(row(&v, "backup")["keep"], json!(4));
    let week = row(&v, "backup")["next_run_at"].as_i64().unwrap() - crate::db::now();
    assert!((7 * 86_400 - 60..=7 * 86_400).contains(&week), "a week from now: {week}");

    let (status, v) = app.call("PUT", "/api/jobs/scheduled/backup", Some(json!({ "schedule": "daily", "keep": 2 }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(
        (row(&v, "backup")["schedule"].clone(), row(&v, "backup")["keep"].clone(), row(&v, "backup")["chosen"].clone()),
        (json!("daily"), json!(2), json!(true))
    );
    let (status, v) = app.call("PUT", "/api/jobs/scheduled/details", Some(json!({ "schedule": "hourly" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(v["error"].as_str().unwrap().starts_with("a schedule is off, daily, weekly, monthly, or after:"), "{v}");
    assert_eq!(app.call("PUT", "/api/jobs/scheduled/everything", Some(json!({}))).await.0, StatusCode::NOT_FOUND);

    let (status, job) = app.call("POST", "/api/jobs", Some(json!({ "task": "backup" }))).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    assert_eq!(
        (job["kind"].clone(), job["task"].clone(), job["options"].clone()),
        (json!("backup"), json!("backup"), json!({ "keep": 2 }))
    );
    let ended = app.job_ended().await;
    assert_eq!(ended["status"], "done", "{ended}");
    assert!(ended["summary"].as_str().unwrap().starts_with("upnext-scheduled-"), "{ended}");
    let v = app.get("/api/jobs/scheduled").await;
    assert_eq!(row(&v, "backup")["last_run"]["id"], ended["id"]);
    let (status, _) = app.call("POST", "/api/jobs", Some(json!({ "task": "backup", "kind": "sync" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A backup asked for by kind counts as the backup task too.
    let (status, job) = app.call("POST", "/api/jobs", Some(json!({ "kind": "backup", "options": { "keep": 1 } }))).await;
    assert_eq!((status, job["task"].clone()), (StatusCode::ACCEPTED, json!("backup")), "{job}");
    assert_eq!(app.job_ended().await["status"], "done", "a second backup in the same second too");
    let backups: Vec<String> =
        std::fs::read_dir(app._dir.path().join("backups")).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    assert_eq!(backups.len(), 1, "keep 1: {backups:?}");

    let ran = |v: Value| v["recent"].as_array().unwrap().len();
    assert_eq!(ran(app.get("/api/jobs?task=backup").await), 2);
    assert_eq!(ran(app.get("/api/jobs?kind=backup&limit=1").await), 1);
    assert_eq!(ran(app.get("/api/jobs?task=none").await), 0);
    assert_eq!(ran(app.get("/api/jobs?failed=true").await), 0);
    assert_eq!(app.call("GET", "/api/jobs?task=nope", None).await.0, StatusCode::BAD_REQUEST);
}

/// The scheduler starts what's due, and only then: schedules on, a library
/// synced, nothing running.
#[tokio::test]
async fn the_scheduler_starts_what_is_due() {
    use crate::db::{JobEnd, JobProgress, JobSource, JobStatus, now};
    // Stands in for every service, so the details run never leaves the machine.
    let services = MockServer::start().await;
    let app = test_app_with(None, Some(&services.uri())).await;
    // Schedules started a minute ago, so a sync ending now is after that.
    let raw = rusqlite::Connection::open(app._dir.path().join("upnext.db")).unwrap();
    raw.execute("UPDATE meta SET value=?1 WHERE key='schedules_since'", [(now() - 60).to_string()]).unwrap();
    for (task, when) in [("sync", "off"), ("reviews", "off")] {
        let (status, v) = app.call("PUT", &format!("/api/jobs/scheduled/{task}"), Some(json!({ "schedule": when }))).await;
        assert_eq!(status, StatusCode::OK, "{v}");
    }
    let week_on = now() + 8 * 86_400;
    assert!(schedule_tick(&app.state, week_on).unwrap().is_none(), "schedules are off in the test config");
    app.call("PUT", "/api/settings", Some(json!({ "auto_sync": true }))).await;
    assert!(schedule_tick(&app.state, now() + 60).unwrap().is_none(), "nothing is due yet");

    let backup = schedule_tick(&app.state, week_on).unwrap().expect("the weekly backup is due a week on");
    assert_eq!((backup.task.as_deref(), backup.source), (Some("backup"), JobSource::Schedule));
    assert!(schedule_tick(&app.state, week_on).unwrap().is_none(), "one at a time");
    app.job_ended().await;
    assert!(schedule_tick(&app.state, now() + 60).unwrap().is_none(), "it just ran");

    // Fetch details follows a sync that ended done, from a terminal too.
    let sync = |status| {
        let id = app.db.job_begin("sync", Some("sync"), JobSource::Terminal, &json!({})).unwrap().unwrap();
        app.db.job_end(id, &JobProgress::default(), &[], &[], &JobEnd { status, summary: None, error: None }).unwrap();
    };
    sync(JobStatus::Failed);
    assert!(schedule_tick(&app.state, now()).unwrap().is_none(), "a failed sync starts nothing");
    sync(JobStatus::Done);
    let details = schedule_tick(&app.state, now()).unwrap().expect("details follow the sync");
    assert_eq!((details.task.as_deref(), details.kind.as_str()), (Some("details"), "enrich"));
    assert_eq!(details.options["hltb"], json!(false));
    app.job_ended().await;
    // Then the profile, which follows every sync too.
    let taste = schedule_tick(&app.state, now()).unwrap().expect("the profile follows the sync");
    assert_eq!((taste.task.as_deref(), taste.kind.as_str()), (Some("taste"), "taste"));
    app.job_ended().await;
    assert!(schedule_tick(&app.state, now()).unwrap().is_none(), "until the next sync");

    // Nothing runs by itself before the first sync.
    raw.execute("DELETE FROM meta WHERE key='last_sync'", []).unwrap();
    sync(JobStatus::Done);
    assert!(schedule_tick(&app.state, week_on).unwrap().is_none());
}

/// The app with its config changed: `base_cfg` is what `cfg()` starts from.
fn reconfigured(mut app: TestApp, change: impl FnOnce(&mut Config)) -> TestApp {
    change(&mut app.state.base_cfg);
    app.router = router(app.state.clone());
    app
}

/// A stand-in for the `claude` binary in `dir`: `/usage` prints `usage`
/// (a fixture), `auth status` a Max plan, and a call the real success
/// envelope. Every `/usage` read is counted in `usage-reads`.
#[cfg(unix)] // The stand-in CLIs are shell scripts.
fn fake_claude(dir: &std::path::Path, usage: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    let (usage, success) =
        (crate::testutil::fixture_path(&format!("llm/{usage}")), crate::testutil::fixture_path("llm/claude_cli_success.json"));
    let reads = dir.join("usage-reads");
    let script = format!(
        "#!/bin/sh\ncase \"$*\" in\n  *--version*) echo '2.1.285 (Claude Code)' ;;\n  'auth status') echo '{{\"loggedIn\":true,\"subscriptionType\":\"max\"}}' ;;\n  *\" /usage \"*) echo read >> '{}'; cat '{}' ;;\n  *) cat > /dev/null; cat '{}' ;;\nesac\n",
        reads.display(),
        usage.display(),
        success.display()
    );
    let bin = dir.join("claude");
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    bin.to_string_lossy().into_owned()
}

/// A local model has no plan and no prices; its calls have no dollar figure.
#[tokio::test]
async fn a_local_model_has_no_plan_or_prices() {
    let app = test_app().await;
    let plan = app.get("/api/ai/plan").await;
    assert_eq!((&plan["backend"], &plan["subscription"], &plan["reading"]), (&json!("openai"), &json!(false), &Value::Null));
    let prices = app.get("/api/ai/prices").await;
    assert_eq!((&prices["prices"], &prices["charged"]), (&Value::Null, &json!(false)));
    let (_, tested) = app.call("POST", "/api/llm/test", None).await;
    assert_eq!((&tested["api_usd"], &tested["cost_usd"]), (&Value::Null, &Value::Null));
}

/// On the Claude subscription: the plan's bars are read without spending
/// anything, a call is priced at API prices but not charged, and readings
/// are taken around it.
#[cfg(unix)]
#[tokio::test]
async fn a_subscription_shows_its_plan_and_api_prices() {
    let services = MockServer::start().await;
    let page = crate::testutil::fixture("prices/anthropic_pricing.md");
    Mock::given(path("/prices/anthropic")).respond_with(ResponseTemplate::new(200).set_body_string(page)).mount(&services).await;
    let app = test_app_with(None, Some(&services.uri())).await;
    let dir = app._dir.path().to_path_buf();
    let bin = fake_claude(&dir, "claude_cli_usage_command.json");
    let app = reconfigured(app, |cfg| {
        cfg.llm_backend = "claude-cli".into();
        cfg.claude_bin = bin;
    });

    let plan = app.get("/api/ai/plan").await;
    assert_eq!((&plan["subscription"], &plan["plan"], &plan["error"]), (&json!(true), &json!("max"), &Value::Null), "{plan}");
    let windows = plan["reading"]["windows"].as_array().unwrap();
    assert_eq!(windows.iter().map(|w| w["id"].as_str().unwrap()).collect::<Vec<_>>(), ["session", "week", "week:Fable"]);
    assert_eq!(windows[0]["used"], 20.0);
    assert_eq!(plan["measured"], Value::Null, "nothing measured yet");
    app.get("/api/ai/plan").await;
    assert_eq!(std::fs::read_to_string(dir.join("usage-reads")).unwrap().lines().count(), 1, "a reading under 30 s old is reused");

    let (_, tested) = app.call("POST", "/api/llm/test", None).await;
    assert_eq!(tested["ok"], true, "{tested}");
    assert_eq!(tested["cost_usd"], Value::Null, "a subscription isn't charged per call");
    assert!((tested["api_usd"].as_f64().unwrap() - 0.00406).abs() < 1e-9, "what the CLI itself says it would cost: {tested}");
    crate::ai_plan::settle(&app.db).await;
    // The readings are the AI account's that ran the call.
    let account = app.db.ai_accounts_for(crate::db::PlayerId::FIRST).unwrap().iter().find(|a| a.in_use).map(|a| a.id);
    let readings = app.db.plan_readings("claude-cli", account, 0).unwrap();
    assert_eq!(readings.last().unwrap().last_call, app.db.ai_last_call_id().unwrap(), "a reading after the call");

    let prices = app.get("/api/ai/prices").await;
    assert_eq!(prices["charged"], false);
    assert_eq!(prices["prices"]["provider"], "anthropic");
    assert!(prices["prices"]["fetched_at"].is_i64(), "read from the page: {prices}");
    assert_eq!(prices["prices"]["page_url"], "https://platform.claude.com/docs/en/about-claude/pricing");
    assert_eq!(prices["prices"]["models"]["claude-sonnet-5-5"]["input"], 2.0);
}

/// A fake `codex`: signed in with ChatGPT unless `signed-out` exists in
/// `dir`, a real model catalog, and an exec that keeps what it was given
/// (arguments, stdin, its folder and the two files in it) and prints the
/// real success's events.
#[cfg(unix)]
fn fake_codex(dir: &std::path::Path) -> String {
    use std::os::unix::fs::PermissionsExt;
    let d = dir.display();
    let script = format!(
        "#!/bin/sh\ncase \"$1\" in\n  --version) echo 'codex-cli 0.159.2' ;;\n  login) if [ -f '{d}/signed-out' ]; then echo 'Not logged in' >&2; exit 1; fi; echo 'Logged in using ChatGPT' >&2 ;;\n  debug) cat '{}' ;;\n  exec) printf '%s\\n' \"$@\" > '{d}/exec-args'; cat > '{d}/exec-stdin'; pwd > '{d}/exec-dir'; cp instructions.md '{d}/exec-instructions.md'; cp schema.json '{d}/exec-schema.json'; cat '{}' ;;\n  app-server) while read -r line; do echo \"$line\" >> '{d}/app-stdin'; case \"$line\" in *'\"id\":1,'*) echo '{{\"id\":1,\"result\":{{}}}}' ;; *rateLimits/read*) tr -d '\\n' < '{}'; echo ;; esac; done ;;\nesac\n",
        crate::testutil::fixture_path("llm/codex_debug_models.json").display(),
        crate::testutil::fixture_path("llm/codex_exec_success.jsonl").display(),
        crate::testutil::fixture_path("llm/codex_app_server_rate_limits.json").display(),
    );
    let bin = dir.join("codex");
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    bin.to_string_lossy().into_owned()
}

/// On the ChatGPT subscription: a call goes through `codex exec` with the
/// prompt on stdin and the instructions and strict schema in a private
/// folder that's removed after, is priced at OpenAI's list prices but not
/// charged, and stops before calling when Codex is signed out.
#[cfg(unix)]
#[tokio::test]
async fn the_chatgpt_subscription_runs_through_codex() {
    let services = MockServer::start().await;
    let page = crate::testutil::fixture("prices/openai_pricing.md");
    Mock::given(path("/prices/openai")).respond_with(ResponseTemplate::new(200).set_body_string(page)).mount(&services).await;
    let app = test_app_with(None, Some(&services.uri())).await;
    let dir = app._dir.path().to_path_buf();
    let bin = fake_codex(&dir);
    let app = reconfigured(app, |cfg| {
        cfg.llm_backend = "codex-cli".into();
        cfg.codex_bin = bin;
    });

    let codex = app.get("/api/llm/codex").await;
    assert_eq!(
        (&codex["installed"], &codex["version"], &codex["login"]["signed_in"]),
        (&json!(true), &json!("codex-cli 0.159.2"), &json!(true))
    );
    assert_eq!(codex["models_from"], "codex");
    let ids: Vec<&str> = codex["models"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["gpt-6-astra", "gpt-6-luna", "gpt-5.5"]);
    assert_eq!(codex["families"], json!([{"id": "astra", "model": "gpt-6-astra"}, {"id": "luna", "model": "gpt-6-luna"}]));
    let settings = app.get("/api/settings").await;
    assert_eq!(
        (&settings["codex_model"], &settings["codex_digest_model"], &settings["codex_effort"]),
        (&json!("luna"), &json!("luna"), &Value::Null),
        "the newest Luna, whatever its version"
    );
    assert!(settings["llm_backends"].as_array().unwrap().contains(&json!("codex-cli")));

    let (status, _) = app.call("PUT", "/api/settings", Some(json!({"codex_effort": "turbo"}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = app.call("PUT", "/api/settings", Some(json!({"codex_effort": "low"}))).await;
    assert_eq!(status, StatusCode::OK);

    app.get("/api/ai/prices").await;
    let (_, tested) = app.call("POST", "/api/llm/test", None).await;
    assert_eq!(
        (&tested["ok"], &tested["backend"], &tested["model"]),
        (&json!(true), &json!("codex-cli"), &json!("gpt-6-luna")),
        "{tested}"
    );
    assert_eq!(tested["usage"]["input_tokens"], 4376);
    assert_eq!(tested["cost_usd"], Value::Null, "a subscription isn't charged per call");
    let at_list_prices = (4376.0 * 0.10 + 15.0 * 0.50) / 1e6;
    assert!((tested["api_usd"].as_f64().unwrap() - at_list_prices).abs() < 1e-12, "{tested}");

    let args = std::fs::read_to_string(dir.join("exec-args")).unwrap();
    let args: Vec<&str> = args.lines().collect();
    assert!(args.windows(2).any(|w| w == ["-m", "gpt-6-luna"]) && args.contains(&r#"model_reasoning_effort="low""#), "{args:?}");
    assert!(std::fs::read_to_string(dir.join("exec-stdin")).unwrap().contains(r#"Reply with {"ok": true}."#));
    let instructions = std::fs::read_to_string(dir.join("exec-instructions.md")).unwrap();
    assert!(instructions.starts_with("You check that an API connection works.") && instructions.contains("AGENTS.md"), "{instructions}");
    let schema: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("exec-schema.json")).unwrap()).unwrap();
    assert_eq!(schema["additionalProperties"], false);
    let call_dir = std::fs::read_to_string(dir.join("exec-dir")).unwrap();
    assert!(call_dir.contains("upnext-codex-") && !std::path::Path::new(call_dir.trim()).exists(), "the call's folder is removed");

    // The plan's bars, from the app-server.
    crate::ai_plan::settle(&app.db).await;
    let plan = app.get("/api/ai/plan").await;
    assert_eq!((&plan["subscription"], &plan["backend"], &plan["plan"]), (&json!(true), &json!("codex-cli"), &json!("plus")), "{plan}");
    assert_eq!(
        plan["reading"]["windows"][0],
        json!({"id": "session", "label": "5-hour limit", "used": 1.0, "resets": null, "resets_at": 1_790_826_486})
    );
    assert_eq!(plan["reading"]["windows"][1]["id"], "week");
    let sent = std::fs::read_to_string(dir.join("app-stdin")).unwrap();
    assert!(sent.contains(r#""method":"initialize""#) && sent.contains(r#""method":"account/rateLimits/read""#), "{sent}");
    let account = app.db.ai_accounts_for(crate::db::PlayerId::FIRST).unwrap().iter().find(|a| a.in_use).map(|a| a.id);
    let readings = app.db.plan_readings("codex-cli", account, 0).unwrap();
    assert!(!readings.is_empty() && readings.iter().all(|r| r.plan.as_deref() == Some("plus")), "{readings:?}");

    std::fs::write(dir.join("signed-out"), "").unwrap();
    std::fs::remove_file(dir.join("exec-args")).unwrap();
    let (_, tested) = app.call("POST", "/api/llm/test", None).await;
    assert_eq!(tested["ok"], false);
    assert!(tested["error"].as_str().unwrap().contains("run `codex login`"), "{tested}");
    assert!(!dir.join("exec-args").exists(), "no call while signed out");
    let config = app.get("/api/config").await;
    assert_eq!(config["claude_available"], false);
    assert!(config["llm_error"].as_str().unwrap().contains("codex login"), "{config}");
}

/// A CLI that sends "/usage" to the model, which spends usage, is caught
/// once and not asked again.
#[cfg(unix)]
#[tokio::test]
async fn a_usage_read_that_spends_is_not_repeated() {
    let app = test_app().await;
    let dir = app._dir.path().to_path_buf();
    let mut spent = crate::testutil::fixture_json("llm/claude_cli_usage_command.json");
    spent["num_turns"] = 1.into();
    std::fs::write(dir.join("spent.json"), spent.to_string()).unwrap();
    let bin = fake_claude(&dir, "claude_cli_usage_command.json");
    // Point the fake at the spending envelope instead.
    let script = std::fs::read_to_string(&bin).unwrap().replace(
        &crate::testutil::fixture_path("llm/claude_cli_usage_command.json").display().to_string(),
        &dir.join("spent.json").display().to_string(),
    );
    std::fs::write(&bin, script).unwrap();
    let app = reconfigured(app, |cfg| {
        cfg.llm_backend = "claude-cli".into();
        cfg.claude_bin = bin;
    });

    let plan = app.get("/api/ai/plan").await;
    assert!(plan["error"].as_str().unwrap().contains("spends usage"), "{plan}");
    let plan = app.get("/api/ai/plan").await;
    assert!(plan["error"].as_str().unwrap().starts_with("UpNext stopped reading"), "{plan}");
    assert_eq!(std::fs::read_to_string(dir.join("usage-reads")).unwrap().lines().count(), 1, "asked once");
}

impl TestApp {
    /// A response's status, headers and raw bytes (a download).
    async fn raw(&self, uri: &str) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
        let req = Request::get(uri).header(header::HOST, "127.0.0.1:8787").body(Body::empty()).unwrap();
        let resp = self.router.clone().oneshot(req).await.unwrap();
        let (status, headers) = (resp.status(), resp.headers().clone());
        (status, headers, to_bytes(resp.into_body(), usize::MAX).await.unwrap().to_vec())
    }

    /// Upload a file to import, as the Backups section does.
    async fn upload(&self, name: &str, bytes: Vec<u8>) -> (StatusCode, Value) {
        let req = Request::post(format!("/api/backups/import?name={name}"))
            .header(header::HOST, "127.0.0.1:8787")
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .body(Body::from(bytes))
            .unwrap();
        self.send(req).await
    }

    fn backups_dir(&self) -> std::path::PathBuf {
        crate::backups::dir_for(&self.state.base_cfg.db_path)
    }

    /// Settings and the Steam profile, read straight from the database.
    fn keys(&self) -> (Vec<String>, Option<String>) {
        let mut keys: Vec<String> = self.db.settings_all(PlayerId::FIRST).unwrap().into_keys().collect();
        keys.sort();
        (keys, self.db.meta_get("profile").unwrap())
    }
}

fn sign_in(db: &Db) {
    db.setting_set(PlayerId::FIRST, "steam_api_key", Some("F493AAAAAAAAAAAAAAAAAAAAAAAA9BE3")).unwrap();
    db.setting_set(PlayerId::FIRST, "steam_id", Some("76561190000000001")).unwrap();
    db.meta_set("profile", r#"{"persona_name":"Sam","avatar":"https://avatars.example/sam.jpg"}"#).unwrap();
}

fn games_in(file: &[u8]) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("copy.db");
    std::fs::write(&path, file).unwrap();
    let c = rusqlite::Connection::open(&path).unwrap();
    let mut stmt = c.prepare("SELECT name FROM titles ORDER BY name").unwrap();
    stmt.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
}

#[tokio::test]
async fn backups_are_made_listed_downloaded_and_deleted() {
    let app = test_app().await;
    sign_in(&app.db);
    app.db.setting_set(PlayerId::FIRST, "theme", Some("ember")).unwrap();
    let list = app.get("/api/backups").await;
    assert_eq!(
        (list["backups"].clone(), list["schema"].clone(), list["keep_scheduled"].clone(), list["keep_automatic"].clone()),
        (json!([]), json!(crate::db::SCHEMA_VERSION), json!(4), json!(5))
    );
    assert_eq!(list["oldest"], json!(crate::db::BASELINE_VERSION), "the oldest backup Restore takes");

    let (status, made) = app.call("POST", "/api/backups", Some(json!({ "note": "before trying the new prompt" }))).await;
    assert_eq!(status, StatusCode::CREATED, "{made}");
    assert_eq!((made["kind"].clone(), made["note"].clone()), (json!("manual"), json!("before trying the new prompt")));
    let name = made["name"].as_str().unwrap().to_string();
    assert!(name.starts_with("upnext-manual-"), "{name}");
    let (status, plain) = app.call("POST", "/api/backups", None).await;
    assert_eq!((status, plain["note"].clone()), (StatusCode::CREATED, Value::Null), "a note is optional");
    assert_eq!(app.call("POST", "/api/backups", Some(json!({ "note": "x".repeat(301) }))).await.0, StatusCode::BAD_REQUEST);
    let list = app.get("/api/backups").await;
    assert_eq!(list["backups"].as_array().unwrap().len(), 2);

    // Everything, for safekeeping: keys and sign-ins included.
    let (status, headers, everything) = app.raw(&format!("/api/backups/{name}/download?keys=true")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_DISPOSITION], format!("attachment; filename=\"{name}\"").as_str());
    assert!(everything.starts_with(b"SQLite format 3\0"));
    let dir = tempfile::tempdir().unwrap();
    let copy = |bytes: &[u8], file: &str| {
        let path = dir.path().join(file);
        std::fs::write(&path, bytes).unwrap();
        Db::open(&path, 120).unwrap()
    };
    let full = copy(&everything, "full.db");
    assert_eq!(full.settings_all(PlayerId::FIRST).unwrap()["steam_api_key"], "F493AAAAAAAAAAAAAAAAAAAAAAAA9BE3");
    assert!(full.meta_get("profile").unwrap().is_some());

    // Without keys and sign-ins, unless asked: safe to share.
    let (status, headers, keyless) = app.raw(&format!("/api/backups/{name}/download")).await;
    assert_eq!(status, StatusCode::OK);
    let stem = name.trim_end_matches(".db");
    assert_eq!(headers[header::CONTENT_DISPOSITION], format!("attachment; filename=\"{stem}-without-keys.db\"").as_str());
    let shared = copy(&keyless, "shared.db");
    let settings = shared.settings_all(PlayerId::FIRST).unwrap();
    assert_eq!(settings.keys().collect::<Vec<_>>(), ["theme"], "{settings:?}");
    assert_eq!(shared.meta_get("profile").unwrap(), None);
    assert_eq!(games_in(&keyless), games_in(&everything), "the library is all there");
    let (_, _, exported) = app.raw("/api/export").await;
    assert_eq!(copy(&exported, "export.db").settings_all(PlayerId::FIRST).unwrap().keys().collect::<Vec<_>>(), ["theme"]);

    assert_eq!(app.raw("/api/backups/upnext-nothing.db/download").await.0, StatusCode::NOT_FOUND);
    assert_eq!(app.raw("/api/backups/..%2Fupnext.db/download").await.0, StatusCode::NOT_FOUND);
    assert_eq!(app.call("DELETE", &format!("/api/backups/{name}"), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(app.call("DELETE", &format!("/api/backups/{name}"), None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(app.get("/api/backups").await["backups"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_restore_puts_a_backup_back_and_can_be_undone() {
    let app = test_app().await;
    app.db.setting_set(PlayerId::FIRST, "steam_api_key", Some("F493AAAAAAAAAAAAAAAAAAAAAAAA9BE3")).unwrap();
    let lost = app.db.token_create("old phone", crate::db::Scope::Full, "device").unwrap();
    let laptop = app.db.token_create("laptop", crate::db::Scope::Read, "token").unwrap();
    let (_, made) = app.call("POST", "/api/backups", Some(json!({ "note": "a good state" }))).await;
    let name = made["name"].as_str().unwrap().to_string();
    // The phone is lost after the backup, and its replacement paired.
    app.db.token_revoke(lost.info.id).unwrap();
    let paired = app.db.token_create("new phone", crate::db::Scope::Full, "device").unwrap();

    // After the backup: a new game, a new look, a new key, and a job.
    let tunic = ImportedGame {
        source: "amazon".into(),
        source_id: "a1".into(),
        launcher: "heroic".into(),
        name: "Tunic".into(),
        ..Default::default()
    };
    app.db.sync_source(PlayerId::FIRST, "heroic", "amazon", &[tunic]).unwrap();
    app.db.setting_set(PlayerId::FIRST, "theme", Some("ember")).unwrap();
    app.db.setting_set(PlayerId::FIRST, "rawg_api_key", Some("abcdef0123456789abcdef0123456789")).unwrap();
    let (status, _) = app.call("POST", "/api/jobs", Some(json!({ "kind": "backup", "options": { "keep": 4 } }))).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let scheduled = app.job_ended().await;
    assert_eq!(scheduled["status"], "done", "{scheduled}");
    assert!(names(&app.get("/api/games?limit=50").await["items"]).contains(&"Tunic".to_string()), "the library is cached with Tunic");

    let (status, job) = app.call("POST", &format!("/api/backups/{name}/restore"), None).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    assert_eq!((job["kind"].clone(), job["task"].clone()), (json!("restore"), Value::Null));
    let ended = app.job_ended().await;
    assert_eq!((ended["id"].clone(), ended["status"].clone()), (job["id"].clone(), json!("done")), "{ended}");
    let summary = ended["summary"].as_str().unwrap();
    assert!(summary.starts_with(&format!("Restored {name}; the database before it is upnext-pre-restore-")), "{summary}");

    let games = names(&app.get("/api/games?limit=50").await["items"]);
    assert!(!games.contains(&"Tunic".to_string()), "the library is the backup's, not the cached one: {games:?}");
    let settings = app.db.settings_all(PlayerId::FIRST).unwrap();
    assert_eq!(settings.get("theme"), None, "settings are the backup's");
    assert_eq!(settings["rawg_api_key"], "abcdef0123456789abcdef0123456789", "a key it didn't have is kept");
    let log = app.get(&format!("/api/jobs/{}", job["id"])).await;
    let lines: Vec<&str> = log["log"].as_array().unwrap().iter().map(|l| l["message"].as_str().unwrap()).collect();
    assert!(lines.iter().any(|l| l.starts_with("backed up the database as it was to upnext-pre-restore-")), "{lines:?}");
    assert!(lines.contains(&format!("kept this computer's rawg_api_key, which {name} doesn't have").as_str()), "{lines:?}");
    // The tokens are this install's, not the backup's: the lost phone stays
    // revoked, the new one keeps working.
    assert!(app.db.token_check(&lost.token).unwrap().is_none(), "a revoked token stays revoked");
    assert_eq!(app.db.token_check(&paired.token).unwrap().map(|t| t.id), Some(paired.info.id), "paired since the backup");
    assert!(app.db.token_check(&laptop.token).unwrap().is_some());
    let kept = format!("kept this computer's 2 API tokens in place of the 2 API tokens in {name}, so a token revoked since stays revoked");
    assert!(lines.contains(&kept.as_str()), "{lines:?}");
    let recent = app.get("/api/jobs").await["recent"].clone();
    assert_eq!(recent.as_array().unwrap().len(), 1, "History is the backup's, with the restore on top: {recent}");
    let journal: String =
        rusqlite::Connection::open(&app.state.base_cfg.db_path).unwrap().query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
    assert_eq!(journal, "wal");

    // Undo it: restore the backup it made first.
    let list = app.get("/api/backups").await;
    let before = list["backups"].as_array().unwrap().iter().find(|b| b["kind"] == "restore").unwrap().clone();
    assert_eq!(before["reason"], format!("Before restoring {name}").as_str());
    let (status, _) = app.call("POST", &format!("/api/backups/{}/restore", before["name"].as_str().unwrap()), None).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(app.job_ended().await["status"], "done");
    assert!(names(&app.get("/api/games?limit=50").await["items"]).contains(&"Tunic".to_string()));
    assert_eq!(app.db.settings_all(PlayerId::FIRST).unwrap()["theme"], "ember");
    let recent = app.get("/api/jobs").await["recent"].clone();
    let runs: Vec<(String, String)> = recent
        .as_array()
        .unwrap()
        .iter()
        .map(|j| (j["kind"].as_str().unwrap().to_string(), j["status"].as_str().unwrap().to_string()))
        .collect();
    assert_eq!(
        runs,
        [("restore".into(), "done".into()), ("restore".into(), "failed".into()), ("backup".into(), "done".into())],
        "the first restore was running when its backup was made: {recent}"
    );
    assert_eq!(recent[1]["error"], "Running when this backup was made, so the backup doesn't know how it ended.");
}

#[tokio::test]
async fn an_import_keeps_this_installs_keys() {
    let app = test_app().await;
    sign_in(&app.db);
    let phone = app.db.token_create("phone", crate::db::Scope::Full, "token").unwrap();
    // A database from another computer, without keys.
    let other = tempfile::tempdir().unwrap();
    let path = other.path().join("upnext.db");
    {
        let db = Db::open(&path, 120).unwrap();
        let game = ImportedGame {
            source: "steam".into(),
            source_id: "753640".into(),
            launcher: "steam".into(),
            name: "Outer Wilds".into(),
            appid: Some(753640),
            ..Default::default()
        };
        db.sync_source(PlayerId::FIRST, "steam", "steam", &[game]).unwrap();
        db.setting_set(PlayerId::FIRST, "theme", Some("daylight")).unwrap();
    }
    rusqlite::Connection::open(&path).unwrap().execute_batch("PRAGMA journal_mode=DELETE;").unwrap();

    let (status, job) = app.upload("old-laptop.db", std::fs::read(&path).unwrap()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    assert_eq!((job["kind"].clone(), job["options"]["name"].clone()), (json!("import"), json!("old-laptop.db")));
    let ended = app.job_ended().await;
    let summary = ended["summary"].as_str().unwrap_or_default();
    assert!(summary.starts_with("Imported old-laptop.db; the database before it is upnext-pre-import-"), "{ended}");
    assert_eq!(names(&app.get("/api/games").await["items"]), ["Outer Wilds"]);
    let (keys, profile) = app.keys();
    assert_eq!(keys, ["steam_api_key", "steam_id", "theme"], "this install's keys and sign-in, the import's theme");
    assert!(profile.unwrap().contains("Sam"), "the profile goes with the sign-in");
    assert!(app.db.token_check(&phone.token).unwrap().is_some(), "this install's tokens, as the import has none");
    let list = app.get("/api/backups").await;
    let before = list["backups"].as_array().unwrap().iter().find(|b| b["kind"] == "import").unwrap().clone();
    assert_eq!(before["reason"], "Before importing old-laptop.db");
    assert_eq!(
        games_in(&std::fs::read(app.backups_dir().join(before["name"].as_str().unwrap())).unwrap()).len(),
        5,
        "the old library is kept"
    );
    let left: Vec<String> = std::fs::read_dir(app.backups_dir()).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    assert!(left.iter().all(|n| !n.starts_with('.')), "no upload or work file is left: {left:?}");

    // Files it can't use are refused at once, and nothing changes.
    let refusal = |v: &Value| v["error"].as_str().unwrap_or_default().to_string();
    let (status, v) =
        app.upload("notes.txt", b"just some notes, long enough to have a header's worth of bytes in them, and more".to_vec()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(refusal(&v).starts_with("that file isn't a database"), "{v}");
    let (status, v) = app.upload("empty.db", vec![]).await;
    assert_eq!((status, refusal(&v)), (StatusCode::BAD_REQUEST, "the upload was empty; choose a database file".into()));
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch(&format!("PRAGMA user_version={};", crate::db::SCHEMA_VERSION + 1)).unwrap();
    drop(c);
    let (status, v) = app.upload("newer.db", std::fs::read(&path).unwrap()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(refusal(&v).contains("newer UpNext"), "{v}");
    rusqlite::Connection::open(&path).unwrap().execute_batch(&format!("PRAGMA user_version={};", crate::db::BASELINE_VERSION - 1)).unwrap();
    let (status, v) = app.upload("old.db", std::fs::read(&path).unwrap()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(refusal(&v).starts_with("that database is from a development build of UpNext made before its first release"), "{v}");
    let notes = other.path().join("notes.db");
    rusqlite::Connection::open(&notes).unwrap().execute_batch("CREATE TABLE notes(x); INSERT INTO notes VALUES (1);").unwrap();
    let (status, v) = app.upload("notes.db", std::fs::read(&notes).unwrap()).await;
    assert_eq!(
        (status, refusal(&v)),
        (StatusCode::BAD_REQUEST, "that's a SQLite database, but not UpNext's: it has no titles or games".into())
    );
    assert_eq!(names(&app.get("/api/games").await["items"]), ["Outer Wilds"]);
    let left: Vec<String> = std::fs::read_dir(app.backups_dir()).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    assert!(left.iter().all(|n| !n.starts_with('.')), "refused uploads are removed: {left:?}");
}

/// A library from before UpNext numbered its schema (v0) is refused when its
/// restore is asked for, before a job starts, as the other old ones are.
#[tokio::test]
async fn a_named_restore_of_an_unnumbered_library_is_refused_at_once() {
    let app = test_app().await;
    let dir = app.backups_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let name = "upnext-manual-20261001-101500.db";
    rusqlite::Connection::open(dir.join(name)).unwrap().execute_batch("CREATE TABLE games(id); CREATE TABLE titles(id);").unwrap();
    let listed = app.get("/api/backups").await;
    assert_eq!(listed["backups"][0]["schema"], json!(0), "{listed}");
    let (status, v) = app.call("POST", &format!("/api/backups/{name}/restore"), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    let why = v["error"].as_str().unwrap_or_default();
    assert!(why.starts_with("that database is from a development build of UpNext made before its first release (schema v0)"), "{v}");
    assert_eq!(app.get("/api/jobs").await["current"], Value::Null, "no restore job started");
}

#[tokio::test]
async fn restores_wait_for_jobs_and_answer_only_on_this_computer() {
    use crate::db::{JobEnd, JobProgress, JobSource, JobStatus};
    let app = test_app().await;
    let (_, made) = app.call("POST", "/api/backups", None).await;
    let name = made["name"].as_str().unwrap().to_string();
    let file = std::fs::read(app.backups_dir().join(&name)).unwrap();

    let id = app.db.job_begin("enrich", None, JobSource::Terminal, &json!({})).unwrap().unwrap();
    let (status, v) = app.call("POST", &format!("/api/backups/{name}/restore"), None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().starts_with("an enrich started from a terminal"), "{v}");
    assert_eq!(app.upload("copy.db", file.clone()).await.0, StatusCode::CONFLICT);
    let (status, again) = app.call("POST", "/api/backups", None).await;
    assert_eq!(status, StatusCode::CREATED, "backing up never waits: {again}");
    let end = JobEnd { status: JobStatus::Done, summary: Some("nothing"), error: None };
    app.db.job_end(id, &JobProgress::default(), &[], &[], &end).unwrap();

    // Listening beyond this computer: only requests from it get these,
    // whatever token another device has.
    let app = reconfigured(app, |c| c.host = "0.0.0.0".into());
    let full = app.db.token_create("phone", crate::db::Scope::Full, "token").unwrap().token;
    let bearer = format!("Bearer {full}");
    let from =
        |ip: [u8; 4]| app.router.clone().layer(axum::extract::connect_info::MockConnectInfo(std::net::SocketAddr::from((ip, 50000))));
    let lan = |r: axum::http::request::Builder| r.header(header::HOST, "den.lan:8787").header(header::AUTHORIZATION, &bearer);
    let download = || lan(Request::get(format!("/api/backups/{name}/download"))).body(Body::empty()).unwrap();
    let delete = || lan(Request::delete(format!("/api/backups/{name}"))).body(Body::empty()).unwrap();
    let import = || lan(Request::post("/api/backups/import")).body(Body::from(file.clone())).unwrap();
    for req in [download(), delete(), import()] {
        assert_eq!(from([192, 168, 0, 20]).oneshot(req).await.unwrap().status(), StatusCode::FORBIDDEN);
    }
    let list = lan(Request::get("/api/backups")).body(Body::empty()).unwrap();
    assert_eq!(from([192, 168, 0, 20]).oneshot(list).await.unwrap().status(), StatusCode::OK, "the list isn't secret");
    assert_eq!(app.send(download()).await.0, StatusCode::FORBIDDEN, "no peer address, not listening on loopback");
    let here = Request::get(format!("/api/backups/{name}/download")).header(header::HOST, "127.0.0.1:8787").body(Body::empty()).unwrap();
    assert_eq!(from([127, 0, 0, 1]).oneshot(here).await.unwrap().status(), StatusCode::OK);
}

/// A stand-in for OpenAI's API: `GET /v1/models` lists the saved models,
/// and chat completions answer from the request's schema (a test, Help me
/// decide, picks from the candidate list, summaries for the titles asked),
/// at the tier asked for. Every call used 1,000 tokens in (400 of them
/// cached) and 50 out. A key other than `key` gets a 401. The request
/// bodies are kept in `sent`.
async fn fake_openai(key: &'static str) -> (String, Arc<std::sync::Mutex<Vec<Value>>>) {
    use axum::http::HeaderMap;
    let sent = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let authorized =
        move |headers: &HeaderMap| headers.get("authorization").and_then(|v| v.to_str().ok()) == Some(&format!("Bearer {key}"));
    let rejected = || {
        let body = json!({"error": {"message": "Incorrect API key provided.", "type": "invalid_request_error", "code": "invalid_api_key", "param": null}});
        (StatusCode::UNAUTHORIZED, Json(body))
    };
    let models = move |headers: HeaderMap| async move {
        if !authorized(&headers) {
            return rejected();
        }
        (StatusCode::OK, Json(crate::testutil::fixture_json("llm/openai_models.json")))
    };
    let kept = sent.clone();
    let complete = move |headers: HeaderMap, Json(body): Json<Value>| {
        let kept = kept.clone();
        async move {
            if !authorized(&headers) {
                return rejected();
            }
            kept.lock().unwrap().push(body.clone());
            let prompt = body.pointer("/messages/1/content").and_then(Value::as_str).unwrap_or_default();
            let properties = body.pointer("/response_format/json_schema/schema/properties").cloned().unwrap_or_default();
            let ids = |pattern: &str| -> Vec<i64> {
                regex::Regex::new(pattern).unwrap().captures_iter(prompt).map(|c| c[1].parse().unwrap()).collect()
            };
            let answer = if properties.get("digests").is_some() {
                let item = |id: i64| {
                    json!({"id": id, "vibes": "Cozy.", "digest": "Praised for its calm.", "facets": [],
                    "praised": [], "criticized": [], "divisive": [], "notes": []})
                };
                json!({"digests": ids(r"(?m)^### #(\d+) ").into_iter().map(item).collect::<Vec<_>>()})
            } else if properties.get("picks").is_some() {
                let line = regex::Regex::new(r"(?m)^#(\d+) ([^|\[]+)").unwrap();
                let picks: Vec<Value> = line
                    .captures_iter(prompt)
                    .take(2)
                    .map(|c| json!({"id": c[1].parse::<i64>().unwrap(), "name": c[2].trim(), "reason": "Fits."}))
                    .collect();
                json!({"picks": picks})
            } else if properties.get("pick_id").is_some() {
                json!({"pick_id": ids(r"(?m)^#(\d+) ")[0], "reason": "Shorter."})
            } else {
                json!({"ok": true})
            };
            let tier = body.get("service_tier").and_then(Value::as_str).unwrap_or("default");
            (
                StatusCode::OK,
                Json(json!({"model": format!("{}-2026-08-01", body["model"].as_str().unwrap()), "service_tier": tier,
                "choices": [{"index": 0, "finish_reason": "stop", "message": {"role": "assistant", "content": answer.to_string(), "refusal": null}}],
                "usage": {"prompt_tokens": 1000, "completion_tokens": 50, "total_tokens": 1050, "prompt_tokens_details": {"cached_tokens": 400}}})),
            )
        }
    };
    let app = Router::new().route("/v1/models", get(models)).route("/v1/chat/completions", post(complete));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}/v1"), sent)
}

/// The OpenAI API key: its settings keep the key secret and out
/// of exports, its models come from the key's list with the newest Luna by
/// default, a pick goes at the standard price and summaries at Flex, each
/// held to a strict schema and charged at OpenAI's list prices.
#[tokio::test]
async fn an_openai_key_runs_picks_and_summaries() {
    const KEY: &str = "sk-proj-test-0123456789abcdef";
    let services = MockServer::start().await;
    let page = crate::testutil::fixture("prices/openai_pricing.md");
    Mock::given(path("/prices/openai")).respond_with(ResponseTemplate::new(200).set_body_string(page)).mount(&services).await;
    let (url, sent) = fake_openai(KEY).await;
    let app = reconfigured(test_app_with(None, Some(&services.uri())).await, |cfg| {
        cfg.llm_backend = "openai-key".into();
        cfg.openai_key_url = url;
    });

    let settings = app.get("/api/settings").await;
    assert_eq!(
        (&settings["openai_key_set"], &settings["openai_key_model"], &settings["openai_key_digest_model"], &settings["openai_key_effort"]),
        (&json!(false), &json!("luna"), &json!("luna"), &Value::Null)
    );
    assert!(settings["llm_backends"].as_array().unwrap().contains(&json!("openai-key")));
    let (_, tested) = app.call("POST", "/api/llm/test", None).await;
    assert!(tested["error"].as_str().unwrap().starts_with("no OpenAI API key"), "{tested}");
    let listed = app.get("/api/llm/openai").await;
    assert_eq!((&listed["key_set"], &listed["models_from"]), (&json!(false), &json!("built_in")));
    assert!(sent.lock().unwrap().is_empty(), "nothing sent without a key");

    assert_eq!(
        app.call("PUT", "/api/settings", Some(json!({"openai_key": "sk-ant-api03-0123456789abcdef"}))).await.0,
        StatusCode::BAD_REQUEST
    );
    let (status, saved) = app.call("PUT", "/api/settings", Some(json!({"openai_key": KEY, "openai_key_effort": "low"}))).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(
        (&saved["openai_key_set"], &saved["openai_key_source"], &saved["openai_key_effort"]),
        (&json!(true), &json!("app"), &json!("low"))
    );
    assert!(!saved.to_string().contains(KEY) && !app.get("/api/settings").await.to_string().contains(KEY));
    assert_eq!(app.call("PUT", "/api/settings", Some(json!({"openai_key_effort": "ultra"}))).await.0, StatusCode::BAD_REQUEST);

    let listed = app.get("/api/llm/openai").await;
    assert_eq!((&listed["key_set"], &listed["models_from"], &listed["models_error"]), (&json!(true), &json!("openai"), &Value::Null));
    assert_eq!(listed["models"][0], "gpt-6.1-sol", "newest first");
    let families: Vec<(&str, &str)> =
        listed["families"].as_array().unwrap().iter().map(|f| (f["id"].as_str().unwrap(), f["model"].as_str().unwrap())).collect();
    assert_eq!(families[..3], [("sol", "gpt-6.1-sol"), ("astra", "gpt-6-astra"), ("luna", "gpt-6-luna")]);

    // A test call: the newest Luna, charged at the standard price.
    app.get("/api/ai/prices").await;
    let (_, tested) = app.call("POST", "/api/llm/test", None).await;
    assert_eq!(
        (&tested["ok"], &tested["backend"], &tested["model"]),
        (&json!(true), &json!("openai-key"), &json!("gpt-6-luna")),
        "{tested}"
    );
    let standard = (600.0 * 0.10 + 400.0 * 0.01 + 50.0 * 0.50) / 1e6;
    assert!((tested["cost_usd"].as_f64().unwrap() - standard).abs() < 1e-12, "{tested}");

    let (status, batch) = app.call("POST", "/api/picks", Some(json!({"prompt": "something short", "count": 2}))).await;
    assert_eq!(status, StatusCode::OK, "{batch}");
    assert_eq!(batch["picks"].as_array().unwrap().len(), 2, "{batch}");
    let pick = sent.lock().unwrap().last().unwrap().clone();
    assert_eq!((&pick["model"], &pick["reasoning_effort"], &pick["store"]), (&json!("gpt-6-luna"), &json!("low"), &json!(false)));
    assert_eq!(pick["service_tier"], Value::Null, "a pick doesn't wait for Flex");
    assert_eq!(pick["response_format"]["json_schema"]["strict"], true);
    assert!(pick["response_format"]["json_schema"]["schema"]["properties"]["picks"].get("maxItems").is_none(), "made strict");

    // Summaries, as a job: at Flex, priced at Flex.
    let review = crate::db::ReviewText {
        source: "steam".into(),
        positive: Some(true),
        rating: None,
        votes: 3,
        hours: Some(10),
        title: None,
        text: "A long enough review about what the game is like to play, for a summary.".into(),
        posted: None,
    };
    for appid in [413150, 504230] {
        app.db.put_review_texts(appid, Some(std::slice::from_ref(&review))).unwrap();
    }
    // Looked up in the Steam store already, so the job only writes summaries.
    for appid in [620, 413150, 504230, 1145360] {
        app.db.put_details(appid, None).unwrap();
    }
    app.db.put_steam_match(app.id_of("Control").await, None).unwrap();
    let (status, job) = app.call("POST", "/api/jobs", Some(json!({"task": "digests"}))).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    let ended = app.job_ended().await;
    assert_eq!(ended["status"], "done", "{ended}");
    let summaries = sent.lock().unwrap().last().unwrap().clone();
    assert_eq!((&summaries["service_tier"], &summaries["model"]), (&json!("flex"), &json!("gpt-6-luna")));
    let stardew = app.id_of("Stardew Valley").await;
    assert_eq!(app.get(&format!("/api/games/{stardew}")).await["digest"], "Praised for its calm.");

    let usage = app.get("/api/ai/usage?days=7").await;
    assert_eq!((&usage["backend"], &usage["priced"]), (&json!("openai-key"), &json!(true)));
    let kind = |k: &str| usage["by_kind"].as_array().unwrap().iter().find(|x| x["kind"] == k).unwrap().clone();
    let flex = (600.0 * 0.05 + 400.0 * 0.005 + 50.0 * 0.25) / 1e6;
    assert!((kind("review_summaries")["cost_usd"].as_f64().unwrap() - flex).abs() < 1e-12, "half price: {usage}");
    assert!((kind("pick")["cost_usd"].as_f64().unwrap() - standard).abs() < 1e-12, "{usage}");
    let estimates = app.get("/api/ai/estimates").await;
    assert_eq!((&estimates["backend"], &estimates["priced"]), (&json!("openai-key"), &json!(true)));
    let feature = |k: &str| estimates["features"].as_array().unwrap().iter().find(|f| f["kind"] == k).unwrap().clone();
    assert_eq!(feature("review_summaries")["model"], "gpt-6-luna", "the family, priced as its newest model");
    assert!(feature("review_summaries")["monthly"]["cost_usd"].as_f64().is_some());

    let (_, _, exported) = app.raw("/api/export").await;
    assert!(!String::from_utf8_lossy(&exported).contains(KEY), "the export leaves the key out");
}

/// The taste profile: what you say, the job that counts patterns, the
/// sources with their deletes, and the export.
#[tokio::test]
async fn taste_profile_statements_sources_and_export() {
    let app = test_app().await;
    let portal = app.id_of("Portal 2").await;
    let v = app.get("/api/taste").await;
    assert_eq!(
        (v["favourites"].clone(), v["patterns"].clone(), v["task"]["schedule"].clone()),
        (json!([]), json!([]), json!("after:sync"))
    );
    assert_eq!(v["suggestions"][0]["name"], "Portal 2", "the most played");
    assert_eq!(v["rules"], json!({ "half_life_days": 365, "min_games": 4 }));
    assert!(v["subjects"].as_array().unwrap().iter().any(|s| s["about"] == "tag:Horror" && s["group"] == "Genres and themes"));

    // Something you said.
    let post = |body: Value| app.call("POST", "/api/taste/statements", Some(body));
    let (status, fav) = post(json!({ "kind": "favourite", "title_id": portal, "why": ["tag:Puzzle", "aspect:writing"] })).await;
    assert_eq!(status, StatusCode::CREATED, "{fav}");
    let (status, like) = post(json!({ "kind": "like", "about": "tag:horror", "note": "the slow kind" })).await;
    assert_eq!((status, like["about"].clone(), like["source"].clone()), (StatusCode::CREATED, json!("tag:Horror"), json!("stated")));
    assert_eq!(post(json!({ "kind": "hard_no", "about": "length:long" })).await.0, StatusCode::CREATED);
    assert_eq!(post(json!({ "kind": "context", "about": "weeknight", "why": 60 })).await.0, StatusCode::CREATED);
    for bad in [
        json!({ "kind": "pattern", "about": "drift:tag:Horror" }),
        json!({ "kind": "like", "about": "genre:RPG" }),
        json!({ "kind": "favourite", "title_id": 999_999 }),
        json!({ "kind": "context", "about": "who", "why": ["dog"] }),
        json!({ "kind": "loves" }),
    ] {
        assert_eq!(post(bad.clone()).await.0, StatusCode::BAD_REQUEST, "{bad}");
    }
    let v = app.get("/api/taste").await;
    assert_eq!(v["favourites"][0]["game"]["name"], "Portal 2");
    assert_eq!(v["favourites"][0]["why"][1], json!({ "key": "aspect:writing", "label": "the writing" }));
    assert_eq!(v["statements"].as_array().unwrap().len(), 2);
    assert_eq!(v["habits"][0]["value"], 60);
    assert!(v["suggestions"].as_array().unwrap().iter().all(|g| g["id"] != portal), "a favourite isn't suggested");

    // Edits and deletes.
    let like_uri = format!("/api/taste/statements/{}", like["id"]);
    let (status, row) = app.call("PUT", &like_uri, Some(json!({ "kind": "dislike" }))).await;
    assert_eq!((status, row["kind"].clone(), row["note"].clone()), (StatusCode::OK, json!("dislike"), json!("the slow kind")));
    let (status, row) = app.call("PUT", &like_uri, Some(json!({ "note": "" }))).await;
    assert_eq!((status, row["note"].clone()), (StatusCode::OK, Value::Null));
    assert_eq!(app.call("PUT", &like_uri, Some(json!({ "kind": "favourite" }))).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(app.call("PUT", &like_uri, Some(json!({ "hidden": true }))).await.0, StatusCode::BAD_REQUEST, "only patterns hide");
    assert_eq!(app.call("DELETE", &like_uri, None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(app.call("DELETE", &like_uri, None).await.0, StatusCode::NOT_FOUND);
    let fav_uri = format!("/api/taste/statements/{}", fav["id"]);
    let (status, row) = app.call("PUT", &fav_uri, Some(json!({ "why": ["tag:Puzzle"], "note": "co-op with my brother" }))).await;
    assert_eq!((status, row["why"].clone(), row["note"].clone()), (StatusCode::OK, json!(["tag:Puzzle"]), json!("co-op with my brother")));

    // The job: no patterns from one favourite.
    let (status, job) = app.call("POST", "/api/jobs", Some(json!({ "task": "taste" }))).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    let ended = app.job_ended().await;
    assert_eq!((ended["status"].clone(), ended["task"].clone(), ended["kind"].clone()), (json!("done"), json!("taste"), json!("taste")));
    assert_eq!(ended["summary"], "No patterns yet, from 1 game you stuck with and 0 you left", "{ended}");
    let scheduled = app.get("/api/jobs/scheduled").await;
    assert!(
        scheduled["tasks"].as_array().unwrap().iter().any(|t| t["task"] == "taste" && t["label"] == "Update your profile"),
        "{scheduled}"
    );

    // What it knows, and deleting it.
    let s = app.get("/api/taste/sources").await;
    assert_eq!(
        (s["answers"]["favourites"].clone(), s["answers"]["hard_nos"].clone(), s["diary"]["on"].clone()),
        (json!(1), json!(1), json!(false))
    );
    assert_eq!(s["library"]["stores"], json!(["steam", "epic"]));
    assert_eq!(app.get("/api/taste/sources/reactions").await, json!([]));
    assert_eq!(app.call("GET", "/api/taste/sources/library", None).await.0, StatusCode::NOT_FOUND);
    let (status, v) = app.call("DELETE", "/api/taste/sources/library", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    let (status, headers, bytes) = app.raw("/api/taste/export").await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers[header::CONTENT_DISPOSITION].to_str().unwrap().starts_with("attachment; filename=\"upnext-profile-"));
    let export: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(export["statements"].as_array().unwrap().iter().any(|r| r["kind"] == "favourite" && r["game"] == "Portal 2"), "{export}");
    let (status, v) = app.call("DELETE", "/api/taste/sources/answers", None).await;
    assert_eq!((status, v["deleted"].clone()), (StatusCode::OK, json!(3)));
    assert_eq!(app.get("/api/taste").await["favourites"], json!([]));

    // The two settings.
    let (status, _) = app.call("PUT", "/api/settings", Some(json!({ "pattern_min_games": 1 }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    app.call("PUT", "/api/settings", Some(json!({ "pattern_half_life_days": 180 }))).await;
    assert_eq!(app.get("/api/taste").await["rules"]["half_life_days"], 180);
}

/// SIGTERM or Ctrl-C: the server answers until then, then stops taking
/// jobs, stops listening and returns.
#[tokio::test]
async fn the_server_stops_when_asked() {
    let app = test_app().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/api/stats", listener.local_addr().unwrap());
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_until(listener, app.state.clone(), async {
        let _ = stopped.await;
    }));
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    assert_eq!(http.get(&url).send().await.unwrap().status(), StatusCode::OK);
    stop.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), server).await.expect("it stopped").unwrap().unwrap();
    assert!(app.state.jobs.closing());
    assert!(http.get(&url).send().await.is_err(), "no longer listening");
    let cfg = app.state.cfg();
    let refused =
        app.state.jobs.start(JobKind::Taste, None, JobSource::Ui, app.db.clone(), cfg, app.state.clients.clone(), app.state.library_llm());
    assert_eq!(refused.unwrap_err().to_string(), "UpNext is shutting down");
}

/// Steam's recently played games, with Portal 2 at `minutes` in all.
async fn recently_played(server: &MockServer, minutes: i64) {
    server.reset().await;
    Mock::given(path("/IPlayerService/GetRecentlyPlayedGames/v1/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "response": { "total_count": 1, "games": [
            { "appid": 620, "name": "Portal 2", "playtime_2weeks": 60, "playtime_forever": minutes }
        ] } })))
        .mount(server)
        .await;
}

/// The play diary: off by default and then it writes nothing; on, the
/// first reading only starts the count, the next an hour later gives one
/// row, the Play button adds a launch, and deleting it empties it.
#[tokio::test]
async fn the_play_diary_records_only_when_on() {
    use crate::taste::diary;
    let services = MockServer::start().await;
    recently_played(&services, 1260).await;
    let app = reconfigured(test_app_with(None, Some(&services.uri())).await, |c| {
        c.steam_api_key = Some("key".into());
        c.steam_id = Some("76561198000000000".into());
    });
    let portal = app.id_of("Portal 2").await;

    let q = app.get("/api/taste/questions").await;
    assert_eq!((q["diary"]["on"].clone(), q["diary"]["steam"].clone()), (json!(false), json!(true)));
    assert_eq!(q["most_played"][0]["game"]["name"], "Portal 2");
    assert_eq!(q["few_played"], json!(true), "one game played");
    assert!(q["hard_nos"].as_array().unwrap().iter().any(|s| s["about"] == "tag:Horror"));
    let (status, v) = app.call("POST", "/api/taste/diary/launch", Some(json!({ "title_id": portal }))).await;
    assert_eq!((status, v["recorded"].clone()), (StatusCode::OK, json!(false)), "off writes nothing");
    let cfg = app.state.cfg();
    assert!(diary::poll(&app.db, &cfg, &app.state.clients, crate::db::now()).await.is_err(), "nor reads Steam");
    assert_eq!(services.received_requests().await.unwrap().len(), 0);
    assert_eq!(app.get("/api/taste/sources").await["diary"]["on"], json!(false));

    // On: the first reading at once.
    let (status, v) = app.call("PUT", "/api/taste/diary", Some(json!({ "on": true }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!((v["diary"]["on"].clone(), v["diary"]["rows"].clone(), v["diary"]["error"].clone()), (json!(true), json!(0), Value::Null));
    let first = v["diary"]["last_reading"].as_i64().expect("a first reading");
    let cfg = app.state.cfg();
    assert!(cfg.play_diary);
    assert!(!diary::due(&app.db, first + 600).unwrap(), "not again for an hour");
    assert!(diary::due(&app.db, first + 3600).unwrap());

    // An hour on, 45 minutes more.
    recently_played(&services, 1305).await;
    let polled = diary::poll(&app.db, &cfg, &app.state.clients, first + 3600).await.unwrap();
    assert_eq!((polled.games, polled.rows, polled.first), (1, 1, false));
    let (status, v) = app.call("POST", "/api/taste/diary/launch", Some(json!({ "title_id": portal }))).await;
    assert_eq!((status, v["recorded"].clone()), (StatusCode::OK, json!(true)));
    let rows = app.get("/api/taste/sources/diary").await;
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 2, "{rows:?}");
    let steam = rows.iter().find(|r| r["source"] == "steam").unwrap();
    assert_eq!((steam["title_id"].as_i64(), steam["minutes"].clone(), steam["name"].clone()), (Some(portal), json!(45), json!("Portal 2")));
    assert_eq!(steam["hour"].as_i64().unwrap() % 3600, 0);
    let s = app.get("/api/taste/sources").await;
    assert_eq!((s["diary"]["rows"].clone(), s["diary"]["launches"].clone()), (json!(1), json!(1)));
    let (_, _, bytes) = app.raw("/api/taste/export").await;
    let export: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(export["play_diary"].as_array().unwrap().len(), 2);

    // A failed reading says why and writes nothing.
    services.reset().await;
    Mock::given(path("/IPlayerService/GetRecentlyPlayedGames/v1/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "response": {} })))
        .mount(&services)
        .await;
    assert!(diary::poll(&app.db, &cfg, &app.state.clients, first + 7200).await.is_err());
    let s = app.get("/api/taste/sources").await;
    assert!(s["diary"]["error"].as_str().unwrap().contains("Game details"), "{s}");

    // Deleting it empties it; off stops it.
    let (status, v) = app.call("DELETE", "/api/taste/sources/diary", None).await;
    assert_eq!((status, v["deleted"].clone()), (StatusCode::OK, json!(2)));
    assert_eq!(app.get("/api/taste/sources/diary").await, json!([]));
    let (_, v) = app.call("PUT", "/api/taste/diary", Some(json!({ "on": false }))).await;
    assert_eq!(
        (v["diary"]["on"].clone(), v["diary"]["last_reading"].clone(), v["diary"]["error"].clone()),
        (json!(false), Value::Null, Value::Null)
    );
}

#[tokio::test]
async fn the_contract_is_served() {
    let app = test_app().await;
    let want = serde_json::to_value(openapi::doc()).unwrap();
    for uri in ["/api/v1/openapi.json", "/api/openapi.json"] {
        let doc = app.get(uri).await;
        assert_eq!(doc, want, "{uri}");
    }
    assert_eq!(want["openapi"], "3.1.0");
    assert_eq!(want["servers"][0]["url"], "/api/v1");
}

/// Summaries on a backend of their own: Settings shows the choice and
/// the models it offers, Test tries either one, the pages that need the
/// summaries' AI ask that one, and the estimates say where each kind goes.
#[tokio::test]
async fn summaries_can_use_their_own_backend() {
    let app = test_app().await;
    let s = app.get("/api/settings").await;
    assert_eq!((&s["digest_backend"], &s["digest_backends"][0]), (&json!("same"), &json!("same")));

    // Picks stay on the OpenAI-compatible server; summaries go to an Anthropic key that isn't there.
    let (status, s) = app.call("PUT", "/api/settings", Some(json!({ "digest_backend": "api-key" }))).await;
    assert_eq!(status, StatusCode::OK, "{s}");
    assert_eq!((&s["llm_backend"], &s["digest_backend"], &s["digest_model"]), (&json!("openai"), &json!("api-key"), &json!("default")));
    assert!(s["digest_models"].as_array().unwrap().contains(&json!("haiku")), "Haiku with the key: {s}");
    assert!(!s["models"].as_array().unwrap().contains(&json!("haiku")), "not for picks: {s}");

    let (_, picks) = app.call("POST", "/api/llm/test", None).await;
    assert_eq!((&picks["ok"], &picks["backend"]), (&json!(true), &json!("openai")), "{picks}");
    let (_, summaries) = app.call("POST", "/api/llm/test?purpose=summaries", None).await;
    assert_eq!((&summaries["ok"], &summaries["backend"]), (&json!(false), &json!("api-key")), "{summaries}");
    assert_eq!(app.get("/api/config").await["claude_available"], true, "picks can still run");
    assert_eq!(app.get("/api/gems").await["ai"]["available"], false);
    assert_eq!(app.get("/api/second-chances").await["ai"]["available"], false);

    let estimates = app.get("/api/ai/estimates").await;
    let backend_of = |kind: &str| estimates["features"].as_array().unwrap().iter().find(|f| f["kind"] == kind).unwrap()["backend"].clone();
    assert_eq!((backend_of("pick"), backend_of("series_guides")), (json!("openai"), json!("openai")));
    assert_eq!((backend_of("review_summaries"), backend_of("hidden_gems")), (json!("api-key"), json!("api-key")));
    assert_eq!(estimates["priced"], true, "the key is charged per call");

    let (status, _) = app.call("PUT", "/api/settings", Some(json!({ "digest_backend": "gpt" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (_, s) = app.call("PUT", "/api/settings", Some(json!({ "digest_backend": "same" }))).await;
    assert_eq!(s["digest_backend"], "same");
    let (_, summaries) = app.call("POST", "/api/llm/test?purpose=summaries", None).await;
    assert_eq!((&summaries["ok"], &summaries["backend"]), (&json!(true), &json!("openai")), "{summaries}");
}

/// `GET /jobs?kind=` takes every kind `POST /jobs` starts, the taste
/// profile's and the backlog verdicts' included, and still refuses one
/// that isn't (B-CORE-5).
#[tokio::test]
async fn jobs_list_by_every_kind() {
    let app = test_app().await;
    let id = app.db.job_begin("taste", Some("taste"), JobSource::Ui, &json!({})).unwrap().unwrap();
    let end = crate::db::JobEnd { status: JobStatus::Done, summary: Some("ok"), error: None };
    app.db.job_end(id, &Default::default(), &[], &[], &end).unwrap();
    for kind in JobKind::NAMES {
        let (status, v) = app.call("GET", &format!("/api/jobs?kind={kind}"), None).await;
        assert_eq!(status, StatusCode::OK, "kind={kind}: {v}");
        let ids: Vec<i64> = v["recent"].as_array().unwrap().iter().map(|j| j["id"].as_i64().unwrap()).collect();
        assert_eq!(ids, if kind == "taste" { vec![id] } else { vec![] }, "kind={kind}");
    }
    let (status, v) = app.call("GET", "/api/jobs?kind=tidy", None).await;
    assert_eq!((status, v["error"].as_str().unwrap()), (StatusCode::BAD_REQUEST, "unknown job kind \"tidy\""));
}
