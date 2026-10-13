//! The taste profile: what UpNext knows about you as a player.
//! Favourites, likes, dislikes, hard nos and habits are what you said; the
//! patterns are counted from how you play and react, never guessed.
//!
//! A pattern compares the games you stuck with (favourites, staples, beaten,
//! played through, still played, chosen from picks and played) with the ones
//! you left (shelved, abandoned, not for you, turned down in picks), by what
//! they're like: review-tag values, genre and theme tags, and length. Old
//! evidence counts less (`TasteRules::half_life_days`), and a pattern needs
//! `TasteRules::min_games` games and a clear difference before it shows.

pub mod about;
pub mod diary;
pub mod questions;
pub mod routine;
mod vocab;

pub use about::{Profile, When, vague};
pub use vocab::Subject;

use crate::db::PlayerId;
use crate::db::{Db, GameView, NewStatement, PatternChanges, PatternRow, ReactionCounts, StatementRow, Status, now};
use crate::enrich::{Progress, ProgressSink};
use crate::lifecycle::{Kind, REASONS, ReasonGroup};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};

const DAY: f64 = 86_400.0;

/// The settings patterns follow. The defaults match `config::SETTINGS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TasteRules {
    pub half_life_days: i64,
    pub min_games: usize,
}

impl Default for TasteRules {
    fn default() -> Self {
        Self { half_life_days: 365, min_games: 4 }
    }
}

/// What the prior is worth, in games at your usual share, so one or two
/// games can't swing a share far.
const PRIOR_GAMES: f64 = 2.0;
/// How far a pattern's odds of sticking with a game have to be from your
/// usual odds: twice them, or half. Odds rather than a share, so "stick"
/// and "drift" are as hard to reach whatever your usual share is.
const CLEAR_RATIO: f64 = 2.0;
/// The odds that the difference is real, below which a pattern isn't shown,
/// and above which it's "sure" rather than "fairly sure".
const MIN_CONFIDENCE: f64 = 0.8;
pub const SURE: f64 = 0.95;
/// A game counting at least this much (two half-lives) is recent enough to
/// be one of the games a pattern needs; older ones only add their weight.
const RECENT: f64 = 0.25;
/// Patterns the same way whose smaller set of games is mostly inside the
/// other's ("real-time strategy games" inside "strategy games") show once.
const OVERLAP: f64 = 0.8;
const MAX_PATTERNS: usize = 15;
/// Strongest Steam tags read per game.
const TOP_TAGS: usize = 10;

// ---------------------------------------------------------------------------
// Which way a game counts
// ---------------------------------------------------------------------------

/// What a game says about your taste. The first six are games you stuck
/// with, the rest games you left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Favourite,
    Staple,
    Beaten,
    /// A game with an ending played past 80% of its main story.
    PlayedThrough,
    /// An endless game marked Still playing.
    StillPlaying,
    /// Chosen from a lineup with Play, and played since.
    Picked,
    Shelved,
    Abandoned,
    NotForMe,
    /// "It's too ___" on a game you hadn't played.
    TurnedDown,
}

impl Outcome {
    pub fn kept(self) -> bool {
        matches!(self, Self::Favourite | Self::Staple | Self::Beaten | Self::PlayedThrough | Self::StillPlaying | Self::Picked)
    }

    /// "2 staples", "3 abandoned".
    pub fn count_text(self, n: usize) -> String {
        let one = n == 1;
        let words = match self {
            Self::Favourite if one => "favourite",
            Self::Favourite => "favourites",
            Self::Staple if one => "staple",
            Self::Staple => "staples",
            Self::Beaten => "beaten",
            Self::PlayedThrough => "played through",
            Self::StillPlaying => "still played",
            Self::Picked => "picked and played",
            Self::Shelved => "shelved",
            Self::Abandoned => "abandoned",
            Self::NotForMe => "not for you",
            Self::TurnedDown => "turned down in picks",
        };
        format!("{n} {words}")
    }
}

/// Whether the only reasons given for putting a game down are about life
/// ("Got busy"), which never count against a game.
fn life_only(g: &GameView) -> bool {
    let Some(r) = &g.stop_reasons else { return false };
    !r.codes.is_empty() && r.codes.iter().all(|c| REASONS.iter().any(|x| x.code == c.as_str() && x.group == ReasonGroup::Life))
}

/// The game reasons given for putting it down, in the menu's order.
fn game_reasons(g: &GameView) -> Vec<&'static str> {
    let Some(r) = &g.stop_reasons else { return vec![] };
    REASONS.iter().filter(|x| x.group == ReasonGroup::Game && r.codes.iter().any(|c| c == x.code)).map(|x| x.code).collect()
}

/// Which way a game counts, and from when (a mark from when you set it,
/// anything else from when you last played it), or None when it says nothing
/// about your taste. What you said wins: a favourite counts as kept, and a
/// game you marked Abandoned counts as left even if it's a staple.
pub fn judge(g: &GameView, favourite: bool) -> Option<(Outcome, Option<i64>)> {
    if g.status == Status::NotGame {
        return None;
    }
    if favourite {
        return Some((Outcome::Favourite, None));
    }
    let played = (g.last_played > 0).then_some(g.last_played);
    match g.status {
        Status::Hidden => return Some((Outcome::NotForMe, g.override_at)),
        // From when you stopped: UpNext only finds a game shelved or
        // abandoned months or years after that, and dating it then would
        // make it weigh more than a game finished at the same time.
        Status::Abandoned if !life_only(g) => return Some((Outcome::Abandoned, g.override_at.or(played).or(g.abandoned_at))),
        Status::Shelved if !life_only(g) => return Some((Outcome::Shelved, g.override_at.or(played).or(g.shelved_at))),
        Status::Abandoned | Status::Shelved => return None,
        _ => {}
    }
    if g.status == Status::Done {
        return Some((Outcome::Beaten, g.override_at));
    }
    if g.staple {
        return Some((Outcome::Staple, played));
    }
    if g.status == Status::Played && g.kind == Kind::Ending {
        return Some((Outcome::PlayedThrough, played));
    }
    if g.status == Status::StillPlaying {
        return Some((Outcome::StillPlaying, g.override_at.or(played)));
    }
    if let Some(c) = &g.chosen {
        // Played since, or Play is all a store without playtime can tell.
        let since = if g.playtime_known { g.last_played >= c.at } else { g.status == Status::Playing };
        if since {
            return Some((Outcome::Picked, Some(c.at)));
        }
    }
    if let Some(t) = &g.too
        && matches!(g.status, Status::Untouched | Status::Sampled)
    {
        return Some((Outcome::TurnedDown, Some(t.at)));
    }
    None
}

