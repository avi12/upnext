//! Building the shared data against one local mock server standing in for
//! SteamSpy, the Steam store, Steam's reviews and Wikidata, with the
//! fixtures under `tests/fixtures/` and data written here, and the summaries
//! from a scripted model. The export is applied to the mock library.

use super::*;
use crate::db::PlayerId;
use crate::db::{DetailsRow, ReviewText};
use crate::digest::DIGEST_VERSION;
use crate::testutil::{Answer, ScriptedLlm, fixture_json, offline_config};
use serde_json::{Value, json};
use wiremock::matchers::{body_string_contains, method, path, query_param};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

/// Keeps the log lines.
#[derive(Default)]
struct Log(std::sync::Mutex<Vec<String>>);

impl ProgressSink for Log {
    fn update(&self, _: &Progress) {}
    fn log(&self, line: String) {
        self.0.lock().unwrap().push(line);
    }
}

/// An empty database made a build, in a temp folder.
fn build_db() -> (tempfile::TempDir, Config, Db) {
    let dir = tempfile::tempdir().unwrap();
    let cfg = offline_config(dir.path());
    let db = Db::open(&cfg.db_path, cfg.sampled_minutes).unwrap();
    mark(&db).unwrap();
    (dir, cfg, db)
}

/// The Steam store's appdetails: 4200 is a soundtrack, 4400 has no store
/// page, and everything else is a game.
struct Store;

impl Respond for Store {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let id = req.url.query_pairs().find(|(k, _)| k == "appids").map(|(_, v)| v.into_owned()).unwrap_or_default();
        let app = match id.as_str() {
            "4400" => json!({"success": false}),
            "4200" => json!({"success": true, "data": {"type": "music", "name": "Soundtrack", "steam_appid": 4200}}),
            _ => json!({"success": true, "data": {"type": "game", "name": "A game", "steam_appid": id.parse::<u32>().unwrap_or(0)}}),
        };
        let mut body = serde_json::Map::new();
        body.insert(id, app);
        ResponseTemplate::new(200).set_body_json(Value::Object(body))
    }
}

async fn steamspy_and_store() -> MockServer {
    let s = MockServer::start().await;
    // The soundtrack has more reviews than Example Puzzle Game, so it ranks
    // above it in their owners band and has to be skipped.
    let mut page = fixture_json("steamspy/all_page0.json");
    page["4200"]["positive"] = json!(9000);
    Mock::given(path("/api.php"))
        .and(query_param("request", "all"))
        .and(query_param("page", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page))
        .expect(1)
        .mount(&s)
        .await;
    Mock::given(path("/api/appdetails")).respond_with(Store).mount(&s).await;
    s
}

fn names(db: &Db) -> Vec<(u32, String)> {
    let mut out: Vec<(u32, String)> = db.all_games(PlayerId::FIRST).unwrap().into_iter().map(|g| (g.appid.unwrap(), g.name)).collect();
    out.sort();
    out
}

fn store_requests(received: &[Request]) -> usize {
    received.iter().filter(|r| r.url.path() == "/api/appdetails").count()
}

/// The most-owned games make the list, ranked by owners and then reviews;
/// a soundtrack and an app with no store page are passed over, and a second
/// run reads SteamSpy's page and the store's answers from what it kept.
#[tokio::test]
async fn the_build_takes_the_most_owned_games() {
    let s = steamspy_and_store().await;
    let (_dir, cfg, db) = build_db();
    let clients = Clients::with_base_url(&cfg, Some(&s.uri())).unwrap();
    let log = Log::default();

    let built = build(&db, &cfg, &clients, 4, &log).await.unwrap();
    assert_eq!(built, Built { games: 4, listed: 6, skipped: 1 }, "{:?}", log.0.lock().unwrap());
    let four = vec![
        (10, "Counter-Strike".to_string()),
        (620, "Portal 2".to_string()),
        (4100, "Example Puzzle Game".to_string()),
        (4300, "Example Racing".to_string()),
    ];
    assert_eq!(names(&db), four, "the soundtrack ranked above Example Puzzle Game, and was left out");
    assert!(
        db.all_games(PlayerId::FIRST).unwrap().iter().all(|g| g.copies.iter().all(|c| c.launcher == CATALOGUE && c.source == CATALOGUE))
    );
    assert!(work_dir(&cfg).join("steamspy/page-000.json").is_file());
    let asked = store_requests(&s.received_requests().await.unwrap());
    assert_eq!(asked, 6, "the four games, the soundtrack and the app with no page");

    // Fewer: the top three are all games, and only they stay.
    let built = build(&db, &cfg, &clients, 3, &log).await.unwrap();
    assert_eq!(built, Built { games: 3, listed: 6, skipped: 0 });
    assert_eq!(names(&db), vec![four[0].clone(), four[1].clone(), four[3].clone()]);
    // More than there are: every game on the list.
    let built = build(&db, &cfg, &clients, 10, &log).await.unwrap();
    assert_eq!(built.games, 4);
    assert_eq!(names(&db), four);
    assert_eq!(store_requests(&s.received_requests().await.unwrap()), asked, "the store is asked about each app once");
}

