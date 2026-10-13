//! Receiving backlog verdicts: what a game's counts
//! become, the download against a stand-in service, what the library and
//! picks read of them, and the mock library's snapshot.

use super::community::{Backlog, FULL_WEIGHT_AT, GameVerdicts};
use super::format::{self, GameCounts, GameSummary, Published, Summary};
use super::receive::{self, Outcome};
use super::vocab::VOCAB;
use crate::db::{Db, GameView, HideScope, ImportedGame, Override, PlayerId, StatementRow};
use crate::prompt::{self, Factor, PromptInput, Ranking};
use crate::testutil::{fixture_path, offline_config};
use std::collections::HashMap;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const HADES: u32 = 1_145_360;
const CELESTE: u32 = 504_230;
const WITCHER: u32 = 292_030;
const WITCHER_GOG: &str = "1207664663";

/// The fixture snapshot in `tests/fixtures/verdicts/`.
fn fixture() -> (Vec<u8>, Vec<u8>, Summary) {
    let dir = fixture_path("verdicts");
    let manifest = std::fs::read(dir.join(format::MANIFEST)).unwrap();
    let m = format::read_manifest(&manifest).unwrap();
    let summary = std::fs::read(dir.join(&m.summary.file)).unwrap();
    let read = format::read_named_summary(&m, &summary).unwrap();
    (manifest, summary, read)
}

fn game_of(s: &Summary, appid: u32) -> GameSummary {
    s.games.iter().find(|g| g.steam_appid == Some(appid)).unwrap().clone()
}

fn backlog(s: &Summary, appid: u32) -> Backlog {
    let g = game_of(s, appid);
    Backlog::of(g.n, g.counts).unwrap()
}

fn counts(v: serde_json::Value) -> GameCounts {
    serde_json::from_value(v).unwrap()
}

// -- what the counts say -------------------------------------------------------------

#[test]
fn a_games_counts_become_its_headline_lines_and_facets() {
    let (_, _, s) = fixture();
    let b = backlog(&s, HADES);
    assert_eq!((b.n, b.headline.as_str(), b.short.as_str()), (23, "Worth it on a backlog, say 18 of 23 players", "Worth it, 18 of 23"));
    assert_eq!(
        b.lines,
        ["Clicks within the first hour", "Fine in sessions under an hour", "You can save anywhere", "Fine to stop after the credits"]
    );
    assert_eq!(b.facets, ["clicked:first_hour", "sessions:short", "limited_time:yes", "stop_point:at_credits", "outcome:finished"]);
    assert!(b.respects_time() && b.clicks_fast());
    assert_eq!(b.bv_line().unwrap(), "bv n=23 worth 78% click first_hour sessions short limited yes stop credits");
    assert!(b.bv_line().unwrap().split(' ').count() <= 15, "about 15 tokens at most");
}

#[test]
fn under_five_verdicts_show_as_counts_and_say_nothing_about_the_game() {
    let (_, _, s) = fixture();
    let b = backlog(&s, CELESTE);
    assert_eq!(b.n, 3);
    assert!(b.lines.is_empty() && b.facets.is_empty() && !b.respects_time());
    assert_eq!((b.bv_line(), b.card_line(&[])), (None, None));
    let page = GameVerdicts::of(&game_of(&s, CELESTE)).unwrap();
    assert_eq!(page.counts_line.as_deref(), Some("2 say it's worth it, 1 says no"));
    // Under 3, nothing at all.
    let two = counts(serde_json::json!({ "outcome": { "finished": 2 }, "worth": { "yes": 2 } }));
    assert_eq!(Backlog::of(2, two), None);
}

