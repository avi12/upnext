//! `jobs`: the scheduled jobs, past runs and their logs, as Settings › Jobs
//! shows them.

use super::format::{bold, dim, green, red, trunc};
use super::open_db;
use crate::config::Config;
use crate::db::{Job, JobFilter, JobStatus, Level, now};
use crate::jobs::tasks::{self, Next, Slot, Task};
use anyhow::{Result, anyhow};
use comfy_table::{ContentArrangement, Table, presets::UTF8_FULL_CONDENSED};

fn local(t: i64) -> String {
    chrono::DateTime::from_timestamp(t, 0).map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default()
}

/// "3 h ago", "in 21 h"
pub(super) fn relative(t: i64) -> String {
    let d = t - now();
    let span = took(d.abs());
    if d.abs() < 60 {
        "now".into()
    } else if d < 0 {
        format!("{span} ago")
    } else {
        format!("in {span}")
    }
}

/// "42 s", "3 min", "1 h 38 min", "4 days"
fn took(secs: i64) -> String {
    match secs {
        s if s < 60 => format!("{s} s"),
        s if s < 3600 => format!("{} min", s / 60),
        s if s < 2 * 86_400 => match (s / 3600, s % 3600 / 60) {
            (h, 0) => format!("{h} h"),
            (h, m) => format!("{h} h {m} min"),
        },
        s => format!("{} days", (s + 43_200) / 86_400),
    }
}

/// "Done: 2,000 games, 3 new", "Failed: …", "Cancelled"
fn result(job: &Job) -> String {
    let failed = if job.failed > 0 { format!(", {} failed", crate::enrich::thousands(job.failed)) } else { String::new() };
    match job.status {
        JobStatus::Running => format!("Running, {} of {}", crate::enrich::thousands(job.done), crate::enrich::thousands(job.total)),
        JobStatus::Done => format!("Done{}{failed}", job.summary.as_deref().map(|s| format!(": {s}")).unwrap_or_default()),
        JobStatus::Cancelled => format!("Cancelled{failed}"),
        JobStatus::Failed => format!("Failed: {}", trunc(job.error.as_deref().unwrap_or("?"), 60)),
    }
}

fn duration(job: &Job) -> String {
    job.finished_at.map(|f| took(f - job.started_at)).unwrap_or_default()
}

fn schedule_text(s: tasks::Schedule) -> String {
    match s {
        tasks::Schedule::After(t) => format!("after {}", t.label_mid()),
        other => other.text(),
    }
}

/// The scheduled jobs: schedule, last run, result and next run.
pub fn list(cfg: &Config) -> Result<()> {
    let (db, cfg) = open_db(cfg)?;
    let setups = tasks::setups(&db)?;
    let runs = db.task_runs()?;
    let slots: Vec<Slot> = setups.iter().map(|s| Slot::new(s, runs.get(s.task.id()))).collect();
    let mut table = Table::new();
    table.load_style(UTF8_FULL_CONDENSED).set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(["Job", "Id", "Schedule", "Last run", "Result", "Took", "Next run"]);
    for (setup, slot) in setups.iter().zip(&slots) {
        let last = runs.get(setup.task.id()).and_then(|r| r.last.as_ref());
        let mut schedule = schedule_text(setup.schedule);
        if setup.task == Task::Backup {
            schedule.push_str(&format!(", keep {}", setup.keep));
        }
        if setup.task.uses_ai() {
            schedule.push_str(", uses the AI");
        }
        let next = match tasks::next(slot, &slots) {
            Next::Off => "-".into(),
            Next::At(t) if t <= now() => "when nothing else runs".into(),
            Next::At(t) => relative(t),
            Next::After(t) => format!("after the next {}", t.noun()),
        };
        table.add_row([
            setup.task.label().to_string(),
            setup.task.id().to_string(),
            schedule,
            last.map(|j| relative(j.started_at)).unwrap_or_else(|| "never".into()),
            last.map(result).unwrap_or_default(),
            last.map(duration).unwrap_or_default(),
            next,
        ]);
    }
    println!("{table}");
    if !cfg.auto_sync {
        println!("{}", red("Schedules are off: nothing runs by itself. Turn them on with `upnext config set auto_sync true`."));
    } else if db.meta_get("last_sync")?.is_none() {
        println!("Nothing runs by itself until the first sync (`upnext sync`).");
    }
    println!(
        "{}",
        dim(
            "Schedules run while `upnext serve` does. Change one with `upnext jobs schedule <job> <when>`; past runs: `upnext jobs history`."
        )
    );
    Ok(())
}

