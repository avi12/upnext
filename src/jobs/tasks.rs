//! The jobs UpNext runs by itself, one task per row of Settings › Jobs: a
//! sync, an enrich with set sources, a backup, or a profile update, on a schedule (daily,
//! weekly, monthly, or after another task ends done) while the server runs,
//! or by hand. Every job records the task it ran as, whoever started it, so
//! a terminal run that does a task's work counts as that task's run.
//!
//! A schedule is a row of the settings table (`schedule.<task>`), written
//! when someone changes it. A task without one has its default schedule,
//! counted from when the database was made (`meta.schedules_since`). A
//! task added later needs a row from its own upgrade step, or its first run
//! would be due at once on databases made before it.
//!
//! When a task is due is a pure function of the schedules, the last runs
//! and the time (`due`), so the tests need no clock.

use super::{BackupOptions, JobKind};
use crate::config::Config;
use crate::db::PlayerId;
use crate::db::{Db, Refused, TaskRuns, now, schedule_key};
use crate::enrich::{self, EnrichOptions, SyncOptions};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const DAY: i64 = 86_400;

/// Backups the backup task keeps unless told otherwise.
pub const DEFAULT_KEEP: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Task {
    Sync,
    Details,
    Reviews,
    Digests,
    SeriesGuides,
    SecondChances,
    HiddenGems,
    Wishlist,
    Taste,
    Hltb,
    Backup,
    /// Download other players' backlog verdicts.
    Verdicts,
}

/// Sources that are tasks of their own, so Fetch details leaves them out.
const SEPARATE: [&str; 6] = ["hltb", "digests", "series_guides", "second_chances", "hidden_gems", "wishlist"];

impl Task {
    /// In the order the Jobs table lists them, which is also the order due
    /// tasks start in: a sync before the details that follow it.
    pub const ALL: [Task; 12] = [
        Self::Sync,
        Self::Details,
        Self::Reviews,
        Self::Digests,
        Self::SeriesGuides,
        Self::SecondChances,
        Self::HiddenGems,
        Self::Wishlist,
        Self::Taste,
        Self::Hltb,
        Self::Backup,
        Self::Verdicts,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Sync => "sync",
            Self::Details => "details",
            Self::Reviews => "reviews",
            Self::Digests => "digests",
            Self::SeriesGuides => "series_guides",
            Self::SecondChances => "second_chances",
            Self::HiddenGems => "hidden_gems",
            Self::Wishlist => "wishlist",
            Self::Taste => "taste",
            Self::Hltb => "hltb",
            Self::Backup => "backup",
            Self::Verdicts => "verdicts",
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Sync => "Sync libraries",
            Self::Details => "Fetch details",
            Self::Reviews => "Refresh player reviews",
            Self::Digests => "People say, Vibes and tags",
            Self::SeriesGuides => "Series guides",
            Self::SecondChances => "Second chances",
            Self::HiddenGems => "Find hidden gems",
            Self::Wishlist => "Update the wishlist",
            Self::Taste => "Update your profile",
            Self::Hltb => "HowLongToBeat",
            Self::Backup => "Back up the database",
            Self::Verdicts => "Backlog verdicts",
        }
    }

    /// Its label in the middle of a sentence: "after sync libraries", but
    /// People say and HowLongToBeat keep their capitals.
    pub fn label_mid(self) -> String {
        match self {
            Self::Digests | Self::Hltb => self.label().to_string(),
            _ => self.label().to_lowercase(),
        }
    }

    /// What "after the next …" calls it.
    pub fn noun(self) -> &'static str {
        match self {
            Self::Sync => "sync",
            Self::Details => "details run",
            Self::Reviews => "reviews refresh",
            Self::Digests => "People say run",
            Self::SeriesGuides => "series guides run",
            Self::SecondChances => "second chances run",
            Self::HiddenGems => "hidden gems run",
            Self::Wishlist => "wishlist update",
            Self::Taste => "profile update",
            Self::Hltb => "HowLongToBeat run",
            Self::Backup => "backup",
            Self::Verdicts => "backlog verdicts download",
        }
    }

    pub fn uses_ai(self) -> bool {
        matches!(self, Self::Digests | Self::SeriesGuides | Self::SecondChances | Self::HiddenGems)
    }

    /// Until someone chooses (the setup wizard, or Settings): the jobs
    /// without the AI run, except HowLongToBeat, which is slow; every AI job
    /// is off until the wizard turns it on (decided 2026-10-02). Updating
    /// your profile only counts, so it follows every sync.
    pub fn default_schedule(self) -> Schedule {
        match self {
            Self::Sync => Schedule::Daily,
            Self::Details | Self::Taste => Schedule::After(Self::Sync),
            // No AI; it reads the series lists Fetch details keeps fresh.
            Self::Wishlist => Schedule::After(Self::Details),
            Self::Reviews => Schedule::Monthly,
            Self::Backup => Schedule::Weekly,
            // A small download that sends nothing; it does nothing while
            // receiving is off (`server::scheduler::schedule_tick`).
            Self::Verdicts => Schedule::Daily,
            Self::Digests | Self::SeriesGuides | Self::SecondChances | Self::HiddenGems | Self::Hltb => Schedule::Off,
        }
    }

    /// What its options turn on and off: libraries for a sync, sources for
    /// Fetch details. Empty for tasks without options.
    fn option_ids(self) -> Vec<&'static str> {
        match self {
            Self::Sync => enrich::sync_libraries().iter().map(|l| l.id()).collect(),
            Self::Details => enrich::sources::ids().into_iter().filter(|id| !SEPARATE.contains(id)).collect(),
            _ => vec![],
        }
    }

    fn option_default(self, id: &str) -> bool {
        match self {
            Self::Details => enrich::sources::default_on(id),
            _ => true,
        }
    }

    /// Whether its enrich fetches again what titles already have.
    fn refresh(self) -> bool {
        self == Self::Reviews
    }
}