#[test]
fn a_split_game_is_mixed_and_has_no_majorities() {
    let c = counts(serde_json::json!({
        "outcome": { "finished": 3, "stopped": 3, "shelved": 2, "bounced": 2 },
        "worth": { "yes": 3, "for_some": 4, "no": 3 },
        "clicked": { "first_hour": 4, "later": 3, "never": 3 },
        "sessions": { "short": 4, "medium": 3, "long": 3 },
    }));
    let b = Backlog::of(10, c).unwrap();
    assert_eq!((b.headline.as_str(), b.short.as_str()), ("Mixed: 4 of 10 say only if it's your kind of game", "Mixed, 10 players"));
    assert!(b.lines.is_empty() && b.facets.is_empty());
    assert_eq!(b.bv_line().unwrap(), "bv n=10 worth 30%");
    let no = counts(serde_json::json!({ "outcome": { "stopped": 6 }, "worth": { "no": 5, "yes": 1 } }));
    assert_eq!(Backlog::of(6, no).unwrap().headline, "Not worth it on a backlog, say 5 of 6 players");
}

#[test]
fn what_stopped_players_stopped_for_reaches_the_page_the_prompt_and_second_chance() {
    let (_, _, s) = fixture();
    let b = backlog(&s, WITCHER);
    assert_eq!((b.stopped(), b.stopped_mostly_for()), (3, vec!["pacing", "too_long"]));
    assert_eq!(b.bv_line().unwrap(), "bv n=6 worth 50% limited no stopped pacing,too_long");
    assert_eq!(b.lines, ["Hard to pick back up after a break"]);
    assert_eq!(b.second_chance_line(&["too_long".into()]).unwrap(), "Of the 3 backlog players who stopped, 3 stopped for the pacing.");
    assert_eq!(
        b.second_chance_line(&["pacing".into()]).unwrap(),
        "Of the 3 backlog players who stopped, 3 stopped for the pacing, as they did."
    );
    let page = GameVerdicts::of(&game_of(&s, WITCHER)).unwrap();
    let labels: Vec<&str> = page.outcomes.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(labels, ["Finished", "Stopped: mostly for the pacing", "Shelved: usually 20 to 40 h"]);
    assert_eq!(page.texts.len(), 1);
    assert_eq!((page.texts[0].stop_note.as_deref(), page.texts[0].month.as_str()), (Some("once Velen is done"), "2026-08"));
    assert!(page.texts[0].about.ends_with("Worth it: No"), "{}", page.texts[0].about);
    // Fewer than three stopped: no reasons at all.
    assert!(backlog(&s, HADES).second_chance_line(&[]).is_none());
}

#[test]
fn the_pick_card_line_leads_with_what_the_request_asks() {
    let (_, _, s) = fixture();
    let b = backlog(&s, HADES);
    let line = |request: &str| b.card_line(&prompt::verdict_wishes(request)).unwrap();
    assert_eq!(line("anything"), "Backlog players: clicks fast, fine in short sessions (23)");
    assert_eq!(line("something short for a weeknight"), "Backlog players: fine in short sessions, respects limited time (23)");
    assert_eq!(line("something I can finish"), "Backlog players: 74% finished it, fine to stop at the credits (23)");
}

#[test]
fn a_facets_weight_grows_with_the_verdicts_and_stops_at_twenty() {
    let c = counts(serde_json::json!({ "outcome": { "finished": 30 }, "worth": { "yes": 30 } }));
    let weight = |n: u32| Backlog::of(n, c.clone()).unwrap().weight();
    assert_eq!([weight(4), weight(10), weight(FULL_WEIGHT_AT), weight(30)], [0.2, 0.5, 1.0, 1.0]);
}

// -- the download --------------------------------------------------------------------

/// A library with Hades and Celeste on Steam and The Witcher 3 only on GOG,
/// receiving on.
fn library() -> (tempfile::TempDir, crate::config::Config, Db) {
    let dir = tempfile::tempdir().unwrap();
    let cfg = offline_config(dir.path());
    let db = Db::open(&cfg.db_path, cfg.sampled_minutes).unwrap();
    let steam = |appid: u32, name: &str| ImportedGame {
        source: "steam".into(),
        source_id: appid.to_string(),
        launcher: "steam".into(),
        name: name.into(),
        appid: Some(appid),
        ..Default::default()
    };
    db.sync_source(PlayerId::FIRST, "steam", "steam", &[steam(HADES, "Hades"), steam(CELESTE, "Celeste")]).unwrap();
    let witcher = ImportedGame {
        source: "gog".into(),
        source_id: WITCHER_GOG.into(),
        launcher: "gog".into(),
        name: "The Witcher 3: Wild Hunt".into(),
        ..Default::default()
    };
    db.sync_source(PlayerId::FIRST, "gog", "gog", &[witcher]).unwrap();
    db.setting_set(PlayerId::FIRST, crate::db::VERDICTS_RECEIVE, Some("true")).unwrap();
    (dir, cfg, db)
}

