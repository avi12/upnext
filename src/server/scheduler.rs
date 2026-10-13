//! The scheduler: while the server runs, it starts whatever task is due,
//! reads the play diary, sends queued backlog verdicts again and keeps the
//! AI's list prices current.

use super::AppState;
use crate::config::Config;
use crate::db::JobSource;
use crate::jobs::tasks::{self, Slot};
use anyhow::Result;

/// While the server runs: two minutes after it starts, and every 30
/// seconds after that, start whatever task is due (`jobs::tasks`). A run
/// missed while the server was stopped happens then. The AI's list prices
/// are read again when they're a week old.
pub(super) async fn scheduler(state: AppState) {
    tokio::time::sleep(std::time::Duration::from_secs(120)).await;
    loop {
        if let Err(e) = schedule_tick(&state, crate::db::now()) {
            tracing::warn!("the scheduler couldn't check what's due: {e:#}");
        }
        diary_tick(&state, crate::db::now()).await;
        verdicts_tick(&state, crate::db::now()).await;
        // Every player's AI, since each may use their own.
        let mut providers = vec![];
        for player in state.db.players().unwrap_or_default() {
            for p in state.priced_providers(player.id) {
                if !providers.contains(&p) {
                    providers.push(p);
                }
            }
        }
        for p in providers {
            crate::prices::refresh_if_stale(&state.db, &state.clients.http, p, state.clients.base_url.as_deref()).await;
        }
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}

/// The play diary's hourly reading from Steam, while it's on,
/// schedules are on and Steam's Web API is set up. It isn't a job: it takes
/// one call, and it runs while a long job holds the slot.
async fn diary_tick(state: &AppState, now: i64) {
    let cfg = state.cfg();
    if !cfg.play_diary || !cfg.auto_sync || cfg.steam_credentials().is_err() {
        return;
    }
    match crate::taste::diary::due(&state.db, now) {
        Ok(true) => {
            if let Err(e) = crate::taste::diary::poll(&state.db, &cfg, &state.clients, now).await {
                tracing::warn!("the play diary couldn't read Steam: {e:#}");
            }
        }
        Ok(false) => {}
        Err(e) => tracing::warn!("the play diary couldn't check when it last read Steam: {e:#}"),
    }
}

/// Backlog verdicts the service was busy for or out of reach when they were
/// sent, sent again once their time comes. It isn't a job: the
/// player already pressed Send, and it takes a call per verdict.
async fn verdicts_tick(state: &AppState, now: i64) {
    let cfg = state.cfg();
    if let Err(e) = crate::verdicts::mine::retry_due(&state.db, &state.clients.http, &cfg, now).await {
        tracing::warn!("couldn't send the queued backlog verdicts: {e:#}");
    }
}

/// Whether a task's schedule may start it: with the wishlist off, its task
/// never runs by itself, nor the backlog verdicts' with receiving
/// off or no service set.
pub(crate) fn runs_by_itself(cfg: &Config, task: crate::jobs::Task) -> bool {
    match task {
        crate::jobs::Task::Wishlist => cfg.wishlist,
        crate::jobs::Task::Verdicts => crate::verdicts::receive::off(cfg).is_none(),
        _ => true,
    }
}

/// One look at the schedules: start the first task that's due, unless
/// schedules are off (`auto_sync`), there's no library yet (the first sync
/// is the person's call), a job is running (it's tried again next time), or
/// the setup wizard holds them while its data-source keys are still to come.
/// The job it started, if any.
pub(super) fn schedule_tick(state: &AppState, now: i64) -> Result<Option<crate::db::Job>> {
    let cfg = state.library_cfg();
    if state.jobs.closing()
        || !cfg.auto_sync
        || state.db.meta_get("last_sync")?.is_none()
        || state.db.job_running()?.is_some()
        || state.setup_sync.holds(now)
    {
        return Ok(None);
    }
    let setups = tasks::setups(&state.db)?;
    let runs = state.db.task_runs()?;
    // With the wishlist off, its task never runs by itself: off
    // for every player.
    let mut wishlist = false;
    for p in state.db.players()? {
        wishlist |= crate::wishlist::on_for(&state.db, &cfg, p.id)?;
    }
    let slots: Vec<Slot> = setups
        .iter()
        .filter(|s| if s.task == crate::jobs::Task::Wishlist { wishlist } else { runs_by_itself(&cfg, s.task) })
        .map(|s| Slot::new(s, runs.get(s.task.id())))
        .collect();
    let Some(setup) = tasks::due(&slots, now).and_then(|t| setups.iter().find(|s| s.task == t)) else { return Ok(None) };
    let started = state.jobs.start(
        setup.job(&cfg, false),
        Some(setup.task),
        JobSource::Schedule,
        state.db.clone(),
        cfg,
        state.clients.clone(),
        state.library_llm(),
    )?;
    match started {
        Ok(job) => {
            tracing::info!("{} started on its schedule", setup.task.label());
            Ok(Some(job))
        }
        // Something else started in between.
        Err(_) => Ok(None),
    }
}
