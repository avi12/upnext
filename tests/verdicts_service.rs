//! The backlog verdicts service (`services/verdicts/`)
//! writes snapshots UpNext reads: its tests write
//! `tests/fixtures/verdicts/service/` from the same verdicts as UpNext's own
//! fixture, and this reads it with UpNext's strict reader, checksums,
//! counts and all, and checks the summary is what the full file sums up to.

use std::path::Path;
use std::process::Command;

#[test]
fn the_services_snapshot_reads_with_upnexts_reader() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/verdicts/service");
    let out = Command::new(env!("CARGO_BIN_EXE_upnext"))
        .args(["verdicts", "check"])
        .arg(&fixture)
        .current_dir(dir.path())
        .env("HOME", dir.path())
        .env("UPNEXT_CONFIG_DIR", dir.path())
        .env("UPNEXT_DATA_DIR", dir.path())
        .env("UPNEXT_CACHE_DIR", dir.path().join("cache"))
        .output()
        .expect("upnext runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "verdicts check: {}\n{stdout}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout.contains("4 games with at least 3 verdicts, 37 verdicts published, CC0-1.0"), "{stdout}");
    assert!(stdout.contains("the summary is what they sum up to"), "{stdout}");
}
