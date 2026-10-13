//! The settings' table: every setting as a row of [`SETTINGS`], and what is
//! read from it: which settings the app can change and how a value for one
//! is checked, which are secret or name a file, and how a value goes into
//! its `Config` field and back out as text.

use super::{
    CODEX_EFFORTS, Config, DEFAULT_OPENAI_BASE_URL, DEFAULT_OPENAI_KEY_URL, DIGEST_BACKENDS, FLAG_SPELLINGS, LINUX_MODES, LLM_BACKENDS,
    OPENAI_EFFORTS, THEMES, THINKING_MODES, check, parse_flag,
};
use anyhow::{Context, Result, bail};
use std::path::PathBuf;

/// The `Config` field a setting fills, by kind.
#[derive(Clone, Copy)]
enum Slot {
    Text(fn(&Config) -> &String, fn(&mut Config) -> &mut String),
    OptText(fn(&Config) -> &Option<String>, fn(&mut Config) -> &mut Option<String>),
    /// A model name; "default" or "cli" mean the CLI's own default (`None`).
    Model(fn(&Config) -> &Option<String>, fn(&mut Config) -> &mut Option<String>),
    /// Text that must be one of the setting's `choices`.
    Choice(fn(&Config) -> &String, fn(&mut Config) -> &mut String),
    I64(fn(&Config) -> &i64, fn(&mut Config) -> &mut i64),
    Usize(fn(&Config) -> &usize, fn(&mut Config) -> &mut usize),
    U16(fn(&Config) -> &u16, fn(&mut Config) -> &mut u16),
    Flag(fn(&Config) -> &bool, fn(&mut Config) -> &mut bool),
    Path(fn(&Config) -> &PathBuf, fn(&mut Config) -> &mut PathBuf),
    OptPath(fn(&Config) -> &Option<PathBuf>, fn(&mut Config) -> &mut Option<PathBuf>),
}

/// `slot!(Kind, field)`: the `Slot` for a `Config` field.
macro_rules! slot {
    ($kind:ident, $field:ident) => {
        Slot::$kind(|c: &Config| &c.$field, |c: &mut Config| &mut c.$field)
    };
}

/// One setting. See the module docs.
pub struct Setting {
    /// The name in `config set`, `config list`, the Settings page and the database.
    pub key: &'static str,
    /// The environment variable (or `.env` line) that sets it.
    pub env: Option<&'static str>,
    /// The built-in value ("" for none). A few are worked out when loading
    /// instead: `data_dir` and `cache_dir` ([`Dirs`]), `db_path` (in the data
    /// dir), `gog_token_path` (next to the database), `steam_id` (the Steam
    /// client's login) and `galaxy_db` (Galaxy's own place, on Windows).
    pub default: &'static str,
    /// The app can change it; the stored value wins over the environment.
    pub in_app: bool,
    /// Kept out of `config list` (masked) and out of database exports.
    pub secret: bool,
    /// The values it accepts, when it is one of a few.
    pub choices: &'static [&'static str],
    /// Checks and normalizes a value typed in the app (given the key and the value).
    check: fn(&str, &str) -> Result<String>,
    slot: Slot,
}

