//! ProtonDB tiers, for games without a native Linux build. Only when Linux
//! mode is on.

use super::{Fetched, Opt, TitleSource};
use crate::db::GameView;
use crate::enrich::{Run, Title, game_like};
use anyhow::Result;
use async_trait::async_trait;

const OPTIONS: &[Opt] = &[Opt::new("protondb", "ProtonDB").blocked(|cfg| (!cfg.linux()).then_some("Linux mode is off"))];

pub struct ProtonDb;

#[async_trait]
impl TitleSource for ProtonDb {
    fn options(&self) -> &'static [Opt] {
        OPTIONS
    }
    fn lane(&self) -> &'static str {
        "ProtonDB"
    }
    fn name(&self) -> &'static str {
        "ProtonDB"
    }
    async fn available(&self, run: &Run<'_>) -> bool {
        run.cfg.linux()
    }
    fn needs(&self, g: &GameView, run: &Run<'_>) -> bool {
        g.appid.is_some() && game_like(g) && g.linux_native != Some(true) && (!g.has_protondb || run.refresh)
    }
    fn after_steam_store(&self) -> bool {
        true
    }
    fn wants(&self, t: &Title<'_>, run: &Run<'_>) -> bool {
        t.appid.is_some() && t.still_game && (self.needs(t.g, run) || (t.fresh() && !t.g.has_protondb))
    }

    async fn run(&self, t: &Title<'_>, run: &Run<'_>) -> Result<()> {
        let Some(a) = t.appid else { return Ok(()) };
        // The store step may have just learned it runs natively.
        if run.db.linux_native(a)? == Some(true) {
            return Ok(());
        }
        let p = run.clients.protondb.summary(a).await.fetched()?;
        run.db.put_protondb(a, p.as_ref())?;
        run.count("protondb");
        Ok(())
    }
}