fn named(db: &Db, name: &str) -> GameView {
    db.all_games(PlayerId::FIRST).unwrap().into_iter().find(|g| g.name == name).unwrap()
}

async fn service(manifest: Vec<u8>, summary: Vec<u8>, downloads: u64) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/verdicts.json"))
        .and(header("if-none-match", "\"one\""))
        .respond_with(ResponseTemplate::new(304))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/verdicts.json"))
        .respond_with(ResponseTemplate::new(200).insert_header("etag", "\"one\"").set_body_bytes(manifest))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/verdicts-summary-2026-10-09.jsonl.gz"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(summary))
        .expect(downloads)
        .mount(&server)
        .await;
    server
}

/// The snapshot applies and matches by Steam appid and by a GOG copy's id;
/// asked again, the manifest's ETag gets a 304 and nothing is downloaded,
/// and a manifest of the same day downloads nothing either.
#[tokio::test]
async fn the_snapshot_matches_by_appid_and_gog_id_and_an_unchanged_one_downloads_nothing() {
    let (_dir, _cfg, db) = library();
    let (manifest, summary, _) = fixture();
    let server = service(manifest, summary, 1).await;
    let http = reqwest::Client::new();
    let r = receive::sync(&db, &http, &server.uri()).await.unwrap();
    assert_eq!(r.outcome, Outcome::Applied);
    let s = &r.snapshot;
    assert_eq!((s.date.as_deref(), s.games, s.verdicts, s.matched), (Some("2026-10-09"), 4, 35, 3), "{s:?}");
    assert_eq!(s.source.as_deref(), Some(server.uri().as_str()));
    assert_eq!(r.summary(), "Backlog verdicts of 2026-10-09: 3 of its 4 games are in this library (35 verdicts read)");
    assert_eq!(named(&db, "Hades").backlog.unwrap().n, 23);
    assert_eq!(named(&db, "The Witcher 3: Wild Hunt").backlog.unwrap().n, 6, "matched through its GOG copy");
    let witcher = named(&db, "The Witcher 3: Wild Hunt").id;
    assert_eq!(db.community_verdicts_for(witcher).unwrap().unwrap().texts.len(), 1);

    let again = receive::sync(&db, &http, &server.uri()).await.unwrap();
    assert_eq!(again.outcome, Outcome::Unchanged);
    assert!(again.summary().starts_with("No newer backlog verdicts than 2026-10-09"));
    // Off, the library shows none of it, and keeps it.
    db.setting_set(PlayerId::FIRST, crate::db::VERDICTS_RECEIVE, Some("false")).unwrap();
    assert_eq!(named(&db, "Hades").backlog, None);
    assert_eq!(db.community_verdicts_for(named(&db, "Hades").id).unwrap(), None);
    assert_eq!(db.community_snapshot().unwrap().games, 4);
}

/// A snapshot applied from a file has no ETag: the manifest is downloaded,
/// names the same day, and nothing else is, but its ETag is kept.
#[tokio::test]
async fn a_manifest_of_the_same_day_downloads_nothing_more() {
    let (_dir, _cfg, db) = library();
    let (manifest, summary, read) = fixture();
    receive::apply(&db, &read, "a file", None).unwrap();
    let server = service(manifest, summary, 0).await;
    let r = receive::sync(&db, &reqwest::Client::new(), &server.uri()).await.unwrap();
    assert_eq!(r.outcome, Outcome::Unchanged);
    assert_eq!(db.community_verdicts_kept().unwrap(), (Some("2026-10-09".into()), Some("\"one\"".into())));
    assert_eq!(r.snapshot.source.as_deref(), Some("a file"));
}

