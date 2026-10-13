//! The profile as picks read it: the About me block the
//! AI gets in place of the most-played list, and what the ranking needs to
//! leave hard nos out, raise likes, lower dislikes and the patterns you
//! drift away from, and find games like your favourites.

use super::{Sense, Subject, features, why_label};
use crate::db::PlayerId;
use crate::db::{Db, GameView, StatementRow};
use crate::llm::{name_line, one_line};
use anyhow::Result;
use chrono::{Datelike, Timelike};
use std::collections::HashSet;

/// Most-played games listed in About me when you have fewer favourites than
/// this, as the signal they were before the profile.
const FEW_FAVOURITES: usize = 5;
const PLAYED_MOST: usize = 8;
/// Favourites listed at most.
const MAX_FAVOURITES: usize = 12;
/// A note in your own words, at most this many characters in the prompt.
const NOTE_CHARS: usize = 120;
/// Games you rated listed in About me at most: those you rated 4 or 5,
/// and those you rated 1 or 2.
const MAX_RATED_HIGH: usize = 12;
const MAX_RATED_LOW: usize = 8;

/// A like, dislike or hard no.
#[derive(Debug, Clone, PartialEq)]
pub struct Stated {
    pub subject: Subject,
    pub note: Option<String>,
}

/// A favourite, with why and what it's like.
#[derive(Debug, Clone)]
pub struct Favourite {
    pub id: i64,
    pub name: String,
    /// "the combat", "tense".
    pub why: Vec<String>,
    pub note: Option<String>,
    pub minutes: i64,
    /// What it's like (`taste::features`), for likeness to other games.
    pub features: Vec<Subject>,
}

/// A game you rated, with what it's like.
#[derive(Debug, Clone)]
pub struct Rated {
    pub id: i64,
    pub name: String,
    /// 1 to 5.
    pub stars: u8,
    pub at: i64,
    /// What it's like (`taste::features`), for likeness to other games.
    pub features: Vec<Subject>,
}

impl Rated {
    /// Rated 4 or 5: it counts like a favourite for likeness.
    pub fn high(&self) -> bool {
        self.stars >= 4
    }

    /// Rated 1 or 2: it counts softly against games like it.
    pub fn low(&self) -> bool {
        self.stars <= 2
    }
}

/// A pattern shown on the page (not hidden).
#[derive(Debug, Clone)]
pub struct Noticed {
    pub sense: Sense,
    pub subject: Subject,
    /// "5 shelved; 1 played through (and 15 older ones)".
    pub summary: String,
    pub confidence: f64,
}

/// When it is: a weeknight or the weekend (Friday from 5 pm to Sunday).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    Weeknight,
    Weekend,
}

impl When {
    pub fn at(t: i64) -> When {
        let Some(local) = chrono::DateTime::from_timestamp(t, 0).map(|d| d.with_timezone(&chrono::Local)) else {
            return When::Weeknight;
        };
        match local.weekday() {
            chrono::Weekday::Sat | chrono::Weekday::Sun => When::Weekend,
            chrono::Weekday::Fri if local.hour() >= 17 => When::Weekend,
            _ => When::Weeknight,
        }
    }
}

/// The usual session for now, from your habits: it stands in for a time
/// budget when the request gives none.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Usual {
    pub minutes: i64,
    pub when: When,
    /// From the play diary, as you haven't said.
    pub from_diary: bool,
}

/// Habits you stated.
#[derive(Debug, Clone, Default)]
pub struct Habits {
    pub weeknight: Option<i64>,
    pub weekend: Option<i64>,
    pub times: Vec<String>,
    pub who: Vec<String>,
    pub r#where: Vec<String>,
    /// Notes on habits, by what they're about.
    pub notes: Vec<(String, String)>,
}

/// The profile as picks read it.
#[derive(Debug, Clone, Default)]
pub struct Profile {
    pub favourites: Vec<Favourite>,
    pub likes: Vec<Stated>,
    pub dislikes: Vec<Stated>,
    pub hard_nos: Vec<Stated>,
    pub patterns: Vec<Noticed>,
    pub habits: Habits,
    /// What the play diary shows, when it holds enough to say anything.
    /// What you stated wins over it.
    pub diary: Option<super::diary::Habits>,
    /// Games you rated, newest rating first. Like everything you
    /// state, ratings never fade.
    pub rated: Vec<Rated>,
    favourite_ids: HashSet<i64>,
}

