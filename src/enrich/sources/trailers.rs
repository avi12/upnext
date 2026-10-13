//! YouTube trailer ids (and first release years and screenshots) from IGDB,
//! for titles IGDB matched, 500 per request. Needs IGDB credentials.

use super::{Opt, Pass, needs_igdb_login};
use crate::enrich::{Progress, Run};
use anyhow::Result;
use async_trait::async_trait;

const OPTIONS: &[Opt] = &[Opt::new("trailers", "Trailers").blocked(needs_igdb_login)];

pub struct Trailers;

#[async_trait]
impl Pass for Trailers {
    fn options(&self) -> &'static [Opt] {
        OPTIONS
    }
    fn name(&self) -> &'static str {
        "IGDB trailers"
    }
    fn wanted(&self, run: &Run<'_>) -> bool {
        run.on("trailers")
    }
    async fn available(&self, run: &Run<'_>) -> bool {
        run.igdb().await.is_some()
    }

    async fn run(&self, run: &Run<'_>) -> Result<()> {
        let Some(igdb) = run.igdb_if_started() else { return Ok(()) };
        let pending = run.db.igdb_ids_without_trailer(run.refresh)?;
        if pending.is_empty() {
            return Ok(());
        }
        let mut progress = Progress { total: pending.len(), done: 0, current: None, message: "Finding trailers on IGDB".into() };
        run.sink.update(&progress);
        let mut with_trailer = 0;
        for chunk in pending.chunks(500) {
            run.check_cancelled()?;
            let ids: Vec<i64> = chunk.iter().map(|(_, i)| *i).collect();
            match igdb.trailers(&ids).await {
                Ok(found) => {
                    for (title_id, igdb_id) in chunk {
                        let (yt, year, shots) = found.get(igdb_id).cloned().unwrap_or_default();
                        run.db.put_trailer(*title_id, yt.as_deref(), year, &shots, "igdb")?;
                        if yt.is_some() {
                            run.count("trailers");
                            with_trailer += 1;
                        }
                    }
                }
                Err(e) => run.error(format!("IGDB trailer lookup failed: {e:#}")),
            }
            progress.done += chunk.len();
            run.sink.update(&progress);
        }
        run.log(format!("IGDB: trailers for {with_trailer} of {}", crate::enrich::plural(pending.len(), "title", "titles")));
        Ok(())
    }
}
