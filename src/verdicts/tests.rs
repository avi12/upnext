//! Backlog verdicts' contract: the vocabulary, the
//! consent, the checks on a verdict, the draft and the published format.

use super::consent::{self, CONSENT_VERSION, LICENCE};
use super::draft::{self, Facts, Source};
use super::format::{self, GameCounts, Published, SHOW_AT, TEXTS_PER_GAME};
use super::verdict::{self, TEXT_CHECKS, TEXT_MAX, Verdict};
use super::vocab::{self, FACTORS, FIELDS, VOCAB};
use crate::db::PlayerId;
use crate::db::{Db, DiaryRow, GameView, Override, Status};
use crate::lifecycle::{Kind, REASONS, ReasonGroup, StopReasons};
use crate::review_tags::SCALES;
use crate::testutil::{fixture_path, offline_config};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io::{Read, Write};

// -- the vocabulary --------------------------------------------------------------

#[test]
fn every_answer_has_a_label_and_a_code_of_its_own() {
    let mut names = BTreeSet::new();
    for f in FIELDS {
        assert!(names.insert(f.name), "{} twice", f.name);
        assert!(!f.question.is_empty() && f.question.ends_with('?'), "{}", f.name);
        let codes: BTreeSet<&str> = f.choices.iter().map(|c| c.code).collect();
        assert_eq!(codes.len(), f.choices.len(), "{}: a code twice", f.name);
        for c in f.choices {
            assert!(!c.label.is_empty(), "{}: {} has no label", f.name, c.code);
            assert!(c.code.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'), "{}", c.code);
        }
    }
    // What the game page sums up has a "most players" line for every answer.
    for f in [&vocab::CLICKED, &vocab::SESSIONS, &vocab::LIMITED_TIME, &vocab::STOP_POINT] {
        assert!(f.choices.iter().all(|c| c.line.is_some_and(|l| !l.is_empty())), "{} needs lines", f.name);
    }
    let factors: BTreeSet<&str> = FACTORS.iter().map(|f| f.code).collect();
    assert_eq!(factors.len(), FACTORS.len());
    assert!(FACTORS.iter().all(|f| !f.label.is_empty() && !f.line.is_empty()));
    assert_eq!(FACTORS.iter().filter(|f| f.helps).count(), 4, "the plan's four that help");
    assert_eq!(FACTORS.iter().filter(|f| !f.helps).count(), 5, "and five that hurt");
    assert_eq!([vocab::OUTCOME.required, vocab::WORTH.required], [true, true]);
    assert!(FIELDS[2..].iter().all(|f| !f.required), "only where you left it and whether it's worth it are required");
}

#[test]
fn clicked_and_sessions_mean_what_review_tags_mean() {
    let scale = |name: &str| SCALES.iter().find(|s| s.name == name).unwrap().values.iter().map(|(v, _)| *v).collect::<Vec<_>>();
    // `sessions` is review tags' scale, value for value.
    let sessions: Vec<&str> = vocab::SESSIONS.choices.iter().map(|c| c.code).collect();
    assert_eq!(sessions, scale("sessions"));
    // Every `clicked` answer but never reads as a `hook` value, and every
    // `hook` value is reached.
    let hook = scale("hook");
    let reached: BTreeSet<&str> = vocab::CLICKED.choices.iter().filter_map(|c| vocab::hook(c.code)).collect();
    assert_eq!(reached, hook.iter().copied().collect());
    assert_eq!(vocab::hook("first_hour"), Some("fast"));
    assert_eq!(vocab::hook("few_hours"), Some("gradual"));
    assert_eq!((vocab::hook("later"), vocab::hook("much_later")), (Some("slow"), Some("slow")));
    assert_eq!(vocab::hook("never"), None);
}

#[test]
fn what_stopped_you_is_a_game_reason_and_never_a_life_reason() {
    let game: Vec<&str> = REASONS.iter().filter(|r| r.group == ReasonGroup::Game).map(|r| r.code).collect();
    let asked: Vec<&str> = vocab::stop_reasons().map(|r| r.code).collect();
    assert_eq!(asked, game);
    assert_eq!(asked, ["grind", "stuck", "lost_story", "too_long", "pacing", "bugs", "too_hard", "boring"]);
    for life in ["busy", "other_game", "mood", "elsewhere"] {
        assert!(!vocab::is_stop_reason(life), "{life} is a life reason");
    }
}

#[test]
fn playtime_and_sessions_fall_into_ranges() {
    let cases = [(0, "under_2"), (119, "under_2"), (120, "2_5"), (299, "2_5"), (300, "5_10"), (840, "10_20"), (1200, "20_40")];
    for (minutes, range) in cases {
        assert_eq!(vocab::hours_range(minutes), range, "{minutes} minutes");
    }
    assert_eq!(vocab::hours_range(4799), "40_80");
    assert_eq!(vocab::hours_range(4800), "80_plus");
    assert_eq!(vocab::hours_range(-5), "under_2");
    let sessions = [(20, "short"), (60, "short"), (61, "medium"), (120, "medium"), (121, "long"), (400, "long")];
    for (minutes, s) in sessions {
        assert_eq!(vocab::sessions_for(minutes), s, "{minutes} minutes");
    }
}

#[test]
fn the_contract_lists_every_question_and_answer() {
    let c = super::contract();
    let fields = c["fields"].as_array().unwrap();
    let names: Vec<&str> = fields.iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        ["outcome", "worth", "hours", "clicked", "sessions", "limited_time", "factors", "stop_point", "stop_note", "stopped_for", "text"]
    );
    for f in FIELDS {
        let listed = fields.iter().find(|x| x["name"] == f.name).unwrap();
        let codes: Vec<&str> = listed["choices"].as_array().unwrap().iter().map(|x| x["code"].as_str().unwrap()).collect();
        assert_eq!(codes, f.choices.iter().map(|x| x.code).collect::<Vec<_>>(), "{}", f.name);
    }
    assert_eq!(c["licence"], LICENCE);
    assert_eq!(c["consent"]["version"], CONSENT_VERSION);
    assert_eq!(c["vocab"], VOCAB);
    assert_eq!(c["text_checks"].as_array().unwrap().len(), TEXT_CHECKS.len());
    assert_eq!(fields[4]["choices"][0]["review_tag"], "sessions:short");
    assert_eq!(fields[3]["choices"][0]["review_tag"], "hook:fast");
}

