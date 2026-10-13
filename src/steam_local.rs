//! Steam library from the local Steam client's cache files, with no API key:
//!
//! - `config/loginusers.vdf`            -> SteamID64 of the most recent account
//! - `appcache/packageinfo.vdf`         -> owned packages and their appids (binary KV),
//!   for the account signed in last
//! - `steamapps/appmanifest_<appid>.acf` -> which account installed a game (`LastOwner`)
//! - `appcache/appinfo.vdf`             -> app name + type (binary KV, string table)
//! - `userdata/<account>/config/localconfig.vdf` -> playtime and last played (text VDF)
//! - `config/libraryfolders.vdf`        -> installed appids (text VDF)
//!
//! Formats are undocumented; parsing mirrors SteamKit2 (appinfo v29,
//! packageinfo v28) and is defensive: unknown magic = clear error.

use crate::db::ImportedGame;
use anyhow::{Context, Result, bail};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const APPINFO_V28: u32 = 0x07_56_44_28;
const APPINFO_V29: u32 = 0x07_56_44_29;
const PACKAGEINFO_V28: u32 = 0x06_56_55_28;

/// How deep a cache file's objects may nest. Steam's own files stay under
/// ten levels; the cap keeps a damaged or crafted file from overflowing the
/// stack, which would abort the whole server rather than fail one read.
const MAX_DEPTH: usize = 64;

/// The most keys appinfo.vdf's string table may hold. Steam's table is the
/// set of distinct key names, some thousands; each entry costs a `String`
/// (24 bytes) even when empty, so a file of nothing but NULs could otherwise
/// cost 24 times its own size before anything else is checked.
const MAX_STRINGS: usize = 1 << 20;

// ---------------------------------------------------------------------------
// Locating the client
// ---------------------------------------------------------------------------

/// The Steam client's folder: the first candidate with the app cache and
/// a login (`steam_dir_candidates`).
pub fn find_steam_dir(explicit: Option<&Path>) -> Option<PathBuf> {
    first_steam_dir(steam_dir_candidates(explicit))
}

/// Folders the Steam client may be in, most likely first: the `steam_dir`
/// setting, then on Windows the folder Steam records in the registry (it
/// can be on any drive), the places under `HOME` ([`home_steam_dirs`]),
/// and Program Files (x86).
pub fn steam_dir_candidates(explicit: Option<&Path>) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = explicit.map(Path::to_path_buf).into_iter().collect();
    // Tests never read the Steam client of the machine they run on, so they
    // pass the same with Steam installed.
    if cfg!(test) {
        return candidates;
    }
    candidates.extend(registry_steam_dirs());
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        candidates.extend(home_steam_dirs(&home));
    }
    if let Some(pf) = std::env::var_os("PROGRAMFILES(X86)").map(PathBuf::from) {
        candidates.push(pf.join("Steam"));
    }
    candidates
}

/// Steam's places under a home folder: Linux's own, the Flatpak's, the
/// Snap's (a snap's home is `~/snap/<name>/common`), then macOS's.
fn home_steam_dirs(home: &Path) -> [PathBuf; 5] {
    [
        home.join(".local/share/Steam"),
        home.join(".steam/steam"),
        home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
        home.join("snap/steam/common/.local/share/Steam"),
        home.join("Library/Application Support/Steam"),
    ]
}

/// The Steam client's folder in each of `homes` that has one, for setup to
/// offer (`other_users`). Only whether its app cache and login file are
/// there is checked.
pub fn steam_dirs_in(homes: &[PathBuf]) -> Vec<PathBuf> {
    homes.iter().filter_map(|home| first_steam_dir(home_steam_dirs(home))).collect()
}

/// The first of `candidates` with the app cache and a login.
fn first_steam_dir(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|p| p.join("appcache/appinfo.vdf").is_file() && p.join("config/loginusers.vdf").is_file())
}

/// Where Steam says it is: `SteamPath` under `HKCU\Software\Valve\Steam`,
/// which the client writes each time it starts, then the installer's
/// `InstallPath` under HKLM.
#[cfg(windows)]
fn registry_steam_dirs() -> Vec<PathBuf> {
    use winreg::{HKCU, HKLM};
    [(HKCU, r"Software\Valve\Steam", "SteamPath"), (HKLM, r"SOFTWARE\WOW6432Node\Valve\Steam", "InstallPath")]
        .into_iter()
        .filter_map(|(root, key, value)| registry_dir(root, key, value))
        .collect()
}

#[cfg(not(windows))]
fn registry_steam_dirs() -> Vec<PathBuf> {
    vec![]
}

#[cfg(windows)]
fn registry_dir(root: &winreg::RegKey, key: &str, value: &str) -> Option<PathBuf> {
    let raw: String = root.open_subkey(key).ok()?.get_value(value).ok()?;
    registry_path(&raw)
}

/// A folder as the registry holds it. `SteamPath` uses forward slashes
/// (`c:/program files (x86)/steam`); they become the platform's own.
#[cfg(any(windows, test))]
fn registry_path(raw: &str) -> Option<PathBuf> {
    let raw = raw.trim();
    (!raw.is_empty()).then(|| PathBuf::from(raw.replace('/', std::path::MAIN_SEPARATOR_STR)))
}

// ---------------------------------------------------------------------------
// Text VDF
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Vdf {
    Str(String),
    Map(Vec<(String, Vdf)>),
}

