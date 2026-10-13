//! "Add to Steam" over the API, on a Steam folder made
//! for the test (tests never read the machine's own Steam).

use super::*;

/// A Steam folder with a login and one account that has no shortcuts yet.
fn steam_folder(dir: &std::path::Path) -> std::path::PathBuf {
    let steam = dir.join("steam");
    std::fs::create_dir_all(steam.join("appcache")).unwrap();
    std::fs::create_dir_all(steam.join("config")).unwrap();
    std::fs::create_dir_all(steam.join("userdata/12345678/config")).unwrap();
    std::fs::write(steam.join("appcache/appinfo.vdf"), b"").unwrap();
    std::fs::write(steam.join("config/loginusers.vdf"), r#""users" { "76561197972611406" { "MostRecent" "1" } }"#).unwrap();
    steam
}

#[tokio::test]
async fn big_screen_goes_into_each_steam_library_once() {
    let app = test_app().await;
    let steam = steam_folder(app._dir.path());
    let app = reconfigured(app, |c| c.steam_dir = Some(steam.clone()));

    let before = app.get("/api/v1/steam/shortcut").await;
    assert_eq!(before["steam_running"], json!(false));
    let file = steam.join("userdata/12345678/config/shortcuts.vdf");
    if !cfg!(feature = "desktop") {
        // A server with no app window has nothing for Steam to open, so
        // Steam's libraries aren't read at all.
        assert!(before["unavailable"].as_str().unwrap().contains("without its app window"), "{before}");
        assert_eq!(before["target"], Value::Null);
        assert_eq!(before["accounts"], json!([]));
        assert_eq!(app.call("POST", "/api/v1/steam/shortcut", None).await.0, StatusCode::CONFLICT);
        assert!(!file.exists());
        return;
    }
    assert_eq!(before["unavailable"], Value::Null, "{before}");
    assert_eq!(
        before["accounts"],
        json!([{ "account_id": "12345678", "added": false, "layout": false, "layout_error": null, "error": null }]),
        "{before}"
    );
    let args = before["target"]["arguments"].as_str().unwrap();
    assert!(args.ends_with("--big-screen"), "{args}");
    assert!(std::path::Path::new(before["target"]["program"].as_str().unwrap()).is_absolute());

    let (status, after) = app.call("POST", "/api/v1/steam/shortcut", None).await;
    assert_eq!(status, StatusCode::OK, "{after}");
    assert_eq!(
        after["accounts"],
        json!([{ "account_id": "12345678", "added": true, "layout": true, "layout_error": null, "error": null }])
    );
    let once = std::fs::read(&file).unwrap();
    assert_eq!(app.call("POST", "/api/v1/steam/shortcut", None).await.0, StatusCode::OK);
    assert_eq!(std::fs::read(&file).unwrap(), once, "pressed twice, nothing changes");
}

#[tokio::test]
async fn add_to_steam_says_why_it_cant() {
    let app = test_app().await;
    // No Steam on the computer.
    let none = app.get("/api/v1/steam/shortcut").await;
    assert!(none["accounts"].as_array().unwrap().is_empty());
    assert!(none["unavailable"].as_str().is_some(), "{none}");
    let (status, refused) = app.call("POST", "/api/v1/steam/shortcut", None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");

    // In a container, whatever is mounted.
    let steam = steam_folder(app._dir.path());
    let app = reconfigured(app, |c| {
        c.steam_dir = Some(steam.clone());
        c.container = true;
    });
    let boxed = app.get("/api/v1/steam/shortcut").await;
    assert!(boxed["unavailable"].as_str().unwrap().contains("container"), "{boxed}");
    assert_eq!(boxed["target"], Value::Null);
    let (status, refused) = app.call("POST", "/api/v1/steam/shortcut", None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(refused["error"].as_str().unwrap().contains("container"), "{refused}");
    assert!(!steam.join("userdata/12345678/config/shortcuts.vdf").exists());
}

#[tokio::test]
async fn a_pairing_code_can_open_setup() {
    let app = test_app().await;
    let (status, made) = app.call("POST", "/api/v1/pairing?then=setup", None).await;
    assert_eq!(status, StatusCode::CREATED, "{made}");
    assert_eq!(made["code"].as_str().map(str::len), Some(6));
    let (status, _) = app.call("POST", "/api/v1/pairing?then=elsewhere", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Big screen's setup waits on a code it showed: a code that ran out must
/// not read as a phone that paired, though neither leaves a code waiting.
#[tokio::test]
async fn a_code_that_runs_out_is_not_a_paired_phone() {
    let app = test_app().await;
    let outcome = |code: &str| format!("/api/v1/pairing/outcome?code={code}");
    let code = app.call("POST", "/api/v1/pairing?then=setup", None).await.1["code"].as_str().unwrap().to_string();
    assert_eq!(app.get(&outcome(&code)).await, json!({ "outcome": "waiting", "device": null }));
    app.state.remote.pairing.expire();
    assert_eq!(app.get("/api/v1/pairing").await, Value::Null);
    assert_eq!(app.get(&outcome(&code)).await, json!({ "outcome": "ended", "device": null }));

    let code = app.call("POST", "/api/v1/pairing?then=setup", None).await.1["code"].as_str().unwrap().to_string();
    let (status, paired) = app.call("POST", "/api/v1/pair", Some(json!({ "code": code, "name": "Phone" }))).await;
    assert_eq!(status, StatusCode::CREATED, "{paired}");
    assert_eq!(app.get(&outcome(&code)).await, json!({ "outcome": "paired", "device": paired }));
    assert_eq!(app.get(&outcome("000000")).await["outcome"], "ended", "another code says nothing about this one");
}
