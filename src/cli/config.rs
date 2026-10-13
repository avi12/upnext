//! `config list|set|unset` and `doctor`.

use super::format::{dim, green, red};
use super::open_db;
use crate::config::{self, Config};
use crate::db::Db;
use crate::enrich::Clients;
use crate::{heroic, llm, steam_local};
use anyhow::Result;

/// Every setting with its value and where it came from. Secrets are masked.
pub fn list(cfg: &Config) -> Result<()> {
    let db = Db::open(&cfg.db_path, cfg.sampled_minutes)?;
    let cfg = cfg.clone().with_db(&db)?;
    let mask = |k: &str| {
        let c: Vec<char> = k.chars().collect();
        if c.len() < 12 {
            "set".to_string()
        } else {
            format!("{}…{}", c[..4].iter().collect::<String>(), c[c.len() - 4..].iter().collect::<String>())
        }
    };
    for s in config::SETTINGS {
        let value = match s.show(&cfg) {
            Some(v) if s.secret => mask(&v),
            Some(v) => v,
            None => "-".into(),
        };
        let mut from = cfg.origin(s.key);
        if !s.in_app {
            from.push_str(", env-only");
        }
        println!("{}{value}  {}", dim(&format!("{:<22}", s.key)), dim(&format!("({from})")));
    }
    Ok(())
}

pub fn set(cfg: &Config, key: &str, value: &str) -> Result<()> {
    let db = Db::open(&cfg.db_path, cfg.sampled_minutes)?;
    let v = config::validate_setting(key, value)?;
    // A player's own keys are theirs (`--player`); the household's are everyone's.
    db.setting_set(super::player_of(&db, cfg)?, key, Some(&v))?;
    println!("{} {key} set", green("✓"));
    Ok(())
}

pub fn unset(cfg: &Config, key: &str) -> Result<()> {
    let db = Db::open(&cfg.db_path, cfg.sampled_minutes)?;
    config::in_app_setting(key)?;
    db.setting_set(super::player_of(&db, cfg)?, key, None)?;
    println!("{} {key} cleared", green("✓"));
    Ok(())
}

/// Where `doctor` says it looked for the AI CLIs on Windows, besides `PATH`
/// (`config::find_program`).
const WINDOWS_FOLDERS: &str = r"in %USERPROFILE%\.local\bin, npm's %APPDATA%\npm or the Codex app's folder";

pub async fn doctor(cfg: &Config) -> Result<()> {
    let cfg = if cfg.db_path.is_file() { cfg.clone().with_db(&Db::open(&cfg.db_path, cfg.sampled_minutes)?)? } else { cfg.clone() };
    // With the commit it was built from, so a report names the build.
    println!("{:<22}{}", "UpNext", env!("UPNEXT_VERSION"));
    paths(&cfg);
    println!(
        "{:<22}{}",
        ".env / Steam creds",
        match cfg.steam_credentials() {
            Ok((_, id)) => green(&format!("ok (STEAM_ID {} from {})", id, cfg.source("steam_id"))),
            Err(e) => red(&e.to_string()),
        }
    );
    {
        let clients = Clients::new(&cfg)?;
        println!(
            "{:<22}{}",
            "GOG account",
            match clients.gog.load_token() {
                Some(t) => green(&format!("connected as {}", t.username.unwrap_or(t.user_id))),
                None => dim("not connected (upnext gog-login)"),
            }
        );
    }
    println!(
        "{:<22}{}",
        "Steam client (local)",
        match steam_local::find_steam_dir(cfg.steam_dir.as_deref()) {
            Some(p) => green(&format!("{} (no-key fallback available)", p.display())),
            None => dim("not found"),
        }
    );
    println!(
        "{:<22}{}",
        "Heroic config",
        match heroic::find_config_dir(cfg.heroic_dir.as_deref()) {
            Some(p) => green(&p.display().to_string()),
            None => dim("not found"),
        }
    );
    println!(
        "{:<22}{}",
        "Galaxy db",
        match &cfg.galaxy_db {
            Some(p) =>
                if p.is_file() {
                    green(&p.display().to_string())
                } else {
                    red(&format!("{} (not a file)", p.display()))
                },
            // On Windows it's found by itself wherever Galaxy is installed.
            None if cfg!(windows) => dim("not found: GOG Galaxy isn't installed on this computer"),
            None => dim("not set"),
        }
    );
    let llm = llm::backend_from_config(&cfg);
    match llm.available().await {
        Ok(()) => {
            println!("{:<22}{} (model: {})", "LLM backend", green(llm.name()), cfg.pick_model().unwrap_or_else(|| "default".into()))
        }
        Err(e) => println!("{:<22}{}", "LLM backend", red(&e.to_string())),
    }
    // Summaries on a backend of their own.
    if llm::summaries_apart(&cfg) {
        let summaries = llm::summary_backend(&cfg);
        match summaries.available().await {
            Ok(()) => println!(
                "{:<22}{} (model: {})",
                "Summaries backend",
                green(summaries.name()),
                cfg.summary_model().unwrap_or_else(|| "default".into())
            ),
            Err(e) => println!("{:<22}{}", "Summaries backend", red(&e.to_string())),
        }
    }
    if cfg.db_path.is_file() {
        let (db, cfg) = open_db(&cfg)?;
        let s = db.stats(cfg.whose())?;
        println!(
            "{:<22}{}, {}, {} enriched",
            "library",
            crate::enrich::plural(s.total, "game", "games"),
            crate::enrich::plural(s.candidates, "candidate", "candidates"),
            s.enriched
        );
    }
    Ok(())
}

