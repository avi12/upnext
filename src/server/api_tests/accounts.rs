//! Store accounts, connections and AI accounts through their routes
//! (R-CHECKS-1): who may change, offer, use and remove each, and
//! that an AI account's keys never come back.

use super::players::{HERE, LAN, Req, household, req, send, token};
use super::*;

/// A request from this computer for `player`, which must answer `want`.
async fn as_player(app: &TestApp, player: i64, method: &str, uri: &str, body: Option<Value>, want: StatusCode) -> Value {
    let p = player.to_string();
    let a = send(app, HERE, Req { player: Some(&p), body, ..req(method, uri) }).await;
    assert_eq!(a.status, want, "{method} {uri} as player {player}: {}", a.body);
    a.body
}

/// The store account of `store` in a `GET /accounts` answer.
fn account<'a>(v: &'a Value, store: &str) -> &'a Value {
    v["accounts"].as_array().unwrap().iter().find(|a| a["store"] == store).unwrap_or_else(|| panic!("no {store} account: {v}"))
}

/// Whether `player`'s library lists a game called `name`.
async fn lists(app: &TestApp, player: i64, name: &str) -> bool {
    let v = as_player(app, player, "GET", &format!("/api/games?q={}", name.replace(' ', "+")), None, StatusCode::OK).await;
    v["items"].as_array().unwrap().iter().any(|g| g["name"] == name)
}

#[tokio::test]
async fn store_accounts_are_changed_by_their_owner_and_used_by_whoever_they_are_offered_to() {
    let (app, sam) = household().await;
    let ok = StatusCode::OK;

    // Player 1's two accounts, offered to everyone, as each player sees them.
    let firsts = as_player(&app, 1, "GET", "/api/accounts", None, ok).await;
    let steam = account(&firsts, "steam").clone();
    assert_eq!((&steam["mine"], &steam["sharing"], &steam["games"]), (&json!(true), &json!("everyone"), &json!(4)), "{firsts}");
    assert_eq!(steam["players"], json!([{ "player_id": sam, "name": "Sam", "offered": false, "uses": true, "plays_on": false }]));
    let id = steam["id"].as_i64().unwrap();
    let uri = format!("/api/accounts/{id}");
    let sams = as_player(&app, sam, "GET", "/api/accounts", None, ok).await;
    assert_eq!(sams["connections"], json!([]), "Sam has no connections of their own: {sams}");
    let offered = account(&sams, "steam");
    assert_eq!((&offered["mine"], &offered["uses"], &offered["players"]), (&json!(false), &json!(true), &json!([])), "{sams}");
    assert!(lists(&app, sam, "Hades").await, "an account offered to everyone is in Sam's library");

    // Only the owner changes it.
    let rename = json!({ "name": "Sam's now" });
    as_player(&app, sam, "PUT", &uri, Some(rename), StatusCode::FORBIDDEN).await;
    as_player(&app, 1, "PUT", "/api/accounts/999", Some(json!({ "name": "x" })), StatusCode::NOT_FOUND).await;
    for bad in [
        json!({ "sharing": "friends" }),
        json!({ "name": " " }),
        json!({ "players": [{ "player_id": 1 }] }),
        json!({ "players": [{ "player_id": 999 }] }),
    ] {
        as_player(&app, 1, "PUT", &uri, Some(bad.clone()), StatusCode::BAD_REQUEST).await;
    }
    assert_eq!(
        account(&as_player(&app, 1, "GET", "/api/accounts", None, ok).await, "steam")["name"],
        steam["name"],
        "nothing was half-saved"
    );

    // The owner renames it, offers it to Sam alone and counts Sam's playtime on it.
    let body = json!({ "name": "Living room", "sharing": "chosen", "players": [{ "player_id": sam, "offered": true, "plays_on": true }] });
    let changed = as_player(&app, 1, "PUT", &uri, Some(body), ok).await;
    assert_eq!((&changed["name"], &changed["sharing"]), (&json!("Living room"), &json!("chosen")), "{changed}");
    assert_eq!(changed["players"], json!([{ "player_id": sam, "name": "Sam", "offered": true, "uses": true, "plays_on": true }]));
    let offered = as_player(&app, sam, "GET", "/api/accounts", None, ok).await;
    let offered = account(&offered, "steam");
    assert_eq!((&offered["name"], &offered["plays_on"], &offered["uses"]), (&json!("Living room"), &json!(true), &json!(true)));

    // Sam's own switch takes its games out of Sam's library, and only Sam's.
    let use_uri = format!("{uri}/use");
    let off = as_player(&app, sam, "PUT", &use_uri, Some(json!({ "uses": false })), ok).await;
    assert_eq!(off["uses"], false, "{off}");
    assert!(!lists(&app, sam, "Hades").await, "switched off for Sam");
    assert!(lists(&app, 1, "Hades").await, "still player 1's");
    let theirs = as_player(&app, 1, "GET", "/api/accounts", None, ok).await;
    assert_eq!(account(&theirs, "steam")["players"][0]["uses"], false, "the owner sees Sam's switch: {theirs}");
    as_player(&app, sam, "PUT", &use_uri, Some(json!({ "uses": true })), ok).await;
    assert!(lists(&app, sam, "Hades").await, "switched back on");

    // The owner's own accounts are always theirs, and an unknown one is 404.
    as_player(&app, 1, "PUT", &use_uri, Some(json!({ "uses": false })), StatusCode::BAD_REQUEST).await;
    as_player(&app, sam, "PUT", "/api/accounts/999/use", Some(json!({ "uses": true })), StatusCode::NOT_FOUND).await;

    // Taken back: no longer offered, so Sam can't switch it on or see it.
    as_player(&app, 1, "PUT", &uri, Some(json!({ "sharing": "owner" })), ok).await;
    as_player(&app, sam, "PUT", &use_uri, Some(json!({ "uses": true })), StatusCode::FORBIDDEN).await;
    let sams = as_player(&app, sam, "GET", "/api/accounts", None, ok).await;
    assert!(sams["accounts"].as_array().unwrap().iter().all(|a| a["store"] != "steam"), "{sams}");
    assert!(!lists(&app, sam, "Hades").await);
}

