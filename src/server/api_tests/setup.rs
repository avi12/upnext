//! The setup wizard's API and the Galaxy upload.

use super::*;
use crate::server::listen::Peer;

/// An install with nothing in it yet.
async fn empty_app() -> TestApp {
    let dir = tempfile::tempdir().unwrap();
    let cfg = test_config(dir.path(), &fake_llm().await);
    let db = Arc::new(Db::open(&cfg.db_path, cfg.sampled_minutes).unwrap());
    let clients = Arc::new(Clients::with_base_url(&cfg, None).unwrap());
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

impl TestApp {
    async fn upload_galaxy(&self, body: Vec<u8>, length: Option<&str>) -> (StatusCode, Value) {
        let mut req = Request::post("/api/v1/galaxy/upload").header(header::HOST, "127.0.0.1:8787");
        req = req.header(header::CONTENT_TYPE, "application/octet-stream");
        if let Some(n) = length {
            req = req.header(header::CONTENT_LENGTH, n);
        }
        self.send(req.body(Body::from(body)).unwrap()).await
    }
}

#[tokio::test]
async fn a_fresh_install_goes_to_the_wizard_and_sees_what_is_missing() {
    let app = empty_app().await;
    let state = app.get("/api/v1/setup/state").await;
    assert_eq!(
        state,
        json!({ "needed": true, "step": null, "setup": "open", "tour": "open", "tour_offer": false,
            "player_needed": false, "player_step": null, "player_setup": "done" })
    );

    let s = app.get("/api/v1/setup").await;
    assert_eq!(s["keys"], json!({ "steam_api_key": false, "steam_id": false, "rawg": false, "igdb": false }));
    let env = &s["environment"];
    assert_eq!((env["os"].as_str(), env["mode"].as_str()), (Some(std::env::consts::OS), Some("serve")));
    assert_eq!((env["container"].as_bool(), env["client_local"].as_bool()), (Some(false), Some(true)));
    let stores = &s["stores"];
    assert_eq!((stores["steam"]["path"].clone(), stores["steam"]["missing"].as_str()), (Value::Null, Some("not found")));
    assert_eq!((stores["steam"]["web_api"].as_bool(), stores["steam"]["account"].clone()), (Some(false), Value::Null));
    assert_eq!((stores["heroic"]["missing"].as_str(), stores["heroic"]["games"].clone()), (Some("not found"), json!({})));
    assert_eq!(stores["gog"]["connected"], false);
    assert_eq!(stores["galaxy"], json!({ "path": null, "exists": false }));
    assert_eq!((stores["ea"].clone(), stores["ubisoft"].clone()), (json!({ "installed": 0 }), json!({ "installed": 0 })));
    assert_eq!(stores["epic"], json!({ "connected": false, "display_name": null, "launcher_dir": null, "installed": null }));
    let blocked: Vec<(&str, bool)> =
        stores["libraries"].as_array().unwrap().iter().map(|l| (l["id"].as_str().unwrap(), l["blocked"].is_string())).collect();
    assert_eq!(
        blocked,
        [("steam", false), ("heroic", false), ("gog", true), ("epic", true), ("ea", true), ("ubisoft", true), ("galaxy", true)],
        "{}",
        stores["libraries"]
    );
    assert_eq!((s["library"]["titles"].as_u64(), s["library"]["job"].clone()), (Some(0), Value::Null));
    assert_eq!(s["ai"]["backend"], "openai");
    assert_eq!(s["ai"]["claude_install"], Value::Null, "nothing installs Claude Code unless asked");
    assert_eq!(s["setup"]["needed"], true);
    // A container installing it (UPNEXT_INSTALL_CLAUDE=1), then failing.
    let installing = crate::llm::claude_install::InstallState { installing: true, error: None };
    app.state.claude_install.set(Some(installing));
    assert_eq!(app.get("/api/v1/setup").await["ai"]["claude_install"], json!({ "installing": true, "error": null }));
    let failed = crate::llm::claude_install::InstallState { installing: false, error: Some("reaching it".into()) };
    app.state.claude_install.set(Some(failed));
    assert_eq!(app.get("/api/v1/setup").await["ai"]["claude_install"], json!({ "installing": false, "error": "reaching it" }));
    app.state.claude_install.set(None);

    // Where it got to, then skipped: the pages are the app's again.
    let (status, v) = app.call("PUT", "/api/v1/setup", Some(json!({ "step": "steam" }))).await;
    assert_eq!((status, v["step"].as_str(), v["needed"].as_bool()), (StatusCode::OK, Some("steam"), Some(true)));
    let (status, v) = app.call("PUT", "/api/v1/setup", Some(json!({ "step": "nowhere" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    let (_, v) = app.call("PUT", "/api/v1/setup", Some(json!({ "setup": "skipped" }))).await;
    assert_eq!((v["needed"].as_bool(), v["setup"].as_str(), v["step"].as_str()), (Some(false), Some("skipped"), Some("steam")));
    assert_eq!(v["tour_offer"], false, "no tour of an empty library; it's offered once games arrive");
    let (_, v) = app.call("PUT", "/api/v1/setup", Some(json!({ "setup": "open" }))).await;
    assert_eq!(v["needed"], true, "run again");
}

#[tokio::test]
async fn an_install_with_games_is_set_up_and_offered_the_tour_once() {
    let app = test_app().await;
    let state = app.get("/api/v1/setup/state").await;
    assert_eq!((state["needed"].as_bool(), state["tour_offer"].as_bool()), (Some(false), Some(true)));
    let s = app.get("/api/v1/setup").await;
    assert_eq!((s["library"]["titles"].as_u64(), s["library"]["multi_store"].as_u64()), (Some(5), Some(1)));
    assert_eq!(s["library"]["by_source"]["steam"], 4);
    assert!(s["library"]["details_minutes"].as_f64().unwrap() > 0.0, "{}", s["library"]);
    let (_, v) = app.call("PUT", "/api/v1/setup", Some(json!({ "tour": "skipped" }))).await;
    assert_eq!((v["tour"].as_str(), v["tour_offer"].as_bool()), (Some("skipped"), Some(false)));
    assert_eq!(app.get("/api/v1/setup/state").await["tour_offer"], false, "kept");
}

#[tokio::test]
async fn keys_are_reported_as_set_never_their_values() {
    let app = empty_app().await;
    let keys = json!({ "steam_api_key": "ABCDEF0123456789ABCDEF0123456789", "steam_id": "76561190000000001",
        "rawg_api_key": "feedfacefeedfacefeedfacefeedface", "igdb_client_id": "abcdefghij0123456789klmnopqrst", "igdb_client_secret": "igdb-secret-5678" });
    let (status, v) = app.call("PUT", "/api/v1/settings", Some(keys)).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let s = app.get("/api/v1/setup").await;
    assert_eq!(s["keys"], json!({ "steam_api_key": true, "steam_id": true, "rawg": true, "igdb": true }));
    assert_eq!((s["stores"]["steam"]["web_api"].as_bool(), s["stores"]["steam"]["account_source"].as_str()), (Some(true), Some("app")));
    let text = s.to_string();
    for secret in ["ABCDEF0123456789", "feedface", "igdb-secret"] {
        assert!(!text.contains(secret), "{secret} in {text}");
    }
}

/// Opening the setup page with a damaged Steam
/// cache reports it and leaves the server running.
#[tokio::test]
async fn a_damaged_steam_cache_is_reported() {
    let app = empty_app().await;
    let steam = crate::testutil::damaged_steam_folder(app._dir.path());
    let app = reconfigured(app, |c| c.steam_dir = Some(steam.clone()));
    let s = app.get("/api/v1/setup").await;
    let found = &s["stores"]["steam"];
    assert_eq!((found["path"].as_str(), found["local_games"].clone()), (steam.to_str(), Value::Null), "{found}");
    let error = found["error"].as_str().unwrap_or_default();
    assert!(error.starts_with("couldn't read the Steam client's cache"), "{error}");
    assert_eq!(app.call("GET", "/api/v1/health", None).await.0, StatusCode::OK, "still serving");
}

#[tokio::test]
async fn a_container_says_what_isnt_mounted() {
    let app = reconfigured(empty_app().await, |c| c.container = true);
    let s = app.get("/api/v1/setup").await;
    let env = &s["environment"];
    assert_eq!((env["container"].as_bool(), env["display"].as_bool(), env["gog_window"].as_bool()), (Some(true), Some(false), Some(false)));
    assert_eq!(s["stores"]["steam"]["missing"], "not mounted");
    assert_eq!(s["stores"]["heroic"]["missing"], "not mounted");
    // No window to open: refused up front, with the way round.
    let (status, v) = app.call("POST", "/api/v1/gog/window", None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    assert!(v.to_string().contains("paste where it lands"), "{v}");
    assert_eq!(app.get("/api/v1/gog").await["login_pending"], false, "nothing started");
}

#[tokio::test]
async fn another_device_is_not_this_computer() {
    let app = reconfigured(empty_app().await, |c| c.host = "0.0.0.0".into());
    let made = app.db.token_create("phone", crate::db::Scope::Full, "token").unwrap();
    let ask = |method: &str, uri: &str| {
        let peer = Peer { addr: std::net::SocketAddr::from(([192, 168, 0, 20], 50000)), tls: false };
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, "den.lan:8787")
            .header(header::AUTHORIZATION, format!("Bearer {}", made.token))
            .body(Body::empty())
            .unwrap();
        app.router.clone().layer(axum::extract::connect_info::MockConnectInfo(peer)).oneshot(req)
    };
    let resp = ask("GET", "/api/v1/setup").await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v: Value = serde_json::from_slice(&to_bytes(resp.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(v["environment"]["client_local"], false);
}

/// The first player of a new install is "Player 1" until somebody names
/// them: setup offers their Steam name, then this computer's user name to
/// someone at this computer only, and nothing once they have a name.
#[tokio::test]
async fn setup_offers_a_name_for_the_first_player_while_theirs_is_made_up() {
    let app = empty_app().await;
    assert_eq!(app.get("/api/v1/setup").await["you"], json!({ "id": 1, "name": "Player 1", "placeholder": true, "suggested": null }));

    // The user name UpNext runs under, to this computer only.
    let app = reconfigured(app, |c| c.login_name = Some("Kit".into()));
    assert_eq!(app.get("/api/v1/setup").await["you"]["suggested"], "Kit");

    // A Steam name from a sync comes first, unless another player has it.
    app.db.meta_set("profile", r#"{"steam_id":"76561197960287930","persona_name":"Alex","game_count":0}"#).unwrap();
    assert_eq!(app.get("/api/v1/setup").await["you"]["suggested"], "Alex");
    app.db.add_player("alex").unwrap();
    assert_eq!(app.get("/api/v1/setup").await["you"]["suggested"], "Kit");

    // Named, nothing is offered, and the name is theirs to keep.
    let (status, v) = app.call("PUT", "/api/v1/players/1", Some(json!({ "name": "Sam" }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(app.get("/api/v1/setup").await["you"], json!({ "id": 1, "name": "Sam", "placeholder": false, "suggested": null }));

    // Another device isn't this computer's user, so it isn't offered there
    // (and the Steam name is still taken).
    let app = reconfigured(app, |c| c.host = "0.0.0.0".into());
    app.db.rename_player(PlayerId::FIRST, "Player 1").unwrap();
    let made = app.db.token_create("phone", crate::db::Scope::Full, "token").unwrap();
    let peer = Peer { addr: std::net::SocketAddr::from(([192, 168, 0, 20], 50000)), tls: false };
    let req = Request::get("/api/v1/setup")
        .header(header::HOST, "den.lan:8787")
        .header(header::AUTHORIZATION, format!("Bearer {}", made.token))
        .body(Body::empty())
        .unwrap();
    let resp = app.router.clone().layer(axum::extract::connect_info::MockConnectInfo(peer)).oneshot(req).await.unwrap();
    let v: Value = serde_json::from_slice(&to_bytes(resp.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!((v["you"]["placeholder"].as_bool(), v["you"]["suggested"].clone()), (Some(true), Value::Null), "{v}");
}

/// Other users' Steam and Heroic folders are named only to someone at
/// this computer: not to a token, even from here, nor to a paired device,
/// nor in a container.
#[tokio::test]
async fn other_users_folders_are_named_only_to_this_computer() {
    let users = tempfile::tempdir().unwrap();
    // Joined as the server joins them (each home, then the folder below it),
    // so the names match on Windows too, where the separators differ.
    let heroic = users.path().join("sam").join(".config/heroic");
    std::fs::create_dir_all(heroic.join("store_cache")).unwrap();
    let steam = users.path().join("sam").join(".local/share/Steam");
    std::fs::create_dir_all(steam.join("config")).unwrap();
    std::fs::create_dir_all(steam.join("appcache")).unwrap();
    std::fs::write(steam.join("config/loginusers.vdf"), crate::testutil::fixture("steam/loginusers.vdf")).unwrap();
    std::fs::write(steam.join("appcache/appinfo.vdf"), "").unwrap();
    let app = reconfigured(empty_app().await, |c| {
        c.host = "0.0.0.0".into();
        c.users_dirs = Some(vec![users.path().to_path_buf()]);
    });
    let token = app.db.token_create("script", crate::db::Scope::Full, "token").unwrap().token;
    let others = |v: &Value| (v["stores"]["heroic"]["other_users"].clone(), v["stores"]["steam"]["other_users"].clone());

    let here = Device { peer: [127, 0, 0, 1], host: "127.0.0.1:8787", cookie: None, bearer: None };
    let (status, _, v) = here.send(&app, "GET", "/api/v1/setup", None).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(others(&v), (json!([heroic.display().to_string()]), json!([steam.display().to_string()])));
    assert!(v["stores"]["heroic"]["games"].as_object().unwrap().is_empty(), "nothing in it is read: {v}");

    let none = (json!([]), json!([]));
    let script = Device { peer: [127, 0, 0, 1], host: "127.0.0.1:8787", cookie: None, bearer: Some(token.clone()) };
    let (status, _, v) = script.send(&app, "GET", "/api/v1/setup", None).await;
    assert_eq!((status, others(&v)), (StatusCode::OK, none.clone()), "a token, even from here: {v}");
    let phone = Device { peer: [192, 168, 0, 42], host: "den.lan:8787", cookie: None, bearer: Some(token) };
    let (status, _, v) = phone.send(&app, "GET", "/api/v1/setup", None).await;
    assert_eq!((status, others(&v)), (StatusCode::OK, none.clone()), "another device: {v}");
    // A paired device's token, bound to a player, from here or not.
    let device = app.db.token_create_for("phone", crate::db::Scope::Full, "device", Some(crate::db::PlayerId::FIRST)).unwrap().token;
    for (peer, host) in [([127, 0, 0, 1], "127.0.0.1:8787"), ([192, 168, 0, 42], "den.lan:8787")] {
        let paired = Device { peer, host, cookie: None, bearer: Some(device.clone()) };
        let (status, _, v) = paired.send(&app, "GET", "/api/v1/setup", None).await;
        assert_eq!((status, others(&v)), (StatusCode::OK, none.clone()), "a paired device from {host}: {v}");
    }

    let app = reconfigured(app, |c| c.container = true);
    let (status, _, v) = here.send(&app, "GET", "/api/v1/setup", None).await;
    assert_eq!((status, others(&v)), (StatusCode::OK, none), "in a container: {v}");
}

/// "That's mine" on another user's Heroic folder is taken through the real
/// connection routes, as are the other settings a connection holds: the
/// folders a connection alone holds (`heroic_dir`, `epic_manifests`) were
/// refused as unknown settings (found in a device test).
#[tokio::test]
async fn another_users_folder_is_taken_on_a_connection() {
    let users = tempfile::tempdir().unwrap();
    let heroic = users.path().join("sam").join(".config/heroic");
    std::fs::create_dir_all(heroic.join("store_cache")).unwrap();
    let app = reconfigured(empty_app().await, |c| {
        c.host = "0.0.0.0".into();
        c.users_dirs = Some(vec![users.path().to_path_buf()]);
    });
    let here = Device { peer: [127, 0, 0, 1], host: "127.0.0.1:8787", cookie: None, bearer: None };
    let (_, _, v) = here.send(&app, "GET", "/api/v1/setup", None).await;
    let folder = heroic.display().to_string();
    assert_eq!(v["stores"]["heroic"]["other_users"], json!([folder]), "{v}");

    // The UI's request when the player has no Heroic connection yet.
    let body = json!({ "kind": "heroic", "settings": { "heroic_dir": format!(" {folder} ") } });
    let (status, _, v) = here.send(&app, "POST", "/api/v1/connections", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    assert_eq!(v["settings"], json!({ "heroic_dir": folder }), "{v}");
    let id = v["id"].as_i64().unwrap();
    let (_, _, v) = here.send(&app, "GET", "/api/v1/setup", None).await;
    assert_eq!(v["stores"]["heroic"]["other_users"], json!([]), "a connection reads it now: {v}");

    // And when they have one: changed, then cleared.
    let other = users.path().join("kit").join(".config/heroic").display().to_string();
    let uri = format!("/api/v1/connections/{id}");
    let (status, _, v) = here.send(&app, "PUT", &uri, Some(json!({ "settings": { "heroic_dir": other } }))).await;
    assert_eq!((status, &v["settings"]), (StatusCode::OK, &json!({ "heroic_dir": other })), "{v}");
    let (status, _, v) = here.send(&app, "PUT", &uri, Some(json!({ "settings": { "heroic_dir": "" } }))).await;
    assert_eq!((status, &v["settings"]), (StatusCode::OK, &json!({})), "{v}");

    // Epic's manifests folder, the same way.
    let manifests = users.path().join("sam").join("epic-manifests").display().to_string();
    let body = json!({ "kind": "epic", "settings": { "epic_manifests": manifests } });
    let (status, _, v) = here.send(&app, "POST", "/api/v1/connections", Some(body)).await;
    assert_eq!((status, &v["settings"]), (StatusCode::CREATED, &json!({ "epic_manifests": manifests })), "{v}");
    let uri = format!("/api/v1/connections/{}", v["id"]);
    let (status, _, v) = here.send(&app, "PUT", &uri, Some(json!({ "settings": { "epic_manifests": other } }))).await;
    assert_eq!((status, &v["settings"]), (StatusCode::OK, &json!({ "epic_manifests": other })), "{v}");

    // Every other connection setting, checked as Settings checks it.
    let galaxy = json!({ "kind": "galaxy", "settings": { "galaxy_db": "/srv/galaxy-2.0.db", "galaxy_skip": "EA, xbox" } });
    let (status, _, v) = here.send(&app, "POST", "/api/v1/connections", Some(galaxy)).await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    assert_eq!(v["settings"], json!({ "galaxy_db": "/srv/galaxy-2.0.db", "galaxy_skip": "origin,xboxone" }), "{v}");
    let steam = json!({ "kind": "steam", "settings": { "steam_id": "76561197960287930", "steam_include_hidden": true } });
    let (status, _, v) = here.send(&app, "POST", "/api/v1/connections", Some(steam)).await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    assert_eq!(v["settings"], json!({ "steam_id": "76561197960287930", "steam_include_hidden": "true" }), "{v}");

    // Still refused: a bad value, another kind's setting, and a folder from another device.
    let bad = json!({ "kind": "galaxy", "settings": { "galaxy_skip": "gog" } });
    let (status, _, v) = here.send(&app, "POST", "/api/v1/connections", Some(bad)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    let (status, _, v) = here.send(&app, "PUT", &uri, Some(json!({ "settings": { "heroic_dir": folder } }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert!(v["error"].as_str().unwrap().contains("isn't a setting of"), "{v}");
    let token = app.db.token_create("phone", crate::db::Scope::Full, "token").unwrap().token;
    let phone = Device { peer: [192, 168, 0, 42], host: "den.lan:8787", cookie: None, bearer: Some(token) };
    let body = json!({ "kind": "heroic", "settings": { "heroic_dir": folder } });
    let (status, _, v) = phone.send(&app, "POST", "/api/v1/connections", Some(body)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
    assert!(v["error"].as_str().unwrap().contains("names a file"), "{v}");
}

#[tokio::test]
async fn a_galaxy_database_is_taken_in_and_anything_else_refused() {
    let app = empty_app().await;
    // A database in Galaxy's shape, as an upload from a Windows PC.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("galaxy-2.0.db");
    rusqlite::Connection::open(&file).unwrap().execute_batch(&crate::testutil::fixture("galaxy/galaxy-2.0.sql")).unwrap();
    let (status, v) = app.upload_galaxy(std::fs::read(&file).unwrap(), None).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["games"], 3);
    let kept = std::path::PathBuf::from(v["path"].as_str().unwrap());
    assert_eq!(kept, app.state.base_cfg.db_dir().join("galaxy-2.0.db"));
    assert!(kept.is_file());
    let settings = app.get("/api/v1/settings").await;
    assert_eq!((settings["galaxy_db"].as_str(), settings["galaxy_db_exists"].as_bool()), (v["path"].as_str(), Some(true)));
    assert_eq!(app.get("/api/v1/setup").await["stores"]["galaxy"]["exists"], true);

    // Not SQLite, SQLite but not Galaxy's, empty, and over the limit: refused, the upload kept.
    let (status, v) = app.upload_galaxy(b"just some text, not a database at all".repeat(200), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert!(v["error"].as_str().unwrap().starts_with("that isn't a GOG Galaxy database"), "{v}");
    let other = dir.path().join("other.db");
    rusqlite::Connection::open(&other).unwrap().execute_batch("CREATE TABLE t (x INTEGER); INSERT INTO t VALUES (1);").unwrap();
    let (status, v) = app.upload_galaxy(std::fs::read(&other).unwrap(), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(v["error"].as_str().unwrap().contains("missing table LibraryReleases"), "{v}");
    assert!(!v["error"].as_str().unwrap().contains(".galaxy-upload-"), "no temporary names: {v}");
    let (status, _) = app.upload_galaxy(vec![], None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, v) = app.upload_galaxy(b"x".to_vec(), Some("5000000000")).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{v}");
    assert!(kept.is_file(), "the earlier upload stays");
    let leftovers: Vec<_> = std::fs::read_dir(kept.parent().unwrap())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with(".galaxy-upload-"))
        .collect();
    assert!(leftovers.is_empty(), "work files are removed");
}

#[tokio::test]
async fn a_steam_sign_in_from_the_wizard_comes_back_to_it() {
    let app = empty_app().await;
    let to = app.redirect("/api/v1/steam/login?then=setup").await;
    let state = regex::Regex::new(r"state%3D([0-9a-f]{32})").unwrap().captures(&to).unwrap()[1].to_string();
    // A reply that isn't Steam's: back to the wizard with why.
    let back = app.redirect(&format!("/api/v1/steam/callback?state={state}")).await;
    assert!(back.starts_with("/setup?steam=error&reason="), "{back}");
    // Without `then`, Settings as before.
    let (state, _) = app.steam_login().await;
    let back = app.redirect(&format!("/api/v1/steam/callback?state={state}")).await;
    assert!(back.starts_with("/settings?steam=error"), "{back}");
}

#[tokio::test]
async fn an_ai_job_takes_a_limit_from_the_wizard() {
    let app = empty_app().await;
    let (status, v) = app.call("PUT", "/api/v1/jobs/scheduled/digests", Some(json!({ "schedule": "daily", "limit": 300 }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let row = v["tasks"].as_array().unwrap().iter().find(|t| t["task"] == "digests").unwrap().clone();
    assert_eq!((row["schedule"].as_str(), row["limit"].as_u64()), (Some("daily"), Some(300)));
    let (status, _) = app.call("PUT", "/api/v1/jobs/scheduled/sync", Some(json!({ "limit": 300 }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// A request from another device (`peer`), addressed to `host`, with a
/// paired browser's cookie or a token, as the wizard's page sends it.
struct Device<'a> {
    peer: [u8; 4],
    host: &'a str,
    cookie: Option<String>,
    bearer: Option<String>,
}

impl Device<'_> {
    async fn send(&self, app: &TestApp, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, axum::http::HeaderMap, Value) {
        let mut b = Request::builder().method(method).uri(uri).header(header::HOST, self.host);
        if let Some(c) = &self.cookie {
            b = b.header(header::COOKIE, c);
        }
        if let Some(t) = &self.bearer {
            b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        if method != "GET" {
            // The wizard's own page, as a browser says it.
            b = b.header(header::ORIGIN, format!("http://{}", self.host)).header("sec-fetch-site", "same-origin");
        }
        let body = match body {
            Some(v) => {
                b = b.header(header::CONTENT_TYPE, "application/json");
                Body::from(v.to_string())
            }
            None => Body::empty(),
        };
        let peer = Peer { addr: std::net::SocketAddr::from((self.peer, 50000)), tls: false };
        let resp =
            app.router.clone().layer(axum::extract::connect_info::MockConnectInfo(peer)).oneshot(b.body(body).unwrap()).await.unwrap();
        let (status, headers) = (resp.status(), resp.headers().clone());
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        (status, headers, serde_json::from_slice(&bytes).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned())))
    }
}

/// Setup from another device, through a pairing.
/// The phone pairs, then the wizard reads and records its steps, and Steam's
/// sign-in comes back to it: Steam's redirect arrives from Steam's site,
/// without the paired browser's `SameSite=Strict` cookie, and the sign-in's
/// state, with the `SameSite=Lax` cookie that ties it to this browser, is
/// what lets it in.
#[tokio::test]
async fn a_paired_device_runs_setup_and_signs_in_to_steam() {
    let steam = super::fake_steam(true, 1).await;
    let app = reconfigured(test_app_with(None, Some(&steam.uri())).await, |c| c.host = "0.0.0.0".into());
    let phone = |cookie: Option<String>| Device { peer: [192, 168, 0, 20], host: "den.lan:8787", cookie, bearer: None };
    let here = Device { peer: [127, 0, 0, 1], host: "127.0.0.1:8787", cookie: None, bearer: None };

    // Not paired: the API refuses with the way to pair.
    let (status, _, v) = phone(None).send(&app, "GET", "/api/v1/setup", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{v}");
    assert!(v["error"].as_str().unwrap().contains("Add a device"), "{v}");

    // Paired with a code made on this computer.
    let (_, _, code) = here.send(&app, "POST", "/api/v1/pairing", None).await;
    let (status, headers, v) = phone(None).send(&app, "POST", "/api/v1/pair", Some(json!({ "code": code["code"] }))).await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    let cookie = headers[header::SET_COOKIE].to_str().unwrap().split(';').next().unwrap().to_string();
    let paired = phone(Some(cookie));

    let (status, _, v) = paired.send(&app, "GET", "/api/v1/setup", None).await;
    assert_eq!((status, &v["environment"]["client_local"]), (StatusCode::OK, &json!(false)), "{v}");
    let (status, _, v) = paired.send(&app, "PUT", "/api/v1/setup", Some(json!({ "step": "steam" }))).await;
    assert_eq!((status, v["step"].as_str()), (StatusCode::OK, Some("steam")), "{v}");
    let (status, _, v) =
        paired.send(&app, "PUT", "/api/v1/settings", Some(json!({ "rawg_api_key": "0123456789abcdef0123456789abcdef" }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let (status, _, v) = paired.send(&app, "PUT", "/api/v1/settings", Some(json!({ "galaxy_db": "/tmp/galaxy-2.0.db" }))).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a path on the server is set there: {v}");

    // Steam's sign-in, back to the phone's address and the wizard.
    let (status, headers, _) = paired.send(&app, "GET", "/api/v1/steam/login?then=setup", None).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let to = headers[header::LOCATION].to_str().unwrap().to_string();
    let signing_in = headers[header::SET_COOKIE].to_str().unwrap().split(';').next().unwrap().to_string();
    assert!(to.contains(&urlenc("http://den.lan:8787/api/steam/callback?state=")), "{to}");
    let state = regex::Regex::new(r"state%3D([0-9a-f]{32})").unwrap().captures(&to).unwrap()[1].to_string();
    let claimed = "https://steamcommunity.com/openid/id/76561190000000001";
    let return_to = format!("http://den.lan:8787/api/steam/callback?state={state}");
    let fields = [
        ("state", state.as_str()),
        ("openid.ns", OPENID_NS),
        ("openid.mode", "id_res"),
        ("openid.op_endpoint", STEAM_OP_ENDPOINT),
        ("openid.claimed_id", claimed),
        ("openid.identity", claimed),
        ("openid.return_to", return_to.as_str()),
        ("openid.response_nonce", "2026-10-04T12:00:00Zc0ffee"),
        ("openid.assoc_handle", "1234567890"),
        ("openid.signed", "signed,op_endpoint,claimed_id,identity,return_to,response_nonce,assoc_handle"),
        ("openid.sig", "c2lnbmF0dXJl"),
    ];
    let query: Vec<String> = fields.iter().map(|(k, v)| format!("{k}={}", urlenc(v))).collect();
    let callback = format!("/api/steam/callback?{}", query.join("&"));
    // The sign-in's own cookie comes back with it, and it's no token.
    let (status, headers, v) = phone(Some(signing_in)).send(&app, "GET", &callback, None).await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{v}");
    assert_eq!(headers[header::LOCATION], "/setup?steam=ok");
    assert_eq!(here.send(&app, "GET", "/api/v1/settings", None).await.2["steam_id"], "76561190000000001");
    // Without a state of its own, the open callback turns anyone away (and Steam isn't asked: `expect(1)`).
    let (_, headers, _) = phone(None).send(&app, "GET", &callback, None).await;
    assert!(headers[header::LOCATION].to_str().unwrap().starts_with("/settings?steam=error"), "used once");

    // A read token can look, but not sign in with Steam, which saves the SteamID.
    let read = app.db.token_create("dashboard", crate::db::Scope::Read, "token").unwrap();
    let dashboard = Device { peer: [192, 168, 0, 30], host: "den.lan:8787", cookie: None, bearer: Some(read.token) };
    assert_eq!(dashboard.send(&app, "GET", "/api/v1/setup", None).await.0, StatusCode::OK);
    assert_eq!(dashboard.send(&app, "GET", "/api/v1/gog/bookmark", None).await.0, StatusCode::FORBIDDEN, "the bookmark's key");
    assert_eq!(dashboard.send(&app, "GET", "/api/v1/steam/login", None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(dashboard.send(&app, "PUT", "/api/v1/setup", Some(json!({ "step": "ai" }))).await.0, StatusCode::FORBIDDEN);
}

/// GOG's sign-in through the "Send to UpNext" bookmark, from
/// a container's host browser. The state is issued to the device that asked,
/// works once and only for it, and the code is exchanged as the window's is.
#[tokio::test]
async fn gog_signs_in_through_the_bookmark_with_a_state_of_its_own() {
    let gog = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/token"))
        .and(wiremock::matchers::query_param("grant_type", "authorization_code"))
        .and(wiremock::matchers::query_param("code", "gog-code-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "a", "refresh_token": "r", "expires_in": 3600, "user_id": "4800"
        })))
        .expect(1)
        .mount(&gog)
        .await;
    Mock::given(path("/userData.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "username": "tester" })))
        .mount(&gog)
        .await;
    let app = reconfigured(test_app_with(None, Some(&gog.uri())).await, |c| {
        c.host = "0.0.0.0".into();
        c.container = true;
    });
    let paired = |name: &str| {
        let t = app.db.token_create(name, crate::db::Scope::Full, "device").unwrap();
        Device { peer: [172, 17, 0, 1], host: "localhost:8787", cookie: Some(format!("upnext_device={}", t.token)), bearer: None }
    };
    let (browser, other) = (paired("host browser"), paired("phone"));

    let (status, _, start) = browser.send(&app, "POST", "/api/v1/gog/browser", None).await;
    assert_eq!(status, StatusCode::OK, "{start}");
    let state = start["state"].as_str().unwrap().to_string();
    assert_eq!(state.len(), 32);
    assert!(start["auth_url"].as_str().unwrap().starts_with("https://auth.gog.com/auth?"), "{start}");
    assert!(start["expires_at"].as_i64().unwrap() > crate::db::now() + 590);

    // The bookmark's key, made once for this install.
    let (status, _, b) = browser.send(&app, "GET", "/api/v1/gog/bookmark", None).await;
    assert_eq!(status, StatusCode::OK, "{b}");
    let key = b["key"].as_str().unwrap().to_string();
    assert_eq!(key.len(), 64);
    assert_eq!(other.send(&app, "GET", "/api/v1/gog/bookmark", None).await.2["key"], key.as_str(), "one key per install");

    // No state, a state never issued, another device's: refused, nothing exchanged.
    let finish = |state: &str, code: &str| Some(json!({ "state": state, "code": code, "key": key }));
    let (status, _, v) = browser.send(&app, "POST", "/api/v1/gog/callback", finish(&"0".repeat(32), "gog-code-1")).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
    let (status, _, v) = other.send(&app, "POST", "/api/v1/gog/callback", finish(&state, "gog-code-1")).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
    let (status, _, v) = browser.send(&app, "POST", "/api/v1/gog/callback", finish(&state, "gog-code-1")).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "another device's try used it up: {v}");
    // Without a pairing, not at all.
    let stranger = Device { peer: [172, 17, 0, 1], host: "localhost:8787", cookie: None, bearer: None };
    assert_eq!(stranger.send(&app, "POST", "/api/v1/gog/browser", None).await.0, StatusCode::UNAUTHORIZED);

    // The bookmark's way: GOG's success page's address, with the device's own state.
    let (_, _, start) = browser.send(&app, "POST", "/api/v1/gog/browser", None).await;
    let state = start["state"].as_str().unwrap().to_string();
    let url = "https://embed.gog.com/on_login_success?origin=client&code=gog-code-1";
    // Another page sending the browser to /gog/callback has no key (or a
    // bookmark from before the key): refused, unlogged, and the sign-in
    // still waits for the real one (security review L10).
    {
        use tracing_subscriber::layer::SubscriberExt;
        let _log = tracing::subscriber::set_default(tracing_subscriber::registry().with(crate::log_layer()));
        for forged in [json!({ "state": state, "code": url }), json!({ "state": state, "code": url, "key": "0".repeat(64) })] {
            let (status, _, v) = browser.send(&app, "POST", "/api/v1/gog/callback", Some(forged)).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
            assert!(v["error"].as_str().unwrap().contains("drag Send to UpNext"), "{v}");
        }
    }
    let logged = crate::jobs::recent_log(crate::db::Level::Warn);
    assert!(logged.iter().any(|l| l.message.contains("without this install's")), "the refusal is logged");
    assert!(!logged.iter().any(|l| l.message.contains("gog-code-1")), "and never the code");
    let (status, _, v) = browser.send(&app, "POST", "/api/v1/gog/callback", finish(&state, url)).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!((&v["connected"], &v["username"]), (&json!(true), &json!("tester")));
    let (status, _, v) = browser.send(&app, "POST", "/api/v1/gog/callback", finish(&state, url)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "once: {v}");
    // GOG's window stays this computer's, and there is none in a container.
    assert_eq!(browser.send(&app, "POST", "/api/v1/gog/window", None).await.0, StatusCode::FORBIDDEN);
}

/// "Test API key" with Steam down: one try, and the error at once.
#[tokio::test]
async fn steams_test_connection_tries_once() {
    let steam = MockServer::start().await;
    Mock::given(path("/IPlayerService/GetOwnedGames/v1/")).respond_with(ResponseTemplate::new(500)).mount(&steam).await;
    let app = reconfigured(test_app_with(None, Some(&steam.uri())).await, |c| {
        c.steam_api_key = Some("KEY".into());
        c.steam_id = Some("76561198000000000".into());
    });
    let (code, v) = app.call("POST", "/api/v1/steam/test", None).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{v}");
    assert!(v["error"].as_str().unwrap().contains("HTTP 500"), "{v}");
    assert_eq!(steam.received_requests().await.unwrap().len(), 1, "tried once");
}

/// "Restart the setup wizard" clears only what was ticked, after a
/// backup, and only from this computer.
#[tokio::test]
async fn restarting_setup_clears_what_was_chosen_after_a_backup() {
    let app = test_app().await;
    let portal = app.id_of("Portal 2").await;
    let celeste = app.id_of("Celeste").await;
    let ok = |(status, v): (StatusCode, Value)| {
        assert!(status.is_success(), "{status}: {v}");
        v
    };
    ok(app.call("PUT", "/api/v1/settings", Some(json!({ "steam_id": "76561197960287930", "auto_sync": false }))).await);
    ok(app.call("PUT", "/api/v1/settings", Some(json!({ "openai_thinking": "on" }))).await);
    ok(app.call("PUT", &format!("/api/v1/games/{portal}/override"), Some(json!({ "status": "done" }))).await);
    ok(app.call("PUT", &format!("/api/v1/games/{celeste}/later"), None).await);
    ok(app.call("POST", "/api/v1/taste/statements", Some(json!({ "kind": "like", "about": "tag:Puzzle" }))).await);
    let lineup = ok(app.call("POST", "/api/v1/picks", Some(json!({ "prompt": "something calm", "count": 1 }))).await);
    let (batch, pick) = (lineup["batch_id"].as_str().unwrap(), &lineup["picks"][0]["id"]);
    ok(app.call("PUT", &format!("/api/v1/picks/{batch}/rating"), Some(json!({ "title_id": pick, "verdict": "good" }))).await);
    ok(app.call("PUT", &format!("/api/v1/games/{portal}/rating"), Some(json!({ "rating": 4 }))).await);
    let statements = || async { app.get("/api/v1/taste/export").await["statements"].as_array().unwrap().len() };
    let backups = || async { app.get("/api/v1/backups").await["backups"].as_array().unwrap().len() };
    assert_eq!((statements().await, backups().await), (1, 0));

    let (status, v) = app.call("POST", "/api/v1/setup/reset", Some(json!({}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "nothing ticked: {v}");

    // Sign-ins, keys and the AI: the library, your statuses and the
    // settings for how UpNext runs stay.
    // The GOG and Epic sign-ins are kept on player 1's connections.
    use crate::signin::{DbSignIn, SignInStore};
    for kind in ["gog", "epic"] {
        DbSignIn::own(app.state.cfg().db_path.clone(), kind, crate::db::PlayerId::FIRST, None).save("{}").unwrap();
        assert_eq!(sign_in(&app, kind).as_deref(), Some("{}"), "signed in to {kind}");
    }
    let done = ok(app.call("POST", "/api/v1/setup/reset", Some(json!({ "accounts": true, "ai": true }))).await);
    assert_eq!(done["from_env"], json!([]), "{done}");
    let listed = app.get("/api/v1/backups").await;
    let first = &listed["backups"][0];
    assert_eq!(first["name"], done["backup"]);
    assert!(first["note"].as_str().unwrap().contains("sign-ins and keys, the AI choice"), "{first}");
    let s = app.get("/api/v1/settings").await;
    assert_eq!((s["steam_id"].clone(), s["auto_sync"].clone()), (Value::Null, json!(false)), "{s}");
    assert_ne!(s["openai_thinking"], "on", "{s}");
    assert_eq!(sign_in(&app, "gog"), None, "GOG signed out");
    assert_eq!(sign_in(&app, "epic"), None, "Epic signed out");
    assert_eq!(app.get("/api/v1/games").await["total"], 5);
    let game = app.get(&format!("/api/v1/games/{portal}")).await;
    assert_eq!((game["override"].clone(), game["my_rating"].clone()), (json!("done"), json!(4)));
    assert_eq!(app.get("/api/v1/play-later").await.as_array().unwrap().len(), 1);
    assert_eq!(statements().await, 1);

    // Statuses and history.
    ok(app.call("POST", "/api/v1/setup/reset", Some(json!({ "history": true }))).await);
    let game = app.get(&format!("/api/v1/games/{portal}")).await;
    assert_eq!((game["override"].clone(), game["my_rating"].clone()), (Value::Null, Value::Null), "statuses and ratings");
    assert_eq!(app.get("/api/v1/play-later").await, json!([]));
    assert_eq!(app.get("/api/v1/picks/history").await, json!([]));
    assert_eq!((statements().await, app.get("/api/v1/games").await["total"].clone()), (1, json!(5)));

    // The taste profile.
    let (status, _) = app.call("POST", "/api/v1/routines", Some(json!({ "name": "Friday with friends" }))).await;
    assert_eq!(status, StatusCode::CREATED);
    ok(app.call("POST", "/api/v1/setup/reset", Some(json!({ "taste": true }))).await);
    assert_eq!((statements().await, app.get("/api/v1/games").await["total"].clone()), (0, json!(5)));
    assert_eq!(app.get("/api/v1/routines").await["routines"], json!([]), "routines are part of the taste profile");

    // The whole library takes the taste profile with it, and setup shows again.
    ok(app.call("POST", "/api/v1/taste/statements", Some(json!({ "kind": "like", "about": "tag:Puzzle" }))).await);
    ok(app.call("POST", "/api/v1/setup/reset", Some(json!({ "library": true }))).await);
    assert_eq!(app.get("/api/v1/games").await["total"], 0);
    assert_eq!(statements().await, 0);
    assert_eq!(app.get("/api/v1/setup/state").await["needed"], true);
    assert_eq!(backups().await, 4, "a backup before each");

    // Never from another device, whatever its token.
    let app = reconfigured(app, |c| c.host = "0.0.0.0".into());
    let made = app.db.token_create("phone", crate::db::Scope::Full, "token").unwrap();
    let peer = Peer { addr: std::net::SocketAddr::from(([192, 168, 0, 20], 50000)), tls: false };
    let req = Request::post("/api/v1/setup/reset")
        .header(header::HOST, "den.lan:8787")
        .header(header::AUTHORIZATION, format!("Bearer {}", made.token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "ai": true }).to_string()))
        .unwrap();
    let resp = app.router.clone().layer(axum::extract::connect_info::MockConnectInfo(peer)).oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let dir = crate::jobs::backup::backups_dir(&app.state.cfg());
    assert_eq!(crate::backups::list(&dir).unwrap().len(), 4, "no backup either");
}

/// Player 1's GOG or Epic sign-in as the database keeps it.
fn sign_in(app: &TestApp, kind: &str) -> Option<String> {
    use rusqlite::OptionalExtension;
    rusqlite::Connection::open(&app.state.cfg().db_path)
        .unwrap()
        .query_row("SELECT secret FROM store_connections WHERE owner_id=1 AND kind=?1 ORDER BY id LIMIT 1", [kind], |r| r.get(0))
        .optional()
        .unwrap()
        .flatten()
}

/// A clear that fails leaves the GOG sign-in too, not only the database.
#[tokio::test]
async fn a_failed_clear_keeps_the_gog_sign_in() {
    let app = test_app().await;
    use crate::signin::{DbSignIn, SignInStore};
    let db_path = app.state.cfg().db_path.clone();
    DbSignIn::own(db_path, "gog", crate::db::PlayerId::FIRST, None).save("{}").unwrap();
    app.call("PUT", "/api/v1/settings", Some(json!({ "steam_id": "76561197960287930" }))).await;
    // The SteamID is kept on player 1's Steam connection, the
    // keys in the household's settings; clearing either fails.
    rusqlite::Connection::open(&app.state.cfg().db_path)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER keep_settings BEFORE DELETE ON settings BEGIN SELECT RAISE(ABORT, 'kept'); END;
             CREATE TRIGGER keep_connections BEFORE UPDATE ON store_connections BEGIN SELECT RAISE(ABORT, 'kept'); END;",
        )
        .unwrap();

    let (status, v) = app.call("POST", "/api/v1/setup/reset", Some(json!({ "accounts": true }))).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{v}");
    assert_eq!(sign_in(&app, "gog").as_deref(), Some("{}"), "still signed in to GOG");
    assert_eq!(app.get("/api/v1/settings").await["steam_id"], "76561197960287930");
}

/// Security review L6: an upload the client abandons midway leaves no work
/// file, and work files from before are swept by the next upload.
#[tokio::test]
async fn an_abandoned_galaxy_upload_leaves_nothing_behind() {
    let app = empty_app().await;
    let dir = app.state.base_cfg.db_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let work_files = || -> Vec<String> {
        std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(".galaxy-upload-"))
            .collect()
    };
    let stale = dir.join(".galaxy-upload-0123.db");
    std::fs::write(&stale, b"from a crash").unwrap();

    // A kilobyte arrives, then nothing: the client has gone, and axum drops
    // the request's handler, as the timeout does here.
    let first = futures::stream::once(async { Ok::<_, std::io::Error>(axum::body::Bytes::from(vec![0u8; 1024])) });
    let body = Body::from_stream(futures::StreamExt::chain(first, futures::stream::pending()));
    let req = Request::post("/api/v1/galaxy/upload")
        .header(header::HOST, "127.0.0.1:8787")
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(body)
        .unwrap();
    let cut = tokio::time::timeout(std::time::Duration::from_millis(300), app.send(req)).await;
    assert!(cut.is_err(), "the upload was still waiting for its body");
    assert_eq!(work_files(), Vec::<String>::new(), "the stale file went when the upload began, and its own when it was dropped");

    // The lock went with it: the next upload isn't kept waiting.
    let (status, _) = tokio::time::timeout(std::time::Duration::from_secs(5), app.upload_galaxy(b"not a database".repeat(100), None))
        .await
        .expect("a second upload runs once the first has gone");
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(work_files(), Vec::<String>::new());
}
