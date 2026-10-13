//! Steam review scores and the most helpful player reviews, in one request.

use super::{Fetched, Opt, TitleSource};
use crate::db::GameView;
use crate::enrich::{Run, Title, game_like};
use crate::steam;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashSet;
use std::sync::Mutex;

const OPTIONS: &[Opt] = &[Opt::new("reviews", "Steam review scores"), Opt::new("review_texts", "Player reviews")];

/// Failures in a row after which Steam's reviews stop for the rest of the
/// run. The Second Chance review fetch asks Steam for the same reviews.
pub(super) const GIVE_UP_AFTER: u32 = 5;

#[derive(Default)]
pub struct SteamReviews {
    /// Appids whose score is stored (several titles can share one).
    scored: Mutex<HashSet<u32>>,
}

impl SteamReviews {
    fn needs_score(g: &GameView, run: &Run<'_>) -> bool {
        g.appid.is_some() && game_like(g) && (!g.has_reviews || run.refresh)
    }

    fn needs_texts(g: &GameView, run: &Run<'_>) -> bool {
        g.appid.is_some() && game_like(g) && (!g.has_review_texts || run.refresh) && Self::texts_allowed(g, run)
    }

    /// A title with its summary from UpNext's shared data needs no
    /// review texts: the game page fetches them when they're asked for. A
    /// run that names it (`--titles`) still fetches them.
    fn texts_allowed(g: &GameView, run: &Run<'_>) -> bool {
        g.seed_date.is_none() || run.named(g)
    }

    /// Whether to store the score and the texts for this title now.
    fn want(&self, t: &Title<'_>, run: &Run<'_>) -> (bool, bool) {
        let (Some(a), true) = (t.appid, t.still_game) else { return (false, false) };
        let (g, fresh) = (t.g, t.fresh());
        let score = run.on("reviews") && (Self::needs_score(g, run) || fresh) && !self.scored.lock().unwrap().contains(&a);
        let texts = run.on("review_texts")
            && (Self::needs_texts(g, run) || fresh)
            && (!g.has_review_texts || run.refresh || fresh)
            && Self::texts_allowed(g, run);
        (score, texts)
    }
}

#[async_trait]
impl TitleSource for SteamReviews {
    fn options(&self) -> &'static [Opt] {
        OPTIONS
    }
    // `/appreviews` is throttled apart from the store API, so it gets a lane.
    fn lane(&self) -> &'static str {
        "Steam reviews"
    }
    fn name(&self) -> &'static str {
        "Steam reviews"
    }
    fn start(&self, all: &[GameView], run: &Run<'_>) {
        if !run.refresh {
            *self.scored.lock().unwrap() = all.iter().filter(|g| g.has_reviews).filter_map(|g| g.appid).collect();
        }
    }
    fn needs(&self, g: &GameView, run: &Run<'_>) -> bool {
        (run.on("reviews") && Self::needs_score(g, run)) || (run.on("review_texts") && Self::needs_texts(g, run))
    }
    fn after_steam_store(&self) -> bool {
        true
    }
    fn wants(&self, t: &Title<'_>, run: &Run<'_>) -> bool {
        let (score, texts) = self.want(t, run);
        score || texts
    }
    fn give_up_after(&self) -> Option<u32> {
        Some(GIVE_UP_AFTER)
    }

    async fn run(&self, t: &Title<'_>, run: &Run<'_>) -> Result<()> {
        let (want_score, want_texts) = self.want(t, run);
        let Some(a) = t.appid else { return Ok(()) };
        // The score alone is one request; the texts take two or three.
        let (score, texts) = if !want_texts && t.g.seed_date.is_some() {
            (run.clients.steam.review_summary(a).await.fetched()?, None)
        } else {
            steam::app_reviews_with_texts(&run.clients.steam, a, 10).await.fetched()?
        };
        if want_score {
            run.db.put_reviews(a, score.as_ref())?;
            self.scored.lock().unwrap().insert(a);
            run.count("reviews");
        }
        if want_texts {
            run.db.put_review_texts(a, texts.as_deref())?;
            run.count("review_texts");
        }
        Ok(())
    }
}
