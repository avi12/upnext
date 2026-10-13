//! Jobs (docs/api/jobs.md and job-routes.md): starting, listing,
//! cancelling and retrying them, a job's log, the scheduled tasks' table,
//! and the server's recent warnings and errors.

use super::players::Me;
use super::{ApiError, ApiResult, AppState, access, refused};
use crate::db::{JobSource, JobStatus, Level};
use crate::enrich::{EnrichOptions, SyncOptions};
use crate::jobs::tasks::{self, Slot, Task};
use crate::jobs::{self, BackupOptions, JobKind};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    use axum::routing::{get, post, put};
    Router::new()
        .route("/jobs", get(jobs_get).post(jobs_post))
        .route("/jobs/scheduled", get(scheduled_get))
        .route("/jobs/scheduled/{task}", put(scheduled_put))
        .route("/jobs/current/cancel", post(jobs_cancel))
        .route("/jobs/{id}", get(job_get))
        .route("/jobs/{id}/log.txt", get(job_log_text))
        .route("/jobs/{id}/retry-failed", post(job_retry))
        .route("/logs", get(logs_get))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct JobsQuery {
    /// Finished jobs listed, newest first (20 unless asked).
    limit: Option<usize>,
    /// "sync", "enrich", "backup", "restore", "import", "taste" or "verdicts".
    kind: Option<String>,
    /// A task's id, or "none" for jobs that weren't a task.
    task: Option<String>,
    /// Only jobs that failed or finished with failures.
    #[serde(default)]
    failed: bool,
}

#[utoipa::path(get, path = "/jobs", tag = "jobs",
    description = "The running job, from either process, and the finished ones, newest first.",
    params(JobsQuery),
    responses((status = 200, body = Object, description = "`{current: Job | null, recent: [Job]}`"),
        (status = 400, description = "An unknown kind or task")))]
