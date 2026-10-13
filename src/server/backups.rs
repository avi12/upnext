//! Settings › Backups: the list, back up now, downloads with and without
//! keys, restore, import and delete (docs/api/backups.md and restore.md). Download,
//! restore, import and delete answer only on this computer
//! (`this_computer_only`), whatever token another device has.
//! A backup with keys holds every secret and the token list, and a restore
//! or an import replaces the library. `/export`, a copy of the database
//! as it is without keys and sign-ins, is here too, and isn't kept to this
//! computer.

use super::access::Caller;
use super::jobs::start_job;
use super::{ApiError, ApiResult, AppState};
use crate::backups::{self, Refused};
use crate::db::{BASELINE_VERSION, SCHEMA_VERSION};
use crate::jobs::{self, ImportOptions, JobKind, RestoreOptions, tasks};
use axum::body::Body;
use axum::extract::{Path, Query, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

/// The biggest upload an import takes. A real library's database is about 40 MB.
const MAX_UPLOAD: u64 = 2 << 30;

pub fn routes() -> Router<AppState> {
    use axum::routing::{get, post};
    let local = Router::new()
        .route("/backups/{name}", axum::routing::delete(delete))
        .route("/backups/{name}/download", get(download))
        .route("/backups/{name}/restore", post(restore))
        .route("/backups/import", post(import))
        .route_layer(axum::middleware::from_fn(this_computer_only));
    Router::new().route("/backups", get(list).post(create)).route("/export", get(export_db)).merge(local)
}

/// Refuse every caller but this computer, as the access guard decided it
/// (it's the outermost layer, so it has always run). The connection's peer
/// isn't enough: behind a proxy on this computer, such as `tailscale serve`
/// or Caddy, every request comes from loopback, a phone's token included.
/// Tokens are also refused by `access::refused_for`.
async fn this_computer_only(req: Request, next: Next) -> Response {
    if !req.extensions().get::<Caller>().is_some_and(Caller::is_local) {
        let error = "backups are downloaded, restored, imported and deleted only on the computer UpNext runs on";
        return (StatusCode::FORBIDDEN, Json(json!({ "error": error }))).into_response();
    }
    next.run(req).await
}

fn refused(e: anyhow::Error) -> ApiError {
    match e.downcast::<Refused>() {
        Ok(r) => ApiError::bad(r.0),
        Err(e) => e.into(),
    }
}

fn not_found(r: Refused) -> ApiError {
    ApiError::not_found(r.0)
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> anyhow::Result<T> {
    tokio::task::spawn_blocking(f).await.map_err(anyhow::Error::from)?
}

/// The backups folder and what's in it.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Backups {
    /// The backups folder on the computer UpNext runs on.
    #[schema(value_type = String)]
    pub folder: std::path::PathBuf,
    /// The newest schema this build reads; a backup above it can't be restored.
    pub schema: i64,
    /// The oldest schema a restore takes, from UpNext's first release; a
    /// backup from 1 to below it is from a development build.
    pub oldest: i64,
    /// Backups the backup job keeps (Settings › Jobs).
    pub keep_scheduled: usize,
    /// Backups of each automatic kind kept: before an upgrade, a restore or an import.
    pub keep_automatic: usize,
    /// Newest first.
    pub backups: Vec<backups::Backup>,
}

#[utoipa::path(get, path = "/backups", tag = "backups", operation_id = "backups_list",
    description = "The backups in the folder, newest first, with how many of each kind are kept.",
    responses((status = 200, body = Backups)))]
async fn list(State(state): State<AppState>) -> ApiResult<Backups> {
    let dir = jobs::backup::backups_dir(&state.cfg());
    let keep = tasks::setups(&state.db)?.iter().find(|s| s.task == jobs::Task::Backup).map_or(tasks::DEFAULT_KEEP, |s| s.keep);
    let list = {
        let dir = dir.clone();
        blocking(move || backups::list(&dir)).await?
    };
    Ok(Json(Backups {
        folder: std::path::absolute(&dir).unwrap_or(dir),
        schema: SCHEMA_VERSION,
        oldest: BASELINE_VERSION,
        keep_scheduled: keep,
        keep_automatic: backups::KEEP_AUTOMATIC,
        backups: list,
    }))
}

#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[schema(as = NewBackupBody)]
pub struct CreateBody {
    note: Option<String>,
}