impl Vdf {
    pub fn get(&self, key: &str) -> Option<&Vdf> {
        match self {
            Vdf::Map(m) => m.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn path(&self, keys: &[&str]) -> Option<&Vdf> {
        keys.iter().try_fold(self, |v, k| v.get(k))
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Vdf::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn entries(&self) -> &[(String, Vdf)] {
        match self {
            Vdf::Map(m) => m,
            _ => &[],
        }
    }
}

pub fn parse_text_vdf(text: &str) -> Result<Vdf> {
    parse_text(text, false)
}

/// `parse_text_vdf` for a file UpNext will write back: a `}` with no `{`
/// before it, or a map the file never closes, is an error rather than the
/// end of what's read, so nothing after it can be lost in the rewrite.
pub fn parse_text_vdf_strict(text: &str) -> Result<Vdf> {
    parse_text(text, true)
}

fn parse_text(text: &str, strict: bool) -> Result<Vdf> {
    let mut chars = text.chars().peekable();
    let mut root: Vec<(String, Vdf)> = vec![];
    parse_pairs(&mut chars, &mut root, 0, strict)?;
    Ok(Vdf::Map(root))
}

fn parse_pairs(chars: &mut std::iter::Peekable<std::str::Chars>, out: &mut Vec<(String, Vdf)>, depth: usize, strict: bool) -> Result<()> {
    if depth > MAX_DEPTH {
        bail!("VDF nested more than {MAX_DEPTH} levels deep");
    }
    loop {
        skip_ws_and_comments(chars);
        match chars.peek() {
            None if strict && depth > 0 => bail!("a map in the VDF that never closes"),
            None => return Ok(()),
            Some('}') if strict && depth == 0 => bail!("a \"}}\" in the VDF with no \"{{\" before it"),
            Some('}') => {
                chars.next();
                return Ok(());
            }
            Some(_) => {}
        }
        let key = read_token(chars)?;
        skip_ws_and_comments(chars);
        match chars.peek() {
            Some('{') => {
                chars.next();
                let mut children = vec![];
                parse_pairs(chars, &mut children, depth + 1, strict)?;
                out.push((key, Vdf::Map(children)));
            }
            Some(_) => {
                let value = read_token(chars)?;
                out.push((key, Vdf::Str(value)));
            }
            None => bail!("unexpected end of VDF after key {key:?}"),
        }
    }
}

fn skip_ws_and_comments(chars: &mut std::iter::Peekable<std::str::Chars>) {
    loop {
        while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
            chars.next();
        }
        if chars.peek() == Some(&'/') {
            let mut c2 = chars.clone();
            c2.next();
            if c2.peek() == Some(&'/') {
                while !matches!(chars.next(), None | Some('\n')) {}
                continue;
            }
        }
        return;
    }
}

fn read_token(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<String> {
    let mut s = String::new();
    if chars.peek() == Some(&'"') {
        chars.next();
        while let Some(c) = chars.next() {
            match c {
                '"' => return Ok(s),
                '\\' => {
                    if let Some(n) = chars.next() {
                        s.push(match n {
                            'n' => '\n',
                            't' => '\t',
                            other => other,
                        });
                    }
                }
                other => s.push(other),
            }
        }
        bail!("unterminated string in VDF");
    }
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() || c == '{' || c == '}' {
            break;
        }
        s.push(c);
        chars.next();
    }
    Ok(s)
}

// ---------------------------------------------------------------------------
// Binary KV (appinfo / packageinfo)
// ---------------------------------------------------------------------------

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn u8(&mut self) -> Result<u8> {
        let v = *self.data.get(self.pos).context("unexpected end of binary VDF")?;
        self.pos += 1;
        Ok(v)
    }
    /// The next `n` bytes; an offset from the file can point anywhere, so
    /// the end is computed without overflowing.
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let b = self.pos.checked_add(n).and_then(|end| self.data.get(self.pos..end)).context("unexpected end of binary VDF")?;
        self.pos += n;
        Ok(b)
    }
    fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }
    fn u64(&mut self) -> Result<u64> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes(b.try_into().unwrap()))
    }
    fn skip(&mut self, n: usize) -> Result<()> {
        self.take(n).map(|_| ())
    }
    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }
    fn cstr(&mut self) -> Result<String> {
        let start = self.pos;
        while *self.data.get(self.pos).context("unterminated string in binary VDF")? != 0 {
            self.pos += 1;
        }
        let s = String::from_utf8_lossy(&self.data[start..self.pos]).into_owned();
        self.pos += 1;
        Ok(s)
    }
}

/// Parse one binary KV object. `strings` is the appinfo v29 key table; when
/// `None`, keys are inline null-terminated strings.
fn parse_binary_kv(c: &mut Cursor, strings: Option<&[String]>, depth: usize) -> Result<Vdf> {
    if depth > MAX_DEPTH {
        bail!("binary VDF nested more than {MAX_DEPTH} levels deep at {}", c.pos);
    }
    let mut out = vec![];
    loop {
        let ty = c.u8()?;
        if ty == 0x08 || ty == 0x0B {
            return Ok(Vdf::Map(out));
        }
        let key = match strings {
            Some(table) => {
                let idx = c.i32()?;
                table.get(idx as usize).cloned().unwrap_or_else(|| format!("#{idx}"))
            }
            None => c.cstr()?,
        };
        let value = match ty {
            0x00 => parse_binary_kv(c, strings, depth + 1)?,
            0x01 => Vdf::Str(c.cstr()?),
            0x02 | 0x03 | 0x04 | 0x06 => Vdf::Str(c.u32()?.to_string()),
            0x05 => {
                // wide string: length-prefixed UTF-16? rare; skip by reading until double NUL
                let mut s = String::new();
                loop {
                    let lo = c.u8()?;
                    let hi = c.u8()?;
                    let ch = u16::from_le_bytes([lo, hi]);
                    if ch == 0 {
                        break;
                    }
                    s.push(char::from_u32(ch as u32).unwrap_or('?'));
                }
                Vdf::Str(s)
            }
            0x07 | 0x0A => Vdf::Str(c.u64()?.to_string()),
            other => bail!("unknown binary VDF type 0x{other:02x} at {}", c.pos),
        };
        out.push((key, value));
    }
}

#[derive(Debug, Clone, Default)]
pub struct AppInfo {
    pub name: String,
    pub type_: String,
}

/// Names and types for every app in appinfo.vdf.
pub fn read_appinfo(path: &Path) -> Result<HashMap<u32, AppInfo>> {
    let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    parse_appinfo(&data).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))
}

