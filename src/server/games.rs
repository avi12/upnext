//! One game, and the library a game at a time: the filtered list, a game's
//! page with its status, ending and series, Play later, and the Second
//! Chance page.

use super::jobs::task_json;
use super::players::{self, Me};
use super::{ApiError, ApiResult, AppState, game_or_404};
use crate::db::{GameFilter, Override, PlayerId, Status};
use crate::jobs::tasks::Task;
use axum::extract::{Path, Query, State};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    use axum::routing::{get, post, put};
    Router::new()
        .route("/games", get(games))
        .route("/review-tags", get(review_tags))
        .route("/stop-reasons", get(stop_reasons))
        .route("/games/{id}", get(game))
        .route("/games/{id}/override", put(set_override))
        .route("/games/{id}/ending", put(set_ending))
        .route("/games/{id}/next-in-series", get(next_in_series))
        .route("/games/{id}/series", get(game_series))
        .route("/games/{id}/later", put(later_add).delete(later_remove))
        .route("/play-later", get(play_later))
        .route("/second-chances", get(second_chances_get))
        .route("/second-chances/{id}", post(second_chance_post))
}

/// The series a game belongs to (for the game sheet), or null.
#[utoipa::path(get, path = "/games/{id}/series", tag = "games",
    description = "The series a game belongs to (for the game sheet), or null.",
    params(("id" = i64, Path, description = "Title id")),
    responses((status = 200, body = Option<crate::series::Collection>)))]
async fn game_series(State(state): State<AppState>, Me(me): Me, Path(id): Path<i64>) -> ApiResult<Option<crate::series::Collection>> {
    let all = state.db.all_games_shared(me)?;
    let guides = state.db.series_guides()?;
    let catalog = state.db.catalog()?;
    Ok(Json(crate::series::collection_of(&all, &guides, &catalog, id)))
}

/// After beating a game: the next unplayed game of its series you own, if
/// it isn't already on Play later.
#[utoipa::path(get, path = "/games/{id}/next-in-series", tag = "games",
    description = "After beating a game: the next unplayed game of its series you own, unless it's already on Play later.",
    params(("id" = i64, Path, description = "Title id")),
    responses((status = 200, body = Object, description = "`{series, game: GameView}`, or null")))]
async fn next_in_series(State(state): State<AppState>, Me(me): Me, Path(id): Path<i64>) -> ApiResult<Value> {
    let all = state.db.all_games_shared(me)?;
    let guides = state.db.series_guides()?;
    let catalog = state.db.catalog()?;
    let next = crate::series::collections(&all, &guides, &catalog).into_iter().find_map(|c| {
        if !c.members.iter().any(|g| g.id == id) {
            return None;
        }
        // Members are in play order (side stories last): the first unplayed
        // one after the game you beat, else the series' own next pick.
        // Spin-offs and tools aren't "the next one".
        let side: Vec<i64> = c.guide.as_ref().map(|g| g.side.iter().map(|s| s.id).collect()).unwrap_or_default();
        let unplayed =
            |g: &&crate::db::GameView| matches!(g.status, Status::Untouched | Status::Sampled) && g.id != id && !side.contains(&g.id);
        let pos = c.members.iter().position(|g| g.id == id)?;
        let next = c.members[pos + 1..]
            .iter()
            .find(unplayed)
            .or_else(|| c.members.iter().find(|g| Some(g.id) == c.next_id).filter(unplayed))
            .filter(|g| g.saved_at.is_none())?;
        Some(json!({ "series": c.name, "game": next }))
    });
    Ok(Json(next.unwrap_or(Value::Null)))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct GamesQuery {
    /// Statuses, a comma list: `untouched,sampled`. An unknown one is a 400.
    status: Option<String>,
    /// "true" for your staples only.
    staple: Option<String>,
    /// Stores, a comma list: `steam,gog`. Any store id (Galaxy brings others): one with no games finds none.
    source: Option<String>,
    /// "true" for installed games only.
    installed: Option<String>,
    /// "true" for games Steam says a gamepad works with, fully or in part.
    controller: Option<String>,
    /// Search the name, editions, genres, tags, themes, categories and developers.
    q: Option<String>,
    /// One genre, exactly: one no game has finds none.
    genre: Option<String>,
    /// One tag, exactly.
    tag: Option<String>,
    /// `ok`, `native`, `issues` or `unknown`; anything else is a 400.
    linux: Option<String>,
    /// Your own rating: `rated`, `unrated`, `high` (4 or 5 stars) or `low` (1 or 2).
    rating: Option<String>,
    /// What other backlog players say: `respects_time` or `clicks_fast`.
    backlog: Option<String>,
    /// `name` (the default), `playtime`, `last_played`, `review`, `hltb`, `release`, `metacritic` or `rating` (yours).
    sort: Option<String>,
    /// `asc` or `desc`.
    order: Option<String>,
    /// 100 unless asked.
    limit: Option<usize>,
    offset: Option<usize>,
}

/// What `GET /games?linux=` takes (`db::filter`'s matches).
const LINUX_FILTERS: [&str; 4] = ["ok", "native", "issues", "unknown"];

/// Every status's name, as an unknown one's 400 lists them.
fn status_names() -> String {
    let names: Vec<&str> = Status::ALL.iter().map(Status::as_str).collect();
    let (last, rest) = names.split_last().expect("there are statuses");
    format!("{} or {last}", rest.join(", "))
}

fn csv(s: &Option<String>) -> Option<Vec<String>> {
    s.as_ref().map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect::<Vec<_>>()).filter(|v| !v.is_empty())
}