impl Setting {
    const fn new(key: &'static str, slot: Slot) -> Self {
        Self { key, env: None, default: "", in_app: false, secret: false, choices: &[], check: check::any, slot }
    }
    const fn env(mut self, var: &'static str) -> Self {
        self.env = Some(var);
        self
    }
    const fn default(mut self, value: &'static str) -> Self {
        self.default = value;
        self
    }
    /// Settable in the app, with this check.
    const fn app(mut self, check: fn(&str, &str) -> Result<String>) -> Self {
        self.in_app = true;
        self.check = check;
        self
    }
    const fn secret(mut self) -> Self {
        self.secret = true;
        self
    }
    const fn choices(mut self, choices: &'static [&'static str]) -> Self {
        self.choices = choices;
        self
    }

    /// Put a value into the config. Returns whether it took the value; a
    /// value it can't read, a choice it doesn't know included, is an error
    /// (B-CORE-4). A setting with `choices` takes only those, whatever its
    /// slot: the effort settings are optional text with a list.
    pub(super) fn set(&self, c: &mut Config, v: &str) -> Result<bool> {
        if !self.choices.is_empty() && !self.choices.contains(&v) {
            bail!("must be one of {}", self.choices.join(", "));
        }
        match self.slot {
            Slot::Text(_, f) => *f(c) = v.to_string(),
            Slot::OptText(_, f) => *f(c) = Some(v.to_string()),
            Slot::Model(_, f) => *f(c) = if v == "default" || v == "cli" { None } else { Some(v.to_string()) },
            Slot::Choice(_, f) => *f(c) = v.to_string(),
            Slot::I64(_, f) => *f(c) = v.parse()?,
            Slot::Usize(_, f) => *f(c) = v.parse()?,
            Slot::U16(_, f) => *f(c) = v.parse()?,
            Slot::Flag(_, f) => *f(c) = parse_flag(v).with_context(|| format!("must be {FLAG_SPELLINGS}"))?,
            Slot::Path(_, f) => *f(c) = PathBuf::from(v),
            Slot::OptPath(_, f) => *f(c) = Some(PathBuf::from(v)),
        }
        Ok(true)
    }

    /// The value as text, or `None` when unset.
    pub fn show(&self, c: &Config) -> Option<String> {
        match self.slot {
            Slot::Text(f, _) | Slot::Choice(f, _) => Some(f(c).clone()),
            Slot::OptText(f, _) => f(c).clone(),
            Slot::Model(f, _) => Some(f(c).clone().unwrap_or_else(|| "default".into())),
            Slot::I64(f, _) => Some(f(c).to_string()),
            Slot::Usize(f, _) => Some(f(c).to_string()),
            Slot::U16(f, _) => Some(f(c).to_string()),
            Slot::Flag(f, _) => Some(f(c).to_string()),
            Slot::Path(f, _) => Some(f(c).display().to_string()),
            Slot::OptPath(f, _) => f(c).as_ref().map(|p| p.display().to_string()),
        }
    }
}

