//! The vocabulary of a backlog verdict (`docs/verdicts/verdict.md`): every
//! question, its fixed answers and their words, in one place. The form, the
//! checks on both sides, the published counts, the game page's lines and the
//! prompt's `bv` line are all built from these tables, so they can't drift.
//!
//! Where an answer means what an existing scale means, it says so: `clicked`
//! maps onto review tags' `hook` and `sessions` is review tags' `sessions`,
//! value for value (`review_tags::SCALES`), and `stopped_for` is the
//! game reasons for stopping (`lifecycle::REASONS`). A verdict, a review tag and a stop
//! reason about the same game can then be compared directly.
//!
//! Adding a value or a field is a new vocabulary: bump [`VOCAB`]. Readers keep
//! older verdicts readable and drop values from a newer vocabulary they don't
//! know, as `seed::checked_tags` does for review tags.

use crate::lifecycle::{REASONS, ReasonGroup};
use serde_json::{Value, json};

/// The vocabulary's version, sent with every verdict and in every snapshot.
pub const VOCAB: i64 = 1;

/// One answer: its code, its words in the form, and (for the fields the game
/// page sums up) the "most players" line it reads as when it has a clear
/// majority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    pub code: &'static str,
    pub label: &'static str,
    pub line: Option<&'static str>,
}

const fn choice(code: &'static str, label: &'static str) -> Choice {
    Choice { code, label, line: None }
}

const fn summed(code: &'static str, label: &'static str, line: &'static str) -> Choice {
    Choice { code, label, line: Some(line) }
}

/// One question with a single answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub name: &'static str,
    /// The question as the form asks it.
    pub question: &'static str,
    pub required: bool,
    pub choices: &'static [Choice],
}

impl Field {
    pub fn has(&self, code: &str) -> bool {
        self.choices.iter().any(|c| c.code == code)
    }

    pub fn label(&self, code: &str) -> Option<&'static str> {
        self.choices.iter().find(|c| c.code == code).map(|c| c.label)
    }
}

pub const OUTCOME: Field = Field {
    name: "outcome",
    question: "Where did you leave it?",
    required: true,
    choices: &[
        choice("finished", "Finished it"),
        choice("playing", "Still at it"),
        choice("shelved", "Shelved, meaning to go back"),
        choice("stopped", "Stopped, not going back"),
        choice("endless", "No ending; I play it now and then"),
        choice("bounced", "Didn't get into it"),
    ],
};

/// The outcomes that ask what stopped the player.
pub const STOPPED_OUTCOMES: [&str; 3] = ["shelved", "stopped", "bounced"];

pub const WORTH: Field = Field {
    name: "worth",
    question: "Worth it on a backlog?",
    required: true,
    choices: &[choice("yes", "Yes"), choice("for_some", "If it's your kind of game"), choice("no", "No")],
};

pub const HOURS: Field = Field {
    name: "hours",
    question: "About how long have you played?",
    required: false,
    choices: &[
        choice("under_2", "Under 2 hours"),
        choice("2_5", "2 to 5 hours"),
        choice("5_10", "5 to 10 hours"),
        choice("10_20", "10 to 20 hours"),
        choice("20_40", "20 to 40 hours"),
        choice("40_80", "40 to 80 hours"),
        choice("80_plus", "80 hours or more"),
    ],
};

/// Where each `hours` range starts, in minutes, in [`HOURS`]' order. Exact
/// playtime never leaves the install, only its range.
const HOURS_FROM_MINUTES: [i64; 7] = [0, 120, 300, 600, 1200, 2400, 4800];

/// The `hours` range a playtime falls in.
pub fn hours_range(minutes: i64) -> &'static str {
    let at = HOURS_FROM_MINUTES.iter().rposition(|from| minutes >= *from).unwrap_or(0);
    HOURS.choices[at].code
}

pub const CLICKED: Field = Field {
    name: "clicked",
    question: "When did it click?",
    required: false,
    choices: &[
        summed("first_hour", "In the first hour", "Clicks within the first hour"),
        summed("few_hours", "After a few hours (1 to 3)", "Clicks after a few hours"),
        summed("later", "Later (3 to 10 hours)", "Takes 3 to 10 hours to click"),
        summed("much_later", "Much later (more than 10 hours)", "Takes more than 10 hours to click"),
        summed("never", "It never did", "Never clicks for most players"),
    ],
};

/// What each `clicked` answer reads as on review tags' `hook` scale. Never
/// clicking says nothing about how fast it hooks.
pub fn hook(clicked: &str) -> Option<&'static str> {
    match clicked {
        "first_hour" => Some("fast"),
        "few_hours" => Some("gradual"),
        "later" | "much_later" => Some("slow"),
        _ => None,
    }
}

pub const SESSIONS: Field = Field {
    name: "sessions",
    question: "How long does a session need to be?",
    required: false,
    choices: &[
        summed("short", "Under an hour is fine", "Fine in sessions under an hour"),
        summed("medium", "1 to 2 hours", "Wants sessions of an hour or two"),
        summed("long", "It wants a long sitting", "Wants long sittings"),
    ],
};

/// The longest median session, in minutes, each `sessions` answer covers
/// when the play diary suggests one.
const SESSION_UP_TO_MINUTES: [i64; 2] = [60, 120];