/// Every value of a parameter that can repeat or hold a comma-separated list:
/// `facet=sessions:short&facet=mood:cozy` or `facet=sessions:short,mood:cozy`.
fn repeated(pairs: &[(String, String)], key: &str) -> Vec<String> {
    pairs.iter().filter(|(k, _)| k == key).flat_map(|(_, v)| v.split(',')).map(|v| v.trim().to_string()).filter(|v| !v.is_empty()).collect()
}

#[utoipa::path(get, path = "/games", tag = "games",
    description = "The library, filtered, sorted and paged.",
    params(GamesQuery,
        ("facet" = Option<Vec<String>>, Query, description = "A review-tag facet the title must have, `name:value` \
            (`sessions:short`); repeat it or give a comma list"),
        ("praised" = Option<Vec<String>>, Query, description = "Aspects players must have praised; repeat or comma list"),
        ("not_criticized" = Option<Vec<String>>, Query, description = "Aspects players must not have criticized or split on")),
    responses((status = 200, body = Object, description = "`{total, items: [GameView]}`"),
        (status = 400, description = "An unknown status, Linux filter, facet, aspect, rating or backlog filter")))]
async fn games(
    State(state): State<AppState>,
    Me(me): Me,
    Query(q): Query<GamesQuery>,
    Query(pairs): Query<Vec<(String, String)>>,
) -> ApiResult<Value> {
    let bad = |what: &str, v: &str| ApiError::bad(format!("unknown {what} {v:?}: see GET /api/review-tags"));
    let facets = repeated(&pairs, "facet")
        .iter()
        .map(|f| crate::review_tags::facet_name(f).ok_or_else(|| bad("facet", f)))
        .collect::<Result<Vec<_>, _>>()?;
    let aspects = |key: &str| {
        repeated(&pairs, key)
            .iter()
            .map(|a| crate::review_tags::aspect_name(a).map(str::to_string).ok_or_else(|| bad("aspect", a)))
            .collect::<Result<Vec<_>, _>>()
    };
    let rating = q.rating.clone().filter(|s| !s.is_empty());
    if let Some(r) = rating.as_deref().filter(|r| !matches!(*r, "rated" | "unrated" | "high" | "low")) {
        return Err(ApiError::bad(format!("unknown rating {r:?}: rated, unrated, high or low")));
    }
    let backlog = q.backlog.clone().filter(|s| !s.is_empty());
    if let Some(b) = backlog.as_deref().filter(|b| !matches!(*b, "respects_time" | "clicks_fast")) {
        return Err(ApiError::bad(format!("unknown backlog filter {b:?}: respects_time or clicks_fast")));
    }
    // A status or a Linux filter it doesn't know is a 400, as a rating is,
    // rather than a filter that quietly finds nothing.
    let statuses = csv(&q.status)
        .map(|v| {
            v.iter()
                .map(|s| Status::parse(s).ok_or_else(|| ApiError::bad(format!("unknown status {s:?}: {}", status_names()))))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    let linux = q.linux.clone().filter(|s| !s.is_empty());
    if let Some(l) = linux.as_deref().filter(|l| !LINUX_FILTERS.contains(l)) {
        return Err(ApiError::bad(format!("unknown Linux filter {l:?}: ok, native, issues or unknown")));
    }
    let filter = GameFilter {
        statuses,
        staple: q.staple.as_deref().map(|v| v == "true" || v == "1"),
        sources: csv(&q.source),
        installed: q.installed.as_deref().map(|v| v == "true" || v == "1"),
        controller: q.controller.as_deref().map(|v| v == "true" || v == "1"),
        q: q.q.clone(),
        genre: q.genre.clone().filter(|s| !s.is_empty()),
        tag: q.tag.clone().filter(|s| !s.is_empty()),
        linux,
        rating,
        backlog,
        facets,
        praised: aspects("praised")?,
        not_criticized: aspects("not_criticized")?,
        sort: q.sort.clone().unwrap_or_else(|| "name".into()),
        order: q.order.clone(),
        limit: q.limit.unwrap_or(100),
        offset: q.offset.unwrap_or(0),
    };
    let (total, items) = tracing::info_span!(target: "upnext::timing", "list").in_scope(|| state.db.list_games(me, &filter))?;
    Ok(Json(json!({ "total": total, "items": items })))
}

/// The review-tag vocabulary, with how many titles have each value.
#[utoipa::path(get, path = "/review-tags", tag = "games",
    description = "The review-tag vocabulary, with how many titles have each value.",
    responses((status = 200, body = Object, description = "Facets and aspects, each value with its count")))]
