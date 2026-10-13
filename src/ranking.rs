//! Picks without AI: a lineup from UpNext's own ranking, the one
//! that orders the AI's candidates, with reasons built from the data and a
//! list of what it was ranked by, for the warning above the lineup.

use crate::db::{GameView, PICK_RATING_DAYS, Pick};
use crate::discovery::Mode;
use crate::lifecycle::Offer;
use crate::prompt::{self, BudgetFrom, Factor, PromptInput, Ranking, Scored};
use crate::recommend::Screen;
use crate::taste::When;

/// Points off a game shown in the last few lineups, as the AI is told to
/// avoid them unless they clearly fit best.
const RECENT_PENALTY: f64 = 10.0;
/// Points off a game for each reason given for turning games down that
/// it shares ("too long" and it's no shorter).
const TURNED_DOWN_PENALTY: f64 = 20.0;
/// Fragments in a reason, at most.
const REASON_PARTS: usize = 5;
/// Store genres that say nothing about the kind of game.
const NOT_GENRES: &[&str] = &["indie", "casual", "free to play", "free-to-play", "early access", "massively multiplayer"];

/// A reason for turning games down, with the games it was said about:
/// "Not these, because…" on a lineup, or "It's too ___" on one game.
pub struct Said<'a> {
    pub reason: String,
    pub about: Vec<&'a GameView>,
}

/// How the ranking reads a reason, without the AI.
enum Steer {
    /// "Too long", "too slow", or "Want something newer" (too old).
    Too(String),
    /// "Want something with a story": words for the request.
    Wish(String),
    /// "Not in the mood for these genres".
    Genres,
    /// Anything else: only the AI reads it.
    Unread,
}

fn read(reason: &str) -> Steer {
    let r = reason.trim().to_lowercase().replace('\u{2019}', "'");
    let r = r.trim_end_matches(['.', '!']);
    if let Some(w) = r.strip_prefix("too ").or_else(|| r.strip_prefix("it's too ")) {
        Steer::Too(w.trim().to_string())
    } else if r.contains("newer") {
        Steer::Too("old".into())
    } else if r.contains("older") {
        Steer::Too("new".into())
    } else if r.contains("genre") {
        Steer::Genres
    } else if let Some(w) = r.strip_prefix("want ") {
        Steer::Wish(w.trim_start_matches("something ").to_string())
    } else {
        Steer::Unread
    }
}

impl Steer {
    /// Whether game `g` has what `said` turned games down for.
    fn holds(&self, g: &GameView, said: &Said) -> bool {
        match self {
            Steer::Too(word) => said.about.iter().any(|t| prompt::rules_out(word, t, g)),
            Steer::Genres => {
                let genre = |s: &String| !NOT_GENRES.contains(&s.to_lowercase().as_str());
                said.about
                    .iter()
                    .flat_map(|t| t.genres.iter().filter(|s| genre(s)))
                    .any(|s| g.genres.iter().any(|x| x.eq_ignore_ascii_case(s)))
            }
            Steer::Wish(_) | Steer::Unread => false,
        }
    }
}

/// A lineup from the ranking.
pub struct Ranked {
    pub picks: Vec<Pick>,
    pub spares: Vec<Pick>,
    /// What it was ranked by, only what counted in this roll.
    pub ranked_by: Vec<String>,
    /// Games it could choose from.
    pub eligible: usize,
}

