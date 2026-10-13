//! Applying shared data on fixture libraries: the mock library and small
//! hand-made ones, with the fixture data in `tests/fixtures/seed/` or data
//! written here.

use super::*;
use crate::db::ImportedGame;
use crate::db::PlayerId;
use crate::digest::DIGEST_VERSION;
use crate::testutil::{fixture_path, offline_config};
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

const FIXTURE: &str = "seed/upnext-seed-2026-10-01.jsonl.gz";

fn fixture_bytes(rel: &str) -> Vec<u8> {
    std::fs::read(fixture_path(rel)).unwrap()
}

/// The mock library with Factorio's and Firewatch's summaries taken out.
fn mock_library() -> (tempfile::TempDir, Config, Db) {
    let dir = tempfile::tempdir().unwrap();
    let cfg = offline_config(dir.path());
    let db = Db::open(&cfg.db_path, cfg.sampled_minutes).unwrap();
    crate::mock::seed(&db).unwrap();
    for name in ["Factorio", "Firewatch"] {
        db.delete_digest(game(&db, name).id).unwrap();
    }
    (dir, cfg, db)
}

fn game(db: &Db, name: &str) -> GameView {
    db.all_games(PlayerId::FIRST).unwrap().into_iter().find(|g| g.name == name).unwrap_or_else(|| panic!("{name} isn't in the library"))
}

fn entry(appid: Option<u32>, gog: Option<&str>, says: &str, version: i64) -> Entry {
    Entry {
        steam_appid: appid,
        gog_id: gog.map(str::to_string),
        name: says.into(),
        people_say: says.into(),
        vibes: None,
        tags: ReviewTags::default(),
        prompt_version: version,
        model: "fixture-model".into(),
        written: "2026-09-30".into(),
        reviews_until: None,
    }
}

/// A gzip'd data file with this header line and these entry lines, as is.
fn raw_file(header: &str, lines: &[&str]) -> Vec<u8> {
    use std::io::Write;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(header.as_bytes()).unwrap();
    gz.write_all(b"\n").unwrap();
    for l in lines {
        gz.write_all(l.as_bytes()).unwrap();
        gz.write_all(b"\n").unwrap();
    }
    gz.finish().unwrap()
}

fn header_for(lines: &[&str], count: usize) -> String {
    let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
    serde_json::to_string(&Header {
        upnext_seed: FORMAT,
        date: "2026-10-02".into(),
        count,
        sha256: sha256_hex(body.as_bytes()),
        licence: LICENCE.into(),
        notice: NOTICE.into(),
    })
    .unwrap()
}

#[test]
fn the_fixture_reads_and_matches_its_manifest() {
    let data = fixture_bytes(FIXTURE);
    let seed = read(&data).unwrap();
    assert_eq!((seed.header.date.as_str(), seed.header.count, seed.entries.len()), ("2026-10-01", 5, 5));
    assert_eq!(seed.header.licence, "CC0-1.0");
    let m: Manifest = serde_json::from_str(&crate::testutil::fixture("seed/upnext-seed.json")).unwrap();
    assert_eq!(manifest(&seed.header, &data), m, "UpNext's own manifest for the file is the one written by hand");
}

#[test]
fn what_is_written_reads_back() {
    let mut e = entry(Some(620), Some("1207658930"), "Portal 2 says.", DIGEST_VERSION);
    e.vibes = Some("Portal 2 feels.".into());
    e.reviews_until = Some("2026-09-01".into());
    let data = write("2026-10-02", std::slice::from_ref(&e)).unwrap();
    let seed = read(&data).unwrap();
    assert_eq!(seed.entries, [e]);
    assert_eq!(seed.header.notice, NOTICE);
    let m = manifest(&seed.header, &data);
    assert_eq!((m.file.as_str(), m.bytes, m.count), ("upnext-seed-2026-10-02.jsonl.gz", data.len() as u64, 1));
}