/// A summary that doesn't match its manifest is refused, and the snapshot
/// kept stays as it was.
#[tokio::test]
async fn a_bad_download_leaves_the_kept_snapshot() {
    let (_dir, _cfg, db) = library();
    let older = format::write_snapshot("2026-10-08", &(0..5).map(|i| verdict(i, HADES, "finished", "yes")).collect::<Vec<_>>()).unwrap();
    receive::apply(&db, &format::read_named_summary(&older.manifest, &older.summary).unwrap(), "a file", None).unwrap();
    let (manifest, mut summary, _) = fixture();
    let last = summary.len() - 10;
    summary[last] ^= 0xff;
    let server = service(manifest, summary, 1).await;
    let err = receive::sync(&db, &reqwest::Client::new(), &server.uri()).await.unwrap_err();
    assert!(err.downcast_ref::<crate::seed::Refused>().is_some(), "{err:#}");
    let kept = db.community_snapshot().unwrap();
    assert_eq!((kept.date.as_deref(), kept.source.as_deref(), kept.games), (Some("2026-10-08"), Some("a file"), 1));
    assert_eq!(named(&db, "Hades").backlog.unwrap().n, 5);
}

#[tokio::test]
async fn a_service_with_nothing_published_yet_says_so() {
    let (_dir, _cfg, db) = library();
    let server = MockServer::start().await;
    let err = receive::sync(&db, &reqwest::Client::new(), &server.uri()).await.unwrap_err();
    assert!(err.to_string().contains("no backlog verdicts are published at"), "{err}");
    assert_eq!(db.community_snapshot().unwrap().date, None);
}

/// Only a newer day replaces the kept snapshot: a service (or a cache in
/// front of it) that answers with an older manifest leaves it as it was.
#[tokio::test]
async fn an_older_snapshot_never_replaces_a_newer_one() {
    let (_dir, _cfg, db) = library();
    let newer = format::write_snapshot("2026-10-10", &(0..5).map(|i| verdict(i, HADES, "finished", "yes")).collect::<Vec<_>>()).unwrap();
    receive::apply(&db, &format::read_named_summary(&newer.manifest, &newer.summary).unwrap(), "a file", None).unwrap();
    let (manifest, summary, _) = fixture();
    let server = service(manifest, summary, 0).await;
    let r = receive::sync(&db, &reqwest::Client::new(), &server.uri()).await.unwrap();
    assert_eq!(r.outcome, Outcome::Unchanged);
    assert_eq!((r.snapshot.date.as_deref(), r.snapshot.games), (Some("2026-10-10"), 1));
    assert!(receive::newer(None, "2026-10-09") && receive::newer(Some("2026-10-09"), "2026-10-10"));
    assert!(!receive::newer(Some("2026-10-10"), "2026-10-10") && !receive::newer(Some("2026-10-10"), "2026-10-09"));
}

/// A manifest or summary far bigger than any snapshot is refused as it
/// downloads, whatever its Content-Length says, and nothing changes.
#[tokio::test]
async fn an_oversize_download_is_refused() {
    let (_dir, _cfg, db) = library();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/verdicts.json"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b' '; format::MAX_MANIFEST + 1]))
        .mount(&server)
        .await;
    let err = receive::sync(&db, &reqwest::Client::new(), &server.uri()).await.unwrap_err();
    assert!(err.to_string().contains("far bigger than UpNext's backlog verdicts"), "{err:#}");
    assert!(err.downcast_ref::<crate::seed::Refused>().is_some());
    assert_eq!(db.community_snapshot().unwrap().date, None);
}

/// An address is held to `verdicts_url`'s rule, before anything is asked:
/// plain http only to this computer.
#[tokio::test]
async fn apply_refuses_plain_http_to_another_computer() {
    let http = reqwest::Client::new();
    for from in ["http://verdicts.example.org", "http://192.168.0.42:8789/verdicts.json"] {
        let err = receive::load(&http, from).await.unwrap_err();
        assert!(err.to_string().contains("http:// only for localhost or 127.0.0.1"), "{from}: {err:#}");
    }
}

