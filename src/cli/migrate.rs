//! `migrate-data`: move a checkout's `data` folder (the database, the GOG
//! token and the backups) into the data dir an installed UpNext uses. It
//! copies, checks the copy, and renames the old folder instead of deleting it.

use super::format::{dim, green};
use crate::config::Config;
use anyhow::{Context, Result, bail};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// Every table's row count, by name.
type Counts = Vec<(String, i64)>;

pub fn migrate_data(cfg: &Config, from: &Path) -> Result<()> {
    let from = std::path::absolute(from).with_context(|| format!("finding {}", from.display()))?;
    let source = from.join("upnext.db");
    if !source.is_file() {
        bail!("there's no upnext.db in {}", from.display());
    }
    let target = &cfg.db_path;
    if target.exists() {
        bail!(
            "{} already exists ({}); migrate-data never writes over a database. Move it aside first if it's not the one you want.",
            target.display(),
            describe(target)
        );
    }
    let to = cfg.db_dir();
    if same_folder(&from, &to) {
        bail!("{} is already the data dir", from.display());
    }
    refuse_if_serving(cfg)?;

    let before = counts(&Connection::open(&source).with_context(|| format!("opening {}", source.display()))?)?;
    create_private_dir(&to)?;
    let target_text = target.to_str().context("the data dir's path is not UTF-8")?;
    {
        // VACUUM INTO reads through the WAL, so a copy taken with -wal files
        // beside the database has everything in it.
        let src = Connection::open_with_flags(&source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        src.busy_timeout(std::time::Duration::from_secs(30))?;
        src.execute("VACUUM INTO ?1", [target_text]).with_context(|| format!("copying the database to {}", target.display()))?;
    }
    crate::http::make_private(target);
    let checked = check_copy(target, &before);
    if let Err(e) = checked {
        let _ = std::fs::remove_file(target);
        return Err(e.context(format!("the copy was removed; {} is untouched", from.display())));
    }
    let titles = before.iter().find(|(t, _)| t == "titles").map_or(0, |(_, n)| *n);
    let picks = before.iter().find(|(t, _)| t == "picks").map_or(0, |(_, n)| *n);
    println!(
        "{} database copied to {} and checked: {} tables, {}, {}, the same rows in each table",
        green("✓"),
        target.display(),
        before.len(),
        crate::enrich::plural(titles, "title", "titles"),
        crate::enrich::plural(picks, "pick", "picks")
    );

    let token = from.join("gog_token.json");
    if token.is_file() && !cfg.gog_token_path.exists() {
        copy_file(&token, &cfg.gog_token_path)?;
        crate::http::make_private(&cfg.gog_token_path);
        println!("{} GOG sign-in copied to {}", green("✓"), cfg.gog_token_path.display());
    }
    let token = from.join("epic_token.json");
    let epic = cfg.epic_token_path();
    if token.is_file() && !epic.exists() {
        copy_file(&token, &epic)?;
        crate::http::make_private(&epic);
        println!("{} Epic sign-in copied to {}", green("✓"), epic.display());
    }
    let backups = from.join("backups");
    if backups.is_dir() {
        let into = crate::backups::dir_for(target);
        let n = copy_tree(&backups, &into)?;
        println!("{} {n} backup files copied to {}", green("✓"), into.display());
    }

    let date = chrono::Local::now();
    let name = from.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "data".into());
    let mut moved = from.with_file_name(format!("{name}.migrated-{}", date.format("%Y%m%d")));
    if moved.exists() {
        moved = from.with_file_name(format!("{name}.migrated-{}", date.format("%Y%m%d-%H%M%S")));
    }
    std::fs::rename(&from, &moved).with_context(|| format!("renaming {} to {}", from.display(), moved.display()))?;
    println!("{} {} renamed to {} {}", green("✓"), from.display(), moved.display(), dim("(delete it once UpNext looks right)"));
    println!("{}", dim("Check it with `upnext stats` and `upnext doctor`."));
    Ok(())
}

/// The copy passes `integrity_check` and has the same rows in every table.
fn check_copy(target: &Path, before: &Counts) -> Result<()> {
    let copy = Connection::open(target)?;
    let check: String = copy.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if check != "ok" {
        bail!("the copy failed its integrity check: {check}");
    }
    let after = counts(&copy)?;
    if &after != before {
        bail!("the copy's tables don't match the original's ({before:?} against {after:?})");
    }
    Ok(())
}

