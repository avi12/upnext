//! The ranking that chooses a prompt's candidates, and picks without AI:
//! each factor a game's score is made of, the request's words and time
//! budget, the taste profile, a routine, a discovery mode and other
//! backlog players' verdicts.

use super::words::{FAMILIAR, RequestWord, STOP_WORDS, asks_for, keyword_matches, request_words, starts_a_word, time_budget_hours};
use crate::db::{GameView, PICK_RATING_DAYS, PickVerdict, Status};
use crate::lifecycle::Offer;

/// Linux compatibility adjustment: borked games effectively drop out.
fn linux_score(g: &GameView) -> f64 {
    match g.linux.as_deref() {
        Some("native") | Some("platinum") => 4.0,
        Some("gold") => 2.0,
        Some("silver") => -4.0,
        Some("bronze") => -10.0,
        Some("borked") => -40.0,
        _ => 0.0,
    }
}

/// Short sessions suit a time budget of this many hours or less.
const SHORT_SESSION_BUDGET: f64 = 2.0;

/// A played endless game or a staple, for a request that doesn't ask for
/// something familiar: these suit a few requests, not every list.
const SESSION_PENALTY: f64 = 12.0;

/// What the ranking weighs. A lineup picked without AI lists the ones that
/// counted in its roll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Factor {
    /// The Steam review score; GOG's rating, else RAWG's, stands in without one.
    SteamReviews,
    GogRating,
    RawgRating,
    ReviewCount,
    Metacritic,
    Installed,
    Sampled,
    /// The time budget against HowLongToBeat's main story.
    Budget,
    /// Short sessions for a short budget, from the review tags.
    ShortSessions,
    /// The same, from other backlog players' verdicts.
    VerdictSessions,
    /// What the request asks that other backlog players' verdicts say it
    /// has: it clicks fast, it respects limited time, it's fine to stop
    /// after the credits.
    Verdicts,
    /// Many players who stopped it stopped for what you drift away from.
    VerdictStopped,
    /// The request's words against tags, review tags and text.
    Words,
    /// Played endless games and staples held back unless asked for.
    Familiar,
    Linux,
    /// Picks without AI: games from the last few lineups held back.
    Recent,
    /// Picks without AI: games like the ones turned down held back.
    TurnedDown,
    /// Likes on your profile.
    Liked,
    /// Dislikes on your profile, softly.
    Disliked,
    /// Patterns of games you drift away from, softly.
    Drift,
    /// Likeness to a favourite.
    Favourites,
    /// Your own rating of the game.
    YourRating,
    /// Likeness to a game you rated 4 or 5, when it isn't like a favourite.
    RatedHigh,
    /// Likeness to a game you rated 1 or 2, softly.
    RatedLow,
    /// Rated "Not a fit" as a pick lately (`PICK_RATING_DAYS`), softly.
    NotAFit,
    /// Hidden gems: how unusual the AI rated it.
    Unusual,
    /// A routine with other people: games you can play together up,
    /// single-player games down.
    Together,
    /// A routine on your own: games that need other players, down.
    NeedsOthers,
    /// A routine on a controller: how well a game plays on one.
    Controller,
    /// A routine with kids: games whose tags say they aren't for children, down.
    NotForKids,
    /// The per-call jitter that varies ties.
    Chance,
}

/// One game's score, with what made it up.
#[derive(Debug, Clone, Default)]
pub struct Scored {
    pub total: f64,
    /// Each factor that moved the score, with its points.
    pub parts: Vec<(Factor, f64)>,
    /// Review-tag labels and tags the request's words matched: "Relaxed pace".
    pub matched: Vec<String>,
    /// Request words found only in its name, blurb or review summary.
    pub mentioned: Vec<String>,
    /// Likes on your profile it matched: "Story-driven".
    pub liked: Vec<String>,
    /// The favourite it's most like, when that counted.
    pub like: Option<String>,
    /// The game you rated highly it's most like, with its stars, when that counted.
    pub like_rated: Option<(String, u8)>,
    /// What the request asked that other backlog players say it has
    /// (`verdicts::community::facet_reason`), when that counted.
    pub verdicts: Vec<String>,
}