/// Back up now: a backup kept until it's deleted, made at once, even while
/// a job runs (it reads a snapshot).
#[utoipa::path(post, path = "/backups", tag = "backups", operation_id = "backups_create",
    description = "Back up now: a backup kept until it's deleted, made at once, even while a job runs.",
    request_body = Option<CreateBody>,
    responses((status = 201, body = Object, description = "The new backup, as listed"),
        (status = 400, description = "A note over 300 characters")))]
async fn create(State(state): State<AppState>, body: Option<Json<CreateBody>>) -> Result<(StatusCode, Json<Value>), ApiError> {
    let note = body.and_then(|Json(b)| b.note);
    if note.as_ref().is_some_and(|n| n.chars().count() > 300) {
        return Err(ApiError::bad("keep the note under 300 characters"));
    }
    let cfg = state.cfg();
    let dir = jobs::backup::backups_dir(&cfg);
    let made = blocking(move || jobs::backup::manual(&cfg, note.as_deref())).await?;
    let entry = blocking(move || backups::list(&dir)).await?.into_iter().find(|b| b.name == made.name);
    Ok((StatusCode::CREATED, Json(serde_json::to_value(entry).unwrap_or(Value::Null))))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct DownloadQuery {
    /// Everything, keys and sign-ins included; without them unless asked.
    #[serde(default)]
    keys: bool,
}

#[utoipa::path(get, path = "/backups/{name}/download", tag = "backups", operation_id = "backups_download",
    description = "Download a backup, without keys and sign-ins unless `keys=true`. Only on the computer UpNext runs on.",
    params(("name" = String, Path, description = "The backup's file name"), DownloadQuery),
    responses((status = 200, description = "The database file", content_type = "application/vnd.sqlite3", body = Vec<u8>),
        (status = 403, description = "From another device"), (status = 404, description = "No such backup")))]
async fn download(State(state): State<AppState>, Path(name): Path<String>, Query(q): Query<DownloadQuery>) -> Result<Response, ApiError> {
    let dir = jobs::backup::backups_dir(&state.cfg());
    let path = backups::find(&dir, &name).map_err(not_found)?;
    let (bytes, file_name) = if q.keys {
        (tokio::fs::read(&path).await.map_err(anyhow::Error::from)?, name)
    } else {
        let out = std::env::temp_dir().join(format!("upnext-keyless-{}.db", uuid::Uuid::new_v4().simple()));
        let bytes = {
            let out = out.clone();
            blocking(move || {
                let made = backups::keyless_copy(&path, &out).and_then(|()| Ok(std::fs::read(&out)?));
                let _ = std::fs::remove_file(&out);
                made
            })
            .await?
        };
        (bytes, format!("{}-without-keys.db", name.trim_end_matches(".db")))
    };
    Ok((
        [
            (header::CONTENT_TYPE, "application/vnd.sqlite3".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{file_name}\"")),
        ],
        bytes,
    )
        .into_response())
}

/// Start restoring a backup: 202 with the job, 409 while another runs, 400
/// for a file this build can't restore.
#[utoipa::path(post, path = "/backups/{name}/restore", tag = "backups", operation_id = "backups_restore",
    description = "Start restoring a backup. Only on the computer UpNext runs on.",
    params(("name" = String, Path, description = "The backup's file name")),
    responses((status = 202, body = crate::db::Job),
        (status = 400, description = "Not a database, or from a newer UpNext or from before its first release"),
        (status = 403, description = "From another device"), (status = 404, description = "No such backup"),
        (status = 409, description = "Another job is running")))]
async fn restore(State(state): State<AppState>, Path(name): Path<String>) -> Result<(StatusCode, Json<Value>), ApiError> {
    let dir = jobs::backup::backups_dir(&state.cfg());
    let path = backups::find(&dir, &name).map_err(not_found)?;
    backups::check_header(&path).map_err(|r| ApiError::bad(r.0))?;
    start_job(&state, JobKind::Restore(RestoreOptions { name }), None)
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ImportQuery {
    /// The file's name on the computer it came from, for the log and the list.
    name: Option<String>,
}

/// Import a database: the file is the request's body. It's checked here, so
/// a file UpNext can't use is refused at once (400); then a job replaces
/// the database with it (202), unless another job runs (409).
#[utoipa::path(post, path = "/backups/import", tag = "backups", operation_id = "backups_import",
    description = "Import a database: the file is the request's body (up to 2 GB). It's checked first, then a job \
        replaces the database with it. Only on the computer UpNext runs on.",
    params(ImportQuery),
    request_body(content = Vec<u8>, content_type = "application/octet-stream", description = "The database file"),
    responses((status = 202, body = crate::db::Job),
        (status = 400, description = "A file UpNext can't use, with why"),
        (status = 403, description = "From another device"),
        (status = 409, description = "Another job is running")))]