/// When a task runs by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schedule {
    Off,
    Daily,
    Weekly,
    /// Every 30 days.
    Monthly,
    /// When that task ends done, from the server, the UI or a terminal.
    After(Task),
}

impl Schedule {
    /// "daily", "after:sync"
    pub fn text(self) -> String {
        match self {
            Self::Off => "off".into(),
            Self::Daily => "daily".into(),
            Self::Weekly => "weekly".into(),
            Self::Monthly => "monthly".into(),
            Self::After(t) => format!("after:{}", t.id()),
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "off" => Some(Self::Off),
            "daily" => Some(Self::Daily),
            "weekly" => Some(Self::Weekly),
            "monthly" => Some(Self::Monthly),
            _ => s.strip_prefix("after:").and_then(Task::parse).map(Self::After),
        }
    }

    pub fn period(self) -> Option<i64> {
        match self {
            Self::Daily => Some(DAY),
            Self::Weekly => Some(7 * DAY),
            Self::Monthly => Some(30 * DAY),
            Self::Off | Self::After(_) => None,
        }
    }
}

impl Serialize for Schedule {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.text())
    }
}

/// A task's schedule and options.
#[derive(Debug, Clone, PartialEq)]
pub struct Setup {
    pub task: Task,
    pub schedule: Schedule,
    /// When the schedule was set. Its clock starts then: a task that hasn't
    /// run since is due one period after.
    pub since: i64,
    /// On or off by library (sync) or source (details); an id left out takes its default.
    pub options: BTreeMap<String, bool>,
    /// Backups kept (backup only).
    pub keep: usize,
    /// At most this many titles (or series, games) a run, for the AI jobs: a
    /// big first run in parts, as the setup wizard sets it. None for no limit.
    pub limit: Option<usize>,
    /// Someone set it, rather than it being the default.
    pub chosen: bool,
}

/// A schedule as the settings table holds it.
#[derive(Debug, Serialize, Deserialize)]
struct Stored {
    every: String,
    since: i64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    options: BTreeMap<String, bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    keep: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    limit: Option<usize>,
}

impl Setup {
    fn default_for(task: Task, since: i64) -> Self {
        Self { task, schedule: task.default_schedule(), since, options: BTreeMap::new(), keep: DEFAULT_KEEP, limit: None, chosen: false }
    }

    fn from_stored(task: Task, value: &str, since: i64) -> Self {
        let stored: Option<Stored> = serde_json::from_str(value).ok();
        let Some(stored) = stored else {
            tracing::warn!("the schedule of {} can't be read ({value}); using its default", task.label());
            return Self::default_for(task, since);
        };
        let schedule = Schedule::parse(&stored.every).unwrap_or_else(|| task.default_schedule());
        Self {
            task,
            schedule,
            since: stored.since,
            options: stored.options,
            keep: stored.keep.unwrap_or(DEFAULT_KEEP),
            limit: stored.limit.filter(|_| task.uses_ai()),
            chosen: true,
        }
    }

