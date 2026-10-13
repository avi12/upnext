//! New to you: a gentle way into a genre you don't usually play,
//! or a mechanic you haven't tried, from your own library.
//!
//! - **A genre you don't usually play** has under `unfamiliar_share` percent
//!   of your played hours and fewer than 2 played games.
//! - **A mechanic you haven't tried** is on none of the games you've played
//!   or sampled.
//!
//! A game is a way into one when it's among its store's five strongest tags
//! or its store genres, as far as a hard no reaches (`taste::HARD_NO_TAGS`).
//! Never a hard no, a genre or mechanic you dislike, or a game picks don't
//! offer. The guardrails keep it gentle: not hard, not a steep learning
//! curve or deep systems, and short (`new_to_you_max_hours`, or short
//! sessions when HowLongToBeat has no time; an endless game needs an easy
//! learning curve instead, or, when reviews don't say, simple systems or a
//! Casual or Relaxing tag).

use super::Member;
use crate::config::Config;
use crate::db::{GameView, Status};
use crate::lifecycle::{Kind, Offer};
use crate::taste::{HARD_NO_TAGS, Profile, Subject};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// Genres, as Steam's tags and store genres name them.
pub const GENRES: &[&str] = &[
    "Action",
    "Adventure",
    "Action-Adventure",
    "RPG",
    "Action RPG",
    "JRPG",
    "CRPG",
    "Strategy",
    "Turn-Based Strategy",
    "Grand Strategy",
    "RTS",
    "Tower Defense",
    "Management",
    "Simulation",
    "Farming Sim",
    "Life Sim",
    "Flight",
    "Driving",
    "Racing",
    "Sports",
    "Survival",
    "Sandbox",
    "Platformer",
    "Puzzle",
    "Horror",
    "Survival Horror",
    "Psychological Horror",
    "Stealth",
    "FPS",
    "Shooter",
    "Twin Stick Shooter",
    "Hack and Slash",
    "Beat 'em up",
    "Fighting",
    "Card Game",
    "Point & Click",
    "Walking Simulator",
    "Detective",
    "Mystery",
    "Arcade",
];

/// Mechanics: the kinds of play it looks for (deckbuilding, roguelike,
/// city builder, rhythm, puzzle platformer, immersive sim, metroidvania,
/// tactics, visual novel, colony sim, 4X, survival crafting and so on).
pub const MECHANICS: &[&str] = &[
    "Deckbuilding",
    "Roguelike",
    "Roguelite",
    "City Builder",
    "Colony Sim",
    "Automation",
    "Base Building",
    "Crafting",
    "Open World Survival Craft",
    "4X",
    "Turn-Based Tactics",
    "Rhythm",
    "Puzzle Platformer",
    "Precision Platformer",
    "Immersive Sim",
    "Metroidvania",
    "Souls-like",
    "Bullet Hell",
    "Visual Novel",
    "Dating Sim",
    "Looter Shooter",
];

/// Tags (or store genres) that say a game is easy going, when its reviews
/// don't say how hard it is or how easy it is to learn.
const EASY_TAGS: &[&str] = &["Casual", "Relaxing"];
/// Tags that say it isn't.
const HARD_TAGS: &[&str] = &["Difficult", "Souls-like", "Precision Platformer"];
/// Store tags read for what you've played, as patterns read them.
const PLAYED_TAGS: usize = 10;
/// Played games a genre needs before it's familiar.
const FAMILIAR_GAMES: usize = 2;

/// What New to you follows. The defaults match `config::SETTINGS`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rules {
    /// The longest main story, in hours.
    pub max_hours: f64,
    /// A genre is unfamiliar under this share of your played hours (0.02 is 2%).
    pub share: f64,
    /// Played from this many minutes; less is sampled.
    pub sampled_minutes: i64,
}

impl Rules {
    pub fn from_config(cfg: &Config) -> Self {
        Rules {
            max_hours: cfg.new_to_you_max_hours as f64,
            share: cfg.unfamiliar_share as f64 / 100.0,
            sampled_minutes: cfg.sampled_minutes,
        }
    }
}

