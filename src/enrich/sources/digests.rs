//! AI review summaries, vibes and review tags for titles that have player
//! reviews but no summary, or one written with an older prompt
//! (`DIGEST_VERSION`), or every such title with `--refresh`; a few batches at
//! a time. Off unless asked for: it uses the LLM. Titles with summaries
//! from UpNext's shared data are skipped unless the run names them.

use super::{Opt, Pass};
use crate::db::{AiKind, Failure, GameView};
use crate::digest::DIGEST_VERSION;
use crate::enrich::{Progress, Run};
use crate::review_tags::Dropped;
use anyhow::Result;
use async_trait::async_trait;

const OPTIONS: &[Opt] = &[Opt::new("digests", "AI review digests").off().note("uses the AI")];

pub struct Digests;

#[async_trait]
impl Pass for Digests {
    fn options(&self) -> &'static [Opt] {
        OPTIONS
    }
    fn name(&self) -> &'static str {
        "review digests"
    }
    fn lane(&self) -> &'static str {
        AiKind::ReviewSummaries.lane().expect("a job's lane")
    }
    fn wanted(&self, run: &Run<'_>) -> bool {
        run.on("digests")
    }

    async fn run(&self, run: &Run<'_>) -> Result<()> {
        // Titles that joined the library since shared data was applied
        // get its summaries first, so the AI isn't asked for them.
        match crate::seed::apply_kept(run.db, run.cfg, "the kept shared data, before writing summaries") {
            Ok(Some(done)) if done.written > 0 => {
                run.log(format!("{} from UpNext's shared data", crate::enrich::plural(done.written, "summary", "summaries")))
            }
            Ok(_) => {}
            Err(e) => run.log(format!("the kept shared data wasn't applied: {e:#}")),
        }
        let Some(llm) = run.summary_llm else {
            run.log("digests skipped: no LLM backend available".into());
            return Ok(());
        };
        // Summaries saved empty or as "placeholder" before the schema kept
        // its key order are written again.
        match run.db.remove_digests_where(crate::digest::stand_in) {
            Ok(0) => {}
            Ok(1) => run.log("1 summary held only a placeholder; it is written again".into()),
            Ok(n) => run.log(format!("{n} summaries held only a placeholder; they are written again")),
            Err(e) => run.log(format!("couldn't look for placeholder summaries: {e:#}")),
        }
        // Summaries from shared data are written again only for titles the
        // run names (`--titles`).
        let mut pending: Vec<GameView> = run
            .db
            .library_games()?
            .into_iter()
            .filter(|g| {
                run.includes(g)
                    && crate::digest::wanted(g)
                    && (g.seed_date.is_none() || run.named(g))
                    && (run.refresh || crate::digest::outdated(g))
            })
            .collect();
        pending.sort_by_key(|g| (g.offer().is_none(), g.name.to_lowercase()));
        if let Some(limit) = run.limit {
            pending.truncate(limit);
        }
        let mut progress = Progress {
            total: pending.len(),
            done: 0,
            current: None,
            message: format!(
                "Writing review digests ({}, up to {} per call)",
                crate::enrich::plural(pending.len(), "title", "titles"),
                crate::digest::BATCH
            ),
        };
        run.sink.update(&progress);
        run.log(format!(
            "{} a review digest (version {DIGEST_VERSION})",
            crate::enrich::plural(pending.len(), "title needs", "titles need")
        ));
        // A few calls at once: each is mostly waiting on the model.
        let chunks = crate::digest::batches(&pending);
        let mut dropped = Dropped::default();
        let model = run.cfg.summary_model();
        let options = run.options_on(OPTIONS);
        super::ai_batches(
            run,
            AiKind::ReviewSummaries,
            &chunks,
            |chunk| crate::digest::generate(run.db, llm, model.clone(), chunk),
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
                        run.add("digests", written.digests);
                        dropped += written.dropped;
                        for g in chunk.iter().filter(|g| written.missing.contains(&g.id)) {
                            fail(g, "the AI's reply left this title out".into());
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
        if !pending.is_empty() {
            run.log(format!("review tags dropped: {} outside the vocabulary, {} over the caps", dropped.unknown, dropped.over_cap));
        }
        Ok(())
    }
}
