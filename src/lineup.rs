//! A lineup after it's picked: what was done with each pick on
//! its card, spares taking the place of picks turned down, and "It's too
//! ___", which asks the AI for spares without the problem (or, without AI,
//! takes the next spare the word doesn't rule out).

use crate::ai_usage::{self, Purpose};
use crate::config::Config;
use crate::db::PlayerId;
use crate::db::{AiKind, Db, FeedbackAction, GameView, LineupChange, Override, Pick, PickBatch};
use crate::lifecycle::Offer;
use crate::llm::{LlmBackend, LlmError, LlmRequest};
use crate::recommend::{Screen, parse_id};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;

/// The longest "It's too ___" word kept, in characters.
pub(crate) const MAX_WORD: usize = 40;

#[derive(Debug, thiserror::Error)]
pub enum LineupError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Bad(String),
    /// Another action changed the lineup first (`db::LineupChanged`).
    #[error("{0}")]
    Conflict(String),
    /// Prints the whole chain itself, so it has no `source`.
    #[error("{0:#}")]
    Other(anyhow::Error),
}

impl From<anyhow::Error> for LineupError {
    fn from(e: anyhow::Error) -> Self {
        match e.downcast_ref::<crate::db::LineupChanged>() {
            Some(c) => LineupError::Conflict(c.to_string()),
            None => LineupError::Other(e),
        }
    }
}

/// A pick's action on its card, as the API takes it.
#[derive(Debug, Clone)]
pub struct Feedback {
    pub title_id: i64,
    pub action: FeedbackAction,
    /// For `started`: how it went, an override's name ("done", "shelved").
    pub text: Option<String>,
    /// With `started` and Shelved, Abandoned or Burnt out: why it stopped.
    pub reasons: Option<(Vec<String>, Option<String>)>,
}

/// The lineup and the pick on screen that an action is about.
fn on_screen(db: &Db, p: PlayerId, batch_id: &str, title_id: i64) -> Result<(PickBatch, Pick), LineupError> {
    let batch = db.pick_batch(p, batch_id)?.ok_or_else(|| LineupError::NotFound(format!("no lineup {batch_id}")))?;
    let pick = batch
        .picks
        .iter()
        .find(|p| p.id == title_id)
        .cloned()
        .ok_or_else(|| LineupError::NotFound(format!("#{title_id} is not on screen in lineup {batch_id}")))?;
    Ok((batch, pick))
}

/// How many second chances a lineup may show at once.
fn second_chance_limit(batch: &PickBatch) -> usize {
    if crate::prompt::asks_for_second_chances(&batch.prompt) { usize::MAX } else { 1 }
}

/// The lineup's screen without the games in `leaving`.
fn screen_without<'a>(batch: &PickBatch, leaving: &[i64], series: &'a HashMap<i64, Vec<String>>) -> Screen<'a> {
    let mut screen = Screen::new(series, second_chance_limit(batch));
    for g in batch.picks.iter().filter(|p| !leaving.contains(&p.id)).filter_map(|p| p.game.as_ref()) {
        screen.add(g);
    }
    screen
}

/// Spares that could go on `screen` now, best first: still offered by
/// picks (nothing marked since rules them out), not of a series on
/// screen, and within the second-chance limit.
fn usable<'b>(batch: &'b PickBatch, screen: &Screen) -> Vec<&'b Pick> {
    batch.spares.iter().filter(|p| p.game.as_ref().is_some_and(|g| g.offer().is_some() && screen.clash(g).is_none())).collect()
}

fn series_map(db: &Db, p: PlayerId) -> anyhow::Result<HashMap<i64, Vec<String>>> {
    Ok(crate::series::of_titles(&db.all_games_shared(p)?, &db.catalog()?))
}

