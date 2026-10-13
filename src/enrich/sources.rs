//! Everything `enrich` can fetch, as a list in run order. The CLI's
//! `--only`/`--skip`/`--with`, the jobs API's options and the Settings page
//! (`GET /api/enrich/sources`) all read it.
//!
//! A source is one file under `sources/` plus one line in `registry()`. A
//! [`TitleSource`] is fetched per title in a lane of its own (one lane per
//! rate-limited host); a [`Pass`] runs once over the whole library, before or
//! after the lanes. The Steam store step (name match and store details) is not
//! here: it always runs, and the lanes that need an appid wait on it.

mod art;
mod digests;
mod gog;
mod hidden_gems;
mod hltb;
mod igdb;
mod pcgw;
mod protondb;
mod rawg;
mod second_chances;
mod series;
mod series_guides;
mod steam_reviews;
mod steamspy;
mod trailers;
mod wikidata;
mod wishlist;

use super::{Run, Title};
use crate::config::Config;
use crate::db::{AiKind, GameView};
use anyhow::Result;
use async_trait::async_trait;

/// AI batches (review summaries, series guides, second chances, hidden gems) in flight at once.
pub const AI_PARALLEL: usize = 3;

/// Failures in a row after which a title source stops for the rest of a run,
/// unless it says otherwise.
pub const GIVE_UP_AFTER: u32 = 3;