fn short(note: &str) -> String {
    let n: String = note.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = n.chars().take(NOTE_CHARS).collect();
    if cut.len() < n.len() { format!("{cut}…") } else { cut }
}

/// "the story", "Horror", "heavy grind": a subject mid-sentence.
fn plain(s: &Subject) -> String {
    match s {
        Subject::Text(t) => format!("\"{}\"", short(t)),
        Subject::Tag(t) => t.clone(),
        _ => {
            let l = s.label();
            let mut c = l.chars();
            match c.next() {
                Some(f) if !l.chars().nth(1).is_some_and(char::is_uppercase) => f.to_lowercase().chain(c).collect(),
                _ => l,
            }
        }
    }
}

fn hours_text(minutes: i64) -> String {
    match minutes {
        m if m % 60 == 0 && m >= 60 => format!("{}h", m / 60),
        m if m < 60 => crate::enrich::plural(m, "minute", "minutes"),
        m => format!("{:.1}h", m as f64 / 60.0),
    }
}

impl Profile {
    /// From the profile's rows and the library.
    pub fn from_rows(rows: &[StatementRow], games: &[GameView]) -> Profile {
        let mut p = Profile::default();
        let stated = |r: &StatementRow| -> Option<Stated> {
            Some(Stated { subject: Subject::parse(r.about.as_deref()?)?, note: r.note.as_deref().map(short) })
        };
        for r in rows {
            match r.kind.as_str() {
                "favourite" => {
                    let Some(g) = r.title_id.and_then(|id| games.iter().find(|g| g.id == id)) else { continue };
                    let why: Vec<String> = r.why.as_ref().and_then(|w| serde_json::from_value(w.clone()).ok()).unwrap_or_default();
                    p.favourite_ids.insert(g.id);
                    p.favourites.push(Favourite {
                        id: g.id,
                        name: g.name.clone(),
                        why: why.iter().map(|k| why_label(k)).collect(),
                        note: r.note.as_deref().map(short),
                        minutes: g.playtime_minutes,
                        features: features(g),
                    });
                }
                "like" => p.likes.extend(stated(r)),
                "dislike" => p.dislikes.extend(stated(r)),
                "hard_no" => p.hard_nos.extend(stated(r)),
                "pattern" if !r.hidden => {
                    let Some(e) = r.evidence.clone().and_then(|e| serde_json::from_value::<super::Evidence>(e).ok()) else { continue };
                    let Some(subject) = Subject::parse(&e.subject) else { continue };
                    p.patterns.push(Noticed { sense: e.sense, subject, summary: e.summary, confidence: r.confidence.unwrap_or(0.0) });
                }
                "context" => {
                    let about = r.about.clone().unwrap_or_default();
                    let list =
                        || -> Vec<String> { r.why.as_ref().and_then(|w| serde_json::from_value(w.clone()).ok()).unwrap_or_default() };
                    match about.as_str() {
                        "weeknight" => p.habits.weeknight = r.why.as_ref().and_then(|w| w.as_i64()).filter(|m| *m > 0),
                        "weekend" => p.habits.weekend = r.why.as_ref().and_then(|w| w.as_i64()).filter(|m| *m > 0),
                        "times" => p.habits.times = list(),
                        "who" => p.habits.who = list(),
                        "where" => p.habits.r#where = list(),
                        _ => {}
                    }
                    if let Some(n) = &r.note {
                        p.habits.notes.push((about, short(n)));
                    }
                }
                _ => {}
            }
        }
        for g in games {
            if let (Some(stars), Some(at)) = (g.my_rating, g.rated_at) {
                p.rated.push(Rated { id: g.id, name: g.name.clone(), stars, at, features: features(g) });
            }
        }
        p.rated.sort_by_key(|r| (std::cmp::Reverse(r.at), r.id));
        p.favourites.sort_by_key(|f| std::cmp::Reverse(f.minutes));
        p.patterns.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
        p
    }

    /// Player `who`'s profile, from their statements and play diary.
    pub fn load(db: &Db, who: PlayerId, games: &[GameView]) -> Result<Profile> {
        let mut p = Profile::from_rows(&db.taste_statements(who)?, games);
        let diary = super::diary::load_habits(db, who, games, crate::db::now())?;
        p.diary = (!diary.is_empty()).then_some(diary);
        Ok(p)
    }

    pub fn is_favourite(&self, id: i64) -> bool {
        self.favourite_ids.contains(&id)
    }