impl Scored {
    pub fn add(&mut self, factor: Factor, points: f64) {
        if points != 0.0 {
            self.total += points;
            self.parts.push((factor, points));
        }
    }

    pub fn points(&self, factor: Factor) -> f64 {
        self.parts.iter().filter(|(f, _)| *f == factor).map(|(_, p)| p).sum()
    }

    pub fn counted(&self, factor: Factor) -> bool {
        self.parts.iter().any(|(f, _)| *f == factor)
    }
}

/// Points for each like a game matches, as a request's tag match.
const LIKE_POINTS: f64 = 12.0;
/// Points for each thing the request asks that other backlog players say a
/// game has, at full weight (from 20 verdicts; `Backlog::weight`): half a
/// like, so a person's own taste always counts for more. At most two count.
const VERDICT_POINTS: f64 = 6.0;
const MAX_VERDICT_FACETS: usize = 2;
/// Points for players saying it's fine in short sessions, for a short
/// budget: less than the review tags' 10, which come from far more people.
const VERDICT_SESSION_POINTS: f64 = 6.0;
/// Likes counted per game at most, so a game can't ride on likes alone.
const MAX_LIKES: usize = 2;
/// Points off for each dislike a game matches, and for each pattern of
/// games you drift away from: softly, as the plan asks.
const DISLIKE_POINTS: f64 = 8.0;
const DRIFT_POINTS: f64 = 5.0;
/// Dislikes and drift patterns counted per game at most, each.
const MAX_AGAINST: usize = 2;
/// Points for each thing a game shares with the favourite it's most like,
/// from 2 shared things, up to 3 of them: a little.
const FAVOURITE_POINTS: f64 = 2.0;
const FAVOURITE_MIN_SHARED: usize = 2;
const FAVOURITE_MAX_SHARED: usize = 3;
/// Points for each star a game you rated is above 3, or off for each it
/// is below: 10 up at 5 stars, 10 down at 1.
const STAR_POINTS: f64 = 5.0;
/// Points off for each thing a game shares with the game you rated 1 or 2
/// it's most like, counted as likeness to a favourite is.
const RATED_LOW_POINTS: f64 = 2.0;
/// Points off for a game rated "Not a fit" as a pick lately.
const NOT_A_FIT_POINTS: f64 = 8.0;

/// Points for a game you can play together on a routine with others, and
/// off one that is single-player only: a lot with friends, who can't share
/// one, and less with a partner or kids, who might watch a story unfold.
/// Off a game that needs other players on a routine on your own.
const TOGETHER_POINTS: f64 = 15.0;
const ALONE_ONLY_POINTS: f64 = 25.0;
const ALONE_ONLY_SHARED_POINTS: f64 = 10.0;
const NEEDS_OTHERS_POINTS: f64 = 15.0;
/// Points on a routine with a controller: full support up, partial a
/// little, none (keyboard and mouse only, by Steam's store details) down.
const PAD_FULL_POINTS: f64 = 10.0;
const PAD_PARTIAL_POINTS: f64 = 4.0;
const PAD_NONE_POINTS: f64 = 10.0;
/// Points off a game that isn't for children on a routine with kids.
const NOT_FOR_KIDS_POINTS: f64 = 20.0;

/// Where the time budget came from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BudgetFrom {
    Request,
    /// The chosen routine's length.
    Routine,
    /// Your usual session for now, from the profile's habits.
    Habit(crate::taste::When),
}

/// What a request asks of the ranking: its words, its time budget, and
/// whether it wants something familiar, with the seed for the jitter, and
/// the taste profile when picks read it.
pub struct Ranking<'a> {
    words: Vec<RequestWord>,
    budget: Option<f64>,
    budget_from: BudgetFrom,
    familiar: bool,
    linux: bool,
    seed: u64,
    profile: Option<&'a crate::taste::Profile>,
    lens: Option<&'a crate::discovery::Lens>,
    routine: Option<&'a crate::taste::routine::Routine>,
    /// What the request says it's played on.
    asked_input: Option<crate::taste::routine::Input>,
    /// Weigh other backlog players' verdicts: the same switch as
    /// the prompt's `bv` lines, so with it off they change nothing the AI
    /// is given, not even which games make the list.
    verdicts: bool,
    /// For how long ago a pick was rated.
    now: i64,
}

