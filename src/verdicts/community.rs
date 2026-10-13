//! Everyone's backlog verdicts about a game, as an install reads them from
//! the snapshot (`docs/verdicts/reading.md`).
//!
//! A game's published counts become, without an AI call:
//! - [`Backlog`], what a game carries through the library: the headline
//!   from `worth`, the "most players" lines, and the facets picks and the
//!   library's filters read. Only answers with a clear majority (at least
//!   half of the game's verdicts, and at least [`PERCENT_AT`] of them) say
//!   anything about what the game is like.
//! - [`GameVerdicts`], the game page's section: the same, how far people
//!   got with the typical hours of each outcome, and the newest texts.
//! - The prompt's `bv` line, the pick card's line and Second Chance's line.
//!
//! The words come from the vocabulary's own tables (`vocab.rs`), so the
//! form and the game page say the same thing.

use super::format::{Counts, GameCounts, GameSummary, PERCENT_AT, SHOW_AT, ShownText};
use super::vocab::{self, FACTORS};
use serde::{Deserialize, Serialize};

/// A verdict facet's weight in picks grows with the game's verdicts up to
/// this many, then stops: four verdicts barely move a game, twenty count
/// fully.
pub const FULL_WEIGHT_AT: u32 = 20;

/// The outcomes that ask what stopped the player, as the counts group them.
const STOPPED: [&str; 3] = vocab::STOPPED_OUTCOMES;

/// What a game carries through the library and picks: a summary of its
/// verdicts, worked out once when the library is read.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Backlog {
    /// Verdicts counted.
    pub n: u32,
    /// "Worth it on a backlog, say 31 of 42 players".
    pub headline: String,
    /// The headline for a table cell: "Worth it, 31 of 42".
    pub short: String,
    /// Up to four "most players" lines, from 5 verdicts: "Clicks within the first hour".
    pub lines: Vec<String>,
    /// What a clear majority answered, for picks and filters:
    /// `clicked:first_hour`, `sessions:short`, `limited_time:yes`,
    /// `stop_point:at_credits`, `outcome:finished`. Empty under 5 verdicts.
    pub facets: Vec<String>,
    /// The counts as published, for the prompt line and Second Chance.
    #[serde(skip)]
    #[schema(ignore)]
    pub counts: GameCounts,
}

/// One answer's share of a game's verdicts, when it's a clear majority.
fn majority(counts: &Counts, n: u32) -> Option<(&str, u32)> {
    if n < PERCENT_AT {
        return None;
    }
    counts
        .iter()
        .max_by_key(|(code, k)| (**k, std::cmp::Reverse(code.as_str())))
        .filter(|(_, k)| **k * 2 >= n)
        .map(|(c, k)| (c.as_str(), *k))
}

fn count(counts: &Counts, code: &str) -> u32 {
    counts.get(code).copied().unwrap_or(0)
}

impl Backlog {
    /// A game's summary from its published line, or None under [`SHOW_AT`].
    pub fn of(n: u32, counts: GameCounts) -> Option<Self> {
        if n < SHOW_AT {
            return None;
        }
        let (headline, short) = headline(n, &counts.worth);
        let mut b = Backlog { n, headline, short, lines: vec![], facets: vec![], counts };
        b.lines = b.most_players();
        b.facets = b.majorities();
        Some(b)
    }

    /// The "most players" lines: when it clicks, the session it wants, what
    /// helps or hurts with limited time (else the answer itself), and when
    /// it's fine to stop.
    fn most_players(&self) -> Vec<String> {
        let c = &self.counts;
        let line = |f: &vocab::Field, counts: &Counts| {
            majority(counts, self.n).and_then(|(code, _)| f.choices.iter().find(|ch| ch.code == code)).and_then(|ch| ch.line)
        };
        let factor = majority(&c.factors, self.n).and_then(|(code, _)| FACTORS.iter().find(|f| f.code == code)).map(|f| f.line);
        [
            line(&vocab::CLICKED, &c.clicked),
            line(&vocab::SESSIONS, &c.sessions),
            factor.or_else(|| line(&vocab::LIMITED_TIME, &c.limited_time)),
            line(&vocab::STOP_POINT, &c.stop_point),
        ]
        .into_iter()
        .flatten()
        .map(String::from)
        .collect()
    }

    fn majorities(&self) -> Vec<String> {
        let c = &self.counts;
        [
            ("clicked", &c.clicked),
            ("sessions", &c.sessions),
            ("limited_time", &c.limited_time),
            ("stop_point", &c.stop_point),
            ("outcome", &c.outcome),
        ]
        .into_iter()
        .filter_map(|(name, counts)| majority(counts, self.n).map(|(code, _)| format!("{name}:{code}")))
        .collect()
    }

    pub fn has(&self, facet: &str) -> bool {
        self.facets.iter().any(|f| f == facet)
    }