    /// A hard no UpNext can check (not your own words) that the game is
    /// about. Never a favourite: what you said about the game itself wins.
    pub fn hard_no(&self, g: &GameView) -> Option<&Stated> {
        if self.is_favourite(g.id) {
            return None;
        }
        self.hard_nos.iter().find(|s| super::hard_no_matches(&s.subject, g))
    }

    /// A hard no on a tag that a game you don't own is about, from its
    /// store tags and genres (the wishlist).
    pub fn hard_no_in_store(&self, tags: &[String], genres: &[String]) -> Option<&Stated> {
        self.hard_nos.iter().find(|s| super::hard_no_matches_store(&s.subject, tags, genres))
    }

    /// Hard nos UpNext can check, which leave games out of picks.
    pub fn checks_hard_nos(&self) -> bool {
        self.hard_nos.iter().any(|s| !matches!(s.subject, Subject::Text(_)))
    }

    /// The usual session for the time `now`: what you said, or else what
    /// the play diary shows.
    pub fn usual(&self, now: i64) -> Option<Usual> {
        let when = When::at(now);
        let stated = match when {
            When::Weeknight => self.habits.weeknight,
            When::Weekend => self.habits.weekend,
        };
        if let Some(minutes) = stated {
            return Some(Usual { minutes, when, from_diary: false });
        }
        let d = self.diary.as_ref()?;
        let s = match when {
            When::Weeknight => d.weeknight,
            When::Weekend => d.weekend,
        }?;
        Some(Usual { minutes: s.minutes, when, from_diary: true })
    }

