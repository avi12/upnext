//! Builds the recommendation prompt: compact candidate lines, About me from
//! the taste profile (or, without it, the most-played games), the user's
//! request, and the JSON schema. The modules beside it read the request's
//! words (`words`), rank the games that become candidates (`scoring`, which
//! picks without AI use too) and write a game's line (`lines`).

pub(crate) mod lines;
mod scoring;
mod words;

pub use lines::{BV_FORMAT, BV_RULE, candidate_line};
pub(crate) use lines::{ago, hours, short_count, stopped_text};
pub use scoring::{BudgetFrom, Factor, Ranking, Scored};
pub use words::{asks_for_second_chances, rules_out, verdict_wishes};

use crate::db::{GameView, PICK_RATING_DAYS, PickVerdict};
use crate::discovery::Mode;
use lines::{label_list, wish_line};
use serde_json::{Value, json};
use std::collections::HashMap;
use words::{STOP_WORDS, request_words, starts_a_word};

pub struct BuiltPrompt {
    pub system: String,
    /// Taste and candidate list: identical across rerolls of the same request
    /// on the same day, so backends can cache it.
    pub prefix: String,
    /// The request, what to avoid, and the task (changes every call).
    pub user: String,
    pub schema: Value,
    /// Candidates actually sent, by internal id.
    pub candidates: HashMap<i64, GameView>,
    pub considered: usize,
    pub eligible: usize,
    /// How many second chances the picks and spares may hold together.
    pub second_chances: usize,
    /// Wishlisted games listed, by wishlist id: one may stand in for a pick.
    pub wishlisted: HashMap<i64, crate::db::WishGame>,
    /// The AI may suggest a game you don't own.
    pub suggests: bool,
}

pub struct PromptInput<'a> {
    pub games: &'a [GameView],
    pub request: &'a str,
    pub count: usize,
    /// Spares to rank after the picks: they replace picks turned down.
    pub spares: usize,
    pub exclude_ids: &'a [i64],
    /// Reasons the excluded games were turned down.
    pub avoid: &'a [String],
    /// Games still in the lineup when asking for more spares: never picked
    /// again, nor another game of their series.
    pub on_screen: &'a [i64],
    /// Each title's series, as Collections groups them (`series::of_titles`).
    pub series: &'a HashMap<i64, Vec<String>>,
    /// Unix seconds, for how long ago a game was put down.
    pub now: i64,
    pub sources: Option<&'a [String]>,
    pub recent_ids: &'a [i64],
    pub max_candidates: usize,
    pub sampled_minutes: i64,
    /// The person plays on Linux: show and weigh native/ProtonDB compatibility.
    pub linux: bool,
    /// Varies the tie-breaking between calls so the same 200 games are not
    /// always the only ones the model sees.
    pub seed: u64,
    /// List each candidate's feel and aspects from the review tags.
    pub review_feel: bool,
    /// Other backlog players' verdicts in what the AI reads
    /// (`verdicts_prompt`): their points in the ranking that chooses the
    /// candidates, and each candidate's `bv` line with the rule that they
    /// count below About me. Off, the AI's input is as if none were kept.
    pub verdicts: bool,
    /// The taste profile: About me in place of the most-played
    /// list, hard nos out of the pool, and the profile in the ranking. None
    /// leaves it all out, as before the profile, for comparing picks.
    pub profile: Option<&'a crate::taste::Profile>,
    /// A discovery mode: only its games are candidates, each
    /// line says what it is to the mode, and the AI gets the mode's rule.
    /// `request` is then the request without the words that asked for it.
    pub lens: Option<&'a crate::discovery::Lens>,
    /// The wishlist in picks, as `wishlist::for_picks`
    /// gives it: wishlisted games you don't own, any of which may stand in
    /// for one pick, and with it the rule that the AI may suggest one game
    /// you don't own. None leaves both out: the wishlist turned off, a
    /// discovery mode, new spares.
    pub wishlist: Option<&'a [crate::db::WishGame]>,
    /// The routine chosen for this session: who with, where, on what
    /// and for how long. The prompt gets "This session" and the ranking
    /// reads it; its length is the time budget when the request gives none.
    pub routine: Option<&'a crate::taste::routine::Routine>,
    /// What the request says it's played on: big screen sends
    /// `controller`. A routine that names its own input outweighs it.
    pub asked_input: Option<crate::taste::routine::Input>,
}

