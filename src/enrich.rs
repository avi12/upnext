//! Sync (pull libraries) and enrich (fetch what each source knows about each
//! title) pipelines. Both report through a `ProgressSink` so the CLI can draw
//! a progress bar and the server can expose job status.
//!
//! Enrichment's sources are listed in `sources.rs`, one file each. This file
//! runs them: the passes before the lanes, then the Steam store step and one
//! lane per rate-limited host at the same time, then the passes after.

pub mod sources;
pub(crate) mod sync;

#[cfg(test)]
mod pipeline_tests;

pub(crate) use sync::counts_text as sync_counts_text;
pub use sync::{Library, SyncOptions, SyncReport, libraries as sync_libraries, run_sync};

use crate::config::Config;
use crate::db::PlayerId;
use crate::db::{Db, Failure, GameView, LaneProgress, LaneState, Level};
use crate::gog::GogClient;
use crate::gogmeta::GogMetaClient;
use crate::hltb::HltbClient;
use crate::igdb::IgdbClient;
use crate::llm::LlmBackend;
use crate::pcgw::PcgwClient;
use crate::protondb::ProtonDbClient;
use crate::rawg::RawgClient;
use crate::signin::DbSignIn;
use crate::steam::{self, SteamClient};
use crate::steamspy::SteamSpyClient;
use crate::wikidata::WikidataClient;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use sources::{Fetch, Pass, TitleSource};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub total: usize,
    pub done: usize,
    pub current: Option<String>,
    pub message: String,
}

pub trait ProgressSink: Send + Sync {
    fn update(&self, p: &Progress);
    fn log(&self, line: String);
    /// A line that is a warning or an error, for sinks that keep levels.
    fn log_at(&self, _level: Level, line: String) {
        self.log(line);
    }
    /// A line that names one game, `title_id`, so the Jobs page can say "a
    /// hidden game" in its place to a player it's hidden from.
    fn log_game(&self, level: Level, _title_id: i64, line: String) {
        self.log_at(level, line);
    }
    fn cancelled(&self) -> bool {
        false
    }
    /// Every lane's state, whenever one changes (enrich only).
    fn lanes(&self, _lanes: &[LaneProgress]) {}
    /// Something failed for a title, or for a pass. Sinks that don't keep
    /// failures log it.
    fn failure(&self, f: &Failure) {
        self.log_at(Level::Error, failure_line(f));
    }
}

/// "Steam reviews failed for Portal 2: HTTP 429 …"
pub fn failure_line(f: &Failure) -> String {
    match &f.name {
        Some(name) => format!("{} failed for {name}: {}", f.source, f.error),
        None => format!("{} failed: {}", f.source, f.error),
    }
}

/// All outbound clients, built once per process.
pub struct Clients {
    pub http: reqwest::Client,
    pub steam: SteamClient,
    pub spy: SteamSpyClient,
    pub hltb: HltbClient,
    pub gog: std::sync::Arc<GogClient>,
    pub epic: std::sync::Arc<crate::epic::EpicClient>,
    pub gogmeta: GogMetaClient,
    pub wikidata: WikidataClient,
    pub pcgw: PcgwClient,
    pub protondb: ProtonDbClient,
    /// Stands in for every service's host when set; clients built later (for
    /// the current credentials) take it too. None means the real services.
    pub base_url: Option<String>,
}

impl Clients {
    pub fn new(cfg: &Config) -> Result<Self> {
        Self::with_base_url(cfg, None)
    }

    /// Clients that send every request to `base` instead of the real
    /// services when it is set. Tests pass a local mock server.
    pub fn with_base_url(cfg: &Config, base: Option<&str>) -> Result<Self> {
        let http = crate::http::client()?;
        Ok(Self {
            http: http.clone(),
            steam: SteamClient::new(http.clone(), cfg.steam_api_key.clone(), cfg.steam_id.clone(), base),
            spy: SteamSpyClient::new(http.clone(), base),
            hltb: HltbClient::new(http.clone(), base),
            gogmeta: GogMetaClient::new(http.clone(), base),
            wikidata: WikidataClient::new(http.clone(), base),
            pcgw: PcgwClient::new(http.clone(), base),
            protondb: ProtonDbClient::new(http.clone(), base),
            // Player 1's sign-ins, on their connections; a sync
            // reads each other player's through `with_store`.
            epic: std::sync::Arc::new(crate::epic::EpicClient::stored_in(
                http.clone(),
                std::sync::Arc::new(DbSignIn::own(cfg.db_path.clone(), "epic", PlayerId::FIRST, Some(cfg.epic_token_path()))),
                cfg.cache_dir.join("epic-catalog.json"),
                base,
            )),
            gog: std::sync::Arc::new(GogClient::stored_in(
                http,
                std::sync::Arc::new(DbSignIn::own(cfg.db_path.clone(), "gog", PlayerId::FIRST, Some(cfg.gog_token_path.clone()))),
                base,
            )),
            base_url: base.map(str::to_string),
        })
    }
}

