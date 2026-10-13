//! Review tags: what players say a game is like, drawn from their reviews by
//! the same model call that writes the review digest (`digest.rs`).
//!
//! The vocabulary is fixed and lives only here: the prompt, the reply schema,
//! validation and the plain labels are all built from these tables. Fixed
//! values are what make filters, pre-ranking and the picks prompt dependable.
//! Changing a table changes the prompt, so bump `digest::DIGEST_VERSION`.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// An ordered scale: at most one value per game, lowest first.
pub struct Scale {
    pub name: &'static str,
    /// Its name on the game page, "Learning curve".
    pub title: &'static str,
    /// What the scale measures, for the prompt.
    pub means: &'static str,
    /// Each value with its plain label for the merged tag list.
    pub values: &'static [(&'static str, &'static str)],
}

/// An unordered set: up to `max` values per game.
pub struct Set {
    pub name: &'static str,
    pub title: &'static str,
    pub means: &'static str,
    pub max: usize,
    pub values: &'static [(&'static str, &'static str)],
}

pub const SCALES: &[Scale] = &[
    Scale {
        name: "difficulty",
        title: "Difficulty",
        means: "how hard players say it is, not what the store says",
        values: &[("easy", "Easy"), ("moderate", "Moderate difficulty"), ("hard", "Hard"), ("punishing", "Punishing")],
    },
    Scale {
        name: "learning",
        title: "Learning curve",
        means: "the learning curve",
        values: &[("gentle", "Easy to learn"), ("moderate", "Some learning curve"), ("steep", "Steep learning curve")],
    },
    Scale {
        name: "hook",
        title: "Hooks you",
        means: "how soon it grabs you: within the first hour, after a few hours, or only after many hours",
        values: &[("fast", "Hooks you fast"), ("gradual", "Grows on you"), ("slow", "Slow to hook")],
    },
    Scale {
        name: "pace",
        title: "Pace",
        means: "the tempo of play",
        values: &[("relaxed", "Relaxed pace"), ("steady", "Steady pace"), ("intense", "Intense")],
    },
    Scale {
        name: "sessions",
        title: "Sessions",
        means: "the shortest session that feels worthwhile: under an hour, one to two hours, or longer sittings",
        values: &[("short", "Short sessions"), ("medium", "Medium sessions"), ("long", "Long sittings")],
    },
    Scale {
        name: "focus",
        title: "Focus",
        means: "the attention it demands; low means players happily play it with a podcast on",
        values: &[("low", "Low focus"), ("medium", "Medium focus"), ("high", "Needs full focus")],
    },
    Scale {
        name: "complexity",
        title: "Complexity",
        means: "how many systems it asks you to juggle",
        values: &[("simple", "Simple"), ("moderate", "Moderately complex"), ("deep", "Deep systems")],
    },
    Scale {
        name: "structure",
        title: "Structure",
        means: "a linear path, hubs you set out from, or an open world or open-ended play",
        values: &[("linear", "Linear"), ("hubs", "Hub-based"), ("open", "Open-ended")],
    },
    Scale {
        name: "grind",
        title: "Grind",
        means: "how much repetition progress takes",
        values: &[("none", "No grind"), ("light", "Light grind"), ("heavy", "Heavy grind")],
    },
    Scale {
        name: "story",
        title: "Story",
        means: "how much the game is about its story",
        values: &[("none", "No story"), ("light", "Light story"), ("central", "Story-driven")],
    },
    Scale {
        name: "emotion",
        title: "Emotional depth",
        means: "emotional depth: how much it moves players",
        values: &[("light", "Emotionally light"), ("moving", "Emotionally moving"), ("powerful", "Emotionally powerful")],
    },
    Scale {
        name: "replay",
        title: "Replay value",
        means: "replay value",
        values: &[("low", "Little replay value"), ("high", "High replay value")],
    },
];

pub const MOOD: Set = Set {
    name: "mood",
    title: "Mood",
    means: "the feel of playing it",
    max: 2,
    values: &[
        ("relaxing", "Relaxing"),
        ("cozy", "Cozy"),
        ("uplifting", "Uplifting"),
        ("funny", "Funny"),
        ("chaotic", "Chaotic"),
        ("mysterious", "Mysterious"),
        ("tense", "Tense"),
        ("scary", "Scary"),
        ("dark", "Dark"),
        ("melancholic", "Melancholic"),
        ("epic", "Epic"),
    ],
};