async fn jobs_get(State(state): State<AppState>, Me(me): Me, Query(q): Query<JobsQuery>) -> ApiResult<Value> {
    let kind = q.kind.filter(|k| !k.is_empty());
    if let Some(k) = &kind
        && !JobKind::NAMES.contains(&k.as_str())
    {
        return Err(ApiError::bad(format!("unknown job kind {k:?}")));
    }
    let task = match q.task.as_deref() {
        None | Some("") => None,
        Some("none") => Some(None),
        Some(t) => Some(Some(Task::parse(t).ok_or_else(|| ApiError::bad(format!("unknown task {t:?}")))?.id().to_string())),
    };
    let filter = crate::db::JobFilter { kind, task, failed: q.failed, limit: q.limit.unwrap_or(20).clamp(1, 200) };
    // A game someone hid from this player reads "a hidden game".
    let names = state.db.hidden_names(me)?;
    let mut current = state.db.job_running()?;
    let mut recent = state.db.jobs_recent(&filter)?;
    for job in current.iter_mut().chain(recent.iter_mut()) {
        names.job(job);
    }
    Ok(Json(json!({ "current": current, "recent": recent })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct JobBody {
    /// "sync", "enrich" or "backup", with `options`; or a `task` instead.
    kind: Option<String>,
    #[serde(default)]
    options: Option<Value>,
    /// Run a scheduled task by hand, with its own options.
    task: Option<String>,
    /// With `task`: fetch again what titles already have.
    #[serde(default)]
    refresh: bool,
}

/// Start a job from the UI: 202 with it, or 409 saying what's running.
pub(super) fn start_job(state: &AppState, kind: JobKind, task: Option<Task>) -> Result<(StatusCode, Json<Value>), ApiError> {
    // Jobs are the household's: they run with the library's settings and AI.
    match state.jobs.start(kind, task, JobSource::Ui, state.db.clone(), state.library_cfg(), state.clients.clone(), state.library_llm())? {
        Ok(job) => Ok((StatusCode::ACCEPTED, Json(serde_json::to_value(job).unwrap_or(Value::Null)))),
        Err(running) => Err(ApiError::conflict(format!("{}; wait for it or cancel it", jobs::busy_text(&running)))),
    }
}

#[utoipa::path(post, path = "/jobs", tag = "jobs",
    description = "Start a job: a `kind` (sync, enrich, backup, taste) with `options`, or a scheduled `task` by hand.",
    request_body = JobBody,
    responses((status = 202, body = crate::db::Job),
        (status = 400, description = "An unknown kind or task, bad options, or both a task and a kind"),
        (status = 403, description = "From another device, a sync's own `galaxy_db` or a backup's `keep`"),
        (status = 409, description = "Another job is running")))]
async fn jobs_post(
    State(state): State<AppState>,
    caller: Option<axum::Extension<access::Caller>>,
    Json(body): Json<JobBody>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    // No caller means the guard didn't run: refuse what's local-only.
    let local = caller.is_some_and(|c| c.is_local());
    if let Some(id) = &body.task {
        if body.kind.is_some() || body.options.is_some() {
            return Err(ApiError::bad("give a task, or a kind with options, not both"));
        }
        let task = Task::parse(id).ok_or_else(|| ApiError::bad(format!("unknown task {id:?}")))?;
        let setups = tasks::setups(&state.db)?;
        let setup = setups.iter().find(|s| s.task == task).ok_or_else(|| ApiError::bad(format!("unknown task {id:?}")))?;
        return start_job(&state, setup.job(&state.cfg(), body.refresh), Some(task));
    }
    let Some(kind) = body.kind else { return Err(ApiError::bad("give a job kind or a task")) };
    let opts = body.options.unwrap_or(Value::Object(Default::default()));
    let kind = match kind.as_str() {
        "sync" => {
            let mut o: SyncOptions = serde_json::from_value(opts).map_err(|e| ApiError::bad(format!("bad options: {e}")))?;
            if o.galaxy_db.as_deref().is_some_and(|s| s.trim().is_empty()) {
                o.galaxy_db = None;
            }
            // A path names a file on the computer UpNext runs on, as the
            // galaxy_db setting does; from elsewhere it could ask whether any
            // file there exists. The path already set is fine.
            let set = state.cfg().galaxy_db.map(|p| p.display().to_string());
            if !local && o.galaxy_db.is_some() && o.galaxy_db != set {
                return Err(ApiError::forbidden(
                    "galaxy_db names a file, so a sync's own path is given only on the computer UpNext runs on; leave it out to read the one in Settings",
                ));
            }
            JobKind::Sync(o)
        }
        "enrich" => JobKind::Enrich(serde_json::from_value::<EnrichOptions>(opts).map_err(|e| ApiError::bad(format!("bad options: {e}")))?),
        "backup" => {
            // Fewer backups kept removes the others, and removing backups
            // is this computer's, as deleting one is.
            if !local && opts.get("keep").is_some() {
                return Err(keep_here());
            }
            JobKind::Backup(serde_json::from_value::<BackupOptions>(opts).map_err(|e| ApiError::bad(format!("bad options: {e}")))?)
        }
        "taste" => JobKind::Taste,
        "verdicts" => JobKind::Verdicts,
        other => return Err(ApiError::bad(format!("unknown job kind {other:?}"))),
    };
    start_job(&state, kind, None)
}

/// How many backups are kept decides which are removed, so, like deleting
/// one, it's chosen only on the computer UpNext runs on.
fn keep_here() -> ApiError {
    ApiError::forbidden("how many backups are kept is set only on the computer UpNext runs on")
}

/// The Jobs table: every task's schedule, options, last run and next run.
fn scheduled_json(state: &AppState) -> Result<Value, ApiError> {
    let setups = tasks::setups(&state.db)?;
    let runs = state.db.task_runs()?;
    let slots: Vec<Slot> = setups.iter().map(|s| Slot::new(s, runs.get(s.task.id()))).collect();
    let rows: Vec<Value> = setups
        .iter()
        .zip(&slots)
        .map(|(setup, slot)| {
            let task = setup.task;
            let run = runs.get(task.id());
            let next = tasks::next(slot, &slots);
            json!({
                "task": task.id(),
                "label": task.label(),
                "schedule": setup.schedule,
                "uses_ai": task.uses_ai(),
                "limit": setup.limit,
                "chosen": setup.chosen,
                "options": setup.resolved_options(),
                "keep": (task == Task::Backup).then_some(setup.keep),
                "last_run": run.and_then(|r| r.last.as_ref()),
                "next_run_at": match next { tasks::Next::At(t) => Some(t), _ => None },
                "next_after": match next { tasks::Next::After(t) => Some(t.id()), _ => None },
            })
        })
        .collect();
    Ok(json!({
        "enabled": state.cfg().auto_sync,
        "waiting_for_first_sync": state.db.meta_get("last_sync")?.is_none(),
        "tasks": rows,
    }))
}

#[utoipa::path(get, path = "/jobs/scheduled", tag = "jobs",
    description = "The Jobs table: every task's schedule, options, last run and next run.",
    responses((status = 200, body = Object, description = "`{enabled, waiting_for_first_sync, tasks: [{task, label, \
        schedule, uses_ai, limit, chosen, options, keep, last_run, next_run_at, next_after}]}`")))]
