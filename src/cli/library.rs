//! Library commands: `mark`, `unmark`, `merge`, `split`, `list`, `show`, `stats`.

use super::format::{accent, bold, dim, green, hours, red, textwrap, trunc};
use super::open_db;
use crate::config::Config;
use crate::db::{GameFilter, GameView, Override, Status};
use crate::lifecycle::{Kind, REASONS};
use anyhow::Result;
use comfy_table::{Cell, ContentArrangement, Table, presets::UTF8_FULL_CONDENSED};

/// `status` is one of the values clap accepts for `mark`.
pub fn mark(cfg: &Config, status: &str, r#ref: &str, note: Option<&str>) -> Result<()> {
    let (db, cfg) = open_db(cfg)?;
    let g = db.resolve(cfg.whose(), r#ref)?.ok_or_else(|| anyhow::anyhow!("unknown game ref {ref:?}", ref = r#ref))?;
    let o = Override::parse(status).expect("validated by clap");
    db.set_override(cfg.whose(), g.id, Some(o), note)?;
    println!("{} {} marked {}", green("✓"), bold(&g.name), o.as_str());
    Ok(())
}

pub fn unmark(cfg: &Config, r#ref: &str) -> Result<()> {
    let (db, cfg) = open_db(cfg)?;
    let g = db.resolve(cfg.whose(), r#ref)?.ok_or_else(|| anyhow::anyhow!("unknown game ref {ref:?}", ref = r#ref))?;
    db.set_override(cfg.whose(), g.id, None, None)?;
    let g = db.game(cfg.whose(), g.id)?.expect("exists");
    println!("{} {} override cleared (now {})", green("✓"), bold(&g.name), g.status.as_str());
    Ok(())
}

pub fn merge(cfg: &Config, refs: &[String]) -> Result<()> {
    let (db, cfg) = open_db(cfg)?;
    let mut ids = vec![];
    for r in refs {
        ids.push(db.resolve(cfg.whose(), r)?.ok_or_else(|| anyhow::anyhow!("unknown game ref {r:?}"))?.id);
    }
    let tid = db.merge_titles(&ids)?;
    let g = db.game(cfg.whose(), tid)?.expect("exists");
    println!("{} merged into {} (#{}), owned on {}", green("✓"), bold(&g.name), g.id, g.owned_on.join(", "));
    Ok(())
}

pub fn split(cfg: &Config, copy: &str) -> Result<()> {
    let (db, cfg) = open_db(cfg)?;
    let g = db.resolve(cfg.whose(), copy)?.ok_or_else(|| anyhow::anyhow!("unknown game ref {copy:?}"))?;
    let (source, sid) = copy.split_once(':').ok_or_else(|| anyhow::anyhow!("give the copy as source:id, e.g. gog:1207658924"))?;
    let c = g.copies.iter().find(|c| c.source == source && c.source_id == sid).ok_or_else(|| anyhow::anyhow!("copy not found"))?;
    let tid = db.split_copy(c.game_id)?;
    let n = db.game(cfg.whose(), tid)?.expect("exists");
    println!("{} {} split off as its own title (#{})", green("✓"), bold(&n.name), n.id);
    Ok(())
}

/// `list --status`'s help, from the statuses themselves so it can't fall
/// behind them.
pub fn status_help() -> String {
    let names: Vec<&str> = Status::ALL.iter().map(|s| s.as_str()).collect();
    format!("Comma list of statuses, or `all` (default: untouched,sampled): {}", names.join(", "))
}

/// `list --status`: the statuses named, or None for `all`. A name that isn't
/// a status is an error rather than a filter that quietly matches less.
pub fn statuses(text: Option<&str>) -> Result<Option<Vec<Status>>> {
    match text.map(str::trim) {
        Some("all") => Ok(None),
        Some(s) => s
            .split(',')
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(|x| Status::parse(x).ok_or_else(|| anyhow::anyhow!("{x:?} isn't a status; {}", status_help().to_lowercase())))
            .collect::<Result<Vec<_>>>()
            .map(Some),
        None => Ok(Some(vec![Status::Untouched, Status::Sampled])),
    }
}

/// The API answers at most `PAGE` games a call, so a larger `--limit` is
/// read a page at a time.
pub fn list(cfg: &Config, filter: &GameFilter, json: bool) -> Result<()> {
    const PAGE: usize = 1000;
    let (db, cfg) = open_db(cfg)?;
    let mut items = vec![];
    let mut total = 0;
    let want = filter.limit.max(1);
    while items.len() < want {
        let page = GameFilter { offset: filter.offset + items.len(), limit: (want - items.len()).min(PAGE), ..filter.clone() };
        let (t, mut got) = db.list_games(cfg.whose(), &page)?;
        total = t;
        let last = got.len() < page.limit;
        items.append(&mut got);
        if last {
            break;
        }
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({"total": total, "items": items}))?);
    } else {
        print_game_table(&items);
        println!("{}", dim(&format!("{} of {total} shown", items.len())));
    }
    Ok(())
}

