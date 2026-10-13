//! Second Chance's "Now": for each abandoned game on the page,
//! and each one shelved again for a second try, its player reviews read again
//! when they're over 3 months old, then what the ones written since it
//! was put down say about why, a few games per call. A game whose "Now" was
//! written from the same reasons and reviews is left alone, unless the run
//! is a refresh. Off unless asked for: it uses the LLM.

use super::{Opt, Pass, gog, steam_reviews};
use crate::db::PlayerId;
use crate::db::{AiKind, Failure, GameView, Level};
use crate::enrich::{Progress, Run, Streak};
use crate::llm::LlmBackend;
use crate::second_chance;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;

const OPTIONS: &[Opt] = &[Opt::new("second_chances", "AI second chances").off().note("uses the AI")];

pub struct SecondChances;

/// The review fetches' failures in a row, one count per service, given up
/// after as many as the service's own source allows: with a service down,
/// every request waits out its backoff, and the run would otherwise sit
/// through that for every game on the page.
struct Fetches {
    steam: Streak,
    gog: Streak,
}

impl Fetches {
    fn new() -> Self {
        Self { steam: Streak::new(Some(steam_reviews::GIVE_UP_AFTER)), gog: Streak::new(Some(gog::GIVE_UP_AFTER)) }
    }
}

/// A failed fetch, logged and counted. The stored reviews are used, and
/// after the failure that reaches the limit, for the rest of the run.
fn fetch_failed(run: &Run<'_>, streak: &mut Streak, service: &str, (title_id, name): (i64, &str), e: anyhow::Error) {
    run.error_game(title_id, format!("{service} reviews failed for {name}: {e:#}"));
    if let Some(limit) = streak.failed() {
        run.log_at(
            Level::Error,
            format!("{service} reviews stopped for the rest of this run after {limit} failures in a row; the stored reviews are used"),
        );
    }
}

impl SecondChances {
    /// Fetch the Steam and GOG reviews again for a game whose stored ones
    /// are older than `REVIEWS_FRESH_DAYS` (or on a refresh), unless that
    /// service has given up. A failure is logged and the stored reviews are
    /// used. Whether anything was asked for.
    async fn fresh_reviews(&self, g: &GameView, run: &Run<'_>, fetches: &mut Fetches) -> Result<bool> {
        let fresh_secs = second_chance::REVIEWS_FRESH_DAYS * 86_400;
        let stale = |fetched: Option<i64>| run.refresh || fetched.is_none_or(|t| crate::db::now() - t > fresh_secs);
        let mut asked = false;
        if let Some(a) = g.appid
            && !fetches.steam.given_up()
            && stale(run.db.review_texts_fetched_at(a)?)
        {
            asked = true;
            match crate::steam::app_reviews_with_texts(&run.clients.steam, a, 10).await {
                Ok((score, texts)) => {
                    fetches.steam.succeeded();
                    if let Some(texts) = texts.filter(|t| !t.is_empty()) {
                        run.db.put_reviews(a, score.as_ref())?;
                        run.db.put_review_texts(a, Some(&texts))?;
                        run.count("review_texts");
                    }
                }
                Err(e) => fetch_failed(run, &mut fetches.steam, "Steam", (g.id, &g.name), e),
            }
        }
        for c in g.copies.iter().filter(|c| c.source == "gog") {
            if fetches.gog.given_up() || !stale(run.db.gog_reviews_fetched_at(&c.source_id)?) {
                continue;
            }
            asked = true;
            match run.clients.gogmeta.reviews(&c.source_id, 8).await {
                Ok(r) => {
                    fetches.gog.succeeded();
                    if let Some(r) = r {
                        run.db.put_gog_reviews(&c.source_id, Some(&r))?;
                    }
                }
                Err(e) => fetch_failed(run, &mut fetches.gog, "GOG", (g.id, &g.name), e),
            }
        }
        Ok(asked)
    }
}

