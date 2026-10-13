//! Where a started game stands: whether it has an ending, whether
//! it's one of your staples, and the status UpNext works out when you haven't
//! set one. All of it is computed when the library is read, from playtime,
//! last played, HowLongToBeat and tags, against [`StatusRules`].

use crate::db::{Override, Status};
use crate::review_tags::ReviewTags;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

const DAY: i64 = 86_400;

/// Played to this share of its HowLongToBeat main story, a game was likely
/// finished: people beat games faster than the average, and a game left just
/// short of its main story's time read as abandoned without this (2026-10-01).
const LIKELY_FINISHED: f64 = 0.8;

/// The settings the statuses follow. The defaults match `config::SETTINGS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StatusRules {
    /// Under this much playtime a game is sampled; at or above, played.
    pub sampled_minutes: i64,
    /// Untouched this long, a game with an ending you haven't finished is shelved.
    pub shelve_after_days: i64,
    /// Shelved and untouched this long, it's abandoned.
    pub abandon_after_days: i64,
    /// This many hours make a game one of your staples.
    pub staple_hours: i64,
    /// How long Burnt out keeps a game out of picks.
    pub burnout_days: i64,
}

impl Default for StatusRules {
    fn default() -> Self {
        Self { sampled_minutes: 120, shelve_after_days: 60, abandon_after_days: 730, staple_hours: 100, burnout_days: 90 }
    }
}

/// Whether a game has an ending to reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Most story games. Near-endless ones (Skyrim, GTA V) are this kind too:
    /// they're never shelved once played past their main story.
    Ending,
    /// No end state: matches, sandboxes, builders, sims, MMOs.
    Endless,
}

/// Tags for games with no end state, each with how high among a game's tags
/// (strongest first) it has to be to count. HowLongToBeat can't tell these
/// games apart: it has a main-story time for nearly all of them (a
/// shooter's short campaign, a strategy game's campaign, hundreds of hours
/// for an MMO). The ranks come from a real library: further down, a tag is often
/// a side mode or a passing feature. Titanfall 2, whose campaign is the
/// point, has "Hero Shooter" 17th; Cult of the Lamb has "Colony Sim" 9th;
/// Fallout 4 and Skyrim have "Sandbox" 15th and 14th; Arkham Knight has
/// "Fighting" 10th. IGDB's names for the same things are included.
const ENDLESS_TAGS: &[(&str, usize)] = &[
    // What the game is, wherever it shows in the strongest 15.
    ("RTS", 15),
    ("Real Time Strategy (RTS)", 15),
    ("MOBA", 15),
    ("4X", 15),
    ("Grand Strategy", 15),
    ("Massively Multiplayer", 15),
    ("Massively Multiplayer Online (MMO)", 15),
    ("MMORPG", 15),
    ("Battle Royale", 15),
    ("Arena Shooter", 15),
    ("Hero Shooter", 15),
    ("Extraction Shooter", 15),
    ("e-sports", 15),
    ("Competitive", 15),
    ("Idler", 15),
    ("Clicker", 15),
    ("Software", 15),
    ("Utilities", 15),
    ("Benchmark", 15),
    // Tags that also turn up on games with an ending.
    ("City Builder", 8),
    ("Colony Sim", 8),
    ("Automation", 8),
    ("Farming Sim", 8),
    ("Life Sim", 8),
    ("Flight", 8),
    ("Sports", 8),
    ("Sport", 8),
    ("PvE", 8),
    ("2D Fighter", 8),
    ("3D Fighter", 8),
    ("Party Game", 8),
    ("Board Game", 8),
    ("Trading Card Game", 8),
    ("Fighting", 3),
    ("Sandbox", 5),
];

/// For a game HowLongToBeat has no main-story time for, one of these among
/// its 3 strongest tags makes it endless (the first rule;
/// further down, "Multiplayer" is often a co-op mode of a story game).
const UNTIMED_ENDLESS_TAGS: &[&str] = &["Multiplayer", "PvP", "Online PvP", "Simulation"];

/// Store categories for playing against others. A game with one of these
/// and no "Single-player" is played for its matches. Co-op isn't one: games
/// like Split Fiction are co-op only and have an ending.
const VERSUS_CATEGORIES: &[&str] = &["MMO", "PvP", "Online PvP"];

fn has(list: &[String], names: &[&str]) -> bool {
    list.iter().any(|t| names.iter().any(|n| t.eq_ignore_ascii_case(n)))
}