async fn import(State(state): State<AppState>, Query(q): Query<ImportQuery>, body: Body) -> Result<(StatusCode, Json<Value>), ApiError> {
    if let Some(running) = state.db.job_running()? {
        return Err(ApiError::conflict(format!("{}; wait for it or cancel it", jobs::busy_text(&running))));
    }
    let name = q
        .name
        .map(|n| n.rsplit(['/', '\\']).next().unwrap_or_default().trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "the uploaded database".into());
    let dir = jobs::backup::backups_dir(&state.cfg());
    backups::clean_work_files(&dir);
    let upload = backups::work_path(&dir)?;
    let received = receive(body, &upload).await;
    let staged = match received {
        Ok(()) => {
            let (upload, dir) = (upload.clone(), dir.clone());
            blocking(move || backups::stage(&upload, &dir)).await
        }
        Err(e) => Err(e),
    };
    backups::remove_work_file(&upload);
    // The job owns the checked copy from here; it goes when the job ends.
    let staged = staged.map_err(refused)?.hand_over();
    let opts = ImportOptions { name, staged: staged.display().to_string() };
    let started = start_job(&state, JobKind::Import(opts), None);
    if started.is_err() {
        backups::remove_work_file(&staged);
    }
    started
}

/// Write the request's body to `to`, refusing more than `MAX_UPLOAD`.
async fn receive(body: Body, to: &std::path::Path) -> anyhow::Result<()> {
    let mut file = tokio::fs::File::create(to).await?;
    crate::http::make_private(to);
    let mut stream = body.into_data_stream();
    let mut size = 0u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| anyhow::anyhow!("the upload broke off: {e}"))?;
        size += chunk.len() as u64;
        if size > MAX_UPLOAD {
            return Err(Refused(format!("that file is over {} GB; an UpNext database is far smaller", MAX_UPLOAD >> 30)).into());
        }
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    if size == 0 {
        return Err(Refused("the upload was empty; choose a database file".into()).into());
    }
    Ok(())
}

#[utoipa::path(delete, path = "/backups/{name}", tag = "backups", operation_id = "backups_delete",
    description = "Delete a backup and its note. Only on the computer UpNext runs on.",
    params(("name" = String, Path, description = "The backup's file name")),
    responses((status = 204, description = "Deleted"), (status = 403, description = "From another device"),
        (status = 404, description = "No such backup")))]
async fn delete(State(state): State<AppState>, Path(name): Path<String>) -> Result<StatusCode, ApiError> {
    let dir = jobs::backup::backups_dir(&state.cfg());
    backups::find(&dir, &name).map_err(not_found)?;
    blocking(move || backups::delete(&dir, &name)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Download a copy of the database as it is, without keys and sign-ins.
#[utoipa::path(get, path = "/export", tag = "backups",
    description = "Download a copy of the database as it is, without keys and sign-ins.",
    responses((status = 200, description = "The database file (`upnext-YYYY-MM-DD-HHMM-without-keys.db`)",
        content_type = "application/vnd.sqlite3", body = Vec<u8>)))]
async fn export_db(State(state): State<AppState>) -> Result<Response, ApiError> {
    let stamp = chrono::Local::now().format("%Y-%m-%d-%H%M").to_string();
    let path = std::env::temp_dir().join(format!("upnext-export-{}.db", uuid::Uuid::new_v4().simple()));
    let db = state.db.clone();
    let p = path.clone();
    let made = tokio::task::spawn_blocking(move || db.export_to(&p)).await.map_err(anyhow::Error::from)?;
    let bytes = made.and_then(|()| Ok(std::fs::read(&path)?));
    let _ = std::fs::remove_file(&path);
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "application/vnd.sqlite3".to_string()),
            (axum::http::header::CONTENT_DISPOSITION, format!("attachment; filename=\"upnext-{stamp}-without-keys.db\"")),
        ],
        bytes?,
    )
        .into_response())
}
