//! Trusted proxies. Forwarded headers are believed only
//! from a listed address; they decide access, never who's playing; and
//! "The proxy does the login" lets a listed proxy's visitors in without a
//! token, to the picker, with PINs as usual.

use super::access::{Ask, HERE, LAN, ask};
use super::*;

/// The proxy: on the network, in front of UpNext.
const PROXY: [u8; 4] = [10, 0, 0, 2];
const PUBLIC: &str = "upnext.den.example.org";

/// Listening beyond loopback at `https://upnext.den.example.org`, with
/// `trusted_proxies` (and the login switch) set on this computer.
async fn behind(list: &str, login: bool) -> TestApp {
    let app = reconfigured(test_app().await, |c| {
        c.host = "0.0.0.0".into();
        c.public_url = Some(format!("https://{PUBLIC}"));
    });
    let (status, v) = app.here("PUT", "/api/settings", Some(json!({ "trusted_proxies": list, "trusted_proxy_login": login }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    app
}

fn through<'a>(method: &'a str, uri: &'a str, headers: &'a [(&'a str, &'a str)]) -> Ask<'a> {
    Ask { headers, ..ask(method, uri, PUBLIC, None) }
}

#[tokio::test]
async fn forwarded_headers_count_only_from_a_listed_proxy() {
    let app = behind("10.0.0.2", false).await;
    let (_, full) = app.make_token("phone", "full").await;
    let from_far = [("X-Forwarded-For", "203.0.113.9"), ("X-Forwarded-Proto", "https")];
    let plain = [("X-Forwarded-For", "203.0.113.9"), ("X-Forwarded-Proto", "http")];

    // Listed, with the switch off: a token as before, and none without.
    let (status, _, v) = app.from(PROXY, through("GET", "/api/games", &from_far)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{v}");
    assert_eq!(app.from(PROXY, Ask { token: Some(&full), ..through("GET", "/api/games", &from_far) }).await.0, StatusCode::OK);
    // The proxy says the client is public and came over plain HTTP: the
    // token isn't taken, as from any public address over plain HTTP.
    let (status, _, v) = app.from(PROXY, Ask { token: Some(&full), ..through("GET", "/api/games", &plain) }).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
    assert!(v["error"].as_str().unwrap().contains("plain HTTP"), "{v}");
    // From an unlisted address the same headers mean nothing: the peer is private.
    assert_eq!(app.from(LAN, Ask { token: Some(&full), ..through("GET", "/api/games", &plain) }).await.0, StatusCode::OK);

    // The host comes from X-Forwarded-Host, from a listed proxy only.
    let rewritten = [("X-Forwarded-For", "192.168.0.42"), ("X-Forwarded-Host", PUBLIC)];
    let to_upstream = |peer| {
        let a = Ask { token: Some(&full), ..ask("GET", "/api/games", "10.0.0.5:8787", None) };
        app.from(peer, Ask { headers: &rewritten, ..a })
    };
    assert_eq!(to_upstream(PROXY).await.0, StatusCode::OK);
    assert_eq!(to_upstream(LAN).await.0, StatusCode::FORBIDDEN, "unlisted: the host is the one it named, not UpNext's");

    // Behind a proxy that sets X-Forwarded-*, a visitor's own Forwarded header
    // that says otherwise is refused, not believed.
    let mixed = [("X-Forwarded-For", "203.0.113.9"), ("X-Forwarded-Proto", "http"), ("Forwarded", "for=192.168.0.42;proto=https")];
    let (status, _, v) = app.from(PROXY, Ask { token: Some(&full), ..through("GET", "/api/games", &mixed) }).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert!(v["error"].as_str().unwrap().contains("say different things"), "{v}");
    // From an address that isn't listed they mean nothing, so they can't disagree.
    assert_eq!(app.from(LAN, Ask { token: Some(&full), ..through("GET", "/api/games", &mixed) }).await.0, StatusCode::OK);
    // An address the proxy can't name is a public client over plain HTTP.
    let unnamed = [("X-Forwarded-For", "unknown"), ("X-Forwarded-Proto", "http")];
    let (status, _, v) = app.from(PROXY, Ask { token: Some(&full), ..through("GET", "/api/games", &unnamed) }).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{v}");

    // Loopback isn't listed, so a proxy on this computer is still another device.
    let (status, _, v) = app.from(HERE, Ask { headers: &from_far, ..ask("GET", "/api/games", "127.0.0.1:8787", None) }).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{v}");
    assert!(v["error"].as_str().unwrap().contains("came through a proxy"), "{v}");
}

#[tokio::test]
async fn the_proxy_does_the_login_with_the_picker_and_pins() {
    let app = behind("10.0.0.2", true).await;
    let headers = [("X-Forwarded-For", "192.168.0.42"), ("X-Forwarded-Proto", "https")];
    let (status, _, v) = app.from(PROXY, through("GET", "/api/games", &headers)).await;
    assert_eq!(status, StatusCode::OK, "{v}");

    // No header from the proxy names a player: it's the default player, as
    // a browser on this computer that hasn't chosen.
    let sam = app.db.add_player("Sam").unwrap();
    let names = [
        ("X-Forwarded-For", "192.168.0.42"),
        ("Remote-User", "Sam"),
        ("X-Forwarded-User", "Sam"),
        ("Tailscale-User-Login", "sam@example.org"),
    ];
    let v = app.from(PROXY, through("GET", "/api/players", &names)).await.2;
    assert_eq!((v["current"].clone(), v["bound"].clone()), (json!(1), json!(false)), "{v}");

    // Nothing only this computer does, and nothing that decides who gets in.
    let origin = format!("https://{PUBLIC}");
    let write = |method, uri, body| Ask { origin: Some(&origin), body: Some(body), ..through(method, uri, &headers) };
    for (method, uri, body) in [
        ("POST", "/api/tokens", json!({ "name": "more", "scope": "full" })),
        ("POST", "/api/pairing", json!({})),
        ("PUT", "/api/settings", json!({ "trusted_proxies": "" })),
        ("PUT", "/api/settings", json!({ "trusted_proxy_login": false })),
        ("PUT", "/api/settings", json!({ "oidc_issuer": "https://sso.den.example.org" })),
        ("POST", "/api/players/1/oidc/link", json!({})),
        ("DELETE", "/api/players/2", json!({})),
    ] {
        let (status, _, v) = app.from(PROXY, write(method, uri, body)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}: {v}");
    }
    assert_eq!(app.from(PROXY, through("GET", "/api/backups/x.db/download", &headers)).await.0, StatusCode::FORBIDDEN);
    assert_eq!(app.from(PROXY, through("GET", "/api/settings", &headers)).await.2["listen"]["this_computer"], json!(false));
    // A write must say where it came from: the proxy's own sign-in cookie rides along on any request.
    let (status, _, _) =
        app.from(PROXY, Ask { body: Some(json!({ "sampled_minutes": 90 })), ..through("PUT", "/api/settings", &headers) }).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(app.from(PROXY, write("PUT", "/api/settings", json!({ "sampled_minutes": 90 }))).await.0, StatusCode::OK);

    // Switching to a player with a PIN takes it, and the browser's cookie remembers.
    app.db.set_pin(sam, Some("2468")).unwrap();
    let (status, _, _) = app.from(PROXY, write("PUT", "/api/players/current", json!({ "player_id": sam.0 }))).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "no PIN");
    let (status, out, v) = app.from(PROXY, write("PUT", "/api/players/current", json!({ "player_id": sam.0, "pin": "2468" }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let cookie = out[header::SET_COOKIE].to_str().unwrap().split(';').next().unwrap().to_string();
    let with_cookie = [("X-Forwarded-For", "192.168.0.42"), ("Cookie", cookie.as_str())];
    assert_eq!(app.from(PROXY, through("GET", "/api/players", &with_cookie)).await.2["current"], json!(sam.0));

    // Another host is DNS rebinding through the proxy's address.
    let (status, _, v) = app.from(PROXY, Ask { headers: &headers, ..ask("GET", "/api/games", "evil.example", None) }).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
    // The same request from an unlisted address needs a token.
    assert_eq!(app.from(LAN, through("GET", "/api/games", &headers)).await.0, StatusCode::UNAUTHORIZED);
    // And one with no forwarding headers is judged as before, from a listed address too.
    assert_eq!(app.from(PROXY, ask("GET", "/api/games", PUBLIC, None)).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_proxy_on_this_computer_is_listed_like_any_other() {
    // `tailscale serve`: a proxy on this computer, listed, doing the login.
    let app = behind("127.0.0.1", true).await;
    let headers = [("X-Forwarded-For", "100.101.102.103"), ("X-Forwarded-Proto", "https")];
    let (status, _, v) = app.from(HERE, through("GET", "/api/games", &headers)).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    // Through it, it's not this computer: no tokens made, no backups downloaded.
    let make = Ask {
        origin: Some("https://upnext.den.example.org"),
        body: Some(json!({ "name": "x", "scope": "full" })),
        ..through("POST", "/api/tokens", &headers)
    };
    assert_eq!(app.from(HERE, make).await.0, StatusCode::FORBIDDEN);
    // This computer itself, with no proxy's headers, is as it always was.
    assert_eq!(app.here("POST", "/api/tokens", Some(json!({ "name": "x", "scope": "full" }))).await.0, StatusCode::CREATED);
    assert_eq!(app.here("GET", "/api/settings", None).await.1["listen"]["this_computer"], json!(true));
}

#[tokio::test]
async fn the_list_is_set_only_on_this_computer_and_checked() {
    let app = reconfigured(test_app().await, |c| c.host = "0.0.0.0".into());
    let (_, full) = app.make_token("phone", "full").await;
    let put = |body: Value| Ask { token: Some(&full), body: Some(body), ..ask("PUT", "/api/settings", "den.lan:8787", None) };
    let (status, _, v) = app.from(LAN, put(json!({ "trusted_proxies": "192.168.0.20" }))).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
    assert!(v["error"].as_str().unwrap().contains("decides who gets in"), "{v}");
    assert_eq!(app.here("PUT", "/api/settings", Some(json!({ "trusted_proxies": "0.0.0.0/0" }))).await.0, StatusCode::BAD_REQUEST);
    let (status, v) = app.here("PUT", "/api/settings", Some(json!({ "trusted_proxies": "10.0.0.2 172.16.0.0/12" }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!((v["trusted_proxies"].clone(), v["trusted_proxies_source"].clone()), (json!("10.0.0.2, 172.16.0.0/12"), json!("app")));
    assert_eq!(v["trusted_proxy_login"], json!(false), "off by default");
}

/// Setup offers this computer's user name as the first
/// player's only to someone at this computer, never through a proxy that
/// does the login, which is in front of another device.
#[tokio::test]
async fn the_login_name_is_offered_only_here_not_through_the_proxy() {
    let app = reconfigured(behind("10.0.0.2", true).await, |c| c.login_name = Some("Kit".into()));
    let here = app.from(HERE, ask("GET", "/api/setup", "127.0.0.1:8787", None)).await.2;
    assert_eq!(here["you"]["suggested"], "Kit", "{here}");
    let headers = [("X-Forwarded-For", "192.168.0.42"), ("X-Forwarded-Proto", "https")];
    let (status, _, v) = app.from(PROXY, through("GET", "/api/setup", &headers)).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["environment"]["client_local"], false, "{v}");
    assert_eq!((v["you"]["placeholder"].as_bool(), v["you"]["suggested"].clone()), (Some(true), Value::Null), "{v}");
}
