//! Roll the dice: one game off your radar, drawn at random, with no
//! AI call. It answers "what have I forgotten I own?", where Curate answers
//! "what fits tonight?".
//!
//! **Off your radar** is a game picks may offer that you haven't started,
//! or that you last played (or marked) over a year ago: tried and left,
//! shelved, or abandoned and still owed its second chance. Endless games
//! and staples you play now and then are on your radar by definition, so
//! they never come up; neither does anything picks leave out (Playing, Beat
//! it, Not for me, your hard nos).
//!
//! **The draw** leaves out the games turned down in this lineup, the last
//! few lineups' picks (unless nothing else is left), and, on Linux, games
//! ProtonDB says don't run. It keeps the lineup's rules: one game per
//! series and one second chance. Every eligible game has the same chance.

use super::Member;
use crate::db::{GameView, Pick, Status};
use crate::lifecycle::Offer;
use crate::prompt::{PromptInput, ago, hours};
use crate::recommend::Screen;
use std::collections::HashMap;

/// A game last played this long ago, or longer, is off your radar.
pub const RADAR_DAYS: i64 = 365;

/// Why a game is off your radar, as its pick's reason says it, or None
/// when it isn't.
pub fn off_radar(g: &GameView, now: i64) -> Option<String> {
    let offer = g.offer()?;
    // When it was put down: when it was last played, or else when it was marked.
    let put_down = if g.last_played > 0 { Some(g.last_played) } else { g.override_at };
    let long_ago = put_down.filter(|t| now - t >= RADAR_DAYS * 86_400);
    let played = (g.playtime_known && g.playtime_minutes > 0).then(|| hours(g.playtime_minutes));
    match offer {
        Offer::New if g.status == Status::Untouched => Some("Drawn at random from the games you haven't started.".into()),
        // A store that doesn't say when it was played can't say it was
        // recent either, and a game only tried is fair game.
        Offer::New if put_down.is_none() || long_ago.is_some() => Some(format!(
            "Drawn at random: you tried it{}{} and haven't been back.",
            played.map(|h| format!(" for {h}")).unwrap_or_default(),
            long_ago.map(|t| format!(" {}", ago(now, t))).unwrap_or_default(),
        )),
        // The card's history line says how long you played it.
        Offer::Shelved | Offer::SecondChance => long_ago.map(|t| format!("Drawn at random: you put it down {}.", ago(now, t))),
        _ => None,
    }
}

/// The games off your radar, each with its reason, as the mode's lens.
pub fn members(games: &[GameView], now: i64) -> HashMap<i64, Member> {
    games
        .iter()
        .filter_map(|g| {
            let reason = off_radar(g, now)?;
            let note = format!("off your radar: {}", reason.trim_start_matches("Drawn at random").trim_start_matches([':', ' ']));
            Some((g.id, Member { note, ways: vec![], points: 0.0, reason }))
        })
        .collect()
}

/// What a draw found: picks, spares, and how many games it drew from.
pub struct Drawn {
    pub picks: Vec<Pick>,
    pub spares: Vec<Pick>,
    pub eligible: usize,
}

/// A stable order for one seed: each game's place is a hash of the seed
/// and its id, so every game is as likely to come first, and a test can
/// fix the seed.
fn shuffled(mut games: Vec<&GameView>, seed: u64) -> Vec<&GameView> {
    use std::hash::{Hash, Hasher};
    let key = |g: &GameView| {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (seed, g.id).hash(&mut h);
        h.finish()
    };
    games.sort_by_cached_key(|g| key(g));
    games
}

/// Draws `input.count` picks, then `input.spares` spares, from the games
/// the mode's lens allows (`input.pool()`), in a random order from
/// `input.seed`. `screen` is what the lineup already holds.
pub fn draw(input: &PromptInput, mut screen: Screen) -> Drawn {
    let runs = |g: &GameView| !(input.linux && g.linux.as_deref() == Some("borked"));
    let pool: Vec<&GameView> = input.pool().into_iter().filter(|g| !input.off_limits(g) && runs(g)).collect();
    let eligible = pool.len();
    // The last few lineups' picks come up again only when nothing else is left.
    let fresh: Vec<&GameView> = pool.iter().copied().filter(|g| !input.recent_ids.contains(&g.id)).collect();
    let pool = if fresh.len() >= input.count { fresh } else { pool };
    let reason =
        |g: &GameView| input.lens.and_then(|l| l.member(g.id)).map(|m| m.reason.clone()).unwrap_or_else(|| "Drawn at random.".into());
    let (mut picks, mut spares) = (vec![], vec![]);
    for g in shuffled(pool, input.seed) {
        if picks.len() >= input.count && spares.len() >= input.spares {
            break;
        }
        if screen.clash(g).is_some() {
            continue;
        }
        if picks.len() < input.count {
            screen.add(g);
            picks.push(Pick::of(g, reason(g)));
        } else {
            screen.add_spare(g);
            spares.push(Pick::of(g, reason(g)));
        }
    }
    Drawn { picks, spares, eligible }
}

#[cfg(test)]
mod tests;