const COMMITTED_CONTRACT: &str = "docs/verdicts-contract.json";

#[test]
fn the_committed_contract_is_current() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(COMMITTED_CONTRACT);
    let now = super::contract_text();
    if std::env::var_os("UPNEXT_UPDATE_VERDICTS").is_some() {
        std::fs::write(&path, &now).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == now,
        "{COMMITTED_CONTRACT} isn't the contract the code checks: run `UPNEXT_UPDATE_VERDICTS=1 cargo test verdicts` and commit it"
    );
}

// -- consent --------------------------------------------------------------------

/// The words of consent version 1. Changing any of them means a new
/// version: bump `CONSENT_VERSION` and pin the new text here.
const CONSENT_SHA256: (i64, &str) = (1, "f3ad5dce2c9005e11afaede765ad4bc62a03a5e156f2e8b6ada2e8fe6218d0f1");

fn consent_text() -> String {
    let mut s = String::from(consent::TITLE);
    for line in consent::PARAGRAPHS.iter().chain(&[consent::ACCEPT, consent::DECLINE, consent::TEXT_BOX_LINE]) {
        s.push('\n');
        s.push_str(line);
    }
    s
}

#[test]
fn the_consent_text_is_pinned_to_its_version() {
    let hash = Sha256::digest(consent_text().as_bytes()).iter().map(|b| format!("{b:02x}")).collect::<String>();
    assert_eq!(
        (CONSENT_VERSION, hash.as_str()),
        CONSENT_SHA256,
        "the consent text changed: bump CONSENT_VERSION and pin the new hash, so every player is asked again"
    );
}

#[test]
fn the_consent_says_it_is_public_cc0_unlinked_and_cannot_be_recalled() {
    let text = consent_text();
    for needed in [
        "will be public",
        "anything you type",
        "anyone to read, copy and reuse",
        "no credit needed",
        "CC0 1.0",
        "public domain",
        "isn't linked to your name",
        "doesn't publish which verdicts came from the same person",
        "rounded to a range",
        "take a verdict down at any time",
        "within a week",
        "can't be recalled",
    ] {
        assert!(text.contains(needed), "the consent doesn't say {needed:?}");
    }
    assert_eq!(consent::ACCEPT, "Publish my verdicts under CC0");
    assert!(consent::TEXT_BOX_LINE.contains("CC0"));
}

// -- a verdict and its checks -----------------------------------------------------

fn full_verdict() -> Verdict {
    Verdict {
        outcome: Some("shelved".into()),
        worth: Some("for_some".into()),
        hours: Some("10_20".into()),
        clicked: Some("few_hours".into()),
        sessions: Some("short".into()),
        limited_time: Some("mostly".into()),
        factors: vec!["save_anywhere".into(), "padding".into()],
        stop_point: Some("at_credits".into()),
        stop_note: Some("after chapter 3".into()),
        stopped_for: vec!["grind".into(), "too_long".into()],
        text: Some("The first act drags, then it opens up. Fine in short runs.".into()),
        ..Verdict::new(Some(620), Some("1207658924".into()))
    }
}

fn problems(v: &Verdict) -> Vec<(&'static str, String)> {
    v.check().into_iter().map(|p| (p.field, p.why)).collect()
}

#[test]
fn a_complete_verdict_passes_and_carries_the_licence_and_consent() {
    let v = full_verdict();
    assert_eq!(problems(&v), vec![]);
    assert_eq!((v.vocab, v.licence.as_str(), v.consent), (VOCAB, LICENCE, CONSENT_VERSION));
    assert_eq!(v.client, env!("CARGO_PKG_VERSION"));
    let sent = serde_json::to_value(&v).unwrap();
    assert_eq!(sent["licence"], "CC0-1.0");
    assert_eq!(sent["consent"], 1);
    // The smallest verdict: a game and the two required answers.
    let least = Verdict { outcome: Some("finished".into()), worth: Some("yes".into()), ..Verdict::new(None, Some("1".into())) };
    assert_eq!(problems(&least), vec![]);
    assert_eq!(
        serde_json::to_value(&least).unwrap(),
        serde_json::json!({"gog_id": "1", "outcome": "finished", "worth": "yes", "vocab": 1, "licence": "CC0-1.0", "consent": 1, "client": env!("CARGO_PKG_VERSION")})
    );
}