/// A plain http service saved before the https rule is ignored when the
/// settings are read, so nothing is downloaded from it and the built-in
/// service applies.
#[test]
fn a_saved_plain_http_service_is_ignored() {
    let (_dir, mut cfg, db) = library();
    cfg.verdicts_url = Some(crate::verdicts::DEFAULT_URL.into());
    db.setting_set(PlayerId::FIRST, "verdicts_url", Some("http://verdicts.example.org")).unwrap();
    assert_eq!(cfg.clone().with_db(&db).unwrap().verdicts_url.as_deref(), Some(crate::verdicts::DEFAULT_URL));
    db.setting_set(PlayerId::FIRST, "verdicts_url", Some("https://verdicts.example.org")).unwrap();
    assert_eq!(cfg.with_db(&db).unwrap().verdicts_url.as_deref(), Some("https://verdicts.example.org"));
}

/// `upnext verdicts apply` of an address: the service's, its manifest's or
/// its summary file's.
#[tokio::test]
async fn apply_reads_an_address() {
    let (manifest, summary, _) = fixture();
    let server = service(manifest, summary, 3).await;
    let http = reqwest::Client::new();
    for (from, with_manifest) in [
        (server.uri(), true),
        (format!("{}/{}", server.uri(), format::MANIFEST), true),
        (format!("{}/verdicts-summary-2026-10-09.jsonl.gz", server.uri()), false),
    ] {
        let (s, m) = receive::load(&http, &from).await.unwrap();
        assert_eq!((s.games.len(), m.is_some()), (4, with_manifest), "{from}");
    }
}

fn verdict(i: u32, appid: u32, outcome: &str, worth: &str) -> Published {
    Published {
        id: format!("z{}{}", "a".repeat(24), char::from(b'a' + i as u8)),
        steam_appid: Some(appid),
        gog_id: None,
        outcome: outcome.into(),
        worth: worth.into(),
        hours: None,
        clicked: None,
        sessions: None,
        limited_time: None,
        factors: vec![],
        stop_point: None,
        stop_note: None,
        stopped_for: vec![],
        text: None,
        vocab: VOCAB,
        month: "2026-10".into(),
    }
}

/// A newer snapshot replaces the kept one whole: a game whose verdicts were
/// all taken down is gone from the library.
#[test]
fn a_new_snapshot_replaces_the_old_one_whole() {
    let (_dir, _cfg, db) = library();
    let (_, _, read) = fixture();
    receive::apply(&db, &read, "a file", Some("\"one\"")).unwrap();
    let only_hades: Vec<Published> = (0..5).map(|i| verdict(i, HADES, "finished", "yes")).collect();
    let snap = format::write_snapshot("2026-10-10", &only_hades).unwrap();
    let newer = format::read_named_summary(&snap.manifest, &snap.summary).unwrap();
    let r = receive::apply(&db, &newer, "another file", None).unwrap();
    assert_eq!((r.snapshot.date.as_deref(), r.snapshot.games, r.snapshot.matched), (Some("2026-10-10"), 1, 1));
    assert_eq!(named(&db, "Hades").backlog.unwrap().n, 5);
    assert_eq!(named(&db, "The Witcher 3: Wild Hunt").backlog, None);
    assert_eq!(db.community_verdicts_kept().unwrap(), (Some("2026-10-10".into()), None), "the old ETag goes with it");
}

/// `upnext verdicts apply` reads a folder, a manifest or a summary file.
#[tokio::test]
async fn apply_reads_a_folder_a_manifest_or_a_summary() {
    let http = reqwest::Client::new();
    let dir = fixture_path("verdicts");
    for (from, with_manifest) in
        [(dir.clone(), true), (dir.join(format::MANIFEST), true), (dir.join("verdicts-summary-2026-10-09.jsonl.gz"), false)]
    {
        let (s, m) = receive::load(&http, from.to_str().unwrap()).await.unwrap();
        assert_eq!((s.games.len(), m.is_some()), (4, with_manifest), "{}", from.display());
    }
    assert!(receive::load(&http, dir.join("text-checks.json").to_str().unwrap()).await.is_err());
}

#[test]
fn the_job_has_nothing_to_do_while_off_or_without_a_service() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = offline_config(dir.path());
    assert!(receive::off(&cfg).unwrap().contains("are off"));
    cfg.verdicts_receive = true;
    assert!(receive::off(&cfg).unwrap().contains("No verdicts service is set"));
    cfg.verdicts_url = Some("http://127.0.0.1:9".into());
    assert_eq!(receive::off(&cfg), None);
}

