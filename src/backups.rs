//! The backups folder next to the database (`~/.local/share/upnext/backups`
//! on Linux): what's in it and why each file was made, writing and
//! trimming backups, the copy without keys, and the checks a file passes
//! before it replaces the database. The jobs that restore and import are in
//! `jobs::backup`; the swap itself is `Db::replace_with`.
//!
//! Why a backup was made is in its name (`Kind`). Notes live in
//! `backups.json` in the folder, not in the database, because a restore
//! replaces the database.

use crate::db::{BASELINE_VERSION, Db, SCHEMA_VERSION};
use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Backups of each automatic kind kept: before an upgrade, a restore or an import.
pub const KEEP_AUTOMATIC: usize = 5;

/// The notes, by file name.
const INDEX: &str = "backups.json";

/// Files the folder holds for a moment (an upload, a copy being checked).
/// They start with a dot, so the list leaves them out.
pub const WORK_PREFIX: &str = ".work-";

/// Why a backup was made, from its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
#[schema(as = BackupKind)]
pub enum Kind {
    /// The backup job's rotating copies (Jobs, "Back up the database";
    /// `upnext backup`), kept by the job's setting.
    Scheduled,
    /// Backups, "Back up now", or `upnext backup --note`, and any file
    /// UpNext didn't name, like the `upnext-pre-*` copies taken by hand
    /// before there was a Backups section. Kept until deleted.
    Manual,
    /// Before the database was upgraded to a newer schema.
    Upgrade,
    /// Before a restore replaced the database.
    Restore,
    /// Before an import replaced it.
    Import,
}

impl Kind {
    const NAMED: [Kind; 5] = [Self::Scheduled, Self::Manual, Self::Upgrade, Self::Restore, Self::Import];

    pub fn prefix(self) -> &'static str {
        match self {
            Self::Scheduled => "upnext-scheduled-",
            Self::Manual => "upnext-manual-",
            Self::Upgrade => "upnext-pre-upgrade-",
            Self::Restore => "upnext-pre-restore-",
            Self::Import => "upnext-pre-import-",
        }
    }

    pub fn of(name: &str) -> Self {
        Self::NAMED.into_iter().find(|k| name.starts_with(k.prefix())).unwrap_or(Self::Manual)
    }

    /// A new file name, without `.db`: "upnext-manual-20261001-101500".
    pub fn stem(self) -> String {
        format!("{}{}", self.prefix(), chrono::Local::now().format("%Y%m%d-%H%M%S"))
    }

    /// Backups of this kind kept by `write`; None for the ones kept until
    /// deleted. The scheduled ones keep the backup job's setting instead.
    pub fn keep(self) -> Option<usize> {
        match self {
            Self::Upgrade | Self::Restore | Self::Import => Some(KEEP_AUTOMATIC),
            Self::Scheduled | Self::Manual => None,
        }
    }
}

/// "upnext-pre-upgrade-v18-20261001-101500"
fn upgrade_stem(to: i64) -> String {
    format!("{}v{to}-{}", Kind::Upgrade.prefix(), chrono::Local::now().format("%Y%m%d-%H%M%S"))
}

/// `…/upnext/backups` for `…/upnext/upnext.db`.
pub fn dir_for(db: &Path) -> PathBuf {
    db.parent().unwrap_or(Path::new(".")).join("backups")
}

/// What the list says about a backup besides its kind.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = BackupAbout)]
pub struct About {
    /// The person's note ("before trying the new prompt").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Why UpNext made it, when it made it on its own: "Before restoring
    /// upnext-manual-20261001-101500.db".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// One backup in the folder, as the list shows it.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct Backup {
    pub name: String,
    pub kind: Kind,
    /// When the file was written (Unix seconds).
    pub made_at: i64,
    pub bytes: u64,
    /// Its schema version, from the file's header; None when it isn't a
    /// SQLite database.
    pub schema: Option<i64>,
    #[serde(flatten)]
    pub about: About,
}

/// A backup to write.
#[derive(Debug, Clone)]
pub struct New {
    pub kind: Kind,
    pub about: About,
    /// This kind's backups kept; older ones are removed. None keeps them all.
    pub keep: Option<usize>,
    /// The job writing it. Its row in the copy is marked done, since the
    /// copy exists only because it got that far; restored, it would
    /// otherwise look like a job that never ended.
    pub job: Option<i64>,
}

/// What one backup did.
#[derive(Debug)]
pub struct Made {
    pub name: String,
    pub bytes: u64,
    /// Older backups of its kind, removed to keep the newest few.
    pub removed: Vec<String>,
}

/// Write a backup of the database at `db` into `dir`, check it, and remove
/// its kind's older backups beyond `new.keep`. `VACUUM INTO` on a connection
/// of its own, so the server's stays free (a 40 MB database takes well under a second). The
/// copy is written and checked under a work file's name and only then takes
/// its own, so a backup that fails (a full disk) leaves nothing that looks
/// like one; older ones go only after that. A second backup in the same
/// second is `<stem>_2.db`, which sorts after the first.
pub fn write(db: &Path, dir: &Path, stem: &str, new: &New) -> Result<Made> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let name = (1..10)
        .map(|n| if n == 1 { format!("{stem}.db") } else { format!("{stem}_{n}.db") })
        .find(|n| !dir.join(n).exists())
        .with_context(|| format!("too many backups named {stem}"))?;
    let work = work_path(dir)?;
    if let Err(e) = write_checked(db, &work, &name, new.job) {
        remove_work_file(&work);
        return Err(e);
    }
    let out = dir.join(&name);
    if let Err(e) = std::fs::rename(&work, &out) {
        remove_work_file(&work);
        return Err(e).with_context(|| format!("writing {}", out.display()));
    }
    if new.about != About::default() {
        annotate(dir, &name, new.about.clone())?;
    }
    let bytes = std::fs::metadata(&out)?.len();
    let removed = match new.keep {
        Some(keep) => remove_old(dir, new.kind, keep.max(1))?,
        None => vec![],
    };
    Ok(Made { name, bytes, removed })
}

/// Copy the database at `db` to the work file `out` for the backup `name`,
/// and check the copy.
fn write_checked(db: &Path, out: &Path, name: &str, job: Option<i64>) -> Result<()> {
    let out_text = out.to_str().context("the backups folder's path is not UTF-8")?;
    let source = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .with_context(|| format!("opening {}", db.display()))?;
    source.busy_timeout(std::time::Duration::from_secs(30))?;
    source.execute("VACUUM INTO ?1", [out_text]).with_context(|| format!("writing {name}"))?;
    // It holds the API keys and sign-ins, as the database does.
    crate::http::make_private(out);
    let copy = Connection::open(out)?;
    if let Some(job) = job
        && has_table(&copy, "jobs")?
    {
        copy.execute(
            "UPDATE jobs SET status='done', finished_at=?2, current=NULL, message='Done', summary=?3 WHERE id=?1 AND status='running'",
            params![job, crate::db::now(), name],
        )?;
    }
    let check: String = copy.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    drop(copy);
    if check != "ok" {
        anyhow::bail!("the backup {name} failed its check ({check}); older backups were kept");
    }
    Ok(())
}

