//! Hidden gems: a short shelf of your unplayed games that players
//! rate very highly, that take a session or two, and that are unusual.
//!
//! **The pool** is unplayed, owned, not endless and not a hard no, with a
//! trusted rating (`gem_min_positive` percent of at least `gem_min_reviews`
//! Steam reviews, or 4.5 of 5 from 200 GOG ratings for a game Steam has no
//! reviews of), and not known to take more than a session or two: a main
//! story of `gem_max_hours` or less, or, with no HowLongToBeat time, nothing
//! in its review tags or strongest Steam tags saying it's long (`length`).
//! Gems whose length isn't known rank after the ones known to be short.
//! Obscurity is not a filter: it would narrow the pool too far.
//!
//! **What's unique** is three signals, chosen when the plan was made, which the Find hidden
//! gems job (`enrich::sources::hidden_gems`) has the AI rate from 0 to 3
//! for each game in the pool, many games per call, with a line on what
//! makes it unusual: a little-known studio (from the developer and
//! publisher, and whether the studio published it themselves), rare
//! mechanics (its three rarest Steam tags, with how many games in the
//! library carry each) and odd and interesting play (People say and Vibes).
//! It also marks party games, which are short but made for a group, and the
//! shelf leaves them out. The shelf is the `gem_count` best by the three
//! ratings together, the review score breaking ties. A gem you play leaves
//! the pool, so the shelf moves up.

use super::Member;
use crate::ai_usage::{self, Purpose};
use crate::config::Config;
use crate::db::{AiKind, Db, GameView, GemRow, Status};
use crate::lifecycle::{Kind, Offer};
use crate::llm::{LlmBackend, LlmRequest};
use crate::taste::Profile;
use anyhow::{Context, Result, anyhow};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;

/// The prompt's version. Bump it to rate every game again.
pub const PROMPT_VERSION: u32 = 1;
/// Games per call. About 80 games are about 15k tokens in.
pub const BATCH: usize = 40;
/// For a game Steam has no reviews of: GOG's rating out of 5, from this many ratings.
const GOG_MIN_RATING: f64 = 4.5;
const GOG_MIN_RATINGS: i64 = 200;
/// Its rarest tags sent, and its strongest tags read for them.
const RAREST: usize = 3;
const TAGS_READ: usize = 10;
/// Caps on what is stored and sent.
const LINE_MAX_CHARS: usize = 200;
const TEXT_MAX_CHARS: usize = 600;
/// Ranking points for each point of the three ratings together, in gem picks.
const POINTS_PER_RATING: f64 = 3.0;

/// What the pool and the shelf follow. The defaults match `config::SETTINGS`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rules {
    pub min_positive: i64,
    pub min_reviews: i64,
    pub max_hours: f64,
    pub count: usize,
}

impl Rules {
    pub fn from_config(cfg: &Config) -> Self {
        Rules {
            min_positive: cfg.gem_min_positive,
            min_reviews: cfg.gem_min_reviews,
            max_hours: cfg.gem_max_hours as f64,
            count: cfg.gem_count.max(1) as usize,
        }
    }
}

impl Default for Rules {
    fn default() -> Self {
        Rules { min_positive: 93, min_reviews: 500, max_hours: 5.0, count: 12 }
    }
}

/// Its rating is one to trust: high, from enough players.
pub fn trusted(g: &GameView, rules: &Rules) -> bool {
    match (g.review_pct, g.review_total) {
        (Some(p), Some(n)) if n > 0 => p >= rules.min_positive && n >= rules.min_reviews,
        _ => g.gog_rating.is_some_and(|r| r >= GOG_MIN_RATING) && g.gog_rating_count.is_some_and(|n| n >= GOG_MIN_RATINGS),
    }
}

/// In the pool: unplayed, offered by picks, not endless and not a hard no,
/// with a trusted rating and not known to take many sittings.
pub fn in_pool(g: &GameView, rules: &Rules, profile: Option<&Profile>) -> bool {
    g.status == Status::Untouched
        && g.offer() == Some(Offer::New)
        && g.kind != Kind::Endless
        && length(g, rules) != Length::Long
        && trusted(g, rules)
        && profile.is_none_or(|p| p.hard_no(g).is_none())
}

