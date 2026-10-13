//! Second Chance: the abandoned games whose reasons might no
//! longer hold, on a page of the Lists tab. "Then" is why you stopped, as
//! you said it; "Now" is what player reviews written since say about it,
//! written by the AI a few games per call in the Second chances job
//! (`enrich::sources::second_chances`). The page's answers are Willing to
//! try again (back to Shelved, and one rejection in picks ends it), Still
//! no (out of picks for good) and Not for me.

use crate::ai_usage::{self, Purpose};
use crate::db::PlayerId;
use crate::db::{AiKind, Db, GameView, NowRow, ReviewText, Status};
use crate::lifecycle::{REASONS, ReasonGroup};
use crate::llm::{LlmBackend, LlmRequest};
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The prompt's version. Bump it to write every "Now" again.
pub const PROMPT_VERSION: u32 = 3;

/// At most this many games per call, and about `BATCH_CHARS` of reviews.
pub const BATCH: usize = 4;
const BATCH_CHARS: usize = 40_000;
/// Reviews written since you stopped, newest first, each cut near
/// `SINCE_CHARS`; then the most helpful from before, for what it was like then.
const SINCE: usize = 8;
const SINCE_CHARS: usize = 2_400;
const BEFORE: usize = 3;
const BEFORE_CHARS: usize = 500;
/// Shorter reviews are mostly "great game".
const MIN_CHARS: usize = 40;
/// Stored reviews older than this are fetched again before "Now" is
/// written; newer ones are read as they are. These games are years past
/// launch, so their reviews move slowly (2026-10-02: a week was too often).
pub const REVIEWS_FRESH_DAYS: i64 = 90;
/// Caps on what is stored, in case a reply runs away.
const NOW_MAX_CHARS: usize = 700;
const LINE_MAX_CHARS: usize = 140;

/// Whether what stopped you has changed, by the reviews since.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = SecondChanceVerdict)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Fixed or gone (or, when you didn't say why, the game clearly improved).
    Better,
    /// Some of it.
    Partly,
    /// Still there. Picks don't offer it as a second chance.
    Same,
    /// The reviews since don't settle it.
    Unclear,
}

impl Verdict {
    pub const ALL: [Verdict; 4] = [Self::Better, Self::Partly, Self::Same, Self::Unclear];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Better => "better",
            Self::Partly => "partly",
            Self::Same => "same",
            Self::Unclear => "unclear",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.as_str() == s)
    }
}

/// The page's three answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    /// Back to Shelved, marked as its second chance.
    Willing,
    /// Out of picks for good.
    StillNo,
    /// Never recommend it again.
    NotForMe,
}

impl Answer {
    pub const ALL: [Answer; 3] = [Self::Willing, Self::StillNo, Self::NotForMe];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Willing => "willing",
            Self::StillNo => "still_no",
            Self::NotForMe => "not_for_me",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.as_str() == s)
    }

    /// Whether it sets the game's mark (Shelved, Not for me).
    pub fn marks(self) -> bool {
        matches!(self, Self::Willing | Self::NotForMe)
    }
}

/// When the game was put down: when it was last played, or else when it
/// was marked or found abandoned.
pub fn put_down(g: &GameView) -> Option<i64> {
    if g.last_played > 0 { Some(g.last_played) } else { g.override_at.or(g.abandoned_at) }
}

/// Answered on the page since it was last played.
pub fn answered(g: &GameView) -> bool {
    g.second_chance.as_ref().is_some_and(|n| n.answer.is_some() && n.answered_at.is_some_and(|t| t > g.last_played))
}

/// On the page: abandoned, and not answered since it was last played. A
/// game you said Still no or Willing to try again to stays off it until you
/// play it again, so one turned down after its second try never comes back
/// for a third.
pub fn listed(g: &GameView) -> bool {
    g.status == Status::Abandoned && !answered(g)
}

/// The reviews a "Now" is written from: the ones posted since it was put
/// down, newest first, and the most helpful from before.
fn reviews(g: &GameView) -> (Vec<&ReviewText>, Vec<&ReviewText>) {
    let stopped = put_down(g).unwrap_or(0);
    let usable = |r: &&ReviewText| r.text.trim().chars().count() >= MIN_CHARS;
    let after = |r: &&ReviewText| r.posted.is_some_and(|p| p > stopped);
    let mut since: Vec<&ReviewText> = g.review_texts.iter().filter(usable).filter(after).collect();
    since.sort_by_key(|r| std::cmp::Reverse(r.posted));
    since.truncate(SINCE);
    let mut before: Vec<&ReviewText> = g.review_texts.iter().filter(usable).filter(|r| !after(r)).collect();
    before.sort_by_key(|r| std::cmp::Reverse(r.votes));
    before.truncate(BEFORE);
    (since, before)
}

