//! In-memory filtering, sorting and stats over the library.

use super::model::{GameFilter, GameView, NameCount, SOURCES, Stats, Status};
use crate::review_tags::Verdict;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// In-memory filtering and stats
// ---------------------------------------------------------------------------

/// Search: every whitespace-separated term must appear in the name, an edition,
/// or the genres, tags, themes, categories or developers. Returns whether the
/// title matched and a name rank (lower sorts first): 0 exact name, 1 name
/// starts with the query, 2 name contains it, 3 metadata only.
fn search_match(g: &GameView, q: &str, terms: &[&str]) -> (bool, u8) {
    let name = g.name.to_lowercase();
    let loose = crate::names::normalize_name(&g.name).to_lowercase();
    if name == q || loose == q {
        return (true, 0);
    }
    if name.starts_with(q) || loose.starts_with(q) {
        return (true, 1);
    }
    if name.contains(q) {
        return (true, 2);
    }
    let meta = [&g.editions, &g.genres, &g.tags, &g.themes, &g.categories, &g.developers];
    let hit = terms.iter().all(|t| name.contains(t) || meta.iter().any(|list| list.iter().any(|v| v.to_lowercase().contains(t))));
    (hit, 3)
}

pub fn apply_filter(all: &[GameView], f: &GameFilter) -> (usize, Vec<GameView>) {
    let q = f.q.as_ref().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty());
    let terms: Vec<&str> = q.as_deref().map(|q| q.split_whitespace().collect()).unwrap_or_default();
    let mut name_rank: HashMap<i64, u8> = HashMap::new();
    let mut items: Vec<&GameView> = all
        .iter()
        .filter(|g| f.statuses.as_ref().is_none_or(|s| s.contains(&g.status)))
        .filter(|g| f.staple.is_none_or(|s| g.staple == s))
        .filter(|g| f.sources.as_ref().is_none_or(|s| g.owned_on.iter().any(|o| s.contains(o))))
        .filter(|g| f.installed.is_none_or(|want| !want || g.installed))
        .filter(|g| f.controller.is_none_or(|want| !want || matches!(g.controller_support.as_deref(), Some("full" | "partial"))))
        .filter(|g| {
            f.rating.as_deref().is_none_or(|want| match (want, g.my_rating) {
                ("rated", r) => r.is_some(),
                ("unrated", r) => r.is_none(),
                ("high", Some(r)) => r >= 4,
                ("low", Some(r)) => r <= 2,
                _ => false,
            })
        })
        .filter(|g| {
            let Some(q) = q.as_deref() else { return true };
            let (hit, rank) = search_match(g, q, &terms);
            name_rank.insert(g.id, rank);
            hit
        })
        .filter(|g| f.genre.as_ref().is_none_or(|x| g.genres.iter().any(|y| y.eq_ignore_ascii_case(x))))
        .filter(|g| f.tag.as_ref().is_none_or(|x| g.tags.iter().any(|y| y.eq_ignore_ascii_case(x))))
        .filter(|g| {
            let tags = g.review_tags.as_ref();
            f.facets.iter().all(|x| tags.is_some_and(|t| t.has(x)))
                && f.praised.iter().all(|a| tags.is_some_and(|t| t.judged(a, Verdict::Praised)))
                && f.not_criticized
                    .iter()
                    .all(|a| tags.is_some_and(|t| !t.judged(a, Verdict::Criticized) && !t.judged(a, Verdict::Divisive)))
        })
        .filter(|g| {
            f.backlog.as_deref().is_none_or(|want| match (want, g.backlog.as_ref()) {
                ("respects_time", Some(b)) => b.respects_time(),
                ("clicks_fast", Some(b)) => b.clicks_fast(),
                _ => false,
            })
        })
        .filter(|g| {
            f.linux.as_deref().is_none_or(|want| match (want, g.linux.as_deref()) {
                ("ok", Some(t)) => crate::protondb::runs_well(t),
                ("native", Some(t)) => t == "native",
                ("issues", Some(t)) => matches!(t, "silver" | "bronze" | "borked"),
                ("unknown", t) => t.is_none() || t == Some("pending"),
                _ => false,
            })
        })
        .collect();

    let sort = f.sort.as_str();
    let default_desc = !matches!(sort, "name" | "status" | "source" | "linux");
    let desc = match f.order.as_deref() {
        Some("asc") => false,
        Some("desc") => true,
        _ => default_desc,
    };
    fn cmp_opt<T: PartialOrd>(a: Option<T>, b: Option<T>, desc: bool) -> std::cmp::Ordering {
        match (a, b) {
            (Some(a), Some(b)) => {
                let o = a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal);
                if desc { o.reverse() } else { o }
            }
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    }
    // Keys that are costly to recompute inside the comparator.
    let lower: HashMap<i64, String> = items.iter().map(|g| (g.id, g.name.to_lowercase())).collect();
    let release: HashMap<i64, Option<(i32, u32, u32)>> =
        if sort == "release" { items.iter().map(|g| (g.id, g.release_key())).collect() } else { HashMap::new() };
    let lname = |g: &GameView| lower.get(&g.id).map(String::as_str).unwrap_or_default();
    items.sort_by(|a, b| {
        let o = match sort {
            "playtime" => cmp_opt(Some(a.playtime_minutes), Some(b.playtime_minutes), desc),
            "last_played" => cmp_opt(Some(a.last_played), Some(b.last_played), desc),
            "review" => cmp_opt(a.review_pct, b.review_pct, desc).then(cmp_opt(a.review_total, b.review_total, desc)),
            "hltb" => cmp_opt(a.hltb_main, b.hltb_main, desc),
            "metacritic" => cmp_opt(a.metacritic, b.metacritic, desc),
            "rating" => cmp_opt(a.my_rating, b.my_rating, desc),
            "release" => cmp_opt(release.get(&a.id).copied().flatten(), release.get(&b.id).copied().flatten(), desc),
            "status" => {
                let rank = |g: &GameView| Status::ALL.iter().position(|s| *s == g.status).unwrap_or(99);
                let o = rank(a).cmp(&rank(b));
                if desc { o.reverse() } else { o }
            }
            "source" => {
                // The stores you own it on, Steam < GOG < Epic < Amazon ("Steam + GOG" sits between Steam and GOG).
                let key = |g: &GameView| {
                    const ORDER: [&str; 4] = ["steam", "gog", "epic", "amazon"];
                    let mut r: Vec<usize> = g.owned_on.iter().map(|s| ORDER.iter().position(|x| x == s).unwrap_or(9)).collect();
                    r.sort_unstable();
                    r
                };
                let o = key(a).cmp(&key(b));
                if desc { o.reverse() } else { o }
            }
            "linux" => {
                const ORDER: [&str; 7] = ["native", "platinum", "gold", "silver", "bronze", "borked", "pending"];
                let rank = |g: &GameView| g.linux.as_deref().and_then(|t| ORDER.iter().position(|x| *x == t)).unwrap_or(ORDER.len());
                let o = rank(a).cmp(&rank(b));
                if desc { o.reverse() } else { o }
            }
            _ => {
                let o = lname(a).cmp(lname(b));
                if desc { o.reverse() } else { o }
            }
        };
        // Exact and leading name matches rank above partial and tag/genre matches.
        let rank = |g: &GameView| name_rank.get(&g.id).copied().unwrap_or(3);
        let by_name = rank(a).cmp(&rank(b));
        by_name.then(o).then_with(|| lname(a).cmp(lname(b)))
    });

    let total = items.len();
    let limit = f.limit.clamp(1, 1000);
    let page = items.into_iter().skip(f.offset).take(limit).map(GameView::slim).collect();
    (total, page)
}

