//! A small library for discovery's tests, built the way sync and enrich
//! build one: Steam games with store details, tags, reviews, HowLongToBeat
//! times and review-tag scales.

use crate::config::Config;
use crate::db::PlayerId;
use crate::db::{Db, DetailsRow, GameView, HltbRow, ImportedGame, ReviewRow};
use crate::review_tags::ReviewTags;

/// One game.
#[derive(Clone, Copy)]
pub struct Spec {
    pub name: &'static str,
    pub appid: u32,
    /// Minutes played.
    pub minutes: i64,
    pub genres: &'static [&'static str],
    /// Steam tags, strongest first.
    pub tags: &'static [&'static str],
    /// Steam reviews: percent positive, and how many.
    pub reviews: (i64, i64),
    /// HowLongToBeat's main story.
    pub hours: Option<f64>,
    /// Review-tag scales: ("difficulty", "easy").
    pub scales: &'static [(&'static str, &'static str)],
    pub developer: &'static str,
    pub publisher: &'static str,
    /// No ending (the game sheet's switch).
    pub endless: bool,
}

pub const fn game(name: &'static str, appid: u32) -> Spec {
    Spec {
        name,
        appid,
        minutes: 0,
        genres: &[],
        tags: &[],
        reviews: (95, 10_000),
        hours: Some(5.0),
        scales: &[],
        developer: "",
        publisher: "",
        endless: false,
    }
}

/// The library, with a config whose discovery settings are the defaults.
pub fn library(specs: &[Spec]) -> (tempfile::TempDir, Db, Config) {
    let dir = tempfile::tempdir().unwrap();
    let cfg = crate::testutil::offline_config(dir.path());
    let db = Db::open(&cfg.db_path, cfg.sampled_minutes).unwrap();
    let now = crate::db::now();
    let imported: Vec<ImportedGame> = specs
        .iter()
        .map(|s| ImportedGame {
            source: "steam".into(),
            source_id: s.appid.to_string(),
            launcher: "steam".into(),
            name: s.name.into(),
            appid: Some(s.appid),
            playtime_minutes: Some(s.minutes),
            // Played a year ago: done with, not playing now.
            last_played: if s.minutes > 0 { now - 365 * 86_400 } else { 0 },
            ..Default::default()
        })
        .collect();
    db.sync_source(PlayerId::FIRST, "steam", "steam", &imported).unwrap();
    for s in specs {
        let strs = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let d = DetailsRow {
            type_: Some("game".into()),
            genres: strs(s.genres),
            release_date: Some("Jan 1, 2020".into()),
            developers: strs(&[s.developer].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>()),
            publishers: strs(&[s.publisher].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>()),
            ..Default::default()
        };
        db.put_details(s.appid, Some(&d)).unwrap();
        let (pct, total) = s.reviews;
        let positive = total * pct / 100;
        let r = ReviewRow {
            review_score: Some(8),
            review_score_desc: Some("Very Positive".into()),
            total_positive: Some(positive),
            total_negative: Some(total - positive),
            total_reviews: Some(total),
        };
        db.put_reviews(s.appid, Some(&r)).unwrap();
        let tags: Vec<(String, i64)> = s.tags.iter().enumerate().map(|(i, t)| (t.to_string(), 100 - i as i64)).collect();
        db.put_tags(s.appid, Some(&tags)).unwrap();
    }
    let all = db.all_games(PlayerId::FIRST).unwrap();
    for s in specs {
        let id = all.iter().find(|g| g.name == s.name).unwrap().id;
        if let Some(h) = s.hours {
            db.put_hltb(id, Some(&HltbRow { main_story: Some(h), ..Default::default() })).unwrap();
        }
        if !s.scales.is_empty() {
            let tags = ReviewTags {
                scales: s.scales.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
                mood: vec![],
                skills: vec![],
                aspects: vec![],
                notes: vec![],
            };
            db.put_digest(id, &format!("Praised for {}.", s.name), "", &tags, "test", 4).unwrap();
        }
        if s.endless {
            db.set_no_ending(id, Some(true)).unwrap();
        }
    }
    (dir, db, cfg)
}

pub fn id_of(games: &[GameView], name: &str) -> i64 {
    games.iter().find(|g| g.name == name).unwrap_or_else(|| panic!("{name} isn't in the library")).id
}
