//! "Add to Steam": UpNext's big screen as a non-Steam
//! shortcut in each Steam account's library on this computer, so Steam's Big
//! Picture, a Deck's Gaming mode or a Windows handheld's Steam opens it.
//!
//! Steam keeps those shortcuts in `userdata/<account>/config/shortcuts.vdf`,
//! in its binary key-value format: a map called `shortcuts` holding one map
//! per shortcut, keyed `"0"`, `"1"` and so on. Steam rewrites the file as it
//! exits, so it's written only while Steam is closed. Every other shortcut is
//! copied through byte for byte (this reads only enough of each to find
//! UpNext's own), the file is copied aside before it changes, and UpNext's
//! entry is found again by the id Steam derives from its program and name,
//! so adding it twice changes nothing.
//!
//! Steam Input gives a non-Steam shortcut the desktop layout, where the pad
//! moves the mouse, until someone picks another. So, as Steam ROM Manager
//! does, the shortcut also gets Steam's own Gamepad template in each
//! account's controller settings (`Steam Controller Configs/<account>/
//! config/configset_controller_<type>.vdf`, a text VDF keyed by the
//! shortcut's name in lower case), one file per controller type, only where
//! it has no layout yet.

use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// The shortcut's name in Steam's library.
pub const NAME: &str = "UpNext";

/// The library images UpNext's shortcut gets (`config/grid`), under the
/// names Steam gives custom artwork: the portrait capsule, the wide one, the
/// hero banner, the logo drawn over it, and the icon. Only the build with the
/// app window carries them: without it there's nothing for Steam to open, so
/// nothing is added, and the container image builds from `src/` alone.
#[cfg(feature = "desktop")]
const GRID: &[(&str, &[u8])] = &[
    ("p.png", include_bytes!("../contrib/steam/library/capsule.png")),
    (".png", include_bytes!("../contrib/steam/library/header.png")),
    ("_hero.png", include_bytes!("../contrib/steam/library/hero.png")),
    ("_logo.png", include_bytes!("../contrib/steam/library/logo.png")),
    ("_icon.png", include_bytes!("../contrib/linux/icons/hicolor/256x256/apps/upnext.png")),
];
#[cfg(not(feature = "desktop"))]
const GRID: &[(&str, &[u8])] = &[];

/// What the shortcut runs: the app window in big screen.
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
pub struct Target {
    /// The program, as a full path.
    #[schema(value_type = String)]
    pub program: PathBuf,
    /// Its arguments.
    pub arguments: String,
}

impl Target {
    /// For a computer whose UpNext is `exe`: on Windows `upnext-app.exe`,
    /// which has no console, beside it (`autostart::app_exe`), with
    /// `--big-screen`; elsewhere `exe` itself with `app --big-screen`, or
    /// `--big-screen` when it's `upnext-app`.
    pub fn for_exe(exe: &Path, windows: bool) -> Self {
        let flag = crate::autostart::BIG_SCREEN_FLAG;
        if windows {
            return Self { program: crate::autostart::app_exe(exe), arguments: flag.to_string() };
        }
        let is_app = exe.file_name().is_some_and(|n| n == "upnext-app");
        Self { program: exe.to_path_buf(), arguments: if is_app { flag.to_string() } else { format!("app {flag}") } }
    }

    /// Steam's `Exe` value: the program in quotes, as Steam writes it.
    fn exe_value(&self) -> String {
        format!("\"{}\"", self.program.display())
    }

    /// Steam's `StartDir`: the program's folder, quoted the same way.
    fn start_dir_value(&self) -> String {
        let dir = self.program.parent().unwrap_or(Path::new(""));
        format!("\"{}\"", dir.display())
    }

    /// The id Steam gives a shortcut with this program and `name`, which
    /// names its library images in `config/grid` too: the CRC-32 of the
    /// quoted program and the name, with the top bit set.
    pub fn shortcut_id(&self, name: &str) -> u32 {
        let mut crc = crc32fast::Hasher::new();
        crc.update(self.exe_value().as_bytes());
        crc.update(name.as_bytes());
        crc.finalize() | 0x8000_0000
    }
}

/// A Steam account on this computer, and whether its library has UpNext.
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
pub struct Account {
    /// Its folder under `userdata`: the account id, the low half of its SteamID.
    pub account_id: String,
    pub added: bool,
    /// Whether UpNext's shortcut has a controller layout for every kind of
    /// controller Add to Steam writes one for: false until Add to Steam has
    /// run in a version that writes them, so a shortcut an older UpNext
    /// added can be given one. A layout file UpNext can't read counts
    /// as done, since pressing again can't change it.
    pub layout: bool,
    /// Why the last Add to Steam couldn't write the controller layouts, while
    /// `layout` is false, or null. Pressing again would fail the same way, so
    /// Settings shows this and the way to pick the layout in Steam instead.
    pub layout_error: Option<String>,
    /// Why its shortcuts file can't be read, or null. While one can't,
    /// "Add to Steam" adds to no account, so the others aren't left
    /// different from it.
    pub error: Option<String>,
}

/// Every Steam account that has signed in on this computer: the folders of
/// `userdata` named by a number other than 0, in order.
pub fn accounts(steam_dir: &Path) -> Vec<String> {
    let Ok(dir) = std::fs::read_dir(steam_dir.join("userdata")) else { return vec![] };
    let mut ids: Vec<String> = dir
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && !n.trim_start_matches('0').is_empty())
        .collect();
    ids.sort_by_key(|n| n.parse::<u64>().unwrap_or(u64::MAX));
    ids
}

fn shortcuts_file(steam_dir: &Path, account: &str) -> PathBuf {
    steam_dir.join("userdata").join(account).join("config").join("shortcuts.vdf")
}

/// Where a copy of the file goes before UpNext first changes it. Only the
/// first copy is kept: a later one would be of a file UpNext had already
/// changed, and the copy is there to undo UpNext.
fn backup_file(file: &Path) -> PathBuf {
    file.with_file_name("shortcuts.vdf.upnext-backup")
}