/// Whether a game has an ending, from what UpNext knows about it: its tags
/// (strongest first), its store categories, its HowLongToBeat main story and
/// what the reviews say. The game page's switch overrides this.
pub fn infer_kind(tags: &[String], categories: &[String], hltb_main: Option<f64>, review_tags: Option<&ReviewTags>) -> Kind {
    let ranked = ENDLESS_TAGS.iter().any(|(name, within)| tags.iter().take(*within).any(|t| t.eq_ignore_ascii_case(name)));
    let versus_only = has(categories, VERSUS_CATEGORIES) && !has(categories, &["Single-player"]);
    let untimed = hltb_main.is_none() && has(&tags[..tags.len().min(3)], UNTIMED_ENDLESS_TAGS);
    let no_story_replayed = review_tags.is_some_and(|t| t.has("story:none") && t.has("replay:high"));
    if ranked || versus_only || untimed || no_story_replayed { Kind::Endless } else { Kind::Ending }
}

/// Turned down this many times in picks, a shelved game is abandoned.
pub const SHELVED_REJECTIONS: usize = 2;

/// What [`classify`] works from: one title's playtime, the status you set
/// and what you did with it in picks.
#[derive(Debug, Clone, Copy)]
pub struct Facts<'a> {
    pub not_game: bool,
    pub playtime_known: bool,
    pub playtime_minutes: i64,
    /// Unix seconds; 0 when no store says.
    pub last_played: i64,
    pub hltb_main: Option<f64>,
    pub kind: Kind,
    /// The status you set, and when.
    pub mark: Option<(Override, i64)>,
    /// When it was turned down for real, oldest first: in picks ("It's too
    /// ___", Never recommend this) or on the Second Chance page (Still no).
    /// Not tonight never counts.
    pub rejections: &'a [i64],
    /// When you said "Willing to try again" on the Second Chance page.
    pub retry_at: Option<i64>,
    /// When it was last chosen from a lineup with Play.
    pub chosen_at: Option<i64>,
}

/// Where a title stands now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Standing {
    pub status: Status,
    /// The status you set, as it stands now: a Shelved mark untouched long
    /// enough reads Abandoned, a Burnt out mark past its break Still playing,
    /// and a Shelved, Abandoned or Burnt out mark you've played since is gone.
    pub mark: Option<Override>,
    pub staple: bool,
    /// When UpNext found it shelved or abandoned (None when you set it, or
    /// when no store says when it was last played).
    pub shelved_at: Option<i64>,
    pub abandoned_at: Option<i64>,
    /// When a Burnt out game comes back.
    pub burnt_out_until: Option<i64>,
    /// Times it was turned down in picks while it has this status: a
    /// shelved game since it was last played or marked, an abandoned one
    /// since it was abandoned. One spends an abandoned game's second chance.
    pub rejections: u32,
    /// Abandoned because it was turned down twice in picks, or once after
    /// you were willing to try it again.
    pub abandoned_by_picks: bool,
    /// Shelved by "Willing to try again": one rejection abandons it.
    pub second_try: bool,
}