/// The top of the ranking: `input.count` picks, then `input.spares` spares,
/// each one that `screen` lets in (one game per series, the second-chance
/// limit). `said` steers it away from games like the ones turned down;
/// `input.seed` varies the jitter, so lineups differ.
pub fn lineup(input: &PromptInput, said: &[Said], mut screen: Screen) -> Ranked {
    let steer: Vec<(Steer, &Said)> = said.iter().map(|s| (read(&s.reason), s)).collect();
    let mut request = vec![input.request.trim()];
    request.extend(steer.iter().filter_map(|(s, _)| if let Steer::Wish(w) = s { Some(w.as_str()) } else { None }));
    let ranking = Ranking::new(&request.join(". "), input.linux, input.seed)
        .with_routine(input.routine)
        .with_input(input.asked_input)
        .with_profile(input.profile, input.now)
        .with_lens(input.lens)
        .with_verdicts(input.verdicts);

    let mut scored: Vec<(&GameView, Scored)> = input
        .pool()
        .into_iter()
        .filter(|g| !input.off_limits(g))
        .map(|g| {
            let mut s = ranking.explain(g);
            if input.recent_ids.contains(&g.id) {
                s.add(Factor::Recent, -RECENT_PENALTY);
            }
            for (st, said) in &steer {
                if st.holds(g, said) {
                    s.add(Factor::TurnedDown, -TURNED_DOWN_PENALTY);
                }
            }
            (g, s)
        })
        .collect();
    scored.sort_by(|a, b| b.1.total.partial_cmp(&a.1.total).unwrap_or(std::cmp::Ordering::Equal));

    // A vague request's last pick is a change of pace, outside your lane,
    // unless an earlier pick already is one. Games passed over for that
    // slot become spares.
    // A discovery mode is already a change of pace.
    let pace = input.profile.filter(|p| input.lens.is_none() && p.wants_change_of_pace(input.request, input.count));
    // A discovery mode's games start their reason with what they are to it.
    let reason = |g: &GameView, s: &Scored| {
        let why = reason(g, s, &ranking, input.sampled_minutes);
        match input.lens.and_then(|l| l.member(g.id)) {
            Some(m) => format!("{} · {why}", m.reason),
            None => why,
        }
    };
    let mut paced = false;
    let (mut picks, mut spares) = (vec![], vec![]);
    for (g, s) in &scored {
        if picks.len() == input.count && spares.len() == input.spares {
            break;
        }
        if picks.len() == input.count {
            screen.picks_done();
        }
        if screen.clash(g).is_some() {
            // New to you: a way into something a pick already is makes a
            // fine spare.
            if picks.len() < input.count && spares.len() < input.spares {
                let mut spare = Screen::new(input.series, input.second_chances());
                for p in picks.iter().chain(&spares).filter_map(|p: &Pick| p.game.as_ref()) {
                    spare.add(p);
                }
                if spare.clash(g).is_none() {
                    screen.add_spare(g);
                    spares.push(pick_of(g, reason(g, s)));
                }
            }
            continue;
        }
        let outside = pace.is_some_and(|p| !p.in_your_lane(g));
        let pick_slot = picks.len() < input.count;
        if pick_slot && pace.is_some() && !paced && !outside && picks.len() + 1 == input.count {
            if spares.len() < input.spares {
                screen.add_spare(g);
                spares.push(pick_of(g, reason(g, s)));
            }
            continue;
        }
        screen.add(g);
        let mut why = reason(g, s);
        if pick_slot && outside && !paced {
            paced = true;
            if picks.len() + 1 == input.count {
                why = format!("A change of pace, outside your usual lane · {why}");
            }
        }
        if pick_slot { picks.push(pick_of(g, why)) } else { spares.push(pick_of(g, why)) }
    }
    // Nothing outside your lane, or too few games: the best spares fill
    // the lineup, each still a way into something different in New to you.
    while picks.len() < input.count {
        let Some(i) = spares.iter().position(|p| p.game.as_ref().is_none_or(|g| screen.ways_fit(g))) else { break };
        let p = spares.remove(i);
        if let Some(g) = p.game.as_ref() {
            screen.add(g);
        }
        picks.push(p);
    }
    let read: Vec<&str> = steer.iter().filter(|(s, _)| !matches!(s, Steer::Unread)).map(|(_, s)| s.reason.trim()).collect();
    let unread: Vec<&str> = steer.iter().filter(|(s, _)| matches!(s, Steer::Unread)).map(|(_, s)| s.reason.trim()).collect();
    let hard_nos = input.profile.is_some_and(|p| p.checks_hard_nos());
    let mut by = ranked_by(&scored, &ranking, &read, &unread, hard_nos);
    if let Some(l) = input.lens {
        by.insert(
            0,
            match l.mode {
                Mode::NewToYou => {
                    "games new to you only: a genre you rarely play or a mechanic you haven't tried, easy to get into and short, \
                     each pick a way into a different one"
                        .into()
                }
                Mode::Gems => "hidden gems only: unplayed, highly rated and a session or two long".into(),
                Mode::Random => "games off your radar only: not started, or last played over a year ago".into(),
            },
        );
    }
    if pace.is_some() && paced {
        let at = by.len().saturating_sub(1 + usize::from(!unread.is_empty()));
        by.insert(
            at,
            "one change of pace, outside your favourites, likes and the patterns you stick with, as the request was vague".into(),
        );
    }
    Ranked { picks, spares, ranked_by: by, eligible: scored.len() }
}