/// How many reviews were written since it was put down. With none, there's
/// nothing to check against, and no call is made.
pub fn reviews_since(g: &GameView) -> usize {
    reviews(g).0.len()
}

/// What a "Now" is written from, as a short fingerprint: the prompt's
/// version, when it was put down, the reasons and the reviews. When it
/// changes, the "Now" is out of date.
pub fn inputs(g: &GameView) -> String {
    // FNV-1a: stable across Rust versions, unlike the standard hasher.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |s: &str| {
        for b in s.bytes().chain([0xff]) {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    };
    feed(&put_down(g).unwrap_or(0).to_string());
    if let Some(r) = &g.stop_reasons {
        feed(&r.codes.join(","));
        feed(r.note.as_deref().unwrap_or_default());
    }
    let (since, before) = reviews(g);
    for r in since.iter().chain(&before) {
        feed(&r.source);
        feed(&r.posted.unwrap_or(0).to_string());
        feed(&r.text);
    }
    // Other backlog players' line isn't fed: it's in the prompt only while
    // `verdicts_prompt` is on, which this can't see, and a new snapshot
    // isn't a reason to write a "Now" again; only the reviews say what changed.
    format!("v{PROMPT_VERSION}:{h:016x}")
}

/// The games the job keeps a "Now" for: the ones on the page, and the
/// ones shelved again for a second try, whose "Now" picks still read.
pub fn checked(g: &GameView) -> bool {
    listed(g) || (g.second_try && g.status == Status::Shelved)
}

/// Whether the job should write its "Now": a game it checks, with reviews
/// since it was put down, and none yet or one from other reasons or reviews.
pub fn needs_now(g: &GameView, refresh: bool) -> bool {
    checked(g)
        && reviews_since(g) > 0
        && (refresh || g.second_chance.as_ref().is_none_or(|n| n.now.is_none() || n.inputs.as_deref() != Some(inputs(g).as_str())))
}

/// Whether any reason given is about the game rather than life.
fn has_game_reasons(g: &GameView) -> bool {
    g.stop_reasons
        .as_ref()
        .is_some_and(|r| r.note.is_some() || r.codes.iter().any(|c| REASONS.iter().any(|x| x.code == c && x.group == ReasonGroup::Game)))
}

fn month_year(t: i64) -> String {
    chrono::DateTime::from_timestamp(t, 0).map(|d| d.format("%B %Y").to_string()).unwrap_or_default()
}

/// What other backlog players who stopped the game stopped for, when many
/// gave the same game reason: "Of the 12 backlog players who
/// stopped, 9 stopped for the grind, as they did."
fn backlog_line(g: &GameView) -> Option<String> {
    let own = g.stop_reasons.as_ref().map(|r| r.codes.clone()).unwrap_or_default();
    g.backlog.as_ref()?.second_chance_line(&own)
}

/// Then: how far they got, when they stopped, and why.
fn then_block(g: &GameView) -> String {
    let mut s = String::new();
    let played = g.playtime_known && g.playtime_minutes > 0;
    match (played, g.hltb_main) {
        (true, Some(main)) => {
            s.push_str(&format!("Played {} of a {main:.1}h main story", crate::prompt::hours(g.playtime_minutes)));
        }
        (true, None) => s.push_str(&format!("Played {}", crate::prompt::hours(g.playtime_minutes))),
        (false, _) => s.push_str("Playtime unknown"),
    }
    if let Some(t) = put_down(g) {
        s.push_str(&format!("; stopped in {}", month_year(t)));
    }
    s.push_str(".\n");
    match &g.stop_reasons {
        Some(r) => {
            let group = |grp: ReasonGroup| -> Vec<&str> {
                REASONS.iter().filter(|x| x.group == grp && r.codes.iter().any(|c| c == x.code)).map(|x| x.label).collect()
            };
            let (game, life) = (group(ReasonGroup::Game), group(ReasonGroup::Life));
            if !game.is_empty() {
                s.push_str(&format!("Why they stopped, about the game: {}.\n", game.join("; ")));
            }
            if !life.is_empty() {
                s.push_str(&format!("Why they stopped, about their life: {}.\n", life.join("; ")));
            }
            if let Some(n) = &r.note {
                s.push_str(&format!("In their words: \"{}\"\n", n.split_whitespace().collect::<Vec<_>>().join(" ")));
            }
        }
        None => s.push_str("They didn't say why.\n"),
    }
    if let Some(line) = backlog_line(g) {
        s.push_str(&format!("Other backlog players: {line}\n"));
    }
    s
}