/// The status of one title at `now`. Anything you set wins, except that
/// playing a game after marking it Shelved, Abandoned or Burnt out undoes
/// the mark. Otherwise UpNext infers only Playing, Shelved and Abandoned.
pub fn classify(f: &Facts, rules: &StatusRules, now: i64) -> Standing {
    let staple = !f.not_game && f.playtime_known && f.playtime_minutes >= rules.staple_hours * 60;
    let stand = |status: Status, mark: Option<Override>| Standing {
        status,
        mark,
        staple,
        shelved_at: None,
        abandoned_at: None,
        burnt_out_until: None,
        rejections: 0,
        abandoned_by_picks: false,
        second_try: false,
    };
    let rejected_after = |t: i64| f.rejections.iter().filter(|&&r| r > t).count() as u32;
    // A game shelved since it was last played or marked: abandoned once
    // `abandon_at` has passed or it's turned down `limit` times in picks,
    // whichever comes first.
    let shelved = |since: i64, abandon_at: Option<i64>, shelved_at: Option<i64>, mark: Option<Override>, limit: usize| {
        let by_time = abandon_at.filter(|&t| now >= t);
        let by_picks = f.rejections.iter().copied().filter(|&r| r > since).nth(limit - 1);
        let abandoned = match (by_time, by_picks) {
            (Some(t), Some(p)) => Some(t.min(p)),
            (t, p) => t.or(p),
        };
        match abandoned {
            Some(at) => Standing {
                abandoned_at: Some(at),
                // The rejection that abandoned it counts as its second chance spent.
                rejections: f.rejections.iter().filter(|&&r| r >= at).count() as u32,
                abandoned_by_picks: by_picks == Some(at),
                ..stand(Status::Abandoned, mark.map(|_| Override::Abandoned))
            },
            None => Standing { shelved_at, rejections: rejected_after(since), second_try: limit == 1, ..stand(Status::Shelved, mark) },
        }
    };
    if let Some((mark, at)) = f.mark {
        let played_since = matches!(mark, Override::Shelved | Override::Abandoned | Override::BurntOut) && f.last_played > at;
        if !played_since {
            return match mark {
                Override::Shelved => {
                    let since = at.max(f.last_played);
                    // Shelved from the Second Chance page: its last chance.
                    let limit = if f.retry_at.is_some_and(|t| t >= at) { 1 } else { SHELVED_REJECTIONS };
                    shelved(since, Some(since + rules.abandon_after_days * DAY), None, Some(mark), limit)
                }
                Override::Abandoned => Standing { rejections: rejected_after(at), ..stand(Status::Abandoned, Some(mark)) },
                Override::BurntOut => {
                    let back = at + rules.burnout_days * DAY;
                    if now < back {
                        Standing { burnt_out_until: Some(back), ..stand(Status::BurntOut, Some(mark)) }
                    } else {
                        stand(Status::StillPlaying, Some(Override::StillPlaying))
                    }
                }
                _ => stand(mark.status(), Some(mark)),
            };
        }
    }
    if f.not_game {
        return stand(Status::NotGame, None);
    }
    if !f.playtime_known || f.playtime_minutes == 0 {
        // No store reports its playtime, so choosing it with Play is all
        // UpNext knows: you're playing it, for as long as a played game
        // would count as Playing.
        if !f.playtime_known && f.chosen_at.is_some_and(|t| now - t < rules.shelve_after_days * DAY) {
            return stand(Status::Playing, None);
        }
        return stand(Status::Untouched, None);
    }
    if f.playtime_minutes < rules.sampled_minutes {
        return stand(Status::Sampled, None);
    }
    let known = f.last_played > 0;
    if known && now - f.last_played < rules.shelve_after_days * DAY {
        return stand(Status::Playing, None);
    }
    // Played to near its main story or past it, it was likely finished, or
    // it's a game people play far past its ending; no known length counts
    // as short of it.
    let under_story = f.hltb_main.is_none_or(|h| (f.playtime_minutes as f64) < LIKELY_FINISHED * h * 60.0);
    if f.kind == Kind::Endless || staple || !under_story {
        return stand(Status::Played, None);
    }
    if !known {
        return shelved(0, None, None, None, SHELVED_REJECTIONS);
    }
    let (abandon_at, shelved_at) = (f.last_played + rules.abandon_after_days * DAY, f.last_played + rules.shelve_after_days * DAY);
    shelved(f.last_played, Some(abandon_at), Some(shelved_at), None, SHELVED_REJECTIONS)
}

/// How a game can come up in picks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Offer {
    /// Not started, or only tried.
    New,
    /// Put down before its ending: offered with its history.
    Shelved,
    /// Abandoned and not turned down since: its one second chance.
    SecondChance,
    /// An endless game you've played, or one of your staples: offered as a
    /// session, never as something to finish.
    Session,
}

/// Whether picks can offer a game, and how. Out: Playing (you're playing
/// it), Burnt out during its break, Beat it, Not for me, a played game with
/// an ending that's no staple (likely finished), an abandoned game turned
/// down since it was abandoned (Still no on the Second Chance page counts),
/// and one whose reviews since say what stopped you is `still_the_same`.
pub fn offer(status: Status, kind: Kind, staple: bool, rejections: u32, still_the_same: bool) -> Option<Offer> {
    match status {
        Status::Untouched | Status::Sampled => Some(Offer::New),
        Status::Shelved => Some(Offer::Shelved),
        Status::Abandoned if rejections == 0 && !still_the_same => Some(Offer::SecondChance),
        Status::StillPlaying => Some(Offer::Session),
        Status::Played if kind == Kind::Endless || staple => Some(Offer::Session),
        _ => None,
    }
}