pub const SKILLS: Set = Set {
    name: "skills",
    title: "Asks for",
    means: "what it asks of the player",
    max: 3,
    values: &[
        ("reflexes", "Tests reflexes"),
        ("precision", "Tests precision"),
        ("timing", "Tests timing"),
        ("strategy", "Rewards strategy"),
        ("logic", "Rewards logic"),
        ("patience", "Rewards patience"),
        ("memorization", "Rewards memorization"),
        ("observation", "Rewards observation"),
        ("management", "Resource management"),
        ("creativity", "Rewards creativity"),
        ("teamwork", "Needs teamwork"),
    ],
};

pub const SETS: [&Set; 2] = [&MOOD, &SKILLS];

/// What players judge, each praised, criticized or divisive.
pub const ASPECTS: &[&str] = &[
    "combat",
    "story",
    "writing",
    "characters",
    "choices",
    "world",
    "atmosphere",
    "exploration",
    "puzzles",
    "level design",
    "core loop",
    "progression",
    "grind",
    "pacing",
    "difficulty balance",
    "controls",
    "camera",
    "performance",
    "bugs",
    "UI",
    "music",
    "sound",
    "voice acting",
    "art",
    "length",
    "ending",
    "endgame",
    "content",
    "co-op",
    "multiplayer",
    "mods",
    "value",
    "monetization",
];

/// Scales a title gets at most, the best-supported first. Without a cap
/// GPT-6-Luna set about 7 a title where Sonnet 5.5 set 3.5, and
/// Sonnet stayed within 5 for 97% of the library.
pub const MAX_SCALES: usize = 5;
pub const MAX_ASPECTS: usize = 8;
pub const MAX_NOTES: usize = 3;
/// Notes are meant to be 2 to 4 words; longer than this is a sentence.
const MAX_NOTE_WORDS: usize = 6;
const MAX_NOTE_CHARS: usize = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = AspectVerdict)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Praised,
    Criticized,
    Divisive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Aspect {
    pub aspect: String,
    pub verdict: Verdict,
}

/// A title's review tags as stored and served. Only vocabulary values.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ReviewTags {
    /// One value per scale the reviews speak to, e.g. "difficulty": "hard".
    #[serde(default)]
    pub scales: BTreeMap<String, String>,
    #[serde(default)]
    pub mood: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    /// Praised first, then criticized, then divisive.
    #[serde(default)]
    pub aspects: Vec<Aspect>,
    /// Short points the vocabulary can't express, e.g. "slow first hours".
    #[serde(default)]
    pub notes: Vec<String>,
}

/// The tag fields of one game in the model's reply, as given.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Reply {
    /// "scale:value", "mood:value" or "skills:value".
    pub facets: Vec<String>,
    pub praised: Vec<String>,
    pub criticized: Vec<String>,
    pub divisive: Vec<String>,
    pub notes: Vec<String>,
}

/// Values left out of a reply's tags.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Dropped {
    /// Not in the vocabulary, or not a string.
    pub unknown: usize,
    /// Past a cap, or a second value for a scale.
    pub over_cap: usize,
}

impl std::ops::AddAssign for Dropped {
    fn add_assign(&mut self, o: Self) {
        self.unknown += o.unknown;
        self.over_cap += o.over_cap;
    }
}

/// Lowercase letters and digits only, so "Co-op", "coop" and "co op" or
/// "level_design" and "Level Design" compare equal.
fn key(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn find<'a>(values: &'a [(&'static str, &'static str)], v: &str) -> Option<&'a (&'static str, &'static str)> {
    let k = key(v);
    values.iter().find(|(x, _)| key(x) == k)
}