/// What UpNext knows of a game's length, for the pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Length {
    /// A session or two.
    Short,
    /// Nothing says either way: a gem, ranked after the short ones.
    Unknown,
    /// More than a session or two: out of the pool.
    Long,
}

/// Steam tags that, among a game's strongest, mark one that takes many
/// sittings. Without them, a game whose length isn't known stays a gem.
const LONG_TAGS: &[&str] = &[
    "Open World",
    "RPG",
    "JRPG",
    "CRPG",
    "Action RPG",
    "Strategy RPG",
    "Party-Based RPG",
    "Grand Strategy",
    "4X",
    "Massively Multiplayer",
    "MMORPG",
    "Open World Survival Craft",
    "Survival",
    "Base Building",
    "City Builder",
    "Colony Sim",
    "Farming Sim",
    "Life Sim",
    "Sandbox",
    "Management",
    "Metroidvania",
];
/// How many of a game's strongest tags are read for `LONG_TAGS`.
const LONG_WITHIN: usize = 5;
/// Ranking points taken from a gem whose length isn't known, so the ones
/// known to be short come first.
const UNKNOWN_LENGTH_POINTS: f64 = -3.0;

/// How long a game is: its HowLongToBeat main story against `max_hours`
/// when there is one. HowLongToBeat is off unless the user turns it on, so
/// without it players' review tags (short or long sessions) and then Steam's
/// tags decide ("Short", or one of `LONG_TAGS` among its strongest), and a
/// game nothing speaks for is `Unknown`, which the pool keeps
/// (with only the tags a library always has, the pool was empty).
pub fn length(g: &GameView, rules: &Rules) -> Length {
    if let Some(h) = g.hltb_main.filter(|h| *h > 0.0) {
        return if h <= rules.max_hours { Length::Short } else { Length::Long };
    }
    match g.review_tags.as_ref() {
        Some(t) if t.has("sessions:short") => return Length::Short,
        Some(t) if t.has("sessions:long") => return Length::Long,
        _ => {}
    }
    let tags = own_tags(g);
    if tags.iter().any(|t| t.eq_ignore_ascii_case("short")) {
        Length::Short
    } else if tags.iter().take(LONG_WITHIN).any(|t| LONG_TAGS.iter().any(|l| t.eq_ignore_ascii_case(l))) {
        Length::Long
    } else {
        Length::Unknown
    }
}

pub fn pool<'a>(games: &'a [GameView], rules: &Rules, profile: Option<&Profile>) -> Vec<&'a GameView> {
    games.iter().filter(|g| in_pool(g, rules, profile)).collect()
}

/// Whether the job should rate it: never rated, or rated by an older prompt.
pub fn needs_rating(g: &GameView, rated: &HashMap<i64, GemRow>, refresh: bool) -> bool {
    refresh || rated.get(&g.id).is_none_or(|r| r.prompt_version != PROMPT_VERSION)
}

/// The store's own tags, strongest first: Steam's, or a GOG copy's, else the merged list.
fn own_tags(g: &GameView) -> &[String] {
    ["steam", "gog"].iter().find_map(|s| g.tags_by_source.get(*s).filter(|t| !t.is_empty())).map(Vec::as_slice).unwrap_or(&g.tags)
}

