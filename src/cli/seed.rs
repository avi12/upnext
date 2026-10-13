//! `seed apply|status|remove`: UpNext's shared game data from the
//! terminal, for installs without the first-run wizard. `seed build|fetch|
//! generate|export` make it, in a database of its own (docs/dev/RELEASE.md
//! has the steps).

use super::format::{dim, green, red};
use super::open_db;
use super::sync::{begin, stopped};
use crate::config::Config;
use crate::enrich::{Clients, plural, thousands};
use crate::jobs::JobKind;
use crate::seed::{self, build};
use crate::{db::Db, llm};
use anyhow::Result;
use std::sync::Arc;

/// Apply shared data from a file or URL (a manifest or the data file),
/// keep it for games added later, and say what it did.
pub async fn apply(cfg: &Config, from: Option<&str>) -> Result<()> {
    let (db, cfg) = open_db(cfg)?;
    let from = from.unwrap_or(seed::DEFAULT_URL);
    let http = seed::client()?;
    let loaded = seed::load(&http, from).await?;
    let done = seed::apply_and_record(&db, &loaded.seed, from)?;
    seed::keep(&cfg, &loaded.seed, &loaded.data)?;
    println!(
        "{} UpNext's shared data of {}: {} of its {} {} in this library; {} written",
        green("✓"),
        done.date,
        done.matched,
        crate::enrich::plural(done.entries, "game", "games"),
        if done.matched == 1 { "is" } else { "are" },
        crate::enrich::plural(done.written, "summary", "summaries")
    );
    if done.kept_own > 0 || done.already > 0 {
        println!(
            "{}",
            dim(&format!("Left alone: {} of your own summaries, {} already from this data or newer.", done.kept_own, done.already))
        );
    }
    if done.written > 0 {
        println!(
            "{}",
            dim(
                "Your own summaries written with an older prompt were replaced; `upnext seed remove` leaves those games without one until your AI writes it again."
            )
        );
    }
    println!("{}", dim("Games added later get theirs after a sync. The data is CC0: AI-written summaries and tags, no player reviews."));
    Ok(())
}

pub fn status(cfg: &Config) -> Result<()> {
    let (db, cfg) = open_db(cfg)?;
    if let Some(b) = build::status(&db, &cfg)? {
        return build_status(&b);
    }
    let s = seed::status(&db, &cfg)?;
    match (&s.seed_date, &s.kept) {
        (None, None) => println!("No shared data in use. `upnext seed apply` applies the newest published data."),
        _ => println!(
            "{} summaries from UpNext's shared data{}.",
            crate::enrich::plural(s.seeded, "title has", "titles have"),
            s.seed_date.as_deref().map(|d| format!(" of {d}")).unwrap_or_default()
        ),
    }
    if let Some(k) = &s.kept {
        println!("{}", dim(&format!("Kept for games added later: {}", seed::kept_dir(&cfg).join(k).display())));
    }
    for a in &s.applies {
        println!(
            "{}",
            dim(&format!(
                "{}: {} from {}, {} matched, {} written, {} of your own kept",
                super::jobs::relative(a.applied_at),
                a.seed_date,
                a.source,
                a.matched,
                a.written,
                a.kept_own
            ))
        );
    }
    Ok(())
}

pub fn remove(cfg: &Config) -> Result<()> {
    let (db, cfg) = open_db(cfg)?;
    let gone = seed::remove(&db, &cfg)?;
    println!(
        "{} {} from shared data removed; your AI writes its own on the next summaries run.",
        green("✓"),
        crate::enrich::plural(gone, "summary", "summaries")
    );
    Ok(())
}

// -- building the data ---------------------------------------

fn build_status(b: &build::BuildStatus) -> Result<()> {
    println!("A build of UpNext's shared data, started {}:", b.started);
    let n = thousands;
    println!(
        "  {}; reviews fetched for {}, and {} reviews to summarize",
        plural(b.games, "game", "games"),
        n(b.reviews),
        plural(b.summarizable, "has", "have")
    );
    println!(
        "  {}; {} a review and {} left out of the export",
        plural(b.summaries, "summary", "summaries"),
        plural(b.copying, "copies", "copy"),
        if b.copying == 1 { "is" } else { "are" }
    );
    println!("  {}", plural(b.gog_ids, "GOG id", "GOG ids"));
    Ok(())
}

/// The build's database, checked: `start` makes an empty one a build.
fn open_build(cfg: &Config, start: bool) -> Result<(Arc<Db>, Config)> {
    let (db, cfg) = open_db(cfg)?;
    build::check(&db, &cfg, start)?;
    Ok((Arc::new(db), cfg))
}

/// The enrich job a build step runs as, with the sources it fetches, so
/// `upnext jobs history` lists it and only one runs at a time.
fn job_kind(sources: &[&str]) -> JobKind {
    JobKind::Enrich(build::only(sources))
}