/// Whose fault it was that a game was put down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReasonGroup {
    /// Never counted against similar games.
    Life,
    /// Compared with what reviews say about the game.
    Game,
}

/// One reason a game was put down. `GET /api/stop-reasons` serves them.
#[derive(Serialize, utoipa::ToSchema)]
pub struct Reason {
    /// Stored and sent over the API.
    pub code: &'static str,
    pub group: ReasonGroup,
    /// As the UI says it.
    pub label: &'static str,
    /// The review-tag aspects (`review_tags::ASPECTS`) it is checked against.
    pub aspects: &'static [&'static str],
}

const fn reason(code: &'static str, group: ReasonGroup, label: &'static str, aspects: &'static [&'static str]) -> Reason {
    Reason { code, group, label, aspects }
}

/// Every reason, in the order the UI lists them.
pub const REASONS: &[Reason] = &[
    reason("busy", ReasonGroup::Life, "Got busy", &[]),
    reason("other_game", ReasonGroup::Life, "Something else came along", &[]),
    reason("mood", ReasonGroup::Life, "Wasn't in the mood anymore", &[]),
    reason("elsewhere", ReasonGroup::Life, "Played it somewhere else", &[]),
    reason("grind", ReasonGroup::Game, "The grind", &["grind"]),
    reason("stuck", ReasonGroup::Game, "Got stuck", &["difficulty balance"]),
    reason("lost_story", ReasonGroup::Game, "Lost the thread of the story", &["story"]),
    reason("too_long", ReasonGroup::Game, "Too long", &["length"]),
    reason("pacing", ReasonGroup::Game, "The pacing dragged", &["pacing"]),
    reason("bugs", ReasonGroup::Game, "Bugs or performance", &["bugs", "performance"]),
    reason("too_hard", ReasonGroup::Game, "Too hard", &["difficulty balance"]),
    reason("boring", ReasonGroup::Game, "Got boring", &["pacing"]),
];

/// Where stop reasons were given.
pub const REASON_SOURCES: [&str; 3] = ["menu", "pick", "second_chance"];

/// The longest note kept with stop reasons, in characters.
const MAX_REASON_NOTE: usize = 500;

/// Why a game was put down, the latest time it was asked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct StopReasons {
    /// Codes from [`REASONS`], in its order.
    pub codes: Vec<String>,
    pub note: Option<String>,
    /// menu, pick or second_chance.
    pub source: String,
    /// Unix seconds.
    pub at: i64,
}