fn counts(c: &Connection) -> Result<Counts> {
    let mut stmt = c.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?;
    let tables: Vec<String> = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    tables.into_iter().map(|t| Ok((t.clone(), c.query_row(&format!("SELECT count(*) FROM \"{t}\""), [], |r| r.get(0))?))).collect()
}

/// "2,000 titles" for a database already in the data dir, or what kept it from opening.
fn describe(db: &Path) -> String {
    let titles = Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|c| c.query_row("SELECT count(*) FROM titles", [], |r| r.get::<_, i64>(0)));
    match titles {
        Ok(0) => "an empty library, as UpNext makes when it starts without one".into(),
        Ok(n) => crate::enrich::plural(n, "title", "titles"),
        Err(_) => "not a library UpNext could read".into(),
    }
}

fn same_folder(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// A server on the configured port is most likely the old UpNext, still
/// writing to the database being moved.
fn refuse_if_serving(cfg: &Config) -> Result<()> {
    let addr = format!("{}:{}", cfg.host, cfg.port);
    let Ok(addrs) = std::net::ToSocketAddrs::to_socket_addrs(&addr) else { return Ok(()) };
    for a in addrs {
        if std::net::TcpStream::connect_timeout(&a, std::time::Duration::from_millis(300)).is_ok() {
            bail!(
                "something is listening on {addr}, most likely UpNext: stop it first (Ctrl-C where `upnext serve` runs, or `systemctl --user stop upnext`), so nothing writes to the old database while it moves. If it's something else, run this with UPNEXT_PORT set to a free port."
            );
        }
    }
    Ok(())
}

/// The data dir holds keys and sign-ins: only you can open it.
fn create_private_dir(dir: &Path) -> Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    let mut b = std::fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    b.create(dir).with_context(|| format!("creating {}", dir.display()))
}

/// Copy a file, keeping its permissions and when it was last changed (the
/// backups list shows those dates).
fn copy_file(from: &Path, to: &Path) -> Result<()> {
    std::fs::copy(from, to).with_context(|| format!("copying {} to {}", from.display(), to.display()))?;
    let modified = std::fs::metadata(from)?.modified()?;
    std::fs::File::options().write(true).open(to)?.set_modified(modified)?;
    Ok(())
}

/// Copy a folder's files and folders; the number of files. A file already
/// there is left alone.
fn copy_tree(from: &Path, to: &Path) -> Result<usize> {
    create_private_dir(to)?;
    let mut n = 0;
    for entry in std::fs::read_dir(from).with_context(|| format!("reading {}", from.display()))? {
        let entry = entry?;
        let dest: PathBuf = to.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            n += copy_tree(&entry.path(), &dest)?;
        } else if kind.is_file() && !dest.exists() {
            copy_file(&entry.path(), &dest)?;
            // Backups hold the API keys, as the database does.
            crate::http::make_private(&dest);
            n += 1;
        }
    }
    Ok(n)
}

/// When the data dir has no database yet but the current folder's `./data`
/// has one (a checkout from before UpNext was installed): what to do about
/// it. Commands stop with this rather than start an empty library, which
/// `migrate-data` would then refuse to write over. Nothing moves on its own.
pub fn unmoved_data(cfg: &Config) -> Option<String> {
    unmoved_data_in(cfg, Path::new("."))
}

fn unmoved_data_in(cfg: &Config, cwd: &Path) -> Option<String> {
    let old = cwd.join("data");
    if cfg.source("db_path") != "default" || cfg.db_path.exists() || !old.join("upnext.db").is_file() {
        return None;
    }
    let from = std::path::absolute(&old).unwrap_or(old);
    Some(format!(
        "UpNext keeps its database in {} now, and there's none there yet. Move the one in {} with `upnext migrate-data --from {}`, or set UPNEXT_DB={} to keep using it where it is.",
        cfg.db_dir().display(),
        from.display(),
        from.display(),
        from.join("upnext.db").display()
    ))
}