pub async fn build(cfg: &Config, count: Option<usize>) -> Result<()> {
    let count = count.unwrap_or(build::DEFAULT_COUNT);
    let (db, cfg) = open_build(cfg, true)?;
    let clients = Clients::new(&cfg)?;
    let (job, bar) = begin(&db, &cfg, &job_kind(&[]))?;
    let result = job.scope(build::build(&db, &cfg, &clients, count, &*job)).await;
    let status = job.finish(result.as_ref().map(|b| format!("{} in the build", crate::enrich::plural(b.games, "game", "games"))));
    bar.finish_and_clear();
    let b = stopped(status, result)?;
    println!(
        "{} {} in the build, the most owned of {} on SteamSpy's list ({} above the last one skipped: not a game, or no store page). Next: `upnext seed fetch`.",
        green("✓"),
        plural(b.games, "game", "games"),
        plural(b.listed, "app", "apps"),
        plural(b.skipped, "app", "apps")
    );
    Ok(())
}

pub async fn fetch(cfg: &Config) -> Result<()> {
    let (db, cfg) = open_build(cfg, false)?;
    let clients = Clients::new(&cfg)?;
    let (job, bar) = begin(&db, &cfg, &job_kind(build::FETCH))?;
    let result = job.scope(build::fetch(&db, &cfg, &clients, &*job)).await;
    let status = job.finish(result.as_ref().map(|f| {
        format!("{}, {}", crate::enrich::plural(f.reviews, "review", "reviews"), crate::enrich::plural(f.gog_ids, "GOG id", "GOG ids"))
    }));
    bar.finish_and_clear();
    let f = stopped(status, result)?;
    println!(
        "{} Reviews for {} now; {} a GOG id.",
        green("✓"),
        plural(f.reviews, "game", "games"),
        plural(f.gog_ids, "game has", "games have")
    );
    if f.errors > 0 {
        println!("  {} failed: run `upnext seed fetch` again to retry.", plural(f.errors, "request", "requests"));
    }
    Ok(())
}

pub async fn generate(cfg: &Config, limit: Option<usize>) -> Result<()> {
    let (db, cfg) = open_build(cfg, false)?;
    let clients = Clients::new(&cfg)?;
    // The summaries' backend: a model at home for this bulk work, say.
    let llm = llm::summary_backend(&cfg);
    let model = cfg.summary_model().unwrap_or_else(|| "its default model".into());
    let on = match cfg.summary_backend() {
        "openai" => {
            let thinking = if cfg.openai_thinking() == Some(false) { "off" } else { "the model's default" };
            format!("{} at {}, thinking {thinking}", model, cfg.openai_base_url)
        }
        backend => format!("{model} on {backend}"),
    };
    println!("{}", dim(&format!("Writing summaries with {on}.")));
    if let Err(e) = llm.available().await {
        anyhow::bail!("the AI can't be used: {e}");
    }
    let (job, bar) = begin(&db, &cfg, &job_kind(&["digests"]))?;
    let result = job.scope(build::generate(&db, &cfg, &clients, llm.as_ref(), limit, &*job)).await;
    let status = job.finish(result.as_ref().map(|g| crate::enrich::plural(g.written + g.rewritten, "summary", "summaries")));
    bar.finish_and_clear();
    let g = stopped(status, result)?;
    println!(
        "{} {} written, {} written again in the model's own words.",
        green("✓"),
        plural(g.written, "summary", "summaries"),
        thousands(g.rewritten)
    );
    if g.unwritten > 0 {
        println!(
            "  {} no summary yet: run `upnext seed generate` again.",
            plural(g.unwritten, "game with reviews has", "games with reviews have")
        );
    }
    if g.copying > 0 {
        let (copy, it) = if g.copying == 1 { ("summary still copies", "it") } else { ("summaries still copy", "them") };
        println!("  {} {copy} a review; the export leaves {it} out.", thousands(g.copying));
    }
    Ok(())
}

pub fn export(cfg: &Config, out: &std::path::Path, date: Option<&str>) -> Result<()> {
    let (db, cfg) = open_db(cfg)?;
    let date = date.map_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string(), str::to_string);
    let e = build::export(&db, &cfg, out, &date)?;
    println!(
        "{} {}, {}: {} and {}",
        green("✓"),
        plural(e.manifest.count, "game", "games"),
        plural(e.manifest.bytes, "byte", "bytes"),
        e.data_file.display(),
        e.manifest_file.display()
    );
    println!(
        "{}",
        dim(&format!(
            "Left out: {} without a summary, {} whose summary copies a review. Check it with `upnext seed apply {}` on a mock library.",
            plural(e.unwritten, "game", "games"),
            thousands(e.copying),
            e.manifest_file.display()
        ))
    );
    match e.gog_ids {
        Some(n) => println!("{}", dim(&format!("{} of them {} a GOG id.", thousands(n), if n == 1 { "has" } else { "have" }))),
        None => println!(
            "{} No GOG ids: `upnext seed fetch` hasn't looked them up, so GOG copies won't match this export. Run it, then export again.",
            red("!")
        ),
    }
    Ok(())
}