impl Reply {
    /// The tag fields of one reply item. A field that is missing counts as
    /// empty; anything that is not a list of strings counts as unknown.
    pub fn from_json(item: &Value) -> (Reply, usize) {
        let mut bad = 0;
        let mut list = |field: &str| -> Vec<String> {
            match item.get(field) {
                None | Some(Value::Null) => vec![],
                Some(Value::Array(a)) => a
                    .iter()
                    .filter_map(|v| {
                        let s = v.as_str().map(str::to_string);
                        if s.is_none() {
                            bad += 1;
                        }
                        s
                    })
                    .collect(),
                Some(_) => {
                    bad += 1;
                    vec![]
                }
            }
        };
        let reply = Reply {
            facets: list("facets"),
            praised: list("praised"),
            criticized: list("criticized"),
            divisive: list("divisive"),
            notes: list("notes"),
        };
        (reply, bad)
    }

    /// Keep what the vocabulary allows, within the caps.
    pub fn validate(&self) -> (ReviewTags, Dropped) {
        let mut tags = ReviewTags::default();
        let mut dropped = Dropped::default();

        for f in &self.facets {
            let Some((name, value)) = f.split_once([':', '=']) else {
                dropped.unknown += 1;
                continue;
            };
            let name = key(name);
            if let Some(scale) = SCALES.iter().find(|s| s.name == name) {
                match find(scale.values, value) {
                    None => dropped.unknown += 1,
                    Some(_) if tags.scales.contains_key(scale.name) || tags.scales.len() >= MAX_SCALES => dropped.over_cap += 1,
                    Some((v, _)) => {
                        tags.scales.insert(scale.name.to_string(), v.to_string());
                    }
                }
            } else if let Some(set) = SETS.iter().find(|s| s.name == name) {
                let list = if set.name == MOOD.name { &mut tags.mood } else { &mut tags.skills };
                match find(set.values, value) {
                    None => dropped.unknown += 1,
                    Some((v, _)) if list.iter().any(|x| x == v) => {}
                    Some(_) if list.len() >= set.max => dropped.over_cap += 1,
                    Some((v, _)) => list.push(v.to_string()),
                }
            } else {
                dropped.unknown += 1;
            }
        }

        // Take the verdict lists in turn, so the cap doesn't only ever cut
        // the complaints. An aspect under two verdicts is divisive.
        let lists = [(&self.praised, Verdict::Praised), (&self.criticized, Verdict::Criticized), (&self.divisive, Verdict::Divisive)];
        let longest = lists.iter().map(|(l, _)| l.len()).max().unwrap_or(0);
        let mut aspects: Vec<Aspect> = vec![];
        for i in 0..longest {
            for (list, verdict) in &lists {
                let Some(raw) = list.get(i) else { continue };
                let Some(name) = ASPECTS.iter().find(|a| key(a) == key(raw)) else {
                    dropped.unknown += 1;
                    continue;
                };
                if let Some(a) = aspects.iter_mut().find(|a| a.aspect == *name) {
                    if a.verdict != *verdict {
                        a.verdict = Verdict::Divisive;
                    }
                } else if aspects.len() >= MAX_ASPECTS {
                    dropped.over_cap += 1;
                } else {
                    aspects.push(Aspect { aspect: name.to_string(), verdict: *verdict });
                }
            }
        }
        aspects.sort_by_key(|a| a.verdict as u8);
        tags.aspects = aspects;

        for n in &self.notes {
            let words: Vec<&str> = n.split_whitespace().collect();
            let text = words.join(" ");
            let text = text.trim_matches(|c: char| c == '"' || c == '\'' || c == '.' || c == ',');
            if text.is_empty() || words.len() > MAX_NOTE_WORDS || text.chars().count() > MAX_NOTE_CHARS {
                dropped.unknown += 1;
                continue;
            }
            // Kept as written: notes can name things ("Path of Pain").
            if tags.notes.iter().any(|x| x.eq_ignore_ascii_case(text)) {
                continue;
            }
            if tags.notes.len() >= MAX_NOTES {
                dropped.over_cap += 1;
                continue;
            }
            tags.notes.push(text.to_string());
        }
        (tags, dropped)
    }
}

impl ReviewTags {
    pub fn is_empty(&self) -> bool {
        self.scales.is_empty() && self.mood.is_empty() && self.skills.is_empty() && self.aspects.is_empty() && self.notes.is_empty()
    }

