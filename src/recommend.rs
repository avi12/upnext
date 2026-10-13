//! Orchestrates a pick: build the prompt, call the LLM, validate that every
//! pick is an eligible owned game, retry once with the error, persist. A
//! pick ranks spares after the lineup; when they run out, `more` asks for
//! a new set for the same lineup. With no AI set up, both come from
//! UpNext's own ranking instead (`ranking`). Rolling the dice (the random
//! mode) draws them by chance, with no AI either way (`discovery::random`).

use crate::ai_usage::{self, Purpose};
use crate::config::Config;
use crate::db::PlayerId;
use crate::db::{AiKind, Alternate, Db, FeedbackAction, GameView, Pick, PickBatch};
use crate::discovery::{Lens, Mode};
use crate::lifecycle::Offer;
use crate::llm::{LlmBackend, LlmError, LlmRequest, Usage};
use crate::prompt::{self, PromptInput};
use crate::ranking::{self, Said};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use tracing::Instrument;

/// Spares ranked after each pick shown: 3 picks, 6 spares.
pub const SPARES_PER_PICK: usize = 2;

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct PickRequest {
    #[serde(default)]
    pub prompt: String,
    #[serde(default = "default_count")]
    pub count: usize,
    #[serde(default)]
    pub exclude_ids: Vec<i64>,
    #[serde(default)]
    pub sources: Option<Vec<String>>,
    /// Why the excluded games were turned down ("too long", "not in the
    /// mood for strategy"): steers the next picks away from them.
    #[serde(default)]
    pub avoid: Vec<String>,
    /// Pick from UpNext's own ranking even with an AI set up: "Pick
    /// without AI" after curating with the AI failed. With no AI set up, picks always do.
    #[serde(default)]
    pub without_ai: bool,
    /// A discovery mode: new_to_you or gems. Without one, "new to
    /// me" or "a hidden gem" in the request asks for it. `random` draws
    /// games off your radar by chance, with no AI call; only this field asks
    /// for it, and the request's words don't steer it.
    #[serde(default)]
    pub mode: Option<Mode>,
    /// The routine this session is, from `GET /api/routines`: who
    /// with, where, on what and for how long. Kept with the lineup.
    #[serde(default)]
    pub routine_id: Option<i64>,
    /// What it's played on: big screen sends `controller`, so
    /// games that play well on one count for a pick and games made for a
    /// mouse against it. A chosen routine that names its own input outweighs it.
    #[serde(default)]
    pub input: Option<crate::taste::routine::Input>,
}

fn default_count() -> usize {
    3
}

/// Limits on a pick request. The whole request goes
/// to the model and into History, and the ranking compares every word and
/// excluded id with every game, so an unbounded paste could make a costly
/// call, break the History page and stall the server. The UI's ask box stops
/// at `MAX_PROMPT_CHARS` too.
pub const MAX_PROMPT_CHARS: usize = 2_000;
pub const MAX_AVOID: usize = 50;
pub const MAX_AVOID_CHARS: usize = 300;
pub const MAX_EXCLUDE: usize = 500;