async fn scheduled_get(State(state): State<AppState>) -> ApiResult<Value> {
    Ok(Json(scheduled_json(&state)?))
}

/// Change a task's schedule, options or backups kept.
#[utoipa::path(put, path = "/jobs/scheduled/{task}", tag = "jobs",
    description = "Change a task's schedule, options, backups kept, or the most titles an AI job takes a run \
        (`limit`, 0 for none). Every field is optional; options are merged.",
    params(("task" = String, Path, description = "The task's id (`sync`, `reviews`, …)")),
    request_body = crate::jobs::tasks::Change,
    responses((status = 200, body = Object, description = "The same shape as GET /jobs/scheduled"),
        (status = 400, description = "A schedule, option or count it doesn't take"),
        (status = 403, description = "Backups kept, from another device"),
        (status = 404, description = "No such task")))]
async fn scheduled_put(
    State(state): State<AppState>,
    caller: Option<axum::Extension<access::Caller>>,
    Path(id): Path<String>,
    Json(change): Json<tasks::Change>,
) -> ApiResult<Value> {
    let task = Task::parse(&id).ok_or_else(|| ApiError::not_found(format!("no job {id:?}")))?;
    if change.keep.is_some() && !caller.is_some_and(|c| c.is_local()) {
        return Err(keep_here());
    }
    tasks::change(&state.db, task, &change).map_err(refused)?;
    Ok(Json(scheduled_json(&state)?))
}

#[utoipa::path(post, path = "/jobs/current/cancel", tag = "jobs",
    description = "Ask the running job to stop, here or in a terminal. It stops at its next check and ends cancelled.",
    responses((status = 200, body = crate::db::Job), (status = 404, description = "No job is running")))]
async fn jobs_cancel(State(state): State<AppState>, Me(me): Me) -> ApiResult<Value> {
    match state.jobs.cancel(&state.db)? {
        Some(mut j) => {
            // Masked as `GET /jobs` masks it.
            state.db.hidden_names(me)?.job(&mut j);
            Ok(Json(serde_json::to_value(j).unwrap_or(Value::Null)))
        }
        None => Err(ApiError::not_found("no job is running")),
    }
}

fn job_or_404(state: &AppState, id: i64) -> Result<crate::db::Job, ApiError> {
    state.db.job(id)?.ok_or_else(|| ApiError::not_found(format!("no job {id}")))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct JobLogQuery {
    /// Lines after this one (a line's `id`).
    after: Option<i64>,
    /// `warn` for warnings and errors, `error` for errors only.
    level: Option<String>,
    /// Lines in the page: 500 unless asked (1 to 5000).
    limit: Option<usize>,
}

/// One job, its failures, and a page of its log.
#[utoipa::path(get, path = "/jobs/{id}", tag = "jobs",
    description = "One job, its failures, and a page of its log.",
    params(("id" = i64, Path, description = "Job id"), JobLogQuery),
    responses((status = 200, body = Object, description = "The Job, plus `failures`, `can_retry`, `log` \
        (`[{id, at, level, lane, message}]`) and `log_more`"),
        (status = 400, description = "An unknown level"),
        (status = 404, description = "No such job")))]
async fn job_get(State(state): State<AppState>, Me(me): Me, Path(id): Path<i64>, Query(q): Query<JobLogQuery>) -> ApiResult<Value> {
    let mut job = job_or_404(&state, id)?;
    let min = match q.level.as_deref() {
        None | Some("") => Level::Info,
        Some(l) => Level::parse(l).ok_or_else(|| ApiError::bad(format!("unknown level {l:?}")))?,
    };
    let limit = q.limit.unwrap_or(500).clamp(1, 5000);
    let mut log = state.db.job_log(id, q.after.unwrap_or(0), min, limit + 1)?;
    let more = log.len() > limit;
    log.truncate(limit);
    let mut failures = state.db.job_failures(id)?;
    let can_retry = job.status != JobStatus::Running && jobs::retry_options(&job, &failures).is_some();
    // A game someone hid from this player reads "a hidden game",
    // wherever the job names it.
    let names = state.db.hidden_names(me)?;
    names.job(&mut job);
    names.lines(&mut log);
    names.failures(&mut failures);
    let mut v = serde_json::to_value(&job).unwrap_or(Value::Null);
    v["failures"] = json!(failures);
    v["can_retry"] = json!(can_retry);
    v["log"] = json!(log);
    v["log_more"] = json!(more);
    Ok(Json(v))
}

