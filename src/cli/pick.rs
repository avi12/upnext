//! `pick`, and `prompt` (prints what `pick` would send).

use super::format::{accent, bold, dim, green, hours, red, textwrap};
use super::open_db;
use crate::config::Config;
use crate::db::PlayerId;
use crate::db::{self, GameView};
use crate::{llm, prompt, recommend};
use anyhow::{Result, bail};
use indicatif::{ProgressBar, ProgressStyle};

/// A routine by its name, any case (`--routine`).
fn routine_named(db: &db::Db, p: PlayerId, name: &str) -> Result<crate::taste::routine::Routine> {
    let all = db.routines(p)?;
    if let Some(r) = all.iter().find(|r| r.name.eq_ignore_ascii_case(name.trim())) {
        return Ok(r.clone());
    }
    match all.is_empty() {
        true => bail!("you have no routines yet; make them on the Profile page, under Routines"),
        false => {
            bail!("no routine called {name:?}; yours are {}", all.iter().map(|r| format!("{:?}", r.name)).collect::<Vec<_>>().join(", "))
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn pick(
    cfg: &Config,
    prompt: Vec<String>,
    count: usize,
    json: bool,
    sources: Vec<String>,
    exclude: Vec<String>,
    without_ai: bool,
    routine: Option<&str>,
) -> Result<()> {
    let (db, cfg) = open_db(cfg)?;
    let routine_id = routine.map(|n| routine_named(&db, cfg.whose(), n)).transpose()?.map(|r| r.id);
    let llm = llm::backend_from_config(&cfg);
    // With no AI set up, picks come from the ranking: no wait to show.
    let without_ai = without_ai || llm.available().await.is_err();
    let mut exclude_ids = vec![];
    for r in &exclude {
        match db.resolve(cfg.whose(), r)? {
            Some(g) => exclude_ids.push(g.id),
            None => bail!("unknown game ref {r:?}"),
        }
    }
    let req = recommend::PickRequest {
        prompt: prompt.join(" "),
        count,
        exclude_ids,
        sources: if sources.is_empty() { None } else { Some(sources) },
        avoid: vec![],
        without_ai,
        mode: None,
        routine_id,
        input: None,
    };
    let spinner = if json || without_ai {
        None
    } else {
        let s = ProgressBar::new_spinner();
        s.set_style(ProgressStyle::with_template("{spinner:.green} {msg}").unwrap());
        s.enable_steady_tick(std::time::Duration::from_millis(100));
        s.set_message(format!(
            "asking {} ({}) for {count} {}…",
            llm.name(),
            cfg.model.clone().unwrap_or_else(|| "default model".into()),
            if count == 1 { "pick" } else { "picks" }
        ));
        Some(s)
    };
    let store = crate::enrich::Clients::new(&cfg)?;
    let res = recommend::pick(&db, &cfg, llm.as_ref(), Some(&store), req).await;
    // The plan's bars are read after the call, in the background.
    crate::ai_plan::settle(&db).await;
    if let Some(s) = spinner {
        s.finish_and_clear();
    }
    match res {
        Ok(batch) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&batch)?);
            } else {
                print_picks(&batch);
            }
        }
        Err(e) => {
            eprintln!("{} {e}", red("✗"));
            std::process::exit(2);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn prompt(
    cfg: &Config,
    prompt: Vec<String>,
    count: usize,
    review_feel: bool,
    with_profile: bool,
    verdicts: Option<bool>,
    json: bool,
    routine: Option<&str>,
) -> Result<()> {
    let (db, cfg) = open_db(cfg)?;
    let routine = routine.map(|n| routine_named(&db, cfg.whose(), n)).transpose()?;
    let games = db.all_games(cfg.whose())?;
    let recent = db.recent_pick_ids(cfg.whose(), 3)?;
    let (mode, request) = crate::discovery::Mode::detect(&prompt.join(" "));
    let series = crate::series::of_titles(&games, &db.catalog()?);
    let profile = crate::taste::Profile::load(&db, cfg.whose(), &games)?;
    let lens = mode.map(|m| crate::discovery::Lens::build(&db, &cfg, m, &games, with_profile.then_some(&profile))).transpose()?;
    let wishlist = match cfg.wishlist && mode.is_none() {
        true => Some(crate::wishlist::picks::for_picks(&db, cfg.whose(), &games)?),
        false => None,
    };
    let built = prompt::build(&prompt::PromptInput {
        games: &games,
        request: &request,
        count,
        spares: count * crate::recommend::SPARES_PER_PICK,
        exclude_ids: &[],
        avoid: &[],
        on_screen: &[],
        series: &series,
        now: crate::db::now(),
        sources: None,
        recent_ids: &recent,
        max_candidates: cfg.max_candidates,
        sampled_minutes: cfg.sampled_minutes,
        linux: cfg.linux(),
        seed: 0,
        review_feel,
        verdicts: verdicts.unwrap_or(cfg.verdicts_prompt),
        profile: with_profile.then_some(&profile),
        lens: lens.as_ref(),
        wishlist: wishlist.as_deref(),
        routine: routine.as_ref(),
        asked_input: None,
    });
    if json {
        let req = llm::LlmRequest {
            system: built.system,
            cached_prefix: Some(built.prefix),
            prompt: built.user,
            schema: built.schema,
            model: cfg.pick_model(),
            background: false,
            effort: None,
        };
        let out = serde_json::json!({
            "system": llm::guarded_system(&req.system),
            "prompt": req.full_prompt(),
            "schema": req.schema,
            "model": req.model,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }
    println!(
        "=== SYSTEM ===\n{}\n\n=== CACHED PREFIX ===\n{}\n=== USER ===\n{}\n\n=== SCHEMA ===\n{}",
        built.system, built.prefix, built.user, built.schema
    );
    eprintln!(
        "\n{} of {} eligible; ~{} chars",
        crate::enrich::plural(built.considered, "candidate", "candidates"),
        built.eligible,
        built.prefix.len() + built.user.len() + built.system.len()
    );
    Ok(())
}

fn game_summary_line(g: &GameView) -> String {
    let mut bits: Vec<String> = vec![];
    if !g.genres.is_empty() {
        bits.push(g.genres.iter().take(3).cloned().collect::<Vec<_>>().join("/"));
    }
    if let Some(h) = g.hltb_main {
        bits.push(format!("{h}h main story"));
    }
    if let (Some(p), Some(d)) = (g.review_pct, &g.review_desc) {
        bits.push(format!("{d} ({p}%)"));
    }
    if g.playtime_known && g.playtime_minutes > 0 {
        bits.push(format!("played {}", hours(g.playtime_minutes)));
    }
    if g.installed {
        bits.push("installed".into());
    }
    bits.join(" · ")
}

fn print_picks(batch: &db::PickBatch) {
    println!();
    for (i, p) in batch.picks.iter().enumerate() {
        let g = p.game.as_ref();
        let stores = match (&p.wishlist, g) {
            (Some(_), _) => "not owned, on your wishlist".into(),
            (None, Some(g)) => g.owned_on.join(", "),
            (None, None) => "?".into(),
        };
        println!("{} {}  {}", accent(&format!("{}.", i + 1)), bold(&p.name), dim(&format!("[{stores}]")));
        if let Some(g) = g {
            let line = game_summary_line(g);
            if !line.is_empty() {
                println!("   {}", dim(&line));
            }
        }
        for line in textwrap(&p.reason, 88) {
            println!("   {line}");
        }
        if let Some(g) = g {
            for c in &g.copies {
                if !c.run_url.is_empty() {
                    println!("   {}  {}", green(&c.run_url), dim(&c.source));
                }
            }
            if !g.store_url.is_empty() {
                println!("   {}", dim(&g.store_url));
            }
            println!("   {}", dim(&format!("mark: upnext mark done #{}", g.id)));
        }
        if let Some(a) = &p.alternate {
            println!("   {}", dim(&format!("or, from your library: {}", a.name)));
        }
        println!();
    }
    if let Some(s) = &batch.suggestion {
        println!("{} {}", dim("Not in your library, but right down your alley:"), bold(&s.game.name));
        for line in textwrap(&s.reason, 88) {
            println!("   {line}");
        }
        println!("   {}", dim("add it to your wishlist, or turn it down, under Lists in the app"));
        println!();
    }
    if let Some(by) = &batch.ranked_by {
        print_ranked_by(by);
        println!(
            "{}",
            dim(&format!("{} ranked · no AI call", crate::enrich::plural(batch.candidates_considered.unwrap_or(0), "game", "games")))
        );
        return;
    }
    let tokens = batch
        .usage
        .map(|u| format!(" · {} tokens in, {} out", crate::ai_usage::tokens(u.input_tokens), crate::ai_usage::tokens(u.output_tokens)));
    let meta = format!(
        "model {} · {:.0}s · {} candidates · {} attempt(s){}{}",
        batch.model.clone().unwrap_or_else(|| "?".into()),
        batch.duration_ms.unwrap_or(0) as f64 / 1000.0,
        batch.candidates_considered.unwrap_or(0),
        batch.attempts.unwrap_or(1),
        tokens.unwrap_or_default(),
        batch.cost_usd.map(|c| format!(" · ${c:.3}")).unwrap_or_default()
    );
    println!("{}", dim(&meta));
}

/// The warning on a lineup picked without AI, and what it was ranked by.
fn print_ranked_by(by: &[String]) {
    println!("{}", bold("Picked without AI: these won't fit your request as well as the AI's picks would."));
    for line in textwrap(&format!("Ranked by {}.", by.join("; ")), 88) {
        println!("{}", dim(&line));
    }
}