#[test]
fn checks_name_each_field_that_is_wrong() {
    let fields = |v: &Verdict| problems(v).into_iter().map(|(f, _)| f).collect::<Vec<_>>();
    let base = full_verdict();
    assert_eq!(fields(&Verdict { outcome: None, worth: None, ..base.clone() }), ["outcome", "worth"]);
    assert_eq!(fields(&Verdict { hours: Some("3_4".into()), ..base.clone() }), ["hours"]);
    assert_eq!(fields(&Verdict { steam_appid: None, gog_id: None, ..base.clone() }), ["game"]);
    assert_eq!(fields(&Verdict { gog_id: Some("gog-1".into()), ..base.clone() }), ["gog_id"]);
    assert_eq!(fields(&Verdict { steam_appid: Some(0), ..base.clone() }), ["steam_appid"]);
    assert_eq!(fields(&Verdict { limited_time: None, ..base.clone() }), ["factors"]);
    let four = ["save_anywhere", "recap", "padding", "obligations"].map(String::from).to_vec();
    assert_eq!(fields(&Verdict { factors: four, ..base.clone() }), ["factors"]);
    assert_eq!(fields(&Verdict { factors: vec!["recap".into(), "recap".into()], ..base.clone() }), ["factors"]);
    assert_eq!(fields(&Verdict { factors: vec!["fun".into()], ..base.clone() }), ["factors"]);
    assert_eq!(fields(&Verdict { stop_point: None, ..base.clone() }), ["stop_note"]);
    assert_eq!(fields(&Verdict { stop_note: Some("x".repeat(81)), ..base.clone() }), ["stop_note"]);
    // What stopped you is only asked of games put down.
    assert_eq!(fields(&Verdict { outcome: Some("finished".into()), ..base.clone() }), ["stopped_for"]);
    let life = Verdict { stopped_for: vec!["busy".into()], ..base.clone() };
    assert_eq!(problems(&life), vec![("stopped_for", "\"busy\" is about you, not the game, and is never sent".to_string())]);
    assert_eq!(fields(&Verdict { licence: "CC-BY-4.0".into(), ..base.clone() }), ["licence"]);
    assert_eq!(fields(&Verdict { consent: 0, ..base.clone() }), ["consent"]);
    assert_eq!(fields(&Verdict { consent: CONSENT_VERSION + 1, ..base.clone() }), ["consent"]);
    assert_eq!(fields(&Verdict { vocab: VOCAB + 1, ..base.clone() }), ["vocab"]);
    assert_eq!(fields(&Verdict { client: "0.1.0; DROP".into(), ..base.clone() }), ["client"]);
    assert_eq!(fields(&Verdict { text: Some("   ".into()), ..base }), ["text"]);
}

#[test]
fn a_verdict_with_any_other_field_is_refused() {
    let mut sent = serde_json::to_value(full_verdict()).unwrap();
    for extra in ["steam_id", "name", "rating", "playtime_minutes", "at", "timezone"] {
        sent[extra] = serde_json::json!("x");
        let e = serde_json::from_value::<Verdict>(sent.clone()).unwrap_err().to_string();
        assert!(e.contains("unknown field"), "{extra}: {e}");
        sent.as_object_mut().unwrap().remove(extra);
    }
    assert_eq!(serde_json::from_value::<Verdict>(sent).unwrap(), full_verdict());
}

#[derive(serde::Deserialize)]
struct TextCase {
    text: String,
    refused: Option<String>,
}

/// The cases both sides check: `tests/fixtures/verdicts/text-checks.json`,
/// which the service's tests read too.
#[test]
fn typed_text_with_contact_details_is_refused_and_says_why() {
    let cases: Vec<TextCase> = serde_json::from_str(&std::fs::read_to_string(fixture_path("verdicts/text-checks.json")).unwrap()).unwrap();
    assert!(cases.len() >= 20);
    for code in TEXT_CHECKS.map(|c| c.code) {
        assert!(cases.iter().any(|c| c.refused.as_deref() == Some(code)), "no case for {code}");
    }
    for c in &cases {
        let got = verdict::text_refusal(&c.text).map(|x| x.code);
        assert_eq!(got, c.refused.as_deref(), "{:?}", c.text);
    }
    let v = Verdict { text: Some("Mail me at someone@example.org".into()), ..full_verdict() };
    assert_eq!(problems(&v), vec![("text", "has an email address; take it out to send".to_string())]);
}

#[test]
fn texts_are_at_most_500_characters_and_notes_80() {
    let at = |n: usize| Verdict { text: Some("é".repeat(n)), ..full_verdict() };
    assert_eq!(problems(&at(TEXT_MAX)), vec![]);
    assert_eq!(problems(&at(TEXT_MAX + 1)), vec![("text", "is 501 characters; at most 500".to_string())]);
    let note = |n: usize| Verdict { stop_note: Some("a".repeat(n)), ..full_verdict() };
    assert_eq!(problems(&note(80)), vec![]);
    assert_eq!(problems(&note(81)).len(), 1);
}

// -- the draft -------------------------------------------------------------------------

fn mock() -> (tempfile::TempDir, crate::config::Config, Db) {
    let dir = tempfile::tempdir().unwrap();
    let cfg = offline_config(dir.path());
    let db = Db::open(&cfg.db_path, cfg.sampled_minutes).unwrap();
    crate::mock::seed(&db).unwrap();
    (dir, cfg, db)
}

fn game(db: &Db, name: &str) -> GameView {
    db.all_games(PlayerId::FIRST).unwrap().into_iter().find(|g| g.name == name).unwrap_or_else(|| panic!("{name} isn't in the library"))
}

fn reasons(codes: &[&str]) -> StopReasons {
    StopReasons { codes: codes.iter().map(|c| c.to_string()).collect(), note: None, source: "menu".into(), at: 0 }
}

fn built(g: &GameView, stop_reasons: &[StopReasons], diary: Option<&[i64]>) -> draft::Draft {
    draft::build(&Facts { game: g, stop_reasons, diary_sessions: diary })
}