// ---------------------------------------------------------------------------
// Options and results
// ---------------------------------------------------------------------------

/// What an enrich run fetches. In JSON every source's option is a key of its
/// own next to `refresh` and `limit`: `{"hltb": true, "rawg": false}`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnrichOptions {
    /// Sources on or off by option id (`sources::options()`). An id left out
    /// takes its default; an unknown id is ignored.
    #[serde(flatten)]
    pub sources: BTreeMap<String, bool>,
    /// Fetch again for titles that already have it.
    #[serde(default)]
    pub refresh: bool,
    /// Process at most this many titles.
    #[serde(default)]
    pub limit: Option<usize>,
    /// Only these titles, by id; empty means every title.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub titles: Vec<i64>,
}

impl EnrichOptions {
    pub fn on(&self, id: &str) -> bool {
        self.sources.get(id).copied().unwrap_or_else(|| sources::default_on(id))
    }

    /// Every option spelled out and unknown ids dropped: what a job reports.
    pub fn resolved(&self) -> Self {
        let sources = sources::options().iter().map(|o| (o.id.to_string(), self.on(o.id))).collect();
        Self { sources, refresh: self.refresh, limit: self.limit, titles: self.titles.clone() }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct EnrichSummary {
    pub processed: usize,
    /// Non-Steam titles matched to a Steam app.
    pub matches: usize,
    /// Steam store pages read.
    pub details: usize,
    /// What each source stored, by option id.
    pub counts: BTreeMap<&'static str, usize>,
    pub errors: usize,
    /// Sources that stopped before the end of the run (a service down, a key
    /// refused), so the job's history says the run is not complete.
    pub stopped: Vec<&'static str>,
}

impl EnrichSummary {
    pub fn count(&self, id: &str) -> usize {
        self.counts.get(id).copied().unwrap_or(0)
    }

    /// "1,600 titles: 3 matched, 12 details, 1,500 reviews, 5 errors", what
    /// a job's history shows: only what it did.
    pub fn summary_text(&self) -> String {
        let mut parts = vec![];
        if self.matches > 0 {
            parts.push(format!("{} matched", thousands(self.matches)));
        }
        if self.details > 0 {
            parts.push(plural(self.details, "detail", "details"));
        }
        parts.extend(sources::options().iter().filter(|o| self.count(o.id) > 0).map(|o| counted(self.count(o.id), o.id)));
        if self.errors > 0 {
            parts.push(plural(self.errors, "error", "errors"));
        }
        let titles = plural(self.processed, "title", "titles");
        let text = match (parts.is_empty(), self.processed) {
            (true, _) => titles,
            // Only passes ran (series guides, second chances): no titles went through the lanes.
            (false, 0) => parts.join(", "),
            (false, _) => format!("{titles}: {}", parts.join(", ")),
        };
        if self.stopped.is_empty() {
            return text;
        }
        format!("{text}; stopped early: {}", self.stopped.join(", "))
    }

    /// "0 matched, 2 details, 2 wikidata, …, 0 errors", sources in run order.
    pub fn counts_text(&self) -> String {
        let mut parts = vec![format!("{} matched", self.matches), plural(self.details, "detail", "details")];
        parts.extend(sources::options().iter().map(|o| counted(self.count(o.id), o.id)));
        parts.push(plural(self.errors, "error", "errors"));
        parts.join(", ")
    }
}

/// "1,500 reviews", "1 second chance": a count of what a source did, named
/// by its id. The ids are plural nouns ("second_chances") or read the same
/// either way ("series", "wikidata"), so one drops a plural's "s".
fn counted(n: usize, id: &str) -> String {
    let what = id.replace('_', " ");
    match what.strip_suffix('s') {
        Some(one) if n == 1 && id != "series" => format!("1 {one}"),
        _ => format!("{} {what}", thousands(n)),
    }
}

/// "1 game", "1,234 games": a count with its noun, singular for one. A
/// count below zero, which none should be, reads as 0.
pub fn plural(n: impl TryInto<usize>, one: &str, many: &str) -> String {
    let n = n.try_into().unwrap_or(0);
    format!("{} {}", thousands(n), if n == 1 { one } else { many })
}

/// 1234 -> "1,234".
pub fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

// ---------------------------------------------------------------------------
// One run
// ---------------------------------------------------------------------------

/// One enrich run: what every source reads, and the counts they add to.
pub struct Run<'a> {
    pub db: &'a Db,
    pub cfg: &'a Config,
    pub clients: &'a Clients,
    /// The AI for series guides, which reason like picks do.
    pub llm: Option<&'a dyn LlmBackend>,
    /// The AI for review summaries, second chances and hidden gems: the
    /// summaries backend when Settings names one of its own, else `llm`.
    pub summary_llm: Option<&'a dyn LlmBackend>,
    /// The tracker, which passes everything on to the job's sink.
    pub sink: &'a dyn ProgressSink,
    tracker: &'a Tracker<'a>,
    pub refresh: bool,
    pub limit: Option<usize>,
    titles: Vec<i64>,
    /// Some usual source is off (`--only`, `--skip`): a refresh then leaves
    /// the Steam store pages alone.
    narrowed: bool,
    on: HashMap<&'static str, bool>,
    igdb: tokio::sync::OnceCell<Option<IgdbClient>>,
    rawg: tokio::sync::OnceCell<Option<RawgClient>>,
    summary: std::sync::Mutex<EnrichSummary>,
}

impl<'a> Run<'a> {
    /// Whether to read Steam store pages and matches again: on a refresh,
    /// unless the run is narrowed to some sources.
    fn refresh_store(&self) -> bool {
        self.refresh && !self.narrowed
    }