    /// How much a facet of this game counts in picks, from 0 to 1.
    pub fn weight(&self) -> f64 {
        f64::from(self.n.min(FULL_WEIGHT_AT)) / f64::from(FULL_WEIGHT_AT)
    }

    /// The library's "Respects limited time": a clear majority said yes or mostly.
    pub fn respects_time(&self) -> bool {
        let c = &self.counts.limited_time;
        self.n >= PERCENT_AT && (count(c, "yes") + count(c, "mostly")) * 2 >= self.n
    }

    /// The library's "Clicks fast": a clear majority said it clicked in the first hour.
    pub fn clicks_fast(&self) -> bool {
        self.has("clicked:first_hour")
    }

    /// The verdicts that say the player stopped (shelved, stopped, bounced).
    pub fn stopped(&self) -> u32 {
        STOPPED.iter().map(|o| count(&self.counts.outcome, o)).sum()
    }

    /// Of those, how many stopped for this game reason.
    pub fn stopped_for(&self, reason: &str) -> u32 {
        count(&self.counts.stopped_for, reason)
    }

    /// The game reasons at least a third of the stopped verdicts give, from
    /// three stopped verdicts, most given first.
    pub fn stopped_mostly_for(&self) -> Vec<&str> {
        let stopped = self.stopped();
        if stopped < SHOW_AT {
            return vec![];
        }
        let mut reasons: Vec<(&str, u32)> =
            self.counts.stopped_for.iter().filter(|(_, k)| **k * 3 >= stopped).map(|(r, k)| (r.as_str(), *k)).collect();
        reasons.sort_by_key(|(r, k)| (std::cmp::Reverse(*k), *r));
        reasons.into_iter().map(|(r, _)| r).collect()
    }

    /// The prompt's line, from [`PERCENT_AT`] verdicts, about 15 tokens:
    /// `bv n=23 worth 78% click first_hour sessions short limited yes stop credits stopped grind`.
    pub fn bv_line(&self) -> Option<String> {
        if self.n < PERCENT_AT {
            return None;
        }
        let c = &self.counts;
        let mut out = format!("bv n={} worth {}%", self.n, (count(&c.worth, "yes") * 100 + self.n / 2) / self.n);
        let mut add = |word: &str, counts: &Counts, short: fn(&str) -> &str| {
            if let Some((code, _)) = majority(counts, self.n) {
                out.push_str(&format!(" {word} {}", short(code)));
            }
        };
        add("click", &c.clicked, |c| c);
        add("sessions", &c.sessions, |c| c);
        add("limited", &c.limited_time, |c| c);
        add("stop", &c.stop_point, stop_word);
        let reasons = self.stopped_mostly_for();
        if !reasons.is_empty() {
            out.push_str(&format!(" stopped {}", reasons.join(",")));
        }
        Some(out)
    }

    /// The pick card's line, from [`PERCENT_AT`] verdicts: two things most
    /// players say, those the request asks about first. "Backlog players:
    /// clicks fast, fine in short sessions (23)".
    pub fn card_line(&self, asked: &[&str]) -> Option<String> {
        if self.n < PERCENT_AT {
            return None;
        }
        let short = asked.iter().any(|f| f.starts_with("sessions:short") || f.starts_with("limited_time:"));
        let finish = asked.iter().any(|f| f.starts_with("stop_point:") || f.starts_with("outcome:"));
        let c = &self.counts;
        let said = |counts: &Counts, words: fn(&str) -> Option<&'static str>| majority(counts, self.n).and_then(|(code, _)| words(code));
        let clicked = said(&c.clicked, |code| match code {
            "first_hour" => Some("clicks fast"),
            "few_hours" => Some("clicks after a few hours"),
            "later" | "much_later" => Some("slow to click"),
            _ => Some("never clicks for most"),
        });
        let sessions = said(&c.sessions, |code| match code {
            "short" => Some("fine in short sessions"),
            "medium" => Some("wants an hour or two"),
            _ => Some("wants long sittings"),
        });
        let limited = said(&c.limited_time, |code| match code {
            "yes" => Some("respects limited time"),
            "mostly" => Some("mostly respects limited time"),
            _ => Some("hard on limited time"),
        });
        let stop = said(&c.stop_point, |code| match code {
            "at_credits" => Some("fine to stop at the credits"),
            "before_endgame" => Some("fine to stop at the endgame"),
            "anytime" => Some("fine to stop any time"),
            _ => Some("worth pushing through"),
        });
        let finished = count(&c.outcome, "finished");
        let finished = (finished * 2 >= self.n).then(|| format!("{}% finished it", (finished * 100 + self.n / 2) / self.n));
        let order: Vec<Option<String>> = if finish {
            vec![finished, stop.map(String::from), clicked.map(String::from), sessions.map(String::from)]
        } else if short {
            vec![sessions.map(String::from), limited.map(String::from), clicked.map(String::from), stop.map(String::from)]
        } else {
            vec![clicked.map(String::from), sessions.map(String::from), limited.map(String::from), stop.map(String::from)]
        };
        let bits: Vec<String> = order.into_iter().flatten().take(2).collect();
        (!bits.is_empty()).then(|| format!("Backlog players: {} ({})", bits.join(", "), self.n))
    }

