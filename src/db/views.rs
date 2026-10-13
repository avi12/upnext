//! Reading titles back as `GameView`s, and the library cache.

use super::filter::{apply_filter, compute_stats};
use super::model::{
    ChosenPick, Copy, FeedbackAction, GameFilter, GameView, MissReason, Override, PickVerdict, RatedPick, ReviewText, Screenshot,
    SecondChanceNote, Stats, Status, TooNote, is_native_launcher,
};
use super::players::Sight;
use super::{CacheKey, Db, PlayerId, json_vec};
use crate::lifecycle::{Facts, Kind, StatusRules, StopReasons, classify, infer_kind};
use crate::review_tags::ReviewTags;
use crate::second_chance::{Answer, Verdict};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

impl Db {
    // -- reads ----------------------------------------------------------------

    const GAME_SELECT: &'static str = r#"
        SELECT t.id, t.name, t.appid,
               g.id, g.source, g.source_id, g.launcher, g.installed, g.playtime_forever, g.playtime_known, g.playtime_2weeks, g.rtime_last_played,
               g.art_cover, g.art_square, g.l_description, g.l_genres, g.l_developer, g.l_release, g.store_url, g.run_url,
               g.account_id,
               d.success, d.type, d.short_description, d.genres, d.categories, d.header_image, d.metacritic, d.release_date, d.developers,
               r.success, r.review_score, r.review_score_desc, r.total_positive, r.total_reviews,
               tg.success, tg.tags,
               h.success, h.hltb_id, h.main_story, h.main_extra, h.completionist,
               m.success,
               g.name, g.edition,
               gm.success, gm.tags, gm.properties, gm.features, gm.description,
               gr.success, gr.avg_rating, gr.rating_count, gr.items,
               ig.success, ig.genres, ig.themes, ig.keywords, ig.perspectives, ig.modes, ig.summary, ig.rating, ig.rating_count,
               rt.success, rt.items,
               dg.digest, dg.tags, dg.prompt_version, dg.vibes,
               t.no_ending, dg.seed_date, d.controller_support
        FROM games g
        JOIN titles t ON t.id = g.title_id
        LEFT JOIN details     d  ON d.appid    = t.appid
        LEFT JOIN reviews     r  ON r.appid    = t.appid
        LEFT JOIN tags        tg ON tg.appid   = t.appid
        LEFT JOIN hltb        h  ON h.title_id = t.id
        LEFT JOIN steam_match m  ON m.title_id = t.id
        LEFT JOIN gog_meta    gm ON g.source = 'gog' AND gm.gog_id = g.source_id
        LEFT JOIN gog_reviews gr ON g.source = 'gog' AND gr.gog_id = g.source_id
        LEFT JOIN igdb_meta   ig ON ig.title_id = t.id
        LEFT JOIN review_texts rt ON rt.appid = t.appid
        LEFT JOIN digests     dg ON dg.title_id = t.id
        WHERE g.removed = 0"#;

    /// One raw row = one copy with its title's metadata.
    fn row_to_parts(r: &Row) -> rusqlite::Result<(i64, Copy, CopyExtras, TitleMeta)> {
        let title_id: i64 = r.get(0)?;
        let copy = Copy {
            game_id: r.get(3)?,
            source: r.get(4)?,
            source_id: r.get(5)?,
            launcher: r.get(6)?,
            installed: r.get::<_, i64>(7)? != 0,
            playtime_minutes: r.get(8)?,
            playtime_known: r.get::<_, i64>(9)? != 0,
            last_played: r.get(11)?,
            run_url: r.get::<_, Option<String>>(19)?.unwrap_or_default(),
            store_url: r.get::<_, Option<String>>(18)?.unwrap_or_default(),
            name: r.get(43)?,
            edition: r.get(44)?,
            has_gog_meta: r.get::<_, Option<i64>>(45)?.is_some(),
            has_gog_reviews: r.get::<_, Option<i64>>(50)?.is_some(),
            account_id: r.get(20)?,
            playtime_2weeks: r.get(10)?,
        };
        let extras = CopyExtras {
            gog_tags: json_vec(r.get(46)?),
            gog_properties: json_vec(r.get(47)?),
            gog_features: json_vec(r.get(48)?),
            gog_description: r.get(49)?,
            gog_rating: r.get(51)?,
            gog_rating_count: r.get(52)?,
            gog_reviews: r.get::<_, Option<String>>(53)?.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
            art_cover: r.get(12)?,
            art_square: r.get(13)?,
            description: r.get(14)?,
            genres: json_vec(r.get(15)?),
            developer: r.get(16)?,
            release: r.get(17)?,
        };
        let meta = TitleMeta {
            name: r.get(1)?,
            appid: r.get(2)?,
            details_success: r.get(21)?,
            type_: r.get(22)?,
            short_description: r.get(23)?,
            d_genres: json_vec(r.get(24)?),
            categories: json_vec(r.get(25)?),
            d_header: r.get(26)?,
            metacritic: r.get(27)?,
            d_release: r.get(28)?,
            d_developers: json_vec(r.get(29)?),
            reviews_success: r.get(30)?,
            review_score: r.get(31)?,
            review_desc: r.get(32)?,
            total_positive: r.get(33)?,
            total_reviews: r.get(34)?,
            tags_success: r.get(35)?,
            tags_raw: r.get(36)?,
            hltb_success: r.get(37)?,
            hltb_id: r.get(38)?,
            hltb_main: r.get(39)?,
            hltb_extra: r.get(40)?,
            hltb_complete: r.get(41)?,
            match_success: r.get(42)?,
            igdb_success: r.get(54)?,
            igdb_genres: json_vec(r.get(55)?),
            igdb_themes: json_vec(r.get(56)?),
            igdb_keywords: json_vec(r.get(57)?),
            igdb_perspectives: json_vec(r.get(58)?),
            igdb_modes: json_vec(r.get(59)?),
            igdb_summary: r.get(60)?,
            igdb_rating: r.get(61)?,
            igdb_rating_count: r.get(62)?,
            rt_success: r.get(63)?,
            rt_items: r.get::<_, Option<String>>(64)?.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
            digest: r.get(65)?,
            review_tags: r.get::<_, Option<String>>(66)?.and_then(|s| serde_json::from_str(&s).ok()),
            digest_version: r.get(67)?,
            vibes: r.get(68)?,
            no_ending: r.get::<_, Option<i64>>(69)?.map(|v| v != 0),
            seed_date: r.get(70)?,
            controller_support: r.get(71)?,
        };
        Ok((title_id, copy, extras, meta))
    }

