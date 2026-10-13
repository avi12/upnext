//! Routines: made, listed with the one that fits now, changed and
//! deleted on the Profile, and chosen with a pick.

use super::*;

#[tokio::test]
async fn routines_are_made_listed_changed_and_chosen_with_a_pick() {
    let app = test_app().await;
    assert_eq!(app.get("/api/v1/routines").await, json!({ "routines": [], "now": null }));

    // Every day and every time of day: it always fits now.
    let every = |name: &str| {
        json!({ "name": name, "days": ["sun", "mon", "tue", "wed", "thu", "fri", "sat"],
            "times": ["late_nights", "mornings", "afternoons", "evenings"] })
    };
    let (status, wide) = app.call("POST", "/api/v1/routines", Some(every("Any time"))).await;
    assert_eq!(status, StatusCode::CREATED, "{wide}");
    assert_eq!(wide["days"], json!(["mon", "tue", "wed", "thu", "fri", "sat", "sun"]), "in order");
    let body = json!({ "name": "Friday with friends", "days": ["fri", "sat", "sun", "mon", "tue", "wed", "thu"],
        "times": ["evenings", "late_nights", "mornings"], "who": "friends", "place": "desk", "input": "keyboard_mouse", "minutes": 120 });
    let (status, narrow) = app.call("POST", "/api/v1/routines", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{narrow}");
    let (wide_id, narrow_id) = (wide["id"].as_i64().unwrap(), narrow["id"].as_i64().unwrap());
    let list = app.get("/api/v1/routines").await;
    assert_eq!(list["routines"].as_array().map(Vec::len), Some(2));
    // The narrower fits now only outside the afternoons.
    let afternoon = crate::taste::routine::slot(crate::db::now()).1 == crate::taste::routine::Time::Afternoons;
    let want = if afternoon { wide_id } else { narrow_id };
    assert_eq!(list["now"].as_i64(), Some(want), "{list}");

    for (body, why) in [
        (json!({ "name": " " }), "no name"),
        (json!({ "name": "any TIME" }), "a name taken"),
        (json!({ "name": "Short", "minutes": 5 }), "too short"),
        (json!({ "name": "Who", "who": "robots" }), "an unknown who"),
        (json!({ "name": "Day", "days": ["someday"] }), "an unknown day"),
        (json!({ "name": "Long", "minutes": "an hour" }), "a length in words"),
        (json!({ "days": ["fri"] }), "no name at all"),
    ] {
        let (status, v) = app.call("POST", "/api/v1/routines", Some(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{why}: {v}");
        assert!(v["error"].as_str().is_some_and(|e| !e.is_empty()), "{why}: says why, as JSON: {v}");
    }
    let (status, v) = app.call("POST", "/api/v1/routines", Some(json!({ "name": "Who", "who": "robots" }))).await;
    assert!(v["error"].as_str().unwrap_or_default().contains("robots"), "{status} {v}");
    let raw = |method: &str, uri: &str, content_type: &str, body: &'static str| {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, "127.0.0.1:8787")
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::from(body))
            .unwrap()
    };
    let (status, v) = app.send(raw("POST", "/api/v1/routines", "application/json", "{\"name\": ")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "JSON that doesn't parse: {v}");
    assert!(v["error"].is_string(), "{v}");
    let (status, v) = app.send(raw("POST", "/api/v1/routines", "text/plain", "{\"name\": \"Plain\"}")).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "not sent as JSON: {v}");
    // PUT answers the same two, as docs/api/routines.md says.
    let one = format!("/api/v1/routines/{narrow_id}");
    let (status, v) = app.send(raw("PUT", &one, "application/json", "{\"name\": ")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "PUT, JSON that doesn't parse: {v}");
    assert!(v["error"].is_string(), "{v}");
    let (status, v) = app.send(raw("PUT", &one, "text/plain", "{\"name\": \"Plain\"}")).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "PUT, not sent as JSON: {v}");

    let (status, changed) = app
        .call("PUT", &format!("/api/v1/routines/{narrow_id}"), Some(json!({ "name": "Couch", "place": "couch", "input": "controller" })))
        .await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!((changed["name"].as_str(), changed["days"].clone(), changed["who"].clone()), (Some("Couch"), json!([]), Value::Null));
    let (status, _) = app.call("PUT", &format!("/api/v1/routines/{narrow_id}"), Some(json!({ "name": "Any time" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "another routine's name");
    let (status, _) = app.call("PUT", "/api/v1/routines/999", Some(json!({ "name": "Nope" }))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, v) = app.call("PUT", &format!("/api/v1/routines/{narrow_id}"), Some(json!({ "name": "Couch", "place": "bed" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "an unknown place: {v}");

    // A pick for a routine keeps it with the lineup, through History, after it's deleted.
    let (status, batch) = app.call("POST", "/api/v1/picks", Some(json!({ "prompt": "something short", "routine_id": narrow_id }))).await;
    assert_eq!(status, StatusCode::OK, "{batch}");
    assert_eq!(batch["routine"]["name"].as_str(), Some("Couch"));
    let (status, _) = app.call("DELETE", &format!("/api/v1/routines/{narrow_id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = app.call("DELETE", &format!("/api/v1/routines/{narrow_id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let history = app.get("/api/v1/picks/history?limit=1").await;
    assert_eq!(history[0]["routine"]["name"].as_str(), Some("Couch"), "{history}");
    let (status, v) = app.call("POST", "/api/v1/picks", Some(json!({ "prompt": "", "routine_id": narrow_id }))).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{v}");

    // What you told it: counted and deleted with the answers, and exported.
    assert_eq!(app.get("/api/v1/taste/sources").await["answers"]["routines"].as_i64(), Some(1));
    assert_eq!(app.get("/api/v1/taste/export").await["routines"][0]["name"].as_str(), Some("Any time"));
    let (status, _) = app.call("DELETE", "/api/v1/taste/sources/answers", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(app.get("/api/v1/routines").await["routines"], json!([]));
}
