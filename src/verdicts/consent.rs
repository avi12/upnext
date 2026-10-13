//! The licence of backlog verdicts and the consent a player gives before
//! their first one (`docs/verdicts/consent.md`). CC0 1.0 was chosen on
//! 2026-10-09, the licence of UpNext's shared game data, so verdicts and the seed
//! can be combined and reused without an attribution chain.
//!
//! The consent screen is a page of its own, shown before anything leaves the
//! computer, and shown again only when its text changes. Its version goes
//! with every verdict, and the service refuses a verdict whose version is
//! older than the oldest it accepts, so any change to the words below must
//! bump [`CONSENT_VERSION`]; a test pins the words to the version.

use serde_json::{Value, json};

/// The licence every verdict is published under, as an SPDX id.
pub const LICENCE: &str = "CC0-1.0";
pub const LICENCE_URL: &str = "https://creativecommons.org/publicdomain/zero/1.0/";

/// The consent text's version. A player who accepted an older one is asked
/// again before their next verdict.
pub const CONSENT_VERSION: i64 = 1;

/// The screen's heading.
pub const TITLE: &str = "Your verdict will be public.";

/// The screen's paragraphs, in order.
pub const PARAGRAPHS: [&str; 4] = [
    "Everything you send, the answers and anything you type, is published for anyone to read, copy and reuse, for any purpose, \
with no credit needed. It's released under CC0 1.0, which puts it in the public domain.",
    "It isn't linked to your name, your Steam or GOG account or your library, and UpNext doesn't publish which verdicts came from \
the same person. Your hours are sent rounded to a range.",
    "You can take a verdict down at any time. It's left out of the next daily update and gone from UpNext's servers within a week, \
but copies people have already downloaded can't be recalled.",
    "Don't type anything you wouldn't post publicly, such as your name, contact details or someone else's.",
];

/// The two buttons. Only the first sends anything, and only the verdict on
/// the form.
pub const ACCEPT: &str = "Publish my verdicts under CC0";
pub const DECLINE: &str = "Not now";

/// The line under the form's text box, repeating the licence where people type.
pub const TEXT_BOX_LINE: &str = "Public, CC0: anyone can read and reuse what you type.";

/// The service's terms, relative to `verdicts_url`, linked from the screen.
pub const TERMS_PATH: &str = "/terms";

/// The consent as the contract's `consent`.
pub fn json() -> Value {
    json!({
        "version": CONSENT_VERSION,
        "title": TITLE,
        "paragraphs": PARAGRAPHS,
        "accept": ACCEPT,
        "decline": DECLINE,
        "text_box_line": TEXT_BOX_LINE,
        "terms_path": TERMS_PATH,
    })
}