    /// Whether the reviews gave this facet, written as the model writes it:
    /// "sessions:short", "mood:cozy", "skills:logic".
    pub fn has(&self, facet: &str) -> bool {
        let Some((name, value)) = facet.split_once(':') else { return false };
        match name {
            "mood" => self.mood.iter().any(|v| v == value),
            "skills" => self.skills.iter().any(|v| v == value),
            _ => self.scales.get(name).is_some_and(|v| v == value),
        }
    }

    /// Whether players judged this aspect that way. A divisive aspect counts
    /// as neither praised nor criticized.
    pub fn judged(&self, aspect: &str, verdict: Verdict) -> bool {
        self.aspects.iter().any(|a| a.verdict == verdict && key(&a.aspect) == key(aspect))
    }

    /// The aspects for the picks prompt, "+story, +music, -grind, ±ending".
    pub fn verdicts(&self) -> String {
        let mark = |v: Verdict| match v {
            Verdict::Praised => '+',
            Verdict::Criticized => '-',
            Verdict::Divisive => '±',
        };
        self.aspects.iter().map(|a| format!("{}{}", mark(a.verdict), a.aspect)).collect::<Vec<_>>().join(", ")
    }

    /// Plain labels for the merged, searchable tag list: moods, then scales in
    /// vocabulary order, then skills ("Relaxing", "Easy", "Short sessions").
    /// Aspects are judgements and notes are free text, so neither is a
    /// search term; they go with the review summary.
    pub fn labels(&self) -> Vec<String> {
        let label = |values: &[(&'static str, &'static str)], v: &str| find(values, v).map(|(_, l)| l.to_string());
        let mut out: Vec<String> = self.mood.iter().filter_map(|m| label(MOOD.values, m)).collect();
        for s in SCALES {
            if let Some(v) = self.scales.get(s.name) {
                out.extend(label(s.values, v));
            }
        }
        out.extend(self.skills.iter().filter_map(|m| label(SKILLS.values, m)));
        out
    }
}

/// An aspect's name as the vocabulary spells it, whatever the case or spacing.
pub fn aspect_name(s: &str) -> Option<&'static str> {
    ASPECTS.iter().find(|a| key(a) == key(s)).copied()
}

/// A facet as the vocabulary spells it ("Sessions:Short" -> "sessions:short"),
/// or None when it isn't one.
pub fn facet_name(s: &str) -> Option<String> {
    let (name, value) = s.split_once([':', '='])?;
    let name = key(name);
    let values =
        SCALES.iter().find(|x| x.name == name).map(|x| x.values).or_else(|| SETS.iter().find(|x| x.name == name).map(|x| x.values))?;
    find(values, value).map(|(v, _)| format!("{name}:{v}"))
}

/// A facet's plain label: "pace:relaxed" -> "Relaxed pace".
pub fn facet_label(facet: &str) -> Option<&'static str> {
    let (name, value) = facet.split_once(':')?;
    let values =
        SCALES.iter().find(|x| x.name == name).map(|x| x.values).or_else(|| SETS.iter().find(|x| x.name == name).map(|x| x.values))?;
    find(values, value).map(|(_, l)| *l)
}

/// The vocabulary for the UI (`GET /api/review-tags`), with how many of the
/// given titles have each value. `known` counts the titles with any value
/// of a scale or set, so a filter can say how much of the library it covers.
pub fn vocabulary<'a>(tagged: impl Iterator<Item = &'a ReviewTags>) -> Value {
    let tagged: Vec<&ReviewTags> = tagged.collect();
    let values = |vs: &[(&str, &str)], has: &dyn Fn(&ReviewTags, &str) -> bool| {
        let list: Vec<Value> = vs
            .iter()
            .map(|(v, label)| json!({ "value": v, "label": label, "count": tagged.iter().filter(|t| has(t, v)).count() }))
            .collect();
        let known = tagged.iter().filter(|t| vs.iter().any(|(v, _)| has(t, v))).count();
        (list, known)
    };
    let scales: Vec<Value> = SCALES
        .iter()
        .map(|s| {
            let (list, known) = values(s.values, &|t, v| t.scales.get(s.name).is_some_and(|x| x == v));
            json!({ "name": s.name, "title": s.title, "means": s.means, "values": list, "known": known })
        })
        .collect();
    let sets: Vec<Value> = SETS
        .iter()
        .map(|s| {
            let (list, known) = values(s.values, &|t, v| t.has(&format!("{}:{v}", s.name)));
            json!({ "name": s.name, "title": s.title, "means": s.means, "max": s.max, "values": list, "known": known })
        })
        .collect();
    let count = |a: &str, v: Verdict| tagged.iter().filter(|t| t.aspects.iter().any(|x| x.aspect == a && x.verdict == v)).count();
    let aspects: Vec<Value> = ASPECTS
        .iter()
        .map(|a| {
            json!({ "name": a, "praised": count(a, Verdict::Praised), "criticized": count(a, Verdict::Criticized),
                    "divisive": count(a, Verdict::Divisive) })
        })
        .collect();
    json!({ "tagged": tagged.len(), "scales": scales, "sets": sets, "aspects": aspects })
}