/// When `./.env` is there but no longer read (release builds read only the
/// config dir's).
pub fn env_hint(cfg: &Config) -> Option<String> {
    let local = Path::new(".env");
    let config_env = cfg.config_dir.join(".env");
    if cfg!(debug_assertions) || !local.is_file() || config_env.is_file() || same_folder(Path::new("."), &cfg.config_dir) {
        return None;
    }
    Some(format!(
        "./.env isn't read any more; UpNext reads {}. Move it with: install -Dm600 .env {}",
        config_env.display(),
        config_env.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::db::PlayerId;
    use crate::testutil::offline_config;

    /// A checkout's data folder: a database with a title in it (left with
    /// its WAL, as a server that's still open leaves it), a GOG token and a backup.
    fn old_folder(root: &Path) -> PathBuf {
        let old = root.join("checkout/data");
        let db = Db::open(&old.join("upnext.db"), 120).unwrap();
        let game = crate::db::ImportedGame {
            source: "steam".into(),
            source_id: "620".into(),
            launcher: "steam".into(),
            name: "Portal 2".into(),
            appid: Some(620),
            playtime_minutes: Some(1260),
            ..Default::default()
        };
        db.sync_source(PlayerId::FIRST, "steam", "steam", &[game]).unwrap();
        std::fs::write(old.join("gog_token.json"), "{}").unwrap();
        std::fs::write(old.join("epic_token.json"), "{}").unwrap();
        std::fs::create_dir_all(old.join("backups")).unwrap();
        std::fs::write(old.join("backups/upnext-manual-20261001-161146.db"), "backup").unwrap();
        // Windows can't rename a folder with a file open in it, so there
        // the database is closed (and its WAL folded in) first.
        #[cfg(unix)]
        std::mem::forget(db);
        #[cfg(not(unix))]
        drop(db);
        old
    }

    fn config(root: &Path) -> Config {
        let mut cfg = offline_config(&root.join("share/upnext"));
        // Nothing listens on port 9.
        cfg.port = 9;
        cfg
    }

    #[test]
    fn moves_the_database_token_and_backups_and_keeps_the_old_folder() {
        let root = tempfile::tempdir().unwrap();
        let old = old_folder(root.path());
        let cfg = config(root.path());
        migrate_data(&cfg, &old).unwrap();

        let db = Db::open(&cfg.db_path, 120).unwrap();
        assert_eq!(db.stats(PlayerId::FIRST).unwrap().total, 1, "the game came along");
        assert!(cfg.gog_token_path.is_file());
        assert!(cfg.epic_token_path().is_file(), "the Epic sign-in too");
        let backups = crate::backups::dir_for(&cfg.db_path);
        assert_eq!(std::fs::read_to_string(backups.join("upnext-manual-20261001-161146.db")).unwrap(), "backup");
        assert!(!old.exists(), "renamed");
        let renamed: Vec<String> = std::fs::read_dir(root.path().join("checkout"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(renamed.len(), 1);
        assert!(renamed[0].starts_with("data.migrated-"), "{renamed:?}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&cfg.db_path), 0o600);
            assert_eq!(mode(&cfg.db_dir()), 0o700);
            assert_eq!(mode(&backups.join("upnext-manual-20261001-161146.db")), 0o600);
        }
    }

    #[test]
    fn never_writes_over_a_database() {
        let root = tempfile::tempdir().unwrap();
        let old = old_folder(root.path());
        let cfg = config(root.path());
        Db::open(&cfg.db_path, 120).unwrap();
        let err = migrate_data(&cfg, &old).unwrap_err().to_string();
        assert!(err.contains("already exists (an empty library"), "{err}");
        assert!(old.join("upnext.db").is_file(), "nothing moved");
    }

    #[test]
    fn refuses_while_a_server_listens() {
        let root = tempfile::tempdir().unwrap();
        let old = old_folder(root.path());
        let mut cfg = config(root.path());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        cfg.port = listener.local_addr().unwrap().port();
        let err = migrate_data(&cfg, &old).unwrap_err().to_string();
        assert!(err.starts_with("something is listening on 127.0.0.1:"), "{err}");
        assert!(!cfg.db_path.exists() && old.join("upnext.db").is_file());
    }

    #[test]
    fn a_checkouts_data_stops_commands_until_it_moves() {
        let root = tempfile::tempdir().unwrap();
        let checkout = root.path().join("checkout");
        let cfg = config(root.path());
        assert_eq!(unmoved_data_in(&cfg, &checkout), None, "no ./data");
        let old = old_folder(root.path());
        let why = unmoved_data_in(&cfg, &checkout).expect("it says what to do");
        let shown = std::path::absolute(&old).unwrap();
        assert!(why.contains(&format!("upnext migrate-data --from {}", shown.display())), "{why}");
        migrate_data(&cfg, &old).unwrap();
        assert_eq!(unmoved_data_in(&cfg, &checkout), None, "moved");
    }
}
