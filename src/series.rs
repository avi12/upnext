//! Game series ("Collections"): groups owned titles by series (PCGamingWiki
//! and Wikidata), orders them by original release, and keeps an AI-written
//! guide per series: which owned entry to start with, the play order, and
//! which entries are spin-offs, remasters or duplicates.

use crate::ai_usage::{self, Purpose};
use crate::db::{AiKind, Db, GameView, Status};
use crate::llm::name_line;
use crate::llm::{LlmBackend, LlmRequest};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;

/// Series per LLM call.
pub const BATCH: usize = 8;

/// Kinds of owned entries that sit outside the main line.
pub const SIDE_KINDS: [&str; 7] = ["spin-off", "remaster", "duplicate", "expansion", "multiplayer", "tool", "other"];

/// Normalised grouping key: "The Witcher", "Witcher" and "The Witcher series" match.
pub fn series_key(name: &str) -> String {
    let decoded = decode_entities(name).to_lowercase();
    let mut s = decoded.trim().to_string();
    for suffix in [" (video game series)", " video game series", " (series)", " series", " franchise"] {
        if let Some(stripped) = s.strip_suffix(suffix) {
            s = stripped.to_string();
        }
    }
    if let Some(stripped) = s.strip_prefix("the ") {
        s = stripped.to_string();
    }
    s.chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// PCGamingWiki stores some punctuation as HTML entities ("40&#44;000").
pub fn decode_entities(s: &str) -> String {
    s.replace("&#44;", ",").replace("&#39;", "'").replace("&amp;", "&").replace("&quot;", "\"").replace("&#58;", ":").replace("&#38;", "&")
}

fn year_of(g: &GameView) -> Option<i32> {
    g.first_year.or_else(|| g.release_year().and_then(|y| y.parse().ok()))
}

#[derive(Debug, Clone)]
pub struct SeriesGroup {
    pub key: String,
    pub name: String,
    /// Owned titles in the series, oldest first.
    pub members: Vec<GameView>,
    /// Series folded into this one ("Black Ops" inside "Call of Duty").
    pub subseries: Vec<SubSeries>,
    /// Games in the series you don't own (PC releases, from Wikidata), oldest first.
    pub missing: Vec<crate::wikidata::SeriesEntry>,
}

/// A smaller series folded into a bigger one, and which owned games are in it.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct SubSeries {
    pub name: String,
    pub ids: Vec<i64>,
}

/// Every game in each series from Wikidata (see `Db::series_catalog`) and the
/// series QIDs found by name for labels that came without one.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub by_series: HashMap<String, Vec<crate::wikidata::SeriesEntry>>,
    pub key_qids: HashMap<String, String>,
}

impl SeriesGroup {
    pub fn members_hash(&self) -> String {
        let mut ids: Vec<i64> = self.members.iter().map(|g| g.id).collect();
        ids.sort_unstable();
        // The prefix is the guide prompt's version: bump it to regenerate every guide.
        format!("v3:{}", ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(","))
    }
}

/// Series with at least two owned games (not-games excluded): the ones that
/// get an AI guide.
pub fn groups(all: &[GameView], catalog: &Catalog) -> Vec<SeriesGroup> {
    build_groups(all, catalog).into_iter().filter(|g| g.members.len() >= 2).collect()
}

/// Each title's series, by name, as Collections groups them: a game in a
/// series folded into a bigger one is in the bigger one.
pub fn of_titles(all: &[GameView], catalog: &Catalog) -> HashMap<i64, Vec<String>> {
    let mut out: HashMap<i64, Vec<String>> = HashMap::new();
    for g in groups(all, catalog) {
        for m in &g.members {
            out.entry(m.id).or_default().push(g.name.clone());
        }
    }
    out
}

/// Owned games collected under one series key, before naming.
#[derive(Default)]
struct Bucket {
    /// Each spelling of the series label, with how many games used it.
    labels: HashMap<String, usize>,
    members: Vec<GameView>,
    /// Wikidata series QIDs seen on members, with counts.
    qids: HashMap<String, usize>,
}