/// Every allowed facet as the model writes it, "difficulty:hard".
pub fn facet_values() -> Vec<String> {
    let scales = SCALES.iter().flat_map(|s| s.values.iter().map(move |(v, _)| format!("{}:{v}", s.name)));
    let sets = SETS.iter().flat_map(|s| s.values.iter().map(move |(v, _)| format!("{}:{v}", s.name)));
    scales.chain(sets).collect()
}

/// The vocabulary as the prompt states it.
pub fn prompt_vocabulary() -> String {
    let values = |vs: &[(&str, &str)]| vs.iter().map(|(v, _)| *v).collect::<Vec<_>>().join(", ");
    let mut s = format!(
        "facets: a list of \"name:value\" strings, at most one value per scale and at most {MAX_SCALES} scales, \
         the best-supported first.\n"
    );
    for sc in SCALES {
        s.push_str(&format!("- {} ({}): {}\n", sc.name, sc.means, values(sc.values)));
    }
    for set in SETS {
        s.push_str(&format!("- {} ({}; up to {}): {}\n", set.name, set.means, set.max, values(set.values)));
    }
    s.push_str(&format!(
        "praised, criticized, divisive: aspects players judge, at most {MAX_ASPECTS} across the three lists, from: {}. \
         \"difficulty balance\" is whether the difficulty feels fair and well tuned, not how hard the game is \
         (the difficulty scale says that).\n",
        ASPECTS.join(", ")
    ));
    s.push_str(&format!(
        "notes: at most {MAX_NOTES} specific points of 2 to 4 words that the tags can't express, \
         like \"slow first hours\", \"great with a friend\", \"punishing save system\".\n"
    ));
    s
}

/// Request words that ask for a facet, for the picks pre-ranking. A word
/// matches a request word that starts with it ("relax" matches "relaxing").
/// A two-word entry matches two request words in a row ("big world").
pub struct Wish {
    pub words: &'static [&'static str],
    /// Matches only a negated request word ("no grind", "nothing too hard"),
    /// where a plain wish never matches.
    pub negated: bool,
    /// Any one of these, "name:value".
    pub facets: &'static [&'static str],
}

const fn wish(words: &'static [&'static str], facets: &'static [&'static str]) -> Wish {
    Wish { words, negated: false, facets }
}

const fn unwish(words: &'static [&'static str], facets: &'static [&'static str]) -> Wish {
    Wish { words, negated: true, facets }
}