    /// Second Chance's line for a game the player abandoned, when enough
    /// players stopped too: "Of the 12 backlog players who stopped, 9 stopped
    /// for the grind, as you did."
    pub fn second_chance_line(&self, own_reasons: &[String]) -> Option<String> {
        let stopped = self.stopped();
        let reason = *self.stopped_mostly_for().first()?;
        let k = self.stopped_for(reason);
        let as_you = if own_reasons.iter().any(|r| r == reason) { ", as they did" } else { "" };
        Some(format!("Of the {stopped} backlog players who stopped, {k} stopped for {}{as_you}.", reason_words(reason)))
    }
}

/// The headline and its short form, from `worth`.
fn headline(n: u32, worth: &Counts) -> (String, String) {
    let (yes, some, no) = (count(worth, "yes"), count(worth, "for_some"), count(worth, "no"));
    if yes * 2 >= n {
        (format!("Worth it on a backlog, say {yes} of {n} players"), format!("Worth it, {yes} of {n}"))
    } else if no * 2 >= n {
        (format!("Not worth it on a backlog, say {no} of {n} players"), format!("Not worth it, {no} of {n}"))
    } else if some >= yes && some >= no {
        (format!("Mixed: {some} of {n} say only if it's your kind of game"), format!("Mixed, {n} players"))
    } else {
        (format!("Mixed: {yes} of {n} say it's worth it on a backlog"), format!("Mixed, {n} players"))
    }
}

/// The prompt's word for a stopping point.
fn stop_word(code: &str) -> &str {
    match code {
        "at_credits" => "credits",
        "before_endgame" => "endgame",
        "push_through" => "push",
        other => other,
    }
}

/// A game reason after "stopped for".
pub fn reason_words(code: &str) -> &'static str {
    match code {
        "grind" => "the grind",
        "stuck" => "getting stuck",
        "lost_story" => "losing the story",
        "too_long" => "the length",
        "pacing" => "the pacing",
        "bugs" => "bugs",
        "too_hard" => "the difficulty",
        "boring" => "getting bored",
        _ => "something else",
    }
}

/// An hours range in a sentence: "10 to 20 h".
fn hours_words(code: &str) -> &'static str {
    match code {
        "under_2" => "under 2 h",
        "2_5" => "2 to 5 h",
        "5_10" => "5 to 10 h",
        "10_20" => "10 to 20 h",
        "20_40" => "20 to 40 h",
        "40_80" => "40 to 80 h",
        _ => "80 h or more",
    }
}

/// An outcome as "How far people got" names it.
fn outcome_words(code: &str) -> &'static str {
    match code {
        "finished" => "Finished",
        "playing" => "Still playing",
        "shelved" => "Shelved",
        "stopped" => "Stopped",
        "endless" => "Plays it now and then",
        _ => "Didn't get into it",
    }
}

/// The request's facets a verdict can speak to, for one word of it: review
/// tags' `sessions` facets and `hook:fast` as the verdicts say them, and the
/// words for finishing and for limited time, which review tags don't have.
pub fn wished(prev: Option<&str>, word: &str, negated: bool, review_facets: &[&str]) -> Vec<&'static str> {
    if negated {
        return vec![];
    }
    let mut out: Vec<&'static str> = vec![];
    for f in review_facets {
        let v = match *f {
            "sessions:short" => Some("sessions:short"),
            "sessions:medium" => Some("sessions:medium"),
            "sessions:long" => Some("sessions:long"),
            "hook:fast" => Some("clicked:first_hour"),
            _ => None,
        };
        out.extend(v);
    }
    let two = prev.map(|p| format!("{p} {word}"));
    let says = |words: &[&str]| {
        words.iter().any(|w| if w.contains(' ') { two.as_deref().is_some_and(|t| t.starts_with(w)) } else { word.starts_with(w) })
    };
    if says(&["finish", "complet", "beatable", "wrap up", "see through", "an ending", "the end"]) {
        out.extend(["stop_point:at_credits", "outcome:finished"]);
    }
    if says(&["busy", "drop in", "dip in", "drop-in", "weeknight", "little time", "limited time", "spare time", "my time"]) {
        out.push("limited_time:yes");
    }
    out.dedup();
    out
}