fn parse_appinfo(data: &[u8]) -> Result<HashMap<u32, AppInfo>> {
    let mut c = Cursor { data, pos: 0 };
    let magic = c.u32()?;
    let _universe = c.u32()?;
    let strings: Option<Vec<String>> = match magic {
        APPINFO_V29 => {
            let offset = usize::try_from(c.u64()?).context("appinfo.vdf's string table is past the end of the file")?;
            let mut sc = Cursor { data, pos: offset };
            let count = sc.u32()? as usize;
            // The count comes from the file: every string takes at least its
            // terminating NUL, so a count beyond the bytes left is damage, as
            // is one far beyond any real table. The table grows as strings
            // are read rather than being reserved from the count up front.
            if count > sc.remaining() {
                bail!("appinfo.vdf's string table claims {count} strings in {} bytes", sc.remaining());
            }
            if count > MAX_STRINGS {
                bail!("appinfo.vdf's string table claims {count} strings; the most expected is {MAX_STRINGS}");
            }
            let mut table = Vec::with_capacity(count.min(4096));
            for _ in 0..count {
                table.push(sc.cstr()?);
            }
            Some(table)
        }
        APPINFO_V28 => None,
        other => bail!("unsupported appinfo.vdf version 0x{other:08x}"),
    };
    let mut out = HashMap::new();
    loop {
        let appid = c.u32()?;
        if appid == 0 {
            break;
        }
        let size = c.u32()? as usize;
        let entry_end = c.pos.saturating_add(size);
        // infoState, lastUpdated, picsToken, sha1[20], changeNumber, sha1_binary[20]
        c.skip(4 + 4 + 8 + 20 + 4 + 20)?;
        let kv = match parse_binary_kv(&mut c, strings.as_deref(), 0) {
            Ok(kv) => kv,
            Err(e) => {
                tracing::debug!("appinfo {appid}: {e}; skipping");
                c.pos = entry_end;
                continue;
            }
        };
        c.pos = entry_end;
        let common = kv.get("appinfo").and_then(|a| a.get("common")).or_else(|| kv.get("common"));
        if let Some(common) = common {
            let name = common.get("name").and_then(Vdf::as_str).unwrap_or("").to_string();
            let type_ = common.get("type").and_then(Vdf::as_str).unwrap_or("").to_lowercase();
            if !name.is_empty() {
                out.insert(appid, AppInfo { name, type_ });
            }
        }
    }
    Ok(out)
}

/// Appids from every package in packageinfo.vdf (the client only caches
/// packages the account has licenses for, plus a few free ones).
pub fn read_packageinfo(path: &Path) -> Result<HashSet<u32>> {
    let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    parse_packageinfo(&data).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))
}

fn parse_packageinfo(data: &[u8]) -> Result<HashSet<u32>> {
    let mut c = Cursor { data, pos: 0 };
    let magic = c.u32()?;
    let _universe = c.u32()?;
    if magic != PACKAGEINFO_V28 {
        bail!("unsupported packageinfo.vdf version 0x{magic:08x}");
    }
    let mut appids = HashSet::new();
    loop {
        let packageid = c.u32()?;
        if packageid == 0xFFFF_FFFF {
            break;
        }
        // sha1[20], changeNumber, picsToken
        c.skip(20 + 4 + 8)?;
        let kv = parse_binary_kv(&mut c, None, 0)?;
        let pkg = kv.entries().first().map(|(_, v)| v).unwrap_or(&kv);
        if let Some(apps) = pkg.get("appids") {
            for (_, v) in apps.entries() {
                if let Some(id) = v.as_str().and_then(|s| s.parse::<u32>().ok()) {
                    appids.insert(id);
                }
            }
        }
    }
    Ok(appids)
}

// ---------------------------------------------------------------------------
// Putting it together
// ---------------------------------------------------------------------------

pub struct LocalSteam {
    pub steam_id: String,
    pub games: Vec<ImportedGame>,
    pub skipped_non_games: usize,
    /// Whether `games` is the account's whole library. Only the account
    /// signed in last has its licences in the client's cache; another is
    /// read from what it played and installed, so a sync mustn't take a
    /// game it lacks as gone.
    pub whole_library: bool,
}

/// Names like "X - Test Server", "X Public Test", "X Beta", "X Playtest",
/// "X Demo", "X Playable Teaser".
pub fn is_test_app(name: &str) -> bool {
    let n = name.trim().to_lowercase();
    n.contains("test server")
        || n.contains("public test")
        || n.contains("playtest")
        || n.ends_with("beta")
        || n.ends_with("(beta)")
        || n.contains(" beta ")
        || n.ends_with(" teaser")
        || crate::heroic::is_demo_title(name)
}

/// Appids installed in any Steam library folder (libraryfolders.vdf). The Web
/// API does not report install state, so API syncs overlay this.
pub fn installed_appids(steam_dir: &Path) -> HashSet<u32> {
    std::fs::read_to_string(steam_dir.join("config/libraryfolders.vdf")).map(|t| parse_libraryfolders(&t)).unwrap_or_default()
}

fn parse_libraryfolders(text: &str) -> HashSet<u32> {
    library_folders(text).into_iter().flat_map(|(_, apps)| apps).collect()
}

/// Each library folder in libraryfolders.vdf, with its path if it gives one
/// and the appids installed in it.
fn library_folders(text: &str) -> Vec<(Option<PathBuf>, Vec<u32>)> {
    let Ok(vdf) = parse_text_vdf(text) else { return vec![] };
    let folders = vdf.get("libraryfolders").map(Vdf::entries).unwrap_or(&[]);
    folders
        .iter()
        .map(|(_, folder)| {
            let path = folder.get("path").and_then(Vdf::as_str).map(PathBuf::from);
            let apps = folder.get("apps").map(Vdf::entries).unwrap_or(&[]).iter().filter_map(|(id, _)| id.parse().ok()).collect();
            (path, apps)
        })
        .collect()
}

