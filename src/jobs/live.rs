//! A job running in this process: the sink its work reports to. It keeps
//! the progress in memory and writes it to the `jobs` table every second,
//! with the new log lines and failures; that write is the heartbeat, and it
//! brings back a cancel asked for by another process.

use super::context::JOB;
use crate::db::{Db, Failure, Job, JobEnd, JobProgress, JobSource, JobStatus, LaneProgress, LaneState, Level, LogLine, now};
use crate::enrich::{Progress, ProgressSink, failure_line};
use anyhow::Result;
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often progress is written. Tests go faster.
const BEAT: Duration = if cfg!(test) { Duration::from_millis(20) } else { Duration::from_secs(1) };
/// The rate is measured over this much of the recent past.
const RATE_WINDOW: Duration = Duration::from_secs(300);
/// No rate, and no time left, until this much time and work has gone by.
const RATE_MIN_TIME: Duration = if cfg!(test) { Duration::ZERO } else { Duration::from_secs(20) };
const RATE_MIN_DONE: usize = 3;

pub struct LiveJob {
    pub id: i64,
    db: Arc<Db>,
    state: Mutex<State>,
    cancel: AtomicBool,
    ended: AtomicBool,
    /// Also shown here (the terminal's progress bar).
    display: Option<Box<dyn ProgressSink>>,
}

#[derive(Default)]
struct State {
    progress: JobProgress,
    /// Lines and failures not written yet.
    lines: Vec<LogLine>,
    failures: Vec<Failure>,
    /// (when, done) since the total last changed, for the rate.
    samples: VecDeque<(Instant, usize)>,
}

/// A service asked a lane to wait, or didn't answer.
pub struct Wait {
    /// "Steam asked us to slow down"
    pub note: String,
    pub delay: Duration,
    /// For the log: "HTTP 429 for https://…/appreviews/620, try 2 of 5".
    pub detail: String,
    /// The last try failed: the request gives up, and the wait is before the next one.
    pub last: bool,
}

impl Wait {
    /// "Steam asked us to slow down, trying again in 40 s (HTTP 429 for …, try 4 of 5)"
    pub fn text(&self) -> String {
        let secs = self.delay.as_secs_f64().ceil() as i64;
        if self.last {
            format!("{}; the next request waits {secs} s ({})", self.note, self.detail)
        } else {
            format!("{}, trying again in {secs} s ({})", self.note, self.detail)
        }
    }
}

