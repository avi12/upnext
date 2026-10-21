//! Runtime configuration. Every setting is one row in [`SETTINGS`]: its key,
//! the environment variable (or `.env` line) that sets it, its default,
//! whether the app can change it (stored in the database, where it wins over
//! the environment), whether it is secret, and the `Config` field it fills.
//! Loading, the in-app overlay, validation, `config list` and the export all
//! walk that table. The table and the lookups that walk it are in
//! `settings`, the checks for a value typed in the app in `check`, loading
//! and the overlay in `load`, the `public_url` setting and local addresses
//! in `address`, and finding the AI CLIs in `programs`.
//!
//! Paths follow the platform's per-user folders ([`Dirs`]): on Linux the
//! database and backups live in `~/.local/share/upnext`, `.env` is
//! read from `~/.config/upnext`, and `~/.cache/upnext` is the AI CLI's
//! working directory. An installed UpNext works the same from any folder.

mod address;
mod check;
mod load;
mod programs;
mod settings;

pub use address::{PublicUrl, is_local_server};
pub use programs::{Found, find_program};
pub use settings::{SETTINGS, SIGN_IN_KEYS, in_app_setting, path_keys, secret_keys, validate_connection_setting, validate_setting};

use crate::db::PlayerId;
use anyhow::{Result, bail};
use load::Dirs;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Build with [`Config::load`] (built-in defaults, then `.env` and the
/// environment) and [`Config::with_db`] (then the app's settings), or
/// [`Config::built_in`] for defaults alone. Its Debug shows a secret
/// setting only as set or not.
#[derive(Clone, Default)]
pub struct Config {
    pub steam_api_key: Option<String>,
    pub steam_id: Option<String>,
    pub db_path: PathBuf,
    /// Model alias passed to `claude -p --model`. `None` means the CLI default.
    pub model: Option<String>,
    /// Games with fewer minutes than this are "sampled"; at/above are "played".
    pub sampled_minutes: i64,
    /// A started game with an ending, untouched this many days, is shelved.
    pub shelve_after_days: i64,
    /// Shelved and untouched this many days, it's abandoned.
    pub abandon_after_days: i64,
    /// Hours that make a game one of your staples.
    pub staple_hours: i64,
    /// Days Burnt out keeps a game out of picks.
    pub burnout_days: i64,
    /// Evidence for the taste profile's patterns counts half after this many days.
    pub pattern_half_life_days: i64,
    /// Games a pattern needs on its side before the profile shows it.
    pub pattern_min_games: i64,
    /// The play diary: note what you play, hour by hour. Off until you turn it on.
    pub play_diary: bool,
    /// Hidden gems: the share of Steam reviews positive, in percent,
    /// and how many reviews, a game needs to be trusted.
    pub gem_min_positive: i64,
    pub gem_min_reviews: i64,
    /// The longest main story a hidden gem has, in hours: a session or two.
    pub gem_max_hours: i64,
    /// Games on the Hidden gems shelf.
    pub gem_count: i64,
    /// New to you: the longest main story a pick has, in hours.
    pub new_to_you_max_hours: i64,
    /// A genre is unfamiliar under this share of your played hours, in percent.
    pub unfamiliar_share: i64,
    /// The recommended wishlist: suggestions of games you don't
    /// own, your Steam wishlist, and the data fetched for them. On by default.
    pub wishlist: bool,
    /// Cap on candidate lines sent to the LLM.
    pub max_candidates: usize,
    pub host: String,
    pub port: u16,
    /// The address other devices reach UpNext at, such as
    /// `http://den.lan:8787` or `https://upnext.example.org`: it
    /// widens the hosts and origins the server answers, and is where Steam
    /// sends a sign-in back to. See [`PublicUrl`].
    pub public_url: Option<String>,
    /// Settings, "Allow other devices": listen on every address
    /// instead of loopback. Off until turned on; a `host` beyond loopback
    /// (`UPNEXT_HOST`, a container) listens there whatever this says.
    pub allow_other_devices: bool,
    /// Settings, "Announce on the network": say over mDNS that
    /// UpNext is here, so a phone or another computer finds it without an
    /// address, whenever other devices may use it. On by default.
    pub announce: bool,
    /// The computer's name on the network, in that announcement and on the
    /// computer's Allow dialog; the host name when unset.
    pub network_name: Option<String>,
    /// A certificate (PEM, the chain) and its private key for HTTPS on the
    /// same port as plain HTTP. Both or neither.
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    /// A folder with a built web UI to serve instead of the one built into
    /// the binary (`UPNEXT_STATIC_DIR`).
    pub static_dir: Option<PathBuf>,
    /// The database's folder by default, with its backups beside it (`UPNEXT_DATA_DIR`).
    pub data_dir: PathBuf,
    /// Where `.env` is read from (`UPNEXT_CONFIG_DIR`, from the environment only).
    pub config_dir: PathBuf,
    /// Scratch space: the working directory of `claude -p` (`UPNEXT_CACHE_DIR`).
    pub cache_dir: PathBuf,
    /// Which LLM backend to use: `claude-cli` (default), `codex-cli`,
    /// `api-key`, `openai-key`, `openai` or, on Windows, `copilot`.
    pub llm_backend: String,
    /// Which backend writes review summaries and the other bulk work
    /// (second chances, hidden gems): "same" (as `llm_backend`, the
    /// default) or one of `LLM_BACKENDS`, with that backend's own address,
    /// key and models. See [`Config::summary_backend`].
    pub digest_backend: String,
    /// Path to the `claude` binary (default: `claude`, resolved via PATH).
    pub claude_bin: String,
    /// Path to the `codex` binary (default: `codex`, resolved via PATH).
    pub codex_bin: String,
    /// Model for picks, series guides and Help me decide on `codex-cli`.
    pub codex_model: Option<String>,
    /// Model for review summaries on `codex-cli` (default: `codex_model`).
    pub codex_digest_model: Option<String>,
    /// Reasoning effort on `codex-cli`; `None` is Codex's default for the model.
    pub codex_effort: Option<String>,
    /// Heroic config directory override (auto-detected when unset).
    pub heroic_dir: Option<PathBuf>,
    /// The Epic Games Launcher's manifests folder (on Windows, found in
    /// `%ProgramData%` when unset).
    pub epic_manifests: Option<PathBuf>,
    /// Steam client directory override (auto-detected when unset).
    pub steam_dir: Option<PathBuf>,
    /// Default path to a GOG Galaxy `galaxy-2.0.db` copy.
    pub galaxy_db: Option<PathBuf>,
    /// Galaxy integrations whose games a sync leaves out: `origin,xboxone`.
    pub galaxy_skip: Option<String>,
    /// Sync games hidden in the Steam client too (off: they're left out).
    pub steam_include_hidden: bool,
    /// Where the GOG sign-in was kept before schema v34. A sign-in
    /// found there moves into the database the first time it's read.
    pub gog_token_path: PathBuf,
    /// Model for review digests (cheaper than the pick model). `None` = CLI default.
    pub digest_model: Option<String>,
    pub igdb_client_id: Option<String>,
    pub igdb_client_secret: Option<String>,
    /// Anthropic API key for the `api-key` backend.
    pub anthropic_api_key: Option<String>,
    /// OpenAI API key for the `openai-key` backend.
    pub openai_key: Option<String>,
    /// Model for picks, series guides and Help me decide on `openai-key`.
    pub openai_key_model: Option<String>,
    /// Model for review summaries on `openai-key` (default: `openai_key_model`).
    pub openai_key_digest_model: Option<String>,
    /// Reasoning effort on `openai-key`; `None` is OpenAI's default for the model.
    pub openai_key_effort: Option<String>,
    /// The OpenAI API's address for `openai-key` (env only, for a mock).
    pub openai_key_url: String,
    /// OpenAI-compatible server for `llm_backend = "openai"` (default: Ollama).
    pub openai_base_url: String,
    pub openai_model: Option<String>,
    /// Cheaper model on the same server for review summaries (default: `openai_model`).
    pub openai_digest_model: Option<String>,
    pub openai_api_key: Option<String>,
    /// Whether the OpenAI-compatible server's model thinks before it
    /// answers: "auto" (off on a server on this computer or network, the
    /// model's own default elsewhere), "off", or "on" (the model's default).
    pub openai_thinking: String,
    /// RAWG API key (free, per user): tags/genres/ratings for any game.
    pub rawg_api_key: Option<String>,
    /// "auto" (Linux compatibility on when running on Linux), "on" or "off".
    pub linux_mode: String,
    /// UI theme: "arcade" (default), "ember" or "daylight".
    pub theme: String,
    /// Switch to Daylight when the OS is in light mode.
    pub theme_follow_system: bool,
    /// Sync (and fetch data for new games) once a day while the server runs.
    pub auto_sync: bool,
    /// The app window's size when it last closed, `WIDTHxHEIGHT` in
    /// logical pixels, not maximized; unset until it has closed once.
    pub window_size: Option<String>,
    /// Whether the app window was maximized when it last closed.
    pub window_maximized: bool,
    /// The app window's zoom, in percent (Ctrl+plus, Ctrl+minus, Ctrl+0).
    pub window_zoom: i64,
    /// On Windows, closing the app window leaves its server running, with
    /// the tray icon as the way back.
    pub keep_running: bool,
    /// On Windows, the app starts at sign-in with only its tray icon
    /// (`autostart`, the Run key).
    pub start_with_windows: bool,
    /// Backlog verdicts: the service's address, the base of its
    /// endpoint and its snapshot. The project's own service
    /// (`verdicts::DEFAULT_URL`) unless set; tests' configs have none.
    pub verdicts_url: Option<String>,
    /// After a mark that ends something, offer "Tell other backlog players
    /// how it went?" ("Don't ask again" turns it off). Each player's own.
    pub verdicts_ask: bool,
    /// Download other players' backlog verdicts daily and show them on game
    /// pages, pick cards and the library. Off until the
    /// wizard or Settings turns it on.
    pub verdicts_receive: bool,
    /// Let the AI read other backlog players' verdicts: their points in the
    /// ranking that chooses picks' candidates, the `bv` lines in picks, "too"
    /// and Help me decide, and Second Chance's line. One switch, so with it
    /// off the AI is given what it would be with none kept (`docs/verdicts/reading.md`).
    /// On by default since the A/B on the mock library.
    pub verdicts_prompt: bool,
    /// Signing in through an identity provider: its
    /// issuer address (`https://sso.den.example.org`), found by discovery.
    /// Off while unset.
    pub oidc_issuer: Option<String>,
    /// UpNext's client id at the provider.
    pub oidc_client_id: Option<String>,
    /// UpNext's client secret there; unset for a public client.
    pub oidc_client_secret: Option<String>,
    /// What the "Sign in with" button calls the provider; the issuer's
    /// host when unset.
    pub oidc_name: Option<String>,
    /// A first sign-in from an account no player is linked to makes a
    /// player for it. On by default; off, a player must be linked first.
    pub oidc_auto_provision: bool,
    /// Groups, comma-separated, one of which the ID token's `groups` claim
    /// must hold. Unset: whoever the provider lets sign in to this client.
    pub oidc_allowed_groups: Option<String>,
    /// Proxies whose forwarding headers UpNext believes, addresses and
    /// ranges, comma-separated (`127.0.0.1, 172.16.0.0/12`). Unset: none.
    pub trusted_proxies: Option<String>,
    /// "The proxy does the login": a request from a listed proxy needs no
    /// token and lands on "Who's playing?". Off by default.
    pub trusted_proxy_login: bool,
    /// Where each setting that isn't at its default came from.
    sources: BTreeMap<&'static str, Source>,
    /// The `.env` file each variable set there came from (not the ones the
    /// environment already had).
    env_files: BTreeMap<String, PathBuf>,
    /// The `.env` files read, in the order they were read.
    pub env_files_read: Vec<PathBuf>,
    /// Running in UpNext's container image (`UPNEXT_CONTAINER=1`):
    /// nobody is "this computer", so the server prints a pairing code to
    /// its log, and the UI says how to get one.
    pub container: bool,
    /// The folders holding other users' home folders, when set: setup
    /// looks there for their Steam and Heroic (`other_users::homes`) in
    /// place of next to this user's own home. Not a setting: nothing loads
    /// it from the environment or a `.env`, so only tests set it.
    pub users_dirs: Option<Vec<PathBuf>>,
    /// Install Claude Code from Anthropic when `serve` starts, if it isn't
    /// found (`UPNEXT_INSTALL_CLAUDE=1`): how a container uses a
    /// Claude subscription without the image carrying the CLI.
    pub install_claude: bool,
    /// How the server was started, for the setup wizard: "app"
    /// inside `upnext app`, "service" when the systemd unit sets
    /// `UPNEXT_MODE=service`, else empty for `serve` (`run_mode()`).
    pub run_mode: String,
    /// The user name UpNext runs under, written as a name ("sam" is "Sam"),
    /// which the wizard offers as the first player's name when Steam gives
    /// none. None for root and the accounts services and devices
    /// come with, and in a container, whose user is the image's. Read from
    /// the environment by `load` only, so tests leave it unset.
    pub login_name: Option<String>,
    /// Whose settings these are: the player a request or command
    /// acts for. None for the library's own work, and before `with_db`.
    pub player: Option<PlayerId>,
    /// The AI account the AI settings came from, when they came from one.
    pub ai_account: Option<i64>,
    /// Read from the database with no AI account to use: there is no AI,
    /// and picks are made without it.
    pub no_ai_account: bool,
    /// An AI account's own Claude sign-in, from `claude setup-token`: the
    /// Claude CLI it starts gets it as `CLAUDE_CODE_OAUTH_TOKEN`, so two
    /// players can each use their own subscription on one computer.
    pub claude_oauth_token: Option<String>,
    /// The Codex CLI's home for an AI account other than this computer's
    /// (`CODEX_HOME`), where its own `codex login` is kept.
    pub codex_home: Option<PathBuf>,
    /// These settings before the database was laid over them, so a job that
    /// runs with the library's can read them again as one player has them.
    pub before_db: Option<std::sync::Arc<Config>>,
    /// The command line's `--player` (or `UPNEXT_PLAYER`): whom a command
    /// acts for, by name or id.
    pub cli_player: Option<String>,
}

/// The Anthropic API key's own models, per feature, when the setting was
/// never chosen (decided 2026-10-09, after measuring both on a real
/// library): picks, series guides and "Help me decide" on Sonnet 5.5,
/// which got the player's own history right where Haiku slipped, and review
/// summaries on Haiku 5.5, close to Sonnet's at a twentieth of the price.
/// The Claude CLI's default stays "sonnet" for both.
pub const API_KEY_PICK_MODEL: &str = "sonnet";
pub const API_KEY_SUMMARY_MODEL: &str = "haiku";

/// A setting from the environment or a `.env` file whose value doesn't
/// parse, as the context of the error `Config::load` returns. The app's error
/// window looks for it to say where to fix the value, where starting again
/// wouldn't help.
#[derive(Debug)]
pub struct BadSetting {
    pub var: &'static str,
    pub value: String,
    /// The `.env` file it came from; `None` for the environment.
    pub file: Option<PathBuf>,
}

impl std::fmt::Display for BadSetting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}={:?}", self.var, self.value)?;
        match &self.file {
            Some(file) => write!(f, " (in {})", file.display()),
            None => write!(f, " (in the environment)"),
        }
    }
}