pub fn show(cfg: &Config, r#ref: &str, json: bool) -> Result<()> {
    let (db, cfg) = open_db(cfg)?;
    let g = db.resolve(cfg.whose(), r#ref)?.ok_or_else(|| anyhow::anyhow!("unknown game ref {ref:?}", ref = r#ref))?;
    if json {
        println!("{}", serde_json::to_string_pretty(&g)?);
    } else {
        print_game(&g);
    }
    Ok(())
}

pub fn stats(cfg: &Config) -> Result<()> {
    let (db, cfg) = open_db(cfg)?;
    let s = db.stats(cfg.whose())?;
    println!(
        "{}",
        bold(&format!(
            "{} ({}, {} owned on more than one store)",
            crate::enrich::plural(s.total, "game", "games"),
            crate::enrich::plural(s.copies, "store copy", "store copies"),
            s.multi_store
        ))
    );
    let mut t = Table::new();
    t.load_style(UTF8_FULL_CONDENSED);
    t.set_header(vec!["status", "count"]);
    for st in Status::ALL {
        t.add_row(vec![st.as_str().to_string(), s.by_status.get(st.as_str()).copied().unwrap_or(0).to_string()]);
    }
    println!("{t}");
    let mut t = Table::new();
    t.load_style(UTF8_FULL_CONDENSED);
    t.set_header(vec!["source", "count"]);
    let mut srcs: Vec<_> = s.by_source.iter().collect();
    srcs.sort();
    for (k, v) in srcs {
        t.add_row(vec![k.clone(), v.to_string()]);
    }
    println!("{t}");
    println!(
        "candidates {} · enriched {}/{} · reviews {} · tags {} · hltb {} · installed {} · total playtime {}",
        s.candidates,
        s.enriched,
        s.total,
        s.with_reviews,
        s.with_tags,
        s.with_hltb,
        s.installed,
        hours(s.total_playtime_minutes)
    );
    if !s.top_played.is_empty() {
        println!("\n{}", bold("most played"));
        for g in &s.top_played {
            println!("  {:>8}  {}", hours(g.playtime_minutes), g.name);
        }
    }
    Ok(())
}

fn print_game_table(items: &[GameView]) {
    let mut t = Table::new();
    t.load_style(UTF8_FULL_CONDENSED).set_content_arrangement(ContentArrangement::Dynamic);
    t.set_header(vec!["id", "name", "owned on", "status", "played", "hltb", "reviews", "genres"]);
    for g in items {
        t.add_row(vec![
            Cell::new(format!("#{}", g.id)),
            Cell::new(trunc(&g.name, 42)),
            Cell::new(g.owned_on.join("+")),
            Cell::new(g.status.as_str()),
            Cell::new(if g.playtime_known { hours(g.playtime_minutes) } else { "?".into() }),
            Cell::new(g.hltb_main.map(|h| format!("{h}h")).unwrap_or_else(|| "-".into())),
            Cell::new(g.review_pct.map(|p| format!("{p}%")).unwrap_or_else(|| "-".into())),
            Cell::new(g.genres.iter().take(3).cloned().collect::<Vec<_>>().join("/")),
        ]);
    }
    println!("{t}");
}

fn print_game(g: &GameView) {
    println!("{}  {}", bold(&g.name), dim(&format!("[#{}  owned on {}]", g.id, g.owned_on.join(", "))));
    let mut rows: Vec<(&str, String)> = vec![
        ("status", format!("{}{}", g.status.as_str(), g.override_note.as_ref().map(|n| format!(" ({n})")).unwrap_or_default())),
        ("played", if g.playtime_known { hours(g.playtime_minutes) } else { "not tracked".into() }),
        (
            "ending",
            format!(
                "{}{}{}",
                if g.kind == Kind::Endless { "none" } else { "has one" },
                if g.no_ending.is_some() { " (set by you)" } else { "" },
                if g.staple { "; one of your staples" } else { "" }
            ),
        ),
    ];
    if let Some(r) = &g.stop_reasons {
        let labels: Vec<&str> = r.codes.iter().filter_map(|c| REASONS.iter().find(|x| x.code == c).map(|x| x.label)).collect();
        rows.push((
            "stopped",
            [labels.join(", "), r.note.clone().unwrap_or_default()]
                .iter()
                .filter(|s| !s.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join(": "),
        ));
    }
    if let Some(a) = g.appid {
        rows.push(("steam appid", format!("{a}{}", if g.steam_matched { " (matched by name)" } else { "" })));
    }
    if let Some(t) = &g.type_ {
        rows.push(("type", t.clone()));
    }
    if !g.genres.is_empty() {
        rows.push(("genres", g.genres.join(", ")));
    }
    if !g.tags.is_empty() {
        rows.push(("tags", g.tags.join(", ")));
    }
    if !g.themes.is_empty() {
        rows.push(("themes", g.themes.join(", ")));
    }
    if !g.categories.is_empty() {
        rows.push(("features", g.categories.iter().take(8).cloned().collect::<Vec<_>>().join(", ")));
    }
    if let (Some(r), Some(n)) = (g.gog_rating, g.gog_rating_count) {
        rows.push(("gog rating", format!("{r:.1}/5 ({n})")));
    }
    if let Some(r) = g.igdb_rating {
        rows.push(("igdb rating", format!("{r}/100{}", g.igdb_rating_count.map(|n| format!(" ({n})")).unwrap_or_default())));
    }
    if let Some(d) = &g.digest {
        rows.push(("players say", d.clone()));
    }
    if let (Some(p), Some(d), Some(t)) = (g.review_pct, &g.review_desc, g.review_total) {
        rows.push(("reviews", format!("{d} — {p}% of {t}")));
    }
    if let Some(m) = g.metacritic {
        rows.push(("metacritic", m.to_string()));
    }
    if let Some(h) = g.hltb_main {
        rows.push((
            "hltb",
            format!(
                "main {h}h{}{}",
                g.hltb_extra.map(|x| format!(" · extra {x}h")).unwrap_or_default(),
                g.hltb_complete.map(|x| format!(" · 100% {x}h")).unwrap_or_default()
            ),
        ));
    }
    if let Some(r) = &g.release_date {
        rows.push(("released", r.clone()));
    }
    if !g.developers.is_empty() {
        rows.push(("developer", g.developers.join(", ")));
    }
    rows.push(("installed", if g.installed { "yes".into() } else { "no".into() }));
    if !g.editions.is_empty() {
        rows.push(("editions", g.editions.join(", ")));
    }
    for c in &g.copies {
        let mut bits = vec![format!("{}:{}", c.source, c.source_id), format!("via {}", c.launcher)];
        if let Some(e) = &c.edition {
            bits.push(format!("[{e}]"));
        }
        if c.installed {
            bits.push("installed".into());
        }
        if c.playtime_known {
            bits.push(format!("played {}", hours(c.playtime_minutes)));
        }
        if !c.run_url.is_empty() {
            bits.push(c.run_url.clone());
        }
        rows.push(("copy", bits.join("  ")));
    }
    if !g.store_url.is_empty() {
        rows.push(("store", g.store_url.clone()));
    }
    if let Some(u) = &g.hltb_url {
        rows.push(("hltb url", u.clone()));
    }
    for (k, v) in rows {
        println!("  {}{v}", dim(&format!("{k:<12}")));
    }
    if let Some(d) = &g.short_description {
        println!();
        for line in textwrap(d, 90) {
            println!("  {line}");
        }
    }
    if !g.review_texts.is_empty() {
        println!("\n  {}", bold("player reviews"));
        for r in g.review_texts.iter().take(6) {
            let mark = match (r.positive, r.rating) {
                (Some(true), _) => green("👍"),
                (Some(false), _) => red("👎"),
                (None, Some(x)) => accent(&format!("{x:.0}★")),
                _ => "·".into(),
            };
            let votes = if r.votes == 1 { "1 vote".to_string() } else { format!("{} votes", r.votes) };
            let meta = format!("{votes}{}", r.hours.map(|h| format!(", {h}h played")).unwrap_or_default());
            println!("  {mark} {}", dim(&meta));
            for line in textwrap(&r.text.chars().take(400).collect::<String>(), 88) {
                println!("     {line}");
            }
        }
    }
}