#[tokio::test]
async fn the_build_never_runs_in_a_library() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = offline_config(dir.path());
    let db = Db::open(&cfg.db_path, cfg.sampled_minutes).unwrap();
    let e = check(&db, &cfg, true).unwrap_err().to_string();
    assert!(e.contains("a database of its own"), "UpNext's usual database, even empty: {e}");
    assert!(check(&db, &cfg, false).unwrap_err().to_string().contains("isn't a build"));

    crate::mock::seed(&db).unwrap();
    let e = check(&db, &cfg, true).unwrap_err().to_string();
    assert!(e.contains("has a library in it"), "{e}");
    assert!(mark(&db).is_err(), "not an empty database");
    let out = dir.path().join("out");
    assert!(export(&db, &cfg, &out, "2026-10-09").unwrap_err().to_string().contains("has a library in it"));
    assert!(!out.exists());

    // A build stays one, and a library copy in it is still refused.
    let (_dir, cfg, db) = build_db();
    check(&db, &cfg, false).unwrap();
    let steam = ImportedGame {
        source: "steam".into(),
        source_id: "620".into(),
        launcher: "steam".into(),
        name: "Portal 2".into(),
        ..Default::default()
    };
    db.sync_source(PlayerId::FIRST, "steam", "steam", &[steam]).unwrap();
    assert!(check(&db, &cfg, false).is_err());
}

/// A build title as `build` leaves it: in the list, a game by its store page.
fn add_game(db: &Db, appid: u32, name: &str) {
    let mut all: Vec<Listed> = db
        .all_games(PlayerId::FIRST)
        .unwrap()
        .into_iter()
        .map(|g| Listed { appid: g.appid.unwrap(), name: g.name, owners: 0, reviews: 0 })
        .collect();
    all.push(Listed { appid, name: name.into(), owners: 0, reviews: 0 });
    place(db, &all.iter().collect::<Vec<_>>()).unwrap();
    let details =
        DetailsRow { type_: Some("game".into()), short_description: Some(format!("PLANTED DESCRIPTION of {name}")), ..Default::default() };
    db.put_details(appid, Some(&details)).unwrap();
}

