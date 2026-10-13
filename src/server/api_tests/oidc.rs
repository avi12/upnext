//! Signing in through an identity provider, the whole
//! flow against a `wiremock` provider with a test signing key: a new
//! account makes a player and pairs the browser, a known one finds its
//! player, a same-named one doesn't take anyone over, an armed link takes
//! the next new account, the rules refuse, and signing out revokes.

use super::access::{Ask, LAN, ask};
use super::*;
use crate::oidc::test_provider::{CLIENT_ID, Key, SECRET, claims, serve};
use std::collections::HashMap;
use wiremock::matchers::{body_string_contains, method, path};

const PUBLIC: &str = "upnext.den.example.org";

struct Idp {
    server: MockServer,
    key: Key,
}

impl Idp {
    async fn start() -> Self {
        let server = MockServer::start().await;
        let key = Key::new("k1");
        serve(&server, &server.uri(), &key).await;
        Self { server, key }
    }

    fn issuer(&self) -> String {
        self.server.uri()
    }

    /// The token endpoint's answer for `code`: an ID token with `claims`.
    async fn answer(&self, code: &str, claims: Value) {
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains(format!("code={code}&")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id_token": self.key.sign(&claims), "token_type": "Bearer" })))
            .mount(&self.server)
            .await;
    }
}

/// UpNext at `https://upnext.den.example.org`, signing in with `idp`.
async fn app_with(idp: &Idp, change: impl FnOnce(&mut Config)) -> TestApp {
    let issuer = idp.issuer();
    reconfigured(test_app().await, |c| {
        c.host = "0.0.0.0".into();
        c.public_url = Some(format!("https://{PUBLIC}"));
        c.oidc_issuer = Some(issuer);
        c.oidc_client_id = Some(CLIENT_ID.into());
        c.oidc_client_secret = Some(SECRET.into());
        c.oidc_name = Some("Authelia".into());
        change(c);
    })
}

/// The value a `Set-Cookie` sets `name` to, when one does.
fn set_cookie(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
    headers.get_all(header::SET_COOKIE).iter().filter_map(|v| v.to_str().ok()).find_map(|v| {
        let (k, rest) = v.split_once('=')?;
        (k == name).then(|| rest.split(';').next().unwrap_or_default().to_string())
    })
}

/// A sign-in started: the provider's page it sends the browser to, its
/// state and nonce, and the browser's cookie.
struct Started {
    state: String,
    nonce: String,
    browser: String,
}

impl TestApp {
    async fn oidc_login(&self) -> Started {
        self.oidc_login_from(LAN).await
    }

    async fn oidc_login_from(&self, peer: [u8; 4]) -> Started {
        let (status, headers, v) = self.from(peer, ask("GET", "/api/v1/oidc/login", PUBLIC, None)).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{v}");
        let to = reqwest::Url::parse(headers[header::LOCATION].to_str().unwrap()).unwrap();
        let q: HashMap<String, String> = to.query_pairs().into_owned().collect();
        assert_eq!(q["redirect_uri"], format!("https://{PUBLIC}/api/v1/oidc/callback"));
        assert_eq!((q["client_id"].as_str(), q["code_challenge_method"].as_str()), (CLIENT_ID, "S256"));
        let browser = set_cookie(&headers, "upnext_oidc_signin").expect("the browser's cookie");
        Started { state: q["state"].clone(), nonce: q["nonce"].clone(), browser }
    }

    /// The provider sends the browser back with `code`: where it goes, and
    /// the device token its cookie holds.
    async fn oidc_back(&self, state: &str, code: &str, browser: Option<&str>) -> (String, Option<String>) {
        let uri = format!("/api/v1/oidc/callback?state={state}&code={code}");
        let cookie = browser.map(|b| format!("upnext_oidc_signin={b}"));
        let headers: Vec<(&str, &str)> = cookie.as_deref().map(|c| vec![("Cookie", c)]).unwrap_or_default();
        let a = Ask { headers: &headers, ..ask("GET", &uri, PUBLIC, None) };
        let (status, out, v) = self.from(LAN, a).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{v}");
        assert_eq!(set_cookie(&out, "upnext_oidc_signin").as_deref(), Some(""), "the browser's cookie is cleared");
        (out[header::LOCATION].to_str().unwrap().to_string(), set_cookie(&out, DEVICE_COOKIE))
    }

