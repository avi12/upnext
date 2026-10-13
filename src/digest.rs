//! Review digests: per title, "People say" (a critical look at what the
//! reviews say: what sets the game apart, what it does well, where it falls
//! short), "Vibes" (what playing it feels like, in the reviewers' images),
//! and review tags from the fixed vocabulary in `review_tags`. One model call
//! writes all three from the same reviews, so they can't disagree.

use crate::ai_usage::{self, Purpose};
use crate::db::{AiKind, Db, GameView, ReviewText, Status};
use crate::llm::{LlmBackend, LlmRequest};
use crate::review_tags::{self, Dropped, Reply};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};

/// At most this many games per call, and at most `BATCH_CHARS` of reviews:
/// whole reviews vary a lot in length, so a batch is sized by its text.
pub const BATCH: usize = 8;
const BATCH_CHARS: usize = 48_000;

/// The version of the prompt and vocabulary. `enrich --with digests` rewrites
/// every summary written with an older one; summaries from before versions
/// were recorded count as 1. Bump it whenever the prompt or the vocabulary
/// changes.
pub const DIGEST_VERSION: i64 = 4;

/// Reviews per side, how many of the most helpful go in whole, and the
/// length the others are cut near.
const POSITIVE: usize = 6;
const NEGATIVE: usize = 4;
const WHOLE: usize = 2;
const REVIEW_CHARS: usize = 600;
/// A review shorter than this is usually "great game"; when a title has
/// nothing longer, its reviews down to `MIN_CHARS_THIN` are all there is
/// ("Very boring game :( pacing is glacial.").
const MIN_CHARS: usize = 40;
const MIN_CHARS_THIN: usize = 12;
/// Caps on what is stored, in case a reply runs away: the digest goes into
/// every pick prompt.
pub(crate) const DIGEST_MAX_CHARS: usize = 600;
pub(crate) const VIBES_MAX_CHARS: usize = 800;

