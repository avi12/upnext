//! Versus' "Help me decide": the AI picks one of two games someone owns,
//! for them, and says why in a few sentences. Its prompt and its check of
//! the answer live here, as every other AI feature's do, so they are tested
//! without HTTP; the route only reads the request and says how it went.

use crate::ai_usage::{self, Purpose};
use crate::config::Config;
use crate::db::{AiKind, Db, GameView, PlayerId};
use crate::llm::{LlmBackend, LlmError, LlmRequest, Usage};
use crate::recommend::MAX_PROMPT_CHARS;
use serde_json::{Value, json};

#[derive(Debug, thiserror::Error)]
pub enum DecideError {
    /// Not exactly two different games.
    #[error("{0}")]
    Bad(String),
    #[error("{0}")]
    NotFound(String),
    /// No AI is set up, or it can't run: said rather than tried.
    #[error("{0}")]
    NotAvailable(String),
    /// The call failed, or its answer wasn't usable.
    #[error("{0}")]
    Failed(String),
    /// Prints the whole chain itself, so it has no `source`.
    #[error("{0:#}")]
    Other(anyhow::Error),
}

impl From<anyhow::Error> for DecideError {
    fn from(e: anyhow::Error) -> Self {
        DecideError::Other(e)
    }
}

/// The AI's choice, and what the call was.
#[derive(Debug, Clone)]
pub struct Decision {
    pub pick_id: i64,
    pub reason: String,
    pub model: Option<String>,
    pub usage: Option<Usage>,
    pub cost_usd: Option<f64>,
    pub api_usd: Option<f64>,
}

const SYSTEM: &str = "Someone can't decide between two games they own. Pick the one that fits them better right now \
and explain why in two or three short, casual sentences that compare the two directly (length, mood, difficulty, \
what players say). About me is what they've told UpNext and what it counted from how they play: weigh it, but \
what they asked for comes first. Use pick_id from the list. No hype, no hedging. Return JSON only.";

/// Ask `llm` which of the games `ids` player `p` should play, with what
/// they asked for, `ask`, when they said.
pub async fn decide(
    db: &Db,
    cfg: &Config,
    llm: &dyn LlmBackend,
    p: PlayerId,
    ids: &[i64],
    ask: Option<&str>,
) -> Result<Decision, DecideError> {
    if ids.len() != 2 || ids[0] == ids[1] {
        return Err(DecideError::Bad("pass exactly two different game ids".into()));
    }
    // What they asked for goes to the model, held to the picks' limit:
    // an unbounded paste would make a costly call, paid for by the AI
    // account's owner when it's shared.
    let chars = ask.map_or(0, |a| a.chars().count());
    if chars > MAX_PROMPT_CHARS {
        return Err(DecideError::Bad(format!("the request is {chars} characters; keep it to {MAX_PROMPT_CHARS}")));
    }
    let mut games = vec![];
    for id in ids {
        games.push(db.game(p, *id)?.ok_or_else(|| DecideError::NotFound(format!("game {id} not found")))?);
    }
    // About me, as picks read it.
    let all = db.all_games_shared(p)?;
    let about_me = crate::taste::Profile::load(db, p, &all)?.about_me(&all, crate::db::now());
    let prompt = prompt(cfg, about_me, ask, &games);
    let mut system = String::from(SYSTEM);
    if cfg.verdicts_prompt {
        system.push(' ');
        system.push_str(crate::prompt::BV_RULE);
    }
    let schema = json!({"type": "object", "properties": {"pick_id": {"type": "integer"}, "reason": {"type": "string"}},
        "required": ["pick_id", "reason"], "additionalProperties": false});
    // An AI feature: with no AI set up, say so rather than try.
    if let Err(e) = llm.available().await {
        return Err(DecideError::NotAvailable(e.reason()));
    }
    let req = LlmRequest { system, cached_prefix: None, prompt, schema, model: cfg.pick_model(), background: false, effort: None };
    let resp =
        ai_usage::call(db, llm, &Purpose::new(AiKind::Decide), req).await.map_err(|e: LlmError| DecideError::Failed(e.to_string()))?;
    let pick_id = resp.json.get("pick_id").and_then(Value::as_i64).filter(|i| ids.contains(i));
    let reason = resp.json.get("reason").and_then(Value::as_str).unwrap_or("").trim().to_string();
    match pick_id {
        Some(pick_id) if !reason.is_empty() => {
            Ok(Decision { pick_id, reason, model: resp.model, usage: resp.usage, cost_usd: resp.cost_usd, api_usd: resp.api_usd })
        }
        _ => Err(DecideError::Failed("the AI did not return a usable answer; try again".into())),
    }
}