/// The schedules never start the job while receiving is off or no service
/// is set, and do once both are.
#[test]
fn the_schedule_skips_the_job_while_off() {
    use crate::jobs::Task;
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = offline_config(dir.path());
    cfg.verdicts_url = Some("https://verdicts.example.org".into());
    assert!(!crate::server::scheduler::runs_by_itself(&cfg, Task::Verdicts), "receiving is off");
    cfg.verdicts_receive = true;
    assert!(crate::server::scheduler::runs_by_itself(&cfg, Task::Verdicts));
    cfg.verdicts_url = None;
    assert!(!crate::server::scheduler::runs_by_itself(&cfg, Task::Verdicts), "no service");
    assert!(crate::server::scheduler::runs_by_itself(&cfg, Task::Backup), "other tasks are as before");
}

// -- picks ---------------------------------------------------------------------------

/// The mock library, with its snapshot applied and receiving on.
fn mock() -> (tempfile::TempDir, Db, Vec<GameView>) {
    let dir = tempfile::tempdir().unwrap();
    let cfg = offline_config(dir.path());
    let db = Db::open(&cfg.db_path, cfg.sampled_minutes).unwrap();
    crate::mock::seed(&db).unwrap();
    let games = db.all_games(PlayerId::FIRST).unwrap();
    (dir, db, games)
}

fn find<'a>(games: &'a [GameView], name: &str) -> &'a GameView {
    games.iter().find(|g| g.name == name).unwrap_or_else(|| panic!("{name}"))
}

fn input<'a>(
    games: &'a [GameView],
    request: &'a str,
    series: &'a HashMap<i64, Vec<String>>,
    profile: Option<&'a crate::taste::Profile>,
) -> PromptInput<'a> {
    PromptInput {
        games,
        request,
        count: 3,
        spares: 6,
        exclude_ids: &[],
        avoid: &[],
        on_screen: &[],
        series,
        now: crate::db::now(),
        sources: None,
        recent_ids: &[],
        max_candidates: 500,
        sampled_minutes: 120,
        linux: false,
        seed: 1,
        review_feel: true,
        verdicts: true,
        profile,
        lens: None,
        wishlist: None,
        routine: None,
        asked_input: None,
    }
}

/// The mock snapshot reaches the games it's about: 28 of its 31 games have
/// at least 3 verdicts, and the prompt lists a `bv` line from 5.
#[test]
fn the_prompt_lists_bv_lines_from_five_verdicts_and_counts_them_below_about_me() {
    let (_dir, _db, games) = mock();
    assert_eq!(games.iter().filter(|g| g.backlog.is_some()).count(), 28);
    for (name, n) in [("Stellaris", None), ("Terraria", None), ("Gone Home", Some(3)), ("Dorfromantik", Some(4)), ("Hades", Some(23))] {
        assert_eq!(find(&games, name).backlog.as_ref().map(|b| b.n), n, "{name}");
    }
    let series = HashMap::new();
    let built = prompt::build(&input(&games, "something short", &series, None));
    let line =
        |name: &str| built.prefix.lines().find(|l| l.contains(&format!(" {name} |"))).unwrap_or_else(|| panic!("{name}")).to_string();
    assert!(line("Vampire Survivors").contains("| bv n=9 worth "), "{}", line("Vampire Survivors"));
    assert!(!line("Dorfromantik").contains("bv "), "4 verdicts give no line");
    assert!(built.prefix.contains("bv: other backlog players' verdicts"), "the format says what bv is");
    assert!(built.system.contains("\"bv\" lines are other backlog players' answers"), "{}", built.system);
    let without = prompt::build(&PromptInput { verdicts: false, ..input(&games, "something short", &series, None) });
    assert!(!without.prefix.contains("| bv ") && !without.system.contains("\"bv\""));
}

