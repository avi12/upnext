//! A made-up library for trying UpNext without a real one
//! (`upnext mock-library`): about a hundred real games with details written
//! by hand in `tests/fixtures/mock-library.json`, playtimes and dates that
//! give a spread of statuses (staples, played through, shelved, abandoned,
//! untouched), a taste profile, and a second player with a small Steam
//! account of their own. Nothing in it comes from anyone's
//! library, so it can be committed and shared, and machines with no real
//! library, such as CI and development containers, use it.

use crate::db::PlayerId;
use crate::db::{Db, DetailsRow, HideScope, HltbRow, ImportedGame, NewDiaryRow, Override, ReviewRow, now};
use crate::review_tags::{self, Aspect, ReviewTags, Verdict};
use crate::taste::{self, StatementBody, TasteRules};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

mod verdicts;

const LIBRARY: &str = include_str!("../tests/fixtures/mock-library.json");
const DAY: i64 = 86_400;

#[derive(Debug, Deserialize)]
struct Library {
    games: Vec<Game>,
    profile: Vec<Statement>,
    /// Routines, as the API takes them.
    #[serde(default)]
    routines: Vec<taste::routine::RoutineBody>,
    /// A second player on the same computer.
    second_player: SecondPlayer,
}

/// Player 2: their Steam account, marks and profile, and what player 1 hid
/// from them.
#[derive(Debug, Deserialize)]
struct SecondPlayer {
    name: String,
    steam: Vec<Owned>,
    statuses: Vec<Mark>,
    profile: Vec<Statement>,
    hidden_by_first: Vec<String>,
}

/// A game in player 2's Steam account, by its name in `games`.
#[derive(Debug, Deserialize)]
struct Owned {
    n: String,
    m: i64,
    d: i64,
    /// Only player 2 owns it; player 1's account leaves it out.
    #[serde(default)]
    only: bool,
}

#[derive(Debug, Deserialize)]
struct Mark {
    n: String,
    st: String,
    #[serde(default)]
    stop: Vec<String>,
}

/// One game, in the fixture's short keys.
#[derive(Debug, Deserialize)]
struct Game {
    n: String,
    a: u32,
    /// The store: steam (default), or epic or gog through Heroic.
    #[serde(default)]
    s: Option<String>,
    /// The launcher, when it isn't Steam or Heroic: `epic` (the Epic
    /// sign-in, with playtime), `origin` or `uplay` (installed, read on
    /// Windows), or `manual` (a list typed in, saved as Settings saves it).
    #[serde(default)]
    l: Option<String>,
    y: i32,
    g: Vec<String>,
    t: Vec<String>,
    /// Steam store categories: single-player or co-op, controller support.
    #[serde(default)]
    c: Vec<String>,
    p: i64,
    r: i64,
    #[serde(default)]
    mc: Option<i64>,
    h: f64,
    /// Minutes played; None for a store that doesn't say.
    #[serde(default)]
    m: Option<i64>,
    /// Days since last played.
    #[serde(default)]
    d: Option<i64>,
    /// A status set by hand.
    #[serde(default)]
    st: Option<String>,
    /// Why it was put down (`lifecycle::REASONS` codes).
    #[serde(default)]
    stop: Vec<String>,
    /// Review-tag scales: "difficulty:hard,pace:intense".
    #[serde(default)]
    sc: String,
    #[serde(default)]
    mo: String,
    #[serde(default)]
    sk: String,
    /// Aspects: "+combat,-grind,~story".
    #[serde(default, rename = "as")]
    aspects: String,
    says: String,
}

#[derive(Debug, Deserialize)]
struct Statement {
    kind: String,
    #[serde(default)]
    game: Option<String>,
    #[serde(default)]
    about: Option<String>,
    #[serde(default)]
    why: Option<Value>,
    #[serde(default)]
    note: Option<String>,
}

fn list(s: &str) -> Vec<String> {
    s.split(',').map(str::trim).filter(|x| !x.is_empty()).map(str::to_string).collect()
}

