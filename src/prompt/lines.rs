//! A prompt's lines about one game: a candidate's line with its history,
//! a wishlisted game's line, and the small formats they share (hours,
//! counts, how long ago). Help me decide and a lineup's new spares list
//! games with the same lines.

use crate::db::{GameView, Status};
use crate::lifecycle::{Kind, Offer};

pub(crate) fn hours(mins: i64) -> String {
    if mins < 60 { format!("{mins}m") } else { format!("{:.1}h", mins as f64 / 60.0) }
}

/// "8 months ago", "3 years ago".
pub(crate) fn ago(now: i64, t: i64) -> String {
    let days = ((now - t) / 86_400).max(0);
    match days {
        0 => "today".into(),
        1 => "yesterday".into(),
        d if d < 60 => format!("{d} days ago"),
        d if d < 730 => format!("{} months ago", d / 30),
        d => format!("{} years ago", d / 365),
    }
}

fn year_of(t: i64) -> Option<i32> {
    use chrono::Datelike;
    chrono::DateTime::from_timestamp(t, 0).map(|d| d.year())
}

/// Why a game was put down, as the AI reads it: "got busy, the grind, \"lost my save\"".
pub(crate) fn stopped_text(r: &crate::lifecycle::StopReasons) -> String {
    let mut words: Vec<String> =
        r.codes.iter().filter_map(|c| crate::lifecycle::REASONS.iter().find(|x| x.code == c)).map(|x| x.label.to_lowercase()).collect();
    if let Some(n) = &r.note {
        let n: String = n.split_whitespace().collect::<Vec<_>>().join(" ");
        let cut: String = n.chars().take(120).collect();
        words.push(format!("\"{}{}\"", cut, if cut.len() < n.len() { "…" } else { "" }));
    }
    words.join(", ")
}

/// The person's history with a game, for its candidate line:
/// "shelved 8 months ago after 14h; stopped: got busy, the grind",
/// "second chance: abandoned 2022 after 6h", "staple, 140h; endless".
/// None for a game with no history to tell. What was said in a lineup
/// ("too long") changes during a day, so it goes with the request instead,
/// where it leaves the cached candidate list alone.
pub fn history_segment(g: &GameView, now: i64) -> Option<String> {
    let mut parts: Vec<String> = vec![];
    let played = (g.playtime_known && g.playtime_minutes > 0).then(|| hours(g.playtime_minutes));
    let after = played.as_ref().map(|h| format!(" after {h}")).unwrap_or_default();
    // When it was put down: when it was last played, or else when it was marked.
    let put_down = if g.last_played > 0 { Some(g.last_played) } else { g.override_at };
    let offer = g.offer();
    let year = put_down.and_then(year_of).map(|y| format!(" {y}")).unwrap_or_default();
    match offer {
        Some(Offer::Shelved) if g.second_try => parts.push(format!("willing to try again: abandoned{year}{after}")),
        Some(Offer::Shelved) => parts.push(format!("shelved{}{after}", put_down.map(|t| format!(" {}", ago(now, t))).unwrap_or_default())),
        Some(Offer::SecondChance) => parts.push(format!("second chance: abandoned{year}{after}")),
        Some(Offer::Session) => {
            let (what, gap) = if g.staple {
                ("staple", ", ")
            } else if g.status == Status::StillPlaying {
                ("still playing now and then", ", ")
            } else {
                ("played", " ")
            };
            parts.push(match &played {
                Some(h) => format!("{what}{gap}{h}"),
                None => what.to_string(),
            });
        }
        Some(Offer::New) | None => {}
    }
    if g.kind == Kind::Endless {
        parts.push("endless".into());
    }
    if matches!(offer, Some(Offer::Shelved | Offer::SecondChance))
        && let Some(r) = &g.stop_reasons
    {
        parts.push(format!("stopped: {}", stopped_text(r)));
    }
    if let Some(line) = g.reviews_since_line() {
        parts.push(format!("reviews since: {}", crate::llm::one_line(line, usize::MAX)));
    }
    // Chosen with Play and never started: a weak signal of its own.
    if offer == Some(Offer::New) && g.chosen.as_ref().is_some_and(|c| g.playtime_known && g.last_played < c.at) {
        parts.push("chosen from picks before, never started".into());
    }
    // Your own rating of it.
    if let Some(stars) = g.my_rating {
        parts.push(format!("rated {stars}/5 by me"));
    }
    (!parts.is_empty()).then(|| parts.join("; "))
}

/// "157636" -> "158k".
pub(crate) fn short_count(n: i64) -> String {
    match n {
        n if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1e6),
        n if n >= 10_000 => format!("{}k", (n as f64 / 1e3).round()),
        n if n >= 1_000 => format!("{:.1}k", n as f64 / 1e3),
        n => n.to_string(),
    }
}

/// Most of a tag or genre a prompt takes; Steam's longest are about 25.
const LABEL_CHARS: usize = 40;

/// Tags and genres as a prompt lists them, each on the line (`one_line`).
pub(super) fn label_list(labels: &[impl AsRef<str>]) -> String {
    labels.iter().map(|l| crate::llm::one_line(l.as_ref(), LABEL_CHARS)).collect::<Vec<_>>().join(", ")
}