/// Applying shared data: seeded Steam games get People say, Vibes and
/// tags with no AI call; an older-version entry is used; an install's own
/// summary at the same version stays.
#[test]
fn seeded_steam_games_get_summaries_without_an_ai_call() {
    let (_dir, _cfg, db) = mock_library();
    let own_hades = game(&db, "Hades").digest;
    let seed = read(&fixture_bytes(FIXTURE)).unwrap();
    let done = apply(&db, &seed).unwrap();
    assert_eq!(
        done,
        Applied { date: "2026-10-01".into(), entries: 5, matched: 3, written: 2, kept_own: 1, already: 0 },
        "Factorio and Firewatch written, Hades kept, the GOG-only game and the unowned one not in this library"
    );

    let factorio = game(&db, "Factorio");
    assert!(factorio.digest.as_deref().unwrap().starts_with("Fixture text: a deep factory builder"));
    assert_eq!(
        factorio.vibes.as_deref(),
        Some("Fixture text: belts hum, trains rumble and the factory keeps asking for one more fix before bed.")
    );
    assert_eq!(factorio.seed_date.as_deref(), Some("2026-10-01"));
    assert_eq!(factorio.digest_version, Some(4));
    let tags = factorio.review_tags.as_ref().unwrap();
    assert_eq!(
        (tags.scales["complexity"].as_str(), tags.skills.as_slice(), tags.notes.as_slice()),
        ("deep", &["management".to_string()][..], &["Hours vanish".to_string()][..])
    );
    assert_eq!(tags.aspects.len(), 2);
    assert!(factorio.tags_by_source["reviews"].contains(&"Deep systems".to_string()), "seeded tags are searchable like the AI's own");

    let firewatch = game(&db, "Firewatch");
    assert_eq!((firewatch.digest_version, firewatch.seed_date.as_deref()), (Some(3), Some("2026-10-01")));
    assert_eq!(firewatch.review_tags.unwrap().mood, ["melancholic"], "a mood outside the vocabulary is dropped");
    assert!(!crate::digest::outdated(&game(&db, "Firewatch")), "an older-version seeded summary isn't queued for regeneration");

    let hades = game(&db, "Hades");
    assert_eq!((hades.digest, hades.seed_date), (own_hades, None), "the install's own summary at the same version wins");
    assert!(db.ai_usage_rows(0).unwrap().is_empty(), "no AI call");
}

#[test]
fn a_gog_only_copy_matches_by_its_gog_id() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("upnext.db"), 120).unwrap();
    let copy = |id: &str, name: &str| ImportedGame {
        source: "gog".into(),
        source_id: id.into(),
        launcher: "heroic".into(),
        name: name.into(),
        ..Default::default()
    };
    db.sync_source(PlayerId::FIRST, "heroic", "gog", &[copy("1207664663", "Fixture Quest"), copy("1", "Something Else")]).unwrap();
    let done = apply(&db, &read(&fixture_bytes(FIXTURE)).unwrap()).unwrap();
    assert_eq!((done.matched, done.written), (1, 1));
    let quest = game(&db, "Fixture Quest");
    assert_eq!(quest.digest.as_deref(), Some("Fixture text: a GOG-only adventure, matched by its GOG id."));
    assert_eq!(quest.review_tags.unwrap().mood, ["cozy"]);
    assert!(game(&db, "Something Else").digest.is_none());
}