/// Verdicts reorder and describe the pool, never widen it: a game marked
/// Beat it or hidden, or a hard no (Inscryption is a horror game, and the
/// mock profile says no to horror), stays out whatever its verdicts.
#[test]
fn games_outside_the_pool_stay_out_whatever_their_verdicts() {
    let (_dir, db, games) = mock();
    db.set_override(PlayerId::FIRST, find(&games, "Balatro").id, Some(Override::Done), None).unwrap();
    db.set_hidden(PlayerId::FIRST, find(&games, "Slay the Spire").id, Some(HideScope::Me)).unwrap();
    let before = games;
    let games = db.all_games(PlayerId::FIRST).unwrap();
    let profile = crate::taste::Profile::load(&db, PlayerId::FIRST, &games).unwrap();
    let series = HashMap::new();
    let built = prompt::build(&input(&games, "something short I can finish", &series, Some(&profile)));
    let offered: Vec<&str> = built.candidates.values().map(|g| g.name.as_str()).collect();
    assert!(offered.contains(&"Vampire Survivors") && offered.contains(&"Stardew Valley"), "{offered:?}");
    for out in ["Balatro", "Slay the Spire", "Inscryption"] {
        assert!(find(&before, out).backlog.is_some(), "{out} has verdicts");
        assert!(!offered.contains(&out), "{out} is out of picks");
    }
}

fn drift(subject: &str) -> StatementRow {
    StatementRow {
        id: 0,
        kind: "pattern".into(),
        title_id: None,
        about: Some(format!("drift:{subject}")),
        why: None,
        note: None,
        source: "derived".into(),
        evidence: Some(
            serde_json::json!({"sense": "drift", "subject": subject, "summary": "3 shelved", "share": 0.2, "usual": 0.6, "games": []}),
        ),
        confidence: Some(0.9),
        hidden: false,
        created_at: 0,
        updated_at: 0,
    }
}

/// The request's words against what most players say, weighed by how many
/// answered; short sessions for a short budget; and a drift pattern against
/// a game many players stopped for the same reason.
#[test]
fn verdict_facets_move_the_ranking_softly() {
    let (_dir, _db, games) = mock();
    let short = Ranking::new("something short", false, 1).with_verdicts(true);
    // Hades: 23 verdicts, full weight; Vampire Survivors: 9, under half.
    assert_eq!(short.explain(find(&games, "Hades")).points(Factor::Verdicts), 6.0);
    assert!((short.explain(find(&games, "Vampire Survivors")).points(Factor::Verdicts) - 2.7).abs() < 1e-9);
    assert!(!short.explain(find(&games, "Dorfromantik")).counted(Factor::Verdicts), "4 verdicts have no majorities");
    assert_eq!(short.explain(find(&games, "Hades")).verdicts, ["sessions:short"]);
    let half_hour = Ranking::new("something for 30 minutes", false, 1).with_verdicts(true);
    assert_eq!(half_hour.explain(find(&games, "Hades")).points(Factor::VerdictSessions), 6.0);
    assert!(!Ranking::new("something long", false, 1).with_verdicts(true).explain(find(&games, "Hades")).counted(Factor::Verdicts));

    // Assassin's Creed Odyssey: 12 verdicts, most who stopped stopped for the grind.
    let odyssey = find(&games, "Assassin's Creed Odyssey");
    assert!(odyssey.backlog.as_ref().unwrap().stopped_mostly_for().contains(&"grind"));
    let p = crate::taste::Profile::from_rows(&[drift("facet:grind:heavy")], &games);
    let with = Ranking::new("", false, 1).with_profile(Some(&p), crate::db::now()).with_verdicts(true);
    assert!((with.explain(odyssey).points(Factor::VerdictStopped) + 3.0).abs() < 1e-9, "-5 at full weight is -3 at 12 of 20");
    assert!(!with.explain(find(&games, "Hades")).counted(Factor::VerdictStopped));
    let asked = Ranking::new("something grindy", false, 1).with_profile(Some(&p), crate::db::now()).with_verdicts(true);
    assert!(!asked.explain(odyssey).counted(Factor::VerdictStopped), "the request wins");

    // With the switch off, none of it counts.
    let off = Ranking::new("something short", false, 1).with_profile(Some(&p), crate::db::now());
    for f in [Factor::Verdicts, Factor::VerdictSessions, Factor::VerdictStopped] {
        assert!(!off.explain(find(&games, "Hades")).counted(f) && !off.explain(odyssey).counted(f), "{f:?}");
    }
}