    /// A title as the household has it: every copy, and everything known
    /// about the game. What depends on who is asking (which copies they see,
    /// their hours, statuses, lists and ratings) is laid over it by
    /// `for_player`.
    fn build_base(title_id: i64, mut copies: Vec<(Copy, CopyExtras)>, meta: TitleMeta) -> GameView {
        copies.sort_by_key(|(c, _)| (!c.installed, source_rank(&c.source), !is_native_launcher(&c.launcher), c.game_id));
        let first = |f: &dyn Fn(&CopyExtras) -> Option<String>| copies.iter().find_map(|(_, e)| f(e));
        let l_description = first(&|e| e.description.clone().filter(|s| !s.is_empty()));
        let l_genres = copies.iter().find(|(_, e)| !e.genres.is_empty()).map(|(_, e)| e.genres.clone()).unwrap_or_default();
        let l_developer = first(&|e| e.developer.clone());
        let l_release = first(&|e| e.release.clone());
        let art_cover = first(&|e| e.art_cover.clone());
        let art_square = first(&|e| e.art_square.clone());

        let steam_tags: Vec<String> = meta
            .tags_raw
            .as_deref()
            .and_then(|s| serde_json::from_str::<Vec<(String, i64)>>(s).ok())
            .map(|v| v.into_iter().take(15).map(|(n, _)| n).collect())
            .unwrap_or_default();
        let gog_tags: Vec<String> = {
            let mut v: Vec<String> = vec![];
            for (_, e) in &copies {
                for t in e.gog_properties.iter().chain(e.gog_tags.iter()) {
                    if !v.iter().any(|x| x.eq_ignore_ascii_case(t)) {
                        v.push(t.clone());
                    }
                }
            }
            v
        };
        // Rows stored before the import filtered catalogue keywords still have them.
        let igdb_tags: Vec<String> = meta
            .igdb_keywords
            .iter()
            .filter(|k| !crate::igdb::is_catalogue_keyword(k))
            .chain(meta.igdb_themes.iter())
            .chain(meta.igdb_genres.iter())
            .cloned()
            .collect();
        let mut tags_by_source: HashMap<String, Vec<String>> = HashMap::new();
        if !steam_tags.is_empty() {
            tags_by_source.insert("steam".into(), steam_tags);
        }
        if let Some(labels) = meta.review_tags.as_ref().map(ReviewTags::labels).filter(|l| !l.is_empty()) {
            tags_by_source.insert("reviews".into(), labels);
        }
        if !gog_tags.is_empty() {
            tags_by_source.insert("gog".into(), gog_tags);
        }
        if !igdb_tags.is_empty() {
            tags_by_source.insert("igdb".into(), igdb_tags);
        }
        let tags = merge_tags(&tags_by_source);
        let mut categories = meta.categories.clone();
        for (_, e) in &copies {
            for f in &e.gog_features {
                if !categories.iter().any(|x| x.eq_ignore_ascii_case(f)) {
                    categories.push(f.clone());
                }
            }
        }
        let kind = match meta.no_ending {
            Some(true) => Kind::Endless,
            Some(false) => Kind::Ending,
            None => infer_kind(&tags, &categories, meta.hltb_main, meta.review_tags.as_ref()),
        };
        let mut themes: Vec<String> = vec![];
        for t in meta.igdb_themes.iter().chain(meta.igdb_perspectives.iter()).chain(meta.igdb_modes.iter()) {
            if !themes.iter().any(|x| x.eq_ignore_ascii_case(t)) {
                themes.push(t.clone());
            }
        }
        let gog_rating = copies.iter().find_map(|(_, e)| e.gog_rating);
        let gog_rating_count = copies.iter().find_map(|(_, e)| e.gog_rating_count);
        let mut review_texts: Vec<ReviewText> = meta.rt_items.clone();
        for (_, e) in &copies {
            review_texts.extend(e.gog_reviews.iter().cloned());
        }
        let gog_description = copies.iter().find_map(|(_, e)| e.gog_description.clone());
        let review_pct = match (meta.total_positive, meta.total_reviews) {
            (Some(p), Some(t)) if t > 0 => Some((p * 100 + t / 2) / t),
            _ => None,
        };
        let enriched = meta.details_success == Some(1);
        let native_steam = copies.iter().any(|(c, _)| c.source == "steam");
        let header_image = if native_steam {
            meta.d_header.clone().or_else(|| meta.appid.map(header_image_url)).unwrap_or_default()
        } else {
            art_cover.or(meta.d_header.clone()).or_else(|| meta.appid.map(header_image_url)).unwrap_or_default()
        };
        let genres = if enriched && !meta.d_genres.is_empty() {
            meta.d_genres.clone()
        } else if !l_genres.is_empty() {
            l_genres
        } else {
            meta.igdb_genres.clone()
        };
        let developers = if !meta.d_developers.is_empty() { meta.d_developers.clone() } else { l_developer.into_iter().collect() };
        // The copies' totals and the primary copy's fields are filled in by
        // `sum_copies`, here and again for each player's own copies; the
        // player's standing by `for_player`.
        let mut g = GameView {
            id: title_id,
            name: meta.name,
            appid: meta.appid,
            steam_matched: false,
            owned_on: vec![],
            primary_source: String::new(),
            editions: vec![],
            source: String::new(),
            source_id: String::new(),
            launcher: String::new(),
            store_url: String::new(),
            run_url: String::new(),
            copies: copies.into_iter().map(|(c, _)| c).collect(),
            installed: false,
            playtime_minutes: 0,
            playtime_known: false,
            playtime_2weeks: 0,
            last_played: 0,
            status: Status::Untouched,
            override_: None,
            override_note: None,
            kind,
            no_ending: meta.no_ending,
            staple: false,
            shelved_at: None,
            abandoned_at: None,
            burnt_out_until: None,
            stop_reasons: None,
            rejections: 0,
            abandoned_by_picks: false,
            too: None,
            chosen: None,
            my_rating: None,
            rated_at: None,
            pick_rating: None,
            second_try: false,
            second_chance: None,
            from_wishlist: None,
            hidden_scope: None,
            own_copy: false,
            type_: meta.type_,
            short_description: meta
                .short_description
                .filter(|s| !s.is_empty())
                .or(l_description)
                .or(gog_description)
                .or(meta.igdb_summary.clone()),
            genres,
            categories,
            tags,
            tags_by_source,
            themes,
            header_image,
            art_square,
            metacritic: meta.metacritic,
            release_date: meta.d_release.or(l_release),
            developers,
            review_score: meta.review_score,
            review_desc: meta.review_desc,
            review_positive: meta.total_positive,
            review_total: meta.total_reviews,
            review_pct,
            hltb_main: meta.hltb_main,
            hltb_extra: meta.hltb_extra,
            hltb_complete: meta.hltb_complete,
            hltb_url: meta.hltb_id.map(|id| format!("https://howlongtobeat.com/game/{id}")),
            gog_rating,
            gog_rating_count,
            igdb_rating: meta.igdb_rating,
            igdb_rating_count: meta.igdb_rating_count,
            review_texts,
            digest: meta.digest.clone(),
            vibes: meta.vibes.clone(),
            review_tags: meta.review_tags.clone(),
            digest_version: meta.digest_version,
            seed_date: meta.seed_date.clone(),
            backlog: None,
            enriched,
            has_details: meta.details_success.is_some(),
            has_reviews: meta.reviews_success.is_some(),
            has_tags: meta.tags_success.is_some(),
            has_hltb: meta.hltb_success.is_some(),
            has_match_attempt: meta.match_success.is_some(),
            has_review_texts: meta.rt_success.is_some(),
            has_igdb: meta.igdb_success.is_some(),
            has_digest: meta.digest.is_some(),
            rawg_rating: None,
            rawg_ratings_count: None,
            rawg_url: None,
            pcgw_url: None,
            wikidata_url: None,
            series: vec![],
            series_qids: HashMap::new(),
            first_year: None,
            has_title_series: false,
            saved_at: None,
            override_at: None,
            trailer_url: None,
            portrait_url: None,
            hero_url: None,
            controller_support: meta.controller_support.clone().filter(|s| !s.is_empty()),
            screenshots: vec![],
            has_trailer_lookup: false,
            linux_native: None,
            protondb_tier: None,
            protondb_trending: None,
            protondb_total: None,
            linux: None,
            has_xref: false,
            xref: None,
            has_rawg: false,
            has_pcgw: false,
            has_protondb: false,
        };
        Self::sum_copies(&mut g);
        g
    }