/// Back up the database before `Db::open` upgrades it from `from` to this
/// build's schema. A failure stops the upgrade.
pub fn before_upgrade(db: &Path, from: i64) -> Result<Made> {
    let new = New {
        kind: Kind::Upgrade,
        about: About { note: None, reason: Some(format!("Before upgrading from v{from} to v{SCHEMA_VERSION}")) },
        keep: Kind::Upgrade.keep(),
        job: None,
    };
    let made = write(db, &dir_for(db), &upgrade_stem(SCHEMA_VERSION), &new)
        .with_context(|| format!("backing up the database before upgrading it from v{from} to v{SCHEMA_VERSION}, so it wasn't upgraded"))?;
    tracing::info!("backed up the database to {} before upgrading it from v{from} to v{SCHEMA_VERSION}", made.name);
    Ok(made)
}

/// What sorts a kind's backups written in the same instant: the name after
/// its prefix, without an upgrade's "v18-" (so v9's sort before v18's by
/// time, not by text).
fn time_key(name: &str, kind: Kind) -> &str {
    let rest = name.strip_prefix(kind.prefix()).unwrap_or(name);
    match rest.split_once('-') {
        Some((v, after)) if v.len() > 1 && v.starts_with('v') && v[1..].bytes().all(|b| b.is_ascii_digit()) => after,
        _ => rest,
    }
}

/// Remove a kind's backups beyond the newest `keep`, oldest first. Only
/// files that are databases count, so a file a failed backup left behind
/// (before backups were written under a work file's name) never pushes a
/// good one out; it stays in the list, where it can be deleted.
fn remove_old(dir: &Path, kind: Kind, keep: usize) -> Result<Vec<String>> {
    // Newest first by when each was written, as the list orders them. A
    // name carries the local time, which the autumn's clock change or a new
    // time zone can move back, so it only breaks a tie (B-CORE-7).
    let written = |n: &str| std::fs::metadata(dir.join(n)).and_then(|m| m.modified()).ok();
    let mut ours: Vec<(Option<std::time::SystemTime>, String)> = names(dir)?
        .into_iter()
        .filter(|n| n.starts_with(kind.prefix()) && header_version(&dir.join(n)).is_some())
        .map(|n| (written(&n), n))
        .collect();
    ours.sort_unstable_by(|(at_a, a), (at_b, b)| {
        at_b.cmp(at_a).then_with(|| time_key(b, kind).cmp(time_key(a, kind))).then_with(|| b.cmp(a))
    });
    let mut removed = vec![];
    for (_, name) in ours.into_iter().skip(keep) {
        remove(dir, &name)?;
        removed.push(name);
    }
    removed.reverse();
    Ok(removed)
}

/// The file names of the backups in the folder; empty when there's no folder.
fn names(dir: &Path) -> Result<Vec<String>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };
    Ok(entries.filter_map(|e| e.ok()?.file_name().into_string().ok()).filter(|n| is_backup_name(n)).collect())
}

/// A file the list shows: `*.db`, not hidden, a plain name.
fn is_backup_name(name: &str) -> bool {
    name.ends_with(".db") && name.len() > 3 && !name.starts_with('.') && !name.contains(['/', '\\'])
}

/// Every backup in the folder, newest first.
pub fn list(dir: &Path) -> Result<Vec<Backup>> {
    let index = read_index(dir);
    let mut out: Vec<Backup> = names(dir)?
        .into_iter()
        .filter_map(|name| {
            let path = dir.join(&name);
            let meta = std::fs::metadata(&path).ok().filter(|m| m.is_file())?;
            let made_at = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs() as i64);
            Some(Backup {
                kind: Kind::of(&name),
                made_at,
                bytes: meta.len(),
                schema: header_version(&path),
                about: index.get(&name).cloned().unwrap_or_default(),
                name,
            })
        })
        .collect();
    out.sort_unstable_by(|a, b| b.made_at.cmp(&a.made_at).then_with(|| b.name.cmp(&a.name)));
    Ok(out)
}

/// The path of a backup in the folder, refusing names that aren't one (a
/// path, a hidden file) or don't exist.
pub fn find(dir: &Path, name: &str) -> Result<PathBuf, Refused> {
    let path = dir.join(name);
    if is_backup_name(name) && path.is_file() { Ok(path) } else { Err(Refused(format!("there is no backup named {name:?}"))) }
}

/// Delete a backup and its note.
pub fn delete(dir: &Path, name: &str) -> Result<()> {
    find(dir, name)?;
    remove(dir, name)
}

fn remove(dir: &Path, name: &str) -> Result<()> {
    std::fs::remove_file(dir.join(name)).with_context(|| format!("removing {name}"))?;
    for extra in ["-wal", "-shm", "-journal"] {
        let _ = std::fs::remove_file(dir.join(format!("{name}{extra}")));
    }
    let mut index = read_index(dir);
    if index.remove(name).is_some() {
        write_index(dir, &index)?;
    }
    Ok(())
}

fn read_index(dir: &Path) -> BTreeMap<String, About> {
    let path = dir.join(INDEX);
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!("{} can't be read ({e}); the backups' notes are left out", path.display());
            BTreeMap::new()
        }),
        Err(_) => BTreeMap::new(),
    }
}

/// Write the index through a temporary file, so a crash never leaves half of it.
fn write_index(dir: &Path, index: &BTreeMap<String, About>) -> Result<()> {
    let path = dir.join(INDEX);
    let tmp = dir.join(format!("{WORK_PREFIX}{INDEX}"));
    std::fs::write(&tmp, serde_json::to_string_pretty(index)?).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

fn annotate(dir: &Path, name: &str, about: About) -> Result<()> {
    let mut index = read_index(dir);
    index.insert(name.to_string(), about);
    write_index(dir, &index)
}

/// The schema version in a database file's header, read without opening it
/// (so nothing is written next to it). None when it isn't a SQLite database.
pub fn header_version(path: &Path) -> Option<i64> {
    use std::io::Read;
    let mut head = [0u8; 64];
    std::fs::File::open(path).ok()?.read_exact(&mut head).ok()?;
    head.starts_with(b"SQLite format 3\0").then(|| i64::from(i32::from_be_bytes([head[60], head[61], head[62], head[63]])))
}

/// A file that can't replace the database, and why, said plainly.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Refused(pub String);

/// Refuse a file whose header isn't a database this build can read. Its schema version otherwise.
pub fn check_header(path: &Path) -> Result<i64, Refused> {
    let Some(version) = header_version(path) else {
        return Err(Refused("that file isn't a database; an UpNext backup is a SQLite file, usually named upnext-….db".into()));
    };
    if version > SCHEMA_VERSION {
        return Err(Refused(format!(
            "that database is from a newer UpNext (schema v{version}); this one reads up to v{SCHEMA_VERSION}, so update UpNext first"
        )));
    }
    // 0 is a SQLite database UpNext never wrote to, or one with a library
    // from before UpNext numbered its schema, which is refused here like the
    // other old ones rather than in the restore job. `stage` says what any
    // other file holds.
    if (version > 0 && version < BASELINE_VERSION) || (version == 0 && holds_library(path)) {
        return Err(Refused(format!("that database {}", crate::db::too_old(version))));
    }
    Ok(version)
}

/// Whether the database at `path` has a games table, read without writing.
fn holds_library(path: &Path) -> bool {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|c| c.query_row("SELECT 1 FROM sqlite_master WHERE type='table' AND name='games'", [], |_| Ok(())).optional())
        .is_ok_and(|found| found.is_some())
}

