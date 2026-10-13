//! Steam cover art, hero and screenshot URLs, 100 apps per request (not the
//! rate-limited store API). Runs before the lanes, so a new library shows
//! art in seconds, and again after them for games matched to Steam meanwhile.

use super::{Opt, Pass};
use crate::enrich::{Progress, Run};
use crate::steam;
use anyhow::Result;
use async_trait::async_trait;

const OPTIONS: &[Opt] = &[Opt::new("art", "Cover art and screenshots")];

pub struct Art {
    /// The second pass, for appids the lanes found. It never refreshes.
    pub after_lanes: bool,
}

#[async_trait]
impl Pass for Art {
    fn options(&self) -> &'static [Opt] {
        if self.after_lanes { &[] } else { OPTIONS }
    }
    fn name(&self) -> &'static str {
        "Steam art"
    }
    fn wanted(&self, run: &Run<'_>) -> bool {
        run.on("art")
    }

    async fn run(&self, run: &Run<'_>) -> Result<()> {
        let refresh = run.refresh && !self.after_lanes;
        let pending = run.db.appids_without_art(refresh)?;
        if pending.is_empty() {
            return Ok(());
        }
        let mut progress = Progress { total: pending.len(), done: 0, current: None, message: "Finding cover art on Steam".into() };
        run.sink.update(&progress);
        let mut found_art = 0;
        for chunk in pending.chunks(100) {
            run.check_cancelled()?;
            match steam::store_art(&run.clients.steam, chunk).await {
                Ok(found) => {
                    for appid in chunk {
                        let (p, h, shots) = found.get(appid).cloned().unwrap_or_default();
                        run.db.put_steam_art(*appid, p.as_deref(), h.as_deref(), &shots)?;
                        if p.is_some() {
                            found_art += 1;
                        }
                    }
                }
                Err(e) => run.error(format!("Steam art lookup failed: {e:#}")),
            }
            progress.done += chunk.len();
            run.sink.update(&progress);
        }
        run.add("art", found_art);
        run.log(format!("Steam: cover art for {found_art} of {}", crate::enrich::plural(pending.len(), "app", "apps")));
        Ok(())
    }
}