async fn review_tags(State(state): State<AppState>, Me(me): Me) -> ApiResult<Value> {
    let games = state.db.all_games_shared(me)?;
    Ok(Json(crate::review_tags::vocabulary(games.iter().filter_map(|g| g.review_tags.as_ref()))))
}

/// The reasons a game can be put down for, in the menu's order.
#[utoipa::path(get, path = "/stop-reasons", tag = "games",
    description = "The reasons a game can be put down for, in the menu's order.",
    responses((status = 200, body = [crate::lifecycle::Reason])))]
async fn stop_reasons() -> Json<&'static [crate::lifecycle::Reason]> {
    Json(crate::lifecycle::REASONS)
}

#[utoipa::path(get, path = "/games/{id}", tag = "games",
    description = "One title, in full.",
    params(("id" = i64, Path, description = "Title id")),
    responses((status = 200, body = crate::db::GameView), (status = 404, description = "No such title")))]
async fn game(
    State(state): State<AppState>,
    Me(me): Me,
    who: Option<axum::Extension<players::Who>>,
    Path(id): Path<i64>,
) -> ApiResult<crate::db::GameView> {
    // A game the player hid from themself opens from their Hidden by me
    // list, while it's open.
    match players::visible_game(&state, me, who.as_deref(), id)? {
        Some(mut g) => {
            // The game page's "From your wishlist", while the wishlist is on.
            if state.cfg_for(me).wishlist {
                g.from_wishlist = state.db.wishlist_from(me, id)?;
            }
            Ok(Json(g))
        }
        None => Err(ApiError::not_found(format!("game {id} not found"))),
    }
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct OverrideBody {
    status: Option<String>,
    note: Option<String>,
    /// Why the game was put down, with Shelved, Abandoned or Burnt out.
    reasons: Option<ReasonsBody>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ReasonsBody {
    #[serde(default)]
    pub(super) codes: Vec<String>,
    pub(super) note: Option<String>,
}

#[utoipa::path(put, path = "/games/{id}/override", tag = "games",
    description = "Set or clear the status you set on a game, with a note, and why it was put down (with shelved, \
        abandoned or burnt_out).",
    params(("id" = i64, Path, description = "Title id")),
    request_body = OverrideBody,
    responses((status = 200, body = crate::db::GameView),
        (status = 400, description = "An unknown status or reason, or reasons with a status that doesn't take them"),
        (status = 404, description = "No such title")))]