/// Take a pick off with `why`, and put the best usable spare in its slot.
/// With no spare left the slot stays empty, for `recommend::more`.
fn take_off(db: &Db, p: PlayerId, batch: &PickBatch, pick: &Pick, why: LineupChange) -> Result<(), LineupError> {
    let series = series_map(db, p)?;
    let screen = screen_without(batch, &[pick.id], &series);
    let next = usable(batch, &screen).first().map(|p| p.id);
    let mut changes = vec![why, LineupChange::Slot { title_id: pick.id, slot: None }];
    if let Some(id) = next {
        changes.push(LineupChange::Slot { title_id: id, slot: pick.slot });
    }
    db.change_lineup(&batch.batch_id, &changes)?;
    Ok(())
}

/// What was done with a pick on its card: Play (`chosen`), Not tonight,
/// Never recommend this (`not_for_me`), or "I've started it…". Not tonight
/// and Never recommend this put the next spare in its place at once, with
/// no AI call; so does "I've started it…" when how it went takes the game
/// out of picks. Returns the lineup as it stands after.
pub fn feedback(db: &Db, p: PlayerId, batch_id: &str, f: Feedback) -> Result<PickBatch, LineupError> {
    let (batch, pick) = on_screen(db, p, batch_id, f.title_id)?;
    owned(&pick)?;
    let note = |action: FeedbackAction, text: Option<String>| LineupChange::Feedback { title_id: f.title_id, action, text };
    match f.action {
        FeedbackAction::Chosen => db.change_lineup(batch_id, &[note(FeedbackAction::Chosen, None)])?,
        FeedbackAction::NotTonight => take_off(db, p, &batch, &pick, note(FeedbackAction::NotTonight, None))?,
        FeedbackAction::NotForMe => {
            db.set_override(p, f.title_id, Some(Override::Hidden), None)?;
            take_off(db, p, &batch, &pick, note(FeedbackAction::NotForMe, None))?;
        }
        FeedbackAction::Started => {
            let how = f.text.as_deref().unwrap_or_default();
            let status = Override::parse(how)
                .ok_or_else(|| LineupError::Bad(format!("say how it went: one of {}", Override::ALL.map(|o| o.as_str()).join(", "))))?;
            if f.reasons.is_some() && !status.asks_why() {
                return Err(LineupError::Bad("reasons go with shelved, abandoned or burnt_out".into()));
            }
            db.set_override(p, f.title_id, Some(status), None)?;
            if let Some((codes, own_words)) = &f.reasons {
                db.add_stop_reasons(p, f.title_id, codes, own_words.as_deref(), "pick")?;
            }
            let why = note(FeedbackAction::Started, Some(status.as_str().to_string()));
            // Shelved and Still playing stay in picks, so they stay on screen.
            match db.game(p, f.title_id)?.and_then(|g| g.offer()) {
                Some(Offer::Shelved | Offer::Session) => db.change_lineup(batch_id, &[why])?,
                _ => take_off(db, p, &batch, &pick, why)?,
            }
        }
        FeedbackAction::Too | FeedbackAction::SameProblem => {
            return Err(LineupError::Bad("\"It's too ___\" has its own call: POST /api/picks/{batch_id}/too".into()));
        }
    }
    Ok(db.pick_batch(p, batch_id)?.expect("the lineup was just read"))
}

/// A game you don't own has one action on its card: the switch.
fn owned(pick: &Pick) -> Result<(), LineupError> {
    if pick.owned {
        return Ok(());
    }
    Err(LineupError::Bad(format!(
        "{} isn't in your library: switch it for an owned game with POST /api/picks/{{batch_id}}/switch",
        pick.name
    )))
}

