//! This computer needs no token; other devices, scripts and a
//! proxy in front do, and `public_url` widens the hosts and origins.

use super::*;
use crate::db::PlayerId;

/// A request as another computer (or this one) sends it: from `peer`,
/// addressed to `host`, with a token or not.
pub(super) struct Ask<'a> {
    pub method: &'a str,
    pub uri: &'a str,
    pub host: &'a str,
    pub token: Option<&'a str>,
    pub origin: Option<&'a str>,
    pub body: Option<Value>,
    /// Any other headers, such as a proxy's `X-Forwarded-For`.
    pub headers: &'a [(&'a str, &'a str)],
}

pub(super) fn ask<'a>(method: &'a str, uri: &'a str, host: &'a str, token: Option<&'a str>) -> Ask<'a> {
    Ask { method, uri, host, token, origin: None, body: None, headers: &[] }
}

impl TestApp {
    pub(super) async fn from(&self, peer: [u8; 4], a: Ask<'_>) -> (StatusCode, axum::http::HeaderMap, Value) {
        let mut req = Request::builder().method(a.method).uri(a.uri).header(header::HOST, a.host);
        if let Some(t) = a.token {
            req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        if let Some(o) = a.origin {
            req = req.header(header::ORIGIN, o);
        }
        for (name, value) in a.headers {
            req = req.header(*name, *value);
        }
        let body = match a.body {
            Some(v) => {
                req = req.header(header::CONTENT_TYPE, "application/json");
                Body::from(v.to_string())
            }
            None => Body::empty(),
        };
        let resp = self
            .router
            .clone()
            .layer(axum::extract::connect_info::MockConnectInfo(std::net::SocketAddr::from((peer, 50000))))
            .oneshot(req.body(body).unwrap())
            .await
            .unwrap();
        let (status, headers) = (resp.status(), resp.headers().clone());
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let v = serde_json::from_slice(&bytes).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        (status, headers, v)
    }

    /// From this computer, which needs no token: the peer address is
    /// loopback, whatever the server listens on.
    pub(super) async fn here(&self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let (status, _, v) = self.from(HERE, Ask { body, ..ask(method, uri, "127.0.0.1:8787", None) }).await;
        (status, v)
    }

    pub(super) async fn make_token(&self, name: &str, scope: &str) -> (i64, String) {
        let (status, v) = self.here("POST", "/api/tokens", Some(json!({ "name": name, "scope": scope }))).await;
        assert_eq!(status, StatusCode::CREATED, "{v}");
        (v["id"].as_i64().unwrap(), v["token"].as_str().unwrap().to_string())
    }
}

pub(super) const HERE: [u8; 4] = [127, 0, 0, 1];
pub(super) const LAN: [u8; 4] = [192, 168, 0, 20];

#[tokio::test]
async fn other_devices_need_a_token() {
    let app = reconfigured(test_app().await, |c| c.host = "0.0.0.0".into());

    // This computer, as always.
    assert_eq!(app.from(HERE, ask("GET", "/api/games", "127.0.0.1:8787", None)).await.0, StatusCode::OK);
    assert_eq!(app.from(HERE, ask("GET", "/", "localhost:8787", None)).await.0, StatusCode::OK, "the UI");
    // Another device, without a token: not the API. The UI's files, yes:
    // they hold nothing private, and a new device pairs on them.
    let (status, headers, v) = app.from(LAN, ask("GET", "/api/games", "den.lan:8787", None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{v}");
    assert!(v["error"].as_str().unwrap().contains("Bearer upnext_"), "{v}");
    assert_eq!(headers[header::WWW_AUTHENTICATE], "Bearer realm=\"UpNext\"");
    assert_eq!(app.from(LAN, ask("GET", "/api/games", "192.168.0.5:8787", None)).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(app.from(LAN, ask("GET", "/", "192.168.0.5:8787", None)).await.0, StatusCode::OK);
    // This computer through another name (a proxy, its own LAN address): a token too.
    assert_ne!(app.from(HERE, ask("GET", "/api/games", "den.lan:8787", None)).await.0, StatusCode::OK);

    let (read_id, read) = app.make_token("dashboard", "read").await;
    let (_, full) = app.make_token("phone", "full").await;
    assert!(read.starts_with("upnext_") && full.starts_with("upnext_"));
    let (status, v) = app.here("POST", "/api/tokens", Some(json!({ "name": "Phone", "scope": "full" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "names are used once: {v}");
    assert_eq!(app.here("POST", "/api/tokens", Some(json!({ "name": "x", "scope": "admin" }))).await.0, StatusCode::BAD_REQUEST);
    let list = app.here("GET", "/api/tokens", None).await.1;
    assert_eq!(list["tokens"].as_array().unwrap().len(), 2);
    assert_eq!(list["current"], Value::Null, "this computer has no token");
    assert_eq!(list["tokens"][0]["scope"], "read");
    assert!(!list.to_string().contains(&read) && !list.to_string().contains(&full), "a list never has the tokens");

    // A read token reads, and only reads.
    for uri in ["/api/games", "/api/stats", "/api/picks/history", "/api/collections", "/"] {
        assert_eq!(app.from(LAN, ask("GET", uri, "den.lan:8787", Some(&read))).await.0, StatusCode::OK, "{uri}");
    }
    for uri in ["/api/export", "/api/tokens"] {
        assert_eq!(app.from(LAN, ask("GET", uri, "den.lan:8787", Some(&read))).await.0, StatusCode::FORBIDDEN, "{uri}");
    }
    let pick = || Ask { body: Some(json!({ "prompt": "something short", "count": 1 })), ..ask("POST", "/api/picks", "den.lan:8787", None) };
    let (status, _, v) = app.from(LAN, Ask { token: Some(&read), ..pick() }).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(v["error"].as_str().unwrap().contains("can only read"), "{v}");

    // A full token does what the UI does.
    let (status, _, batch) = app.from(LAN, Ask { token: Some(&full), ..pick() }).await;
    assert_eq!(status, StatusCode::OK, "{batch}");
    assert_eq!(batch["picks"].as_array().unwrap().len(), 1);
    let app = &app;
    let put = |body: Value, token: &str| {
        let token = token.to_string();
        async move {
            let a = Ask { token: Some(&token), body: Some(body), ..ask("PUT", "/api/settings", "den.lan:8787", None) };
            app.from(LAN, a).await.0
        }
    };
    assert_eq!(put(json!({ "sampled_minutes": 90 }), &full).await, StatusCode::OK);
    assert_eq!(put(json!({ "sampled_minutes": 60 }), &read).await, StatusCode::FORBIDDEN);
    assert_eq!(put(json!({ "galaxy_db": "/tmp/galaxy-2.0.db" }), &full).await, StatusCode::FORBIDDEN, "a file on the server");
    assert_eq!(app.here("PUT", "/api/settings", Some(json!({ "galaxy_db": "" }))).await.0, StatusCode::OK, "this computer can");
    let verdicts = json!({ "verdicts_url": "https://verdicts.example.org" });
    assert_eq!(put(verdicts.clone(), &full).await, StatusCode::FORBIDDEN, "where the verdicts key goes");
    assert_eq!(app.here("PUT", "/api/settings", Some(verdicts)).await.0, StatusCode::OK, "this computer can");
    assert_eq!(app.from(LAN, ask("POST", "/api/gog/window", "den.lan:8787", Some(&full))).await.0, StatusCode::FORBIDDEN);
    let mine = app.from(LAN, ask("GET", "/api/tokens", "den.lan:8787", Some(&full))).await.2;
    assert_eq!(mine["current"], mine["tokens"][1]["id"], "the list says which token asked");
    assert!(mine["tokens"][1]["last_used_at"].is_i64(), "{mine}");

    // The export has no tokens, not even their hashes.
    let resp = app
        .router
        .clone()
        .layer(axum::extract::connect_info::MockConnectInfo(std::net::SocketAddr::from((LAN, 50000))))
        .oneshot(
            Request::get("/api/export")
                .header(header::HOST, "den.lan:8787")
                .header(header::AUTHORIZATION, format!("Bearer {full}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let export = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let export = String::from_utf8_lossy(&export);
    for t in [&read, &full] {
        assert!(!export.contains(t.as_str()) && !export.contains(&crate::db::token_hash(t)), "the export leaves tokens out");
    }

    // A token that isn't one is refused, even here; a revoked one at once.
    assert_eq!(app.from(HERE, ask("GET", "/api/games", "127.0.0.1:8787", Some("upnext_nope"))).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(app.here("DELETE", &format!("/api/tokens/{read_id}"), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(app.here("DELETE", &format!("/api/tokens/{read_id}"), None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(app.from(LAN, ask("GET", "/api/games", "den.lan:8787", Some(&read))).await.0, StatusCode::UNAUTHORIZED);
    // Backups stay this computer's, whatever the token.
    assert_eq!(app.from(LAN, ask("DELETE", "/api/backups/x.db", "den.lan:8787", Some(&full))).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn public_url_widens_hosts_and_origins() {
    // Behind a proxy on this computer: the server listens on loopback and
    // every request comes from 127.0.0.1, addressed to the public name.
    let steam = fake_steam(true, 0).await;
    let app = test_app_with(None, Some(&steam.uri())).await;
    let (_, full) = app.make_token("phone", "full").await;
    let public = "upnext.example.org";
    assert_eq!(app.from(HERE, ask("GET", "/api/games", public, Some(&full))).await.0, StatusCode::FORBIDDEN, "not ours until set");

    let (status, v) = app.call("PUT", "/api/settings", Some(json!({ "public_url": "HTTPS://UpNext.Example.org/" }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!((v["public_url"].clone(), v["public_url_source"].clone()), (json!("https://upnext.example.org"), json!("app")));
    assert_eq!(v["listen"]["other_devices"], false);
    assert_eq!(app.call("PUT", "/api/settings", Some(json!({ "public_url": "upnext.example.org/x" }))).await.0, StatusCode::BAD_REQUEST);

    assert_eq!(app.from(HERE, ask("GET", "/api/games", public, Some(&full))).await.0, StatusCode::OK);
    assert_eq!(app.from(HERE, ask("GET", "/api/games", "upnext.example.org:443", Some(&full))).await.0, StatusCode::OK);
    assert_eq!(app.from(HERE, ask("GET", "/api/games", public, None)).await.0, StatusCode::UNAUTHORIZED, "a proxy's requests need a token");
    // Rebinding is still refused, token or not.
    assert_eq!(app.from(HERE, ask("GET", "/api/games", "evil.example", Some(&full))).await.0, StatusCode::FORBIDDEN);
    assert_eq!(app.from(HERE, ask("GET", "/api/games", "evil.example", None)).await.0, StatusCode::FORBIDDEN);

    // Writes from the public origin get through; another site's don't.
    let merge = |origin: &'static str| Ask {
        origin: Some(origin),
        body: Some(json!({ "ids": [1] })),
        ..ask("POST", "/api/titles/merge", public, None)
    };
    assert_eq!(
        app.from(HERE, Ask { token: Some(&full), ..merge("https://upnext.example.org") }).await.0,
        StatusCode::BAD_REQUEST,
        "reaches the handler"
    );
    assert_eq!(app.from(HERE, Ask { token: Some(&full), ..merge("https://evil.example") }).await.0, StatusCode::FORBIDDEN);
    assert_eq!(
        app.from(HERE, Ask { token: Some(&full), ..merge("http://upnext.example.org") }).await.0,
        StatusCode::FORBIDDEN,
        "not its scheme"
    );

    // Steam sends the browser back to the address it was reached at.
    let location = |headers: axum::http::HeaderMap| headers[header::LOCATION].to_str().unwrap().to_string();
    let (status, headers, _) = app.from(HERE, ask("GET", "/api/steam/login", public, Some(&full))).await;
    assert!(status.is_redirection());
    assert!(headers[header::SET_COOKIE].to_str().unwrap().ends_with("; Secure"), "the sign-in's cookie, over HTTPS");
    assert!(location(headers).contains(&urlenc("https://upnext.example.org/api/steam/callback?state=")), "public_url's scheme");
    let to = app.redirect("/api/steam/login").await;
    assert!(to.contains(&urlenc("http://127.0.0.1:8787/api/steam/callback?state=")), "{to}");

    // Listening on the network with no public_url: a token holder may use
    // any name; without a token, nothing but this computer's.
    app.call("PUT", "/api/settings", Some(json!({ "public_url": null }))).await;
    let app = reconfigured(app, |c| c.host = "0.0.0.0".into());
    assert_eq!(app.from(LAN, ask("GET", "/api/games", "192.168.0.5:8787", Some(&full))).await.0, StatusCode::OK);
    assert_eq!(app.from(LAN, ask("GET", "/api/games", "den.tailnet.ts.net", Some(&full))).await.0, StatusCode::OK);
    assert_eq!(app.from(HERE, ask("GET", "/api/games", "evil.example", None)).await.0, StatusCode::FORBIDDEN);
}

/// Behind a proxy on this computer (`tailscale serve`, Caddy) every request
/// comes from loopback, a phone's token included. Backups still answer only
/// this computer: a backup with keys holds every secret and the token list.
#[tokio::test]
async fn backups_answer_only_this_computer_behind_a_proxy() {
    let app = test_app().await;
    app.call("PUT", "/api/settings", Some(json!({ "public_url": "https://upnext.example.org" }))).await;
    let (status, made) = app.call("POST", "/api/backups", None).await;
    assert_eq!(status, StatusCode::CREATED, "{made}");
    let name = made["name"].as_str().unwrap().to_string();
    let (_, read) = app.make_token("dashboard", "read").await;
    let (_, full) = app.make_token("phone", "full").await;

    let download = format!("/api/backups/{name}/download?keys=true");
    let restore = format!("/api/backups/{name}/restore");
    let delete = format!("/api/backups/{name}");
    let local_only = [("GET", download.as_str()), ("POST", restore.as_str()), ("POST", "/api/backups/import"), ("DELETE", delete.as_str())];
    for token in [&read, &full] {
        for (method, uri) in local_only {
            let a = Ask { origin: Some("https://upnext.example.org"), ..ask(method, uri, "upnext.example.org", Some(token)) };
            let (status, _, v) = app.from(HERE, a).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}: {v}");
            assert!(v["error"].as_str().unwrap().contains("only on the computer UpNext runs on"), "{v}");
        }
    }
    // The list isn't secret, and backing up now changes nothing that was there.
    assert_eq!(app.from(HERE, ask("GET", "/api/backups", "upnext.example.org", Some(&read))).await.0, StatusCode::OK);
    let now = Ask { origin: Some("https://upnext.example.org"), ..ask("POST", "/api/backups", "upnext.example.org", Some(&full)) };
    assert_eq!(app.from(HERE, now).await.0, StatusCode::CREATED);
    // This computer, addressed by a loopback name, still can.
    let (status, _, _) = app.from(HERE, ask("GET", &download, "127.0.0.1:8787", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(app.backups_dir().join(&name).is_file(), "nothing was deleted");
}

/// Security review L5: a job's options can't do from another device what
/// the settings and backups refuse there: name a file for a sync to open,
/// or keep fewer backups so the rest are removed.
#[tokio::test]
async fn a_jobs_file_path_and_backups_kept_are_this_computers() {
    let app = test_app().await;
    app.call("PUT", "/api/settings", Some(json!({ "public_url": "https://upnext.example.org" }))).await;
    let (_, full) = app.make_token("phone", "full").await;
    let remote = |method, uri, body: Value| Ask {
        origin: Some("https://upnext.example.org"),
        body: Some(body),
        ..ask(method, uri, "upnext.example.org", Some(full.as_str()))
    };
    let refused = [
        ("POST", "/api/jobs", json!({ "kind": "sync", "options": { "galaxy_db": "/etc/passwd" } })),
        ("POST", "/api/jobs", json!({ "kind": "backup", "options": { "keep": 1 } })),
        ("PUT", "/api/jobs/scheduled/backup", json!({ "keep": 1 })),
    ];
    for (method, uri, body) in refused {
        let (status, _, v) = app.from(HERE, remote(method, uri, body.clone())).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri} {body}: {v}");
        assert!(v["error"].as_str().unwrap().contains("only on the computer UpNext runs on"), "{v}");
    }
    assert_eq!(app.here("GET", "/api/jobs", None).await.1["current"], Value::Null, "nothing started");
    let backup = |v: &Value| v["tasks"].as_array().unwrap().iter().find(|t| t["task"] == "backup").unwrap().clone();
    let kept = backup(&app.here("GET", "/api/jobs/scheduled", None).await.1)["keep"].clone();
    assert_ne!(kept, 1, "the count is unchanged");

    // The rest of a schedule is the phone's to change, and this computer
    // still sets the count.
    let (status, _, v) = app.from(HERE, remote("PUT", "/api/jobs/scheduled/backup", json!({ "schedule": "daily" }))).await;
    assert_eq!((status, backup(&v)["schedule"].as_str()), (StatusCode::OK, Some("daily")), "{v}");
    let (status, v) = app.here("PUT", "/api/jobs/scheduled/backup", Some(json!({ "keep": 3 }))).await;
    assert_eq!((status, backup(&v)["keep"].as_u64()), (StatusCode::OK, Some(3)), "{v}");
    // A backup without a count of its own still starts from the phone.
    let (status, _, v) = app.from(HERE, remote("POST", "/api/jobs", json!({ "kind": "backup" }))).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{v}");
}

/// A proxy on this computer that rewrites `Host` to its upstream's address
/// (nginx's default) sends what this computer sends, plus its own headers.
/// With any of them the request needs a token, so the proxy's visitors
/// can't make tokens or download backups.
#[tokio::test]
async fn a_proxy_that_rewrites_host_needs_a_token() {
    let app = test_app().await;
    let (_, full) = app.make_token("phone", "full").await;
    for header in ["X-Forwarded-For", "Forwarded", "X-Forwarded-Host", "X-Forwarded-Proto", "X-Real-IP", "Via"] {
        let headers = [(header, "203.0.113.9")];
        let proxied = |method, uri, token| Ask { headers: &headers, ..ask(method, uri, "127.0.0.1:8787", token) };
        let (status, headers_out, v) = app.from(HERE, proxied("GET", "/api/games", None)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{header}: {v}");
        assert!(v["error"].as_str().unwrap().contains("came through a proxy"), "{v}");
        assert_eq!(headers_out[header::WWW_AUTHENTICATE], "Bearer realm=\"UpNext\"");
        let make = Ask { body: Some(json!({ "name": format!("via {header}"), "scope": "full" })), ..proxied("POST", "/api/tokens", None) };
        assert_eq!(app.from(HERE, make).await.0, StatusCode::UNAUTHORIZED, "{header}");
        assert_eq!(app.from(HERE, proxied("GET", "/api/backups", None)).await.0, StatusCode::UNAUTHORIZED, "{header}");

        // With a token it's another device's request, as through any proxy.
        assert_eq!(app.from(HERE, proxied("GET", "/api/games", Some(&full))).await.0, StatusCode::OK, "{header}");
        let make = Ask { body: Some(json!({ "name": "more", "scope": "full" })), ..proxied("POST", "/api/tokens", Some(&full)) };
        assert_eq!(app.from(HERE, make).await.0, StatusCode::FORBIDDEN, "{header}");
        // What needs no token still needs none.
        assert_eq!(app.from(HERE, proxied("GET", "/api/health", None)).await.0, StatusCode::OK, "{header}");
        assert_eq!(app.from(HERE, proxied("GET", "/", None)).await.0, StatusCode::OK, "{header}");
    }
    let tokens = app.here("GET", "/api/tokens", None).await.1;
    assert_eq!(tokens["tokens"].as_array().unwrap().len(), 1, "only the test's own: {tokens}");

    // This computer without a proxy is as it was, and settings say so.
    let (status, v) = app.here("GET", "/api/settings", None).await;
    assert_eq!((status, v["listen"]["this_computer"].clone()), (StatusCode::OK, json!(true)));
    let headers = [("X-Forwarded-For", "203.0.113.9")];
    let a = Ask { headers: &headers, ..ask("GET", "/api/settings", "localhost:8787", Some(&full)) };
    assert_eq!(app.from(HERE, a).await.2["listen"]["this_computer"], json!(false));
}

/// The OpenAI-compatible key goes to whatever address is set. From another
/// device, a new address clears the saved key unless the same request sends
/// one, so a lost phone can't point the key at a server of its own.
#[tokio::test]
async fn a_new_ai_server_address_from_another_device_clears_the_key() {
    let app = reconfigured(test_app().await, |c| c.host = "0.0.0.0".into());
    let (_, full) = app.make_token("phone", "full").await;
    let app = &app;
    let put = |body: Value| {
        let full = full.clone();
        async move {
            let a = Ask { token: Some(&full), body: Some(body), ..ask("PUT", "/api/settings", "den.lan:8787", None) };
            let (status, _, v) = app.from(LAN, a).await;
            (status, v)
        }
    };
    let saved = || app.db.settings_all(PlayerId::FIRST).unwrap().get("openai_api_key").cloned();
    let ollama = json!({ "openai_base_url": "http://localhost:11434/v1", "openai_api_key": "sk-local-0123456789" });
    assert_eq!(app.here("PUT", "/api/settings", Some(ollama)).await.0, StatusCode::OK);

    // The same address, or another setting: the key stays.
    let (status, v) = put(json!({ "openai_base_url": "http://localhost:11434/v1/", "openai_model": "qwen" })).await;
    assert_eq!((status, v["openai_api_key_set"].clone()), (StatusCode::OK, json!(true)), "{v}");
    // A new address alone: the key goes.
    let (status, v) = put(json!({ "openai_base_url": "https://collector.example/v1" })).await;
    assert_eq!((status, v["openai_api_key_set"].clone()), (StatusCode::OK, json!(false)), "{v}");
    assert_eq!(saved(), None);
    // With a key of its own, that key is the one saved.
    let (status, v) = put(json!({ "openai_base_url": "https://openrouter.ai/api/v1", "openai_api_key": "sk-or-0123456789" })).await;
    assert_eq!((status, v["openai_api_key_set"].clone()), (StatusCode::OK, json!(true)), "{v}");
    assert_eq!(saved().as_deref(), Some("sk-or-0123456789"));
    // This computer changes the address and keeps the key.
    let back = json!({ "openai_base_url": "http://localhost:11434/v1" });
    assert_eq!(app.here("PUT", "/api/settings", Some(back)).await.1["openai_api_key_set"], json!(true));

    // A key from the environment can't be cleared from here, so the address
    // is this computer's to change.
    let app = reconfigured(test_app().await, |c| {
        c.host = "0.0.0.0".into();
        c.openai_api_key = Some("sk-env-0123456789".into());
    });
    let (_, full) = app.make_token("phone", "full").await;
    let a = Ask {
        token: Some(&full),
        body: Some(json!({ "openai_base_url": "https://collector.example/v1" })),
        ..ask("PUT", "/api/settings", "den.lan:8787", None)
    };
    let (status, _, v) = app.from(LAN, a).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
    assert!(v["error"].as_str().unwrap().contains("set in the environment"), "{v}");
    let mine = json!({ "openai_base_url": "https://collector.example/v1", "openai_api_key": "sk-mine-0123456789" });
    let a = Ask { token: Some(&full), body: Some(mine), ..ask("PUT", "/api/settings", "den.lan:8787", None) };
    assert_eq!(app.from(LAN, a).await.0, StatusCode::OK, "with a key of its own");
}

/// Another site's page can't make the server act with a GET either: an
/// image pointing at the API arrives as this computer, so the browser's
/// `Sec-Fetch-Site` decides. Steam's way back from its sign-in comes from
/// Steam's site by design.
#[tokio::test]
async fn other_sites_cant_make_the_server_act_with_a_get() {
    let app = test_app().await;
    let get = |uri: &'static str, site: Option<&'static str>| {
        let headers: &'static [(&'static str, &'static str)] = match site {
            Some("cross-site") => &[("Sec-Fetch-Site", "cross-site")],
            Some("same-site") => &[("Sec-Fetch-Site", "same-site")],
            Some("same-origin") => &[("Sec-Fetch-Site", "same-origin")],
            Some(_) => &[("Sec-Fetch-Site", "none")],
            None => &[],
        };
        let app = &app;
        async move { app.from(HERE, Ask { headers, ..ask("GET", uri, "127.0.0.1:8787", None) }).await }
    };
    for site in ["cross-site", "same-site"] {
        for uri in ["/api/games", "/api/v1/steam/login", "/api/v1/seed"] {
            let (status, _, v) = get(uri, Some(site)).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{site} {uri}: {v}");
            assert_eq!(v["error"], "cross-site request refused");
        }
        assert_eq!(get("/", Some(site)).await.0, StatusCode::OK, "the UI's pages: a link to UpNext works");
        let (_, _, v) = get("/api/steam/callback?state=nope", Some(site)).await;
        assert_ne!(v.get("error").and_then(Value::as_str), Some("cross-site request refused"), "Steam's way back");
    }
    for site in [Some("same-origin"), Some("none"), None] {
        assert_eq!(get("/api/games", site).await.0, StatusCode::OK, "{site:?}");
    }
}

/// Every answer says it mustn't be framed or sniffed: pages, the API and
/// the guard's refusals alike.
#[tokio::test]
async fn every_answer_refuses_framing_and_sniffing() {
    let app = reconfigured(test_app().await, |c| c.host = "0.0.0.0".into());
    for (peer, uri) in [(HERE, "/"), (HERE, "/settings"), (HERE, "/api/games"), (LAN, "/api/games"), (LAN, "/")] {
        let (status, headers, _) = app.from(peer, ask("GET", uri, "127.0.0.1:8787", None)).await;
        assert_eq!(headers[header::X_FRAME_OPTIONS], "DENY", "{uri} ({status})");
        assert_eq!(headers[header::CONTENT_SECURITY_POLICY], "frame-ancestors 'none'", "{uri}");
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff", "{uri}");
        assert_eq!(headers[header::REFERRER_POLICY], "same-origin", "{uri}");
    }
}

/// nginx's default upstream connection: a bare `proxy_pass` speaks HTTP/1.0
/// and adds no forwarding header, with `Host` rewritten to the upstream's
/// address. HTTP/1.0 from this computer is a proxy's, so it needs a token;
/// browsers, the app window and UpNext's CLI speak HTTP/1.1 or newer.
#[tokio::test]
async fn http_1_0_from_this_computer_is_a_proxy() {
    let app = test_app().await;
    let (_, full) = app.make_token("phone", "full").await;
    let send = |version: axum::http::Version, method: &str, uri: &str, token: Option<&str>| {
        let mut req = Request::builder().method(method).uri(uri).version(version).header(header::HOST, "127.0.0.1:8787");
        if let Some(t) = token {
            req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        let body = if method == "POST" {
            req = req.header(header::CONTENT_TYPE, "application/json");
            Body::from(json!({ "name": "via nginx", "scope": "full" }).to_string())
        } else {
            Body::empty()
        };
        let router = app.router.clone().layer(axum::extract::connect_info::MockConnectInfo(std::net::SocketAddr::from((HERE, 50000))));
        router.oneshot(req.body(body).unwrap())
    };
    use axum::http::Version;
    let refused = send(Version::HTTP_10, "GET", "/api/games", None).await.unwrap();
    assert_eq!(refused.status(), StatusCode::UNAUTHORIZED);
    let v: Value = serde_json::from_slice(&to_bytes(refused.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert!(v["error"].as_str().unwrap().contains("HTTP/1.0"), "{v}");
    assert_eq!(send(Version::HTTP_10, "POST", "/api/tokens", None).await.unwrap().status(), StatusCode::UNAUTHORIZED);
    assert_eq!(send(Version::HTTP_10, "GET", "/api/games", Some(&full)).await.unwrap().status(), StatusCode::OK, "with a token");
    assert_eq!(send(Version::HTTP_10, "GET", "/api/health", None).await.unwrap().status(), StatusCode::OK);
    for version in [Version::HTTP_11, Version::HTTP_2] {
        assert_eq!(send(version, "GET", "/api/games", None).await.unwrap().status(), StatusCode::OK, "{version:?}");
    }
    assert_eq!(app.here("GET", "/api/tokens", None).await.1["tokens"].as_array().unwrap().len(), 1, "no token was made");
}

/// A browser too old for `Sec-Fetch-Site` still sends `Origin` on another
/// site's fetch, so an API request whose `Origin` isn't UpNext's own is
/// refused whatever its method. Steam's way back stays the exception.
#[tokio::test]
async fn another_sites_origin_is_refused_on_reads_too() {
    let app = test_app().await;
    for origin in ["https://evil.example", "http://localhost:3000", "null"] {
        for uri in ["/api/games", "/api/v1/steam/login", "/api/v1/seed"] {
            let (status, _, v) = app.from(HERE, Ask { origin: Some(origin), ..ask("GET", uri, "127.0.0.1:8787", None) }).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{origin} {uri}: {v}");
            assert_eq!(v["error"], "cross-site request refused");
        }
        let (_, _, v) =
            app.from(HERE, Ask { origin: Some(origin), ..ask("GET", "/api/steam/callback?state=nope", "127.0.0.1:8787", None) }).await;
        assert_ne!(v.get("error").and_then(Value::as_str), Some("cross-site request refused"), "Steam's way back");
    }
    // UpNext's own origin, or none, as before.
    for origin in [Some("http://127.0.0.1:8787"), Some("http://localhost:8787"), None] {
        let (status, _, v) = app.from(HERE, Ask { origin, ..ask("GET", "/api/games", "127.0.0.1:8787", None) }).await;
        assert_eq!(status, StatusCode::OK, "{origin:?}: {v}");
    }
}
