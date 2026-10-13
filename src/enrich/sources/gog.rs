//! GOG store tags and features, and GOG user reviews, for each GOG copy.

use super::{Fetched, Opt, TitleSource};
use crate::db::GameView;
use crate::enrich::{Run, Title};
use anyhow::Result;
use async_trait::async_trait;

const OPTIONS: &[Opt] = &[Opt::new("gog_meta", "GOG tags and reviews")];

/// Failures in a row after which GOG stops for the rest of the run. The
/// Second Chance review fetch asks GOG for the same reviews.
pub(super) const GIVE_UP_AFTER: u32 = 3;

pub struct Gog;

#[async_trait]
impl TitleSource for Gog {
    fn options(&self) -> &'static [Opt] {
        OPTIONS
    }
    fn lane(&self) -> &'static str {
        "GOG"
    }
    fn name(&self) -> &'static str {
        "GOG"
    }
    fn needs(&self, g: &GameView, run: &Run<'_>) -> bool {
        let reviews = g.seed_date.is_none() || run.named(g);
        g.copies.iter().any(|c| c.source == "gog" && (!c.has_gog_meta || (!c.has_gog_reviews && reviews) || run.refresh))
    }
    fn give_up_after(&self) -> Option<u32> {
        Some(GIVE_UP_AFTER)
    }

    // Two requests per copy, each tried whatever the other did. The first
    // failure is the title's, which the lane lists and counts towards giving
    // up, so a GOG that is down stops holding the job; any after it are
    // logged on their own.
    async fn run(&self, t: &Title<'_>, run: &Run<'_>) -> Result<()> {
        let g = t.g;
        let mut first: Option<anyhow::Error> = None;
        let mut failed = |what: &str, e: anyhow::Error| match first {
            None => first = Some(e),
            Some(_) => run.error_game(g.id, format!("GOG {what} failed for {}: {e}", g.name)),
        };
        for c in g.copies.iter().filter(|c| c.source == "gog") {
            if !c.has_gog_meta || run.refresh {
                match run.clients.gogmeta.game(&c.source_id).await {
                    Ok(m) => {
                        run.db.put_gog_meta(&c.source_id, m.as_ref())?;
                        run.count("gog_meta");
                    }
                    Err(e) => failed("meta", e),
                }
            }
            // A summary from UpNext's shared data needs no
            // reviews; the game page fetches them when asked.
            if (!c.has_gog_reviews || run.refresh) && (g.seed_date.is_none() || run.named(g)) {
                match run.clients.gogmeta.reviews(&c.source_id, 8).await {
                    Ok(r) => run.db.put_gog_reviews(&c.source_id, r.as_ref())?,
                    Err(e) => failed("reviews", e),
                }
            }
        }
        first.map_or(Ok(()), Err).fetched()
    }
}
