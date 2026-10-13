//! Each player's own short setup and tour, the wizard's
//! "Who else plays here?", and restarting setup clearing only the asking
//! player's taste profile and history when others play here too.

use super::*;

impl TestApp {
    /// A request for `player` by the `UpNext-Player` header, from this computer.
    async fn as_player(&self, player: i64, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(uri).header(header::HOST, "127.0.0.1:8787").header("UpNext-Player", player);
        let body = match body {
            Some(v) => {
                req = req.header(header::CONTENT_TYPE, "application/json");
                Body::from(v.to_string())
            }
            None => Body::empty(),
        };
        self.send(req.body(body).unwrap()).await
    }

    async fn add_player(&self, name: &str) -> i64 {
        let (status, v) = self.call("POST", "/api/v1/players", Some(json!({ "name": name }))).await;
        assert_eq!(status, StatusCode::CREATED, "{v}");
        v["id"].as_i64().unwrap()
    }
}

/// The parts of `GET /setup/state` that are the player's.
fn own(v: &Value) -> (bool, Option<&str>, &str, &str, bool) {
    (
        v["player_needed"].as_bool().unwrap(),
        v["player_step"].as_str(),
        v["player_setup"].as_str().unwrap(),
        v["tour"].as_str().unwrap(),
        v["tour_offer"].as_bool().unwrap(),
    )
}

