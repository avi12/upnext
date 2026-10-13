//! What the taste profile can talk about, and how each reads in a sentence.
//!
//! A subject is written `facet:<scale>:<value>` (a review-tag value),
//! `tag:<Steam tag>`, `length:<short|medium|long>` or `text:<your words>`.
//! Only the review-tag values and tags listed here can become patterns, so a
//! pattern always has a sentence that reads well ("You drift away from games
//! with heavy grind"); likes and dislikes can be about any of them, or about
//! words of your own.

use crate::review_tags;

/// Review-tag values that say something about taste, with the noun phrase a
/// pattern uses. Middle values ("Steady pace", "Some learning curve") are
/// left out: they're what most games are.
pub const FACETS: &[(&str, &str)] = &[
    ("difficulty:easy", "easy games"),
    ("difficulty:hard", "hard games"),
    ("difficulty:punishing", "punishing games"),
    ("learning:gentle", "games that are easy to learn"),
    ("learning:steep", "games with a steep learning curve"),
    ("hook:fast", "games that hook you fast"),
    ("hook:gradual", "games that grow on you"),
    ("hook:slow", "games that are slow to hook you"),
    ("pace:relaxed", "relaxed games"),
    ("pace:intense", "intense games"),
    ("sessions:short", "games for short sessions"),
    ("sessions:long", "games that want long sittings"),
    ("focus:low", "games you can play with half your attention"),
    ("focus:high", "games that need your full focus"),
    ("complexity:simple", "simple games"),
    ("complexity:deep", "games with deep systems"),
    ("structure:linear", "linear games"),
    ("structure:hubs", "hub-based games"),
    ("structure:open", "open-ended games"),
    ("grind:none", "games without grind"),
    ("grind:heavy", "games with heavy grind"),
    ("story:none", "games without a story"),
    ("story:central", "story-driven games"),
    ("emotion:moving", "emotionally moving games"),
    ("emotion:powerful", "emotionally powerful games"),
    ("replay:high", "games with high replay value"),
    ("mood:relaxing", "relaxing games"),
    ("mood:cozy", "cozy games"),
    ("mood:uplifting", "uplifting games"),
    ("mood:funny", "funny games"),
    ("mood:chaotic", "chaotic games"),
    ("mood:mysterious", "mysterious games"),
    ("mood:tense", "tense games"),
    ("mood:scary", "scary games"),
    ("mood:dark", "dark games"),
    ("mood:melancholic", "melancholic games"),
    ("mood:epic", "games that feel epic"),
    ("skills:reflexes", "games that test your reflexes"),
    ("skills:precision", "games that test your precision"),
    ("skills:timing", "games that test your timing"),
    ("skills:strategy", "games that reward strategy"),
    ("skills:logic", "games that reward logic"),
    ("skills:patience", "games that reward patience"),
    ("skills:memorization", "games that reward memorization"),
    ("skills:observation", "games that reward observation"),
    ("skills:management", "games about managing resources"),
    ("skills:creativity", "games that reward creativity"),
    ("skills:teamwork", "games that need teamwork"),
];