/// With the switch off, the AI gets what it would with no verdicts kept:
/// the same candidates, cut by the same ranking, in the same prompt.
#[test]
fn the_switch_off_gives_the_ai_what_it_would_get_without_verdicts() {
    let (_dir, db, games) = mock();
    let profile = crate::taste::Profile::load(&db, PlayerId::FIRST, &games).unwrap();
    let bare: Vec<GameView> = games
        .iter()
        .cloned()
        .map(|mut g| {
            g.backlog = None;
            g
        })
        .collect();
    let series = HashMap::new();
    for request in ["something short for a weeknight", "a game that clicks fast", ""] {
        let off = PromptInput { verdicts: false, max_candidates: 12, ..input(&games, request, &series, Some(&profile)) };
        let none = PromptInput { verdicts: false, max_candidates: 12, ..input(&bare, request, &series, Some(&profile)) };
        let (a, b) = (prompt::build(&off), prompt::build(&none));
        assert_eq!((a.prefix, a.system), (b.prefix, b.system), "{request:?}");
        // And without AI, the ranking's own lineup is the same.
        let ids = |i: &PromptInput| -> Vec<i64> {
            let screen = crate::recommend::Screen::new(&series, i.second_chances());
            crate::ranking::lineup(i, &[], screen).picks.iter().map(|p| p.id).collect()
        };
        assert_eq!(ids(&off), ids(&none), "{request:?}");
    }
}

/// Second Chance reads the stopped players' reasons beside the reviews,
/// with the rule about them only when a game has the line; the job takes
/// the line off while the switch is off. A snapshot doesn't change what a
/// "Now" was written from, so no "Now" goes out of date with one.
#[test]
fn second_chance_reads_what_stopped_players_stopped_for() {
    let (_dir, _db, games) = mock();
    let odyssey = find(&games, "Assassin's Creed Odyssey");
    let req = crate::second_chance::request(std::slice::from_ref(odyssey), None);
    let asked = req.full_prompt();
    assert!(asked.contains("Other backlog players: Of the 8 backlog players who stopped, 8 stopped for the grind"), "{asked}");
    assert!(req.system.contains("A line \"Other backlog players\""));
    let mut bare = odyssey.clone();
    bare.backlog = None;
    let without = crate::second_chance::request(std::slice::from_ref(&bare), None);
    assert!(!without.full_prompt().contains("Other backlog players") && !without.system.contains("Other backlog players"));
    assert_eq!(crate::second_chance::inputs(odyssey), crate::second_chance::inputs(&bare));
}

// -- the mock library's snapshot -----------------------------------------------------

/// The mock library's snapshot, committed in `tests/fixtures/verdicts/mock/`
/// for the end to end run's stand-in service, written by
/// `UPNEXT_UPDATE_VERDICTS=1 cargo test verdicts`.
#[test]
fn the_committed_mock_snapshot_is_current() {
    let dir = fixture_path("verdicts/mock");
    let snap = crate::mock::verdicts_snapshot().unwrap();
    let manifest = format!("{}\n", serde_json::to_string_pretty(&snap.manifest).unwrap());
    let files = [
        (format::MANIFEST.to_string(), manifest.into_bytes()),
        (snap.manifest.summary.file.clone(), snap.summary.clone()),
        (snap.manifest.all.file.clone(), snap.all.clone()),
    ];
    if std::env::var_os("UPNEXT_UPDATE_VERDICTS").is_some() {
        std::fs::create_dir_all(&dir).unwrap();
        for (name, bytes) in &files {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
        return;
    }
    for (name, bytes) in &files {
        let committed = std::fs::read(dir.join(name)).unwrap_or_default();
        assert!(&committed == bytes, "tests/fixtures/verdicts/mock/{name} is stale: run `UPNEXT_UPDATE_VERDICTS=1 cargo test verdicts`");
    }
    let read = format::read_named_summary(&snap.manifest, &snap.summary).unwrap();
    assert_eq!((read.games.len(), read.header.date.as_str()), (28, "2026-10-01"), "three games have too few to publish");
}
