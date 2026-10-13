//! The first-run questions: your most-played games to choose
//! favourites from, then quick questions on what you love, your hard nos,
//! your sessions, who you play with and the play diary. The answers are the
//! same statements the profile page writes, so picks read them at once.
//! The setup wizard asks them, and they are also a flow of their own,
//! "Tune your profile" on the profile page.

use super::{Subject, SubjectChoice, hard_no_matches, most_played, subject_choices};
use crate::config::Config;
use crate::db::PlayerId;
use crate::db::{Db, GameView, Status, now};
use anyhow::Result;
use serde::Serialize;
use std::collections::HashMap;

/// Most-played games offered as covers.
pub const MOST_PLAYED: usize = 12;
/// With fewer games than this past 2 hours, the first question becomes a
/// search of the library (a new account, or only Heroic's Epic and Amazon
/// games, which report no playtime).
pub const FEW_PLAYED: usize = 6;
/// Genres and themes offered as loves, the ones most of your games have.
const LOVED_TAGS: usize = 18;
/// Hard nos people often have, in this order.
const HARD_NOS: [&str; 14] = [
    "tag:Horror",
    "facet:mood:scary",
    "facet:difficulty:punishing",
    "tag:Souls-like",
    "facet:grind:heavy",
    "tag:PvP",
    "tag:Massively Multiplayer",
    "tag:Battle Royale",
    "tag:Sports",
    "tag:Racing",
    "tag:Survival",
    "tag:Zombies",
    "tag:Visual Novel",
    "tag:Dating Sim",
];

/// A most-played game, and its favourite's id when it is one.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct MostPlayed {
    pub game: GameView,
    pub favourite: Option<i64>,
}

/// The play diary's question.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct DiaryQuestion {
    pub on: bool,
    /// The Steam Web API key and ID are set, so it can read playtime.
    pub steam: bool,
}

/// `GET /api/taste/questions`.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct Questions {
    pub most_played: Vec<MostPlayed>,
    pub few_played: bool,
    /// Moods, then genres and themes, for "what you love".
    pub loves: Vec<SubjectChoice>,
    pub hard_nos: Vec<SubjectChoice>,
    pub diary: DiaryQuestion,
}

pub fn questions(db: &Db, cfg: &Config) -> Result<Questions> {
    let games = db.all_games_shared(PlayerId::FIRST)?;
    let rows = db.taste_statements(PlayerId::FIRST)?;
    let favourites: HashMap<i64, i64> = rows.iter().filter(|r| r.kind == "favourite").filter_map(|r| Some((r.title_id?, r.id))).collect();
    let said = |about: &str| rows.iter().any(|r| r.kind == "hard_no" && r.about.as_deref() == Some(about));
    let played = most_played(&games, now());
    let few_played = played.iter().filter(|g| g.playtime_minutes >= 120).count() < FEW_PLAYED;
    let most_played =
        played.iter().take(MOST_PLAYED).map(|g| MostPlayed { game: g.slim(), favourite: favourites.get(&g.id).copied() }).collect();
    let all = subject_choices(&games);
    let mut hard_nos: Vec<SubjectChoice> = HARD_NOS.iter().filter_map(|a| all.iter().find(|s| s.about == *a).cloned()).collect();
    for c in hard_nos.iter_mut() {
        if let Some(s) = Subject::parse(&c.about) {
            c.count = games.iter().filter(|g| g.status != Status::NotGame && hard_no_matches(&s, g)).count();
        }
    }
    // Only what your library has or you already said, unless it has none
    // of them (a new library).
    if hard_nos.iter().any(|c| c.count > 0) {
        hard_nos.retain(|c| c.count > 0 || said(&c.about));
    }
    Ok(Questions {
        most_played,
        few_played,
        loves: loves(&all),
        // Counted as a hard no reaches, which is less far than a like.
        hard_nos,
        diary: DiaryQuestion { on: cfg.play_diary, steam: cfg.steam_credentials().is_ok() },
    })
}

/// The moods your games have, then the genres and themes most of them have.
fn loves(all: &[SubjectChoice]) -> Vec<SubjectChoice> {
    let mut moods: Vec<SubjectChoice> = all.iter().filter(|s| s.about.starts_with("facet:mood:") && s.count > 0).cloned().collect();
    moods.sort_by_key(|s| std::cmp::Reverse(s.count));
    let mut tags: Vec<SubjectChoice> = all.iter().filter(|s| s.about.starts_with("tag:") && s.count > 0).cloned().collect();
    tags.sort_by_key(|s| std::cmp::Reverse(s.count));
    tags.truncate(LOVED_TAGS);
    moods.into_iter().chain(tags).collect()
}