#[test]
fn beat_it_with_14_hours_is_finished_10_to_20_hours_and_asks_no_reasons() {
    let (_d, _cfg, db) = mock();
    let mut g = game(&db, "Portal 2");
    g.override_ = Some(Override::Done);
    g.playtime_minutes = 14 * 60;
    g.playtime_known = true;
    let d = built(&g, &[reasons(&["grind"])], None);
    assert_eq!(d.verdict.outcome.as_deref(), Some("finished"));
    assert_eq!(d.verdict.hours.as_deref(), Some("10_20"));
    assert!(d.verdict.stopped_for.is_empty(), "finished games aren't asked what stopped them");
    assert_eq!(d.from.get("outcome"), Some(&Source::Mark));
    assert_eq!(d.from.get("hours"), Some(&Source::Playtime));
    assert_eq!(d.verdict.steam_appid, Some(620));
    assert_eq!(d.cant_send, None);
    // Worth it is never guessed, so it's what's left.
    assert_eq!(d.problems.iter().map(|p| p.field).collect::<Vec<_>>(), ["worth"]);
}

#[test]
fn marks_map_onto_outcomes() {
    let (_d, _cfg, db) = mock();
    let mut g = game(&db, "Portal 2");
    let cases = [
        (Override::Done, "finished"),
        (Override::Shelved, "shelved"),
        (Override::BurntOut, "shelved"),
        (Override::Abandoned, "stopped"),
        (Override::StillPlaying, "endless"),
        (Override::Hidden, "bounced"),
    ];
    for (mark, outcome) in cases {
        g.override_ = Some(mark);
        assert_eq!(built(&g, &[], None).verdict.outcome.as_deref(), Some(outcome), "{mark:?}");
    }
    // Unmarked, only playing it now is filled in: UpNext's own shelved and
    // abandoned are guesses, not something the player said.
    g.override_ = None;
    g.status = Status::Playing;
    let d = built(&g, &[], None);
    assert_eq!((d.verdict.outcome.as_deref(), d.from.get("outcome")), (Some("playing"), Some(&Source::Playing)));
    for status in [Status::Shelved, Status::Abandoned, Status::Played, Status::Untouched] {
        g.status = status;
        assert_eq!(built(&g, &[], None).verdict.outcome, None, "{status:?}");
    }
}

#[test]
fn abandoned_sends_only_the_game_reasons_newest_first() {
    let (_d, _cfg, db) = mock();
    let mut g = game(&db, "Portal 2");
    g.override_ = Some(Override::Abandoned);
    let d = built(&g, &[reasons(&["busy", "grind"])], None);
    assert_eq!(d.verdict.stopped_for, ["grind"]);
    assert_eq!(d.from.get("stopped_for"), Some(&Source::StopReasons));
    let d = built(&g, &[reasons(&["busy", "grind"]), reasons(&["too_long", "grind"]), reasons(&["mood"])], None);
    assert_eq!(d.verdict.stopped_for, ["grind", "too_long"]);
    // Only life reasons: nothing to send, and nothing claimed as their source.
    let d = built(&g, &[reasons(&["busy", "other_game"])], None);
    assert!(d.verdict.stopped_for.is_empty() && !d.from.contains_key("stopped_for"));
    assert_eq!(problems(&Verdict { worth: Some("no".into()), ..d.verdict }), vec![]);
}

#[test]
fn the_stop_reason_history_ends_where_every_reason_was_cleared() {
    let (_d, _cfg, db) = mock();
    let id = game(&db, "Portal 2").id;
    db.add_stop_reasons(PlayerId::FIRST, id, &["boring".to_string()], None, "menu").unwrap();
    db.add_stop_reasons(PlayerId::FIRST, id, &[], None, "second_chance").unwrap();
    db.add_stop_reasons(PlayerId::FIRST, id, &["too_hard".to_string()], None, "menu").unwrap();
    db.add_stop_reasons(PlayerId::FIRST, id, &["busy".to_string(), "stuck".to_string()], Some("the water level"), "pick").unwrap();
    let history = db.stop_reason_history(PlayerId::FIRST, id).unwrap();
    let codes: Vec<Vec<String>> = history.iter().map(|r| r.codes.clone()).collect();
    assert_eq!(codes, [vec!["busy".to_string(), "stuck".to_string()], vec!["too_hard".to_string()]]);
    db.set_override(PlayerId::FIRST, id, Some(Override::Shelved), None).unwrap();
    let cfg = offline_config(_d.path());
    let d = draft::for_title(&db, &cfg, id).unwrap().unwrap();
    assert_eq!(d.verdict.stopped_for, ["stuck", "too_hard"]);
    assert_eq!(d.verdict.text, None, "the note stays on the install");
}

#[test]
fn the_play_diary_suggests_a_session_from_five_sessions_on() {
    let (_d, _cfg, db) = mock();
    let g = game(&db, "Portal 2");
    let d = built(&g, &[], Some(&[30, 40, 200, 45, 50]));
    assert_eq!((d.verdict.sessions.as_deref(), d.from.get("sessions")), (Some("short"), Some(&Source::Diary)));
    assert_eq!(built(&g, &[], Some(&[90, 100, 110, 130, 150, 20])).verdict.sessions.as_deref(), Some("medium"));
    assert_eq!(built(&g, &[], Some(&[300, 240, 200, 180])).verdict.sessions, None, "four sessions are too few");
    assert_eq!(built(&g, &[], None).verdict.sessions, None, "the diary is off");
}