impl PickRequest {
    /// Refuses a request over the limits, saying which one.
    pub fn check(&self) -> Result<(), PickError> {
        let too = |m: String| Err(PickError::TooLarge(m));
        let chars = self.prompt.chars().count();
        if chars > MAX_PROMPT_CHARS {
            return too(format!("the request is {chars} characters; keep it to {MAX_PROMPT_CHARS}"));
        }
        if self.avoid.len() > MAX_AVOID || self.avoid.iter().any(|r| r.chars().count() > MAX_AVOID_CHARS) {
            return too(format!("give at most {MAX_AVOID} reasons to avoid games, each at most {MAX_AVOID_CHARS} characters"));
        }
        if self.exclude_ids.len() > MAX_EXCLUDE {
            return too(format!("exclude at most {MAX_EXCLUDE} games at once"));
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PickError {
    #[error("{0}")]
    NoCandidates(String),
    #[error("{0}")]
    NotFound(String),
    /// The request is over a limit (`PickRequest::check`).
    #[error("{0}")]
    TooLarge(String),
    #[error(transparent)]
    Llm(#[from] LlmError),
    #[error("the model returned invalid picks twice: {0}")]
    Invalid(String),
    /// Prints the whole chain itself, so it has no `source`.
    #[error("{0:#}")]
    Other(anyhow::Error),
}

impl From<anyhow::Error> for PickError {
    fn from(e: anyhow::Error) -> Self {
        PickError::Other(e)
    }
}

#[derive(Debug, Deserialize)]
struct RawPick {
    id: Value,
    #[serde(default)]
    name: String,
    #[serde(default)]
    reason: String,
}

#[derive(Debug, Deserialize)]
struct RawPicks {
    picks: Vec<RawPick>,
    #[serde(default)]
    spares: Vec<RawPick>,
    /// A wishlisted game standing in for one of the picks.
    #[serde(default)]
    wishlisted: Option<RawWish>,
    /// A game you don't own, right down your alley.
    #[serde(default)]
    suggestion: Option<RawSuggestion>,
}

#[derive(Debug, Deserialize)]
struct RawWish {
    id: Value,
    #[serde(default)]
    reason: String,
    instead_of: Value,
}

#[derive(Debug, Deserialize)]
struct RawSuggestion {
    #[serde(default)]
    name: String,
    #[serde(default)]
    reason: String,
}

/// A wishlist id as the model wrote it: "w12", "W12", "12" or 12.
fn parse_wish_id(v: &Value) -> Option<i64> {
    match v {
        Value::String(s) => s.trim().trim_start_matches(['w', 'W', '#']).parse::<i64>().ok(),
        v => parse_id(v),
    }
}

/// A candidate id as the model wrote it: 42, "42" or "#42".
pub(crate) fn parse_id(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().trim_start_matches('#').parse::<i64>().ok(),
        _ => None,
    }
}

/// What a lineup already holds, as a new game is checked against it: one
/// game per series, and at most `second_chances` second chances. In New to
/// you, the picks are each a way into a different genre or mechanic.
pub(crate) struct Screen<'a> {
    series: &'a HashMap<i64, Vec<String>>,
    taken: HashSet<&'a str>,
    second_chances: usize,
    limit: usize,
    /// New to you: each game's ways in, while picks (not spares) are checked.
    ways: Option<&'a HashMap<i64, Vec<String>>>,
    /// The ways in of the picks so far.
    picked_ways: Vec<&'a [String]>,
}

impl<'a> Screen<'a> {
    pub fn new(series: &'a HashMap<i64, Vec<String>>, limit: usize) -> Self {
        Screen { series, taken: HashSet::new(), second_chances: 0, limit, ways: None, picked_ways: vec![] }
    }

    /// Picks each a way into something different (the New to you mode).
    pub fn spread(mut self, ways: Option<&'a HashMap<i64, Vec<String>>>) -> Self {
        self.ways = ways.filter(|w| !w.is_empty());
        self
    }

    /// The picks are in: spares needn't be a way into something else, as
    /// one only takes the place of a pick.
    pub fn picks_done(&mut self) {
        self.ways = None;
    }

    /// Whether each pick so far, and this game, can each stand for a
    /// different way in. True when ways aren't checked.
    pub fn ways_fit(&self, g: &GameView) -> bool {
        let Some(ways) = self.ways else { return true };
        let Some(own) = ways.get(&g.id) else { return false };
        let mut sets = self.picked_ways.clone();
        sets.push(own);
        distinct(&sets, &mut vec![])
    }

    /// Why a game can't join, or None when it can.
    pub fn clash(&self, g: &GameView) -> Option<String> {
        if let Some(s) = self.series.get(&g.id).into_iter().flatten().find(|s| self.taken.contains(s.as_str())) {
            return Some(format!("#{} ({}) is a second game of the {s} series", g.id, g.name));
        }
        if g.offer() == Some(Offer::SecondChance) && self.second_chances >= self.limit {
            return Some(format!("#{} ({}) is a second second chance", g.id, g.name));
        }
        if let Some(ways) = self.ways
            && !self.ways_fit(g)
        {
            return Some(match ways.get(&g.id).map(Vec::as_slice).unwrap_or_default() {
                [] => format!("#{} ({}) is a way into nothing new to them", g.id, g.name),
                [one] => format!("#{} ({}) is a way into {one}, which another pick already is", g.id, g.name),
                many => format!("#{} ({}) is a way into {}, which the other picks already are", g.id, g.name, many.join(", ")),
            });
        }
        None
    }

    /// A pick joins: its series, a second chance, and what it's a way into.
    pub fn add(&mut self, g: &GameView) {
        self.add_spare(g);
        if let Some(own) = self.ways.and_then(|w| w.get(&g.id)) {
            self.picked_ways.push(own);
        }
    }

    /// A spare joins: its series and a second chance count, but it stands
    /// for no way in until it becomes a pick.
    pub fn add_spare(&mut self, g: &GameView) {
        self.taken.extend(self.series.get(&g.id).into_iter().flatten().map(String::as_str));
        if g.offer() == Some(Offer::SecondChance) {
            self.second_chances += 1;
        }
    }
}

/// Whether each set can give a different one of its ways, after `used`.
fn distinct<'a>(sets: &[&'a [String]], used: &mut Vec<&'a str>) -> bool {
    let Some((first, rest)) = sets.split_first() else { return true };
    for w in first.iter().map(String::as_str) {
        if !used.contains(&w) {
            used.push(w);
            if distinct(rest, used) {
                return true;
            }
            used.pop();
        }
    }
    false
}