/// "Switch with owned game" on a wishlisted pick: the owned game
/// the pick call chose with it takes its slot at once, with no AI call.
/// When that game can't go on screen any more (marked since, or of a series
/// a spare brought on), the best usable spare does, as for Not tonight.
pub fn switch(db: &Db, p: PlayerId, batch_id: &str, title_id: i64) -> Result<PickBatch, LineupError> {
    let (batch, pick) = on_screen(db, p, batch_id, title_id)?;
    if pick.owned {
        return Err(LineupError::Bad(format!("{} is in your library: there's nothing to switch", pick.name)));
    }
    let alt = pick.alternate.as_ref().ok_or_else(|| LineupError::Bad(format!("{} came with no owned game to switch to", pick.name)))?;
    let series = series_map(db, p)?;
    let screen = screen_without(&batch, &[pick.id], &series);
    let fits = |g: &GameView| g.offer().is_some() && screen.clash(g).is_none();
    let held = batch.spares.iter().chain(&batch.removed).chain(&batch.picks).any(|p| p.id == alt.id);
    match db.game(p, alt.id)? {
        Some(g) if !held && fits(&g) => {
            db.change_lineup(batch_id, &[LineupChange::Slot { title_id: pick.id, slot: None }])?;
            db.add_to_batch(batch_id, &[Pick { slot: pick.slot, ..Pick::of(&g, alt.reason.clone()) }])?;
        }
        _ => {
            tracing::info!("{} can't go on screen any more; the next spare takes the switch", alt.name);
            let next = usable(&batch, &screen).first().map(|p| p.id);
            let mut changes = vec![LineupChange::Slot { title_id: pick.id, slot: None }];
            if let Some(id) = next {
                changes.push(LineupChange::Slot { title_id: id, slot: pick.slot });
            }
            db.change_lineup(batch_id, &changes)?;
        }
    }
    Ok(db.pick_batch(p, batch_id)?.expect("the lineup was just read"))
}

/// The word as typed after "It's too", tidied: "long", "slow paced".
pub fn clean_word(word: &str) -> Result<String, LineupError> {
    // One line: the word goes into the system prompt and a "## " heading.
    let mut w = crate::llm::one_line(word, usize::MAX).to_lowercase().replace('\u{2019}', "'");
    for prefix in ["it's too ", "its too ", "too "] {
        if let Some(rest) = w.strip_prefix(prefix) {
            w = rest.trim().to_string();
        }
    }
    let w = w.trim_end_matches(['.', '!', '?', ',', ';', ':']).trim().to_string();
    if w.is_empty() || w == "too" {
        return Err(LineupError::Bad("say what it's too much of: \"long\", \"hard\"".into()));
    }
    if w.chars().count() > MAX_WORD {
        return Err(LineupError::Bad(format!("keep it to {MAX_WORD} characters")));
    }
    Ok(w)
}

/// One slot changed by "It's too ___".
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
pub struct Swap {
    pub slot: usize,
    /// The pick taken off.
    pub out: i64,
    /// The spare put in, or None when no spare was free of the problem
    /// (the slot waits for new spares).
    pub into: Option<i64>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct TooOutcome {
    pub batch: PickBatch,
    /// The turned-down pick first, then any other with the same problem.
    pub swaps: Vec<Swap>,
    /// Why the AI couldn't check the spares, when it couldn't: the swap
    /// then came from the word alone (`prompt::rules_out`).
    pub ai_error: Option<String>,
}

const TOO_SYSTEM: &str = "Someone turned down a game in the lineup of picks made for them: it's too {word}. Their request, \
the turned-down game, the rest of the lineup and its spares (best first) are below. Choose the spare to take its place: \
one that isn't too {word} and still fits the request; among spares that fit as well, the earlier one. Then look at the \
rest of the lineup: replace any game that is clearly too {word} as well, each with another such spare. Leave the others alone. \
Never put two games of one series on screen (\"series:\" on a line names it). Use 0 when no spare is free of the problem. \
Respond with JSON only, matching the schema.";

/// Added to [`TOO_SYSTEM`] when the lines carry other backlog players'
/// verdicts (`verdicts_prompt`): there's no About me here, so the
/// request and the order of the spares come first.
const TOO_BV_RULE: &str = " \"bv\" on a line is other backlog players' answers about how a game fits limited time: it may help \
judge whether a game is too {word}, but never above the request or the order of the spares.";

pub(crate) fn too_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "replacement": {"type": "integer", "description": "Spare id for the turned-down game, or 0"},
            "also": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "pick_id": {"type": "integer", "description": "A game still in the lineup with the same problem"},
                        "spare_id": {"type": "integer"}
                    },
                    "required": ["pick_id", "spare_id"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["replacement", "also"],
        "additionalProperties": false
    })
}

