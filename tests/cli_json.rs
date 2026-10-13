//! The CLI's `--json` output stays JSON with logging turned up: log lines and
//! the timing spans go to stderr, never stdout.

use std::path::Path;
use std::process::{Command, Output};

/// Run `upnext` with its own folders in `dir`, nothing read from this machine.
/// It runs in `dir` too, so a checkout's own `.env` or an old `data/upnext.db`
/// (which `unmoved_data` stops for) doesn't change what it does. The verdicts
/// service is a port on this computer nothing listens on, never the project's.
const NO_SERVICE: &str = "http://127.0.0.1:9";

fn upnext(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_upnext"))
        .args(args)
        .current_dir(dir)
        .env("HOME", dir)
        .env("UPNEXT_CONFIG_DIR", dir)
        .env("UPNEXT_DATA_DIR", dir)
        .env("UPNEXT_CACHE_DIR", dir.join("cache"))
        .env("UPNEXT_STEAM_DIR", dir.join("no-steam"))
        .env("UPNEXT_HEROIC_DIR", dir.join("no-heroic"))
        .env("UPNEXT_VERDICTS_URL", NO_SERVICE)
        .env("RUST_LOG", "upnext=info")
        .env_remove("JOURNAL_STREAM")
        .output()
        .expect("upnext runs")
}

#[test]
fn pick_json_is_json_with_info_logging() {
    let dir = tempfile::tempdir().unwrap();
    let seeded = upnext(dir.path(), &["mock-library"]);
    assert!(seeded.status.success(), "mock-library: {}", String::from_utf8_lossy(&seeded.stderr));

    let out = upnext(dir.path(), &["pick", "--without-ai", "--json", "something short"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "pick: {stderr}");
    let batch: serde_json::Value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("stdout isn't JSON ({e}):\n{}", String::from_utf8_lossy(&out.stdout)));
    assert!(!batch["picks"].as_array().unwrap().is_empty(), "{batch}");
    assert!(stderr.contains("pick") && stderr.contains("close"), "the timing spans go to stderr:\n{stderr}");
}

#[test]
fn verdicts_draft_json_is_json_and_contract_matches_the_docs() {
    let dir = tempfile::tempdir().unwrap();
    let seeded = upnext(dir.path(), &["mock-library"]);
    assert!(seeded.status.success(), "mock-library: {}", String::from_utf8_lossy(&seeded.stderr));
    let marked = upnext(dir.path(), &["mark", "done", "620"]);
    assert!(marked.status.success(), "mark: {}", String::from_utf8_lossy(&marked.stderr));

    let out = upnext(dir.path(), &["verdicts", "draft", "--json", "620"]);
    assert!(out.status.success(), "verdicts draft: {}", String::from_utf8_lossy(&out.stderr));
    let draft: serde_json::Value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("stdout isn't JSON ({e}):\n{}", String::from_utf8_lossy(&out.stdout)));
    assert_eq!(draft["verdict"]["outcome"], "finished", "{draft}");
    assert_eq!(draft["verdict"]["steam_appid"], 620, "{draft}");
    assert_eq!(draft["from"]["outcome"], "mark", "{draft}");

    let out = upnext(dir.path(), &["verdicts", "contract"]);
    assert!(out.status.success());
    let docs = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/verdicts-contract.json")).unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), docs);

    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/verdicts");
    let out = upnext(dir.path(), &["verdicts", "check", fixture.to_str().unwrap()]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "verdicts check: {}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout.contains("4 games") && stdout.contains("sum up to"), "{stdout}");
}