/// Every setting, in the order `config list` shows them.
pub static SETTINGS: &[Setting] = &[
    Setting::new("steam_api_key", slot!(OptText, steam_api_key)).env("STEAM_API_KEY").app(check::steam_api_key).secret(),
    Setting::new("steam_id", slot!(OptText, steam_id)).env("STEAM_ID").app(check::steam_id),
    // Sonnet by default: picks are a ranking task it does well, at a
    // fraction of Opus's cost (Settings can switch to Opus).
    Setting::new("model", slot!(Model, model)).env("UPNEXT_MODEL").default("sonnet").app(check::any),
    // Review summaries and review tags: Sonnet. Haiku tagged what the reviews
    // didn't say and was far slower on the tag schema in a sample run.
    Setting::new("digest_model", slot!(Model, digest_model)).env("UPNEXT_DIGEST_MODEL").default("sonnet").app(check::any),
    Setting::new("llm_backend", slot!(Choice, llm_backend)).env("UPNEXT_LLM").default("claude-cli").app(check::any).choices(&LLM_BACKENDS),
    // A subscription for picks and a model at home for the bulk summary work, say.
    Setting::new("digest_backend", slot!(Choice, digest_backend))
        .env("UPNEXT_DIGEST_LLM")
        .default("same")
        .app(check::any)
        .choices(&DIGEST_BACKENDS),
    // The newest Luna, whatever its version (`llm::families`): the
    // model a Plus plan allows the most messages of.
    Setting::new("codex_model", slot!(OptText, codex_model)).env("UPNEXT_CODEX_MODEL").default("luna").app(check::no_spaces),
    Setting::new("codex_digest_model", slot!(OptText, codex_digest_model))
        .env("UPNEXT_CODEX_DIGEST_MODEL")
        .default("luna")
        .app(check::no_spaces),
    Setting::new("codex_effort", slot!(OptText, codex_effort)).env("UPNEXT_CODEX_EFFORT").app(check::any).choices(&CODEX_EFFORTS),
    Setting::new("anthropic_api_key", slot!(OptText, anthropic_api_key)).env("ANTHROPIC_API_KEY").app(check::anthropic_api_key).secret(),
    Setting::new("openai_key", slot!(OptText, openai_key)).env("OPENAI_API_KEY").app(check::openai_key).secret(),
    // The newest Luna, as on the ChatGPT plan, after a quality check.
    Setting::new("openai_key_model", slot!(OptText, openai_key_model)).env("UPNEXT_OPENAI_KEY_MODEL").default("luna").app(check::no_spaces),
    Setting::new("openai_key_digest_model", slot!(OptText, openai_key_digest_model))
        .env("UPNEXT_OPENAI_KEY_DIGEST_MODEL")
        .default("luna")
        .app(check::no_spaces),
    Setting::new("openai_key_effort", slot!(OptText, openai_key_effort))
        .env("UPNEXT_OPENAI_KEY_EFFORT")
        .app(check::any)
        .choices(&OPENAI_EFFORTS),
    Setting::new("openai_base_url", slot!(Text, openai_base_url))
        .env("UPNEXT_OPENAI_BASE_URL")
        .default(DEFAULT_OPENAI_BASE_URL)
        .app(check::openai_base_url),
    Setting::new("openai_model", slot!(OptText, openai_model)).env("UPNEXT_OPENAI_MODEL").app(check::no_spaces),
    // A local model that reasons first can take minutes a pick.
    Setting::new("openai_thinking", slot!(Choice, openai_thinking))
        .env("UPNEXT_OPENAI_THINKING")
        .default("auto")
        .app(check::any)
        .choices(&THINKING_MODES),
    Setting::new("openai_digest_model", slot!(OptText, openai_digest_model)).env("UPNEXT_OPENAI_DIGEST_MODEL").app(check::no_spaces),
    Setting::new("openai_api_key", slot!(OptText, openai_api_key)).env("UPNEXT_OPENAI_API_KEY").app(check::no_spaces).secret(),
    // An AI account's own Claude subscription. The environment's
    // CLAUDE_CODE_OAUTH_TOKEN reaches the CLI without UpNext reading it.
    Setting::new("claude_oauth_token", slot!(OptText, claude_oauth_token)).app(check::no_spaces).secret(),
    Setting::new("igdb_client_id", slot!(OptText, igdb_client_id)).env("IGDB_CLIENT_ID").app(check::twitch),
    Setting::new("igdb_client_secret", slot!(OptText, igdb_client_secret)).env("IGDB_CLIENT_SECRET").app(check::twitch).secret(),
    Setting::new("rawg_api_key", slot!(OptText, rawg_api_key)).env("RAWG_API_KEY").app(check::rawg_api_key).secret(),
    Setting::new("sampled_minutes", slot!(I64, sampled_minutes)).env("UPNEXT_SAMPLED_MINUTES").default("120").app(check::sampled_minutes),
    // At 2 months you no longer remember where you were in a game.
    Setting::new("shelve_after_days", slot!(I64, shelve_after_days)).default("60").app(check::days),
    Setting::new("abandon_after_days", slot!(I64, abandon_after_days)).default("730").app(check::days),
    Setting::new("staple_hours", slot!(I64, staple_hours)).default("100").app(check::staple_hours),
    Setting::new("burnout_days", slot!(I64, burnout_days)).default("90").app(check::days),
    // Interest changes, so a year-old reaction counts half.
    Setting::new("pattern_half_life_days", slot!(I64, pattern_half_life_days)).default("365").app(check::days),
    Setting::new("pattern_min_games", slot!(I64, pattern_min_games)).default("4").app(check::pattern_min_games),
    // Personal, so nothing is recorded until you turn it on.
    Setting::new("play_diary", slot!(Flag, play_diary)).default("false").app(check::flag),
    // Trusted ratings and a session or two (decided 2026-09-30).
    Setting::new("gem_min_positive", slot!(I64, gem_min_positive)).default("93").app(check::discovery),
    Setting::new("gem_min_reviews", slot!(I64, gem_min_reviews)).default("500").app(check::discovery),
    Setting::new("gem_max_hours", slot!(I64, gem_max_hours)).default("5").app(check::discovery),
    Setting::new("gem_count", slot!(I64, gem_count)).default("12").app(check::discovery),
    Setting::new("new_to_you_max_hours", slot!(I64, new_to_you_max_hours)).default("8").app(check::discovery),
    Setting::new("unfamiliar_share", slot!(I64, unfamiliar_share)).default("2").app(check::discovery),
    // Off removes every trace of the wishlist from picks and the app (decided 2026-09-30).
    Setting::new("wishlist", slot!(Flag, wishlist)).default("true").app(check::flag),
    // The pre-ranking puts the likely fits first; 120 keeps the prompt
    // around 11k tokens without starving the model of choice.
    Setting::new("max_candidates", slot!(Usize, max_candidates)).env("UPNEXT_MAX_CANDIDATES").default("120").app(check::max_candidates),
    Setting::new("linux_mode", slot!(Choice, linux_mode)).env("UPNEXT_LINUX").default("auto").app(check::any).choices(&LINUX_MODES),
    Setting::new("theme", slot!(Choice, theme)).default("arcade").app(check::any).choices(&THEMES),
    Setting::new("theme_follow_system", slot!(Flag, theme_follow_system)).default("false").app(check::flag),
    Setting::new("auto_sync", slot!(Flag, auto_sync)).default("true").app(check::flag),
    // The app window saves these when it closes.
    Setting::new("window_size", slot!(OptText, window_size)).app(check::window_size),
    Setting::new("window_maximized", slot!(Flag, window_maximized)).default("false").app(check::flag),
    Setting::new("window_zoom", slot!(I64, window_zoom)).default("100").app(check::window_zoom),
    // Windows only: the tray keeps sync running with the
    // window closed, as the systemd user service does on Linux; starting at sign-in
    // is the user's choice.
    Setting::new("keep_running", slot!(Flag, keep_running)).default("true").app(check::flag),
    Setting::new("start_with_windows", slot!(Flag, start_with_windows)).default("false").app(check::flag),
    Setting::new("galaxy_db", slot!(OptPath, galaxy_db)).env("UPNEXT_GALAXY_DB").app(check::any),
    Setting::new("galaxy_skip", slot!(OptText, galaxy_skip)).app(check::galaxy_skip),
    // People hide games in Steam for a reason.
    Setting::new("steam_include_hidden", slot!(Flag, steam_include_hidden)).default("false").app(check::flag),
    Setting::new("data_dir", slot!(Path, data_dir)).env("UPNEXT_DATA_DIR"),
    Setting::new("db_path", slot!(Path, db_path)).env("UPNEXT_DB"),
    Setting::new("gog_token_path", slot!(Path, gog_token_path)).env("UPNEXT_GOG_TOKEN"),
    Setting::new("cache_dir", slot!(Path, cache_dir)).env("UPNEXT_CACHE_DIR"),
    // Unset: the UI built into the binary.
    Setting::new("static_dir", slot!(OptPath, static_dir)).env("UPNEXT_STATIC_DIR"),
    Setting::new("host", slot!(Text, host)).env("UPNEXT_HOST").default("127.0.0.1"),
    Setting::new("port", slot!(U16, port)).env("UPNEXT_PORT").default("8787"),
    Setting::new("public_url", slot!(OptText, public_url)).env("UPNEXT_PUBLIC_URL").app(check::public_url),
    Setting::new("allow_other_devices", slot!(Flag, allow_other_devices)).default("false").app(check::flag),
    Setting::new("announce", slot!(Flag, announce)).default("true").app(check::flag),
    Setting::new("network_name", slot!(OptText, network_name)).app(check::network_name),
    Setting::new("tls_cert", slot!(OptPath, tls_cert)).env("UPNEXT_TLS_CERT").app(check::any),
    Setting::new("tls_key", slot!(OptPath, tls_key)).env("UPNEXT_TLS_KEY").app(check::any),
    Setting::new("claude_bin", slot!(Text, claude_bin)).env("UPNEXT_CLAUDE_BIN").default("claude"),
    Setting::new("codex_bin", slot!(Text, codex_bin)).env("UPNEXT_CODEX_BIN").default("codex"),
    Setting::new("openai_key_url", slot!(Text, openai_key_url)).env("UPNEXT_OPENAI_URL").default(DEFAULT_OPENAI_KEY_URL),
    Setting::new("heroic_dir", slot!(OptPath, heroic_dir)).env("UPNEXT_HEROIC_DIR"),
    Setting::new("epic_manifests", slot!(OptPath, epic_manifests)).env("UPNEXT_EPIC_MANIFESTS"),
    Setting::new("steam_dir", slot!(OptPath, steam_dir)).env("UPNEXT_STEAM_DIR"),
    // The project's own service. Sending is still a press of Send, and
    // receiving a switch that starts off.
    Setting::new("verdicts_url", slot!(OptText, verdicts_url))
        .env("UPNEXT_VERDICTS_URL")
        .default(crate::verdicts::DEFAULT_URL)
        .app(check::verdicts_url),
    Setting::new("verdicts_ask", slot!(Flag, verdicts_ask)).default("true").app(check::flag),
    Setting::new("verdicts_receive", slot!(Flag, verdicts_receive)).default("false").app(check::flag),
    Setting::new("verdicts_prompt", slot!(Flag, verdicts_prompt)).default("true").app(check::flag),
    // Everything off until an issuer is set, and set only
    // on the computer UpNext runs on, since it decides who gets in.
    Setting::new("oidc_issuer", slot!(OptText, oidc_issuer)).env("UPNEXT_OIDC_ISSUER").app(check::oidc_issuer),
    Setting::new("oidc_client_id", slot!(OptText, oidc_client_id)).env("UPNEXT_OIDC_CLIENT_ID").app(check::no_spaces),
    Setting::new("oidc_client_secret", slot!(OptText, oidc_client_secret)).env("UPNEXT_OIDC_CLIENT_SECRET").app(check::no_spaces).secret(),
    Setting::new("oidc_name", slot!(OptText, oidc_name)).env("UPNEXT_OIDC_NAME").app(check::oidc_name),
    Setting::new("oidc_auto_provision", slot!(Flag, oidc_auto_provision))
        .env("UPNEXT_OIDC_AUTO_PROVISION")
        .default("true")
        .app(check::flag),
    Setting::new("oidc_allowed_groups", slot!(OptText, oidc_allowed_groups)).env("UPNEXT_OIDC_ALLOWED_GROUPS").app(check::groups),
    Setting::new("trusted_proxies", slot!(OptText, trusted_proxies)).env("UPNEXT_TRUSTED_PROXIES").app(check::trusted_proxies),
    Setting::new("trusted_proxy_login", slot!(Flag, trusted_proxy_login))
        .env("UPNEXT_TRUSTED_PROXY_LOGIN")
        .default("false")
        .app(check::flag),
];

