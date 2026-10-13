//! The play diary's counting: what two readings say was played, and the
//! habits a few weeks of rows show.

use super::*;
use crate::db::ImportedGame;
use crate::db::PlayerId;

const H: i64 = HOUR;

fn reading(at: i64, games: &[(u32, i64, i64)]) -> Reading {
    Reading {
        at,
        games: games
            .iter()
            .map(|&(appid, forever, two_weeks)| (appid, Played { name: format!("App {appid}"), forever, two_weeks }))
            .collect(),
    }
}

#[test]
fn two_readings_an_hour_apart_give_what_was_played() {
    let t = 1_790_000_000 - 1_790_000_000 % H + 20 * 60; // 20 past an hour
    let before = reading(t, &[(620, 1260, 30), (400, 500, 10)]);
    assert!(changes(None, &before).is_empty(), "the first reading only starts the count");

    // Portal 2 grew 45 minutes, App 400 didn't, and Hades wasn't played in
    // the two weeks before, so its two-week minutes are new.
    let after = reading(t + H, &[(620, 1305, 75), (400, 500, 10), (1145360, 3000, 40)]);
    let got = changes(Some(&before), &after);
    let hour = (t + H / 2).div_euclid(H) * H;
    assert_eq!(got, vec![Change { appid: 620, hour, minutes: 45 }, Change { appid: 1145360, hour, minutes: 40 }]);

    // No more minutes than passed.
    let after = reading(t + H, &[(620, 1260 + 500, 600)]);
    assert_eq!(changes(Some(&before), &after)[0].minutes, 60);
    // A long gap can't say when; a drop isn't play.
    assert!(changes(Some(&before), &reading(t + MAX_GAP + 60, &[(620, 1400, 100)])).is_empty());
    assert!(changes(Some(&before), &reading(t + H, &[(620, 1200, 0)])).is_empty());
}

/// Rows for a session of `hours` hours from `start`, an hour of `minutes` each.
fn session(title: i64, start: i64, hours: i64, minutes: i64) -> Vec<DiaryRow> {
    (0..hours)
        .map(|i| DiaryRow {
            id: 0,
            hour: start + i * H,
            at: start + (i + 1) * H,
            title_id: Some(title),
            appid: None,
            name: String::new(),
            minutes,
            source: "steam".into(),
        })
        .collect()
}

/// Local time on a day of the week that week, at an hour.
fn local(weekday: chrono::Weekday, hour: u32) -> i64 {
    use chrono::TimeZone;
    let monday = chrono::NaiveDate::from_isoywd_opt(2026, 30, chrono::Weekday::Mon).unwrap();
    let day = monday + chrono::Days::new(weekday.num_days_from_monday() as u64);
    chrono::Local.from_local_datetime(&day.and_hms_opt(hour, 0, 0).unwrap()).single().unwrap().timestamp()
}

#[test]
fn habits_from_a_few_weeks_of_play() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = crate::testutil::offline_config(dir.path());
    let db = Db::open(&cfg.db_path, cfg.sampled_minutes).unwrap();
    let steam = |appid: u32, name: &str| ImportedGame {
        source: "steam".into(),
        source_id: appid.to_string(),
        launcher: "steam".into(),
        name: name.into(),
        appid: Some(appid),
        playtime_minutes: Some(600),
        ..Default::default()
    };
    db.sync_source(PlayerId::FIRST, "steam", "steam", &[steam(620, "Portal 2"), steam(1, "Raid Night")]).unwrap();
    let games = db.all_games(PlayerId::FIRST).unwrap();
    let id = |n: &str| games.iter().find(|g| g.name == n).unwrap().id;
    db.put_tags(1, Some(&[("Co-op".to_string(), 900), ("Action".to_string(), 500)])).unwrap();
    let games = db.all_games(PlayerId::FIRST).unwrap();
    let (portal, raid) = (id("Portal 2"), id("Raid Night"));

    use chrono::Weekday::*;
    let mut rows = vec![];
    // Weeknights: an hour or so in the evening, four times.
    for (day, minutes) in [(Mon, 60), (Tue, 50), (Wed, 70), (Thu, 60)] {
        rows.extend(session(portal, local(day, 20), 1, minutes));
    }
    // Weekends: three hours of co-op on Saturday and Sunday evening, and
    // Friday from 6 pm, which is the weekend too.
    for day in [Sat, Sun, Fri] {
        rows.extend(session(raid, local(day, if day == Fri { 18 } else { 19 }), 3, 60));
    }
    let h = habits(&rows, &games);
    assert_eq!(h.weeknight, Some(Sessions { minutes: 60, count: 4 }));
    assert_eq!(h.weekend, Some(Sessions { minutes: 180, count: 3 }));
    assert_eq!(h.times, vec!["evenings".to_string()]);
    let mp = h.multiplayer.unwrap();
    assert_eq!((mp.minutes, mp.weekend_share, mp.time.as_deref()), (540, 1.0, Some("evenings")));
    assert_eq!((h.days, h.minutes), (7, 780));

    // Two weeknight sessions aren't enough to say what's usual.
    let few: Vec<DiaryRow> =
        rows.iter().filter(|r| r.title_id == Some(raid)).cloned().chain(session(portal, local(Mon, 20), 1, 60)).collect();
    let h = habits(&few, &games);
    assert_eq!((h.weeknight, h.weekend.map(|s| s.count)), (None, Some(3)));
    // Launches have no minutes and don't count as play.
    let mut launch = session(portal, local(Tue, 8), 1, 0);
    launch[0].source = "launch".into();
    assert!(habits(&launch, &games).is_empty());
}
