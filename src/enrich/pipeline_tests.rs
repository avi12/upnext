//! Sync and enrich end to end: a temp database, and one local mock server
//! standing in for every outside service, answering with the fixtures under
//! `tests/fixtures/`. The library is Portal 2 on Steam (which the fixtures
//! describe) and The Witcher on GOG (which only GOG knows).

use super::*;
use crate::db::ImportedGame;
use crate::db::PlayerId;
use crate::testutil::{fixture_json, offline_config};
use serde_json::{Value, json};
use wiremock::matchers::{body_string_contains, method, path, query_param};
use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

const WITCHER: &str = "1207658924";

/// Keeps the log lines for assertions.
#[derive(Default)]
struct Log(std::sync::Mutex<Vec<String>>);

impl ProgressSink for Log {
    fn update(&self, _: &Progress) {}
    fn log(&self, line: String) {
        self.0.lock().unwrap().push(line);
    }
}

impl Log {
    fn text(&self) -> String {
        self.0.lock().unwrap().join("\n")
    }
}

/// A Wikidata request whose SPARQL contains the given text.
struct Sparql(&'static str);

impl Match for Sparql {
    fn matches(&self, req: &Request) -> bool {
        form_value(&req.body, "query").is_some_and(|q| q.contains(self.0))
    }
}

fn form_value(body: &[u8], key: &str) -> Option<String> {
    String::from_utf8_lossy(body).split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| percent_decode(&v.replace('+', " ")))
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The default options with these set.
fn with(options: &[(&str, bool)]) -> EnrichOptions {
    EnrichOptions { sources: options.iter().map(|(id, on)| (id.to_string(), *on)).collect(), ..Default::default() }
}

fn json_response(v: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(v)
}

async fn serve(server: &MockServer, http_method: &str, at: &str, body: Value) {
    Mock::given(method(http_method)).and(path(at)).respond_with(json_response(body)).mount(server).await;
}

/// Every service, answering from the fixtures. Anything not mounted gets a 404.
async fn services() -> MockServer {
    let s = MockServer::start().await;
    // Steam
    serve(&s, "GET", "/IPlayerService/GetOwnedGames/v1/", fixture_json("steam/owned_games.json")).await;
    serve(&s, "GET", "/ISteamUser/GetPlayerSummaries/v2/", fixture_json("steam/player_summary.json")).await;
    Mock::given(path("/api/appdetails"))
        .and(query_param("appids", "620"))
        .respond_with(json_response(fixture_json("steam/appdetails_620.json")))
        .mount(&s)
        .await;
    serve(&s, "GET", "/appreviews/620", fixture_json("steam/appreviews_620.json")).await;
    serve(&s, "GET", "/api/storesearch/", fixture_json("steam/storesearch_portal2.json")).await;
    serve(&s, "GET", "/IStoreBrowseService/GetItems/v1/", fixture_json("steam/getitems_620.json")).await;
    Mock::given(path("/api.php"))
        .and(query_param("appid", "620"))
        .respond_with(json_response(fixture_json("steamspy/appdetails_620.json")))
        .mount(&s)
        .await;
    serve(&s, "GET", "/api/v1/reports/summaries/620.json", fixture_json("protondb/summary_620.json")).await;
    // GOG
    serve(&s, "GET", &format!("/v2/games/{WITCHER}"), fixture_json("gog/v2_game.json")).await;
    serve(&s, "GET", &format!("/v1/products/{WITCHER}/reviews"), fixture_json("gog/reviews.json")).await;
    serve(&s, "GET", "/products/1207658930", json!({"images": {"logo2x": "//images.gog.example/portal.png"}})).await;
    Mock::given(path("/account/getFilteredProducts"))
        .and(query_param("page", "1"))
        .respond_with(json_response(fixture_json("gog/filtered_products.json")))
        .mount(&s)
        .await;
    Mock::given(path("/account/getFilteredProducts"))
        .and(query_param("page", "2"))
        .respond_with(json_response(json!({"page": 2, "totalPages": 2, "products": []})))
        .mount(&s)
        .await;
    serve(&s, "GET", &format!("/games/{WITCHER}/users/4800/sessions"), fixture_json("gog/sessions.json")).await;
    // IGDB and its Twitch token
    serve(&s, "POST", "/oauth2/token", json!({"access_token": "igdb-token", "expires_in": 5000000})).await;
    for (marker, fixture) in
        [("search ", "igdb/search.json"), ("fields id,videos", "igdb/trailers.json"), ("fields slug", "igdb/covers.json")]
    {
        Mock::given(path("/v4/games")).and(body_string_contains(marker)).respond_with(json_response(fixture_json(fixture))).mount(&s).await;
    }
    // RAWG: the genre list checks the key; the game by the slug Wikidata gives.
    serve(&s, "GET", "/api/genres", json!({"count": 1, "results": []})).await;
    serve(&s, "GET", "/api/games/portal-2", fixture_json("rawg/search.json")["results"][0].clone()).await;
    serve(&s, "GET", "/api/games", fixture_json("rawg/search.json")).await;
    // PCGamingWiki
    Mock::given(path("/w/api.php"))
        .and(query_param("action", "parse"))
        .and(query_param("page", "Portal 2"))
        .respond_with(json_response(fixture_json("pcgw/parse_page.json")))
        .mount(&s)
        .await;
    Mock::given(path("/w/api.php"))
        .and(query_param("action", "opensearch"))
        .respond_with(json_response(fixture_json("pcgw/opensearch_portal2.json")))
        .mount(&s)
        .await;
    // Wikidata
    for (query, fixture) in [
        ("VALUES ?steam", "wikidata/by_steam_appids.json"),
        ("VALUES ?name", "wikidata/by_names.json"),
        ("VALUES ?item", "wikidata/series_for.json"),
        ("VALUES ?l ", "wikidata/series_by_labels.json"),
        ("VALUES ?s ", "wikidata/series_members.json"),
    ] {
        Mock::given(path("/sparql")).and(Sparql(query)).respond_with(json_response(fixture_json(fixture))).mount(&s).await;
    }
    // HowLongToBeat: no search path in the (missing) home page, so `api/s`.
    serve(&s, "GET", "/api/s/init", json!({"token": "hltb-token"})).await;
    Mock::given(method("POST"))
        .and(path("/api/s"))
        .and(wiremock::matchers::header("x-auth-token", "hltb-token"))
        .respond_with(json_response(fixture_json("hltb/search.json")))
        .mount(&s)
        .await;
    s
}

struct Setup {
    _dir: tempfile::TempDir,
    db: Db,
    cfg: Config,
    clients: Clients,
    server: MockServer,
}

/// An empty library, every key set, and every service at the mock server.
async fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = offline_config(dir.path());
    cfg.steam_api_key = Some("0123456789ABCDEF0123456789ABCDEF".into());
    cfg.steam_id = Some("76561198000000000".into());
    cfg.igdb_client_id = Some("igdb-id".into());
    cfg.igdb_client_secret = Some("igdb-secret".into());
    cfg.rawg_api_key = Some("rawg-key".into());
    let server = services().await;
    let db = Db::open(&cfg.db_path, cfg.sampled_minutes).unwrap();
    let clients = Clients::with_base_url(&cfg, Some(&server.uri())).unwrap();
    Setup { _dir: dir, db, cfg, clients, server }
}

impl Setup {
    /// A Steam client folder that `find_steam_dir` accepts, with Portal 2 installed.
    fn steam_folder(&mut self) {
        let steam = self._dir.path().join("steam");
        std::fs::create_dir_all(steam.join("appcache")).unwrap();
        std::fs::create_dir_all(steam.join("config")).unwrap();
        std::fs::write(steam.join("appcache/appinfo.vdf"), b"").unwrap();
        std::fs::write(steam.join("config/loginusers.vdf"), b"").unwrap();
        std::fs::write(
            steam.join("config/libraryfolders.vdf"),
            "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\"/games\"\n\t\t\"apps\"\n\t\t{\n\t\t\t\"620\"\t\"1\"\n\t\t}\n\t}\n}\n",
        )
        .unwrap();
        self.cfg.steam_dir = Some(steam);
    }

    fn sign_in_to_gog(&self) {
        let token = crate::gog::GogToken {
            access_token: "gog-access".into(),
            refresh_token: "gog-refresh".into(),
            expires_at: crate::db::now() + 3600,
            user_id: "4800".into(),
            username: Some("tester".into()),
        };
        std::fs::write(&self.cfg.gog_token_path, serde_json::to_string(&token).unwrap()).unwrap();
    }

    /// Portal 2 on Steam (played) and The Witcher on GOG, as a sync leaves them.
    fn library(&self) {
        let game = |source: &str, id: &str, launcher: &str, name: &str, appid: Option<u32>, minutes: i64| ImportedGame {
            source: source.into(),
            source_id: id.into(),
            launcher: launcher.into(),
            name: name.into(),
            appid,
            playtime_minutes: Some(minutes),
            ..Default::default()
        };
        self.db.sync_source(PlayerId::FIRST, "steam", "steam", &[game("steam", "620", "steam", "Portal 2", Some(620), 1260)]).unwrap();
        self.db
            .sync_source(PlayerId::FIRST, "gog", "gog", &[game("gog", WITCHER, "gog", "The Witcher: Enhanced Edition", None, 0)])
            .unwrap();
    }

    fn game(&self, name: &str) -> GameView {
        self.db
            .all_games(PlayerId::FIRST)
            .unwrap()
            .into_iter()
            .find(|g| g.name == name)
            .unwrap_or_else(|| panic!("{name} not in the library"))
    }

    /// Paths the mock server has been asked for, in order.
    async fn requested(&self) -> Vec<String> {
        self.server.received_requests().await.unwrap_or_default().iter().map(|r| r.url.path().to_string()).collect()
    }
}

#[tokio::test]
async fn sync_reads_the_steam_api_and_the_gog_account() {
    let mut t = setup().await;
    t.steam_folder();
    t.sign_in_to_gog();
    let opts = SyncOptions { libraries: [("heroic".to_string(), false)].into(), ..Default::default() };
    let log = Log::default();
    let summaries = run_sync(&t.db, &t.cfg, &t.clients, &opts, &log).await.unwrap().summaries;
    assert_eq!(summaries.iter().map(|s| (s.total, s.added)).collect::<Vec<_>>(), [(3, 3), (1, 1)], "{}", log.text());

    let portal = t.game("Portal 2");
    assert_eq!((portal.appid, portal.playtime_minutes, portal.installed), (Some(620), 1260, true));
    assert!(!t.game("Portal").installed, "only what libraryfolders.vdf lists is installed");
    let witcher = t.game("The Witcher: Enhanced Edition");
    assert_eq!((witcher.playtime_minutes, witcher.last_played), (754, 1790000000), "GOG playtime from the sessions API");
    assert_eq!(t.db.profile(PlayerId::FIRST, Some("76561198000000000")).unwrap().persona_name.as_deref(), Some("Player One"));
}

/// A game hidden in the Steam client stays out,
/// unless `steam_include_hidden` is on.
#[tokio::test]
async fn sync_leaves_out_games_hidden_in_steam() {
    let mut t = setup().await;
    t.steam_folder();
    let config = t._dir.path().join("steam/userdata/39734272/config/cloudstorage");
    std::fs::create_dir_all(&config).unwrap();
    let hidden = json!([["user-collections.hidden", {"key": "user-collections.hidden", "value": json!({"added": [400]}).to_string()}]]);
    std::fs::write(config.join("cloud-storage-namespace-1.json"), hidden.to_string()).unwrap();
    let opts = SyncOptions { libraries: [("heroic".to_string(), false), ("gog".to_string(), false)].into(), ..Default::default() };
    let names = |t: &Setup| -> Vec<String> { t.db.all_games(PlayerId::FIRST).unwrap().into_iter().map(|g| g.name).collect() };

    let log = Log::default();
    run_sync(&t.db, &t.cfg, &t.clients, &opts, &log).await.unwrap();
    assert!(log.text().contains("Steam: left out 1 game hidden in Steam"), "{}", log.text());
    assert!(!names(&t).iter().any(|n| n == "Portal"), "{:?}", names(&t));

    t.cfg.steam_include_hidden = true;
    run_sync(&t.db, &t.cfg, &t.clients, &opts, &Log::default()).await.unwrap();
    assert!(names(&t).iter().any(|n| n == "Portal"), "back when hidden games are wanted");
}