pub const WISHES: &[Wish] = &[
    wish(
        &["relax", "chill", "calm", "unwind", "laid-back", "laidback", "peaceful", "zen"],
        &["pace:relaxed", "mood:relaxing", "mood:cozy"],
    ),
    wish(&["cozy", "cosy", "comfy", "comfort", "wholesome"], &["mood:cozy", "mood:relaxing", "mood:uplifting"]),
    wish(&["uplifting", "feel-good", "feelgood", "cheerful", "happy", "heartwarming"], &["mood:uplifting"]),
    wish(&["funny", "hilarious", "comedy", "comedic", "humor", "humour", "laugh", "silly", "goofy"], &["mood:funny"]),
    wish(&["chaos", "chaotic", "frantic", "hectic", "mayhem"], &["mood:chaotic"]),
    wish(&["mystery", "mysteries", "mysterious", "enigma"], &["mood:mysterious"]),
    wish(&["tense", "tension", "suspense"], &["mood:tense"]),
    wish(&["scary", "horror", "spooky", "creepy", "terrifying", "frightening"], &["mood:scary", "mood:tense"]),
    wish(&["dark", "grim", "bleak", "gritty"], &["mood:dark"]),
    wish(&["sad", "melanchol", "bittersweet"], &["mood:melancholic"]),
    wish(&["epic", "grand"], &["mood:epic"]),
    wish(&["emotional", "moving", "touching", "tearjerker", "heartfelt"], &["emotion:moving", "emotion:powerful"]),
    wish(&["easy", "casual", "forgiving", "gentle", "accessible"], &["difficulty:easy", "learning:gentle"]),
    wish(
        &["hard", "challeng", "difficult", "tough", "brutal", "punishing", "soulslike", "souls-like"],
        &["difficulty:hard", "difficulty:punishing"],
    ),
    unwish(&["hard", "challeng", "difficult", "tough", "punishing"], &["difficulty:easy", "difficulty:moderate"]),
    wish(&["intense", "adrenaline", "fast-paced", "action-packed", "frenetic"], &["pace:intense"]),
    wish(&["story", "stories", "narrative", "plot"], &["story:central"]),
    wish(&["short", "quick", "bite-sized", "bitesize", "lunch", "pick up", "pick-up"], &["sessions:short"]),
    wish(&["weekend", "marathon", "binge", "long session", "long sitting"], &["sessions:long"]),
    wish(&["podcast", "background", "mindless", "brainless", "brain off", "zone out", "switch off", "autopilot"], &["focus:low"]),
    wish(
        &[
            "open world",
            "open-world",
            "big world",
            "huge world",
            "sandbox",
            "explore",
            "exploring",
            "exploration",
            "get lost",
            "freedom",
            "nonlinear",
            "non-linear",
        ],
        &["structure:open"],
    ),
    wish(&["linear"], &["structure:linear"]),
    wish(&["grindless", "grind-free"], &["grind:none"]),
    unwish(&["grind"], &["grind:none"]),
    wish(&["deep", "depth", "complex", "intricate"], &["complexity:deep"]),
    wish(&["simple", "straightforward"], &["complexity:simple", "learning:gentle"]),
    wish(&["replay", "endless"], &["replay:high"]),
    wish(&["puzzle", "logic", "clever", "thinky", "thinking"], &["skills:logic"]),
    wish(&["strategy", "strategic", "tactic"], &["skills:strategy"]),
    wish(&["co-op", "coop", "friend", "together", "teamwork", "partner"], &["skills:teamwork"]),
    wish(&["creative", "creativity", "build"], &["skills:creativity"]),
    wish(&["manag", "economy", "optimiz", "optimis"], &["skills:management"]),
];