/// An install's own summary at the same or a newer version always wins; an
/// older one gives way. Newer data replaces older data, never the reverse.
#[test]
fn own_summaries_and_newer_data_win() {
    let (_dir, _cfg, db) = mock_library();
    let (factorio, firewatch, hades) = (game(&db, "Factorio").id, game(&db, "Firewatch").id, game(&db, "Hades").id);
    db.put_digest(factorio, "Own, newer.", "", &Default::default(), "claude-sonnet-5-5", DIGEST_VERSION + 1).unwrap();
    db.put_digest(firewatch, "Own, older.", "", &Default::default(), "claude-sonnet-5-5", 2).unwrap();
    let october = vec![
        entry(Some(427520), None, "Shared Factorio.", DIGEST_VERSION),
        entry(Some(383870), None, "Shared Firewatch.", DIGEST_VERSION),
        entry(Some(1145360), None, "Shared Hades, newer prompt.", DIGEST_VERSION + 1),
    ];
    let done = apply(&db, &read(&write("2026-10-01", &october).unwrap()).unwrap()).unwrap();
    assert_eq!((done.written, done.kept_own), (2, 1));
    assert_eq!(game(&db, "Factorio").digest.as_deref(), Some("Own, newer."));
    assert_eq!(game(&db, "Firewatch").digest.as_deref(), Some("Shared Firewatch."), "an own summary at an older version gives way");
    assert_eq!(game(&db, "Hades").digest.as_deref(), Some("Shared Hades, newer prompt."));

    let again = apply(&db, &read(&write("2026-10-01", &october).unwrap()).unwrap()).unwrap();
    assert_eq!((again.written, again.already), (0, 2), "the same data again changes nothing");

    let november = vec![entry(Some(383870), None, "November's Firewatch.", DIGEST_VERSION)];
    assert_eq!(apply(&db, &read(&write("2026-11-01", &november).unwrap()).unwrap()).unwrap().written, 1);
    assert_eq!(game(&db, "Firewatch").seed_date.as_deref(), Some("2026-11-01"));
    let september = vec![entry(Some(383870), None, "September's Firewatch.", DIGEST_VERSION)];
    let older = apply(&db, &read(&write("2026-09-01", &september).unwrap()).unwrap()).unwrap();
    assert_eq!((older.written, older.already), (0, 1), "older data doesn't replace newer");
    assert_eq!(game(&db, "Firewatch").digest.as_deref(), Some("November's Firewatch."));

    // Writing its own summary takes a title off the shared data.
    db.put_digest(hades, "Own again.", "", &Default::default(), "claude-sonnet-5-5", DIGEST_VERSION + 1).unwrap();
    assert_eq!(game(&db, "Hades").seed_date, None);
}