pub(crate) fn schema() -> Value {
    let strings = |values: Vec<String>| json!({"type": "array", "items": {"type": "string", "enum": values}});
    let aspects = || strings(review_tags::ASPECTS.iter().map(|a| a.to_string()).collect());
    json!({
        "type": "object",
        "properties": {"digests": {"type": "array", "items": {"type": "object",
            "properties": {
                "id": {"type": "integer"},
                "vibes": {"type": "string"},
                "digest": {"type": "string"},
                "facets": strings(review_tags::facet_values()),
                "praised": aspects(),
                "criticized": aspects(),
                "divisive": aspects(),
                "notes": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["id", "vibes", "digest", "facets", "praised", "criticized", "divisive", "notes"],
            "additionalProperties": false}}},
        "required": ["digests"], "additionalProperties": false
    })
}

fn system() -> String {
    format!(
        "You write about games for a personal game-recommendation tool, from the player reviews given for each game \
and only from them.

For each game return:
- vibes: what the game feels like to play, in 3 or 4 sentences and at most 70 words, with some cheek. Write like a \
friend who has played it and can't help grinning while describing it: playful, a little irreverent, happy to tease the \
game about its quirks, with the odd wry aside or unexpected comparison. Paint its world, tone and texture the way the \
reviewers experience them, and borrow their vivid words, jokes and images freely, woven into your own sentences rather \
than quoted. Say what you get to do and how that feels. Describe the game as it plays today. Never describe how it \
used to be: no launch troubles, old versions or \"it once was\". If a comeback is its story, say only that it is a \
comeback that keeps getting better. Use plain words anyone would get: no gamer jargon (i-frames, DPS, \
procs) and no references only fans of another game would catch. Flaws belong only as part of the feel, never as a \
closing list, and when the game is loved despite them, make it clear they are part of its charm. Vary the rhythm, a \
long sentence and then a short punchy one, and don't open every game the same way. The voice to aim for, written for Fallout: New Vegas (match the voice, never \
reuse its phrases): \"A dusty, chaotic, brutal world awaits in Fallout: New Vegas. The desolate Mojave contrasts with \
...interesting... people who feel like they have been there for a long time. You are free to be a hero or a villain, \
but your choices have consequences. It might have some jank, but... the good kind.\" Leave the verdict to the digest: no scores, \
hours, prices, pros and cons, or who likes it and who doesn't, and never mention reviews, reviewers, players or fans. \
Every name, place and detail must come from the reviews: no catchphrases, lore or facts you know from elsewhere. \
Write vibes whenever the reviews say anything about what playing it is like, even when they disagree about whether \
it's good. Leave it empty only when they say nothing about the experience.
- digest: a critical look in at most 40 words: what sets the game apart, what it does well, and where it falls short. \
Be concrete (mechanics, length, difficulty, story, performance, bugs), name the divisive points, no marketing tone, \
no restating scores. Leave the feel to vibes. Weigh recent reviews for the game's current state, since updates can fix \
old complaints. If the reviews are too thin, say what little they indicate.
- tags, using only these values:
{}
Evidence for tags:
- Tag something when the reviews support it: at least two say it, or one says it plainly and none disagree.
- A scale needs a review that speaks to it directly: how long a sitting lasts for sessions, when it grabbed them for \
hook, whether it needs full attention for focus. A middle value (moderate, medium, steady, light) needs that evidence \
as much as an end does; never use one because nothing points either way. Set at most five scales: the ones the \
reviews speak to most plainly, strongest first. Fewer is fine, and most games get 2 to 4.
- An aspect is divisive when reviews clearly disagree about it.
- When reviews are thin, return fewer tags, or none. Don't guess from the game's name, genre or reputation, \
or from what you know about it. The same goes for vibes and the digest.

Each review starts with a marker: + recommends the game, - doesn't, 4/5 is a star rating, 12h is the reviewer's \
playtime, and 2016 the year it was written. The most helpful reviews are given whole; the rest are cut.
Return JSON only.",
        review_tags::prompt_vocabulary()
    )
}

/// Firmer rules for models on the OpenAI-compatible backend (local models
/// such as Qwen). On a 200-title test for the shared data, Qwen with thinking off kept
/// the tags close to Sonnet's but ran Vibes long (three in four past 70
/// words), talked about players and reviewers, and set more scales, more of
/// them middle values. Sonnet follows the base prompt, so this is added only
/// there: the base prompt and `DIGEST_VERSION` stay as they are.
const FIRM: &str = "Firm rules, which come before everything above:
- The digests list has as many entries as there are games below, each under its game's id. Leave none out, even \
when its reviews are thin.
- vibes: 3 or 4 sentences and 40 to 60 words. Count the words before answering, and drop a sentence rather than go \
past 60.
- digest: at most 40 words.
- In vibes, never use the words review, reviewer, player, players, fan, fans, people, community, critic or audience, \
and never say what anyone thinks, says, rates or recommends. Describe the game itself.
- Scales: leave a scale out unless a review speaks to it plainly. Most games get 2 or 3. A middle value needs a \
review that says it in those terms.
- Mood and skills: set them whenever the reviews describe how the game feels or what it asks of you.";

/// Cut `text` at the last sentence end within `max` characters, or failing
/// that (no sentence end past the first half) at a word, marked with "…".
pub(crate) fn cut_near(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let head: String = text.chars().take(max).collect();
    let chars: Vec<(usize, char)> = head.char_indices().collect();
    let sentence_end = chars
        .iter()
        .enumerate()
        .rev()
        .find(|&(i, &(_, c))| matches!(c, '.' | '!' | '?' | '…') && chars.get(i + 1).is_none_or(|&(_, n)| n.is_whitespace()));
    if let Some((i, &(at, c))) = sentence_end
        && i >= max / 2
    {
        return head[..at + c.len_utf8()].to_string();
    }
    let cut = head.rfind(char::is_whitespace).filter(|&at| at > 0).unwrap_or(head.len());
    format!("{}…", head[..cut].trim_end())
}

/// The reviews worth sending, most helpful first.
fn usable(g: &GameView) -> Vec<&ReviewText> {
    let at_least = |n: usize| g.review_texts.iter().filter(|r| r.text.trim().chars().count() >= n).collect::<Vec<_>>();
    let long = at_least(MIN_CHARS);
    if long.is_empty() { at_least(MIN_CHARS_THIN) } else { long }
}

/// A title the summaries are for: a game with player reviews to summarize.
pub fn wanted(g: &GameView) -> bool {
    g.status != Status::NotGame && has_usable_reviews(g)
}

/// No summary yet, or one from an older prompt. A summary from UpNext's
/// shared data stays until newer data arrives, whatever its
/// version, rather than every install writing its own when the prompt
/// changes.
pub fn outdated(g: &GameView) -> bool {
    if g.seed_date.is_some() && g.has_digest {
        return false;
    }
    !g.has_digest
        || g.digest_version.unwrap_or(1) < DIGEST_VERSION
        // Written before the scales were capped. The cap changed the prompt
        // without a new version: only these titles need rewriting.
        || g.review_tags.as_ref().is_some_and(|t| t.scales.len() > review_tags::MAX_SCALES)
}

/// Whether a title has any review a summary can be written from.
pub fn has_usable_reviews(g: &GameView) -> bool {
    !usable(g).is_empty()
}

/// The year a review was posted, from Unix seconds.
pub(crate) fn year(posted: i64) -> Option<i32> {
    use chrono::Datelike;
    chrono::DateTime::from_timestamp(posted, 0).map(|d| d.year())
}

/// The reviews a summary is written from: the most helpful ones with real
/// text, a few of each side so complaints aren't drowned out. The `WHOLE`
/// most voted go in whole; the rest are cut at a sentence end near
/// `REVIEW_CHARS`.
pub fn review_block(g: &GameView) -> String {
    let useful = usable(g);
    let (mut up, mut down): (Vec<_>, Vec<_>) =
        useful.into_iter().partition(|r| r.positive != Some(false) && r.rating.is_none_or(|x| x >= 3.0));
    up.truncate(POSITIVE);
    down.truncate(NEGATIVE);
    let chosen: Vec<&ReviewText> = up.into_iter().chain(down).collect();
    let mut by_votes: Vec<usize> = (0..chosen.len()).collect();
    by_votes.sort_by_key(|&i| std::cmp::Reverse(chosen[i].votes));
    let whole: Vec<usize> = by_votes.into_iter().take(WHOLE).collect();
    let mut s = String::new();
    for (i, r) in chosen.iter().enumerate() {
        let mut mark = match (r.positive, r.rating) {
            (Some(true), _) => "+".to_string(),
            (Some(false), _) => "-".to_string(),
            (None, Some(x)) => format!("{x:.0}/5"),
            _ => "?".to_string(),
        };
        if let Some(h) = r.hours {
            mark.push_str(&format!(", {h}h"));
        }
        if let Some(y) = r.posted.and_then(year) {
            mark.push_str(&format!(", {y}"));
        }
        let text = crate::llm::one_line(&r.text, usize::MAX);
        let text = if whole.contains(&i) { text } else { cut_near(&text, REVIEW_CHARS) };
        s.push_str(&format!("  [{mark}] {text}\n"));
    }
    s
}

/// The request for one batch of titles; `firm` adds the rules in `FIRM`.
/// At low effort: Haiku 5.5 thinks at medium by default, and on a batch of
/// 8 that could spend all of `max_tokens` thinking and write nothing.
pub fn request(chunk: &[GameView], model: Option<String>, firm: bool) -> LlmRequest {
    let mut user = String::from("Write the vibes, digest and tags for each game below.\n\n");
    for g in chunk {
        user.push_str(&format!("### #{} {}\n{}\n", g.id, crate::llm::name_line(&g.name), review_block(g)));
    }
    let system = if firm { format!("{}\n\n{FIRM}", system()) } else { system() };
    LlmRequest { system, cached_prefix: None, prompt: user, schema: schema(), model, background: true, effort: Some("low") }
}

/// Split titles into calls: at most `BATCH` titles and about `BATCH_CHARS`
/// of reviews each, though a title with more than that still gets a call.
pub fn batches(titles: &[GameView]) -> Vec<&[GameView]> {
    let mut rest = titles;
    batch_sizes(titles.iter().map(|g| review_block(g).len()))
        .into_iter()
        .map(|n| {
            let (call, after) = rest.split_at(n);
            rest = after;
            call
        })
        .collect()
}

/// How many titles each call gets, for titles whose reviews are this long.
pub fn batch_sizes(lengths: impl IntoIterator<Item = usize>) -> Vec<usize> {
    let mut out = vec![];
    let (mut size, mut chars) = (0, 0);
    for n in lengths {
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
}

/// Whether a summary's text holds nothing: empty, too short to say
/// anything, or a word a model wrote where the text belongs. With the
/// schema's keys out of order, models wrote "placeholder" and "x" and the
/// app saved them.
pub(crate) fn stand_in(text: &str) -> bool {
    let word = text.trim().trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    word.chars().count() < 3 || matches!(word.as_str(), "placeholder" | "tbd" | "todo" | "n/a" | "none" | "null" | "empty" | "lorem ipsum")
}

/// What one call stored.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Written {
    pub digests: usize,
    /// Tag values the vocabulary or the caps turned away.
    pub dropped: Dropped,
    /// Titles the reply left out, or answered without a summary.
    pub missing: Vec<i64>,
}

/// Generate digests, vibes and review tags for `titles` (each must have
/// review texts). A reply item that is unusable is skipped; the rest are
/// stored, and the titles without one are listed as missing.
pub async fn generate(db: &Db, llm: &dyn LlmBackend, model: Option<String>, titles: &[GameView]) -> Result<Written> {
    generate_with(db, llm, model, titles, None).await
}

/// [`generate`], with `note` after the instructions. The shared data's build
/// uses it to ask again for summaries that copied a review.
pub async fn generate_with(
    db: &Db,
    llm: &dyn LlmBackend,
    model: Option<String>,
    titles: &[GameView],
    note: Option<&str>,
) -> Result<Written> {
    let mut written = Written::default();
    let firm = llm.name() == "openai";
    for chunk in batches(titles) {
        let purpose = Purpose::new(AiKind::ReviewSummaries).items(chunk.len());
        let mut req = request(chunk, model.clone(), firm);
        if let Some(note) = note {
            req.system = format!("{}\n\n{note}", req.system);
        }
        let resp = ai_usage::call(db, llm, &purpose, req).await.context("digest generation")?;
        let items = resp.json.get("digests").and_then(Value::as_array).ok_or_else(|| anyhow!("digest output has no \"digests\" list"))?;
        let model_name = resp.model.clone().unwrap_or_else(|| "unknown".into());
        let mut stored: Vec<i64> = vec![];
        for item in items {
            let id = match item.get("id") {
                Some(Value::Number(n)) => n.as_i64(),
                Some(Value::String(s)) => s.trim_start_matches('#').parse().ok(),
                _ => None,
            };
            let Some(id) = id.filter(|id| chunk.iter().any(|g| g.id == *id)) else { continue };
            let text = |field: &str, max: usize| -> String {
                item.get(field).and_then(Value::as_str).unwrap_or_default().trim().chars().take(max).collect()
            };
            let digest = text("digest", DIGEST_MAX_CHARS);
            if stand_in(&digest) {
                continue;
            }
            let vibes = Some(text("vibes", VIBES_MAX_CHARS)).filter(|v| !stand_in(v)).unwrap_or_default();
            let (reply, malformed) = Reply::from_json(item);
            let (tags, dropped) = reply.validate();
            written.dropped += dropped;
            written.dropped.unknown += malformed;
            db.put_digest(id, &digest, &vibes, &tags, &model_name, DIGEST_VERSION)?;
            if !stored.contains(&id) {
                stored.push(id);
                written.digests += 1;
            }
        }
        written.missing.extend(chunk.iter().map(|g| g.id).filter(|id| !stored.contains(id)));
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ImportedGame;
    use crate::db::PlayerId;
    use crate::llm::openai_compat::OpenAiCompatBackend;
    use crate::review_tags::{Aspect, Verdict};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn reviews_are_cut_at_a_sentence_end() {
        let long = format!("{} Last sentence runs past the limit and keeps going for a while.", "Good game. ".repeat(REVIEW_CHARS / 11));
        let cut = cut_near(&long, REVIEW_CHARS);
        assert!(cut.ends_with("Good game."), "{cut}");
        assert!(cut.chars().count() <= REVIEW_CHARS);

        let one_sentence = "word ".repeat(150);
        let cut = cut_near(one_sentence.trim(), REVIEW_CHARS);
        assert!(cut.ends_with("word…"), "a cut mid-sentence falls on a word: {cut}");

        // A sentence end in the first half is too early to cut at.
        let early = format!("Short. {}", "word ".repeat(150));
        assert!(cut_near(early.trim(), REVIEW_CHARS).ends_with('…'));
        // "3.5" is not a sentence end.
        assert_eq!(cut_near("Rated 3.5 by me", 10), "Rated 3.5…");
        assert_eq!(cut_near("Short enough.", REVIEW_CHARS), "Short enough.");
    }

    fn review(positive: Option<bool>, rating: Option<f64>, hours: Option<i64>, text: &str) -> ReviewText {
        ReviewText { source: "steam".into(), positive, rating, votes: 0, hours, title: None, text: text.into(), posted: None }
    }

    /// Two titles in a fresh database.
    fn db_with_two_games() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("upnext.db"), 120).unwrap();
        let game = |id: &str, name: &str| ImportedGame {
            source: "epic".into(),
            source_id: id.into(),
            launcher: "heroic".into(),
            name: name.into(),
            ..Default::default()
        };
        db.sync_source(PlayerId::FIRST, "heroic", "epic", &[game("a", "Quiet Farm"), game("b", "Iron Keep")]).unwrap();
        (dir, db)
    }

    /// The two games with made-up reviews: more than fit on either side, one
    /// too short to use, a long one, and GOG star ratings.
    fn games() -> Vec<GameView> {
        let (_dir, db) = db_with_two_games();
        let mut all = db.all_games(PlayerId::FIRST).unwrap();
        all.sort_by_key(|g| g.name.clone());
        let [mut keep, mut farm]: [GameView; 2] = all.try_into().unwrap();
        farm.review_texts = (1..=8)
            .map(|i| {
                review(Some(true), None, Some(i * 3), &format!("Review {i}: calm evenings of planting and fishing, a lovely slow loop."))
            })
            .chain(
                (1..=5)
                    .map(|i| review(Some(false), None, Some(2), &format!("Complaint {i}: the first hours drag and the menus are clumsy."))),
            )
            .chain([review(Some(true), None, None, "Too short.")])
            .collect();
        keep.review_texts = vec![
            review(
                Some(true),
                None,
                Some(40),
                &format!("{}Worth it.", "Hard but fair, every death teaches you something new. ".repeat(12)),
            ),
            review(None, Some(2.0), None, "Brutal difficulty spikes and a save system that punishes you."),
            review(None, Some(5.0), None, "Punishing, precise, and the boss fights are the best I have played."),
        ];
        vec![farm, keep]
    }

    #[test]
    fn short_reviews_count_only_when_there_is_nothing_longer() {
        let mut g = games().remove(1);
        let long = "Punishing, precise, and the boss fights are the best I have played.";
        g.review_texts =
            vec![review(Some(false), None, Some(1), "Very boring game :( pacing is glacial."), review(Some(true), None, None, "ok")];
        assert!(has_usable_reviews(&g));
        assert_eq!(review_block(&g), "  [-, 1h] Very boring game :( pacing is glacial.\n");
        g.review_texts.push(review(Some(true), None, None, long));
        assert_eq!(review_block(&g), format!("  [+] {long}\n"), "a long review crowds out the short ones");
        g.review_texts = vec![review(Some(true), None, None, "10/10")];
        assert!(!has_usable_reviews(&g) && review_block(&g).is_empty(), "nothing to write a summary from");
    }

    #[test]
    fn the_most_helpful_reviews_go_in_whole() {
        let mut g = games().remove(1);
        let long = |n: usize| format!("{}The end.", "A long and vivid sentence about the game. ".repeat(n));
        let voted =
            |votes: i64, posted: Option<i64>, text: String| ReviewText { votes, posted, ..review(Some(true), None, Some(5), &text) };
        g.review_texts =
            vec![voted(3, None, long(30)), voted(900, Some(1_318_291_200), long(40)), voted(2, None, long(30)), voted(400, None, long(50))];
        let block = review_block(&g);
        let lines: Vec<&str> = block.lines().collect();
        assert!(!lines[0].ends_with("The end.") && lines[0].len() < 620 && lines[2].len() < 620, "the rest are cut: {block}");
        assert!(lines[1].ends_with("The end.") && lines[3].ends_with("The end."), "the two most voted are whole: {block}");
        assert!(lines[1].starts_with("  [+, 5h, 2011] "), "the year it was written: {}", lines[1]);
    }

    #[test]
    fn batches_follow_the_length_of_the_reviews() {
        let mut games: Vec<GameView> = (0..20).map(|_| games().remove(0)).collect();
        let sizes = |b: Vec<&[GameView]>| b.iter().map(|c| c.len()).collect::<Vec<_>>();
        assert_eq!(sizes(batches(&games)), [BATCH, BATCH, 4], "short reviews: full batches");
        let huge = "word ".repeat(BATCH_CHARS / 8);
        for g in games.iter_mut().take(3) {
            g.review_texts = vec![ReviewText { votes: 1, ..review(Some(true), None, None, &huge) }];
        }
        let b = sizes(batches(&games));
        assert_eq!(b[..2], [1, 1], "a title with long reviews gets a call of its own: {b:?}");
        assert_eq!(b.iter().sum::<usize>(), games.len());
        assert!(batches(&[]).is_empty());
    }

    #[test]
    fn the_request_is_stable() {
        let req = request(&games(), Some("sonnet".into()), false);
        insta::assert_snapshot!("digest_system", req.system);
        insta::assert_snapshot!("digest_prompt", req.prompt);
        insta::assert_snapshot!("digest_schema", serde_json::to_string_pretty(&req.schema).unwrap());
    }

    #[test]
    fn local_models_get_the_firm_rules_after_the_prompt() {
        let base = request(&games(), None, false);
        let firm = request(&games(), None, true);
        assert_eq!(firm.system, format!("{}\n\n{FIRM}", base.system));
        assert_eq!(firm.prompt, base.prompt);
        assert_eq!(firm.schema, base.schema);
    }

    /// One call per batch, each recorded with the titles it covered.
    #[tokio::test]
    async fn each_batch_is_recorded() {
        let (_dir, db) = db_with_two_games();
        let mut titles = db.all_games(PlayerId::FIRST).unwrap();
        let long = "word ".repeat(BATCH_CHARS / 8);
        for g in titles.iter_mut() {
            g.review_texts = vec![ReviewText { votes: 1, ..review(Some(true), None, None, &long) }];
        }
        assert_eq!(batches(&titles).len(), 2, "too long to share a call");
        let server = MockServer::start().await;
        let completion = json!({"model": "fake-model", "usage": {"prompt_tokens": 5000, "completion_tokens": 10},
            "choices": [{"finish_reason": "stop", "message": {"role": "assistant", "content": "{\"digests\": []}"}}]});
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(completion))
            .expect(2)
            .mount(&server)
            .await;
        let llm = OpenAiCompatBackend::new(format!("{}/v1", server.uri()), Some("fake-model".into()), None);

        generate(&db, &llm, None, &titles).await.unwrap();
        let rows = db.ai_usage_rows(0).unwrap();
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(
            (rows[0].kind, rows[0].backend.as_str(), rows[0].model.as_deref()),
            (AiKind::ReviewSummaries, "openai", Some("fake-model"))
        );
        assert_eq!((rows[0].totals.calls, rows[0].totals.items, rows[0].totals.usage.input_tokens), (2, 2, 10_000));
    }

    #[tokio::test]
    async fn a_reply_is_validated_and_stored() {
        let (_dir, db) = db_with_two_games();
        let titles = db.all_games(PlayerId::FIRST).unwrap();
        let id = |name: &str| titles.iter().find(|g| g.name == name).unwrap().id;
        let (farm, keep) = (id("Quiet Farm"), id("Iron Keep"));

        let reply =
            crate::testutil::fixture("llm/digest_reply.json").replace("\"FARM\"", &farm.to_string()).replace("#KEEP", &format!("#{keep}"));
        let content = serde_json::to_string(&reply).unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                format!(r#"{{"model":"fake-model","choices":[{{"finish_reason":"stop","message":{{"role":"assistant","content":{content}}}}}]}}"#),
                "application/json",
            ))
            .mount(&server)
            .await;
        let llm = OpenAiCompatBackend::new(format!("{}/v1", server.uri()), Some("fake-model".into()), None);

        let written = generate(&db, &llm, None, &titles).await.unwrap();
        assert_eq!(written.digests, 2, "the item for an unknown game is skipped");
        assert_eq!(written.dropped, Dropped { unknown: 4, over_cap: 3 });

        let all = db.all_games(PlayerId::FIRST).unwrap();
        let farm = all.iter().find(|g| g.id == farm).unwrap();
        let tags = farm.review_tags.as_ref().unwrap();
        assert_eq!(farm.digest_version, Some(DIGEST_VERSION));
        assert_eq!(tags.scales.get("pace").map(String::as_str), Some("relaxed"));
        assert_eq!(tags.scales.get("sessions").map(String::as_str), Some("short"));
        assert_eq!(tags.mood, ["relaxing", "cozy"]);
        assert_eq!(tags.aspects.len(), review_tags::MAX_ASPECTS);
        assert_eq!(tags.aspects[0], Aspect { aspect: "music".into(), verdict: Verdict::Praised });
        assert_eq!(tags.notes, ["Slow first hours"]);
        assert_eq!(farm.tags_by_source["reviews"][..2], ["Relaxing".to_string(), "Cozy".to_string()]);
        assert!(farm.tags.contains(&"Short sessions".to_string()), "review labels join the merged tags: {:?}", farm.tags);

        assert_eq!(farm.vibes.as_deref(), Some("Slow mornings on a little farm, fishing at dusk while the music hums."));

        let keep = all.iter().find(|g| g.id == keep).unwrap();
        assert_eq!(keep.digest.as_deref(), Some("Praised for precise, fair combat. Complaints: difficulty spikes."));
        assert_eq!(keep.vibes, None, "empty vibes are stored as none");
        let tags = keep.review_tags.as_ref().unwrap();
        assert_eq!(tags.scales.get("difficulty").map(String::as_str), Some("punishing"));
        assert!(tags.aspects.is_empty() && tags.notes.is_empty(), "a malformed list is dropped, not the item");
        assert!(written.missing.is_empty());
    }

    /// With the schema's keys out of order, models wrote "placeholder" or
    /// "x" where the summary belongs: such a digest isn't stored, the
    /// title is missing, and placeholder vibes are left out.
    #[tokio::test]
    async fn a_placeholder_is_not_a_summary() {
        let (_dir, db) = db_with_two_games();
        let titles = db.all_games(PlayerId::FIRST).unwrap();
        let farm = titles.iter().find(|g| g.name == "Quiet Farm").unwrap().id;
        let keep = titles.iter().find(|g| g.name == "Iron Keep").unwrap().id;
        let mut reply: Value = serde_json::from_str(
            &crate::testutil::fixture("llm/digest_reply.json").replace("\"FARM\"", &farm.to_string()).replace("#KEEP", &format!("#{keep}")),
        )
        .unwrap();
        reply["digests"][0]["digest"] = json!("Placeholder.");
        reply["digests"][1]["vibes"] = json!("placeholder");
        let content = serde_json::to_string(&reply.to_string()).unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                format!(r#"{{"model":"fake-model","choices":[{{"finish_reason":"stop","message":{{"role":"assistant","content":{content}}}}}]}}"#),
                "application/json",
            ))
            .mount(&server)
            .await;
        let llm = OpenAiCompatBackend::new(format!("{}/v1", server.uri()), Some("fake-model".into()), None);
        let written = generate(&db, &llm, None, &titles).await.unwrap();
        assert_eq!((written.digests, written.missing), (1, vec![farm]));
        let all = db.all_games(PlayerId::FIRST).unwrap();
        let keep = all.iter().find(|g| g.id == keep).unwrap();
        assert!(keep.digest.is_some() && keep.vibes.is_none(), "{:?}", keep.vibes);

        for text in ["", "  ", "x", "placeholder", "Placeholder.", "TBD", "n/a", "\"placeholder\""] {
            assert!(stand_in(text), "{text:?}");
        }
        for text in ["Six.", "Fun.", "Praised for precise, fair combat."] {
            assert!(!stand_in(text), "{text:?}");
        }
    }

    /// Summaries saved as placeholders before the fix are taken out, so the
    /// summaries job writes them again; real ones stay.
    #[test]
    fn saved_placeholders_are_written_again() {
        let (_dir, db) = db_with_two_games();
        let titles = db.all_games(PlayerId::FIRST).unwrap();
        let tags = review_tags::ReviewTags::default();
        db.put_digest(titles[0].id, "placeholder", "", &tags, "claude-sonnet-5-5", DIGEST_VERSION).unwrap();
        db.put_digest(titles[1].id, "Praised for precise, fair combat.", "x", &tags, "claude-sonnet-5-5", DIGEST_VERSION).unwrap();
        assert_eq!(db.remove_digests_where(stand_in).unwrap(), 2, "a placeholder digest, and placeholder vibes");
        assert!(db.all_games(PlayerId::FIRST).unwrap().iter().all(|g| g.digest.is_none() && outdated(g)));
        db.put_digest(titles[0].id, "Praised for calm farming.", "Slow mornings on a little farm.", &tags, "m", DIGEST_VERSION).unwrap();
        assert_eq!(db.remove_digests_where(stand_in).unwrap(), 0);
    }

    /// Review summaries ask for low effort.
    #[test]
    fn summaries_ask_for_low_effort() {
        assert_eq!(request(&games(), None, false).effort, Some("low"));
    }

    /// A current summary with more scales than the cap was written before
    /// it, and is rewritten.
    #[test]
    fn a_summary_over_the_scale_cap_is_outdated() {
        let (_dir, db) = db_with_two_games();
        let titles = db.all_games(PlayerId::FIRST).unwrap();
        let mut tags = review_tags::ReviewTags::default();
        for name in ["pace", "focus", "story", "emotion", "learning"] {
            tags.scales.insert(name.into(), review_tags::SCALES.iter().find(|s| s.name == name).unwrap().values[0].0.into());
        }
        db.put_digest(titles[0].id, "Five.", "", &tags, "claude-sonnet-5-5", DIGEST_VERSION).unwrap();
        tags.scales.insert("hook".into(), "fast".into());
        db.put_digest(titles[1].id, "Six.", "", &tags, "claude-sonnet-5-5", DIGEST_VERSION).unwrap();
        let all = db.all_games(PlayerId::FIRST).unwrap();
        let outdated = |id: i64| outdated(all.iter().find(|g| g.id == id).unwrap());
        assert!(!outdated(titles[0].id));
        assert!(outdated(titles[1].id));
    }

    /// Seen with GPT-6-Luna on 2026-10-01: one title of eight had no
    /// summary in the reply. It's listed, so the job can count it failed.
    #[tokio::test]
    async fn a_title_left_out_of_the_reply_is_missing() {
        let (_dir, db) = db_with_two_games();
        let titles = db.all_games(PlayerId::FIRST).unwrap();
        let farm = titles.iter().find(|g| g.name == "Quiet Farm").unwrap().id;
        let keep = titles.iter().find(|g| g.name == "Iron Keep").unwrap().id;
        let mut reply: Value = serde_json::from_str(
            &crate::testutil::fixture("llm/digest_reply.json").replace("\"FARM\"", &farm.to_string()).replace("#KEEP", &format!("#{keep}")),
        )
        .unwrap();
        reply["digests"].as_array_mut().unwrap().retain(|d| d["id"] == farm);
        let content = serde_json::to_string(&reply.to_string()).unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                format!(r#"{{"model":"fake-model","choices":[{{"finish_reason":"stop","message":{{"role":"assistant","content":{content}}}}}]}}"#),
                "application/json",
            ))
            .mount(&server)
            .await;
        let llm = OpenAiCompatBackend::new(format!("{}/v1", server.uri()), Some("fake-model".into()), None);
        let written = generate(&db, &llm, None, &titles).await.unwrap();
        assert_eq!((written.digests, written.missing), (1, vec![keep]));
    }
}
