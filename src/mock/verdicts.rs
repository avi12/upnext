//! The mock library's backlog verdicts from other players:
//! a made-up snapshot about 31 of its games, for the tests, the end to
//! end run and the A/B of the picks prompt. Three games have too few
//! verdicts to show, one has only counts, two are split down the middle, and
//! some carry texts.
//!
//! The answers are worked out from each game's own made-up details (its
//! review-tag scales, its length and its score), with a little variety by
//! verdict, so they read like what players of that game would say. Nothing
//! in them comes from anyone.

use super::Game;
use crate::verdicts::format::Published;
use crate::verdicts::vocab::{self, VOCAB};
use anyhow::{Context, Result};

/// The snapshot's day. Its months run up to this one.
pub const DATE: &str = "2026-10-01";

/// The games with verdicts, by name, with how many. `split` games get
/// answers that disagree, so nothing about them has a majority.
const GAMES: &[(&str, u32, bool)] = &[
    ("Hades", 23, false),
    ("Balatro", 18, false),
    ("Stardew Valley", 26, false),
    ("Slay the Spire", 12, false),
    ("Vampire Survivors", 9, false),
    ("Celeste", 8, false),
    ("Portal 2", 14, false),
    ("The Witcher 3: Wild Hunt", 22, false),
    ("Red Dead Redemption 2", 9, false),
    ("Persona 5 Royal", 7, false),
    ("ELDEN RING", 20, false),
    ("Hollow Knight", 11, false),
    ("Disco Elysium - The Final Cut", 6, false),
    ("Outer Wilds", 10, false),
    ("Baldur's Gate 3", 16, false),
    ("Factorio", 5, false),
    ("Sid Meier's Civilization VI", 13, false),
    ("Assassin's Creed Odyssey", 12, false),
    ("Dragon Age: Inquisition", 8, false),
    ("MONSTER HUNTER: WORLD", 9, false),
    ("DARK SOULS III", 15, false),
    ("A Short Hike", 6, false),
    ("Into the Breach", 5, false),
    ("Inscryption", 7, false),
    ("Dorfromantik", 4, false),
    ("Gone Home", 3, false),
    ("Subnautica", 8, true),
    ("Cyberpunk 2077", 10, true),
    ("Stellaris", 2, false),
    ("Firewatch", 2, false),
    ("Terraria", 1, false),
];

/// A verdict id: 26 base32 characters from a number.
fn id(n: u32) -> String {
    let alphabet = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut id = vec![b'a'; 26];
    let mut x = n;
    for slot in id.iter_mut().rev() {
        *slot = alphabet[(x % 32) as usize];
        x /= 32;
    }
    // The ids of the tests' own fixture start with "a"; these with "m".
    id[0] = b'm';
    String::from_utf8(id).expect("ascii")
}

fn scale<'a>(g: &'a Game, name: &str) -> Option<&'a str> {
    g.sc.split(',').filter_map(|f| f.split_once(':')).find(|(n, _)| *n == name).map(|(_, v)| v)
}

/// What a text says, by what the game is like: a few made-up lines.
fn text(g: &Game, i: u32) -> Option<String> {
    if i % 4 != 1 {
        return None;
    }
    let short = scale(g, "sessions") == Some("short");
    let line = if short {
        [
            "Every run is half an hour, so it fits a weeknight.",
            "Easy to put down and pick up a week later.",
            "Twenty minutes is enough to get somewhere.",
        ]
    } else if g.h >= 40.0 {
        [
            "Plan on many evenings; the main story alone is long.",
            "Hard to come back to after a break of a few weeks.",
            "The side content can wait; the story is the point.",
        ]
    } else {
        [
            "A weekend or two is enough to see it through.",
            "It gets going within the first hour.",
            "Best played in sittings of an hour or more.",
        ]
    };
    Some(line[(i as usize / 4) % line.len()].to_string())
}