/// What a `bv` field is, for a prompt's line format: every
/// prompt that lists `bv` fields explains them with this and gives the AI
/// [`BV_RULE`].
pub const BV_FORMAT: &str = "bv: other backlog players' verdicts (how many, % saying worth it on a backlog, and what most said: \
when it clicks, session length, whether it respects limited time, when it's fine to stop, why those who stopped did)";

/// How the AI may use the `bv` fields: for fit with the request, said when
/// they decide, and never above what the person told UpNext.
pub const BV_RULE: &str = "\"bv\" lines are other backlog players' answers about how a game fits limited time. Use them for fit \
with the request, and say so when they decide a pick (\"other players say it's easy to pick back up\"), but never above what About me says.";

/// One compact candidate line. The format is explained once in the prompt:
/// `#id name [stores] | genres & tags | series | main story | played | history | reviews | year | linux | installed | feel | aspects | players say`.
/// `feel` adds the review tags: the feel and the aspects players judged.
/// `series` is named only when another candidate shares it.
/// `bv` adds other backlog players' verdicts, from 5 of them.
pub fn candidate_line(g: &GameView, sampled_minutes: i64, linux: bool, feel: bool, bv: bool, series: Option<&str>, now: i64) -> String {
    let mut name = format!("#{} {}", g.id, crate::llm::name_line(&g.name));
    if g.owned_on.len() > 1 || g.owned_on.first().is_some_and(|s| s != "steam") {
        name.push_str(&format!(" [{}]", g.owned_on.join(",")));
    }
    let mut parts = vec![name];
    // Genres, tags and themes overlap a lot: one de-duplicated list. With the
    // feel shown, the review tags' labels are left to it rather than said twice.
    let review_tags = g.review_tags.as_ref().filter(|_| feel);
    let feel_labels = review_tags.map(|t| t.labels()).unwrap_or_default();
    let tags = g.tags.iter().filter(|t| !feel_labels.iter().any(|l| l.eq_ignore_ascii_case(t))).take(8);
    let mut labels: Vec<&str> = vec![];
    for l in g.genres.iter().take(3).chain(tags).chain(g.themes.iter().take(3)) {
        if labels.len() >= 9 {
            break;
        }
        if !labels.iter().any(|x| x.eq_ignore_ascii_case(l)) {
            labels.push(l);
        }
    }
    if !labels.is_empty() {
        parts.push(label_list(&labels));
    }
    if let Some(s) = series {
        parts.push(format!("series: {}", crate::llm::name_line(s)));
    }
    parts.push(g.hltb_main.map(|h| format!("{h}h")).unwrap_or_else(|| "?h".into()));
    if g.playtime_known && g.playtime_minutes > 0 && g.playtime_minutes < sampled_minutes {
        parts.push(format!("played {}", hours(g.playtime_minutes)));
    }
    if let Some(h) = history_segment(g, now) {
        parts.push(h);
    }
    if let (Some(p), Some(t)) = (g.review_pct, g.review_total) {
        parts.push(format!("{p}%/{}", short_count(t)));
    } else if let Some(r) = g.gog_rating.or(g.rawg_rating) {
        parts.push(format!("{r:.1}/5"));
    }
    if let Some(y) = g.release_year() {
        parts.push(y);
    }
    if linux && let Some(l) = &g.linux {
        parts.push(l.clone());
    }
    if g.installed {
        parts.push("installed".into());
    }
    if let Some(t) = review_tags {
        if !feel_labels.is_empty() {
            parts.push(format!("feel: {}", feel_labels.join(", ").to_lowercase()));
        }
        if !t.aspects.is_empty() {
            parts.push(t.verdicts());
        }
    }
    if bv && let Some(line) = g.backlog.as_ref().and_then(|b| b.bv_line()) {
        parts.push(line);
    }
    // The whole digest: it is praise first, then complaints, so any cut drops
    // the complaints. Length is bounded where digests are written (digest.rs).
    if let Some(d) = &g.digest {
        parts.push(format!("says: {}", crate::llm::one_line(d, usize::MAX)));
    }
    parts.join(" | ")
}

/// One wishlisted game's line, in the candidates' format as far as the
/// store data goes: `w12 Hades II | Action, Roguelike | 25h | 96%/120k | 2025 | says: …`.
pub fn wish_line(w: &crate::db::WishGame) -> String {
    let mut parts = vec![format!("w{} {}", w.id, crate::llm::name_line(&w.name))];
    let mut labels: Vec<&str> = vec![];
    for l in w.genres.iter().take(3).chain(w.tags.iter().take(8)) {
        if labels.len() < 9 && !labels.iter().any(|x| x.eq_ignore_ascii_case(l)) {
            labels.push(l);
        }
    }
    if !labels.is_empty() {
        parts.push(label_list(&labels));
    }
    parts.push(w.hltb_main.map(|h| format!("{h}h")).unwrap_or_else(|| "?h".into()));
    if let (Some(p), Some(t)) = (w.review_pct, w.review_total) {
        parts.push(format!("{p}%/{}", short_count(t)));
    }
    if let Some(y) =
        w.release_date.as_deref().and_then(|d| regex::Regex::new(r"\b(19|20)\d\d\b").ok()?.find(d).map(|m| m.as_str().to_string()))
    {
        parts.push(y);
    }
    if let Some(d) = w.short_description.as_deref().filter(|d| !d.trim().is_empty()) {
        parts.push(format!("says: {}", crate::llm::one_line(d, usize::MAX)));
    }
    parts.join(" | ")
}
