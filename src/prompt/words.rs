//! The pick request's words, as the ranking and the prompt read them:
//! negations and their reach, the time budget, what a request asks for,
//! and how its words match a game's tags, review tags and text.

use crate::db::GameView;

/// Parse a time budget like "2 hours", "45 min", "an hour", "a quiet hour"
/// from the request.
pub(super) fn time_budget_hours(request: &str) -> Option<f64> {
    let lower = request.to_lowercase();
    let re = regex::Regex::new(r"(\d+(?:\.\d+)?|an?|half an?|a couple of|couple)\s*(hours?|hrs?|h\b|minutes?|mins?)").ok()?;
    if let Some(c) = re.captures(&lower) {
        let n = match &c[1] {
            "a" | "an" => 1.0,
            "half a" | "half an" => 0.5,
            "a couple of" | "couple" => 2.0,
            s => s.parse::<f64>().ok()?,
        };
        let unit = &c[2];
        return Some(if unit.starts_with('m') { n / 60.0 } else { n });
    }
    // One word between: "a quiet hour", "a free hour", "a half hour".
    let one = regex::Regex::new(r"\ban?\s+(\w+)\s+hour\b").ok()?;
    one.captures(&lower).map(|c| if &c[1] == "half" { 0.5 } else { 1.0 })
}

/// Whether a word of `hay` starts with `stem`: "story" is in "Story Rich"
/// and "a story-driven game", not in "History".
pub(super) fn starts_a_word(hay: &str, stem: &str) -> bool {
    hay.match_indices(stem).any(|(i, _)| hay[..i].chars().next_back().is_none_or(|c| !c.is_alphanumeric()))
}

/// A word of the pick request, lower case.
pub(super) struct RequestWord {
    pub(super) text: String,
    /// It follows a "no", "not" or "without" in the same clause.
    pub(super) negated: bool,
    /// The review-tag facets it asks for (`review_tags::WISHES`).
    pub(super) facets: Vec<&'static str>,
    /// What it asks that other backlog players' verdicts can speak to
    /// (`verdicts::community::wished`).
    pub(super) verdict_facets: Vec<&'static str>,
}

/// Words that negate what follows, up to the end of the clause.
const NEGATIONS: &[&str] =
    &["no", "not", "without", "never", "nothing", "none", "zero", "less", "avoid", "don't", "dont", "isn't", "aren't", "won't", "nor"];
/// Words that end a negation's reach, like a comma: "no grind but cozy".
const CLAUSE_BREAKS: &[&str] = &["but", "and", "just", "only"];
/// How many words a negation reaches: "not too hard", "no heavy grind".
const NEGATION_REACH: usize = 3;

/// What a request asks that other backlog players' verdicts can speak to,
/// for ordering a pick card's line (`verdicts::community::Backlog::card_line`).
pub fn verdict_wishes(request: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = vec![];
    for f in request_words(request).into_iter().flat_map(|w| w.verdict_facets) {
        if !out.contains(&f) {
            out.push(f);
        }
    }
    out
}

pub(super) fn request_words(request: &str) -> Vec<RequestWord> {
    let lower = request.to_lowercase().replace('\u{2019}', "'");
    let mut words: Vec<RequestWord> = vec![];
    let mut reach = 0;
    let mut prev: Option<String> = None;
    // Punctuation ends a clause; a hyphen or an apostrophe stays in a word.
    for chunk in lower.split_inclusive(|c: char| !c.is_alphanumeric() && c != '-' && c != '\'') {
        let text: String = chunk.chars().filter(|c| c.is_alphanumeric() || *c == '-' || *c == '\'').collect();
        let text = text.trim_matches(|c| c == '-' || c == '\'').to_string();
        if !text.is_empty() {
            if NEGATIONS.contains(&text.as_str()) {
                reach = NEGATION_REACH;
            } else if CLAUSE_BREAKS.contains(&text.as_str()) {
                reach = 0;
            } else {
                let negated = reach > 0;
                reach = reach.saturating_sub(1);
                let facets = crate::review_tags::wished_facets(prev.as_deref(), &text, negated);
                let verdict_facets = crate::verdicts::community::wished(prev.as_deref(), &text, negated, &facets);
                words.push(RequestWord { text: text.clone(), negated, facets, verdict_facets });
            }
            prev = Some(text);
        }
        if chunk.ends_with([',', '.', ';', ':', '!', '?', '(', ')']) {
            reach = 0;
        }
    }
    words
}

pub(super) const STOP_WORDS: &[&str] = &[
    "with",
    "that",
    "have",
    "want",
    "something",
    "some",
    "game",
    "games",
    "play",
    "like",
    "just",
    "really",
    "this",
    "from",
    "into",
    "hour",
    "hours",
    "minutes",
    "mins",
];

/// Words asking for something you know: staples and endless games you've
/// played suit them. Each is matched as a word, with or without one of
/// [`ENDINGS`].
pub(super) const FAMILIAR: &[&str] = &[
    "familiar",
    "comfort",
    "favorite",
    "favourite",
    "staple",
    "go-to",
    "usual",
    "again",
    "replay",
    "nostalgia",
    "nostalgic",
    "old friend",
    "back to",
];

/// Words asking for games you put down: more than one second chance may
/// come with them.
const SECOND_CHANCES: &[&str] = &[
    "second chance",
    "another chance",
    "another go",
    "another try",
    "abandon",
    "gave up",
    "given up",
    "give up",
    "dropped",
    "put down",
    "unfinished",
    "back to",
    "come back",
    "return to",
];