impl Default for Rules {
    fn default() -> Self {
        Rules { max_hours: 8.0, share: 0.02, sampled_minutes: 120 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WayKind {
    Genre,
    Mechanic,
}

/// A genre or mechanic a game is a way into.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WayIn {
    /// The tag, as `taste::vocab` spells it: "Deckbuilding".
    pub tag: String,
    /// As a sentence says it: "deckbuilders".
    pub phrase: &'static str,
    pub kind: WayKind,
}

impl WayIn {
    /// The profile's subject for it: "tag:Deckbuilding", for a hard no or a like.
    pub fn about(&self) -> String {
        Subject::Tag(self.tag.clone()).key()
    }

    /// On a candidate line: "deckbuilders (a mechanic you haven't tried)".
    pub fn described(&self) -> String {
        match self.kind {
            WayKind::Mechanic => format!("{} (a mechanic you haven't tried)", self.phrase),
            WayKind::Genre => format!("{} (a genre you rarely play)", self.phrase),
        }
    }
}

/// A listed tag's own spelling and phrase, whatever the case or punctuation.
fn known(tag: &str) -> Option<(String, &'static str)> {
    match Subject::parse(&format!("tag:{tag}"))? {
        Subject::Tag(name) => {
            let phrase = Subject::Tag(name.clone()).phrase()?;
            Some((name, phrase))
        }
        _ => None,
    }
}

fn kind_of(tag: &str) -> Option<WayKind> {
    if MECHANICS.contains(&tag) {
        Some(WayKind::Mechanic)
    } else if GENRES.contains(&tag) {
        Some(WayKind::Genre)
    } else {
        None
    }
}

/// The store's own tags, strongest first: Steam's, or a GOG copy's.
fn own_tags(g: &GameView) -> &[String] {
    ["steam", "gog"].iter().find_map(|s| g.tags_by_source.get(*s).filter(|t| !t.is_empty())).map(Vec::as_slice).unwrap_or(&[])
}

/// The genres and mechanics a game has: in its strongest `n` store tags or
/// its store genres, each once, as `taste::vocab` spells them.
fn kinds_of_play(g: &GameView, n: usize) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for t in own_tags(g).iter().take(n).chain(&g.genres) {
        if let Some((name, _)) = known(t)
            && kind_of(&name).is_some()
            && !out.contains(&name)
        {
            out.push(name);
        }
    }
    out
}

/// Played past sampling, or marked in a way that says you played it.
fn played(g: &GameView, rules: &Rules) -> bool {
    matches!(
        g.status,
        Status::Playing | Status::Shelved | Status::Played | Status::Done | Status::StillPlaying | Status::BurntOut | Status::Abandoned
    ) || (g.playtime_known && g.playtime_minutes >= rules.sampled_minutes)
}

/// What you've played, by genre and mechanic.
#[derive(Debug, Default)]
pub struct Familiar {
    hours: HashMap<String, f64>,
    games: HashMap<String, usize>,
    /// Genres and mechanics on any game you've played or sampled.
    tried: HashSet<String>,
    total_hours: f64,
}

impl Familiar {
    pub fn of<'a>(games: impl IntoIterator<Item = &'a GameView>, rules: &Rules) -> Familiar {
        let mut f = Familiar::default();
        for g in games {
            let played = played(g, rules);
            let tried = played || g.status == Status::Sampled || g.playtime_minutes > 0;
            if !tried {
                continue;
            }
            let hours = g.playtime_minutes.max(0) as f64 / 60.0;
            f.total_hours += hours;
            for k in kinds_of_play(g, PLAYED_TAGS) {
                *f.hours.entry(k.clone()).or_default() += hours;
                if played {
                    *f.games.entry(k.clone()).or_default() += 1;
                }
                f.tried.insert(k);
            }
        }
        f
    }

    /// Whether a genre or mechanic is new to you.
    pub fn is_new(&self, tag: &str, rules: &Rules) -> bool {
        match kind_of(tag) {
            Some(WayKind::Mechanic) => !self.tried.contains(tag),
            Some(WayKind::Genre) => {
                let hours = self.hours.get(tag).copied().unwrap_or(0.0);
                let few = self.games.get(tag).copied().unwrap_or(0) < FAMILIAR_GAMES;
                few && (self.total_hours <= 0.0 || hours / self.total_hours < rules.share)
            }
            None => false,
        }
    }
}

