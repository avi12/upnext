//! HowLongToBeat times, by name (every store). Off unless asked for: it is
//! the slowest source.

use super::{Fetched, Opt, TitleSource};
use crate::db::{GameView, Level, Status};
use crate::enrich::{Run, Title};
use anyhow::Result;
use async_trait::async_trait;

const OPTIONS: &[Opt] = &[Opt::new("hltb", "HowLongToBeat").off().note("slow")];

pub struct Hltb;

#[async_trait]
impl TitleSource for Hltb {
    fn options(&self) -> &'static [Opt] {
        OPTIONS
    }
    fn lane(&self) -> &'static str {
        "HowLongToBeat"
    }
    fn name(&self) -> &'static str {
        "HowLongToBeat"
    }
    fn needs(&self, g: &GameView, run: &Run<'_>) -> bool {
        g.status != Status::NotGame && (!g.has_hltb || run.refresh)
    }
    fn wants(&self, t: &Title<'_>, run: &Run<'_>) -> bool {
        self.needs(t.g, run) && t.game_like()
    }
    async fn run(&self, t: &Title<'_>, run: &Run<'_>) -> Result<()> {
        let h = run.clients.hltb.lookup(&t.g.name).await.fetched()?;
        let id = t.id(run.db)?;
        run.db.put_hltb(id, h.as_ref())?;
        run.count("hltb");
        if h.is_none() {
            run.log_game(Level::Info, id, format!("HLTB: no confident match for {}", t.g.name));
        }
        Ok(())
    }
}