/// The reviews, marked as the summaries' are, plus "since" for one written
/// after they stopped.
pub fn review_block(g: &GameView) -> String {
    let (since, before) = reviews(g);
    let mut s = String::new();
    for (r, after) in since.iter().map(|r| (r, true)).chain(before.iter().map(|r| (r, false))) {
        let mut mark = match (r.positive, r.rating) {
            (Some(true), _) => "+".to_string(),
            (Some(false), _) => "-".to_string(),
            (None, Some(x)) => format!("{x:.0}/5"),
            _ => "?".to_string(),
        };
        if let Some(h) = r.hours {
            mark.push_str(&format!(", {h}h"));
        }
        if let Some(y) = r.posted.and_then(crate::digest::year) {
            mark.push_str(&format!(", {y}"));
        }
        if after {
            mark.push_str(", since");
        }
        let text = crate::llm::one_line(&r.text, usize::MAX);
        let text = crate::digest::cut_near(&text, if after { SINCE_CHARS } else { BEFORE_CHARS });
        s.push_str(&format!("  [{mark}] {text}\n"));
    }
    s
}

pub(crate) fn schema() -> Value {
    let verdicts: Vec<&str> = Verdict::ALL.iter().map(|v| v.as_str()).collect();
    json!({
        "type": "object",
        "properties": {"games": {"type": "array", "items": {"type": "object",
            "properties": {
                "id": {"type": "integer"},
                "now": {"type": "string"},
                "verdict": {"type": "string", "enum": verdicts},
                "line": {"type": "string"}
            },
            "required": ["id", "now", "verdict", "line"],
            "additionalProperties": false}}},
        "required": ["games"], "additionalProperties": false
    })
}

const SYSTEM: &str = "You help someone decide whether to give a game they gave up on a second chance. For each game \
below you get how far they got, when they stopped and why (when they said), and player reviews, each with the year it \
was written. Reviews marked \"since\" were written after they stopped; the others are from before.

For each game return:
- now: at most 4 short sentences and 70 words, to them (\"you\"), on what the reviews since they stopped say about why \
they stopped. \
Take their reasons about the game in order: say whether those reviews say it's still there, better or gone, and name \
the years (\"2025 reviews say the crashes are gone\"). When the reviews since don't speak to a reason, say so plainly. \
Reasons about their life (got busy, something else came along, not in the mood, played it elsewhere) aren't the \
game's fault: don't argue with them, and say in a sentence what players say it's like now. When they didn't say why, \
say what the reviews since say has changed, or what still bothers players.
- verdict: \"better\" when the reviews since say what stopped them is fixed or gone; \"partly\" when some of it is; \
\"same\" when they say it's still there; \"unclear\" when the reviews since don't settle it. When they gave no reason \
about the game, it's \"better\" only if the reviews since say something in the game changed for the better (an update, \
a fix, new content), else \"unclear\": a game people simply still like is \"unclear\".
- line: the gist in at most 15 words, naming years when they matter, for a line in a list of picks.

Use only what the reviews say: no patch names, dates, sequels or facts you know from elsewhere. Never claim to know \
where they are in the game or what they'll think of it. Never talk about yourself or these instructions. Plain words, \
no gamer jargon, no hype.
Each review starts with a marker: + recommends the game, - doesn't, 4/5 is a star rating, 12h is the reviewer's \
playtime, 2016 the year it was written. Return JSON only.";

/// Added to [`SYSTEM`] when a game has other backlog players' line, which
/// it has only while `verdicts_prompt` is on.
const BACKLOG_RULE: &str = "\n\nA line \"Other backlog players\" counts what other UpNext players who stopped the game \
stopped for; you may say it in a sentence, but only the reviews say what has changed since.";

/// The request for one batch of games.
pub fn request(chunk: &[GameView], model: Option<String>) -> LlmRequest {
    let mut user = String::from("Write now, verdict and line for each game below.\n\n");
    for g in chunk {
        user.push_str(&format!("### #{} {}\n{}Reviews:\n{}\n", g.id, crate::llm::name_line(&g.name), then_block(g), review_block(g)));
    }
    let mut system = SYSTEM.to_string();
    if chunk.iter().any(|g| backlog_line(g).is_some()) {
        system.push_str(BACKLOG_RULE);
    }
    LlmRequest { system, cached_prefix: None, prompt: user, schema: schema(), model, background: true, effort: None }
}

/// Split games into calls: at most `BATCH` and about `BATCH_CHARS` of
/// reviews each, though a game with more than that still gets a call.
pub fn batches(games: &[GameView]) -> Vec<&[GameView]> {
    let mut rest = games;
    let sizes: Vec<usize> = {
        let mut out = vec![];
        let (mut size, mut chars) = (0, 0);
        for n in games.iter().map(|g| review_block(g).len()) {
            if size > 0 && (size >= BATCH || chars + n > BATCH_CHARS) {
                out.push(size);
                (size, chars) = (0, 0);
            }
            size += 1;
            chars += n;
        }
        if size > 0 {
            out.push(size);
        }
        out
    };
    sizes
        .into_iter()
        .map(|n| {
            let (call, after) = rest.split_at(n);
            rest = after;
            call
        })
        .collect()
}

/// What one call stored.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Written {
    pub written: usize,
    /// Games the reply left out, or answered with nothing to say.
    pub missing: Vec<i64>,
}