async fn set_override(
    State(state): State<AppState>,
    Me(me): Me,
    Path(id): Path<i64>,
    Json(body): Json<OverrideBody>,
) -> ApiResult<crate::db::GameView> {
    let status = match body.status.as_deref() {
        None | Some("") | Some("none") | Some("null") => None,
        Some(s) => Some(Override::parse(s).ok_or_else(|| ApiError::bad(format!("invalid status {s:?}")))?),
    };
    let reasons = match &body.reasons {
        Some(r) => crate::lifecycle::check_reasons(&r.codes, r.note.as_deref()).map_err(|e| ApiError::bad(e.to_string()))?,
        None => None,
    };
    if reasons.is_some() && !status.is_some_and(|s| s.asks_why()) {
        return Err(ApiError::bad("reasons go with shelved, abandoned or burnt_out"));
    }
    game_or_404(&state, me, id)?;
    state.db.set_override(me, id, status, body.note.as_deref())?;
    if let Some((codes, note)) = reasons {
        state.db.add_stop_reasons(me, id, &codes, note.as_deref(), "menu")?;
    }
    Ok(Json(state.db.game(me, id)?.expect("just checked")))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct EndingBody {
    /// true: no ending; false: has one; null: let UpNext infer it.
    no_ending: Option<bool>,
}

/// The game page's "This game has no ending" switch.
#[utoipa::path(put, path = "/games/{id}/ending", tag = "games",
    description = "The game page's \"This game has no ending\" switch.",
    params(("id" = i64, Path, description = "Title id")),
    request_body = EndingBody,
    responses((status = 200, body = crate::db::GameView), (status = 404, description = "No such title")))]
async fn set_ending(
    State(state): State<AppState>,
    Me(me): Me,
    Path(id): Path<i64>,
    Json(body): Json<EndingBody>,
) -> ApiResult<crate::db::GameView> {
    game_or_404(&state, me, id)?;
    state.db.set_no_ending(id, body.no_ending)?;
    Ok(Json(state.db.game(me, id)?.expect("just checked")))
}

#[utoipa::path(put, path = "/games/{id}/later", tag = "games",
    description = "Save a game to Play later.",
    params(("id" = i64, Path, description = "Title id")),
    responses((status = 200, body = crate::db::GameView), (status = 404, description = "No such title")))]
async fn later_add(State(state): State<AppState>, Me(me): Me, Path(id): Path<i64>) -> ApiResult<crate::db::GameView> {
    set_later(&state, me, id, true)
}

#[utoipa::path(delete, path = "/games/{id}/later", tag = "games",
    description = "Take a game off Play later.",
    params(("id" = i64, Path, description = "Title id")),
    responses((status = 200, body = crate::db::GameView), (status = 404, description = "No such title")))]
async fn later_remove(State(state): State<AppState>, Me(me): Me, Path(id): Path<i64>) -> ApiResult<crate::db::GameView> {
    set_later(&state, me, id, false)
}

fn set_later(state: &AppState, me: PlayerId, id: i64, saved: bool) -> ApiResult<crate::db::GameView> {
    game_or_404(state, me, id)?;
    state.db.set_play_later(me, id, saved)?;
    Ok(Json(state.db.game(me, id)?.expect("just checked")))
}

/// Saved games, most recently saved first.
#[utoipa::path(get, path = "/play-later", tag = "games",
    description = "Saved games, most recently saved first, without review texts, per-source tags and screenshots.",
    responses((status = 200, body = Vec<crate::db::GameView>)))]