/// Who installed each installed app: the SteamID64 in its app manifest's
/// `LastOwner`, or `None` when the manifest is missing, unreadable or says
/// `0`. A folder without a path is taken to be the Steam folder itself.
fn installed_owners(steam_dir: &Path) -> HashMap<u32, Option<String>> {
    let text = std::fs::read_to_string(steam_dir.join("config/libraryfolders.vdf")).unwrap_or_default();
    let mut owners = HashMap::new();
    for (path, apps) in library_folders(&text) {
        let steamapps = path.as_deref().unwrap_or(steam_dir).join("steamapps");
        for appid in apps {
            let manifest = std::fs::read_to_string(steamapps.join(format!("appmanifest_{appid}.acf"))).ok();
            let owner = manifest
                .as_deref()
                .and_then(|t| parse_text_vdf(t).ok())
                .and_then(|vdf| vdf.path(&["AppState", "LastOwner"]).and_then(Vdf::as_str).filter(|o| *o != "0").map(str::to_string));
            owners.insert(appid, owner);
        }
    }
    owners
}

/// Games hidden in the Steam client, for the account `steam_id` (SteamID64).
/// The current client keeps its Hidden collection in the cloud-storage file
/// (`user-collections.hidden`, whose value is a JSON string with `added` and
/// `removed` appids); older ones mark `"Hidden" "1"` per app in
/// sharedconfig.vdf. Both are read; a file that's missing or unreadable adds
/// nothing, since hiding is a preference, not something a sync needs.
///
/// Where the current client keeps the collection
/// is from memory of its format and waits on a check against a real Steam
/// folder; until then, a folder with neither file hides nothing.
pub fn hidden_appids(steam_dir: &Path, steam_id: &str) -> HashSet<u32> {
    let Some(account_id) = steam_id.parse::<u64>().ok().and_then(|id| id.checked_sub(76561197960265728)) else {
        return HashSet::new();
    };
    let user = steam_dir.join(format!("userdata/{account_id}"));
    let mut hidden = std::fs::read_to_string(user.join("config/cloudstorage/cloud-storage-namespace-1.json"))
        .map(|t| parse_cloud_hidden(&t))
        .unwrap_or_default();
    for shared in ["7/remote/sharedconfig.vdf", "config/sharedconfig.vdf"] {
        if let Ok(t) = std::fs::read_to_string(user.join(shared)) {
            hidden.extend(parse_sharedconfig_hidden(&t));
        }
    }
    hidden
}

/// The Hidden collection in cloud-storage-namespace-1.json: a list of
/// `[key, {key, value, is_deleted, …}]` pairs.
fn parse_cloud_hidden(text: &str) -> HashSet<u32> {
    let Ok(serde_json::Value::Array(entries)) = serde_json::from_str::<serde_json::Value>(text) else { return HashSet::new() };
    let Some(entry) = entries.iter().find_map(|e| {
        let e = e.as_array()?;
        (e.first()?.as_str()? == "user-collections.hidden").then(|| e.get(1)).flatten()
    }) else {
        return HashSet::new();
    };
    if entry.get("is_deleted").and_then(serde_json::Value::as_bool) == Some(true) {
        return HashSet::new();
    }
    let Some(value) =
        entry.get("value").and_then(serde_json::Value::as_str).and_then(|v| serde_json::from_str::<serde_json::Value>(v).ok())
    else {
        return HashSet::new();
    };
    let ids = |k: &str| -> HashSet<u32> {
        value
            .get(k)
            .and_then(serde_json::Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_u64().and_then(|n| u32::try_from(n).ok())).collect())
            .unwrap_or_default()
    };
    &ids("added") - &ids("removed")
}

/// Apps marked hidden in sharedconfig.vdf (older clients).
fn parse_sharedconfig_hidden(text: &str) -> HashSet<u32> {
    let Ok(vdf) = parse_text_vdf(text) else { return HashSet::new() };
    let apps = vdf.path(&["UserRoamingConfigStore", "Software", "Valve", "Steam", "apps"]);
    apps.map(Vdf::entries)
        .unwrap_or(&[])
        .iter()
        .filter(|(_, app)| app.get("Hidden").and_then(Vdf::as_str) == Some("1"))
        .filter_map(|(id, _)| id.parse().ok())
        .collect()
}

/// Minutes played and last-played time per appid, from localconfig.vdf.
fn parse_localconfig(text: &str) -> HashMap<u32, (i64, i64)> {
    let mut playtime = HashMap::new();
    if let Ok(vdf) = parse_text_vdf(text) {
        let apps = vdf.path(&["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"]);
        for (id, app) in apps.map(Vdf::entries).unwrap_or(&[]) {
            let Ok(id) = id.parse::<u32>() else { continue };
            let mins = app.get("Playtime").and_then(Vdf::as_str).and_then(|s| s.parse().ok()).unwrap_or(0);
            let last = app.get("LastPlayed").and_then(Vdf::as_str).and_then(|s| s.parse().ok()).unwrap_or(0);
            playtime.insert(id, (mins, last));
        }
    }
    playtime
}

pub fn read_library(steam_dir: &Path, steam_id: Option<&str>) -> Result<LocalSteam> {
    let steam_id = match steam_id {
        Some(s) => s.to_string(),
        None => crate::steam::detect_steam_id(Some(steam_dir)).context("no Steam login found in loginusers.vdf")?,
    };
    // A SteamID64 is an account id plus this base; a smaller number (a made-up
    // id, or a short account id given by mistake) is no account's.
    let account_id = steam_id
        .parse::<u64>()
        .ok()
        .and_then(|id| id.checked_sub(76561197960265728))
        .with_context(|| format!("{steam_id:?} isn't a SteamID64 (a 17-digit number starting 7656)"))?;

    // packageinfo.vdf caches the licences of the account signed in to the
    // client, whoever asks, so with two accounts on one client the second
    // was given the first's games (B-STORES-8). It is read only for the
    // account loginusers.vdf lists as the most recent, or when it lists
    // none. Another account has what its own localconfig.vdf says it
    // played, and what it installed itself.
    let users = crate::steam::login_users_in(steam_dir);
    let whole_library = users.first().is_none_or(|u| u.steam_id == steam_id);
    let cache = "couldn't read the Steam client's cache";
    let mut owned =
        if whole_library { read_packageinfo(&steam_dir.join("appcache/packageinfo.vdf")).context(cache)? } else { HashSet::new() };
    let info = read_appinfo(&steam_dir.join("appcache/appinfo.vdf")).context(cache)?;

    let localconfig = steam_dir.join(format!("userdata/{account_id}/config/localconfig.vdf"));
    let playtime = std::fs::read_to_string(&localconfig).map(|t| parse_localconfig(&t)).unwrap_or_default();
    let installed = installed_appids(steam_dir);
    if users.len() <= 1 && whole_library {
        // One account: whatever is installed is its own.
        owned.extend(&installed);
    } else {
        // Several: a game counts as this account's if it installed it, and
        // for the most recent one also if no manifest says who did.
        let owners = installed_owners(steam_dir);
        owned.extend(owners.iter().filter(|(_, o)| o.as_deref().map_or(whole_library, |o| o == steam_id)).map(|(id, _)| *id));
    }
    let (games, skipped) = assemble(owned, &info, &playtime, &installed);
    Ok(LocalSteam { steam_id, games, skipped_non_games: skipped, whole_library })
}