/// Picks and spares the model chose, checked.
#[derive(Debug)]
struct Answer {
    picks: Vec<Pick>,
    spares: Vec<Pick>,
    /// A wishlisted game in place of the pick at this index, which becomes
    /// its alternate.
    wished: Option<(usize, Pick)>,
    /// A game you don't own, not checked yet: its name and the reason.
    suggestion: Option<(String, String)>,
}

/// Validate the model's JSON against the candidate set. Picks must be
/// exactly right, or the answer is rejected and asked again. A spare that
/// isn't (not a candidate, a second game of a series, no reason) is dropped
/// instead: spares are a reserve, and running out only asks for more.
/// Names come from the database (the model's is informational only).
fn validate(json: &Value, built: &prompt::BuiltPrompt, count: usize, mut screen: Screen) -> Result<Answer, String> {
    let raw: RawPicks = serde_json::from_value(json.clone()).map_err(|e| format!("output does not match the schema: {e}"))?;
    let mut errors = vec![];
    let mut seen: HashSet<i64> = HashSet::new();
    // A raw pick checked against the candidates and the lineup so far.
    let mut check = |r: &RawPick, screen: &mut Screen| -> Result<Pick, String> {
        let id = parse_id(&r.id).ok_or_else(|| format!("id {:?} is not a number", r.id))?;
        if !seen.insert(id) {
            return Err(format!("#{id} was listed twice"));
        }
        let g = built.candidates.get(&id).ok_or_else(|| format!("#{id} ({}) is not in the candidate list", r.name))?;
        let reason = r.reason.trim().to_string();
        if reason.is_empty() {
            return Err(format!("#{id} has an empty reason"));
        }
        if let Some(e) = screen.clash(g) {
            return Err(e);
        }
        screen.add(g);
        Ok(Pick::of(g, reason))
    };
    let mut picks = vec![];
    for r in &raw.picks {
        match check(r, &mut screen) {
            Ok(p) => picks.push(p),
            Err(e) => errors.push(e),
        }
    }
    screen.picks_done();
    if picks.len() != count {
        errors.push(format!("expected exactly {count} valid picks, got {}", picks.len()));
    }
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    let mut spares = vec![];
    for r in &raw.spares {
        match check(r, &mut screen) {
            Ok(p) => spares.push(p),
            Err(e) => tracing::info!("spare dropped: {e}"),
        }
    }
    // The wishlist's parts are extras: one that doesn't hold up is dropped,
    // and the lineup stands as picked.
    let wished = raw.wishlisted.as_ref().and_then(|w| match wished(w, built, &picks) {
        Ok(x) => Some(x),
        Err(e) => {
            tracing::info!("wishlisted pick dropped: {e}");
            None
        }
    });
    let suggestion = raw
        .suggestion
        .filter(|_| built.suggests)
        .map(|s| (s.name.trim().to_string(), s.reason.trim().to_string()))
        .filter(|(n, r)| !n.is_empty() && !r.is_empty());
    Ok(Answer { picks, spares, wished, suggestion })
}