/// The settings that decide who gets in: set only on
/// the computer UpNext runs on, or in its environment.
pub const SIGN_IN_KEYS: [&str; 8] = [
    "oidc_issuer",
    "oidc_client_id",
    "oidc_client_secret",
    "oidc_name",
    "oidc_auto_provision",
    "oidc_allowed_groups",
    "trusted_proxies",
    "trusted_proxy_login",
];

/// A setting the app can change, by key.
pub fn in_app_setting(key: &str) -> Result<&'static Setting> {
    SETTINGS.iter().find(|s| s.key == key && s.in_app).ok_or_else(|| {
        let known: Vec<&str> = SETTINGS.iter().filter(|s| s.in_app).map(|s| s.key).collect();
        anyhow::anyhow!("unknown setting {key:?}; known: {}", known.join(", "))
    })
}

/// Check and normalize a value for an in-app setting.
pub fn validate_setting(key: &str, value: &str) -> Result<String> {
    checked(in_app_setting(key)?, value)
}

/// Check and normalize a value for a store connection's setting (the keys
/// in [`crate::db::CONNECTION_KEYS`]). Most are in-app settings too, but a
/// connection's Heroic folder and Epic manifests folder are set only on the
/// connection (the app has them from the environment alone), so the in-app
/// lookup would refuse them.
pub fn validate_connection_setting(key: &str, value: &str) -> Result<String> {
    let s = SETTINGS
        .iter()
        .find(|s| s.key == key && crate::db::CONNECTION_KEYS.iter().any(|(k, _)| *k == key))
        .ok_or_else(|| anyhow::anyhow!("{key:?} isn't a store connection's setting"))?;
    checked(s, value)
}