/// Check the AI's answer: the spare for the turned-down pick (None: no
/// spare fits), then other picks with the same problem and theirs.
fn check_too(json: &Value, batch: &PickBatch, target: i64, series: &HashMap<i64, Vec<String>>) -> Result<Vec<(i64, Option<i64>)>, String> {
    let spare = |id: i64| batch.spares.iter().find(|p| p.id == id).and_then(|p| p.game.as_ref());
    let replacement = json.get("replacement").and_then(parse_id).ok_or("no replacement id")?;
    let mut plan: Vec<(i64, Option<i64>)> = vec![(target, (replacement != 0).then_some(replacement))];
    for a in json.get("also").and_then(Value::as_array).into_iter().flatten() {
        let (Some(out), Some(into)) = (a.get("pick_id").and_then(parse_id), a.get("spare_id").and_then(parse_id)) else {
            return Err(format!("an entry of \"also\" isn't two ids: {a}"));
        };
        if into == 0 {
            continue;
        }
        if !batch.picks.iter().any(|p| p.id == out && p.owned) || plan.iter().any(|(o, _)| *o == out) {
            return Err(format!("#{out} isn't another game on screen"));
        }
        plan.push((out, Some(into)));
    }
    let leaving: Vec<i64> = plan.iter().map(|(o, _)| *o).collect();
    let mut screen = screen_without(batch, &leaving, series);
    let mut used: Vec<i64> = vec![];
    for (_, into) in &plan {
        let Some(id) = into else { continue };
        let g = spare(*id).ok_or_else(|| format!("#{id} isn't one of the spares"))?;
        if used.contains(id) {
            return Err(format!("#{id} was chosen twice"));
        }
        if g.offer().is_none() {
            return Err(format!("#{id} can't be picked any more"));
        }
        if let Some(e) = screen.clash(g) {
            return Err(e);
        }
        screen.add(g);
        used.push(*id);
    }
    Ok(plan)
}

/// When the AI can't check: the first usable spare the word doesn't rule out.
fn fallback(batch: &PickBatch, target: &Pick, word: &str, series: &HashMap<i64, Vec<String>>) -> Option<i64> {
    let screen = screen_without(batch, &[target.id], series);
    let out = target.game.as_ref()?;
    usable(batch, &screen).into_iter().find(|p| p.game.as_ref().is_some_and(|g| !crate::prompt::rules_out(word, out, g))).map(|p| p.id)
}

/// The "too" call's instructions, with the `bv` rule when the lines carry them.
fn too_system(cfg: &Config, word: &str) -> String {
    let mut system = TOO_SYSTEM.to_string();
    if cfg.verdicts_prompt {
        system.push_str(TOO_BV_RULE);
    }
    system.replace("{word}", word)
}

fn too_prompt(cfg: &Config, batch: &PickBatch, target: &Pick, spares: &[&Pick], word: &str, series: &HashMap<i64, Vec<String>>) -> String {
    let now = crate::db::now();
    let line = |g: &GameView| {
        let s = series.get(&g.id).and_then(|s| s.first()).map(String::as_str);
        crate::prompt::candidate_line(g, cfg.sampled_minutes, cfg.linux(), true, cfg.verdicts_prompt, s, now)
    };
    let mut p = String::new();
    let request = batch.prompt.trim();
    p.push_str(&format!("## What they asked for\n{}\n", if request.is_empty() { "No specific request." } else { request }));
    p.push_str(
        "\nLines: #id name [stores] | genres & tags | series | main-story hours | played so far | history | reviews | year | ... | \
         feel and aspects players praised (+), criticized (-) or disagree on (±) | ",
    );
    if cfg.verdicts_prompt {
        p.push_str(crate::prompt::BV_FORMAT);
        p.push_str(" | ");
    }
    p.push_str("what players say\n");
    if let Some(g) = &target.game {
        p.push_str(&format!("\n## Turned down: too {word}\n{}\n", line(g)));
    }
    let rest: Vec<String> = batch.picks.iter().filter(|x| x.id != target.id).filter_map(|x| x.game.as_ref()).map(line).collect();
    if !rest.is_empty() {
        p.push_str(&format!("\n## The rest of the lineup\n{}\n", rest.join("\n")));
    }
    let spare_lines: Vec<String> = spares.iter().filter_map(|x| x.game.as_ref()).map(line).collect();
    p.push_str(&format!("\n## Spares, best first\n{}\n", spare_lines.join("\n")));
    p
}