    fn stored(&self) -> Stored {
        Stored {
            every: self.schedule.text(),
            since: self.since,
            options: self.options.clone(),
            keep: (self.task == Task::Backup).then_some(self.keep),
            limit: self.limit,
        }
    }

    /// Every option spelled out, for the API. None for tasks without options.
    pub fn resolved_options(&self) -> Option<BTreeMap<String, bool>> {
        let ids = self.task.option_ids();
        (!ids.is_empty()).then(|| ids.into_iter().map(|id| (id.to_string(), self.on(id))).collect())
    }

    fn on(&self, id: &str) -> bool {
        self.options.get(id).copied().unwrap_or_else(|| self.task.option_default(id))
    }

    /// The enrich sources it runs; empty for sync and backup.
    fn sources(&self) -> Vec<&'static str> {
        match self.task {
            Task::Details => self.task.option_ids().into_iter().filter(|id| self.on(id)).collect(),
            Task::Reviews => vec!["reviews", "review_texts", "gog_meta"],
            Task::Digests => vec!["digests"],
            Task::SeriesGuides => vec!["series_guides"],
            Task::SecondChances => vec!["second_chances"],
            Task::HiddenGems => vec!["hidden_gems"],
            Task::Wishlist => vec!["wishlist"],
            Task::Hltb => vec!["hltb"],
            Task::Sync | Task::Backup | Task::Taste | Task::Verdicts => vec![],
        }
    }

    /// The job that runs it. `refresh` fetches again what titles have
    /// (enrich tasks; Refresh player reviews always does).
    pub fn job(&self, cfg: &Config, refresh: bool) -> JobKind {
        match self.task {
            Task::Sync => JobKind::Sync(SyncOptions {
                libraries: self.options.clone(),
                steam_local: false,
                galaxy_db: cfg.galaxy_db.as_ref().filter(|p| p.is_file()).map(|p| p.display().to_string()),
                connection: None,
            }),
            Task::Backup => JobKind::Backup(BackupOptions { keep: self.keep }),
            Task::Taste => JobKind::Taste,
            Task::Verdicts => JobKind::Verdicts,
            _ => {
                let on = self.sources();
                JobKind::Enrich(EnrichOptions {
                    sources: enrich::sources::ids().into_iter().map(|id| (id.to_string(), on.contains(&id))).collect(),
                    refresh: refresh || self.task.refresh(),
                    limit: self.limit,
                    titles: vec![],
                })
            }
        }
    }
}

/// Every task's schedule, in table order.
pub fn setups(db: &Db) -> Result<Vec<Setup>> {
    let stored = db.settings_all(PlayerId::FIRST)?;
    let since = db.meta_get("schedules_since")?.and_then(|v| v.parse().ok()).unwrap_or_else(now);
    Ok(Task::ALL
        .into_iter()
        .map(|t| match stored.get(&schedule_key(t.id())) {
            Some(v) => Setup::from_stored(t, v, since),
            None => Setup::default_for(t, since),
        })
        .collect())
}

/// Whether HowLongToBeat is turned on for good: its job has a schedule.
/// HowLongToBeat is scraped, so nothing reaches it until you choose this or
/// ask for it in a run (`--with hltb`, Run now); the wishlist's times follow
/// the same choice.
pub fn hltb_scheduled(db: &Db) -> Result<bool> {
    Ok(setups(db)?.iter().any(|s| s.task == Task::Hltb && s.schedule != Schedule::Off))
}

/// A change to a task's schedule or options, from Settings or the CLI.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
pub struct Change {
    /// "off", "daily", "weekly", "monthly" or "after:<task>".
    pub schedule: Option<String>,
    /// On or off by library (sync) or source (details).
    pub options: Option<BTreeMap<String, bool>>,
    /// Backups kept (backup only).
    pub keep: Option<usize>,
    /// At most this many titles a run, for a job that uses the AI; 0 for no limit.
    pub limit: Option<usize>,
}