/// The wishlisted game the model put in the lineup, checked: one listed,
/// with a reason, standing in for one of the picks, which becomes the game
/// "Switch with owned game" puts back.
fn wished(w: &RawWish, built: &prompt::BuiltPrompt, picks: &[Pick]) -> Result<(usize, Pick), String> {
    let id = parse_wish_id(&w.id).ok_or_else(|| format!("wishlist id {:?} is not w and a number", w.id))?;
    let game = built.wishlisted.get(&id).ok_or_else(|| format!("w{id} is not on the wishlist list"))?;
    let reason = w.reason.trim();
    if reason.is_empty() {
        return Err(format!("w{id} has an empty reason"));
    }
    let instead = parse_id(&w.instead_of).ok_or_else(|| format!("instead_of {:?} is not a number", w.instead_of))?;
    let at = picks.iter().position(|p| p.id == instead).ok_or_else(|| format!("instead_of #{instead} is not one of the picks"))?;
    let alt = &picks[at];
    let pick = Pick {
        id: -game.id,
        appid: Some(game.appid),
        name: game.name.clone(),
        reason: reason.to_string(),
        game: None,
        slot: None,
        feedback: None,
        rating: None,
        owned: false,
        wishlist: Some(game.clone()),
        alternate: Some(Alternate { id: alt.id, name: alt.name.clone(), reason: alt.reason.clone(), game: alt.game.clone() }),
        backlog: None,
    };
    Ok((at, pick))
}

/// One answer from the model, with what its calls took.
struct Asked {
    answer: Answer,
    attempts: i64,
    duration_ms: u64,
    cost_usd: Option<f64>,
    api_usd: Option<f64>,
    usage: Option<Usage>,
    model: Option<String>,
}

/// Ask for picks and spares, and once more with the error when the picks
/// don't hold up. `screen` is what the lineup already holds.
async fn ask<'a>(
    db: &Db,
    cfg: &Config,
    llm: &dyn LlmBackend,
    built: &prompt::BuiltPrompt,
    count: usize,
    screen: impl Fn() -> Screen<'a>,
    purpose: &Purpose,
) -> Result<Asked, PickError> {
    let mut asked = Asked {
        answer: Answer { picks: vec![], spares: vec![], wished: None, suggestion: None },
        attempts: 0,
        duration_ms: 0,
        cost_usd: None,
        api_usd: None,
        usage: None,
        model: None,
    };
    let mut prompt_text = built.user.clone();
    loop {
        asked.attempts += 1;
        let req = LlmRequest {
            system: built.system.clone(),
            cached_prefix: Some(built.prefix.clone()),
            prompt: prompt_text.clone(),
            schema: built.schema.clone(),
            model: cfg.pick_model(),
            background: false,
            effort: None,
        };
        let resp = ai_usage::call(db, llm, purpose, req).await?;
        asked.duration_ms += resp.duration_ms;
        if let Some(c) = resp.cost_usd {
            asked.cost_usd = Some(asked.cost_usd.unwrap_or(0.0) + c);
        }
        if let Some(c) = resp.api_usd {
            asked.api_usd = Some(asked.api_usd.unwrap_or(0.0) + c);
        }
        if let Some(u) = resp.usage {
            *asked.usage.get_or_insert_default() += u;
        }
        asked.model = resp.model.clone().or(asked.model);
        match validate(&resp.json, built, count, screen()) {
            Ok(a) => {
                asked.answer = a;
                return Ok(asked);
            }
            Err(e) => {
                tracing::warn!("attempt {} rejected: {e}", asked.attempts);
                if asked.attempts >= 2 {
                    return Err(PickError::Invalid(e));
                }
                // The reasons quote game, series and store names and the
                // model's own text, any of which could carry a new line and
                // a heading, so they go in as one line.
                prompt_text = format!(
                    "{}\n\n## Correction\nYour previous answer was rejected: {}.\nReturn exactly {count} distinct #id values that appear in the candidate list above, \
                     never two of one series, with a non-empty reason for each, then the spares.",
                    built.user,
                    crate::llm::one_line(&e, CORRECTION_CHARS)
                );
            }
        }
    }
}

/// The most of a rejected answer's reasons a retry quotes: enough for
/// every pick of a full lineup to have one.
const CORRECTION_CHARS: usize = 4000;

/// How many spares to ask for with `count` picks, out of `eligible` games.
fn spares_for(count: usize, eligible: usize) -> usize {
    (count * SPARES_PER_PICK).min(eligible.saturating_sub(count))
}