/// How much a game's evidence counts now: half per half-life since `at`. A
/// favourite always counts fully; a game with no date counts a half.
pub fn weight(outcome: Outcome, at: Option<i64>, rules: &TasteRules, now: i64) -> f64 {
    if outcome == Outcome::Favourite {
        return 1.0;
    }
    match at.filter(|&t| t > 0) {
        Some(t) => 0.5f64.powf((now - t).max(0) as f64 / DAY / rules.half_life_days.max(1) as f64),
        None => 0.5,
    }
}

/// A game's tags, strongest first: Steam's, or else the merged list without
/// the review-tag labels, which patterns read as review tags.
fn store_tags(g: &GameView) -> Vec<String> {
    if let Some(steam) = g.tags_by_source.get("steam").filter(|t| !t.is_empty()) {
        return steam.clone();
    }
    let labels = g.tags_by_source.get("reviews");
    g.tags.iter().filter(|t| !labels.is_some_and(|l| l.contains(t))).cloned().collect()
}

/// What a game is like, as patterns can read it: its review-tag values and
/// genre tags from `vocab`, and its length when it has an ending.
pub fn features(g: &GameView) -> Vec<Subject> {
    let mut out: Vec<Subject> = vec![];
    let mut push = |s: Subject| {
        if s.phrase().is_some() && !out.contains(&s) {
            out.push(s);
        }
    };
    if let Some(t) = &g.review_tags {
        for (scale, value) in &t.scales {
            push(Subject::Facet(format!("{scale}:{value}")));
        }
        for m in &t.mood {
            push(Subject::Facet(format!("mood:{m}")));
        }
        for s in &t.skills {
            push(Subject::Facet(format!("skills:{s}")));
        }
    }
    for t in store_tags(g).iter().take(TOP_TAGS).chain(&g.genres) {
        if let Some(name) = vocab::known_tag(t) {
            push(Subject::Tag(name.to_string()));
        }
    }
    if g.kind == Kind::Ending
        && let Some(h) = g.hltb_main.filter(|h| *h > 0.0)
    {
        push(Subject::Length(vocab::length_of(h)));
    }
    out
}

/// Whether a game is about a subject, for the counts beside likes and
/// dislikes. Your own words match nothing here; only the AI reads them.
pub fn matches(subject: &Subject, g: &GameView) -> bool {
    match subject {
        Subject::Facet(f) => g.review_tags.as_ref().is_some_and(|t| t.has(f)),
        Subject::Tag(_) | Subject::Length(_) => features(g).contains(subject),
        Subject::Text(_) => false,
    }
}

/// How far down a game's tags a hard no on a tag reaches (decided
/// 2026-10-02). At ten, a Horror hard no left out many
/// games that are only horror-adjacent; at five, a little over half as many.
pub const HARD_NO_TAGS: usize = 5;

/// The tags a hard no reads, strongest first: the store's own, Steam's or a
/// GOG copy's. RAWG's, IGDB's and PCGamingWiki's are matched to a game by
/// name, sometimes to the wrong one (Football Manager 2022 and Dispatch had
/// Horror from RAWG), and a hard no hides a game for good, so they don't
/// count here; likes, dislikes and patterns still read them.
fn own_store_tags(g: &GameView) -> &[String] {
    ["steam", "gog"].iter().find_map(|s| g.tags_by_source.get(*s).filter(|t| !t.is_empty())).map(Vec::as_slice).unwrap_or(&[])
}

/// Whether a hard no is about a game: a tag among the store's five strongest
/// or its store genres, and review-tag values and lengths as `matches`.
pub fn hard_no_matches(subject: &Subject, g: &GameView) -> bool {
    match subject {
        Subject::Tag(t) => own_store_tags(g).iter().take(HARD_NO_TAGS).chain(&g.genres).any(|x| vocab::known_tag(x) == Some(t.as_str())),
        _ => matches(subject, g),
    }
}

/// Whether a hard no is about a game you don't own, from the store's tags
/// (strongest first) and genres alone: the same five-tag reach as
/// `hard_no_matches`. Such a game has no review tags or length of its own
/// in the library, so only a hard no on a tag can reach it.
pub fn hard_no_matches_store(subject: &Subject, tags: &[String], genres: &[String]) -> bool {
    match subject {
        Subject::Tag(t) => tags.iter().take(HARD_NO_TAGS).chain(genres).any(|x| vocab::known_tag(x) == Some(t.as_str())),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Patterns
// ---------------------------------------------------------------------------

/// Which way a pattern goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sense {
    /// You stick with games like these.
    Stick,
    /// You drift away from them.
    Drift,
}

impl Sense {
    fn id(self) -> &'static str {
        match self {
            Sense::Stick => "stick",
            Sense::Drift => "drift",
        }
    }

    /// The kind of statement "That's right" turns it into.
    pub fn statement_kind(self) -> &'static str {
        match self {
            Sense::Stick => "like",
            Sense::Drift => "dislike",
        }
    }
}

/// One game behind a pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceGame {
    pub id: i64,
    pub name: String,
    pub outcome: Outcome,
    /// When it counts from; None for favourites and games with no date.
    pub at: Option<i64>,
    pub weight: f64,
    /// The game reasons you gave for putting it down (`lifecycle::REASONS` codes).
    #[serde(default)]
    pub reasons: Vec<String>,
}

/// What a pattern was found from, stored with it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub sense: Sense,
    /// The subject's stored form, "facet:grind:heavy".
    pub subject: String,
    /// "4 abandoned, 1 shelved, 3 of them for the grind; 1 played through"
    pub summary: String,
    /// The share of these games you stuck with, weighted.
    pub share: f64,
    /// Your share over every game that counts, weighted.
    pub usual: f64,
    /// The pattern's side first, the heaviest first.
    pub games: Vec<EvidenceGame>,
}

/// A pattern as found.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    /// "drift:facet:grind:heavy"
    pub key: String,
    pub sentence: String,
    pub evidence: Evidence,
    /// The odds that the difference is real.
    pub confidence: f64,
}

/// How many games the patterns were read from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Judged {
    pub kept: usize,
    pub left: usize,
}

/// One game that counts, with what it's like.
struct Counted<'a> {
    g: &'a GameView,
    outcome: Outcome,
    at: Option<i64>,
    weight: f64,
    features: Vec<Subject>,
}

