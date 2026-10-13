//! PCGamingWiki taxonomy (genres, themes, perspectives, pacing, series). The
//! page comes from Wikidata, the appid redirect, or the exact title.

use super::{Fetched, Opt, TitleSource};
use crate::db::{GameView, Status};
use crate::enrich::{Run, Title};
use anyhow::Result;
use async_trait::async_trait;

const OPTIONS: &[Opt] = &[Opt::new("pcgw", "PCGamingWiki")];

pub struct Pcgw;

#[async_trait]
impl TitleSource for Pcgw {
    fn options(&self) -> &'static [Opt] {
        OPTIONS
    }
    fn lane(&self) -> &'static str {
        "PCGamingWiki"
    }
    fn name(&self) -> &'static str {
        "PCGamingWiki"
    }
    fn needs(&self, g: &GameView, run: &Run<'_>) -> bool {
        g.status != Status::NotGame && (!g.has_pcgw || run.refresh)
    }
    fn wants(&self, t: &Title<'_>, run: &Run<'_>) -> bool {
        self.needs(t.g, run) && t.game_like()
    }
    async fn run(&self, t: &Title<'_>, run: &Run<'_>) -> Result<()> {
        let (g, pcgw) = (t.g, &run.clients.pcgw);
        let meta = async {
            let mut page = g.xref.as_ref().and_then(|x| x.pcgw_page.clone());
            if page.is_none()
                && let Some(appid) = g.appid
            {
                page = pcgw.page_for_appid(appid).await?;
            }
            if page.is_none() {
                page = pcgw.page_for_name(&g.name).await?;
            }
            match page {
                Some(p) => pcgw.meta(&p).await,
                None => Ok(None),
            }
        }
        .await
        .fetched()?;
        run.db.put_pcgw(t.id(run.db)?, meta.as_ref())?;
        run.count("pcgw");
        Ok(())
    }
}
