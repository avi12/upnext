//! The setup wizard's work in the background: the first sync starts
//! when the person leaves the store steps, Fetch details once they've passed
//! the data-source keys, and UpNext's shared data is applied when the first
//! sync ends, whatever page is open by then. The wizard says what it wants
//! (`POST /setup/background`) and one task here starts each in turn as the
//! job slot frees, so the person carries on through the AI steps while the
//! jobs run.
//!
//! What was asked lives in memory only. A restart forgets it, and the
//! schedules then carry on as they always do: Fetch details follows a sync.

use super::{ApiResult, AppState};
use crate::db::{JobSource, JobStatus, PlayerId};
use crate::jobs::{Task, tasks};
use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

/// How often the follower looks at the job slot.
const TICK: Duration = Duration::from_millis(500);

/// Looks in a row that fail (the database can't be read, say) before the
/// follower stops, so an error that doesn't go away can't keep it spinning.
/// The next ask starts it again.
const MAX_ERRORS: u32 = 20;

/// The longest the schedules wait for the wizard's word on Fetch details:
/// someone who leaves the wizard on the keys step gets them back after an
/// hour.
const HOLD_SECS: i64 = 3600;

/// A sync the wizard asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SyncAsk {
    /// Sync, unless the wizard's last sync read these same accounts and is
    /// running, waiting to start, or brought games.
    IfChanged,
    /// Sync, or sync again once the running job ends.
    Again,
}

/// What the wizard says about Fetch details.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DetailsAsk {
    /// Not yet: the data-source keys are still to come, so on a first run
    /// the schedules wait instead of starting it after the sync without them.
    Wait,
    /// Fetch details once the sync ends, or now when none is running or
    /// waiting.
    AfterSync,
}

/// What the wizard wants run; every field is optional.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[schema(as = SetupBackgroundAsk)]
pub struct BackgroundAsk {
    pub sync: Option<SyncAsk>,
    pub details: Option<DetailsAsk>,
    /// Apply UpNext's shared data when the first sync ends (the default), or not.
    pub seed: Option<bool>,
}

/// Where UpNext's shared data stands for this setup.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SeedState {
    /// No sync asked for yet.
    #[default]
    Idle,
    /// Applied when the first sync ends with games.
    Waiting,
    Applying,
    Applied,
    /// The apply failed; `error` says why.
    Failed,
    /// Not applied: the library had games already (not a first sync),
    /// shared data is kept already (each sync applies it), it was turned
    /// off, or the newest release has none or couldn't be reached.
    Skipped,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct SeedProgress {
    /// Applied after the first sync: on unless the person turned it off.
    pub wanted: bool,
    pub state: SeedState,
    /// Summaries written, once applied.
    pub written: Option<usize>,
    pub error: Option<String>,
}

/// What the wizard's background work is waiting on.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct SetupBackground {
    /// A sync starts once the running job ends: an account changed while a
    /// sync ran, or another job holds the slot.
    pub sync_waiting: bool,
    /// Fetch details starts once the sync ends.
    pub details_waiting: bool,
    pub seed: SeedProgress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Details {
    /// Held since this time (Unix seconds).
    Wait(i64),
    AfterSync,
}

#[derive(Default)]
struct Plan {
    /// A sync waits for the slot.
    sync: bool,
    /// The accounts the wizard's last sync read (a hash, never shown or
    /// kept), and its job.
    synced: Option<(u64, i64)>,
    details: Option<Details>,
    /// The Fetch details the wizard started, which an account changed
    /// later stops so the new sync needn't wait for it.
    details_job: Option<i64>,
    seed_off: bool,
    seed: SeedState,
    /// The sync the shared data waits for.
    seed_after: Option<i64>,
    seed_written: Option<usize>,
    seed_error: Option<String>,
    /// The follower task is running.
    following: bool,
}

/// The server's side of it, in `AppState`.
#[derive(Default)]
pub struct WizardJobs {
    plan: Mutex<Plan>,
    /// Where the shared data comes from: the newest release's, unless a
    /// test points it at a mock.
    seed_url: Mutex<Option<String>>,
}

impl WizardJobs {
    fn plan(&self) -> MutexGuard<'_, Plan> {
        self.plan.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Whether the schedules wait: the wizard has a sync going and said
    /// Fetch details waits for the keys, within the last hour.
    pub fn holds(&self, now: i64) -> bool {
        matches!(self.plan().details, Some(Details::Wait(since)) if now < since + HOLD_SECS)
    }