fn checked(s: &Setting, value: &str) -> Result<String> {
    let (key, v) = (s.key, value.trim());
    if !s.choices.is_empty() && !s.choices.contains(&v) {
        bail!("{key} must be one of: {}", s.choices.join(", "));
    }
    (s.check)(key, v)
}

/// The keys of secret settings (left out of database exports).
pub fn secret_keys() -> impl Iterator<Item = &'static str> {
    SETTINGS.iter().filter(|s| s.secret).map(|s| s.key)
}

/// The keys of settings that name a file or folder (left out of a copy
/// without keys, where they would show the computer's user and folders).
pub fn path_keys() -> impl Iterator<Item = &'static str> {
    SETTINGS.iter().filter(|s| matches!(s.slot, Slot::Path(..) | Slot::OptPath(..))).map(|s| s.key)
}

impl Config {
    /// Put a setting back to its built-in value, as if nothing set it.
    pub(super) fn unset(&mut self, key: &str) {
        let Some(s) = SETTINGS.iter().find(|s| s.key == key) else { return };
        match s.slot {
            Slot::OptText(_, f) => *f(self) = None,
            Slot::OptPath(_, f) => *f(self) = None,
            Slot::Model(_, f) => *f(self) = None,
            Slot::Flag(_, f) => *f(self) = false,
            _ => {}
        }
        if !s.default.is_empty() {
            let _ = s.set(self, s.default);
        }
        self.sources.remove(s.key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_in_values_are_checked() {
        assert_eq!(
            validate_setting("oidc_issuer", "https://sso.den.example.org/application/o/upnext/").unwrap(),
            "https://sso.den.example.org/application/o/upnext/"
        );
        assert!(validate_setting("oidc_issuer", "http://sso.den.lan:9091").is_ok(), "plain http on the network");
        for bad in ["http://sso.example.org", "https://sso.den.example.org/?x=1", "sso.den.example.org", "ftp://den.lan"] {
            assert!(validate_setting("oidc_issuer", bad).is_err(), "{bad}");
        }
        assert_eq!(validate_setting("oidc_allowed_groups", " games, family ,games,").unwrap(), "games,family");
        assert_eq!(validate_setting("trusted_proxies", "127.0.0.1 172.16.0.0/12").unwrap(), "127.0.0.1, 172.16.0.0/12");
        assert!(validate_setting("trusted_proxies", "0.0.0.0/0").is_err());
        assert_eq!(validate_setting("oidc_name", "  Authelia ").unwrap(), "Authelia");
        let c = Config::built_in();
        assert!(
            c.oidc_auto_provision && !c.trusted_proxy_login && c.oidc_issuer.is_none() && c.trusted_proxies.is_none(),
            "off by default"
        );
    }

    #[test]
    fn table_is_consistent() {
        let keys: Vec<&str> = SETTINGS.iter().map(|s| s.key).collect();
        let mut unique = keys.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), keys.len(), "no key twice");
        // The keys the app could set before the table, and the ones exports drop.
        let in_app: Vec<&str> = SETTINGS.iter().filter(|s| s.in_app).map(|s| s.key).collect();
        assert_eq!(in_app.len(), 69);
        assert_eq!(
            secret_keys().collect::<Vec<_>>(),
            [
                "steam_api_key",
                "anthropic_api_key",
                "openai_key",
                "openai_api_key",
                "claude_oauth_token",
                "igdb_client_secret",
                "rawg_api_key",
                "oidc_client_secret"
            ]
        );
        for s in SETTINGS.iter().filter(|s| !s.default.is_empty()) {
            let mut c = Config::default();
            s.set(&mut c, s.default).unwrap_or_else(|e| panic!("{}: {e}", s.key));
        }
    }