/// Wishlisted games listed for the AI at most, best fits first.
pub const WISHLIST_LISTED: usize = 8;

/// The schema with the wishlist's parts: `suggestion` whenever the AI may
/// suggest a game you don't own, and `wishlisted` when the wishlist lists
/// any games. Both are optional: most lineups have neither.
pub fn schema_with_wishlist(count: usize, spares: usize, suggests: bool, listed: bool) -> Value {
    let mut s = schema(count, spares);
    let props = s["properties"].as_object_mut().expect("the schema has properties");
    if listed {
        props.insert(
            "wishlisted".into(),
            json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "Wishlist id exactly as listed, like w12"},
                    "name": {"type": "string"},
                    "reason": {"type": "string"},
                    "instead_of": {"type": "integer", "description": "The #id of the pick it stands in for"}
                },
                "required": ["id", "name", "reason", "instead_of"],
                "additionalProperties": false
            }),
        );
    }
    if suggests {
        props.insert(
            "suggestion".into(),
            json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "The game's name as Steam's store has it"},
                    "reason": {"type": "string"}
                },
                "required": ["name", "reason"],
                "additionalProperties": false
            }),
        );
    }
    s
}

pub fn schema(count: usize, spares: usize) -> Value {
    let item = json!({
        "type": "object",
        "properties": {
            "id": {"type": "integer", "description": "Candidate id exactly as listed"},
            "name": {"type": "string"},
            "reason": {"type": "string"}
        },
        "required": ["id", "name", "reason"],
        "additionalProperties": false
    });
    json!({
        "type": "object",
        "properties": {
            "picks": {"type": "array", "minItems": count, "maxItems": count, "items": item},
            "spares": {"type": "array", "minItems": spares, "maxItems": spares, "items": item}
        },
        "required": ["picks", "spares"],
        "additionalProperties": false
    })
}

/// Same request, sources and day -> same candidate list, so a reroll can reuse
/// the cached prefix. Variety between rerolls comes from the exclusions.
fn stable_seed(request: &str, sources: Option<&[String]>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    request.trim().to_lowercase().hash(&mut h);
    let mut src: Vec<&String> = sources.map(|s| s.iter().collect()).unwrap_or_default();
    src.sort();
    src.hash(&mut h);
    chrono::Local::now().date_naive().hash(&mut h);
    h.finish()
}

impl PromptInput<'_> {
    /// The games picks may offer from the chosen stores, off-limits ones
    /// included, and none of your hard nos. Any store you own it on counts, as in the library filter.
    pub fn pool(&self) -> Vec<&GameView> {
        let sources_ok = |g: &GameView| self.sources.is_none_or(|s| s.iter().any(|x| g.owned_on.contains(x)));
        // A hard no is never picked.
        let hard_no = |g: &GameView| self.profile.is_some_and(|p| p.hard_no(g).is_some());
        let in_mode = |g: &GameView| self.lens.is_none_or(|l| l.allows(g.id));
        self.games.iter().filter(|g| g.offer().is_some()).filter(|g| sources_ok(g) && !hard_no(g) && in_mode(g)).collect()
    }

    /// Turned down, or still in the lineup: never picked again.
    pub fn off_limits(&self, g: &GameView) -> bool {
        self.exclude_ids.contains(&g.id) || self.on_screen.contains(&g.id)
    }

    /// How many second chances the picks and spares may hold together.
    pub fn second_chances(&self) -> usize {
        if asks_for_second_chances(self.request) { self.count + self.spares } else { 1 }
    }
}

/// Pick ratings listed under "How recent picks landed" at most.
const LANDED_LISTED: usize = 8;

