//! UpNext's shared game data (docs/api/seed.md):
//! what is applied, applying it from a URL or an uploaded file, removing it,
//! and the game page's player reviews fetched when they're asked for.

use super::players::Me;
use super::{ApiError, ApiResult, AppState, game_or_404};
use crate::db::GameView;
use crate::seed::{self, Applied, Manifest, Refused, SeedStatus};
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route("/seed", get(seed_get).post(seed_post).delete(seed_delete))
        .route("/seed/upload", post(seed_upload))
        .route("/seed/offer", post(seed_offer))
        .route("/games/{id}/reviews", post(game_reviews))
}

/// A file UpNext won't apply is the request's fault (400); a download that
/// failed is the other server's (502).
fn failed(e: anyhow::Error) -> ApiError {
    match e.downcast::<Refused>() {
        Ok(r) => ApiError::bad(r.0),
        Err(e) => ApiError::bad_gateway(format!("{e:#}")),
    }
}

/// One download or upload is applied at a time: each can hold its file and
/// up to 256 MB unpacked, so several at once could run the server out of
/// memory. A second waits for the first.
static APPLYING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> anyhow::Result<T> {
    tokio::task::spawn_blocking(f).await.map_err(anyhow::Error::from)?
}

/// Apply checked data, keep it for games added later, and record it.
async fn apply_loaded(state: &AppState, loaded: seed::Loaded, source: String) -> anyhow::Result<Applied> {
    let (db, cfg) = (state.db.clone(), state.cfg());
    blocking(move || {
        let done = seed::apply_and_record(&db, &loaded.seed, &source)?;
        seed::keep(&cfg, &loaded.seed, &loaded.data)?;
        Ok(done)
    })
    .await
}

async fn apply(state: &AppState, loaded: seed::Loaded, source: String) -> ApiResult<Applied> {
    Ok(Json(apply_loaded(state, loaded, source).await?))
}

/// The setup wizard's apply when the first sync ends: the release's
/// data at `url`. None when its manifest can't be read, since no release
/// has published one yet or the release can't be reached: there's nothing
/// to apply, which isn't a failure.
pub(super) async fn apply_offered(state: &AppState, url: &str) -> anyhow::Result<Option<Applied>> {
    let http = http()?;
    if let Err(e) = seed::offer(&http, url).await {
        tracing::info!("no shared data to apply after the first sync: {e:#}");
        return Ok(None);
    }
    let _one = APPLYING.lock().await;
    let loaded = seed::load(&http, url).await?;
    apply_loaded(state, loaded, url.to_string()).await.map(Some)
}

#[utoipa::path(get, path = "/seed", tag = "seed", operation_id = "seed_get",
    description = "What shared data is applied: how many titles have summaries from it, its date, the kept copy that games added later get theirs from after a sync, the latest times it was applied, and where the newest release's data is.",
    responses((status = 200, body = SeedStatus)))]
async fn seed_get(State(state): State<AppState>) -> ApiResult<SeedStatus> {
    Ok(Json(seed::status(&state.db, &state.cfg())?))
}

#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[schema(as = SeedOfferBody)]
pub struct OfferBody {
    /// A manifest's address (default: the newest release's).
    url: Option<String>,
}

/// The download client, built once and shared by every request.
fn http() -> anyhow::Result<reqwest::Client> {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    if let Some(c) = CLIENT.get() {
        return Ok(c.clone());
    }
    let built = seed::client()?;
    Ok(CLIENT.get_or_init(|| built).clone())
}

/// http(s) only: the server fetches it.
fn web_address(url: Option<String>) -> Result<String, ApiError> {
    let url = url.map(|u| u.trim().to_string()).filter(|u| !u.is_empty()).unwrap_or_else(|| seed::DEFAULT_URL.into());
    if url.starts_with("https://") || url.starts_with("http://") {
        Ok(url)
    } else {
        Err(ApiError::bad("give the shared data's address, starting with https://"))
    }
}

#[utoipa::path(post, path = "/seed/offer", tag = "seed", operation_id = "seed_offer",
    description = "Read a release's manifest without downloading its data: how many games and how recent, for the first-run wizard's \"Use UpNext's game data (10,000 games, updated October 2026)\". \
        A POST, though it changes nothing, so another website can't make the server fetch an address with an image or a link.",
    request_body = Option<OfferBody>,
    responses((status = 200, body = Manifest),
        (status = 400, description = "Not an http(s) address, an answer that isn't 2xx, or not a manifest"),
        (status = 502, description = "The download failed")))]
async fn seed_offer(body: Option<Json<OfferBody>>) -> ApiResult<Manifest> {
    let url = web_address(body.and_then(|Json(b)| b.url))?;
    Ok(Json(seed::offer(&http()?, &url).await.map_err(failed)?))
}

#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[schema(as = SeedApplyBody)]
pub struct ApplyBody {
    /// A manifest (`upnext-seed.json`) or data file (`upnext-seed-<date>.jsonl.gz`), http or https.
    /// Default: the newest release's manifest.
    url: Option<String>,
}