/// Find the patterns in a library. `stated` holds the subjects you've
/// already said something about: a stated line wins over a derived one, so
/// those aren't patterns.
pub fn find_patterns(
    games: &[GameView],
    favourites: &HashSet<i64>,
    stated: &HashSet<String>,
    rules: &TasteRules,
    now: i64,
) -> (Vec<Found>, Judged) {
    let counted: Vec<Counted> = games
        .iter()
        .filter_map(|g| {
            let (outcome, at) = judge(g, favourites.contains(&g.id))?;
            Some(Counted { g, outcome, at, weight: weight(outcome, at, rules, now), features: features(g) })
        })
        .collect();
    let judged =
        Judged { kept: counted.iter().filter(|c| c.outcome.kept()).count(), left: counted.iter().filter(|c| !c.outcome.kept()).count() };
    let sum = |it: &mut dyn Iterator<Item = &Counted>| -> (f64, f64) {
        it.fold((0.0, 0.0), |(k, l), c| if c.outcome.kept() { (k + c.weight, l) } else { (k, l + c.weight) })
    };
    let (kept_w, left_w) = sum(&mut counted.iter());
    if kept_w <= 0.0 || left_w <= 0.0 {
        // Nothing to compare: every game that counts went the same way.
        return (vec![], judged);
    }
    let usual = kept_w / (kept_w + left_w);

    let mut subjects: Vec<&Subject> = counted.iter().flat_map(|c| c.features.iter()).collect();
    subjects.sort_by_key(|s| s.key());
    subjects.dedup();
    let mut found: Vec<(Found, HashSet<i64>, f64)> = vec![];
    for subject in subjects {
        if stated.contains(&subject.key()) {
            continue;
        }
        let with: Vec<&Counted> = counted.iter().filter(|c| c.features.contains(subject)).collect();
        let (k, l) = sum(&mut with.iter().copied());
        let (a, b) = (k + PRIOR_GAMES * usual, l + PRIOR_GAMES * (1.0 - usual));
        let share = a / (a + b);
        let odds = |p: f64| p / (1.0 - p);
        let ratio = odds(share) / odds(usual);
        let sense = if ratio >= CLEAR_RATIO {
            Sense::Stick
        } else if ratio <= 1.0 / CLEAR_RATIO {
            Sense::Drift
        } else {
            continue;
        };
        let side: Vec<&Counted> = with.iter().copied().filter(|c| c.outcome.kept() == (sense == Sense::Stick)).collect();
        if side.iter().filter(|c| c.weight >= RECENT).count() < rules.min_games {
            continue;
        }
        let below = beta_cdf(usual, a, b);
        let confidence = if sense == Sense::Stick { 1.0 - below } else { below };
        if confidence < MIN_CONFIDENCE {
            continue;
        }
        let Some(phrase) = subject.phrase() else { continue };
        let sentence = match sense {
            Sense::Stick => format!("You stick with {phrase}."),
            Sense::Drift => format!("You drift away from {phrase}."),
        };
        let mut ordered = with.clone();
        ordered.sort_by(|x, y| {
            let theirs = |c: &Counted| c.outcome.kept() != (sense == Sense::Stick);
            (theirs(x), y.weight).partial_cmp(&(theirs(y), x.weight)).unwrap_or(std::cmp::Ordering::Equal).then(x.g.name.cmp(&y.g.name))
        });
        let evidence = Evidence {
            sense,
            subject: subject.key(),
            summary: summary(&with, sense, subject),
            share: k / (k + l),
            usual,
            games: ordered
                .iter()
                .map(|c| EvidenceGame {
                    id: c.g.id,
                    name: c.g.name.clone(),
                    outcome: c.outcome,
                    at: c.at,
                    weight: (c.weight * 1000.0).round() / 1000.0,
                    reasons: if c.outcome.kept() { vec![] } else { game_reasons(c.g).into_iter().map(str::to_string).collect() },
                })
                .collect(),
        };
        let ids: HashSet<i64> = side.iter().map(|c| c.g.id).collect();
        let strength = ratio.ln().abs();
        found.push((Found { key: format!("{}:{}", sense.id(), subject.key()), sentence, evidence, confidence }, ids, strength));
    }

    // The surest first; one that says the same as a surer one, about the
    // same games, goes.
    found.sort_by(|(x, _, xs), (y, _, ys)| {
        (y.confidence, ys).partial_cmp(&(x.confidence, xs)).unwrap_or(std::cmp::Ordering::Equal).then(x.key.cmp(&y.key))
    });
    let mut kept: Vec<(Found, HashSet<i64>, f64)> = vec![];
    for (f, ids, s) in found {
        let same = kept.iter().any(|(k, kids, _)| {
            let both = ids.intersection(kids).count() as f64;
            let smaller = ids.len().min(kids.len()).max(1) as f64;
            k.evidence.sense == f.evidence.sense && both / smaller >= OVERLAP
        });
        if !same && kept.len() < MAX_PATTERNS {
            kept.push((f, ids, s));
        }
    }
    (kept.into_iter().map(|(f, ..)| f).collect(), judged)
}

/// "4 abandoned, 1 shelved, 3 of them for the grind; 1 played through (and
/// 6 older ones)": the recent games, the pattern's side first, the commonest
/// outcome first, then how many older ones count little.
fn summary(with: &[&Counted], sense: Sense, subject: &Subject) -> String {
    let older = with.iter().filter(|c| c.weight < RECENT).count();
    let with: Vec<&Counted> = with.iter().copied().filter(|c| c.weight >= RECENT).collect();
    let side_text = |kept: bool| -> Option<String> {
        let mut by: BTreeMap<Outcome, usize> = BTreeMap::new();
        for c in with.iter().filter(|c| c.outcome.kept() == kept) {
            *by.entry(c.outcome).or_default() += 1;
        }
        if by.is_empty() {
            return None;
        }
        let mut by: Vec<(Outcome, usize)> = by.into_iter().collect();
        by.sort_by_key(|&(o, n)| (std::cmp::Reverse(n), o));
        let mut text = by.into_iter().map(|(o, n)| o.count_text(n)).collect::<Vec<_>>().join(", ");
        if !kept && let Some(aspect) = subject.aspect() {
            let codes: Vec<&str> = REASONS.iter().filter(|r| r.aspects.contains(&aspect)).map(|r| r.code).collect();
            let n = with.iter().filter(|c| !c.outcome.kept() && game_reasons(c.g).iter().any(|r| codes.contains(r))).count();
            if n > 0 {
                text.push_str(&format!(", {n} of them for {}", vocab::aspect_reason(aspect)));
            }
        }
        Some(text)
    };
    let first = sense == Sense::Stick;
    let mut text = [side_text(first), side_text(!first)].into_iter().flatten().collect::<Vec<_>>().join("; ");
    match older {
        0 => {}
        1 => text.push_str(" (and 1 older one)"),
        n => text.push_str(&format!(" (and {n} older ones)")),
    }
    text
}

