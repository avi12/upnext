//! Command handlers for the `upnext` binary, one file per group of commands.
//! `main.rs` parses the arguments and calls one of these.

#[cfg(feature = "desktop")]
pub mod app;
pub mod community;
pub mod config;
#[cfg(all(windows, feature = "desktop"))]
pub mod double_click;
mod format;
pub mod gog;
pub mod jobs;
pub mod library;
pub mod migrate;
pub mod pick;
pub mod player;
pub mod running;
pub mod seed;
pub mod serve;
pub mod sync;
pub mod taste;
pub mod token;
pub mod verdicts;

use crate::config::Config;
use crate::db::{Db, PlayerId};
use anyhow::Result;

/// Logging for both binaries: warnings go to stderr, and into the log of the
/// job that raised them. In a terminal the server also prints a line per
/// request (`server::request_log`). `RUST_LOG=upnext=warn,upnext::timing=info`
/// also prints how long a library list, a pick, a sync, an enrich and each job
/// lane took, as each finishes (`time.busy` and `time.idle`). Nothing goes to
/// stdout, so `--json` output stays JSON whatever `RUST_LOG` says.
pub fn init_logging() {
    use tracing_subscriber::prelude::*;
    // Under systemd, stderr goes to the journal, which adds its own times
    // and shows no colours (`journalctl --user -u upnext`).
    let journal = std::env::var_os("JOURNAL_STREAM").is_some();
    let terminal = !journal && std::io::IsTerminal::is_terminal(&std::io::stderr());
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| default_filter(terminal).into());
    // Nor into a file or a pipe, or with NO_COLOR set.
    let ansi = format::colours(terminal);
    let fmt = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(false)
        .with_ansi(ansi)
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE);
    let fmt = if journal { fmt.without_time().boxed() } else { fmt.boxed() };
    tracing_subscriber::registry().with(fmt.with_filter(filter)).with(crate::log_layer()).init();
    crate::flatpak::log_dropped_cert_env();
}

/// What is logged when `RUST_LOG` isn't set: warnings, and in a terminal a
/// line per request too, so `upnext serve` in a console shows the
/// window's requests. A service's journal and a redirected log keep to
/// warnings, since the UI asks for something every few seconds.
fn default_filter(terminal: bool) -> &'static str {
    if terminal { "upnext=warn,upnext::request=info" } else { "upnext=warn" }
}

/// Open the database and overlay app settings (which also sets the sampled
/// threshold the status computation uses).
fn open_db(cfg: &Config) -> Result<(Db, Config)> {
    let db = Db::open(&cfg.db_path, cfg.sampled_minutes)?;
    let p = player_of(&db, cfg)?;
    let cfg = cfg.clone().with_db_for(&db, p)?;
    Ok((db, cfg))
}

/// Open the database with the settings the library's own work runs with
/// (sync, enrich, backups): the library's AI account.
fn open_library_db(cfg: &Config) -> Result<(Db, Config)> {
    let db = Db::open(&cfg.db_path, cfg.sampled_minutes)?;
    let cfg = cfg.clone().for_library(&db)?;
    Ok((db, cfg))
}

/// The player a command acts for: `--player` (or `UPNEXT_PLAYER`), by name
/// or id, else the default player. A PIN isn't asked for: whoever runs
/// commands on this computer can read its database anyway.
fn player_of(db: &Db, cfg: &Config) -> Result<PlayerId> {
    match cfg.cli_player.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(wanted) => match db.find_player(wanted)? {
            Some(p) => Ok(p.id),
            None => {
                let names: Vec<String> = db.players()?.into_iter().map(|p| p.name).collect();
                anyhow::bail!("no player {wanted:?}; the players are {}", names.join(", "))
            }
        },
        None => db.default_player(),
    }
}

#[cfg(test)]
mod tests {
    use super::default_filter;
    use tracing_subscriber::EnvFilter;

    #[test]
    fn requests_are_logged_by_default_only_in_a_terminal() {
        assert_eq!(default_filter(true), "upnext=warn,upnext::request=info");
        assert_eq!(default_filter(false), "upnext=warn");
        // Both parse; `server::request_log` logs under upnext::request.
        for terminal in [true, false] {
            EnvFilter::try_new(default_filter(terminal)).unwrap();
        }
    }
}