    #[test]
    fn app_values_are_checked_and_normalized() {
        assert_eq!(validate_setting("steam_api_key", " f493aaaaaaaaaaaaaaaaaaaaaaaa9be3 ").unwrap(), "F493AAAAAAAAAAAAAAAAAAAAAAAA9BE3");
        assert_eq!(validate_setting("rawg_api_key", "ABCDEF0123456789ABCDEF0123456789").unwrap(), "abcdef0123456789abcdef0123456789");
        assert_eq!(validate_setting("openai_base_url", "http://box:11434/v1/").unwrap(), "http://box:11434/v1");
        assert_eq!(validate_setting("verdicts_url", "https://verdicts.example.org/").unwrap(), "https://verdicts.example.org");
        assert_eq!(validate_setting("verdicts_url", "http://127.0.0.1:8789/v").unwrap(), "http://127.0.0.1:8789/v");
        assert_eq!(validate_setting("verdicts_url", "http://localhost:8789").unwrap(), "http://localhost:8789");
        for bad in [
            "verdicts.example.org",
            "ftp://verdicts.example.org",
            "https://verdicts.example.org/?a=1",
            "https://",
            "http://verdicts.example.org",
            "http://192.168.0.42:8789",
        ] {
            assert!(validate_setting("verdicts_url", bad).is_err(), "{bad}");
        }
        assert_eq!(validate_setting("auto_sync", "on").unwrap(), "true");
        assert_eq!(validate_setting("theme", "ember").unwrap(), "ember");
        assert_eq!(validate_setting("theme", "neon").unwrap_err().to_string(), "theme must be one of: arcade, ember, daylight");
        assert_eq!(validate_setting("auto_sync", "maybe").unwrap_err().to_string(), "auto_sync must be true or false");
        assert!(validate_setting("sampled_minutes", "0").is_err());
        assert_eq!(validate_setting("openai_key", " sk-proj-abcdefghijklmnop ").unwrap(), "sk-proj-abcdefghijklmnop");
        assert!(validate_setting("openai_key", "sk-ant-api03-abcdefghijklmnop").unwrap_err().to_string().contains("Anthropic"));
        assert!(validate_setting("openai_key", "abcdefghijklmnopqrstuvwxyz").is_err());
        assert!(validate_setting("openai_key_effort", "ultra").is_err(), "a Codex effort the API doesn't take");
        assert_eq!(validate_setting("window_size", "1440 × 900").unwrap(), "1440x900");
        assert!(validate_setting("window_size", "1440").is_err());
        assert!(validate_setting("window_size", "100x900").is_err(), "too small to use");
        assert_eq!(validate_setting("window_zoom", "125%").unwrap(), "125");
        assert!(validate_setting("window_zoom", "1000").is_err());
        assert_eq!(validate_setting("galaxy_skip", " Origin, xbox,origin, EA ").unwrap(), "origin,xboxone");
        assert_eq!(validate_setting("galaxy_skip", "").unwrap(), "");
        assert!(validate_setting("openai_thinking", "sometimes").is_err());
        assert!(validate_setting("galaxy_skip", "gog").is_err(), "GOG's own games are always read");
        assert!(validate_setting("galaxy_skip", "ea/app").is_err());
        assert!(validate_setting("db_path", "/tmp/x.db").unwrap_err().to_string().starts_with("unknown setting \"db_path\""), "env-only");
    }