/// Reviews come from Steam, the GOG ids from Wikidata (and GOG's catalogue,
/// for a store page without one: `gog_ids_from_store_pages_in_gogs_catalogue`);
/// a second run asks for neither again.
#[tokio::test]
async fn fetch_gets_the_reviews_and_gog_ids() {
    let s = MockServer::start().await;
    Mock::given(path("/appreviews/620"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture_json("steam/appreviews_620.json")))
        .mount(&s)
        .await;
    let rows = json!({"results": {"bindings": [{"steam": {"value": "620"}, "gog": {"value": "1207658930"}}]}});
    Mock::given(method("POST")).and(path("/sparql")).respond_with(ResponseTemplate::new(200).set_body_json(rows)).expect(1).mount(&s).await;
    let (_dir, cfg, db) = build_db();
    add_game(&db, 620, "Portal 2");
    add_game(&db, 4100, "Example Puzzle Game");
    let clients = Clients::with_base_url(&cfg, Some(&s.uri())).unwrap();

    let got = fetch(&db, &cfg, &clients, &Log::default()).await.unwrap();
    assert_eq!((got.reviews, got.gog_ids), (1, 1), "Example Puzzle Game's reviews answer 404: {got:?}");
    assert_eq!(got.errors, 1);
    let portal = db.all_games(PlayerId::FIRST).unwrap().into_iter().find(|g| g.appid == Some(620)).unwrap();
    assert!(portal.has_review_texts && !portal.review_texts.is_empty());
    let kept: BTreeMap<u32, Option<String>> = read_json(&gog_ids_file(&cfg)).unwrap();
    assert_eq!(kept, BTreeMap::from([(620, Some("1207658930".to_string())), (4100, None)]));

    let reviews_before = s.received_requests().await.unwrap().iter().filter(|r| r.url.path() == "/appreviews/620").count();
    let again = fetch(&db, &cfg, &clients, &Log::default()).await.unwrap();
    assert_eq!((again.reviews, again.gog_ids), (0, 1));
    let reviews_after = s.received_requests().await.unwrap().iter().filter(|r| r.url.path() == "/appreviews/620").count();
    assert_eq!(reviews_before, reviews_after, "Portal 2's reviews are kept");

    // Wikidata down: the reviews are kept, the lookup counts as a failure,
    // and the next run asks again.
    add_game(&db, 4300, "Example Racing");
    let broken = MockServer::start().await;
    Mock::given(path("/sparql")).respond_with(ResponseTemplate::new(200).set_body_string("<html>busy</html>")).mount(&broken).await;
    let down = Clients::with_base_url(&cfg, Some(&broken.uri())).unwrap();
    let failed = fetch(&db, &cfg, &down, &Log::default()).await.unwrap();
    assert_eq!((failed.gog_ids, failed.errors), (1, 3), "Example Puzzle Game's and Example Racing's reviews, and Wikidata: {failed:?}");
    let kept: BTreeMap<u32, Option<String>> = read_json(&gog_ids_file(&cfg)).unwrap();
    assert!(!kept.contains_key(&4300));
}

/// Wikidata keeps GOG's numeric product id in P12727; P2725 is the store
/// page's slug ("game/portal_2"), which no GOG copy carries. The first
/// builds asked for P2725 alone and found no GOG id at all; this answer has
/// both shapes, and the "none" an old build kept is asked again.
#[tokio::test]
async fn gog_ids_are_product_ids_not_store_pages() {
    let s = MockServer::start().await;
    let rows = json!({"results": {"bindings": [
        {"steam": {"value": "620"}, "gog": {"value": "1207658930"}, "page": {"value": "game/portal_2"}},
        {"steam": {"value": "400"}, "page": {"value": "game/portal"}},
    ]}});
    Mock::given(method("POST"))
        .and(path("/sparql"))
        .and(body_string_contains("P12727"))
        .respond_with(ResponseTemplate::new(200).set_body_json(rows))
        .expect(1)
        .mount(&s)
        .await;
    Mock::given(path("/v1/catalog"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"products": []})))
        .expect(1)
        .mount(&s)
        .await;
    let (_dir, cfg, db) = build_db();
    for (appid, name) in [(620, "Portal 2"), (400, "Portal"), (4100, "Example Puzzle Game")] {
        add_game(&db, appid, name);
    }
    write_file(&work_dir(&cfg).join("gog-ids.json"), br#"{"620": null, "400": null, "4100": null}"#).unwrap();
    let clients = Clients::with_base_url(&cfg, Some(&s.uri())).unwrap();

    let log = Log::default();
    let got = fetch(&db, &cfg, &clients, &log).await.unwrap();
    assert_eq!(got.gog_ids, 1, "{got:?}");
    let kept: BTreeMap<u32, Option<String>> = read_json(&gog_ids_file(&cfg)).unwrap();
    assert_eq!(kept, BTreeMap::from([(400, None), (620, Some("1207658930".to_string())), (4100, None)]));
    let lines = log.0.lock().unwrap().join("\n");
    assert!(lines.contains("1 of 3 games asked has one; 1 more are on GOG with no product id there"), "{lines}");

    // An answer with no results is a failure, asked again next run, not
    // "no GOG id" for every game in it.
    add_game(&db, 4300, "Example Racing");
    let odd = MockServer::start().await;
    Mock::given(path("/sparql")).respond_with(ResponseTemplate::new(200).set_body_json(json!({}))).mount(&odd).await;
    let odd = Clients::with_base_url(&cfg, Some(&odd.uri())).unwrap();
    let log = Log::default();
    let failed = fetch(&db, &cfg, &odd, &log).await.unwrap();
    assert!(failed.errors >= 1, "{failed:?}");
    let kept: BTreeMap<u32, Option<String>> = read_json(&gog_ids_file(&cfg)).unwrap();
    assert!(!kept.contains_key(&4300));
    assert!(log.0.lock().unwrap().iter().any(|l| l.contains("Wikidata's answer has no results")));
}