/// The facets a request word asks for. `prev` is the word before it, for the
/// two-word entries; `negated` says the word follows a "no" or a "not".
pub fn wished_facets(prev: Option<&str>, word: &str, negated: bool) -> Vec<&'static str> {
    let matches = |entry: &str| match entry.split_once(' ') {
        Some((first, second)) => prev == Some(first) && word.starts_with(second),
        None => word.starts_with(entry),
    };
    let mut out: Vec<&'static str> = vec![];
    for w in WISHES.iter().filter(|w| w.negated == negated && w.words.iter().any(|e| matches(e))) {
        for f in w.facets {
            if !out.contains(f) {
                out.push(f);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wishes_use_the_vocabulary() {
        let facets = facet_values();
        for w in WISHES {
            for f in w.facets {
                assert!(facets.iter().any(|x| x == f), "{f} is not in the vocabulary");
            }
        }
    }

    #[test]
    fn request_words_ask_for_facets() {
        assert_eq!(wished_facets(None, "relaxing", false), ["pace:relaxed", "mood:relaxing", "mood:cozy"]);
        assert_eq!(wished_facets(Some("big"), "world", false), ["structure:open"]);
        assert!(wished_facets(Some("small"), "world", false).is_empty(), "a two-word entry needs both words");
        assert_eq!(wished_facets(Some("something"), "story-heavy", false), ["story:central"]);
        assert_eq!(wished_facets(None, "grind", true), ["grind:none"], "\"no grind\" asks for none");
        assert!(wished_facets(None, "grind", false).is_empty(), "\"grind\" alone asks for nothing");
        assert_eq!(wished_facets(None, "hard", true), ["difficulty:easy", "difficulty:moderate"]);
        assert!(wished_facets(None, "relaxing", true).is_empty(), "\"not relaxing\" is not a wish for relaxing");
        assert!(wished_facets(None, "something", false).is_empty());
    }

    #[test]
    fn verdicts_read_compactly() {
        let (tags, _) = reply(json!({
            "facets": ["difficulty:easy", "sessions:short", "mood:relaxing"],
            "praised": ["music", "level design"],
            "criticized": ["grind"],
            "divisive": ["ending"],
        }));
        assert_eq!(tags.verdicts(), "+music, +level design, -grind, ±ending");
        assert!(tags.has("sessions:short") && tags.has("mood:relaxing") && !tags.has("sessions:long") && !tags.has("nonsense"));
        assert!(tags.judged("Level Design", Verdict::Praised) && tags.judged("grind", Verdict::Criticized));
        assert!(!tags.judged("ending", Verdict::Praised) && !tags.judged("ending", Verdict::Criticized), "divisive is neither");
    }

    fn reply(v: Value) -> (ReviewTags, Dropped) {
        let (r, bad) = Reply::from_json(&v);
        let (tags, mut dropped) = r.validate();
        dropped.unknown += bad;
        (tags, dropped)
    }

    #[test]
    fn the_vocabulary_has_no_duplicates_and_labels_are_distinct() {
        let facets = facet_values();
        let mut keys: Vec<String> = facets.iter().map(|f| key(f)).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), facets.len());
        let mut aspects: Vec<String> = ASPECTS.iter().map(|a| key(a)).collect();
        aspects.sort();
        aspects.dedup();
        assert_eq!(aspects.len(), ASPECTS.len());
        let mut labels: Vec<&str> =
            SCALES.iter().flat_map(|s| s.values.iter()).chain(SETS.iter().flat_map(|s| s.values.iter())).map(|(_, l)| *l).collect();
        let n = labels.len();
        labels.sort();
        labels.dedup();
        assert_eq!(labels.len(), n, "two values share a label");
    }

    #[test]
    fn a_reply_keeps_known_values_in_canonical_form() {
        let (tags, dropped) = reply(json!({
            "facets": ["Difficulty:Hard", "pace: intense", "mood:tense", "mood:DARK", "skills:reflexes", "structure=hubs"],
            "praised": ["Combat", "level_design", "ui"],
            "criticized": ["Co-Op"],
            "divisive": ["ending"],
            "notes": ["Slow first hours.", "UI bugs"],
        }));
        assert_eq!(dropped, Dropped::default());
        assert_eq!(tags.scales.get("difficulty").map(String::as_str), Some("hard"));
        assert_eq!(tags.scales.get("pace").map(String::as_str), Some("intense"));
        assert_eq!(tags.scales.get("structure").map(String::as_str), Some("hubs"));
        assert_eq!(tags.mood, ["tense", "dark"]);
        assert_eq!(tags.skills, ["reflexes"]);
        let aspects: Vec<(&str, Verdict)> = tags.aspects.iter().map(|a| (a.aspect.as_str(), a.verdict)).collect();
        assert_eq!(
            aspects,
            [
                ("combat", Verdict::Praised),
                ("level design", Verdict::Praised),
                ("UI", Verdict::Praised),
                ("co-op", Verdict::Criticized),
                ("ending", Verdict::Divisive)
            ]
        );
        assert_eq!(tags.notes, ["Slow first hours", "UI bugs"], "trimmed, case kept");
    }

    #[test]
    fn unknown_values_are_dropped_and_counted() {
        let (tags, dropped) = reply(json!({
            "facets": ["difficulty:brutal", "vibe:chill", "no colon", "mood:happy", 7, "story:central"],
            "praised": ["graphics", "story"],
            "criticized": "grind",
            "divisive": [],
            "notes": ["this note is far too long to be one of the short notes", ""],
        }));
        assert_eq!(dropped, Dropped { unknown: 9, over_cap: 0 });
        assert_eq!(tags.scales.len(), 1);
        assert_eq!(tags.aspects, [Aspect { aspect: "story".into(), verdict: Verdict::Praised }]);
        assert!(tags.notes.is_empty() && tags.mood.is_empty());
    }

    #[test]
    fn caps_hold_and_cut_every_verdict_fairly() {
        let (tags, dropped) = reply(json!({
            "facets": ["difficulty:easy", "difficulty:hard", "mood:cozy", "mood:funny", "mood:epic",
                       "skills:logic", "skills:timing", "skills:patience", "skills:strategy"],
            "praised": ["combat", "story", "music", "art", "world", "writing"],
            "criticized": ["grind", "bugs", "camera", "controls"],
            "divisive": ["ending", "length"],
            "notes": ["one", "two", "three", "four"],
        }));
        assert_eq!(tags.scales.get("difficulty").map(String::as_str), Some("easy"), "the first value wins");
        assert_eq!(tags.mood.len(), MOOD.max);
        assert_eq!(tags.skills.len(), SKILLS.max);
        assert_eq!(tags.aspects.len(), MAX_ASPECTS);
        let count = |v: Verdict| tags.aspects.iter().filter(|a| a.verdict == v).count();
        assert_eq!((count(Verdict::Praised), count(Verdict::Criticized), count(Verdict::Divisive)), (3, 3, 2));
        assert_eq!(tags.notes.len(), MAX_NOTES);
        // 1 difficulty + 1 mood + 1 skill + 4 aspects + 1 note.
        assert_eq!(dropped, Dropped { unknown: 0, over_cap: 8 });
    }

    /// GPT-6-Luna's Papers, Please: eight scales. The first five, the
    /// best-supported by the prompt's rule, are kept; moods and skills
    /// don't count against the cap.
    #[test]
    fn a_title_gets_five_scales_at_most() {
        let (tags, dropped) = reply(json!({
            "facets": ["pace:intense", "focus:high", "mood:dark", "story:central", "emotion:moving", "learning:moderate",
                       "complexity:moderate", "hook:fast", "structure:linear", "skills:observation"],
        }));
        let kept: Vec<&str> = SCALES.iter().map(|s| s.name).filter(|n| tags.scales.contains_key(*n)).collect();
        assert_eq!(kept.len(), MAX_SCALES);
        for name in ["pace", "focus", "story", "emotion", "learning"] {
            assert!(tags.scales.contains_key(name), "{name}: {kept:?}");
        }
        assert_eq!((tags.mood.len(), tags.skills.len()), (1, 1));
        assert_eq!(dropped.over_cap, 3);
    }

    #[test]
    fn an_aspect_under_two_verdicts_is_divisive() {
        let (tags, _) = reply(json!({ "praised": ["story", "music"], "criticized": ["Story"] }));
        assert_eq!(
            tags.aspects,
            [Aspect { aspect: "music".into(), verdict: Verdict::Praised }, Aspect { aspect: "story".into(), verdict: Verdict::Divisive },]
        );
    }

    #[test]
    fn labels_read_as_plain_tags() {
        let (tags, _) = reply(json!({
            "facets": ["difficulty:easy", "sessions:short", "mood:relaxing", "skills:patience"],
            "praised": ["music"],
            "notes": ["slow first hours"],
        }));
        assert_eq!(tags.labels(), ["Relaxing", "Easy", "Short sessions", "Rewards patience"], "no aspects or notes");
        assert_eq!(facet_label("pace:relaxed"), Some("Relaxed pace"));
        assert_eq!(facet_label("mood:cozy"), Some("Cozy"));
        assert_eq!((facet_label("pace:sideways"), facet_label("pace")), (None, None));
    }

    #[test]
    fn stored_tags_round_trip_through_json() {
        let (tags, _) = reply(json!({ "facets": ["hook:slow", "mood:cozy"], "divisive": ["ending"], "notes": ["great with a friend"] }));
        let s = serde_json::to_string(&tags).unwrap();
        assert_eq!(serde_json::from_str::<ReviewTags>(&s).unwrap(), tags);
        assert!(s.contains(r#""verdict":"divisive""#), "{s}");
        assert_eq!(serde_json::from_str::<ReviewTags>("{}").unwrap(), ReviewTags::default());
    }
}