/// The genres and mechanics a game is a way into: new to you, and neither
/// disliked nor a hard no. Mechanics first, as they say more.
pub fn ways_in(g: &GameView, familiar: &Familiar, profile: Option<&Profile>, rules: &Rules) -> Vec<WayIn> {
    // A hard no or dislike on Horror covers Survival Horror too.
    let key = |t: &str| -> String { t.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect() };
    let refused = |tag: &str| {
        profile.is_some_and(|p| {
            p.dislikes.iter().chain(&p.hard_nos).any(|s| matches!(&s.subject, Subject::Tag(t) if key(tag).contains(&key(t))))
        })
    };
    let mut ways: Vec<WayIn> = kinds_of_play(g, HARD_NO_TAGS)
        .into_iter()
        .filter(|t| familiar.is_new(t, rules) && !refused(t))
        .filter_map(|t| {
            let (tag, phrase) = known(&t)?;
            Some(WayIn { kind: kind_of(&tag)?, tag, phrase })
        })
        .collect();
    ways.sort_by_key(|w| w.kind != WayKind::Mechanic);
    ways
}

/// Why a game isn't a gentle way in, or None when it is.
pub fn too_much(g: &GameView, rules: &Rules) -> Option<&'static str> {
    let scale = |name: &str| g.review_tags.as_ref().and_then(|t| t.scales.get(name)).map(String::as_str);
    // Steam's store genre Casual counts with its tags.
    let tagged =
        |list: &[&str]| own_tags(g).iter().take(PLAYED_TAGS).chain(&g.genres).any(|t| list.iter().any(|x| t.eq_ignore_ascii_case(x)));
    match scale("difficulty") {
        Some("hard" | "punishing") => return Some("players say it's hard"),
        Some(_) => {}
        // When reviews don't say, the tags decide: Casual or Relaxing for, Difficult or Souls-like against.
        None if tagged(HARD_TAGS) && !tagged(EASY_TAGS) => return Some("tagged as difficult"),
        None => {}
    }
    if scale("learning") == Some("steep") {
        return Some("a steep learning curve");
    }
    if scale("complexity") == Some("deep") {
        return Some("deep systems");
    }
    if g.kind == Kind::Endless {
        // No ending to be short: easy to learn instead. Reviews say so for
        // few games, so simple systems or a Casual or Relaxing tag do too.
        let easy = match scale("learning") {
            Some(l) => l == "gentle",
            None => scale("complexity") == Some("simple") || tagged(EASY_TAGS),
        };
        return (!easy).then_some("endless, and not known to be easy to learn");
    }
    match g.hltb_main.filter(|h| *h > 0.0) {
        Some(h) if h <= rules.max_hours => None,
        Some(_) => Some("longer than a short main story"),
        None if g.review_tags.as_ref().is_some_and(|t| t.has("sessions:short")) => None,
        None => Some("its length isn't known"),
    }
}

/// The games New to you may pick: offered by picks and not played yet, not
/// a hard no, gentle, and a way into something new to you.
pub fn members(games: &[GameView], profile: Option<&Profile>, rules: &Rules) -> HashMap<i64, Member> {
    let familiar = Familiar::of(games, rules);
    let mut out = HashMap::new();
    for g in games {
        if g.offer() != Some(Offer::New) || profile.is_some_and(|p| p.hard_no(g).is_some()) || too_much(g, rules).is_some() {
            continue;
        }
        let ways = ways_in(g, &familiar, profile, rules);
        if ways.is_empty() {
            continue;
        }
        let described: Vec<String> = ways.iter().map(WayIn::described).collect();
        let phrases: Vec<String> = ways.iter().map(|w| w.phrase.to_string()).collect();
        out.insert(
            g.id,
            Member {
                note: format!("new to you: {}", described.join(", ")),
                reason: format!("New to you: {}", phrases.join(", ")),
                ways: phrases,
                points: 0.0,
            },
        );
    }
    out
}

/// A game's ways in now, guardrails and status aside: what "Not for me" on
/// a New to you pick offers to make a hard no.
pub fn ways_for(games: &[GameView], id: i64, profile: Option<&Profile>, rules: &Rules) -> Vec<WayIn> {
    let Some(g) = games.iter().find(|g| g.id == id) else { return vec![] };
    // The game itself doesn't count towards what you know.
    ways_in(g, &Familiar::of(games.iter().filter(|x| x.id != id), rules), profile, rules)
}

#[cfg(test)]
mod tests;
