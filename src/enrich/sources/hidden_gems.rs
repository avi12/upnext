//! Find hidden gems: the AI rates the games in the gem pool that
//! it hasn't rated yet (or rated with an older prompt, or all of them on a
//! refresh) on how little-known the studio is, how rare the mechanics are
//! and how odd the play is, many games per call (`discovery::gems`). Off
//! unless asked for: it uses the LLM.

use super::{Opt, Pass};
use crate::db::{AiKind, Failure, GameView};
use crate::discovery::gems;
use crate::enrich::{Progress, Run};
use anyhow::Result;
use async_trait::async_trait;

const OPTIONS: &[Opt] = &[Opt::new("hidden_gems", "AI hidden gems").off().note("uses the AI")];

pub struct HiddenGems;

#[async_trait]
impl Pass for HiddenGems {
    fn options(&self) -> &'static [Opt] {
        OPTIONS
    }
    fn name(&self) -> &'static str {
        "hidden gems"
    }
    fn lane(&self) -> &'static str {
        AiKind::HiddenGems.lane().expect("a job's lane")
    }
    fn wanted(&self, run: &Run<'_>) -> bool {
        run.on("hidden_gems")
    }

    async fn run(&self, run: &Run<'_>) -> Result<()> {
        let Some(llm) = run.summary_llm else {
            run.log("hidden gems skipped: no LLM backend available".into());
            return Ok(());
        };
        // The ratings are the household's: the pool is every game in some
        // player's pool, each judged by that player's hard nos.
        let games = run.db.library_games()?;
        let rules = gems::Rules::from_config(run.cfg);
        let rated = run.db.hidden_gems()?;
        let mut in_some_pool = std::collections::HashSet::new();
        for p in run.db.players()? {
            let theirs = run.db.all_games(p.id)?;
            let profile = crate::taste::Profile::load(run.db, p.id, &theirs)?;
            in_some_pool.extend(gems::pool(&theirs, &rules, Some(&profile)).into_iter().map(|g| g.id));
        }
        let pool: Vec<&GameView> = games.iter().filter(|g| in_some_pool.contains(&g.id)).collect();
        let mut pending: Vec<&GameView> =
            pool.iter().copied().filter(|g| run.includes(g) && gems::needs_rating(g, &rated, run.refresh)).collect();
        pending.sort_by_key(|g| g.name.to_lowercase());
        if let Some(limit) = run.limit {
            pending.truncate(limit);
        }
        run.log(format!(
            "{} of the {} in the pool {} rating (unplayed, {}% of {} or more, {} {} or less; the rest are rated)",
            pending.len(),
            crate::enrich::plural(pool.len(), "game", "games"),
            if pending.len() == 1 { "needs" } else { "need" },
            rules.min_positive,
            crate::enrich::plural(rules.min_reviews, "review", "reviews"),
            rules.max_hours,
            if rules.max_hours == 1.0 { "hour" } else { "hours" }
        ));
        let mut progress = Progress {
            total: pending.len(),
            done: 0,
            current: None,
            message: if pending.len() == 1 {
                format!("Rating how unusual 1 game is (up to {} per call)", gems::BATCH)
            } else {
                format!("Rating how unusual {} games are (up to {} per call)", crate::enrich::thousands(pending.len()), gems::BATCH)
            },
        };
        run.sink.update(&progress);
        let rarity = gems::Rarity::of(&games);
        let appids: Vec<u32> = pending.iter().filter_map(|g| g.appid).collect();
        let publishers = run.db.publishers(&appids)?;
        let chunks: Vec<&[&GameView]> = pending.chunks(gems::BATCH).collect();
        let model = run.cfg.summary_model();
        let options = run.options_on(OPTIONS);
        super::ai_batches(
            run,
            AiKind::HiddenGems,
            &chunks,
            |chunk| gems::generate(run.db, llm, model.clone(), chunk, &rarity, &publishers),
            |chunk, result| {
                let fail = |g: &GameView, error: String| {
                    run.fail(Failure {
                        lane: self.lane().into(),
                        source: self.name().into(),
                        options: options.clone(),
                        title_id: Some(g.id),
                        name: Some(g.name.clone()),
                        error,
                        at: 0,
                    })
                };
                match result {
                    Ok(written) => {
                        run.add("hidden_gems", written.written);
                        for g in chunk.iter().filter(|g| written.missing.contains(&g.id)) {
                            fail(g, "the AI's reply left this game out".into());
                        }
                    }
                    Err(e) => {
                        for g in chunk.iter() {
                            fail(g, format!("{e:#}"));
                        }
                    }
                }
                progress.done += chunk.len();
                run.sink.update(&progress);
            },
        )
        .await?;
        Ok(())
    }
}