fn pick_of(g: &GameView, reason: String) -> Pick {
    Pick::of(g, reason)
}

/// "a, b and c".
fn and_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// A label as it reads mid-sentence: "Relaxed pace" -> "relaxed pace",
/// but "RPG" and "4X" stay.
fn plain(label: &str) -> String {
    label.split(' ').map(|w| if w.chars().any(char::is_lowercase) { w.to_lowercase() } else { w.to_string() }).collect::<Vec<_>>().join(" ")
}

/// "2 hours", "an hour", "45 minutes".
fn budget_text(hours: f64) -> String {
    // Rounded first, so 1.04 hours reads "an hour", not "1 hours".
    let tenths = (hours * 10.0).round() / 10.0;
    if hours < 1.0 {
        let minutes = (hours * 60.0).round();
        format!("{minutes} {}", if minutes == 1.0 { "minute" } else { "minutes" })
    } else if tenths == 1.0 {
        "an hour".into()
    } else {
        format!("{tenths} hours")
    }
}

/// The budget, and where it came from: "2 hours", "your usual weekend
/// session of 3 hours", or "2 hours, your routine "Friday with friends"".
fn budget_said(hours: f64, ranking: &Ranking) -> String {
    match ranking.budget_from() {
        BudgetFrom::Request => budget_text(hours),
        BudgetFrom::Routine => match ranking.routine() {
            Some(r) => format!("{}, your routine \"{}\"", budget_text(hours), r.name),
            None => budget_text(hours),
        },
        BudgetFrom::Habit(When::Weekend) => format!("your usual weekend session of {}", budget_text(hours)),
        BudgetFrom::Habit(When::Weeknight) => format!("your usual weeknight session of {}", budget_text(hours)),
    }
}