/// One account's shortcuts file as it is now: its bytes (None when there's
/// no file yet) and whether UpNext is in it.
fn read_shortcuts(file: &Path, target: &Target) -> Result<(Option<Vec<u8>>, bool)> {
    let old = match std::fs::read(file) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).with_context(|| format!("reading {}", file.display())),
    };
    let found = match &old {
        Some(bytes) => Shortcuts::parse(bytes).with_context(|| format!("reading {}", file.display()))?.find(target).is_some(),
        None => false,
    };
    Ok((old, found))
}

/// What each account's library has: whether UpNext's shortcut is in it, or
/// why its file can't be read.
pub fn status(steam_dir: &Path, target: &Target) -> Vec<Account> {
    accounts(steam_dir)
        .into_iter()
        .map(|account| match read_shortcuts(&shortcuts_file(steam_dir, &account), target) {
            Ok((_, added)) => {
                let layout = has_layouts(steam_dir, &account);
                let layout_error = if layout { None } else { failed_layouts(steam_dir, &account, None) };
                Account { layout, layout_error, account_id: account, added, error: None }
            }
            Err(e) => Account { layout: false, layout_error: None, account_id: account, added: false, error: Some(format!("{e:#}")) },
        })
        .collect()
}

/// Why each account's last layout write failed, by its `userdata` folder.
/// It's kept in memory only, so after a restart one more press finds out
/// again; a file is no place for it, since writing is what failed.
static LAYOUT_FAILED: std::sync::Mutex<std::collections::BTreeMap<PathBuf, String>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// One account's kept layout failure. With `now`, it's replaced first by
/// what the latest press found: its problems, or none.
fn failed_layouts(steam_dir: &Path, account: &str, now: Option<&[String]>) -> Option<String> {
    let key = steam_dir.join("userdata").join(account);
    let mut failed = LAYOUT_FAILED.lock().unwrap_or_else(|e| e.into_inner());
    match now {
        Some([]) => {
            failed.remove(&key);
        }
        Some(problems) => {
            failed.insert(key.clone(), problems.join("; "));
        }
        None => {}
    }
    failed.get(&key).cloned()
}

/// Why "Add to Steam" can't write now.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum Refused {
    #[error("Close Steam first: it rewrites its shortcuts as it exits, which would undo this")]
    SteamRunning,
    #[error("no Steam account has signed in on this computer yet")]
    NoAccounts,
    #[error("Steam's shortcuts for account {0} can't be read, so UpNext was added to no account: {1}")]
    Unreadable(String, String),
    #[error("UpNext's folder has a name Steam can't store ({0}); move UpNext to a folder whose name is plain text")]
    PathNotText(String),
}

/// Add UpNext's shortcut to every account's library, with its images, unless
/// it's there already. `steam_running` is asked first, and nothing is written
/// while it says yes, nor while any account's file can't be read. The
/// accounts afterwards.
pub fn add(steam_dir: &Path, target: &Target, steam_running: impl FnOnce() -> bool) -> Result<Vec<Account>> {
    let ids = accounts(steam_dir);
    if ids.is_empty() {
        return Err(Refused::NoAccounts.into());
    }
    // Steam keeps the program's path as text: a path that isn't valid
    // UTF-8 would be stored with its odd bytes replaced, and point nowhere.
    if target.program.to_str().is_none() {
        return Err(Refused::PathNotText(target.program.display().to_string()).into());
    }
    if steam_running() {
        return Err(Refused::SteamRunning.into());
    }
    // Every file is read before any is written, so one that can't be read
    // leaves them all as they were.
    let mut read = Vec::with_capacity(ids.len());
    for account in &ids {
        match read_shortcuts(&shortcuts_file(steam_dir, account), target) {
            Ok((old, _)) => read.push((account, old)),
            Err(e) => return Err(Refused::Unreadable(account.clone(), format!("{e:#}")).into()),
        }
    }
    for (account, old) in read {
        add_to(&shortcuts_file(steam_dir, account), old, target)?;
        // The layout is a convenience, with a way to pick it by hand in
        // Steam: a file UpNext can't read is left alone and logged.
        let problems = add_layouts(steam_dir, account);
        for problem in &problems {
            tracing::warn!("Add to Steam: no Gamepad layout for account {account}: {problem}");
        }
        failed_layouts(steam_dir, account, Some(&problems));
    }
    Ok(status(steam_dir, target))
}

/// Add the shortcut to one account's file, read as `old`, and the images to
/// its grid folder.
fn add_to(file: &Path, old: Option<Vec<u8>>, target: &Target) -> Result<()> {
    let config = file.parent().context("a file with no folder")?;
    std::fs::create_dir_all(config).with_context(|| format!("creating {}", config.display()))?;
    let empty = Shortcuts::EMPTY.to_vec();
    let shortcuts = Shortcuts::parse(old.as_deref().unwrap_or(&empty)).with_context(|| format!("reading {}", file.display()))?;
    if shortcuts.find(target).is_none() {
        let new = shortcuts.with(&entry(target, &grid_path(config, target, "_icon.png")));
        let backup = backup_file(file);
        if let Some(old) = &old
            && !backup.exists()
        {
            std::fs::write(&backup, old).with_context(|| format!("copying {} aside", file.display()))?;
        }
        // Through a file beside it, so a failed write never leaves half of
        // one; flushed to the disk before the rename, so a crash right after
        // can't leave an empty file in its place on file systems that don't
        // order the two (XFS).
        let part = file.with_file_name("shortcuts.vdf.upnext-new");
        write_synced(&part, &new).with_context(|| format!("writing {}", part.display()))?;
        std::fs::rename(&part, file).with_context(|| format!("writing {}", file.display()))?;
    }
    write_grid(config, target)
}

fn write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}