/// The request: About me, what they asked for, and the two games' lines.
fn prompt(cfg: &Config, about_me: String, ask: Option<&str>, games: &[GameView]) -> String {
    let mut prompt = about_me;
    prompt.push('\n');
    if let Some(p) = ask.map(str::trim).filter(|p| !p.is_empty()) {
        prompt.push_str(&format!("What they asked for: {p}\n\n"));
    }
    prompt.push_str(
        "The two games (#id | name | genres | tags | themes | length | reviews | ... | feel and aspects players praised (+), \
         criticized (-) or disagree on (±) | ",
    );
    // Other backlog players' verdicts only under the one switch.
    if cfg.verdicts_prompt {
        prompt.push_str(crate::prompt::BV_FORMAT);
        prompt.push_str(" | ");
    }
    prompt.push_str("what players say):\n");
    for g in games {
        prompt.push_str(&crate::prompt::candidate_line(
            g,
            cfg.sampled_minutes,
            cfg.linux(),
            true,
            cfg.verdicts_prompt,
            None,
            crate::db::now(),
        ));
        prompt.push('\n');
    }
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recommend::tests::{id_of, library};
    use crate::testutil::{NoAi, ScriptedLlm};

    /// The request carries what they asked for and both games; the answer
    /// must name one of the two, with a reason.
    #[tokio::test]
    async fn the_ai_picks_one_of_the_two() {
        let (_dir, db, cfg) = library();
        let (hades, celeste) = (id_of(&db, "Hades"), id_of(&db, "Celeste"));
        let llm = ScriptedLlm::new(vec![
            Box::new(move |_: &LlmRequest| Ok(json!({"pick_id": celeste, "reason": "Shorter, and it fits tonight."}))),
            Box::new(|_: &LlmRequest| Ok(json!({"pick_id": 999_999, "reason": "Neither."}))),
        ]);
        let d = decide(&db, &cfg, &llm, PlayerId::FIRST, &[hades, celeste], Some("  something short  ")).await.unwrap();
        assert_eq!((d.pick_id, d.reason.as_str()), (celeste, "Shorter, and it fits tonight."));
        let asked = llm.asked.lock().unwrap()[0].clone();
        assert!(asked.prompt.contains("What they asked for: something short\n"), "{}", asked.prompt);
        assert!(asked.prompt.contains(&format!("#{hades} Hades")) && asked.prompt.contains(&format!("#{celeste} Celeste")));

        let err = decide(&db, &cfg, &llm, PlayerId::FIRST, &[hades, celeste], None).await.unwrap_err();
        assert!(matches!(&err, DecideError::Failed(m) if m.contains("usable answer")), "{err:?}");
    }

    /// Two games, both there, a request no longer than a pick's (B-AI-7),
    /// and an AI to ask, or no call is made.
    #[tokio::test]
    async fn what_is_refused_before_any_call() {
        let (_dir, db, cfg) = library();
        let hades = id_of(&db, "Hades");
        let none = ScriptedLlm::new(vec![]);
        let err = decide(&db, &cfg, &none, PlayerId::FIRST, &[hades, hades], None).await.unwrap_err();
        assert!(matches!(err, DecideError::Bad(_)), "{err:?}");
        let err = decide(&db, &cfg, &none, PlayerId::FIRST, &[hades, 999_999], None).await.unwrap_err();
        assert!(matches!(err, DecideError::NotFound(_)), "{err:?}");
        let celeste = id_of(&db, "Celeste");
        let longest = "a".repeat(MAX_PROMPT_CHARS);
        let err = decide(&db, &cfg, &none, PlayerId::FIRST, &[hades, celeste], Some(&format!("{longest}a"))).await.unwrap_err();
        assert!(matches!(&err, DecideError::Bad(m) if m == "the request is 2001 characters; keep it to 2000"), "{err:?}");
        let err = decide(&db, &cfg, &NoAi, PlayerId::FIRST, &[hades, id_of(&db, "Celeste")], None).await.unwrap_err();
        assert!(matches!(&err, DecideError::NotAvailable(m) if m == "no AI set up"), "{err:?}");
        assert!(db.ai_usage_rows(0).unwrap().is_empty());
    }
}