/// Applying shared data, refused: a damaged file, a bad checksum and anything
/// else off the format refuse the whole file, before anything is written.
#[test]
fn a_corrupt_file_or_a_bad_checksum_is_refused() {
    let refused = |data: &[u8]| read(data).expect_err("refused").0;
    let good = fixture_bytes(FIXTURE);

    let cut = &good[..good.len() - 20];
    assert!(refused(cut).contains("damaged"), "a cut-off download: {}", refused(cut));
    assert!(refused(b"not gzip at all").contains("isn't UpNext's shared data"));
    let mut flipped = good.clone();
    let mid = flipped.len() / 2;
    flipped[mid] ^= 0xff;
    assert!(read(&flipped).is_err(), "a flipped byte fails gzip's own check");

    let line = r#"{"steam_appid":620,"name":"Portal 2","people_say":"Fine.","prompt_version":4,"model":"m","written":"2026-09-30"}"#;
    let changed = line.replace("Fine.", "Changed.");
    let header = header_for(&[line], 1);
    assert!(read(&raw_file(&header, &[line])).is_ok());
    assert!(refused(&raw_file(&header, &[&changed])).contains("checksum"), "an entry changed after the checksum");

    // A field outside the allowlist, even with a checksum to match.
    let review = line.replace(r#""model""#, r#""reviews":["A player's words."],"model""#);
    assert!(refused(&raw_file(&header_for(&[&review], 1), &[&review])).contains("unknown field `reviews`"));
    let art = line.replace(r#""model""#, r#""cover":"https://example.com/a.jpg","model""#);
    assert!(refused(&raw_file(&header_for(&[&art], 1), &[&art])).contains("unknown field `cover`"));

    assert!(refused(&raw_file(&header_for(&[line], 2), &[line])).contains("says it has 2 games but holds 1"));
    let nameless = r#"{"name":"Nowhere","people_say":"Fine.","prompt_version":4,"model":"m","written":"2026-09-30"}"#;
    assert!(refused(&raw_file(&header_for(&[nameless], 1), &[nameless])).contains("neither a Steam appid nor a GOG id"));
    let undated = line.replace("2026-09-30", "last week");
    assert!(refused(&raw_file(&header_for(&[&undated], 1), &[&undated])).contains("date"));

    let newer = header_for(&[line], 1).replace(r#""upnext_seed":1"#, r#""upnext_seed":2"#);
    assert!(refused(&raw_file(&newer, &[line])).contains("update UpNext"));
    let licence = header_for(&[line], 1).replace("CC0-1.0", "CC-BY-SA-4.0");
    assert!(refused(&raw_file(&licence, &[line])).contains("licence"));
}

#[tokio::test]
async fn a_manifest_downloads_and_checks_its_data_file() {
    let server = MockServer::start().await;
    let data = fixture_bytes(FIXTURE);
    let manifest_json = crate::testutil::fixture("seed/upnext-seed.json");
    Mock::given(path("/releases/latest/download/upnext-seed.json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(manifest_json.clone()))
        .mount(&server)
        .await;
    Mock::given(path("/releases/latest/download/upnext-seed-2026-10-01.jsonl.gz"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(data.clone()))
        .mount(&server)
        .await;
    let http = client().unwrap();
    let at = format!("{}/releases/latest/download/upnext-seed.json", server.uri());
    let loaded = load(&http, &at).await.unwrap();
    assert_eq!((loaded.seed.header.count, loaded.data), (5, data.clone()));

    // The data file straight, and from a path.
    assert_eq!(load(&http, &at.replace("upnext-seed.json", "upnext-seed-2026-10-01.jsonl.gz")).await.unwrap().seed.entries.len(), 5);
    assert_eq!(load(&http, fixture_path("seed/upnext-seed.json").to_str().unwrap()).await.unwrap().seed.header.date, "2026-10-01");

    // A manifest whose file doesn't match it.
    let mut wrong: serde_json::Value = serde_json::from_str(&manifest_json).unwrap();
    wrong["bytes"] = (data.len() + 1).into();
    let wrong = wrong.to_string();
    Mock::given(path("/bad/upnext-seed.json")).respond_with(ResponseTemplate::new(200).set_body_string(wrong)).mount(&server).await;
    Mock::given(path("/bad/upnext-seed-2026-10-01.jsonl.gz"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(data))
        .mount(&server)
        .await;
    let e = load(&http, &format!("{}/bad/upnext-seed.json", server.uri())).await.unwrap_err();
    assert!(e.downcast_ref::<Refused>().is_some_and(|r| r.0.contains("doesn't match its manifest")), "{e:#}");

    // A missing file is the server's answer, said plainly.
    let e = load(&http, &format!("{}/gone/upnext-seed.json", server.uri())).await.unwrap_err();
    assert!(e.to_string().contains("no shared game data is published at") && e.to_string().contains("404"), "{e:#}");

    // A redirect is followed only to https, so an https address can't send
    // the server on to plain http (here, back to the mock server).
    Mock::given(path("/moved/upnext-seed.json"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", at.as_str()))
        .mount(&server)
        .await;
    let e = load(&http, &format!("{}/moved/upnext-seed.json", server.uri())).await.unwrap_err();
    assert!(e.downcast_ref::<Refused>().is_some_and(|r| r.0.contains("302")), "{e:#}");

    // A manifest can't send UpNext somewhere else.
    assert!(beside("https://example.com/a/upnext-seed.json", "../etc/passwd").is_err());
    assert!(beside("https://example.com/a/upnext-seed.json", "https://evil.example/x.gz").is_err());
    assert_eq!(beside("https://example.com/a/upnext-seed.json?x=1", "f.jsonl.gz").unwrap(), "https://example.com/a/f.jsonl.gz");
}

/// The kept copy gives games added after the apply their summaries, and
/// removing the data takes its summaries and the copy away.
#[test]
fn the_kept_copy_serves_games_added_later_and_remove_clears_it() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = offline_config(dir.path());
    let db = Db::open(&cfg.db_path, cfg.sampled_minutes).unwrap();
    let steam = |id: u32, name: &str| ImportedGame {
        source: "steam".into(),
        source_id: id.to_string(),
        launcher: "steam".into(),
        name: name.into(),
        appid: Some(id),
        ..Default::default()
    };
    db.sync_source(PlayerId::FIRST, "steam", "steam", &[steam(427520, "Factorio")]).unwrap();
    let data = fixture_bytes(FIXTURE);
    let seed = read(&data).unwrap();
    assert_eq!(apply_and_record(&db, &seed, "a test").unwrap().written, 1);
    keep(&cfg, &seed, &data).unwrap();
    assert_eq!(kept(&cfg).unwrap().file_name().unwrap(), "upnext-seed-2026-10-01.jsonl.gz");

    // Older data doesn't replace the kept copy.
    let older = read(&write("2026-09-01", &[entry(Some(1), None, "Old.", 4)]).unwrap()).unwrap();
    keep(&cfg, &older, &write("2026-09-01", &[entry(Some(1), None, "Old.", 4)]).unwrap()).unwrap();
    assert_eq!(kept(&cfg).unwrap().file_name().unwrap(), "upnext-seed-2026-10-01.jsonl.gz");

    db.sync_source(PlayerId::FIRST, "steam", "steam", &[steam(427520, "Factorio"), steam(383870, "Firewatch")]).unwrap();
    let after = apply_kept(&db, &cfg, "after a sync").unwrap().unwrap();
    assert_eq!((after.written, after.already), (1, 1));
    assert_eq!(game(&db, "Firewatch").seed_date.as_deref(), Some("2026-10-01"));
    assert!(apply_kept(&db, &cfg, "again").unwrap().unwrap().written == 0);

    let s = status(&db, &cfg).unwrap();
    assert_eq!((s.seeded, s.seed_date.as_deref(), s.kept.as_deref()), (2, Some("2026-10-01"), Some("upnext-seed-2026-10-01.jsonl.gz")));
    let sources: Vec<&str> = s.applies.iter().map(|a| a.source.as_str()).collect();
    assert_eq!(sources, ["after a sync", "a test"], "an apply that wrote nothing isn't recorded");

    assert_eq!(remove(&db, &cfg).unwrap(), 2);
    assert!(kept(&cfg).is_none() && game(&db, "Factorio").digest.is_none());
    assert!(apply_kept(&db, &cfg, "nothing kept").unwrap().is_none());
}

/// A sync applies the kept copy to what it added.
#[tokio::test]
async fn a_sync_applies_the_kept_copy() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = offline_config(dir.path());
    let db = Db::open(&cfg.db_path, cfg.sampled_minutes).unwrap();
    let data = fixture_bytes(FIXTURE);
    keep(&cfg, &read(&data).unwrap(), &data).unwrap();
    // Heroic's GOG library, from a folder with Fixture Quest in it.
    let heroic = dir.path().join("heroic");
    std::fs::create_dir_all(heroic.join("store_cache")).unwrap();
    std::fs::write(
        heroic.join("store_cache/gog_library.json"),
        serde_json::json!({"games": [{"app_name": "1207664663", "title": "Fixture Quest", "is_installed": false}]}).to_string(),
    )
    .unwrap();
    let mut cfg = cfg;
    cfg.heroic_dir = Some(heroic);
    let clients = crate::enrich::Clients::with_base_url(&cfg, Some("http://127.0.0.1:9")).unwrap();
    let off = ["steam", "gog", "galaxy"].map(|id| (id.to_string(), false));
    let opts = crate::enrich::SyncOptions { libraries: off.into(), ..Default::default() };
    struct Quiet;
    impl crate::enrich::ProgressSink for Quiet {
        fn update(&self, _: &crate::enrich::Progress) {}
        fn log(&self, _: String) {}
    }
    crate::enrich::run_sync(&db, &cfg, &clients, &opts, &Quiet).await.unwrap();
    let quest = game(&db, "Fixture Quest");
    assert_eq!(
        quest.seed_date.as_deref(),
        Some("2026-10-01"),
        "{:?}",
        db.all_games(PlayerId::FIRST).unwrap().iter().map(|g| &g.name).collect::<Vec<_>>()
    );
}