impl BadSetting {
    /// What to do about it, for someone who can't see a terminal: the app
    /// window and a double-clicked `upnext.exe`, so the server alone has no use for it.
    #[cfg_attr(not(feature = "desktop"), allow(dead_code))]
    pub fn advice(&self) -> String {
        match &self.file {
            Some(file) => format!("Fix or remove {} in {}, then start UpNext again.", self.var, file.display()),
            None => format!(
                "Fix or remove the environment variable {} where it's set (on Windows, Edit environment variables for your account), then start UpNext again.",
                self.var
            ),
        }
    }
}

/// Where a setting's value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Env,
    App,
    /// The SteamID of the account logged in to the local Steam client.
    SteamClient,
}

/// Values accepted for `theme`.
pub const THEMES: [&str; 3] = ["arcade", "ember", "daylight"];

/// Values accepted for `linux_mode`.
pub const LINUX_MODES: [&str; 3] = ["auto", "on", "off"];

/// Values accepted for `openai_thinking`.
pub const THINKING_MODES: [&str; 3] = ["auto", "off", "on"];

/// Values accepted for `llm_backend`.
#[cfg(not(windows))]
pub const LLM_BACKENDS: [&str; 5] = ["claude-cli", "codex-cli", "api-key", "openai-key", "openai"];
#[cfg(windows)]
pub const LLM_BACKENDS: [&str; 6] = ["claude-cli", "codex-cli", "api-key", "openai-key", "openai", "copilot"];