/// Every path UpNext uses, and where each came from: an environment
/// variable, a `.env` file, or the default.
fn paths(cfg: &Config) {
    let line = |label: &str, value: String, note: String| println!("{:<22}{value}  {}", label, dim(&format!("({note})")));
    let found = |p: &std::path::Path| if p.exists() { "" } else { ", not there yet" };
    let config_from = if std::env::var_os("UPNEXT_CONFIG_DIR").is_some() { "env UPNEXT_CONFIG_DIR" } else { "default" };
    line("config dir", cfg.config_dir.display().to_string(), config_from.into());
    let env_file = cfg.config_dir.join(".env");
    let read: Vec<String> = cfg.env_files_read.iter().map(|p| p.display().to_string()).collect();
    line(".env", env_file.display().to_string(), if read.is_empty() { "none read".into() } else { format!("read: {}", read.join(", ")) });
    line("data dir", cfg.data_dir.display().to_string(), format!("{}{}", cfg.origin("data_dir"), found(&cfg.data_dir)));
    let db_from = match cfg.source("db_path") {
        "default" => "in the data dir".to_string(),
        _ => cfg.origin("db_path"),
    };
    println!("{:<22}{} ({})  {}", "database", cfg.db_path.display(), ok_or_missing(cfg.db_path.is_file()), dim(&format!("({db_from})")));
    let token_from = match cfg.source("gog_token_path") {
        "default" | "none" => "next to the database".to_string(),
        _ => cfg.origin("gog_token_path"),
    };
    // A sign-in is kept in the database; a file here is one from before,
    // moved in the first time it's read.
    line("old GOG sign-in", cfg.gog_token_path.display().to_string(), format!("{token_from}{}", found(&cfg.gog_token_path)));
    let backups = crate::backups::dir_for(&cfg.db_path);
    line("backups", backups.display().to_string(), format!("next to the database{}", found(&backups)));
    line("cache dir", cfg.cache_dir.display().to_string(), format!("{}; the AI CLI's working directory", cfg.origin("cache_dir")));
    match &cfg.static_dir {
        Some(dir) => println!(
            "{:<22}{} ({})  {}",
            "web UI",
            dir.display(),
            ok_or_missing(dir.join("index.html").is_file()),
            dim(&format!("({})", cfg.origin("static_dir")))
        ),
        None if crate::server::ui_built_in() => println!("{:<22}{}", "web UI", green("built into the binary")),
        None => println!("{:<22}{}", "web UI", red("not built: cd web && npm ci && npm run build")),
    }
    for (label, bin) in [("claude CLI", &cfg.claude_bin), ("codex CLI", &cfg.codex_bin)] {
        match config::find_program(bin) {
            Some((p, how)) => {
                let how = match how {
                    config::Found::AsSet => "as set",
                    config::Found::OnPath => "on PATH",
                    config::Found::Usual => "not on PATH; found where its installer puts it",
                };
                // An npm install's `claude.cmd` on Windows runs what it starts.
                let launch = crate::process::Launch::of(&p.display().to_string());
                let how = match (launch.program != p, launch.batch) {
                    (true, _) if launch.node_missing() => {
                        println!(
                            "{:<22}{}",
                            label,
                            red(&format!("{}: runs {}, but node.exe wasn't found; install Node.js", p.display(), launch.describe()))
                        );
                        continue;
                    }
                    (true, _) => format!("{how}; runs {}", launch.describe()),
                    (false, true) => format!("{how}; a batch file, so the system prompt goes in a file"),
                    (false, false) => how.to_string(),
                };
                line(label, p.display().to_string(), how);
            }
            None => println!(
                "{:<22}{}",
                label,
                dim(&format!("{bin} not found on PATH or {}", if cfg!(windows) { WINDOWS_FOLDERS } else { "in ~/.local/bin" }))
            ),
        }
    }
    for hint in [super::migrate::unmoved_data(cfg), super::migrate::env_hint(cfg)].into_iter().flatten() {
        println!("{}", red(&hint));
    }
}

fn ok_or_missing(b: bool) -> String {
    if b { green("ok") } else { red("missing") }
}

#[cfg(test)]
mod tests {
    /// `--version` and doctor name the commit when the build knows it, after
    /// the version the API reports.
    #[test]
    fn the_version_names_the_build() {
        let version = env!("UPNEXT_VERSION");
        let rest = version.strip_prefix(env!("CARGO_PKG_VERSION")).expect("starts with the package version");
        assert!(rest.is_empty() || (rest.len() == 10 && rest.starts_with(" (") && rest.ends_with(')')), "{version}");
    }
}