    /// A flag from the environment reads true/false, 1/0, on/off and yes/no
    /// in any case, and anything else is an error, as `UPNEXT_PORT=abc` is,
    /// rather than a quiet "off" that Settings says came from the
    /// environment. A choice it doesn't know is an error too, not dropped
    /// for the default (B-CORE-4).
    #[test]
    fn flags_and_choices_from_the_environment_are_read_or_refused() {
        let setting = |key: &str| SETTINGS.iter().find(|s| s.key == key).unwrap();
        for (v, on) in
            [("yes", true), ("YES", true), ("True", true), ("on", true), ("1", true), ("no", false), ("Off", false), ("0", false)]
        {
            let mut c = Config::built_in();
            c.trusted_proxy_login = !on;
            assert!(setting("trusted_proxy_login").set(&mut c, v).unwrap(), "{v}");
            assert_eq!(c.trusted_proxy_login, on, "UPNEXT_TRUSTED_PROXY_LOGIN={v}");
            assert_eq!(check::flag("auto_sync", v).unwrap(), on.to_string(), "the app takes the same spellings");
        }
        let mut c = Config::built_in();
        let e = setting("trusted_proxy_login").set(&mut c, "maybe").unwrap_err();
        assert_eq!(e.to_string(), format!("must be {FLAG_SPELLINGS}"));
        assert!(!c.trusted_proxy_login);

        let e = setting("llm_backend").set(&mut c, "api_key").unwrap_err();
        assert!(e.to_string().starts_with("must be one of claude-cli, "), "{e}");
        assert!(e.to_string().contains("api-key"), "{e}");
        assert_eq!(c.llm_backend, "claude-cli", "unchanged");
        assert!(setting("llm_backend").set(&mut c, "api-key").unwrap());
        assert_eq!(c.llm_backend, "api-key");

        // The effort settings are optional text, but take only their choices too.
        for (key, bad) in [("codex_effort", "bogus"), ("openai_key_effort", "none")] {
            let e = setting(key).set(&mut c, bad).unwrap_err();
            assert!(e.to_string().starts_with("must be one of low, medium, high, xhigh, max"), "{key}: {e}");
            assert!(setting(key).show(&c).is_none(), "{key} unchanged");
            assert!(setting(key).set(&mut c, "high").unwrap());
            assert_eq!(setting(key).show(&c).as_deref(), Some("high"));
        }
        assert!(setting("codex_effort").set(&mut c, "ultra").unwrap(), "Codex's own top effort");
        assert!(setting("openai_key_effort").set(&mut c, "ultra").is_err(), "not the API's");
    }
}