#[utoipa::path(post, path = "/seed", tag = "seed", operation_id = "seed_apply",
    description = "Download shared data and apply it: matching titles get People say, Vibes and review tags, with no review fetch and no AI call. \
        An install's own summary written with the same prompt version or a newer one is kept. The data is kept, so games added later get theirs after a sync.",
    request_body = Option<ApplyBody>,
    responses((status = 200, body = Applied),
        (status = 400, description = "Not shared data UpNext can use: damaged, a bad checksum, an unknown field, a newer format, or not an http(s) URL"),
        (status = 502, description = "The download failed")))]
async fn seed_post(State(state): State<AppState>, body: Option<Json<ApplyBody>>) -> ApiResult<Applied> {
    let url = web_address(body.and_then(|Json(b)| b.url))?;
    let _one = APPLYING.lock().await;
    let loaded = seed::load(&http()?, &url).await.map_err(failed)?;
    apply(&state, loaded, url).await
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct UploadQuery {
    /// The file's name on the computer it came from, for the record.
    name: Option<String>,
}

#[utoipa::path(post, path = "/seed/upload", tag = "seed", operation_id = "seed_upload",
    description = "Apply shared data from a data file (`upnext-seed-<date>.jsonl.gz`), the request's body (up to 64 MB). \
        It checks itself with the checksum in its first line.",
    params(UploadQuery),
    request_body(content = Vec<u8>, content_type = "application/octet-stream", description = "The data file"),
    responses((status = 200, body = Applied),
        (status = 400, description = "Not shared data UpNext can use, with why")))]
async fn seed_upload(State(state): State<AppState>, Query(q): Query<UploadQuery>, body: Body) -> ApiResult<Applied> {
    let _one = APPLYING.lock().await;
    let data = axum::body::to_bytes(body, seed::MAX_DOWNLOAD)
        .await
        .map_err(|_| ApiError::bad("that file is far bigger than UpNext's shared data"))?
        .to_vec();
    let name = q
        .name
        .map(|n| n.rsplit(['/', '\\']).next().unwrap_or_default().trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "an uploaded file".into());
    let checked = blocking(move || Ok(seed::read(&data).map(|seed| seed::Loaded { seed, data }))).await?.map_err(|r| ApiError::bad(r.0))?;
    apply(&state, checked, name).await
}

#[utoipa::path(delete, path = "/seed", tag = "seed", operation_id = "seed_remove",
    description = "Stop using shared data: its summaries and the kept copy go, so this install's own AI writes them on the next summaries run.",
    responses((status = 200, body = Object, description = "`{removed}`: how many summaries went")))]
async fn seed_delete(State(state): State<AppState>) -> ApiResult<Value> {
    let (db, cfg) = (state.db.clone(), state.cfg());
    let removed = blocking(move || seed::remove(&db, &cfg)).await?;
    Ok(Json(json!({ "removed": removed })))
}

/// The game page's "What players wrote" for a title whose summary came
/// from shared data, so its reviews were never fetched: Steam's by appid,
/// else GOG's. Stored as the enrich sources store them. Someone is
/// waiting, so each request is tried once (`http::quick`): a store that's
/// down says so in seconds, not after the jobs' minute of backoff.
#[utoipa::path(post, path = "/games/{id}/reviews", tag = "games", operation_id = "game_reviews_fetch",
    description = "Fetch the title's player reviews now (Steam's, else GOG's) and store them: the game page asks when \"What players wrote\" is opened on a game whose summary came from shared data. Returns the title in full.",
    params(("id" = i64, Path, description = "Title id")),
    responses((status = 200, body = GameView),
        (status = 400, description = "Neither a Steam appid nor a GOG copy to read reviews for"),
        (status = 404, description = "No such title"), (status = 502, description = "The store didn't answer")))]
async fn game_reviews(State(state): State<AppState>, Me(me): Me, Path(id): Path<i64>) -> ApiResult<GameView> {
    let g = game_or_404(&state, me, id)?;
    let store = |e: anyhow::Error| ApiError::bad_gateway(format!("the store didn't answer: {e:#}"));
    if let Some(appid) = g.appid {
        let (score, texts) =
            crate::http::quick(crate::steam::app_reviews_with_texts(&state.clients.steam, appid, 10)).await.map_err(store)?;
        if score.is_some() {
            state.db.put_reviews(appid, score.as_ref())?;
        }
        state.db.put_review_texts(appid, texts.as_deref())?;
    } else if let Some(c) = g.copies.iter().find(|c| c.source == "gog") {
        let reviews = crate::http::quick(state.clients.gogmeta.reviews(&c.source_id, 8)).await.map_err(store)?;
        state.db.put_gog_reviews(&c.source_id, reviews.as_ref())?;
    } else {
        return Err(ApiError::bad(format!("{} has no Steam or GOG page to read reviews from", g.name)));
    }
    Ok(Json(game_or_404(&state, me, id)?))
}