#[test]
fn diary_sessions_join_back_to_back_hours() {
    let row = |hour: i64, title: i64, minutes: i64, source: &str| DiaryRow {
        id: 0,
        hour,
        at: hour,
        title_id: Some(title),
        appid: None,
        name: String::new(),
        minutes,
        source: source.into(),
    };
    // An hour's start, as the diary writes them.
    let h = 1_799_996_400;
    let rows = [
        row(h + 3600, 7, 30, "steam"),
        row(h, 7, 60, "steam"),
        row(h + 10 * 3600, 7, 20, "steam"),
        row(h + 10 * 3600, 7, 0, "launch"),
        row(h + 11 * 3600, 8, 60, "steam"),
        row(h + 30 * 3600, 7, 0, "steam"),
    ];
    assert_eq!(draft::diary_sessions(&rows, 7), [90, 20]);
    assert_eq!(draft::diary_sessions(&rows, 8), [60]);
    assert!(draft::diary_sessions(&rows, 9).is_empty());
}

#[test]
fn no_playtime_asks_for_hours_and_no_ending_means_any_time() {
    let (_d, _cfg, db) = mock();
    let mut g = game(&db, "Control");
    assert!(!g.playtime_known, "an Epic game through Heroic has no playtime");
    let d = built(&g, &[], None);
    assert_eq!((d.verdict.hours.as_deref(), d.from.get("hours")), (None, None));
    g.kind = Kind::Endless;
    let d = built(&g, &[], None);
    assert_eq!((d.verdict.stop_point.as_deref(), d.from.get("stop_point")), (Some("anytime"), Some(&Source::NoEnding)));
    g.kind = Kind::Ending;
    g.override_ = Some(Override::StillPlaying);
    assert_eq!(built(&g, &[], None).verdict.stop_point.as_deref(), Some("anytime"));
}

#[test]
fn a_game_with_neither_id_cant_get_a_verdict_and_a_gog_copy_gives_its_id() {
    let (_d, _cfg, db) = mock();
    let mut g = game(&db, "Control");
    g.appid = None;
    let d = built(&g, &[], None);
    assert!(d.cant_send.as_deref().is_some_and(|w| w.contains("neither a Steam appid nor a GOG id")), "{:?}", d.cant_send);
    assert!(d.problems.iter().any(|p| p.field == "game"));
    let mut copy = g.copies[0].clone();
    copy.source = "gog".into();
    copy.source_id = "1207658924".into();
    g.copies.push(copy);
    let d = built(&g, &[], None);
    assert_eq!((d.verdict.gog_id.as_deref(), d.cant_send), (Some("1207658924"), None));
}

/// The fields a verdict may carry, and nothing else.
const ALLOWED: [&str; 17] = [
    "steam_appid",
    "gog_id",
    "outcome",
    "worth",
    "hours",
    "clicked",
    "sessions",
    "limited_time",
    "factors",
    "stop_point",
    "stop_note",
    "stopped_for",
    "text",
    "vocab",
    "licence",
    "consent",
    "client",
];

#[test]
fn a_draft_never_carries_the_player_their_library_or_exact_times() {
    let (dir, mut cfg, db) = mock();
    cfg.play_diary = true;
    db.setting_set(PlayerId::FIRST, "steam_id", Some("76561197960287930")).unwrap();
    db.add_stop_reasons(PlayerId::FIRST, game(&db, "Hades").id, &["busy".into(), "grind".into()], Some("my brother's birthday"), "menu")
        .unwrap();
    db.set_game_rating(PlayerId::FIRST, game(&db, "Hades").id, Some(2)).unwrap();
    for g in db.all_games(PlayerId::FIRST).unwrap() {
        db.set_override(PlayerId::FIRST, g.id, Some(Override::Abandoned), None).unwrap();
        let d = draft::for_title(&db, &cfg, g.id).unwrap().unwrap();
        let sent = serde_json::to_value(&d.verdict).unwrap();
        let keys: Vec<&String> = sent.as_object().unwrap().keys().collect();
        assert!(keys.iter().all(|k| ALLOWED.contains(&k.as_str())), "{}: {keys:?}", g.name);
        let text = sent.to_string();
        for never in ["76561197960287930", "birthday", "busy", &g.name, &format!("\"{}\"", g.playtime_minutes)] {
            assert!(!text.contains(never), "{}'s verdict carries {never:?}: {text}", g.name);
        }
        assert!(!text.contains(&dir.path().display().to_string()));
    }
}

// -- the published format ---------------------------------------------------------------

/// A verdict as the service publishes it, with a made-up id.
fn published(n: u32, appid: Option<u32>, gog: Option<&str>, outcome: &str, worth: &str, month: &str) -> Published {
    // 26 base32 characters from the number, so ids are distinct and sort by it.
    let alphabet = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut id: Vec<u8> = vec![b'a'; 26];
    let mut x = n;
    for slot in id.iter_mut().rev() {
        *slot = alphabet[(x % 32) as usize];
        x /= 32;
    }
    Published {
        id: String::from_utf8(id).unwrap(),
        steam_appid: appid,
        gog_id: gog.map(str::to_string),
        outcome: outcome.into(),
        worth: worth.into(),
        hours: None,
        clicked: None,
        sessions: None,
        limited_time: None,
        factors: vec![],
        stop_point: None,
        stop_note: None,
        stopped_for: vec![],
        text: None,
        vocab: VOCAB,
        month: month.into(),
    }
}