    /// Whether this run covers the title: every title, or the ones asked for.
    pub fn includes(&self, g: &GameView) -> bool {
        self.titles.is_empty() || self.titles.contains(&g.id)
    }

    /// Whether the run names this title (`--titles`): titles with summaries
    /// from shared data are fetched and written again only then.
    pub fn named(&self, g: &GameView) -> bool {
        self.titles.contains(&g.id)
    }

    fn new(
        db: &'a Db,
        cfg: &'a Config,
        clients: &'a Clients,
        llm: Option<&'a dyn LlmBackend>,
        summary_llm: Option<&'a dyn LlmBackend>,
        opts: &EnrichOptions,
        tracker: &'a Tracker<'a>,
    ) -> Self {
        Self {
            db,
            cfg,
            clients,
            llm,
            summary_llm,
            sink: tracker,
            tracker,
            refresh: opts.refresh,
            limit: opts.limit,
            titles: opts.titles.clone(),
            narrowed: sources::options().iter().any(|o| sources::default_on(o.id) && !opts.on(o.id)),
            on: sources::options().iter().map(|o| (o.id, opts.on(o.id))).collect(),
            igdb: Default::default(),
            rawg: Default::default(),
            summary: Default::default(),
        }
    }

    /// Whether an option is on for this run.
    pub fn on(&self, id: &str) -> bool {
        self.on.get(id).copied().unwrap_or(false)
    }

    pub fn count(&self, id: &'static str) {
        self.add(id, 1);
    }

    pub fn add(&self, id: &'static str, n: usize) {
        self.with_summary(|s| *s.counts.entry(id).or_default() += n);
    }