/// Whether there are `count` games to pick from.
fn enough(eligible: usize, count: usize, library_empty: bool, lens: Option<&Lens>) -> Result<(), PickError> {
    if let Some(l) = lens.filter(|_| eligible < count && !library_empty) {
        return Err(PickError::NoCandidates(format!("no candidates: {}", l.too_few(eligible, count))));
    }
    if eligible == 0 {
        let hint = if library_empty {
            "your library is empty: run `upnext sync` first"
        } else {
            "no games you could play next match the selected sources/exclusions"
        };
        return Err(PickError::NoCandidates(format!("no candidates: {hint}")));
    }
    if eligible < count {
        return Err(PickError::NoCandidates(format!("only {eligible} eligible games, cannot pick {count}")));
    }
    Ok(())
}

/// Whether to pick from UpNext's own ranking: with no AI set up, or when
/// asked to.
async fn without_ai(llm: &dyn LlmBackend, asked: bool) -> bool {
    asked || llm.available().await.is_err()
}

/// A different jitter for every lineup without AI, so lineups differ. (The
/// AI's lineups keep one per request and day, for the prompt cache.)
fn fresh_seed() -> u64 {
    uuid::Uuid::new_v4().as_u64_pair().0 | 1
}

/// A request's mode and the rest of its words: an explicit mode, or one
/// its words ask for ("new to me, 2 hours").
fn mode_of(prompt: &str, mode: Option<Mode>) -> (Option<Mode>, String) {
    let (found, rest) = Mode::detect(prompt);
    match (mode, found) {
        (Some(m), _) => (Some(m), if found.is_some() { rest } else { prompt.trim().to_string() }),
        (None, f) => (f, rest),
    }
}

/// What a lineup is picked from: the player's games with their series, the
/// games picked lately, the taste profile, and the mode's lens with the ways
/// into each of its games.
struct Pool {
    games: std::sync::Arc<Vec<GameView>>,
    series: HashMap<i64, Vec<String>>,
    recent_ids: Vec<i64>,
    profile: crate::taste::Profile,
    lens: Option<Lens>,
    ways: Option<HashMap<i64, Vec<String>>>,
}

/// Runs the reading and ranking of a pick, which take some seconds of one
/// thread on a very large library, without holding up the server's other
/// requests: on a multi-threaded runtime (the server's and the command
/// line's) the async worker first hands its other tasks to another thread.
/// A current-thread runtime, as most tests use, has no other thread to hand
/// them to, so there the work just runs.
fn off_the_workers<T>(work: impl FnOnce() -> T) -> T {
    match tokio::runtime::Handle::try_current().map(|h| h.runtime_flavor()) {
        Ok(tokio::runtime::RuntimeFlavor::MultiThread) => tokio::task::block_in_place(work),
        _ => work(),
    }
}

impl Pool {
    fn read(db: &Db, cfg: &Config, mode: Option<Mode>) -> Result<Pool, PickError> {
        let games = db.all_games_shared(cfg.whose())?;
        let series = crate::series::of_titles(&games, &db.catalog()?);
        let recent_ids = db.recent_pick_ids(cfg.whose(), 3)?;
        let profile = crate::taste::Profile::load(db, cfg.whose(), &games)?;
        let lens = mode.map(|m| Lens::build(db, cfg, m, &games, Some(&profile))).transpose()?;
        let ways = lens.as_ref().map(Lens::ways);
        Ok(Pool { games, series, recent_ids, profile, lens, ways })
    }
}