/// Genre and theme tags (Steam's names, which store genres share), with the
/// noun phrase a pattern uses. Tags about looks, sound or the store
/// ("Pixel Graphics", "Great Soundtrack", "Early Access") aren't here.
pub const TAGS: &[(&str, &str)] = &[
    ("Action", "action games"),
    ("Adventure", "adventure games"),
    ("Action-Adventure", "action-adventures"),
    ("Casual", "casual games"),
    ("Indie", "indie games"),
    ("RPG", "RPGs"),
    ("Action RPG", "action RPGs"),
    ("JRPG", "JRPGs"),
    ("CRPG", "CRPGs"),
    ("Party-Based RPG", "party-based RPGs"),
    ("Strategy", "strategy games"),
    ("Turn-Based Strategy", "turn-based strategy games"),
    ("Turn-Based Tactics", "turn-based tactics games"),
    ("Tactical", "tactical games"),
    ("RTS", "real-time strategy games"),
    ("Grand Strategy", "grand strategy games"),
    ("4X", "4X games"),
    ("Tower Defense", "tower defense games"),
    ("City Builder", "city builders"),
    ("Colony Sim", "colony sims"),
    ("Management", "management games"),
    ("Automation", "automation games"),
    ("Simulation", "simulation games"),
    ("Farming Sim", "farming sims"),
    ("Life Sim", "life sims"),
    ("Dating Sim", "dating sims"),
    ("Flight", "flight sims"),
    ("Driving", "driving games"),
    ("Racing", "racing games"),
    ("Sports", "sports games"),
    ("Survival", "survival games"),
    ("Open World Survival Craft", "survival crafting games"),
    ("Crafting", "crafting games"),
    ("Base Building", "base building games"),
    ("Sandbox", "sandbox games"),
    ("Open World", "open-world games"),
    ("Exploration", "exploration games"),
    ("Roguelike", "roguelikes"),
    ("Roguelite", "roguelites"),
    ("Metroidvania", "metroidvanias"),
    ("Souls-like", "soulslikes"),
    ("Platformer", "platformers"),
    ("Precision Platformer", "precision platformers"),
    ("Puzzle", "puzzle games"),
    ("Puzzle Platformer", "puzzle platformers"),
    ("Horror", "horror games"),
    ("Psychological Horror", "psychological horror"),
    ("Survival Horror", "survival horror"),
    ("Stealth", "stealth games"),
    ("Immersive Sim", "immersive sims"),
    ("FPS", "first-person shooters"),
    ("Shooter", "shooters"),
    ("Looter Shooter", "looter shooters"),
    ("Twin Stick Shooter", "twin-stick shooters"),
    ("Bullet Hell", "bullet hells"),
    ("Hack and Slash", "hack-and-slash games"),
    ("Beat 'em up", "beat 'em ups"),
    ("Fighting", "fighting games"),
    ("Card Game", "card games"),
    ("Deckbuilding", "deckbuilders"),
    ("Rhythm", "rhythm games"),
    ("Visual Novel", "visual novels"),
    ("Point & Click", "point-and-click adventures"),
    ("Walking Simulator", "walking sims"),
    ("Story Rich", "story-rich games"),
    ("Choices Matter", "games where your choices matter"),
    ("Detective", "detective games"),
    ("Mystery", "mystery games"),
    ("Co-op", "co-op games"),
    ("Multiplayer", "multiplayer games"),
    ("PvP", "competitive multiplayer games"),
    ("Massively Multiplayer", "MMOs"),
    ("MMORPG", "MMORPGs"),
    ("MOBA", "MOBAs"),
    ("Battle Royale", "battle royales"),
    ("Arcade", "arcade games"),
    ("Retro", "retro games"),
    ("Sci-fi", "sci-fi games"),
    ("Space", "space games"),
    ("Cyberpunk", "cyberpunk games"),
    ("Fantasy", "fantasy games"),
    ("Dark Fantasy", "dark fantasy games"),
    ("Post-apocalyptic", "post-apocalyptic games"),
    ("Zombies", "zombie games"),
    ("Historical", "historical games"),
    ("War", "war games"),
    ("Military", "military games"),
    ("Anime", "anime games"),
];

/// HowLongToBeat's main story in bands, as (subject, label, phrase, up to
/// this many hours).
pub const LENGTHS: &[(&str, &str, &str, f64)] = &[
    ("length:short", "Under 10 hours", "games under 10 hours", 10.0),
    ("length:medium", "10 to 30 hours", "games of 10 to 30 hours", 30.0),
    ("length:long", "Over 30 hours", "games over 30 hours", f64::INFINITY),
];

/// The band a main story falls in.
pub fn length_of(hours: f64) -> &'static str {
    LENGTHS.iter().find(|(_, _, _, upto)| hours < *upto).map(|(s, ..)| *s).unwrap_or("length:long")
}

/// Tags compared without case or punctuation: "Souls-like" is "Soulslike".
pub fn tag_key(t: &str) -> String {
    t.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// A listed tag's own spelling, whatever the case or punctuation.
pub fn known_tag(t: &str) -> Option<&'static str> {
    // Keyed once: picks ask this for every tag of every game in the library.
    static BY_KEY: std::sync::LazyLock<std::collections::HashMap<String, &'static str>> = std::sync::LazyLock::new(|| {
        let mut by_key = std::collections::HashMap::new();
        for (name, _) in TAGS {
            by_key.entry(tag_key(name)).or_insert(*name);
        }
        by_key
    });
    BY_KEY.get(&tag_key(t)).copied()
}

/// What the profile can say about a subject, checked and spelled as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    /// A review-tag value, "grind:heavy".
    Facet(String),
    /// A tag, as `TAGS` spells it when it's listed there.
    Tag(String),
    /// A band of `LENGTHS`, "length:short".
    Length(&'static str),
    /// Your own words.
    Text(String),
}