/// Rated picks from the last `PICK_RATING_DAYS`, newest first, as lines:
/// `- Hades, for "something short": a good pick`. A request with no
/// words is "no particular request". Lines go in the per-call part: a
/// rating given during the day leaves the cached candidate list alone.
fn recent_ratings(input: &PromptInput) -> Vec<String> {
    let mut rated: Vec<(&GameView, &crate::db::RatedPick)> = input
        .games
        .iter()
        .filter_map(|g| g.pick_rating.as_ref().map(|r| (g, r)))
        .filter(|(_, r)| input.now - r.at < PICK_RATING_DAYS * 86_400)
        .collect();
    rated.sort_by_key(|(g, r)| (std::cmp::Reverse(r.at), g.id));
    rated
        .into_iter()
        .take(LANDED_LISTED)
        .map(|(g, r)| {
            let asked: String = r.prompt.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(120).collect();
            let asked = if asked.is_empty() { "no particular request".to_string() } else { format!("\"{asked}\"") };
            let verdict = match (r.verdict, r.reason) {
                (PickVerdict::Good, _) => "a good pick".to_string(),
                (PickVerdict::NotAFit, Some(why)) => format!("not a fit ({})", why.words()),
                (PickVerdict::NotAFit, None) => "not a fit".to_string(),
            };
            format!("- {}, for {asked}: {verdict}", crate::llm::name_line(&g.name))
        })
        .collect()
}

/// The taste signal before the profile, and without it: the most-played
/// games (any status), with genres and tags.
fn most_played(games: &[GameView]) -> String {
    let mut played: Vec<&GameView> = games.iter().filter(|g| g.playtime_minutes >= 60).collect();
    played.sort_by_key(|g| std::cmp::Reverse(g.playtime_minutes));
    let taste: Vec<String> = played
        .iter()
        .take(15)
        .map(|g| {
            let mut tags: Vec<String> = g.genres.iter().take(2).cloned().collect();
            for t in g.tags.iter().take(3) {
                if !tags.iter().any(|x| x.eq_ignore_ascii_case(t)) {
                    tags.push(t.clone());
                }
            }
            let name = crate::llm::name_line(&g.name);
            if tags.is_empty() {
                format!("- {name} ({})", hours(g.playtime_minutes))
            } else {
                format!("- {name} ({}; {})", hours(g.playtime_minutes), label_list(&tags))
            }
        })
        .collect();
    let mut out = String::from("## My taste (most-played games, as a signal only)\n");
    if taste.is_empty() {
        out.push_str("(no playtime data yet)\n");
    } else {
        out.push_str(&taste.join("\n"));
        out.push('\n');
    }
    out
}

/// The wishlisted games to list for a request, best fits first: none
/// turned down or on screen, none a hard no, ranked by the request's words
/// against their tags, genres and blurb, then by their reviews.
fn wishlist_listed<'a>(input: &PromptInput<'a>) -> Vec<&'a crate::db::WishGame> {
    let Some(list) = input.wishlist else { return vec![] };
    let words = request_words(input.request);
    let fit = |w: &crate::db::WishGame| -> f64 {
        let hay: Vec<String> = w.tags.iter().chain(&w.genres).map(|t| t.to_lowercase()).collect();
        let text = format!("{} {}", w.name.to_lowercase(), w.short_description.as_deref().unwrap_or_default().to_lowercase());
        let matched = words
            .iter()
            .filter(|x| !x.negated && x.text.len() >= 4 && !STOP_WORDS.contains(&x.text.as_str()))
            .filter(|x| {
                let stem: String = x.text.chars().take(5).collect();
                hay.iter().any(|h| starts_a_word(h, &stem)) || starts_a_word(&text, &stem)
            })
            .count();
        matched as f64 * 12.0 + w.review_pct.unwrap_or(70) as f64 / 10.0
    };
    let mut out: Vec<(&crate::db::WishGame, f64)> = list
        .iter()
        .filter(|w| !input.exclude_ids.contains(&-w.id) && !input.on_screen.contains(&-w.id))
        .filter(|w| input.profile.is_none_or(|p| p.hard_no_in_store(&w.tags, &w.genres).is_none()))
        .map(|w| (w, fit(w)))
        .collect();
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.id.cmp(&b.0.id)));
    out.into_iter().take(WISHLIST_LISTED).map(|(w, _)| w).collect()
}