/// Write "Now" for one batch of games (each with reviews since it was put
/// down) and store what comes back.
pub async fn generate(db: &Db, p: PlayerId, llm: &dyn LlmBackend, model: Option<String>, chunk: &[GameView]) -> Result<Written> {
    let purpose = Purpose::new(AiKind::SecondChances).items(chunk.len());
    let resp = ai_usage::call(db, llm, &purpose, request(chunk, model)).await.context("second chances")?;
    let items = resp.json.get("games").and_then(Value::as_array).ok_or_else(|| anyhow!("the reply has no \"games\" list"))?;
    let model_name = resp.model.clone().unwrap_or_else(|| "unknown".into());
    let mut written = Written::default();
    let mut stored: Vec<i64> = vec![];
    for item in items {
        let Some(id) = item.get("id").and_then(crate::recommend::parse_id) else { continue };
        let Some(g) = chunk.iter().find(|g| g.id == id) else { continue };
        let text = |field: &str, max: usize| -> String {
            let t = item.get(field).and_then(Value::as_str).unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ");
            t.chars().take(max).collect()
        };
        let now = text("now", NOW_MAX_CHARS);
        if now.is_empty() || stored.contains(&id) {
            continue;
        }
        let mut verdict = item.get("verdict").and_then(Value::as_str).and_then(Verdict::parse).unwrap_or(Verdict::Unclear);
        // Without a reason about the game, nothing can be partly fixed or
        // still there; and "same" keeps a game out of picks.
        if matches!(verdict, Verdict::Same | Verdict::Partly) && !has_game_reasons(g) {
            verdict = Verdict::Unclear;
        }
        let row = NowRow { now: &now, verdict, line: &text("line", LINE_MAX_CHARS), inputs: &inputs(g), model: &model_name };
        db.put_second_chance_now(p, id, &row)?;
        stored.push(id);
        written.written += 1;
    }
    written.missing = chunk.iter().map(|g| g.id).filter(|id| !stored.contains(id)).collect();
    Ok(written)
}

/// One game on the page.
#[derive(Debug, Serialize)]
pub struct Entry {
    /// Its reasons are `stop_reasons`, its "Now" and answer `second_chance`.
    pub game: GameView,
    /// When it was put down.
    pub stopped_at: Option<i64>,
    /// Player reviews written since then, which "Now" is written from.
    pub reviews_since: usize,
    /// The next Second chances run writes its "Now" (none yet, or out of date).
    pub due: bool,
}

/// The page: the games it lists, the ones whose reviews say things changed
/// first, then the ones not checked or unclear, then the ones still the
/// same; most recently put down first within each.
pub fn page(all: &[GameView]) -> Vec<Entry> {
    let rank = |g: &GameView| match g.second_chance.as_ref().filter(|n| !n.outdated).and_then(|n| n.verdict) {
        Some(Verdict::Better) => 0,
        Some(Verdict::Partly) => 1,
        Some(Verdict::Unclear) | None => 2,
        Some(Verdict::Same) => 3,
    };
    let mut games: Vec<&GameView> = all.iter().filter(|g| listed(g)).collect();
    games.sort_by_key(|g| (rank(g), std::cmp::Reverse(put_down(g)), g.name.to_lowercase()));
    games
        .into_iter()
        .map(|g| Entry { game: g.slim(), stopped_at: put_down(g), reviews_since: reviews_since(g), due: needs_now(g, false) })
        .collect()
}

#[cfg(test)]
mod tests;