/// Values accepted for `digest_backend`: "same" as picks, or a backend of its own.
#[cfg(not(windows))]
pub const DIGEST_BACKENDS: [&str; 6] = ["same", "claude-cli", "codex-cli", "api-key", "openai-key", "openai"];
#[cfg(windows)]
pub const DIGEST_BACKENDS: [&str; 7] = ["same", "claude-cli", "codex-cli", "api-key", "openai-key", "openai", "copilot"];

/// Values accepted for `codex_effort`, least to most. A model takes some of
/// them (`codex debug models` lists which).
pub const CODEX_EFFORTS: [&str; 6] = ["low", "medium", "high", "xhigh", "max", "ultra"];

/// Values accepted for `openai_key_effort`, least to most: the efforts
/// OpenAI's Chat Completions takes for the GPT-6 models, without `none` and
/// `minimal`, which some of them refuse.
pub const OPENAI_EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

pub const DEFAULT_OPENAI_BASE_URL: &str = "http://localhost:11434/v1";

pub const DEFAULT_OPENAI_KEY_URL: &str = "https://api.openai.com/v1";

/// Where the app's windows keep their webview data: the cache folder when
/// one was chosen (`UPNEXT_CACHE_DIR`), else `cache` in the data folder when
/// that was moved (`UPNEXT_DATA_DIR`), else the platform's cache folder,
/// `standard`. A second copy beside an installed one usually moves only its
/// data, and the two then shared WebView2's folder, which it refuses while
/// the other copy runs (Windows retest).
pub fn webview_cache(cache: Option<&Path>, data: Option<&Path>, standard: &Path) -> PathBuf {
    match (cache, data) {
        (Some(cache), _) => cache.into(),
        (None, Some(data)) => data.join("cache"),
        (None, None) => standard.into(),
    }
}

