//! AI series guides (where to start, play order, spin-offs) for series whose
//! owned games changed or that have none. Off unless asked for: it uses the LLM.

use super::{Opt, Pass};
use crate::db::{AiKind, Failure};
use crate::enrich::{Progress, Run};
use crate::series::SeriesGroup;
use anyhow::Result;
use async_trait::async_trait;

const OPTIONS: &[Opt] = &[Opt::new("series_guides", "AI series guides").off().note("uses the AI")];

pub struct SeriesGuides;

#[async_trait]
impl Pass for SeriesGuides {
    fn options(&self) -> &'static [Opt] {
        OPTIONS
    }
    fn name(&self) -> &'static str {
        "series guides"
    }
    fn lane(&self) -> &'static str {
        AiKind::SeriesGuides.lane().expect("a job's lane")
    }
    fn wanted(&self, run: &Run<'_>) -> bool {
        run.on("series_guides")
    }

    async fn run(&self, run: &Run<'_>) -> Result<()> {
        let Some(llm) = run.llm else {
            run.log("series guides skipped: no LLM backend available".into());
            return Ok(());
        };
        let db = run.db;
        let guides = db.series_guides()?;
        let mut pending: Vec<SeriesGroup> = crate::series::groups(&db.library_games()?, &db.catalog()?)
            .into_iter()
            .filter(|g| run.refresh || crate::series::needs_guide(g, &guides))
            .collect();
        if let Some(limit) = run.limit {
            pending.truncate(limit);
        }
        let mut progress = Progress {
            total: pending.len(),
            done: 0,
            current: None,
            message: format!("Writing series guides ({} series, {} per call)", pending.len(), crate::series::BATCH),
        };
        run.sink.update(&progress);
        run.log(format!("{} a guide", crate::enrich::plural(pending.len(), "series needs", "series need")));
        let chunks: Vec<&[SeriesGroup]> = pending.chunks(crate::series::BATCH).collect();
        let model = run.cfg.pick_model();
        let options = run.options_on(OPTIONS);
        super::ai_batches(
            run,
            AiKind::SeriesGuides,
            &chunks,
            |chunk| crate::series::generate_guides(db, llm, model.clone(), chunk),
            |chunk, result| {
                match result {
                    Ok(written) => run.add("series_guides", written),
                    Err(e) => {
                        for group in chunk.iter() {
                            run.fail(Failure {
                                lane: self.lane().into(),
                                source: self.name().into(),
                                options: options.clone(),
                                title_id: None,
                                name: Some(format!("{} series", group.name)),
                                error: format!("{e:#}"),
                                at: 0,
                            });
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
