//! Backlog verdicts, sending side, against a wiremock
//! stand-in for the verdicts service.

use super::*;
use crate::verdicts::mine;
use wiremock::matchers::header_regex;

const RID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaa";
const RID2: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbb";

fn key_header() -> wiremock::matchers::HeaderRegexMatcher {
    header_regex("authorization", "^Bearer upnext_v_[0-9a-f]{64}$")
}

fn taken(status: u16, id: &str, state: &str, replaced: bool) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_json(json!({ "id": id, "state": state, "replaced": replaced }))
}

/// Set the service's address and accept the consent.
async fn ready(app: &TestApp, url: &str) {
    let (status, v) = app.call("PUT", "/api/settings", Some(json!({ "verdicts_url": url }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let (status, v) = app.call("POST", "/api/v1/verdicts/consent", Some(json!({ "version": 1 }))).await;
    assert_eq!((status, &v["current"]), (StatusCode::OK, &json!(true)), "{v}");
}

/// The form is pre-filled from the mark and the playtime; sending needs a
/// service and the consent; the verdict sent carries only the allowlisted
/// fields and the key; sending again replaces it; taking it down withdraws
/// it at the service. The key never comes back from the API.
#[tokio::test]
async fn a_verdict_is_sent_after_consent_replaced_and_taken_down() {
    let app = test_app().await;
    let service = MockServer::start().await;
    let portal = app.id_of("Portal 2").await;
    app.call("PUT", &format!("/api/games/{portal}/override"), Some(json!({ "status": "done" }))).await;

    let form = app.get(&format!("/api/v1/verdicts/draft/{portal}")).await;
    assert_eq!(
        (&form["answers"]["outcome"], &form["answers"]["hours"], &form["from"]["outcome"]),
        (&json!("finished"), &json!("20_40"), &json!("mark")),
        "{form}"
    );
    assert_eq!((&form["service"], &form["consent"]["current"], &form["mine"]), (&json!(false), &json!(false), &Value::Null));
    assert!(form["problems"].as_array().unwrap().iter().any(|p| p["field"] == "worth"), "{form}");

    let mut answers = form["answers"].clone();
    answers["worth"] = json!("yes");
    answers["limited_time"] = json!("mostly");
    answers["factors"] = json!(["save_anywhere"]);
    answers["text"] = json!("Short chapters, easy to stop between.");
    let send = format!("/api/v1/verdicts/{portal}/send");
    let (status, v) = app.call("POST", &send, Some(answers.clone())).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "no service yet: {v}");
    app.call("PUT", "/api/settings", Some(json!({ "verdicts_url": service.uri() }))).await;
    let (status, v) = app.call("POST", &send, Some(answers.clone())).await;
    assert_eq!(status, StatusCode::CONFLICT, "no consent yet: {v}");
    assert_eq!(app.call("POST", "/api/v1/verdicts/consent", Some(json!({ "version": 2 }))).await.0, StatusCode::BAD_REQUEST);
    ready(&app, &service.uri()).await;

    // What the form can fix is refused here, before anything leaves.
    let mut bad = answers.clone();
    bad["text"] = json!("write to me@example.org");
    bad["worth"] = Value::Null;
    let (status, v) = app.call("POST", &send, Some(bad)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    let fields: Vec<&str> = v["problems"].as_array().unwrap().iter().map(|p| p["field"].as_str().unwrap()).collect();
    assert_eq!(fields, ["worth", "text"], "{v}");
    assert!(service.received_requests().await.unwrap().is_empty());

    Mock::given(method("POST"))
        .and(path("/v1/verdicts"))
        .and(key_header())
        .respond_with(taken(201, RID, "live", false))
        .up_to_n_times(1)
        .mount(&service)
        .await;
    let (status, v) = app.call("POST", &send, Some(answers.clone())).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!((&v["verdict"]["state"], &v["verdict"]["remote_id"]), (&json!("sent"), &json!(RID)), "{v}");
    let sent = &service.received_requests().await.unwrap()[0];
    let body: Value = serde_json::from_slice(&sent.body).unwrap();
    let mut keys: Vec<&str> = body.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["client", "consent", "factors", "hours", "licence", "limited_time", "outcome", "steam_appid", "text", "vocab", "worth"],
        "{body}"
    );
    assert_eq!(
        (&body["steam_appid"], &body["licence"], &body["consent"], &body["hours"]),
        (&json!(620), &json!("CC0-1.0"), &json!(1), &json!("20_40"))
    );
    assert!(!sent.headers.contains_key("cookie"));
    let key = sent.headers["authorization"].to_str().unwrap().to_string();

    // A sent verdict is replaced by sending, not saving.
    let (status, _) = app.call("PUT", &format!("/api/v1/verdicts/{portal}"), Some(answers.clone())).await;
    assert_eq!(status, StatusCode::CONFLICT);
    Mock::given(method("POST")).and(path("/v1/verdicts")).respond_with(taken(200, RID, "live", true)).mount(&service).await;
    answers["worth"] = json!("for_some");
    let (status, v) = app.call("POST", &send, Some(answers.clone())).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert!(v["message"].as_str().unwrap().contains("replaces"), "{v}");
    let again = &service.received_requests().await.unwrap()[1];
    assert_eq!(again.headers["authorization"].to_str().unwrap(), key, "the same key each time");

    // The mark changed: the page offers to update the verdict.
    app.call("PUT", &format!("/api/games/{portal}/override"), Some(json!({ "status": "abandoned" }))).await;
    let form = app.get(&format!("/api/v1/verdicts/draft/{portal}")).await;
    assert_eq!((&form["outdated"], &form["answers"]["worth"], &form["from"]), (&json!(true), &json!("for_some"), &json!({})), "{form}");

    Mock::given(method("DELETE"))
        .and(path(format!("/v1/verdicts/{RID}")))
        .and(key_header())
        .respond_with(ResponseTemplate::new(204))
        .mount(&service)
        .await;
    let (status, v) = app.call("DELETE", &format!("/api/v1/verdicts/{portal}"), None).await;
    assert_eq!((status, &v["verdict"]["state"]), (StatusCode::OK, &json!("withdrawn")), "{v}");
    assert_eq!(app.call("DELETE", &format!("/api/v1/verdicts/{portal}"), None).await.0, StatusCode::CONFLICT);

    let list = app.get("/api/v1/verdicts").await;
    assert_eq!((&list["has_key"], &list["verdicts"][0]["state"]), (&json!(true), &json!("withdrawn")), "{list}");
    for uri in ["/api/v1/verdicts", "/api/v1/settings", &format!("/api/v1/verdicts/draft/{portal}"), "/api/v1/verdicts/consent"] {
        assert!(!app.get(uri).await.to_string().contains("upnext_v_"), "{uri} shows the key");
    }
}

/// A game with neither a Steam appid nor a GOG id can't get a verdict; a
/// draft is saved and forgotten without the service.
#[tokio::test]
async fn drafts_stay_on_this_computer() {
    let app = test_app().await;
    let control = app.id_of("Control").await;
    let form = app.get(&format!("/api/v1/verdicts/draft/{control}")).await;
    assert!(form["cant_send"].as_str().unwrap().contains("neither a Steam appid nor a GOG id"), "{form}");
    assert_eq!(app.call("PUT", &format!("/api/v1/verdicts/{control}"), Some(json!({}))).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(app.call("GET", "/api/v1/verdicts/draft/999999", None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(app.call("PUT", "/api/v1/verdicts/1", Some(json!({ "worth": "yes", "rating": 5 }))).await.0.as_u16(), 422, "unknown fields");

    let hades = app.id_of("Hades").await;
    let (status, v) = app.call("PUT", &format!("/api/v1/verdicts/{hades}"), Some(json!({ "worth": "yes", "text": "  " }))).await;
    assert_eq!((status, &v["state"], &v["answers"]["text"]), (StatusCode::OK, &json!("draft"), &Value::Null), "{v}");
    let form = app.get(&format!("/api/v1/verdicts/draft/{hades}")).await;
    assert_eq!(form["answers"]["worth"], "yes", "the saved answers come back");
    let huge = json!({ "worth": "yes", "text": "x".repeat(20_000) });
    assert_eq!(app.call("PUT", &format!("/api/v1/verdicts/{hades}"), Some(huge)).await.0, StatusCode::PAYLOAD_TOO_LARGE);
    let (status, v) = app.call("DELETE", &format!("/api/v1/verdicts/{hades}"), None).await;
    assert_eq!((status, &v["verdict"]), (StatusCode::OK, &Value::Null), "a draft is forgotten: {v}");
    assert!(app.db.my_verdict(PlayerId::FIRST, hades).unwrap().is_none());
    assert!(app.db.verdicts_key(PlayerId::FIRST).unwrap().is_none(), "no key until something is sent");
}

/// A busy service (429) or one out of reach queues the verdict, and it's
/// sent when its time comes; a refusal on the retry makes it a draft again.
#[tokio::test]
async fn a_busy_or_unreachable_service_queues_the_verdict() {
    let app = test_app().await;
    let service = MockServer::start().await;
    ready(&app, &service.uri()).await;
    let hades = app.id_of("Hades").await;
    let answers = json!({ "outcome": "finished", "worth": "yes" });
    let send = format!("/api/v1/verdicts/{hades}/send");

    Mock::given(method("POST"))
        .and(path("/v1/verdicts"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "120").set_body_json(json!({"error": "a limit"})))
        .up_to_n_times(1)
        .mount(&service)
        .await;
    let before = crate::db::now();
    let (status, v) = app.call("POST", &send, Some(answers.clone())).await;
    assert_eq!((status, &v["verdict"]["state"]), (StatusCode::ACCEPTED, &json!("queued")), "{v}");
    let retry_at = v["verdict"]["retry_at"].as_i64().unwrap();
    assert!((before + 120..=before + 125).contains(&retry_at), "{v}");
    assert!(v["message"].as_str().unwrap().contains("2 minutes"), "{v}");

    Mock::given(method("POST"))
        .and(path("/v1/verdicts"))
        .respond_with(taken(201, RID, "held", false))
        .up_to_n_times(1)
        .mount(&service)
        .await;
    let cfg = app.state.cfg();
    let http = &app.state.clients.http;
    assert_eq!(mine::retry_due(&app.db, http, &cfg, retry_at - 1).await.unwrap(), 0, "not yet");
    assert_eq!(mine::retry_due(&app.db, http, &cfg, retry_at).await.unwrap(), 1);
    let m = app.db.my_verdict(PlayerId::FIRST, hades).unwrap().unwrap();
    assert_eq!((m.state, m.remote_id.as_deref(), m.error), (crate::db::VerdictState::Held, Some(RID), None));

    // Out of reach: queued for an hour.
    let portal = app.id_of("Portal 2").await;
    app.call("PUT", "/api/settings", Some(json!({ "verdicts_url": "http://127.0.0.1:9" }))).await;
    let (status, v) = app.call("POST", &format!("/api/v1/verdicts/{portal}/send"), Some(answers.clone())).await;
    assert_eq!((status, &v["verdict"]["state"]), (StatusCode::ACCEPTED, &json!("queued")), "{v}");
    assert!(v["verdict"]["error"].as_str().unwrap().contains("couldn't be reached"), "{v}");

    // Back, and blocking the key: the queued verdict is a draft again with why.
    app.call("PUT", "/api/settings", Some(json!({ "verdicts_url": service.uri() }))).await;
    Mock::given(method("POST"))
        .and(path("/v1/verdicts"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({"error": "this key is blocked"})))
        .mount(&service)
        .await;
    let cfg = app.state.cfg();
    assert_eq!(mine::retry_due(&app.db, http, &cfg, crate::db::now() + 7200).await.unwrap(), 0);
    let m = app.db.my_verdict(PlayerId::FIRST, portal).unwrap().unwrap();
    assert_eq!(m.state, crate::db::VerdictState::Draft);
    assert!(m.error.unwrap().contains("blocked"));
    // A 403 on a direct send says so, and keeps the draft.
    let (status, v) = app.call("POST", &format!("/api/v1/verdicts/{portal}/send"), Some(answers)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
}

/// A restored install asks the service what its key sent; taking down
/// everything withdraws at the service and forgets the key.
#[tokio::test]
async fn refresh_restores_the_list_and_withdraw_all_forgets_the_key() {
    let app = test_app().await;
    let service = MockServer::start().await;
    ready(&app, &service.uri()).await;
    let portal = app.id_of("Portal 2").await;
    let hades = app.id_of("Hades").await;
    Mock::given(method("POST")).and(path("/v1/verdicts")).respond_with(taken(201, RID, "live", false)).mount(&service).await;
    let (status, _) =
        app.call("POST", &format!("/api/v1/verdicts/{portal}/send"), Some(json!({ "outcome": "finished", "worth": "yes" }))).await;
    assert_eq!(status, StatusCode::OK);
    // As if a backup from before the verdicts was restored.
    app.db.delete_my_verdict(PlayerId::FIRST, portal).unwrap();

    let listed = |id: &str, appid: u32, state: &str| {
        json!({ "id": id, "state": state, "created_day": "2026-10-09", "updated_day": "2026-10-10", "steam_appid": appid,
                "outcome": "stopped", "worth": "no", "stopped_for": ["grind"], "vocab": 1, "licence": "CC0-1.0", "consent": 1 })
    };
    Mock::given(method("GET")).and(path("/v1/verdicts/mine")).and(key_header())
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "verdicts": [listed(RID, 620, "live"), listed(RID2, 1145360, "removed"), listed("cccccccccccccccccccccccccc", 4242, "live")] })))
        .mount(&service).await;
    let (status, r) = app.call("POST", "/api/v1/verdicts/refresh", None).await;
    assert_eq!((status, r), (StatusCode::OK, json!({ "listed": 3, "added": 2, "changed": 0, "unmatched": 1 })));
    let m = app.db.my_verdict(PlayerId::FIRST, hades).unwrap().unwrap();
    assert_eq!(
        (m.state, m.remote_id.as_deref(), m.verdict.stopped_for.clone()),
        (crate::db::VerdictState::Removed, Some(RID2), vec!["grind".to_string()])
    );
    assert!(m.sent_at.is_some());

    Mock::given(method("DELETE"))
        .and(path("/v1/verdicts"))
        .and(key_header())
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"withdrawn": 1})))
        .mount(&service)
        .await;
    let (status, v) = app.call("POST", "/api/v1/verdicts/withdraw-all", None).await;
    assert_eq!((status, v), (StatusCode::OK, json!({ "withdrawn": 1, "forgot_key": true })));
    assert!(app.db.verdicts_key(PlayerId::FIRST).unwrap().is_none());
    assert_eq!(app.db.my_verdict(PlayerId::FIRST, portal).unwrap().unwrap().state, crate::db::VerdictState::Withdrawn);
    assert_eq!(app.db.my_verdict(PlayerId::FIRST, hades).unwrap().unwrap().state, crate::db::VerdictState::Removed);
    // With no key, there's nothing at the service to list.
    assert_eq!(app.call("POST", "/api/v1/verdicts/refresh", None).await.1["listed"], 0);
}