/// The longest tag and text a like or dislike takes, in characters.
const MAX_SUBJECT: usize = 120;

impl Subject {
    /// From its stored form; None when it isn't one.
    pub fn parse(s: &str) -> Option<Subject> {
        let (kind, rest) = s.split_once(':')?;
        let rest = rest.trim();
        if rest.is_empty() || rest.chars().count() > MAX_SUBJECT {
            return None;
        }
        match kind {
            "facet" => review_tags::facet_name(rest).map(Subject::Facet),
            "tag" => Some(Subject::Tag(known_tag(rest).map(str::to_string).unwrap_or_else(|| rest.to_string()))),
            "length" => LENGTHS.iter().find(|(k, ..)| k.strip_prefix("length:") == Some(rest)).map(|(k, ..)| Subject::Length(k)),
            "text" => Some(Subject::Text(rest.to_string())),
            _ => None,
        }
    }

    pub fn key(&self) -> String {
        match self {
            Subject::Facet(f) => format!("facet:{f}"),
            Subject::Tag(t) => format!("tag:{t}"),
            Subject::Length(k) => k.to_string(),
            Subject::Text(t) => format!("text:{t}"),
        }
    }

    /// As a list shows it: "Heavy grind", "Horror", "Under 10 hours".
    pub fn label(&self) -> String {
        match self {
            Subject::Facet(f) => review_tags::facet_label(f).unwrap_or(f).to_string(),
            Subject::Tag(t) | Subject::Text(t) => t.clone(),
            Subject::Length(k) => LENGTHS.iter().find(|(s, ..)| s == k).map(|(_, l, ..)| l.to_string()).unwrap_or_default(),
        }
    }

    /// As a pattern's sentence says it: "games with heavy grind". None for
    /// subjects patterns aren't drawn from.
    pub fn phrase(&self) -> Option<&'static str> {
        match self {
            Subject::Facet(f) => FACETS.iter().find(|(k, _)| k == f).map(|(_, p)| *p),
            Subject::Tag(t) => TAGS.iter().find(|(k, _)| k == t).map(|(_, p)| *p),
            Subject::Length(k) => LENGTHS.iter().find(|(s, ..)| s == k).map(|(.., p, _)| *p),
            Subject::Text(_) => None,
        }
    }

    /// The review-tag aspect stop reasons are checked against, for "3 of them
    /// for the grind" (`lifecycle::REASONS`).
    pub fn aspect(&self) -> Option<&'static str> {
        match self {
            Subject::Facet(f) => match f.split_once(':').map(|(scale, _)| scale) {
                Some("grind") => Some("grind"),
                Some("story") => Some("story"),
                Some("difficulty") => Some("difficulty balance"),
                Some("pace") => Some("pacing"),
                _ => None,
            },
            Subject::Length(_) => Some("length"),
            _ => None,
        }
    }
}

/// How a reason given for an aspect reads after "for": "3 of them for the grind".
pub fn aspect_reason(aspect: &str) -> &'static str {
    match aspect {
        "grind" => "the grind",
        "story" => "losing the story",
        "difficulty balance" => "the difficulty",
        "pacing" => "the pacing",
        "length" => "the length",
        _ => "that",
    }
}

/// What a game can be loved for, as a favourite's reasons: what players
/// praise about it ("the story"), then its moods and what it asks of you,
/// then its strongest tags. Aspects that are only ever complaints (bugs,
/// monetization) aren't reasons.
pub fn aspect_why(aspect: &str) -> Option<&'static str> {
    Some(match aspect {
        "combat" => "the combat",
        "story" => "the story",
        "writing" => "the writing",
        "characters" => "the characters",
        "choices" => "the choices",
        "world" => "the world",
        "atmosphere" => "the atmosphere",
        "exploration" => "exploration",
        "puzzles" => "the puzzles",
        "level design" => "the level design",
        "core loop" => "the core loop",
        "progression" => "the progression",
        "pacing" => "the pacing",
        "difficulty balance" => "the challenge",
        "controls" => "the controls",
        "music" => "the music",
        "sound" => "the sound",
        "voice acting" => "the voice acting",
        "art" => "the art",
        "ending" => "the ending",
        "endgame" => "the endgame",
        "co-op" => "co-op",
        "multiplayer" => "multiplayer",
        "mods" => "mods",
        _ => return None,
    })
}
