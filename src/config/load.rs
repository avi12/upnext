//! Loading the settings: the built-in defaults, in the platform's folders
//! ([`Dirs`]), then `.env` files and the environment, then the app's
//! settings from the database as a player reads them.

use super::{BadSetting, Config, FLAG_SPELLINGS, SETTINGS, Source, check, parse_flag};
use crate::db::PlayerId;
use anyhow::{Context, Result};
use std::path::PathBuf;

/// The per-user folders UpNext keeps its files in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    pub data: PathBuf,
    pub config: PathBuf,
    pub cache: PathBuf,
}

impl Dirs {
    /// The platform's folders for UpNext (`directories`; on Linux the XDG
    /// ones: `~/.local/share/upnext`, `~/.config/upnext`, `~/.cache/upnext`).
    /// On Windows all three are under `%LOCALAPPDATA%\upnext` (`data`,
    /// `config`, `cache`): the local folders, not the roaming ones, so a
    /// roaming profile doesn't copy the database from PC to PC. Elsewhere the
    /// local folders are the usual ones.
    /// Without a home folder, `data`, `.` and `cache` under the current one.
    pub fn standard() -> Self {
        match directories::ProjectDirs::from("", "", "upnext") {
            Some(p) => Self { data: p.data_local_dir().into(), config: p.config_local_dir().into(), cache: p.cache_dir().into() },
            None => Self { data: "data".into(), config: ".".into(), cache: "cache".into() },
        }
    }
}

/// Where loading reads the variables from, and adds a `.env` file's to.
enum Env {
    /// The process's environment. A `.env` file's variables are added to it,
    /// so the code that reads one later (`UPNEXT_PLAYER`, `CODEX_HOME`) and
    /// the programs UpNext starts see them too.
    Process,
    /// A map of its own, for tests. Every test running at the same time
    /// shares the process's environment, so a test that added its `.env`
    /// file's variables to it changed what the others read.
    #[cfg(test)]
    Vars(std::collections::HashMap<String, String>),
}

impl Env {
    /// The variable, trimmed; None when it's unset or blank.
    fn get(&self, key: &str) -> Option<String> {
        let value = match self {
            Self::Process => std::env::var(key).ok(),
            #[cfg(test)]
            Self::Vars(vars) => vars.get(key).cloned(),
        };
        value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
    }

    fn is_set(&self, key: &str) -> bool {
        match self {
            Self::Process => std::env::var_os(key).is_some(),
            #[cfg(test)]
            Self::Vars(vars) => vars.contains_key(key),
        }
    }

    /// Add the variables of the `.env` file at `path` that aren't set yet:
    /// `new`, which the caller read from it.
    fn add_file(&mut self, path: &std::path::Path, new: Vec<(String, String)>) -> Result<()> {
        match self {
            // dotenvy adds them itself, as it always has: the same ones.
            Self::Process => {
                let _ = new;
                dotenvy::from_path(path).with_context(|| format!("reading {}", path.display()))?;
            }
            #[cfg(test)]
            Self::Vars(vars) => vars.extend(new),
        }
        Ok(())
    }
}

/// The user name from the environment (`USER`, `LOGNAME`, or Windows'
/// `USERNAME`) as a player's name: one in lower case gets a capital, and
/// one nobody would call themselves is left out (root, the service and
/// system accounts, Windows' `NAME$` machine accounts, and the shared
/// logins that devices and images come with, such as the Steam Deck's
/// `deck`). None in a container, whose user is the image's.
fn login_name(container: bool, var: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    const NOT_A_NAME: [&str; 17] = [
        "root",
        "nobody",
        "daemon",
        "system",
        "local service",
        "network service",
        "upnext",
        "deck",
        "steam",
        "gamer",
        "pi",
        "ubuntu",
        "user",
        "admin",
        "administrator",
        "guest",
        "default",
    ];
    if container {
        return None;
    }
    let name = crate::db::clean_name(&var("USER").or_else(|| var("LOGNAME")).or_else(|| var("USERNAME"))?);
    let lower = name.to_lowercase();
    if name.is_empty() || name.chars().count() > 40 || lower.ends_with('$') || NOT_A_NAME.contains(&lower.as_str()) {
        return None;
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if name.chars().all(|c| c.is_lowercase()) => Some(first.to_uppercase().chain(chars).collect()),
        _ => Some(name),
    }
}

