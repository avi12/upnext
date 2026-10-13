//! Backlog verdicts: players answer, for everyone, how a game fits
//! a backlog and limited time, and every install that turns it on reads
//! everyone's answers from a daily CC0 snapshot.
//!
//! This is the contract both sides share, with no network and no schema:
//! - [`vocab`]: the questions and their fixed answers, the one source the
//!   form, validation, the dataset, the labels and the prompt lines come from.
//! - [`consent`]: the licence and the consent text a player accepts before
//!   their first verdict, with its version.
//! - [`verdict`]: one verdict as it is sent, an allowlist, and its checks
//!   (the answers against the vocabulary, the typed text against links,
//!   addresses, handles and Steam IDs).
//! - [`format`]: the published snapshot (a manifest, a summary file installs
//!   download whole, and a file of every verdict), read strictly and written
//!   for fixtures.
//! - [`draft`]: a verdict pre-filled from a player's own marks, playtime,
//!   play diary and stop reasons.
//!
//! - [`client`]: the service's endpoint as UpNext calls it, with the
//!   player's key.
//! - [`mine`]: a player's own verdicts: the form, drafts, the consent,
//!   sending, withdrawing and reporting.
//! - [`receive`]: the daily download of everyone's verdicts, and
//!   [`community`], what a game's counts say on its page, in the library and
//!   in picks.
//!
//! `docs/VERDICTS.md` describes the endpoint and the format in prose, and
//! `docs/verdicts-contract.json` is [`contract`] written out, so the service
//! in `services/verdicts/` checks with the same tables.

pub mod client;
pub mod community;
pub mod consent;
pub mod draft;
pub mod format;
pub mod mine;
pub mod receive;
pub mod verdict;
pub mod vocab;

use serde_json::{Value, json};

/// The project's own service, where `verdicts_url` points unless it's set:
/// its endpoint and its daily snapshot share this address. Nothing is sent
/// there until a player presses Send, and nothing is downloaded until
/// "Backlog verdicts from other players" is on.
pub const DEFAULT_URL: &str = "https://verdicts.upnext.games";

/// The contract as JSON: the vocabulary with its labels, the limits, the
/// text checks and the consent text. `upnext verdicts contract` prints it,
/// and `docs/verdicts-contract.json` is a copy a test keeps current.
pub fn contract() -> Value {
    json!({
        "upnext_verdicts_contract": 1,
        "vocab": vocab::VOCAB,
        "format": format::FORMAT,
        "licence": consent::LICENCE,
        "licence_url": consent::LICENCE_URL,
        "consent": consent::json(),
        "fields": vocab::json(),
        "limits": {
            "text_chars": verdict::TEXT_MAX,
            "stop_note_chars": verdict::STOP_NOTE_MAX,
            "factors": verdict::MAX_FACTORS,
            "client_chars": verdict::CLIENT_MAX,
            "show_at": format::SHOW_AT,
            "percent_at": format::PERCENT_AT,
            "texts_per_game": format::TEXTS_PER_GAME,
        },
        "text_checks": verdict::text_checks_json(),
        "text_ignored": {"pattern": verdict::IGNORED, "flags": "g"},
        "report_reasons": verdict::REPORT_REASONS.iter().map(|(code, label)| json!({"code": code, "label": label})).collect::<Vec<_>>(),
        "key_prefix": verdict::KEY_PREFIX,
    })
}

/// The contract as `docs/verdicts-contract.json` holds it.
pub fn contract_text() -> String {
    let mut s = serde_json::to_string_pretty(&contract()).expect("the contract serializes");
    s.push('\n');
    s
}

#[cfg(test)]
mod receive_tests;
#[cfg(test)]
mod tests;