/// The endings a word of [`FAMILIAR`] or [`SECOND_CHANCES`] may carry:
/// "favourites", "replaying", "abandoned", "usually", "nostalgically",
/// "replayable", "comfortably", "replayability", "familiarity",
/// "abandonment".
const ENDINGS: &[&str] = &["", "s", "es", "d", "ed", "ing", "ly", "ally", "able", "ably", "ability", "ity", "ment"];

/// Whether the request says one of `words` as a word of its own, not inside
/// another: "my usual" and "usually" ask for something familiar, "unusual"
/// doesn't, and "again" isn't in "against".
pub(super) fn asks_for(request: &str, words: &[&str]) -> bool {
    let lower = request.to_lowercase().replace('\u{2019}', "'");
    words.iter().any(|w| {
        lower.match_indices(w).any(|(i, _)| {
            let rest: String = lower[i + w.len()..].chars().take_while(|c| c.is_alphanumeric()).collect();
            lower[..i].chars().next_back().is_none_or(|c| !c.is_alphanumeric()) && ENDINGS.contains(&rest.as_str())
        })
    })
}

/// The request asks for games put down before: second chances may be
/// more than one per lineup.
pub fn asks_for_second_chances(request: &str) -> bool {
    asks_for(request, SECOND_CHANCES)
}

/// What the request's words matched in a game (`keyword_matches`).
#[derive(Debug, Default)]
pub(super) struct Matches {
    pub(super) score: f64,
    /// Review-tag labels and tags the words matched: "Relaxed pace", "Cozy".
    pub(super) labels: Vec<String>,
    /// Words found only in its name, blurb or review summary.
    pub(super) mentioned: Vec<String>,
}

/// How well the request's words match a game: a tag or a review-tag facet
/// the word asks for counts 12, a match in the name, blurb or review summary 6.
/// A negated word ("no grind") never counts for the games that have it.
pub(super) fn keyword_boost(words: &[RequestWord], g: &GameView) -> f64 {
    keyword_matches(words, g).score
}

pub(super) fn keyword_matches(words: &[RequestWord], g: &GameView) -> Matches {
    let mut m = Matches::default();
    if words.is_empty() {
        return m;
    }
    let hay: Vec<&String> = g.tags.iter().chain(g.genres.iter()).chain(g.themes.iter()).collect();
    // Free text catches moods tags miss ("cozy", "relaxing", "short").
    let text = format!(
        "{} {} {}",
        g.name.to_lowercase(),
        g.short_description.as_deref().unwrap_or_default().to_lowercase(),
        g.digest.as_deref().unwrap_or_default().to_lowercase()
    );
    let mut label = |l: &str| {
        if !m.labels.iter().any(|x| x.eq_ignore_ascii_case(l)) {
            m.labels.push(l.to_string());
        }
    };
    let mut score = 0.0;
    let mut mentioned = vec![];
    for w in words {
        let facet = g.review_tags.as_ref().and_then(|t| w.facets.iter().find(|f| t.has(f)));
        let keyword = !w.negated && w.text.len() >= 4 && !STOP_WORDS.contains(&w.text.as_str());
        // First five characters, not bytes: requests can be in any script.
        let stem: String = w.text.chars().take(5).collect();
        let tag = hay.iter().find(|h| keyword && starts_a_word(&h.to_lowercase(), &stem));
        if let Some(f) = facet {
            score += 12.0;
            label(crate::review_tags::facet_label(f).unwrap_or(f));
        } else if let Some(t) = tag {
            score += 12.0;
            label(t);
        } else if keyword && starts_a_word(&text, &stem) {
            score += 6.0;
            mentioned.push(w.text.clone());
        }
    }
    m.score = score;
    m.mentioned = mentioned;
    m
}

/// Words for a game that takes too long, and for one that's over too soon.
const LONG: &[&str] = &["long", "lengthy", "big", "huge", "massive", "time-consuming", "commitment"];
const SHORT: &[&str] = &["short", "small", "brief", "quick"];
/// Words for a game that's too old, and for one that's too new.
const OLD: &[&str] = &["old", "dated", "retro", "aged"];
const NEW: &[&str] = &["new", "recent", "modern"];

/// Whether a spare has the problem a pick was turned down for ("It's too
/// ___"), as far as UpNext can tell without the AI: "long" rules out a
/// spare that isn't known to be shorter, "short" one that isn't longer,
/// "old" one that isn't newer, "new" one that isn't older, and any other
/// word a spare whose tags, review tags or summary match it, the way a
/// request's words are matched.
pub fn rules_out(word: &str, turned_down: &GameView, spare: &GameView) -> bool {
    let less = |a: Option<f64>, b: Option<f64>| matches!((a, b), (Some(a), Some(b)) if a < b);
    let year = |g: &GameView| g.release_year().and_then(|y| y.parse::<f64>().ok());
    let mut other: Vec<RequestWord> = vec![];
    for w in request_words(word) {
        let is = |list: &[&str]| list.contains(&w.text.as_str());
        let out = if is(LONG) {
            !less(spare.hltb_main, turned_down.hltb_main)
        } else if is(SHORT) {
            !less(turned_down.hltb_main, spare.hltb_main)
        } else if is(OLD) {
            !less(year(turned_down), year(spare))
        } else if is(NEW) {
            !less(year(spare), year(turned_down))
        } else {
            other.push(w);
            false
        };
        if out {
            return true;
        }
    }
    keyword_boost(&other, spare) > 0.0
}