/// The regularized incomplete beta function I_x(a, b): the chance that a
/// share drawn from Beta(a, b) is below x. Continued fraction, as in
/// Numerical Recipes.
pub fn beta_cdf(x: f64, a: f64, b: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let front = (ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1.0 - x).ln()).exp();
    if x < (a + 1.0) / (a + b + 2.0) { front * beta_fraction(x, a, b) / a } else { 1.0 - front * beta_fraction(1.0 - x, b, a) / b }
}

fn beta_fraction(x: f64, a: f64, b: f64) -> f64 {
    const TINY: f64 = 1e-300;
    let (qab, qap, qam) = (a + b, a + 1.0, a - 1.0);
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < TINY {
        d = TINY;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..300 {
        let m = m as f64;
        let m2 = 2.0 * m;
        let aa = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + aa / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        h *= d * c;
        let aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + aa / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < 1e-12 {
            break;
        }
    }
    h
}

/// ln Γ(x) for x > 0 (Lanczos, g = 7).
fn ln_gamma(x: f64) -> f64 {
    const G: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        // Reflection, for the small shares a prior can give.
        return (std::f64::consts::PI / (std::f64::consts::PI * x).sin()).ln() - ln_gamma(1.0 - x);
    }
    let x = x - 1.0;
    let t = x + 7.5;
    let s = G.iter().enumerate().skip(1).fold(G[0], |s, (i, g)| s + g / (x + i as f64));
    0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + s.ln()
}

// ---------------------------------------------------------------------------
// Updating the profile
// ---------------------------------------------------------------------------

/// What an update found and changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Update {
    pub patterns: usize,
    pub judged: Judged,
    pub changes: PatternChanges,
}

/// Count the patterns again and store them. Runs after each sync (the
/// profile job) and after every change to the profile.
pub fn update(db: &Db, p: PlayerId, rules: &TasteRules) -> Result<Update> {
    let games = db.all_games_shared(p)?;
    let rows = db.taste_statements(p)?;
    let favourites: HashSet<i64> = rows.iter().filter(|r| r.kind == "favourite").filter_map(|r| r.title_id).collect();
    let stated: HashSet<String> =
        rows.iter().filter(|r| matches!(r.kind.as_str(), "like" | "dislike" | "hard_no")).filter_map(|r| r.about.clone()).collect();
    let (found, judged) = find_patterns(&games, &favourites, &stated, rules, now());
    let stored: Vec<PatternRow> = found
        .iter()
        .map(|f| PatternRow {
            key: f.key.clone(),
            sentence: f.sentence.clone(),
            evidence: serde_json::to_value(&f.evidence).unwrap_or(Value::Null),
            confidence: f.confidence,
        })
        .collect();
    let changes = db.taste_put_patterns(p, &stored)?;
    db.meta_set("taste_updated_at", &now().to_string())?;
    Ok(Update { patterns: found.len(), judged, changes })
}

/// The profile job, "Update your profile".
pub async fn run_job(db: &Db, p: PlayerId, rules: &TasteRules, sink: &dyn ProgressSink) -> Result<String> {
    sink.update(&Progress { total: 1, done: 0, current: None, message: "Counting what you finish and what you leave".into() });
    let u = update(db, p, rules)?;
    // The job's log is everyone's, so with two players each one's patterns
    // stay on their own profile page.
    let alone = db.players()?.len() == 1;
    for r in db.taste_statements(p)?.iter().filter(|r| alone && r.kind == "pattern") {
        let hidden = if r.hidden { " (hidden: you said not really)" } else { "" };
        sink.log(format!("{}{hidden}", r.note.as_deref().unwrap_or_default()));
    }
    sink.update(&Progress { total: 1, done: 1, current: None, message: "Done".into() });
    let games = |n: usize| if n == 1 { "1 game".to_string() } else { format!("{n} games") };
    let from = format!("{} you stuck with and {} you left", games(u.judged.kept), u.judged.left);
    Ok(match u.patterns {
        0 => format!("No patterns yet, from {from}"),
        1 => format!("1 pattern, from {from}"),
        n => format!("{n} patterns, from {from}"),
    })
}

/// "Update your profile" for every player, each with their own
/// rules; the summary names each player when there are two or more.
pub async fn run_for_everyone(db: &Db, cfg: &crate::config::Config, sink: &dyn ProgressSink) -> Result<String> {
    let players = db.players()?;
    let mut lines = vec![];
    for player in &players {
        let rules = cfg.as_player(db, player.id)?.taste_rules();
        let line = run_job(db, player.id, &rules, sink).await?;
        lines.push(if players.len() == 1 { line } else { format!("{}: {line}", player.name) });
    }
    Ok(lines.join("; "))
}

// ---------------------------------------------------------------------------
// What you say
// ---------------------------------------------------------------------------

/// The longest note kept with a statement, in characters.
const MAX_NOTE: usize = 300;
/// A favourite's reasons at most.
const MAX_WHY: usize = 16;

/// Habits you can state: (about, what the answer is).
pub const HABITS: &[(&str, Habit)] = &[
    ("weeknight", Habit::Minutes),
    ("weekend", Habit::Minutes),
    ("times", Habit::Choices(&["mornings", "afternoons", "evenings", "late_nights"])),
    ("who", Habit::Choices(&["solo", "partner", "friends", "kids"])),
    ("where", Habit::Choices(&["desk", "couch", "deck"])),
];

#[derive(Debug, Clone, Copy)]
pub enum Habit {
    /// How long a session usually is, in minutes.
    Minutes,
    /// Any of these.
    Choices(&'static [&'static str]),
}

/// A statement as sent over the API.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
pub struct StatementBody {
    pub kind: Option<String>,
    pub title_id: Option<i64>,
    pub about: Option<String>,
    pub why: Option<Value>,
    pub note: Option<String>,
}

fn clean_note(note: Option<&str>) -> Result<Option<String>> {
    let note = note.map(str::trim).filter(|n| !n.is_empty());
    if note.is_some_and(|n| n.chars().count() > MAX_NOTE) {
        bail!("a note is at most {MAX_NOTE} characters");
    }
    Ok(note.map(str::to_string))
}