/// Owned games grouped by series, with the unowned entries of each series.
/// A series whose owned games all belong to a bigger one is folded into it.
fn build_groups(all: &[GameView], catalog: &Catalog) -> Vec<SeriesGroup> {
    let games: Vec<&GameView> = all.iter().filter(|g| g.status != Status::NotGame).collect();
    let mut by_key: HashMap<String, Bucket> = HashMap::new();
    for g in &games {
        let mut seen: Vec<String> = vec![];
        for label in &g.series {
            let key = series_key(label);
            if key.is_empty() || seen.contains(&key) {
                continue;
            }
            seen.push(key.clone());
            let e = by_key.entry(key.clone()).or_default();
            *e.labels.entry(decode_entities(label).trim().to_string()).or_insert(0) += 1;
            e.members.push(g.slim());
            if let Some(q) = g.series_qids.get(&key) {
                *e.qids.entry(q.clone()).or_insert(0) += 1;
            }
        }
    }
    // Titles no source tagged ("The Elder Scrolls IV: Oblivion - GOTY Deluxe")
    // join a series whose name they start with, followed by a number.
    let keys: Vec<String> = by_key.iter().filter(|(_, b)| b.members.len() >= 2).map(|(k, _)| k.clone()).collect();
    for g in games.iter().filter(|g| g.series.is_empty()) {
        if let Some(key) = numbered_entry_of(&g.name, &keys)
            && let Some(e) = by_key.get_mut(&key)
            && !e.members.iter().any(|m| m.id == g.id)
        {
            e.members.push(g.slim());
        }
    }

    // Owned games by Wikidata QID, Steam appid and name, to tell which
    // catalog entries you already have (even ones no source tagged).
    let mut owned_by_qid: HashMap<&str, &GameView> = HashMap::new();
    let mut owned_by_appid: HashMap<u32, &GameView> = HashMap::new();
    let mut owned_by_name: HashMap<String, &GameView> = HashMap::new();
    for g in &games {
        if let Some(x) = &g.xref {
            owned_by_qid.insert(x.qid.as_str(), g);
        }
        if let Some(a) = g.appid {
            owned_by_appid.insert(a, g);
        }
        owned_by_name.insert(crate::db::loose_key(&g.name), g);
    }
    let owned_match = |e: &crate::wikidata::SeriesEntry| -> Option<&GameView> {
        let loose = crate::db::loose_key(&e.name);
        owned_by_qid
            .get(e.qid.as_str())
            .or_else(|| e.steam_appid.and_then(|a| owned_by_appid.get(&a)))
            .or_else(|| owned_by_name.get(&loose))
            // An edition you own under a longer name: "The Witcher 3: Complete
            // Edition" is the "The Witcher 3: Wild Hunt - Complete Edition" you have.
            .or_else(|| {
                crate::db::edition_of(&e.name)?;
                let prefix = format!("{loose} ");
                owned_by_name.iter().find(|(k, _)| k.starts_with(&prefix)).map(|(_, g)| g)
            })
            .copied()
    };

    let mut out: Vec<(SeriesGroup, Option<String>)> = by_key
        .into_iter()
        .map(|(key, Bucket { labels: names, members, qids })| {
            let name = names.into_iter().max_by(|a, b| a.1.cmp(&b.1).then(b.0.len().cmp(&a.0.len()))).map(|(n, _)| n).unwrap_or_default();
            let qid = qids.into_iter().max_by_key(|(_, n)| *n).map(|(q, _)| q).or_else(|| catalog.key_qids.get(&key).cloned());
            (SeriesGroup { key, name, members, subseries: vec![], missing: vec![] }, qid)
        })
        .collect();

    // Catalog entries: owned ones join the group, the rest are "missing".
    for (grp, qid) in out.iter_mut() {
        let Some(entries) = qid.as_ref().and_then(|q| catalog.by_series.get(q)) else { continue };
        for e in entries {
            match owned_match(e) {
                Some(g) => {
                    if !grp.members.iter().any(|m| m.id == g.id) {
                        grp.members.push(g.slim());
                    }
                }
                None => grp.missing.push(e.clone()),
            }
        }
    }

    // Fold series into the smallest bigger series that holds all their owned games.
    let ids_of = |g: &SeriesGroup| -> std::collections::BTreeSet<i64> { g.members.iter().map(|m| m.id).collect() };
    out.sort_by(|a, b| {
        b.0.members.len().cmp(&a.0.members.len()).then(b.0.missing.len().cmp(&a.0.missing.len())).then(a.0.name.len().cmp(&b.0.name.len()))
    });
    let mut kept: Vec<SeriesGroup> = vec![];
    for (grp, _) in out {
        let mine = ids_of(&grp);
        // The last (smallest) kept group that contains this one.
        let parent = kept.iter_mut().rev().find(|p| mine.is_subset(&ids_of(p)));
        match parent {
            Some(p) => {
                if mine.len() < p.members.len() {
                    p.subseries.push(SubSeries { name: grp.name.clone(), ids: mine.into_iter().collect() });
                }
                for e in grp.missing {
                    if !p.missing.iter().any(|x| x.qid == e.qid) {
                        p.missing.push(e);
                    }
                }
            }
            None => kept.push(grp),
        }
    }
    for g in kept.iter_mut() {
        g.members.sort_by(|a, b| {
            year_of(a).unwrap_or(9999).cmp(&year_of(b).unwrap_or(9999)).then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        g.missing.sort_by(|a, b| a.year.unwrap_or(9999).cmp(&b.year.unwrap_or(9999)).then(a.name.cmp(&b.name)));
    }
    // A collection needs two games, owned or not.
    kept.retain(|g| g.members.len() + g.missing.len() >= 2);
    kept.sort_by(|a, b| a.key.cmp(&b.key));
    kept
}

/// The longest series key that `name` starts with, when the next word is a
/// number or Roman numeral ("elder scrolls iv oblivion" -> "elder scrolls").
fn numbered_entry_of(name: &str, keys: &[String]) -> Option<String> {
    const ROMAN: [&str; 12] = ["i", "ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x", "xi", "xii"];
    let nk = series_key(name);
    keys.iter()
        .filter(|k| {
            nk.strip_prefix(k.as_str())
                .and_then(|rest| rest.strip_prefix(' '))
                .and_then(|rest| rest.split(' ').next())
                .is_some_and(|w| ROMAN.contains(&w) || w.chars().all(|c| c.is_ascii_digit()))
        })
        .max_by_key(|k| k.len())
        .cloned()
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SideEntry {
    pub id: i64,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Guide {
    pub start_id: i64,
    /// Main-line play order (owned entries only).
    pub order: Vec<i64>,
    pub side: Vec<SideEntry>,
    pub note: String,
    pub model: Option<String>,
    pub generated_at: i64,
    #[serde(skip)]
    pub members_hash: String,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct Collection {
    pub key: String,
    pub name: String,
    /// Owned titles, in play order when a guide exists, else oldest first.
    pub members: Vec<GameView>,
    /// Original release year per member id (Wikidata, else store date).
    pub years: HashMap<i64, i32>,
    pub played: usize,
    /// Where to begin the series.
    pub start_id: Option<i64>,
    /// The next main-line game to play given what you've played.
    pub next_id: Option<i64>,
    pub guide: Option<Guide>,
    /// The guide was written for a different set of owned games.
    pub guide_stale: bool,
    /// Smaller series folded into this one, with the owned games in each.
    pub subseries: Vec<SubSeries>,
    /// Games in the series you don't own (released on PC), oldest first.
    pub missing: Vec<crate::wikidata::SeriesEntry>,
}

fn is_unplayed(g: &GameView) -> bool {
    matches!(g.status, Status::Untouched | Status::Sampled)
}

/// The collection containing title `id`, if it's in a series.
pub fn collection_of(all: &[GameView], guides: &HashMap<String, Guide>, catalog: &Catalog, id: i64) -> Option<Collection> {
    collections(all, guides, catalog).into_iter().find(|c| c.members.iter().any(|g| g.id == id))
}

pub fn collections(all: &[GameView], guides: &HashMap<String, Guide>, catalog: &Catalog) -> Vec<Collection> {
    let mut out: Vec<Collection> = build_groups(all, catalog)
        .into_iter()
        .map(|grp| {
            let hash = grp.members_hash();
            let guide = guides.get(&grp.key).cloned();
            let stale = guide.as_ref().is_some_and(|g| g.members_hash != hash);
            let ids: Vec<i64> = grp.members.iter().map(|g| g.id).collect();
            let side: Vec<i64> = guide.as_ref().map(|g| g.side.iter().map(|s| s.id).collect()).unwrap_or_default();
            // Guide order first, then anything it didn't mention, oldest first.
            let mut order: Vec<i64> =
                guide.as_ref().map(|g| g.order.iter().copied().filter(|i| ids.contains(i)).collect()).unwrap_or_default();
            for id in &ids {
                if !order.contains(id) && !side.contains(id) {
                    order.push(*id);
                }
            }
            let by_id: HashMap<i64, &GameView> = grp.members.iter().map(|g| (g.id, g)).collect();
            let start_id = guide.as_ref().map(|g| g.start_id).filter(|i| ids.contains(i)).or_else(|| order.first().copied());
            let next_id = match start_id.and_then(|s| by_id.get(&s)) {
                Some(s) if is_unplayed(s) => Some(s.id),
                _ => order.iter().copied().find(|i| by_id.get(i).is_some_and(|g| is_unplayed(g))),
            };
            let mut members: Vec<GameView> = order.iter().filter_map(|i| by_id.get(i).map(|g| g.slim())).collect();
            for id in &side {
                if let Some(g) = by_id.get(id) {
                    members.push(g.slim());
                }
            }
            let years = grp.members.iter().filter_map(|g| year_of(g).map(|y| (g.id, y))).collect();
            let played = grp.members.iter().filter(|g| !is_unplayed(g)).count();
            Collection {
                key: grp.key,
                name: grp.name,
                members,
                years,
                played,
                start_id,
                next_id,
                guide,
                guide_stale: stale,
                subseries: grp.subseries,
                missing: grp.missing,
            }
        })
        .collect();
    // Started series first, then bigger ones.
    out.sort_by(|a, b| (b.played > 0).cmp(&(a.played > 0)).then(b.members.len().cmp(&a.members.len())).then(a.name.cmp(&b.name)));
    out
}

pub(crate) fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {"series": {"type": "array", "items": {"type": "object", "properties": {
            "key": {"type": "string"},
            "start_id": {"type": "integer"},
            "order": {"type": "array", "items": {"type": "integer"}},
            "side": {"type": "array", "items": {"type": "object", "properties": {
                "id": {"type": "integer"},
                "kind": {"type": "string", "enum": SIDE_KINDS}
            }, "required": ["id", "kind"], "additionalProperties": false}},
            "note": {"type": "string"}
        }, "required": ["key", "start_id", "order", "side", "note"], "additionalProperties": false}}},
        "required": ["series"], "additionalProperties": false
    })
}

fn member_line(g: &GameView) -> String {
    let mut parts = vec![format!("#{}", g.id), name_line(&g.name)];
    if let Some(y) = year_of(g) {
        parts.push(y.to_string());
    }
    parts.push(match g.status {
        Status::Untouched => "not started".to_string(),
        _ if g.playtime_minutes > 0 => format!("played {}h", (g.playtime_minutes as f64 / 60.0 * 10.0).round() / 10.0),
        s => s.as_str().to_string(),
    });
    if let Some(h) = g.hltb_main {
        parts.push(format!("{h}h story"));
    }
    if let Some(p) = g.review_pct {
        parts.push(format!("{p}% positive"));
    }
    if let Some(l) = &g.linux {
        parts.push(format!("linux {l}"));
    }
    parts.join(" | ")
}

#[derive(Deserialize)]
struct Out {
    series: Vec<OutItem>,
}
#[derive(Deserialize)]
struct OutItem {
    key: String,
    start_id: i64,
    order: Vec<i64>,
    side: Vec<SideEntry>,
    note: String,
}

/// No guide yet, or one written before the owned games changed.
pub fn needs_guide(group: &SeriesGroup, guides: &HashMap<String, Guide>) -> bool {
    guides.get(&group.key).is_none_or(|g| g.members_hash != group.members_hash())
}

/// Write guides for `groups` in batches. Returns the number stored.
pub async fn generate_guides(db: &Db, llm: &dyn LlmBackend, model: Option<String>, groups: &[SeriesGroup]) -> Result<usize> {
    let system = "You know video game series well. For each series below, the person owns only the games listed \
        (ids, original release year, how much they've played, length, reviews). Decide, using your knowledge of the series:\n\
        - start_id: the owned game this person should play first. Consider how well older entries hold up today, whether \
          entries stand alone, and what they've already played. Often the first main entry, but not always.\n\
        - order: the owned main-line games in the order to play them (ids): release order, or story order where the \
          story needs it. Don't sort by quality; \
          start_id and the note are where you say what's best to begin with.\n\
        - side: owned entries that are not main line, with a kind: spin-off, remaster (a newer version of a game also listed), \
          duplicate (the same game twice, e.g. two editions), expansion, multiplayer (online-only or party game), tool (editor, benchmark, mod kit), other.\n\
        - note: one or two short sentences to the player, casual and specific: why start there, and anything worth knowing \
          (e.g. 'you can skip the first one', 'play the Redux versions'). No hype.\n\
        Every owned id goes in exactly one of order or side. When the person owns two versions of the same game (an \
        original and a remaster, Redux or definitive edition), put the one you recommend in order and the other in side \
        (as duplicate or remaster). start_id, order and the note must agree: if the note says to play a version, that \
        version's id is the one in order. Plain punctuation, no em dashes. Use only the listed ids. Return JSON only.";
    let mut written = 0;
    for chunk in groups.chunks(BATCH) {
        let mut user = String::new();
        for grp in chunk {
            user.push_str(&format!("### key: {} | series: {}\n", grp.key, name_line(&grp.name)));
            for g in &grp.members {
                user.push_str(&member_line(g));
                if let Some(sub) = grp.subseries.iter().find(|s| s.ids.contains(&g.id)) {
                    user.push_str(&format!(" | sub-series: {}", name_line(&sub.name)));
                }
                user.push('\n');
            }
            if !grp.missing.is_empty() {
                let names: Vec<String> = grp
                    .missing
                    .iter()
                    .map(|e| match e.year {
                        Some(y) => format!("{} ({y})", name_line(&e.name)),
                        None => name_line(&e.name),
                    })
                    .collect();
                user.push_str(&format!("Not owned (context only, never use in order or side): {}\n", names.join("; ")));
            }
            user.push('\n');
        }
        let req = LlmRequest {
            system: system.into(),
            cached_prefix: None,
            prompt: user,
            schema: schema(),
            model: model.clone(),
            background: true,
            effort: None,
        };
        let resp = ai_usage::call(db, llm, &Purpose::new(AiKind::SeriesGuides).items(chunk.len()), req)
            .await
            .context("series guide generation")?;
        let out: Out = serde_json::from_value(resp.json).context("series guide output did not match the schema")?;
        let model_name = resp.model.clone();
        for item in out.series {
            let Some(grp) = chunk.iter().find(|g| g.key == item.key) else { continue };
            let ids: Vec<i64> = grp.members.iter().map(|g| g.id).collect();
            if !ids.contains(&item.start_id) {
                continue;
            }
            let order: Vec<i64> = item.order.into_iter().filter(|i| ids.contains(i)).collect();
            let side: Vec<SideEntry> = item
                .side
                .into_iter()
                .filter(|s| ids.contains(&s.id) && !order.contains(&s.id))
                .map(|s| SideEntry { id: s.id, kind: if SIDE_KINDS.contains(&s.kind.as_str()) { s.kind } else { "other".into() } })
                .collect();
            let guide = Guide {
                start_id: item.start_id,
                order,
                side,
                note: item.note.trim().to_string(),
                model: model_name.clone(),
                generated_at: crate::db::now(),
                members_hash: grp.members_hash(),
            };
            db.put_series_guide(&grp.key, &guide)?;
            written += 1;
        }
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        assert_eq!(series_key("The Witcher"), "witcher");
        assert_eq!(series_key("Witcher"), "witcher");
        assert_eq!(series_key("Warhammer 40&#44;000: Dawn of War"), "warhammer 40 000 dawn of war");
        assert_eq!(series_key("Metro (video game series)"), "metro");
        assert_eq!(series_key("The Elder Scrolls"), series_key("Elder Scrolls"));
    }

    #[test]
    fn numbered_entries() {
        let keys = vec!["elder scrolls".to_string(), "portal".to_string(), "metro".to_string()];
        assert_eq!(
            numbered_entry_of("The Elder Scrolls IV: Oblivion - Game of the Year Edition Deluxe", &keys).as_deref(),
            Some("elder scrolls")
        );
        assert_eq!(numbered_entry_of("Portal 2", &keys).as_deref(), Some("portal"));
        assert_eq!(numbered_entry_of("Portal Knights", &keys), None);
        assert_eq!(numbered_entry_of("Metronome", &keys), None);
    }
}