impl LiveJob {
    /// Start a job, unless one is running here or in another process: then
    /// `Err` with that job. Progress is written from now on, so call it from
    /// inside the Tokio runtime.
    pub fn begin(
        db: Arc<Db>,
        kind: &str,
        task: Option<&str>,
        source: JobSource,
        options: Value,
        display: Option<Box<dyn ProgressSink>>,
    ) -> Result<std::result::Result<Arc<Self>, Job>> {
        let id = match db.job_begin(kind, task, source, &options)? {
            Ok(id) => id,
            Err(running) => return Ok(Err(running)),
        };
        let job = Arc::new(Self {
            id,
            db,
            state: Mutex::new(State { progress: JobProgress { message: "Starting".into(), ..Default::default() }, ..Default::default() }),
            cancel: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            display,
        });
        let beating = Arc::downgrade(&job);
        tokio::spawn(async move {
            let mut beat = tokio::time::interval(BEAT);
            beat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                beat.tick().await;
                match beating.upgrade() {
                    Some(job) if !job.ended.load(Ordering::Relaxed) => job.write(),
                    _ => break,
                }
            }
        });
        Ok(Ok(job))
    }

    /// Run `work` as this job: log lines, rate-limit waits and warnings from
    /// inside it (`jobs::waiting`, the tracing layer) are recorded as the job's.
    pub async fn scope<F: std::future::Future>(self: &Arc<Self>, work: F) -> F::Output {
        JOB.scope(self.clone(), work).await
    }

    /// Stop at the next check, saying why in the job's log.
    pub fn stop_because(&self, why: &str) {
        self.record(Level::Info, None, why.to_string(), true);
        self.cancel_now();
    }

    /// Stop at the next check, as if the UI had asked.
    pub fn cancel_now(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.with(|s| s.progress.message = "Cancelling…".into());
    }

    /// End the job with how its work went: done with a summary ("2,000
    /// games, 3 new"), or failed, or cancelled when it stopped after a cancel.
    pub fn finish(&self, result: std::result::Result<String, &anyhow::Error>) -> JobStatus {
        self.ended.store(true, Ordering::Relaxed);
        let (status, summary, error) = match result {
            Ok(summary) => (JobStatus::Done, Some(summary), None),
            Err(e) if self.cancelled() => (JobStatus::Cancelled, None, Some(e.to_string())),
            Err(e) => (JobStatus::Failed, None, Some(format!("{e:#}"))),
        };
        if status == JobStatus::Failed {
            self.record(Level::Error, None, format!("failed: {}", error.as_deref().unwrap_or_default()), false);
        }
        let (mut progress, lines, failures) = self.take();
        if status != JobStatus::Done {
            let why = if status == JobStatus::Cancelled { "the run was cancelled" } else { "the run failed" };
            crate::db::stop_unfinished_lanes(&mut progress.lanes, why);
        }
        let end = JobEnd { status, summary: summary.as_deref(), error: error.as_deref() };
        if let Err(e) = self.db.job_end(self.id, &progress, &lines, &failures, &end) {
            tracing::error!("couldn't record the end of job {}: {e:#}", self.id);
        }
        status
    }

    fn with<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        f(&mut self.state.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// The progress to write, with the rate, and the lines and failures not written yet.
    fn take(&self) -> (JobProgress, Vec<LogLine>, Vec<Failure>) {
        self.with(|s| {
            let at = now();
            for l in &mut s.progress.lanes {
                if l.state == LaneState::Waiting && l.retry_at.is_none_or(|t| t <= at) {
                    l.state = LaneState::Running;
                    l.note = None;
                    l.retry_at = None;
                }
            }
            let (per_minute, eta_secs) = rate(&s.samples, s.progress.done, s.progress.total);
            s.progress.per_minute = per_minute;
            s.progress.eta_secs = eta_secs;
            (s.progress.clone(), std::mem::take(&mut s.lines), std::mem::take(&mut s.failures))
        })
    }

    /// The heartbeat: write what's new, and learn whether to stop.
    fn write(&self) {
        let (progress, lines, failures) = self.take();
        match self.db.job_pulse(self.id, &progress, &lines, &failures) {
            Ok(pulse) => {
                if pulse.cancel && !self.cancel.swap(true, Ordering::Relaxed) {
                    self.record(Level::Info, None, "asked to stop from the UI".into(), true);
                }
                if !pulse.alive && !self.cancel.swap(true, Ordering::Relaxed) {
                    // Another process gave up on this job while this one
                    // couldn't write (a long database lock); it may have
                    // started another, so this one stops.
                    self.record(Level::Error, None, "stopping: the job was marked as stopped while it couldn't be recorded".into(), true);
                }
            }
            Err(e) => {
                // Keep them for the next beat.
                self.with(|s| {
                    s.lines.splice(0..0, lines);
                    s.failures.splice(0..0, failures);
                });
                tracing::warn!("couldn't record job {}'s progress: {e:#}", self.id);
            }
        }
    }

    fn record(&self, level: Level, lane: Option<&str>, message: String, display: bool) {
        self.record_game(level, lane, None, message, display);
    }

    fn record_game(&self, level: Level, lane: Option<&str>, title_id: Option<i64>, message: String, display: bool) {
        if display && let Some(d) = &self.display {
            d.log_at(level, message.clone());
        }
        self.with(|s| s.lines.push(LogLine { at: now(), level, lane: lane.map(str::to_string), message, title_id }));
    }

    /// A warning or error from `tracing` inside the job (the tracing layer).
    /// Not shown on the display: the terminal prints those itself.
    pub(super) fn traced(&self, level: Level, message: String) {
        self.record(level, super::context::current_lane(), message, false);
    }

    /// A lane is waiting on a service (a 429, a 503, no answer).
    pub(super) fn waiting(&self, lane: Option<&str>, wait: &Wait) {
        let secs = wait.delay.as_secs_f64().ceil() as i64;
        self.record(Level::Warn, lane, wait.text(), true);
        let Some(lane) = lane else { return };
        let lanes = self.with(|s| {
            let l = s.progress.lanes.iter_mut().find(|l| l.name == lane)?;
            if l.state == LaneState::Running {
                l.state = LaneState::Waiting;
            }
            l.note = Some(wait.note.clone());
            l.retry_at = Some(now() + secs);
            Some(s.progress.lanes.clone())
        });
        if let (Some(lanes), Some(d)) = (lanes, &self.display) {
            d.lanes(&lanes);
        }
    }
}

