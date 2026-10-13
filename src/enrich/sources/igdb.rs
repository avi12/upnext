//! IGDB genres, themes, keywords, perspectives and rating, by name. Needs
//! IGDB (Twitch) credentials.

use super::{Fetched, Opt, TitleSource, needs_igdb_login};
use crate::db::{GameView, Status};
use crate::enrich::{Run, Title};
use anyhow::Result;
use async_trait::async_trait;

const OPTIONS: &[Opt] = &[Opt::new("igdb", "IGDB").blocked(needs_igdb_login)];

pub struct Igdb;

#[async_trait]
impl TitleSource for Igdb {
    fn options(&self) -> &'static [Opt] {
        OPTIONS
    }
    fn lane(&self) -> &'static str {
        "IGDB"
    }
    fn name(&self) -> &'static str {
        "IGDB"
    }
    async fn available(&self, run: &Run<'_>) -> bool {
        run.igdb().await.is_some()
    }
    fn needs(&self, g: &GameView, run: &Run<'_>) -> bool {
        g.status != Status::NotGame && (!g.has_igdb || run.refresh)
    }
    fn wants(&self, t: &Title<'_>, run: &Run<'_>) -> bool {
        self.needs(t.g, run) && t.game_like()
    }

    async fn run(&self, t: &Title<'_>, run: &Run<'_>) -> Result<()> {
        let Some(igdb) = run.igdb_if_started() else { return Ok(()) };
        let r = igdb.lookup(&t.g.name, 0.8).await.fetched()?;
        run.db.put_igdb(t.id(run.db)?, r.as_ref())?;
        run.count("igdb");
        Ok(())
    }
}
