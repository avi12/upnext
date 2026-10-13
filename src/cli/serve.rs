//! `serve`: the HTTP API and web UI.

use crate::config::Config;
use crate::db::Db;
use crate::enrich::Clients;
use crate::{jobs, server};
use anyhow::Result;
use std::sync::Arc;

/// The arguments override the configured port, host and static dir.
pub async fn serve(mut cfg: Config, port: Option<u16>, host: Option<String>, static_dir: Option<String>) -> Result<()> {
    if let Some(p) = port {
        cfg.port = p;
    }
    if let Some(h) = host {
        cfg.host = h;
    }
    if let Some(d) = static_dir {
        cfg.static_dir = Some(d.into());
    }
    let state = state(cfg)?;
    if state.base_cfg.install_claude {
        crate::llm::claude_install::start(&state.cfg(), state.claude_install.clone());
    }
    server::serve::serve(state).await
}

/// `upnext install-claude`: Claude Code from Anthropic, now, as
/// `UPNEXT_INSTALL_CLAUDE=1` installs it when the server starts.
pub async fn install_claude(cfg: &Config) -> Result<()> {
    crate::llm::claude_install::run(cfg).await
}

/// The server inside `upnext app`, on a port it already holds, until `stop`
/// (the window closing). `cfg`'s host and port are the listener's.
/// `on_request` runs when a device asks to be allowed, for the
/// window to get the person's attention.
#[cfg(feature = "desktop")]
pub(crate) async fn serve_in_app(
    mut cfg: Config,
    listener: tokio::net::TcpListener,
    stop: impl Future<Output = ()> + Send + 'static,
    on_request: server::approval::OnRequest,
) -> Result<()> {
    cfg.run_mode = "app".into();
    let state = state(cfg)?;
    state.remote.approvals.on_request(on_request);
    server::serve::serve_until(listener, state, stop).await
}

pub(crate) fn state(cfg: Config) -> Result<server::AppState> {
    let db = Arc::new(Db::open(&cfg.db_path, cfg.sampled_minutes)?);
    // AppState overlays DB settings per request; keep the base free of them
    // so a setting cleared in the app falls back to .env without a restart.
    let base_cfg = cfg.clone();
    let cfg = cfg.with_db(&db)?;
    let clients = Arc::new(Clients::new(&cfg)?);
    Ok(server::AppState {
        db,
        base_cfg,
        clients,
        jobs: Arc::new(jobs::JobRunner::new()),
        remote: Default::default(),
        claude_install: Default::default(),
        setup_sync: Default::default(),
    })
}