/// A favourite's reasons: keys from `why_choices`, each once, any game's.
fn check_why(why: Option<&Value>) -> Result<Value> {
    let Some(why) = why.filter(|w| !w.is_null()) else { return Ok(json!([])) };
    let Some(list) = why.as_array() else { bail!("why is a list of reasons") };
    let mut out: Vec<String> = vec![];
    for item in list {
        let Some(k) = item.as_str().map(str::trim) else { bail!("each reason is a string") };
        let ok = match k.split_once(':') {
            Some(("aspect", a)) => vocab::aspect_why(a).is_some(),
            Some(("facet", f)) => crate::review_tags::facet_name(f).as_deref() == Some(f),
            Some(("tag", t)) => !t.trim().is_empty() && t.chars().count() <= 60,
            _ => false,
        };
        if !ok {
            bail!("{k:?} isn't a reason UpNext knows (aspect:…, facet:… or tag:…)");
        }
        if !out.iter().any(|x| x == k) {
            out.push(k.to_string());
        }
    }
    if out.len() > MAX_WHY {
        bail!("at most {MAX_WHY} reasons");
    }
    Ok(json!(out))
}

/// A habit's answer: minutes, or choices from its list.
fn check_habit(about: &str, value: Option<&Value>) -> Result<Option<Value>> {
    let Some((_, habit)) = HABITS.iter().find(|(a, _)| *a == about) else {
        let known: Vec<&str> = HABITS.iter().map(|(a, _)| *a).collect();
        bail!("a habit is one of {}; not {about:?}", known.join(", "));
    };
    let Some(value) = value.filter(|v| !v.is_null()) else { return Ok(None) };
    match habit {
        Habit::Minutes => match value.as_i64() {
            Some(m) if (0..=24 * 60).contains(&m) => Ok(Some(json!(m))),
            _ => bail!("{about} is a number of minutes, up to a day"),
        },
        Habit::Choices(choices) => {
            let Some(list) = value.as_array() else { bail!("{about} is a list of choices") };
            let mut out: Vec<&str> = vec![];
            for v in list {
                match v.as_str().and_then(|s| choices.iter().find(|c| **c == s)) {
                    Some(c) if !out.contains(c) => out.push(c),
                    Some(_) => {}
                    None => bail!("{about} is any of {}; not {v}", choices.join(", ")),
                }
            }
            // In the list's order.
            Ok(Some(json!(choices.iter().filter(|c| out.contains(c)).collect::<Vec<_>>())))
        }
    }
}

/// Check a new statement. Patterns can't be stated: the job finds them.
pub fn check_statement(db: &Db, p: PlayerId, b: &StatementBody) -> Result<NewStatement> {
    let kind = b.kind.as_deref().unwrap_or_default();
    let note = clean_note(b.note.as_deref())?;
    match kind {
        "favourite" => {
            let Some(id) = b.title_id else { bail!("a favourite needs the game's title_id") };
            if db.game(p, id)?.is_none() {
                bail!("game id {id} is not in your library");
            }
            Ok(NewStatement { kind: kind.into(), title_id: Some(id), about: None, why: Some(check_why(b.why.as_ref())?), note })
        }
        "like" | "dislike" | "hard_no" => {
            let about = b.about.as_deref().unwrap_or_default();
            let Some(subject) = Subject::parse(about) else {
                bail!("about is facet:<scale>:<value>, tag:<tag>, length:<short|medium|long> or text:<your words>; not {about:?}")
            };
            Ok(NewStatement { kind: kind.into(), title_id: None, about: Some(subject.key()), why: None, note })
        }
        "context" => {
            let about = b.about.as_deref().unwrap_or_default();
            let why = check_habit(about, b.why.as_ref())?;
            if why.is_none() && note.is_none() {
                bail!("give an answer or a note");
            }
            Ok(NewStatement { kind: kind.into(), title_id: None, about: Some(about.into()), why, note })
        }
        "pattern" => bail!("patterns come from how you play; say That's right or Not really to one instead"),
        other => bail!("kind is favourite, like, dislike, hard_no or context; not {other:?}"),
    }
}

/// A change to a statement, as sent over the API.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
pub struct EditBody {
    /// A like, dislike or hard no turned into another of those.
    pub kind: Option<String>,
    /// A favourite's reasons, or a habit's answer.
    pub why: Option<Value>,
    /// A new note; an empty one clears it.
    pub note: Option<String>,
    /// A pattern: "Not really" (true) or "Show again" (false).
    pub hidden: Option<bool>,
    /// A pattern: "That's right", which makes it a like or a dislike.
    #[serde(default)]
    pub confirm: bool,
}

/// What an edit did: the row as it is now, or the statement a confirmed
/// pattern became.
pub fn edit(db: &Db, p: PlayerId, rules: &TasteRules, id: i64, b: &EditBody) -> Result<Option<StatementRow>> {
    let Some(row) = db.taste_statement(p, id)? else { return Ok(None) };
    if row.kind == "pattern" {
        if b.kind.is_some() || b.why.is_some() || b.note.is_some() {
            bail!("a pattern can only be confirmed or hidden");
        }
        if b.confirm {
            let evidence: Evidence = row
                .evidence
                .clone()
                .and_then(|e| serde_json::from_value(e).ok())
                .ok_or_else(|| anyhow::anyhow!("this pattern has no evidence to go by"))?;
            let new = NewStatement {
                kind: evidence.sense.statement_kind().into(),
                title_id: None,
                about: Some(evidence.subject),
                why: None,
                note: None,
            };
            let made = db.taste_state(p, &new)?;
            db.taste_delete(p, id)?;
            update(db, p, rules)?;
            return db.taste_statement(p, made);
        }
        match b.hidden {
            Some(h) => db.taste_hide(p, id, h)?,
            None => bail!("say confirm or hidden"),
        }
        return db.taste_statement(p, id);
    }
    if b.hidden.is_some() || b.confirm {
        bail!("only patterns are confirmed or hidden");
    }
    let kind = match b.kind.as_deref() {
        None => None,
        Some(k) if row.kind == k => None,
        Some(k @ ("like" | "dislike" | "hard_no")) if matches!(row.kind.as_str(), "like" | "dislike" | "hard_no") => Some(k),
        Some(k) => bail!("a {} can't become a {k}", row.kind),
    };
    let why = match (row.kind.as_str(), &b.why) {
        (_, None) => None,
        ("favourite", Some(w)) => Some(check_why(Some(w))?),
        ("context", Some(w)) => Some(check_habit(row.about.as_deref().unwrap_or_default(), Some(w))?.unwrap_or(Value::Null)),
        (k, Some(_)) => bail!("a {k} has no reasons; change its note"),
    };
    let note = match &b.note {
        None => None,
        Some(n) => Some(clean_note(Some(n))?),
    };
    db.taste_edit(p, id, kind, why.as_ref(), note.as_ref().map(|n| n.as_deref()))?;
    update(db, p, rules)?;
    db.taste_statement(p, id)
}