/// A lineup for a request. `store` reaches Steam's store to check the
/// AI's suggestion of a game you don't own; without it, none is shown.
#[tracing::instrument(target = "upnext::timing", name = "pick", skip_all)]
pub async fn pick(
    db: &Db,
    cfg: &Config,
    llm: &dyn LlmBackend,
    store: Option<&crate::enrich::Clients>,
    req: PickRequest,
) -> Result<PickBatch, PickError> {
    req.check()?;
    let count = req.count.clamp(1, 5);
    let (mode, request) = mode_of(&req.prompt, req.mode);
    let Pool { games, series, recent_ids, profile, lens, ways } = off_the_workers(|| Pool::read(db, cfg, mode))?;
    let routine = match req.routine_id {
        Some(id) => Some(db.routine(cfg.whose(), id)?.ok_or_else(|| PickError::NotFound(format!("no routine {id}")))?),
        None => None,
    };
    // The wishlist joins lineups from the whole library, picked by the AI.
    let wishlist = match cfg.wishlist && mode.is_none() {
        true => Some(off_the_workers(|| crate::wishlist::picks::for_picks(db, cfg.whose(), &games))?),
        false => None,
    };
    let input = |spares: usize, seed: u64| PromptInput {
        games: &games,
        request: &request,
        count,
        spares,
        exclude_ids: &req.exclude_ids,
        avoid: &req.avoid,
        on_screen: &[],
        series: &series,
        now: crate::db::now(),
        sources: req.sources.as_deref(),
        recent_ids: &recent_ids,
        max_candidates: cfg.max_candidates,
        sampled_minutes: cfg.sampled_minutes,
        linux: cfg.linux(),
        seed,
        review_feel: true,
        verdicts: cfg.verdicts_prompt,
        profile: Some(&profile),
        lens: lens.as_ref(),
        wishlist: wishlist.as_deref(),
        routine: routine.as_ref(),
        asked_input: req.input,
    };
    // Rolling the dice never asks the AI: it's meant to be instant.
    if mode == Some(Mode::Random) {
        return off_the_workers(|| pick_random(db, cfg.whose(), &req, &input(count * SPARES_PER_PICK, fresh_seed())));
    }
    if without_ai(llm, req.without_ai).await {
        return off_the_workers(|| pick_ranked(db, cfg.whose(), &req, &input(count * SPARES_PER_PICK, fresh_seed()), ways.as_ref()));
    }
    // 0: a stable seed per request and day, so rerolls hit the prompt cache.
    let prompt_span = tracing::info_span!(target: "upnext::timing", "prompt");
    let mut built = off_the_workers(|| prompt_span.in_scope(|| prompt::build(&input(count * SPARES_PER_PICK, 0))));
    enough(built.eligible, count, games.is_empty(), lens.as_ref())?;
    let spares = spares_for(count, built.eligible);
    if spares < count * SPARES_PER_PICK {
        built = off_the_workers(|| prompt_span.in_scope(|| prompt::build(&input(spares, 0))));
    }

    // Made first, so each call is recorded with the batch it's for.
    let batch_id = uuid::Uuid::new_v4().to_string();
    let purpose = Purpose::new(AiKind::Pick).batch(&batch_id);
    let limit = built.second_chances;
    let asked = ask(db, cfg, llm, &built, count, || Screen::new(&series, limit).spread(ways.as_ref()), &purpose)
        .instrument(tracing::info_span!(target: "upnext::timing", "model"))
        .await?;

    let mut picks = asked.answer.picks;
    if let Some((at, wished)) = asked.answer.wished {
        picks[at] = wished;
    }
    for (i, p) in picks.iter_mut().enumerate() {
        p.slot = Some(i);
    }
    let suggestion = match (asked.answer.suggestion, store) {
        (Some((name, reason)), Some(store)) => {
            match crate::wishlist::picks::check_suggestion(db, cfg.whose(), store, &games, Some(&profile), &name, &reason, &batch_id).await
            {
                Ok(s) => Some(s),
                Err(why) => {
                    tracing::info!("suggestion {name:?} not shown: {why}");
                    None
                }
            }
        }
        _ => None,
    };
    let mut batch = PickBatch {
        batch_id,
        prompt: req.prompt.trim().to_string(),
        created_at: crate::db::now(),
        model: asked.model,
        duration_ms: Some(asked.duration_ms as i64),
        cost_usd: asked.cost_usd,
        api_usd: asked.api_usd,
        candidates_considered: Some(built.considered as i64),
        attempts: Some(asked.attempts),
        usage: asked.usage,
        shown: count,
        picks,
        spares: asked.answer.spares,
        removed: vec![],
        ranked_by: None,
        mode,
        suggestion,
        routine,
    };
    db.save_batch(cfg.whose(), &batch)?;
    batch.fill_backlog_lines();
    Ok(batch)
}