impl<'a> Ranking<'a> {
    pub fn new(request: &str, linux: bool, seed: u64) -> Self {
        Ranking {
            words: request_words(request),
            budget: time_budget_hours(request),
            budget_from: BudgetFrom::Request,
            familiar: asks_for(request, FAMILIAR),
            linux,
            seed,
            profile: None,
            lens: None,
            routine: None,
            asked_input: None,
            verdicts: false,
            now: crate::db::now(),
        }
    }

    /// Weigh other backlog players' verdicts too (`verdicts_prompt`).
    pub fn with_verdicts(mut self, on: bool) -> Self {
        self.verdicts = on;
        self
    }

    /// Read the chosen routine too. With no time in the request, its
    /// length is the budget, ahead of the usual session from your habits,
    /// so call this before `with_profile`.
    pub fn with_routine(mut self, routine: Option<&'a crate::taste::routine::Routine>) -> Self {
        self.routine = routine;
        if self.budget.is_none()
            && let Some(m) = routine.and_then(|r| r.minutes)
        {
            self.budget = Some(m as f64 / 60.0);
            self.budget_from = BudgetFrom::Routine;
        }
        self
    }

    /// The chosen routine, if any.
    pub fn routine(&self) -> Option<&'a crate::taste::routine::Routine> {
        self.routine
    }

    /// Read what the request says it's played on too: big screen
    /// asks on a controller.
    pub fn with_input(mut self, input: Option<crate::taste::routine::Input>) -> Self {
        self.asked_input = input;
        self
    }

    /// What this session is played on: the routine's own input when it names
    /// one (a Deck counts as a controller), else what the request says.
    pub fn input_now(&self) -> Option<crate::taste::routine::Input> {
        self.routine.and_then(|r| r.input_now()).or(self.asked_input)
    }

    /// Whether the controller came from the request rather than a routine,
    /// so the reasons and the prompt say "as you asked on one".
    pub fn controller_asked(&self) -> bool {
        use crate::taste::routine::Input;
        self.input_now() == Some(Input::Controller) && self.routine.and_then(|r| r.input_now()).is_none()
    }

    /// Read a discovery mode's points too: how unusual a hidden gem is.
    pub fn with_lens(mut self, lens: Option<&'a crate::discovery::Lens>) -> Self {
        self.lens = lens;
        self
    }

    /// Read the profile too. With no time in the request, the usual session
    /// for `now` stands in for one.
    pub fn with_profile(mut self, profile: Option<&'a crate::taste::Profile>, now: i64) -> Self {
        self.profile = profile;
        self.now = now;
        if self.budget.is_none()
            && let Some(u) = profile.and_then(|p| p.usual(now))
        {
            self.budget = Some(u.minutes as f64 / 60.0);
            self.budget_from = BudgetFrom::Habit(u.when);
        }
        self
    }

    /// The time budget, in hours: the request's, or your usual session.
    pub fn budget(&self) -> Option<f64> {
        self.budget
    }

    pub fn budget_from(&self) -> BudgetFrom {
        self.budget_from
    }

    pub fn score(&self, g: &GameView) -> f64 {
        self.explain(g).total
    }

    /// Whether the request asks for what a subject is about: then a dislike
    /// or a drift pattern about it doesn't count against a game.
    fn asks_about(&self, subject: &crate::taste::Subject) -> bool {
        use crate::taste::Subject;
        let label = subject.label().to_lowercase();
        self.words.iter().filter(|w| !w.negated).any(|w| {
            let facet = matches!(subject, Subject::Facet(f) if w.facets.contains(&f.as_str()));
            let stem: String = w.text.chars().take(5).collect();
            facet || (w.text.len() >= 4 && !STOP_WORDS.contains(&w.text.as_str()) && starts_a_word(&label, &stem))
        })
    }

    pub fn explain(&self, g: &GameView) -> Scored {
        let mut s = Scored::default();
        // The base: Steam's review score, else GOG's or RAWG's rating out of 5.
        if let Some(p) = g.review_pct {
            s.add(Factor::SteamReviews, p as f64);
        } else if let Some(r) = g.gog_rating {
            s.add(Factor::GogRating, r * 20.0);
        } else if let Some(r) = g.rawg_rating {
            s.add(Factor::RawgRating, r * 20.0);
        } else {
            s.total = 68.0;
        }
        if self.linux {
            s.add(Factor::Linux, linux_score(g));
        }
        s.add(Factor::ReviewCount, 6.0 * ((g.review_total.unwrap_or(0) as f64) + 1.0).log10());
        if let Some(mc) = g.metacritic {
            s.add(Factor::Metacritic, (mc as f64 - 75.0) / 4.0);
        }
        if g.installed {
            s.add(Factor::Installed, 6.0);
        }
        if g.status == Status::Sampled {
            s.add(Factor::Sampled, 3.0);
        }
        if let (Some(b), Some(h)) = (self.budget, g.hltb_main) {
            if h <= b * 2.5 {
                s.add(Factor::Budget, 10.0);
            } else if h > b * 8.0 {
                s.add(Factor::Budget, -10.0);
            }
        }
        // Players say it works in short sittings, however long it is overall.
        if self.budget.is_some_and(|b| b <= SHORT_SESSION_BUDGET) && g.review_tags.as_ref().is_some_and(|t| t.has("sessions:short")) {
            s.add(Factor::ShortSessions, 10.0);
        }
        let m = keyword_matches(&self.words, g);
        s.add(Factor::Words, m.score);
        (s.matched, s.mentioned) = (m.labels, m.mentioned);
        if self.verdicts
            && let Some(b) = &g.backlog
        {
            self.verdict_points(b, &mut s);
        }
        if g.offer() == Some(Offer::Session) && !self.familiar {
            s.add(Factor::Familiar, -SESSION_PENALTY);
        }
        if let Some(p) = self.profile {
            self.profile_points(p, g, &mut s);
        }
        if g.pick_rating.as_ref().is_some_and(|r| r.verdict == PickVerdict::NotAFit && self.now - r.at < PICK_RATING_DAYS * 86_400) {
            s.add(Factor::NotAFit, -NOT_A_FIT_POINTS);
        }
        if let Some(m) = self.lens.and_then(|l| l.member(g.id)) {
            s.add(Factor::Unusual, m.points);
        }
        if let Some(r) = self.routine {
            routine_points(r, g, &mut s);
        }
        if self.input_now() == Some(crate::taste::routine::Input::Controller) {
            pad_points(g, &mut s);
        }
        // Deterministic per-call jitter in [0, 10).
        let mix = (g.id as u64).wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(self.seed).rotate_left(17);
        s.add(Factor::Chance, (mix % 1000) as f64 / 100.0);
        s
    }

    /// Other backlog players' verdicts: evidence about the game,
    /// weighed below the person's own taste and growing with how many
    /// players answered. The request's words against what most of them
    /// say, and short sessions for a short budget, as the review tags'.
    fn verdict_points(&self, b: &crate::verdicts::community::Backlog, s: &mut Scored) {
        let w = b.weight();
        let mut asked: Vec<&str> = vec![];
        for f in self.words.iter().filter(|w| !w.negated).flat_map(|w| w.verdict_facets.iter()) {
            if b.has(f) && !asked.contains(f) {
                asked.push(f);
            }
        }
        asked.truncate(MAX_VERDICT_FACETS);
        s.add(Factor::Verdicts, VERDICT_POINTS * w * asked.len() as f64);
        s.verdicts = asked.iter().map(|f| f.to_string()).collect();
        if self.budget.is_some_and(|b| b <= SHORT_SESSION_BUDGET) && b.has("sessions:short") {
            s.add(Factor::VerdictSessions, VERDICT_SESSION_POINTS * w);
        }
    }

    /// Likes up, dislikes and drift patterns down unless the request asks
    /// for them, and a little for likeness to a favourite.
    fn profile_points(&self, p: &crate::taste::Profile, g: &GameView, s: &mut Scored) {
        use crate::taste::{Sense, about::matches_in, features};
        let f = features(g);
        let liked: Vec<String> =
            p.likes.iter().filter(|l| matches_in(&l.subject, g, &f)).take(MAX_LIKES).map(|l| l.subject.label()).collect();
        s.add(Factor::Liked, LIKE_POINTS * liked.len() as f64);
        s.liked = liked;
        let disliked = p.dislikes.iter().filter(|d| matches_in(&d.subject, g, &f) && !self.asks_about(&d.subject)).count();
        s.add(Factor::Disliked, -DISLIKE_POINTS * disliked.min(MAX_AGAINST) as f64);
        let drift =
            p.patterns.iter().filter(|n| n.sense == Sense::Drift && matches_in(&n.subject, g, &f) && !self.asks_about(&n.subject)).count();
        s.add(Factor::Drift, -DRIFT_POINTS * drift.min(MAX_AGAINST) as f64);
        // A pattern of drifting away from something, and many of the
        // backlog players who stopped this game stopped for it:
        // softly against it, once, as a drift pattern counts.
        if self.verdicts
            && let Some(b) = &g.backlog
        {
            let stopped_for = b.stopped_mostly_for();
            let against = p.patterns.iter().filter(|n| n.sense == Sense::Drift && !self.asks_about(&n.subject)).any(|n| {
                n.subject.aspect().is_some_and(|a| {
                    crate::verdicts::vocab::stop_reasons().any(|r| r.aspects.contains(&a) && stopped_for.contains(&r.code))
                })
            });
            if against {
                s.add(Factor::VerdictStopped, -DRIFT_POINTS * b.weight());
            }
        }
        if let Some((fav, shared)) = p.likeness(g, &f).filter(|(_, n)| *n >= FAVOURITE_MIN_SHARED) {
            s.add(Factor::Favourites, FAVOURITE_POINTS * shared.min(FAVOURITE_MAX_SHARED) as f64);
            s.like = Some(fav.name.clone());
        }
        // Your ratings: the game's own, then games you rated
        // highly as favourites (unless a favourite already counted), and
        // games you rated low softly against.
        if let Some(stars) = g.my_rating {
            s.add(Factor::YourRating, STAR_POINTS * (stars as f64 - 3.0));
        }
        if !s.counted(Factor::Favourites)
            && let Some((r, shared)) = p.rated_likeness(g, &f).filter(|(_, n)| *n >= FAVOURITE_MIN_SHARED)
        {
            s.add(Factor::RatedHigh, FAVOURITE_POINTS * shared.min(FAVOURITE_MAX_SHARED) as f64);
            s.like_rated = Some((r.name.clone(), r.stars));
        }
        if let Some((_, shared)) = p.rated_unlikeness(g, &f).filter(|(_, n)| *n >= FAVOURITE_MIN_SHARED) {
            s.add(Factor::RatedLow, -RATED_LOW_POINTS * shared.min(FAVOURITE_MAX_SHARED) as f64);
        }
    }
}