/// Delete something you said. Patterns aren't deleted: "Not really" hides one.
pub fn delete(db: &Db, p: PlayerId, rules: &TasteRules, id: i64) -> Result<Option<()>> {
    let Some(row) = db.taste_statement(p, id)? else { return Ok(None) };
    if row.kind == "pattern" {
        bail!("a pattern comes back while the evidence is there; say Not really to hide it");
    }
    db.taste_delete(p, id)?;
    update(db, p, rules)?;
    Ok(Some(()))
}

// ---------------------------------------------------------------------------
// The page
// ---------------------------------------------------------------------------

/// A reason, or a choice of one, with how the page says it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Choice {
    pub key: String,
    pub label: String,
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) if !s.chars().nth(1).is_some_and(char::is_uppercase) => f.to_lowercase().chain(c).collect(),
        _ => s.to_string(),
    }
}

/// How a favourite's reason reads: "the story", "tense", "Open World".
pub fn why_label(key: &str) -> String {
    match key.split_once(':') {
        Some(("aspect", a)) => vocab::aspect_why(a).unwrap_or(a).to_string(),
        Some(("facet", f)) => crate::review_tags::facet_label(f).map(lower_first).unwrap_or_else(|| f.to_string()),
        Some(("tag", t)) => t.to_string(),
        _ => key.to_string(),
    }
}

/// What a game can be a favourite for: what players praise about it, its
/// review-tag values, then its strongest tags.
pub fn why_choices(g: &GameView) -> Vec<Choice> {
    let mut keys: Vec<String> = vec![];
    if let Some(t) = &g.review_tags {
        for a in t.aspects.iter().filter(|a| a.verdict == crate::review_tags::Verdict::Praised) {
            if let Some(name) = crate::review_tags::aspect_name(&a.aspect)
                && vocab::aspect_why(name).is_some()
            {
                keys.push(format!("aspect:{name}"));
            }
        }
        keys.extend(t.mood.iter().map(|m| format!("facet:mood:{m}")));
        for s in crate::review_tags::SCALES {
            if let Some(v) = t.scales.get(s.name) {
                keys.push(format!("facet:{}:{v}", s.name));
            }
        }
        keys.extend(t.skills.iter().map(|s| format!("facet:skills:{s}")));
    }
    keys.extend(store_tags(g).iter().take(6).map(|t| format!("tag:{t}")));
    let mut seen = HashSet::new();
    keys.retain(|k| seen.insert(k.clone()));
    keys.truncate(MAX_WHY);
    keys.into_iter().map(|key| Choice { label: why_label(&key), key }).collect()
}

/// Your played games, most played first with recent play weighted up.
pub fn most_played(games: &[GameView], now: i64) -> Vec<&GameView> {
    let score = |g: &GameView| {
        let years = if g.last_played > 0 { (now - g.last_played).max(0) as f64 / (365.0 * DAY) } else { 5.0 };
        g.playtime_minutes as f64 * (0.5 + 0.5 * 0.5f64.powf(years))
    };
    let mut list: Vec<&GameView> =
        games.iter().filter(|g| g.playtime_minutes > 0 && !matches!(g.status, Status::NotGame | Status::Hidden)).collect();
    list.sort_by(|a, b| score(b).partial_cmp(&score(a)).unwrap_or(std::cmp::Ordering::Equal).then(a.name.cmp(&b.name)));
    list
}

/// About 12 of your most-played games, recent play weighted up, that
/// aren't favourites yet: where choosing favourites starts.
pub fn suggestions<'a>(games: &'a [GameView], favourites: &HashSet<i64>, now: i64) -> Vec<&'a GameView> {
    most_played(games, now).into_iter().filter(|g| !favourites.contains(&g.id)).take(12).collect()
}

/// Something a like, dislike or hard no can be about, with how many games in
/// the library it matches.
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
pub struct SubjectChoice {
    pub about: String,
    pub label: String,
    /// "Mood", "Grind", "Genres and themes", "Length".
    pub group: String,
    pub count: usize,
}

/// Everything a like or dislike can be about, by group.
pub fn subject_choices(games: &[GameView]) -> Vec<SubjectChoice> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for g in games.iter().filter(|g| g.status != Status::NotGame) {
        for s in features(g) {
            *counts.entry(s.key()).or_default() += 1;
        }
    }
    let group_of = |facet: &str| -> String {
        let name = facet.split(':').next().unwrap_or_default();
        crate::review_tags::SCALES
            .iter()
            .find(|s| s.name == name)
            .map(|s| s.title)
            .or_else(|| crate::review_tags::SETS.iter().find(|s| s.name == name).map(|s| s.title))
            .unwrap_or(name)
            .to_string()
    };
    let subjects = vocab::FACETS
        .iter()
        .map(|(f, _)| (Subject::Facet(f.to_string()), group_of(f)))
        .chain(vocab::TAGS.iter().map(|(t, _)| (Subject::Tag(t.to_string()), "Genres and themes".to_string())))
        .chain(vocab::LENGTHS.iter().map(|(k, ..)| (Subject::Length(k), "Length".to_string())));
    subjects
        .map(|(s, group)| SubjectChoice { about: s.key(), label: s.label(), group, count: counts.get(&s.key()).copied().unwrap_or(0) })
        .collect()
}

/// A favourite on the page.
#[derive(Debug, Clone, Serialize)]
pub struct FavouriteView {
    pub id: i64,
    pub game: GameView,
    pub why: Vec<Choice>,
    /// What else it could be a favourite for.
    pub choices: Vec<Choice>,
    pub note: Option<String>,
    pub updated_at: i64,
}

/// A like, dislike or hard no on the page.
#[derive(Debug, Clone, Serialize)]
pub struct StatementView {
    pub id: i64,
    /// like, dislike or hard_no.
    pub kind: String,
    pub about: String,
    pub label: String,
    pub note: Option<String>,
    /// Games in the library it's about; None for your own words.
    pub matches: Option<usize>,
    pub updated_at: i64,
}

/// A habit you stated.
#[derive(Debug, Clone, Serialize)]
pub struct HabitView {
    pub id: i64,
    pub about: String,
    pub value: Option<Value>,
    pub note: Option<String>,
    pub updated_at: i64,
}