    fn with_summary<T>(&self, f: impl FnOnce(&mut EnrichSummary) -> T) -> T {
        f(&mut self.summary.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn log(&self, line: String) {
        self.sink.log(line);
    }

    pub fn log_at(&self, level: Level, line: String) {
        self.sink.log_at(level, line);
    }

    /// Log a failure and count it.
    pub fn error(&self, line: String) {
        self.with_summary(|s| s.errors += 1);
        self.sink.log_at(Level::Error, line);
    }

    /// A line about one game (`ProgressSink::log_game`).
    pub fn log_game(&self, level: Level, title_id: i64, line: String) {
        self.sink.log_game(level, title_id, line);
    }

    /// Log a failure for one game and count it.
    pub fn error_game(&self, title_id: i64, line: String) {
        self.with_summary(|s| s.errors += 1);
        self.sink.log_game(Level::Error, title_id, line);
    }

    /// Something failed for a title (or for a pass): counted, logged, and
    /// listed with the job's failures so it can be run again.
    pub fn fail(&self, mut f: Failure) {
        // A request cut short by a cancel isn't a failure to run again: the
        // title is simply not done, and the next run takes it.
        if self.sink.cancelled() {
            return;
        }
        f.at = crate::db::now();
        self.with_summary(|s| s.errors += 1);
        self.sink.failure(&f);
    }

    /// The options of a source that are on in this run, for its failures.
    pub fn options_on(&self, opts: &[sources::Opt]) -> Vec<String> {
        opts.iter().filter(|o| self.on(o.id)).map(|o| o.id.to_string()).collect()
    }

    /// The pass running now gives up for the rest of this run.
    pub fn stop_pass(&self, note: String) {
        if let Some(lane) = self.tracker.pass() {
            self.sink.log_at(Level::Error, format!("{lane} stopped: {note}"));
            self.tracker.stop(lane, note);
        }
    }

    /// An error once the user has cancelled the job.
    pub fn check_cancelled(&self) -> Result<()> {
        if self.sink.cancelled() {
            bail!("cancelled");
        }
        Ok(())
    }

    /// The IGDB client for this run's credentials, set up and checked on first
    /// use. None without credentials or when Twitch turns them down (logged).
    pub async fn igdb(&self) -> Option<&IgdbClient> {
        self.igdb
            .get_or_init(|| async {
                let (Some(id), Some(secret)) = (&self.cfg.igdb_client_id, &self.cfg.igdb_client_secret) else { return None };
                let c = IgdbClient::new(self.clients.http.clone(), id.clone(), secret.clone(), self.clients.base_url.as_deref());
                match c.check().await {
                    Ok(()) => Some(c),
                    Err(e) => {
                        self.log(format!("IGDB disabled: {e}"));
                        None
                    }
                }
            })
            .await
            .as_ref()
    }

    /// The IGDB client, only if something in this run already set it up.
    pub fn igdb_if_started(&self) -> Option<&IgdbClient> {
        self.igdb.get().and_then(Option::as_ref)
    }

    /// The RAWG client for this run's key, checked on first use (like `igdb`).
    pub async fn rawg(&self) -> Option<&RawgClient> {
        self.rawg
            .get_or_init(|| async {
                let key = self.cfg.rawg_api_key.as_ref()?;
                let c = RawgClient::new(self.clients.http.clone(), key.clone(), self.clients.base_url.as_deref());
                match c.check().await {
                    Ok(()) => Some(c),
                    Err(e) => {
                        self.log(format!("RAWG disabled: {e}"));
                        None
                    }
                }
            })
            .await
            .as_ref()
    }
}

/// One title as a lane sees it.
pub struct Title<'a> {
    pub g: &'a GameView,
    /// Its appid now. The Steam store step may have just matched one, or
    /// dropped a bad match.
    pub appid: Option<u32>,
    /// Whether the store still calls it a game (or doesn't say).
    pub still_game: bool,
}

impl Title<'_> {
    /// The Steam store step gave it a different appid in this run.
    pub fn fresh(&self) -> bool {
        self.appid != self.g.appid
    }

    pub fn game_like(&self) -> bool {
        game_like(self.g)
    }