async fn play_later(State(state): State<AppState>, Me(me): Me) -> ApiResult<Vec<crate::db::GameView>> {
    let mut items: Vec<crate::db::GameView> =
        state.db.all_games_shared(me)?.iter().filter(|g| g.saved_at.is_some()).map(crate::db::GameView::slim).collect();
    items.sort_by_key(|g| std::cmp::Reverse(g.saved_at));
    Ok(Json(items))
}

/// The Second Chance page: the abandoned games it lists, best news first,
/// whether an AI is there to write "Now", and the job that writes it.
#[utoipa::path(get, path = "/second-chances", tag = "games",
    description = "The Second Chance page: the abandoned games it lists, best news first, whether an AI is there to \
        write \"Now\", and the job that writes it.",
    responses((status = 200, body = Object, description = "`{games: [GameView], ai: {available, reason}, \
        task: {schedule, enabled, last_run, next_run_at}}`")))]
async fn second_chances_get(State(state): State<AppState>, Me(me): Me) -> ApiResult<Value> {
    let games = crate::second_chance::page(&state.db.all_games_shared(me)?);
    let ai = match state.summary_llm().available().await {
        Ok(()) => json!({ "available": true, "reason": null }),
        Err(e) => json!({ "available": false, "reason": e.reason() }),
    };
    let task = task_json(&state, Task::SecondChances)?;
    Ok(Json(json!({ "games": games, "ai": ai, "task": task })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SecondChanceBody {
    /// willing, still_no or not_for_me.
    answer: Option<String>,
    /// Take the answer back, putting back the mark the game had before it.
    #[serde(default)]
    undo: bool,
    /// Why it was put down, as edited on the page: the reply box is its note.
    reasons: Option<ReasonsBody>,
}

/// An answer on the Second Chance page, your reply to "Now", or both.
#[utoipa::path(post, path = "/second-chances/{id}", tag = "games",
    description = "An answer on the Second Chance page (willing, still_no, not_for_me), undo, why it was put down, or a mix.",
    params(("id" = i64, Path, description = "Title id")),
    request_body = SecondChanceBody,
    responses((status = 200, body = crate::db::GameView),
        (status = 400, description = "No answer, undo or reasons; an unknown answer; or an answer with undo"),
        (status = 404, description = "No such title"),
        (status = 409, description = "The game isn't waiting on the Second Chance page")))]
async fn second_chance_post(
    State(state): State<AppState>,
    Me(me): Me,
    Path(id): Path<i64>,
    Json(body): Json<SecondChanceBody>,
) -> ApiResult<crate::db::GameView> {
    use crate::second_chance::Answer;
    let answer = match body.answer.as_deref() {
        None => None,
        Some(a) => Some(Answer::parse(a).ok_or_else(|| ApiError::bad(format!("an answer is willing, still_no or not_for_me; not {a:?}")))?),
    };
    if answer.is_some() && body.undo {
        return Err(ApiError::bad("give an answer or undo, not both"));
    }
    let reasons = match &body.reasons {
        Some(r) => Some(crate::lifecycle::check_reasons(&r.codes, r.note.as_deref()).map_err(|e| ApiError::bad(e.to_string()))?),
        None => None,
    };
    if answer.is_none() && !body.undo && reasons.is_none() {
        return Err(ApiError::bad("give an answer, undo, or reasons"));
    }
    let g = game_or_404(&state, me, id)?;
    if answer.is_some() && !crate::second_chance::listed(&g) {
        return Err(ApiError::conflict(format!(
            "{} isn't waiting on the Second Chance page: only abandoned games you haven't answered for are",
            g.name
        )));
    }
    if let Some(r) = reasons {
        match r {
            Some((codes, note)) => state.db.add_stop_reasons(me, id, &codes, note.as_deref(), "second_chance")?,
            // Everything cleared: say so, so the old reasons no longer stand.
            None => state.db.add_stop_reasons(me, id, &[], None, "second_chance")?,
        }
    }
    if answer.is_some() || body.undo {
        state.db.answer_second_chance(me, id, answer)?;
    }
    Ok(Json(state.db.game(me, id)?.expect("just checked")))
}