/// Past runs, newest first.
pub fn history(cfg: &Config, limit: usize, failed: bool) -> Result<()> {
    let (db, _cfg) = open_db(cfg)?;
    let mut jobs: Vec<Job> = db.job_running()?.into_iter().collect();
    jobs.extend(db.jobs_recent(&JobFilter { failed, limit, ..Default::default() })?);
    if jobs.is_empty() {
        println!("No jobs yet.");
        return Ok(());
    }
    let mut table = Table::new();
    table.load_style(UTF8_FULL_CONDENSED).set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(["#", "Started", "Job", "By", "Result", "Took", "Tokens"]);
    for j in &jobs {
        let name = j.task.as_deref().and_then(Task::parse).map_or_else(|| j.kind.clone(), |t| t.label().to_string());
        let by = match j.source {
            crate::db::JobSource::Ui => "hand",
            crate::db::JobSource::Schedule => "schedule",
            crate::db::JobSource::Terminal => "terminal",
        };
        let tokens = j
            .ai_usage
            .as_ref()
            .map(|u| {
                format!(
                    "{} in, {} out",
                    crate::ai_usage::tokens(u.totals.usage.input_tokens),
                    crate::ai_usage::tokens(u.totals.usage.output_tokens)
                )
            })
            .unwrap_or_default();
        table.add_row([j.id.to_string(), local(j.started_at), name, by.into(), result(j), duration(j), tokens]);
    }
    println!("{table}");
    println!("{}", dim("A job's log: `upnext jobs log <#>`."));
    Ok(())
}

/// A job's log, oldest line first.
pub fn log(cfg: &Config, id: i64, warnings: bool) -> Result<()> {
    let (db, _cfg) = open_db(cfg)?;
    let job = db.job(id)?.ok_or_else(|| anyhow!("no job {id}; `upnext jobs history` lists them"))?;
    let name = job.task.as_deref().and_then(Task::parse).map_or_else(|| job.kind.clone(), |t| t.label().to_string());
    println!("{} #{id}, started {} {}: {}", bold(&name), local(job.started_at), job.source.started_from(), result(&job));
    let min = if warnings { Level::Warn } else { Level::Info };
    for l in db.job_log(id, 0, min, usize::MAX >> 1)? {
        let lane = l.line.lane.as_deref().map(|n| format!("[{n}] ")).unwrap_or_default();
        let line = format!("{} {lane}{}", dim(&local(l.line.at)), l.line.message);
        match l.line.level {
            Level::Error => println!("{}", red(&line)),
            _ => println!("{line}"),
        }
    }
    for f in db.job_failures(id)? {
        println!("{} {}", red("failed:"), crate::enrich::failure_line(&f));
    }
    Ok(())
}

/// Change when a job runs, or the backups it keeps.
pub fn schedule(cfg: &Config, task: &str, when: Option<&str>, keep: Option<usize>) -> Result<()> {
    let (db, _cfg) = open_db(cfg)?;
    let task = Task::parse(task).ok_or_else(|| anyhow!("no job {task:?}"))?;
    let setup = tasks::change(&db, task, &tasks::Change { schedule: when.map(str::to_string), options: None, keep, limit: None })?;
    let keep = if task == Task::Backup { format!(", keeping {}", setup.keep) } else { String::new() };
    println!("{} {}: {}{keep}", green("✓"), task.label(), schedule_text(setup.schedule));
    Ok(())
}

/// The task ids, for `jobs schedule`.
pub fn task_ids() -> Vec<&'static str> {
    Task::ALL.iter().map(|t| t.id()).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn durations_read_naturally() {
        assert_eq!(super::took(42), "42 s");
        assert_eq!(super::took(62), "1 min");
        assert_eq!(super::took(3600 + 38 * 60), "1 h 38 min");
        assert_eq!(super::took(3 * 3600), "3 h");
        assert_eq!(super::took(8 * 86_400), "8 days");
    }
}