    /// The title to write to now: a Steam match can merge titles while the
    /// other lanes are still working, so every title-keyed write asks first.
    pub fn id(&self, db: &Db) -> Result<i64> {
        match self.g.copies.first() {
            Some(c) => Ok(db.title_of_copy(c.game_id)?.unwrap_or(self.g.id)),
            None => Ok(self.g.id),
        }
    }
}

/// The store calls it a game, or has no type for it.
fn game_like(g: &GameView) -> bool {
    g.type_.as_deref().is_none_or(|t| t == "game")
}

// ---------------------------------------------------------------------------
// Enrich
// ---------------------------------------------------------------------------

/// `llm` is the picks' backend. Summaries go to their own when Settings
/// names one; a run without the AI has neither.
pub async fn run_enrich(
    db: &Db,
    cfg: &Config,
    clients: &Clients,
    llm: Option<&dyn LlmBackend>,
    opts: &EnrichOptions,
    sink: &dyn ProgressSink,
) -> Result<EnrichSummary> {
    let own = llm.filter(|_| crate::llm::summaries_apart(cfg)).map(|_| crate::llm::summary_backend(cfg));
    run_enrich_with(db, cfg, clients, llm, own.as_deref().or(llm), opts, sink).await
}

/// An enrich run with both backends given: `llm` for series guides, and
/// `summary_llm` for review summaries, second chances and hidden gems.
#[tracing::instrument(target = "upnext::timing", name = "enrich", skip_all)]
pub async fn run_enrich_with(
    db: &Db,
    cfg: &Config,
    clients: &Clients,
    llm: Option<&dyn LlmBackend>,
    summary_llm: Option<&dyn LlmBackend>,
    opts: &EnrichOptions,
    sink: &dyn ProgressSink,
) -> Result<EnrichSummary> {
    let registry = sources::registry();
    let tracker = Tracker::new(sink);
    let run = Run::new(db, cfg, clients, llm, summary_llm, opts, &tracker);
    // Which sources run, asked once up front (the IGDB and RAWG keys are checked here).
    let mut before: Vec<&dyn Pass> = vec![];
    for p in &registry.before {
        if p.wanted(&run) && p.available(&run).await {
            before.push(p.as_ref());
        }
    }
    let mut titles: Vec<&dyn TitleSource> = vec![];
    for s in &registry.titles {
        if s.wanted(&run) && s.available(&run).await {
            titles.push(s.as_ref());
        }
    }
    let mut after: Vec<&dyn Pass> = vec![];
    for p in &registry.after {
        if p.wanted(&run) && p.available(&run).await {
            after.push(p.as_ref());
        }
    }

    let pruned = db.prune_bad_matches(0.86)?;
    if pruned > 0 {
        run.log(format!("{pruned} loose Steam matches dropped; they will be retried with the strict rule"));
    }
    for p in &before {
        run_pass(*p, &run).await?;
    }

    let all = db.all_games(PlayerId::FIRST)?;
    for s in &titles {
        s.start(&all, &run);
    }
    let store = SteamStore::new(&all, &run);
    let mut work: Vec<GameView> = all
        .into_iter()
        .filter(|g| run.includes(g))
        .filter(|g| store.needs(g, &run).any() || titles.iter().any(|s| s.needs(g, &run)))
        .collect();
    // Games most likely to be picked first: candidates, installed, then the
    // best known (most reviewed), so a first run is useful long before it ends.
    work.sort_by_key(|g| {
        (
            g.offer().is_none(),
            g.status == crate::db::Status::Hidden,
            !g.installed,
            std::cmp::Reverse(g.review_total.unwrap_or(0)),
            g.name.to_lowercase(),
        )
    });
    if let Some(limit) = run.limit {
        work.truncate(limit);
    }
    run.log(format!("{} enrichment", plural(work.len(), "title needs", "titles need")));

    run_lanes(&run, &work, store, &titles).await?;
    run.with_summary(|s| s.processed = work.len());

    for p in &after {
        run_pass(*p, &run).await?;
    }

    let summary = run.with_summary(|s| s.clone());
    sink.update(&Progress { total: work.len(), done: work.len(), current: None, message: "Enrichment complete".into() });
    sink.log(format!("done: {} processed, {}", summary.processed, summary.counts_text()));
    Ok(summary)
}

/// One pass, in a lane of its own: its progress, waits and failures are the lane's.
async fn run_pass(p: &dyn Pass, run: &Run<'_>) -> Result<()> {
    let lane = p.lane();
    run.tracker.begin_pass(lane);
    let result = crate::jobs::in_lane(lane, p.run(run)).await;
    run.tracker.end_pass(lane);
    match result {
        Ok(()) => Ok(()),
        Err(e) if p.errors_are_fatal() => Err(e),
        Err(_) if run.sink.cancelled() => bail!("cancelled"),
        Err(e) => {
            let error = format!("{e:#} (non-fatal)");
            run.fail(Failure {
                lane: lane.into(),
                source: p.name().into(),
                options: run.options_on(p.options()),
                title_id: None,
                name: None,
                error,
                at: 0,
            });
            Ok(())
        }
    }
}

// ---- the lanes ------------------------------------------------------------

/// Progress for the job and each of its lanes. It is the run's sink: sources
/// report to it and it passes everything on, adding what each lane is doing.
/// In the lanes phase, the job's "done" is the slowest lane still running.
pub struct Tracker<'a> {
    inner: &'a dyn ProgressSink,
    state: std::sync::Mutex<TrackerState>,
}

#[derive(Default)]
struct TrackerState {
    lanes: Vec<LaneProgress>,
    /// The pass running now: `update` is its progress.
    pass: Option<&'static str>,
    /// Titles going through the lanes.
    total: usize,
    /// The lanes that take titles (not the passes).
    title_lanes: Vec<String>,
}

impl<'a> Tracker<'a> {
    fn new(inner: &'a dyn ProgressSink) -> Self {
        Self { inner, state: Default::default() }
    }