/// How a flag may be spelt, for the error when it isn't.
const FLAG_SPELLINGS: &str = "true or false (or 1 or 0, on or off, yes or no)";

/// A yes-or-no setting as the environment, a `.env` file or the app spells
/// it, in any case: true or false, 1 or 0, on or off, yes or no. Anything
/// else is `None`, an error to whoever reads it, rather than a quiet "no"
/// (B-CORE-4).
pub fn parse_flag(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "on" | "yes" => Some(true),
        "false" | "0" | "off" | "no" => Some(false),
        _ => None,
    }
}

impl std::fmt::Debug for Config {
    /// Every setting by its key, a secret one (the keys and tokens) as
    /// `<set>`, so a log line or a panic never prints one, then the fields
    /// that aren't settings.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = f.debug_struct("Config");
        for s in SETTINGS {
            let shown = s.show(self);
            match shown {
                Some(_) if s.secret => out.field(s.key, &"<set>"),
                shown => out.field(s.key, &shown),
            };
        }
        out.field("config_dir", &self.config_dir)
            .field("env_files_read", &self.env_files_read)
            .field("container", &self.container)
            .field("install_claude", &self.install_claude)
            .field("run_mode", &self.run_mode)
            .field("player", &self.player)
            .field("ai_account", &self.ai_account)
            .field("no_ai_account", &self.no_ai_account)
            .field("codex_home", &self.codex_home)
            .field("before_db", &self.before_db)
            .field("cli_player", &self.cli_player)
            .field("sources", &self.sources)
            .field("env_files", &self.env_files)
            .finish()
    }
}