/// Runs a pass's AI calls, `AI_PARALLEL` at a time, handing each result to
/// `each` as it comes back, so the count moves call by call. Stops starting
/// calls when the model can't be used any more (a usage limit, a rejected
/// key): the calls already running still finish and are counted, and the
/// pass's lane says why it stopped. A cancel doesn't wait for them: a call
/// can take up to 15 minutes on a slow server, so the calls still running
/// are dropped (a CLI's process with them) and only the results already
/// back are kept. At the end, the job's log says what the calls used.
async fn ai_batches<'c, C, T, Fut>(
    run: &Run<'_>,
    kind: AiKind,
    chunks: &'c [C],
    call: impl Fn(&'c C) -> Fut,
    mut each: impl FnMut(&'c C, Result<T>),
) -> Result<()>
where
    C: Sync,
    Fut: std::future::Future<Output = Result<T>>,
{
    use futures::StreamExt;
    use std::sync::atomic::{AtomicBool, Ordering};
    let stop = AtomicBool::new(false);
    let mut gave_up = None;
    {
        // By index: closures over references to the chunks trip the Send
        // check `async_trait` makes on the pass's future.
        let started = (0..chunks.len()).take_while(|_| !stop.load(Ordering::Relaxed) && !run.sink.cancelled()).map(|i| {
            let pending = call(&chunks[i]);
            async move { (i, pending.await) }
        });
        let mut calls = futures::stream::iter(started).buffer_unordered(AI_PARALLEL);
        // The cancel is a flag, so it's looked at as often as the rate
        // limiters look at it while they wait.
        let cancelled = async {
            while !run.sink.cancelled() {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        };
        tokio::pin!(cancelled);
        loop {
            let next = tokio::select! {
                // A result that is back is taken before the cancel is.
                biased;
                next = calls.next() => next,
                () = &mut cancelled => break,
            };
            let Some((i, result)) = next else { break };
            let chunk = &chunks[i];
            if let Err(e) = &result
                && gave_up.is_none()
                && let Some(why) = crate::llm::gave_up(e)
            {
                gave_up = Some(why);
                stop.store(true, Ordering::Relaxed);
            }
            each(chunk, result);
        }
    }
    // The plan's last reading after these calls, before a terminal job exits.
    crate::ai_plan::settle(run.db).await;
    if let Some(job) = crate::jobs::current_job() {
        let used = run.db.ai_job_totals(job, kind)?;
        if used.calls > 0 {
            run.log(crate::ai_usage::used_line(&used));
        }
    }
    run.check_cancelled()?;
    if let Some(why) = gave_up {
        // OpenAI's messages end in a full stop of their own.
        run.stop_pass(format!("{}. What's left waits for the next run.", why.trim_end_matches('.')));
    }
    Ok(())
}

/// A request that failed. The lane logs it, counts it and goes on to the
/// next title; any other error from a source (a database write) ends the run.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Fetch(pub anyhow::Error);

/// Marks a request's error as a [`Fetch`] failure.
pub trait Fetched<T> {
    fn fetched(self) -> Result<T>;
}

impl<T> Fetched<T> for Result<T> {
    fn fetched(self) -> Result<T> {
        self.map_err(|e| Fetch(e).into())
    }
}

/// An option that turns a source on or off: `--only igdb`, `{"igdb": true}`
/// in the jobs API, a checkbox on the Settings page.
#[derive(Clone, Copy)]
pub struct Opt {
    pub id: &'static str,
    pub label: &'static str,
    /// On unless turned off. HowLongToBeat and the AI steps are opt-in.
    pub default_on: bool,
    /// A word on cost for the Settings page ("slow", "uses the AI").
    pub note: Option<&'static str>,
    /// Why it can't run with this config (a missing key), if it can't.
    pub blocked: fn(&Config) -> Option<&'static str>,
}

impl Opt {
    const fn new(id: &'static str, label: &'static str) -> Self {
        Self { id, label, default_on: true, note: None, blocked: |_| None }
    }
    const fn off(mut self) -> Self {
        self.default_on = false;
        self
    }
    const fn note(mut self, note: &'static str) -> Self {
        self.note = Some(note);
        self
    }
    const fn blocked(mut self, f: fn(&Config) -> Option<&'static str>) -> Self {
        self.blocked = f;
        self
    }
}

/// Fetched per title, in its lane. Lanes run at the same time; sources on
/// the same lane share a host's rate limit and take turns on each title.
#[async_trait]
pub trait TitleSource: Send + Sync {
    /// The options it brings (Steam reviews has two: scores and texts).
    fn options(&self) -> &'static [Opt];
    /// The rate-limited host it talks to.
    fn lane(&self) -> &'static str;
    /// What to call it in log lines ("IGDB failed for …").
    fn name(&self) -> &'static str;
    /// Whether any of its options is on.
    fn wanted(&self, run: &Run<'_>) -> bool {
        self.options().iter().any(|o| run.on(o.id))
    }
    /// Whether it can run this time (a key that works, Linux mode on). Asked
    /// once at the start of a run; logs why not.
    async fn available(&self, _run: &Run<'_>) -> bool {
        true
    }
    /// Called with the library as it is when the lanes start.
    fn start(&self, _all: &[GameView], _run: &Run<'_>) {}
    /// Whether this title needs it (nothing stored yet, or a refresh). Decides
    /// which titles the run processes.
    fn needs(&self, g: &GameView, run: &Run<'_>) -> bool;
    /// Takes each title only after the Steam store step, with the appid that
    /// step found (a name match can give a title its first appid).
    fn after_steam_store(&self) -> bool {
        false
    }
    /// Whether to fetch for this title now, in the lane.
    fn wants(&self, t: &Title<'_>, run: &Run<'_>) -> bool {
        self.needs(t.g, run)
    }
    /// Failures in a row after which it stops for the rest of the run. Three
    /// by default: a service that is down would otherwise have every title
    /// wait out its backoff, which holds the whole job for hours.
    fn give_up_after(&self) -> Option<u32> {
        Some(GIVE_UP_AFTER)
    }
    /// An error that stops it for the rest of the run at once (a rejected key).
    fn gives_up_on(&self, _e: &anyhow::Error) -> bool {
        false
    }
    /// Fetch and store for one title. Counts what it stored with `run.count`.
    async fn run(&self, t: &Title<'_>, run: &Run<'_>) -> Result<()>;
}

/// Runs once over the whole library, before the lanes or after them.
#[async_trait]
pub trait Pass: Send + Sync {
    /// The options it brings; empty when it runs on other sources' options.
    fn options(&self) -> &'static [Opt] {
        &[]
    }
    /// What to call it in "… failed (non-fatal)" log lines.
    fn name(&self) -> &'static str;
    /// The lane it shows as while it runs. Passes that talk to the same
    /// service share one ("Wikidata").
    fn lane(&self) -> &'static str {
        self.name()
    }
    fn wanted(&self, run: &Run<'_>) -> bool;
    /// Whether it can run this time. Asked once at the start of a run.
    async fn available(&self, _run: &Run<'_>) -> bool {
        true
    }
    /// Whether an error from `run` ends the whole enrich run. Passes that
    /// handle their own request failures only return database errors, which
    /// should end it. The Wikidata passes return request failures, which
    /// are logged and counted instead.
    fn errors_are_fatal(&self) -> bool {
        true
    }
    async fn run(&self, run: &Run<'_>) -> Result<()>;
}

/// The sources of one run, in order. Built fresh per run: sources keep what
/// they have seen during a run (appids already fetched) in themselves.
pub struct Registry {
    pub before: Vec<Box<dyn Pass>>,
    pub titles: Vec<Box<dyn TitleSource>>,
    pub after: Vec<Box<dyn Pass>>,
}

pub fn registry() -> Registry {
    Registry {
        before: vec![
            Box::new(wikidata::Crossref),
            Box::new(series::Membership),
            Box::new(series::Lists),
            Box::new(series::Covers),
            Box::new(art::Art { after_lanes: false }),
        ],
        titles: vec![
            Box::new(steam_reviews::SteamReviews::default()),
            Box::new(steamspy::SteamSpy::default()),
            Box::new(protondb::ProtonDb),
            Box::new(gog::Gog),
            Box::new(igdb::Igdb),
            Box::new(rawg::Rawg),
            Box::new(pcgw::Pcgw),
            Box::new(hltb::Hltb),
        ],
        after: vec![
            Box::new(art::Art { after_lanes: true }),
            Box::new(trailers::Trailers),
            Box::new(digests::Digests),
            Box::new(series_guides::SeriesGuides),
            Box::new(second_chances::SecondChances),
            Box::new(hidden_gems::HiddenGems),
            Box::new(wishlist::Wishlist),
        ],
    }
}

/// Every option, in run order.
pub fn options() -> &'static [Opt] {
    static OPTIONS: std::sync::LazyLock<Vec<Opt>> = std::sync::LazyLock::new(|| {
        let r = registry();
        let mut out: Vec<Opt> = vec![];
        let all =
            r.before.iter().map(|p| p.options()).chain(r.titles.iter().map(|s| s.options())).chain(r.after.iter().map(|p| p.options()));
        for o in all.flatten() {
            if !out.iter().any(|x| x.id == o.id) {
                out.push(*o);
            }
        }
        out
    });
    &OPTIONS
}

/// Every option id, in run order.
pub fn ids() -> Vec<&'static str> {
    options().iter().map(|o| o.id).collect()
}

pub fn default_on(id: &str) -> bool {
    options().iter().find(|o| o.id == id).is_some_and(|o| o.default_on)
}

/// Shared by the IGDB source and trailers.
fn needs_igdb_login(cfg: &Config) -> Option<&'static str> {
    (cfg.igdb_client_id.is_none() || cfg.igdb_client_secret.is_none()).then_some("needs an IGDB login")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{LaneProgress, LaneState};
    use crate::enrich::{EnrichOptions, Progress, ProgressSink, Tracker};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Keeps the lanes as last reported, and says whether the run was cancelled.
    #[derive(Default)]
    struct Lanes(Mutex<Vec<LaneProgress>>, std::sync::atomic::AtomicBool);

    impl ProgressSink for Lanes {
        fn update(&self, _: &Progress) {}
        fn log(&self, _: String) {}
        fn lanes(&self, lanes: &[LaneProgress]) {
            *self.0.lock().unwrap() = lanes.to_vec();
        }
        fn cancelled(&self) -> bool {
            self.1.load(Ordering::SeqCst)
        }
    }

    /// The first call hits the usage limit: no call starts after it, the
    /// ones already running finish and are counted, and the lane says why.
    #[tokio::test]
    async fn ai_batches_stop_at_a_usage_limit() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = crate::testutil::offline_config(dir.path());
        let db = crate::db::Db::open(&cfg.db_path, cfg.sampled_minutes).unwrap();
        let clients = crate::enrich::Clients::new(&cfg).unwrap();
        let sink = Lanes::default();
        let tracker = Tracker::new(&sink);
        let run = Run::new(&db, &cfg, &clients, None, None, &EnrichOptions::default(), &tracker);
        tracker.begin_pass("AI review summaries");

        let chunks: Vec<usize> = (0..10).collect();
        let started = AtomicUsize::new(0);
        let mut finished = vec![];
        // Ending in a full stop, as Codex's and OpenAI's messages do.
        let limit = || anyhow::Error::new(crate::llm::LlmError::LimitReached("You've hit your limit · resets 4pm.".into()));
        ai_batches(
            &run,
            AiKind::ReviewSummaries,
            &chunks,
            |&i| {
                started.fetch_add(1, Ordering::SeqCst);
                async move {
                    if i == 0 {
                        return Err(limit().context("digest generation"));
                    }
                    tokio::task::yield_now().await;
                    Ok(i)
                }
            },
            |&i, result: Result<usize>| finished.push((i, result.is_ok())),
        )
        .await
        .unwrap();

        assert_eq!(started.load(Ordering::SeqCst), AI_PARALLEL, "none after the limit");
        finished.sort_unstable();
        assert_eq!(finished, [(0, false), (1, true), (2, true)], "each counted as it comes back");
        let lanes = sink.0.lock().unwrap();
        let lane = lanes.iter().find(|l| l.name == "AI review summaries").unwrap();
        assert_eq!(lane.state, LaneState::Stopped);
        assert!(lane.note.as_deref().unwrap().starts_with("You've hit your limit · resets 4pm. What's left waits"), "{lane:?}");
    }

    /// A cancel while calls are running: the one already back is counted,
    /// the two that would answer only after many minutes are dropped, and
    /// the pass ends cancelled at once instead of waiting for them.
    #[tokio::test]
    async fn ai_batches_drop_running_calls_on_a_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = crate::testutil::offline_config(dir.path());
        let db = crate::db::Db::open(&cfg.db_path, cfg.sampled_minutes).unwrap();
        let clients = crate::enrich::Clients::new(&cfg).unwrap();
        let sink = Lanes::default();
        let tracker = Tracker::new(&sink);
        let run = Run::new(&db, &cfg, &clients, None, None, &EnrichOptions::default(), &tracker);
        tracker.begin_pass("AI review summaries");

        let chunks: Vec<usize> = (0..10).collect();
        let started = AtomicUsize::new(0);
        let mut finished = vec![];
        let batches = ai_batches(
            &run,
            AiKind::ReviewSummaries,
            &chunks,
            |&i| {
                started.fetch_add(1, Ordering::SeqCst);
                async move {
                    if i > 0 {
                        tokio::time::sleep(std::time::Duration::from_secs(900)).await;
                    }
                    Ok(i)
                }
            },
            |&i, result: Result<usize>| finished.push((i, result.is_ok())),
        );
        let cancel = async {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            sink.1.store(true, Ordering::SeqCst);
        };
        let at = std::time::Instant::now();
        let ended = tokio::time::timeout(std::time::Duration::from_secs(5), async { tokio::join!(batches, cancel).0 });
        let err = ended.await.expect("the cancel didn't wait for the calls").unwrap_err();
        assert_eq!(err.to_string(), "cancelled");
        assert!(at.elapsed() < std::time::Duration::from_secs(2), "{:?}", at.elapsed());
        assert_eq!(finished, [(0, true)], "the result already back is kept");
        assert_eq!(started.load(Ordering::SeqCst), AI_PARALLEL + 1, "the first's place was taken, and nothing started after the cancel");
    }

    /// The ids are the jobs API's option keys and the Settings page's
    /// checkboxes. Renaming one breaks saved choices in the browser.
    #[test]
    fn option_ids_are_the_api_keys() {
        let mut ids = ids();
        ids.sort_unstable();
        let mut unique = ids.clone();
        unique.dedup();
        assert_eq!(ids, unique, "no id twice");
        assert_eq!(
            ids,
            [
                "art",
                "digests",
                "gog_meta",
                "hidden_gems",
                "hltb",
                "igdb",
                "pcgw",
                "protondb",
                "rawg",
                "review_texts",
                "reviews",
                "second_chances",
                "series",
                "series_guides",
                "tags",
                "trailers",
                "wikidata",
                "wishlist"
            ]
        );
        let off: Vec<&str> = options().iter().filter(|o| !o.default_on).map(|o| o.id).collect();
        assert_eq!(off, ["hltb", "digests", "series_guides", "second_chances", "hidden_gems", "wishlist"]);
    }

    #[test]
    fn options_json_keeps_its_shape() {
        let o: EnrichOptions =
            serde_json::from_str(r#"{"hltb": true, "rawg": false, "refresh": true, "limit": 5, "no_such_source": true}"#).unwrap();
        assert!(o.on("hltb") && !o.on("rawg"), "given");
        assert!(o.on("reviews") && !o.on("digests"), "defaults");
        assert_eq!((o.refresh, o.limit), (true, Some(5)));
        let resolved = serde_json::to_value(o.resolved()).unwrap();
        let obj = resolved.as_object().unwrap();
        assert_eq!(obj.len(), options().len() + 2, "every option, refresh and limit: {resolved}");
        assert_eq!(
            (&obj["hltb"], &obj["reviews"], &obj["limit"]),
            (&serde_json::json!(true), &serde_json::json!(true), &serde_json::json!(5))
        );
        assert!(!obj.contains_key("no_such_source"));
        let empty: EnrichOptions = serde_json::from_str("{}").unwrap();
        assert!(empty.on("wikidata") && !empty.on("hltb") && !empty.refresh);
    }

    #[test]
    fn blocked_reasons_follow_the_config() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = crate::testutil::offline_config(dir.path());
        let blocked = |cfg: &Config| -> Vec<(&str, &str)> { options().iter().filter_map(|o| Some((o.id, (o.blocked)(cfg)?))).collect() };
        assert_eq!(blocked(&cfg), [("igdb", "needs an IGDB login"), ("rawg", "needs a key"), ("trailers", "needs an IGDB login")]);
        cfg.igdb_client_id = Some("id".into());
        cfg.igdb_client_secret = Some("secret".into());
        cfg.rawg_api_key = Some("key".into());
        cfg.linux_mode = "off".into();
        assert_eq!(blocked(&cfg), [("protondb", "Linux mode is off")]);
    }
}