/// Titles per minute and seconds left, from the samples since the total
/// last changed; None until there's enough to go on.
fn rate(samples: &VecDeque<(Instant, usize)>, done: usize, total: usize) -> (Option<f64>, Option<i64>) {
    let Some(&(since, first)) = samples.front() else { return (None, None) };
    let elapsed = since.elapsed();
    let moved = done.saturating_sub(first);
    if elapsed < RATE_MIN_TIME || moved < RATE_MIN_DONE || elapsed.is_zero() {
        return (None, None);
    }
    let per_sec = moved as f64 / elapsed.as_secs_f64();
    let left = total.saturating_sub(done) as f64 / per_sec;
    (Some(per_sec * 60.0), Some(left.round() as i64))
}

impl ProgressSink for LiveJob {
    fn update(&self, p: &Progress) {
        self.with(|s| {
            if p.total != s.progress.total || p.done < s.progress.done {
                s.samples.clear();
            }
            let at = Instant::now();
            // One sample a second is plenty; keep the last few minutes.
            if s.samples.back().is_none_or(|(t, _)| t.elapsed() >= Duration::from_secs(1)) || s.samples.len() < 2 {
                s.samples.push_back((at, p.done));
            }
            while s.samples.len() > 2 && s.samples.front().is_some_and(|(t, _)| t.elapsed() > RATE_WINDOW) {
                s.samples.pop_front();
            }
            s.progress.total = p.total;
            s.progress.done = p.done;
            s.progress.current = p.current.clone();
            if !self.cancel.load(Ordering::Relaxed) {
                s.progress.message = p.message.clone();
            }
        });
        if let Some(d) = &self.display {
            d.update(p);
        }
    }

    fn log(&self, line: String) {
        self.record(Level::Info, super::context::current_lane(), line, true);
    }

    fn log_at(&self, level: Level, line: String) {
        self.record(level, super::context::current_lane(), line, true);
    }