/// A lineup from UpNext's own ranking (`ranking::lineup`), with no AI call.
/// The reasons the excluded games were turned down for steer it, as far as
/// it can read them.
fn pick_ranked(
    db: &Db,
    p: PlayerId,
    req: &PickRequest,
    input: &PromptInput,
    ways: Option<&HashMap<i64, Vec<String>>>,
) -> Result<PickBatch, PickError> {
    let started = std::time::Instant::now();
    let turned_down: Vec<&GameView> = input.games.iter().filter(|g| req.exclude_ids.contains(&g.id)).collect();
    let said: Vec<Said> = req.avoid.iter().map(|r| Said { reason: r.clone(), about: turned_down.clone() }).collect();
    let ranked = ranking::lineup(input, &said, Screen::new(input.series, input.second_chances()).spread(ways));
    enough(ranked.eligible, input.count, input.games.is_empty(), input.lens)?;
    if ranked.picks.len() < input.count {
        return Err(PickError::NoCandidates(format!(
            "only {} of the eligible games can share a lineup (one game per series{}), cannot pick {}",
            ranked.picks.len(),
            if ways.is_some_and(|w| !w.is_empty()) { ", each a way into something different" } else { "" },
            input.count
        )));
    }
    let mut picks = ranked.picks;
    for (i, p) in picks.iter_mut().enumerate() {
        p.slot = Some(i);
    }
    let mut batch = PickBatch {
        batch_id: uuid::Uuid::new_v4().to_string(),
        prompt: req.prompt.trim().to_string(),
        created_at: crate::db::now(),
        model: None,
        duration_ms: Some(started.elapsed().as_millis() as i64),
        cost_usd: None,
        api_usd: None,
        candidates_considered: Some(ranked.eligible as i64),
        attempts: Some(0),
        usage: None,
        shown: input.count,
        picks,
        spares: ranked.spares,
        removed: vec![],
        ranked_by: Some(ranked.ranked_by),
        mode: input.lens.map(|l| l.mode),
        suggestion: None,
        routine: input.routine.cloned(),
    };
    db.save_batch(p, &batch)?;
    batch.fill_backlog_lines();
    Ok(batch)
}

/// A lineup drawn at random from the games off your radar, with no
/// AI call. Its `ranked_by` says so, so History, "It's too ___" and new
/// spares treat it as a lineup picked without AI.
fn pick_random(db: &Db, p: PlayerId, req: &PickRequest, input: &PromptInput) -> Result<PickBatch, PickError> {
    let started = std::time::Instant::now();
    let drawn = crate::discovery::random::draw(input, Screen::new(input.series, input.second_chances()));
    enough(drawn.eligible, input.count, input.games.is_empty(), input.lens)?;
    if drawn.picks.len() < input.count {
        return Err(PickError::NoCandidates(format!(
            "only {} of the games off your radar can share a lineup (one game per series), cannot pick {}",
            drawn.picks.len(),
            input.count
        )));
    }
    let mut picks = drawn.picks;
    for (i, p) in picks.iter_mut().enumerate() {
        p.slot = Some(i);
    }
    let mut batch = PickBatch {
        batch_id: uuid::Uuid::new_v4().to_string(),
        prompt: req.prompt.trim().to_string(),
        created_at: crate::db::now(),
        model: None,
        duration_ms: Some(started.elapsed().as_millis() as i64),
        cost_usd: None,
        api_usd: None,
        candidates_considered: Some(drawn.eligible as i64),
        attempts: Some(0),
        usage: None,
        shown: input.count,
        picks,
        spares: drawn.spares,
        removed: vec![],
        ranked_by: Some(vec![format!(
            "chance alone: drawn at random from {} off your radar",
            crate::enrich::plural(drawn.eligible, "game", "games")
        )]),
        mode: Some(Mode::Random),
        suggestion: None,
        // Chance doesn't read a routine, so the lineup isn't for one.
        routine: None,
    };
    db.save_batch(p, &batch)?;
    batch.fill_backlog_lines();
    Ok(batch)
}

/// What the lineup's turned-down picks were turned down for, to steer new
/// spares away: "too long".
pub(crate) fn turned_down_for(batch: &PickBatch) -> Vec<String> {
    let mut why: Vec<String> = vec![];
    for p in &batch.removed {
        if let Some(f) = &p.feedback
            && f.action == FeedbackAction::Too
            && let Some(w) = &f.text
        {
            let w = format!("too {w}");
            if !why.contains(&w) {
                why.push(w);
            }
        }
    }
    why
}