fn grid_path(config: &Path, target: &Target, suffix: &str) -> PathBuf {
    config.join("grid").join(format!("{}{suffix}", target.shortcut_id(NAME)))
}

/// UpNext's library images, each only where there's none yet: Steam's "Set
/// custom artwork" writes the same files, and what someone chose stays.
fn write_grid(config: &Path, target: &Target) -> Result<()> {
    let grid = config.join("grid");
    std::fs::create_dir_all(&grid).with_context(|| format!("creating {}", grid.display()))?;
    for (suffix, bytes) in GRID {
        let path = grid_path(config, target, suffix);
        if !path.exists() {
            std::fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
        }
    }
    Ok(())
}

/// Steam's Gamepad template for each controller type it has one for, as its
/// `controller_base/templates` folder names them: the layout Steam itself
/// calls "Gamepad", sticks and buttons as a controller's.
const PAD_LAYOUTS: &[(&str, &str)] = &[
    ("xbox360", "controller_xbox360_gamepad_joystick.vdf"),
    ("xboxone", "controller_xboxone_gamepad_joystick.vdf"),
    ("ps3", "controller_ps3_gamepad_joystick.vdf"),
    ("ps4", "controller_ps4_gamepad_joystick.vdf"),
    ("ps5", "controller_ps5_gamepad_joystick.vdf"),
    ("switch_pro", "controller_switch_pro_gamepad_joystick.vdf"),
    ("neptune", "controller_neptune_gamepad_joystick.vdf"),
    ("generic", "controller_generic_gamepad_joystick.vdf"),
];

/// The file holding one account's layout choices for controllers of `kind`.
fn layout_file(steam_dir: &Path, account: &str, kind: &str) -> PathBuf {
    steam_dir
        .join("steamapps")
        .join("common")
        .join("Steam Controller Configs")
        .join(account)
        .join("config")
        .join(format!("configset_controller_{kind}.vdf"))
}

/// The key a non-Steam shortcut's layout has: its name in lower case,
/// without the characters Steam ROM Manager leaves out of it.
fn layout_key(name: &str) -> String {
    name.to_lowercase().chars().filter(|c| !r#"/\?%*:|"<>."#.contains(*c)).collect()
}

/// Give UpNext's shortcut the Gamepad template for every controller type in
/// one account, writing each file only when it has no layout for UpNext (so
/// one chosen in Steam stays), copied aside the first time UpNext changes
/// it. What couldn't be done, for the log.
fn add_layouts(steam_dir: &Path, account: &str) -> Vec<String> {
    let mut problems = vec![];
    for (kind, template) in PAD_LAYOUTS {
        let file = layout_file(steam_dir, account, kind);
        if let Err(e) = add_layout(&file, template) {
            problems.push(format!("{}: {e:#}", file.display()));
        }
    }
    problems
}

/// Whether every controller type's file in one account has a layout for
/// UpNext, or can't be given one (it can't be read).
fn has_layouts(steam_dir: &Path, account: &str) -> bool {
    PAD_LAYOUTS.iter().all(|(kind, template)| {
        let text = match std::fs::read_to_string(layout_file(steam_dir, account, kind)) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return true,
        };
        !matches!(with_layout(text.as_deref(), template), Ok(Some(_)))
    })
}

fn add_layout(file: &Path, template: &str) -> Result<()> {
    let old = match std::fs::read_to_string(file) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).context("reading it"),
    };
    let Some(new) = with_layout(old.as_deref(), template)? else { return Ok(()) };
    let folder = file.parent().context("a file with no folder")?;
    std::fs::create_dir_all(folder).with_context(|| format!("creating {}", folder.display()))?;
    let name = file.file_name().context("a file with no name")?.to_string_lossy();
    let backup = file.with_file_name(format!("{name}.upnext-backup"));
    if let Some(old) = &old
        && !backup.exists()
    {
        std::fs::write(&backup, old).context("copying it aside")?;
    }
    let part = file.with_file_name(format!("{name}.upnext-new"));
    std::fs::write(&part, new).with_context(|| format!("writing {}", part.display()))?;
    std::fs::rename(&part, file).context("writing it")
}

/// A configset file, `text` (None for none yet), with UpNext's shortcut
/// given `template`; None when it has a layout for UpNext already. Every
/// other entry stays, in its order. A file that isn't one map called
/// `controller_config` is refused, so nothing in it is lost.
fn with_layout(text: Option<&str>, template: &str) -> Result<Option<String>> {
    use crate::steam_local::Vdf;
    const TOP: &str = "controller_config";
    let mut entries = match text.map(crate::steam_local::parse_text_vdf_strict).transpose().context("reading it")? {
        None => vec![],
        Some(Vdf::Map(root)) => match <[_; 1]>::try_from(root) {
            Ok([(key, Vdf::Map(entries))]) if key.eq_ignore_ascii_case(TOP) => entries,
            Err(root) if root.is_empty() => vec![],
            _ => bail!("it isn't one \"{TOP}\" map, as Steam writes it"),
        },
        Some(Vdf::Str(_)) => bail!("it isn't one \"{TOP}\" map, as Steam writes it"),
    };
    let key = layout_key(NAME);
    if entries.iter().any(|(k, _)| k.eq_ignore_ascii_case(&key)) {
        return Ok(None);
    }
    entries.push((key, Vdf::Map(vec![("template".into(), Vdf::Str(template.into()))])));
    let mut out = String::new();
    write_vdf(&mut out, &[(TOP.to_string(), Vdf::Map(entries))], 0);
    Ok(Some(out))
}