/// Why a game is in the lineup, from what counted for it: "94% of 12k
/// Steam reviews positive · 4h main story, about 2 sittings of 2 hours ·
/// relaxed pace, as asked". A game put down starts with why it stopped.
fn reason(g: &GameView, s: &Scored, ranking: &Ranking, sampled_minutes: i64) -> String {
    let mut parts: Vec<String> = vec![];
    if matches!(g.offer(), Some(Offer::Shelved | Offer::SecondChance))
        && let Some(r) = &g.stop_reasons
    {
        parts.push(format!("you stopped: {}", prompt::stopped_text(r)));
    }
    if let Some(line) = g.reviews_since_line() {
        parts.push(line.to_string());
    }
    if s.counted(Factor::SteamReviews)
        && let Some(p) = g.review_pct
    {
        parts.push(match g.review_total {
            Some(t) if t > 0 => format!("{p}% of {} Steam reviews positive", prompt::short_count(t)),
            _ => format!("{p}% positive on Steam"),
        });
    } else if s.counted(Factor::GogRating)
        && let Some(r) = g.gog_rating
    {
        parts.push(format!("rated {r:.1}/5 on GOG"));
    } else if s.counted(Factor::RawgRating)
        && let Some(r) = g.rawg_rating
    {
        parts.push(format!("rated {r:.1}/5 on RAWG"));
    }
    if let (Some(b), Some(h)) = (ranking.budget(), g.hltb_main)
        && s.counted(Factor::Budget)
    {
        let budget = budget_said(b, ranking);
        parts.push(if s.points(Factor::Budget) < 0.0 {
            format!("{h}h main story, long for {budget}")
        } else if h <= b {
            format!("{h}h main story fits in {budget}")
        } else {
            format!("{h}h main story, about {} sittings of {budget}", (h / b).ceil())
        });
    }
    // The routine is the session's frame, so it comes before the rest.
    if let Some(w) = ranking.routine().and_then(|r| r.who).filter(|_| s.points(Factor::Together) > 0.0) {
        parts.push(format!("co-op or multiplayer, for playing {}", w.words()));
    }
    if s.points(Factor::Controller) > 0.0 {
        parts.push(if s.points(Factor::Controller) >= 10.0 { "full controller support" } else { "plays on a controller" }.into());
    }
    if !s.matched.is_empty() {
        let labels: Vec<String> = s.matched.iter().map(|l| plain(l)).collect();
        parts.push(format!("{}, as asked", and_list(&labels)));
    }
    if !s.liked.is_empty() {
        let labels: Vec<String> = s.liked.iter().map(|l| plain(l)).collect();
        parts.push(format!("{}, which you like", and_list(&labels)));
    }
    if let Some(f) = s.like.as_ref().filter(|_| s.counted(Factor::Favourites)) {
        parts.push(format!("like {f}, a favourite"));
    }
    if let Some((name, stars)) = s.like_rated.as_ref().filter(|_| s.counted(Factor::RatedHigh)) {
        parts.push(format!("like {name}, which you rated {stars}/5"));
    }
    if let Some(stars) = g.my_rating.filter(|_| s.points(Factor::YourRating) > 0.0) {
        parts.push(format!("you rated it {stars}/5"));
    }
    if s.counted(Factor::ShortSessions) {
        parts.push("players say it suits short sessions".into());
    }
    let said: Vec<&str> = s.verdicts.iter().filter_map(|f| crate::verdicts::community::facet_reason(f)).collect();
    if s.counted(Factor::Verdicts) && !said.is_empty() {
        let said: Vec<String> = said.iter().map(|x| x.to_string()).collect();
        parts.push(format!("backlog players say {}", and_list(&said)));
    } else if s.counted(Factor::VerdictSessions) && !s.counted(Factor::ShortSessions) {
        parts.push("backlog players say it's fine in sessions under an hour".into());
    }
    if !s.mentioned.is_empty() {
        let words: Vec<String> = s.mentioned.iter().map(|w| format!("\"{w}\"")).collect();
        parts.push(format!("{} in its description or reviews", and_list(&words)));
    }
    if let Some(mc) = g.metacritic.filter(|mc| *mc >= 80) {
        parts.push(format!("Metacritic {mc}"));
    }
    if s.counted(Factor::Sampled) && g.playtime_minutes > 0 && g.playtime_minutes < sampled_minutes {
        parts.push(format!("you've tried it for {}", prompt::hours(g.playtime_minutes)));
    }
    if s.counted(Factor::Installed) {
        parts.push("installed".into());
    }
    parts.truncate(REASON_PARTS);
    let text = parts.join(" · ");
    let mut chars = text.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => "High on UpNext's ranking".into(),
    }
}

