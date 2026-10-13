//! One backlog verdict as it is sent (`docs/verdicts/verdict.md`), and the checks
//! both sides run on it.
//!
//! [`Verdict`] is the allowlist: the install builds one with only these
//! fields, and the service refuses a body with any other, as the shared data's
//! reader does. Nothing here can carry a Steam ID, a name, a rating, exact playtime,
//! a time finer than a day or anything from settings, and the typed text is
//! checked for the ways people put contact details in by accident.

use super::consent::{CONSENT_VERSION, LICENCE};
use super::vocab::{self, Field, STOPPED_OUTCOMES, VOCAB};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::LazyLock;

/// The longest text, in characters (Unicode scalar values, not bytes or
/// UTF-16 units).
pub const TEXT_MAX: usize = 500;
/// The longest note on when it's fine to stop ("after chapter 3").
pub const STOP_NOTE_MAX: usize = 80;
/// The most helps-or-hurts factors one verdict picks.
pub const MAX_FACTORS: usize = 3;
/// The longest UpNext version a verdict carries.
pub const CLIENT_MAX: usize = 32;
/// The oldest consent version the checks accept. Raised when a change to
/// the consent text must be accepted before anyone sends again.
pub const OLDEST_CONSENT: i64 = 1;

/// A player's key starts with this, so people and secret scanners know it,
/// as API tokens start with `upnext_`. 64 lowercase hex characters (32
/// random bytes) follow; the service keeps only the SHA-256 of the whole key.
pub const KEY_PREFIX: &str = "upnext_v_";

/// Why a text shown in the app can be reported, with the words the menu uses.
pub const REPORT_REASONS: [(&str, &str); 4] =
    [("spam", "Spam"), ("personal", "Personal information"), ("abuse", "Abuse or hate"), ("off_topic", "Not about this game")];

/// One verdict as it is sent. The answers are codes from [`vocab`]; the two
/// required ones are options here so a draft can leave them unanswered and
/// [`Verdict::check`] says so.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verdict {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steam_appid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gog_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worth: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hours: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clicked: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sessions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limited_time: Option<String>,
    /// What helps or hurts, with a `limited_time` answer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub factors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_point: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_note: Option<String>,
    /// Game reasons only, for shelved, stopped and bounced.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stopped_for: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The vocabulary the answers are from.
    pub vocab: i64,
    /// The licence the player accepted, [`LICENCE`].
    pub licence: String,
    /// The consent text's version the player accepted.
    pub consent: i64,
    /// The UpNext version that sent it, for finding a bad client. Never
    /// published.
    pub client: String,
}

impl Verdict {
    /// An empty verdict about a game, with the vocabulary, licence, consent
    /// and version this build sends.
    pub fn new(steam_appid: Option<u32>, gog_id: Option<String>) -> Self {
        Verdict {
            steam_appid,
            gog_id,
            vocab: VOCAB,
            licence: LICENCE.into(),
            consent: CONSENT_VERSION,
            client: env!("CARGO_PKG_VERSION").into(),
            ..Default::default()
        }
    }