fn tag_key(t: &str) -> String {
    t.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// How many games in the library carry each tag.
#[derive(Debug, Default)]
pub struct Rarity {
    counts: HashMap<String, usize>,
    pub total: usize,
}

impl Rarity {
    pub fn of(games: &[GameView]) -> Rarity {
        let mut r = Rarity { total: games.len(), ..Default::default() };
        for g in games {
            let mut seen: Vec<String> = vec![];
            for t in own_tags(g) {
                let k = tag_key(t);
                if !seen.contains(&k) {
                    *r.counts.entry(k.clone()).or_default() += 1;
                    seen.push(k);
                }
            }
        }
        r
    }

    /// A game's rarest tags among its strongest, with how many games carry
    /// each: ("Deckbuilding", 40).
    pub fn rarest(&self, g: &GameView) -> Vec<(String, usize)> {
        let mut tags: Vec<(String, usize)> =
            own_tags(g).iter().take(TAGS_READ).map(|t| (t.clone(), self.counts.get(&tag_key(t)).copied().unwrap_or(1))).collect();
        tags.sort_by_key(|(_, n)| *n);
        tags.truncate(RAREST);
        tags
    }
}

/// The facts one game is rated from.
fn block(g: &GameView, rarity: &Rarity, publishers: &HashMap<u32, Vec<String>>) -> String {
    let one_line = |s: &str| -> String { crate::digest::cut_near(&crate::llm::one_line(s, usize::MAX), TEXT_MAX_CHARS) };
    // Names, studios and tags come from stores, each kept to its line.
    let label = |s: &String| crate::llm::one_line(s, 60);
    let year = g.release_year().map(|y| format!(" ({y})")).unwrap_or_default();
    let mut s = format!("### #{} {}{year}\n", g.id, crate::llm::name_line(&g.name));
    let pubs: Vec<String> = g.appid.and_then(|a| publishers.get(&a)).into_iter().flatten().map(label).collect();
    let devs: Vec<String> = g.developers.iter().map(label).collect();
    let studio = match (devs.is_empty(), pubs.is_empty()) {
        (true, true) => "not known".to_string(),
        (false, true) => format!("developed by {}", devs.join(", ")),
        (true, false) => format!("published by {}", pubs.join(", ")),
        (false, false) => {
            let own = devs.iter().any(|d| pubs.iter().any(|p| p.eq_ignore_ascii_case(d)));
            format!("developed by {}; published by {}{}", devs.join(", "), pubs.join(", "), if own { " (self-published)" } else { "" })
        }
    };
    s.push_str(&format!("Studio: {studio}\n"));
    let rare: Vec<String> =
        rarity.rarest(g).into_iter().map(|(t, n)| format!("{} ({n} of {} games)", crate::llm::one_line(&t, 40), rarity.total)).collect();
    if !rare.is_empty() {
        s.push_str(&format!("Rarest tags in their library: {}\n", rare.join(", ")));
    }
    let tags: Vec<String> = own_tags(g).iter().take(TAGS_READ).map(|t| crate::llm::one_line(t, 40)).collect();
    if !tags.is_empty() {
        s.push_str(&format!("Tags: {}\n", tags.join(", ")));
    }
    if let Some(h) = g.hltb_main {
        s.push_str(&format!("Main story: {h}h\n"));
    }
    if let Some(d) = g.digest.as_deref().filter(|d| !d.trim().is_empty()) {
        s.push_str(&format!("People say: {}\n", one_line(d)));
    } else if let Some(d) = g.short_description.as_deref().filter(|d| !d.trim().is_empty()) {
        s.push_str(&format!("Store blurb: {}\n", one_line(d)));
    }
    if let Some(v) = g.vibes.as_deref().filter(|v| !v.trim().is_empty()) {
        s.push_str(&format!("Vibes: {}\n", one_line(v)));
    }
    s
}

pub(crate) fn schema() -> Value {
    let rating = |what: &str| json!({"type": "integer", "description": format!("{what}, 0 to 3")});
    json!({
        "type": "object",
        "properties": {"games": {"type": "array", "items": {"type": "object",
            "properties": {
                "id": {"type": "integer"},
                "studio": rating("how little-known the studio is"),
                "mechanics": rating("how rare its mechanics are"),
                "play": rating("how odd and interesting its play is"),
                "party": {"type": "boolean"},
                "line": {"type": "string"}
            },
            "required": ["id", "studio", "mechanics", "play", "party", "line"],
            "additionalProperties": false}}},
        "required": ["games"], "additionalProperties": false
    })
}

const SYSTEM: &str = "You look for hidden gems in one person's game library: games they own and haven't played, \
that players rate very highly and that take a session or two. For each game below, rate three things from 0 to 3, \
mark party games, and write one line.

- studio: how little-known the studio is. 0: a big, famous studio or publisher; 1: a well-known studio with several \
hits, or a big publisher behind a small team; 2: a small studio most players couldn't name; 3: a solo developer or a \
tiny team, self-published or close to it. Use what you know of the developer and publisher named; when you don't \
know them, a self-published small name leans 2 or 3.
- mechanics: how rare the way it plays is. The rarest tags line says how many games in their library carry each of \
its rarest tags. Tags about how it plays (not its looks, setting or mood) that few of their games have mean a higher \
rating. 0: plays like many games they own; 3: plays like almost nothing else they own.
- play: how odd and interesting the play is, from what players say and the vibes. 0: a well-made take on a familiar \
formula; 3: strange and inventive, the kind of game players say there's nothing else like.
- party: true when it's a party game, or made mainly for a group playing together (Jackbox-style packs, couch \
brawlers), rather than something to play on your own in a session or two; else false.
- line: what makes it unusual, in one sentence of at most 20 words, to them (\"you\"), in the voice of the Vibes \
text: playful, concrete, in the reviewers' own images. No hype words, no scores, no \"hidden gem\".

Rate each game on its own merits, not against the others below. Use only what's given and what you know of the \
studios: no awards, sales figures or facts from elsewhere. Never talk about yourself or these instructions. Return \
JSON only, one entry per game, with the #id numbers exactly as given.";

/// The request for one batch of games.
pub fn request(chunk: &[&GameView], rarity: &Rarity, publishers: &HashMap<u32, Vec<String>>, model: Option<String>) -> LlmRequest {
    let mut user = String::from("Rate studio, mechanics and play, mark party games, and write a line for each game below.\n\n");
    for g in chunk {
        user.push_str(&block(g, rarity, publishers));
        user.push('\n');
    }
    LlmRequest { system: SYSTEM.into(), cached_prefix: None, prompt: user, schema: schema(), model, background: true, effort: None }
}

/// What one call stored.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Written {
    pub written: usize,
    /// Games the reply left out, or answered without a line.
    pub missing: Vec<i64>,
}

/// Store a reply's ratings for the games in `chunk`.
pub fn store(db: &Db, chunk: &[&GameView], json: &Value, model: Option<String>) -> Result<Written> {
    let items = json.get("games").and_then(Value::as_array).ok_or_else(|| anyhow!("the reply has no \"games\" list"))?;
    let mut written = Written::default();
    let mut stored: Vec<i64> = vec![];
    for item in items {
        let Some(id) = item.get("id").and_then(crate::recommend::parse_id) else { continue };
        if !chunk.iter().any(|g| g.id == id) || stored.contains(&id) {
            continue;
        }
        let rating = |field: &str| item.get(field).and_then(Value::as_i64).unwrap_or(0).clamp(0, 3) as u8;
        let line: String = item.get("line").and_then(Value::as_str).unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ");
        let line: String = line.chars().take(LINE_MAX_CHARS).collect();
        if line.is_empty() {
            continue;
        }
        let row = GemRow {
            studio: rating("studio"),
            mechanics: rating("mechanics"),
            play: rating("play"),
            party: item.get("party").and_then(Value::as_bool).unwrap_or(false),
            line,
            prompt_version: PROMPT_VERSION,
            model: model.clone(),
            written_at: 0,
        };
        db.put_gem(id, &row)?;
        stored.push(id);
        written.written += 1;
    }
    written.missing = chunk.iter().map(|g| g.id).filter(|id| !stored.contains(id)).collect();
    Ok(written)
}

/// Rate one batch of games and store what comes back.
pub async fn generate(
    db: &Db,
    llm: &dyn LlmBackend,
    model: Option<String>,
    chunk: &[&GameView],
    rarity: &Rarity,
    publishers: &HashMap<u32, Vec<String>>,
) -> Result<Written> {
    let purpose = Purpose::new(AiKind::HiddenGems).items(chunk.len());
    let resp = ai_usage::call(db, llm, &purpose, request(chunk, rarity, publishers, model)).await.context("hidden gems")?;
    let model_name = resp.model.clone().or_else(|| Some("unknown".into()));
    store(db, chunk, &resp.json, model_name)
}

/// The rated games in the pool, best first: the ones known to be short,
/// then the three ratings together, the review score and how many reviews.
/// Party games are left out.
pub fn ranked<'a>(pool: &[&'a GameView], rules: &Rules, rated: &'a HashMap<i64, GemRow>) -> Vec<(&'a GameView, &'a GemRow)> {
    let score = |g: &GameView| g.review_pct.map(|p| p as f64).or(g.gog_rating.map(|r| r * 20.0)).unwrap_or(0.0);
    let mut out: Vec<(&GameView, &GemRow)> = pool.iter().filter_map(|g| rated.get(&g.id).filter(|r| !r.party).map(|r| (*g, r))).collect();
    // Known to be short first, then by the three ratings together.
    let unknown = |g: &GameView| length(g, rules) == Length::Unknown;
    out.sort_by(|(a, ra), (b, rb)| {
        unknown(a)
            .cmp(&unknown(b))
            .then(rb.score().cmp(&ra.score()))
            .then(score(b).partial_cmp(&score(a)).unwrap_or(std::cmp::Ordering::Equal))
            .then(b.review_total.cmp(&a.review_total))
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

/// One game on the shelf, or in the ranked pool.
#[derive(Debug, Serialize)]
pub struct Gem {
    pub game: GameView,
    /// The AI's ratings and line; None for a game in the pool not rated yet.
    pub rating: Option<GemRow>,
    /// The three ratings together, 0 to 9.
    pub score: Option<u8>,
    /// Its three rarest tags, with how many games in the library carry each.
    pub rarest: Vec<(String, usize)>,
}

/// The shelf and what's behind it.
#[derive(Debug, Serialize)]
pub struct Shelf {
    /// The best `gem_count` rated games.
    pub gems: Vec<Gem>,
    /// Games in the pool now.
    pub pool: usize,
    /// Games in the pool rated by the current prompt.
    pub rated: usize,
    /// Games the next Find hidden gems run rates.
    pub unrated: usize,
    /// Rated as party games, so off the shelf.
    pub party: usize,
    /// With `all`: every game in the pool, ranked ones first, then the ones not rated yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ranked: Option<Vec<Gem>>,
}

pub fn shelf(games: &[GameView], profile: Option<&Profile>, rules: &Rules, rated: &HashMap<i64, GemRow>, all: bool) -> Shelf {
    let pool = pool(games, rules, profile);
    let rarity = Rarity::of(games);
    let order = ranked(&pool, rules, rated);
    let gem = |g: &GameView, r: Option<&GemRow>| Gem {
        game: g.slim(),
        rating: r.cloned(),
        score: r.map(GemRow::score),
        rarest: rarity.rarest(g),
    };
    let gems: Vec<Gem> = order.iter().take(rules.count).map(|(g, r)| gem(g, Some(r))).collect();
    let current = |g: &&&GameView| rated.get(&g.id).is_some_and(|r| r.prompt_version == PROMPT_VERSION);
    let ranked = all.then(|| {
        let mut list: Vec<Gem> = order.iter().map(|(g, r)| gem(g, Some(r))).collect();
        let party = pool.iter().filter(|g| rated.get(&g.id).is_some_and(|r| r.party)).map(|g| gem(g, rated.get(&g.id)));
        let unrated = pool.iter().filter(|g| !rated.contains_key(&g.id)).map(|g| gem(g, None));
        list.extend(party.chain(unrated));
        list
    });
    Shelf {
        gems,
        pool: pool.len(),
        rated: pool.iter().filter(current).count(),
        unrated: pool.iter().filter(|g| needs_rating(g, rated, false)).count(),
        party: pool.iter().filter(|g| rated.get(&g.id).is_some_and(|r| r.party)).count(),
        ranked,
    }
}

/// The games "A hidden gem" may pick: the pool without party games, the
/// rated ones with their line and points for how unusual they are.
pub fn members(games: &[GameView], profile: Option<&Profile>, rules: &Rules, rated: &HashMap<i64, GemRow>) -> HashMap<i64, Member> {
    let mut out = HashMap::new();
    for g in pool(games, rules, profile) {
        let mut member = match rated.get(&g.id) {
            Some(r) if r.party => continue,
            Some(r) => Member {
                note: format!("unusual: {} (studio {}/3, mechanics {}/3, play {}/3)", r.line, r.studio, r.mechanics, r.play),
                reason: format!("Unusual: {}", r.line.trim_end_matches('.')),
                ways: vec![],
                points: POINTS_PER_RATING * r.score() as f64,
            },
            None => Member { note: "hidden gem, not rated yet".into(), reason: "A hidden gem".into(), ways: vec![], points: 0.0 },
        };
        if length(g, rules) == Length::Unknown {
            member.note.push_str("; its length isn't known");
            member.points += UNKNOWN_LENGTH_POINTS;
        }
        out.insert(g.id, member);
    }
    out
}

#[cfg(test)]
mod tests;