/// What the lineup was ranked by, in plain words: only what counted for
/// some game it could choose from, and any reason it couldn't read.
fn ranked_by(scored: &[(&GameView, Scored)], ranking: &Ranking, read: &[&str], unread: &[&str], hard_nos: bool) -> Vec<String> {
    let counted = |f: Factor| scored.iter().any(|(_, s)| s.counted(f));
    let mut out: Vec<String> = vec![];
    let rated: Vec<String> = [(Factor::GogRating, "GOG's"), (Factor::RawgRating, "RAWG's")]
        .iter()
        .filter(|(f, _)| counted(*f))
        .map(|(_, n)| n.to_string())
        .collect();
    let rated = (!rated.is_empty()).then(|| format!("{} rating", rated.join(" or ")));
    let mut reviews = match (counted(Factor::SteamReviews), rated) {
        (true, Some(r)) => format!("Steam review scores, else {r}"),
        (true, None) => "Steam review scores".to_string(),
        (false, Some(r)) => r,
        (false, None) => String::new(),
    };
    if counted(Factor::ReviewCount) {
        reviews = if reviews.is_empty() { "how many reviews it has".into() } else { format!("{reviews}, and how many reviews") };
    }
    if !reviews.is_empty() {
        out.push(reviews);
    }
    if counted(Factor::Metacritic) {
        out.push("Metacritic scores".into());
    }
    if let Some(b) = ranking.budget().filter(|_| counted(Factor::Budget)) {
        out.push(format!("HowLongToBeat's main-story times, for {}", budget_said(b, ranking)));
    }
    if counted(Factor::ShortSessions) {
        out.push("games players say suit short sessions, for a short budget".into());
    }
    if counted(Factor::VerdictSessions) {
        out.push("games other backlog players say are fine in short sessions, for a short budget".into());
    }
    if counted(Factor::Words) {
        out.push("your words against tags, genres and review tags, then names, blurbs and review summaries".into());
    }
    if counted(Factor::Verdicts) {
        out.push("your words against what most other backlog players say, a little, more as more of them answered".into());
    }
    if hard_nos {
        out.push("your hard nos, left out".into());
    }
    if counted(Factor::Liked) {
        out.push("what you like, on your profile".into());
    }
    if counted(Factor::Favourites) {
        out.push("likeness to your favourites".into());
    }
    if counted(Factor::YourRating) {
        out.push("your own ratings of games".into());
    }
    if counted(Factor::RatedHigh) {
        out.push("likeness to games you rated 4 or 5".into());
    }
    if counted(Factor::Unusual) {
        out.push("how unusual the AI rated each hidden gem".into());
    }
    if let Some(r) = ranking.routine() {
        let who = r.who.map(|w| w.words()).unwrap_or_default();
        if counted(Factor::Together) {
            out.push(format!("co-op and multiplayer games first, as your routine \"{}\" is {who}", r.name));
        }
        if counted(Factor::NeedsOthers) {
            out.push(format!("games that need other players kept lower, as your routine \"{}\" is on your own", r.name));
        }
        if counted(Factor::NotForKids) {
            out.push(format!("gory, scary and adult games kept lower, as your routine \"{}\" is with kids", r.name));
        }
        if counted(Factor::Controller) && !ranking.controller_asked() {
            out.push(format!("how well it plays on a controller, by Steam's store details, for your routine \"{}\"", r.name));
        }
    }
    if counted(Factor::Controller) && ranking.controller_asked() {
        out.push("how well it plays on a controller, by Steam's store details, as you asked on one".into());
    }
    if counted(Factor::Disliked) {
        out.push("what you dislike, kept a little lower".into());
    }
    if counted(Factor::Drift) {
        out.push("the kinds of game you drift away from, kept a little lower".into());
    }
    if counted(Factor::VerdictStopped) {
        out.push("games other backlog players stopped for what you drift away from, kept a little lower".into());
    }
    if counted(Factor::RatedLow) {
        out.push("likeness to games you rated 1 or 2, kept a little lower".into());
    }
    if counted(Factor::NotAFit) {
        out.push(format!("picks you said weren't a fit in the last {PICK_RATING_DAYS} days, kept a little lower"));
    }
    if !read.is_empty() {
        let said: Vec<String> = read.iter().map(|r| format!("\"{}\"", r.to_lowercase())).collect();
        out.push(format!("what you turned games down for: {}", and_list(&said)));
    }
    if counted(Factor::Installed) {
        out.push("installed games".into());
    }
    if counted(Factor::Sampled) {
        out.push("games you've sampled".into());
    }
    if counted(Factor::Familiar) {
        out.push("staples and endless games you've played, kept lower since you didn't ask for something familiar".into());
    }
    if counted(Factor::Recent) {
        out.push("games from your last few lineups, kept lower".into());
    }
    if counted(Factor::Linux) {
        out.push("how well it runs on Linux, native or by its ProtonDB tier".into());
    }
    out.push("a little chance, so each lineup differs".into());
    if !unread.is_empty() {
        let said: Vec<String> = unread.iter().map(|r| format!("\"{r}\"")).collect();
        out.push(format!("not {}: only the AI reads reasons in your own words", and_list(&said)));
    }
    out
}

#[cfg(test)]
mod tests;