    /// Fill in what a title's copies add up to: where it's owned, its
    /// editions, the primary copy (the first, as `build_base` sorted them),
    /// whether any is installed, and its hours and last play.
    fn sum_copies(g: &mut GameView) {
        let copies = &g.copies;
        let mut owned_on: Vec<String> = copies.iter().map(|c| c.source.clone()).collect();
        let mut seen = std::collections::HashSet::new();
        owned_on.retain(|s| seen.insert(s.clone()));
        let mut editions: Vec<String> = copies.iter().filter_map(|c| c.edition.clone()).collect();
        editions.sort();
        editions.dedup();
        let primary = &copies[0];
        g.steam_matched = g.appid.is_some() && !copies.iter().any(|c| c.source == "steam");
        g.primary_source = primary.source.clone();
        g.source = primary.source.clone();
        g.source_id = primary.source_id.clone();
        g.launcher = primary.launcher.clone();
        g.store_url = if primary.store_url.is_empty() {
            g.appid.map(|a| format!("https://store.steampowered.com/app/{a}")).unwrap_or_default()
        } else {
            primary.store_url.clone()
        };
        g.run_url = primary.run_url.clone();
        g.installed = copies.iter().any(|c| c.installed);
        g.playtime_known = copies.iter().any(|c| c.playtime_known);
        g.playtime_minutes = copies.iter().filter(|c| c.playtime_known).map(|c| c.playtime_minutes).max().unwrap_or(0);
        g.playtime_2weeks = copies.iter().map(|c| c.playtime_2weeks).max().unwrap_or(0);
        g.last_played = copies.iter().map(|c| c.last_played).max().unwrap_or(0);
        g.owned_on = owned_on;
        g.editions = editions;
    }

    /// A title as player `p` has it, or None when they don't see it: only
    /// the copies they see, hours only from the accounts they play on (the
    /// others count as never played), and their own status, lists, ratings,
    /// reasons and Second Chance.
    fn for_player(base: &GameView, sight: &Sight, own: &Own, rules: &StatusRules, now: i64) -> Option<GameView> {
        if sight.hid(base.id) {
            return None;
        }
        let copies: Vec<Copy> = base
            .copies
            .iter()
            .filter(|c| sight.sees(c.account_id, base.id))
            .map(|c| {
                let mut c = c.clone();
                if !sight.plays(c.account_id) {
                    (c.playtime_minutes, c.playtime_known, c.last_played, c.playtime_2weeks) = (0, true, 0, 0);
                }
                c
            })
            .collect();
        if copies.is_empty() {
            return None;
        }
        let mut g = base.clone();
        let same_primary = copies[0].game_id == base.copies[0].game_id;
        g.copies = copies;
        let store_url = g.store_url.clone();
        Self::sum_copies(&mut g);
        // attach_ext found the GOG store page for the household's primary copy.
        if same_primary {
            g.store_url = store_url;
        }
        let signals = own.signals.get(&g.id).unwrap_or(&NO_SIGNALS);
        let mark = own.marks.get(&g.id);
        let facts = Facts {
            not_game: matches!(g.type_.as_deref(), Some(t) if t != "game"),
            playtime_known: g.playtime_known,
            playtime_minutes: g.playtime_minutes,
            last_played: g.last_played,
            hltb_main: g.hltb_main,
            kind: g.kind,
            mark: mark.map(|m| (m.status, m.at)),
            rejections: &signals.rejections,
            retry_at: signals.retry_at,
            chosen_at: signals.chosen.as_ref().map(|c| c.at),
        };
        let standing = classify(&facts, rules, now);
        g.status = standing.status;
        g.override_ = standing.mark;
        g.override_note = mark.and_then(|m| m.note.clone());
        g.override_at = standing.mark.and(mark.map(|m| m.at));
        g.staple = standing.staple;
        g.shelved_at = standing.shelved_at;
        g.abandoned_at = standing.abandoned_at;
        g.burnt_out_until = standing.burnt_out_until;
        g.rejections = standing.rejections;
        g.abandoned_by_picks = standing.abandoned_by_picks;
        g.second_try = standing.second_try;
        g.too = signals.too.clone();
        g.chosen = signals.chosen.clone();
        g.pick_rating = signals.rated.clone();
        g.saved_at = own.saved.get(&g.id).copied();
        g.hidden_scope = sight.hid_as(g.id);
        g.own_copy = g.copies.iter().any(|c| sight.owns(c.account_id));
        (g.my_rating, g.rated_at) = own.ratings.get(&g.id).map(|(s, at)| (Some(*s), Some(*at))).unwrap_or_default();
        g.stop_reasons = own.stop_reasons.get(&g.id).cloned();
        g.second_chance = own.second_chances.get(&g.id).cloned().map(|mut n| {
            n.outdated = n.now.is_some() && n.inputs.as_deref() != Some(crate::second_chance::inputs(&g).as_str());
            n
        });
        Some(g)
    }