pub fn build(input: &PromptInput) -> BuiltPrompt {
    let off_limits = |g: &GameView| input.off_limits(g);
    // Turned-down games stay in the list (so the list, and the cache, don't
    // change on a reroll) but are named as off limits and rejected if picked.
    let pool = input.pool();
    let eligible = pool.iter().filter(|g| !off_limits(g)).count();

    let seed = if input.seed == 0 { stable_seed(input.request, input.sources) } else { input.seed };
    let ranking = Ranking::new(input.request, input.linux, seed)
        .with_routine(input.routine)
        .with_input(input.asked_input)
        .with_profile(input.profile, input.now)
        .with_lens(input.lens)
        .with_verdicts(input.verdicts);
    let mut ranked: Vec<(&GameView, f64)> = pool.iter().map(|g| (*g, ranking.score(g))).collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let chosen: Vec<&GameView> = ranked.iter().take(input.max_candidates).map(|(g, _)| *g).collect();
    let considered = chosen.iter().filter(|g| !off_limits(g)).count();

    // A series is named on a line only when another candidate shares it.
    let mut shared: HashMap<&str, usize> = HashMap::new();
    for g in &chosen {
        for s in input.series.get(&g.id).into_iter().flatten() {
            *shared.entry(s.as_str()).or_default() += 1;
        }
    }
    let series_of = |g: &GameView| input.series.get(&g.id).into_iter().flatten().find(|s| shared.get(s.as_str()).is_some_and(|n| *n > 1));

    // ---- cacheable prefix: About me (or the most-played games) + candidates
    let mut prefix = match input.profile {
        Some(p) => p.about_me(input.games, input.now),
        None => most_played(input.games),
    };
    let mode = input.lens.map(|l| l.mode);
    prefix.push_str(&format!(
        "\n## Candidates ({} of {} {})\n\
         Format: #id name [stores, if not only Steam] | genres & tags | series: shared with another candidate | \
         main-story hours (?h = unknown) | played so far | my history with it | \
         review % positive/count (or rating out of 5) | year | {}installed | {}{}says: what players say{}\n\n",
        chosen.len(),
        pool.len(),
        match mode {
            Some(Mode::NewToYou) => "owned games in a genre I rarely play or with a mechanic I haven't tried, easy to get into and short",
            Some(Mode::Gems) => "hidden gems: owned games I haven't played, rated very highly by players, a session or two long",
            Some(Mode::Random) => "owned games off my radar: not started, or last played over a year ago",
            None => "owned games I haven't finished, or play now and then",
        },
        if input.linux { "linux (native or ProtonDB tier) | " } else { "" },
        if input.review_feel {
            "feel: what players say it's like | aspects players praised (+), criticized (-) or disagree on (±) | "
        } else {
            ""
        },
        if input.verdicts {
            format!("{BV_FORMAT} | ")
        } else {
            String::new()
        },
        match mode {
            Some(Mode::NewToYou) => " | new to you: the genres or mechanics it's a way into",
            Some(Mode::Gems) => " | unusual: what makes it stand out, with how little-known its studio is, how rare its mechanics are and how odd its play is, out of 3",
            Some(Mode::Random) => " | off your radar: why it is",
            None => "",
        }
    ));
    for g in &chosen {
        prefix.push_str(&candidate_line(
            g,
            input.sampled_minutes,
            input.linux,
            input.review_feel,
            input.verdicts,
            series_of(g).map(String::as_str),
            input.now,
        ));
        if let Some(m) = input.lens.and_then(|l| l.member(g.id)) {
            prefix.push_str(" | ");
            prefix.push_str(&crate::llm::one_line(&m.note, usize::MAX));
        }
        prefix.push('\n');
    }

    // ---- per-call part ------------------------------------------------------
    let request = input.request.trim();
    let profile = input.profile.is_some();
    // A mode is already a change of pace.
    let pace = mode.is_none() && input.profile.is_some_and(|p| p.wants_change_of_pace(request, input.count));
    let pace_note = if pace { ", with one change of pace" } else { "" };
    let request_text = if let Some(m) = mode {
        let asked = match m {
            Mode::NewToYou => {
                "New to me: games in a genre I rarely play or with a mechanic I haven't tried, easy to get into and short, \
                 each a way into a different one."
            }
            Mode::Gems => "A hidden gem: an unplayed, very highly rated game of a session or two that's unusual.",
            // Drawn without the AI (`discovery::random`); said here in case a prompt is ever built for it.
            Mode::Random => "Something off my radar: a game I haven't started, or last played over a year ago.",
        };
        if request.is_empty() { asked.to_string() } else { format!("{asked}\nAlso: {request}") }
    } else if request.is_empty() {
        if profile {
            format!("No specific request. Pick a varied, well-regarded set that fits About me{pace_note}.")
        } else {
            "No specific request. Pick a varied, well-regarded set that suits my taste.".to_string()
        }
    } else if profile && crate::taste::vague(request) {
        format!("{request}\n(A vague request: lean on About me{pace_note}.)")
    } else {
        request.to_string()
    };
    let mut user = String::new();
    user.push_str("## My request\n");
    user.push_str(&request_text);
    user.push('\n');
    if ranking.controller_asked() {
        // The routine's "This session" says it in the same words when the
        // routine is the one on a controller.
        user.push_str("On a controller: games that play well on one, not games made for a mouse.\n");
    }
    if let Some(r) = input.routine {
        user.push('\n');
        user.push_str(&r.prompt_section());
    }
    let named = |ids: &[i64]| -> Vec<String> {
        input.games.iter().filter(|g| ids.contains(&g.id)).map(|g| format!("#{} {}", g.id, crate::llm::name_line(&g.name))).collect()
    };
    let turned_down = named(input.exclude_ids);
    // One line each, as routine names and notes are, so a pasted reason
    // with a "## " line in it can't add a section of its own.
    let avoid: Vec<String> = input.avoid.iter().map(|a| crate::llm::one_line(a, 200)).filter(|a| !a.is_empty()).collect();
    if !turned_down.is_empty() {
        user.push_str("\n## Not this time\n");
        user.push_str(&format!("I turned down: {}. Don't pick these.\n", turned_down.join(", ")));
        if !avoid.is_empty() {
            user.push_str("Because:\n");
            for a in &avoid {
                user.push_str(&format!("- {a}\n"));
            }
            user.push_str(if profile {
                "Steer away from games like that; these reasons outweigh About me.\n"
            } else {
                "Steer away from games like that; these reasons outweigh my taste list.\n"
            });
        }
    }
    let kept = named(input.on_screen);
    if !kept.is_empty() {
        user.push_str("\n## Still in my lineup\n");
        user.push_str(&format!("{}. Don't pick these again, or another game of their series.\n", kept.join(", ")));
    }
    let said: Vec<String> = chosen
        .iter()
        .filter(|g| !off_limits(g))
        .filter_map(|g| {
            let t = g.too.as_ref()?;
            Some(format!("- #{} {}: too {}", g.id, crate::llm::name_line(&g.name), crate::llm::one_line(&t.word, crate::lineup::MAX_WORD)))
        })
        .collect();
    if !said.is_empty() {
        user.push_str("\n## What I said about some of these in earlier lineups\n");
        user.push_str(&said.join("\n"));
        user.push('\n');
    }
    let landed = recent_ratings(input);
    if !landed.is_empty() {
        user.push_str("\n## How recent picks landed\n");
        user.push_str(
            "How well earlier picks fit what I asked for then. Lean toward games like the good picks when my request is like \
             theirs, and away from what wasn't a fit, for the reason given.\n",
        );
        user.push_str(&landed.join("\n"));
        user.push('\n');
    }
    let recent_names: Vec<String> =
        input.games.iter().filter(|g| input.recent_ids.contains(&g.id) && !off_limits(g)).map(|g| crate::llm::name_line(&g.name)).collect();
    if !recent_names.is_empty() {
        user.push_str("\n## Recently recommended (avoid unless clearly the best fit)\n");
        user.push_str(&recent_names.join(", "));
        user.push('\n');
    }
    let wished = wishlist_listed(input);
    if !wished.is_empty() {
        user.push_str(
            "\n## On my wishlist (games I don't own)\n\
             Format: w<id> name | genres & tags | main-story hours (?h = unknown) | review % positive/count | year | says: the store's blurb\n",
        );
        for w in &wished {
            user.push_str(&wish_line(w));
            user.push('\n');
        }
    }
    user.push_str(&format!(
        "\n## Task\nPick exactly {} games from the candidates, in order of fit, then {} spares, the next best in order. \
         Use the #id numbers exactly as listed.",
        input.count, input.spares
    ));
    if !wished.is_empty() {
        user.push_str(" If one wishlist game fits clearly better than a pick, add it as \"wishlisted\".");
    }
    if input.wishlist.is_some() {
        user.push_str(" Add a \"suggestion\" only for a game I don't own that fits unusually well.");
    }

    let second_chances = input.second_chances();
    let system = format!(
        "You pick games from one person's library. Most candidates are games they own and haven't really played; \
         some have a history with them, shown on their line. Choose exactly {count} that best fit the request and their taste, \
         then exactly {spares} spares ranked after them: a spare takes the place of a pick they turn down, so rank the spares as carefully.\n\
         Rules:\n\
         - Only ids from the candidate list, all distinct across picks and spares. Never invent games.\n\
         - Never two games of one series among the picks and spares together (\"series:\" on a line names it); vary the rest too.\n\
         - Respect time budgets using the main-story hours: a short session needs something satisfying in that window, not a 60-hour RPG.\n\
         - Match mood words (relaxing, cozy, story, challenging, co-op…) against the tags and what players say.\n\
         {feel_rule}\
         {verdicts_rule}\
         - History: \"shelved\" means they put it down meaning to finish it, \"second chance: abandoned\" that they gave up on it, \
           \"willing to try again\" that they gave up on it and since said they'd give it another go; \"stopped:\" is why, when \
           they said, and \"reviews since:\" what reviews written after they stopped say about it. Weigh why it stopped against \
           the request and against what players say now. \
           Its reason must speak to the old problem honestly (\"You stopped over crashes; players say the 2.0 patch fixed them\"), \
           and say so when what players say doesn't settle it. Never claim to know where they are in a game.\n\
         - {second_chance_rule}\n\
         - \"endless\" games have no ending, and staples are games they've played for a long time: pitch these as a session, \
           never as something to finish. Staples suit requests for something familiar or a comfort game.\n\
         - What they said about a game in an earlier lineup (\"too long\") counts against it unless the request wants just that.\n\
         {taste_rule}\
         {mode_rule}\
         {routine_rule}\
         - Prefer variety unless the request narrows it. A great niche fit beats a generic hit.\n\
         - Each reason: 1-2 specific sentences on why it fits this request and this person. No fluff, no restating numbers.\n\
         {linux_rule}\
         {wishlist_rule}\
         {suggestion_rule}\
         - Respond with JSON only, matching the schema.",
        count = input.count,
        spares = input.spares,
        feel_rule = if input.review_feel {
            "- The feel and the +/-/± aspects come from player reviews. Weigh them against the request: \
               a criticized aspect counts against a game when the request cares about it.\n"
        } else {
            ""
        },
        verdicts_rule = if input.verdicts { format!("- {BV_RULE}\n") } else { String::new() },
        taste_rule = if profile {
            "- About me is a signal, not a template: when the request asks for something different, follow the request. \
               Hard nos always hold: never pick a game that is one, whatever the request.\n\
             - A vague request (\"anything\", \"surprise me\", \"something good\", or none at all) leans on About me: \
               games like their favourites, for the reasons they love them, and like the ones they stick with, away from what \
               they drift from or dislike. Each reason names the favourite, like or pattern it follows. \
               When it says \"with one change of pace\", make the last pick a change of pace instead: a well-regarded game \
               outside their favourites, likes and the patterns they stick with (one they'd usually drift from is fine; never a \
               hard no), whose reason says it's a change of pace and why it could still land.\n"
        } else {
            "- The taste list is a signal, not a template: when the request asks for something different, follow the request.\n"
        },
        routine_rule = if input.routine.is_some() {
            "- \"This session\" is the routine they're playing in now: who with, where, on what and for how long. Fit every pick \
               and spare to it (a game for others when they're not alone, one that plays well on what they're holding, one that \
               suits the length) unless the request asks otherwise, and say so in a reason when it's why a game fits.\n"
        } else {
            ""
        },
        mode_rule = match mode {
            Some(Mode::NewToYou) => {
                "- New to you: every candidate is a way into a genre they rarely play or a mechanic they haven't tried, named \
                   after \"new to you:\", and is easy to get into and short. Make each pick a way into a different one of those, \
                   so the lineup shows a spread. Each reason is a way in: say in plain words what that genre or mechanic is like, \
                   then why this game is a gentle first one (\"You've never played a deckbuilder. This one teaches you in 20 \
                   minutes.\"). Never say they've played that kind of game before.\n"
            }
            Some(Mode::Gems) => {
                "- Hidden gems: every candidate is an unplayed game players rate very highly, a session or two long; \
                   \"unusual:\" says what makes it stand out. Prefer the more unusual ones that fit the request. Each reason \
                   says what makes it unusual and why it fits now.\n"
            }
            Some(Mode::Random) | None => "",
        },
        second_chance_rule = if second_chances > 1 {
            "They asked for games they put down: second chances are welcome where they fit."
        } else {
            "At most one second chance among the picks and spares together, and only when it fits the request."
        },
        wishlist_rule = if wished.is_empty() {
            ""
        } else {
            "- \"On my wishlist\" lists games they want but don't own. At most one of them may join the lineup, as \"wishlisted\", \
               and only when it fits this request clearly better than one of the picks: \"instead_of\" names the pick it stands in \
               for. That pick stays a full pick, as it's what they get when they switch to a game they own. Its id is the w-number \
               as listed. Most lineups have none.\n"
        },
        suggestion_rule = if input.wishlist.is_some() {
            "- A suggestion is optional: one real PC game on Steam that they don't own, from your own knowledge, named in \
               \"suggestion\" only when it fits this request and their taste unusually well. Never a candidate, a wishlist game, \
               another edition of one, or a hard no. Its reason is one sentence on why it's right down their alley. Leave it out \
               unless the fit is unusually strong.\n"
        } else {
            ""
        },
        linux_rule = if input.linux {
            "- They play on Linux: native and platinum/gold run well; silver/bronze have problems; borked doesn't run. \
               Avoid silver/bronze/borked unless asked; no value means unknown, not bad.\n"
        } else {
            ""
        }
    );

    let candidates: HashMap<i64, GameView> = chosen.into_iter().filter(|g| !off_limits(g)).map(|g| (g.id, g.clone())).collect();
    let schema = schema_with_wishlist(input.count, input.spares, input.wishlist.is_some(), !wished.is_empty());
    let wishlisted = wished.into_iter().map(|w| (w.id, w.clone())).collect();
    BuiltPrompt {
        system,
        prefix,
        user,
        schema,
        candidates,
        considered,
        eligible,
        second_chances,
        wishlisted,
        suggests: input.wishlist.is_some(),
    }
}

#[cfg(test)]
mod tests;