/// A catalogue search's answer: products by id and slug.
fn catalogue(products: &[(&str, &str)]) -> ResponseTemplate {
    let products: Vec<Value> =
        products.iter().map(|(id, slug)| json!({"id": id, "slug": slug, "title": "A game", "productType": "game"})).collect();
    ResponseTemplate::new(200).set_body_json(json!({"pages": 1, "productCount": products.len(), "products": products}))
}

/// Wikidata has the GOG store page's slug for many games and a product id
/// for next to none, so a build from before the slugs were kept asks it
/// again, and GOG's catalogue gives each page's id: one search per slug,
/// only an exact slug counts, and an id two games would share goes to
/// neither. A second run asks nothing again; a failed search is asked
/// again next run.
#[tokio::test]
async fn gog_ids_from_store_pages_in_gogs_catalogue() {
    let s = MockServer::start().await;
    let page = |steam: &str, page: &str| json!({"steam": {"value": steam}, "page": {"value": page}});
    let rows = json!({"results": {"bindings": [
        {"steam": {"value": "620"}, "gog": {"value": "1207658930"}, "page": {"value": "game/portal_2"}},
        page("400", "game/portal"),
        page("4100", "game/example_puzzle_game"),
        page("220", "game/example_shooter"),
        page("4300", "game/example_racing"),
        page("4500", "game/example_racing"),
    ]}});
    Mock::given(method("POST")).and(path("/sparql")).respond_with(ResponseTemplate::new(200).set_body_json(rows)).expect(1).mount(&s).await;
    for (words, products) in [
        ("like:portal", vec![("1207658940", "portal"), ("1207658930", "portal_2")]),
        ("like:example puzzle game", vec![("2000000001", "example_puzzle_game")]),
        ("like:example shooter", vec![("2000000003", "example_shooter_soundtrack")]),
        ("like:example racing", vec![("2000000002", "example_racing")]),
    ] {
        Mock::given(path("/v1/catalog")).and(query_param("query", words)).respond_with(catalogue(&products)).expect(1).mount(&s).await;
    }
    let (_dir, cfg, db) = build_db();
    let games = [
        (620, "Portal 2"),
        (400, "Portal"),
        (4100, "Example Puzzle Game"),
        (220, "Example Shooter"),
        (4300, "Example Racing"),
        (4500, "Example Racing Remastered"),
    ];
    for (appid, name) in games {
        add_game(&db, appid, name);
    }
    // What the builds before this kept: "none" from Wikidata for every game.
    let none: BTreeMap<u32, Option<String>> = games.iter().map(|(a, _)| (*a, None)).collect();
    write_file(&gog_ids_file(&cfg), &serde_json::to_vec(&none).unwrap()).unwrap();
    let clients = Clients::with_base_url(&cfg, Some(&s.uri())).unwrap();

    let log = Log::default();
    assert_eq!(gog_ids(&db, &cfg, &clients, &log).await.unwrap(), (3, 0));
    let kept: BTreeMap<u32, Option<String>> = read_json(&gog_ids_file(&cfg)).unwrap();
    let id = |s: &str| Some(s.to_string());
    let want = BTreeMap::from([
        (220, None),
        (400, id("1207658940")),
        (620, id("1207658930")),
        (4100, id("2000000001")),
        (4300, None),
        (4500, None),
    ]);
    assert_eq!(kept, want);
    let lines = log.0.lock().unwrap().join("\n");
    assert!(lines.contains("GOG ids from GOG's catalogue: 3 of 4 store pages found; 1 not in the catalogue"), "{lines}");
    assert!(lines.contains("2 games left without a GOG id: their store page's id is another game's too"), "{lines}");

    assert_eq!(gog_ids(&db, &cfg, &clients, &Log::default()).await.unwrap(), (3, 0), "nothing is asked again");
    assert_eq!(read_json::<BTreeMap<u32, Option<String>>>(&gog_ids_file(&cfg)).unwrap(), want);

    // GOG down: the search counts as a failure and is asked again next run.
    add_game(&db, 4600, "Example Sim");
    let down = MockServer::start().await;
    let rows = json!({"results": {"bindings": [page("4600", "game/example_sim")]}});
    Mock::given(path("/sparql")).respond_with(ResponseTemplate::new(200).set_body_json(rows)).mount(&down).await;
    Mock::given(path("/v1/catalog")).respond_with(ResponseTemplate::new(503)).mount(&down).await;
    let down = Clients::with_base_url(&cfg, Some(&down.uri())).unwrap();
    let log = Log::default();
    assert_eq!(gog_ids(&db, &cfg, &down, &log).await.unwrap(), (3, 1));
    let asked: BTreeMap<String, Option<String>> = read_json(&gog_slug_ids_file(&cfg)).unwrap();
    assert!(!asked.contains_key("example_sim") && asked.contains_key("example_shooter"), "{asked:?}");
    assert!(log.0.lock().unwrap().iter().any(|l| l.starts_with("GOG ids from GOG's catalogue failed")));
}

