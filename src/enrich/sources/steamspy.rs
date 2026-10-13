//! Player-voted tags from SteamSpy.

use super::{Fetched, Opt, TitleSource};
use crate::db::GameView;
use crate::enrich::{Run, Title, game_like};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashSet;
use std::sync::Mutex;

const OPTIONS: &[Opt] = &[Opt::new("tags", "Tags from SteamSpy")];

#[derive(Default)]
pub struct SteamSpy {
    /// Appids whose tags are stored (several titles can share one).
    known: Mutex<HashSet<u32>>,
}

#[async_trait]
impl TitleSource for SteamSpy {
    fn options(&self) -> &'static [Opt] {
        OPTIONS
    }
    fn lane(&self) -> &'static str {
        "SteamSpy"
    }
    fn name(&self) -> &'static str {
        "SteamSpy"
    }
    fn start(&self, all: &[GameView], run: &Run<'_>) {
        if !run.refresh {
            *self.known.lock().unwrap() = all.iter().filter(|g| g.has_tags).filter_map(|g| g.appid).collect();
        }
    }
    fn needs(&self, g: &GameView, run: &Run<'_>) -> bool {
        g.appid.is_some() && game_like(g) && (!g.has_tags || run.refresh)
    }
    fn after_steam_store(&self) -> bool {
        true
    }
    fn wants(&self, t: &Title<'_>, run: &Run<'_>) -> bool {
        let (Some(a), true) = (t.appid, t.still_game) else { return false };
        (self.needs(t.g, run) || t.fresh()) && !self.known.lock().unwrap().contains(&a)
    }

    async fn run(&self, t: &Title<'_>, run: &Run<'_>) -> Result<()> {
        let Some(a) = t.appid else { return Ok(()) };
        let tags = run.clients.spy.tags(a).await.fetched()?;
        run.db.put_tags(a, tags.as_deref())?;
        self.known.lock().unwrap().insert(a);
        run.count("tags");
        Ok(())
    }
}