    /// Everything wrong with it, the answers first; empty when it can be
    /// sent. The service runs the same checks.
    pub fn check(&self) -> Vec<Problem> {
        let mut out = vec![];
        let mut bad = |field: &'static str, why: String| out.push(Problem { field, why });
        if self.steam_appid == Some(0) {
            bad("steam_appid", "isn't a Steam appid".into());
        }
        if let Some(g) = &self.gog_id
            && !is_gog_id(g)
        {
            bad("gog_id", "isn't a GOG id: digits only".into());
        }
        if self.steam_appid.is_none() && self.gog_id.is_none() {
            bad("game", "a verdict needs the game's Steam appid or GOG id".into());
        }
        let answers = [
            (&vocab::OUTCOME, &self.outcome),
            (&vocab::WORTH, &self.worth),
            (&vocab::HOURS, &self.hours),
            (&vocab::CLICKED, &self.clicked),
            (&vocab::SESSIONS, &self.sessions),
            (&vocab::LIMITED_TIME, &self.limited_time),
            (&vocab::STOP_POINT, &self.stop_point),
        ];
        for (f, v) in answers {
            if let Some(why) = answer(f, v) {
                bad(f.name, why);
            }
        }
        if !self.factors.is_empty() && self.limited_time.is_none() {
            bad("factors", "go with an answer to whether it respects limited time".into());
        }
        if self.factors.len() > MAX_FACTORS {
            bad("factors", format!("at most {MAX_FACTORS}"));
        }
        if let Some(f) = self.factors.iter().find(|f| !vocab::is_factor(f)) {
            bad("factors", format!("{f:?} isn't one of them"));
        }
        if has_repeats(&self.factors) {
            bad("factors", "each at most once".into());
        }
        if let Some(note) = &self.stop_note {
            if self.stop_point.is_none() {
                bad("stop_note", "goes with an answer to when it's fine to stop".into());
            }
            if let Err(why) = check_typed(note, STOP_NOTE_MAX) {
                bad("stop_note", why);
            }
        }
        if !self.stopped_for.is_empty() && self.outcome.as_deref().is_some_and(|o| !STOPPED_OUTCOMES.contains(&o)) {
            bad("stopped_for", "is only for games shelved, stopped or bounced".into());
        }
        if let Some(r) = self.stopped_for.iter().find(|r| !vocab::is_stop_reason(r)) {
            let life = crate::lifecycle::REASONS.iter().any(|x| x.code == r.as_str());
            bad(
                "stopped_for",
                if life { format!("{r:?} is about you, not the game, and is never sent") } else { format!("{r:?} isn't one of them") },
            );
        }
        if has_repeats(&self.stopped_for) {
            bad("stopped_for", "each at most once".into());
        }
        if let Some(text) = &self.text
            && let Err(why) = check_typed(text, TEXT_MAX)
        {
            bad("text", why);
        }
        if !(1..=VOCAB).contains(&self.vocab) {
            bad("vocab", format!("{} isn't a vocabulary this build knows (1 to {VOCAB})", self.vocab));
        }
        if self.licence != LICENCE {
            bad("licence", format!("verdicts are published under {LICENCE} only"));
        }
        if !(OLDEST_CONSENT..=CONSENT_VERSION).contains(&self.consent) {
            bad("consent", format!("consent version {} isn't accepted ({OLDEST_CONSENT} to {CONSENT_VERSION})", self.consent));
        }
        if self.client.is_empty()
            || self.client.len() > CLIENT_MAX
            || !self.client.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
        {
            bad("client", "isn't an UpNext version".into());
        }
        out
    }
}

/// One thing wrong with a verdict, said so the form can show it beside the
/// field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    /// The field's name, or `game` for the missing ids.
    pub field: &'static str,
    pub why: String,
}

/// What's wrong with one answer, if anything.
fn answer(f: &Field, v: &Option<String>) -> Option<String> {
    match v {
        None if f.required => Some("is required".into()),
        Some(code) if !f.has(code) => Some(format!("{code:?} isn't one of its answers")),
        _ => None,
    }
}

fn has_repeats(list: &[String]) -> bool {
    list.iter().enumerate().any(|(i, x)| list[..i].contains(x))
}

/// GOG's product ids are numbers.
pub fn is_gog_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit())
}

/// A check on typed text: what it refuses, why in the form's words, and the
/// pattern. Patterns are case-insensitive and use nothing beyond what both
/// Rust's `regex` and JavaScript's `RegExp` (without the `u` flag) read the
/// same way: other characters are written as four-digit `\uXXXX` escapes,
/// so the service runs these exact strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextCheck {
    pub code: &'static str,
    pub why: &'static str,
    pub pattern: &'static str,
}