    fn with<T>(&self, f: impl FnOnce(&mut TrackerState) -> T) -> T {
        f(&mut self.state.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Change one lane (adding it the first time) and pass every lane on.
    fn lane(&self, name: &str, f: impl FnOnce(&mut LaneProgress)) {
        let lanes = self.with(|s| {
            let at = match s.lanes.iter().position(|l| l.name == name) {
                Some(i) => i,
                None => {
                    s.lanes.push(LaneProgress { name: name.into(), ..Default::default() });
                    s.lanes.len() - 1
                }
            };
            f(&mut s.lanes[at]);
            s.lanes.clone()
        });
        self.inner.lanes(&lanes);
    }

    /// A lane starts, or starts again (the Steam art pass runs twice). Its
    /// failures so far are kept.
    fn start(&self, name: &str, total: usize) {
        self.lane(name, |l| {
            *l = LaneProgress { name: l.name.clone(), total, failed: l.failed, ..Default::default() };
        });
    }

    fn begin_pass(&self, name: &'static str) {
        self.with(|s| s.pass = Some(name));
        self.start(name, 0);
    }

    fn end_pass(&self, name: &'static str) {
        self.with(|s| s.pass = None);
        self.lane(name, |l| {
            if l.state != LaneState::Stopped {
                l.state = LaneState::Done;
            }
            l.current = None;
            l.done = l.done.max(l.total);
        });
    }

    fn pass(&self) -> Option<&'static str> {
        self.with(|s| s.pass)
    }

    fn stop(&self, name: &str, note: String) {
        self.lane(name, |l| {
            l.state = LaneState::Stopped;
            l.note = Some(note);
            l.current = None;
            l.retry_at = None;
        });
    }

    /// The lanes phase begins, with this many titles for every lane.
    fn begin_lanes(&self, total: usize) {
        self.with(|s| s.total = total);
    }

    fn start_lane(&self, name: &str) {
        let total = self.with(|s| {
            s.title_lanes.push(name.to_string());
            s.total
        });
        self.start(name, total);
    }

    fn at(&self, name: &str, title: &str) {
        self.lane(name, |l| l.current = Some(title.to_string()));
    }

    fn tick(&self, name: &str) {
        self.lane(name, |l| {
            l.done += 1;
            l.current = None;
        });
        self.report();
    }

    fn finish(&self, name: &str) {
        self.lane(name, |l| {
            l.done = l.total;
            l.current = None;
            if l.state != LaneState::Stopped {
                l.state = LaneState::Done;
            }
        });
        self.report();
    }

    /// The job's progress in the lanes phase.
    fn report(&self) {
        let (total, done, message) = self.with(|s| {
            let active: Vec<&LaneProgress> =
                s.lanes.iter().filter(|l| s.title_lanes.contains(&l.name) && l.state != LaneState::Done).collect();
            let done = if s.title_lanes.is_empty() { 0 } else { active.iter().map(|l| l.done).min().unwrap_or(s.total) };
            let message = if active.is_empty() {
                "Enriching".to_string()
            } else {
                format!("in parallel: {}", active.iter().map(|l| format!("{} {}", l.name, l.done)).collect::<Vec<_>>().join(" · "))
            };
            (s.total, done, message)
        });
        self.inner.update(&Progress { total, done, current: None, message });
    }
}

impl ProgressSink for Tracker<'_> {
    fn update(&self, p: &Progress) {
        if let Some(pass) = self.pass() {
            self.lane(pass, |l| {
                l.done = p.done;
                l.total = p.total;
                l.current = p.current.clone();
            });
        }
        self.inner.update(p);
    }
    fn log(&self, line: String) {
        self.inner.log(line);
    }
    fn log_at(&self, level: Level, line: String) {
        self.inner.log_at(level, line);
    }
    fn log_game(&self, level: Level, title_id: i64, line: String) {
        self.inner.log_game(level, title_id, line);
    }
    fn cancelled(&self) -> bool {
        self.inner.cancelled()
    }
    fn failure(&self, f: &Failure) {
        self.lane(&f.lane, |l| l.failed += 1);
        self.inner.failure(f);
    }
}

/// What the Steam store step learned about a title, for the lanes that need
/// an appid.
struct Handoff {
    idx: usize,
    appid: Option<u32>,
    still_game: bool,
}

/// A source's failures in a row, and whether it has given up for the rest
/// of the run: with a service down, every request waits out its backoff,
/// so a source that keeps failing stops instead of holding the job.
pub struct Streak {
    limit: u32,
    failures: u32,
}

impl Streak {
    /// Gives up after `limit` failures in a row, or never.
    pub fn new(limit: Option<u32>) -> Self {
        Self { limit: limit.unwrap_or(u32::MAX), failures: 0 }
    }