pub fn compute_stats(all: &[GameView]) -> Stats {
    let mut by_status: HashMap<String, usize> = Status::ALL.iter().map(|s| (s.as_str().to_string(), 0)).collect();
    let mut by_source: HashMap<String, usize> = SOURCES.iter().map(|s| (s.to_string(), 0)).collect();
    let mut genres: HashMap<String, usize> = HashMap::new();
    let mut tags: HashMap<String, usize> = HashMap::new();
    let mut s = Stats {
        total: all.len(),
        copies: 0,
        multi_store: 0,
        by_status: HashMap::new(),
        by_source: HashMap::new(),
        installed: 0,
        candidates: 0,
        staples: 0,
        rated: 0,
        enriched: 0,
        unenriched: 0,
        with_reviews: 0,
        with_tags: 0,
        with_hltb: 0,
        with_review_texts: 0,
        with_digests: 0,
        with_xref: 0,
        with_rawg: 0,
        with_pcgw: 0,
        by_linux: HashMap::new(),
        total_playtime_minutes: 0,
        genres: vec![],
        tags: vec![],
        top_played: vec![],
    };
    for g in all {
        *by_status.entry(g.status.as_str().to_string()).or_insert(0) += 1;
        for src in &g.owned_on {
            *by_source.entry(src.clone()).or_insert(0) += 1;
        }
        s.copies += g.copies.len();
        if g.owned_on.len() > 1 {
            s.multi_store += 1;
        }
        if g.installed {
            s.installed += 1;
        }
        if g.offer().is_some() {
            s.candidates += 1;
        }
        if g.staple {
            s.staples += 1;
        }
        if g.my_rating.is_some() {
            s.rated += 1;
        }
        if g.enriched {
            s.enriched += 1;
        } else {
            s.unenriched += 1;
        }
        if g.review_total.is_some() {
            s.with_reviews += 1;
        }
        if !g.tags.is_empty() {
            s.with_tags += 1;
        }
        if g.hltb_main.is_some() {
            s.with_hltb += 1;
        }
        if !g.review_texts.is_empty() {
            s.with_review_texts += 1;
        }
        if g.digest.is_some() {
            s.with_digests += 1;
        }
        if g.wikidata_url.is_some() {
            s.with_xref += 1;
        }
        if g.rawg_url.is_some() {
            s.with_rawg += 1;
        }
        if g.pcgw_url.is_some() {
            s.with_pcgw += 1;
        }
        if g.status != Status::NotGame {
            *s.by_linux.entry(g.linux.clone().unwrap_or_else(|| "unknown".into())).or_insert(0) += 1;
        }
        s.total_playtime_minutes += g.playtime_minutes;
        for x in &g.genres {
            *genres.entry(x.clone()).or_insert(0) += 1;
        }
        for x in &g.tags {
            *tags.entry(x.clone()).or_insert(0) += 1;
        }
    }
    let to_sorted = |m: HashMap<String, usize>| {
        let mut v: Vec<NameCount> = m.into_iter().map(|(name, count)| NameCount { name, count }).collect();
        v.sort_by(|a, b| b.count.cmp(&a.count).then(a.name.cmp(&b.name)));
        v
    };
    s.genres = to_sorted(genres);
    s.tags = to_sorted(tags);
    s.by_status = by_status;
    s.by_source = by_source;
    let mut played: Vec<&GameView> = all.iter().filter(|g| g.playtime_minutes > 0).collect();
    played.sort_by_key(|g| std::cmp::Reverse(g.playtime_minutes));
    s.top_played = played.into_iter().take(10).map(GameView::slim).collect();
    s
}