/// A pattern on the page.
#[derive(Debug, Clone, Serialize)]
pub struct PatternView {
    pub id: i64,
    pub key: String,
    pub sentence: String,
    pub sense: Option<Sense>,
    pub summary: String,
    pub confidence: f64,
    pub sure: bool,
    pub games: Vec<EvidenceGame>,
    pub hidden: bool,
    /// When it was first found.
    pub since: i64,
}

/// `GET /api/taste`.
#[derive(Debug, Clone, Serialize)]
pub struct Page {
    pub favourites: Vec<FavouriteView>,
    pub statements: Vec<StatementView>,
    pub habits: Vec<HabitView>,
    pub patterns: Vec<PatternView>,
    pub hidden_patterns: Vec<PatternView>,
    /// When the patterns were last counted.
    pub updated_at: Option<i64>,
    pub judged: Judged,
    pub suggestions: Vec<GameView>,
    pub subjects: Vec<SubjectChoice>,
    /// What the play diary shows about your habits, from its last
    /// `diary::WINDOW_DAYS` days; empty when it holds too little.
    pub diary: diary::Habits,
}

pub fn page(db: &Db, p: PlayerId) -> Result<Page> {
    let games = db.all_games_shared(p)?;
    let by_id: HashMap<i64, &GameView> = games.iter().map(|g| (g.id, g)).collect();
    let rows = db.taste_statements(p)?;
    let favourite_ids: HashSet<i64> = rows.iter().filter(|r| r.kind == "favourite").filter_map(|r| r.title_id).collect();
    let mut favourites: Vec<FavouriteView> = vec![];
    let mut statements: Vec<StatementView> = vec![];
    let mut habits: Vec<HabitView> = vec![];
    let mut patterns: Vec<PatternView> = vec![];
    for r in &rows {
        match r.kind.as_str() {
            "favourite" => {
                let Some(g) = r.title_id.and_then(|id| by_id.get(&id)) else { continue };
                let why: Vec<String> = r.why.as_ref().and_then(|w| serde_json::from_value(w.clone()).ok()).unwrap_or_default();
                favourites.push(FavouriteView {
                    id: r.id,
                    game: g.slim(),
                    why: why.iter().map(|k| Choice { key: k.clone(), label: why_label(k) }).collect(),
                    choices: why_choices(g),
                    note: r.note.clone(),
                    updated_at: r.updated_at,
                });
            }
            "like" | "dislike" | "hard_no" => {
                let about = r.about.clone().unwrap_or_default();
                let subject = Subject::parse(&about);
                statements.push(StatementView {
                    id: r.id,
                    kind: r.kind.clone(),
                    label: subject.as_ref().map(Subject::label).unwrap_or_else(|| about.clone()),
                    matches: subject.filter(|s| !matches!(s, Subject::Text(_))).map(|s| {
                        let about = |g: &GameView| if r.kind == "hard_no" { hard_no_matches(&s, g) } else { matches(&s, g) };
                        games.iter().filter(|g| g.status != Status::NotGame && about(g)).count()
                    }),
                    about,
                    note: r.note.clone(),
                    updated_at: r.updated_at,
                });
            }
            "context" => habits.push(HabitView {
                id: r.id,
                about: r.about.clone().unwrap_or_default(),
                value: r.why.clone(),
                note: r.note.clone(),
                updated_at: r.updated_at,
            }),
            "pattern" => {
                let evidence: Option<Evidence> = r.evidence.clone().and_then(|e| serde_json::from_value(e).ok());
                let confidence = r.confidence.unwrap_or(0.0);
                patterns.push(PatternView {
                    id: r.id,
                    key: r.about.clone().unwrap_or_default(),
                    sentence: r.note.clone().unwrap_or_default(),
                    sense: evidence.as_ref().map(|e| e.sense),
                    summary: evidence.as_ref().map(|e| e.summary.clone()).unwrap_or_default(),
                    confidence,
                    sure: confidence >= SURE,
                    games: evidence.map(|e| e.games).unwrap_or_default(),
                    hidden: r.hidden,
                    since: r.created_at,
                });
            }
            _ => {}
        }
    }
    favourites.sort_by_key(|f| std::cmp::Reverse(f.game.playtime_minutes));
    statements.sort_by_key(|a| a.label.to_lowercase());
    patterns.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
    let (hidden_patterns, patterns): (Vec<PatternView>, Vec<PatternView>) = patterns.into_iter().partition(|p| p.hidden);
    let judged = {
        let mut j = Judged::default();
        for g in games.iter() {
            if let Some((o, _)) = judge(g, favourite_ids.contains(&g.id)) {
                if o.kept() { j.kept += 1 } else { j.left += 1 }
            }
        }
        j
    };
    Ok(Page {
        favourites,
        statements,
        habits,
        patterns,
        hidden_patterns,
        updated_at: db.meta_get("taste_updated_at")?.and_then(|v| v.parse().ok()),
        judged,
        suggestions: suggestions(&games, &favourite_ids, now()).into_iter().map(GameView::slim).collect(),
        subjects: subject_choices(&games),
        diary: diary::load_habits(db, p, &games, now())?,
    })
}

// ---------------------------------------------------------------------------
// What UpNext knows about you
// ---------------------------------------------------------------------------

/// The sources of the profile, `GET /api/taste/sources`.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct Sources {
    pub library: LibrarySource,
    pub statuses: StatusesSource,
    pub reactions: ReactionCounts,
    pub answers: AnswersSource,
    pub diary: diary::State,
    pub wishlist: WishlistSource,
    pub ratings: RatingsSource,
}

/// Your own ratings of games, 1 to 5 stars.
#[derive(Debug, Clone, Default, Serialize, utoipa::ToSchema)]
pub struct RatingsSource {
    pub games: usize,
    /// By stars, "1" to "5".
    pub by: BTreeMap<String, usize>,
}