/// The Epic Games Launcher's installs come in without an Epic
/// sign-in, and a game uninstalled later stays, as not installed.
#[tokio::test]
async fn sync_reads_the_epic_launchers_installs_and_keeps_uninstalled_games() {
    let mut t = setup().await;
    let manifests = t._dir.path().join("epic-manifests");
    std::fs::create_dir_all(&manifests).unwrap();
    for (file, app, name) in [("a.item", "SpaceApp", "Space Game"), ("b.item", "RaceApp", "Racer")] {
        let m = json!({ "AppName": app, "DisplayName": name, "CatalogNamespace": "ns", "CatalogItemId": format!("{app}-id"),
            "AppCategories": ["public", "games", "applications"], "bIsIncompleteInstall": false });
        std::fs::write(manifests.join(file), m.to_string()).unwrap();
    }
    t.cfg.epic_manifests = Some(manifests.clone());
    let opts = SyncOptions { libraries: ["steam", "heroic", "gog"].map(|l| (l.to_string(), false)).into(), ..Default::default() };
    let log = Log::default();
    let summaries = run_sync(&t.db, &t.cfg, &t.clients, &opts, &log).await.unwrap().summaries;
    assert_eq!(summaries.iter().map(|s| (s.source.as_str(), s.added)).collect::<Vec<_>>(), [("epic", 2)], "{}", log.text());
    let racer = t.game("Racer");
    assert_eq!((racer.installed, racer.copies[0].launcher.as_str()), (true, "epic"));
    assert_eq!(racer.copies[0].run_url, "com.epicgames.launcher://apps/ns%3ARaceApp-id%3ARaceApp?action=launch&silent=true");

    std::fs::remove_file(manifests.join("b.item")).unwrap();
    let log = Log::default();
    run_sync(&t.db, &t.cfg, &t.clients, &opts, &log).await.unwrap();
    assert!(log.text().contains("Epic: 2 games (0 new, 2 updated, 0 removed)"), "{}", log.text());
    assert!(!t.game("Racer").installed, "uninstalled, and still owned");
    assert!(t.game("Space Game").installed);
    assert!(t.requested().await.iter().all(|p| !p.starts_with("/library/api")), "no Epic account request without a sign-in");
}

/// The code-health review's B-STORES-2: an Epic catalog item that keeps failing no
/// longer fails the whole library read. The others come in, the catalog read
/// before it is kept, and a game the library already has stays rather than
/// being removed until the catalog answers.
#[tokio::test]
async fn one_failing_epic_catalog_item_leaves_the_rest_of_the_library() {
    use crate::signin::SignInStore;
    let t = setup().await;
    let token = json!({ "access_token": "epic-access", "refresh_token": "epic-refresh", "expires_at": crate::db::now() + 3600,
        "refresh_expires_at": 0, "account_id": "acct", "display_name": null });
    crate::signin::DbSignIn::own(t.cfg.db_path.clone(), "epic", PlayerId::FIRST, None).save(&token.to_string()).unwrap();
    let rec = |ns: &str, item: &str, app: &str| json!({ "namespace": ns, "catalogItemId": item, "appName": app, "sandboxType": "PUBLIC" });
    let records =
        json!({ "records": [rec("ns1", "a1", "AppA"), rec("ns2", "b1", "AppB"), rec("ns3", "c1", "AppC")], "responseMetadata": {} });
    Mock::given(path("/library/api/public/items")).respond_with(json_response(records)).with_priority(1).mount(&t.server).await;
    let item = |id: &str, title: &str| json!({ id: { "id": id, "title": title, "categories": [{ "path": "games" }, { "path": "applications" }] } });
    for (ns, id, title) in [("ns1", "a1", "Game A"), ("ns3", "c1", "Game C")] {
        Mock::given(path(format!("/catalog/api/shared/namespace/{ns}/bulk/items")))
            .respond_with(json_response(item(id, title)))
            .with_priority(1)
            .mount(&t.server)
            .await;
    }
    Mock::given(path("/catalog/api/shared/namespace/ns2/bulk/items"))
        .respond_with(ResponseTemplate::new(500))
        .with_priority(1)
        .mount(&t.server)
        .await;
    let b = ImportedGame {
        source: "epic".into(),
        source_id: "AppB".into(),
        launcher: "epic".into(),
        name: "Game B".into(),
        ..Default::default()
    };
    t.db.sync_source(PlayerId::FIRST, "epic", "epic", &[b]).unwrap();

    let opts = SyncOptions { libraries: ["steam", "heroic", "gog"].map(|l| (l.to_string(), false)).into(), ..Default::default() };
    let log = Log::default();
    run_sync(&t.db, &t.cfg, &t.clients, &opts, &log).await.unwrap();
    let text = log.text();
    assert!(text.contains("1 game not read from Epic's catalog this time (kept as before)"), "{text}");
    assert!(text.contains("Epic: 3 games (2 new, 1 updated, 0 removed)"), "{text}");
    for name in ["Game A", "Game B", "Game C"] {
        t.game(name);
    }
    let kept = std::fs::read_to_string(t.cfg.cache_dir.join("epic-catalog.json")).unwrap();
    assert!(kept.contains("Game A") && kept.contains("Game C") && !kept.contains("b1"), "{kept}");
}

/// No EA game is installed in a test (the registry is never read), so a
/// sync that read some before marks them uninstalled and keeps them, and the
/// games added by hand for the same store stay as they are.
#[tokio::test]
async fn the_ea_apps_games_stay_owned_once_uninstalled() {
    let t = setup().await;
    let opts = SyncOptions { libraries: ["steam", "heroic", "gog", "epic"].map(|l| (l.to_string(), false)).into(), ..Default::default() };
    let log = Log::default();
    assert!(run_sync(&t.db, &t.cfg, &t.clients, &opts, &log).await.unwrap().summaries.is_empty(), "nothing to read, nothing read before");
    let bf = crate::ea::copy(&crate::ea::EaInstall { id: "1026023".into(), name: "Battlefield 1".into() });
    t.db.sync_source(PlayerId::FIRST, "origin", "origin", &[bf]).unwrap();
    crate::manual::save(&t.db, PlayerId::FIRST, "origin", &["Dead Space".to_string()]).unwrap();
    assert!(t.game("Battlefield 1").installed);
    let log = Log::default();
    run_sync(&t.db, &t.cfg, &t.clients, &opts, &log).await.unwrap();
    assert!(log.text().contains("EA app: 1 game (0 new, 1 updated, 0 removed)"), "{}", log.text());
    assert!(!t.game("Battlefield 1").installed, "uninstalled, and still owned");
    assert_eq!(t.game("Dead Space").copies[0].launcher, "manual");
}

/// As for the EA app: Ubisoft Connect's games read before stay, uninstalled,
/// with their launch links.
#[tokio::test]
async fn ubisoft_connects_games_stay_owned_once_uninstalled() {
    let t = setup().await;
    let opts =
        SyncOptions { libraries: ["steam", "heroic", "gog", "epic", "ea"].map(|l| (l.to_string(), false)).into(), ..Default::default() };
    let fc = crate::ubisoft::copy(&crate::ubisoft::UbisoftInstall { id: "635".into(), name: "Far Cry 5".into() });
    t.db.sync_source(PlayerId::FIRST, "uplay", "uplay", &[fc]).unwrap();
    let log = Log::default();
    run_sync(&t.db, &t.cfg, &t.clients, &opts, &log).await.unwrap();
    assert!(log.text().contains("Ubisoft Connect: 1 game (0 new, 1 updated, 0 removed)"), "{}", log.text());
    let g = t.game("Far Cry 5");
    assert_eq!((g.installed, g.copies[0].run_url.as_str()), (false, "uplay://launch/635/0"));
}