/// "It's too ___" on a pick: one small AI call chooses a spare without the
/// problem for its slot, and replaces any other pick on screen with the
/// same problem. The word is kept as a light note on the game, and counts
/// as a rejection for shelved and abandoned games. When the call fails, the
/// first spare the word doesn't rule out takes the slot, as it does with
/// no AI set up or on a lineup picked without AI, where there's no call.
/// With no usable spare, the slot is left empty for `recommend::more`.
pub async fn too(
    db: &Db,
    cfg: &Config,
    llm: &dyn LlmBackend,
    batch_id: &str,
    title_id: i64,
    word: &str,
) -> Result<TooOutcome, LineupError> {
    let word = clean_word(word)?;
    let (batch, target) = on_screen(db, cfg.whose(), batch_id, title_id)?;
    owned(&target)?;
    let series = series_map(db, cfg.whose())?;
    // What the AI chooses from: spares that could go on screen beside the
    // rest of the lineup.
    let spares = usable(&batch, &screen_without(&batch, &[title_id], &series));
    let mut ai_error = None;
    // With no AI set up, or on a lineup picked without it, the word alone
    // chooses, and nothing failed.
    let without_ai = batch.ranked_by.is_some() || llm.available().await.is_err();
    let plan: Vec<(i64, Option<i64>)> = if spares.is_empty() {
        vec![(title_id, None)]
    } else if without_ai {
        vec![(title_id, fallback(&batch, &target, &word, &series))]
    } else {
        let req = LlmRequest {
            system: too_system(cfg, &word),
            cached_prefix: None,
            prompt: too_prompt(cfg, &batch, &target, &spares, &word, &series),
            schema: too_schema(),
            model: cfg.pick_model(),
            background: false,
            effort: None,
        };
        let answer = match ai_usage::call(db, llm, &Purpose::new(AiKind::Too).batch(batch_id), req).await {
            Ok(resp) => {
                check_too(&resp.json, &batch, title_id, &series).map_err(|e| format!("the AI's answer didn't fit the lineup ({e})"))
            }
            // Said as the other calls' errors say it, without the kind of error first.
            Err(LlmError::NotAvailable(m)) => Err(m),
            Err(e) => Err(e.to_string()),
        };
        match answer {
            Ok(plan) => plan,
            Err(e) => {
                tracing::warn!("\"It's too {word}\": {e}");
                ai_error = Some(e);
                vec![(title_id, fallback(&batch, &target, &word, &series))]
            }
        }
    };

    let mut changes = vec![];
    let mut swaps = vec![];
    for (i, (out, into)) in plan.into_iter().enumerate() {
        let action = if i == 0 { FeedbackAction::Too } else { FeedbackAction::SameProblem };
        let slot = batch.picks.iter().find(|p| p.id == out).and_then(|p| p.slot).expect("checked: on screen");
        changes.push(LineupChange::Feedback { title_id: out, action, text: Some(word.clone()) });
        changes.push(LineupChange::Slot { title_id: out, slot: None });
        if let Some(id) = into {
            changes.push(LineupChange::Slot { title_id: id, slot: Some(slot) });
        }
        swaps.push(Swap { slot, out, into });
    }
    db.change_lineup(batch_id, &changes)?;
    Ok(TooOutcome { batch: db.pick_batch(cfg.whose(), batch_id)?.expect("the lineup was just read"), swaps, ai_error })
}

#[cfg(test)]
mod tests;