/// The fixture snapshot's verdicts: games of the mock library with three
/// or more, one with two (left out of the summary), one whose GOG-only
/// verdicts are tied to its appid by a verdict with both ids, contradictory
/// answers, texts, and more texts than a summary carries.
fn fixture_verdicts() -> Vec<Published> {
    let mut out = vec![];
    let mut n = 0;
    let mut next = |appid: Option<u32>, gog: Option<&str>, outcome: &str, worth: &str, month: &str| {
        n += 1;
        published(n, appid, gog, outcome, worth, month)
    };
    // Hades: mostly loved and easy to drop into, with 23 texts.
    for i in 0..23 {
        let mut v = next(
            Some(1_145_360),
            None,
            if i % 4 == 0 { "endless" } else { "finished" },
            if i % 5 == 0 { "for_some" } else { "yes" },
            if i < 12 { "2026-09" } else { "2026-10" },
        );
        v.hours = Some(if i % 3 == 0 { "20_40" } else { "10_20" }.into());
        v.clicked = Some("first_hour".into());
        v.sessions = Some("short".into());
        v.limited_time = Some("yes".into());
        v.factors = vec!["short_chunks".into(), "save_anywhere".into()];
        v.stop_point = Some(if i % 2 == 0 { "at_credits" } else { "anytime" }.into());
        v.text = Some(format!("Runs are 30 minutes; stopping mid-run is fine (verdict {i})."));
        out.push(v);
    }
    // The Witcher 3: contradictory, stopped for its length by some.
    for (i, (outcome, worth)) in
        [("finished", "yes"), ("stopped", "no"), ("shelved", "for_some"), ("stopped", "for_some"), ("finished", "yes")].iter().enumerate()
    {
        let mut v = next(Some(292_030), (i == 4).then_some("1207664663"), outcome, worth, "2026-08");
        v.hours = Some(["80_plus", "10_20", "20_40", "5_10", "40_80"][i].into());
        v.clicked = Some(["few_hours", "never", "later", "much_later", "later"][i].into());
        v.limited_time = Some(["mostly", "no", "no", "no", "mostly"][i].into());
        v.factors = vec!["hard_to_resume".into()];
        if *outcome != "finished" {
            v.stopped_for = vec!["too_long".into(), "pacing".into()];
        }
        if i == 1 {
            v.stop_point = Some("anytime".into());
            v.stop_note = Some("once Velen is done".into());
        }
        out.push(v);
    }
    // The Witcher 3's GOG-only verdicts join it through the verdict above.
    out.push(next(None, Some("1207664663"), "finished", "yes", "2026-10"));
    // Celeste: exactly three verdicts.
    for worth in ["yes", "yes", "no"] {
        let mut v = next(Some(504_230), None, "finished", worth, "2026-10");
        v.sessions = Some("short".into());
        out.push(v);
    }
    // Stellaris: two verdicts, too few to show.
    out.push(next(Some(281_990), None, "endless", "yes", "2026-10"));
    out.push(next(Some(281_990), None, "bounced", "no", "2026-10"));
    // A GOG-only game nobody tied to an appid, with three verdicts.
    for _ in 0..3 {
        out.push(next(None, Some("1453375253"), "bounced", "no", "2026-07"));
    }
    out
}

const FIXTURE_DATE: &str = "2026-10-09";

#[test]
fn the_fixture_snapshot_reads_back_whole() {
    let verdicts = fixture_verdicts();
    let snap = format::write_snapshot(FIXTURE_DATE, &verdicts).unwrap();
    let m = format::read_manifest(&serde_json::to_vec(&snap.manifest).unwrap()).unwrap();
    assert_eq!((m.date.as_str(), m.licence.as_str(), m.verdicts), (FIXTURE_DATE, LICENCE, verdicts.len() as u64));
    let summary = format::read_named_summary(&m, &snap.summary).unwrap();
    let all = format::read_all(&snap.all).unwrap();
    assert_eq!(all.verdicts, verdicts);
    assert_eq!(summary.games, format::summarise(&all.verdicts));
    // Stellaris's two verdicts are too few; the rest show.
    let shown: Vec<(Option<u32>, Option<&str>, u32)> = summary.games.iter().map(|g| (g.steam_appid, g.gog_id.as_deref(), g.n)).collect();
    assert_eq!(
        shown,
        [(Some(292_030), Some("1207664663"), 6), (Some(504_230), None, 3), (Some(1_145_360), None, 23), (None, Some("1453375253"), 3)]
    );
    let hades = &summary.games[2];
    assert_eq!(hades.texts.len(), TEXTS_PER_GAME);
    assert!(hades.texts.windows(2).all(|w| (w[0].month.as_str(), w[1].id.as_str()) >= (w[1].month.as_str(), w[0].id.as_str())));
    assert_eq!(hades.texts[0].month, "2026-10");
    assert_eq!(hades.counts.clicked.get("first_hour"), Some(&23));
    assert_eq!(hades.counts.factors.get("save_anywhere"), Some(&23));
    let witcher = &summary.games[0];
    assert_eq!(witcher.counts.stopped_for.get("too_long"), Some(&3));
    assert_eq!(witcher.counts.hours_by_outcome["finished"].values().sum::<u32>(), 2, "the GOG-only verdict has no hours");
    assert_eq!(witcher.texts.len(), 1);
    assert_eq!(witcher.texts[0].stop_note.as_deref(), Some("once Velen is done"));
}

fn unpacked(gz: &[u8]) -> String {
    let mut s = String::new();
    flate2::read::GzDecoder::new(gz).read_to_string(&mut s).unwrap();
    s
}

#[test]
fn published_files_carry_months_and_no_keys() {
    let snap = format::write_snapshot(FIXTURE_DATE, &fixture_verdicts()).unwrap();
    let day = regex::Regex::new(r"\d{4}-\d{2}-\d{2}").unwrap();
    for gz in [&snap.summary, &snap.all] {
        let text = unpacked(gz);
        let (header, rest) = text.split_once('\n').unwrap();
        assert!(header.contains("\"licence\":\"CC0-1.0\"") && header.contains("CC0 1.0"));
        assert!(!rest.contains("key") && !rest.contains("client") && !rest.contains("consent"));
        assert!(!day.is_match(rest), "a day in the published lines");
    }
}