    /// Setup ended (done or skipped): the schedules decide again.
    pub fn release(&self) {
        let mut p = self.plan();
        if matches!(p.details, Some(Details::Wait(_))) {
            p.details = None;
        }
    }

    pub fn now(&self) -> SetupBackground {
        let p = self.plan();
        SetupBackground {
            sync_waiting: p.sync,
            details_waiting: p.details == Some(Details::AfterSync),
            seed: SeedProgress { wanted: !p.seed_off, state: p.seed, written: p.seed_written, error: p.seed_error.clone() },
        }
    }

    #[cfg(test)]
    pub fn use_seed_url(&self, url: String) {
        *self.seed_url.lock().unwrap() = Some(url);
    }
    #[cfg(test)]
    pub fn following(&self) -> bool {
        self.plan().following
    }
}

/// The accounts and folders a sync reads, as one number: a different one
/// means a sync would read something new.
fn accounts(state: &AppState) -> u64 {
    let cfg = state.cfg();
    let gog = state.clients.gog.status();
    let blocked: Vec<_> = crate::enrich::sync_libraries().iter().map(|l| (l.id(), l.blocked(&cfg, &state.clients).is_some())).collect();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (&cfg.steam_id, &cfg.steam_api_key, &cfg.steam_dir, &cfg.heroic_dir, &cfg.galaxy_db, &cfg.galaxy_skip).hash(&mut h);
    let epic = state.clients.epic.status();
    (cfg.steam_include_hidden, gog.connected, &gog.user_id, epic.connected, &epic.display_name, blocked).hash(&mut h);
    h.finish()
}

#[utoipa::path(post, path = "/setup/background", tag = "setup",
    description = "What the setup wizard wants run in the background: a sync (`if_changed` when the person leaves \
        the store steps, `again` from its button), Fetch details (`wait` while the data-source keys are still to come, \
        which on a first run, while setup is neither done nor skipped, holds the schedules for up to an hour; \
        `after_sync` once they're past), and UpNext's shared data after the first sync (on unless `seed` is false). Each starts as the job slot frees, in that order. Answers what's waiting.",
    request_body = BackgroundAsk,
    responses((status = 200, body = SetupBackground)))]
pub(super) async fn background(State(state): State<AppState>, Json(ask): Json<BackgroundAsk>) -> ApiResult<SetupBackground> {
    let accounts = accounts(&state);
    let titles = state.db.stats(PlayerId::FIRST)?.total;
    let setup_open = super::setup::setup_open(&state)?;
    let follow = {
        let mut p = state.setup_sync.plan();
        if let Some(on) = ask.seed {
            p.seed_off = !on;
        }
        if let Some(sync) = ask.sync {
            let read = |id: i64| -> anyhow::Result<bool> {
                Ok(state.db.job(id)?.is_some_and(|j| j.status == JobStatus::Running || (j.status == JobStatus::Done && titles > 0)))
            };
            let same = match p.synced {
                Some((acc, id)) => acc == accounts && read(id)?,
                None => false,
            };
            if sync == SyncAsk::Again || !(p.sync || same) {
                p.sync = true;
            }
            // An empty library makes this a first sync, which the shared
            // data follows; a library with games doesn't get it unasked.
            if titles == 0 && !matches!(p.seed, SeedState::Waiting | SeedState::Applying) {
                (p.seed, p.seed_written, p.seed_error) = (SeedState::Waiting, None, None);
            } else if titles > 0 && p.seed == SeedState::Idle {
                p.seed = SeedState::Skipped;
            }
        }
        match ask.details {
            // Only a first run holds the schedules: a library that has run
            // them before (setup done or skipped, or the wizard restarted
            // from Settings) keeps them, so a stray ask can't stop them.
            Some(DetailsAsk::Wait) if p.details != Some(Details::AfterSync) && setup_open => {
                p.details = Some(Details::Wait(crate::db::now()))
            }
            Some(DetailsAsk::AfterSync) => p.details = Some(Details::AfterSync),
            _ => {}
        }
        let work = p.sync || p.details == Some(Details::AfterSync) || p.seed_after.is_some();
        let follow = work && !p.following;
        if follow {
            p.following = true;
        }
        follow
    };
    if follow {
        tokio::spawn(run(state.clone(), TICK, next));
    }
    Ok(Json(state.setup_sync.now()))
}

pub(super) enum Step {
    Wait,
    Seed,
    Done,
}