/// A checked copy of a backup or an upload, in the backups folder, ready to
/// replace the database. The file goes when this does.
#[derive(Debug)]
pub struct Staged {
    pub path: PathBuf,
    /// Its schema version before `prepare` upgrades it.
    pub schema: i64,
}

impl Staged {
    /// Hand the file over to whoever removes it later (an import job).
    pub fn hand_over(mut self) -> PathBuf {
        std::mem::take(&mut self.path)
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if !self.path.as_os_str().is_empty() {
            remove_work_file(&self.path);
        }
    }
}

/// A new name for a file the folder holds for a moment.
pub fn work_path(dir: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok(dir.join(format!("{WORK_PREFIX}{}.db", uuid::Uuid::new_v4().simple())))
}

/// Remove a file the folder held for a moment, with whatever SQLite left next to it.
pub fn remove_work_file(path: &Path) {
    let _ = std::fs::remove_file(path);
    for extra in ["-wal", "-shm", "-journal"] {
        let mut p = path.as_os_str().to_owned();
        p.push(extra);
        let _ = std::fs::remove_file(PathBuf::from(p));
    }
}

/// Remove work files a crash or a closed server left behind, after a day.
pub fn clean_work_files(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let day_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(86_400);
    for e in entries.flatten() {
        let old = e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t < day_ago);
        if old && e.file_name().to_str().is_some_and(|n| n.starts_with(WORK_PREFIX) && n != format!("{WORK_PREFIX}{INDEX}")) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Copy `source` into the backups folder and check the copy: a database
/// this build can read, not damaged, with UpNext's own tables. The copy is
/// made with `VACUUM INTO`, so whatever `source` was (a file with a
/// write-ahead log, an upload), the copy is one plain file.
pub fn stage(source: &Path, dir: &Path) -> Result<Staged> {
    let schema = check_header(source)?;
    let path = work_path(dir)?;
    let staged = Staged { path, schema };
    let out = staged.path.to_str().context("the backups folder's path is not UTF-8")?;
    let copied =
        Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY).and_then(|c| c.execute("VACUUM INTO ?1", [out]).map(|_| ()));
    if let Err(e) = copied {
        return Err(Refused(format!("that database can't be read: {e}")).into());
    }
    crate::http::make_private(&staged.path);
    let c = Connection::open(&staged.path)?;
    let check: String = c.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    if check != "ok" {
        return Err(Refused(format!("that database is damaged ({check})")).into());
    }
    if !has_table(&c, "titles")? || !has_table(&c, "games")? {
        return Err(Refused("that's a SQLite database, but not UpNext's: it has no titles or games".into()).into());
    }
    Ok(staged)
}

fn has_table(c: &Connection, table: &str) -> Result<bool> {
    Ok(c.query_row("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1", [table], |_| Ok(())).optional()?.is_some())
}

/// The settings a copy without keys leaves out: the secret keys, and the
/// Steam sign-in (`steam_id`), which names the account.
fn keyless_settings() -> Vec<&'static str> {
    crate::config::secret_keys().chain(["steam_id"]).chain(crate::config::path_keys()).chain(HOUSEHOLD_ADDRESSES).collect()
}

/// Settings that name the household's own computers and services: its
/// address from outside, the computer's name on the network, its proxies,
/// its identity provider and the groups it signs people in by, its own
/// backlog verdicts service and its own AI server. A copy without keys may
/// go to someone else.
const HOUSEHOLD_ADDRESSES: [&str; 9] = [
    "public_url",
    "network_name",
    "trusted_proxies",
    "oidc_issuer",
    "oidc_client_id",
    "oidc_name",
    "oidc_allowed_groups",
    "verdicts_url",
    "openai_base_url",
];