/// Check reason codes and a note as typed: known codes only, each once, in
/// the menu's order; a blank note is none. Returns None when both are empty.
pub fn check_reasons(codes: &[String], note: Option<&str>) -> Result<Option<(Vec<String>, Option<String>)>> {
    if let Some(bad) = codes.iter().find(|c| !REASONS.iter().any(|r| r.code == c.as_str())) {
        let known: Vec<&str> = REASONS.iter().map(|r| r.code).collect();
        bail!("unknown reason {bad:?}; known: {}", known.join(", "));
    }
    let codes: Vec<String> = REASONS.iter().filter(|r| codes.iter().any(|c| c == r.code)).map(|r| r.code.to_string()).collect();
    let note = note.map(str::trim).filter(|n| !n.is_empty());
    if note.is_some_and(|n| n.chars().count() > MAX_REASON_NOTE) {
        bail!("the note is longer than {MAX_REASON_NOTE} characters");
    }
    if codes.is_empty() && note.is_none() {
        return Ok(None);
    }
    Ok(Some((codes, note.map(str::to_string))))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_790_000_000;

    fn tags(t: &[&str]) -> Vec<String> {
        t.iter().map(|s| s.to_string()).collect()
    }

    fn played(hours: f64, days_ago: i64, hltb: Option<f64>) -> Facts<'static> {
        Facts {
            not_game: false,
            playtime_known: true,
            playtime_minutes: (hours * 60.0) as i64,
            last_played: NOW - days_ago * DAY,
            hltb_main: hltb,
            kind: Kind::Ending,
            mark: None,
            rejections: &[],
            retry_at: None,
            chosen_at: None,
        }
    }

    fn status(f: &Facts) -> Status {
        classify(f, &StatusRules::default(), NOW).status
    }

    #[test]
    fn kinds_from_real_libraries() {
        // Steam's top tags and categories for games in a real library, cut short.
        let sp = tags(&["Single-player", "Multi-player", "PvP", "Online PvP"]);
        let titanfall = tags(&[
            "FPS",
            "Mechs",
            "Multiplayer",
            "Shooter",
            "Singleplayer",
            "Action",
            "Parkour",
            "Sci-fi",
            "First-Person",
            "Futuristic",
            "PvP",
            "Military",
            "Combat",
            "Fast-Paced",
            "Team-Based",
            "Time Travel",
            "Hero Shooter",
        ]);
        assert_eq!(infer_kind(&titanfall, &sp, Some(6.1), None), Kind::Ending, "a campaign that's the point");
        let battlefield_1 = tags(&[
            "FPS",
            "Multiplayer",
            "Singleplayer",
            "World War I",
            "War",
            "Shooter",
            "Action",
            "First-Person",
            "Historical",
            "Atmospheric",
            "PvP",
            "Massively Multiplayer",
        ]);
        assert_eq!(infer_kind(&battlefield_1, &sp, Some(6.7), None), Kind::Endless, "a campaign HLTB times, but it's about matches");
        let mp_only = tags(&["Multi-player", "PvP", "Online PvP", "Co-op"]);
        assert_eq!(infer_kind(&tags(&["FPS", "Multiplayer", "Shooter"]), &mp_only, Some(42.5), None), Kind::Endless, "Battlefield 2042");
        let rise_of_nations = tags(&["Strategy", "RTS", "Classic", "Historical", "Multiplayer"]);
        assert_eq!(infer_kind(&rise_of_nations, &sp, Some(19.8), None), Kind::Endless);
        let god_of_war = tags(&["Action", "Singleplayer", "Story Rich", "Mythology", "Adventure"]);
        assert_eq!(infer_kind(&god_of_war, &tags(&["Single-player"]), Some(20.6), None), Kind::Ending);
        assert_eq!(infer_kind(&tags(&["Benchmark", "Utilities", "Software"]), &[], None, None), Kind::Endless, "3DMark");

        // The first rules still hold.
        assert_eq!(infer_kind(&tags(&["Realistic", "Simulation", "VR"]), &sp, None, None), Kind::Endless, "no main story, a sim");
        let no_story: ReviewTags = serde_json::from_str(r#"{"scales": {"story": "none", "replay": "high"}}"#).unwrap();
        assert_eq!(infer_kind(&tags(&["FPS", "Shooter"]), &sp, Some(14.4), Some(&no_story)), Kind::Endless, "Splitgate");
        let light: ReviewTags = serde_json::from_str(r#"{"scales": {"story": "light", "replay": "high"}}"#).unwrap();
        assert_eq!(infer_kind(&tags(&["FPS", "Shooter"]), &sp, Some(14.4), Some(&light)), Kind::Ending);

        // A tag counts only high enough in the list: past the strongest 15,
        // and for broad tags sooner.
        let at = |tag: &str, rank: usize| {
            let mut t = tags(&vec!["Action"; rank - 1]);
            t.push(tag.into());
            infer_kind(&t, &[], Some(10.0), None)
        };
        assert_eq!((at("RTS", 15), at("RTS", 16)), (Kind::Endless, Kind::Ending));
        assert_eq!((at("Colony Sim", 8), at("Colony Sim", 9)), (Kind::Endless, Kind::Ending), "Cult of the Lamb has it 9th");
        assert_eq!((at("Sandbox", 5), at("Sandbox", 14)), (Kind::Endless, Kind::Ending), "Skyrim has it 14th");
        assert_eq!(at("Fighting", 10), Kind::Ending, "Arkham Knight");

        // Co-op only isn't endless; versus only is.
        let split_fiction = tags(&["Co-op", "Split Screen", "Female Protagonist", "Action-Adventure", "3D Platformer"]);
        let coop = tags(&["Online Co-op", "Shared/Split Screen Co-op", "Multi-player"]);
        assert_eq!(infer_kind(&split_fiction, &coop, Some(13.8), None), Kind::Ending);
        // Multiplayer further down with no main story is a story game's co-op (Divinity: Original Sin 2).
        let dos2 = tags(&["Tactical RPG", "RPG", "Turn-Based Strategy", "Story Rich", "Fantasy", "Co-op", "Multiplayer"]);
        assert_eq!(infer_kind(&dos2, &sp, None, None), Kind::Ending);
        assert_eq!(infer_kind(&tags(&["Action", "Multiplayer", "FPS"]), &sp, None, None), Kind::Endless, "Black Ops II multiplayer");
    }

    #[test]
    fn started_games_are_inferred() {
        // A 14-hour story game untouched for 3 months is shelved, from when it crossed 2 months.
        let shelved = classify(&played(14.0, 90, Some(20.0)), &StatusRules::default(), NOW);
        assert_eq!((shelved.status, shelved.shelved_at, shelved.abandoned_at), (Status::Shelved, Some(NOW - 30 * DAY), None));
        // The same game played a month ago is Playing.
        assert_eq!(status(&played(14.0, 30, Some(20.0))), Status::Playing);
        // Two years untouched, abandoned.
        let gone = classify(&played(14.0, 800, Some(20.0)), &StatusRules::default(), NOW);
        assert_eq!((gone.status, gone.abandoned_at, gone.shelved_at), (Status::Abandoned, Some(NOW - 70 * DAY), None));
        // Past its main story: likely finished, or played far past the end (GTA V, 150 of 32 hours).
        let gta = classify(&played(150.0, 400, Some(32.0)), &StatusRules::default(), NOW);
        assert_eq!((gta.status, gta.staple), (Status::Played, true));
        // 80% of the story is likely finished (7.6 of 8.0 hours, 9.5 of 11.8).
        assert_eq!(status(&played(7.6, 700, Some(8.0))), Status::Played);
        assert_eq!(status(&played(9.5, 3000, Some(11.8))), Status::Played);
        assert_eq!(status(&played(15.9, 90, Some(20.0))), Status::Shelved, "79%");
        // No known length counts as under it; no known last play is shelved, not abandoned.
        assert_eq!(status(&played(14.0, 90, None)), Status::Shelved);
        assert_eq!(status(&Facts { last_played: 0, ..played(14.0, 0, Some(20.0)) }), Status::Shelved);
        // Endless games and staples are never shelved by inference.
        assert_eq!(status(&Facts { kind: Kind::Endless, ..played(14.0, 900, Some(6.0 * 60.0)) }), Status::Played);
        let staple = classify(&played(120.0, 900, Some(300.0)), &StatusRules::default(), NOW);
        assert_eq!((staple.status, staple.staple), (Status::Played, true));
        // Untouched and sampled games stay as they are; so does a game with no playtime.
        assert_eq!(status(&played(0.0, 900, Some(20.0))), Status::Untouched);
        assert_eq!(status(&played(1.5, 900, Some(20.0))), Status::Sampled);
        assert_eq!(status(&Facts { playtime_known: false, ..played(0.0, 900, Some(20.0)) }), Status::Untouched);
        assert_eq!(status(&Facts { not_game: true, ..played(14.0, 900, Some(20.0)) }), Status::NotGame);
    }

    #[test]
    fn every_threshold_follows_its_setting() {
        let rules = StatusRules { sampled_minutes: 60, shelve_after_days: 30, abandon_after_days: 365, staple_hours: 50, burnout_days: 10 };
        let at = |f: &Facts| classify(f, &rules, NOW);
        assert_eq!(at(&played(1.5, 400, Some(20.0))).status, Status::Abandoned, "sampled under 60 minutes, abandoned after a year");
        assert_eq!(at(&played(14.0, 45, Some(20.0))).status, Status::Shelved, "shelved after 30 days");
        let staple = at(&played(55.0, 45, Some(80.0)));
        assert_eq!((staple.status, staple.staple), (Status::Played, true), "a staple at 50 hours");
        let burnt = Facts { mark: Some((Override::BurntOut, NOW - 11 * DAY)), ..played(55.0, 45, None) };
        assert_eq!(at(&burnt).status, Status::StillPlaying, "back after 10 days");
    }

    #[test]
    fn marks_win_until_you_play_again() {
        let set = |o: Override, days_ago: i64, last_played_days_ago: i64| Facts {
            mark: Some((o, NOW - days_ago * DAY)),
            ..played(14.0, last_played_days_ago, Some(20.0))
        };
        let rules = StatusRules::default();
        let done = classify(&set(Override::Done, 5, 900), &rules, NOW);
        assert_eq!((done.status, done.mark), (Status::Done, Some(Override::Done)));
        // Burnt out for 3 months, then back by itself.
        let burnt = classify(&set(Override::BurntOut, 10, 20), &rules, NOW);
        assert_eq!((burnt.status, burnt.burnt_out_until), (Status::BurntOut, Some(NOW + 80 * DAY)));
        let back = classify(&set(Override::BurntOut, 91, 100), &rules, NOW);
        assert_eq!((back.status, back.mark), (Status::StillPlaying, Some(Override::StillPlaying)));
        // Shelved by hand counts from the mark, not from an old last play.
        assert_eq!(classify(&set(Override::Shelved, 10, 2000), &rules, NOW).status, Status::Shelved);
        let lapsed = classify(&set(Override::Shelved, 731, 2000), &rules, NOW);
        assert_eq!((lapsed.status, lapsed.mark, lapsed.abandoned_at), (Status::Abandoned, Some(Override::Abandoned), Some(NOW - DAY)));
        // Played since: the mark no longer says where it stands.
        let again = classify(&set(Override::Abandoned, 30, 2), &rules, NOW);
        assert_eq!((again.status, again.mark), (Status::Playing, None));
        assert_eq!(classify(&set(Override::Done, 30, 2), &rules, NOW).status, Status::Done, "beaten stays beaten");
        assert_eq!(classify(&set(Override::Hidden, 30, 2), &rules, NOW).status, Status::Hidden);
        // A mark wins over "not a game", as before.
        assert_eq!(classify(&Facts { not_game: true, ..set(Override::Done, 5, 900) }, &rules, NOW).status, Status::Done);
    }

    #[test]
    fn rejections_in_picks_move_a_game_along() {
        let rules = StatusRules::default();
        let at = |f: &Facts| classify(f, &rules, NOW);
        let offer_of = |s: &Standing| offer(s.status, Kind::Ending, s.staple, s.rejections, false);
        let shelved = played(14.0, 90, Some(20.0));
        assert_eq!(offer_of(&at(&shelved)), Some(Offer::Shelved));

        // Turned down once, still shelved; twice, abandoned and out of picks.
        let once = [NOW - 5 * DAY];
        let one = at(&Facts { rejections: &once, ..shelved });
        assert_eq!((one.status, one.rejections), (Status::Shelved, 1));
        let twice = [NOW - 5 * DAY, NOW - DAY];
        let two = at(&Facts { rejections: &twice, ..shelved });
        assert_eq!((two.status, two.abandoned_at, two.abandoned_by_picks), (Status::Abandoned, Some(NOW - DAY), true));
        assert_eq!(offer_of(&two), None, "the rejection that abandoned it spends its second chance");
        // Rejections from before it was last played don't count.
        let old = [NOW - 200 * DAY, NOW - 100 * DAY];
        assert_eq!(at(&Facts { rejections: &old, ..shelved }).status, Status::Shelved);
        // Marked Shelved by hand: counted from the mark.
        let marked = Facts { mark: Some((Override::Shelved, NOW - 3 * DAY)), ..played(14.0, 90, Some(20.0)) };
        let both = at(&Facts { rejections: &twice, ..marked });
        assert_eq!((both.status, both.mark, both.rejections), (Status::Shelved, Some(Override::Shelved), 1));

        // Abandoned after two years untouched: one second chance, then out for good.
        let gone = played(14.0, 800, Some(20.0));
        assert_eq!(offer_of(&at(&gone)), Some(Offer::SecondChance));
        let before = [NOW - 75 * DAY];
        assert_eq!(offer_of(&at(&Facts { rejections: &before, ..gone })), Some(Offer::SecondChance), "turned down while still shelved");
        let since = [NOW - 2 * DAY];
        assert_eq!(offer_of(&at(&Facts { rejections: &since, ..gone })), None);
        // Marked Abandoned by hand: one second chance from the mark.
        let dropped = Facts { mark: Some((Override::Abandoned, NOW - 10 * DAY)), ..played(14.0, 300, Some(20.0)) };
        assert_eq!(offer_of(&at(&dropped)), Some(Offer::SecondChance));
        assert_eq!(offer_of(&at(&Facts { rejections: &since, ..dropped })), None);

        // Untouched and sampled games stay in picks however often they're turned down.
        let three = [NOW - 3 * DAY, NOW - 2 * DAY, NOW - DAY];
        assert_eq!(offer_of(&at(&Facts { rejections: &three, ..played(0.0, 0, Some(20.0)) })), Some(Offer::New));
        assert_eq!(offer_of(&at(&Facts { rejections: &three, ..played(1.0, 400, Some(20.0)) })), Some(Offer::New));
    }

    #[test]
    fn second_chance_answers_stick() {
        let rules = StatusRules::default();
        let at = |f: &Facts| classify(f, &rules, NOW);
        let offer_of = |s: &Standing| offer(s.status, Kind::Ending, s.staple, s.rejections, false);
        // Abandoned, then "Willing to try again": Shelved, and one rejection in picks ends it.
        let willing = NOW - 5 * DAY;
        let again = Facts { mark: Some((Override::Shelved, willing)), retry_at: Some(willing), ..played(14.0, 800, Some(20.0)) };
        let back = at(&again);
        assert_eq!((back.status, back.second_try, offer_of(&back)), (Status::Shelved, true, Some(Offer::Shelved)));
        let once = [NOW - DAY];
        let out = at(&Facts { rejections: &once, ..again });
        assert_eq!((out.status, out.abandoned_by_picks, out.second_try, offer_of(&out)), (Status::Abandoned, true, false, None));
        // Marked Shelved again later from the menu: the usual two.
        let later = at(&Facts { mark: Some((Override::Shelved, NOW - 2 * DAY)), rejections: &once, ..again });
        assert_eq!((later.status, later.second_try, later.rejections), (Status::Shelved, false, 1));
        // Still no comes in as a rejection, and spends the second chance.
        let gone = played(14.0, 800, Some(20.0));
        assert_eq!(offer_of(&at(&Facts { rejections: &once, ..gone })), None);
        // Reviews since say what stopped you is still there: no second chance.
        assert_eq!(offer(Status::Abandoned, Kind::Ending, false, 0, true), None);
        assert_eq!(offer(Status::Shelved, Kind::Ending, false, 0, true), Some(Offer::Shelved), "only second chances");
    }

    #[test]
    fn what_picks_offer() {
        let s = |status: Status, kind: Kind, staple: bool| offer(status, kind, staple, 0, false);
        assert_eq!(s(Status::Untouched, Kind::Endless, false), Some(Offer::New));
        assert_eq!(s(Status::StillPlaying, Kind::Endless, false), Some(Offer::Session));
        assert_eq!(s(Status::Played, Kind::Endless, false), Some(Offer::Session), "Battlefield, played");
        assert_eq!(s(Status::Played, Kind::Ending, true), Some(Offer::Session), "GTA V at 150 hours");
        assert_eq!(s(Status::Played, Kind::Ending, false), None, "likely finished");
        for out in [Status::Playing, Status::BurntOut, Status::Done, Status::Hidden, Status::NotGame] {
            assert_eq!(s(out, Kind::Ending, true), None, "{out:?}");
        }
    }

    #[test]
    fn play_on_a_game_without_playtime_means_playing() {
        let rules = StatusRules::default();
        let epic = Facts { playtime_known: false, last_played: 0, ..played(0.0, 0, Some(20.0)) };
        assert_eq!(classify(&Facts { chosen_at: Some(NOW - 10 * DAY), ..epic }, &rules, NOW).status, Status::Playing);
        assert_eq!(
            classify(&Facts { chosen_at: Some(NOW - 61 * DAY), ..epic }, &rules, NOW).status,
            Status::Untouched,
            "it says what it knows"
        );
        // A store that reports playtime says whether you started it.
        let steam = played(0.0, 0, Some(20.0));
        assert_eq!(classify(&Facts { chosen_at: Some(NOW - DAY), ..steam }, &rules, NOW).status, Status::Untouched);
    }

    #[test]
    fn reasons_are_checked_and_ordered() {
        let codes = |c: &[&str]| c.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let (got, note) = check_reasons(&codes(&["grind", "busy", "grind"]), Some("  the second act  ")).unwrap().unwrap();
        assert_eq!((got, note.as_deref()), (codes(&["busy", "grind"]), Some("the second act")));
        assert_eq!(check_reasons(&[], Some("  ")).unwrap(), None);
        assert!(check_reasons(&codes(&["meh"]), None).unwrap_err().to_string().starts_with("unknown reason \"meh\""));
        assert!(check_reasons(&[], Some(&"x".repeat(501))).is_err());
        let aspects = crate::review_tags::ASPECTS;
        for r in REASONS {
            assert_eq!(r.aspects.is_empty(), r.group == ReasonGroup::Life, "{}: life reasons are never compared with reviews", r.code);
            assert!(r.aspects.iter().all(|a| aspects.contains(a)), "{}: aspects come from review_tags", r.code);
        }
    }
}