impl Config {
    /// The player these settings are read for: [`Config::player`], else
    /// player 1, as before there were players.
    pub fn whose(&self) -> PlayerId {
        self.player.unwrap_or(PlayerId::FIRST)
    }

    /// The folder the app's windows keep their webview data under (see
    /// `desktop::webview_dir` and [`webview_cache`]).
    pub fn webview_cache(&self) -> PathBuf {
        let chosen = |key| self.source(key) != "default";
        let cache = chosen("cache_dir").then_some(self.cache_dir.as_path());
        webview_cache(cache, chosen("data_dir").then_some(self.data_dir.as_path()), &self.cache_dir)
    }

    /// The same when the settings didn't load (a bad one), from the
    /// environment alone (`var`), since a `.env` may be what failed: the
    /// error window then still finds the folder the app would have used.
    pub fn webview_cache_unloaded(var: impl Fn(&str) -> Option<String>) -> PathBuf {
        let path = |key| var(key).map(|v| v.trim().to_string()).filter(|v| !v.is_empty()).map(PathBuf::from);
        webview_cache(path("UPNEXT_CACHE_DIR").as_deref(), path("UPNEXT_DATA_DIR").as_deref(), &Dirs::standard().cache)
    }

    /// Where a setting's value came from, for people: "env UPNEXT_DB",
    /// "/home/you/.config/upnext/.env", "app", "steam client", "default" or
    /// "none". [`Config::source`] says "env" for both of the first two.
    pub fn origin(&self, key: &str) -> String {
        match (self.source(key), SETTINGS.iter().find(|s| s.key == key).and_then(|s| s.env)) {
            ("env", Some(var)) => match self.env_files.get(var) {
                Some(file) => file.display().to_string(),
                None => format!("env {var}"),
            },
            (source, _) => source.to_string(),
        }
    }