/// A facet as a reason names it: "players say it clicks in the first hour".
pub fn facet_reason(facet: &str) -> Option<&'static str> {
    Some(match facet {
        "clicked:first_hour" => "it clicks in the first hour",
        "sessions:short" => "it's fine in sessions under an hour",
        "sessions:medium" => "it suits sessions of an hour or two",
        "sessions:long" => "it wants long sittings",
        "limited_time:yes" => "it respects limited time",
        "stop_point:at_credits" => "it's fine to stop after the credits",
        "outcome:finished" => "most finish it",
        _ => return None,
    })
}

/// One outcome in "How far people got".
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
pub struct OutcomeShare {
    pub outcome: String,
    /// "Finished: usually 10 to 20 h", "Stopped: usually 2 to 5 h, mostly for the grind".
    pub label: String,
    pub count: u32,
}

/// A game's verdicts as its page shows them.
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
pub struct GameVerdicts {
    pub n: u32,
    pub headline: String,
    /// Up to four "most players" lines, from 5 verdicts.
    pub lines: Vec<String>,
    /// Under 5 verdicts, the counts as counts: "2 say worth it, 1 says no".
    pub counts_line: Option<String>,
    /// How far people got, most common first.
    pub outcomes: Vec<OutcomeShare>,
    /// Newest month first.
    pub texts: Vec<ShownTextView>,
}

/// A text as the game page shows it, with its answers in words.
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
pub struct ShownTextView {
    /// The verdict's id, for reporting it.
    pub id: String,
    pub outcome: String,
    /// "Finished it · 10 to 20 hours · Worth it: Yes".
    pub about: String,
    pub text: Option<String>,
    pub stop_note: Option<String>,
    /// YYYY-MM.
    pub month: String,
}

impl GameVerdicts {
    pub fn of(g: &GameSummary) -> Option<Self> {
        let b = Backlog::of(g.n, g.counts.clone())?;
        let c = &g.counts;
        let counts_line = (g.n < PERCENT_AT).then(|| {
            let (yes, some, no) = (count(&c.worth, "yes"), count(&c.worth, "for_some"), count(&c.worth, "no"));
            let mut bits = vec![];
            for (k, one, many) in [
                (yes, "says it's worth it", "say it's worth it"),
                (some, "says only if it's your kind of game", "say only if it's your kind of game"),
                (no, "says no", "say no"),
            ] {
                if k > 0 {
                    bits.push(format!("{k} {}", if k == 1 { one } else { many }));
                }
            }
            bits.join(", ")
        });
        let mut outcomes: Vec<(&String, &u32)> = c.outcome.iter().collect();
        outcomes.sort_by_key(|(o, k)| (std::cmp::Reverse(**k), vocab::OUTCOME.choices.iter().position(|ch| ch.code == o.as_str())));
        let stopped_most = outcomes.iter().find(|(o, _)| STOPPED.contains(&o.as_str())).map(|(o, _)| o.as_str());
        let reasons = b.stopped_mostly_for();
        let outcomes = outcomes
            .into_iter()
            .map(|(o, k)| {
                let mut label = outcome_words(o).to_string();
                let hours = c.hours_by_outcome.get(o);
                let usual = hours.and_then(|h| {
                    let total: u32 = h.values().sum();
                    h.iter().max_by_key(|(code, k)| (**k, std::cmp::Reverse(code.as_str()))).filter(|(_, k)| **k * 2 > total)
                });
                let mut bits = vec![];
                if let Some((code, _)) = usual {
                    bits.push(format!("usually {}", hours_words(code)));
                }
                if Some(o.as_str()) == stopped_most
                    && let Some(r) = reasons.first()
                {
                    bits.push(format!("mostly for {}", reason_words(r)));
                }
                if !bits.is_empty() {
                    label = format!("{label}: {}", bits.join(", "));
                }
                OutcomeShare { outcome: o.clone(), label, count: *k }
            })
            .collect();
        Some(GameVerdicts {
            n: b.n,
            headline: b.headline,
            lines: b.lines,
            counts_line,
            outcomes,
            texts: g.texts.iter().map(ShownTextView::of).collect(),
        })
    }
}

impl ShownTextView {
    fn of(t: &ShownText) -> Self {
        let mut about = vec![vocab::OUTCOME.label(&t.outcome).unwrap_or(&t.outcome).to_string()];
        if let Some(h) = t.hours.as_deref().and_then(|h| vocab::HOURS.label(h)) {
            about.push(h.to_string());
        }
        if let Some(w) = vocab::WORTH.label(&t.worth) {
            about.push(format!("Worth it: {w}"));
        }
        ShownTextView {
            id: t.id.clone(),
            outcome: t.outcome.clone(),
            about: about.join(" · "),
            text: t.text.clone(),
            stop_note: t.stop_note.clone(),
            month: t.month.clone(),
        }
    }
}
