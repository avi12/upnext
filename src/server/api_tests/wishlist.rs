//! The recommended wishlist, its job and the switch.

use super::*;
use crate::db::PlayerId;
use crate::wikidata::SeriesEntry;
use wiremock::Request as MockRequest;
use wiremock::matchers::path_regex;

const STEAM_ID: &str = "76561197960287930";

/// Steam's store, as far as the wishlist reads it: the Steam wishlist,
/// names, review summaries, store details and SteamSpy's tags. Portal:
/// Revolution is rated well and Bridge Constructor Portal isn't.
async fn store() -> MockServer {
    let s = MockServer::start().await;
    Mock::given(path("/IWishlistService/GetWishlist/v1/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(crate::testutil::fixture_json("steam/wishlist.json")))
        .mount(&s)
        .await;
    let names = json!({ "response": { "store_items": [
        { "appid": 1145350, "name": "Hades II" },
        { "appid": 400, "name": "Portal" },
        { "appid": 1902490, "name": "Aperture Desk Job" }
    ]}});
    Mock::given(path("/IStoreBrowseService/GetItems/v1/")).respond_with(ResponseTemplate::new(200).set_body_json(names)).mount(&s).await;
    Mock::given(path_regex(r"^/appreviews/\d+$"))
        .respond_with(|req: &MockRequest| {
            let (positive, total) = if req.url.path().ends_with("/684410") { (1_800, 3_000) } else { (1_900, 2_000) };
            ResponseTemplate::new(200).set_body_json(json!({
                "success": 1,
                "query_summary": { "review_score": 8, "review_score_desc": "Very Positive", "total_positive": positive,
                    "total_negative": total - positive, "total_reviews": total },
                "reviews": []
            }))
        })
        .mount(&s)
        .await;
    Mock::given(path("/api/appdetails"))
        .respond_with(|req: &MockRequest| {
            let appid = req.url.query_pairs().find(|(k, _)| k == "appids").map(|(_, v)| v.into_owned()).unwrap_or_default();
            let data = json!({ "type": "game", "short_description": "A test game.", "genres": [{ "id": "1", "description": "Puzzle" }],
                "developers": ["A Studio"], "release_date": { "date": "Jan 1, 2024" } });
            ResponseTemplate::new(200).set_body_json(json!({ appid: { "success": true, "data": data } }))
        })
        .mount(&s)
        .await;
    Mock::given(path("/api.php"))
        .respond_with(|req: &MockRequest| {
            let appid: i64 = req.url.query_pairs().find(|(k, _)| k == "appid").and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
            ResponseTemplate::new(200).set_body_json(json!({ "appid": appid, "name": "x", "tags": { "Puzzle": 900, "First-Person": 400 } }))
        })
        .mount(&s)
        .await;
    s
}

fn entry(qid: &str, name: &str, year: i32, appid: u32) -> SeriesEntry {
    SeriesEntry {
        qid: qid.into(),
        name: name.into(),
        year: Some(year),
        steam_appid: Some(appid),
        igdb_slug: None,
        gog_id: None,
        gog_slug: None,
        cover: None,
        cover_checked: false,
    }
}

/// Portal 2, played 21 hours, is in a series with three games you don't own.
async fn portal_series(app: &TestApp) {
    let id = app.id_of("Portal 2").await;
    app.db.put_title_series(id, &[("Portal".into(), "Q2".into())], Some(2011)).unwrap();
    app.db
        .put_series_catalog(
            "Q2",
            &[
                entry("Q20", "Portal", 2007, 400),
                entry("Q21", "Portal 2", 2011, 620),
                entry("Q22", "Bridge Constructor Portal", 2017, 684410),
                entry("Q23", "Portal: Revolution", 2024, 601360),
            ],
        )
        .unwrap();
}

fn names(list: &Value) -> Vec<&str> {
    let mut out: Vec<&str> = list.as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect();
    out.sort();
    out
}

fn requests_to(reqs: &[MockRequest], part: &str) -> usize {
    reqs.iter().filter(|r| r.url.path().contains(part)).count()
}

#[tokio::test]
async fn the_wishlist_reads_steam_suggests_from_collections_and_takes_answers() {
    let s = store().await;
    let app = test_app_with(None, Some(&s.uri())).await;
    portal_series(&app).await;
    let (status, v) = app.call("PUT", "/api/settings", Some(json!({ "steam_id": STEAM_ID }))).await;
    assert_eq!((status, &v["wishlist"]), (StatusCode::OK, &json!(true)), "{v}");

    let v = app.get("/api/wishlist").await;
    assert_eq!((&v["enabled"], &v["suggested"], &v["wishlist"]), (&json!(true), &json!([]), &json!([])), "{v}");
    assert_eq!((&v["task"]["schedule"], &v["steam_account"]), (&json!("after:details"), &json!(true)), "{v}");
    assert_eq!(v["rules"], json!({ "series_hours": 10, "min_positive": 80, "min_reviews": 50, "per_series": 3 }));

    let (status, job) = app.call("POST", "/api/jobs", Some(json!({ "task": "wishlist" }))).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    let ended = app.job_ended().await;
    assert_eq!(ended["status"], "done", "{ended}");
    let v = app.get("/api/wishlist").await;
    assert_eq!(names(&v["wishlist"]), vec!["Aperture Desk Job", "Hades II", "Portal"], "{v}");
    assert!(v["wishlist"].as_array().unwrap().iter().all(|e| e["on_steam"] == true && e["source"] == "steam"), "{v}");
    // Portal is on the Steam wishlist, so it isn't suggested; Bridge
    // Constructor Portal is rated 60%.
    assert_eq!(names(&v["suggested"]), vec!["Portal: Revolution"], "{v}");
    let sug = &v["suggested"][0];
    assert_eq!(
        (&sug["reason"], &sug["source"], &sug["series"], &sug["review_pct"], &sug["type"], &sug["tags"]),
        (
            &json!("You played Portal 2 for 21 hours. This one came after it, in 2024."),
            &json!("collection"),
            &json!("Portal"),
            &json!(95),
            &json!("game"),
            &json!(["Puzzle", "First-Person"])
        ),
        "{sug}"
    );
    let job = app.get(&format!("/api/jobs/{}", ended["id"])).await;
    let log = job["log"].to_string();
    assert!(log.contains("Steam wishlist: 3 games (3 new here, 0 suggestions already, 0 taken off since the last read)"), "{log}");
    assert!(log.contains("1 new suggestion, 1 passed over (1 rated too low or by too few)"), "{log}");

    // Answers.
    let id = sug["id"].as_i64().unwrap();
    let (status, v) = app.call("PUT", &format!("/api/wishlist/{id}"), Some(json!({ "state": "wishlisted" }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!((names(&v["suggested"]).len(), names(&v["wishlist"]).len()), (0, 4), "{v}");
    for (state, code) in [("in_library", StatusCode::BAD_REQUEST), ("maybe", StatusCode::BAD_REQUEST)] {
        let (status, v) = app.call("PUT", &format!("/api/wishlist/{id}"), Some(json!({ "state": state }))).await;
        assert_eq!(status, code, "{state}: {v}");
    }
    let (status, v) = app.call("PUT", &format!("/api/wishlist/{id}"), Some(json!({ "state": "dismissed" }))).await;
    assert_eq!((status, v["dismissed"].as_u64()), (StatusCode::OK, Some(1)), "{v}");
    let (status, v) = app.call("POST", "/api/wishlist", Some(json!({ "id": id }))).await;
    assert_eq!((status, names(&v["wishlist"]).len()), (StatusCode::OK, 4), "{v}");
    let (status, v) = app.call("POST", "/api/wishlist", Some(json!({}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    let (status, v) = app.call("DELETE", &format!("/api/wishlist/{id}"), None).await;
    assert_eq!((status, names(&v["wishlist"]).len()), (StatusCode::OK, 3), "{v}");
    assert_eq!(app.call("DELETE", &format!("/api/wishlist/{id}"), None).await.0, StatusCode::NOT_FOUND);

    // What UpNext knows about you lists it, and can delete it.
    let sources = app.get("/api/taste/sources").await;
    assert_eq!(sources["wishlist"], json!({ "on": true, "steam": 3, "added": 0, "suggested": 0, "answered": 0, "bought": 0 }));
}

/// HowLongToBeat is scraped, so the wishlist job asks it for times only
/// once its job has a schedule: off by default, like `--with hltb`.
#[tokio::test]
async fn the_wishlist_asks_howlongtobeat_only_once_it_is_turned_on() {
    let s = store().await;
    let app = test_app_with(None, Some(&s.uri())).await;
    portal_series(&app).await;
    let (status, v) = app.call("PUT", "/api/settings", Some(json!({ "steam_id": STEAM_ID }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    // The client starts at the site's home page, `/` on the mock.
    let hltb = |reqs: Vec<MockRequest>| reqs.iter().filter(|r| r.url.path() == "/").count();

    let (status, job) = app.call("POST", "/api/jobs", Some(json!({ "task": "wishlist" }))).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    let ended = app.job_ended().await;
    assert_eq!(ended["status"], "done", "{ended}");
    assert_eq!(hltb(s.received_requests().await.unwrap()), 0, "HowLongToBeat is off by default");
    let log = app.get(&format!("/api/jobs/{}", ended["id"])).await["log"].to_string();
    assert!(log.contains("HowLongToBeat is off"), "{log}");

    let (status, v) = app.call("PUT", "/api/jobs/scheduled/hltb", Some(json!({ "schedule": "weekly" }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let (status, job) = app.call("POST", "/api/jobs", Some(json!({ "task": "wishlist" }))).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    app.job_ended().await;
    assert!(hltb(s.received_requests().await.unwrap()) > 0, "turned on, the wishlist asks it");
}

/// A game added by name is found on Steam's store first, and one you own is turned away.
#[tokio::test]
async fn a_game_is_added_by_name_unless_you_own_it() {
    let s = store().await;
    Mock::given(path("/api/storesearch/"))
        .respond_with(|req: &MockRequest| {
            let term = req.url.query_pairs().find(|(k, _)| k == "term").map(|(_, v)| v.into_owned()).unwrap_or_default();
            let items = if term.to_lowercase().contains("hades") {
                json!([{ "type": "app", "name": "Hades II", "id": 1145350 }])
            } else {
                crate::testutil::fixture_json("steam/storesearch_portal2.json")["items"].clone()
            };
            ResponseTemplate::new(200).set_body_json(json!({ "total": 1, "items": items }))
        })
        .mount(&s)
        .await;
    let app = test_app_with(None, Some(&s.uri())).await;
    let (status, v) = app.call("POST", "/api/wishlist", Some(json!({ "name": "Portal 2" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"], "Portal 2 is in your library already, as Portal 2");
    let (status, v) = app.call("POST", "/api/wishlist", Some(json!({ "name": "Hades II" }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let added = &v["wishlist"][0];
    assert_eq!((&added["name"], &added["source"], &added["review_pct"]), (&json!("Hades II"), &json!("manual"), &json!(95)), "{added}");
    let (status, v) = app.call("POST", "/api/wishlist", Some(json!({ "name": "Nothing Like It" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
}

/// The plan's tests: a synced purchase leaves the wishlist and its page
/// says so; with the switch off nothing calls Steam's wishlist, the page
/// shows nothing, and what's stored stays until it's deleted.
#[tokio::test]
async fn a_purchase_leaves_it_and_the_switch_turns_it_all_off() {
    let s = store().await;
    for (at, fixture) in [("/IPlayerService/GetOwnedGames/v1/", "owned_games"), ("/ISteamUser/GetPlayerSummaries/v2/", "player_summary")] {
        let body = crate::testutil::fixture_json(&format!("steam/{fixture}.json"));
        Mock::given(path(at)).respond_with(ResponseTemplate::new(200).set_body_json(body)).mount(&s).await;
    }
    let app = test_app_with(None, Some(&s.uri())).await;
    let (status, v) = app.call("PUT", "/api/settings", Some(json!({ "steam_id": STEAM_ID }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    app.call("POST", "/api/jobs", Some(json!({ "task": "wishlist" }))).await;
    assert_eq!(app.job_ended().await["status"], "done");
    assert_eq!(names(&app.get("/api/wishlist").await["wishlist"]), vec!["Aperture Desk Job", "Hades II", "Portal"]);

    // Portal is bought on Steam: the sync brings it in, and it leaves the wishlist.
    let key = "0123456789ABCDEF0123456789ABCDEF";
    let (status, v) = app.call("PUT", "/api/settings", Some(json!({ "steam_api_key": key }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let (status, job) = app.call("POST", "/api/jobs", Some(json!({ "task": "sync" }))).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    let ended = app.job_ended().await;
    assert_eq!(ended["status"], "done", "{ended}");
    let v = app.get("/api/wishlist").await;
    assert_eq!((names(&v["wishlist"]), v["in_library"].as_u64()), (vec!["Aperture Desk Job", "Hades II"], Some(1)), "{v}");
    let portal = app.id_of("Portal").await;
    let page = app.get(&format!("/api/games/{portal}")).await;
    assert_eq!(page["from_wishlist"]["source"], "steam", "{page}");
    assert!(app.get(&format!("/api/games/{}", app.id_of("Portal 2").await)).await.get("from_wishlist").is_none());

    // Off: nothing shows, nothing changes, and the job reads nothing.
    let (status, v) = app.call("PUT", "/api/settings", Some(json!({ "wishlist": false }))).await;
    assert_eq!((status, &v["wishlist"]), (StatusCode::OK, &json!(false)), "{v}");
    let v = app.get("/api/wishlist").await;
    assert_eq!((&v["enabled"], &v["wishlist"]), (&json!(false), &json!([])), "{v}");
    assert_eq!(app.call("POST", "/api/wishlist", Some(json!({ "name": "Hades II" }))).await.0, StatusCode::CONFLICT);
    assert!(app.get(&format!("/api/games/{portal}")).await.get("from_wishlist").is_none(), "no trace on the game page");
    let before = s.received_requests().await.unwrap();
    app.call("POST", "/api/jobs", Some(json!({ "task": "wishlist" }))).await;
    assert_eq!(app.job_ended().await["status"], "done");
    let after = s.received_requests().await.unwrap();
    for part in ["IWishlistService", "appreviews", "GetItems", "api.php"] {
        assert_eq!(requests_to(&after, part), requests_to(&before, part), "{part} was called with the wishlist off");
    }
    let sources = app.get("/api/taste/sources").await;
    assert_eq!((&sources["wishlist"]["on"], &sources["wishlist"]["steam"]), (&json!(false), &json!(2)), "kept until deleted");
    let (status, v) = app.call("DELETE", "/api/taste/sources/wishlist", None).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(app.get("/api/taste/sources").await["wishlist"]["steam"], 0);
    assert!(app.db.wishlist_rows(PlayerId::FIRST).unwrap().is_empty());
}

/// Adding by name tries Steam's store once: when it doesn't answer, the
/// person hears so at once (a 502 saying why), not after the job's backoff.
#[tokio::test]
async fn adding_by_name_fails_fast_when_steam_is_down() {
    let s = MockServer::start().await;
    Mock::given(path("/api/storesearch/")).respond_with(ResponseTemplate::new(503)).mount(&s).await;
    let app = test_app_with(None, Some(&s.uri())).await;
    let (status, v) = app.call("POST", "/api/wishlist", Some(json!({ "name": "Hades II" }))).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{v}");
    let error = v["error"].as_str().unwrap();
    assert!(
        error.starts_with("Steam's store didn't answer (HTTP 503")
            && error.ends_with("so \"Hades II\" couldn't be looked up; try again in a minute"),
        "{error}"
    );
    assert_eq!(requests_to(&s.received_requests().await.unwrap(), "storesearch"), 1, "one try");
    assert!(app.db.wishlist_rows(PlayerId::FIRST).unwrap().is_empty());
}

/// At most 3 suggestions wait from one series and 30 in all: eleven series
/// you've played, each with four entries you don't own, all rated well.
#[tokio::test]
async fn suggestions_wait_three_per_series_and_thirty_in_all() {
    let s = store().await;
    // The made-up GOG games have no store page to match.
    Mock::given(path("/api/storesearch/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "total": 0, "items": [] })))
        .mount(&s)
        .await;
    let app = test_app_with(None, Some(&s.uri())).await;
    let played: Vec<ImportedGame> = (0..11)
        .map(|i| ImportedGame {
            source: "gog".into(),
            source_id: format!("g{i}"),
            launcher: "heroic".into(),
            name: format!("Saga {i}"),
            playtime_minutes: Some(700),
            ..Default::default()
        })
        .collect();
    app.db.sync_source(PlayerId::FIRST, "heroic", "gog", &played).unwrap();
    for i in 0..11u32 {
        let id = app.id_of(&format!("Saga {i}")).await;
        let qid = format!("Q{}", 100 + i);
        app.db.put_title_series(id, &[(format!("Saga {i}"), qid.clone())], Some(2000)).unwrap();
        let mut entries = vec![SeriesEntry { steam_appid: None, ..entry(&format!("{qid}0"), &format!("Saga {i}"), 2000, 0) }];
        for n in 1..=4 {
            entries.push(entry(&format!("{qid}{n}"), &format!("Saga {i}: Part {n}"), 2000 + n as i32, 700_000 + i * 10 + n));
        }
        app.db.put_series_catalog(&qid, &entries).unwrap();
    }
    let (status, job) = app.call("POST", "/api/jobs", Some(json!({ "task": "wishlist" }))).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    let ended = app.job_ended().await;
    let log = app.get(&format!("/api/jobs/{}", ended["id"])).await["log"].to_string();
    assert_eq!(ended["status"], "done", "{ended} {log}");
    let v = app.get("/api/wishlist").await;
    let suggested = v["suggested"].as_array().unwrap();
    assert_eq!(suggested.len(), 30, "{v}");
    let mut per_series: std::collections::HashMap<&str, usize> = Default::default();
    for e in suggested {
        *per_series.entry(e["series"].as_str().unwrap()).or_default() += 1;
    }
    assert_eq!(per_series.len(), 10, "{per_series:?}");
    assert!(per_series.values().all(|n| *n == 3), "{per_series:?}");
    assert!(suggested.iter().all(|e| !e["name"].as_str().unwrap().ends_with("Part 4")), "oldest first within a series");
}

/// "Switch with owned game" over the API: the owned game takes the
/// wishlisted pick's slot; any other card action on it is refused.
/// A lineup "b"-like batch with Celeste in slot 0 and a wishlisted game in
/// slot 1 whose owned alternate is Hades. Returns the wishlist id.
async fn save_wished_lineup(app: &TestApp, batch_id: &str, appid: u32, name: &str) -> i64 {
    let hades = app.id_of("Hades").await;
    let celeste = app.id_of("Celeste").await;
    let new = crate::db::NewWish {
        appid,
        name: name.into(),
        source: crate::db::WishSource::Manual,
        reason: None,
        state: crate::db::WishState::Wishlisted,
        on_steam: false,
        series_key: None,
        batch_id: None,
    };
    let (wid, _) = app.db.wishlist_add(PlayerId::FIRST, &new).unwrap();
    let wished = app.db.wish_games(&[wid]).unwrap().remove(&wid).unwrap();
    let all = app.db.all_games(PlayerId::FIRST).unwrap();
    let of = |id: i64| all.iter().find(|g| g.id == id).unwrap();
    let mut w = crate::db::Pick::of(of(celeste), "The one you've waited for.".into());
    (w.id, w.appid, w.name, w.game, w.owned, w.slot) = (-wid, Some(appid), name.into(), None, false, Some(1));
    w.wishlist = Some(wished);
    w.alternate = Some(crate::db::Alternate { id: hades, name: "Hades".into(), reason: "Hades fits.".into(), game: None });
    let batch = crate::db::PickBatch {
        batch_id: batch_id.into(),
        prompt: String::new(),
        created_at: 1,
        model: None,
        duration_ms: None,
        cost_usd: None,
        api_usd: None,
        candidates_considered: None,
        attempts: None,
        usage: None,
        shown: 2,
        picks: vec![crate::db::Pick { slot: Some(0), ..crate::db::Pick::of(of(celeste), "Celeste fits.".into()) }, w],
        spares: vec![],
        removed: vec![],
        ranked_by: None,
        mode: None,
        suggestion: None,
        routine: None,
    };
    app.db.save_batch(PlayerId::FIRST, &batch).unwrap();
    wid
}

#[tokio::test]
async fn a_wishlisted_pick_switches_to_its_owned_game() {
    let app = test_app().await;
    let hades = app.id_of("Hades").await;
    let celeste = app.id_of("Celeste").await;
    let wid = save_wished_lineup(&app, "b", 1145350, "Hades II").await;
    let v = app.get("/api/picks/history").await;
    let shown = &v[0]["picks"][1];
    assert_eq!(
        (&shown["id"], &shown["owned"], &shown["wishlist"]["name"], &shown["alternate"]["id"]),
        (&json!(-wid), &json!(false), &json!("Hades II"), &json!(hades)),
        "{shown}"
    );
    assert_eq!(v[0]["picks"][0]["owned"], true);

    let (status, v) = app.call("POST", "/api/picks/b/feedback", Some(json!({ "title_id": -wid, "action": "chosen" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    let (status, v) = app.call("POST", "/api/picks/b/switch", Some(json!({ "title_id": celeste }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    let (status, v) = app.call("POST", "/api/picks/b/switch", Some(json!({ "title_id": -wid }))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let picks: Vec<(&Value, &Value)> = v["picks"].as_array().unwrap().iter().map(|p| (&p["name"], &p["owned"])).collect();
    assert_eq!(picks, [(&json!("Celeste"), &json!(true)), (&json!("Hades"), &json!(true))], "{v}");
    assert_eq!(v["picks"][1]["reason"], "Hades fits.");
    assert_eq!(v["removed"][0]["id"], json!(-wid));
    assert_eq!(
        app.call("POST", "/api/picks/b/switch", Some(json!({ "title_id": -wid }))).await.0,
        StatusCode::NOT_FOUND,
        "not on screen now"
    );
}

#[tokio::test]
async fn a_wishlisted_pick_keeps_its_game_after_its_wishlist_entry_is_deleted() {
    let app = test_app().await;
    let hades = app.id_of("Hades").await;
    let one = save_wished_lineup(&app, "one", 1145350, "Hades II").await;
    // One entry deleted from the wishlist.
    let (status, v) = app.call("DELETE", &format!("/api/wishlist/{one}"), None).await;
    assert!(status.is_success(), "{status} {v}");
    let v = app.get("/api/picks/history").await;
    let shown = &v[0]["picks"][1];
    assert_eq!(
        (&shown["owned"], &shown["wishlist"]["id"], &shown["wishlist"]["appid"], &shown["wishlist"]["name"]),
        (&json!(false), &json!(one), &json!(1145350), &json!("Hades II")),
        "{shown}"
    );
    assert!(shown["wishlist"]["tags"].is_array() && shown["wishlist"]["state"].is_string(), "{shown}");
    let (status, v) = app.call("POST", "/api/picks/one/switch", Some(json!({ "title_id": -one }))).await;
    assert_eq!((status, &v["picks"][1]["id"]), (StatusCode::OK, &json!(hades)), "{v}");
    assert_eq!(v["removed"][0]["wishlist"]["name"], "Hades II", "{v}");

    // The whole wishlist forgotten from the profile.
    let two = save_wished_lineup(&app, "two", 1030300, "Hollow Knight: Silksong").await;
    let (status, v) = app.call("DELETE", "/api/taste/sources/wishlist", None).await;
    assert!(status.is_success(), "{status} {v}");
    assert_eq!(app.db.wish_games(&[two]).unwrap().len(), 0, "the entry is gone");
    let v = app.get("/api/picks/history").await;
    let lineup = v.as_array().unwrap().iter().find(|b| b["batch_id"] == "two").unwrap();
    assert_eq!(lineup["picks"][1]["wishlist"]["name"], "Hollow Knight: Silksong", "{lineup}");
}