/// GOG answers some searches with HTTP 400 every time (the slug
/// `breach_and_clear`, searched as "like:breach and clear"): that slug is a
/// miss, kept so it isn't asked again, and the next slug is still looked up
/// and saved.
#[tokio::test]
async fn a_search_gog_refuses_is_a_miss_and_the_run_carries_on() {
    let s = MockServer::start().await;
    let page = |steam: &str, page: &str| json!({"steam": {"value": steam}, "page": {"value": page}});
    let rows = json!({"results": {"bindings": [page("4100", "game/breach_and_clear"), page("4300", "game/example_two")]}});
    Mock::given(path("/sparql")).respond_with(ResponseTemplate::new(200).set_body_json(rows)).expect(1).mount(&s).await;
    Mock::given(path("/v1/catalog"))
        .and(query_param("query", "like:breach and clear"))
        .respond_with(ResponseTemplate::new(400))
        .expect(1)
        .mount(&s)
        .await;
    Mock::given(path("/v1/catalog"))
        .and(query_param("query", "like:example two"))
        .respond_with(catalogue(&[("2000000002", "example_two")]))
        .expect(1)
        .mount(&s)
        .await;
    let (_dir, cfg, db) = build_db();
    for (appid, name) in [(4100, "Breach & Clear"), (4300, "Example Two")] {
        add_game(&db, appid, name);
    }
    let clients = Clients::with_base_url(&cfg, Some(&s.uri())).unwrap();

    let log = Log::default();
    assert_eq!(gog_ids(&db, &cfg, &clients, &log).await.unwrap(), (1, 0), "no failure");
    let kept: BTreeMap<u32, Option<String>> = read_json(&gog_ids_file(&cfg)).unwrap();
    assert_eq!(kept, BTreeMap::from([(4100, None), (4300, Some("2000000002".to_string()))]));
    let asked: BTreeMap<String, Option<String>> = read_json(&gog_slug_ids_file(&cfg)).unwrap();
    assert_eq!(
        asked,
        BTreeMap::from([("breach_and_clear".to_string(), None), ("example_two".to_string(), Some("2000000002".to_string()))])
    );
    let lines = log.0.lock().unwrap().join("\n");
    assert!(lines.contains("GOG's catalogue refused the search for breach_and_clear"), "{lines}");
    assert!(lines.contains("GOG ids from GOG's catalogue: 1 of 2 store pages found; 0 not in the catalogue"), "{lines}");
    assert!(lines.contains("; 1 search refused by GOG"), "{lines}");

    // Not asked again: the mocks above expect one search each.
    assert_eq!(gog_ids(&db, &cfg, &clients, &Log::default()).await.unwrap(), (1, 0));

    // A 404 isn't about one search (the endpoint may have moved): it stops
    // the lookups as a failure, and the slug is asked again next run.
    add_game(&db, 4500, "Example Three");
    let moved = MockServer::start().await;
    let rows = json!({"results": {"bindings": [page("4500", "game/example_three")]}});
    Mock::given(path("/sparql")).respond_with(ResponseTemplate::new(200).set_body_json(rows)).mount(&moved).await;
    Mock::given(path("/v1/catalog")).respond_with(ResponseTemplate::new(404)).expect(1).mount(&moved).await;
    let moved = Clients::with_base_url(&cfg, Some(&moved.uri())).unwrap();
    let log = Log::default();
    assert_eq!(gog_ids(&db, &cfg, &moved, &log).await.unwrap(), (1, 1));
    let asked: BTreeMap<String, Option<String>> = read_json(&gog_slug_ids_file(&cfg)).unwrap();
    assert!(!asked.contains_key("example_three"), "{asked:?}");
    assert!(log.0.lock().unwrap().iter().any(|l| l.starts_with("GOG ids from GOG's catalogue failed")));
}