/// Text VDF as Steam writes it: keys and values quoted, a tab per level,
/// two tabs between a key and its value.
fn write_vdf(out: &mut String, entries: &[(String, crate::steam_local::Vdf)], depth: usize) {
    use crate::steam_local::Vdf;
    let quoted = |s: &str| {
        let escaped = s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\t', "\\t");
        format!("\"{escaped}\"")
    };
    let indent = "\t".repeat(depth);
    for (key, value) in entries {
        match value {
            Vdf::Str(v) => out.push_str(&format!("{indent}{}\t\t{}\n", quoted(key), quoted(v))),
            Vdf::Map(children) => {
                out.push_str(&format!("{indent}{}\n{indent}{{\n", quoted(key)));
                write_vdf(out, children, depth + 1);
                out.push_str(&format!("{indent}}}\n"));
            }
        }
    }
}

/// Whether Steam is running on this computer. On Windows, `steam.exe` in the
/// process list; elsewhere a process called `steam` (the native client,
/// the Flatpak's and the Snap's are all called that).
pub fn steam_running() -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let listed =
            std::process::Command::new("tasklist").args(["/NH", "/FI", "IMAGENAME eq steam.exe"]).creation_flags(CREATE_NO_WINDOW).output();
        match listed {
            Ok(out) => String::from_utf8_lossy(&out.stdout).to_ascii_lowercase().contains("steam.exe"),
            // Not knowing is no reason to write under a running Steam.
            Err(e) => {
                tracing::warn!("couldn't list the running programs: {e}");
                true
            }
        }
    }
    #[cfg(not(windows))]
    {
        // As on Windows: not knowing is no reason to write under a running Steam.
        let procs = match std::fs::read_dir("/proc") {
            Ok(procs) => procs,
            Err(e) => {
                tracing::warn!("couldn't list the running programs: {e}");
                return true;
            }
        };
        procs.filter_map(|e| e.ok()).any(|e| std::fs::read_to_string(e.path().join("comm")).is_ok_and(|c| c.trim_end() == "steam"))
    }
}

// ---------------------------------------------------------------------------
// shortcuts.vdf
// ---------------------------------------------------------------------------

/// The binary key-value format's value types: a map, a string, a 32-bit
/// integer, and the rest only to step over them.
const MAP: u8 = 0x00;
const STRING: u8 = 0x01;
const INT: u8 = 0x02;
const FLOAT: u8 = 0x03;
const POINTER: u8 = 0x04;
const COLOR: u8 = 0x06;
const UINT64: u8 = 0x07;
const END: u8 = 0x08;
const INT64: u8 = 0x0a;

/// How deep the maps may nest (a shortcut's `tags` is the only map in one).
const MAX_DEPTH: usize = 16;

/// One shortcut, as far as UpNext needs to read it.
#[derive(Debug, Default)]
struct Shortcut {
    key: String,
    appid: Option<u32>,
    name: Option<String>,
    exe: Option<String>,
}

/// The file, read: where each shortcut is in it, and the bytes themselves.
struct Shortcuts<'a> {
    bytes: &'a [u8],
    entries: Vec<Shortcut>,
    /// Where the shortcuts map ends (its END byte), which is where a new
    /// shortcut goes.
    end: usize,
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn u8(&mut self) -> Result<u8> {
        let b = *self.bytes.get(self.at).context("the file ends early")?;
        self.at += 1;
        Ok(b)
    }

    fn skip(&mut self, n: usize) -> Result<()> {
        if self.bytes.len() - self.at < n {
            bail!("the file ends early");
        }
        self.at += n;
        Ok(())
    }

    fn i32(&mut self) -> Result<i32> {
        let raw = self.bytes.get(self.at..self.at + 4).context("the file ends early")?;
        self.at += 4;
        Ok(i32::from_le_bytes(raw.try_into()?))
    }

    fn cstr(&mut self) -> Result<String> {
        let len = self.bytes[self.at..].iter().position(|&b| b == 0).context("a string with no end")?;
        let s = String::from_utf8_lossy(&self.bytes[self.at..self.at + len]).into_owned();
        self.at += len + 1;
        Ok(s)
    }

    /// Step over a map's contents and its END.
    fn skip_map(&mut self, depth: usize) -> Result<()> {
        if depth > MAX_DEPTH {
            bail!("maps nested too deep");
        }
        loop {
            let ty = self.u8()?;
            if ty == END {
                return Ok(());
            }
            self.cstr()?;
            self.skip_value(ty, depth)?;
        }
    }

    fn skip_value(&mut self, ty: u8, depth: usize) -> Result<()> {
        match ty {
            MAP => self.skip_map(depth + 1),
            STRING => self.cstr().map(drop),
            INT | FLOAT | POINTER | COLOR => self.skip(4),
            UINT64 | INT64 => self.skip(8),
            other => bail!("a value of type {other:#04x}, which UpNext doesn't know"),
        }
    }
}

impl<'a> Shortcuts<'a> {
    /// A file with no shortcuts, for an account that has none yet.
    const EMPTY: &'static [u8] = b"\x00shortcuts\x00\x08\x08";

    fn parse(bytes: &'a [u8]) -> Result<Self> {
        let mut r = Reader { bytes, at: 0 };
        if r.u8()? != MAP || !r.cstr()?.eq_ignore_ascii_case("shortcuts") {
            bail!("not Steam's shortcuts file");
        }
        let mut entries = Vec::new();
        loop {
            match r.u8()? {
                END => break,
                MAP => {}
                other => bail!("a value of type {other:#04x} where a shortcut should be"),
            }
            let mut shortcut = Shortcut { key: r.cstr()?, ..Shortcut::default() };
            loop {
                let ty = r.u8()?;
                if ty == END {
                    break;
                }
                let key = r.cstr()?.to_ascii_lowercase();
                match (ty, key.as_str()) {
                    (INT, "appid") => shortcut.appid = Some(r.i32()? as u32),
                    (STRING, "appname") => shortcut.name = Some(r.cstr()?),
                    (STRING, "exe") => shortcut.exe = Some(r.cstr()?),
                    _ => r.skip_value(ty, 1)?,
                }
            }
            entries.push(shortcut);
        }
        // The shortcuts map's END, then the file's own.
        let end = r.at - 1;
        if r.u8()? != END || r.at != bytes.len() {
            bail!("something after the shortcuts that UpNext doesn't know");
        }
        Ok(Self { bytes, entries, end })
    }