/// The wishlist: your Steam wishlist's copy, what you added and
/// answered, and the suggestions waiting.
#[derive(Debug, Clone, Default, Serialize, utoipa::ToSchema)]
pub struct WishlistSource {
    /// The `wishlist` setting.
    pub on: bool,
    /// Games on your Steam wishlist, as last read.
    pub steam: usize,
    /// Games you put on the wishlist here.
    pub added: usize,
    pub suggested: usize,
    /// Not interested, or owned elsewhere.
    pub answered: usize,
    /// In your library now, remembered for "From your wishlist".
    pub bought: usize,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct LibrarySource {
    pub games: usize,
    /// Games a store reports any playtime for.
    pub played: usize,
    pub hours: i64,
    /// The stores your games come from, by source id.
    pub stores: Vec<String>,
    pub last_sync: Option<i64>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct StatusesSource {
    pub games: usize,
    /// By status you set.
    pub by: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Default, Serialize, utoipa::ToSchema)]
pub struct AnswersSource {
    pub favourites: usize,
    pub likes: usize,
    pub dislikes: usize,
    pub hard_nos: usize,
    pub habits: usize,
    /// Named routines.
    pub routines: usize,
    /// Patterns you said weren't right.
    pub not_really: usize,
}

pub fn sources(db: &Db, cfg: &crate::config::Config) -> Result<Sources> {
    let games = db.all_games_shared(cfg.whose())?;
    let mut stores: Vec<String> = vec![];
    for g in games.iter() {
        for s in &g.owned_on {
            if !stores.contains(s) {
                stores.push(s.clone());
            }
        }
    }
    stores.sort_by_key(|s| crate::db::SOURCES.iter().position(|x| x == s).unwrap_or(usize::MAX));
    let mut by: BTreeMap<String, usize> = BTreeMap::new();
    for g in games.iter() {
        if let Some(o) = g.override_ {
            *by.entry(o.as_str().to_string()).or_default() += 1;
        }
    }
    let rows = db.taste_statements(cfg.whose())?;
    let count = |kind: &str| rows.iter().filter(|r| r.kind == kind && r.source == "stated").count();
    Ok(Sources {
        library: LibrarySource {
            games: games.len(),
            played: games.iter().filter(|g| g.playtime_known && g.playtime_minutes > 0).count(),
            hours: games.iter().map(|g| g.playtime_minutes).sum::<i64>() / 60,
            stores,
            last_sync: db.meta_get("last_sync")?.and_then(|v| v.parse().ok()),
        },
        statuses: StatusesSource { games: by.values().sum(), by },
        reactions: db.taste_reaction_counts(cfg.whose())?,
        answers: AnswersSource {
            favourites: count("favourite"),
            likes: count("like"),
            dislikes: count("dislike"),
            hard_nos: count("hard_no"),
            habits: count("context"),
            routines: db.routines(cfg.whose())?.len(),
            not_really: rows.iter().filter(|r| r.kind == "pattern" && r.hidden).count(),
        },
        diary: diary::state(db, cfg)?,
        wishlist: wishlist_source(db, cfg)?,
        ratings: ratings_source(db, cfg.whose())?,
    })
}

fn ratings_source(db: &Db, p: PlayerId) -> Result<RatingsSource> {
    let rows = db.game_ratings(p)?;
    let mut by: BTreeMap<String, usize> = BTreeMap::new();
    for r in &rows {
        *by.entry(r.stars.to_string()).or_default() += 1;
    }
    Ok(RatingsSource { games: rows.len(), by })
}

fn wishlist_source(db: &Db, cfg: &crate::config::Config) -> Result<WishlistSource> {
    use crate::db::WishState;
    let rows = db.wishlist_rows(cfg.whose())?;
    let count = |f: &dyn Fn(&crate::db::WishRow) -> bool| rows.iter().filter(|r| f(r)).count();
    Ok(WishlistSource {
        on: cfg.wishlist,
        steam: count(&|r| r.on_steam && r.state == WishState::Wishlisted),
        added: count(&|r| r.state == WishState::Wishlisted && !r.on_steam),
        suggested: count(&|r| r.state == WishState::Suggested),
        answered: count(&|r| matches!(r.state, WishState::Dismissed | WishState::OwnedElsewhere)),
        bought: count(&|r| r.state == WishState::InLibrary && r.wishlisted_at.is_some()),
    })
}

/// The rows behind a source, for "See it": reactions, the play diary or
/// your ratings of games, newest first; the others have pages of their own
/// (Library, Lists) or are this page.
pub fn source_rows(db: &Db, p: PlayerId, source: &str) -> Result<Option<Value>> {
    Ok(match source {
        "reactions" => Some(serde_json::to_value(db.taste_reactions(p)?)?),
        "diary" => {
            let mut rows = db.diary_rows(p, 0)?;
            rows.reverse();
            Some(serde_json::to_value(rows)?)
        }
        "ratings" => Some(serde_json::to_value(db.game_ratings(p)?)?),
        _ => None,
    })
}

/// Delete one source. Returns how many rows went, or an error saying why
/// it can't be deleted here.
pub fn delete_source(db: &Db, p: PlayerId, rules: &TasteRules, source: &str) -> Result<usize> {
    let n = match source {
        "library" => bail!("your library comes from your stores at each sync; disconnect a store in Settings › Accounts to remove it"),
        "statuses" => db.taste_delete_marks(p)?,
        "reactions" => {
            let d = db.taste_delete_reactions(p)?;
            d.picks + d.stop_reasons + d.second_chance_answers
        }
        "answers" => db.taste_delete_answers(p)?,
        "diary" => db.diary_wipe(p)?,
        "wishlist" => db.wishlist_wipe(p)?,
        "ratings" => db.delete_game_ratings(p)?,
        other => bail!("a source is library, statuses, reactions, answers, diary, wishlist or ratings; not {other:?}"),
    };
    update(db, p, rules)?;
    Ok(n)
}

/// The whole profile as JSON, `GET /api/taste/export`: what you said, the
/// patterns with their evidence, and every reaction and status they read.
pub fn export(db: &Db, cfg: &crate::config::Config) -> Result<Value> {
    let games = db.all_games_shared(cfg.whose())?;
    let name: HashMap<i64, &str> = games.iter().map(|g| (g.id, g.name.as_str())).collect();
    let rows = db.taste_statements(cfg.whose())?;
    let statements: Vec<Value> = rows
        .iter()
        .map(|r| {
            let mut v = serde_json::to_value(r).unwrap_or(Value::Null);
            if let Some(id) = r.title_id {
                v["game"] = json!(name.get(&id));
            }
            v
        })
        .collect();
    let statuses: Vec<Value> = games
        .iter()
        .filter_map(|g| g.override_.map(|o| json!({ "title_id": g.id, "game": g.name, "status": o.as_str(), "at": g.override_at })))
        .collect();
    Ok(json!({
        "exported_at": now(),
        "statements": statements,
        "statuses": statuses,
        "reactions": db.taste_reactions(cfg.whose())?,
        "play_diary": db.diary_rows(cfg.whose(), 0)?,
        "ratings": db.game_ratings(cfg.whose())?,
        "routines": db.routines(cfg.whose())?,
        "sources": sources(db, cfg)?,
    }))
}

#[cfg(test)]
mod tests;
