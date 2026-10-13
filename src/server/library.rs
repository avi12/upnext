//! The library as a whole: the profile with each source's state, the
//! library's counts and series, and merging and splitting titles.

use super::players::Me;
use super::{ApiError, ApiResult, AppState, refused};
use crate::config::Config;
use crate::db::PlayerId;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route("/profile", get(profile))
        .route("/collections", get(collections))
        .route("/stats", get(stats))
        .route("/titles/merge", post(titles_merge))
        .route("/titles/{id}/split", post(titles_split))
}

/// `who`'s sources as the profile shows them: set up or not, with how.
pub fn configured_sources(cfg: &Config, state: &AppState, who: PlayerId, p: &mut crate::db::Profile) {
    let steam_api = cfg.steam_credentials().is_ok();
    let steam_dir = crate::steam_local::find_steam_dir(cfg.steam_dir.as_deref());
    // This computer's Steam client and Heroic folder are the first player's
    // unless another player's own SteamID is signed in to it or their own
    // Heroic folder is named.
    let (steam_local, heroic) = if who == PlayerId::FIRST {
        (steam_dir.is_some(), crate::heroic::find_config_dir(cfg.heroic_dir.as_deref()).is_some())
    } else {
        let signed_in =
            cfg.steam_id.as_ref().is_some_and(|id| crate::steam::local_users(steam_dir.as_deref()).iter().any(|u| &u.steam_id == id));
        (signed_in, cfg.heroic_dir.as_ref().is_some_and(|d| d.join("store_cache").is_dir()))
    };
    let steam = steam_api || steam_local;
    let gog = state.gog_for(who).load_token().is_some();
    for s in p.sources.iter_mut() {
        match s.source.as_str() {
            "steam" => {
                s.configured = steam;
                s.note = Some(match (steam_api, steam_local) {
                    (true, _) => "Web API".into(),
                    (false, true) => "local Steam client (no API key)".into(),
                    (false, false) => "not set up".into(),
                });
            }
            "gog" => {
                s.configured = heroic || gog;
                s.note = Some(match (heroic, gog) {
                    (true, true) => "Heroic + GOG account".into(),
                    (true, false) => "Heroic (connect GOG for playtime)".into(),
                    (false, true) => "GOG account".into(),
                    (false, false) => "not set up".into(),
                });
            }
            "epic" => {
                let account = state.epic_for(who).load_token().is_some();
                let launcher = crate::epic::find_manifests_dir(cfg.epic_manifests.as_deref()).is_some();
                s.configured = heroic || account || launcher;
                let ways: Vec<&str> = [(heroic, "Heroic"), (account, "Epic account"), (launcher, "Epic Games Launcher")]
                    .into_iter()
                    .filter_map(|(on, name)| on.then_some(name))
                    .collect();
                s.note = Some(if ways.is_empty() { "not set up".into() } else { ways.join(" + ") });
            }
            "galaxy" => s.configured = cfg.galaxy_db.as_ref().is_some_and(|p| p.is_file()) || s.last_sync.is_some(),
            // Their games come from Windows' installs, GOG Galaxy or a list
            // typed in, and the row is only there when it has some.
            "origin" | "uplay" => s.configured = s.count > 0,
            _ => s.configured = if s.launcher == "steam" { steam } else { heroic },
        }
    }
}

#[utoipa::path(get, path = "/profile", tag = "library",
    description = "The Steam profile and every library source: whether it's set up, its game count and last sync.",
    responses((status = 200, body = crate::db::Profile)))]
async fn profile(State(state): State<AppState>, Me(me): Me) -> ApiResult<crate::db::Profile> {
    let cfg = state.cfg_for(me);
    let mut p = state.db.profile(me, cfg.steam_id.as_deref())?;
    configured_sources(&cfg, &state, me, &mut p);
    Ok(Json(p))
}

#[utoipa::path(get, path = "/collections", tag = "library",
    description = "Every game series in the library, with play order, the next one to play and the games you don't own.",
    responses((status = 200, body = Vec<crate::series::Collection>)))]
async fn collections(State(state): State<AppState>, Me(me): Me) -> ApiResult<Vec<crate::series::Collection>> {
    let all = state.db.all_games_shared(me)?;
    let guides = state.db.series_guides()?;
    let catalog = state.db.catalog()?;
    Ok(Json(crate::series::collections(&all, &guides, &catalog)))
}

#[utoipa::path(get, path = "/stats", tag = "library",
    description = "Counts for the library: by status, source and Linux support, what's enriched, top genres, tags and most played.",
    responses((status = 200, body = crate::db::Stats)))]
async fn stats(State(state): State<AppState>, Me(me): Me) -> ApiResult<crate::db::Stats> {
    Ok(Json(state.db.stats(me)?))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct MergeBody {
    ids: Vec<i64>,
}

#[utoipa::path(post, path = "/titles/merge", tag = "library",
    description = "Merge two or more titles into the first: one game owned on several stores or in several editions. The first title survives with its name, Steam appid, status, note, ending switch and Play later.",
    request_body = MergeBody,
    responses((status = 200, body = crate::db::GameView),
        (status = 400, description = "Fewer than two ids, or an id named twice"),
        (status = 404, description = "A title this player doesn't see, or no such title")))]
async fn titles_merge(State(state): State<AppState>, Me(me): Me, Json(body): Json<MergeBody>) -> ApiResult<crate::db::GameView> {
    if body.ids.len() < 2 {
        return Err(ApiError::bad("give at least two title ids"));
    }
    // Only titles this player sees: a merge changes every player's library,
    // so a title hidden from them (by its owner, or by them) isn't theirs to
    // merge, and saying it isn't there gives nothing away.
    for &id in &body.ids {
        if state.db.game(me, id)?.is_none() {
            return Err(ApiError::not_found(format!("title {id} not found")));
        }
    }
    // `merge_titles` refuses an id named twice, adjacent or not.
    let tid = state.db.merge_titles(&body.ids).map_err(refused)?;
    // The merge is done by now, so a title that can't be read back is the
    // server's fault, not a missing title.
    state.db.game(me, tid)?.map(Json).ok_or_else(|| {
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, "the titles were merged, but the merged title couldn't be read back".into())
    })
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SplitBody {
    game_id: i64,
}

#[utoipa::path(post, path = "/titles/{id}/split", tag = "library",
    description = "Detach one copy from a title into a title of its own.",
    params(("id" = i64, Path, description = "Title id")),
    request_body = SplitBody,
    responses((status = 200, body = Object, description = "`{title: GameView, detached: GameView}`"),
        (status = 400, description = "The title has a single copy"),
        (status = 404, description = "The copy isn't part of that title")))]
async fn titles_split(State(state): State<AppState>, Me(me): Me, Path(id): Path<i64>, Json(body): Json<SplitBody>) -> ApiResult<Value> {
    let owns: bool = state.db.game(me, id)?.is_some_and(|g| g.copies.iter().any(|c| c.game_id == body.game_id));
    if !owns {
        return Err(ApiError::not_found(format!("copy {} is not part of title {id}", body.game_id)));
    }
    let new_tid = state.db.split_copy(body.game_id).map_err(refused)?;
    Ok(Json(json!({ "title": state.db.game(me, id)?, "detached": state.db.game(me, new_tid)? })))
}