    /// UpNext's own shortcut: Steam's id for its program and name, or the
    /// same program and name when the id isn't there.
    fn find(&self, target: &Target) -> Option<&Shortcut> {
        let id = target.shortcut_id(NAME);
        let exe = target.exe_value();
        self.entries.iter().find(|s| s.appid == Some(id) || (s.name.as_deref() == Some(NAME) && s.exe.as_deref() == Some(exe.as_str())))
    }

    /// The file with `fields` added as one more shortcut, after the others,
    /// which stay exactly as they were.
    fn with(&self, fields: &[(&str, Value)]) -> Vec<u8> {
        let taken = |k: &str| self.entries.iter().any(|s| s.key == k);
        let mut index = self.entries.len();
        while taken(&index.to_string()) {
            index += 1;
        }
        let mut out = self.bytes[..self.end].to_vec();
        out.push(MAP);
        push_str(&mut out, &index.to_string());
        for (key, value) in fields {
            match value {
                Value::Str(s) => {
                    out.push(STRING);
                    push_str(&mut out, key);
                    push_str(&mut out, s);
                }
                Value::Int(n) => {
                    out.push(INT);
                    push_str(&mut out, key);
                    out.extend_from_slice(&n.to_le_bytes());
                }
                Value::EmptyMap => {
                    out.push(MAP);
                    push_str(&mut out, key);
                    out.push(END);
                }
            }
        }
        out.push(END);
        out.extend_from_slice(&self.bytes[self.end..]);
        out
    }
}

fn push_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
    out.push(0);
}

enum Value {
    Str(String),
    Int(i32),
    EmptyMap,
}