/// In order: the first that matches is the one the form names.
///
/// `control` refuses control characters other than new lines, tabs and
/// carriage returns, and the characters that reorder text as it's shown
/// (bidirectional embeddings, overrides and isolates), which could make a
/// reversed address read the right way round. `wide` refuses full-width
/// letters, digits, `＠`, `．` and `／`, which read like their ordinary
/// forms but would slip past every pattern after it; full-width `：`, `！`,
/// `？` and the like that Chinese and Japanese text uses stay allowed.
pub const TEXT_CHECKS: [TextCheck; 8] = [
    TextCheck {
        code: "control",
        why: "has invisible control characters",
        pattern: r"[\x00-\x08\x0b\x0c\x0e-\x1f\x7f-\x9f\u202a-\u202e\u2066-\u206f\ufff9-\ufffb]",
    },
    TextCheck { code: "wide", why: "has full-width letters or digits", pattern: r"[\uff0e-\uff19\uff20-\uff3a\uff41-\uff5a]" },
    TextCheck { code: "steam_profile", why: "has a Steam profile or link", pattern: r"steamcommunity|s\.team/|steam://" },
    TextCheck { code: "email", why: "has an email address", pattern: r"[a-z0-9._%+-]+@[a-z0-9-]+(\.[a-z0-9-]+)*\.[a-z]{2,}" },
    TextCheck {
        code: "link",
        why: "has a link or a web address",
        pattern: r"[a-z][a-z0-9+.-]*://|www\.|[a-z0-9-]+\.(com|net|org|io|gg|tv|me|co|ly|app|dev|xyz|info|ru|de|uk|fr|jp|cn|br|au|ca|eu|us|to|cc|link|site|online|club|live|store|shop|gl)([^a-z0-9]|$)",
    },
    TextCheck { code: "handle", why: "has an @handle", pattern: r"(^|[^a-z0-9_.@])@[a-z0-9_]{2,}" },
    TextCheck { code: "long_number", why: "has a long number, which could be a Steam ID", pattern: r"[0-9]{17}" },
    TextCheck { code: "phone", why: "has what looks like a phone number", pattern: r"\+?\(?[0-9]([ ().-]{0,2}[0-9]){8,}" },
];

/// Characters that show as nothing and change nothing about the words: the
/// soft hyphen, the combining grapheme joiner, the Mongolian vowel
/// separator, the zero-width space, non-joiner and joiner, the left-to-right
/// and right-to-left marks (and Arabic's), the word joiner and invisible
/// operators, and the byte order mark. They're removed before the
/// [`TEXT_CHECKS`] run, so "example" with a zero-width space before ".com"
/// is still a link, while the joiners that emoji, Persian and Indic scripts
/// need stay in the text that's sent.
pub const IGNORED: &str = r"[\u00ad\u034f\u061c\u180e\u200b-\u200f\u2060-\u2064\ufeff]";

static TEXT_REGEXES: LazyLock<Vec<Regex>> =
    LazyLock::new(|| TEXT_CHECKS.iter().map(|c| Regex::new(&format!("(?i){}", c.pattern)).expect("a valid pattern")).collect());

static IGNORED_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(IGNORED).expect("a valid pattern"));

/// The first check typed text fails, if any, once the [`IGNORED`]
/// characters are taken out.
pub fn text_refusal(text: &str) -> Option<&'static TextCheck> {
    let text = IGNORED_REGEX.replace_all(text, "");
    TEXT_CHECKS.iter().zip(TEXT_REGEXES.iter()).find(|(_, re)| re.is_match(&text)).map(|(c, _)| c)
}

/// Check typed text: not blank, at most `max` characters, and none of the
/// [`TEXT_CHECKS`]. Says which part is refused rather than dropping it.
pub fn check_typed(text: &str, max: usize) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("is empty; leave it out instead".into());
    }
    let n = text.chars().count();
    if n > max {
        return Err(format!("is {n} characters; at most {max}"));
    }
    match text_refusal(text) {
        Some(c) => Err(format!("{}; take it out to send", c.why)),
        None => Ok(()),
    }
}

/// The checks as the contract's `text_checks`.
pub fn text_checks_json() -> Value {
    Value::Array(TEXT_CHECKS.iter().map(|c| json!({"code": c.code, "why": c.why, "pattern": c.pattern, "flags": "i"})).collect())
}