/// A game's review tags, checked against the vocabulary.
fn review_tags(g: &Game) -> Result<ReviewTags> {
    let mut scales = BTreeMap::new();
    for kv in list(&g.sc) {
        if review_tags::facet_name(&kv).as_deref() != Some(kv.as_str()) {
            bail!("{}: {kv} isn't a review-tag scale value", g.n);
        }
        let (k, v) = kv.split_once(':').expect("checked");
        scales.insert(k.to_string(), v.to_string());
    }
    let set = |name: &str, values: &str| -> Result<Vec<String>> {
        let out = list(values);
        if let Some(v) = out.iter().find(|v| review_tags::facet_name(&format!("{name}:{v}")).is_none()) {
            bail!("{}: {name} {v} isn't a review-tag value", g.n);
        }
        Ok(out)
    };
    let mut aspects = vec![];
    for a in list(&g.aspects) {
        let (verdict, name) = match a.split_at(1) {
            ("+", n) => (Verdict::Praised, n),
            ("-", n) => (Verdict::Criticized, n),
            ("~", n) => (Verdict::Divisive, n),
            _ => bail!("{}: aspect {a} starts with +, - or ~", g.n),
        };
        let Some(name) = review_tags::aspect_name(name) else { bail!("{}: {name} isn't an aspect", g.n) };
        aspects.push(Aspect { aspect: name.to_string(), verdict });
    }
    Ok(ReviewTags {
        scales: scales.into_iter().collect(),
        mood: set("mood", &g.mo)?,
        skills: set("skills", &g.sk)?,
        aspects,
        notes: vec![],
    })
}

/// What a seed wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seeded {
    pub games: usize,
    pub statements: usize,
    pub patterns: usize,
    /// Play diary rows.
    pub diary: usize,
    pub routines: usize,
}

/// Steam's controller support for a made-up game, guessed from its genres
/// the way it tends to go: strategy is played with a mouse, simulations and
/// RPGs often take a pad in part, and the rest fully.
fn controller_support(genres: &[String]) -> Option<String> {
    let has = |g: &str| genres.iter().any(|x| x.eq_ignore_ascii_case(g));
    if has("Strategy") {
        None
    } else if has("Simulation") || has("RPG") {
        Some("partial".into())
    } else {
        Some("full".into())
    }
}

/// Fill an empty database with the mock library and its profile, then
/// count the profile's patterns. Schedules are turned off, so a server on it
/// never syncs a real store into it.
pub fn seed(db: &Db) -> Result<Seeded> {
    fill(db, true)
}

/// The mock library with its first player alone (`upnext mock-library
/// --one-player`): no second player, none of their own games and nothing
/// hidden from them, for trying and testing UpNext as one person sees it.
pub fn seed_one_player(db: &Db) -> Result<Seeded> {
    fill(db, false)
}