/// Stops the job after a number of progress updates.
struct StopAfter(std::sync::atomic::AtomicUsize);

impl ProgressSink for StopAfter {
    fn update(&self, _: &Progress) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
    fn log(&self, _: String) {}
    fn cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::SeqCst) == 0
    }
}

/// A run stopped among the catalogue's searches keeps the answers it had,
/// so the next run asks only the slugs left.
#[tokio::test]
async fn a_stopped_catalogue_lookup_keeps_its_answers() {
    let s = MockServer::start().await;
    let page = |steam: &str, page: &str| json!({"steam": {"value": steam}, "page": {"value": page}});
    let rows = json!({"results": {"bindings": [page("4100", "game/example_one"), page("4300", "game/example_two"), page("4500", "game/example_three")]}});
    Mock::given(path("/sparql")).respond_with(ResponseTemplate::new(200).set_body_json(rows)).expect(1).mount(&s).await;
    for (words, id, slug) in [
        ("like:example one", "2000000001", "example_one"),
        ("like:example three", "2000000003", "example_three"),
        ("like:example two", "2000000002", "example_two"),
    ] {
        Mock::given(path("/v1/catalog")).and(query_param("query", words)).respond_with(catalogue(&[(id, slug)])).expect(1).mount(&s).await;
    }
    let (_dir, cfg, db) = build_db();
    for (appid, name) in [(4100, "Example One"), (4300, "Example Two"), (4500, "Example Three")] {
        add_game(&db, appid, name);
    }
    let clients = Clients::with_base_url(&cfg, Some(&s.uri())).unwrap();

    // One update for Wikidata's query, then two searches.
    let stop = StopAfter(std::sync::atomic::AtomicUsize::new(3));
    let e = gog_ids(&db, &cfg, &clients, &stop).await.unwrap_err();
    assert_eq!(e.to_string(), "cancelled");
    let asked: BTreeMap<String, Option<String>> = read_json(&gog_slug_ids_file(&cfg)).unwrap();
    assert_eq!(asked.keys().collect::<Vec<_>>(), ["example_one", "example_three"], "searched in the slugs' order");

    assert_eq!(gog_ids(&db, &cfg, &clients, &Log::default()).await.unwrap(), (3, 0), "only example_two is searched");
}

const COPIED: &str = "Honestly the best automation game I have ever played, it eats whole weekends without asking.";

fn review(text: &str, posted: i64) -> ReviewText {
    ReviewText {
        source: "steam".into(),
        positive: Some(true),
        rating: None,
        votes: 10,
        hours: Some(40),
        title: None,
        text: text.into(),
        posted: Some(posted),
    }
}