/// The verdicts about one game, numbered from `first`.
fn about(g: &Game, n: u32, split: bool, first: u32) -> Vec<Published> {
    let sessions = scale(g, "sessions");
    let grind = scale(g, "grind");
    let difficulty = scale(g, "difficulty");
    let replay = scale(g, "replay") == Some("high");
    let story = scale(g, "story") == Some("central");
    let long = g.h >= 40.0;
    (0..n)
        .map(|i| {
            // One in five disagrees with the rest; a split game has no majority.
            let odd = if split { i % 2 == 1 } else { i % 5 == 4 };
            let outcome = if split {
                ["finished", "stopped", "shelved", "bounced"][(i % 4) as usize]
            } else if replay && sessions == Some("short") {
                if i % 3 == 0 { "endless" } else { "finished" }
            } else if grind == Some("heavy") || difficulty == Some("punishing") {
                if i % 3 == 0 { "finished" } else { "stopped" }
            } else if long {
                ["finished", "shelved", "finished", "stopped"][(i % 4) as usize]
            } else if odd {
                "bounced"
            } else {
                "finished"
            };
            let worth = if split {
                ["yes", "no", "for_some"][(i % 3) as usize]
            } else if odd || outcome == "bounced" {
                "for_some"
            } else if g.p >= 85 && outcome != "stopped" {
                "yes"
            } else if outcome == "stopped" {
                if i % 2 == 0 { "no" } else { "for_some" }
            } else {
                "yes"
            };
            let played = match outcome {
                "finished" | "endless" => g.h * if odd { 1.6 } else { 1.1 },
                "bounced" => 1.0,
                _ => g.h * 0.35,
            };
            let clicked = match (scale(g, "hook"), sessions, odd || split) {
                (_, _, true) => ["later", "few_hours", "never", "first_hour"][(i % 4) as usize],
                (Some("fast"), _, _) | (None, Some("short"), _) => "first_hour",
                (Some("slow"), _, _) => "later",
                _ if long => "few_hours",
                _ => "first_hour",
            };
            let session = match (sessions, odd || split) {
                (_, true) => ["medium", "short", "long"][(i % 3) as usize],
                (Some(s), _) => s,
                (None, _) if long => "long",
                (None, _) => "medium",
            };
            let limited = match session {
                _ if split => ["yes", "no", "mostly"][(i % 3) as usize],
                "short" if grind != Some("heavy") => "yes",
                "short" | "medium" => "mostly",
                _ => "no",
            };
            let mut factors: Vec<String> = vec![];
            match limited {
                "yes" => factors.extend(["short_chunks", "easy_to_resume"].map(String::from)),
                "no" if grind == Some("heavy") => factors.extend(["padding", "hard_to_resume"].map(String::from)),
                "no" => factors.push("hard_to_resume".into()),
                _ => factors.push("save_anywhere".into()),
            }
            let stop_point = if outcome == "endless" || replay {
                "anytime"
            } else if story && long {
                "at_credits"
            } else if difficulty.is_some_and(|d| d == "hard" || d == "punishing") {
                "push_through"
            } else {
                "at_credits"
            };
            let mut stopped_for: Vec<String> = vec![];
            if vocab::STOPPED_OUTCOMES.contains(&outcome) {
                if grind == Some("heavy") {
                    stopped_for.push("grind".into());
                }
                if long {
                    stopped_for.push("too_long".into());
                }
                if difficulty == Some("punishing") {
                    stopped_for.push("too_hard".into());
                }
                if stopped_for.is_empty() {
                    stopped_for.push(if i % 2 == 0 { "pacing" } else { "boring" }.into());
                }
            }
            Published {
                id: id(first + i),
                steam_appid: Some(g.a),
                gog_id: None,
                outcome: outcome.into(),
                worth: worth.into(),
                hours: (i % 6 != 5).then(|| vocab::hours_range((played * 60.0) as i64).to_string()),
                clicked: (i % 7 != 6).then(|| clicked.to_string()),
                sessions: Some(session.to_string()),
                limited_time: Some(limited.to_string()),
                factors,
                stop_point: (i % 4 != 3).then(|| stop_point.to_string()),
                stop_note: None,
                stopped_for,
                text: text(g, i),
                vocab: VOCAB,
                month: ["2026-08", "2026-09", "2026-10"][(i % 3) as usize].into(),
            }
        })
        .collect()
}

/// Every made-up verdict, in the order of [`GAMES`].
pub(super) fn all(games: &[Game]) -> Result<Vec<Published>> {
    let mut out = vec![];
    for (name, n, split) in GAMES {
        let g = games.iter().find(|g| g.n == *name).with_context(|| format!("{name} has verdicts but isn't in the mock library"))?;
        let first = out.len() as u32 + 1;
        out.extend(about(g, *n, *split, first));
    }
    Ok(out)
}
