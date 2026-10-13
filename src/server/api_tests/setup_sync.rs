//! The setup wizard's sync, shared data and Fetch details, run in the
//! background while the person carries on.

use super::*;
use wiremock::matchers::any;

const STEAM_ID: &str = "76561197960287930";

/// An empty install whose services, shared data included, are `services`.
async fn wizard_app(services: &MockServer) -> TestApp {
    let dir = tempfile::tempdir().unwrap();
    let cfg = test_config(dir.path(), &fake_llm().await);
    let db = Arc::new(Db::open(&cfg.db_path, cfg.sampled_minutes).unwrap());
    let clients = Arc::new(Clients::with_base_url(&cfg, Some(&services.uri())).unwrap());
    let state = AppState {
        db: db.clone(),
        base_cfg: cfg,
        clients,
        jobs: Arc::new(JobRunner::new()),
        remote: Default::default(),
        claude_install: Default::default(),
        setup_sync: Default::default(),
    };
    state.setup_sync.use_seed_url(format!("{}/latest/upnext-seed.json", services.uri()));
    TestApp { _dir: dir, router: router(state.clone()), db, state }
}

/// Steam's Web API with Hades (which the shared data has) and Portal 2, the
/// shared data, and every other address answering after `slow`, so a Fetch
/// details is still running when the test looks.
async fn services(slow: std::time::Duration) -> MockServer {
    let s = MockServer::start().await;
    let owned = json!({ "response": { "game_count": 2, "games": [
        { "appid": 1145360, "name": "Hades", "playtime_forever": 600 },
        { "appid": 620, "name": "Portal 2", "playtime_forever": 1260 },
    ] } });
    Mock::given(path("/IPlayerService/GetOwnedGames/v1/")).respond_with(ResponseTemplate::new(200).set_body_json(owned)).mount(&s).await;
    let summary = crate::testutil::fixture_json("steam/player_summary.json");
    Mock::given(path("/ISteamUser/GetPlayerSummaries/v2/")).respond_with(ResponseTemplate::new(200).set_body_json(summary)).mount(&s).await;
    Mock::given(path("/latest/upnext-seed.json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(crate::testutil::fixture("seed/upnext-seed.json")))
        .mount(&s)
        .await;
    let data = std::fs::read(crate::testutil::fixture_path("seed/upnext-seed-2026-10-01.jsonl.gz")).unwrap();
    Mock::given(path("/latest/upnext-seed-2026-10-01.jsonl.gz"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(data))
        .mount(&s)
        .await;
    Mock::given(any()).respond_with(ResponseTemplate::new(404).set_delay(slow)).with_priority(u8::MAX).mount(&s).await;
    s
}

impl TestApp {
    async fn background(&self, ask: Value) -> Value {
        let (status, v) = self.call("POST", "/api/v1/setup/background", Some(ask)).await;
        assert_eq!(status, StatusCode::OK, "{v}");
        v
    }

    /// `GET /setup` once `done` says so.
    async fn setup_when(&self, what: &str, done: impl Fn(&Value) -> bool) -> Value {
        for _ in 0..1000 {
            let s = self.get("/api/v1/setup").await;
            if done(&s) {
                return s;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("{what} didn't happen: {}", self.get("/api/v1/setup").await);
    }

    /// The jobs, oldest first, as (task, source, status).
    async fn job_list(&self) -> Vec<(String, String, String)> {
        let v = self.get("/api/v1/jobs?limit=50").await;
        let mut jobs: Vec<_> = v["recent"]
            .as_array()
            .unwrap()
            .iter()
            .map(|j| {
                let s = |k: &str| j[k].as_str().unwrap_or_default().to_string();
                (s("task"), s("source"), s("status"))
            })
            .collect();
        jobs.reverse();
        jobs
    }
}

fn job(task: &str, status: &str) -> (String, String, String) {
    (task.into(), "ui".into(), status.into())
}

/// Leaving the stores step starts the first sync; the shared data follows it
/// with no page open; Fetch details waits for the keys step, holding the
/// schedules meanwhile; an account changed later stops the wizard's own
/// Fetch details for a new sync, then fetches the rest.
#[tokio::test]
async fn the_first_sync_runs_in_the_background_then_shared_data_then_details() {
    let s = services(std::time::Duration::from_secs(2)).await;
    let app = wizard_app(&s).await;
    let key = "0123456789ABCDEF0123456789ABCDEF";
    let settings = json!({ "steam_id": STEAM_ID, "steam_api_key": key, "auto_sync": true });
    let (status, v) = app.call("PUT", "/api/v1/settings", Some(settings)).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let before = app.get("/api/v1/setup").await["background"].clone();
    assert_eq!(
        before,
        json!({ "sync_waiting": false, "details_waiting": false,
        "seed": { "wanted": true, "state": "idle", "written": null, "error": null } })
    );

    // Next on the stores step.
    let v = app.background(json!({ "sync": "if_changed", "details": "wait" })).await;
    assert_eq!(v["seed"]["state"], "waiting", "a first sync: {v}");
    let done = app.setup_when("the shared data applied", |s| s["background"]["seed"]["state"] == "applied").await;
    assert_eq!((done["library"]["titles"].as_u64(), done["background"]["seed"]["written"].as_u64()), (Some(2), Some(1)), "{done}");
    assert_eq!(app.get("/api/v1/seed").await["kept"], "upnext-seed-2026-10-01.jsonl.gz", "kept for games added later");
    assert_eq!(app.job_list().await, [job("sync", "done")]);

    // The keys are still to come: the schedules don't start Fetch details after the sync.
    let now = crate::db::now();
    assert!(app.state.setup_sync.holds(now));
    assert!(crate::server::scheduler::schedule_tick(&app.state, now + 60).unwrap().is_none());
    assert!(!app.state.setup_sync.holds(now + 3600), "for an hour at most");

    // Back to the stores and Next again, with nothing changed: no second sync.
    let v = app.background(json!({ "sync": "if_changed", "details": "wait" })).await;
    assert_eq!(v["sync_waiting"], false, "{v}");

    // Past the keys: Fetch details starts.
    app.background(json!({ "details": "after_sync" })).await;
    app.setup_when("Fetch details started", |s| s["library"]["job"]["task"] == "details").await;
    assert!(!app.state.setup_sync.holds(now));

    // An account changes while it runs: it stops, a sync reads the new
    // setting, and Fetch details starts again after it.
    let (status, v) = app.call("PUT", "/api/v1/settings", Some(json!({ "steam_include_hidden": true }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    app.background(json!({ "sync": "if_changed" })).await;
    for _ in 0..1000 {
        if app.job_list().await.len() == 4 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(app.job_list().await.len(), 4, "{:?}", app.job_list().await);
    app.job_ended().await;
    let jobs = app.job_list().await;
    assert_eq!(jobs[..3], [job("sync", "done"), job("details", "cancelled"), job("sync", "done")], "{jobs:?}");
    assert_eq!((jobs[3].0.as_str(), jobs[3].1.as_str()), ("details", "ui"), "{jobs:?}");
    let after = app.get("/api/v1/setup").await;
    assert_eq!((&after["background"]["seed"]["state"], &after["background"]["details_waiting"]), (&json!("applied"), &json!(false)));
}

/// Shared data turned off isn't applied, and a library that has games isn't
/// a first sync, so it isn't applied unasked either; "Sync again" syncs
/// whatever the accounts.
#[tokio::test]
async fn a_library_with_games_syncs_again_without_shared_data() {
    let s = services(std::time::Duration::ZERO).await;
    let app = wizard_app(&s).await;
    let key = "0123456789ABCDEF0123456789ABCDEF";
    let (status, v) = app.call("PUT", "/api/v1/settings", Some(json!({ "steam_id": STEAM_ID, "steam_api_key": key }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    app.background(json!({ "sync": "if_changed", "seed": false })).await;
    // An empty library, but the shared data was turned off.
    let synced = app.setup_when("the shared data skipped", |s| s["background"]["seed"]["state"] == "skipped").await;
    assert_eq!((synced["library"]["titles"].as_u64(), &synced["background"]["seed"]["wanted"]), (Some(2), &json!(false)));

    app.background(json!({ "sync": "again", "seed": true })).await;
    app.setup_when("the second sync", |s| s["library"]["job"].is_null() && !s["background"]["sync_waiting"].as_bool().unwrap()).await;
    app.job_ended().await;
    assert_eq!(app.job_list().await, [job("sync", "done"), job("sync", "done")]);
    let v = app.get("/api/v1/setup").await;
    assert_eq!(v["background"]["seed"]["state"], "skipped", "not a first sync: {v}");
    let fetched = s.received_requests().await.unwrap().iter().filter(|r| r.url.path().contains("upnext-seed")).count();
    assert_eq!(fetched, 0, "the shared data was never fetched");
}

/// The hold ends when setup does, so the schedules decide again.
#[tokio::test]
async fn leaving_setup_ends_the_hold() {
    let s = services(std::time::Duration::ZERO).await;
    let app = wizard_app(&s).await;
    app.background(json!({ "details": "wait" })).await;
    assert!(app.state.setup_sync.holds(crate::db::now()));
    let (status, v) = app.call("PUT", "/api/v1/setup", Some(json!({ "setup": "skipped" }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert!(!app.state.setup_sync.holds(crate::db::now()));
}

/// A look that keeps failing (the database can't be read, say) stops the
/// follower after a few tries instead of spinning, and the next ask can
/// start it again.
#[tokio::test]
async fn a_follower_that_keeps_failing_stops() {
    use crate::server::setup_sync::{Step, follow_with};
    static LOOKS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    fn broken(_: &AppState) -> anyhow::Result<Step> {
        LOOKS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        anyhow::bail!("the database is locked")
    }
    let s = services(std::time::Duration::ZERO).await;
    let app = wizard_app(&s).await;
    tokio::time::timeout(std::time::Duration::from_secs(5), follow_with(app.state.clone(), broken)).await.expect("it stops");
    assert_eq!(LOOKS.load(std::sync::atomic::Ordering::SeqCst), 20);
    assert!(!app.state.setup_sync.following(), "a new ask starts it again");
}

/// Only a first run holds the schedules: once setup is done (or the wizard
/// is restarted from Settings, which leaves it done), "wait" is ignored.
#[tokio::test]
async fn only_a_first_run_holds_the_schedules() {
    let s = services(std::time::Duration::ZERO).await;
    let app = wizard_app(&s).await;
    let (status, v) = app.call("PUT", "/api/v1/setup", Some(json!({ "setup": "done" }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    app.background(json!({ "details": "wait" })).await;
    assert!(!app.state.setup_sync.holds(crate::db::now()));
}

/// Starting jobs takes a full token, as `POST /jobs` does: a read token is
/// refused, and nothing is held or started.
#[tokio::test]
async fn a_read_token_cant_start_the_background_work() {
    let s = services(std::time::Duration::ZERO).await;
    let app = reconfigured(wizard_app(&s).await, |c| c.host = "0.0.0.0".into());
    let read = app.db.token_create("dashboard", crate::db::Scope::Read, "token").unwrap().token;
    let full = app.db.token_create("script", crate::db::Scope::Full, "token").unwrap().token;
    let ask = |token: String| {
        let req = Request::post("/api/v1/setup/background")
            .header(header::HOST, "den.lan:8787")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({ "details": "wait" }).to_string()))
            .unwrap();
        let peer = crate::server::listen::Peer { addr: std::net::SocketAddr::from(([192, 168, 0, 20], 50000)), tls: false };
        app.router.clone().layer(axum::extract::connect_info::MockConnectInfo(peer)).oneshot(req)
    };
    assert_eq!(ask(read).await.unwrap().status(), StatusCode::FORBIDDEN);
    assert!(!app.state.setup_sync.holds(crate::db::now()), "nothing held");
    assert_eq!(ask(full).await.unwrap().status(), StatusCode::OK);
    assert!(app.state.setup_sync.holds(crate::db::now()));
}