/// The whole log as a text file.
#[utoipa::path(get, path = "/jobs/{id}/log.txt", tag = "jobs",
    description = "A job's whole log as a text file, one line each: local time, level, [lane], message.",
    params(("id" = i64, Path, description = "Job id")),
    responses((status = 200, body = String, content_type = "text/plain"), (status = 404, description = "No such job")))]
async fn job_log_text(State(state): State<AppState>, Me(me): Me, Path(id): Path<i64>) -> Result<Response, ApiError> {
    let mut job = job_or_404(&state, id)?;
    let mut lines = state.db.job_log(id, 0, Level::Info, usize::MAX >> 1)?;
    let names = state.db.hidden_names(me)?;
    names.job(&mut job);
    names.lines(&mut lines);
    let when = |t: i64| {
        chrono::DateTime::from_timestamp(t, 0)
            .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_default()
    };
    let mut text = format!("UpNext {} job {id}, started {} {}\n", job.kind, when(job.started_at), job.source.started_from());
    for l in &lines {
        let lane = l.line.lane.as_deref().map(|n| format!("[{n}] ")).unwrap_or_default();
        text.push_str(&format!("{} {:<5} {lane}{}\n", when(l.line.at), l.line.level.as_str().to_uppercase(), l.line.message));
    }
    if let Some(e) = &job.error {
        text.push_str(&format!("error: {e}\n"));
    }
    let disposition = format!("attachment; filename=\"upnext-{}-{id}.log\"", job.kind);
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8".to_string()),
            (axum::http::header::CONTENT_DISPOSITION, disposition),
        ],
        text,
    )
        .into_response())
}

/// Run a finished enrich job's failures again: the sources that failed, for
/// the titles they failed on.
#[utoipa::path(post, path = "/jobs/{id}/retry-failed", tag = "jobs",
    description = "Run a finished enrich job's failures again: the sources that failed, for the titles they failed on.",
    params(("id" = i64, Path, description = "Job id")),
    responses((status = 202, body = crate::db::Job),
        (status = 400, description = "Nothing in that job can be run again for single titles"),
        (status = 404, description = "No such job"),
        (status = 409, description = "That job or another is running")))]
async fn job_retry(State(state): State<AppState>, Path(id): Path<i64>) -> Result<(StatusCode, Json<Value>), ApiError> {
    let job = job_or_404(&state, id)?;
    if job.status == JobStatus::Running {
        return Err(ApiError::conflict("that job is still running"));
    }
    let failures = state.db.job_failures(id)?;
    let Some(opts) = jobs::retry_options(&job, &failures) else {
        return Err(ApiError::bad("nothing in that job can be tried again for single titles"));
    };
    start_job(&state, JobKind::Enrich(opts), None)
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LogsQuery {
    /// `warn` (the default) for warnings and errors, `error` for errors only.
    level: Option<String>,
}

/// The server's recent warnings and errors, from jobs and from everything else.
#[utoipa::path(get, path = "/logs", tag = "system",
    description = "The server's recent warnings and errors (the last 300), from jobs and everything else, oldest first.",
    params(LogsQuery),
    responses((status = 200, body = Vec<crate::jobs::ServerLine>), (status = 400, description = "An unknown level")))]
async fn logs_get(State(state): State<AppState>, Me(me): Me, Query(q): Query<LogsQuery>) -> ApiResult<Vec<jobs::ServerLine>> {
    let min = match q.level.as_deref() {
        None | Some("") => Level::Warn,
        Some(l) => Level::parse(l).ok_or_else(|| ApiError::bad(format!("unknown level {l:?}")))?,
    };
    // A warning can name a game someone hid from this player.
    let names = state.db.hidden_names(me)?;
    let mut lines = jobs::recent_log(min);
    for l in &mut lines {
        names.mask(&mut l.message);
    }
    Ok(Json(lines))
}

/// A task's schedule, last run and next run, for a page that runs it.
pub(super) fn task_json(state: &AppState, task: Task) -> Result<Value, ApiError> {
    let setups = tasks::setups(&state.db)?;
    let runs = state.db.task_runs()?;
    let slots: Vec<Slot> = setups.iter().map(|s| Slot::new(s, runs.get(s.task.id()))).collect();
    let (setup, slot) = setups.iter().zip(&slots).find(|(s, _)| s.task == task).expect("every task has a setup");
    Ok(json!({
        "schedule": setup.schedule,
        "enabled": state.cfg().auto_sync,
        "last_run": runs.get(task.id()).and_then(|r| r.last.as_ref()),
        "next_run_at": match tasks::next(slot, &slots) { tasks::Next::At(t) => Some(t), _ => None },
    }))
}