#[tokio::test]
async fn a_connection_is_changed_and_removed_by_its_owner_alone() {
    let (app, sam) = household().await;
    let ok = StatusCode::OK;
    let firsts = as_player(&app, 1, "GET", "/api/accounts", None, ok).await;
    let steam = firsts["connections"].as_array().unwrap().iter().find(|c| c["kind"] == "steam").unwrap()["id"].as_i64().unwrap();
    let uri = format!("/api/connections/{steam}");

    // Another player can neither change nor remove it.
    as_player(&app, sam, "PUT", &uri, Some(json!({ "settings": { "steam_include_hidden": true } })), StatusCode::FORBIDDEN).await;
    as_player(&app, sam, "DELETE", &uri, None, StatusCode::FORBIDDEN).await;
    as_player(&app, 1, "DELETE", "/api/connections/999", None, StatusCode::NOT_FOUND).await;
    assert!(lists(&app, 1, "Hades").await);

    // Removing it takes its account and games with it, for everyone.
    as_player(&app, 1, "DELETE", &uri, None, StatusCode::NO_CONTENT).await;
    let firsts = as_player(&app, 1, "GET", "/api/accounts", None, ok).await;
    assert!(firsts["connections"].as_array().unwrap().iter().all(|c| c["id"] != steam), "{firsts}");
    assert!(firsts["accounts"].as_array().unwrap().iter().all(|a| a["store"] != "steam"), "{firsts}");
    assert!(!lists(&app, 1, "Hades").await && !lists(&app, sam, "Hades").await);
    assert!(lists(&app, 1, "Control").await, "the Epic connection stays");
    as_player(&app, 1, "DELETE", &uri, None, StatusCode::NOT_FOUND).await;
}