    /// A whole sign-in of `sub` with these extra claims.
    async fn sign_in_as(&self, idp: &Idp, sub: &str, extra: Value) -> (String, Option<String>) {
        let s = self.oidc_login().await;
        let mut c = claims(&idp.issuer(), sub, &s.nonce, crate::db::now());
        for (k, v) in extra.as_object().into_iter().flatten() {
            c[k] = v.clone();
        }
        let code = format!("code-{}", &s.state[..12]);
        idp.answer(&code, c).await;
        self.oidc_back(&s.state, &code, Some(&s.browser)).await
    }

    /// The players list as a paired browser with `token` sees it.
    async fn players_as(&self, token: &str) -> Value {
        let cookie = format!("upnext_device={token}");
        let headers = [("Cookie", cookie.as_str())];
        let (status, _, v) = self.from(LAN, Ask { headers: &headers, ..ask("GET", "/api/players", PUBLIC, None) }).await;
        assert_eq!(status, StatusCode::OK, "{v}");
        v
    }
}

/// Why a sign-in sent the browser back, from the fragment.
fn refused(location: &str) -> String {
    let why = location.strip_prefix("/#signin_error=").unwrap_or_else(|| panic!("not refused: {location}"));
    let url = reqwest::Url::parse(&format!("http://x/?e={why}")).unwrap();
    url.query_pairs().next().unwrap().1.into_owned()
}

#[tokio::test]
async fn a_first_sign_in_makes_a_player_and_pairs_the_browser() {
    let idp = Idp::start().await;
    let app = app_with(&idp, |_| {}).await;
    let (status, _, v) = app.from(LAN, ask("GET", "/api/v1/oidc", PUBLIC, None)).await;
    assert_eq!((status, v), (StatusCode::OK, json!({ "enabled": true, "name": "Authelia" })));

    let (to, token) = app.sign_in_as(&idp, "u-sam", json!({ "name": "Sam" })).await;
    assert_eq!(to, "/");
    let token = token.expect("a device token");
    let v = app.players_as(&token).await;
    let sam = v["players"].as_array().unwrap().iter().find(|p| p["name"] == "Sam").cloned().expect("Sam was made");
    assert_eq!((v["current"].clone(), v["bound"].clone()), (sam["id"].clone(), json!(true)), "{v}");
    assert_eq!(sam["oidc"], json!(idp.issuer()));
    let tokens = app.here("GET", "/api/tokens", None).await.1;
    let made = &tokens["tokens"][0];
    assert_eq!((made["kind"].clone(), made["scope"].clone(), made["player_id"].clone()), (json!("oidc"), json!("full"), sam["id"].clone()));
    assert_eq!(made["name"], "Browser (Authelia)");

    // The same account again finds Sam, with a device of its own.
    let (_, again) = app.sign_in_as(&idp, "u-sam", json!({ "name": "Someone else now" })).await;
    assert_eq!(app.players_as(&again.unwrap()).await["current"], sam["id"]);
    // Another account calling itself Sam is a new player, never Sam.
    let (_, other) = app.sign_in_as(&idp, "u-other", json!({ "preferred_username": "Sam" })).await;
    let v = app.players_as(&other.unwrap()).await;
    assert_ne!(v["current"], sam["id"]);
    assert!(v["players"].as_array().unwrap().iter().any(|p| p["name"] == "Sam 2" && p["id"] == v["current"]), "{v}");
    // With no name at all, "Player N".
    let (_, nameless) = app.sign_in_as(&idp, "u-3", json!({})).await;
    let v = app.players_as(&nameless.unwrap()).await;
    assert!(v["players"].as_array().unwrap().iter().any(|p| p["name"] == "Player 4" && p["id"] == v["current"]), "{v}");
    assert_eq!(app.here("GET", "/api/tokens", None).await.1["tokens"].as_array().unwrap().len(), 4);

    // A browser signed in is a bound device: it sees only its own token.
    let (_, mine) = app.sign_in_as(&idp, "u-sam", json!({})).await;
    let cookie = format!("upnext_device={}", mine.unwrap());
    let headers = [("Cookie", cookie.as_str())];
    let list = app.from(LAN, Ask { headers: &headers, ..ask("GET", "/api/tokens", PUBLIC, None) }).await.2;
    assert_eq!(list["tokens"].as_array().unwrap().len(), 1, "{list}");
}

#[tokio::test]
async fn an_armed_link_waits_for_a_confirmation_on_this_computer() {
    let idp = Idp::start().await;
    let app = app_with(&idp, |c| {
        c.trusted_proxies = Some("10.0.0.2".into());
        c.trusted_proxy_login = true;
    })
    .await;
    // Only on this computer, and with the player's PIN.
    let (_, full) = app.make_token("phone", "full").await;
    let from_phone = Ask { token: Some(&full), body: Some(json!({})), ..ask("POST", "/api/players/1/oidc/link", PUBLIC, None) };
    assert_eq!(app.from(LAN, from_phone).await.0, StatusCode::FORBIDDEN);
    app.db.set_pin(PlayerId::FIRST, Some("1357")).unwrap();
    assert_eq!(app.here("POST", "/api/players/1/oidc/link", Some(json!({}))).await.0, StatusCode::FORBIDDEN, "no PIN");
    let (status, v) = app.here("POST", "/api/players/1/oidc/link", Some(json!({ "pin": "1357" }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert!(v["oidc_link_until"].as_i64().unwrap() > crate::db::now(), "{v}");

    // Whoever signs in first isn't linked: they wait, with nothing made and no token.
    let (to, token) = app.sign_in_as(&idp, "u-other", json!({ "name": "Kit", "email": "kit@example.org" })).await;
    assert!(to.starts_with("/#signin_waiting="), "{to}");
    assert!(token.is_none());
    assert_eq!(app.db.players().unwrap().len(), 1, "nobody was made");
    // A newer sign-in replaces it; this computer sees who it is.
    let (to, _) = app.sign_in_as(&idp, "u-first", json!({ "name": "Alex", "preferred_username": "alex" })).await;
    assert!(to.starts_with("/#signin_waiting="), "{to}");
    let v = app.here("GET", "/api/players", None).await.1;
    let waiting = &v["players"][0]["oidc_waiting"];
    assert_eq!((waiting["subject"].clone(), waiting["username"].clone()), (json!("u-first"), json!("alex")), "{v}");
    // Only this computer, which confirms it, is shown who it is: not a
    // device with a token, nor a visitor a proxy let in.
    let v = app.from(LAN, Ask { token: Some(&full), ..ask("GET", "/api/players", PUBLIC, None) }).await.2;
    assert_eq!((v["players"][0]["oidc_waiting"].clone(), v["players"][0]["oidc_link_until"].is_i64()), (Value::Null, true), "{v}");
    let proxied = [("X-Forwarded-For", "192.168.0.42"), ("X-Forwarded-Proto", "https")];
    let (status, _, v) = app.from([10, 0, 0, 2], Ask { headers: &proxied, ..ask("GET", "/api/players", PUBLIC, None) }).await;
    assert_eq!((status, v["players"][0]["oidc_waiting"].clone()), (StatusCode::OK, Value::Null), "{v}");

    // Confirming: on this computer, with the PIN again, naming the account shown.
    let confirm = |body: Value| app.here("POST", "/api/players/1/oidc/confirm", Some(body));
    let from_phone = Ask {
        token: Some(&full),
        body: Some(json!({ "subject": "u-first", "pin": "1357" })),
        ..ask("POST", "/api/players/1/oidc/confirm", PUBLIC, None)
    };
    assert_eq!(app.from(LAN, from_phone).await.0, StatusCode::FORBIDDEN);
    assert_eq!(confirm(json!({ "subject": "u-first" })).await.0, StatusCode::FORBIDDEN, "no PIN");
    assert_eq!(confirm(json!({ "subject": "u-other", "pin": "1357" })).await.0, StatusCode::CONFLICT, "not the one waiting");
    let (status, v) = confirm(json!({ "subject": "u-first", "pin": "1357" })).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!((v["oidc_link_until"].clone(), v["oidc_waiting"].clone()), (Value::Null, Value::Null), "a link works once");
    assert_eq!(confirm(json!({ "subject": "u-first", "pin": "1357" })).await.0, StatusCode::CONFLICT, "once");

    // Signing in again, the account is player 1; the one replaced is a player of its own.
    let (_, token) = app.sign_in_as(&idp, "u-first", json!({})).await;
    assert_eq!(app.players_as(&token.unwrap()).await["current"], json!(1));
    let (_, token) = app.sign_in_as(&idp, "u-other", json!({ "name": "Kit" })).await;
    assert_eq!(app.players_as(&token.unwrap()).await["current"], json!(2));

    // Linking another account to player 1 signs out the browsers the old one
    // signed in, and only those: a device paired with a code for them stays.
    let (_, old_browser) = app.sign_in_as(&idp, "u-first", json!({})).await;
    let old_browser = old_browser.unwrap();
    let paired = json!({ "name": "Alex's phone", "scope": "full", "player_id": 1, "pin": "1357" });
    let (status, v) = app.here("POST", "/api/tokens", Some(paired)).await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    let phone = v["token"].as_str().unwrap().to_string();
    assert_eq!(app.here("POST", "/api/players/1/oidc/link", Some(json!({ "pin": "1357" }))).await.0, StatusCode::OK);
    app.sign_in_as(&idp, "u-new", json!({ "name": "Alex" })).await;
    let (status, v) = confirm(json!({ "subject": "u-new", "pin": "1357" })).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let cookie = format!("upnext_device={old_browser}");
    let headers = [("Cookie", cookie.as_str())];
    assert_eq!(app.from(LAN, Ask { headers: &headers, ..ask("GET", "/api/players", PUBLIC, None) }).await.0, StatusCode::UNAUTHORIZED);
    let (status, _, v) = app.from(LAN, Ask { token: Some(&phone), ..ask("GET", "/api/players", PUBLIC, None) }).await;
    assert_eq!((status, v["current"].clone()), (StatusCode::OK, json!(1)), "{v}");
    let (_, token) = app.sign_in_as(&idp, "u-new", json!({})).await;
    assert_eq!(app.players_as(&token.unwrap()).await["current"], json!(1));
    let (_, token) = app.sign_in_as(&idp, "u-first", json!({ "name": "Alex" })).await;
    assert_ne!(app.players_as(&token.unwrap()).await["current"], json!(1), "the old account is nobody's now");

    // Ending a link drops the account waiting; a link ten minutes old has gone.
    assert_eq!(app.here("POST", "/api/players/2/oidc/link", Some(json!({}))).await.0, StatusCode::OK);
    app.sign_in_as(&idp, "u-third", json!({})).await;
    let (status, v) = app.here("DELETE", "/api/players/2/oidc/link", None).await;
    assert_eq!((status, v["oidc_link_until"].clone(), v["oidc_waiting"].clone()), (StatusCode::OK, Value::Null, Value::Null));
    assert_eq!(app.here("POST", "/api/players/2/oidc/link", Some(json!({}))).await.0, StatusCode::OK);
    app.state.remote.oidc.expire();
    let (_, token) = app.sign_in_as(&idp, "u-third", json!({})).await;
    assert_eq!(app.players_as(&token.unwrap()).await["current"], json!(4));

    // Unlinking player 1 signs out the browsers that account signed in, and
    // the account is a new player's next time.
    let (status, v) = app.here("DELETE", "/api/players/1/oidc", None).await;
    assert_eq!((status, v["oidc"].clone()), (StatusCode::OK, Value::Null));
    let left: Vec<Value> = app.here("GET", "/api/tokens", None).await.1["tokens"].as_array().unwrap().clone();
    assert!(left.iter().all(|t| t["player_id"] != json!(1) || t["kind"] != "oidc"), "{left:?}");
}

#[tokio::test]
async fn confirming_needs_the_armed_player_and_locks_after_wrong_pins() {
    let idp = Idp::start().await;
    let app = app_with(&idp, |_| {}).await;
    app.db.set_pin(PlayerId::FIRST, Some("1357")).unwrap();
    let (status, kit) = app.here("POST", "/api/players", Some(json!({ "name": "Kit" }))).await;
    assert_eq!(status, StatusCode::CREATED, "{kit}");
    assert_eq!(app.here("POST", "/api/players/1/oidc/link", Some(json!({ "pin": "1357" }))).await.0, StatusCode::OK);
    app.sign_in_as(&idp, "u-alex", json!({ "name": "Alex" })).await;

    // The account waits for player 1, so it can't be confirmed as anyone else.
    let uri = format!("/api/players/{}/oidc/confirm", kit["id"]);
    assert_eq!(app.here("POST", &uri, Some(json!({ "subject": "u-alex" }))).await.0, StatusCode::CONFLICT);

    // Five wrong PINs lock player 1 (the fifth says so), and then the right
    // one is refused too.
    let confirm = |pin: &str| app.here("POST", "/api/players/1/oidc/confirm", Some(json!({ "subject": "u-alex", "pin": pin })));
    for n in 1..=5 {
        let (status, v) = confirm("0000").await;
        let locked = if n < 5 { StatusCode::FORBIDDEN } else { StatusCode::TOO_MANY_REQUESTS };
        assert_eq!(status, locked, "wrong PIN {n}: {v}");
    }
    let (status, v) = confirm("1357").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{v}");
    // Nothing was linked, and the account still waits.
    let v = app.here("GET", "/api/players", None).await.1;
    assert_eq!(
        (v["players"][0]["oidc"].clone(), v["players"][0]["oidc_waiting"]["subject"].clone()),
        (Value::Null, json!("u-alex")),
        "{v}"
    );
}

#[tokio::test]
async fn the_rules_refuse_and_say_why() {
    let idp = Idp::start().await;
    // Auto-provisioning off: a player must be linked first.
    let app = app_with(&idp, |c| c.oidc_auto_provision = false).await;
    let (to, token) = app.sign_in_as(&idp, "u-new", json!({ "name": "Alex" })).await;
    assert!(refused(&to).contains("isn't linked to a player here yet"), "{to}");
    assert!(token.is_none());
    assert_eq!(app.db.players().unwrap().len(), 1, "nobody was made");

    // The groups rule.
    let app = app_with(&idp, |c| c.oidc_allowed_groups = Some("games,family".into())).await;
    let (to, _) = app.sign_in_as(&idp, "u-new", json!({ "groups": ["staff"] })).await;
    assert!(refused(&to).contains("isn't in a group allowed"), "{to}");
    let (to, token) = app.sign_in_as(&idp, "u-new", json!({ "groups": ["staff", "family"] })).await;
    assert_eq!((to.as_str(), token.is_some()), ("/", true));

    // A bad state, another browser, a replay, a wrong nonce and a bad signature.
    let app = app_with(&idp, |_| {}).await;
    let s = app.oidc_login().await;
    assert!(refused(&app.oidc_back("made-up", "c", Some(&s.browser)).await.0).contains("expired or didn't start here"));
    let s = app.oidc_login().await;
    let (to, _) = app.oidc_back(&s.state, "c", Some("another-browser")).await;
    assert!(refused(&to).contains("another browser"), "{to}");
    assert!(refused(&app.oidc_back(&s.state, "c", Some(&s.browser)).await.0).contains("expired"), "a state works once");
    let s = app.oidc_login().await;
    idp.answer("code-nonce", claims(&idp.issuer(), "u-x", "another-nonce", crate::db::now())).await;
    let (to, token) = app.oidc_back(&s.state, "code-nonce", Some(&s.browser)).await;
    assert!(refused(&to).contains("isn't for this sign-in"), "{to}");
    assert!(token.is_none());
    let s = app.oidc_login().await;
    let forged = Key::new("k1").sign(&claims(&idp.issuer(), "u-x", &s.nonce, crate::db::now()));
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("code=code-forged&"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id_token": forged })))
        .mount(&idp.server)
        .await;
    let (to, _) = app.oidc_back(&s.state, "code-forged", Some(&s.browser)).await;
    assert!(refused(&to).contains("signature"), "{to}");
    // A sign-in that waited ten minutes.
    let s = app.oidc_login().await;
    app.state.remote.oidc.expire();
    assert!(refused(&app.oidc_back(&s.state, "c", Some(&s.browser)).await.0).contains("expired"));
    // The provider said no.
    let s = app.oidc_login().await;
    let uri = format!("/api/v1/oidc/callback?state={}&error=access_denied&error_description=User%20cancelled", s.state);
    let cookie = format!("upnext_oidc_signin={}", s.browser);
    let headers = [("Cookie", cookie.as_str())];
    let (_, out, _) = app.from(LAN, Ask { headers: &headers, ..ask("GET", &uri, PUBLIC, None) }).await;
    assert!(refused(out[header::LOCATION].to_str().unwrap()).contains("User cancelled"));
    assert_eq!(app.db.players().unwrap().len(), 1);
}

#[tokio::test]
async fn off_until_set_up_and_never_over_plain_http_from_afar() {
    let idp = Idp::start().await;
    let app = app_with(&idp, |c| c.public_url = None).await;
    let (status, _, v) = app.from(LAN, ask("GET", "/api/v1/oidc", "den.lan:8787", None)).await;
    assert_eq!((status, v), (StatusCode::OK, json!({ "enabled": false, "name": null })), "needs public_url");
    let (status, out, _) = app.from(LAN, ask("GET", "/api/v1/oidc/login", "den.lan:8787", None)).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(refused(out[header::LOCATION].to_str().unwrap()).contains("public_url"));

    let plain = reconfigured(test_app().await, |c| c.host = "0.0.0.0".into());
    assert_eq!(plain.from(LAN, ask("GET", "/api/v1/oidc", "den.lan:8787", None)).await.2["enabled"], json!(false), "off by default");

    // From a public address over plain HTTP, the sign-in hands out no token.
    let app = app_with(&idp, |_| {}).await;
    assert_eq!(app.from([203, 0, 113, 9], ask("GET", "/api/v1/oidc/login", PUBLIC, None)).await.0, StatusCode::FORBIDDEN);
    assert_eq!(app.from([203, 0, 113, 9], ask("GET", "/api/v1/oidc/callback?state=x&code=y", PUBLIC, None)).await.0, StatusCode::FORBIDDEN);
    // The settings are this computer's.
    let (_, full) = app.make_token("phone", "full").await;
    let put = Ask { token: Some(&full), body: Some(json!({ "oidc_auto_provision": false })), ..ask("PUT", "/api/settings", PUBLIC, None) };
    assert_eq!(app.from(LAN, put).await.0, StatusCode::FORBIDDEN);
    let v = app.here("GET", "/api/settings", None).await.1;
    assert_eq!(v["oidc_redirect_uri"], json!(format!("https://{PUBLIC}/api/v1/oidc/callback")));
    assert_eq!((v["oidc_client_secret_set"].clone(), v["oidc_auto_provision_source"].clone()), (json!(true), json!("default")));
    assert!(!v.to_string().contains(SECRET), "the secret never comes back");

    // This computer sets them all, the secret write-only.
    let all = json!({
        "oidc_issuer": "https://sso.den.example.org",
        "oidc_client_id": "upnext-den",
        "oidc_client_secret": "another-secret",
        "oidc_name": "Den sign-in",
        "oidc_auto_provision": false,
        "oidc_allowed_groups": "players, family",
    });
    let (status, v) = app.here("PUT", "/api/settings", Some(all)).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["oidc_client_secret_source"], json!("app"));
    assert!(!v.to_string().contains("another-secret"), "{v}");
    assert_eq!((v["oidc_name"].clone(), v["oidc_auto_provision"].clone()), (json!("Den sign-in"), json!(false)));
    assert_eq!(app.from(LAN, ask("GET", "/api/v1/oidc", PUBLIC, None)).await.2["name"], json!("Den sign-in"));
}

#[tokio::test]
async fn signing_out_revokes_the_browser() {
    let idp = Idp::start().await;
    let app = app_with(&idp, |_| {}).await;
    let (_, token) = app.sign_in_as(&idp, "u-sam", json!({ "name": "Sam" })).await;
    let cookie = format!("upnext_device={}", token.unwrap());
    let headers = [("Cookie", cookie.as_str())];
    let origin = format!("https://{PUBLIC}");
    let out = Ask { headers: &headers, origin: Some(&origin), ..ask("POST", "/api/v1/oidc/logout", PUBLIC, None) };
    let (status, out_headers, v) = app.from(LAN, out).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{v}");
    assert_eq!(set_cookie(&out_headers, DEVICE_COOKIE).as_deref(), Some(""));
    assert_eq!(set_cookie(&out_headers, "upnext_device").as_deref(), Some(""), "and the old name's");
    let (status, _, _) = app.from(LAN, Ask { headers: &headers, ..ask("GET", "/api/games", PUBLIC, None) }).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "revoked");
    // A browser paired with a code isn't signed out this way.
    let (_, full) = app.make_token("phone", "full").await;
    let out = Ask { token: Some(&full), origin: Some(&origin), ..ask("POST", "/api/v1/oidc/logout", PUBLIC, None) };
    assert_eq!(app.from(LAN, out).await.0, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn one_device_cant_push_out_anothers_sign_in() {
    let idp = Idp::start().await;
    let app = app_with(&idp, |_| {}).await;
    let theirs = app.oidc_login_from([192, 168, 0, 43]).await;
    // One address starting sign-in after sign-in pushes out only its own oldest.
    let mut mine = vec![];
    for _ in 0..40 {
        mine.push(app.oidc_login().await);
    }
    let code = |s: &Started| format!("code-{}", &s.state[..12]);
    idp.answer(&code(&theirs), claims(&idp.issuer(), "u-theirs", &theirs.nonce, crate::db::now())).await;
    assert!(back(&app, &theirs).await.1.is_some(), "the other device's sign-in still works");
    let last = mine.last().unwrap();
    idp.answer(&code(last), claims(&idp.issuer(), "u-mine", &last.nonce, crate::db::now())).await;
    assert!(back(&app, last).await.1.is_some());
    assert!(refused(&back(&app, &mine[0]).await.0).contains("expired or didn't start here"), "its own oldest went");

    // With 32 waiting (two of mine and 30 from other addresses), a new one
    // is refused, and none is pushed out.
    let mut many = vec![];
    for n in 0..30u8 {
        many.push(app.oidc_login_from([192, 168, 1, n]).await);
    }
    let (status, out, _) = app.from([192, 168, 2, 1], ask("GET", "/api/v1/oidc/login", PUBLIC, None)).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(refused(out[header::LOCATION].to_str().unwrap()).contains("too many sign-ins"));
    let first = &many[0];
    idp.answer(&code(first), claims(&idp.issuer(), "u-many", &first.nonce, crate::db::now())).await;
    assert!(back(&app, first).await.1.is_some(), "still there");
}

/// The provider sends `s`'s browser back with its code.
async fn back(app: &TestApp, s: &Started) -> (String, Option<String>) {
    app.oidc_back(&s.state, &format!("code-{}", &s.state[..12]), Some(&s.browser)).await
}
