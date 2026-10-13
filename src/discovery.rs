//! Discovery: ways to find something you wouldn't have picked
//! yourself, from your own library. **New to you** picks easy, short games
//! in genres or mechanics you've never played (`new_to_you`); **Hidden
//! gems** are unplayed, highly rated, short games the AI rated as unusual
//! (`gems`). Each is a pick mode: "New to me" or "A hidden gem" in a
//! request, or `mode` on `POST /api/picks`, narrows the candidates to its
//! games and gives the AI its rule. A third mode, **Roll the dice**
//! (`random`), narrows them to games off your radar and draws one at
//! random, with no AI; only `mode` asks for it.

#[cfg(test)]
pub(crate) mod fixture;
pub mod gems;
pub mod new_to_you;
pub mod random;

use crate::config::Config;
use crate::db::{Db, GameView};
use crate::taste::Profile;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A pick mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Only games in a genre or with a mechanic you've never played,
    /// easy and short.
    NewToYou,
    /// Only hidden gems: unplayed, highly rated, a session or two.
    Gems,
    /// Roll the dice: games off your radar, drawn at random with no
    /// AI call. No words in a request ask for it.
    Random,
}

/// What asks for a mode in a request, longest first.
const PHRASES: &[(&str, Mode)] = &[
    ("something new to me", Mode::NewToYou),
    ("new to me", Mode::NewToYou),
    ("new to you", Mode::NewToYou),
    ("new-to-me", Mode::NewToYou),
    ("a hidden gem", Mode::Gems),
    ("hidden gems", Mode::Gems),
    ("hidden gem", Mode::Gems),
];

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NewToYou => "new_to_you",
            Self::Gems => "gems",
            Self::Random => "random",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::NewToYou, Self::Gems, Self::Random].into_iter().find(|m| m.as_str() == s)
    }

    /// The mode a request asks for, with the rest of the request: "new to
    /// me, 2 hours" is New to you with "2 hours".
    pub fn detect(request: &str) -> (Option<Mode>, String) {
        let lower = request.to_lowercase().replace('\u{2019}', "'");
        for (phrase, mode) in PHRASES {
            // Whole words only: "brand new to medieval games" isn't asking.
            let found = lower.match_indices(phrase).find(|(i, _)| {
                let before = lower[..*i].chars().next_back();
                let after = lower[i + phrase.len()..].chars().next();
                before.is_none_or(|c| !c.is_alphanumeric()) && after.is_none_or(|c| !c.is_alphanumeric())
            });
            if let Some((i, _)) = found {
                // Lowercasing can change byte lengths outside ASCII; the
                // phrases are ASCII, so cut the original where they are when
                // it lines up, else the lowercased text.
                let source = if lower.len() == request.len() { request } else { lower.as_str() };
                let rest = format!("{} {}", &source[..i], &source[i + phrase.len()..]);
                let tidy = |c: char| c.is_whitespace() || matches!(c, ',' | '.' | ';' | ':' | '-' | '!' | '+' | '&');
                let rest: String = rest.split_whitespace().collect::<Vec<_>>().join(" ");
                let rest = rest.trim_matches(tidy).replace(" , ", ", ");
                return (Some(*mode), rest);
            }
        }
        (None, request.trim().to_string())
    }

    /// As a lineup's heading or a list says it.
    pub fn label(self) -> &'static str {
        match self {
            Self::NewToYou => "New to you",
            Self::Gems => "Hidden gems",
            Self::Random => "Rolled the dice",
        }
    }
}

/// One game a mode may pick, with what it tells the AI and the ranking.
#[derive(Debug, Clone, Default)]
pub struct Member {
    /// On its candidate line: "new to you: deckbuilders (a mechanic you
    /// haven't tried)", "unusual: …".
    pub note: String,
    /// New to you: the genres and mechanics it's a way into, as phrases.
    /// A lineup shows each at most once.
    pub ways: Vec<String>,
    /// Gems: points in the ranking for how unusual the AI rated it.
    pub points: f64,
    /// Where a lineup without AI starts its reason: "New to you: deckbuilders".
    pub reason: String,
}

/// What a mode narrows picks to: the games it may offer, each with its note.
#[derive(Debug, Clone)]
pub struct Lens {
    pub mode: Mode,
    pub members: HashMap<i64, Member>,
}

