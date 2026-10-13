//! RAWG tags, genres and rating: by the slug Wikidata gives, else by name.
//! Needs a (free, per-user) RAWG key.

use super::{Fetched, Opt, TitleSource};
use crate::db::{GameView, Status};
use crate::enrich::{Run, Title};
use anyhow::Result;
use async_trait::async_trait;

const OPTIONS: &[Opt] = &[Opt::new("rawg", "RAWG").blocked(|cfg| cfg.rawg_api_key.is_none().then_some("needs a key"))];

pub struct Rawg;

#[async_trait]
impl TitleSource for Rawg {
    fn options(&self) -> &'static [Opt] {
        OPTIONS
    }
    fn lane(&self) -> &'static str {
        "RAWG"
    }
    fn name(&self) -> &'static str {
        "RAWG"
    }
    async fn available(&self, run: &Run<'_>) -> bool {
        run.rawg().await.is_some()
    }
    fn needs(&self, g: &GameView, run: &Run<'_>) -> bool {
        g.status != Status::NotGame && (!g.has_rawg || run.refresh)
    }
    fn wants(&self, t: &Title<'_>, run: &Run<'_>) -> bool {
        self.needs(t.g, run) && t.game_like()
    }
    fn gives_up_on(&self, e: &anyhow::Error) -> bool {
        e.downcast_ref::<crate::rawg::KeyRejected>().is_some()
    }

    async fn run(&self, t: &Title<'_>, run: &Run<'_>) -> Result<()> {
        let Some(rawg) = run.rawg().await else { return Ok(()) };
        let g = t.g;
        let found = match g.xref.as_ref().and_then(|x| x.rawg_slug.clone()) {
            Some(slug) => match rawg.by_slug(&slug).await {
                Ok(None) => rawg.search(&g.name, 0.85).await,
                other => other,
            },
            None => rawg.search(&g.name, 0.85).await,
        }
        .fetched()?;
        run.db.put_rawg(t.id(run.db)?, found.as_ref())?;
        run.count("rawg");
        Ok(())
    }
}