    fn log_game(&self, level: Level, title_id: i64, line: String) {
        self.record_game(level, super::context::current_lane(), Some(title_id), line, true);
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn lanes(&self, lanes: &[LaneProgress]) {
        let merged = self.with(|s| {
            let at = now();
            let mut merged = lanes.to_vec();
            for l in &mut merged {
                // A wait lasts until the lane moves on or its time is up.
                let Some(old) = s.progress.lanes.iter().find(|o| o.name == l.name) else { continue };
                let still_waiting = old.state == LaneState::Waiting
                    && old.retry_at.is_some_and(|t| t > at)
                    && (old.done, old.failed) == (l.done, l.failed)
                    && l.state == LaneState::Running;
                if still_waiting {
                    l.state = LaneState::Waiting;
                    l.note = old.note.clone();
                    l.retry_at = old.retry_at;
                }
            }
            s.progress.lanes = merged.clone();
            merged
        });
        if let Some(d) = &self.display {
            d.lanes(&merged);
        }
    }

    fn failure(&self, f: &Failure) {
        let line = failure_line(f);
        if let Some(d) = &self.display {
            d.log_at(Level::Error, line.clone());
        }
        self.with(|s| {
            s.lines.push(LogLine { at: now(), level: Level::Error, lane: Some(f.lane.clone()), message: line, title_id: f.title_id });
            s.failures.push(f.clone());
            s.progress.failed += 1;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn wait(secs: u64) -> Wait {
        Wait {
            note: "Steam asked us to slow down".into(),
            delay: Duration::from_secs(secs),
            detail: "HTTP 429 Too Many Requests for https://store.example/appreviews/620, try 1 of 5".into(),
            last: false,
        }
    }

    #[tokio::test]
    async fn a_wait_shows_on_its_lane_until_the_lane_moves_on() {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Db::open(&dir.path().join("upnext.db"), 120).unwrap());
        let job = LiveJob::begin(db, "enrich", None, JobSource::Ui, json!({}), None).unwrap().unwrap();
        let lane = |done| LaneProgress { name: "Steam reviews".into(), done, total: 10, ..Default::default() };
        job.lanes(&[lane(3)]);
        let reported = job.scope(super::super::in_lane("Steam reviews", async { super::super::waiting(&wait(40)) })).await;
        assert!(reported);

        let (p, lines, _) = job.take();
        let l = &p.lanes[0];
        assert_eq!((l.state, l.note.as_deref()), (LaneState::Waiting, Some("Steam asked us to slow down")));
        assert!(l.retry_at.is_some_and(|t| (now() + 39..=now() + 41).contains(&t)), "{l:?}");
        assert_eq!((lines[0].level, lines[0].lane.as_deref()), (Level::Warn, Some("Steam reviews")));
        assert_eq!(
            lines[0].message,
            "Steam asked us to slow down, trying again in 40 s (HTTP 429 Too Many Requests for https://store.example/appreviews/620, try 1 of 5)"
        );

        job.lanes(&[lane(3)]);
        assert_eq!(job.take().0.lanes[0].state, LaneState::Waiting, "a report that didn't move keeps the wait");
        job.lanes(&[lane(4)]);
        let l = &job.take().0.lanes[0];
        assert_eq!((l.state, l.note.as_deref(), l.retry_at), (LaneState::Running, None, None), "the next title clears it");

        assert!(!super::super::waiting(&wait(5)), "outside a job the caller logs it itself");
        job.finish(Ok("done".into()));
    }

    /// A cancelled run's lanes don't stay "running": the finished
    /// ones stay done, the rest stop with the reason.
    #[tokio::test]
    async fn a_cancelled_run_stops_its_unfinished_lanes() {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Db::open(&dir.path().join("upnext.db"), 120).unwrap());
        let job = LiveJob::begin(db.clone(), "enrich", None, JobSource::Ui, json!({}), None).unwrap().unwrap();
        let lane = |name: &str, state| LaneProgress { name: name.into(), done: 1, total: 10, state, ..Default::default() };
        job.lanes(&[lane("Wikidata", LaneState::Done), lane("Steam store", LaneState::Running), lane("SteamSpy", LaneState::Waiting)]);
        job.cancel_now();
        assert_eq!(job.finish(Err(&anyhow::anyhow!("cancelled"))), JobStatus::Cancelled);
        let lanes = db.job(job.id).unwrap().unwrap().lanes;
        let states: Vec<_> = lanes.iter().map(|l| (l.name.as_str(), l.state, l.note.as_deref())).collect();
        assert_eq!(
            states,
            [
                ("Wikidata", LaneState::Done, None),
                ("Steam store", LaneState::Stopped, Some("the run was cancelled")),
                ("SteamSpy", LaneState::Stopped, Some("the run was cancelled")),
            ]
        );
    }

    #[test]
    fn the_rate_needs_some_history() {
        let now = Instant::now();
        let minute_ago = now - Duration::from_secs(60);
        let samples: VecDeque<(Instant, usize)> = [(minute_ago, 100)].into();
        let (per_minute, eta) = rate(&samples, 160, 1000);
        assert!(per_minute.is_some_and(|r| (59.0..61.0).contains(&r)), "{per_minute:?}");
        assert!(eta.is_some_and(|s| (820..860).contains(&s)), "840 left at one a second: {eta:?}");
        assert_eq!(rate(&samples, 101, 1000), (None, None), "one title isn't enough to go on");
        assert_eq!(rate(&VecDeque::new(), 5, 10), (None, None));
    }
}