/// Reports go to the service with the player's key.
#[tokio::test]
async fn a_text_is_reported_to_the_service() {
    let app = test_app().await;
    let service = MockServer::start().await;
    let report = |id: &str, reason: &str| json!({ "verdict_id": id, "reason": reason });
    assert_eq!(app.call("POST", "/api/v1/verdicts/reports", Some(report(RID, "spam"))).await.0, StatusCode::SERVICE_UNAVAILABLE);
    app.call("PUT", "/api/settings", Some(json!({ "verdicts_url": service.uri() }))).await;
    // The consent screen, which tells of the key, comes before the first report.
    assert_eq!(app.call("POST", "/api/v1/verdicts/reports", Some(report(RID, "spam"))).await.0, StatusCode::CONFLICT);
    assert_eq!(app.db.verdicts_key(PlayerId::FIRST).unwrap(), None, "no key made");
    ready(&app, &service.uri()).await;
    assert_eq!(app.call("POST", "/api/v1/verdicts/reports", Some(report(RID, "rude"))).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(app.call("POST", "/api/v1/verdicts/reports", Some(report("nope", "spam"))).await.0, StatusCode::BAD_REQUEST);
    Mock::given(method("POST"))
        .and(path("/v1/reports"))
        .and(key_header())
        .and(body_string_contains(RID))
        .respond_with(ResponseTemplate::new(202).set_body_json(json!({})))
        .mount(&service)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/reports"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"error": "no such verdict"})))
        .mount(&service)
        .await;
    assert_eq!(app.call("POST", "/api/v1/verdicts/reports", Some(report(RID, "personal"))).await.0, StatusCode::ACCEPTED);
    assert_eq!(app.call("POST", "/api/v1/verdicts/reports", Some(report(RID2, "spam"))).await.0, StatusCode::NOT_FOUND);
}

/// A queue that never empties gives up after a week; a long Retry-After is
/// taken as a day at most; a refused change to a sent verdict is a draft of
/// changes, with the service's verdict as it was; a verdict the service no
/// longer has is marked removed when it's taken down.
#[tokio::test]
async fn the_queue_gives_up_and_refusals_keep_what_the_service_has() {
    use crate::db::VerdictState;
    let app = test_app().await;
    let service = MockServer::start().await;
    ready(&app, &service.uri()).await;
    let hades = app.id_of("Hades").await;
    let send = format!("/api/v1/verdicts/{hades}/send");
    let cfg = app.state.cfg();
    let http = &app.state.clients.http;

    // Sent once.
    let first =
        Mock::given(method("POST")).and(path("/v1/verdicts")).respond_with(taken(201, RID, "live", false)).mount_as_scoped(&service).await;
    let (status, v) = app.call("POST", &send, Some(json!({ "outcome": "finished", "worth": "yes" }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    drop(first);

    // A change meets a limit with a Retry-After of a year: queued for a day.
    let limited = Mock::given(method("POST"))
        .and(path("/v1/verdicts"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "31536000").set_body_json(json!({"error": "a limit"})))
        .mount_as_scoped(&service)
        .await;
    let before = crate::db::now();
    let (status, v) = app.call("POST", &send, Some(json!({ "outcome": "finished", "worth": "no" }))).await;
    assert_eq!((status, &v["verdict"]["state"]), (StatusCode::ACCEPTED, &json!("queued")), "{v}");
    let retry_at = v["verdict"]["retry_at"].as_i64().unwrap();
    assert!((before + 86_400..=before + 86_405).contains(&retry_at), "{v}");
    drop(limited);

    // Refused on the retry: the changes are a draft, still pointing at the
    // verdict the service has, not marked sent.
    let refused = Mock::given(method("POST"))
        .and(path("/v1/verdicts"))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(json!({"error": "no", "problems": [{"field": "worth", "why": "not today"}]})),
        )
        .mount_as_scoped(&service)
        .await;
    assert_eq!(mine::retry_due(&app.db, http, &cfg, retry_at).await.unwrap(), 0);
    let m = app.db.my_verdict(PlayerId::FIRST, hades).unwrap().unwrap();
    assert_eq!((m.state, m.remote_id.as_deref(), m.verdict.worth.as_deref()), (VerdictState::Draft, Some(RID), Some("no")));
    assert!(m.error.unwrap().contains("not today"));
    drop(refused);

    // Queued again, and the service is down for over a week: back to a
    // draft with why, and no more tries.
    let down =
        Mock::given(method("POST")).and(path("/v1/verdicts")).respond_with(ResponseTemplate::new(503)).mount_as_scoped(&service).await;
    let (status, v) = app.call("POST", &send, Some(json!({ "outcome": "finished", "worth": "no" }))).await;
    assert_eq!((status, &v["verdict"]["state"]), (StatusCode::ACCEPTED, &json!("queued")), "{v}");
    let sent_at = app.db.my_verdict(PlayerId::FIRST, hades).unwrap().unwrap().updated_at;
    assert_eq!(mine::retry_due(&app.db, http, &cfg, sent_at + 3 * 86_400).await.unwrap(), 0);
    assert_eq!(app.db.my_verdict(PlayerId::FIRST, hades).unwrap().unwrap().state, VerdictState::Queued, "still trying after three days");
    let tries = service.received_requests().await.unwrap().len();
    assert_eq!(mine::retry_due(&app.db, http, &cfg, sent_at + mine::QUEUE_SECS + 1).await.unwrap(), 0);
    let m = app.db.my_verdict(PlayerId::FIRST, hades).unwrap().unwrap();
    assert_eq!((m.state, m.remote_id.as_deref(), m.retry_at), (VerdictState::Draft, Some(RID), None));
    assert!(m.error.unwrap().contains("within a week"));
    assert_eq!(service.received_requests().await.unwrap().len(), tries, "given up without sending");
    drop(down);

    // Checking what the service has leaves the unsent changes as they are:
    // the service lists its earlier verdict, which the row doesn't hold.
    let listing = Mock::given(method("GET"))
        .and(path("/v1/verdicts/mine"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "verdicts": [{
            "id": RID, "state": "live", "created_day": "2026-10-09", "updated_day": "2026-10-09", "steam_appid": 1145360,
            "outcome": "finished", "worth": "yes", "vocab": 1, "licence": "CC0-1.0", "consent": 1 }] })))
        .mount_as_scoped(&service)
        .await;
    let (status, r) = app.call("POST", "/api/v1/verdicts/refresh", None).await;
    assert_eq!((status, &r["changed"], &r["added"]), (StatusCode::OK, &json!(0), &json!(0)), "{r}");
    let m = app.db.my_verdict(PlayerId::FIRST, hades).unwrap().unwrap();
    assert_eq!((m.state, m.verdict.worth.as_deref()), (VerdictState::Draft, Some("no")), "not flipped back to sent");
    drop(listing);

    // Taking it down: the service no longer has it, so it was removed.
    Mock::given(method("DELETE"))
        .and(path(format!("/v1/verdicts/{RID}")))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"error": "no such verdict"})))
        .mount(&service)
        .await;
    let (status, v) = app.call("DELETE", &format!("/api/v1/verdicts/{hades}"), None).await;
    assert_eq!((status, &v["verdict"]["state"]), (StatusCode::OK, &json!("removed")), "{v}");
}