    /// Where a setting's value came from: "app", "env" (the environment or
    /// a `.env`), "steam client", "default" (its built-in value), or "none" (unset).
    pub fn source(&self, key: &str) -> &'static str {
        match self.sources.get(key) {
            Some(Source::App) => "app",
            Some(Source::Env) => "env",
            Some(Source::SteamClient) => "steam client",
            None => match SETTINGS.iter().find(|s| s.key == key).and_then(|s| s.show(self)) {
                Some(_) => "default",
                None => "none",
            },
        }
    }

    /// The `public_url` setting, when it's set and makes sense.
    pub fn public(&self) -> Option<PublicUrl> {
        self.public_url.as_deref().and_then(PublicUrl::parse)
    }

    /// The settings statuses follow.
    pub fn status_rules(&self) -> crate::lifecycle::StatusRules {
        crate::lifecycle::StatusRules {
            sampled_minutes: self.sampled_minutes,
            shelve_after_days: self.shelve_after_days,
            abandon_after_days: self.abandon_after_days,
            staple_hours: self.staple_hours,
            burnout_days: self.burnout_days,
        }
    }

    /// The settings the taste profile's patterns follow.
    pub fn taste_rules(&self) -> crate::taste::TasteRules {
        crate::taste::TasteRules { half_life_days: self.pattern_half_life_days, min_games: self.pattern_min_games.max(1) as usize }
    }

    /// The folder the database is in; the backups go next to it. The data dir, unless `UPNEXT_DB` puts the database elsewhere.
    /// "app", "service" or "serve": how the server was started.
    pub fn run_mode(&self) -> &str {
        if self.run_mode.is_empty() { "serve" } else { &self.run_mode }
    }

    pub fn db_dir(&self) -> PathBuf {
        self.db_path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| self.data_dir.clone())
    }

    /// Where the Epic sign-in was kept before schema v34, next to
    /// the database; one found there moves into the database.
    pub fn epic_token_path(&self) -> PathBuf {
        self.db_dir().join("epic_token.json")
    }

    /// The `claude` program to run: as set when it's a path or on `PATH`,
    /// else where its installer puts it (a service doesn't get the login
    /// shell's `PATH`, so `~/.local/bin` is often missing from it).
    pub fn claude_program(&self) -> String {
        find_program(&self.claude_bin).map(|(p, _)| p.display().to_string()).unwrap_or_else(|| self.claude_bin.clone())
    }

    /// The `codex` program to run, found the same way.
    pub fn codex_program(&self) -> String {
        find_program(&self.codex_bin).map(|(p, _)| p.display().to_string()).unwrap_or_else(|| self.codex_bin.clone())
    }

    /// The model to ask for picks, series guides and "Help me decide", in the
    /// active backend's terms (a Claude alias, or the other server's model name).
    pub fn pick_model(&self) -> Option<String> {
        match self.llm_backend.as_str() {
            "openai" => self.openai_model.clone(),
            "openai-key" => self.openai_key_model.clone(),
            "codex-cli" => self.codex_model.clone(),
            "api-key" => self.chosen("model", &self.model).or_else(|| Some(API_KEY_PICK_MODEL.into())),
            _ => self.model.clone(),
        }
    }

    /// The Claude model setting `key` (`value`) for the Anthropic API key
    /// backend: none when it was never chosen, so the key's own default for
    /// the feature applies (`API_KEY_PICK_MODEL`, `API_KEY_SUMMARY_MODEL`)
    /// rather than the Claude CLI's default alias, "sonnet". A model chosen
    /// in the app or the environment is kept.
    fn chosen(&self, key: &str, value: &Option<String>) -> Option<String> {
        value.clone().filter(|_| self.source(key) != "default")
    }

    /// What to tell the OpenAI-compatible server about thinking: `Some(false)`
    /// turns it off (`chat_template_kwargs.enable_thinking`, which vLLM and
    /// llama.cpp pass to the model's chat template, as Qwen's reads it), and
    /// None leaves the model's default. "auto" turns it off only for a server
    /// on this computer or network: a hosted API may refuse a field it
    /// doesn't know.
    pub fn openai_thinking(&self) -> Option<bool> {
        match self.openai_thinking.as_str() {
            "off" => Some(false),
            "on" => None,
            _ => is_local_server(&self.openai_base_url).then_some(false),
        }
    }

    /// The backend that writes review summaries, review tags, second
    /// chances' "Now" and hidden gems: `digest_backend` when it names one,
    /// else the picks' `llm_backend`.
    pub fn summary_backend(&self) -> &str {
        match self.digest_backend.as_str() {
            "" | "same" => &self.llm_backend,
            other => other,
        }
    }

    /// The model for review summaries: the cheap one, in the summaries
    /// backend's terms.
    pub fn summary_model(&self) -> Option<String> {
        match self.summary_backend() {
            "openai" => self.openai_digest_model.clone().or_else(|| self.openai_model.clone()),
            "openai-key" => self.openai_key_digest_model.clone().or_else(|| self.openai_key_model.clone()),
            "codex-cli" => self.codex_digest_model.clone().or_else(|| self.codex_model.clone()),
            "api-key" => self.chosen("digest_model", &self.digest_model).or_else(|| Some(API_KEY_SUMMARY_MODEL.into())),
            _ => self.digest_model.clone(),
        }
    }

    /// Whether Linux compatibility (ProtonDB, native builds) matters for picks.
    pub fn linux(&self) -> bool {
        match self.linux_mode.as_str() {
            "on" => true,
            "off" => false,
            _ => running_on_linux(),
        }
    }

    /// Steam credentials, required for `sync`.
    pub fn steam_credentials(&self) -> Result<(&str, &str)> {
        match (&self.steam_api_key, &self.steam_id) {
            (Some(k), Some(id)) => {
                if id.len() != 17 || !id.chars().all(|c| c.is_ascii_digit()) {
                    bail!("STEAM_ID must be a 17-digit SteamID64 (got {id:?})");
                }
                Ok((k, id))
            }
            (None, Some(id)) => bail!(
                "no Steam Web API key: add one on the Settings page, with `upnext config set steam_api_key`, or in .env (get one at https://steamcommunity.com/dev/apikey); STEAM_ID {id} was {}",
                match self.source("steam_id") {
                    "env" => "read from .env",
                    "app" => "set in app",
                    _ => "auto-detected from the Steam client",
                }
            ),
            (Some(_), None) => {
                bail!("STEAM_ID is not set and no Steam client login was found; sign in with Steam on the Settings page or set it")
            }
            (None, None) => bail!("Steam is not configured: sign in with Steam and add an API key on the Settings page (or use .env)"),
        }
    }
}