/// Start what was asked, in order, until nothing is left: `next` every
/// `tick`. After `MAX_ERRORS` failed looks in a row it stops with one
/// warning, leaving what was asked for the next ask to pick up.
async fn run(state: AppState, tick: Duration, next: fn(&AppState) -> anyhow::Result<Step>) {
    let mut errors = 0;
    loop {
        if state.jobs.closing() {
            state.setup_sync.plan().following = false;
            return;
        }
        match next(&state) {
            Ok(Step::Done) => return,
            Ok(Step::Wait) => errors = 0,
            Ok(Step::Seed) => {
                errors = 0;
                apply_seed(&state).await
            }
            Err(e) => {
                errors += 1;
                if errors >= MAX_ERRORS {
                    tracing::warn!("the setup wizard's background work stopped: {e:#}");
                    state.setup_sync.plan().following = false;
                    return;
                }
            }
        }
        tokio::time::sleep(tick).await;
    }
}

/// Run the follower with `next` in place of the real look, as the route
/// starts it, with no wait between looks.
#[cfg(test)]
pub(super) async fn follow_with(state: AppState, next: fn(&AppState) -> anyhow::Result<Step>) {
    state.setup_sync.plan().following = true;
    run(state, Duration::ZERO, next).await
}

/// One look: start the next thing when the slot is free. `Done` (with the
/// plan marked so under the same lock, so an ask that comes in now starts a
/// new follower) when nothing is left.
fn next(state: &AppState) -> anyhow::Result<Step> {
    let running = state.db.job_running()?;
    let mut p = state.setup_sync.plan();
    if let Some(job) = running {
        // An account changed while the wizard's own Fetch details runs:
        // stop it, sync, and fetch the rest after (it carries on where it
        // stopped), rather than have the sync wait for every game.
        if p.sync && p.details_job == Some(job.id) {
            state.jobs.cancel(&state.db)?;
            (p.details, p.details_job) = (Some(Details::AfterSync), None);
        }
        return Ok(Step::Wait);
    }
    // The first sync ended: the shared data comes before anything else, so
    // Fetch details doesn't ask for what it has.
    if let Some(id) = p.seed_after
        && let Some(job) = state.db.job(id)?
    {
        p.seed_after = None;
        if job.status == JobStatus::Done && p.seed == SeedState::Waiting && state.db.stats(PlayerId::FIRST)?.total > 0 {
            if p.seed_off || crate::seed::kept(&state.cfg()).is_some() {
                p.seed = SeedState::Skipped;
            } else {
                p.seed = SeedState::Applying;
                return Ok(Step::Seed);
            }
        }
        // A sync that failed or brought nothing leaves it for the next one.
    }
    let start = |task: Task| -> anyhow::Result<Option<i64>> {
        let cfg = state.library_cfg();
        let setups = tasks::setups(&state.db)?;
        let Some(setup) = setups.iter().find(|s| s.task == task) else { return Ok(None) };
        let started = state.jobs.start(
            setup.job(&cfg, false),
            Some(task),
            JobSource::Ui,
            state.db.clone(),
            cfg,
            state.clients.clone(),
            state.library_llm(),
        )?;
        // Something else took the slot in between: try again next time.
        Ok(started.ok().map(|j| j.id))
    };
    if p.sync {
        let accounts = accounts(state);
        if let Some(id) = start(Task::Sync)? {
            p.sync = false;
            p.synced = Some((accounts, id));
            if p.seed == SeedState::Waiting {
                p.seed_after = Some(id);
            }
        }
        return Ok(Step::Wait);
    }
    if p.details == Some(Details::AfterSync) {
        if state.db.stats(PlayerId::FIRST)?.unenriched == 0 {
            // No games, or every game has its details.
            p.details = None;
        } else if let Some(id) = start(Task::Details)? {
            (p.details, p.details_job) = (None, Some(id));
        }
        return Ok(Step::Wait);
    }
    p.following = false;
    Ok(Step::Done)
}

async fn apply_seed(state: &AppState) {
    let url = state.setup_sync.seed_url.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let url = url.unwrap_or_else(|| crate::seed::DEFAULT_URL.into());
    let done = super::seed::apply_offered(state, &url).await;
    let mut p = state.setup_sync.plan();
    match done {
        Ok(Some(applied)) => (p.seed, p.seed_written) = (SeedState::Applied, Some(applied.written)),
        Ok(None) => p.seed = SeedState::Skipped,
        Err(e) => {
            tracing::warn!("UpNext's shared data wasn't applied after the first sync: {e:#}");
            (p.seed, p.seed_error) = (SeedState::Failed, Some(format!("{e:#}")));
        }
    }
}