    /// Overlay the per-title sources kept outside GAME_SELECT (Wikidata,
    /// RAWG, PCGamingWiki, ProtonDB, Steam platforms).
    fn attach_ext(c: &Connection, views: &mut [GameView]) -> Result<()> {
        if views.is_empty() {
            return Ok(());
        }
        let one = if views.len() == 1 { Some(views[0].id) } else { None };
        let filter = |col: &str| one.map(|id| format!(" WHERE {col} = {id}")).unwrap_or_default();

        let mut xref: HashMap<i64, (i64, crate::wikidata::Xref)> = HashMap::new();
        let mut st = c.prepare(&format!(
            "SELECT title_id, success, qid, pcgw_page, rawg_slug, hltb_id, igdb_slug, epic_slug FROM xref{}",
            filter("title_id")
        ))?;
        for r in st.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                crate::wikidata::Xref {
                    qid: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    pcgw_page: r.get(3)?,
                    rawg_slug: r.get(4)?,
                    hltb_id: r.get(5)?,
                    igdb_slug: r.get(6)?,
                    epic_slug: r.get(7)?,
                },
            ))
        })? {
            let (id, ok, x) = r?;
            xref.insert(id, (ok, x));
        }

        struct Rawg {
            ok: i64,
            slug: Option<String>,
            genres: Vec<String>,
            tags: Vec<String>,
            rating: Option<f64>,
            count: Option<i64>,
            metacritic: Option<i64>,
            developers: Vec<String>,
        }
        let mut rawg: HashMap<i64, Rawg> = HashMap::new();
        let mut st = c.prepare(&format!(
            "SELECT title_id, success, slug, genres, tags, rating, ratings_count, metacritic, developers FROM rawg_meta{}",
            filter("title_id")
        ))?;
        for r in st.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                Rawg {
                    ok: r.get(1)?,
                    slug: r.get(2)?,
                    genres: json_vec(r.get(3)?),
                    tags: json_vec(r.get(4)?),
                    rating: r.get(5)?,
                    count: r.get(6)?,
                    metacritic: r.get(7)?,
                    developers: json_vec(r.get(8)?),
                },
            ))
        })? {
            let (id, v) = r?;
            rawg.insert(id, v);
        }

        struct Pcgw {
            ok: i64,
            page: Option<String>,
            genres: Vec<String>,
            themes: Vec<String>,
            perspectives: Vec<String>,
            pacing: Vec<String>,
            modes: Vec<String>,
            art: Vec<String>,
            series: Vec<String>,
        }
        let mut pcgw: HashMap<i64, Pcgw> = HashMap::new();
        let mut st = c.prepare(&format!(
            "SELECT title_id, success, page, genres, themes, perspectives, pacing, modes, art_styles, series FROM pcgw_meta{}",
            filter("title_id")
        ))?;
        for r in st.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                Pcgw {
                    ok: r.get(1)?,
                    page: r.get(2)?,
                    genres: json_vec(r.get(3)?),
                    themes: json_vec(r.get(4)?),
                    perspectives: json_vec(r.get(5)?),
                    pacing: json_vec(r.get(6)?),
                    modes: json_vec(r.get(7)?),
                    art: json_vec(r.get(8)?),
                    series: json_vec(r.get(9)?),
                },
            ))
        })? {
            let (id, v) = r?;
            pcgw.insert(id, v);
        }

        type SeriesRow = (Vec<String>, Option<i32>, HashMap<String, String>);
        let mut tseries: HashMap<i64, SeriesRow> = HashMap::new();
        let mut st = c.prepare(&format!("SELECT title_id, series, first_year, series_qids FROM title_series{}", filter("title_id")))?;
        for r in st.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                json_vec(r.get(1)?),
                r.get::<_, Option<i32>>(2)?,
                r.get::<_, Option<String>>(3)?.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
            ))
        })? {
            let (id, labels, year, qids) = r?;
            tseries.insert(id, (labels, year, qids));
        }

        let mut trailers: HashMap<i64, (Option<String>, Option<i32>)> = HashMap::new();
        let mut igdb_shots: HashMap<i64, Vec<String>> = HashMap::new();
        let mut st = c.prepare(&format!("SELECT title_id, youtube_id, first_year, screenshots FROM trailers{}", filter("title_id")))?;
        for r in st.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<i32>>(2)?, json_vec(r.get(3)?)))
        })? {
            let (id, yt, year, shots) = r?;
            trailers.insert(id, (yt, year));
            igdb_shots.insert(id, shots);
        }

        // appid -> (portrait, hero, screenshots)
        let mut steam_art: HashMap<u32, SteamArt> = HashMap::new();
        let mut st = c.prepare("SELECT appid, portrait, hero, screenshots FROM steam_art")?;
        for r in st.query_map([], |r| {
            Ok((r.get::<_, u32>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?, json_vec(r.get(3)?)))
        })? {
            let (a, p, h, s) = r?;
            steam_art.insert(a, (p, h, s));
        }

        // GOG store pages (the launcher caches only give a search link).
        let mut gog_pages: HashMap<String, String> = HashMap::new();
        let mut st = c.prepare("SELECT gog_id, store_url FROM gog_meta WHERE store_url IS NOT NULL")?;
        for r in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (id, url) = r?;
            gog_pages.insert(id, url);
        }

        let appid_filter = one.and_then(|_| views[0].appid).map(|a| format!(" WHERE appid = {a}"));
        // appid -> (success, tier, trending_tier, total)
        let mut proton: HashMap<u32, ProtonRow> = HashMap::new();
        let mut platforms: HashMap<u32, Vec<String>> = HashMap::new();
        if one.is_none() || appid_filter.is_some() {
            let f = appid_filter.clone().unwrap_or_default();
            let mut st = c.prepare(&format!("SELECT appid, success, tier, trending_tier, total FROM protondb{f}"))?;
            for r in st.query_map([], |r| Ok((r.get::<_, u32>(0)?, (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))))? {
                let (a, v) = r?;
                proton.insert(a, v);
            }
            let f = appid_filter.map(|w| format!("{w} AND success = 1")).unwrap_or_else(|| " WHERE success = 1".into());
            let mut st = c.prepare(&format!("SELECT appid, platforms FROM details{f}"))?;
            for r in st.query_map([], |r| Ok((r.get::<_, u32>(0)?, json_vec(r.get(1)?))))? {
                let (a, v) = r?;
                platforms.insert(a, v);
            }
        }

        for g in views.iter_mut() {
            if let Some((ok, x)) = xref.remove(&g.id) {
                g.has_xref = true;
                if ok == 1 {
                    g.wikidata_url = Some(format!("https://www.wikidata.org/wiki/{}", x.qid));
                    g.xref = Some(x);
                }
            }
            if let Some(r) = rawg.remove(&g.id) {
                g.has_rawg = true;
                if r.ok == 1 {
                    g.rawg_rating = r.rating;
                    g.rawg_ratings_count = r.count;
                    g.rawg_url = r.slug.map(|s| format!("https://rawg.io/games/{s}"));
                    g.metacritic = g.metacritic.or(r.metacritic);
                    if g.genres.is_empty() {
                        g.genres = r.genres.clone();
                    }
                    if g.developers.is_empty() {
                        g.developers = r.developers;
                    }
                    let mut t: Vec<String> = r.tags;
                    for x in r.genres {
                        if !t.iter().any(|y| y.eq_ignore_ascii_case(&x)) {
                            t.push(x);
                        }
                    }
                    if !t.is_empty() {
                        g.tags_by_source.insert("rawg".into(), t);
                    }
                }
            }
            if let Some(p) = pcgw.remove(&g.id) {
                g.has_pcgw = true;
                if p.ok == 1 {
                    g.pcgw_url = p.page.map(|pg| {
                        format!("https://www.pcgamingwiki.com/wiki/{}", pg.replace('%', "%25").replace('?', "%3F").replace(' ', "_"))
                    });
                    if g.genres.is_empty() {
                        g.genres = p.genres.clone();
                    }
                    for t in p.themes.iter().chain(p.perspectives.iter()).chain(p.pacing.iter()).chain(p.modes.iter()).chain(p.art.iter()) {
                        if !g.themes.iter().any(|x| x.eq_ignore_ascii_case(t)) {
                            g.themes.push(t.clone());
                        }
                    }
                    g.series = p.series;
                    let t: Vec<String> = p.genres.into_iter().chain(p.themes).chain(p.art).collect();
                    if !t.is_empty() {
                        g.tags_by_source.insert("pcgw".into(), t);
                    }
                }
            }
            for cp in g.copies.iter_mut().filter(|c| c.source == "gog") {
                if let Some(url) = gog_pages.get(&cp.source_id)
                    && (cp.store_url.is_empty() || cp.store_url.contains("/games?query="))
                {
                    cp.store_url = url.clone();
                }
            }
            if g.source == "gog"
                && (g.store_url.is_empty() || g.store_url.contains("/games?query="))
                && let Some(url) = gog_pages.get(&g.source_id)
            {
                g.store_url = url.clone();
            }
            if let Some((p, h, shots)) = g.appid.and_then(|a| steam_art.get(&a)) {
                g.portrait_url = p.clone();
                g.hero_url = h.clone();
                g.screenshots = shots.iter().map(|f| Screenshot::steam(f)).collect();
            }
            if g.screenshots.is_empty()
                && let Some(ids) = igdb_shots.get(&g.id)
            {
                g.screenshots = ids.iter().map(|i| Screenshot::igdb(i)).collect();
            }
            let (yt, igdb_year) = trailers.remove(&g.id).inspect(|_| g.has_trailer_lookup = true).unwrap_or_default();
            g.trailer_url = Some(match yt {
                Some(id) => format!("https://www.youtube.com/watch?v={id}"),
                None => format!("https://www.youtube.com/results?search_query={}", url_query(&format!("{} trailer", g.name))),
            });
            if let Some((labels, year, qids)) = tseries.remove(&g.id) {
                g.has_title_series = true;
                g.first_year = year;
                g.series_qids = qids;
                for l in labels {
                    let key = crate::series::series_key(&l);
                    if !g.series.iter().any(|x| crate::series::series_key(x) == key) {
                        g.series.push(l);
                    }
                }
            }
            if let Some(appid) = g.appid {
                if let Some(pl) = platforms.get(&appid) {
                    g.linux_native = Some(pl.iter().any(|p| p == "linux"));
                }
                if let Some((ok, tier, trending, total)) = proton.get(&appid) {
                    g.has_protondb = true;
                    if *ok == 1 {
                        g.protondb_tier = tier.clone();
                        g.protondb_trending = trending.clone();
                        g.protondb_total = *total;
                    }
                }
            }
            // Wikidata's original year wins; IGDB's first release fills gaps.
            if g.first_year.is_none() {
                g.first_year = igdb_year;
            }
            g.linux = if g.linux_native == Some(true) { Some("native".into()) } else { g.protondb_tier.clone() };
            g.tags = merge_tags(&g.tags_by_source);
        }
        super::community::attach(c, views)
    }

    /// The household's views of the titles `sql` selects (`build_base`).
    fn query_base(c: &Connection, sql: &str, p: &[&dyn rusqlite::ToSql]) -> Result<Vec<GameView>> {
        let mut stmt = c.prepare(sql)?;
        let rows = stmt.query_map(p, Self::row_to_parts)?;
        let mut out: Vec<GameView> = vec![];
        let mut current: Option<TitleRows> = None;
        for r in rows {
            let (tid, copy, extras, meta) = r?;
            match &mut current {
                Some((id, copies, _)) if *id == tid => copies.push((copy, extras)),
                _ => {
                    if let Some((id, copies, meta)) = current.take() {
                        out.push(Self::build_base(id, copies, meta));
                    }
                    current = Some((tid, vec![(copy, extras)], meta));
                }
            }
        }
        if let Some((id, copies, meta)) = current.take() {
            out.push(Self::build_base(id, copies, meta));
        }
        tracing::info_span!(target: "upnext::timing", "attach_ext").in_scope(|| Self::attach_ext(c, &mut out))?;
        Ok(out)
    }

    /// What the caches are good for: rows changed on our connection, commits
    /// by other processes (the CLI), the settings statuses follow, and the day.
    fn cache_key(&self, c: &Connection) -> Result<CacheKey> {
        let data_version: i64 = c.query_row("PRAGMA data_version", [], |r| r.get(0))?;
        Ok((c.total_changes(), data_version, self.status_rules(), super::now() / 86_400))
    }

    /// The rules player `p`'s statuses follow: the household's (the
    /// environment's or the built-in ones, `Db::status_rules`), with the
    /// ones the player set in place of them.
    fn rules_for(&self, c: &Connection, p: PlayerId) -> Result<StatusRules> {
        let mut rules = self.status_rules();
        let mut st = c.prepare_cached(
            "SELECT key, value FROM player_settings WHERE player_id=?1
             AND key IN ('sampled_minutes', 'shelve_after_days', 'abandon_after_days', 'staple_hours', 'burnout_days')",
        )?;
        for row in st.query_map(params![p], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (key, value) = row?;
            let Ok(v) = value.trim().parse::<i64>() else { continue };
            match key.as_str() {
                "sampled_minutes" => rules.sampled_minutes = v,
                "shelve_after_days" => rules.shelve_after_days = v,
                "abandon_after_days" => rules.abandon_after_days = v,
                "staple_hours" => rules.staple_hours = v,
                _ => rules.burnout_days = v,
            }
        }
        Ok(rules)
    }

    /// Every title as the household has it, with every copy, sorted by name.
    /// Built once per database change and shared by every player's library.
    fn household(&self, c: &Connection, key: CacheKey) -> Result<Arc<Vec<GameView>>> {
        if let Some((k, v)) = self.cache.lock().map_err(|_| anyhow::anyhow!("cache mutex poisoned"))?.as_ref()
            && *k == key
        {
            return Ok(v.clone());
        }
        let build = tracing::info_span!(target: "upnext::timing", "library_build");
        let v = Arc::new(
            build.in_scope(|| Self::query_base(c, &format!("{} ORDER BY t.name COLLATE NOCASE, t.id, g.id", Self::GAME_SELECT), &[]))?,
        );
        *self.cache.lock().map_err(|_| anyhow::anyhow!("cache mutex poisoned"))? = Some((key, v.clone()));
        Ok(v)
    }

    /// Every title player `p` sees, sorted by name, as they have it
    /// (`for_player`). Built once per database change and reused, so list
    /// pages don't rebuild the library on each request.
    pub fn all_games_shared(&self, p: PlayerId) -> Result<Arc<Vec<GameView>>> {
        self.with(|c| {
            let key = self.cache_key(c)?;
            if let Some((k, v)) = self.player_cache.lock().map_err(|_| anyhow::anyhow!("cache mutex poisoned"))?.get(&p)
                && *k == key
            {
                return Ok(v.clone());
            }
            let base = self.household(c, key)?;
            let (sight, own) = (Sight::of(c, p)?, Own::load(c, p, None)?);
            let (rules, now) = (self.rules_for(c, p)?, super::now());
            let v: Arc<Vec<GameView>> = Arc::new(base.iter().filter_map(|g| Self::for_player(g, &sight, &own, &rules, now)).collect());
            self.player_cache.lock().map_err(|_| anyhow::anyhow!("cache mutex poisoned"))?.insert(p, (key, v.clone()));
            Ok(v)
        })
    }

    /// Every title anyone in the household sees, each as the first player
    /// who sees it has it, sorted by name. Library jobs (store data, series,
    /// digests) work on this, so a game only one player sees is still
    /// looked after; with one player it is that player's library.
    pub fn library_games(&self) -> Result<Vec<GameView>> {
        let mut seen = HashSet::new();
        let mut all = vec![];
        for p in self.players()? {
            for g in self.all_games_shared(p.id)?.iter() {
                if seen.insert(g.id) {
                    all.push(g.clone());
                }
            }
        }
        // The same order as the library query (COLLATE NOCASE folds ASCII only).
        all.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()).then(a.id.cmp(&b.id)));
        Ok(all)
    }

    /// An owned copy of every title player `p` sees (for callers that
    /// consume or change them).
    pub fn all_games(&self, p: PlayerId) -> Result<Vec<GameView>> {
        Ok(self.all_games_shared(p)?.as_ref().clone())
    }

    /// One title as player `p` has it; None when it isn't in their library.
    pub fn game(&self, p: PlayerId, title_id: i64) -> Result<Option<GameView>> {
        self.with(|c| {
            let v = Self::query_base(c, &format!("{} AND t.id = ?1 ORDER BY g.id", Self::GAME_SELECT), &[&title_id])?;
            let Some(base) = v.into_iter().next() else { return Ok(None) };
            let (sight, own) = (Sight::of(c, p)?, Own::load(c, p, Some(title_id))?);
            Ok(Self::for_player(&base, &sight, &own, &self.rules_for(c, p)?, super::now()))
        })
    }

    /// One title as player `p` has it, also when they hid it from themself.
    pub fn game_unhidden(&self, p: PlayerId, title_id: i64) -> Result<Option<GameView>> {
        self.with(|c| {
            let v = Self::query_base(c, &format!("{} AND t.id = ?1 ORDER BY g.id", Self::GAME_SELECT), &[&title_id])?;
            let Some(base) = v.into_iter().next() else { return Ok(None) };
            let (mut sight, own) = (Sight::of(c, p)?, Own::load(c, p, Some(title_id))?);
            sight.unhide(title_id);
            Ok(Self::for_player(&base, &sight, &own, &self.rules_for(c, p)?, super::now()))
        })
    }

    /// Resolve a CLI reference to a title in player `p`'s library: `#id`
    /// (title id), `source:source_id` (any copy), or a bare number (Steam
    /// appid first, then title id).
    pub fn resolve(&self, p: PlayerId, reference: &str) -> Result<Option<GameView>> {
        let reference = reference.trim();
        if let Some(num) = reference.strip_prefix('#') {
            return self.game(p, num.parse()?);
        }
        let titles_by_copy = |source: &str, sid: &str| -> Result<Vec<i64>> {
            self.with(|c| {
                let mut st = c.prepare("SELECT DISTINCT title_id FROM games WHERE source=?1 AND source_id=?2 AND removed=0 ORDER BY id")?;
                Ok(st.query_map(params![source, sid], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
            })
        };
        if let Some((source, sid)) = reference.split_once(':') {
            for id in titles_by_copy(source, sid)? {
                if let Some(g) = self.game(p, id)? {
                    return Ok(Some(g));
                }
            }
            return Ok(None);
        }
        if let Ok(n) = reference.parse::<i64>() {
            let by_appid: Option<i64> =
                self.with(|c| Ok(c.query_row("SELECT id FROM titles WHERE appid=?1", params![n], |r| r.get(0)).optional()?))?;
            if let Some(id) = by_appid
                && let Some(g) = self.game(p, id)?
            {
                return Ok(Some(g));
            }
            return self.game(p, n);
        }
        Ok(None)
    }

    /// Filter/sort/paginate player `p`'s library in memory. Libraries are a
    /// few thousand rows at most.
    pub fn list_games(&self, p: PlayerId, f: &GameFilter) -> Result<(usize, Vec<GameView>)> {
        let all = self.all_games_shared(p)?;
        Ok(apply_filter(&all, f))
    }

    pub fn stats(&self, p: PlayerId) -> Result<Stats> {
        let all = self.all_games_shared(p)?;
        Ok(compute_stats(&all))
    }
}

type SteamArt = (Option<String>, Option<String>, Vec<String>);
type ProtonRow = (i64, Option<String>, Option<String>, Option<i64>);
/// One title's rows while `query_views` folds copies into views: title id, copies, title-level data.
type TitleRows = (i64, Vec<(Copy, CopyExtras)>, TitleMeta);

#[derive(Debug, Clone)]
struct CopyExtras {
    gog_tags: Vec<String>,
    gog_properties: Vec<String>,
    gog_features: Vec<String>,
    gog_description: Option<String>,
    gog_rating: Option<f64>,
    gog_rating_count: Option<i64>,
    gog_reviews: Vec<ReviewText>,
    art_cover: Option<String>,
    art_square: Option<String>,
    description: Option<String>,
    genres: Vec<String>,
    developer: Option<String>,
    release: Option<String>,
}

#[derive(Debug, Clone)]
struct TitleMeta {
    name: String,
    appid: Option<u32>,
    details_success: Option<i64>,
    type_: Option<String>,
    short_description: Option<String>,
    d_genres: Vec<String>,
    categories: Vec<String>,
    d_header: Option<String>,
    metacritic: Option<i64>,
    d_release: Option<String>,
    d_developers: Vec<String>,
    reviews_success: Option<i64>,
    review_score: Option<i64>,
    review_desc: Option<String>,
    total_positive: Option<i64>,
    total_reviews: Option<i64>,
    tags_success: Option<i64>,
    tags_raw: Option<String>,
    hltb_success: Option<i64>,
    hltb_id: Option<i64>,
    hltb_main: Option<f64>,
    hltb_extra: Option<f64>,
    hltb_complete: Option<f64>,
    match_success: Option<i64>,
    igdb_success: Option<i64>,
    igdb_genres: Vec<String>,
    igdb_themes: Vec<String>,
    igdb_keywords: Vec<String>,
    igdb_perspectives: Vec<String>,
    igdb_modes: Vec<String>,
    igdb_summary: Option<String>,
    igdb_rating: Option<f64>,
    igdb_rating_count: Option<i64>,
    rt_success: Option<i64>,
    rt_items: Vec<ReviewText>,
    digest: Option<String>,
    review_tags: Option<ReviewTags>,
    digest_version: Option<i64>,
    vibes: Option<String>,
    /// The "This game has no ending" switch, when set.
    no_ending: Option<bool>,
    /// The shared data's date, for a summary that came from it.
    seed_date: Option<String>,
    /// Steam's `controller_support`: "full" or "partial".
    controller_support: Option<String>,
}

/// Merged tag list, most trusted source first: Steam user tags, what player
/// reviews say (review tags), GOG, RAWG, IGDB, PCGamingWiki.
/// Case-insensitive dedup, capped at 30.
fn merge_tags(by_source: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut tags: Vec<String> = vec![];
    for src in ["steam", "reviews", "gog", "rawg", "igdb", "pcgw"] {
        for t in by_source.get(src).into_iter().flatten() {
            if tags.len() >= 30 {
                return tags;
            }
            if !tags.iter().any(|x| x.eq_ignore_ascii_case(t)) {
                tags.push(t.clone());
            }
        }
    }
    tags
}

/// Percent-encode a query-string value.
fn url_query(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn header_image_url(appid: u32) -> String {
    format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/header.jpg")
}

fn source_rank(source: &str) -> u8 {
    match source {
        "steam" => 0,
        "gog" => 1,
        "epic" => 2,
        "amazon" => 3,
        _ => 4,
    }
}

/// A status a player set, with its note and when.
struct Mark {
    status: Override,
    note: Option<String>,
    at: i64,
}

/// Everything in a player's library that is theirs alone, by title.
#[derive(Default)]
struct Own {
    marks: HashMap<i64, Mark>,
    signals: HashMap<i64, PickSignals>,
    /// Play later, and when.
    saved: HashMap<i64, i64>,
    ratings: HashMap<i64, (u8, i64)>,
    /// The latest reasons each title was put down for.
    stop_reasons: HashMap<i64, StopReasons>,
    second_chances: HashMap<i64, SecondChanceNote>,
}

impl Own {
    /// Player `p`'s rows, for every title or (`one`) a single one.
    fn load(c: &Connection, p: PlayerId, one: Option<i64>) -> Result<Self> {
        let filter = one.map(|id| format!(" AND title_id = {id}")).unwrap_or_default();
        let mut own = Own { signals: pick_signals(c, p)?, ..Default::default() };
        let mut st = c.prepare(&format!("SELECT title_id, status, note, updated_at FROM overrides WHERE player_id=?1{filter}"))?;
        for r in st.query_map(params![p], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, i64>(3)?))
        })? {
            let (id, status, note, at) = r?;
            if let Some(status) = Override::parse(&status) {
                own.marks.insert(id, Mark { status, note, at });
            }
        }
        let mut st = c.prepare(&format!("SELECT title_id, added_at FROM play_later WHERE player_id=?1{filter}"))?;
        for r in st.query_map(params![p], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
            let (id, at) = r?;
            own.saved.insert(id, at);
        }
        let mut st = c.prepare(&format!("SELECT title_id, stars, at FROM game_ratings WHERE player_id=?1{filter}"))?;
        for r in st.query_map(params![p], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))? {
            let (id, stars, at) = r?;
            own.ratings.insert(id, (stars.clamp(1, 5) as u8, at));
        }
        let mut st =
            c.prepare(&format!("SELECT title_id, reasons, note, source, at FROM stop_reasons WHERE player_id=?1{filter} ORDER BY id"))?;
        for r in st.query_map(params![p], |r| {
            Ok((r.get::<_, i64>(0)?, StopReasons { codes: json_vec(r.get(1)?), note: r.get(2)?, source: r.get(3)?, at: r.get(4)? }))
        })? {
            let (id, reasons) = r?;
            // Every reason cleared on the Second Chance page: none stand.
            if reasons.codes.is_empty() && reasons.note.is_none() {
                own.stop_reasons.remove(&id);
            } else {
                own.stop_reasons.insert(id, reasons);
            }
        }
        // The Second Chance page's "Now" and answers.
        let mut st = c.prepare(&format!(
            "SELECT title_id, now, verdict, line, inputs, written_at, model, answer, answered_at FROM second_chances
             WHERE player_id=?1{filter}"
        ))?;
        for r in st.query_map(params![p], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                SecondChanceNote {
                    now: r.get(1)?,
                    verdict: r.get::<_, Option<String>>(2)?.as_deref().and_then(Verdict::parse),
                    line: r.get(3)?,
                    inputs: r.get(4)?,
                    written_at: r.get(5)?,
                    model: r.get(6)?,
                    outdated: false,
                    answer: r.get::<_, Option<String>>(7)?.as_deref().and_then(Answer::parse),
                    answered_at: r.get(8)?,
                },
            ))
        })? {
            let (id, note) = r?;
            own.second_chances.insert(id, note);
        }
        Ok(own)
    }
}