/// What a routine asks of a game: playing together or alone, a
/// controller, and nothing for adults with kids. Each reads only what the
/// game's tags, store categories and IGDB modes say, and leaves a game
/// that says nothing alone.
fn routine_points(r: &crate::taste::routine::Routine, g: &GameView, s: &mut Scored) {
    use crate::taste::routine::{Marks, Who};
    let m = Marks::of(g);
    match r.who {
        Some(w) if w.together() => match m.plays_together() {
            Some(true) => s.add(Factor::Together, TOGETHER_POINTS),
            Some(false) if w == Who::Friends => s.add(Factor::Together, -ALONE_ONLY_POINTS),
            Some(false) => s.add(Factor::Together, -ALONE_ONLY_SHARED_POINTS),
            None => {}
        },
        Some(_) if m.needs_others() => s.add(Factor::NeedsOthers, -NEEDS_OTHERS_POINTS),
        _ => {}
    }
    if r.who == Some(Who::Kids) && m.adult() {
        s.add(Factor::NotForKids, -NOT_FOR_KIDS_POINTS);
    }
}

/// How well a game plays on a controller, for a session on one: a routine
/// on a controller or a Deck, or a request from big screen.
fn pad_points(g: &GameView, s: &mut Scored) {
    use crate::taste::routine::{Marks, Pad};
    match Marks::of(g).pad() {
        Some(Pad::Full) => s.add(Factor::Controller, PAD_FULL_POINTS),
        Some(Pad::Partial) => s.add(Factor::Controller, PAD_PARTIAL_POINTS),
        Some(Pad::None) => s.add(Factor::Controller, -PAD_NONE_POINTS),
        None => {}
    }
}