fn fill(db: &Db, two_players: bool) -> Result<Seeded> {
    if !db.all_games(PlayerId::FIRST)?.is_empty() {
        bail!("this database already has a library; the mock library goes only into an empty one (point UPNEXT_DB at a new file)");
    }
    let lib: Library = serde_json::from_str(LIBRARY).context("reading the mock library")?;
    let t = now();
    let launcher = |g: &Game| g.l.clone().unwrap_or_else(|| if g.s.is_none() { "steam" } else { "heroic" }.into());
    let imported = |g: &Game| ImportedGame {
        source: g.s.clone().unwrap_or_else(|| "steam".into()),
        source_id: if g.s.is_none() { g.a.to_string() } else { format!("mock-{}", g.a) },
        launcher: launcher(g),
        name: g.n.clone(),
        appid: g.s.is_none().then_some(g.a),
        installed: g.d.is_some_and(|d| d <= 60),
        playtime_minutes: if g.s.is_none() || g.l.as_deref() == Some("epic") { Some(g.m.unwrap_or(0)) } else { None },
        last_played: g.d.filter(|_| g.m.unwrap_or(0) > 0).map(|d| t - d * DAY).unwrap_or(0),
        // Play links as sync makes them, so a Play button has somewhere to
        // go; the EA app's, the Epic launcher's and games added by hand have none.
        run_url: match (launcher(g).as_str(), g.s.as_deref()) {
            ("steam", _) => Some(format!("steam://run/{}", g.a)),
            // Heroic names Epic's runner legendary.
            ("heroic", Some(store)) => Some(format!("heroic://launch/{}/mock-{}", if store == "epic" { "legendary" } else { store }, g.a)),
            (l, _) if l == crate::ubisoft::SOURCE => Some(crate::ubisoft::run_url(&g.a.to_string())),
            _ => None,
        },
        ..Default::default()
    };
    let second = &lib.second_player;
    let theirs_only = |g: &Game| second.steam.iter().any(|o| o.only && o.n == g.n);
    let mut syncs: Vec<(String, String)> = vec![];
    for g in lib.games.iter().filter(|g| !theirs_only(g)).map(imported).filter(|g| g.launcher != crate::manual::LAUNCHER) {
        if !syncs.contains(&(g.launcher.clone(), g.source.clone())) {
            syncs.push((g.launcher, g.source));
        }
    }
    for (launcher, source) in &syncs {
        let games: Vec<ImportedGame> =
            lib.games.iter().filter(|g| !theirs_only(g)).map(imported).filter(|g| (&g.launcher, &g.source) == (launcher, source)).collect();
        db.sync_source(PlayerId::FIRST, launcher, source, &games)?;
    }
    // Games added by hand go in as Settings puts them, a list per store.
    let mut by_hand: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for g in lib.games.iter().filter(|g| g.l.as_deref() == Some(crate::manual::LAUNCHER)) {
        by_hand.entry(g.s.clone().context("a game added by hand names its store")?).or_default().push(g.n.clone());
    }
    for (store, titles) in &by_hand {
        crate::manual::save(db, PlayerId::FIRST, store, titles)?;
    }
    let sam = if two_players {
        let sam = db.add_player(&second.name)?;
        // Their library is filled in here, so they don't need their own setup.
        crate::server::own_setup_done(db, sam)?;
        let mut games = vec![];
        for o in &second.steam {
            let g = lib.games.iter().find(|g| g.n == o.n).with_context(|| format!("{} isn't in the mock library", o.n))?;
            games.push(ImportedGame { playtime_minutes: Some(o.m), last_played: t - o.d * DAY, installed: o.d <= 60, ..imported(g) });
        }
        db.sync_source(sam, "steam", "steam", &games)?;
        Some(sam)
    } else {
        None
    };
    let ids: BTreeMap<String, i64> = db.all_games(PlayerId::FIRST)?.into_iter().map(|g| (g.name, g.id)).collect();
    for g in &lib.games {
        // With one player, the second player's own games aren't anyone's.
        if !two_players && theirs_only(g) {
            continue;
        }
        let id = *ids.get(&g.n).with_context(|| format!("{} didn't make it into the library", g.n))?;
        if g.s.is_some() {
            db.put_steam_match(id, Some((g.a, &g.n, 1.0)))?;
        }
        let details = DetailsRow {
            type_: Some("game".into()),
            genres: g.g.clone(),
            categories: g.c.clone(),
            metacritic: g.mc,
            release_date: Some(format!("Jan 1, {}", g.y)),
            platforms: vec!["windows".into()],
            controller_support: controller_support(&g.g),
            ..Default::default()
        };
        db.put_details(g.a, Some(&details))?;
        let positive = g.r * g.p / 100;
        let reviews = ReviewRow {
            review_score: Some(if g.p >= 95 {
                9
            } else if g.p >= 80 {
                8
            } else {
                6
            }),
            review_score_desc: Some(
                if g.p >= 95 {
                    "Overwhelmingly Positive"
                } else if g.p >= 80 {
                    "Very Positive"
                } else {
                    "Mostly Positive"
                }
                .into(),
            ),
            total_positive: Some(positive),
            total_negative: Some(g.r - positive),
            total_reviews: Some(g.r),
        };
        db.put_reviews(g.a, Some(&reviews))?;
        let tags: Vec<(String, i64)> = g.t.iter().enumerate().map(|(i, s)| (s.clone(), 1000 - 100 * i as i64)).collect();
        db.put_tags(g.a, Some(&tags))?;
        db.put_hltb(id, Some(&HltbRow { main_story: Some(g.h), main_extra: Some(g.h * 1.5), ..Default::default() }))?;
        db.put_digest(id, &g.says, "", &review_tags(g)?, "mock", crate::digest::DIGEST_VERSION)?;
        if let Some(st) = &g.st {
            let o = Override::parse(st).with_context(|| format!("{}: {st} isn't a status", g.n))?;
            db.set_override(PlayerId::FIRST, id, Some(o), None)?;
        }
        if !g.stop.is_empty() {
            db.add_stop_reasons(PlayerId::FIRST, id, &g.stop, None, "menu")?;
        }
    }
    for s in &lib.profile {
        let title_id = match &s.game {
            Some(name) => Some(*ids.get(name).with_context(|| format!("the profile names {name}, which isn't in the mock library"))?),
            None => None,
        };
        let body = StatementBody { kind: Some(s.kind.clone()), title_id, about: s.about.clone(), why: s.why.clone(), note: s.note.clone() };
        db.taste_state(PlayerId::FIRST, &taste::check_statement(db, PlayerId::FIRST, &body)?)?;
    }
    for r in lib.routines {
        let checked = taste::routine::check(r, &db.routines(PlayerId::FIRST)?)?;
        db.add_routine(PlayerId::FIRST, &checked)?;
    }
    if let Some(sam) = sam {
        second_player(db, sam, second, &ids)?;
    }
    db.setting_set(PlayerId::FIRST, "auto_sync", Some("false"))?;
    // Other players' backlog verdicts, as if the daily job had downloaded
    // them, and turned on.
    let snap = verdicts_snapshot()?;
    let summary = crate::verdicts::format::read_summary(&snap.summary)?;
    crate::verdicts::receive::apply(db, &summary, "the mock library", None)?;
    db.setting_set(PlayerId::FIRST, crate::db::VERDICTS_RECEIVE, Some("true"))?;
    let diary = diary(db, t)?;
    if let Some(sam) = sam {
        taste::update(db, sam, &TasteRules::default())?;
    }
    let update = taste::update(db, PlayerId::FIRST, &TasteRules::default())?;
    Ok(Seeded {
        games: if two_players { lib.games.len() } else { lib.games.iter().filter(|g| !theirs_only(g)).count() },
        statements: lib.profile.len(),
        patterns: update.patterns,
        diary,
        routines: db.routines(PlayerId::FIRST)?.len(),
    })
}

