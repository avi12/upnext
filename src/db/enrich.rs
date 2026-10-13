//! Writes from enrichment: store matches, metadata, reviews, overrides.

use super::model::{DetailsRow, HltbRow, Override, ReviewRow, ReviewText};
use super::{Db, PlayerId, json_vec, now, to_json};
use crate::review_tags::ReviewTags;
use anyhow::Result;
use rusqlite::{OptionalExtension, params};
use std::collections::HashMap;

impl Db {
    // -- enrichment writes ----------------------------------------------------

    /// Record a Steam store match for a title. With `Some`, the title takes
    /// the appid (and merges into an existing title with that appid).
    /// Returns the title id the copies ended up in (a match can merge the
    /// title into one that already has the appid).
    pub fn put_steam_match(&self, title_id: i64, m: Option<(u32, &str, f64)>) -> Result<i64> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            let copy: Option<i64> = tx
                .query_row("SELECT id FROM games WHERE title_id=?1 AND removed=0 ORDER BY id LIMIT 1", params![title_id], |r| r.get(0))
                .optional()?;
            match m {
                Some((appid, name, sim)) => {
                    tx.execute(
                        "INSERT OR REPLACE INTO steam_match(title_id,fetched_at,success,appid,matched_name,similarity) VALUES(?1,?2,1,?3,?4,?5)",
                        params![title_id, now(), appid, name, sim],
                    )?;
                    tx.execute("UPDATE games SET appid=?2, steam_matched=1 WHERE title_id=?1 AND source<>'steam'", params![title_id, appid])?;
                    // A name-only Wikidata miss can now be retried by appid.
                    tx.execute("DELETE FROM xref WHERE title_id=?1 AND success=0", params![title_id])?;
                }
                None => {
                    tx.execute(
                        "INSERT OR REPLACE INTO steam_match(title_id,fetched_at,success) VALUES(?1,?2,0)",
                        params![title_id, now()],
                    )?;
                    tx.execute("UPDATE games SET appid=NULL, steam_matched=0 WHERE title_id=?1 AND steam_matched=1", params![title_id])?;
                }
            }
            Self::assign_titles_in(&tx)?;
            let survivor = match copy {
                Some(id) => tx.query_row("SELECT title_id FROM games WHERE id=?1", params![id], |r| r.get(0))?,
                None => title_id,
            };
            tx.commit()?;
            Ok(survivor)
        })
    }

    pub fn put_details(&self, appid: u32, d: Option<&DetailsRow>) -> Result<()> {
        self.with(|c| {
            match d {
                Some(d) => c.execute(
                    "INSERT OR REPLACE INTO details(appid,fetched_at,success,type,short_description,genres,categories,header_image,metacritic,release_date,developers,publishers,is_free,controller_support,platforms)
                     VALUES(?1,?2,1,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                    params![
                        appid,
                        now(),
                        d.type_,
                        d.short_description,
                        to_json(&d.genres),
                        to_json(&d.categories),
                        d.header_image,
                        d.metacritic,
                        d.release_date,
                        to_json(&d.developers),
                        to_json(&d.publishers),
                        d.is_free.map(|b| b as i64),
                        d.controller_support,
                        to_json(&d.platforms)
                    ],
                )?,
                None => c.execute("INSERT OR REPLACE INTO details(appid,fetched_at,success) VALUES(?1,?2,0)", params![appid, now()])?,
            };
            Ok(())
        })
    }

    pub fn put_reviews(&self, appid: u32, r: Option<&ReviewRow>) -> Result<()> {
        self.with(|c| {
            match r {
                Some(r) => c.execute(
                    "INSERT OR REPLACE INTO reviews(appid,fetched_at,success,review_score,review_score_desc,total_positive,total_negative,total_reviews)
                     VALUES(?1,?2,1,?3,?4,?5,?6,?7)",
                    params![appid, now(), r.review_score, r.review_score_desc, r.total_positive, r.total_negative, r.total_reviews],
                )?,
                None => c.execute("INSERT OR REPLACE INTO reviews(appid,fetched_at,success) VALUES(?1,?2,0)", params![appid, now()])?,
            };
            Ok(())
        })
    }

    /// `tags` is sorted by votes desc: `[(name, votes), ...]`.
    pub fn put_tags(&self, appid: u32, tags: Option<&[(String, i64)]>) -> Result<()> {
        self.with(|c| {
            match tags {
                Some(t) => c.execute(
                    "INSERT OR REPLACE INTO tags(appid,fetched_at,success,tags) VALUES(?1,?2,1,?3)",
                    params![appid, now(), to_json(&t)],
                )?,
                None => c.execute("INSERT OR REPLACE INTO tags(appid,fetched_at,success) VALUES(?1,?2,0)", params![appid, now()])?,
            };
            Ok(())
        })
    }

    pub fn put_review_texts(&self, appid: u32, items: Option<&[ReviewText]>) -> Result<()> {
        self.with(|c| {
            match items {
                Some(v) => c.execute(
                    "INSERT OR REPLACE INTO review_texts(appid,fetched_at,success,items) VALUES(?1,?2,1,?3)",
                    params![appid, now(), to_json(&v)],
                )?,
                None => {
                    c.execute("INSERT OR REPLACE INTO review_texts(appid,fetched_at,success) VALUES(?1,?2,0)", params![appid, now()])?
                }
            };
            Ok(())
        })
    }

    pub fn put_gog_meta(&self, gog_id: &str, m: Option<&crate::gogmeta::GogMeta>) -> Result<()> {
        self.with(|c| {
            match m {
                Some(m) => c.execute(
                    "INSERT OR REPLACE INTO gog_meta(gog_id,fetched_at,success,tags,properties,features,description,store_url) VALUES(?1,?2,1,?3,?4,?5,?6,?7)",
                    params![gog_id, now(), to_json(&m.tags), to_json(&m.properties), to_json(&m.features), m.description, m.store_url],
                )?,
                None => c.execute("INSERT OR REPLACE INTO gog_meta(gog_id,fetched_at,success) VALUES(?1,?2,0)", params![gog_id, now()])?,
            };
            Ok(())
        })
    }

    pub fn put_gog_reviews(&self, gog_id: &str, r: Option<&crate::gogmeta::GogReviews>) -> Result<()> {
        self.with(|c| {
            match r {
                Some(r) => {
                    let items: Vec<ReviewText> = r
                        .items
                        .iter()
                        .map(|(title, content, rating, votes, posted)| ReviewText {
                            source: "gog".into(),
                            positive: None,
                            rating: Some(*rating),
                            votes: *votes,
                            hours: None,
                            title: Some(title.clone()).filter(|t| !t.is_empty()),
                            text: content.clone(),
                            posted: *posted,
                        })
                        .collect();
                    c.execute(
                        "INSERT OR REPLACE INTO gog_reviews(gog_id,fetched_at,success,avg_rating,rating_count,items) VALUES(?1,?2,1,?3,?4,?5)",
                        params![gog_id, now(), r.avg_rating, r.rating_count, to_json(&items)],
                    )?
                }
                None => c.execute("INSERT OR REPLACE INTO gog_reviews(gog_id,fetched_at,success) VALUES(?1,?2,0)", params![gog_id, now()])?,
            };
            Ok(())
        })
    }

    pub fn put_igdb(&self, title_id: i64, g: Option<&crate::igdb::IgdbGame>) -> Result<()> {
        self.with(|c| {
            match g {
                Some(g) => c.execute(
                    "INSERT OR REPLACE INTO igdb_meta(title_id,fetched_at,success,igdb_id,name,similarity,genres,themes,keywords,perspectives,modes,summary,rating,rating_count)
                     VALUES(?1,?2,1,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                    params![title_id, now(), g.igdb_id, g.name, g.similarity, to_json(&g.genres), to_json(&g.themes), to_json(&g.keywords), to_json(&g.perspectives), to_json(&g.modes), g.summary, g.rating, g.rating_count],
                )?,
                None => c.execute("INSERT OR REPLACE INTO igdb_meta(title_id,fetched_at,success) VALUES(?1,?2,0)", params![title_id, now()])?,
            };
            Ok(())
        })
    }

    /// Whether Steam lists a Linux build for the app (None = no details yet).
    /// The title a store copy belongs to now (titles can merge mid-enrich).
    pub fn title_of_copy(&self, game_id: i64) -> Result<Option<i64>> {
        self.with(|c| Ok(c.query_row("SELECT title_id FROM games WHERE id=?1", params![game_id], |r| r.get(0)).optional()?))
    }

    pub fn linux_native(&self, appid: u32) -> Result<Option<bool>> {
        self.with(|c| {
            let v: Option<Option<String>> =
                c.query_row("SELECT platforms FROM details WHERE appid=?1 AND success=1", params![appid], |r| r.get(0)).optional()?;
            Ok(v.map(|p| json_vec(p).iter().any(|x| x == "linux")))
        })
    }

    pub fn put_steam_art(&self, appid: u32, portrait: Option<&str>, hero: Option<&str>, screenshots: &[String]) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO steam_art(appid,fetched_at,portrait,hero,screenshots) VALUES(?1,?2,?3,?4,?5)",
                params![appid, now(), portrait, hero, to_json(&screenshots.to_vec())],
            )?;
            Ok(())
        })
    }

    /// Steam appids (native or matched) with no art lookup yet.
    pub fn appids_without_art(&self, refresh: bool) -> Result<Vec<u32>> {
        self.with(|c| {
            let sql = if refresh {
                "SELECT DISTINCT appid FROM titles WHERE appid IS NOT NULL"
            } else {
                "SELECT DISTINCT appid FROM titles WHERE appid IS NOT NULL AND appid NOT IN (SELECT appid FROM steam_art WHERE screenshots IS NOT NULL)"
            };
            let mut st = c.prepare(sql)?;
            let rows = st.query_map([], |r| r.get(0))?.collect::<std::result::Result<Vec<u32>, _>>()?;
            Ok(rows)
        })
    }

    pub fn set_play_later(&self, p: PlayerId, title_id: i64, saved: bool) -> Result<()> {
        self.with(|c| {
            if saved {
                c.execute("INSERT OR IGNORE INTO play_later(player_id,title_id,added_at) VALUES(?1,?2,?3)", params![p, title_id, now()])?;
            } else {
                c.execute("DELETE FROM play_later WHERE player_id=?1 AND title_id=?2", params![p, title_id])?;
            }
            Ok(())
        })
    }

    pub fn put_trailer(
        &self,
        title_id: i64,
        youtube_id: Option<&str>,
        first_year: Option<i32>,
        screenshots: &[String],
        source: &str,
    ) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO trailers(title_id,fetched_at,youtube_id,source,first_year,screenshots) VALUES(?1,?2,?3,?4,?5,?6)",
                params![title_id, now(), youtube_id, source, first_year, to_json(&screenshots.to_vec())],
            )?;
            Ok(())
        })
    }

    /// (title_id, igdb_id) for IGDB-matched titles without a trailer lookup.
    pub fn igdb_ids_without_trailer(&self, refresh: bool) -> Result<Vec<(i64, i64)>> {
        self.with(|c| {
            let sql = if refresh {
                "SELECT title_id, igdb_id FROM igdb_meta WHERE success=1 AND igdb_id IS NOT NULL"
            } else {
                "SELECT title_id, igdb_id FROM igdb_meta WHERE success=1 AND igdb_id IS NOT NULL AND title_id NOT IN (SELECT title_id FROM trailers WHERE screenshots IS NOT NULL)"
            };
            let mut st = c.prepare(sql)?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// `series`: (English label, Wikidata series QID) pairs.
    pub fn put_title_series(&self, title_id: i64, series: &[(String, String)], first_year: Option<i32>) -> Result<()> {
        let labels: Vec<String> = series.iter().map(|(l, _)| l.clone()).collect();
        let qids: HashMap<String, String> = series.iter().map(|(l, q)| (crate::series::series_key(l), q.clone())).collect();
        self.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO title_series(title_id,fetched_at,series,first_year,series_qids) VALUES(?1,?2,?3,?4,?5)",
                params![title_id, now(), to_json(&labels), first_year, to_json(&qids)],
            )?;
            Ok(())
        })
    }

    /// Series found by name (for labels without a Wikidata QID): key -> QID.
    pub fn series_key_qids(&self) -> Result<HashMap<String, String>> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT series_key, qid FROM series_keys WHERE qid IS NOT NULL")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            Ok(rows.collect::<std::result::Result<HashMap<_, _>, _>>()?)
        })
    }

    /// Series keys already looked up by name (found or not).
    pub fn series_keys_tried(&self) -> Result<std::collections::HashSet<String>> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT series_key FROM series_keys")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            Ok(rows.collect::<std::result::Result<_, _>>()?)
        })
    }

    pub fn put_series_key(&self, key: &str, qid: Option<&str>) -> Result<()> {
        self.with(|c| {
            c.execute("INSERT OR REPLACE INTO series_keys(series_key,fetched_at,qid) VALUES(?1,?2,?3)", params![key, now(), qid])?;
            Ok(())
        })
    }

    /// Series QIDs owned games belong to whose full game list hasn't been fetched.
    pub fn series_without_catalog(&self, refresh: bool) -> Result<Vec<String>> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT series_qids FROM title_series WHERE series_qids IS NOT NULL")?;
            let mut all: std::collections::BTreeSet<String> = Default::default();
            for r in stmt.query_map([], |r| r.get::<_, String>(0))? {
                if let Ok(m) = serde_json::from_str::<HashMap<String, String>>(&r?) {
                    all.extend(m.into_values());
                }
            }
            let mut named = c.prepare("SELECT qid FROM series_keys WHERE qid IS NOT NULL")?;
            for q in named.query_map([], |r| r.get::<_, String>(0))? {
                all.insert(q?);
            }
            if !refresh {
                let mut have = c.prepare("SELECT series_qid FROM series_catalog")?;
                for q in have.query_map([], |r| r.get::<_, String>(0))? {
                    all.remove(&q?);
                }
            }
            Ok(all.into_iter().collect())
        })
    }

    pub fn put_series_catalog(&self, series_qid: &str, entries: &[crate::wikidata::SeriesEntry]) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO series_catalog(series_qid,fetched_at,entries) VALUES(?1,?2,?3)",
                params![series_qid, now(), serde_json::to_string(entries)?],
            )?;
            Ok(())
        })
    }

    /// Everything Collections needs from Wikidata beyond the titles themselves.
    pub fn catalog(&self) -> Result<crate::series::Catalog> {
        Ok(crate::series::Catalog { by_series: self.series_catalog()?, key_qids: self.series_key_qids()? })
    }

    /// Every game in each series (owned or not), by series QID.
    pub fn series_catalog(&self) -> Result<HashMap<String, Vec<crate::wikidata::SeriesEntry>>> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT series_qid, entries FROM series_catalog")?;
            let mut out = HashMap::new();
            for r in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
                let (q, e) = r?;
                out.insert(q, serde_json::from_str(&e).unwrap_or_default());
            }
            Ok(out)
        })
    }

    pub fn put_series_guide(&self, key: &str, g: &crate::series::Guide) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO series_guides(series_key,generated_at,model,members_hash,start_id,order_json,side_json,note)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    key,
                    g.generated_at,
                    g.model,
                    g.members_hash,
                    g.start_id,
                    serde_json::to_string(&g.order)?,
                    serde_json::to_string(&g.side)?,
                    g.note
                ],
            )?;
            Ok(())
        })
    }

    pub fn series_guides(&self) -> Result<HashMap<String, crate::series::Guide>> {
        self.with(|c| {
            let mut st =
                c.prepare("SELECT series_key,generated_at,model,members_hash,start_id,order_json,side_json,note FROM series_guides")?;
            let rows = st.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    crate::series::Guide {
                        generated_at: r.get(1)?,
                        model: r.get(2)?,
                        members_hash: r.get(3)?,
                        start_id: r.get::<_, Option<i64>>(4)?.unwrap_or(0),
                        order: r.get::<_, Option<String>>(5)?.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
                        side: r.get::<_, Option<String>>(6)?.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
                        note: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
                    },
                ))
            })?;
            let mut out = HashMap::new();
            for r in rows {
                let (k, g) = r?;
                out.insert(k, g);
            }
            Ok(out)
        })
    }

    pub fn put_xref(&self, title_id: i64, x: Option<&crate::wikidata::Xref>) -> Result<()> {
        self.with(|c| {
            match x {
                Some(x) => c.execute(
                    "INSERT OR REPLACE INTO xref(title_id,fetched_at,success,qid,pcgw_page,rawg_slug,hltb_id,igdb_slug,epic_slug)
                     VALUES(?1,?2,1,?3,?4,?5,?6,?7,?8)",
                    params![title_id, now(), x.qid, x.pcgw_page, x.rawg_slug, x.hltb_id, x.igdb_slug, x.epic_slug],
                )?,
                None => c.execute("INSERT OR REPLACE INTO xref(title_id,fetched_at,success) VALUES(?1,?2,0)", params![title_id, now()])?,
            };
            Ok(())
        })
    }

    pub fn put_rawg(&self, title_id: i64, g: Option<&crate::rawg::RawgGame>) -> Result<()> {
        self.with(|c| {
            match g {
                Some(g) => c.execute(
                    "INSERT OR REPLACE INTO rawg_meta(title_id,fetched_at,success,rawg_id,slug,name,similarity,genres,tags,rating,ratings_count,metacritic,developers)
                     VALUES(?1,?2,1,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                    params![title_id, now(), g.rawg_id, g.slug, g.name, g.similarity, to_json(&g.genres), to_json(&g.tags), g.rating, g.ratings_count, g.metacritic, to_json(&g.developers)],
                )?,
                None => c.execute("INSERT OR REPLACE INTO rawg_meta(title_id,fetched_at,success) VALUES(?1,?2,0)", params![title_id, now()])?,
            };
            Ok(())
        })
    }

    pub fn put_pcgw(&self, title_id: i64, m: Option<&crate::pcgw::PcgwMeta>) -> Result<()> {
        self.with(|c| {
            match m {
                Some(m) => c.execute(
                    "INSERT OR REPLACE INTO pcgw_meta(title_id,fetched_at,success,page,genres,themes,perspectives,pacing,modes,controls,art_styles,series)
                     VALUES(?1,?2,1,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                    params![title_id, now(), m.page, to_json(&m.genres), to_json(&m.themes), to_json(&m.perspectives), to_json(&m.pacing), to_json(&m.modes), to_json(&m.controls), to_json(&m.art_styles), to_json(&m.series)],
                )?,
                None => c.execute("INSERT OR REPLACE INTO pcgw_meta(title_id,fetched_at,success) VALUES(?1,?2,0)", params![title_id, now()])?,
            };
            Ok(())
        })
    }

    pub fn put_protondb(&self, appid: u32, p: Option<&crate::protondb::ProtonSummary>) -> Result<()> {
        self.with(|c| {
            match p {
                Some(p) => c.execute(
                    "INSERT OR REPLACE INTO protondb(appid,fetched_at,success,tier,trending_tier,confidence,score,total) VALUES(?1,?2,1,?3,?4,?5,?6,?7)",
                    params![appid, now(), p.tier, p.trending_tier, p.confidence, p.score, p.total],
                )?,
                None => c.execute("INSERT OR REPLACE INTO protondb(appid,fetched_at,success) VALUES(?1,?2,0)", params![appid, now()])?,
            };
            Ok(())
        })
    }

    /// Store a title's review summary, vibes and tags, replacing the old ones
    /// in the same write, so a title never goes without a summary. Empty
    /// vibes are stored as none.
    pub fn put_digest(&self, title_id: i64, digest: &str, vibes: &str, tags: &ReviewTags, model: &str, version: i64) -> Result<()> {
        let vibes = Some(vibes.trim()).filter(|v| !v.is_empty());
        self.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO digests(title_id,generated_at,model,digest,vibes,tags,prompt_version) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![title_id, now(), model, digest, vibes, to_json(tags), version],
            )?;
            Ok(())
        })
    }

    /// Take out the summaries whose digest or vibes `bad` says hold nothing,
    /// so the summaries job writes them again; how many went.
    pub fn remove_digests_where(&self, bad: impl Fn(&str) -> bool) -> Result<usize> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT title_id, digest, vibes FROM digests")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?)))?;
            let mut gone = vec![];
            for row in rows {
                let (id, digest, vibes) = row?;
                if bad(&digest) || vibes.as_deref().is_some_and(&bad) {
                    gone.push(id);
                }
            }
            for id in &gone {
                c.execute("DELETE FROM digests WHERE title_id=?1", params![id])?;
            }
            Ok(gone.len())
        })
    }

    pub fn put_hltb(&self, title_id: i64, h: Option<&HltbRow>) -> Result<()> {
        self.with(|c| {
            match h {
                Some(h) => c.execute(
                    "INSERT OR REPLACE INTO hltb(title_id,fetched_at,success,hltb_id,hltb_name,similarity,main_story,main_extra,completionist)
                     VALUES(?1,?2,1,?3,?4,?5,?6,?7,?8)",
                    params![title_id, now(), h.hltb_id, h.hltb_name, h.similarity, h.main_story, h.main_extra, h.completionist],
                )?,
                None => c.execute("INSERT OR REPLACE INTO hltb(title_id,fetched_at,success) VALUES(?1,?2,0)", params![title_id, now()])?,
            };
            Ok(())
        })
    }

    // -- overrides ------------------------------------------------------------

    /// `note = None` keeps an existing note when updating the status.
    pub fn set_override(&self, p: PlayerId, title_id: i64, status: Option<Override>, note: Option<&str>) -> Result<()> {
        self.with(|c| {
            let exists: bool = c.query_row("SELECT COUNT(*) FROM titles WHERE id=?1", params![title_id], |r| r.get::<_, i64>(0))? > 0;
            if !exists {
                anyhow::bail!("game id {title_id} is not in your library");
            }
            match status {
                Some(s) => {
                    c.execute(
                        "INSERT INTO overrides(player_id,title_id,status,note,updated_at) VALUES(?1,?2,?3,?4,?5)
                         ON CONFLICT(player_id,title_id) DO UPDATE SET status=excluded.status,
                            note=COALESCE(excluded.note, overrides.note), updated_at=excluded.updated_at",
                        params![p, title_id, s.as_str(), note, now()],
                    )?;
                }
                None => {
                    c.execute("DELETE FROM overrides WHERE player_id=?1 AND title_id=?2", params![p, title_id])?;
                }
            }
            Ok(())
        })
    }

    /// Record why a game was put down. `codes` are checked ones
    /// (`lifecycle::check_reasons`); `source` is where they were given.
    pub fn add_stop_reasons(&self, p: PlayerId, title_id: i64, codes: &[String], note: Option<&str>, source: &str) -> Result<()> {
        debug_assert!(crate::lifecycle::REASON_SOURCES.contains(&source));
        self.with(|c| {
            c.execute(
                "INSERT INTO stop_reasons(player_id, title_id, reasons, note, source, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![p, title_id, serde_json::to_string(codes)?, note, source, now()],
            )?;
            Ok(())
        })
    }

    /// Every time player `p` said a game was put down, newest first, back to the
    /// latest time every reason was cleared (the Second Chance page's "none
    /// of these"), which ends what still stands. A backlog verdict's draft
    /// reads its game reasons from these.
    pub fn stop_reason_history(&self, p: PlayerId, title_id: i64) -> Result<Vec<crate::lifecycle::StopReasons>> {
        self.with(|c| {
            let mut st =
                c.prepare("SELECT reasons, note, source, at FROM stop_reasons WHERE player_id=?1 AND title_id=?2 ORDER BY id DESC")?;
            let rows = st.query_map(params![p, title_id], |r| {
                Ok(crate::lifecycle::StopReasons { codes: json_vec(r.get(0)?), note: r.get(1)?, source: r.get(2)?, at: r.get(3)? })
            })?;
            let mut out = vec![];
            for r in rows {
                let r = r?;
                if r.codes.is_empty() && r.note.is_none() {
                    break;
                }
                out.push(r);
            }
            Ok(out)
        })
    }

    /// The "This game has no ending" switch: Some(true) no ending, Some(false)
    /// has one, None to infer it again.
    pub fn set_no_ending(&self, title_id: i64, no_ending: Option<bool>) -> Result<()> {
        self.with(|c| {
            c.execute("UPDATE titles SET no_ending=?2 WHERE id=?1", params![title_id, no_ending])?;
            Ok(())
        })
    }
}