#[tokio::test]
async fn a_new_player_has_their_own_setup_then_their_own_tour() {
    let app = test_app().await;
    // One player: nothing new. Their own setup was the household's wizard.
    let one = app.get("/api/v1/setup/state").await;
    assert_eq!(own(&one), (false, None, "done", "open", true), "{one}");

    let sam = app.add_player("Sam").await;
    assert_eq!(own(&app.get("/api/v1/setup/state").await), (false, None, "done", "open", true), "player 1 is set up");
    let (status, v) = app.as_player(sam, "GET", "/api/v1/setup/state", None).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(own(&v), (true, None, "open", "open", false), "Sam's own setup first, then the tour: {v}");
    assert_eq!((v["needed"].as_bool(), v["setup"].as_str()), (Some(false), Some("open")), "the household's part is as it was");

    // Where Sam got to is Sam's.
    let (status, v) = app.as_player(sam, "PUT", "/api/v1/setup", Some(json!({ "player_step": "games" }))).await;
    assert_eq!((status, own(&v)), (StatusCode::OK, (true, Some("games"), "open", "open", false)), "{v}");
    let (status, v) = app.as_player(sam, "PUT", "/api/v1/setup", Some(json!({ "player_step": "steam" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "a household step isn't a player's: {v}");
    assert_eq!(app.get("/api/v1/setup/state").await["player_step"], Value::Null);

    // Done: the tour's offer is Sam's now, and turning it down leaves player 1's.
    let (_, v) = app.as_player(sam, "PUT", "/api/v1/setup", Some(json!({ "player_setup": "done" }))).await;
    assert_eq!(own(&v), (false, Some("games"), "done", "open", true), "{v}");
    let (_, v) = app.as_player(sam, "PUT", "/api/v1/setup", Some(json!({ "tour": "skipped" }))).await;
    assert_eq!(own(&v), (false, Some("games"), "done", "skipped", false), "{v}");
    assert_eq!(own(&app.get("/api/v1/setup/state").await), (false, None, "done", "open", true), "player 1's tour is theirs");

    // Run again from Settings.
    let (_, v) = app.as_player(sam, "PUT", "/api/v1/setup", Some(json!({ "player_setup": "open" }))).await;
    assert!(own(&v).0, "{v}");
}

#[tokio::test]
async fn the_first_players_tour_carries_on_from_the_households() {
    let app = test_app().await;
    // Taken before players had their own tour, in the household's state.
    app.db.setting_set(crate::db::PlayerId::FIRST, "setup.state", Some(r#"{"step":"done","setup":"done","tour":"done"}"#)).unwrap();
    let v = app.get("/api/v1/setup/state").await;
    assert_eq!(own(&v), (false, None, "done", "done", false), "{v}");
    let sam = app.add_player("Sam").await;
    let (_, v) = app.as_player(sam, "GET", "/api/v1/setup/state", None).await;
    assert_eq!(own(&v).3, "open", "a new player's tour is their own to take: {v}");
}

#[tokio::test]
async fn whoever_goes_through_the_wizard_has_had_their_own_setup() {
    let app = test_app().await;
    let sam = app.add_player("Sam").await;
    let (_, v) = app.as_player(sam, "PUT", "/api/v1/setup", Some(json!({ "step": "players", "setup": "done" }))).await;
    assert_eq!((v["step"].as_str(), own(&v)), (Some("players"), (false, None, "done", "open", true)), "{v}");
}

#[tokio::test]
async fn a_bound_token_can_end_its_own_setup_but_not_the_households() {
    let app = test_app().await;
    let sam = app.add_player("Sam").await;
    let made = app.db.token_create_for("Sam's phone", crate::db::Scope::Full, "token", Some(crate::db::PlayerId(sam))).unwrap();
    let put = |body: Value| {
        Request::put("/api/v1/setup")
            .header(header::HOST, "127.0.0.1:8787")
            .header(header::AUTHORIZATION, format!("Bearer {}", made.token))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    let (status, v) = app.send(put(json!({ "setup": "skipped" }))).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
    let (status, v) = app.send(put(json!({ "player_setup": "done", "tour": "done" }))).await;
    assert_eq!((status, own(&v)), (StatusCode::OK, (false, None, "done", "done", false)), "{v}");
    assert_eq!(app.get("/api/v1/setup/state").await["setup"], "open", "the household's is as it was");
}

#[tokio::test]
async fn restarting_setup_clears_only_your_own_taste_and_history_when_others_play_here() {
    let app = test_app().await;
    let sam = app.add_player("Sam").await;
    let games = app.get("/api/v1/games").await;
    let id = games["items"][0]["id"].as_i64().unwrap();
    let mark = format!("/api/v1/games/{id}/override");
    let later = format!("/api/v1/games/{id}/later");
    let routine = json!({ "name": "Friday with friends" });
    for p in [1, sam] {
        let (status, v) = app.as_player(p, "PUT", &mark, Some(json!({ "status": "done" }))).await;
        assert_eq!(status, StatusCode::OK, "{v}");
        let (status, v) = app.as_player(p, "PUT", &later, None).await;
        assert!(status.is_success(), "{v}");
        let (status, v) = app.as_player(p, "POST", "/api/v1/routines", Some(routine.clone())).await;
        assert_eq!(status, StatusCode::CREATED, "{v}");
    }
    let (status, v) = app.as_player(sam, "POST", "/api/v1/setup/reset", Some(json!({ "history": true, "taste": true }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");

    let state = |p: i64| {
        let game = format!("/api/v1/games/{id}");
        let app = &app;
        async move {
            let (_, game) = app.as_player(p, "GET", &game, None).await;
            let (_, list) = app.as_player(p, "GET", "/api/v1/play-later", None).await;
            let (_, routines) = app.as_player(p, "GET", "/api/v1/routines", None).await;
            (game["override"].clone(), list.as_array().map_or(0, Vec::len), routines["routines"].as_array().map_or(0, Vec::len))
        }
    };
    assert_eq!(state(sam).await, (Value::Null, 0, 0), "Sam's own are gone");
    assert_eq!(state(1).await, (json!("done"), 1, 1), "player 1 keeps theirs");
}

/// Restarting setup's sign-ins and keys with other players here clears the
/// asker's own SteamID and leaves the household's keys, which everyone's
/// libraries are read with, and the other players' accounts.
#[tokio::test]
async fn restarting_setup_with_others_here_keeps_the_households_keys() {
    let app = test_app().await;
    let sam = app.add_player("Sam").await;
    let household = json!({ "steam_api_key": "0123456789ABCDEF0123456789ABCDEF", "rawg_api_key": "FEDCBA9876543210FEDCBA9876543210" });
    let (status, v) = app.call("PUT", "/api/v1/settings", Some(household)).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    for (p, id) in [(1, "76561197960287930"), (sam, "76561197960287931")] {
        let (status, v) = app.as_player(p, "PUT", "/api/v1/settings", Some(json!({ "steam_id": id }))).await;
        assert_eq!(status, StatusCode::OK, "{v}");
    }
    app.state.db.meta_set("profile", r#"{"name":"Player One"}"#).unwrap();

    let (status, v) = app.as_player(sam, "POST", "/api/v1/setup/reset", Some(json!({ "accounts": true }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let (_, sams) = app.as_player(sam, "GET", "/api/v1/settings", None).await;
    assert_eq!(sams["steam_id"], Value::Null, "Sam's own SteamID is cleared: {sams}");
    let firsts = app.get("/api/v1/settings").await;
    assert_eq!(firsts["steam_id"], "76561197960287930", "player 1's stays: {firsts}");
    for key in ["steam_api_key", "rawg_api_key"] {
        assert_eq!(firsts[format!("{key}_set")], true, "{key} is everyone's and stays: {firsts}");
    }
    assert!(app.state.db.meta_get("profile").unwrap().is_some(), "the top bar's Steam profile is player 1's");

    // Player 1 restarting clears their own SteamID and profile, and still
    // not the household's keys while Sam plays here.
    let (status, v) = app.call("POST", "/api/v1/setup/reset", Some(json!({ "accounts": true }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let firsts = app.get("/api/v1/settings").await;
    assert_eq!(firsts["steam_id"], Value::Null, "{firsts}");
    assert_eq!(firsts["steam_api_key_set"], true, "{firsts}");
    assert!(app.state.db.meta_get("profile").unwrap().is_none());
}

/// A player other than the first is offered a Steam account signed in on
/// this computer that nobody has yet, and told whose this computer's
/// Heroic folder is, rather than being shown the first player's as theirs.
#[tokio::test]
async fn other_players_find_their_own_steam_account_on_this_computer() {
    let app = test_app().await;
    let folders = tempfile::tempdir().unwrap();
    let steam = folders.path().join("steam");
    std::fs::create_dir_all(steam.join("config")).unwrap();
    std::fs::write(steam.join("config/loginusers.vdf"), crate::testutil::fixture("steam/loginusers.vdf")).unwrap();
    // A Steam folder is one with the client's app cache too.
    std::fs::create_dir_all(steam.join("appcache")).unwrap();
    std::fs::write(steam.join("appcache/appinfo.vdf"), "").unwrap();
    let heroic = folders.path().join("heroic");
    std::fs::create_dir_all(heroic.join("store_cache")).unwrap();
    let app = reconfigured(app, |cfg| {
        cfg.steam_dir = Some(steam.clone());
        cfg.heroic_dir = Some(heroic.clone());
    });
    let sam = app.add_player("Sam").await;
    let first = app.get("/api/v1/players").await["players"][0]["name"].as_str().unwrap().to_string();
    let accounts = |v: &Value| -> Vec<(Value, Value, Value)> {
        v["stores"]["steam"]["local_accounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| (a["steam_id"].clone(), a["persona"].clone(), a["player"].clone()))
            .collect()
    };

    // The first player reads the client's most recent account, as before.
    let theirs = app.get("/api/v1/setup").await;
    assert_eq!(theirs["stores"]["steam"]["account"], "76561198000000002", "{theirs}");
    assert_eq!(theirs["stores"]["heroic"]["owner"], json!(first), "{theirs}");

    // Sam is offered the other one, and told whose the rest are, with
    // nothing more of another player's account than that.
    let (_, sams) = app.as_player(sam, "GET", "/api/v1/setup", None).await;
    let steam_found = &sams["stores"]["steam"];
    assert_eq!((&steam_found["account"], &steam_found["persona"]), (&json!("76561198000000001"), &json!("First Player")), "{sams}");
    assert_eq!(
        accounts(&sams),
        [(Value::Null, Value::Null, json!(first)), (json!("76561198000000001"), json!("First Player"), Value::Null)],
        "{sams}"
    );
    assert!(!sams.to_string().contains("76561198000000002"), "the first player's SteamID isn't Sam's to see: {sams}");
    assert_eq!(sams["stores"]["heroic"]["owner"], json!(first), "this computer's Heroic is the first player's: {sams}");
    let (_, profile) = app.as_player(sam, "GET", "/api/v1/profile", None).await;
    let source = |name: &str| profile["sources"].as_array().unwrap().iter().find(|s| s["source"] == name).cloned();
    if let Some(gog) = source("gog") {
        assert_eq!(gog["configured"], false, "the first player's Heroic isn't Sam's: {profile}");
    }

    // Once Sam takes it, it's Sam's, and the first player's stays theirs.
    let (status, v) = app.as_player(sam, "PUT", "/api/v1/settings", Some(json!({ "steam_id": "76561198000000001" }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let (_, sams) = app.as_player(sam, "GET", "/api/v1/setup", None).await;
    assert_eq!(sams["stores"]["steam"]["account_source"], "app", "{sams}");
    assert_eq!(accounts(&sams)[1], (json!("76561198000000001"), json!("First Player"), json!("Sam")), "{sams}");
    // The first player sees whose Sam's is, and only that, and their own in full.
    let theirs = app.get("/api/v1/setup").await;
    assert_eq!(
        accounts(&theirs),
        [(json!("76561198000000002"), json!("Second Player"), json!(first)), (Value::Null, Value::Null, json!("Sam"))],
        "{theirs}"
    );
    // The account the first player's Steam reads here is theirs, unset as it is.
    let (status, v) = app.as_player(sam, "PUT", "/api/v1/settings", Some(json!({ "steam_id": "76561198000000002" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    let (_, profile) = app.as_player(sam, "GET", "/api/v1/profile", None).await;
    let steam_source = profile["sources"].as_array().unwrap().iter().find(|s| s["source"] == "steam").cloned().unwrap();
    assert_eq!(steam_source["configured"], true, "Sam's Steam account is signed in here: {profile}");
    assert_eq!(app.get("/api/v1/setup").await["stores"]["steam"]["account"], "76561198000000002");
}

#[tokio::test]
async fn a_steam_account_another_player_linked_cant_be_taken() {
    let app = test_app().await;
    let sam = app.add_player("Sam").await;
    let first = app.get("/api/v1/players").await["players"][0]["name"].as_str().unwrap().to_string();
    let (status, v) = app.call("PUT", "/api/v1/settings", Some(json!({ "steam_id": "76561197960287930" }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");

    // Sam can't take the first player's account, by setting or connection.
    let (status, v) = app.as_player(sam, "PUT", "/api/v1/settings", Some(json!({ "steam_id": "76561197960287930" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert!(v["error"].as_str().unwrap().contains(&format!("is {first}'s already")), "{v}");
    let body = json!({ "kind": "steam", "settings": { "steam_id": "76561197960287930" } });
    let (status, v) = app.as_player(sam, "POST", "/api/v1/connections", Some(body)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");

    // Their own is theirs, set again as often as they like.
    for _ in 0..2 {
        let (status, v) = app.as_player(sam, "PUT", "/api/v1/settings", Some(json!({ "steam_id": "76561197960287931" }))).await;
        assert_eq!(status, StatusCode::OK, "{v}");
    }
    // And the first player can't take Sam's either.
    let connection = app.get("/api/v1/accounts").await["connections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == "steam")
        .map(|c| c["id"].as_i64().unwrap())
        .unwrap();
    let body = json!({ "settings": { "steam_id": "76561197960287931" } });
    let (status, v) = app.call("PUT", &format!("/api/v1/connections/{connection}"), Some(body)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert!(v["error"].as_str().unwrap().contains("is Sam's already"), "{v}");
    assert_eq!(app.get("/api/v1/settings").await["steam_id"], "76561197960287930");
}

#[tokio::test]
async fn a_steam_sign_in_cant_take_another_players_account() {
    let steam = fake_steam(true, 1).await;
    let app = test_app_with(None, Some(&steam.uri())).await;
    let sam = app.add_player("Sam").await;
    let (status, v) = app.as_player(sam, "PUT", "/api/v1/settings", Some(json!({ "steam_id": "76561190000000001" }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");

    // The first player signs in to Steam as Sam's account: Steam says yes,
    // and UpNext still says it's Sam's.
    let (state, cookie) = app.steam_login().await;
    let back_here = format!("http://127.0.0.1:8787/api/steam/callback?state={state}");
    let to = app.steam_callback(&state, Some(&cookie), &back_here, STEAM_OP_ENDPOINT).await;
    assert!(to.starts_with("/settings?steam=error&reason="), "{to}");
    assert!(to.contains(&urlenc("is Sam's already")), "{to}");
    assert!(app.get("/api/v1/settings").await["steam_id"].is_null());
}

#[tokio::test]
async fn a_steam_sign_in_from_a_players_own_setup_comes_back_to_it() {
    let app = test_app().await;
    let to = app.redirect("/api/v1/steam/login?then=player").await;
    let state = regex::Regex::new(r"state%3D([0-9a-f]{32})").unwrap().captures(&to).unwrap()[1].to_string();
    let back = app.redirect(&format!("/api/v1/steam/callback?state={state}")).await;
    assert!(back.starts_with("/setup/player?steam=error&reason="), "{back}");
}