impl Lens {
    /// The mode's games in this library, as they stand now.
    pub fn build(db: &Db, cfg: &Config, mode: Mode, games: &[GameView], profile: Option<&Profile>) -> Result<Lens> {
        let members = match mode {
            Mode::NewToYou => new_to_you::members(games, profile, &new_to_you::Rules::from_config(cfg)),
            Mode::Gems => gems::members(games, profile, &gems::Rules::from_config(cfg), &db.hidden_gems()?),
            Mode::Random => random::members(games, crate::db::now()),
        };
        Ok(Lens { mode, members })
    }

    pub fn allows(&self, id: i64) -> bool {
        self.members.contains_key(&id)
    }

    pub fn member(&self, id: i64) -> Option<&Member> {
        self.members.get(&id)
    }

    /// Each game's ways in, for the lineup's one-of-each rule. Empty
    /// outside New to you.
    pub fn ways(&self) -> HashMap<i64, Vec<String>> {
        self.members.iter().filter(|(_, m)| !m.ways.is_empty()).map(|(id, m)| (*id, m.ways.clone())).collect()
    }

    /// Why nothing could be picked, when the mode has too few games.
    pub fn too_few(&self, eligible: usize, count: usize) -> String {
        let (one, many, what) = match self.mode {
            Mode::NewToYou => (
                "is new to you",
                "are new to you",
                "easy, short games in a genre or with a mechanic you've never played, that you haven't turned down",
            ),
            Mode::Gems => ("is a hidden gem", "are hidden gems", "unplayed, highly rated games of a session or two"),
            Mode::Random => (
                "is off your radar",
                "are off your radar",
                "games you haven't started, or last played over a year ago, that you haven't turned down",
            ),
        };
        // Hidden gems need a trusted rating, which Fetch details gathers
        // from Steam's reviews; a length helps, and HowLongToBeat gives it,
        // off until the user turns it on.
        let help = match self.mode {
            Mode::Gems => {
                ". A game's rating comes from its Steam reviews, which Fetch details gathers; \
                 HowLongToBeat (turn it on in Settings › Jobs) tells which games take a session or two"
            }
            Mode::NewToYou | Mode::Random => "",
        };
        match eligible {
            0 => format!("none of your games {one} yet ({what}){help}"),
            1 => format!("only 1 of your games {one} ({what}), cannot pick {count}"),
            n => format!("only {n} of your games {many} ({what}), cannot pick {count}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With no hidden gems, the message says where a game's length comes
    /// from; New to you's doesn't need it.
    #[test]
    fn too_few_gems_say_what_they_need() {
        let gems = Lens { mode: Mode::Gems, members: HashMap::new() };
        assert_eq!(
            gems.too_few(0, 3),
            "none of your games is a hidden gem yet (unplayed, highly rated games of a session or two). A game's rating comes from its \
             Steam reviews, which Fetch details gathers; HowLongToBeat (turn it on in Settings › Jobs) tells which games take a session \
             or two"
        );
        let new = Lens { mode: Mode::NewToYou, members: HashMap::new() };
        assert!(new.too_few(0, 3).ends_with("that you haven't turned down)"));
    }

    #[test]
    fn a_request_asks_for_a_mode_in_its_own_words() {
        assert_eq!(Mode::detect("New to me"), (Some(Mode::NewToYou), String::new()));
        assert_eq!(Mode::detect("new to me, 2 hours"), (Some(Mode::NewToYou), "2 hours".into()));
        assert_eq!(Mode::detect("2 hours, something new to me"), (Some(Mode::NewToYou), "2 hours".into()));
        assert_eq!(Mode::detect("A hidden gem"), (Some(Mode::Gems), String::new()));
        assert_eq!(Mode::detect("a hidden gem for tonight"), (Some(Mode::Gems), "for tonight".into()));
        assert_eq!(Mode::detect("Hidden gems with a story"), (Some(Mode::Gems), "with a story".into()));
        assert_eq!(Mode::detect("something new"), (None, "something new".into()));
        assert_eq!(Mode::detect("brand new to medieval games"), (None, "brand new to medieval games".into()));
        assert_eq!(Mode::detect("  Surprise me "), (None, "Surprise me".into()));
        assert_eq!(Mode::parse("new_to_you"), Some(Mode::NewToYou));
        assert_eq!(Mode::parse("gems"), Some(Mode::Gems));
        assert_eq!(Mode::parse("random"), Some(Mode::Random));
        assert_eq!(Mode::detect("roll the dice"), (None, "roll the dice".into()), "only `mode` asks for a random draw");
        assert_eq!(Mode::parse("other"), None);
    }
}