/// What a title's picks and the Second Chance page say about it, from
/// `pick_feedback`, `pick_ratings` and `second_chances`.
#[derive(Default)]
struct PickSignals {
    /// When it was really turned down, oldest first: in picks
    /// (`FeedbackAction::is_rejection`) or with Still no.
    rejections: Vec<i64>,
    too: Option<TooNote>,
    chosen: Option<ChosenPick>,
    /// When "Willing to try again" was said.
    retry_at: Option<i64>,
    /// The latest rating it had as a pick.
    rated: Option<RatedPick>,
}

static NO_SIGNALS: PickSignals = PickSignals { rejections: Vec::new(), too: None, chosen: None, retry_at: None, rated: None };

/// Every title's pick signals for player `p`, from their own lineups. The
/// table holds a few rows per lineup, so it's read whole.
fn pick_signals(c: &Connection, p: PlayerId) -> Result<HashMap<i64, PickSignals>> {
    let mut out: HashMap<i64, PickSignals> = HashMap::new();
    let mut st = c.prepare(
        "SELECT f.title_id, f.action, f.text, f.at, b.prompt FROM pick_feedback f
         JOIN pick_batches b ON b.id = f.batch_id WHERE b.player_id = ?1 ORDER BY f.at, f.id",
    )?;
    let rows = st.query_map(params![p], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, Option<String>>(4)?,
        ))
    })?;
    for row in rows {
        let (id, action, text, at, prompt) = row?;
        let Some(action) = FeedbackAction::parse(&action) else { continue };
        let s = out.entry(id).or_default();
        if action.is_rejection() {
            s.rejections.push(at);
        }
        match (action, text) {
            (FeedbackAction::Too, Some(word)) => s.too = Some(TooNote { word, at }),
            (FeedbackAction::Chosen, _) => s.chosen = Some(ChosenPick { at, prompt: prompt.unwrap_or_default() }),
            _ => {}
        }
    }
    let mut st = c.prepare(
        "SELECT r.title_id, r.verdict, r.reason, r.at, b.prompt FROM pick_ratings r
         JOIN pick_batches b ON b.id = r.batch_id WHERE b.player_id = ?1 ORDER BY r.at, r.rowid",
    )?;
    let rows = st.query_map(params![p], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, Option<String>>(4)?,
        ))
    })?;
    for row in rows {
        let (id, verdict, reason, at, prompt) = row?;
        let Some(verdict) = PickVerdict::parse(&verdict) else { continue };
        let reason = reason.as_deref().and_then(MissReason::parse);
        out.entry(id).or_default().rated = Some(RatedPick { verdict, reason, at, prompt: prompt.unwrap_or_default() });
    }
    let mut st = c.prepare("SELECT title_id, answer, answered_at FROM second_chances WHERE player_id = ?1 AND answered_at IS NOT NULL")?;
    let rows = st.query_map(params![p], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, i64>(2)?)))?;
    for row in rows {
        let (id, answer, at) = row?;
        match answer.as_deref().and_then(Answer::parse) {
            Some(Answer::StillNo) => {
                let s = out.entry(id).or_default();
                s.rejections.push(at);
                s.rejections.sort_unstable();
            }
            Some(Answer::Willing) => out.entry(id).or_default().retry_at = Some(at),
            Some(Answer::NotForMe) | None => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_labels_merge_right_after_steam() {
        let by_source: HashMap<String, Vec<String>> = [
            ("gog", vec!["Simulation", "relaxing"]),
            ("reviews", vec!["Relaxing", "Short sessions"]),
            ("steam", vec!["Farming Sim", "Relaxing"]),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.into_iter().map(String::from).collect()))
        .collect();
        assert_eq!(merge_tags(&by_source), ["Farming Sim", "Relaxing", "Short sessions", "Simulation"]);
    }
}