/// Other players' verdicts from the terminal: the mock
/// library comes with its snapshot, `verdicts apply` puts another in its
/// place from a folder, and `prompt --with-verdicts` lists the `bv` lines.
#[test]
fn verdicts_apply_and_the_prompts_bv_lines() {
    let dir = tempfile::tempdir().unwrap();
    let seeded = upnext(dir.path(), &["mock-library"]);
    assert!(seeded.status.success(), "mock-library: {}", String::from_utf8_lossy(&seeded.stderr));

    let with = upnext(dir.path(), &["prompt", "--with-verdicts", "something short"]);
    let stdout = String::from_utf8_lossy(&with.stdout);
    assert!(with.status.success(), "prompt: {}", String::from_utf8_lossy(&with.stderr));
    assert!(stdout.contains("| bv n=9 worth "), "Vampire Survivors: {stdout}");
    let without = upnext(dir.path(), &["prompt", "--without-verdicts", "something short"]);
    assert!(!String::from_utf8_lossy(&without.stdout).contains("| bv "));

    // Nothing answers at the service's address, so the download fails and
    // says where it looked.
    let out = upnext(dir.path(), &["verdicts", "sync"]);
    assert!(!out.status.success(), "sync: {}", String::from_utf8_lossy(&out.stdout));
    let reached = format!("couldn't reach {NO_SERVICE}/verdicts.json");
    assert!(String::from_utf8_lossy(&out.stderr).contains(&reached), "{}", String::from_utf8_lossy(&out.stderr));

    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/verdicts");
    let out = upnext(dir.path(), &["verdicts", "apply", fixture.to_str().unwrap()]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "apply: {}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout.contains("Backlog verdicts of 2026-10-09:") && stdout.contains("of its 4 games"), "{stdout}");

    // The mock library's own snapshot is older: skipped, unless forced.
    let older = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/verdicts/mock");
    let out = upnext(dir.path(), &["verdicts", "apply", older.to_str().unwrap()]);
    assert!(out.status.success(), "apply: {}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stderr).contains("Skipped: this snapshot is of 2026-10-01, and the one kept is of 2026-10-09"));
    assert!(String::from_utf8_lossy(&out.stdout).is_empty());
    let out = upnext(dir.path(), &["verdicts", "apply", "--force", older.to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("Backlog verdicts of 2026-10-01:"), "{}", String::from_utf8_lossy(&out.stderr));
}

/// What a command printed, once it succeeded.
fn ok(out: &Output, what: &str) -> String {
    assert!(out.status.success(), "{what}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A command's stdout as JSON, once it succeeded.
fn json(out: &Output, what: &str) -> serde_json::Value {
    let text = ok(out, what);
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{what}: stdout isn't JSON ({e}):\n{text}"))
}

/// A command that must fail, saying `why` on stderr.
fn refused(out: &Output, what: &str, why: &str) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{what} succeeded: {}", String::from_utf8_lossy(&out.stdout));
    assert!(stderr.contains(why), "{what}: {stderr}");
}

fn names(list: &serde_json::Value) -> Vec<String> {
    list["items"].as_array().unwrap().iter().map(|g| g["name"].as_str().unwrap().to_string()).collect()
}

/// The library from a terminal (code health R-CHECKS-2): `list`, `show`,
/// `mark`, `unmark` and `stats` on the mock library, for each player.
#[test]
fn the_library_from_a_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    ok(&upnext(d, &["mock-library"]), "mock-library");

    let all = json(&upnext(d, &["list", "--json", "--limit", "500"]), "list");
    let total = all["total"].as_u64().unwrap();
    assert!(total > 0 && all["items"].as_array().unwrap().len() as u64 == total, "{all}");
    let sorted = names(&all);
    let mut by_name = sorted.clone();
    by_name.sort_by_key(|n| n.to_lowercase());
    assert_eq!(sorted, by_name, "sorted by name");
    let page = json(&upnext(d, &["list", "--json", "--limit", "5"]), "list --limit 5");
    assert_eq!((page["total"].as_u64(), names(&page)), (Some(total), sorted[..5].to_vec()), "{page}");

    let longest = json(&upnext(d, &["list", "--json", "--sort", "playtime", "--limit", "3"]), "list --sort playtime");
    let minutes: Vec<i64> = longest["items"].as_array().unwrap().iter().map(|g| g["playtime_minutes"].as_i64().unwrap()).collect();
    assert!(minutes.windows(2).all(|w| w[0] >= w[1]) && minutes[0] > 0, "{minutes:?}");
    let shelved = json(&upnext(d, &["list", "--status", "shelved", "--json"]), "list --status shelved");
    assert!(shelved["total"].as_u64().unwrap() > 0, "{shelved}");
    assert!(shelved["items"].as_array().unwrap().iter().all(|g| g["status"] == "shelved"), "{shelved}");
    let gog = json(&upnext(d, &["list", "--source", "gog", "--json"]), "list --source gog");
    assert!(gog["items"].as_array().unwrap().iter().all(|g| g["owned_on"].as_array().unwrap().contains(&"gog".into())), "{gog}");

    let portal = json(&upnext(d, &["show", "620", "--json"]), "show 620");
    assert_eq!((&portal["name"], &portal["appid"]), (&"Portal 2".into(), &620.into()), "{portal}");
    refused(&upnext(d, &["show", "999999"]), "show 999999", "unknown game ref \"999999\"");

    // Marks and hours are each player's own: the mock library's Sam has
    // beaten Portal 2 on their own Steam account, player 1 hasn't.
    let sams = |args: &[&str]| upnext(d, &[&["--player", "Sam"], args].concat());
    let done = |out: Output| names(&json(&out, "list --status done"));
    assert_eq!(done(upnext(d, &["list", "--status", "done", "--json"])), Vec::<String>::new());
    assert_eq!(done(sams(&["list", "--status", "done", "--json"])), ["Portal 2"]);
    let theirs = json(&sams(&["show", "620", "--json"]), "Sam's show 620");
    assert_eq!((&theirs["playtime_minutes"], &portal["playtime_minutes"]), (&900.into(), &600.into()));
    assert!(ok(&upnext(d, &["mark", "done", "620", "--note", "the co-op too"]), "mark").contains("Portal 2 marked done"));
    assert!(ok(&sams(&["unmark", "620"]), "Sam's unmark").contains("Portal 2 override cleared"));
    assert_eq!(done(upnext(d, &["list", "--status", "done", "--json"])), ["Portal 2"]);
    assert_eq!(done(sams(&["list", "--status", "done", "--json"])), Vec::<String>::new());
    assert_eq!(done(upnext(d, &["--player", "2", "list", "--status", "done", "--json"])), Vec::<String>::new(), "by id too");
    refused(&upnext(d, &["--player", "Nobody", "list"]), "--player Nobody", "no player \"Nobody\"");
    ok(&upnext(d, &["unmark", "620"]), "unmark");
    assert_eq!(done(upnext(d, &["list", "--status", "done", "--json"])), Vec::<String>::new());

    let stats = ok(&upnext(d, &["stats"]), "stats");
    assert!(stats.contains(" store copies, ") && stats.contains("untouched") && stats.contains("shelved"), "{stats}");
    // Each player's own count: Sam's library leaves out the game player 1
    // hid from them.
    let theirs = ok(&sams(&["stats"]), "Sam's stats");
    assert!(stats.starts_with("103 games") && theirs.starts_with("102 games"), "{stats}\n{theirs}");

    // Merging and splitting find games in the named player's library:
    // Hotline Miami, which player 1 hid from Sam, is unknown to Sam.
    refused(&sams(&["merge", "219150", "620"]), "Sam's merge of a hidden game", "unknown game ref \"219150\"");
    refused(&sams(&["split", "steam:219150"]), "Sam's split of a hidden game", "unknown game ref \"steam:219150\"");
    let hotline = json(&upnext(d, &["show", "219150", "--json"]), "show 219150");
    assert_eq!(
        (&hotline["name"], hotline["copies"].as_array().unwrap().len()),
        (&"Hotline Miami".into(), 1),
        "a refused merge changes nothing"
    );
    assert!(ok(&sams(&["merge", "413150", "1145360"]), "Sam's merge").contains("merged into Stardew Valley"));
    assert_eq!(json(&sams(&["show", "steam:1145360", "--json"]), "Sam's show steam:1145360")["name"], "Stardew Valley");
    assert!(ok(&sams(&["split", "steam:413150"]), "Sam's split").contains("Stardew Valley split off"));
    let (stardew, hades) = (
        json(&sams(&["show", "413150", "--json"]), "show 413150"),
        json(&sams(&["show", "steam:1145360", "--json"]), "show steam:1145360"),
    );
    assert_eq!((&stardew["name"], &hades["name"], &hades["playtime_minutes"]), (&"Stardew Valley".into(), &"Hades".into(), &120.into()));
}

/// Jobs, settings, players, tokens, the taste profile and the shared data
/// from a terminal (code health R-CHECKS-2), on the mock library.
#[test]
fn jobs_settings_players_and_tokens_from_a_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    ok(&upnext(d, &["mock-library"]), "mock-library");

    let jobs = ok(&upnext(d, &["jobs"]), "jobs");
    assert!(jobs.contains("Back up the database") && jobs.contains("weekly, keep 4"), "{jobs}");
    assert!(jobs.contains("Schedules are off"), "the mock library's schedules are off: {jobs}");
    assert!(ok(&upnext(d, &["jobs", "schedule", "backup", "monthly", "--keep", "3"]), "jobs schedule").contains("monthly, keeping 3"));
    assert!(ok(&upnext(d, &["jobs"]), "jobs").contains("monthly, keep 3"));
    refused(&upnext(d, &["jobs", "schedule", "backup", "sometimes"]), "a bad schedule", "not \"sometimes\"");
    refused(&upnext(d, &["jobs", "schedule", "nonsense", "daily"]), "an unknown job", "nonsense");
    assert!(ok(&upnext(d, &["jobs", "history"]), "jobs history").contains("No jobs yet."));

    // A setting from the app wins over the default, and unset gives it back.
    let setting = |d: &Path| {
        let list = ok(&upnext(d, &["config", "list"]), "config list");
        list.lines()
            .find(|l| l.starts_with("sampled_minutes "))
            .map(|l| l.split_whitespace().skip(1).collect::<Vec<_>>().join(" "))
            .unwrap()
    };
    assert_eq!(setting(d), "120 (default)");
    ok(&upnext(d, &["config", "set", "sampled_minutes", "45"]), "config set");
    assert_eq!(setting(d), "45 (app)");
    refused(&upnext(d, &["config", "set", "sampled_minutes", "lots"]), "a bad value", "sampled_minutes must be a number");
    refused(&upnext(d, &["config", "set", "nonsense", "1"]), "an unknown setting", "unknown setting \"nonsense\"");
    assert_eq!(setting(d), "45 (app)", "a refused value changes nothing");
    ok(&upnext(d, &["config", "unset", "sampled_minutes"]), "config unset");
    assert_eq!(setting(d), "120 (default)");

    // Players: added, renamed and removed after a backup.
    let players = ok(&upnext(d, &["player", "list"]), "player list");
    assert!(players.contains("Player 1") && players.contains("Sam"), "{players}");
    assert!(ok(&upnext(d, &["player", "add", "Kit"]), "player add").contains("Kit added"));
    refused(&upnext(d, &["player", "add", "Kit"]), "a second Kit", "Kit");
    ok(&upnext(d, &["player", "rename", "Kit", "Alex"]), "player rename");
    let removed = ok(&upnext(d, &["player", "remove", "Alex", "--yes"]), "player remove");
    assert!(removed.contains("Alex removed; the backup upnext-manual-"), "{removed}");
    let players = ok(&upnext(d, &["player", "list"]), "player list");
    assert!(!players.contains("Kit") && !players.contains("Alex"), "{players}");

    // A token is printed once, listed by its start, and revoked by name.
    let made = ok(&upnext(d, &["token", "create", "--name", "phone", "--scope", "read"]), "token create");
    let token = made.lines().find(|l| l.starts_with("upnext_")).unwrap_or_else(|| panic!("no token: {made}"));
    let list = ok(&upnext(d, &["token", "list"]), "token list");
    assert!(list.contains("phone") && list.contains(&token[..11]) && !list.contains(token), "{list}");
    refused(&upnext(d, &["token", "create", "--name", "x", "--scope", "admin"]), "an unknown scope", "admin");
    ok(&upnext(d, &["token", "revoke", "phone"]), "token revoke");
    assert!(ok(&upnext(d, &["token", "list"]), "token list").contains("No tokens."));

    let taste = ok(&upnext(d, &["taste"]), "taste");
    assert!(taste.contains("Favourites") && taste.contains("Hard nos"), "{taste}");
    assert!(ok(&upnext(d, &["seed", "status"]), "seed status").contains("No shared data in use."));
    let verdicts = json(&upnext(d, &["verdicts", "list", "--json"]), "verdicts list");
    assert_eq!((&verdicts["has_key"], &verdicts["verdicts"]), (&false.into(), &serde_json::json!([])), "{verdicts}");
    assert_eq!(verdicts["consent"]["licence"], "CC0-1.0", "{verdicts}");
}