    pub fn given_up(&self) -> bool {
        self.failures >= self.limit
    }

    pub fn succeeded(&mut self) {
        self.failures = 0;
    }

    /// Gives up at once (a rejected key).
    pub fn give_up(&mut self) {
        self.failures = u32::MAX;
    }

    /// Counts a failure. The limit, when this is the failure that reaches it.
    pub fn failed(&mut self) -> Option<u32> {
        self.failures = self.failures.saturating_add(1);
        (self.failures == self.limit).then_some(self.limit)
    }
}

/// One lane: the sources that share a host, taking each title in turn.
struct Lane<'r> {
    name: &'static str,
    /// Each source with its failures in a row.
    sources: Vec<(&'r dyn TitleSource, Streak)>,
}

impl Lane<'_> {
    async fn take(&mut self, t: &Title<'_>, run: &Run<'_>) -> Result<()> {
        run.check_cancelled()?;
        run.tracker.at(self.name, &t.g.name);
        for (s, streak) in &mut self.sources {
            if streak.given_up() || !s.wants(t, run) {
                continue;
            }
            match s.run(t, run).await {
                Ok(()) => streak.succeeded(),
                Err(e) => {
                    // A request that failed is logged and counted; anything else
                    // (a database write) ends the run.
                    let Some(Fetch(err)) = e.downcast_ref::<Fetch>() else { return Err(e) };
                    run.fail(Failure {
                        lane: self.name.into(),
                        source: s.name().into(),
                        options: run.options_on(s.options()),
                        title_id: Some(t.id(run.db).unwrap_or(t.g.id)),
                        name: Some(t.g.name.clone()),
                        error: format!("{err}"),
                        at: 0,
                    });
                    let note = if s.gives_up_on(err) {
                        streak.give_up();
                        Some(format!("{} stopped for the rest of this run", s.name()))
                    } else {
                        streak
                            .failed()
                            .map(|limit| format!("{} stopped for the rest of this run after {limit} failures in a row", s.name()))
                    };
                    if let Some(note) = note {
                        run.log_at(Level::Error, note.clone());
                        run.tracker.stop(self.name, note);
                        run.with_summary(|sum| sum.stopped.push(s.name()));
                    }
                }
            }
        }
        Ok(())
    }
}

/// Every per-title source at once, one lane per rate-limited host, so a title
/// takes as long as the slowest service instead of the sum of all.
async fn run_lanes(run: &Run<'_>, work: &[GameView], store: SteamStore, sources: &[&dyn TitleSource]) -> Result<()> {
    let mut lanes: Vec<(Lane, bool)> = vec![];
    for s in sources {
        match lanes.iter_mut().find(|(l, _)| l.name == s.lane()) {
            Some((l, _)) => l.sources.push((*s, Streak::new(s.give_up_after()))),
            None => lanes.push((Lane { name: s.lane(), sources: vec![(*s, Streak::new(s.give_up_after()))] }, s.after_steam_store())),
        }
    }
    let progress = run.tracker;
    progress.begin_lanes(work.len());
    progress.report();

    let mut handoffs = vec![];
    let mut running: Vec<std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>> = vec![];
    for (mut lane, after_steam_store) in lanes {
        let name = lane.name;
        if after_steam_store {
            // Takes each title from the Steam store step, which may have found its appid.
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Handoff>();
            handoffs.push(tx);
            running.push(Box::pin(crate::jobs::in_lane(name, async move {
                progress.start_lane(lane.name);
                while let Some(h) = rx.recv().await {
                    lane.take(&Title { g: &work[h.idx], appid: h.appid, still_game: h.still_game }, run).await?;
                    progress.tick(lane.name);
                }
                progress.finish(lane.name);
                Ok(())
            })));
        } else if work.iter().any(|g| lane.sources.iter().any(|(s, _)| s.needs(g, run))) {
            running.push(Box::pin(crate::jobs::in_lane(name, async move {
                progress.start_lane(lane.name);
                for g in work {
                    lane.take(&Title { g, appid: g.appid, still_game: game_like(g) }, run).await?;
                    progress.tick(lane.name);
                }
                progress.finish(lane.name);
                Ok(())
            })));
        }
    }
    let store = crate::jobs::in_lane(SteamStore::LANE, store.run(run, work, handoffs));
    tokio::try_join!(store, futures::future::try_join_all(running))?;
    Ok(())
}

/// The Steam store step: a name match for non-Steam copies, then the store
/// details. It always runs; the lanes that need an appid wait on it.
struct SteamStore {
    /// Appids whose store details are already stored (shared across titles).
    details_known: HashSet<u32>,
}