/// Three games of the mock library's, with reviews: Factorio's first
/// review is the one a summary copies.
fn summarizable() -> (tempfile::TempDir, Config, Db) {
    let (dir, cfg, db) = build_db();
    for (appid, name) in [(427520, "Factorio"), (383870, "Firewatch"), (4100, "Example Puzzle Game")] {
        add_game(&db, appid, name);
        let reviews = [
            review(
                if appid == 427520 { COPIED } else { "A lovely little game with a clever twist in every single chapter." },
                1_700_000_000,
            ),
            review("Short but sweet, and the soundtrack carries every quiet moment along with it.", 1_760_000_000),
        ];
        db.put_review_texts(appid, Some(&reviews)).unwrap();
    }
    db.setting_set(PlayerId::FIRST, "anthropic_api_key", Some("PLANTED-KEY")).unwrap();
    (dir, cfg, db)
}

/// The games listed in a request, by id and name.
fn listed(req: &crate::llm::LlmRequest) -> Vec<(i64, String)> {
    let line = regex::Regex::new(r"(?m)^### #(\d+) (.+)$").unwrap();
    line.captures_iter(&req.prompt).map(|c| (c[1].parse().unwrap(), c[2].to_string())).collect()
}

/// A reply with a summary for each game listed but `skip`; Factorio's
/// copies its review unless `own_words`.
fn reply(skip: &'static str, own_words: bool) -> Answer {
    Box::new(move |req| {
        assert_eq!(req.system.contains(OWN_WORDS), own_words, "the note goes only with a second try");
        let digests: Vec<Value> = listed(req)
            .into_iter()
            .filter(|(_, name)| name != skip)
            .map(|(id, name)| {
                let says = if name == "Factorio" && !own_words {
                    format!("{COPIED} Belts everywhere.")
                } else {
                    format!("{name} is tidy and clever, with a few rough edges near the end.")
                };
                json!({"id": id, "vibes": format!("{name} hums along like a well-oiled toy."), "digest": says,
                       "facets": ["sessions:long"], "praised": ["core loop"], "criticized": [], "divisive": [], "notes": []})
            })
            .collect();
        Ok(json!({ "digests": digests }))
    })
}