/// The server runs on the player's machine, so its OS is the player's OS.
pub fn running_on_linux() -> bool {
    cfg!(target_os = "linux")
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context;

    #[test]
    fn debug_never_shows_a_secret_setting() {
        let mut cfg = Config::built_in();
        for key in secret_keys() {
            SETTINGS.iter().find(|s| s.key == key).unwrap().set(&mut cfg, "not-a-real-secret").unwrap();
        }
        cfg.steam_id = Some("76561197960287930".into());
        cfg.before_db = Some(std::sync::Arc::new(cfg.clone()));
        let shown = format!("{cfg:?}");
        assert!(!shown.contains("not-a-real-secret"), "{shown}");
        assert!(shown.contains("claude_oauth_token: \"<set>\"") && shown.contains("76561197960287930"), "{shown}");
        assert!(shown.contains("oidc_client_secret: \"<set>\""), "the identity provider's secret: {shown}");
    }

    /// When a bad setting stops the app, its error window finds the same
    /// folder from the environment, so WebView2 starts beside a copy that
    /// holds the default one.
    #[test]
    fn the_error_window_finds_the_webview_data_without_the_settings() {
        let standard = Dirs::standard().cache;
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |key: &str| pairs.iter().find(|(k, _)| *k == key).map(|(_, v)| v.to_string())
        };
        assert_eq!(Config::webview_cache_unloaded(env(&[])), standard);
        assert_eq!(
            Config::webview_cache_unloaded(env(&[("UPNEXT_DATA_DIR", "scratch"), ("UPNEXT_CACHE_DIR", " ")])),
            Path::new("scratch").join("cache")
        );
        assert_eq!(
            Config::webview_cache_unloaded(env(&[("UPNEXT_DATA_DIR", "scratch"), ("UPNEXT_CACHE_DIR", "scratch-cache")])),
            PathBuf::from("scratch-cache")
        );
        assert_eq!(Config::webview_cache_unloaded(env(&[("UPNEXT_PORT", "abc")])), standard);
    }

    /// With an Anthropic API key, models left at their defaults are the
    /// key's own per feature, Sonnet 5.5 for picks and Haiku 5.5 for
    /// summaries, while one chosen is kept; the Claude CLI keeps "sonnet"
    /// for both.
    #[test]
    fn the_api_key_has_default_models_of_its_own() {
        use crate::llm::api_key::resolve_model;
        let mut c = Config::built_in();
        assert_eq!((c.pick_model().as_deref(), c.summary_model().as_deref()), (Some("sonnet"), Some("sonnet")), "the Claude CLI");
        c.llm_backend = "api-key".into();
        assert_eq!(resolve_model(c.pick_model().as_deref()), "claude-sonnet-5-5");
        assert_eq!(resolve_model(c.summary_model().as_deref()), "claude-haiku-5-5");
        c.sources.insert("model", Source::App);
        c.sources.insert("digest_model", Source::Env);
        assert_eq!((c.pick_model().as_deref(), c.summary_model().as_deref()), (Some("sonnet"), Some("sonnet")), "chosen, so kept");
        c.model = Some("opus".into());
        assert_eq!(c.pick_model().as_deref(), Some("opus"));
        c.model = None;
        assert_eq!(c.pick_model().as_deref(), Some(API_KEY_PICK_MODEL), "\"default\" chosen in Settings");
    }

    /// Summaries follow the picks' backend until `digest_backend` names one
    /// of its own, and then take that backend's summaries model and its
    /// defaults: Haiku 5.5 with an Anthropic API key beside a Claude
    /// subscription for picks, say.
    #[test]
    fn summaries_can_have_a_backend_of_their_own() {
        use crate::llm::api_key::resolve_model;
        let mut c = Config::built_in();
        assert_eq!((c.digest_backend.as_str(), c.summary_backend()), ("same", "claude-cli"));
        c.llm_backend = "codex-cli".into();
        assert_eq!(c.summary_backend(), "codex-cli", "same as picks");
        assert_eq!(c.summary_model().as_deref(), Some("luna"));

        c.llm_backend = "claude-cli".into();
        c.digest_backend = "api-key".into();
        assert_eq!(c.summary_backend(), "api-key");
        assert_eq!(resolve_model(c.pick_model().as_deref()), "claude-sonnet-5-5", "picks keep the CLI's sonnet");
        assert_eq!(resolve_model(c.summary_model().as_deref()), "claude-haiku-5-5", "the key's own summaries default");

        c.digest_backend = "openai".into();
        c.openai_model = Some("qwen-big".into());
        assert_eq!(c.summary_model().as_deref(), Some("qwen-big"), "the server's model when it has no summaries model");
        c.openai_digest_model = Some("qwen-small".into());
        assert_eq!(c.summary_model().as_deref(), Some("qwen-small"));
        assert_eq!(c.pick_model().as_deref(), Some("sonnet"));

        assert!(validate_setting("digest_backend", "openai").is_ok());
        assert!(validate_setting("digest_backend", "same").is_ok());
        assert!(validate_setting("digest_backend", "gpt").is_err());
    }

    /// A value that doesn't parse names where it came from, and the advice
    /// says where to fix it, for the app's error window.
    #[test]
    fn a_bad_setting_says_where_it_was_set() {
        let from_file = BadSetting { var: "UPNEXT_PORT", value: "abc".into(), file: Some(PathBuf::from("/home/you/.config/upnext/.env")) };
        let e = Err::<(), _>(anyhow::anyhow!("port must be a number")).context(from_file).unwrap_err();
        assert_eq!(format!("{e:#}"), r#"UPNEXT_PORT="abc" (in /home/you/.config/upnext/.env): port must be a number"#);
        let found = e.downcast_ref::<BadSetting>().expect("the error carries the setting");
        assert_eq!(found.advice(), "Fix or remove UPNEXT_PORT in /home/you/.config/upnext/.env, then start UpNext again.");
        let from_env = BadSetting { var: "UPNEXT_PORT", value: "abc".into(), file: None };
        assert_eq!(from_env.to_string(), r#"UPNEXT_PORT="abc" (in the environment)"#);
        assert!(
            from_env.advice().starts_with("Fix or remove the environment variable UPNEXT_PORT where it's set"),
            "{}",
            from_env.advice()
        );
    }
}
