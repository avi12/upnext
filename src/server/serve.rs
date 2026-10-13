//! Serving: the address the server binds, what it says when it starts, and
//! stopping gracefully on SIGTERM or Ctrl-C.

use super::listen::{self, is_loopback_host};
use super::scheduler::scheduler;
use super::{AppState, pairing, router, setup};
use anyhow::Result;

/// How long a running job gets to stop and record where it got to when the
/// server is asked to stop.
const JOB_GRACE: std::time::Duration = std::time::Duration::from_secs(10);
/// How long the server waits, after being asked to stop, before it stops
/// whatever is still open (a slow AI call). Under systemd's 30 s in the unit.
const STOP_LIMIT: std::time::Duration = std::time::Duration::from_secs(20);

/// Serve until SIGTERM (`systemctl --user stop upnext`) or Ctrl-C, then stop
/// gracefully (`serve_until`).
pub async fn serve(state: AppState) -> Result<()> {
    // A Galaxy upload cut off by a crash leaves its work file.
    setup::sweep_galaxy_uploads(&state.cfg().db_dir());
    // "Allow other devices" may have been turned on in the app.
    let host = listen::wanted_host(&state.cfg());
    let addr = format!("{host}:{}", state.base_cfg.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    if state.base_cfg.container {
        // Nobody is this computer in a container: the pairing line instead
        // of the warning below.
        println!("UpNext is listening on {addr} in its container.");
        println!("{}", pairing::container_greeting(&state)?);
    } else {
        println!("UpNext web UI: http://{}:{}", if is_loopback_host(&host) { host.as_str() } else { "127.0.0.1" }, state.base_cfg.port);
    }
    if !is_loopback_host(&host) && !state.base_cfg.container {
        eprintln!(
            "listening on {addr}: other devices pair in Settings › Devices and API, or use a token (`upnext token create`), \
             and the Host they use must be localhost or public_url unless they send one"
        );
    }
    serve_until(listener, state, stop_signal()).await?;
    println!("UpNext stopped");
    Ok(())
}

/// Serve until `stop` resolves. Then no job starts, a running one is asked
/// to stop and given `JOB_GRACE` to record its progress, new connections are
/// refused and open requests finish. After `STOP_LIMIT`, or a second Ctrl-C,
/// it returns without waiting for the rest.
pub async fn serve_until(
    listener: tokio::net::TcpListener,
    state: AppState,
    stop: impl Future<Output = ()> + Send + 'static,
) -> Result<()> {
    let scheduler = tokio::spawn(scheduler(state.clone()));
    let jobs = state.jobs.clone();
    let (stopping, stopped) = tokio::sync::oneshot::channel::<()>();
    let wind_down = async move {
        stop.await;
        println!("UpNext stopping…");
        let _ = stopping.send(());
        scheduler.abort();
        if !jobs.shut_down(JOB_GRACE).await {
            tracing::warn!("the running job didn't stop within {} s; it is left to be marked as stopped", JOB_GRACE.as_secs());
        }
    };
    // The peer tells this computer from other devices, and HTTPS from
    // plain HTTP (`access`, `backups`).
    let listener = listen::start(listener, &state);
    let app = router(state).into_make_service_with_connect_info::<listen::Peer>();
    let server = axum::serve(listener, app).with_graceful_shutdown(wind_down).into_future();
    let cut_short = async {
        if stopped.await.is_err() {
            return std::future::pending().await;
        }
        tokio::select! {
            _ = tokio::time::sleep(STOP_LIMIT) => tracing::warn!("requests still open after {} s; stopping anyway", STOP_LIMIT.as_secs()),
            _ = tokio::signal::ctrl_c() => {}
        }
    };
    tokio::select! {
        served = server => served?,
        _ = cut_short => {}
    }
    Ok(())
}

/// SIGTERM, which systemd sends to stop a service, or Ctrl-C in a terminal.
/// On Windows, Ctrl-C, Ctrl-Break, closing the console window, signing out
/// or shutting down. After the last three Windows ends the process within
/// about 5 seconds, wherever the stop has got to; a job cut off there is
/// recorded later as stopped without finishing, as after a crash.
pub(crate) async fn stop_signal() {
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(e) => {
                tracing::warn!("can't listen for SIGTERM: {e}");
                std::future::pending::<()>().await
            }
        }
    };
    #[cfg(windows)]
    let term = async {
        use tokio::signal::windows::{ctrl_break, ctrl_close, ctrl_logoff, ctrl_shutdown};
        match (ctrl_break(), ctrl_close(), ctrl_logoff(), ctrl_shutdown()) {
            (Ok(mut brk), Ok(mut close), Ok(mut logoff), Ok(mut shutdown)) => tokio::select! {
                _ = brk.recv() => {}
                _ = close.recv() => {}
                _ = logoff.recv() => {}
                _ = shutdown.recv() => {}
            },
            _ => {
                tracing::warn!("can't listen for the console closing");
                std::future::pending::<()>().await
            }
        }
    };
    #[cfg(not(any(unix, windows)))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term => {}
    }
}