/// This install's keys and sign-ins, kept when a restored or imported
/// database has none of its own, and its API tokens, always kept.
#[derive(Debug, Default)]
pub struct Keys {
    settings: Vec<(String, String)>,
    /// The Steam profile (`meta.profile`), which goes with `steam_id`.
    profile: Option<String>,
    /// The API tokens, which replace the copy's: a revoked token stays
    /// revoked, and devices paired since the copy was made keep working.
    tokens: Vec<crate::db::StoredToken>,
    /// Each player's backlog verdicts key, kept for a player the
    /// copy has with none, so their sent verdicts can still be taken down.
    verdicts_keys: Vec<(crate::db::PlayerId, String)>,
    /// This install's own secrets in `meta` (`db::INSTALL_SECRETS`), which
    /// replace the copy's.
    secrets: Vec<(&'static str, String)>,
    /// Each player's GOG and Epic sign-ins, kept for a player the
    /// copy has who isn't signed in there: before they moved into the
    /// database a restore never touched them.
    sign_ins: Vec<crate::db::SignIn>,
}

/// What `prepare` kept of this install's.
#[derive(Debug, Default)]
pub struct Kept {
    /// The settings the copy had none of.
    pub settings: Vec<String>,
    /// This install's API tokens, which took the place of the copy's.
    pub tokens: usize,
    /// The copy's API tokens, left out.
    pub copy_tokens: usize,
}

impl Keys {
    pub fn of(db: &Db) -> Result<Self> {
        let all = db.settings_all(crate::db::PlayerId::FIRST)?;
        let settings = keyless_settings().into_iter().filter_map(|k| Some((k.to_string(), all.get(k)?.clone()))).collect();
        Ok(Self {
            settings,
            profile: db.meta_get("profile")?,
            tokens: db.stored_tokens()?,
            verdicts_keys: db.verdicts_keys()?,
            secrets: crate::db::INSTALL_SECRETS
                .into_iter()
                .filter_map(|k| Some(db.meta_get(k).transpose()?.map(|v| (k, v))))
                .collect::<Result<_>>()?,
            sign_ins: db.sign_ins()?,
        })
    }
}

/// Bring a staged copy up to this build: upgrade its schema, keep this
/// install's keys and sign-ins where it has none (a copy without keys, or
/// a backup from before a key was set), and put this install's API tokens
/// in place of its own.
pub fn prepare(staged: &Staged, keys: &Keys) -> Result<Kept> {
    let c = Connection::open(&staged.path)?;
    Db::upgrade(&c).context("upgrading it to this build's schema")?;
    let tx = c.unchecked_transaction()?;
    let mut kept = vec![];
    let first = crate::db::PlayerId::FIRST;
    for (key, value) in &keys.settings {
        if crate::db::setting_get_in(&tx, first, key)?.is_none() {
            crate::db::setting_set_in(&tx, first, key, Some(value))?;
            kept.push(key.clone());
        }
    }
    if kept.iter().any(|k| k == "steam_id")
        && let Some(profile) = &keys.profile
    {
        tx.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('profile', ?1)", [profile])?;
    }
    if !crate::db::keep_verdicts_keys(&tx, &keys.verdicts_keys)?.is_empty() {
        kept.push(crate::db::VERDICTS_KEY.to_string());
    }
    kept.extend(crate::db::keep_sign_ins(&tx, &keys.sign_ins)?.into_iter().map(str::to_string));
    let copy_tokens = crate::db::StoredToken::replace_in(&tx, &keys.tokens)?;
    for key in crate::db::INSTALL_SECRETS {
        tx.execute("DELETE FROM meta WHERE key=?1", [key])?;
    }
    for (key, value) in &keys.secrets {
        tx.execute("INSERT INTO meta(key, value) VALUES (?1, ?2)", [key, value.as_str()])?;
    }
    tx.commit()?;
    Ok(Kept { settings: kept, tokens: keys.tokens.len(), copy_tokens })
}

/// Leave out of a copy what a download without keys mustn't carry: the API
/// keys and secrets, the Steam sign-in, the Steam profile (persona name
/// and avatar), the players' names, PINs and identity provider accounts,
/// the API tokens' hashes, the connections' secrets, which hold the GOG
/// and Epic sign-ins, the store accounts' names and ids, and every setting
/// that names a file or folder or the household's own addresses, and every
/// job's log line, result and error and every AI call's error
/// (`strip_messages`).
pub fn strip_keys(c: &Connection) -> Result<()> {
    for key in keyless_settings() {
        c.execute("DELETE FROM settings WHERE key=?1", [key])?;
    }
    c.execute("DELETE FROM meta WHERE key='profile'", [])?;
    // Whoever has the copy could make a "Send to UpNext" bookmark this
    // install would take (L10), or sign a player cookie for anyone.
    for key in crate::db::INSTALL_SECRETS {
        c.execute("DELETE FROM meta WHERE key=?1", [key])?;
    }
    if has_table(c, "api_tokens")? {
        c.execute("DELETE FROM api_tokens", [])?;
    }
    // From v32 the AI keys and the SteamID live on the players' accounts.
    if has_table(c, "ai_accounts")? {
        crate::db::strip_account_keys(c)?;
    }
    // Backlog verdicts keys, each player's.
    if has_table(c, "player_settings")? {
        crate::db::strip_verdicts_keys(c)?;
    }
    strip_messages(c)
}

/// What a copy without keys says in place of an error or a job's result.
const LEFT_OUT: &str = "(left out of a copy without keys)";

/// Jobs' log lines, results and errors and AI calls' errors are free text that can name the
/// computer's folders and the household's own hosts, so a copy without keys
/// leaves them out. A failed job or AI call still reads as failed.
fn strip_messages(c: &Connection) -> Result<()> {
    if has_table(c, "jobs")? {
        c.execute(
            "UPDATE jobs SET current=NULL, message='', summary=CASE WHEN summary IS NULL THEN NULL ELSE ?1 END,
                 error=CASE WHEN error IS NULL THEN NULL ELSE ?1 END",
            [LEFT_OUT],
        )?;
    }
    if has_table(c, "job_log")? {
        c.execute("DELETE FROM job_log", [])?;
    }
    if has_table(c, "job_failures")? {
        c.execute("UPDATE job_failures SET error=?1", [LEFT_OUT])?;
    }
    if has_table(c, "ai_calls")? {
        c.execute("UPDATE ai_calls SET error=?1 WHERE error IS NOT NULL", [LEFT_OUT])?;
    }
    // The last errors kept to show again, and where the backlog verdicts came from.
    c.execute(
        "DELETE FROM meta WHERE key IN ('diary_error', 'plan_usage.error', 'plan_usage.stopped', ?1) OR key LIKE 'prices.%.tried'",
        [crate::db::VERDICTS_SOURCE_META],
    )?;
    Ok(())
}

/// Write a copy of the database at `source` without keys and sign-ins to
/// `out`, which must not exist. `source` is only read, so it may be the
/// live database while the server runs, and it is never upgraded.
pub fn keyless_copy(source: &Path, out: &Path) -> Result<()> {
    let c =
        Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY).with_context(|| format!("opening {}", source.display()))?;
    keyless_copy_of(&c, out)
}

/// Write a copy of `c`'s database without keys and sign-ins to `out`, which
/// must not exist. The keys come out in memory, so they never reach `out`,
/// not even for a moment.
pub fn keyless_copy_of(c: &Connection, out: &Path) -> Result<()> {
    let out_text = out.to_str().context("the path is not UTF-8")?;
    let mut mem = Connection::open_in_memory()?;
    // An in-memory database only takes a copy with the source's page size.
    let page_size: i64 = c.query_row("PRAGMA main.page_size", [], |r| r.get(0))?;
    mem.execute_batch(&format!("PRAGMA page_size={page_size};"))?;
    {
        use rusqlite::backup::{Backup, StepResult};
        let copy = Backup::new(c, &mut mem)?;
        while copy.step(-1)? != StepResult::Done {}
    }
    strip_keys(&mem)?;
    mem.execute("VACUUM INTO ?1", [out_text]).with_context(|| format!("writing {}", out.display()))?;
    crate::http::make_private(out);
    Ok(())
}