#[tokio::test]
async fn ai_accounts_keep_their_keys_and_go_where_their_owner_offers_them() {
    let (app, sam) = household().await;
    let (ok, created) = (StatusCode::OK, StatusCode::CREATED);

    // Player 1's is this computer's, offered to everyone and used by both.
    let firsts = as_player(&app, 1, "GET", "/api/ai-accounts", None, ok).await;
    let computer = &firsts[0];
    assert_eq!(
        (&computer["this_computer"], &computer["in_use"], &computer["mine"]),
        (&json!(true), &json!(true), &json!(true)),
        "{firsts}"
    );
    let computer = computer["id"].as_i64().unwrap();

    // Refused before anything is written.
    for bad in [
        json!({ "backend": "telepathy" }),
        json!({ "settings": { "steam_id": "76561197960287930" } }),
        json!({ "settings": { "codex_effort": "loud" } }),
        json!({ "sharing": "friends" }),
        json!({ "sharing": "chosen", "offered_to": [sam] }),
        json!({ "sharing": "chosen", "offered_to": [999] }),
    ] {
        as_player(&app, sam, "POST", "/api/ai-accounts", Some(bad.clone()), StatusCode::BAD_REQUEST).await;
    }
    assert_eq!(app.db.ai_accounts_for(PlayerId(sam)).unwrap().len(), 1, "only player 1's");

    // Sam's own key-based account, offered to player 1 alone. Its key is
    // named, never shown.
    let key = "sk-ant-api03-not-a-real-key-0000000000";
    let body = json!({ "backend": "api-key", "settings": { "anthropic_api_key": key, "model": "claude-sonnet-5-5" }, "sharing": "chosen", "offered_to": [1] });
    let made = as_player(&app, sam, "POST", "/api/ai-accounts", Some(body), created).await;
    assert!(!made.to_string().contains(key), "{made}");
    assert_eq!(
        (&made["backend"], &made["keys_set"], &made["settings"]["model"]),
        (&json!("api-key"), &json!(["anthropic_api_key"]), &json!("claude-sonnet-5-5"))
    );
    assert_eq!(
        (&made["sharing"], &made["offered_to"], &made["in_use"]),
        (&json!("chosen"), &json!([1]), &json!(true)),
        "Sam had none, so uses it: {made}"
    );
    let id = made["id"].as_i64().unwrap();
    let uri = format!("/api/ai-accounts/{id}");
    let listed = as_player(&app, 1, "GET", "/api/ai-accounts", None, ok).await;
    assert!(!listed.to_string().contains(key), "{listed}");
    let theirs = listed.as_array().unwrap().iter().find(|a| a["id"] == id).expect("offered to player 1");
    assert_eq!((&theirs["mine"], &theirs["offered_to"], &theirs["keys_set"]), (&json!(false), &json!([]), &json!(["anthropic_api_key"])));

    // Player 1 may use it, but not change or remove it.
    let used = as_player(&app, 1, "PUT", &format!("{uri}/use"), None, ok).await;
    assert_eq!(used["in_use"], true, "{used}");
    as_player(&app, 1, "PUT", &uri, Some(json!({ "settings": { "model": "claude-haiku-5-5" } })), StatusCode::FORBIDDEN).await;
    as_player(&app, 1, "DELETE", &uri, None, StatusCode::FORBIDDEN).await;
    as_player(&app, 1, "PUT", "/api/ai-accounts/999", Some(json!({})), StatusCode::NOT_FOUND).await;
    as_player(&app, 1, "PUT", "/api/ai-accounts/999/use", None, StatusCode::NOT_FOUND).await;

    // Its owner changes it; a cleared key is gone and the rest is kept.
    let body = json!({ "settings": { "anthropic_api_key": null, "model": "claude-haiku-5-5" } });
    let changed = as_player(&app, sam, "PUT", &uri, Some(body), ok).await;
    assert_eq!(
        (&changed["keys_set"], &changed["settings"]["model"], &changed["backend"]),
        (&json!([]), &json!("claude-haiku-5-5"), &json!("api-key"))
    );
    as_player(&app, sam, "PUT", &uri, Some(json!({ "backend": "telepathy" })), StatusCode::BAD_REQUEST).await;

    // Taken back to Sam alone: player 1 can't choose it again.
    let owner_only = as_player(&app, sam, "PUT", &uri, Some(json!({ "sharing": "owner" })), ok).await;
    assert_eq!(owner_only["sharing"], "owner", "{owner_only}");
    as_player(&app, 1, "PUT", &format!("/api/ai-accounts/{computer}/use"), None, ok).await;
    as_player(&app, 1, "PUT", &format!("{uri}/use"), None, StatusCode::FORBIDDEN).await;
    assert!(as_player(&app, 1, "GET", "/api/ai-accounts", None, ok).await.as_array().unwrap().iter().all(|a| a["id"] != id));

    // Removed by its owner; Sam is left with the shared one.
    as_player(&app, sam, "DELETE", &uri, None, StatusCode::NO_CONTENT).await;
    as_player(&app, sam, "DELETE", &uri, None, StatusCode::NOT_FOUND).await;
    let left = as_player(&app, sam, "GET", "/api/ai-accounts", None, ok).await;
    assert_eq!(left.as_array().unwrap().iter().map(|a| a["id"].as_i64().unwrap()).collect::<Vec<_>>(), [computer], "{left}");
}