    /// The About me block for the prompt: favourites with why, likes,
    /// dislikes and hard nos, the patterns, your most-played games while
    /// you have few favourites, and the habits that fit now.
    pub fn about_me(&self, games: &[GameView], now: i64) -> String {
        let mut out = String::from("## About me\n");
        let mut said_any = false;
        if !self.favourites.is_empty() {
            said_any = true;
            out.push_str("Favourites, and why I love them:\n");
            for f in self.favourites.iter().take(MAX_FAVOURITES) {
                let mut why = f.why.clone();
                if let Some(n) = &f.note {
                    why.push(format!("\"{}\"", one_line(n, usize::MAX)));
                }
                let name = name_line(&f.name);
                if why.is_empty() {
                    out.push_str(&format!("- {name}\n"));
                } else {
                    out.push_str(&format!("- {name}: {}\n", why.join(", ")));
                }
            }
        }
        let list = |items: &[Stated]| -> String {
            items
                .iter()
                .map(|s| {
                    // "story-driven games", as a pattern says it, else the label.
                    let what = one_line(&s.subject.phrase().map(str::to_string).unwrap_or_else(|| plain(&s.subject)), usize::MAX);
                    match &s.note {
                        Some(n) => format!("{what} (\"{}\")", one_line(n, usize::MAX)),
                        None => what,
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        if !self.likes.is_empty() {
            said_any = true;
            out.push_str(&format!("I like: {}\n", list(&self.likes)));
        }
        if !self.dislikes.is_empty() {
            said_any = true;
            out.push_str(&format!("I dislike: {}\n", list(&self.dislikes)));
        }
        if !self.hard_nos.is_empty() {
            said_any = true;
            out.push_str(&format!("Hard nos, never pick these: {}\n", list(&self.hard_nos)));
        }
        let rated = |keep: fn(&Rated) -> bool, most: usize| -> Vec<String> {
            self.rated.iter().filter(|r| keep(r)).take(most).map(|r| format!("{} ({})", name_line(&r.name), r.stars)).collect()
        };
        let (high, low) = (rated(Rated::high, MAX_RATED_HIGH), rated(Rated::low, MAX_RATED_LOW));
        if !high.is_empty() || !low.is_empty() {
            said_any = true;
            out.push_str("My ratings of games I've played, out of 5:\n");
            if !high.is_empty() {
                out.push_str(&format!("- Loved or liked: {}\n", high.join(", ")));
            }
            if !low.is_empty() {
                out.push_str(&format!("- Disliked or didn't click: {}\n", low.join(", ")));
            }
        }
        if !self.patterns.is_empty() {
            out.push_str("Patterns UpNext counted from the games I stick with and the ones I leave:\n");
            for p in &self.patterns {
                let Some(phrase) = p.subject.phrase() else { continue };
                let verb = match p.sense {
                    Sense::Stick => "I stick with",
                    Sense::Drift => "I drift away from",
                };
                let sure = if p.confidence >= super::SURE { "sure" } else { "fairly sure" };
                out.push_str(&format!("- {verb} {} ({sure}: {})\n", one_line(phrase, usize::MAX), one_line(&p.summary, usize::MAX)));
            }
        }
        if self.favourites.len() < FEW_FAVOURITES {
            let mut played: Vec<&GameView> = games
                .iter()
                .filter(|g| g.playtime_minutes >= 60 && !self.is_favourite(g.id) && g.status != crate::db::Status::NotGame)
                .collect();
            played.sort_by_key(|g| std::cmp::Reverse(g.playtime_minutes));
            let lines: Vec<String> = played
                .iter()
                .take(PLAYED_MOST)
                .map(|g| {
                    let mut tags: Vec<String> = g.genres.iter().take(2).cloned().collect();
                    for t in g.tags.iter().take(2) {
                        if !tags.iter().any(|x| x.eq_ignore_ascii_case(t)) {
                            tags.push(t.clone());
                        }
                    }
                    let h = crate::prompt::hours(g.playtime_minutes);
                    let name = name_line(&g.name);
                    if tags.is_empty() {
                        format!("{name} ({h})")
                    } else {
                        let tags: Vec<String> = tags.iter().map(|t| one_line(t, 40)).collect();
                        format!("{name} ({h}; {})", tags.join(", "))
                    }
                })
                .collect();
            if !lines.is_empty() {
                out.push_str(&format!("Played most, a signal only: {}\n", lines.join("; ")));
            }
        }
        if let Some(h) = self.habits_now(now) {
            out.push_str(&h);
        }
        if !said_any && self.patterns.is_empty() && !out.contains("Played most") {
            out.push_str("(nothing yet)\n");
        }
        out
    }

    /// "Now it's the weekend; my weekend sessions usually run 3h, so with no
    /// time in my request, plan for that. Mostly evenings, with a partner,
    /// on the couch." Personal, so only what you stated.
    fn habits_now(&self, now: i64) -> Option<String> {
        let h = &self.habits;
        let mut parts: Vec<String> = vec![];
        if let Some(u) = self.usual(now) {
            let when = match u.when {
                When::Weeknight => "a weeknight",
                When::Weekend => "the weekend",
            };
            parts.push(format!(
                "Now it's {when}, when my sessions usually run {}{}: with no time in my request, take that as my time budget.",
                hours_text(u.minutes),
                if u.from_diary { " (from my play diary)" } else { "" }
            ));
        }
        let say = |list: &[String], f: fn(&str) -> &str| list.iter().map(|x| f(x)).collect::<Vec<_>>().join(", ");
        fn time(t: &str) -> &str {
            if t == "late_nights" { "late at night" } else { t }
        }
        if !h.times.is_empty() {
            parts.push(format!("I play mostly {}.", say(&h.times, time)));
        } else if let Some(d) = self.diary.as_ref().filter(|d| !d.times.is_empty()) {
            parts.push(format!("I play mostly {} (from my play diary).", say(&d.times, time)));
        }
        if let Some(m) = self.diary.as_ref().and_then(|d| d.multiplayer.as_ref()) {
            let when = match m.weekend_share {
                s if s >= 0.6 => " mostly at the weekend",
                s if s <= 0.4 => " mostly on weeknights",
                _ => "",
            };
            let at = m
                .time
                .as_deref()
                .map(|t| format!(", in the {}", if t == "late_nights" { "late night" } else { t.trim_end_matches('s') }))
                .unwrap_or_default();
            if !when.is_empty() || !at.is_empty() {
                parts.push(format!("My play diary shows multiplayer games get played{when}{at}."));
            }
        }
        if !h.who.is_empty() {
            parts.push(format!(
                "I play {}.",
                say(&h.who, |w| match w {
                    "solo" => "solo",
                    "partner" => "with a partner",
                    "friends" => "with friends",
                    "kids" => "with kids",
                    other => other,
                })
            ));
        }
        if !h.r#where.is_empty() {
            parts.push(format!(
                "I play {}.",
                say(&h.r#where, |w| match w {
                    "desk" => "at a desk",
                    "couch" => "on the couch",
                    "deck" => "on a Steam Deck",
                    other => other,
                })
            ));
        }
        for (about, note) in &h.notes {
            let about = match about.as_str() {
                "times" => "when I play",
                "who" => "who I play with",
                "where" => "where I play",
                other => other,
            };
            parts.push(format!("About {about}: \"{note}\"."));
        }
        (!parts.is_empty()).then(|| format!("Habits: {}\n", parts.join(" ")))
    }

    /// The favourite a game is most like, by what they share (review-tag
    /// values, genre and theme tags, length), with how many things they
    /// share; None for a favourite itself.
    pub fn likeness(&self, g: &GameView, f: &[Subject]) -> Option<(&Favourite, usize)> {
        if self.is_favourite(g.id) {
            return None;
        }
        self.favourites
            .iter()
            .map(|fav| (fav, fav.features.iter().filter(|s| f.contains(s)).count()))
            .filter(|(_, n)| *n > 0)
            .max_by_key(|(fav, n)| (*n, std::cmp::Reverse(fav.id)))
    }
}

impl Profile {
    /// The game you rated 4 or 5 that a game is most like, by what they
    /// share, with how many things; never the game itself.
    pub fn rated_likeness(&self, g: &GameView, f: &[Subject]) -> Option<(&Rated, usize)> {
        self.rated_like(g, f, Rated::high)
    }

    /// The game you rated 1 or 2 that a game is most like, the same way.
    pub fn rated_unlikeness(&self, g: &GameView, f: &[Subject]) -> Option<(&Rated, usize)> {
        self.rated_like(g, f, Rated::low)
    }

    fn rated_like(&self, g: &GameView, f: &[Subject], keep: fn(&Rated) -> bool) -> Option<(&Rated, usize)> {
        self.rated
            .iter()
            .filter(|r| keep(r) && r.id != g.id)
            .map(|r| (r, r.features.iter().filter(|s| f.contains(s)).count()))
            .filter(|(_, n)| *n > 0)
            .max_by_key(|(r, n)| (*n, std::cmp::Reverse(r.id)))
    }

    /// Whether a game is in your lane: a favourite, or about something you
    /// like or a pattern you stick with. A change of pace is a game that's
    /// none of these.
    pub fn in_your_lane(&self, g: &GameView) -> bool {
        if self.is_favourite(g.id) {
            return true;
        }
        let f = features(g);
        self.likes.iter().any(|s| matches_in(&s.subject, g, &f))
            || self.patterns.iter().filter(|p| p.sense == Sense::Stick).any(|p| matches_in(&p.subject, g, &f))
    }

    /// Whether a lineup for this request gets a change of pace: a vague
    /// request (decided 2026-10-02, after the live A/B showed every pick in
    /// one lane), with something on the profile to step out of.
    pub fn wants_change_of_pace(&self, request: &str, count: usize) -> bool {
        count >= CHANGE_OF_PACE_FROM && vague(request) && (!self.likes.is_empty() || self.patterns.iter().any(|p| p.sense == Sense::Stick))
    }
}

/// Lineups this long or longer get a change of pace on a vague request.
pub const CHANGE_OF_PACE_FROM: usize = 3;

/// Whether a game is about a subject, with its features already found.
pub fn matches_in(subject: &Subject, g: &GameView, features: &[Subject]) -> bool {
    match subject {
        Subject::Facet(f) => g.review_tags.as_ref().is_some_and(|t| t.has(f)),
        Subject::Tag(_) | Subject::Length(_) => features.contains(subject),
        Subject::Text(_) => false,
    }
}

/// Whether a request is vague: nothing in it but filler ("anything",
/// "surprise me", "something good"), or nothing at all.
pub fn vague(request: &str) -> bool {
    const FILLER: &[&str] = &[
        "a",
        "an",
        "the",
        "any",
        "anything",
        "something",
        "some",
        "surprise",
        "me",
        "good",
        "great",
        "fun",
        "nice",
        "cool",
        "whatever",
        "random",
        "game",
        "games",
        "to",
        "play",
        "i",
        "want",
        "wanna",
        "pick",
        "for",
        "you",
        "choose",
        "decide",
        "just",
        "please",
        "now",
        "tonight",
        "today",
        "idk",
        "dunno",
        "else",
        "really",
        "new",
        "go",
        "ahead",
        "your",
        "call",
        "fine",
        "ok",
        "okay",
    ];
    request.to_lowercase().split(|c: char| !c.is_alphanumeric() && c != '\'').filter(|w| !w.is_empty()).all(|w| FILLER.contains(&w))
}