/// "39 MB", "1.2 MB"
pub fn megabytes(bytes: u64) -> String {
    let mb = bytes as f64 / 1_000_000.0;
    if mb < 10.0 { format!("{mb:.1} MB") } else { format!("{mb:.0} MB") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::PlayerId;

    fn files(dir: &Path) -> Vec<String> {
        let mut left: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        left.sort();
        left
    }

    fn scheduled(keep: usize) -> New {
        New { kind: Kind::Scheduled, about: About::default(), keep: Some(keep), job: None }
    }

    /// An older backup: a small database, which is all rotation looks for.
    fn old_backup(path: &Path) {
        Connection::open(path).unwrap().execute_batch("CREATE TABLE old(x);").unwrap();
    }

    #[test]
    fn a_backup_opens_with_the_same_rows_and_only_its_kinds_old_files_go() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upnext.db");
        let db = Db::open(&path, 120).unwrap();
        db.setting_set(PlayerId::FIRST, "steam_api_key", Some("ABCDEF")).unwrap();
        db.meta_set("last_sync", "1790000000").unwrap();
        let backups = dir.path().join("backups");
        std::fs::create_dir_all(&backups).unwrap();
        let by_hand = ["upnext-pre-v15-ai-calls-20260930.db", "upnext-pre-functest-20260927-2219.db", "upnext-manual-20260801-030000.db"];
        for name in by_hand.iter().chain(&["upnext-scheduled-20260901-030000.db", "upnext-scheduled-20260908-030000.db"]) {
            old_backup(&backups.join(name));
        }

        let made = write(&path, &backups, "upnext-scheduled-20260915-030000", &scheduled(2)).unwrap();
        assert_eq!(made.removed, ["upnext-scheduled-20260901-030000.db"]);
        {
            let copy = Db::open(&backups.join(&made.name), 120).unwrap();
            assert_eq!(copy.meta_get("last_sync").unwrap().as_deref(), Some("1790000000"));
            assert_eq!(copy.settings_all(PlayerId::FIRST).unwrap()["steam_api_key"], "ABCDEF", "a full backup keeps the keys");
        }
        let schema = |c: &Connection| -> Vec<String> {
            let mut stmt = c.prepare("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name").unwrap();
            stmt.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
        };
        assert_eq!(schema(&Connection::open(&path).unwrap()), schema(&Connection::open(backups.join(&made.name)).unwrap()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(backups.join(&made.name)).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "only you can read it");
        }

        write(&path, &backups, "upnext-scheduled-20260922-030000", &scheduled(2)).unwrap();
        assert_eq!(
            files(&backups),
            [
                "upnext-manual-20260801-030000.db",
                "upnext-pre-functest-20260927-2219.db",
                "upnext-pre-v15-ai-calls-20260930.db",
                "upnext-scheduled-20260915-030000.db",
                "upnext-scheduled-20260922-030000.db",
            ]
        );
        let again = write(&path, &backups, "upnext-scheduled-20260922-030000", &scheduled(2)).unwrap();
        assert_eq!(again.name, "upnext-scheduled-20260922-030000_2.db", "never over an existing file");
        assert_eq!(again.removed, ["upnext-scheduled-20260915-030000.db"], "and it counts as the newer one");

        // The automatic kinds keep their newest five, by time, whatever
        // version an upgrade went to; backups made by hand stay.
        for stem in ["upnext-pre-upgrade-v9-20260801-000000", "upnext-pre-upgrade-v10-20260802-000000"] {
            old_backup(&backups.join(format!("{stem}.db")));
        }
        for day in 3..=7 {
            let stem = format!("upnext-pre-upgrade-v1{day}-2026080{day}-000000");
            write(&path, &backups, &stem, &New { kind: Kind::Upgrade, about: About::default(), keep: Some(5), job: None }).unwrap();
        }
        let upgrades: Vec<String> = files(&backups).into_iter().filter(|n| n.contains("upgrade")).collect();
        assert_eq!(upgrades.len(), 5);
        assert!(!upgrades.iter().any(|n| n.contains("-v9-") || n.contains("-v10-")), "{upgrades:?}");
        assert!(files(&backups).contains(&"upnext-manual-20260801-030000.db".to_string()));
    }

    #[test]
    fn the_writing_jobs_row_is_done_in_its_copy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upnext.db");
        let db = Db::open(&path, 120).unwrap();
        let id = db.job_begin("backup", Some("backup"), crate::db::JobSource::Schedule, &serde_json::json!({})).unwrap().unwrap();
        let backups = dir.path().join("backups");
        let new = New { kind: Kind::Scheduled, about: About::default(), keep: Some(4), job: Some(id) };
        let made = write(&path, &backups, &Kind::Scheduled.stem(), &new).unwrap();
        let copy = Db::open(&backups.join(&made.name), 120).unwrap();
        let job = copy.job(id).unwrap().unwrap();
        assert_eq!((job.status, job.summary.as_deref()), (crate::db::JobStatus::Done, Some(made.name.as_str())));
        assert_eq!(db.job(id).unwrap().unwrap().status, crate::db::JobStatus::Running, "the live row is the job's own");
    }

    #[test]
    fn the_list_says_why_each_was_made_and_notes_go_with_their_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upnext.db");
        Db::open(&path, 120).unwrap();
        let backups = dir.path().join("backups");
        assert!(list(&backups).unwrap().is_empty(), "no folder yet");
        std::fs::create_dir_all(&backups).unwrap();
        for name in ["upnext-pre-v12-20260929.db", "upnext-pre-v13-whole-reviews-20260930.db-wal", "notes.txt", ".work-x.db"] {
            std::fs::write(backups.join(name), b"x").unwrap();
        }
        let note = About { note: Some("before trying the new prompt".into()), reason: None };
        let made =
            write(&path, &backups, &Kind::Manual.stem(), &New { kind: Kind::Manual, about: note.clone(), keep: None, job: None }).unwrap();

        let all = list(&backups).unwrap();
        let names: Vec<&str> = all.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names.len(), 2, "{names:?}");
        let mine = all.iter().find(|b| b.name == made.name).unwrap();
        assert_eq!((mine.kind, &mine.about, mine.schema), (Kind::Manual, &note, Some(SCHEMA_VERSION)));
        let old = all.iter().find(|b| b.name == "upnext-pre-v12-20260929.db").unwrap();
        assert_eq!((old.kind, old.schema), (Kind::Manual, None), "made by hand, and not a database");
        assert_eq!(Kind::of("upnext-pre-upgrade-v18-20261001-000000.db"), Kind::Upgrade);
        assert_eq!(Kind::of("upnext-pre-restore-20261001-000000.db"), Kind::Restore);

        assert!(find(&backups, "../upnext.db").is_err() && find(&backups, ".work-x.db").is_err());
        delete(&backups, &made.name).unwrap();
        assert!(read_index(&backups).is_empty(), "its note goes with it");
        assert!(delete(&backups, &made.name).is_err());
    }

    #[test]
    fn a_copy_without_keys_has_no_keys_sign_in_or_profile() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upnext.db");
        let db = Db::open(&path, 120).unwrap();
        for (k, v) in
            [("steam_api_key", "ABCDEF"), ("rawg_api_key", "r"), ("igdb_client_secret", "s"), ("steam_id", "7656"), ("theme", "ember")]
        {
            db.setting_set(PlayerId::FIRST, k, Some(v)).unwrap();
        }
        db.meta_set("profile", r#"{"persona_name":"Sam"}"#).unwrap();
        db.meta_set("last_sync", "1790000000").unwrap();
        let bookmark = db.gog_bookmark_key().unwrap();
        db.meta_set(crate::db::PLAYER_COOKIE_KEY, &"c".repeat(64)).unwrap();
        // The GOG and Epic sign-ins live on their connections.
        use crate::signin::{DbSignIn, SignInStore};
        DbSignIn::own(path.clone(), "gog", PlayerId::FIRST, None).save("GOGTOKEN").unwrap();
        DbSignIn::own(path.clone(), "epic", PlayerId::FIRST, None).save("EPICTOKEN").unwrap();
        assert_eq!(db.sign_ins().unwrap().len(), 2);
        let out = dir.path().join("keyless.db");
        keyless_copy(&path, &out).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        for secret in [&b"ABCDEF"[..], b"GOGTOKEN", b"EPICTOKEN"] {
            assert!(!bytes.windows(secret.len()).any(|w| w == secret), "{} isn't anywhere in the file", String::from_utf8_lossy(secret));
        }
        assert!(keyless_copy(&path, &out).is_err(), "an existing file is left alone");
        let copy = Db::open(&out, 120).unwrap();
        let settings = copy.settings_all(PlayerId::FIRST).unwrap();
        assert_eq!(settings.keys().collect::<Vec<_>>(), ["theme"], "{settings:?}");
        assert_eq!(copy.meta_get("profile").unwrap(), None);
        assert_eq!(copy.meta_get(crate::db::GOG_BOOKMARK_KEY).unwrap(), None, "nor the GOG bookmark's key (L10)");
        assert_eq!(copy.meta_get(crate::db::PLAYER_COOKIE_KEY).unwrap(), None, "nor the player cookie's key");
        assert!(!bytes.windows(bookmark.len()).any(|w| w == bookmark.as_bytes()));
        assert_eq!(copy.meta_get("last_sync").unwrap().as_deref(), Some("1790000000"), "everything else stays");
        assert_eq!(copy.sign_ins().unwrap(), [], "signed out of GOG and Epic");

        let exported = dir.path().join("export.db");
        db.export_to(&exported).unwrap();
        assert!(!std::fs::read(&exported).unwrap().windows(6).any(|w| w == b"ABCDEF"));
        let exported = Db::open(&exported, 120).unwrap();
        assert_eq!(
            exported.settings_all(PlayerId::FIRST).unwrap().keys().collect::<Vec<_>>(),
            ["theme"],
            "the export follows the same rule"
        );
        assert_eq!(exported.meta_get("profile").unwrap(), None);
    }

    /// An upgraded library named player 1 after the Steam persona:
    /// a copy without keys carries no player's name.
    #[test]
    fn a_copy_without_keys_names_no_player() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upnext.db");
        let c = Connection::open(&path).unwrap();
        c.execute_batch(&crate::testutil::fixture("schema/v31.sql")).unwrap();
        c.execute("INSERT INTO meta(key, value) VALUES ('profile', '{\"persona_name\":\"Quillfeather\"}')", []).unwrap();
        drop(c);
        let db = Db::open(&path, 120).unwrap();
        db.add_player("Marigold").unwrap();
        Connection::open(&path).unwrap().execute("UPDATE players SET pin_hash = 'argon2-hash' WHERE id = 2", []).unwrap();
        assert_eq!(db.players().unwrap()[0].name, "Quillfeather");

        let out = dir.path().join("keyless.db");
        keyless_copy(&path, &out).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        for name in [&b"Quillfeather"[..], b"Marigold", b"argon2-hash"] {
            assert!(!bytes.windows(name.len()).any(|w| w == name), "{}", String::from_utf8_lossy(name));
        }
        let names: Vec<(String, bool)> = Db::open(&out, 120).unwrap().players().unwrap().into_iter().map(|p| (p.name, p.has_pin)).collect();
        assert_eq!(names, [("Player 1".to_string(), false), ("Player 2".to_string(), false)]);
    }

    /// B-CORE-2 (code health review 2026-10-11): the copy "without keys and
    /// sign-ins", which Settings' export, a backup's download and `upnext
    /// export` make to share, kept each player's identity provider account
    /// (often an email address), the files and folders on each connection
    /// (a Heroic folder in someone's home folder) and the household's own
    /// addresses. Restored here, this install's own come back.
    #[test]
    fn a_copy_without_keys_has_no_identity_accounts_paths_or_addresses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upnext.db");
        let db = Db::open(&path, 120).unwrap();
        for (k, v) in [
            ("tls_cert", "/home/you/certs/den.pem"),
            ("public_url", "https://upnext.den.example.org"),
            ("network_name", "den"),
            ("trusted_proxies", "192.168.0.42"),
            ("oidc_issuer", "https://sso.den.example.org"),
            ("theme", "ember"),
        ] {
            db.setting_set(PlayerId::FIRST, k, Some(v)).unwrap();
        }
        {
            let c = Connection::open(&path).unwrap();
            c.execute(
                "UPDATE players SET oidc_issuer='https://sso.den.example.org', oidc_subject='sam@example.org' WHERE id=?1",
                [PlayerId::FIRST],
            )
            .unwrap();
            c.execute(
                "INSERT INTO store_connections(kind, owner_id, settings, created_at) VALUES ('heroic', 1, ?1, 1)",
                [r#"{"heroic_dir":"/home/you/.config/heroic","keep":"this"}"#],
            )
            .unwrap();
            let heroic = c.last_insert_rowid();
            c.execute(
                "INSERT INTO store_accounts(connection_id, store, external_id, name, created_at) VALUES (?1, 'epic', 'epic-account-1', 'Sam''s Epic', 1)",
                [heroic],
            )
            .unwrap();
        }
        let out = dir.path().join("keyless.db");
        keyless_copy(&path, &out).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        for leak in ["sam@example.org", "/home/you", "den.example.org", "192.168.0.42", "Sam's Epic", "epic-account-1"] {
            assert!(!bytes.windows(leak.len()).any(|w| w == leak.as_bytes()), "{leak} is still in the copy");
        }
        let c = Connection::open(&out).unwrap();
        let settings: String =
            c.query_row("SELECT settings FROM store_connections WHERE kind='heroic' ORDER BY id DESC LIMIT 1", [], |r| r.get(0)).unwrap();
        assert_eq!(settings, r#"{"keep":"this"}"#, "the rest of a connection's settings stay");
        let name: String = c.query_row("SELECT name FROM store_accounts WHERE store='epic'", [], |r| r.get(0)).unwrap();
        assert_eq!(name, "Epic", "the store's own name");
        drop(c);
        let copy = Db::open(&out, 120).unwrap();
        assert_eq!(copy.settings_all(PlayerId::FIRST).unwrap().get("theme").map(String::as_str), Some("ember"), "everything else stays");

        // Restored on this install, its own paths and addresses come back.
        let staged = stage(&out, &dir.path().join("backups")).unwrap();
        let kept = prepare(&staged, &Keys::of(&db).unwrap()).unwrap();
        for key in ["tls_cert", "public_url", "network_name", "trusted_proxies", "oidc_issuer"] {
            assert!(kept.settings.iter().any(|k| k == key), "{key} kept: {:?}", kept.settings);
        }
    }

    /// The settings naming the household's own services, and every job's
    /// log line, result and error and every AI call's error, which can name folders and hosts, are left
    /// out too. Restored here, this install's settings come back.
    #[test]
    fn a_copy_without_keys_leaves_out_messages_and_the_households_services() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upnext.db");
        let db = Db::open(&path, 120).unwrap();
        for (k, v) in [
            ("oidc_name", "Den sign-in"),
            ("oidc_allowed_groups", "den-family"),
            ("verdicts_url", "https://verdicts.den.example.org"),
            ("openai_base_url", "http://192.168.0.42:8000/v1"),
        ] {
            db.setting_set(PlayerId::FIRST, k, Some(v)).unwrap();
        }
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(
                "INSERT INTO jobs(id, kind, source, options, status, started_at, finished_at, heartbeat_at, current, message, summary, error)
                     VALUES (7, 'backup', 'all', '{}', 'failed', 1, 2, 2, '/home/you/a', '/home/you/b', 'saved /home/you/c', 'no room on den.lan');
                 INSERT INTO job_log(job_id, at, level, message) VALUES (7, 1, 'warn', 'reading /home/you/.config/heroic');
                 INSERT INTO job_failures(job_id, at, lane, source, options, error) VALUES (7, 1, 'igdb', 'igdb', '{}', 'den.lan refused');
                 INSERT INTO ai_calls(at, kind, backend, error, duration_ms) VALUES (1, 'pick', 'openai', 'http://192.168.0.42:8000 timed out', 5);
                 INSERT INTO ai_calls(at, kind, backend, duration_ms) VALUES (2, 'pick', 'openai', 5);
                 INSERT INTO meta(key, value) VALUES ('diary_error', '{\"error\":\"/home/you/d\"}'), ('plan_usage.error', 'den.lan'),
                     ('prices.anthropic.tried', '{\"at\":1,\"error\":\"den.lan\"}'), ('verdicts_source', 'https://verdicts.den.example.org/x');",
            )
            .unwrap();
        }
        let out = dir.path().join("keyless.db");
        keyless_copy(&path, &out).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        for leak in ["/home/you", "den.lan", "den.example.org", "192.168.0.42", "Den sign-in", "den-family"] {
            assert!(!bytes.windows(leak.len()).any(|w| w == leak.as_bytes()), "{leak} is still in the copy");
        }
        let c = Connection::open(&out).unwrap();
        let (status, error): (String, String) =
            c.query_row("SELECT status, error FROM jobs WHERE id=7", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((status.as_str(), error.as_str()), ("failed", LEFT_OUT), "a failed job still reads as failed");
        let failed: i64 = c.query_row("SELECT COUNT(*) FROM ai_calls WHERE error IS NOT NULL", [], |r| r.get(0)).unwrap();
        assert_eq!(failed, 1, "a failed AI call still reads as failed, and one that worked as working");
        drop(c);

        let staged = stage(&out, &dir.path().join("backups")).unwrap();
        let kept = prepare(&staged, &Keys::of(&db).unwrap()).unwrap();
        for key in ["oidc_name", "oidc_allowed_groups", "verdicts_url", "openai_base_url"] {
            assert!(kept.settings.iter().any(|k| k == key), "{key} kept: {:?}", kept.settings);
        }
    }

    /// The GOG and Epic sign-ins were files a restore never touched; now in
    /// the database, this install's are kept for each player the copy has
    /// who isn't signed in there, and a copy's own sign-in stays its own.
    #[test]
    fn a_restore_keeps_this_installs_sign_ins_where_the_copy_has_none() {
        use crate::signin::{DbSignIn, SignInStore};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upnext.db");
        let live = Db::open(&path, 120).unwrap();
        let sam = live.add_player("Sam").unwrap();
        DbSignIn::own(path.clone(), "gog", PlayerId::FIRST, None).save("mine").unwrap();
        DbSignIn::own(path.clone(), "epic", sam, None).save("sams").unwrap();
        let copy = dir.path().join("keyless.db");
        keyless_copy(&path, &copy).unwrap();
        let secret = |db: &Path, kind: &'static str, p: PlayerId| DbSignIn::own(db.to_path_buf(), kind, p, None).load();

        // A copy without keys: both sign-ins come back as they were.
        let staged = stage(&copy, &dir.path().join("backups")).unwrap();
        let kept = prepare(&staged, &Keys::of(&live).unwrap()).unwrap();
        assert_eq!(kept.settings, ["GOG sign-in", "Epic sign-in"]);
        assert_eq!(secret(&staged.path, "gog", PlayerId::FIRST).as_deref(), Some("mine"));
        assert_eq!(secret(&staged.path, "epic", sam).as_deref(), Some("sams"));

        // A copy signed in to GOG as someone else keeps its own.
        DbSignIn::own(copy.clone(), "gog", PlayerId::FIRST, None).save("the copy's").unwrap();
        let staged = stage(&copy, &dir.path().join("backups")).unwrap();
        let kept = prepare(&staged, &Keys::of(&live).unwrap()).unwrap();
        assert_eq!(kept.settings, ["Epic sign-in"]);
        assert_eq!(secret(&staged.path, "gog", PlayerId::FIRST).as_deref(), Some("the copy's"));
    }

    #[test]
    fn a_restore_keeps_its_own_job_and_moves_the_backups_job_with_its_id() {
        use crate::db::{JobEnd, JobProgress, JobSource, JobStatus, Level, LogLine, now};
        let dir = tempfile::tempdir().unwrap();
        let line = |m: &str| LogLine { at: now(), level: Level::Info, lane: None, message: m.into(), title_id: None };
        let done = JobEnd { status: JobStatus::Done, summary: Some("ok"), error: None };
        let run = |db: &Db, kind: &str, log: &str, end: bool| {
            let id = db.job_begin(kind, None, JobSource::Ui, &serde_json::json!({})).unwrap().unwrap();
            if end {
                db.job_end(id, &JobProgress::default(), &[line(log)], &[], &done).unwrap();
            } else {
                db.job_pulse(id, &JobProgress::default(), &[line(log)], &[]).unwrap();
            }
            id
        };
        // The backup: four finished jobs, and a fifth it caught running.
        let source = dir.path().join("backup.db");
        {
            let b = Db::open(&source, 120).unwrap();
            for n in 1..=4 {
                run(&b, "sync", &format!("backup's job {n}"), true);
            }
            run(&b, "enrich", "backup's running job", false);
        }
        // This install: three finished jobs, then the restore, running as job 4.
        let live = Db::open(&dir.path().join("upnext.db"), 120).unwrap();
        for _ in 1..=3 {
            run(&live, "sync", "this install's", true);
        }
        let restore = run(&live, "restore", "backed up first", false);
        assert_eq!(restore, 4);
        assert!(live.all_games_shared(PlayerId::FIRST).unwrap().is_empty());

        let staged = stage(&source, &dir.path().join("backups")).unwrap();
        prepare(&staged, &Keys::default()).unwrap();
        live.replace_with(&staged.path, restore).unwrap();

        let job = |id: i64| live.job(id).unwrap().unwrap();
        let log = |id: i64| live.job_log(id, 0, Level::Info, 50).unwrap().into_iter().map(|l| l.line.message).collect::<Vec<_>>();
        assert_eq!((job(4).kind.as_str(), job(4).status, log(4)), ("restore", JobStatus::Running, vec!["backed up first".to_string()]));
        assert_eq!((job(6).kind.as_str(), log(6)), ("sync", vec!["backup's job 4".to_string()]), "the backup's job 4 moved to 6");
        assert_eq!(
            (job(5).status, job(5).error.as_deref()),
            (JobStatus::Failed, Some("Running when this backup was made, so the backup doesn't know how it ended."))
        );
        assert_eq!(live.job_running().unwrap().map(|j| j.id), Some(4), "the restore still holds the slot");
        live.job_end(4, &JobProgress::default(), &[line("restored")], &[], &done).unwrap();
        assert_eq!(log(4), ["backed up first", "restored"]);
        assert_eq!(run(&live, "sync", "after", true), 7, "new jobs come after every kept one");
    }

    /// The bookmarks dragged on this install and its player cookies keep
    /// working after a restore, and another install's copy brings none of
    /// its own (L10).
    #[test]
    fn a_restore_keeps_this_installs_own_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("backup.db");
        let theirs = Db::open(&source, 120).unwrap().gog_bookmark_key().unwrap();
        let live = Db::open(&dir.path().join("upnext.db"), 120).unwrap();
        let ours = live.gog_bookmark_key().unwrap();
        live.meta_set(crate::db::PLAYER_COOKIE_KEY, &"c".repeat(64)).unwrap();
        assert_ne!(ours, theirs);
        assert_eq!(live.gog_bookmark_key().unwrap(), ours, "made once");
        let staged = stage(&source, &dir.path().join("backups")).unwrap();
        prepare(&staged, &Keys::of(&live).unwrap()).unwrap();
        let restored = Db::open(&staged.path, 120).unwrap();
        assert_eq!(restored.meta_get(crate::db::GOG_BOOKMARK_KEY).unwrap(), Some(ours));
        assert_eq!(restored.meta_get(crate::db::PLAYER_COOKIE_KEY).unwrap(), Some("c".repeat(64)), "the player cookies stay signed");
        let staged = stage(&source, &dir.path().join("backups")).unwrap();
        prepare(&staged, &Keys::default()).unwrap();
        assert_eq!(
            Db::open(&staged.path, 120).unwrap().meta_get(crate::db::GOG_BOOKMARK_KEY).unwrap(),
            None,
            "a new one is made when asked"
        );
    }

    #[test]
    fn files_that_cant_replace_the_database_are_refused_plainly() {
        let dir = tempfile::tempdir().unwrap();
        let backups = dir.path().join("backups");
        let refusal = |path: &Path| stage(path, &backups).unwrap_err().downcast::<Refused>().unwrap().0;

        let text = dir.path().join("notes.db");
        std::fs::write(&text, "not a database at all, just some words long enough to fill a header and then some more words").unwrap();
        assert!(refusal(&text).starts_with("that file isn't a database"), "{}", refusal(&text));

        let other = dir.path().join("other.db");
        Connection::open(&other).unwrap().execute_batch("CREATE TABLE notes(x); INSERT INTO notes VALUES (1);").unwrap();
        assert_eq!(refusal(&other), "that's a SQLite database, but not UpNext's: it has no titles or games");

        let newer = dir.path().join("newer.db");
        Db::open(&newer, 120).unwrap();
        Connection::open(&newer).unwrap().execute_batch(&format!("PRAGMA user_version={};", SCHEMA_VERSION + 1)).unwrap();
        assert!(refusal(&newer).contains(&format!("newer UpNext (schema v{})", SCHEMA_VERSION + 1)), "{}", refusal(&newer));

        let older = dir.path().join("older.db");
        Db::open(&older, 120).unwrap();
        Connection::open(&older).unwrap().execute_batch(&format!("PRAGMA user_version={};", BASELINE_VERSION - 1)).unwrap();
        assert!(
            refusal(&older).starts_with("that database is from a development build of UpNext made before its first release"),
            "{}",
            refusal(&older)
        );
        // A library from before the schema was numbered is refused by its
        // header check, which the server runs before it starts a restore.
        let unnumbered = dir.path().join("unnumbered.db");
        Connection::open(&unnumbered).unwrap().execute_batch("CREATE TABLE games(id); CREATE TABLE titles(id);").unwrap();
        assert!(check_header(&unnumbered).unwrap_err().0.contains("(schema v0)"), "{}", refusal(&unnumbered));
        assert_eq!(check_header(&other).unwrap(), 0, "a database with no library goes on to `stage`");

        let good = dir.path().join("good.db");
        Db::open(&good, 120).unwrap();
        let staged = stage(&good, &backups).unwrap();
        assert_eq!(staged.schema, SCHEMA_VERSION);
        let at = staged.path.clone();
        assert!(at.is_file() && list(&backups).unwrap().is_empty(), "a work file isn't a backup");
        drop(staged);
        assert!(!at.exists(), "and it goes when it's done with");
        assert!(files(&backups).is_empty(), "{:?}", files(&backups));
    }

    /// A database whose last pages are overwritten: `VACUUM INTO` creates
    /// its copy, then fails partway, as a full disk makes it fail.
    fn damaged_database(path: &Path) {
        let c = Connection::open(path).unwrap();
        c.execute_batch("CREATE TABLE t(x); WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 200) INSERT INTO t SELECT printf('%.500c', 'x') FROM n;").unwrap();
        drop(c);
        let mut bytes = std::fs::read(path).unwrap();
        let len = bytes.len();
        bytes[len - 3 * 4096..len - 4096].fill(0xff);
        std::fs::write(path, bytes).unwrap();
    }

    /// Rotation keeps the backups written last, whatever their names say:
    /// in the hour the clocks go back, or after a move to a time zone to the
    /// west, a new backup's local-time name sorts before an older one's, and
    /// the new one was removed (B-CORE-7).
    #[test]
    fn rotation_keeps_the_newest_backup_when_the_clock_went_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upnext.db");
        Db::open(&path, 120).unwrap();
        let backups = dir.path().join("backups");
        std::fs::create_dir_all(&backups).unwrap();
        // Written at 02:30 summer time, an hour before the next one.
        let earlier = backups.join("upnext-scheduled-20261025-023000.db");
        old_backup(&earlier);
        let hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options().write(true).open(&earlier).unwrap().set_modified(hour_ago).unwrap();

        // 02:00 winter time, after the clocks went back.
        let made = write(&path, &backups, "upnext-scheduled-20261025-020000", &scheduled(1)).unwrap();
        assert_eq!(made.removed, ["upnext-scheduled-20261025-023000.db"], "the older one goes");
        assert_eq!(files(&backups), ["upnext-scheduled-20261025-020000.db"]);
    }

    #[test]
    fn a_failed_backup_leaves_no_file_and_never_pushes_a_good_one_out() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upnext.db");
        Db::open(&path, 120).unwrap();
        let damaged = dir.path().join("damaged.db");
        damaged_database(&damaged);
        let backups = dir.path().join("backups");

        let good = ["upnext-scheduled-20260901-030000", "upnext-scheduled-20260908-030000"];
        for stem in good {
            write(&path, &backups, stem, &scheduled(3)).unwrap();
        }
        for stem in ["upnext-scheduled-20260915-030000", "upnext-scheduled-20260922-030000", "upnext-scheduled-20260929-030000"] {
            let err = write(&damaged, &backups, stem, &scheduled(3)).unwrap_err();
            assert!(format!("{err:#}").contains(stem), "{err:#}");
        }
        let made = write(&path, &backups, "upnext-scheduled-20261006-030000", &scheduled(3)).unwrap();
        assert_eq!((made.name.as_str(), made.removed.len()), ("upnext-scheduled-20261006-030000.db", 0), "{made:?}");

        let listed: Vec<(String, Option<i64>)> = list(&backups).unwrap().into_iter().map(|b| (b.name, b.schema)).collect();
        assert_eq!(
            listed,
            [
                ("upnext-scheduled-20261006-030000.db".to_string(), Some(SCHEMA_VERSION)),
                ("upnext-scheduled-20260908-030000.db".to_string(), Some(SCHEMA_VERSION)),
                ("upnext-scheduled-20260901-030000.db".to_string(), Some(SCHEMA_VERSION)),
            ]
        );
        assert_eq!(files(&backups).len(), 3, "no work files left behind: {:?}", files(&backups));

        // A file a failed backup left before this was fixed isn't a backup:
        // it neither counts toward the three kept nor pushes one out.
        std::fs::write(backups.join("upnext-scheduled-20261007-030000.db"), b"").unwrap();
        let made = write(&path, &backups, "upnext-scheduled-20261008-030000", &scheduled(3)).unwrap();
        assert_eq!(made.removed, ["upnext-scheduled-20260901-030000.db"]);
    }
}