/// The mock library's snapshot of other players' backlog verdicts, as the
/// service would publish it: `tests/fixtures/verdicts/mock/` is a copy the
/// end to end run's fake service serves.
pub fn verdicts_snapshot() -> Result<crate::verdicts::format::Snapshot> {
    let lib: Library = serde_json::from_str(LIBRARY).context("reading the mock library")?;
    crate::verdicts::format::write_snapshot(verdicts::DATE, &verdicts::all(&lib.games)?)
}

/// Player 2's marks and profile, and the games player 1 hid from them.
fn second_player(db: &Db, sam: PlayerId, second: &SecondPlayer, ids: &BTreeMap<String, i64>) -> Result<()> {
    let id = |name: &str| ids.get(name).copied().with_context(|| format!("{name} isn't in the mock library"));
    for m in &second.statuses {
        let o = Override::parse(&m.st).with_context(|| format!("{}: {} isn't a status", m.n, m.st))?;
        db.set_override(sam, id(&m.n)?, Some(o), None)?;
        if !m.stop.is_empty() {
            db.add_stop_reasons(sam, id(&m.n)?, &m.stop, None, "menu")?;
        }
    }
    for s in &second.profile {
        let title_id = s.game.as_deref().map(id).transpose()?;
        let body = StatementBody { kind: Some(s.kind.clone()), title_id, about: s.about.clone(), why: s.why.clone(), note: s.note.clone() };
        db.taste_state(sam, &taste::check_statement(db, sam, &body)?)?;
    }
    for name in &second.hidden_by_first {
        db.set_hidden(PlayerId::FIRST, id(name)?, Some(HideScope::Others))?;
    }
    Ok(())
}

