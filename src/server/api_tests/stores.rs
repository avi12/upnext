//! The stores beyond Steam, GOG and Heroic: games added by hand and
//! the Epic sign-in.

use super::*;

/// A list saved for the EA app joins the library as EA app copies, merged
/// with the same title from another store, and saving it again replaces it.
#[tokio::test]
async fn games_added_by_hand_join_the_library_and_leave_it_again() {
    let app = test_app().await;
    let lists = app.get("/api/v1/library/by-hand").await;
    let stores: Vec<&str> = lists["stores"].as_array().unwrap().iter().map(|s| s["store"].as_str().unwrap()).collect();
    assert_eq!(stores, ["origin", "uplay", "battlenet", "xboxone"]);
    assert_eq!(lists["stores"][0]["label"], "EA app");

    let (status, saved) =
        app.call("PUT", "/api/v1/library/by-hand/origin", Some(json!({ "titles": ["Dead Space\n- Portal 2™", "dead space"] }))).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!((saved["total"].as_u64(), saved["added"].as_u64(), saved["removed"].as_u64()), (Some(2), Some(2), Some(0)));
    assert_eq!(saved["lists"]["stores"][0]["titles"], json!(["Dead Space", "Portal 2"]));
    let portal = app.db.all_games(PlayerId::FIRST).unwrap().into_iter().find(|g| g.name == "Portal 2").unwrap();
    let mut via: Vec<(String, String)> = portal.copies.iter().map(|c| (c.source.clone(), c.launcher.clone())).collect();
    via.sort();
    assert_eq!(via, [("epic".into(), "heroic".into()), ("origin".into(), "manual".into()), ("steam".into(), "steam".into())]);

    let (_, saved) = app.call("PUT", "/api/v1/library/by-hand/origin", Some(json!({ "titles": [] }))).await;
    assert_eq!((saved["total"].as_u64(), saved["removed"].as_u64()), (Some(0), Some(2)));
    assert!(app.db.all_games(PlayerId::FIRST).unwrap().iter().all(|g| g.name != "Dead Space"));

    let (status, e) = app.call("PUT", "/api/v1/library/by-hand/steam", Some(json!({ "titles": ["Hades"] }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{e}");
}

/// Epic's sign-in from Settings: the pasted page signs in, Settings and
/// setup say who, a code Epic turns down is a 400, and signing out forgets it.
#[tokio::test]
async fn epic_signs_in_with_the_pasted_code_and_out_again() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/account/api/oauth/token"))
        .and(body_string_contains("code=0123456789abcdef0123456789abcdef"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "access_token": "a", "expires_in": 7200, "refresh_token": "r",
            "refresh_expires": 1_987_200, "account_id": "acct1", "displayName": "Player One" })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/account/api/oauth/token"))
        .and(body_string_contains("code=ffffffffffffffffffffffffffffffff"))
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_json(json!({ "errorCode": "errors.com.epicgames.account.oauth.authorization_code_not_found",
            "errorMessage": "Sorry the authorization code you supplied was not found." })),
        )
        .mount(&server)
        .await;
    let app = test_app_with(None, Some(&server.uri())).await;
    let before = app.get("/api/v1/epic").await;
    assert_eq!(before["connected"], false);
    assert!(before["auth_url"].as_str().unwrap().starts_with("https://www.epicgames.com/id/login"));

    let (status, e) = app.call("POST", "/api/v1/epic/login", Some(json!({ "code": "ffffffffffffffffffffffffffffffff" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(e["error"].as_str().unwrap().contains("not found"), "{e}");
    let (status, e) = app.call("POST", "/api/v1/epic/login", Some(json!({ "code": "not a code" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{e}");

    let page = r#"{"authorizationCode":"0123456789abcdef0123456789abcdef"}"#;
    let (status, signed) = app.call("POST", "/api/v1/epic/login", Some(json!({ "code": page }))).await;
    assert_eq!(status, StatusCode::OK, "{signed}");
    assert_eq!((signed["connected"].as_bool(), signed["display_name"].as_str()), (Some(true), Some("Player One")));
    assert_eq!(app.get("/api/v1/settings").await["epic"]["display_name"], "Player One");
    assert_eq!(app.get("/api/v1/setup").await["stores"]["epic"]["connected"], true);
    let profile = app.get("/api/v1/profile").await;
    let epic = profile["sources"].as_array().unwrap().iter().find(|s| s["source"] == "epic").unwrap().clone();
    assert_eq!((epic["configured"].as_bool(), epic["note"].as_str()), (Some(true), Some("Epic account")));

    let (status, out) = app.call("DELETE", "/api/v1/epic", None).await;
    assert_eq!((status, out["connected"].as_bool()), (StatusCode::OK, Some(false)));
}