/// A job's summary counts one of something in the singular: "1 second
/// chance", not "1 second chances", and every id still reads as before for
/// other counts.
#[test]
fn a_summary_counts_one_in_the_singular() {
    let summary = |counts: &[(&'static str, usize)]| EnrichSummary { counts: counts.iter().copied().collect(), ..Default::default() };
    assert_eq!(summary(&[("second_chances", 1)]).summary_text(), "1 second chance");
    assert_eq!(summary(&[("digests", 1), ("series_guides", 2)]).summary_text(), "1 digest, 2 series guides");
    assert_eq!(summary(&[("series", 1), ("wikidata", 1)]).summary_text(), "1 wikidata, 1 series");
    let mut many = summary(&[("reviews", 1_500), ("review_texts", 1), ("hidden_gems", 1)]);
    many.processed = 1_600;
    assert_eq!(many.summary_text(), "1,600 titles: 1,500 reviews, 1 review text, 1 hidden gem");
    let counts = summary(&[("tags", 1)]).counts_text();
    assert!(counts.contains(", 1 tag,") && counts.contains(", 0 reviews,") && counts.contains(", 0 series,"), "{counts}");
}

/// The counts outside a source's id read in the singular for one too: a
/// title, its details and an error in the job's summary, and a game in the
/// sync's summary and its log lines.
#[test]
fn one_title_detail_error_and_game_read_in_the_singular() {
    let one = EnrichSummary { processed: 1, details: 1, errors: 1, ..Default::default() };
    assert_eq!(one.summary_text(), "1 title: 1 detail, 1 error");
    let counts = one.counts_text();
    assert!(counts.starts_with("0 matched, 1 detail, ") && counts.ends_with(", 1 error"), "{counts}");
    let two = EnrichSummary { processed: 2, details: 2, errors: 2, ..Default::default() };
    assert_eq!(two.summary_text(), "2 titles: 2 details, 2 errors");
    let stopped = EnrichSummary { processed: 5, errors: 3, stopped: vec!["SteamSpy", "IGDB"], ..Default::default() };
    assert_eq!(stopped.summary_text(), "5 titles: 3 errors; stopped early: SteamSpy, IGDB");
    let synced = |total| crate::db::SyncSummary { total, ..Default::default() };
    assert_eq!(super::sync::counts_text(&[synced(1)]), "1 game, 0 new");
    assert_eq!(super::sync::counts_text(&[synced(1), synced(1_999)]), "2,000 games, 0 new");
    assert_eq!((plural(1, "title needs", "titles need"), plural(-3i64, "game", "games")), ("1 title needs".into(), "0 games".into()));
}

#[test]
fn sync_options_json_keeps_its_shape() {
    // What the Settings page has always sent.
    let o: SyncOptions = serde_json::from_str(r#"{"steam": true, "heroic": false, "gog": true, "galaxy_db": null}"#).unwrap();
    assert!(o.on("steam") && !o.on("heroic") && o.on("galaxy"), "a library left out is on");
    assert_eq!((o.steam_local, o.galaxy_db.as_deref()), (false, None));
    let resolved = serde_json::to_value(o.resolved()).unwrap();
    assert_eq!(
        resolved,
        json!({"ea": true, "ubisoft": true, "epic": true, "steam": true, "heroic": false, "gog": true, "galaxy": true, "steam_local": false, "galaxy_db": null})
    );
}

#[tokio::test]
async fn sync_skips_what_is_off_or_not_ready() {
    let mut t = setup().await;
    t.steam_folder();
    // Only Steam is on; GOG isn't signed in and Galaxy has no path, so they'd be skipped anyway.
    let opts = SyncOptions { libraries: [("heroic".to_string(), false)].into(), ..Default::default() };
    let summaries = run_sync(&t.db, &t.cfg, &t.clients, &opts, &Log::default()).await.unwrap().summaries;
    assert_eq!(summaries.len(), 1, "Steam only");
    let requested = t.requested().await;
    assert!(requested.iter().all(|p| !p.contains("getFilteredProducts")), "no GOG request without a sign-in: {requested:?}");
}

/// A damaged Steam cache skips Steam with the reason
/// in the log, and the sync goes on to the other libraries.
#[tokio::test]
async fn a_damaged_steam_cache_skips_steam_only() {
    let mut t = setup().await;
    t.cfg.steam_dir = Some(crate::testutil::damaged_steam_folder(t._dir.path()));
    let opts = SyncOptions { steam_local: true, ..Default::default() };
    let log = Log::default();
    let summaries = run_sync(&t.db, &t.cfg, &t.clients, &opts, &log).await.unwrap_or_else(|e| panic!("{e:#}\n{}", log.text())).summaries;
    assert!(summaries.is_empty(), "{}", log.text());
    let text = log.text();
    assert!(text.contains("Steam skipped: couldn't read the Steam client's cache"), "{text}");
    assert!(text.contains("claims 4294967295 strings"), "{text}");
}

/// The code-health review's B-STORES-6: a Web API key Steam turns down is asked once,
/// not five times with backoff, and the sync reads the local Steam client
/// instead, as it does with no key.
#[tokio::test]
async fn a_refused_steam_key_falls_back_to_the_local_client() {
    let mut t = setup().await;
    t.cfg.steam_dir = Some(crate::testutil::damaged_steam_folder(t._dir.path()));
    Mock::given(path("/IPlayerService/GetOwnedGames/v1/"))
        .respond_with(ResponseTemplate::new(403).set_body_string("<html><body>Access is denied. Retrying will not help.</body></html>"))
        .with_priority(1)
        .mount(&t.server)
        .await;
    let log = Log::default();
    run_sync(&t.db, &t.cfg, &t.clients, &only(&["steam"]), &log).await.unwrap_or_else(|e| panic!("{e:#}\n{}", log.text()));
    let text = log.text();
    assert!(
        text.contains("Steam Web API not usable (Steam turned the Web API key down); reading the local Steam client instead"),
        "{text}"
    );
    assert!(text.contains("Steam skipped: couldn't read the Steam client's cache"), "the local client was read: {text}");
    let asked = t.requested().await.iter().filter(|p| p.contains("GetOwnedGames")).count();
    assert_eq!(asked, 1, "{text}");
}

/// A Galaxy sync lists every
/// integration with when Galaxy last added to it, removes the copies of
/// one the setting leaves out, and keeps the copies of one that vanished
/// from Galaxy (its plugins can disappear).
#[tokio::test]
async fn a_galaxy_sync_reports_and_leaves_out_integrations() {
    let mut t = setup().await;
    let path = t._dir.path().join("galaxy-2.0.db");
    let galaxy = rusqlite::Connection::open(&path).unwrap();
    galaxy.execute_batch(&crate::testutil::fixture("galaxy/galaxy-2.0-2026.sql")).unwrap();
    let opts = SyncOptions {
        libraries: [("steam", false), ("heroic", false), ("gog", false)].map(|(k, v)| (k.to_string(), v)).into(),
        galaxy_db: Some(path.display().to_string()),
        ..Default::default()
    };
    let owned = |t: &Setup, source: &str| {
        t.db.all_games(PlayerId::FIRST).unwrap().iter().filter(|g| g.owned_on.iter().any(|o| o == source)).count()
    };

    let log = Log::default();
    run_sync(&t.db, &t.cfg, &t.clients, &opts, &log).await.unwrap_or_else(|e| panic!("{e:#}\n{}", log.text()));
    let text = log.text();
    assert!(text.contains("Galaxy: EA app: 1 game, last added 2024-05-18"), "{text}");
    assert!(text.contains("Galaxy: left out 2 not owned (family shared or a subscription) and 3 betas, test servers and demos"), "{text}");
    assert_eq!((owned(&t, "origin"), owned(&t, "xboxone")), (1, 1));
    let seen = crate::enrich::sync::galaxy::seen_integrations(&t.db);
    assert_eq!(seen.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["GOG", "Steam", "Epic", "EA app", "Ubisoft Connect", "Xbox"]);

    // Left out in Settings: its copies go. Gone from Galaxy: they stay.
    t.cfg.galaxy_skip = Some("origin".into());
    galaxy.execute_batch("DELETE FROM LibraryReleases WHERE releaseKey LIKE 'xboxone%';").unwrap();
    let log = Log::default();
    run_sync(&t.db, &t.cfg, &t.clients, &opts, &log).await.unwrap_or_else(|e| panic!("{e:#}\n{}", log.text()));
    let text = log.text();
    assert!(text.contains("Galaxy: removed 1 EA app copy, left out in Settings"), "{text}");
    assert!(text.contains("Galaxy: no Xbox games in Galaxy now; the 1 copy read before is kept"), "{text}");
    assert_eq!((owned(&t, "origin"), owned(&t, "xboxone")), (0, 1));
    let seen = crate::enrich::sync::galaxy::seen_integrations(&t.db);
    let xbox = seen.iter().find(|s| s.integration.platform == "xboxone").expect("still listed, so it can be left out");
    let ea = seen.iter().find(|s| s.integration.platform == "origin").unwrap();
    assert!(ea.integration.left_out && !xbox.integration.left_out);
    assert!(xbox.seen_at <= ea.seen_at);
}

#[tokio::test]
async fn enrich_fills_every_source() {
    let t = setup().await;
    t.library();
    let opts = with(&[("hltb", true)]);
    let log = Log::default();
    let s = run_enrich(&t.db, &t.cfg, &t.clients, None, &opts, &log).await.unwrap_or_else(|e| panic!("{e:#}\n{}", log.text()));
    let counts = [
        ("processed", s.processed),
        ("matches", s.matches),
        ("details", s.details),
        ("reviews", s.count("reviews")),
        ("review_texts", s.count("review_texts")),
        ("tags", s.count("tags")),
        ("protondb", s.count("protondb")),
        ("gog_meta", s.count("gog_meta")),
        ("igdb", s.count("igdb")),
        ("rawg", s.count("rawg")),
        ("pcgw", s.count("pcgw")),
        ("hltb", s.count("hltb")),
        ("wikidata", s.count("wikidata")),
        ("series", s.count("series")),
        ("trailers", s.count("trailers")),
        ("art", s.count("art")),
        ("errors", s.errors),
    ];
    let expected = [
        ("processed", 2),
        ("matches", 0),
        ("details", 1),
        ("reviews", 1),
        ("review_texts", 1),
        ("tags", 1),
        ("protondb", 0),
        ("gog_meta", 1),
        ("igdb", 2),
        ("rawg", 2),
        ("pcgw", 2),
        ("hltb", 2),
        ("wikidata", 1),
        ("series", 1),
        ("trailers", 1),
        ("art", 1),
        ("errors", 0),
    ];
    assert_eq!(counts, expected, "{}", log.text());

    let portal = t.game("Portal 2");
    assert_eq!(portal.type_.as_deref(), Some("game"), "Steam details");
    assert!(portal.review_pct.is_some() && !portal.review_texts.is_empty(), "Steam reviews");
    assert!(portal.tags_by_source.contains_key("steam"), "SteamSpy tags");
    assert_eq!((portal.linux.as_deref(), portal.protondb_tier.as_deref()), (Some("native"), None), "a Linux build, so no ProtonDB");
    assert_eq!(portal.wikidata_url.as_deref(), Some("https://www.wikidata.org/wiki/Q279446"), "Wikidata");
    assert_eq!(portal.series, ["Portal"], "Wikidata series");
    assert!(portal.tags_by_source.contains_key("igdb") && portal.trailer_url.is_some(), "IGDB and its trailer");
    assert_eq!(portal.rawg_url.as_deref(), Some("https://rawg.io/games/portal-2"), "RAWG by the slug from Wikidata");
    assert!(portal.pcgw_url.is_some() && portal.tags_by_source.contains_key("pcgw"), "PCGamingWiki");
    assert_eq!(portal.hltb_main, Some(8.6), "HowLongToBeat, in hours to one decimal");
    assert!(portal.portrait_url.is_some() && !portal.screenshots.is_empty(), "Steam art");

    let witcher = t.game("The Witcher: Enhanced Edition");
    assert!(witcher.has_match_attempt && witcher.appid.is_none(), "no Steam game by that name");
    assert!(witcher.gog_rating.is_some() && witcher.tags_by_source.contains_key("gog"), "GOG store and reviews");

    let unowned: Vec<_> = t.db.series_catalog().unwrap().into_values().flatten().filter(|e| e.name == "Portal").collect();
    assert_eq!(unowned.len(), 1, "the series list has the Portal you don't own");
    assert_eq!(unowned[0].cover.as_deref(), Some("https://images.gog.example/portal.png"), "its cover from GOG");
}

/// Wikidata gives next to no series game a GOG product id, so one with
/// only its GOG store page gets the id from GOG's catalogue, by the page's
/// slug, and its cover from GOG by that id.
#[tokio::test]
async fn a_series_game_with_only_a_gog_store_page_gets_its_gog_cover() {
    let t = setup().await;
    t.library();
    let mut members = fixture_json("wikidata/series_members.json");
    for row in members["results"]["bindings"].as_array_mut().unwrap() {
        row.as_object_mut().unwrap().remove("gogid");
    }
    Mock::given(path("/sparql")).and(Sparql("VALUES ?s ")).respond_with(json_response(members)).with_priority(1).mount(&t.server).await;
    Mock::given(path("/v1/catalog"))
        .and(query_param("query", "like:portal"))
        .respond_with(json_response(
            json!({"products": [{"id": "1207658930", "slug": "portal"}, {"id": "1207658931", "slug": "portal_2"}]}),
        ))
        .expect(1)
        .mount(&t.server)
        .await;
    let log = Log::default();
    run_enrich(&t.db, &t.cfg, &t.clients, None, &EnrichOptions::default(), &log).await.unwrap_or_else(|e| panic!("{e:#}\n{}", log.text()));
    let unowned: Vec<_> = t.db.series_catalog().unwrap().into_values().flatten().filter(|e| e.name == "Portal").collect();
    assert_eq!((unowned[0].gog_id.as_deref(), unowned[0].gog_slug.as_deref()), (None, Some("portal")));
    assert_eq!(unowned[0].cover.as_deref(), Some("https://images.gog.example/portal.png"), "{}", log.text());
}

#[tokio::test]
async fn protondb_is_asked_about_games_without_a_linux_build() {
    let t = setup().await;
    t.library();
    let mut details = fixture_json("steam/appdetails_620.json");
    details["620"]["data"]["platforms"]["linux"] = json!(false);
    // Answers ahead of the fixture as it is (priority 1 beats the default 5).
    Mock::given(path("/api/appdetails")).respond_with(json_response(details)).with_priority(1).mount(&t.server).await;
    let log = Log::default();
    let s = run_enrich(&t.db, &t.cfg, &t.clients, None, &EnrichOptions::default(), &log).await.unwrap();
    assert_eq!((s.count("protondb"), s.errors), (1, 0), "{}", log.text());
    let portal = t.game("Portal 2");
    assert_eq!(portal.linux_native, Some(false));
    assert!(portal.protondb_tier.is_some() && portal.linux == portal.protondb_tier, "{:?}", portal.linux);
}

#[tokio::test]
async fn enrich_again_fetches_nothing_new() {
    let t = setup().await;
    t.library();
    let opts = with(&[("hltb", true)]);
    let log = Log::default();
    run_enrich(&t.db, &t.cfg, &t.clients, None, &opts, &log).await.unwrap();
    let before = t.requested().await.len();

    let again = run_enrich(&t.db, &t.cfg, &t.clients, None, &opts, &log).await.unwrap();
    assert_eq!(again.processed, 0);
    let new: Vec<String> = t.requested().await.split_off(before);
    assert_eq!(new, ["/oauth2/token", "/api/genres"], "only the IGDB and RAWG key checks, which run every time");
}

/// `--only review_texts --refresh --titles` fetches the reviews again for the
/// titles asked for, both Steam rankings, and nothing else: not the store
/// pages, and not the other titles.
#[tokio::test]
async fn a_narrowed_refresh_fetches_only_what_was_asked_for() {
    let t = setup().await;
    t.library();
    let log = Log::default();
    run_enrich(&t.db, &t.cfg, &t.clients, None, &EnrichOptions::default(), &log).await.unwrap();
    let before = t.requested().await.len();

    let portal = t.game("Portal 2");
    let mut sources: BTreeMap<String, bool> = crate::cli::sync::source_ids().iter().map(|id| (id.to_string(), false)).collect();
    sources.insert("review_texts".into(), true);
    let opts = EnrichOptions { sources, refresh: true, limit: None, titles: vec![portal.id] };
    let s = run_enrich(&t.db, &t.cfg, &t.clients, None, &opts, &log).await.unwrap();
    assert_eq!((s.processed, s.count("review_texts"), s.details, s.errors), (1, 1, 0, 0), "{}", log.text());
    let new: Vec<String> = t.requested().await.split_off(before);
    assert_eq!(new, ["/appreviews/620", "/appreviews/620"], "the last month's ranking and the all-time one, no store page");
}

/// `--with digests` rewrites summaries written with an older prompt, and
/// leaves the current ones alone.
#[tokio::test]
async fn digests_rewrite_only_outdated_summaries() {
    let t = setup().await;
    t.library();
    let log = Log::default();
    run_enrich(&t.db, &t.cfg, &t.clients, None, &EnrichOptions::default(), &log).await.unwrap();
    let (portal, witcher) = (t.game("Portal 2"), t.game("The Witcher: Enhanced Edition"));
    assert!(!portal.review_texts.is_empty() && !witcher.review_texts.is_empty(), "both have reviews");
    t.db.put_digest(portal.id, "Old summary.", "", &Default::default(), "claude-sonnet-5", 1).unwrap();
    t.db.put_digest(witcher.id, "Current summary.", "", &Default::default(), "claude-sonnet-5-5", crate::digest::DIGEST_VERSION).unwrap();

    let reply = json!({"digests": [{"id": portal.id, "digest": "New summary.", "facets": ["difficulty:moderate", "difficulty:hard", "mood:grim"],
        "praised": ["puzzles"], "criticized": [], "divisive": [], "notes": []}]});
    let completion = json!({"model": "fake-model", "choices": [{"finish_reason": "stop", "message": {"role": "assistant", "content": reply.to_string()}}]});
    Mock::given(method("POST")).and(path("/v1/chat/completions")).respond_with(json_response(completion)).expect(1).mount(&t.server).await;
    let llm = crate::llm::openai_compat::OpenAiCompatBackend::new(format!("{}/v1", t.server.uri()), Some("fake-model".into()), None);

    let s = run_enrich(&t.db, &t.cfg, &t.clients, Some(&llm), &with(&[("digests", true)]), &log).await.unwrap();
    assert_eq!((s.count("digests"), s.errors), (1, 0), "{}", log.text());
    let requests = t.server.received_requests().await.unwrap();
    let asked = requests.iter().find(|r| r.url.path() == "/v1/chat/completions").unwrap();
    let body = String::from_utf8_lossy(&asked.body);
    assert!(body.contains("Portal 2") && !body.contains("The Witcher"), "only the outdated summary is rewritten");

    let portal = t.game("Portal 2");
    assert_eq!((portal.digest.as_deref(), portal.digest_version), (Some("New summary."), Some(crate::digest::DIGEST_VERSION)));
    let tags = portal.review_tags.unwrap();
    assert_eq!((tags.scales["difficulty"].as_str(), tags.aspects[0].aspect.as_str()), ("moderate", "puzzles"));
    assert_eq!(t.game("The Witcher: Enhanced Edition").digest.as_deref(), Some("Current summary."));
    assert!(log.text().contains("review tags dropped: 1 outside the vocabulary, 1 over the caps"), "{}", log.text());
}

/// With a summaries backend of its own, the summaries go there, with
/// that backend's summaries model, and the picks' backend gets no call.
#[tokio::test]
async fn summaries_go_to_their_own_backend() {
    let mut t = setup().await;
    t.library();
    run_enrich(&t.db, &t.cfg, &t.clients, None, &EnrichOptions::default(), &Log::default()).await.unwrap();
    let portal = t.game("Portal 2");
    t.db.put_digest(t.game("The Witcher: Enhanced Edition").id, "Current.", "", &Default::default(), "m", crate::digest::DIGEST_VERSION)
        .unwrap();
    t.cfg.digest_backend = "openai".into();
    t.cfg.openai_base_url = format!("{}/v1", t.server.uri());
    t.cfg.openai_model = Some("home-picks-model".into());
    t.cfg.openai_digest_model = Some("home-summaries-model".into());

    let reply = json!({"digests": [{"id": portal.id, "digest": "From home.", "facets": [], "praised": [], "criticized": [], "divisive": [], "notes": []}]});
    let completion = json!({"model": "home-summaries-model", "choices": [{"finish_reason": "stop", "message": {"role": "assistant", "content": reply.to_string()}}]});
    Mock::given(method("POST")).and(path("/v1/chat/completions")).respond_with(json_response(completion)).expect(1).mount(&t.server).await;
    // The picks' backend: any call to it panics, as the script is empty.
    let picks = crate::testutil::ScriptedLlm::new(vec![]);

    let log = Log::default();
    let s = run_enrich(&t.db, &t.cfg, &t.clients, Some(&picks), &with(&[("digests", true)]), &log).await.unwrap();
    assert_eq!((s.count("digests"), s.errors), (1, 0), "{}", log.text());
    assert!(picks.asked.lock().unwrap().is_empty(), "the picks' backend wasn't asked");
    let requests = t.server.received_requests().await.unwrap();
    let asked = requests.iter().find(|r| r.url.path() == "/v1/chat/completions").unwrap();
    let body: Value = serde_json::from_slice(&asked.body).unwrap();
    assert_eq!(body["model"], "home-summaries-model");
    assert_eq!(t.game("Portal 2").digest.as_deref(), Some("From home."));
    let calls = t.db.ai_usage_rows(0).unwrap();
    assert!(!calls.is_empty() && calls.iter().all(|r| r.backend == "openai"), "recorded against the summaries' backend");

    // Without the AI, neither runs.
    let s = run_enrich(&t.db, &t.cfg, &t.clients, None, &with(&[("digests", true)]), &log).await.unwrap();
    assert_eq!(s.count("digests"), 0);
}

/// Inside a job, each summary call is recorded with the job and the titles
/// it covered, the job shows its tokens, and its log says what they came to.
#[tokio::test]
async fn a_summary_run_records_its_calls_in_its_job() {
    let t = setup().await;
    t.library();
    run_enrich(&t.db, &t.cfg, &t.clients, None, &EnrichOptions::default(), &Log::default()).await.unwrap();
    let (portal, witcher) = (t.game("Portal 2"), t.game("The Witcher: Enhanced Edition"));
    let item =
        |id: i64| json!({"id": id, "digest": "A summary.", "facets": [], "praised": [], "criticized": [], "divisive": [], "notes": []});
    let reply = json!({"digests": [item(portal.id), item(witcher.id)]});
    let completion = json!({"model": "fake-model", "usage": {"prompt_tokens": 9000, "completion_tokens": 1200},
        "choices": [{"finish_reason": "stop", "message": {"role": "assistant", "content": reply.to_string()}}]});
    Mock::given(method("POST")).and(path("/v1/chat/completions")).respond_with(json_response(completion)).expect(1).mount(&t.server).await;
    let llm = crate::llm::openai_compat::OpenAiCompatBackend::new(format!("{}/v1", t.server.uri()), Some("fake-model".into()), None);

    let shared = std::sync::Arc::new(Db::open(&t.cfg.db_path, t.cfg.sampled_minutes).unwrap());
    let job = crate::jobs::LiveJob::begin(shared, "enrich", None, crate::db::JobSource::Terminal, json!({}), None).unwrap().unwrap();
    let opts = with(&[("digests", true)]);
    let s = job.scope(run_enrich(&t.db, &t.cfg, &t.clients, Some(&llm), &opts, job.as_ref())).await.unwrap();
    assert_eq!(s.count("digests"), 2);
    job.finish(Ok("done".into()));

    let done = t.db.job(job.id).unwrap().unwrap();
    let used = done.ai_usage.expect("the job's AI usage");
    assert_eq!((used.totals.calls, used.totals.items), (1, 2), "one call for both titles");
    assert_eq!((used.totals.usage.input_tokens, used.totals.usage.output_tokens), (9000, 1200));
    let log: Vec<String> = t.db.job_log(job.id, 0, crate::db::Level::Info, 500).unwrap().into_iter().map(|l| l.line.message).collect();
    assert!(log.iter().any(|l| l == "1 call used 9.0k tokens in and 1.2k out"), "{log:#?}");
}

/// An OpenAI-compatible server that stops answering: a cancel ends the job
/// in moments, not when the client's 15-minute timeout runs out.
#[tokio::test]
async fn a_cancel_doesnt_wait_for_a_slow_ai_server() {
    let t = setup().await;
    t.library();
    run_enrich(&t.db, &t.cfg, &t.clients, None, &EnrichOptions::default(), &Log::default()).await.unwrap();
    let completion =
        json!({"model": "fake-model", "choices": [{"finish_reason": "stop", "message": {"role": "assistant", "content": "{}"}}]});
    let slow = json_response(completion).set_delay(std::time::Duration::from_secs(600));
    Mock::given(method("POST")).and(path("/v1/chat/completions")).respond_with(slow).mount(&t.server).await;
    let llm = crate::llm::openai_compat::OpenAiCompatBackend::new(format!("{}/v1", t.server.uri()), Some("fake-model".into()), None);

    let (db, job) = job_on(&t);
    let opts = with(&[("digests", true)]);
    let run = job.scope(run_enrich(&t.db, &t.cfg, &t.clients, Some(&llm), &opts, job.as_ref()));
    let cancel = async {
        while !t.requested().await.iter().any(|p| p == "/v1/chat/completions") {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        job.cancel_now();
        std::time::Instant::now()
    };
    let ended = tokio::time::timeout(std::time::Duration::from_secs(30), async { tokio::join!(run, cancel) });
    let (result, cancelled_at) = ended.await.expect("the run ended after the cancel");
    assert!(cancelled_at.elapsed() < std::time::Duration::from_secs(5), "{:?}", cancelled_at.elapsed());
    let err = result.expect_err("the run stopped");
    assert_eq!(job.finish(Err(&err)), crate::db::JobStatus::Cancelled);
    assert_eq!(db.job(job.id).unwrap().unwrap().status, crate::db::JobStatus::Cancelled);
    assert_eq!(db.job_failures(job.id).unwrap().len(), 0, "a call cut short by the cancel isn't a failure");
    assert!(t.game("Portal 2").digest.is_none());
}

/// A 401 from RAWG means the key was revoked: the lane stops at once instead
/// of failing on every title until its failure limit.
#[tokio::test]
async fn rawg_stops_at_once_when_its_key_is_turned_down() {
    let t = setup().await;
    let games: Vec<ImportedGame> = ["Alpha Game", "Beta Game", "Gamma Game"]
        .iter()
        .enumerate()
        .map(|(i, name)| ImportedGame {
            source: "gog".into(),
            source_id: format!("{}", 100 + i),
            launcher: "gog".into(),
            name: name.to_string(),
            ..Default::default()
        })
        .collect();
    t.db.sync_source(PlayerId::FIRST, "gog", "gog", &games).unwrap();
    // The key passes the check at the start, then RAWG turns it down.
    Mock::given(path("/api/games")).respond_with(ResponseTemplate::new(401)).with_priority(1).mount(&t.server).await;
    let only_rawg: Vec<(&str, bool)> = crate::enrich::sources::ids().into_iter().map(|id| (id, id == "rawg")).collect();
    let log = Log::default();
    let s = run_enrich(&t.db, &t.cfg, &t.clients, None, &with(&only_rawg), &log).await.unwrap();
    let searches = t.requested().await.iter().filter(|p| *p == "/api/games").count();
    assert_eq!((searches, s.errors), (1, 1), "{}", log.text());
    assert!(log.text().contains("RAWG stopped for the rest of this run"), "{}", log.text());
}

/// Games on one store, `id` and `appid` counting up from `first` (Steam
/// copies get the appid), played for `minutes` and last on `last_played`.
fn store_games(t: &Setup, store: &str, names: &[&str], first: u32, minutes: i64, last_played: i64) {
    let games: Vec<ImportedGame> = names
        .iter()
        .zip(first..)
        .map(|(name, id)| ImportedGame {
            source: store.into(),
            source_id: id.to_string(),
            launcher: store.into(),
            name: name.to_string(),
            appid: (store == "steam").then_some(id),
            playtime_minutes: Some(minutes),
            last_played,
            ..Default::default()
        })
        .collect();
    t.db.sync_source(PlayerId::FIRST, store, store, &games).unwrap();
}

/// No store page for any appid (`parse_app_details` reads a body keyed by
/// any id), so the Steam store step asks nothing that fails.
async fn no_store_pages(t: &Setup) {
    Mock::given(path("/api/appdetails"))
        .respond_with(json_response(json!({"0": {"success": false}})))
        .with_priority(1)
        .mount(&t.server)
        .await;
}

/// Each of these failed: 500 for every request under it.
async fn down(t: &Setup, under: &str) {
    Mock::given(wiremock::matchers::path_regex(under)).respond_with(ResponseTemplate::new(500)).with_priority(1).mount(&t.server).await;
}

/// The ids asked for under `prefix` (the next part of the path), each once.
async fn asked_ids(t: &Setup, prefix: &str) -> Vec<String> {
    let mut ids: Vec<String> = t
        .requested()
        .await
        .iter()
        .filter_map(|p| p.strip_prefix(prefix).map(|rest| rest.split(['/', '.']).next().unwrap_or_default().to_string()))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// With ProtonDB and GOG down, each stops after three titles failed in a
/// row instead of waiting out its backoff for every title in the library.
#[tokio::test]
async fn protondb_and_gog_give_up_when_they_keep_failing() {
    let t = setup().await;
    store_games(&t, "steam", &["Alpha Steam", "Beta Steam", "Gamma Steam", "Delta Steam", "Epsilon Steam"], 3001, 60, 0);
    store_games(&t, "gog", &["Alpha Gog", "Beta Gog", "Gamma Gog", "Delta Gog", "Epsilon Gog"], 2001, 0, 0);
    no_store_pages(&t).await;
    down(&t, "^/api/v1/reports/summaries/").await;
    down(&t, "^/v2/games/").await;
    down(&t, "^/v1/products/[0-9]+/reviews").await;
    let only: Vec<(&str, bool)> = sources::ids().into_iter().map(|id| (id, id == "protondb" || id == "gog_meta")).collect();
    let log = Log::default();
    let s = run_enrich(&t.db, &t.cfg, &t.clients, None, &with(&only), &log).await.unwrap();

    assert_eq!(asked_ids(&t, "/api/v1/reports/summaries/").await.len(), 3, "{}", log.text());
    let mut gog = asked_ids(&t, "/v2/games/").await;
    gog.extend(asked_ids(&t, "/v1/products/").await);
    gog.sort_unstable();
    gog.dedup();
    assert_eq!(gog.len(), 3, "{gog:?}");
    let text = log.text();
    assert!(text.contains("ProtonDB stopped for the rest of this run after 3 failures in a row"), "{text}");
    assert!(text.contains("GOG stopped for the rest of this run after 3 failures in a row"), "{text}");
    assert!(text.contains("GOG reviews failed for "), "the second request's failure is logged too: {text}");
    assert_eq!(s.errors, 3 + 3 + 3, "three ProtonDB titles, three GOG titles and their review requests");
    let mut stopped = s.stopped.clone();
    stopped.sort_unstable();
    assert_eq!(stopped, ["GOG", "ProtonDB"]);
}

/// The code-health review's B-STORES-1: SteamSpy and IGDB stop after three failures in a
/// row, as the other lanes do, instead of waiting out their backoff for every
/// title while the service is down; the job's summary names them.
#[tokio::test]
async fn steamspy_gives_up_when_it_keeps_failing() {
    let t = setup().await;
    store_games(&t, "steam", &["Alpha Steam", "Beta Steam", "Gamma Steam", "Delta Steam", "Epsilon Steam"], 3001, 60, 0);
    no_store_pages(&t).await;
    down(&t, "^/api.php").await;
    let only: Vec<(&str, bool)> = sources::ids().into_iter().map(|id| (id, id == "tags")).collect();
    let log = Log::default();
    let s = run_enrich(&t.db, &t.cfg, &t.clients, None, &with(&only), &log).await.unwrap();
    let reqs = t.server.received_requests().await.unwrap();
    let mut asked: Vec<String> = reqs
        .iter()
        .filter(|r| r.url.path() == "/api.php")
        .filter_map(|r| r.url.query_pairs().find(|(k, _)| k == "appid").map(|(_, v)| v.to_string()))
        .collect();
    let total = asked.len();
    asked.sort_unstable();
    asked.dedup();
    assert_eq!(asked.len(), 3, "SteamSpy asked about {} titles in {total} requests: {}", asked.len(), log.text());
    assert!(log.text().contains("SteamSpy stopped for the rest of this run after 3 failures in a row"), "{}", log.text());
    assert_eq!(s.stopped, ["SteamSpy"]);
    assert!(s.summary_text().ends_with("3 errors; stopped early: SteamSpy"), "{}", s.summary_text());
}

#[tokio::test]
async fn igdb_gives_up_when_it_keeps_failing() {
    let t = setup().await;
    store_games(&t, "gog", &["Alpha Gog", "Beta Gog", "Gamma Gog", "Delta Gog", "Epsilon Gog"], 2001, 0, 0);
    no_store_pages(&t).await;
    Mock::given(path("/v4/games")).respond_with(ResponseTemplate::new(500)).with_priority(1).mount(&t.server).await;
    let only: Vec<(&str, bool)> = sources::ids().into_iter().map(|id| (id, id == "igdb")).collect();
    let log = Log::default();
    let s = run_enrich(&t.db, &t.cfg, &t.clients, None, &with(&only), &log).await.unwrap();
    let searches = t.server.received_requests().await.unwrap().iter().filter(|r| r.url.path() == "/v4/games").count();
    assert!(searches <= 3 * 3, "IGDB sent {searches} searches for 5 titles: {}", log.text());
    assert_eq!((s.errors, s.stopped.as_slice()), (3, ["IGDB"].as_slice()), "{}", log.text());
}

/// The Second Chance review fetch, with Steam down: it stops asking after
/// five failures in a row, as the Steam reviews lane does, and with a limit
/// it fetches for no game past it.
#[tokio::test]
async fn second_chances_fetch_gives_up_and_keeps_to_its_limit() {
    let t = setup().await;
    // Played for 21 hours and last in 2022: abandoned.
    let names = ["Game A", "Game B", "Game C", "Game D", "Game E", "Game F", "Game G"];
    store_games(&t, "steam", &names, 3101, 1260, 1_646_092_800);
    assert_eq!(t.db.all_games(PlayerId::FIRST).unwrap().iter().filter(|g| crate::second_chance::checked(g)).count(), names.len());
    no_store_pages(&t).await;
    down(&t, "^/appreviews/").await;
    let llm = crate::testutil::ScriptedLlm::new(vec![]);
    let only: Vec<(&str, bool)> = sources::ids().into_iter().map(|id| (id, id == "second_chances")).collect();
    let mut opts = with(&only);
    opts.limit = Some(3);
    let log = Log::default();
    run_enrich(&t.db, &t.cfg, &t.clients, Some(&llm), &opts, &log).await.unwrap();
    assert_eq!(asked_ids(&t, "/appreviews/").await, ["3101", "3102", "3103"], "{}", log.text());
    assert!(log.text().contains("0 of the first 3 of 7 games put down need their Now written"), "{}", log.text());

    let before = t.requested().await.len();
    let log = Log::default();
    run_enrich(&t.db, &t.cfg, &t.clients, Some(&llm), &with(&only), &log).await.unwrap();
    let mut again: Vec<String> =
        t.requested().await.split_off(before).into_iter().filter_map(|p| p.strip_prefix("/appreviews/").map(str::to_string)).collect();
    again.dedup();
    assert_eq!(again, ["3101", "3102", "3103", "3104", "3105"], "{}", log.text());
    assert!(log.text().contains("Steam reviews stopped for the rest of this run after 5 failures in a row"), "{}", log.text());
    assert!(llm.asked.lock().unwrap().is_empty(), "no reviews since, so no Now to write");
}

/// A Steam match found in the store lane merges the title into the one that
/// already has the appid while the other lanes still hold the old title id.
/// Their writes must follow the copy to its new title, not fail on the old one.
#[tokio::test]
async fn a_merge_in_the_middle_of_enrich_does_not_abort_it() {
    let t = setup().await;
    let game = |source: &str, id: &str, name: &str, appid: Option<u32>, minutes: i64| ImportedGame {
        source: source.into(),
        source_id: id.into(),
        launcher: if source == "steam" { "steam".into() } else { "heroic".into() },
        name: name.into(),
        appid,
        playtime_minutes: Some(minutes),
        ..Default::default()
    };
    t.db.sync_source(PlayerId::FIRST, "steam", "steam", &[game("steam", "620", "Portal 2", Some(620), 1260)]).unwrap();
    // A different name, so it is a title of its own until Steam's store search matches it.
    t.db.sync_source(PlayerId::FIRST, "heroic", "epic", &[game("epic", "e1", "Portall 2", None, 0)]).unwrap();
    assert_eq!(t.db.all_games(PlayerId::FIRST).unwrap().len(), 2);

    // PCGamingWiki answers late, so the match has merged the title before that lane writes.
    t.server.reset().await;
    serve(&t.server, "GET", "/api/storesearch/", fixture_json("steam/storesearch_portal2.json")).await;
    Mock::given(path("/api/appdetails"))
        .and(query_param("appids", "620"))
        .respond_with(json_response(fixture_json("steam/appdetails_620.json")))
        .mount(&t.server)
        .await;
    Mock::given(path("/w/api.php"))
        .and(query_param("action", "opensearch"))
        .respond_with(json_response(fixture_json("pcgw/opensearch_portal2.json")).set_delay(std::time::Duration::from_millis(300)))
        .mount(&t.server)
        .await;
    Mock::given(path("/w/api.php"))
        .and(query_param("action", "parse"))
        .respond_with(json_response(fixture_json("pcgw/parse_page.json")))
        .mount(&t.server)
        .await;
    let only_pcgw: Vec<(&str, bool)> = crate::enrich::sources::ids().into_iter().map(|id| (id, id == "pcgw")).collect();
    let opts = with(&only_pcgw);
    let log = Log::default();
    let s = run_enrich(&t.db, &t.cfg, &t.clients, None, &opts, &log).await.unwrap_or_else(|e| panic!("{e:#}\n{}", log.text()));
    assert_eq!((s.matches, s.count("pcgw"), s.errors), (1, 2, 0), "{}", log.text());

    let all = t.db.all_games(PlayerId::FIRST).unwrap();
    assert_eq!(all.len(), 1, "the Epic copy joined the Steam title");
    assert_eq!(all[0].copies.len(), 2);
    assert!(all[0].has_pcgw, "the PCGamingWiki result landed on the surviving title");
}

// ---- as a job ---------------------------------------------------------------

/// A second connection to the test database, the way the server and a
/// terminal each have their own, and a job recorded through it.
fn job_on(t: &Setup) -> (std::sync::Arc<Db>, std::sync::Arc<crate::jobs::LiveJob>) {
    let db = std::sync::Arc::new(Db::open(&t.cfg.db_path, t.cfg.sampled_minutes).unwrap());
    let job = crate::jobs::LiveJob::begin(db.clone(), "enrich", None, crate::db::JobSource::Terminal, json!({}), None).unwrap().unwrap();
    (db, job)
}

/// Steam answers 429 twice: the Steam reviews lane says it's waiting, then
/// the request goes through and nothing counts as failed.
#[tokio::test]
async fn a_lane_says_when_a_service_makes_it_wait() {
    let t = setup().await;
    t.library();
    Mock::given(path("/appreviews/620")).respond_with(ResponseTemplate::new(429)).up_to_n_times(2).with_priority(1).mount(&t.server).await;
    let (db, job) = job_on(&t);
    let s = job.scope(run_enrich(&t.db, &t.cfg, &t.clients, None, &EnrichOptions::default(), &*job)).await.unwrap();
    job.finish(Ok(s.summary_text()));

    let warnings = db.job_log(job.id, 0, crate::db::Level::Warn, 100).unwrap();
    let waits: Vec<&crate::db::LogLine> =
        warnings.iter().map(|l| &l.line).filter(|l| l.message.starts_with("Steam asked us to slow down, trying again in")).collect();
    assert_eq!(waits.len(), 2, "{warnings:?}");
    assert!(waits.iter().all(|l| l.lane.as_deref() == Some("Steam reviews")), "{waits:?}");
    assert!(waits[0].message.contains("HTTP 429 Too Many Requests for http://") && waits[0].message.ends_with("try 1 of 5)"), "{waits:?}");

    let done = db.job(job.id).unwrap().unwrap();
    assert_eq!((done.status, done.failed, s.errors), (crate::db::JobStatus::Done, 0, 0));
    let lane = done.lanes.iter().find(|l| l.name == "Steam reviews").unwrap();
    assert_eq!((lane.state, lane.failed, lane.done, lane.total), (crate::db::LaneState::Done, 0, 2, 2));
    let names: Vec<&str> = done.lanes.iter().map(|l| l.name.as_str()).collect();
    let ran = ["Wikidata", "Series covers", "Steam art", "Steam store", "Steam reviews", "SteamSpy", "ProtonDB", "GOG", "IGDB", "RAWG"];
    assert!(names.starts_with(&ran), "passes and lanes in the order they started: {names:?}");
    assert!(done.lanes.iter().all(|l| l.state == crate::db::LaneState::Done), "{:?}", done.lanes);
    assert!(!t.game("Portal 2").review_texts.is_empty(), "the reviews came after the waits");
}

/// Steam keeps saying 429 for Portal 2's reviews: the failure is listed with
/// the title and its sources, and running it again asks for only that.
#[tokio::test]
async fn failures_are_listed_and_run_again_alone() {
    let t = setup().await;
    t.library();
    Mock::given(path("/appreviews/620")).respond_with(ResponseTemplate::new(429)).up_to_n_times(5).with_priority(1).mount(&t.server).await;
    let (db, job) = job_on(&t);
    let s = job.scope(run_enrich(&t.db, &t.cfg, &t.clients, None, &EnrichOptions::default(), &*job)).await.unwrap();
    job.finish(Ok(s.summary_text()));
    let portal = t.game("Portal 2");
    assert!(portal.review_texts.is_empty());

    let failures = db.job_failures(job.id).unwrap();
    assert_eq!(failures.len(), 1, "{failures:?}");
    let f = &failures[0];
    assert_eq!(
        (f.lane.as_str(), f.source.as_str(), f.title_id, f.name.as_deref()),
        ("Steam reviews", "Steam reviews", Some(portal.id), Some("Portal 2"))
    );
    assert_eq!(f.options, ["reviews", "review_texts"]);
    assert!(f.error.contains("429"), "{}", f.error);
    let finished = db.job(job.id).unwrap().unwrap();
    assert_eq!(finished.failed, 1);
    let lane = finished.lanes.iter().find(|l| l.name == "Steam reviews").unwrap();
    assert_eq!(lane.failed, 1);
    let errors = db.job_log(job.id, 0, crate::db::Level::Error, 100).unwrap();
    assert!(errors.iter().any(|l| l.line.message.starts_with("Steam reviews failed for Portal 2: HTTP 429")), "{errors:?}");

    let retry = crate::jobs::retry_options(&finished, &failures).expect("something to run again");
    assert_eq!(retry.titles, [portal.id]);
    let on: Vec<&str> = retry.sources.iter().filter(|(_, on)| **on).map(|(id, _)| id.as_str()).collect();
    assert_eq!(on, ["review_texts", "reviews"]);

    let before = t.requested().await.len();
    let log = Log::default();
    let again = run_enrich(&t.db, &t.cfg, &t.clients, None, &retry, &log).await.unwrap();
    let asked: Vec<String> = t.requested().await.split_off(before);
    assert!(!asked.is_empty() && asked.iter().all(|p| p == "/appreviews/620"), "only Portal 2's reviews: {asked:?}");
    assert_eq!(again.errors, 0, "{}", log.text());
    assert!(!t.game("Portal 2").review_texts.is_empty());
}

/// The UI cancels a job a terminal runs: the flag goes through the table,
/// the terminal's heartbeat picks it up, and the run stops.
#[tokio::test]
async fn a_cancel_from_another_process_stops_the_run() {
    let t = setup().await;
    t.library();
    // The Witcher is looked up first and answers slowly, so the cancel lands mid-run.
    Mock::given(path("/api/storesearch/"))
        .respond_with(json_response(fixture_json("steam/storesearch_portal2.json")).set_delay(std::time::Duration::from_millis(300)))
        .with_priority(1)
        .mount(&t.server)
        .await;
    let (db, job) = job_on(&t);
    let ui = Db::open(&t.cfg.db_path, t.cfg.sampled_minutes).unwrap();
    let opts = EnrichOptions::default();
    let run = job.scope(run_enrich(&t.db, &t.cfg, &t.clients, None, &opts, &*job));
    let cancel = async {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        ui.job_request_cancel().unwrap().expect("the terminal's job is running")
    };
    let (result, asked) = tokio::join!(run, cancel);
    assert!(asked.cancel_requested);
    let err = result.expect_err("the run stopped");
    assert_eq!(job.finish(Err(&err)), crate::db::JobStatus::Cancelled);
    let row = db.job(job.id).unwrap().unwrap();
    assert_eq!(row.status, crate::db::JobStatus::Cancelled);
    assert!(ui.job_running().unwrap().is_none());
    assert!(!t.requested().await.iter().any(|p| p.starts_with("/appreviews")), "no lane went on after the cancel");
}

/// `systemctl --user stop upnext` mid-enrich: the server's job stops at its
/// next check and records itself as cancelled, the database stays sound,
/// and the next run finishes the work without fetching anything twice.
#[tokio::test]
async fn a_shutdown_stops_enrich_and_the_next_run_picks_up() {
    let t = setup().await;
    t.library();
    // The Witcher's Steam lookup answers slowly: the shutdown lands while
    // it waits, after Portal 2's store page is in.
    Mock::given(path("/api/storesearch/"))
        .respond_with(json_response(fixture_json("steam/storesearch_portal2.json")).set_delay(std::time::Duration::from_secs(4)))
        .with_priority(1)
        .mount(&t.server)
        .await;
    let db = std::sync::Arc::new(Db::open(&t.cfg.db_path, t.cfg.sampled_minutes).unwrap());
    let clients = std::sync::Arc::new(Clients::with_base_url(&t.cfg, Some(&t.server.uri())).unwrap());
    let llm: std::sync::Arc<dyn crate::llm::LlmBackend> = std::sync::Arc::new(crate::testutil::ScriptedLlm::new(vec![]));
    let runner = std::sync::Arc::new(crate::jobs::JobRunner::new());
    let kind = crate::jobs::JobKind::Enrich(EnrichOptions::default());
    let job = runner.start(kind, None, crate::db::JobSource::Schedule, db.clone(), t.cfg.clone(), clients, llm).unwrap().unwrap();
    // Stop at a fixed point, not after a guessed wait: once the Steam store
    // has handed Portal 2 on and holds The Witcher's lookup, and every other
    // lane has stored all it can without that lookup (the lanes after the
    // store: Portal 2; the others: everything). The held lookup is then the
    // only request in flight, so nothing finished is lost to the stop.
    let after_store: Vec<&str> = sources::registry().titles.iter().filter(|s| s.after_steam_store()).map(|s| s.lane()).collect();
    let started = std::time::Instant::now();
    loop {
        let lanes = db.job(job.id).unwrap().unwrap().lanes;
        let store = lanes.iter().find(|l| l.name == SteamStore::LANE);
        let held = store.is_some_and(|s| s.done == 1 && s.current.as_deref().is_some_and(|c| c.starts_with("The Witcher")));
        let settled = lanes.iter().filter(|l| l.name != SteamStore::LANE).all(|l| {
            l.state == crate::db::LaneState::Done || (after_store.contains(&l.name.as_str()) && l.done == 1 && l.current.is_none())
        });
        if held && settled && lanes.len() > 1 {
            break;
        }
        assert!(store.is_none_or(|s| s.done < 2), "The Witcher's lookup answered before the lanes settled: {lanes:?}");
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "the lanes never settled: {lanes:?}");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(runner.shut_down(std::time::Duration::from_secs(10)).await, "it stopped within the grace");

    let row = db.job(job.id).unwrap().unwrap();
    assert_eq!(row.status, crate::db::JobStatus::Cancelled);
    let log = db.job_log(job.id, 0, crate::db::Level::Info, 100).unwrap();
    assert!(log.iter().any(|l| l.line.message == "stopping: UpNext is shutting down"), "{log:?}");
    assert!(db.job_running().unwrap().is_none(), "nothing left running");
    let check: String = rusqlite::Connection::open(&t.cfg.db_path).unwrap().query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap();
    assert_eq!(check, "ok");

    // Each request whole: a batched one (Steam art) asks for different games each time.
    let whole = || async {
        let got = t.server.received_requests().await.unwrap_or_default();
        got.iter()
            .map(|r| format!("{} {} {}", r.url.path(), r.url.query().unwrap_or(""), String::from_utf8_lossy(&r.body)))
            .collect::<Vec<_>>()
    };
    let before = whole().await;
    let log = Log::default();
    let next = run_enrich(&t.db, &t.cfg, &t.clients, None, &EnrichOptions::default(), &log).await.unwrap();
    assert_eq!(next.errors, 0, "{}", log.text());
    let asked: Vec<String> = whole().await.split_off(before.len());
    // The passes after the lanes (IGDB's trailers) hadn't run.
    assert!(asked.iter().any(|p| p.starts_with("/v4/games  fields id,videos")), "the next run did what was left: {asked:?}");
    // Only the key checks run every time. Nothing the stopped run finished
    // is fetched twice: Portal 2's store page least of all.
    assert!(before.iter().any(|p| p.starts_with("/api/appdetails appids=620")));
    let every_time = ["/oauth2/token ", "/api/genres "];
    let again = |p: &&String| !every_time.iter().any(|e| p.starts_with(e));
    let twice: Vec<&String> = asked.iter().filter(again).filter(|p| before.contains(p)).collect();
    assert!(twice.is_empty(), "fetched again: {twice:?}");
}

/// A backoff can last two minutes; a job asked to stop doesn't sit it out.
#[tokio::test]
async fn a_stop_cuts_a_backoff_short() {
    let t = setup().await;
    let (_db, job) = job_on(&t);
    let limiter = crate::http::RateLimiter::new("Steam", std::time::Duration::ZERO);
    limiter.penalize(std::time::Duration::from_secs(60)).await;
    let started = std::time::Instant::now();
    let waited = job.scope(async {
        let stop = async {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            job.cancel_now();
        };
        tokio::join!(limiter.wait(), stop).0
    });
    assert!(!waited.await, "it gave up waiting");
    assert!(started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());
}

/// A title whose summary came from shared data skips the review
/// texts (its score is one request) and GOG's reviews, and the AI isn't
/// asked to write its summary even once it has reviews.
#[tokio::test]
async fn seeded_titles_skip_the_review_fetch_and_the_ai() {
    let t = setup().await;
    t.library();
    let shared = |appid: Option<u32>, gog: Option<&str>, says: &str| crate::seed::Entry {
        steam_appid: appid,
        gog_id: gog.map(str::to_string),
        name: says.into(),
        people_say: says.into(),
        vibes: Some("Shared vibes.".into()),
        tags: Default::default(),
        prompt_version: crate::digest::DIGEST_VERSION - 1,
        model: "fixture-model".into(),
        written: "2026-09-30".into(),
        reviews_until: None,
    };
    let data =
        crate::seed::write("2026-10-01", &[shared(Some(620), None, "Shared Portal 2."), shared(None, Some(WITCHER), "Shared Witcher.")])
            .unwrap();
    assert_eq!(crate::seed::apply(&t.db, &crate::seed::read(&data).unwrap()).unwrap().written, 2);

    let log = Log::default();
    run_enrich(&t.db, &t.cfg, &t.clients, None, &EnrichOptions::default(), &log).await.unwrap_or_else(|e| panic!("{e:#}\n{}", log.text()));
    let requests = t.server.received_requests().await.unwrap();
    let reviews: Vec<_> = requests.iter().filter(|r| r.url.path() == "/appreviews/620").collect();
    assert_eq!(reviews.len(), 1, "the score alone");
    assert!(reviews[0].url.query_pairs().any(|(k, v)| k == "num_per_page" && v == "0"), "{}", reviews[0].url);
    assert!(!requests.iter().any(|r| r.url.path().ends_with("/reviews")), "no GOG reviews");
    let portal = t.game("Portal 2");
    assert!(portal.review_total.is_some() && portal.review_texts.is_empty(), "a score but no texts");
    assert_eq!(portal.digest.as_deref(), Some("Shared Portal 2."));

    // Reviews fetched later (the game page asks) don't bring the AI back.
    let texts = vec![crate::db::ReviewText {
        source: "steam".into(),
        positive: Some(true),
        rating: None,
        votes: 3,
        hours: Some(20),
        title: None,
        text: "Clever puzzles and the funniest robots around, a joy from start to finish.".into(),
        posted: None,
    }];
    t.db.put_review_texts(620, Some(&texts)).unwrap();
    assert!(crate::digest::wanted(&t.game("Portal 2")), "it has reviews to summarize");
    Mock::given(method("POST")).and(path("/v1/chat/completions")).respond_with(ResponseTemplate::new(500)).expect(0).mount(&t.server).await;
    let llm = crate::llm::openai_compat::OpenAiCompatBackend::new(format!("{}/v1", t.server.uri()), Some("fake-model".into()), None);
    let s = run_enrich(&t.db, &t.cfg, &t.clients, Some(&llm), &with(&[("digests", true)]), &log).await.unwrap();
    assert_eq!(s.count("digests"), 0, "{}", log.text());
    assert_eq!(t.game("Portal 2").digest.as_deref(), Some("Shared Portal 2."), "still the shared summary, at its older version");
}

// -- two players -----------------------------------------

const SAM_STEAM: &str = "76561198000000001";

/// Sam, a second player with a Steam account of their own: Portal 2, which
/// player 1 owns too, and Celeste.
async fn sam(t: &Setup) -> PlayerId {
    let sam = t.db.add_player("Sam").unwrap();
    t.db.setting_set(sam, "steam_id", Some(SAM_STEAM)).unwrap();
    let games = json!({"response": {"game_count": 2, "games": [
        {"appid": 620, "name": "Portal 2", "playtime_forever": 60},
        {"appid": 504230, "name": "Celeste", "playtime_forever": 120}
    ]}});
    Mock::given(path("/IPlayerService/GetOwnedGames/v1/"))
        .and(query_param("steamid", SAM_STEAM))
        .respond_with(json_response(games))
        .with_priority(1)
        .mount(&t.server)
        .await;
    sam
}

fn only(libraries: &[&str]) -> SyncOptions {
    let all = super::sync::libraries();
    let off = all.iter().map(|l| l.id()).filter(|id| !libraries.contains(id)).map(|id| (id.to_string(), false));
    SyncOptions { libraries: off.collect(), ..Default::default() }
}

#[tokio::test]
async fn each_steam_account_syncs_into_its_owners_library() {
    let t = setup().await;
    let sam = sam(&t).await;
    let log = Log::default();
    run_sync(&t.db, &t.cfg, &t.clients, &only(&["steam"]), &log).await.unwrap();
    let text = log.text();
    assert!(text.contains("Steam (Player 1): 3 games (3 new"), "{text}");
    assert!(text.contains("Steam (Sam): 2 games (2 new, 0 updated, 0 removed)"), "{text}");

    // One Portal 2 with two copies; each player's hours are their own.
    let hours = |p: PlayerId, name: &str| {
        let g = t.db.all_games(p).unwrap().into_iter().find(|g| g.name == name).unwrap();
        (g.copies.len(), g.playtime_minutes)
    };
    assert_eq!(hours(PlayerId::FIRST, "Portal 2"), (2, 1260));
    assert_eq!(hours(sam, "Portal 2"), (2, 60));
    assert_eq!(hours(PlayerId::FIRST, "Celeste"), (1, 0), "Sam's game, shared, unplayed by player 1");
    assert_eq!(hours(sam, "Celeste"), (1, 120));
    let c = rusqlite::Connection::open(&t.cfg.db_path).unwrap();
    let mut st = c.prepare("SELECT external_id FROM store_accounts WHERE store='steam' ORDER BY id").unwrap();
    let ids: Vec<String> = st.query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
    assert_eq!(ids, ["76561198000000000", SAM_STEAM]);
    // The top bar's Steam profile stays player 1's.
    assert_eq!(t.db.profile(PlayerId::FIRST, None).unwrap().steam_id, "76561198000000000");
}

/// B-STORES-8: two players with an account each on one Steam client and
/// no Web API key. Each gets its own account's games; the one not signed
/// in to Steam gets what it played or installed, and loses nothing it had
/// when the other account is the one signed in.
#[tokio::test]
async fn two_accounts_on_one_steam_client_each_sync_their_own_games() {
    use crate::steam_local::tests::{FIRST, SECOND, two_accounts};
    let mut t = setup().await;
    let steam = t._dir.path().join("steam");
    two_accounts(&steam, FIRST, &[400, 620]);
    t.cfg.steam_dir = Some(steam.clone());
    t.cfg.steam_id = Some(FIRST.into());
    let sam = t.db.add_player("Sam").unwrap();
    t.db.setting_set(sam, "steam_id", Some(SECOND)).unwrap();
    let local = SyncOptions { steam_local: true, ..only(&["steam"]) };
    let libraries = |t: &Setup| -> Vec<(i64, String)> {
        let c = rusqlite::Connection::open(&t.cfg.db_path).unwrap();
        let mut st = c
            .prepare(
                "SELECT k.owner_id, g.name FROM games g JOIN store_accounts a ON a.id=g.account_id
                 JOIN store_connections k ON k.id=a.connection_id WHERE g.source='steam' AND g.removed=0 ORDER BY k.owner_id, g.name",
            )
            .unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap()
    };
    let row = |p: PlayerId, name: &str| (p.0, name.to_string());

    let log = Log::default();
    run_sync(&t.db, &t.cfg, &t.clients, &local, &log).await.unwrap();
    let text = log.text();
    assert!(text.contains("2 games (2 new, 0 updated, 0 removed)"), "{text}");
    assert!(text.contains("2 games this account played or installed (2 new, 0 updated)"), "{text}");
    assert_eq!(
        libraries(&t),
        [row(PlayerId::FIRST, "Portal"), row(PlayerId::FIRST, "Portal 2"), row(sam, "Celeste"), row(sam, "Hades")],
        "{text}"
    );

    // Sam signs in to Steam: Sam's whole library comes in, and player 1
    // keeps Portal, which their account's read no longer sees.
    two_accounts(&steam, SECOND, &[413150, 504230]);
    let log = Log::default();
    run_sync(&t.db, &t.cfg, &t.clients, &local, &log).await.unwrap();
    let text = log.text();
    assert!(text.contains("1 game this account played or installed (0 new, 1 updated)"), "{text}");
    assert_eq!(
        libraries(&t),
        [
            row(PlayerId::FIRST, "Portal"),
            row(PlayerId::FIRST, "Portal 2"),
            row(sam, "Celeste"),
            row(sam, "Hades"),
            row(sam, "Stardew Valley")
        ],
        "{text}"
    );
}

#[tokio::test]
async fn each_gog_sign_in_reads_its_own_account() {
    let t = setup().await;
    t.sign_in_to_gog();
    let sam = t.db.add_player("Sam").unwrap();
    let token = crate::gog::GogToken {
        access_token: "gog-sam".into(),
        refresh_token: "gog-sam-refresh".into(),
        expires_at: crate::db::now() + 3600,
        user_id: "4801".into(),
        username: Some("sam".into()),
    };
    use crate::signin::SignInStore;
    crate::signin::DbSignIn::own(t.cfg.db_path.clone(), "gog", sam, None).save(&serde_json::to_string(&token).unwrap()).unwrap();
    let gwent = json!({"page": 1, "totalPages": 1, "totalProducts": 1, "products": [{
        "id": 1207658800, "title": "Gwent", "isGame": true, "image": "//images.gog.example/gwent", "url": "/en/game/gwent",
        "category": "Strategy", "slug": "gwent", "releaseDate": {"date": "2018-10-23 00:00:00.000000", "timezone_type": 3, "timezone": "UTC"}
    }]});
    Mock::given(path("/account/getFilteredProducts"))
        .and(wiremock::matchers::header("authorization", "Bearer gog-sam"))
        .respond_with(json_response(gwent))
        .with_priority(1)
        .mount(&t.server)
        .await;
    let log = Log::default();
    run_sync(&t.db, &t.cfg, &t.clients, &only(&["gog"]), &log).await.unwrap();
    let text = log.text();
    assert!(text.contains("GOG account (Player 1): 1 game (1 new"), "{text}");
    assert!(text.contains("GOG account (Sam): 1 game (1 new"), "{text}");
    // Each copy is in its owner's GOG account; both players see both games,
    // since a new account is offered to everyone.
    let c = rusqlite::Connection::open(&t.cfg.db_path).unwrap();
    let mut st = c
        .prepare(
            "SELECT k.owner_id, g.name FROM games g JOIN store_accounts a ON a.id=g.account_id
             JOIN store_connections k ON k.id=a.connection_id WHERE g.source='gog' ORDER BY g.name",
        )
        .unwrap();
    let owners: Vec<(i64, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap();
    assert_eq!(owners, [(sam.0, "Gwent".to_string()), (1, "The Witcher: Enhanced Edition".to_string())]);
    assert_eq!(t.db.all_games(sam).unwrap().len(), 2);
    // Player 1's sign-in moved from its file onto their connection.
    assert!(!t.cfg.gog_token_path.exists());
    assert!(t.clients.gog.load_token().is_some_and(|tok| tok.user_id == "4800"));
    assert!(t.db.signed_in("gog").unwrap());
}

#[tokio::test]
async fn a_game_both_players_own_is_enriched_once() {
    let t = setup().await;
    t.library();
    let sam = t.db.add_player("Sam").unwrap();
    let portal = ImportedGame {
        source: "steam".into(),
        source_id: "620".into(),
        launcher: "steam".into(),
        name: "Portal 2".into(),
        appid: Some(620),
        playtime_minutes: Some(60),
        ..Default::default()
    };
    t.db.sync_source(sam, "steam", "steam", &[portal]).unwrap();
    let log = Log::default();
    let s = run_enrich(&t.db, &t.cfg, &t.clients, None, &with(&[]), &log).await.unwrap_or_else(|e| panic!("{e:#}\n{}", log.text()));
    assert_eq!((s.processed, s.details), (2, 1), "two titles, one of them with two copies");
    let asked = t.requested().await.into_iter().filter(|p| p == "/api/appdetails").count();
    assert_eq!(asked, 1, "Portal 2's details were fetched once");
    let details = |p: PlayerId| t.db.all_games(p).unwrap().into_iter().find(|g| g.name == "Portal 2").unwrap().short_description;
    assert!(details(PlayerId::FIRST).is_some());
    assert_eq!(details(sam), details(PlayerId::FIRST), "one record for both");
}

/// Steam's recently played games for each account.
async fn readings(server: &MockServer, first: i64, sam: i64) {
    server.reset().await;
    for (steamid, minutes, priority) in [(SAM_STEAM, sam, 1), ("76561198000000000", first, 2)] {
        Mock::given(path("/IPlayerService/GetRecentlyPlayedGames/v1/"))
            .and(query_param("steamid", steamid))
            .respond_with(json_response(json!({"response": {"total_count": 1, "games": [
                {"appid": 620, "name": "Portal 2", "playtime_2weeks": 60, "playtime_forever": minutes}
            ]}})))
            .with_priority(priority)
            .mount(server)
            .await;
    }
}

#[tokio::test]
async fn each_steam_account_keeps_its_own_play_diary() {
    let mut t = setup().await;
    t.cfg.play_diary = true;
    let sam = sam(&t).await;
    run_sync(&t.db, &t.cfg, &t.clients, &only(&["steam"]), &Log::default()).await.unwrap();
    let now = crate::db::now();
    readings(&t.server, 1260, 60).await;
    let first = crate::taste::diary::poll(&t.db, &t.cfg, &t.clients, now).await.unwrap();
    assert_eq!((first.games, first.rows, first.first), (2, 0, true));
    readings(&t.server, 1305, 90).await;
    let next = crate::taste::diary::poll(&t.db, &t.cfg, &t.clients, now + 3600).await.unwrap();
    assert_eq!((next.games, next.rows), (2, 2));
    let minutes = |p: PlayerId| t.db.diary_rows(p, 0).unwrap().iter().map(|r| r.minutes).collect::<Vec<_>>();
    assert_eq!(minutes(PlayerId::FIRST), [45]);
    assert_eq!(minutes(sam), [30]);
    assert_eq!(t.db.meta_family("diary_reading").unwrap().len(), 2, "one last reading per account");
}

/// One Steam account Steam turns down leaves the others synced: the
/// failure is a log line, not the end of the sync.
#[tokio::test]
async fn a_failing_steam_account_leaves_the_others_synced() {
    let t = setup().await;
    let sam = t.db.add_player("Sam").unwrap();
    t.db.setting_set(sam, "steam_id", Some(SAM_STEAM)).unwrap();
    Mock::given(path("/IPlayerService/GetOwnedGames/v1/"))
        .and(query_param("steamid", SAM_STEAM))
        .respond_with(ResponseTemplate::new(500))
        .with_priority(1)
        .mount(&t.server)
        .await;
    let log = Log::default();
    let report = run_sync(&t.db, &t.cfg, &t.clients, &only(&["steam"]), &log).await.unwrap_or_else(|e| panic!("{e:#}\n{}", log.text()));
    let text = log.text();
    assert!(text.contains("Steam (Player 1): 3 games (3 new"), "{text}");
    assert!(text.contains("Steam (Sam) failed"), "{text}");
    assert_eq!(t.db.all_games(PlayerId::FIRST).unwrap().len(), 3);
    // Partial, not a plain success: the failed account is reported.
    assert_eq!(report.failed, ["Steam (Sam)"]);
    assert!(
        crate::jobs::sync_summary(&report).starts_with("Partial: 3 games, 3 new; Steam (Sam) failed"),
        "{}",
        crate::jobs::sync_summary(&report)
    );
}

/// When every Steam account fails and Steam is all the sync reads, nothing
/// synced, so the sync fails, and its error names each account and why.
#[tokio::test]
async fn a_sync_where_every_account_fails_names_each_and_why() {
    let t = setup().await;
    let sam = t.db.add_player("Sam").unwrap();
    t.db.setting_set(sam, "steam_id", Some(SAM_STEAM)).unwrap();
    Mock::given(path("/IPlayerService/GetOwnedGames/v1/")).respond_with(ResponseTemplate::new(500)).with_priority(1).mount(&t.server).await;
    let log = Log::default();
    let err = run_sync(&t.db, &t.cfg, &t.clients, &only(&["steam"]), &log).await.unwrap_err().to_string();
    let (first, second) = err.split_once("; ").unwrap_or_else(|| panic!("two failures: {err}"));
    assert!(first.starts_with("Steam (Player 1) failed: ") && first.contains("500"), "{err}");
    assert!(second.starts_with("Steam (Sam) failed: ") && second.contains("500"), "{err}");
    // Each is a failure of the job too, as in a partial sync.
    let text = log.text();
    assert!(text.contains("Steam (Player 1) failed") && text.contains("Steam (Sam) failed"), "{text}");
}

/// When every account of one store fails, the sync carries on with the next
/// store and ends partial, naming the store that failed, with the reason in
/// the log.
#[tokio::test]
async fn a_store_whose_every_account_fails_doesnt_stop_the_next() {
    let t = setup().await;
    t.sign_in_to_gog();
    Mock::given(path("/IPlayerService/GetOwnedGames/v1/")).respond_with(ResponseTemplate::new(500)).with_priority(1).mount(&t.server).await;
    let log = Log::default();
    let report =
        run_sync(&t.db, &t.cfg, &t.clients, &only(&["steam", "gog"]), &log).await.unwrap_or_else(|e| panic!("{e:#}\n{}", log.text()));
    assert_eq!(report.failed, ["Steam"]);
    assert_eq!(report.summaries.iter().map(|s| (s.source.as_str(), s.added)).collect::<Vec<_>>(), [("gog", 1)], "{}", log.text());
    assert_eq!(crate::jobs::sync_summary(&report), "Partial: 1 game, 1 new; Steam failed");
    assert!(t.db.all_games(PlayerId::FIRST).unwrap().iter().any(|g| g.name == "The Witcher: Enhanced Edition"));
    let text = log.text();
    assert!(text.lines().any(|l| l.contains("Steam failed: ") && l.contains("500")), "the reason is kept: {text}");
}

/// A store that read nothing (Heroic with no folder) was skipped, not
/// synced: with Steam failing, nothing was read, so the sync fails rather
/// than ending as a partial of 0 games.
#[tokio::test]
async fn a_failure_with_only_skipped_stores_besides_is_the_syncs() {
    let t = setup().await;
    Mock::given(path("/IPlayerService/GetOwnedGames/v1/")).respond_with(ResponseTemplate::new(500)).with_priority(1).mount(&t.server).await;
    let log = Log::default();
    let err = format!("{:#}", run_sync(&t.db, &t.cfg, &t.clients, &only(&["steam", "heroic"]), &log).await.unwrap_err());
    assert!(err.starts_with("Steam failed: ") && err.contains("500") && !err.contains("; "), "{err}");
    assert!(log.text().contains("Heroic skipped"), "{}", log.text());
}

/// A GOG account whose games can't be read is a failure, as any other
/// store's account is: the sync is partial and names it. It used to be a
/// log line only, so the sync read as done.
#[tokio::test]
async fn a_gog_account_that_cant_be_read_makes_the_sync_partial() {
    let t = setup().await;
    t.sign_in_to_gog();
    Mock::given(path("/account/getFilteredProducts")).respond_with(ResponseTemplate::new(500)).with_priority(1).mount(&t.server).await;
    let log = Log::default();
    let report =
        run_sync(&t.db, &t.cfg, &t.clients, &only(&["steam", "gog"]), &log).await.unwrap_or_else(|e| panic!("{e:#}\n{}", log.text()));
    assert_eq!(report.failed, ["GOG account"]);
    assert_eq!(crate::jobs::sync_summary(&report), "Partial: 3 games, 3 new; GOG account failed");
    let text = log.text();
    assert!(text.lines().any(|l| l.contains("GOG account failed: ") && l.contains("500")), "the reason is kept: {text}");
    assert!(!text.contains("non-fatal"), "{text}");
}

/// When every store fails, the sync still fails, and its error names each.
#[tokio::test]
async fn a_sync_where_every_store_fails_still_fails() {
    let t = setup().await;
    Mock::given(path("/IPlayerService/GetOwnedGames/v1/")).respond_with(ResponseTemplate::new(500)).with_priority(1).mount(&t.server).await;
    let missing = t._dir.path().join("no-galaxy.db").display().to_string();
    let opts = SyncOptions { galaxy_db: Some(missing), ..only(&["steam", "galaxy"]) };
    let log = Log::default();
    let err = format!("{:#}", run_sync(&t.db, &t.cfg, &t.clients, &opts, &log).await.unwrap_err());
    let (steam, galaxy) = err.split_once("; ").unwrap_or_else(|| panic!("two failures: {err}"));
    assert!(steam.starts_with("Steam failed: ") && steam.contains("500"), "{err}");
    assert!(galaxy.starts_with("Galaxy failed: Galaxy import from "), "{err}");
}

/// A diary reading taken before the first sync made player 1's Steam
/// connection carries on as that connection's.
#[tokio::test]
async fn the_diarys_first_reading_moves_onto_the_steam_account_the_sync_makes() {
    let mut t = setup().await;
    t.cfg.play_diary = true;
    let recently = |minutes: i64| {
        Mock::given(path("/IPlayerService/GetRecentlyPlayedGames/v1/")).respond_with(json_response(json!({"response": {
            "total_count": 1, "games": [{"appid": 620, "name": "Portal 2", "playtime_2weeks": 60, "playtime_forever": minutes}]
        }})))
    };
    recently(1260).with_priority(1).up_to_n_times(1).mount(&t.server).await;
    let now = crate::db::now();
    let first = crate::taste::diary::poll(&t.db, &t.cfg, &t.clients, now).await.unwrap();
    assert!(first.first);
    assert_eq!(t.db.meta_family("diary_reading").unwrap().into_iter().map(|(k, _)| k).collect::<Vec<_>>(), ["diary_reading"]);

    run_sync(&t.db, &t.cfg, &t.clients, &only(&["steam"]), &Log::default()).await.unwrap();
    recently(1305).with_priority(1).mount(&t.server).await;
    let next = crate::taste::diary::poll(&t.db, &t.cfg, &t.clients, now + 3600).await.unwrap();
    assert_eq!((next.first, next.rows), (false, 1), "the count carries on");
    let keys: Vec<String> = t.db.meta_family("diary_reading").unwrap().into_iter().map(|(k, _)| k).collect();
    assert_eq!(keys, ["diary_reading.1"]);
}