struct StoreNeeds {
    match_: bool,
    details: bool,
}

impl StoreNeeds {
    fn any(&self) -> bool {
        self.match_ || self.details
    }
}

impl SteamStore {
    const LANE: &str = "Steam store";

    fn new(all: &[GameView], run: &Run<'_>) -> Self {
        let details_known =
            if run.refresh_store() { HashSet::new() } else { all.iter().filter(|g| g.has_details).filter_map(|g| g.appid).collect() };
        Self { details_known }
    }

    fn needs(&self, g: &GameView, run: &Run<'_>) -> StoreNeeds {
        let native_steam = g.owned_on.iter().any(|s| s == "steam");
        StoreNeeds {
            match_: !native_steam && g.appid.is_none() && (!g.has_match_attempt || run.refresh_store()),
            details: g.appid.is_some() && (!g.has_details || run.refresh_store()),
        }
    }

    /// A store request failed for this title.
    fn fail(run: &Run<'_>, title: &Title<'_>, source: &str, error: String) {
        run.fail(Failure {
            lane: Self::LANE.into(),
            source: source.into(),
            options: vec![],
            title_id: Some(title.id(run.db).unwrap_or(title.g.id)),
            name: Some(title.g.name.clone()),
            error,
            at: 0,
        });
    }

    async fn run(mut self, run: &Run<'_>, work: &[GameView], handoffs: Vec<tokio::sync::mpsc::UnboundedSender<Handoff>>) -> Result<()> {
        let db = run.db;
        let lanes = run.tracker;
        lanes.start_lane(Self::LANE);
        let mut failures = 0u32;
        for (idx, g) in work.iter().enumerate() {
            run.check_cancelled()?;
            let needs = self.needs(g, run);
            let title = Title { g, appid: g.appid, still_game: game_like(g) };
            if needs.any() {
                lanes.at(Self::LANE, &g.name);
            }
            let mut appid = g.appid;
            let mut matched = g.steam_matched;
            let mut type_ = g.type_.clone();
            if needs.match_ {
                match steam::store_search(&run.clients.steam, &g.name).await {
                    Ok(Some((a, name, sim))) => {
                        db.put_steam_match(title.id(db)?, Some((a, &name, sim)))?;
                        appid = Some(a);
                        matched = true;
                        run.with_summary(|s| s.matches += 1);
                        failures = 0;
                        run.log(format!("matched {} -> Steam {a} ({name}, {sim:.2})", g.name));
                    }
                    Ok(None) => {
                        db.put_steam_match(title.id(db)?, None)?;
                        failures = 0;
                    }
                    Err(e) => {
                        failures += 1;
                        Self::fail(run, &title, "store search", format!("{e}"));
                    }
                }
            }
            if let Some(a) = appid
                && (needs.details || needs.match_ || appid != g.appid)
                && !self.details_known.contains(&a)
            {
                match run.clients.steam.app_details(a).await {
                    Ok(Some(d)) => {
                        db.put_details(a, Some(&d))?;
                        if matched && d.type_.as_deref() != Some("game") {
                            // A name match to DLC or a soundtrack is a bad match; drop it.
                            db.put_steam_match(title.id(db)?, None)?;
                            run.log(format!("dropped match for {}: Steam {a} is a {}", g.name, d.type_.clone().unwrap_or_default()));
                            appid = None;
                        } else {
                            type_ = d.type_.clone();
                        }
                        self.details_known.insert(a);
                        run.with_summary(|s| s.details += 1);
                        failures = 0;
                    }
                    Ok(None) => {
                        db.put_details(a, None)?;
                        self.details_known.insert(a);
                        failures = 0;
                        run.log(format!("no store page for {} ({a})", g.name));
                    }
                    Err(e) => {
                        failures += 1;
                        Self::fail(run, &title, "store details", format!("{e} (Steam app {a})"));
                    }
                }
            }
            if failures >= 5 {
                lanes.stop(Self::LANE, "5 failures in a row: the Steam store is probably rate-limiting".into());
                bail!(
                    "5 consecutive Steam store failures; the store is probably rate-limiting. Wait a few minutes and run enrich again (it resumes)."
                );
            }
            let still_game = type_.as_deref().is_none_or(|t| t == "game");
            for tx in &handoffs {
                let _ = tx.send(Handoff { idx, appid, still_game });
            }
            lanes.tick(Self::LANE);
        }
        drop(handoffs);
        lanes.finish(Self::LANE);
        Ok(())
    }
}