#[async_trait]
impl Pass for SecondChances {
    fn options(&self) -> &'static [Opt] {
        OPTIONS
    }
    fn name(&self) -> &'static str {
        "second chances"
    }
    fn lane(&self) -> &'static str {
        AiKind::SecondChances.lane().expect("a job's lane")
    }
    fn wanted(&self, run: &Run<'_>) -> bool {
        run.on("second_chances")
    }

    async fn run(&self, run: &Run<'_>) -> Result<()> {
        let Some(llm) = run.summary_llm else {
            run.log("second chances skipped: no LLM backend available".into());
            return Ok(());
        };
        // Each player's games put down: a game two players put
        // down is listed for both, and its reviews are fetched once.
        let mut listed: Vec<(PlayerId, GameView)> = vec![];
        for p in run.db.players()? {
            listed.extend(run.db.all_games(p.id)?.into_iter().filter(|g| run.includes(g) && second_chance::checked(g)).map(|g| (p.id, g)));
        }
        listed.sort_by_key(|(p, g)| (g.name.to_lowercase(), *p));
        let mut progress = Progress {
            total: listed.len(),
            done: 0,
            current: None,
            message: format!("Reading recent reviews of {} put down", crate::enrich::plural(listed.len(), "game", "games")),
        };
        run.sink.update(&progress);
        // The limit counts the games this run works on, those whose reviews
        // it fetches and those whose "Now" it writes, as the lanes count the
        // titles they take. It applies as the games are reached, so none
        // past it is fetched.
        let mut fetches = Fetches::new();
        let mut pending: Vec<(PlayerId, GameView)> = vec![];
        let mut taken = 0;
        for (p, g) in &listed {
            if run.limit.is_some_and(|limit| taken >= limit) {
                break;
            }
            run.check_cancelled()?;
            progress.current = Some(g.name.clone());
            run.sink.update(&progress);
            let asked = self.fresh_reviews(g, run, &mut fetches).await?;
            progress.done += 1;
            // Read again, with the reviews just fetched.
            let due = run.db.game(*p, g.id)?.filter(|g| second_chance::needs_now(g, run.refresh));
            if asked || due.is_some() {
                taken += 1;
            }
            // Other backlog players' line goes to the AI only under the one
            // switch for what the AI reads of their verdicts.
            pending.extend(due.map(|mut g| {
                if !run.cfg.verdicts_prompt {
                    g.backlog = None;
                }
                (*p, g)
            }));
        }
        let checked = progress.done;
        let mut progress = Progress {
            total: pending.len(),
            done: 0,
            current: None,
            message: format!(
                "Checking why you stopped against reviews since ({}, up to {} per call)",
                crate::enrich::plural(pending.len(), "game", "games"),
                second_chance::BATCH
            ),
        };
        run.sink.update(&progress);
        if checked < listed.len() {
            run.log(format!(
                "{} of the first {checked} of {} put down {} their Now written; the limit leaves the rest for another run",
                pending.len(),
                crate::enrich::plural(listed.len(), "game", "games"),
                if pending.len() == 1 { "needs" } else { "need" }
            ));
        } else {
            run.log(format!(
                "{} of {} put down {} their Now written (the rest are up to date or have no reviews since you stopped)",
                pending.len(),
                crate::enrich::plural(listed.len(), "game", "games"),
                if pending.len() == 1 { "needs" } else { "need" }
            ));
        }
        // A call is one player's games, since what it writes is theirs.
        let mut by_player: Vec<(PlayerId, Vec<GameView>)> = vec![];
        for (p, g) in pending.iter().cloned() {
            match by_player.iter_mut().find(|(q, _)| *q == p) {
                Some((_, games)) => games.push(g),
                None => by_player.push((p, vec![g])),
            }
        }
        // Each player's "Now" is written on the AI account they use;
        // one with none waits until they have one.
        let mut backends: HashMap<PlayerId, (Option<Box<dyn LlmBackend>>, Option<String>)> = HashMap::new();
        let players = run.db.players()?;
        by_player.retain(|(p, games)| {
            let Ok(theirs) = run.cfg.as_player(run.db, *p) else { return false };
            if theirs.no_ai_account {
                let name = players.iter().find(|x| x.id == *p).map_or("A player", |x| x.name.as_str());
                run.log(format!(
                    "{name} has no AI account, so the Now of their {} waits until they have one",
                    crate::enrich::plural(games.len(), "game", "games")
                ));
                return false;
            }
            let own = (theirs.ai_account != run.cfg.ai_account).then(|| crate::llm::summary_backend(&theirs));
            backends.insert(*p, (own, theirs.summary_model()));
            true
        });
        let chunks: Vec<(PlayerId, &[GameView])> =
            by_player.iter().flat_map(|(p, games)| second_chance::batches(games).into_iter().map(move |c| (*p, c))).collect();
        let options = run.options_on(OPTIONS);
        super::ai_batches(
            run,
            AiKind::SecondChances,
            &chunks,
            |(p, chunk)| {
                let (own, model) = &backends[p];
                second_chance::generate(run.db, *p, own.as_deref().unwrap_or(llm), model.clone(), chunk)
            },
            |(_, chunk), result| {
                let fail = |g: &GameView, error: String| {
                    run.fail(Failure {
                        lane: self.lane().into(),
                        source: self.name().into(),
                        options: options.clone(),
                        title_id: Some(g.id),
                        name: Some(g.name.clone()),
                        error,
                        at: 0,
                    })
                };
                match result {
                    Ok(written) => {
                        run.add("second_chances", written.written);
                        for g in chunk.iter().filter(|g| written.missing.contains(&g.id)) {
                            fail(g, "the AI's reply left this game out".into());
                        }
                    }
                    Err(e) => {
                        for g in chunk.iter() {
                            fail(g, format!("{e:#}"));
                        }
                    }
                }
                progress.done += chunk.len();
                run.sink.update(&progress);
            },
        )
        .await?;
        Ok(())
    }
}