/// The fixture snapshot the service's tests compare against, in
/// `tests/fixtures/verdicts/`, written by
/// `UPNEXT_UPDATE_VERDICTS=1 cargo test verdicts`.
#[test]
fn the_committed_fixture_snapshot_is_current() {
    let dir = fixture_path("verdicts");
    let snap = format::write_snapshot(FIXTURE_DATE, &fixture_verdicts()).unwrap();
    let manifest = format!("{}\n", serde_json::to_string_pretty(&snap.manifest).unwrap());
    let files = [
        (format::MANIFEST.to_string(), manifest.into_bytes()),
        (snap.manifest.summary.file.clone(), snap.summary.clone()),
        (snap.manifest.all.file.clone(), snap.all.clone()),
    ];
    if std::env::var_os("UPNEXT_UPDATE_VERDICTS").is_some() {
        for (name, bytes) in &files {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
        return;
    }
    for (name, bytes) in &files {
        let committed = std::fs::read(dir.join(name)).unwrap_or_default();
        assert!(&committed == bytes, "tests/fixtures/verdicts/{name} is stale: run `UPNEXT_UPDATE_VERDICTS=1 cargo test verdicts`");
    }
    let m = format::read_manifest(&std::fs::read(dir.join(format::MANIFEST)).unwrap()).unwrap();
    assert_eq!(format::read_named_summary(&m, &snap.summary).unwrap().games.len(), 4);
}

/// A gzip'd data file with this header line and these lines, as is.
fn raw_file(header: &serde_json::Value, lines: &[serde_json::Value]) -> Vec<u8> {
    let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let mut h = header.clone();
    if h["sha256"] == "auto" {
        h["sha256"] = serde_json::json!(Sha256::digest(body.as_bytes()).iter().map(|b| format!("{b:02x}")).collect::<String>());
    }
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(format!("{h}\n{body}").as_bytes()).unwrap();
    gz.finish().unwrap()
}

fn header(kind: &str, count: usize, verdicts: u64) -> serde_json::Value {
    serde_json::json!({
        "upnext_verdicts": 1, "kind": kind, "date": FIXTURE_DATE, "vocab": 1, "count": count, "verdicts": verdicts,
        "sha256": "auto", "licence": "CC0-1.0", "notice": "test",
    })
}

fn game_line() -> serde_json::Value {
    serde_json::json!({
        "steam_appid": 620, "n": 3,
        "counts": {"outcome": {"finished": 3}, "worth": {"yes": 2, "no": 1}, "hours": {"5_10": 3}, "hours_by_outcome": {"finished": {"5_10": 3}}},
        "texts": [{"id": "aaaaaaaaaaaaaaaaaaaaaaaaab", "outcome": "finished", "worth": "yes", "text": "Short and sweet.", "month": "2026-10"}],
    })
}

fn refusal(gz: &[u8]) -> String {
    format::read_summary(gz).unwrap_err().to_string()
}

#[test]
fn a_summary_that_breaks_the_format_is_refused_whole() {
    assert!(format::read_summary(&raw_file(&header("summary", 1, 3), &[game_line()])).is_ok());
    let with = |f: &dyn Fn(&mut serde_json::Value)| {
        let mut l = game_line();
        f(&mut l);
        refusal(&raw_file(&header("summary", 1, 3), &[l]))
    };
    assert!(with(&|l| l["owner"] = serde_json::json!("x")).contains("unknown field"));
    assert!(with(&|l| l["counts"]["key_hash"] = serde_json::json!({})).contains("unknown field"));
    assert!(with(&|l| l["counts"]["worth"] = serde_json::json!({"yes": 2})).contains("don't add up"));
    assert!(with(&|l| l["counts"]["hours"] = serde_json::json!({"3_4": 3})).contains("isn't one of hours"));
    assert!(with(&|l| l["counts"]["clicked"] = serde_json::json!({"never": 4})).contains("more answers"));
    assert!(with(&|l| l["counts"]["hours_by_outcome"] = serde_json::json!({"finished": {"5_10": 4}})).contains("more hours"));
    assert!(with(&|l| l["texts"][0]["month"] = serde_json::json!("2026-11")).contains("month"));
    assert!(with(&|l| l["texts"][0]["id"] = serde_json::json!("not-an-id")).contains("verdict id"));
    assert!(with(&|l| l["texts"][0]["text"] = serde_json::json!("x".repeat(501))).contains("too long"));
    // Other players read the texts, so they're held to the form's checks.
    assert!(with(&|l| l["texts"][0]["text"] = serde_json::json!("Map at example\u{200b}.com")).contains("has a link"));
    assert!(with(&|l| l["texts"][0]["text"] = serde_json::json!("Ends \u{202e}moc.elpmaxe")).contains("control characters"));
    // Fewer than three verdicts aren't a summary.
    let mut two = game_line();
    two["n"] = serde_json::json!(2);
    two["counts"] = serde_json::json!({"outcome": {"finished": 2}, "worth": {"yes": 2}});
    assert!(refusal(&raw_file(&header("summary", 1, 2), &[two])).contains("at least 3"));
    // The same game twice.
    assert!(refusal(&raw_file(&header("summary", 2, 6), &[game_line(), game_line()])).contains("same game"));
    // The header's promises.
    assert!(refusal(&raw_file(&header("summary", 2, 3), &[game_line()])).contains("hold 1"));
    assert!(refusal(&raw_file(&header("summary", 1, 4), &[game_line()])).contains("add up to 3"));
    let mut h = header("summary", 1, 3);
    h["sha256"] = serde_json::json!("00");
    assert!(refusal(&raw_file(&h, &[game_line()])).contains("checksum"));
    let mut h = header("summary", 1, 3);
    h["licence"] = serde_json::json!("CC-BY-4.0");
    assert!(refusal(&raw_file(&h, &[game_line()])).contains("CC0-1.0"));
    let mut h = header("summary", 1, 3);
    h["upnext_verdicts"] = serde_json::json!(2);
    assert!(refusal(&raw_file(&h, &[game_line()])).contains("update UpNext"));
    assert!(refusal(&raw_file(&header("all", 1, 3), &[game_line()])).contains("not the Summary file"));
    assert!(refusal(b"not gzip").contains("damaged"));
}

#[test]
fn a_newer_vocabulary_drops_answers_this_build_does_not_know() {
    let mut h = header("summary", 1, 3);
    h["vocab"] = serde_json::json!(VOCAB + 1);
    let mut l = game_line();
    l["counts"]["outcome"] = serde_json::json!({"finished": 2, "replaying": 1});
    l["counts"]["sessions"] = serde_json::json!({"short": 1, "tiny": 2});
    l["counts"]["factors"] = serde_json::json!({"recap": 1, "co_op_drop_in": 3});
    l["counts"]["hours_by_outcome"] = serde_json::json!({"finished": {"5_10": 2}, "replaying": {"5_10": 1}});
    let s = format::read_summary(&raw_file(&h, &[l])).unwrap();
    let c: &GameCounts = &s.games[0].counts;
    assert_eq!(c.outcome.keys().collect::<Vec<_>>(), ["finished"]);
    assert_eq!(c.sessions.keys().collect::<Vec<_>>(), ["short"]);
    assert_eq!(c.factors.keys().collect::<Vec<_>>(), ["recap"]);
    assert_eq!(c.hours_by_outcome.keys().collect::<Vec<_>>(), ["finished"]);
    // The same answers in this build's own vocabulary are a broken file.
    let mut l = game_line();
    l["counts"]["sessions"] = serde_json::json!({"tiny": 2});
    assert!(refusal(&raw_file(&header("summary", 1, 3), &[l])).contains("isn't one of sessions"));
}

#[test]
fn the_full_file_is_checked_line_by_line() {
    let line = |f: &dyn Fn(&mut serde_json::Value)| {
        let mut v = serde_json::to_value(published(1, Some(620), None, "finished", "yes", "2026-10")).unwrap();
        f(&mut v);
        v
    };
    assert_eq!(format::read_all(&raw_file(&header("all", 1, 1), &[line(&|_| {})])).unwrap().verdicts.len(), 1);
    let refused = |l: serde_json::Value| format::read_all(&raw_file(&header("all", 1, 1), &[l])).unwrap_err().to_string();
    assert!(refused(line(&|v| v["key_hash"] = serde_json::json!("ab"))).contains("unknown field"));
    assert!(refused(line(&|v| v["day"] = serde_json::json!("2026-10-01"))).contains("unknown field"));
    assert!(refused(line(&|v| v["outcome"] = serde_json::json!("won"))).contains("outcome"));
    assert!(refused(line(&|v| v["stopped_for"] = serde_json::json!(["busy"]))).contains("stopped_for"));
    assert!(refused(line(&|v| v["vocab"] = serde_json::json!(2))).contains("vocabulary"));
    assert!(refused(line(&|v| v["month"] = serde_json::json!("2026-10-01"))).contains("month"));
    assert!(refused(line(&|v| v["text"] = serde_json::json!("mail someone@example.org"))).contains("email address"));
    assert!(refused(line(&|v| v["text"] = serde_json::json!("\u{ff45}\u{ff58}.com"))).contains("full-width"));
    let twice = [line(&|_| {}), line(&|_| {})];
    assert!(format::read_all(&raw_file(&header("all", 2, 2), &twice)).unwrap_err().to_string().contains("used before"));
}

#[test]
fn a_manifest_names_its_own_snapshot() {
    let snap = format::write_snapshot(FIXTURE_DATE, &fixture_verdicts()).unwrap();
    let mut m = serde_json::to_value(&snap.manifest).unwrap();
    m["summary"]["file"] = serde_json::json!("../verdicts-summary-2026-10-09.jsonl.gz");
    assert!(format::read_manifest(m.to_string().as_bytes()).unwrap_err().to_string().contains("aren't its snapshot's"));
    let mut m = serde_json::to_value(&snap.manifest).unwrap();
    m["licence"] = serde_json::json!("CC-BY-SA-4.0");
    assert!(format::read_manifest(m.to_string().as_bytes()).is_err());
    let mut m = snap.manifest.clone();
    m.summary.bytes += 1;
    assert!(format::read_named_summary(&m, &snap.summary).unwrap_err().to_string().contains("doesn't match its manifest"));
    // A summary that isn't the one the manifest counts.
    let other = format::write_snapshot(FIXTURE_DATE, &fixture_verdicts()[..30]).unwrap();
    let mut m = snap.manifest.clone();
    m.summary = other.manifest.summary.clone();
    assert!(format::read_named_summary(&m, &other.summary).unwrap_err().to_string().contains("isn't the snapshot"));
    assert_eq!(SHOW_AT, 3);
}

/// An hour and a half rounds to an hour, not "1 hours".
#[test]
fn a_wait_of_an_hour_and_a_half_reads_an_hour() {
    use crate::verdicts::client::wait_words;
    assert_eq!(wait_words(5_400), "an hour");
    assert_eq!(wait_words(5_401), "2 hours");
    assert_eq!(wait_words(90), "2 minutes");
}