/// New spares for a lineup whose spares ran out: the empty slots are
/// filled from them, the rest kept as spares. Every game the lineup has
/// held is excluded, and its "It's too ___" words steer the new ones.
pub async fn more(
    db: &Db,
    cfg: &Config,
    llm: &dyn LlmBackend,
    batch_id: &str,
    sources: Option<Vec<String>>,
    input: Option<crate::taste::routine::Input>,
) -> Result<PickBatch, PickError> {
    let batch = db.pick_batch(cfg.whose(), batch_id)?.ok_or_else(|| PickError::NotFound(format!("no lineup {batch_id}")))?;
    let empty = batch.empty_slots();
    if empty.is_empty() {
        return Ok(batch);
    }
    let (mode, request) = mode_of(&batch.prompt, batch.mode);
    let Pool { games, series, recent_ids, profile, lens, ways } = off_the_workers(|| Pool::read(db, cfg, mode))?;
    // A wishlisted pick's owned alternate counts as on screen: its switch
    // may bring it on at any time.
    let alternates: Vec<&crate::db::Alternate> = batch.picks.iter().filter_map(|p| p.alternate.as_ref()).collect();
    let on_screen: Vec<i64> = batch.picks.iter().map(|p| p.id).chain(alternates.iter().map(|a| a.id)).collect();
    let exclude: Vec<i64> = batch.spares.iter().chain(&batch.removed).map(|p| p.id).collect();
    let avoid = turned_down_for(&batch);
    let count = empty.len();
    // A lineup picked without AI goes on without it.
    let ranked = batch.ranked_by.is_some() || without_ai(llm, false).await;
    let input = |spares: usize| PromptInput {
        games: &games,
        request: &request,
        count,
        spares,
        exclude_ids: &exclude,
        avoid: &avoid,
        on_screen: &on_screen,
        series: &series,
        now: crate::db::now(),
        sources: sources.as_deref(),
        recent_ids: &recent_ids,
        max_candidates: cfg.max_candidates,
        sampled_minutes: cfg.sampled_minutes,
        linux: cfg.linux(),
        seed: if ranked { fresh_seed() } else { 0 },
        review_feel: true,
        verdicts: cfg.verdicts_prompt,
        profile: Some(&profile),
        lens: lens.as_ref(),
        wishlist: None,
        routine: batch.routine.as_ref(),
        asked_input: input,
    };
    let none_left = || PickError::NoCandidates("no more games fit this lineup: every game left was turned down or is on screen".into());
    let on_screen_games: Vec<&GameView> =
        batch.picks.iter().filter_map(|p| p.game.as_ref()).chain(alternates.iter().filter_map(|a| a.game.as_ref())).collect();
    let screen = |limit: usize| {
        let mut s = Screen::new(&series, limit).spread(ways.as_ref());
        for g in &on_screen_games {
            s.add(g);
        }
        s
    };
    let (mut new, spares) = if mode == Some(Mode::Random) {
        // A lineup drawn at random goes on at random.
        let input = input(batch.shown * SPARES_PER_PICK);
        let drawn = off_the_workers(|| crate::discovery::random::draw(&input, screen(input.second_chances())));
        if drawn.picks.is_empty() {
            return Err(none_left());
        }
        (drawn.picks, drawn.spares)
    } else if ranked {
        let input = input(batch.shown * SPARES_PER_PICK);
        // Each "It's too ___" word steers away from games like the one it was said about.
        let by_id = |id: i64| games.iter().find(|g| g.id == id);
        let said: Vec<Said> = batch
            .removed
            .iter()
            .filter_map(|p| {
                let f = p.feedback.as_ref().filter(|f| matches!(f.action, FeedbackAction::Too | FeedbackAction::SameProblem))?;
                Some(Said { reason: format!("too {}", f.text.as_deref()?), about: vec![by_id(p.id)?] })
            })
            .collect();
        let r = off_the_workers(|| ranking::lineup(&input, &said, screen(input.second_chances())));
        if r.picks.is_empty() {
            return Err(none_left());
        }
        (r.picks, r.spares)
    } else {
        let mut built = off_the_workers(|| prompt::build(&input(batch.shown * SPARES_PER_PICK)));
        if built.eligible < count {
            return Err(none_left());
        }
        let spares = (batch.shown * SPARES_PER_PICK).min(built.eligible - count);
        if spares < batch.shown * SPARES_PER_PICK {
            built = off_the_workers(|| prompt::build(&input(spares)));
        }
        let limit = built.second_chances;
        let purpose = Purpose::new(AiKind::Pick).batch(batch_id);
        let asked = ask(db, cfg, llm, &built, count, || screen(limit), &purpose).await?;
        (asked.answer.picks, asked.answer.spares)
    };
    for (p, slot) in new.iter_mut().zip(&empty) {
        p.slot = Some(*slot);
    }
    new.extend(spares);
    db.add_to_batch(batch_id, &new)?;
    Ok(db.pick_batch(cfg.whose(), batch_id)?.expect("the lineup was just read"))
}

#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod wishlist_tests;