/// The `sessions` answer a median session suggests.
pub fn sessions_for(median_minutes: i64) -> &'static str {
    let at = SESSION_UP_TO_MINUTES.iter().position(|up_to| median_minutes <= *up_to).unwrap_or(2);
    SESSIONS.choices[at].code
}

pub const LIMITED_TIME: Field = Field {
    name: "limited_time",
    question: "Does it respect limited time?",
    required: false,
    choices: &[
        summed("yes", "Yes", "Respects limited time"),
        summed("mostly", "Mostly", "Mostly respects limited time"),
        summed("no", "No", "Hard on limited time"),
    ],
};

/// What helps or hurts with limited time: up to three with a `limited_time`
/// answer. Not a [`Field`], since a verdict can have several.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Factor {
    pub code: &'static str,
    pub helps: bool,
    pub label: &'static str,
    pub line: &'static str,
}

const fn factor(code: &'static str, helps: bool, label: &'static str, line: &'static str) -> Factor {
    Factor { code, helps, label, line }
}

pub const FACTORS: &[Factor] = &[
    factor("save_anywhere", true, "Save anywhere", "You can save anywhere"),
    factor("easy_to_resume", true, "Easy to resume: you remember what you were doing", "Easy to pick back up after a break"),
    factor("short_chunks", true, "Missions or levels that fit a session", "Missions or levels fit a session"),
    factor("recap", true, "A journal, recap or clear objectives", "Keeps you on track with a journal or recap"),
    factor("hard_to_resume", false, "Hard to resume: a week off and you're lost", "Hard to pick back up after a break"),
    factor("long_stretches", false, "Long stretches without a save or a stopping point", "Long stretches without a save"),
    factor("padding", false, "Filler, backtracking or grind", "Padded with filler or grind"),
    factor("obligations", false, "Dailies, seasons or events that punish missing a week", "Punishes missing a week"),
    factor("needs_a_group", false, "Needs friends or a schedule", "Needs friends or a schedule"),
];

pub fn is_factor(code: &str) -> bool {
    FACTORS.iter().any(|f| f.code == code)
}

pub const STOP_POINT: Field = Field {
    name: "stop_point",
    question: "When is it fine to stop?",
    required: false,
    choices: &[
        summed("at_credits", "After the main story; the rest is optional", "Fine to stop after the credits"),
        summed("before_endgame", "Once the endgame or postgame starts", "Fine to stop when the endgame starts"),
        summed("anytime", "Any time: no ending to reach, or nothing builds to one", "Fine to stop any time"),
        summed("push_through", "Push through: stopping early misses what makes it good", "Worth pushing through to the end"),
    ],
};

/// What stopped a player: the game reasons for stopping, never the life reasons
/// (got busy, something else came along, not in the mood, played it
/// elsewhere), which are about the person, not the game.
pub fn stop_reasons() -> impl Iterator<Item = &'static crate::lifecycle::Reason> {
    REASONS.iter().filter(|r| r.group == ReasonGroup::Game)
}

pub fn is_stop_reason(code: &str) -> bool {
    stop_reasons().any(|r| r.code == code)
}

/// The single-answer fields, in the form's order.
pub const FIELDS: [&Field; 7] = [&OUTCOME, &WORTH, &HOURS, &CLICKED, &SESSIONS, &LIMITED_TIME, &STOP_POINT];

/// The vocabulary as the contract's `fields`, in the form's order, with the
/// multi-answer lists after the field they belong to.
pub fn json() -> Value {
    let choices = |f: &Field| {
        f.choices
            .iter()
            .map(|c| {
                let mut v = json!({"code": c.code, "label": c.label});
                if let Some(line) = c.line {
                    v["line"] = json!(line);
                }
                // The review tag each answer agrees with, so the two can be compared.
                let tag = match f.name {
                    "clicked" => hook(c.code).map(|h| format!("hook:{h}")),
                    "sessions" => Some(format!("sessions:{}", c.code)),
                    _ => None,
                };
                if let Some(tag) = tag {
                    v["review_tag"] = json!(tag);
                }
                v
            })
            .collect::<Vec<_>>()
    };
    let one = |f: &Field| json!({"name": f.name, "question": f.question, "required": f.required, "many": false, "choices": choices(f)});
    let mut out: Vec<Value> = vec![];
    for f in FIELDS {
        out.push(one(f));
        if f.name == LIMITED_TIME.name {
            out.push(json!({
                "name": "factors",
                "question": "What helps or hurts?",
                "required": false,
                "many": true,
                "with": LIMITED_TIME.name,
                "choices": FACTORS.iter().map(|x| json!({"code": x.code, "label": x.label, "line": x.line, "helps": x.helps})).collect::<Vec<_>>(),
            }));
        }
    }
    out.push(json!({"name": "stop_note", "question": "When exactly?", "required": false, "text": true, "with": STOP_POINT.name, "max_chars": super::verdict::STOP_NOTE_MAX}));
    out.push(json!({
        "name": "stopped_for",
        "question": "What stopped you?",
        "required": false,
        "many": true,
        "only_for": STOPPED_OUTCOMES,
        "choices": stop_reasons().map(|r| json!({"code": r.code, "label": r.label, "aspects": r.aspects})).collect::<Vec<_>>(),
    }));
    out.push(json!({"name": "text", "question": "Anything else a busy player should know?", "required": false, "text": true, "max_chars": super::verdict::TEXT_MAX}));
    Value::Array(out)
}
