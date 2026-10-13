//! `taste`: the taste profile as the profile page shows it, and `--update`
//! to count its patterns again, as the profile job does after each sync.

use super::format::{bold, dim, green};
use super::open_db;
use super::sync::{begin, stopped};
use crate::config::Config;
use crate::jobs::JobKind;
use crate::taste;
use anyhow::Result;
use std::sync::Arc;

pub async fn show(cfg: &Config, update: bool) -> Result<()> {
    let (db, cfg) = open_db(cfg)?;
    let db = Arc::new(db);
    if update {
        let (job, bar) = begin(&db, &cfg, &JobKind::Taste)?;
        let result = job.scope(taste::run_job(&db, cfg.whose(), &cfg.taste_rules(), &*job)).await;
        let status = job.finish(result.as_ref().map(String::clone));
        bar.finish_and_clear();
        println!("{} {}", green("✓"), stopped(status, result)?);
    }
    let p = taste::page(&db, cfg.whose())?;
    let none = |empty: bool| {
        if empty {
            println!("  {}", dim("none yet"))
        }
    };

    println!("{}", bold("Favourites"));
    none(p.favourites.is_empty());
    for f in &p.favourites {
        let why: Vec<&str> = f.why.iter().map(|c| c.label.as_str()).collect();
        let note = f.note.as_deref().map(|n| format!(" · {n}")).unwrap_or_default();
        println!("  {} {}{note}", f.game.name, dim(&why.join(", ")));
    }
    for (kind, title) in [("like", "Likes"), ("dislike", "Dislikes"), ("hard_no", "Hard nos")] {
        println!("{}", bold(title));
        let list: Vec<&taste::StatementView> = p.statements.iter().filter(|s| s.kind == kind).collect();
        none(list.is_empty());
        for s in list {
            let matches = s.matches.map(|n| format!(" ({})", crate::enrich::plural(n, "game", "games"))).unwrap_or_default();
            println!("  {}{}", s.label, dim(&matches));
        }
    }
    println!("{}", bold("Patterns"));
    let updated = p.updated_at.map(|t| format!(", counted {}", super::jobs::relative(t))).unwrap_or_default();
    println!(
        "  {}",
        dim(&format!(
            "from {} you stuck with and {} you left{updated}",
            crate::enrich::plural(p.judged.kept, "game", "games"),
            p.judged.left
        ))
    );
    none(p.patterns.is_empty());
    for x in &p.patterns {
        let sure = if x.sure { "sure" } else { "fairly sure" };
        println!("  {} {}", x.sentence, dim(&format!("({}; {sure}, #{})", x.summary, x.id)));
    }
    if !p.hidden_patterns.is_empty() {
        println!("  {}", dim(&format!("{} more you said weren't right", p.hidden_patterns.len())));
    }
    println!("{}", bold("Habits (personal)"));
    none(p.habits.is_empty());
    for h in &p.habits {
        let value = h.value.as_ref().map(|v| v.to_string()).unwrap_or_default();
        let note = h.note.as_deref().map(|n| format!(" · {n}")).unwrap_or_default();
        println!("  {}: {value}{note}", h.about);
    }
    println!("{}", bold("Routines (personal)"));
    let routines = db.routines(cfg.whose())?;
    none(routines.is_empty());
    let now = taste::routine::for_now(&routines, crate::db::now()).map(|r| r.id);
    for r in &routines {
        let said: Vec<String> = [r.when_text(), r.session_text()].into_iter().filter(|s| !s.is_empty()).collect();
        let now = if now == Some(r.id) { " (fits now)" } else { "" };
        println!("  {}{} {}", r.name, green(now), dim(&said.join("; ")));
    }
    let d = taste::diary::state(&db, &cfg)?;
    println!("{}", bold("Play diary (personal)"));
    if !d.on && d.rows + d.launches == 0 {
        println!("  {}", dim("off; turn it on from the profile page (Tune your profile, or What UpNext knows about you)"));
    } else {
        let state = if d.on { "on" } else { "off" };
        println!(
            "  {state}: {} of play over {}, {}",
            crate::enrich::plural(d.rows, "hour", "hours"),
            crate::enrich::plural(d.days, "day", "days"),
            crate::enrich::plural(d.launches, "launch", "launches")
        );
        for note in [d.paused.as_deref(), d.error.as_deref()].into_iter().flatten() {
            println!("  {}", dim(note));
        }
        let h = &p.diary;
        let hours = |m: i64| if m % 60 == 0 { format!("{}h", m / 60) } else { format!("{:.1}h", m as f64 / 60.0) };
        for (when, s) in [("weeknight", h.weeknight), ("weekend", h.weekend)] {
            if let Some(s) = s {
                println!(
                    "  {when} sessions usually {} {}",
                    hours(s.minutes),
                    dim(&format!("({})", crate::enrich::plural(s.count, "session", "sessions")))
                );
            }
        }
        if !h.times.is_empty() {
            println!("  mostly {}", h.times.join(", ").replace('_', " "));
        }
        if let Some(m) = &h.multiplayer {
            let time = m.time.as_deref().unwrap_or("").replace('_', " ");
            println!("  multiplayer: {}, {:.0}% at the weekend, mostly {time}", hours(m.minutes), m.weekend_share * 100.0);
        }
    }
    Ok(())
}

/// `upnext mock-library`: a made-up library and profile in an empty
/// database, with a second player unless `one_player`.
pub fn mock_library(cfg: &Config, one_player: bool) -> Result<()> {
    let (db, _) = open_db(cfg)?;
    let s = if one_player { crate::mock::seed_one_player(&db)? } else { crate::mock::seed(&db)? };
    println!(
        "{} {}, {}, {} and {} in {}",
        green("✓"),
        crate::enrich::plural(s.games, "game", "games"),
        crate::enrich::plural(s.statements, "profile statement", "profile statements"),
        crate::enrich::plural(s.patterns, "pattern", "patterns"),
        crate::enrich::plural(s.routines, "routine", "routines"),
        cfg.db_path.display()
    );
    println!(
        "{}",
        dim(&format!(
            "Made up for trying UpNext: schedules are off, so nothing syncs into it. Its play diary is on, with {}.",
            crate::enrich::plural(s.diary, "made-up hour", "made-up hours")
        ))
    );
    Ok(())
}