/// On Windows, GOG Galaxy keeps its database in a well-known place.
fn default_galaxy_db() -> Option<PathBuf> {
    if cfg!(windows) {
        let base = std::env::var_os("PROGRAMDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
        let p = base.join("GOG.com").join("Galaxy").join("storage").join("galaxy-2.0.db");
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

impl Config {
    /// Every setting at its built-in default, with the paths in the
    /// platform's standard folders. Reads no file, and no environment
    /// variable but `HOME` and the XDG ones; tests start from this and move the
    /// paths into a temporary folder (`testutil::offline_config`).
    pub fn built_in() -> Self {
        Self::built_in_at(Dirs::standard())
    }

    fn built_in_at(dirs: Dirs) -> Self {
        let mut c = Self::default();
        for s in SETTINGS.iter().filter(|s| !s.default.is_empty()) {
            s.set(&mut c, s.default).expect("built-in defaults parse");
        }
        c.data_dir = dirs.data;
        c.config_dir = dirs.config;
        c.cache_dir = dirs.cache;
        c.db_path = c.data_dir.join("upnext.db");
        c.gog_token_path = c.db_dir().join("gog_token.json");
        c
    }

    /// The built-in defaults, then `.env` and the environment. The `.env`
    /// read is the one in the config dir; debug builds read `./.env` first
    /// (it wins), so a checkout can keep its own. An installed UpNext never
    /// reads the `.env` of whatever folder it was started from.
    pub fn load() -> Result<Self> {
        let mut dirs = Dirs::standard();
        if let Some(d) = Env::Process.get("UPNEXT_CONFIG_DIR") {
            dirs.config = d.into();
        }
        let mut files = Vec::new();
        if cfg!(debug_assertions) {
            files.push(PathBuf::from(".env"));
        }
        files.push(dirs.config.join(".env"));
        Self::load_from(dirs, &files, &mut Env::Process)
    }

    /// A flag only the environment sets (`UPNEXT_CONTAINER`), read as the
    /// settings' flags are: unset is off, and a value [`parse_flag`] can't
    /// read stops the start, saying where it came from.
    fn env_flag(&self, env: &Env, var: &'static str) -> Result<bool> {
        let Some(v) = env.get(var) else { return Ok(false) };
        parse_flag(&v).with_context(|| format!("must be {FLAG_SPELLINGS}")).with_context(|| BadSetting {
            var,
            value: v.clone(),
            file: self.env_files.get(var).cloned(),
        })
    }

    /// [`Config::load`] with these folders, `.env` files and environment.
    fn load_from(dirs: Dirs, env_files: &[PathBuf], env: &mut Env) -> Result<Self> {
        let mut c = Self::built_in_at(dirs);
        for f in env_files {
            c.read_env_file(env, f)?;
        }
        for s in SETTINGS {
            if let Some(var) = s.env
                && let Some(v) = env.get(var)
                && s.set(&mut c, &v).with_context(|| BadSetting { var, value: v.clone(), file: c.env_files.get(var).cloned() })?
            {
                c.sources.insert(s.key, Source::Env);
            }
        }
        // The environment's verdicts service is held to the app setting's
        // rule: https, or plain http to this computer only.
        if let Some(u) = &c.verdicts_url {
            c.verdicts_url = Some(check::verdicts_url("verdicts_url", u).context("UPNEXT_VERDICTS_URL")?);
        }
        c.container = c.env_flag(env, "UPNEXT_CONTAINER")?;
        c.install_claude = c.env_flag(env, "UPNEXT_INSTALL_CLAUDE")?;
        if env.get("UPNEXT_MODE").is_some_and(|v| v == "service") {
            c.run_mode = "service".into();
        }
        c.login_name = login_name(c.container, &|k: &str| env.get(k));
        // Worked out when the environment leaves them unset.
        if !c.sources.contains_key("db_path") {
            c.db_path = c.data_dir.join("upnext.db");
        }
        if c.steam_id.is_none()
            && let Some(id) = crate::steam::detect_steam_id(c.steam_dir.as_deref())
        {
            c.steam_id = Some(id);
            c.sources.insert("steam_id", Source::SteamClient);
        }
        if !c.sources.contains_key("gog_token_path") {
            c.gog_token_path = c.db_dir().join("gog_token.json");
        }
        if c.galaxy_db.is_none() {
            c.galaxy_db = default_galaxy_db();
        }
        Ok(c)
    }

    /// Put a `.env` file's variables into the environment, the ones it
    /// doesn't already have, and remember which came from it. A missing file
    /// is fine; one that doesn't parse is an error, with its path.
    fn read_env_file(&mut self, env: &mut Env, path: &std::path::Path) -> Result<()> {
        if !path.is_file() {
            return Ok(());
        }
        let shown = || path.display().to_string();
        let mut new = Vec::new();
        for item in dotenvy::from_path_iter(path).with_context(|| format!("reading {}", shown()))? {
            let (key, value) = item.with_context(|| format!("reading {}", shown()))?;
            if !env.is_set(&key) && !new.iter().any(|(k, _)| *k == key) {
                self.env_files.insert(key.clone(), path.to_path_buf());
                new.push((key, value));
            }
        }
        env.add_file(path, new)?;
        self.env_files_read.push(path.to_path_buf());
        Ok(())
    }

    /// Overlay app settings stored in the database (they win over `.env`),
    /// as the default player reads them. A stored value that doesn't parse
    /// is skipped.
    pub fn with_db(self, db: &crate::db::Db) -> Result<Self> {
        let p = db.default_player()?;
        self.with_db_for(db, p)
    }

    /// Overlay app settings as player `p` reads them: the household's,
    /// their own, their connections' and their AI account's.
    pub fn with_db_for(self, db: &crate::db::Db, p: PlayerId) -> Result<Self> {
        let view = db.settings_view(p)?;
        self.overlay(db, p, Some(p), view)
    }

    /// The settings the library's own work runs with (summaries, review
    /// tags, series guides, hidden gems, enrich and sync): player 1's, with
    /// the library's AI account in place of theirs.
    pub fn for_library(self, db: &crate::db::Db) -> Result<Self> {
        let view = db.library_settings_view()?;
        self.overlay(db, PlayerId::FIRST, None, view)
    }

    fn overlay(mut self, db: &crate::db::Db, reader: PlayerId, player: Option<PlayerId>, view: crate::db::SettingsView) -> Result<Self> {
        if let Some(before) = self.before_db.take() {
            self = (*before).clone();
        }
        self.before_db = Some(std::sync::Arc::new(self.clone()));
        // The statuses' household rules are the environment's and the
        // built-in ones; each player's own are laid over them as their
        // library is read.
        db.set_status_rules(self.status_rules());
        // This computer's folders, signed-in Steam user and Galaxy database
        // are player 1's first connections'.
        if reader != PlayerId::FIRST {
            for (key, _) in crate::db::CONNECTION_KEYS {
                self.unset(key);
            }
        }
        // The environment's AI keys and models, and the CLIs signed in on
        // this computer, are the first AI account's. Any other account has
        // only what is set on it.
        if !view.machine_ai {
            for key in crate::db::AI_KEYS.iter().chain(crate::db::AI_SECRET_KEYS).chain(&["llm_backend", "claude_oauth_token"]) {
                self.unset(key);
            }
            self.codex_home = view.ai_account.map(|id| self.data_dir.join("codex").join(id.to_string()));
        }
        for s in SETTINGS.iter().filter(|s| s.in_app) {
            // A verdicts service saved before the https rule is held to it:
            // skipped, so the environment's value or the built-in one applies.
            if s.key == "verdicts_url"
                && let Some(v) = view.stored.get(s.key)
                && let Err(e) = check::verdicts_url(s.key, v)
            {
                tracing::warn!("ignoring the saved verdicts service {v:?}: {e}");
                continue;
            }
            if let Some(v) = view.stored.get(s.key)
                && s.set(&mut self, v).unwrap_or(false)
            {
                self.sources.insert(s.key, Source::App);
            }
        }
        self.player = player;
        self.ai_account = view.ai_account;
        self.no_ai_account = view.ai_account.is_none();
        Ok(self)
    }

    /// These settings as player `p` has them, from the same environment.
    pub fn as_player(&self, db: &crate::db::Db, p: PlayerId) -> Result<Self> {
        self.clone().with_db_for(db, p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_login_name_is_offered_only_when_it_is_a_persons() {
        let from = |pairs: &[(&str, &str)]| {
            let vars: std::collections::HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
            login_name(false, &|k: &str| vars.get(k).cloned())
        };
        assert_eq!(from(&[("USER", "sam")]), Some("Sam".into()));
        assert_eq!(from(&[("LOGNAME", "alex")]), Some("Alex".into()));
        // Windows names keep their own capitals, and only letters get one.
        assert_eq!(from(&[("USERNAME", "KitB")]), Some("KitB".into()));
        assert_eq!(from(&[("USER", "kit.b")]), Some("kit.b".into()));
        // Shared logins, service and machine accounts are nobody's name.
        let shared = ["root", "deck", "steam", "gamer", "pi", "ubuntu", "user", "admin", "Administrator", "guest", "Guest"];
        for nobody in shared.into_iter().chain(["DEN$", "SYSTEM", "LOCAL SERVICE", "NETWORK SERVICE", "upnext", "nobody", " "]) {
            assert_eq!(from(&[("USER", nobody)]), None, "{nobody}");
            assert_eq!(from(&[("USERNAME", nobody)]), None, "{nobody}");
        }
        assert_eq!(from(&[]), None);
        // In a container the user is the image's, whatever it's called.
        assert_eq!(login_name(true, &|k: &str| (k == "USER").then(|| "sam".to_string())), None);
    }

    #[test]
    fn built_in_defaults() {
        let c = Config::built_in();
        assert_eq!((c.model.as_deref(), c.digest_model.as_deref(), c.llm_backend.as_str()), (Some("sonnet"), Some("sonnet"), "claude-cli"));
        assert_eq!((c.sampled_minutes, c.max_candidates, c.port, c.host.as_str()), (120, 120, 8787, "127.0.0.1"));
        assert_eq!((c.linux_mode.as_str(), c.theme.as_str(), c.theme_follow_system, c.auto_sync), ("auto", "arcade", false, true));
        // Other players' verdicts: received once chosen, read by the AI by
        // default after the A/B on the mock library, from the project's service.
        assert_eq!((c.verdicts_receive, c.verdicts_prompt), (false, true));
        assert_eq!((c.verdicts_url.as_deref(), c.source("verdicts_url")), (Some(crate::verdicts::DEFAULT_URL), "default"));
        // Windows' tray: closing keeps it running; starting at sign-in is chosen.
        assert_eq!((c.keep_running, c.start_with_windows), (true, false));
        assert_eq!(c.db_path, c.data_dir.join("upnext.db"));
        assert_eq!(c.gog_token_path, c.data_dir.join("gog_token.json"), "next to the database");
        assert_eq!(c.static_dir, None, "the UI built into the binary");
        assert_eq!((c.steam_api_key.as_deref(), c.steam_id.as_deref(), c.galaxy_db.as_deref()), (None, None, None));
        assert_eq!(c.status_rules(), crate::lifecycle::StatusRules::default(), "the statuses' defaults are these settings'");
        assert_eq!((c.source("model"), c.source("steam_api_key")), ("default", "none"));
    }

    #[test]
    fn app_values_win_and_say_so() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = Config::built_in();
        c.db_path = dir.path().join("upnext.db");
        let db = crate::db::Db::open(&c.db_path, 120).unwrap();
        for (k, v) in [
            ("model", "default"),
            ("sampled_minutes", "45"),
            ("auto_sync", "false"),
            ("galaxy_db", "/g/galaxy-2.0.db"),
            ("linux_mode", "off"),
            ("staple_hours", "50"),
        ] {
            db.setting_set(PlayerId::FIRST, k, Some(v)).unwrap();
        }
        let c = c.with_db(&db).unwrap();
        assert_eq!((c.model.as_deref(), c.sampled_minutes, c.auto_sync, c.linux_mode.as_str()), (None, 45, false, "off"));
        assert_eq!(c.galaxy_db, Some(PathBuf::from("/g/galaxy-2.0.db")));
        assert_eq!((c.source("model"), c.source("sampled_minutes"), c.source("max_candidates")), ("app", "app", "default"));
        assert_eq!(
            (c.status_rules().sampled_minutes, c.status_rules().staple_hours),
            (45, 50),
            "the player's statuses follow their setting"
        );
        // A player's own rules are laid over the household's as their
        // library is read; the household's stay the built-in ones.
        assert_eq!(db.sampled_minutes(), Config::built_in().sampled_minutes);
    }

    /// The config dir's `.env` can move the data dir, and the database and
    /// GOG token follow it. `config list` and `doctor` name the file.
    #[test]
    fn the_config_dirs_env_file_moves_the_data() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("elsewhere");
        let file = dir.path().join(".env");
        // Quoted, as a Windows path must be: a backslash outside quotes is an escape.
        std::fs::write(&file, format!("UPNEXT_DATA_DIR='{}'\n", data.display())).unwrap();
        let dirs = Dirs { data: dir.path().join("share"), config: dir.path().into(), cache: dir.path().join("cache") };
        let mut env = Env::Vars(Default::default());
        let c = Config::load_from(dirs, &[dir.path().join("no/.env"), file.clone()], &mut env).unwrap();
        assert_eq!((c.db_path.clone(), c.gog_token_path.clone()), (data.join("upnext.db"), data.join("gog_token.json")));
        assert_eq!(c.data_dir, data);
        assert_eq!(c.origin("data_dir"), file.display().to_string());
        assert_eq!((c.origin("db_path"), c.origin("cache_dir")), ("default".to_string(), "default".to_string()));
        assert_eq!(c.env_files_read, [file]);
    }

    /// The environment wins over the `.env` files, and the first file over
    /// the later ones. A test's files go into a map of its own, never into
    /// the process's environment, which every test running at the same time
    /// reads: two tests that each put their file there raced.
    #[test]
    fn the_environment_wins_over_env_files_and_the_first_file_wins() {
        let dir = tempfile::tempdir().unwrap();
        let dirs = Dirs { data: dir.path().join("share"), config: dir.path().into(), cache: dir.path().join("cache") };
        let (first, second) = (dir.path().join("first.env"), dir.path().join("second.env"));
        std::fs::write(&first, "UPNEXT_PORT=8790\nUPNEXT_HOST=0.0.0.0\n").unwrap();
        std::fs::write(&second, "UPNEXT_PORT=8791\nUPNEXT_HOST=::\nUPNEXT_MAX_CANDIDATES=60\nUPNEXT_LOAD_TEST_ONLY=1\n").unwrap();
        let mut env = Env::Vars([("UPNEXT_HOST".to_string(), "127.0.0.2".to_string())].into());
        let c = Config::load_from(dirs, &[first.clone(), second.clone()], &mut env).unwrap();
        assert_eq!((c.host.as_str(), c.port, c.max_candidates), ("127.0.0.2", 8790, 60));
        assert_eq!((c.origin("host"), c.origin("port")), ("env UPNEXT_HOST".to_string(), first.display().to_string()));
        assert_eq!(c.origin("max_candidates"), second.display().to_string());
        assert_eq!(env.get("UPNEXT_LOAD_TEST_ONLY").as_deref(), Some("1"), "the files' other variables are added too");
        assert_eq!(std::env::var_os("UPNEXT_LOAD_TEST_ONLY"), None, "but not to the process's environment");
    }

    /// A second copy started with its own `UPNEXT_DATA_DIR` keeps its
    /// webview data there, not in the installed copy's cache folder (Windows
    /// retest); `UPNEXT_CACHE_DIR` still wins, and with neither set nothing
    /// moves.
    #[test]
    fn a_moved_data_folder_takes_the_webview_data_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let dirs = || Dirs { data: dir.path().join("share"), config: dir.path().into(), cache: dir.path().join("cache") };
        let (data, cache) = (dir.path().join("scratch"), dir.path().join("scratch-cache"));
        let file = dir.path().join(".env");
        let load = |text: String| {
            std::fs::write(&file, text).unwrap();
            Config::load_from(dirs(), std::slice::from_ref(&file), &mut Env::Vars(Default::default())).unwrap()
        };
        assert_eq!(load(String::new()).webview_cache(), dir.path().join("cache"));
        assert_eq!(load(format!("UPNEXT_DATA_DIR='{}'\n", data.display())).webview_cache(), data.join("cache"));
        let both = format!("UPNEXT_DATA_DIR='{}'\nUPNEXT_CACHE_DIR='{}'\n", data.display(), cache.display());
        assert_eq!(load(both).webview_cache(), cache);
    }

    /// On Windows everything is in the local app data folder, which a
    /// roaming profile doesn't copy between PCs.
    #[cfg(windows)]
    #[test]
    fn windows_keeps_its_files_in_local_app_data() {
        let local = PathBuf::from(std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA is set")).join("upnext");
        let d = Dirs::standard();
        assert_eq!((d.data, d.config, d.cache), (local.join("data"), local.join("config"), local.join("cache")));
    }
}