/// Check and save a change. A new schedule starts its clock now; new
/// options leave it where it was.
pub fn change(db: &Db, task: Task, change: &Change) -> Result<Setup> {
    let all = setups(db)?;
    let mut setup = all.iter().find(|s| s.task == task).cloned().unwrap_or_else(|| Setup::default_for(task, now()));
    if let Some(text) = &change.schedule {
        let Some(schedule) = Schedule::parse(text.trim()) else {
            return Err(Refused(format!("a schedule is off, daily, weekly, monthly, or after:<job> (after:sync); not {text:?}")).into());
        };
        if let Schedule::After(first) = schedule {
            if first == task {
                return Err(Refused("a job can't run after itself".into()).into());
            }
            // Follow what the other job runs after; coming back here would
            // make them start each other over and over.
            let mut at = first;
            for _ in 0..Task::ALL.len() {
                match all.iter().find(|s| s.task == at).map(|s| s.schedule) {
                    Some(Schedule::After(next)) if next == task => {
                        return Err(Refused(format!(
                            "{} already follows {}, so they would start each other over and over",
                            first.label(),
                            task.label()
                        ))
                        .into());
                    }
                    Some(Schedule::After(next)) => at = next,
                    _ => break,
                }
            }
        }
        if schedule != setup.schedule {
            setup.schedule = schedule;
            setup.since = now();
        }
    }
    if let Some(options) = &change.options {
        let ids = task.option_ids();
        if ids.is_empty() {
            return Err(Refused(format!("{} has no options", task.label())).into());
        }
        if let Some(unknown) = options.keys().find(|k| !ids.contains(&k.as_str())) {
            return Err(Refused(format!("{} has no option {unknown:?}; it has {}", task.label(), ids.join(", "))).into());
        }
        setup.options.extend(options.clone());
    }
    if let Some(keep) = change.keep {
        if task != Task::Backup {
            return Err(Refused("only the backup job keeps a number of files".into()).into());
        }
        if !(1..=100).contains(&keep) {
            return Err(Refused("keep between 1 and 100 backups".into()).into());
        }
        setup.keep = keep;
    }
    if let Some(limit) = change.limit {
        if !task.uses_ai() {
            return Err(Refused("only the jobs that use the AI take a limit".into()).into());
        }
        // No upper bound: a run takes only the titles that need it, so a
        // limit above the library's size is the same as none.
        setup.limit = (limit > 0).then_some(limit);
    }
    setup.chosen = true;
    db.setting_set(PlayerId::FIRST, &schedule_key(task.id()), Some(&serde_json::to_string(&setup.stored())?))?;
    Ok(setup)
}

/// The task a job does the work of, for a job not started as one:
/// - every sync is Sync libraries, whichever libraries it read;
/// - an enrich of every title (no titles picked, no limit) counts as the
///   first task, in table order, whose sources it all ran, leaving out the
///   ones these settings block, and that refreshed when the task does. So
///   `enrich --with hltb` is Fetch details, and `enrich --only digests`
///   People say;
/// - a backup is Back up the database.
pub fn task_for(kind: &JobKind, setups: &[Setup], cfg: &Config) -> Option<Task> {
    let o = match kind {
        JobKind::Sync(_) => return Some(Task::Sync),
        JobKind::Backup(_) => return Some(Task::Backup),
        JobKind::Taste => return Some(Task::Taste),
        JobKind::Verdicts => return Some(Task::Verdicts),
        JobKind::Restore(_) | JobKind::Import(_) => return None,
        JobKind::Enrich(o) => o,
    };
    if !o.titles.is_empty() || o.limit.is_some() {
        return None;
    }
    let blocked = |id: &str| enrich::sources::options().iter().any(|opt| opt.id == id && (opt.blocked)(cfg).is_some());
    setups.iter().find_map(|s| {
        let wanted: Vec<&str> = s.sources().into_iter().filter(|id| !blocked(id)).collect();
        let covers = !wanted.is_empty() && wanted.iter().all(|id| o.on(id));
        (covers && (o.refresh || !s.task.refresh())).then_some(s.task)
    })
}

/// What deciding when a task runs needs: its schedule and its last runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slot {
    pub task: Task,
    pub schedule: Schedule,
    pub since: i64,
    /// When its last run started, however it ended.
    pub last_started: Option<i64>,
    /// When its last run that ended done ended.
    pub last_done_at: Option<i64>,
}

impl Slot {
    pub fn new(setup: &Setup, runs: Option<&TaskRuns>) -> Self {
        Self {
            task: setup.task,
            schedule: setup.schedule,
            since: setup.since,
            last_started: runs.and_then(|r| r.last.as_ref()).map(|j| j.started_at),
            last_done_at: runs.and_then(|r| r.last_done.as_ref()).and_then(|j| j.finished_at),
        }
    }