/// Three weeks of a made-up play diary, switched on: an hour or so on
/// weeknight evenings with the games being played now, and three-hour
/// weekend evenings that start with a co-op game. Returns the rows written.
fn diary(db: &Db, t: i64) -> Result<usize> {
    use chrono::{Datelike, TimeZone};
    let games = db.all_games(PlayerId::FIRST)?;
    let playing: Vec<&crate::db::GameView> = games.iter().filter(|g| g.status == crate::db::Status::Playing).collect();
    let coop = games.iter().find(|g| g.tags.iter().any(|t| t == "Co-op" || t == "Online Co-Op"));
    let Some(first) = playing.first() else { return Ok(0) };
    let today = chrono::Local.timestamp_opt(t, 0).single().context("today")?.date_naive();
    let mut rows = vec![];
    let mut row = |g: &crate::db::GameView, hour: i64, minutes: i64| {
        rows.push(NewDiaryRow {
            hour,
            at: hour + 3600,
            title_id: Some(g.id),
            appid: g.appid.map(i64::from),
            name: g.name.clone(),
            minutes,
            source: "steam",
            player: Some(PlayerId::FIRST),
            account: None,
        })
    };
    for back in 1..=21u64 {
        let day = today - chrono::Days::new(back);
        let at = |h: u32| chrono::Local.from_local_datetime(&day.and_hms_opt(h, 0, 0).unwrap_or_default()).single().map(|d| d.timestamp());
        let g = playing[back as usize % playing.len()];
        match day.weekday() {
            chrono::Weekday::Sat | chrono::Weekday::Sun => {
                let Some(start) = at(19) else { continue };
                row(coop.unwrap_or(g), start, 60);
                row(g, start + 3600, 60);
                row(first, start + 7200, 45);
            }
            _ if back % 3 != 0 => {
                let Some(start) = at(20) else { continue };
                row(g, start, 50 + (back as i64 % 3) * 10);
            }
            _ => {}
        }
    }
    db.setting_set(PlayerId::FIRST, "play_diary", Some("true"))?;
    db.diary_add(&rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Status;

    #[test]
    fn the_mock_library_seeds_a_spread_of_statuses_and_a_profile() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("upnext.db"), 120).unwrap();
        let seeded = seed(&db).unwrap();
        let games = db.all_games(PlayerId::FIRST).unwrap();
        assert_eq!(games.len(), seeded.games, "one title per game");
        let count = |s: Status| games.iter().filter(|g| g.status == s).count();
        assert!(games.iter().filter(|g| g.staple).count() >= 4, "staples");
        for s in [Status::Untouched, Status::Played, Status::Shelved, Status::Abandoned, Status::Sampled] {
            assert!(count(s) >= 3, "{s:?}: {}", count(s));
        }
        assert!(games.iter().all(|g| g.review_tags.is_some() && g.hltb_main.is_some()), "every game has review tags and a length");
        // Play links as sync makes them, for the launchers that have them.
        let link =
            |g: &crate::db::GameView| ["steam://run/", "heroic://launch/", "uplay://launch/"].iter().any(|p| g.run_url.starts_with(p));
        assert!(games.iter().all(|g| g.run_url.is_empty() || link(g)), "only real-looking Play links");
        let linked = games.iter().filter(|g| link(g)).count();
        assert!(linked * 10 >= games.len() * 9, "nearly every game has a Play link: {linked} of {}", games.len());
        assert!(seeded.patterns >= 2, "the profile has patterns to read: {seeded:?}");
        let profile = taste::Profile::load(&db, PlayerId::FIRST, &games).unwrap();
        assert_eq!((profile.favourites.len(), profile.hard_nos.len()), (3, 1));
        assert!(games.iter().any(|g| profile.hard_no(g).is_some()), "the hard no on horror has games to leave out");
        assert!(seeded.diary >= 15, "a made-up play diary: {seeded:?}");
        assert_eq!(seeded.routines, 3, "routines to choose on the Pick page");
        let portal = games.iter().find(|g| g.name == "Portal 2").unwrap();
        assert!(portal.categories.iter().any(|c| c == "Full controller support"), "store categories: {:?}", portal.categories);
        let d = taste::diary::load_habits(&db, PlayerId::FIRST, &games, now()).unwrap();
        assert!(d.weeknight.is_some() && d.weekend.is_some() && !d.times.is_empty(), "{d:?}");
        let mut launchers: Vec<String> = games.iter().flat_map(|g| &g.copies).map(|c| format!("{}/{}", c.launcher, c.source)).collect();
        launchers.sort();
        launchers.dedup();
        assert_eq!(
            launchers,
            ["epic/epic", "heroic/epic", "heroic/gog", "manual/battlenet", "origin/origin", "steam/steam", "uplay/uplay"],
            "every kind of store copy, the EA app's and Ubisoft Connect's included"
        );

        // A second player with a small Steam account of their own.
        let sam = db.players().unwrap()[1].id;
        let theirs = db.all_games(sam).unwrap();
        let minutes = |gs: &[crate::db::GameView], name: &str| gs.iter().find(|g| g.name == name).map(|g| g.playtime_minutes);
        assert_eq!((minutes(&theirs, "Stardew Valley"), minutes(&games, "Stardew Valley")), (Some(6000), Some(0)));
        assert_eq!((minutes(&theirs, "Portal 2"), minutes(&games, "Portal 2")), (Some(900), Some(600)), "each their own hours");
        assert_eq!((minutes(&theirs, "Hotline Miami"), minutes(&games, "Hotline Miami")), (None, Some(0)), "player 1 hid it");
        let marked = |gs: &[crate::db::GameView], name: &str| gs.iter().find(|g| g.name == name).and_then(|g| g.override_);
        assert_eq!((marked(&theirs, "Portal 2"), marked(&games, "Portal 2")), (Some(Override::Done), None));
        assert_eq!(db.taste_statements(sam).unwrap().len(), 4);
        let own = db.private_setting(sam, "setup.player").unwrap().unwrap_or_default();
        assert!(own.contains(r#""setup":"done""#), "their library is filled in, so no setup of their own: {own}");
        assert!(seed(&db).is_err(), "never into a library that has games");
    }

    /// `--one-player`: the first player alone, with their whole profile and
    /// nothing from the second player.
    #[test]
    fn the_mock_library_can_have_one_player() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("upnext.db"), 120).unwrap();
        let seeded = seed_one_player(&db).unwrap();
        assert_eq!(db.players().unwrap().len(), 1);
        let games = db.all_games(PlayerId::FIRST).unwrap();
        assert_eq!(games.len(), seeded.games);
        assert!(games.iter().any(|g| g.name == "Hotline Miami"), "nothing is hidden with no one to hide it from");
        assert!(db.hidden_by(PlayerId::FIRST).unwrap().is_empty());
        let lib: Library = serde_json::from_str(LIBRARY).unwrap();
        let theirs: Vec<&str> = lib.second_player.steam.iter().filter(|o| o.only).map(|o| o.n.as_str()).collect();
        assert!(!theirs.is_empty() && games.iter().all(|g| !theirs.contains(&g.name.as_str())), "the second player's own games stay out");
        let profile = taste::Profile::load(&db, PlayerId::FIRST, &games).unwrap();
        assert_eq!((profile.favourites.len(), profile.hard_nos.len()), (3, 1), "the first player's whole profile");
        assert_eq!(seeded.routines, 3);
    }
}