/// Games from what the caches say: everything owned (installed games the
/// caller counts as owned included) or played that appinfo calls a game,
/// minus test apps; `installed` marks which are on this computer. Returns
/// the games (sorted by name) and how many apps were skipped as non-games.
fn assemble(
    owned: HashSet<u32>,
    info: &HashMap<u32, AppInfo>,
    playtime: &HashMap<u32, (i64, i64)>,
    installed: &HashSet<u32>,
) -> (Vec<ImportedGame>, usize) {
    let mut all: HashSet<u32> = owned;
    all.extend(playtime.keys().copied());

    let mut games = vec![];
    let mut skipped = 0;
    for appid in all {
        let Some(ai) = info.get(&appid) else { continue };
        if !ai.type_.is_empty() && ai.type_ != "game" {
            skipped += 1;
            continue;
        }
        // Test servers, betas and playtests are typed "game" but are not games you play.
        if appid == 480 || is_test_app(&ai.name) {
            skipped += 1;
            continue;
        }
        let (mins, last) = playtime.get(&appid).copied().unwrap_or((0, 0));
        games.push(ImportedGame {
            source: "steam".into(),
            source_id: appid.to_string(),
            launcher: "steam".into(),
            name: ai.name.clone(),
            appid: Some(appid),
            installed: installed.contains(&appid),
            playtime_minutes: Some(mins),
            playtime_2weeks: 0,
            last_played: last,
            art_cover: None,
            art_square: None,
            description: None,
            genres: vec![],
            developer: None,
            release_date: None,
            store_url: Some(format!("https://store.steampowered.com/app/{appid}")),
            run_url: Some(format!("steam://run/{appid}")),
        });
    }
    games.sort_by(|a, b| a.name.cmp(&b.name));
    (games, skipped)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Two accounts on one Steam client: the first's licences (Portal,
    /// Portal 2) in the package cache, which is the signed-in account's,
    /// each account's own play time (the first played Portal 2, the second
    /// Celeste), and two installed games, Portal 2 by the first and Hades
    /// by the second. `signed_in` is the account loginusers.vdf marks as
    /// the most recent; `licences` are the cache's appids.
    pub(crate) fn two_accounts(root: &Path, signed_in: &str, licences: &[u32]) {
        let write = |rel: &str, bytes: &[u8]| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, bytes).unwrap();
        };
        let games = [(620, "Portal 2"), (400, "Portal"), (504230, "Celeste"), (1145360, "Hades"), (413150, "Stardew Valley")];
        write("appcache/appinfo.vdf", &appinfo(true, games.iter().map(|&(id, name)| (id, app(name, "game"))).collect()));
        write("appcache/packageinfo.vdf", &packageinfo(&[(0, &[]), (1, licences)]));
        let recent = |id: &str| if id == signed_in { "1" } else { "0" };
        let login = format!(
            r#""users" {{ "{FIRST}" {{ "PersonaName" "First" "MostRecent" "{}" }} "{SECOND}" {{ "PersonaName" "Second" "MostRecent" "{}" }} }}"#,
            recent(FIRST),
            recent(SECOND)
        );
        write("config/loginusers.vdf", login.as_bytes());
        let played = |appid: u32, minutes: u32| {
            format!(
                r#""UserLocalConfigStore" {{ "Software" {{ "Valve" {{ "Steam" {{ "apps" {{ "{appid}" {{ "Playtime" "{minutes}" }} }} }} }} }} }}"#
            )
        };
        // SteamID64 76561198000000001 is account 39734273, and ...002 is 39734274.
        write("userdata/39734273/config/localconfig.vdf", played(620, 1260).as_bytes());
        write("userdata/39734274/config/localconfig.vdf", played(504230, 120).as_bytes());
        let library = root.join("library");
        let folders = format!(
            r#""libraryfolders" {{ "0" {{ "path" "{}" "apps" {{ "620" "1" "1145360" "1" }} }} }}"#,
            library.display().to_string().replace('\\', "\\\\")
        );
        write("config/libraryfolders.vdf", folders.as_bytes());
        for (appid, owner) in [(620, FIRST), (1145360, SECOND)] {
            let manifest = format!(r#""AppState" {{ "appid" "{appid}" "LastOwner" "{owner}" }}"#);
            write(&format!("library/steamapps/appmanifest_{appid}.acf"), manifest.as_bytes());
        }
    }

    pub(crate) const FIRST: &str = "76561198000000001";
    pub(crate) const SECOND: &str = "76561198000000002";

    // Minimal writers for Steam's binary KV caches (little-endian).
    enum V {
        Map(Vec<(&'static str, V)>),
        Str(&'static str),
        Int(u32),
        /// A type byte the parser doesn't know, to exercise skipping.
        Bad,
    }

    /// Binary KV body. With `table`, keys are i32 indices into it (appinfo v29).
    fn kv(entries: &[(&'static str, V)], table: Option<&mut Vec<&'static str>>, out: &mut Vec<u8>) {
        let mut table = table;
        for (k, v) in entries {
            let ty: u8 = match v {
                V::Map(_) => 0x00,
                V::Str(_) => 0x01,
                V::Int(_) => 0x02,
                V::Bad => 0x09,
            };
            out.push(ty);
            match table.as_deref_mut() {
                Some(t) => {
                    let idx = t.iter().position(|x| x == k).unwrap_or_else(|| {
                        t.push(k);
                        t.len() - 1
                    });
                    out.extend((idx as i32).to_le_bytes());
                }
                None => out.extend(k.bytes().chain([0])),
            }
            match v {
                V::Map(m) => kv(m, table.as_deref_mut(), out),
                V::Str(s) => out.extend(s.bytes().chain([0])),
                V::Int(n) => out.extend(n.to_le_bytes()),
                V::Bad => return,
            }
        }
        out.push(0x08);
    }

    fn app(name: &'static str, ty: &'static str) -> Vec<(&'static str, V)> {
        vec![("appinfo", V::Map(vec![("common", V::Map(vec![("name", V::Str(name)), ("type", V::Str(ty))]))]))]
    }

    fn appinfo(v29: bool, apps: Vec<(u32, Vec<(&'static str, V)>)>) -> Vec<u8> {
        let mut out = vec![];
        out.extend((if v29 { APPINFO_V29 } else { APPINFO_V28 }).to_le_bytes());
        out.extend(1u32.to_le_bytes()); // universe
        let offset_at = out.len();
        if v29 {
            out.extend(0u64.to_le_bytes()); // string table offset, patched below
        }
        let mut table: Vec<&'static str> = vec![];
        for (appid, entries) in apps {
            let mut body = vec![0u8; 4 + 4 + 8 + 20 + 4 + 20];
            kv(&entries, if v29 { Some(&mut table) } else { None }, &mut body);
            out.extend(appid.to_le_bytes());
            out.extend((body.len() as u32).to_le_bytes());
            out.extend(body);
        }
        out.extend(0u32.to_le_bytes());
        if v29 {
            let offset = out.len() as u64;
            out[offset_at..offset_at + 8].copy_from_slice(&offset.to_le_bytes());
            out.extend((table.len() as u32).to_le_bytes());
            for s in table {
                out.extend(s.bytes().chain([0]));
            }
        }
        out
    }

    fn packageinfo(packages: &[(u32, &[u32])]) -> Vec<u8> {
        let mut out = vec![];
        out.extend(PACKAGEINFO_V28.to_le_bytes());
        out.extend(1u32.to_le_bytes());
        for (id, apps) in packages {
            out.extend(id.to_le_bytes());
            out.extend([0u8; 20 + 4 + 8]);
            let ids: Vec<(&'static str, V)> = apps.iter().enumerate().map(|(i, a)| (["0", "1", "2"][i], V::Int(*a))).collect();
            kv(&[("pkg", V::Map(vec![("appids", V::Map(ids))]))], None, &mut out);
        }
        out.extend(0xFFFF_FFFFu32.to_le_bytes());
        out
    }

    #[test]
    fn appinfo_v29() {
        let mut broken = app("Broken", "game");
        broken.push(("oops", V::Bad));
        let data = appinfo(true, vec![(620, app("Portal 2", "Game")), (7, app("Steam Client", "Config")), (999, broken)]);
        let info = parse_appinfo(&data).unwrap();
        assert_eq!(info[&620].name, "Portal 2");
        assert_eq!(info[&620].type_, "game", "types are lowercased");
        assert_eq!(info[&7].type_, "config");
        assert!(!info.contains_key(&999), "an entry that fails to parse is skipped, not fatal");
    }

    #[test]
    fn appinfo_v28_and_bad_magic() {
        let info = parse_appinfo(&appinfo(false, vec![(400, app("Portal", "game"))])).unwrap();
        assert_eq!(info[&400].name, "Portal");
        let mut data = appinfo(false, vec![]);
        data[0] = 0x99;
        assert!(parse_appinfo(&data).unwrap_err().to_string().contains("unsupported appinfo.vdf version"));
    }

    #[test]
    fn packageinfo_appids() {
        let ids = parse_packageinfo(&packageinfo(&[(0, &[7, 620]), (1, &[400])])).unwrap();
        assert_eq!(ids, HashSet::from([7, 620, 400]));
        assert!(parse_packageinfo(&[0u8; 8]).is_err());
    }

    /// Sizes and nesting come from the file, so a
    /// damaged or crafted cache must fail as an error, never abort the server
    /// by reserving gigabytes or overflowing the stack.
    #[test]
    fn damaged_caches_are_errors() {
        // A 20-byte appinfo whose string table claims 0xFFFFFFFF strings.
        let mut huge = vec![];
        huge.extend(APPINFO_V29.to_le_bytes());
        huge.extend(1u32.to_le_bytes());
        huge.extend(16u64.to_le_bytes());
        huge.extend(0xFFFF_FFFFu32.to_le_bytes());
        assert!(parse_appinfo(&huge).unwrap_err().to_string().contains("claims 4294967295 strings"));
        // A string table offset past the end, and one that would overflow.
        for offset in [1_000u64, u64::MAX] {
            huge[8..16].copy_from_slice(&offset.to_le_bytes());
            assert!(parse_appinfo(&huge).is_err(), "offset {offset}");
        }

        // A count the bytes could hold but no real table has: a file of
        // NULs, one more than the cap, fails before a string is read.
        let mut nuls = huge[..16].to_vec();
        nuls.extend(((MAX_STRINGS + 1) as u32).to_le_bytes());
        nuls.resize(nuls.len() + MAX_STRINGS + 1, 0);
        nuls[8..16].copy_from_slice(&16u64.to_le_bytes());
        assert!(parse_appinfo(&nuls).unwrap_err().to_string().contains("the most expected is"));

        // Truncated anywhere: an error, never a panic.
        let good = appinfo(true, vec![(620, app("Portal 2", "game"))]);
        for n in 0..good.len() {
            let _ = parse_appinfo(&good[..n]);
        }
        assert!(parse_appinfo(&good[..good.len() - 3]).is_err());
        let pkgs = packageinfo(&[(0, &[620])]);
        for n in 0..pkgs.len() {
            assert!(parse_packageinfo(&pkgs[..n]).is_err(), "packageinfo cut at {n}");
        }

        // An entry's size that points past the end of the file.
        let mut long = good.clone();
        long[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_appinfo(&long).is_err());

        // Nesting far deeper than the stack could take.
        let deep = 200_000;
        let mut body = vec![];
        for _ in 0..deep {
            body.extend([0x00, b'k', 0]);
        }
        assert!(parse_binary_kv(&mut Cursor { data: &body, pos: 0 }, None, 0).unwrap_err().to_string().contains("nested"));
        let mut pkg = vec![];
        pkg.extend(PACKAGEINFO_V28.to_le_bytes());
        pkg.extend(1u32.to_le_bytes());
        pkg.extend(0u32.to_le_bytes());
        pkg.extend([0u8; 20 + 4 + 8]);
        pkg.extend(&body);
        assert!(parse_packageinfo(&pkg).unwrap_err().to_string().contains("nested"));
        let text = "\"k\" {".repeat(deep);
        assert!(parse_text_vdf(&text).unwrap_err().to_string().contains("nested"));
        assert!(parse_localconfig(&text).is_empty());
        // Steam's real depth still parses.
        let ok = format!("{}{}", "\"k\" {".repeat(MAX_DEPTH), "}".repeat(MAX_DEPTH));
        assert!(parse_text_vdf(&ok).is_ok());
    }

    #[test]
    fn text_files() {
        let local = r#""UserLocalConfigStore" { "Software" { "Valve" { "Steam" { "apps" {
            "620" { "Playtime" "1260" "LastPlayed" "1790000000" }
            "400" { "LastPlayed" "1700000000" }
            "junk" { "Playtime" "5" }
        } } } } }"#;
        let p = parse_localconfig(local);
        assert_eq!(p[&620], (1260, 1790000000));
        assert_eq!(p[&400], (0, 1700000000));
        assert_eq!(p.len(), 2);

        let folders = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"/home/you/.steam\"\n\t\t\"apps\"\n\t\t{\n\t\t\t\"620\"\t\t\"123\"\n\t\t}\n\t}\n\t\"1\"\n\t{\n\t\t// a comment\n\t\t\"apps\"\n\t\t{\n\t\t\t\"400\"\t\t\"456\"\n\t\t}\n\t}\n}\n";
        assert_eq!(parse_libraryfolders(folders), HashSet::from([620, 400]));
        assert!(parse_libraryfolders("not { vdf").is_empty());
    }

    /// Both places the client keeps hidden games.
    #[test]
    fn reads_the_games_hidden_in_steam() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("userdata/39734272");
        std::fs::create_dir_all(user.join("config/cloudstorage")).unwrap();
        std::fs::create_dir_all(user.join("7/remote")).unwrap();
        let collection = serde_json::json!({"id": "hidden", "name": "", "added": [620, 413150, 70], "removed": [70]}).to_string();
        let cloud = serde_json::json!([
            ["user-collections.favorite", {"key": "user-collections.favorite", "timestamp": 1, "value": "{\"added\":[440]}"}],
            ["user-collections.hidden", {"key": "user-collections.hidden", "timestamp": 2, "value": collection, "version": "3"}]
        ]);
        std::fs::write(user.join("config/cloudstorage/cloud-storage-namespace-1.json"), cloud.to_string()).unwrap();
        std::fs::write(
            user.join("7/remote/sharedconfig.vdf"),
            r#""UserRoamingConfigStore" { "Software" { "Valve" { "Steam" { "apps" {
                "550" { "Hidden" "1" } "730" { "Hidden" "0" } "10" { "tags" { "0" "favorite" } }
            } } } } }"#,
        )
        .unwrap();
        let mut hidden: Vec<u32> = hidden_appids(dir.path(), "76561198000000000").into_iter().collect();
        hidden.sort();
        assert_eq!(hidden, [550, 620, 413150], "added less removed, plus sharedconfig's");
        assert!(hidden_appids(dir.path(), "76561198000000001").is_empty(), "another account's folder");
        assert!(hidden_appids(dir.path(), "not an id").is_empty());
        assert!(parse_cloud_hidden("not json").is_empty());
        let deleted =
            r#"[["user-collections.hidden", {"key": "user-collections.hidden", "is_deleted": true, "value": "{\"added\":[1]}"}]]"#;
        assert!(parse_cloud_hidden(deleted).is_empty(), "a deleted collection hides nothing");
    }

    #[test]
    fn assemble_filters_non_games() {
        let info: HashMap<u32, AppInfo> = [
            (620, "Portal 2", "game"),
            (400, "Portal", "game"),
            (7, "Steam Client", "config"),
            (480, "Spacewar", "game"),
            (1000, "Some Game Demo", "game"),
            (1001, "Some Game Playtest", "game"),
        ]
        .into_iter()
        .map(|(id, n, t)| (id, AppInfo { name: n.into(), type_: t.into() }))
        .collect();
        let owned = HashSet::from([620, 7, 480, 999, 1001]);
        let playtime = HashMap::from([(400, (30, 1)), (1000, (5, 0))]);
        let installed = HashSet::from([620, 1001]);
        let (games, skipped) = assemble(owned, &info, &playtime, &installed);
        let names: Vec<&str> = games.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["Portal", "Portal 2"], "owned, played or installed games, sorted");
        assert_eq!(skipped, 4, "config app, Spacewar, a demo and a playtest");
        let p2 = &games[1];
        assert!(p2.installed && p2.appid == Some(620) && p2.playtime_minutes == Some(0));
        assert_eq!(games[0].playtime_minutes, Some(30));
    }

    #[test]
    fn read_library_from_a_steam_folder() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let write = |rel: &str, bytes: &[u8]| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, bytes).unwrap();
        };
        write("appcache/appinfo.vdf", &appinfo(true, vec![(620, app("Portal 2", "game")), (400, app("Portal", "game"))]));
        write("appcache/packageinfo.vdf", &packageinfo(&[(0, &[620])]));
        // SteamID64 76561198000000002 is account 39734274.
        write(
            "userdata/39734274/config/localconfig.vdf",
            br#""UserLocalConfigStore" { "Software" { "Valve" { "Steam" { "apps" { "400" { "Playtime" "90" } } } } } }"#,
        );
        write("config/libraryfolders.vdf", br#""libraryfolders" { "0" { "apps" { "620" "1" } } }"#);
        let lib = read_library(root, Some("76561198000000002")).unwrap();
        assert_eq!(lib.steam_id, "76561198000000002");
        let got: Vec<(&str, bool, Option<i64>)> = lib.games.iter().map(|g| (g.name.as_str(), g.installed, g.playtime_minutes)).collect();
        assert_eq!(got, [("Portal", false, Some(90)), ("Portal 2", true, Some(0))]);
    }

    /// B-STORES-8: with two accounts on one Steam client, each is read as
    /// its own. The signed-in one has the package cache's games; the other
    /// has what it played and installed, and is marked as a part of its
    /// library. Neither gets a game the other installed.
    #[test]
    fn each_account_on_one_steam_client_gets_its_own_games() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        two_accounts(root, FIRST, &[400, 620]);
        let names = |lib: &LocalSteam| -> Vec<(String, bool)> { lib.games.iter().map(|g| (g.name.clone(), g.installed)).collect() };
        let pair = |n: &str, installed: bool| (n.to_string(), installed);

        let first = read_library(root, Some(FIRST)).unwrap();
        assert!(first.whole_library);
        assert_eq!(names(&first), [pair("Portal", false), pair("Portal 2", true)], "not Hades, which the second installed");
        assert_eq!(first.games[1].playtime_minutes, Some(1260));
        // With no account named, the most recent is read, as before.
        assert_eq!(read_library(root, None).unwrap().steam_id, FIRST);

        let second = read_library(root, Some(SECOND)).unwrap();
        assert!(!second.whole_library);
        assert_eq!(names(&second), [pair("Celeste", false), pair("Hades", true)], "none of the first's licences");
        assert_eq!(second.games[0].playtime_minutes, Some(120));

        // Signed in as the second, the cache holds its licences instead.
        two_accounts(root, SECOND, &[413150, 504230]);
        let second = read_library(root, Some(SECOND)).unwrap();
        assert!(second.whole_library);
        assert_eq!(names(&second), [pair("Celeste", false), pair("Hades", true), pair("Stardew Valley", false)]);
        let first = read_library(root, Some(FIRST)).unwrap();
        assert!(!first.whole_library);
        assert_eq!(names(&first), [pair("Portal 2", true)]);

        // An account this client doesn't list has nothing here.
        assert!(read_library(root, Some("76561198000000003")).unwrap().games.is_empty());
    }

    /// One account on the client keeps every installed game, whoever its
    /// manifest names (a game lent through Family Sharing, say).
    #[test]
    fn one_account_keeps_every_installed_game() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        two_accounts(root, FIRST, &[400]);
        std::fs::write(root.join("config/loginusers.vdf"), format!(r#""users" {{ "{FIRST}" {{ "MostRecent" "0" }} }}"#)).unwrap();
        let lib = read_library(root, Some(FIRST)).unwrap();
        assert!(lib.whole_library);
        let names: Vec<&str> = lib.games.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["Hades", "Portal", "Portal 2"]);
    }

    /// A number smaller than a SteamID64's base, as tests and typos give,
    /// is an error rather than an overflow.
    #[test]
    fn a_short_steam_id_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        for id in ["12345", "0", "76561197960265727"] {
            let e = read_library(dir.path(), Some(id)).err().unwrap_or_else(|| panic!("{id} read"));
            assert!(e.to_string().contains("isn't a SteamID64"), "{id}: {e:#}");
        }
        assert!(hidden_appids(dir.path(), "12345").is_empty());
    }

    #[test]
    fn the_steam_dir_setting_comes_first() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(steam_dir_candidates(Some(dir.path()))[0], dir.path());
        // Tests look nowhere else, so with no app cache or login it's no Steam.
        assert_eq!(find_steam_dir(Some(dir.path())), None, "no app cache or login there");
        for rel in ["appcache/appinfo.vdf", "config/loginusers.vdf"] {
            let p = dir.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"").unwrap();
        }
        assert_eq!(find_steam_dir(Some(dir.path())).as_deref(), Some(dir.path()), "the setting wins over any other install");
    }

    /// Steam from the Snap Store keeps its folder under the snap's own home.
    #[test]
    fn the_snap_steam_is_found() {
        let home = tempfile::tempdir().unwrap();
        let snap = home.path().join("snap/steam/common/.local/share/Steam");
        assert_eq!(first_steam_dir(home_steam_dirs(home.path())), None);
        for rel in ["appcache/appinfo.vdf", "config/loginusers.vdf"] {
            let p = snap.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"").unwrap();
        }
        assert_eq!(first_steam_dir(home_steam_dirs(home.path())), Some(snap.clone()));
        // A native install on the same machine comes first.
        let native = home.path().join(".local/share/Steam");
        for rel in ["appcache/appinfo.vdf", "config/loginusers.vdf"] {
            let p = native.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"").unwrap();
        }
        assert_eq!(first_steam_dir(home_steam_dirs(home.path())), Some(native));
    }

    #[test]
    fn registry_paths() {
        let sep = std::path::MAIN_SEPARATOR_STR;
        assert_eq!(registry_path(" d:/Games/Steam "), Some(PathBuf::from(format!("d:{sep}Games{sep}Steam"))));
        assert_eq!(registry_path(""), None);
    }

    /// Steam's folder from the registry, in a key of the test's own (the
    /// real `HKCU\Software\Valve\Steam` is left alone).
    #[cfg(windows)]
    #[test]
    fn the_steam_folder_from_the_registry() {
        use winreg::HKCU;
        let key = format!(r"Software\UpNext-test-{}\Valve\Steam", uuid::Uuid::new_v4().simple());
        let (k, _) = HKCU.create_subkey(&key).unwrap();
        k.set_value("SteamPath", &"d:/Games/Steam").unwrap();
        let found = registry_dir(HKCU, &key, "SteamPath");
        assert!(registry_dir(HKCU, &key, "Missing").is_none());
        HKCU.delete_subkey_all(key.split_once(r"\Valve").unwrap().0).unwrap();
        assert_eq!(found, Some(PathBuf::from(r"d:\Games\Steam")));
    }
}