#[tokio::test]
async fn the_library_ai_and_a_new_address_are_kept_from_other_devices() {
    let (app, sam) = household().await;
    let computer = app.db.ai_accounts_for(PlayerId::FIRST).unwrap()[0].id;
    let uri = format!("/api/ai-accounts/{computer}");

    // A token bound to Sam can't choose the library's AI, adding or changing.
    let bound = token(&app, "full", Some(sam)).await;
    let library = json!({ "backend": "openai", "library": true });
    let a = send(&app, LAN, Req { bearer: Some(&bound), body: Some(library), ..req("POST", "/api/ai-accounts") }).await;
    assert_eq!(a.status, StatusCode::FORBIDDEN, "{}", a.body);
    let own =
        send(&app, LAN, Req { bearer: Some(&bound), body: Some(json!({ "backend": "openai" })), ..req("POST", "/api/ai-accounts") }).await;
    assert_eq!(own.status, StatusCode::CREATED, "without the library: {}", own.body);
    let own_uri = format!("/api/ai-accounts/{}", own.body["id"]);
    let a = send(&app, LAN, Req { bearer: Some(&bound), body: Some(json!({ "library": true })), ..req("PUT", &own_uri) }).await;
    assert_eq!(a.status, StatusCode::FORBIDDEN, "{}", a.body);
    assert!(app.db.ai_accounts_for(PlayerId(sam)).unwrap().iter().all(|a| !a.library || a.id == computer));

    // From this computer, its owner moves the library's AI to it and back.
    let moved =
        send(&app, HERE, Req { player: Some(&sam.to_string()), body: Some(json!({ "library": true })), ..req("PUT", &own_uri) }).await;
    assert_eq!((moved.status, &moved.body["library"]), (StatusCode::OK, &json!(true)), "{}", moved.body);
    let back = send(&app, HERE, Req { body: Some(json!({ "library": true })), ..req("PUT", &uri) }).await;
    assert_eq!((back.status, &back.body["library"]), (StatusCode::OK, &json!(true)), "{}", back.body);

    // The OpenAI-compatible key goes with its address: a new address from
    // another device drops the saved key, unless the request sends one.
    let key = json!({ "settings": { "openai_api_key": "sk-local-0000", "openai_base_url": "http://den.lan:11434/v1" } });
    let set = send(&app, HERE, Req { body: Some(key), ..req("PUT", &uri) }).await;
    assert_eq!(set.body["keys_set"], json!(["openai_api_key"]), "{}", set.body);
    let full = token(&app, "full", None).await;
    let moved = json!({ "settings": { "openai_base_url": "http://192.168.0.42:11434/v1" } });
    let a = send(&app, LAN, Req { bearer: Some(&full), body: Some(moved.clone()), ..req("PUT", &uri) }).await;
    assert_eq!((a.status, &a.body["keys_set"]), (StatusCode::OK, &json!([])), "{}", a.body);
    assert_eq!(a.body["settings"]["openai_base_url"], "http://192.168.0.42:11434/v1");
    let both = json!({ "settings": { "openai_api_key": "sk-local-1111", "openai_base_url": "http://den.lan:11434/v1" } });
    let a = send(&app, LAN, Req { bearer: Some(&full), body: Some(both), ..req("PUT", &uri) }).await;
    assert_eq!((a.status, &a.body["keys_set"]), (StatusCode::OK, &json!(["openai_api_key"])), "{}", a.body);
    // This computer is trusted with its own key.
    let here_moved = send(&app, HERE, Req { body: Some(moved), ..req("PUT", &uri) }).await;
    assert_eq!(here_moved.body["keys_set"], json!(["openai_api_key"]), "{}", here_moved.body);
}