    /// Where its clock starts: its last run, or when its schedule was set if
    /// that was later.
    fn start(&self) -> i64 {
        self.last_started.map_or(self.since, |t| t.max(self.since))
    }
}

/// When a task runs next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    Off,
    /// At this time, or as soon as nothing else runs if that's past.
    At(i64),
    /// When this task next ends done.
    After(Task),
}

pub fn next(slot: &Slot, slots: &[Slot]) -> Next {
    match slot.schedule {
        Schedule::Off => Next::Off,
        Schedule::After(first) => {
            let done = slots.iter().find(|s| s.task == first).and_then(|s| s.last_done_at);
            match done {
                Some(at) if at > slot.start() => Next::At(at),
                _ => Next::After(first),
            }
        }
        every => Next::At(slot.start() + every.period().unwrap_or(DAY)),
    }
}

/// The first task due at `now`, in table order.
pub fn due(slots: &[Slot], now: i64) -> Option<Task> {
    slots.iter().find(|s| matches!(next(s, slots), Next::At(t) if t <= now)).map(|s| s.task)
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_790_000_000;

    fn slot(task: Task, schedule: Schedule) -> Slot {
        Slot { task, schedule, since: T0, last_started: None, last_done_at: None }
    }

    fn ran(mut s: Slot, started: i64, done_at: Option<i64>) -> Slot {
        s.last_started = Some(started);
        s.last_done_at = done_at;
        s
    }

    #[test]
    fn a_task_is_due_one_period_after_its_last_run() {
        let backup = ran(slot(Task::Backup, Schedule::Weekly), T0 + DAY, Some(T0 + DAY + 5));
        assert_eq!(due(&[backup], T0 + 8 * DAY - 1), None);
        assert_eq!(due(&[backup], T0 + 8 * DAY), Some(Task::Backup));
        assert_eq!(next(&backup, &[backup]), Next::At(T0 + 8 * DAY));
        let monthly = ran(slot(Task::Reviews, Schedule::Monthly), T0, None);
        assert_eq!(due(&[monthly], T0 + 29 * DAY), None, "a failed run counts as a run too");
        assert_eq!(due(&[monthly], T0 + 30 * DAY), Some(Task::Reviews));
    }

    /// A task that has never run is due one period after its schedule was
    /// set, not at once: the upgrade mustn't start an 85-minute reviews
    /// refresh two minutes after the server restarts.
    #[test]
    fn a_task_that_never_ran_waits_a_period_from_when_it_was_set() {
        let reviews = slot(Task::Reviews, Schedule::Monthly);
        assert_eq!(due(&[reviews], T0 + 2 * 60), None);
        assert_eq!(due(&[reviews], T0 + 30 * DAY), Some(Task::Reviews));
        // A schedule changed after the last run starts its clock again.
        let changed = Slot { since: T0 + 10 * DAY, ..ran(slot(Task::Backup, Schedule::Weekly), T0, Some(T0)) };
        assert_eq!(due(&[changed], T0 + 8 * DAY), None);
        assert_eq!(next(&changed, &[changed]), Next::At(T0 + 17 * DAY));
    }

    #[test]
    fn a_task_that_is_off_never_starts() {
        let off = ran(slot(Task::Hltb, Schedule::Off), T0 - 400 * DAY, Some(T0 - 400 * DAY));
        assert_eq!(due(&[off], T0 + 1000 * DAY), None);
        assert_eq!(next(&off, &[off]), Next::Off);
    }

    #[test]
    fn an_after_task_starts_when_the_one_before_it_ends_done() {
        let sync = ran(slot(Task::Sync, Schedule::Daily), T0 + 100, Some(T0 + 142));
        let details = slot(Task::Details, Schedule::After(Task::Sync));
        let slots = [sync, details];
        assert_eq!(due(&slots, T0 + 150), Some(Task::Details));
        assert_eq!(next(&details, &slots), Next::At(T0 + 142));

        // Once it has run, it waits for the next sync that ends done.
        let details = ran(details, T0 + 160, Some(T0 + 220));
        let slots = [sync, details];
        assert_eq!(due(&slots, T0 + 300), None);
        assert_eq!(next(&details, &slots), Next::After(Task::Sync));

        // A sync that failed or was cancelled starts nothing.
        let failed = ran(sync, T0 + DAY, sync.last_done_at);
        assert_eq!(due(&[failed, details], T0 + DAY + 60), None);

        // A sync that ended done before the schedule was set doesn't count.
        let set_later = Slot { since: T0 + 200, ..slot(Task::Details, Schedule::After(Task::Sync)) };
        assert_eq!(due(&[sync, set_later], T0 + 300), None);
    }

    #[test]
    fn due_tasks_start_in_table_order() {
        let sync = ran(slot(Task::Sync, Schedule::Daily), T0, Some(T0 + 40));
        let backup = ran(slot(Task::Backup, Schedule::Daily), T0, Some(T0 + 1));
        assert_eq!(due(&[sync, backup], T0 + 2 * DAY), Some(Task::Sync));
    }

    fn cfg() -> Config {
        let mut cfg = Config::built_in();
        cfg.linux_mode = "on".into();
        cfg.rawg_api_key = Some("0".repeat(32));
        cfg.igdb_client_id = Some("id".into());
        cfg.igdb_client_secret = Some("secret".into());
        cfg
    }

    fn defaults() -> Vec<Setup> {
        Task::ALL.into_iter().map(|t| Setup::default_for(t, T0)).collect()
    }

    fn enrich(set: &[&str], refresh: bool) -> JobKind {
        let sources = enrich::sources::ids().into_iter().map(|id| (id.to_string(), set.contains(&id))).collect();
        JobKind::Enrich(EnrichOptions { sources, refresh, limit: None, titles: vec![] })
    }

    /// A run from a terminal, or with the options Settings used to send,
    /// counts as the task whose work it did.
    #[test]
    fn a_job_counts_as_the_task_whose_work_it_did() {
        let (cfg, setups) = (cfg(), defaults());
        let task = |kind: &JobKind| task_for(kind, &setups, &cfg);
        let usual = || -> Vec<&str> { enrich::sources::ids().into_iter().filter(|id| enrich::sources::default_on(id)).collect() };
        assert_eq!(task(&JobKind::Sync(SyncOptions::default())), Some(Task::Sync));
        assert_eq!(task(&JobKind::Backup(BackupOptions { keep: 2 })), Some(Task::Backup));
        assert_eq!(task(&JobKind::Taste), Some(Task::Taste));
        assert_eq!(task(&enrich(&usual(), false)), Some(Task::Details));
        assert_eq!(task(&enrich(&[usual(), vec!["hltb", "digests"]].concat(), true)), Some(Task::Details), "more than the task");
        assert_eq!(task(&enrich(&["reviews", "review_texts", "gog_meta"], true)), Some(Task::Reviews));
        assert_eq!(task(&enrich(&["reviews", "review_texts", "gog_meta"], false)), None, "without refresh it only fills gaps");
        assert_eq!(task(&enrich(&["digests"], false)), Some(Task::Digests));
        assert_eq!(task(&enrich(&["series_guides"], true)), Some(Task::SeriesGuides));
        assert_eq!(task(&enrich(&["second_chances"], false)), Some(Task::SecondChances));
        assert_eq!(task(&enrich(&["hidden_gems"], false)), Some(Task::HiddenGems));
        assert_eq!(task(&enrich(&["igdb"], false)), None, "one source isn't a task");

        // Narrowed runs (a retry, a sample) aren't the task's run.
        let JobKind::Enrich(mut o) = enrich(&usual(), false) else { unreachable!() };
        o.titles = vec![7];
        assert_eq!(task(&JobKind::Enrich(o.clone())), None);
        o.titles.clear();
        o.limit = Some(60);
        assert_eq!(task(&JobKind::Enrich(o)), None);

        // What these settings block doesn't have to run: without a RAWG key
        // Settings sent `rawg: false`.
        let mut no_rawg = cfg.clone();
        no_rawg.rawg_api_key = None;
        let without: Vec<&str> = usual().into_iter().filter(|id| *id != "rawg").collect();
        assert_eq!(task_for(&enrich(&without, false), &setups, &no_rawg), Some(Task::Details));
        assert_eq!(task(&enrich(&without, false)), None, "with a key, RAWG is part of it");

        // Fetch details with a source turned off in its options.
        let mut setups = setups.clone();
        setups[1].options.insert("trailers".into(), false);
        let fewer: Vec<&str> = usual().into_iter().filter(|id| *id != "trailers").collect();
        assert_eq!(task_for(&enrich(&fewer, false), &setups, &cfg), Some(Task::Details));
    }

    #[test]
    fn a_task_runs_its_own_sources() {
        let (cfg, mut setups) = (cfg(), defaults());
        let JobKind::Enrich(reviews) = setups[2].job(&cfg, false) else { panic!("an enrich") };
        assert!(reviews.refresh && reviews.on("reviews") && reviews.on("gog_meta") && !reviews.on("tags"));
        setups[1].options.insert("igdb".into(), false);
        let JobKind::Enrich(details) = setups[1].job(&cfg, true) else { panic!("an enrich") };
        assert!(details.refresh && details.on("tags") && !details.on("igdb") && !details.on("hltb") && !details.on("digests"));
        assert!(matches!(setups[10].job(&cfg, false), JobKind::Backup(BackupOptions { keep: DEFAULT_KEEP })));
        assert!(matches!(setups[8].job(&cfg, false), JobKind::Taste));
        let JobKind::Enrich(wishlist) = setups[7].job(&cfg, false) else { panic!("an enrich") };
        assert!(wishlist.on("wishlist") && !wishlist.on("tags") && !details.on("wishlist"));
        for s in &setups {
            assert_eq!(task_for(&s.job(&cfg, false), &setups, &cfg), Some(s.task), "{:?}", s.task);
        }
    }

    #[test]
    fn schedules_are_checked_and_saved() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("upnext.db"), 120).unwrap();
        let fresh = setups(&db).unwrap();
        assert_eq!(
            fresh.iter().map(|s| s.schedule.text()).collect::<Vec<_>>(),
            ["daily", "after:sync", "monthly", "off", "off", "off", "off", "after:details", "after:sync", "off", "weekly", "daily"]
        );
        assert!(fresh.iter().all(|s| !s.chosen && (now() - s.since).abs() < 5), "a new database gets the defaults, from now");

        let set = |task, text: &str| change(&db, task, &Change { schedule: Some(text.into()), ..Default::default() });
        let digests = set(Task::Digests, "after:details").unwrap();
        assert_eq!((digests.schedule, digests.chosen), (Schedule::After(Task::Details), true));
        assert!(set(Task::Digests, "after:digests").unwrap_err().to_string().contains("after itself"));
        let circle = set(Task::Sync, "after:digests").unwrap_err().to_string();
        assert_eq!(circle, "People say, Vibes and tags already follows Sync libraries, so they would start each other over and over");
        assert!(set(Task::Sync, "hourly").is_err());

        let backup = change(&db, Task::Backup, &Change { keep: Some(8), ..Default::default() }).unwrap();
        assert_eq!((backup.keep, backup.schedule), (8, Schedule::Weekly), "the schedule stays");
        assert!(change(&db, Task::Sync, &Change { keep: Some(8), ..Default::default() }).is_err());
        let opts = |pairs: &[(&str, bool)]| Change {
            options: Some(pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()),
            ..Default::default()
        };
        assert!(change(&db, Task::Details, &opts(&[("hltb", true)])).is_err(), "HowLongToBeat is its own job");
        assert!(change(&db, Task::Reviews, &opts(&[("reviews", true)])).is_err());
        let details = change(&db, Task::Details, &opts(&[("igdb", false)])).unwrap();
        assert!(!details.resolved_options().unwrap()["igdb"]);
        assert_eq!(details.since, fresh[1].since, "new options don't restart the clock");

        let saved = setups(&db).unwrap();
        assert_eq!(saved[3], digests);
        assert_eq!(saved[10].keep, 8);
        assert!(!saved[0].options.contains_key("igdb") && !saved[1].options["igdb"]);
    }

    /// The wizard's "in parts": an AI job's run takes at most `limit` titles.
    #[test]
    fn an_ai_job_runs_in_parts() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("upnext.db"), 120).unwrap();
        let cfg = crate::testutil::offline_config(dir.path());
        let limit = |task, n| change(&db, task, &Change { schedule: Some("daily".into()), limit: Some(n), ..Default::default() });
        let digests = limit(Task::Digests, 300).unwrap();
        assert_eq!(digests.limit, Some(300));
        let JobKind::Enrich(o) = digests.job(&cfg, false) else { panic!("an enrich") };
        assert_eq!(o.limit, Some(300));
        assert_eq!(setups(&db).unwrap()[3].limit, Some(300), "kept");
        assert!(limit(Task::Sync, 300).unwrap_err().to_string().contains("use the AI"));
        assert_eq!(limit(Task::Digests, 0).unwrap().limit, None, "0 is no limit");
    }
}