/// The summaries pass, once more for the title a reply left out, then
/// once more for the summary that copied a review, which is then clean.
/// The export has every game, only allowed fields, and applies to the mock
/// library with no AI call.
#[tokio::test]
async fn generate_then_export_then_apply() {
    let (dir, cfg, db) = summarizable();
    let clients = Clients::new(&cfg).unwrap();
    let llm = ScriptedLlm::new(vec![reply("Firewatch", false), reply("", false), reply("", true)]);
    let got = generate(&db, &cfg, &clients, &llm, None, &Log::default()).await.unwrap();
    assert_eq!((got.written, got.rewritten, got.unwritten, got.copying), (3, 1, 0, 0), "{got:?}");
    assert_eq!(llm.asked.lock().unwrap().len(), 3);
    assert_eq!(status(&db, &cfg).unwrap().unwrap().summaries, 3);

    write_file(&gog_ids_file(&cfg), br#"{"383870": "1207658890", "427520": null}"#).unwrap();
    let out = dir.path().join("release");
    let done = export(&db, &cfg, &out, "2026-10-09").unwrap();
    assert_eq!((done.manifest.count, done.unwritten, done.copying), (3, 0, 0));
    assert_eq!(done.gog_ids, Some(1), "Firewatch's");
    assert_eq!(done.data_file, out.join("upnext-seed-2026-10-09.jsonl.gz"));
    assert_eq!(done.manifest_file, out.join("upnext-seed.json"));

    // What the apply side reads: the manifest, its data file and the checks.
    let loaded = crate::seed::load(&crate::seed::client().unwrap(), &done.manifest_file.display().to_string()).await.unwrap();
    let firewatch = loaded.seed.entries.iter().find(|e| e.steam_appid == Some(383870)).unwrap();
    assert_eq!(firewatch.gog_id.as_deref(), Some("1207658890"));
    assert_eq!((firewatch.prompt_version, firewatch.model.as_str()), (DIGEST_VERSION, "scripted"));
    assert_eq!(firewatch.reviews_until.as_deref(), Some("2025-10-09"), "the newest review's day");
    assert_eq!(firewatch.written, chrono::Utc::now().format("%Y-%m-%d").to_string());
    assert!(firewatch.tags.aspects.iter().any(|a| a.aspect == "core loop"));
    let factorio = loaded.seed.entries.iter().find(|e| e.steam_appid == Some(427520)).unwrap();
    assert!(factorio.gog_id.is_none() && !factorio.people_say.contains("best automation game"));

    let mut text = String::new();
    std::io::Read::read_to_string(&mut flate2::read::GzDecoder::new(&std::fs::read(&done.data_file).unwrap()[..]), &mut text).unwrap();
    for planted in ["A lovely little game", "Short but sweet", "PLANTED DESCRIPTION", "PLANTED-KEY", COPIED] {
        assert!(!text.contains(planted), "{planted:?} is in the export");
    }
    for line in text.lines().skip(1) {
        let keys: Vec<String> = serde_json::from_str::<Value>(line).unwrap().as_object().unwrap().keys().cloned().collect();
        let allowed =
            ["steam_appid", "gog_id", "name", "people_say", "vibes", "tags", "prompt_version", "model", "written", "reviews_until"];
        assert!(keys.iter().all(|k| allowed.contains(&k.as_str())), "{keys:?}");
    }

    // Applied to the mock library, without its own summaries for these two.
    let lib = tempfile::tempdir().unwrap();
    let lib_cfg = offline_config(lib.path());
    let lib_db = Db::open(&lib_cfg.db_path, lib_cfg.sampled_minutes).unwrap();
    crate::mock::seed(&lib_db).unwrap();
    for g in lib_db.all_games(PlayerId::FIRST).unwrap().iter().filter(|g| g.name == "Factorio" || g.name == "Firewatch") {
        lib_db.delete_digest(g.id).unwrap();
    }
    let applied = crate::seed::apply(&lib_db, &loaded.seed).unwrap();
    assert_eq!((applied.matched, applied.written), (2, 2), "{applied:?}");
    let firewatch = lib_db.all_games(PlayerId::FIRST).unwrap().into_iter().find(|g| g.name == "Firewatch").unwrap();
    assert_eq!(firewatch.seed_date.as_deref(), Some("2026-10-09"));
    assert!(firewatch.digest.unwrap().starts_with("Firewatch is tidy"));
}

/// A summary that copies a review again on its second try is left out of
/// the export, and isn't asked for a third time.
#[tokio::test]
async fn a_summary_that_still_copies_is_left_out() {
    let (dir, cfg, db) = summarizable();
    let clients = Clients::new(&cfg).unwrap();
    let stubborn: Answer = Box::new(|req| {
        assert!(req.system.contains(OWN_WORDS));
        let (id, _) = listed(req).into_iter().next().unwrap();
        Ok(json!({"digests": [{"id": id, "vibes": "", "digest": format!("{COPIED} Again."),
            "facets": [], "praised": [], "criticized": [], "divisive": [], "notes": []}]}))
    });
    let llm = ScriptedLlm::new(vec![reply("", false), stubborn]);
    let got = generate(&db, &cfg, &clients, &llm, None, &Log::default()).await.unwrap();
    assert_eq!((got.written, got.rewritten, got.copying), (3, 1, 1), "{got:?}");

    let quiet = ScriptedLlm::new(vec![]);
    let again = generate(&db, &cfg, &clients, &quiet, None, &Log::default()).await.unwrap();
    assert_eq!((again.written, again.rewritten, again.copying), (0, 0, 1), "no call: the script is empty");

    let done = export(&db, &cfg, &dir.path().join("release"), "2026-10-09").unwrap();
    assert_eq!((done.manifest.count, done.copying), (2, 1));
    assert_eq!(done.gog_ids, None, "never fetched: the export says it has no GOG ids");
    assert!(export(&db, &cfg, &dir.path().join("release"), "October").is_err(), "a date that isn't one");
}

/// With a limit, one pass writes at most that many, a night's share.
#[tokio::test]
async fn a_limit_writes_a_share() {
    let (_dir, cfg, db) = summarizable();
    let clients = Clients::new(&cfg).unwrap();
    let llm = ScriptedLlm::new(vec![reply("", false), reply("", true)]);
    let got = generate(&db, &cfg, &clients, &llm, Some(2), &Log::default()).await.unwrap();
    assert_eq!((got.written, got.unwritten), (2, 1), "{got:?}");
    assert_eq!(listed(&llm.asked.lock().unwrap()[0]).len(), 2);
}