/// UpNext's shortcut, with the fields in the order Steam writes them.
fn entry(target: &Target, icon: &Path) -> Vec<(&'static str, Value)> {
    let s = |v: &str| Value::Str(v.to_string());
    vec![
        ("appid", Value::Int(target.shortcut_id(NAME) as i32)),
        ("AppName", s(NAME)),
        ("Exe", Value::Str(target.exe_value())),
        ("StartDir", Value::Str(target.start_dir_value())),
        ("icon", Value::Str(icon.display().to_string())),
        ("ShortcutPath", s("")),
        ("LaunchOptions", Value::Str(target.arguments.clone())),
        ("IsHidden", Value::Int(0)),
        ("AllowDesktopConfig", Value::Int(1)),
        ("AllowOverlay", Value::Int(1)),
        ("OpenVR", Value::Int(0)),
        ("Devkit", Value::Int(0)),
        ("DevkitGameID", s("")),
        ("DevkitOverrideAppID", Value::Int(0)),
        ("LastPlayTime", Value::Int(0)),
        ("FlatpakAppID", s("")),
        ("tags", Value::EmptyMap),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A shortcuts file as Steam writes one, with two games added by hand:
    /// every field Steam writes, a tag, and a 64-bit value UpNext only
    /// steps over.
    fn steams_file() -> Vec<u8> {
        let mut out = vec![MAP];
        push_str(&mut out, "shortcuts");
        for (i, (name, exe)) in [("Some Game", "\"/opt/some game/run.sh\""), ("Emulator", "\"/usr/bin/emu\"")].iter().enumerate() {
            out.push(MAP);
            push_str(&mut out, &i.to_string());
            let int = |out: &mut Vec<u8>, k: &str, v: i32| {
                out.push(INT);
                push_str(out, k);
                out.extend_from_slice(&v.to_le_bytes());
            };
            let string = |out: &mut Vec<u8>, k: &str, v: &str| {
                out.push(STRING);
                push_str(out, k);
                push_str(out, v);
            };
            int(&mut out, "appid", -1_234_567 - i as i32);
            string(&mut out, "AppName", name);
            string(&mut out, "Exe", exe);
            string(&mut out, "StartDir", "\"/opt/\"");
            string(&mut out, "icon", "");
            string(&mut out, "ShortcutPath", "");
            string(&mut out, "LaunchOptions", "-fullscreen");
            int(&mut out, "IsHidden", 0);
            int(&mut out, "AllowDesktopConfig", 1);
            int(&mut out, "AllowOverlay", 1);
            int(&mut out, "OpenVR", 0);
            int(&mut out, "Devkit", 0);
            string(&mut out, "DevkitGameID", "");
            int(&mut out, "DevkitOverrideAppID", 0);
            int(&mut out, "LastPlayTime", 1_760_000_000);
            string(&mut out, "FlatpakAppID", "");
            out.push(UINT64);
            push_str(&mut out, "SomethingNewer");
            out.extend_from_slice(&42u64.to_le_bytes());
            out.push(MAP);
            push_str(&mut out, "tags");
            string(&mut out, "0", "Favourites");
            out.push(END);
            out.push(END);
        }
        out.extend_from_slice(&[END, END]);
        out
    }

    fn target() -> Target {
        Target::for_exe(Path::new("/home/you/.cargo/bin/upnext"), false)
    }

    /// A Steam folder with one account whose shortcuts file is `file`.
    fn steam_with(file: Option<&[u8]>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("userdata/12345678/config");
        std::fs::create_dir_all(&config).unwrap();
        if let Some(bytes) = file {
            std::fs::write(config.join("shortcuts.vdf"), bytes).unwrap();
        }
        dir
    }

    #[test]
    fn the_target_is_the_app_window_in_big_screen() {
        let linux = Target::for_exe(Path::new("/usr/bin/upnext"), false);
        assert_eq!((linux.program.as_path(), linux.arguments.as_str()), (Path::new("/usr/bin/upnext"), "app --big-screen"));
        let app = Target::for_exe(Path::new("/usr/bin/upnext-app"), false);
        assert_eq!(app.arguments, "--big-screen");
        let windows = Target::for_exe(Path::new("/install/upnext.exe"), true);
        assert_eq!((windows.program.as_path(), windows.arguments.as_str()), (Path::new("/install/upnext-app.exe"), "--big-screen"));
        assert_eq!(linux.exe_value(), "\"/usr/bin/upnext\"");
        assert_eq!(linux.start_dir_value(), "\"/usr/bin\"");
    }

    #[test]
    fn the_id_is_steams_for_the_program_and_name() {
        // The CRC-32 of `"/usr/bin/upnext"UpNext`, with the top bit set.
        let id = Target::for_exe(Path::new("/usr/bin/upnext"), false).shortcut_id(NAME);
        assert_eq!(id, crc32fast::hash(b"\"/usr/bin/upnext\"UpNext") | 0x8000_0000);
        assert!(id >= 0x8000_0000);
    }

    #[test]
    fn steams_file_reads_and_other_shortcuts_stay_byte_for_byte() {
        let file = steams_file();
        let read = Shortcuts::parse(&file).unwrap();
        assert_eq!(read.entries.len(), 2);
        assert_eq!(read.entries[0].name.as_deref(), Some("Some Game"));
        assert_eq!(read.entries[1].exe.as_deref(), Some("\"/usr/bin/emu\""));
        assert!(read.find(&target()).is_none());

        let new = read.with(&entry(&target(), Path::new("/icon.png")));
        // Everything before the end of the shortcuts map is as it was.
        assert_eq!(&new[..file.len() - 2], &file[..file.len() - 2]);
        assert!(new.ends_with(&[END, END, END]));
        let again = Shortcuts::parse(&new).unwrap();
        assert_eq!(again.entries.len(), 3);
        let ours = again.find(&target()).unwrap();
        assert_eq!(ours.key, "2");
        assert_eq!(ours.appid, Some(target().shortcut_id(NAME)));
        assert_eq!(ours.exe.as_deref(), Some("\"/home/you/.cargo/bin/upnext\""));
        // Any other key is free to use; a taken one is skipped.
        let mut gap = Shortcuts::parse(&file).unwrap();
        gap.entries[1].key = "2".into();
        assert_eq!(Shortcuts::parse(&gap.with(&entry(&target(), Path::new("")))).unwrap().entries[2].key, "3");
    }

    #[test]
    fn files_upnext_doesnt_know_are_left_alone() {
        assert!(Shortcuts::parse(b"").is_err());
        assert!(Shortcuts::parse(b"\x00something\x00\x08\x08").is_err());
        assert!(Shortcuts::parse(b"\x00shortcuts\x00\x00\x30\x00\x09odd\x00\x08\x08\x08").is_err());
        assert!(Shortcuts::parse(b"\x00shortcuts\x00\x08\x08more").is_err());
        assert!(Shortcuts::parse(b"\x00shortcuts\x00\x00\x30\x00\x01name").is_err(), "cut short");
        // Nested deeper than any of Steam's.
        let mut deep = b"\x00shortcuts\x00\x00\x30\x00".to_vec();
        for _ in 0..40 {
            deep.extend_from_slice(b"\x00m\x00");
        }
        assert!(Shortcuts::parse(&deep).is_err());
        assert!(Shortcuts::parse(Shortcuts::EMPTY).unwrap().entries.is_empty());
    }

    #[test]
    fn adding_writes_once_keeps_a_copy_and_the_images() {
        let file = steams_file();
        let steam = steam_with(Some(&file));
        let config = steam.path().join("userdata/12345678/config");
        assert_eq!(
            status(steam.path(), &target()),
            [Account { account_id: "12345678".into(), added: false, layout: false, layout_error: None, error: None }]
        );

        let added = add(steam.path(), &target(), || false).unwrap();
        assert_eq!(added, [Account { account_id: "12345678".into(), added: true, layout: true, layout_error: None, error: None }]);
        assert_eq!(std::fs::read(config.join("shortcuts.vdf.upnext-backup")).unwrap(), file);
        let once = std::fs::read(config.join("shortcuts.vdf")).unwrap();
        assert_eq!(Shortcuts::parse(&once).unwrap().entries.len(), 3);
        let id = target().shortcut_id(NAME);
        for &(suffix, bytes) in GRID {
            assert_eq!(std::fs::read(config.join(format!("grid/{id}{suffix}"))).unwrap(), bytes, "{suffix}");
        }
        assert!(!config.join("shortcuts.vdf.upnext-new").exists());

        // Again: nothing changes, and artwork chosen in Steam stays.
        std::fs::write(config.join(format!("grid/{id}p.png")), b"chosen in Steam").unwrap();
        add(steam.path(), &target(), || false).unwrap();
        assert_eq!(std::fs::read(config.join("shortcuts.vdf")).unwrap(), once);
        assert_eq!(std::fs::read(config.join("shortcuts.vdf.upnext-backup")).unwrap(), file);
        assert_eq!(std::fs::read(config.join(format!("grid/{id}p.png"))).unwrap(), b"chosen in Steam");
    }

    /// One account's controller settings for Xbox One pads as Steam writes
    /// them, with a layout chosen for another shortcut and a game's own.
    const CONFIGSET: &str = "\"controller_config\"\n{\n\t\"some game\"\n\t{\n\t\t\"template\"\t\t\"controller_xboxone_wasd.vdf\"\n\t}\n\t\"620\"\n\t{\n\t\t\"workshop\"\t\t\"1234567890\"\n\t\t\"srmAppId\"\t\t\"a12\"\n\t}\n}\n";

    #[test]
    fn the_layout_is_added_once_and_every_other_entry_stays() {
        let pad = "controller_xboxone_gamepad_joystick.vdf";
        assert_eq!(layout_key("UpNext"), "upnext");
        assert_eq!(layout_key("A: Game. \"Two\" / 3?"), "a game two  3");
        // No file yet: one with UpNext's entry alone.
        let made = with_layout(None, pad).unwrap().unwrap();
        assert_eq!(
            made,
            "\"controller_config\"\n{\n\t\"upnext\"\n\t{\n\t\t\"template\"\t\t\"controller_xboxone_gamepad_joystick.vdf\"\n\t}\n}\n"
        );
        assert_eq!(with_layout(Some(""), pad).unwrap().as_deref(), Some(made.as_str()), "an empty file");
        // Steam's file: the others first, as they were, then UpNext's.
        let added = with_layout(Some(CONFIGSET), pad).unwrap().unwrap();
        assert_eq!(
            added,
            format!("{}\t\"upnext\"\n\t{{\n\t\t\"template\"\t\t\"{pad}\"\n\t}}\n}}\n", CONFIGSET.strip_suffix("}\n").unwrap())
        );
        // Once there, or chosen in Steam, it stays.
        assert_eq!(with_layout(Some(&added), pad).unwrap(), None);
        let chosen = CONFIGSET.replace("some game", "UpNext");
        assert_eq!(with_layout(Some(&chosen), pad).unwrap(), None);
        // A file UpNext can't read whole is left alone.
        for odd in
            ["\"something_else\"\n{\n}\n", "\"controller_config\" { } }", "\"controller_config\" { \"a\" {", "\"controller_config\" \"x\""]
        {
            assert!(with_layout(Some(odd), pad).is_err(), "{odd}");
        }
        // Quotes and backslashes in a value come back as they went.
        let escaped = "\"controller_config\"\n{\n\t\"odd \\\"one\\\"\"\n\t{\n\t\t\"template\"\t\t\"C:\\\\x.vdf\"\n\t}\n}\n";
        assert!(with_layout(Some(escaped), pad).unwrap().unwrap().starts_with(escaped.strip_suffix("}\n").unwrap()));
    }

    #[test]
    fn adding_gives_the_shortcut_the_gamepad_layout_for_each_controller() {
        let steam = steam_with(Some(&steams_file()));
        let configs = steam.path().join("steamapps/common/Steam Controller Configs/12345678/config");
        std::fs::create_dir_all(&configs).unwrap();
        std::fs::write(configs.join("configset_controller_xboxone.vdf"), CONFIGSET).unwrap();
        // A layout someone picked in Steam for UpNext on a DualSense.
        let picked = "\"controller_config\"\n{\n\t\"upnext\"\n\t{\n\t\t\"template\"\t\t\"controller_ps5_wasd.vdf\"\n\t}\n}\n";
        std::fs::write(configs.join("configset_controller_ps5.vdf"), picked).unwrap();

        add(steam.path(), &target(), || false).unwrap();
        for (kind, template) in PAD_LAYOUTS {
            let text = std::fs::read_to_string(configs.join(format!("configset_controller_{kind}.vdf"))).unwrap();
            let root = crate::steam_local::parse_text_vdf_strict(&text).unwrap();
            let layout = root.path(&["controller_config", "upnext", "template"]).and_then(|v| v.as_str());
            let want = if *kind == "ps5" { "controller_ps5_wasd.vdf" } else { template };
            assert_eq!(layout, Some(want), "{kind}");
        }
        let xbox = configs.join("configset_controller_xboxone.vdf");
        assert_eq!(std::fs::read_to_string(configs.join("configset_controller_xboxone.vdf.upnext-backup")).unwrap(), CONFIGSET);
        assert!(std::fs::read_to_string(&xbox).unwrap().starts_with(CONFIGSET.strip_suffix("}\n").unwrap()), "the others stay");
        assert!(!configs.join("configset_controller_ps5.vdf.upnext-backup").exists(), "left as it was");
        assert!(!configs.join("configset_controller_neptune.vdf.upnext-backup").exists(), "nothing to copy aside");

        // Again: nothing changes.
        let once = std::fs::read_to_string(&xbox).unwrap();
        add(steam.path(), &target(), || false).unwrap();
        assert_eq!(std::fs::read_to_string(&xbox).unwrap(), once);
    }

    /// A shortcut added before the layouts were written reports none, and
    /// pressing Add to Steam again gives it them without a second shortcut.
    #[test]
    fn a_shortcut_added_before_the_layouts_gets_them_on_the_next_add() {
        let steam = steam_with(Some(&steams_file()));
        add(steam.path(), &target(), || false).unwrap();
        let shortcuts = steam.path().join("userdata/12345678/config/shortcuts.vdf");
        let with_shortcut = std::fs::read(&shortcuts).unwrap();
        // As an older UpNext left it: the shortcut, and no layout files.
        std::fs::remove_dir_all(steam.path().join("steamapps")).unwrap();
        let before = status(steam.path(), &target());
        assert!(before[0].added && !before[0].layout, "{before:?}");
        // One type's file missing is enough to offer it again.
        let after = add(steam.path(), &target(), || false).unwrap();
        assert!(after[0].added && after[0].layout, "{after:?}");
        std::fs::remove_file(layout_file(steam.path(), "12345678", "neptune")).unwrap();
        assert!(!status(steam.path(), &target())[0].layout);
        add(steam.path(), &target(), || false).unwrap();
        assert!(status(steam.path(), &target())[0].layout);
        assert_eq!(std::fs::read(&shortcuts).unwrap(), with_shortcut, "the shortcut once");
        // A file it can't read can't be helped, so it doesn't keep the button.
        std::fs::write(layout_file(steam.path(), "12345678", "neptune"), "not { a vdf } }").unwrap();
        assert!(status(steam.path(), &target())[0].layout);
    }

    /// A controller file UpNext can't read gets no layout, and the shortcut
    /// is added all the same.
    #[test]
    fn an_unreadable_controller_file_is_left_alone() {
        let steam = steam_with(None);
        let configs = steam.path().join("steamapps/common/Steam Controller Configs/12345678/config");
        std::fs::create_dir_all(&configs).unwrap();
        std::fs::write(configs.join("configset_controller_xboxone.vdf"), "not { a vdf } }").unwrap();
        let added = add(steam.path(), &target(), || false).unwrap();
        assert!(added[0].added);
        assert_eq!(std::fs::read_to_string(configs.join("configset_controller_xboxone.vdf")).unwrap(), "not { a vdf } }");
        let problems = add_layouts(steam.path(), "12345678");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("configset_controller_xboxone.vdf"), "{problems:?}");
    }

    /// A layout that can't be written is reported with why, so Settings
    /// stops offering a press that would fail the same way, and a press
    /// that works clears it.
    #[test]
    fn a_layout_that_cant_be_written_says_why() {
        let steam = steam_with(None);
        let configs = steam.path().join("steamapps/common/Steam Controller Configs/12345678/config");
        // A folder where the new file goes fails the write even for root.
        let blocker = configs.join("configset_controller_ps5.vdf.upnext-new");
        std::fs::create_dir_all(&blocker).unwrap();
        let added = add(steam.path(), &target(), || false).unwrap();
        assert!(added[0].added && !added[0].layout, "{added:?}");
        let why = added[0].layout_error.as_deref().unwrap_or_default();
        assert!(why.contains("configset_controller_ps5.vdf.upnext-new"), "{why}");
        assert_eq!(status(steam.path(), &target())[0].layout_error.as_deref(), Some(why), "kept between asks");
        std::fs::remove_dir(&blocker).unwrap();
        let fixed = add(steam.path(), &target(), || false).unwrap();
        assert!(fixed[0].layout && fixed[0].layout_error.is_none(), "{fixed:?}");
    }

    #[test]
    fn an_account_with_no_shortcuts_gets_a_file() {
        let steam = steam_with(None);
        add(steam.path(), &target(), || false).unwrap();
        let config = steam.path().join("userdata/12345678/config");
        let made = std::fs::read(config.join("shortcuts.vdf")).unwrap();
        assert_eq!(Shortcuts::parse(&made).unwrap().entries.len(), 1);
        assert!(!config.join("shortcuts.vdf.upnext-backup").exists(), "nothing to copy aside");
    }

    #[test]
    fn nothing_is_written_while_steam_runs_or_with_no_account() {
        let file = steams_file();
        let steam = steam_with(Some(&file));
        let err = add(steam.path(), &target(), || true).unwrap_err();
        assert_eq!(err.downcast_ref::<Refused>(), Some(&Refused::SteamRunning));
        let config = steam.path().join("userdata/12345678/config");
        assert_eq!(std::fs::read(config.join("shortcuts.vdf")).unwrap(), file);
        assert!(!config.join("grid").exists());

        let none = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(none.path().join("userdata/0")).unwrap();
        std::fs::create_dir_all(none.path().join("userdata/anonymous")).unwrap();
        let err = add(none.path(), &target(), || panic!("not asked without an account")).unwrap_err();
        assert_eq!(err.downcast_ref::<Refused>(), Some(&Refused::NoAccounts));
    }

    /// The code-health review's B-STORES-11: a program path that isn't valid UTF-8
    /// would be stored with its odd bytes replaced and point nowhere, so
    /// nothing is written and the reason says what to do.
    #[cfg(unix)]
    #[test]
    fn a_path_that_isnt_text_is_refused() {
        use std::os::unix::ffi::OsStrExt;
        let file = steams_file();
        let steam = steam_with(Some(&file));
        let odd =
            Target { program: std::path::PathBuf::from(std::ffi::OsStr::from_bytes(b"/opt/up\xffnext/upnext")), arguments: "app".into() };
        let err = add(steam.path(), &odd, || false).unwrap_err();
        assert!(matches!(err.downcast_ref::<Refused>(), Some(Refused::PathNotText(_))), "{err:#}");
        let config = steam.path().join("userdata/12345678/config");
        assert_eq!(std::fs::read(config.join("shortcuts.vdf")).unwrap(), file);
    }

    #[test]
    fn every_account_gets_it() {
        let steam = steam_with(None);
        std::fs::create_dir_all(steam.path().join("userdata/99/config")).unwrap();
        let accounts: Vec<String> =
            add(steam.path(), &target(), || false).unwrap().into_iter().filter(|a| a.added).map(|a| a.account_id).collect();
        assert_eq!(accounts, ["99", "12345678"]);
    }

    #[test]
    fn one_unreadable_file_stops_every_account_and_says_which() {
        let steam = steam_with(Some(b"not Steam's file"));
        std::fs::create_dir_all(steam.path().join("userdata/99/config")).unwrap();
        let shown = status(steam.path(), &target());
        assert_eq!(shown[0], Account { account_id: "99".into(), added: false, layout: false, layout_error: None, error: None });
        assert_eq!(shown[1].account_id, "12345678");
        assert!(shown[1].error.as_deref().is_some_and(|e| e.contains("shortcuts.vdf")), "{shown:?}");

        let err = add(steam.path(), &target(), || false).unwrap_err();
        assert!(matches!(err.downcast_ref::<Refused>(), Some(Refused::Unreadable(account, _)) if account == "12345678"), "{err:#}");
        assert!(!steam.path().join("userdata/99/config/shortcuts.vdf").exists(), "the readable account is left as it was too");
        assert!(!steam.path().join("userdata/99/config/grid").exists());
    }

    #[test]
    fn the_first_copy_is_kept() {
        let file = steams_file();
        let steam = steam_with(Some(&file));
        let config = steam.path().join("userdata/12345678/config");
        add(steam.path(), &target(), || false).unwrap();
        // UpNext removed in Steam, which writes the file without it, then added again.
        let removed_in_steam = Shortcuts::EMPTY.to_vec();
        std::fs::write(config.join("shortcuts.vdf"), &removed_in_steam).unwrap();
        add(steam.path(), &target(), || false).unwrap();
        assert_eq!(std::fs::read(config.join("shortcuts.vdf.upnext-backup")).unwrap(), file, "the file as it was before UpNext");
    }
}
