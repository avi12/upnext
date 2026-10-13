//! `verdicts contract|draft|check|list|send|withdraw|refresh`: backlog
//! verdicts from the terminal. `contract`, `draft` and `check`
//! send nothing and change nothing: they print the contract the service
//! checks against, the verdict UpNext would pre-fill for a game, and what a
//! downloaded snapshot holds. `send` sends one verdict after showing it and
//! the consent text, `withdraw` takes one or all down, `list` shows what
//! this computer keeps and `refresh` asks the service what it has.

use super::format::{bold, dim, green};
use super::open_db;
use crate::config::Config;
use crate::enrich::plural;
use crate::verdicts::mine::{self, VerdictAnswers};
use crate::verdicts::{self, consent, draft, format, vocab};
use anyhow::{Context, Result};
use std::path::Path;

/// Print the contract as `docs/verdicts-contract.json` holds it.
pub fn contract() -> Result<()> {
    print!("{}", verdicts::contract_text());
    Ok(())
}

/// Print the verdict UpNext would pre-fill for a game, and where each
/// answer came from.
pub fn draft(cfg: &Config, r#ref: &str, json: bool) -> Result<()> {
    let (db, cfg) = open_db(cfg)?;
    let g = db.resolve(cfg.whose(), r#ref)?.ok_or_else(|| anyhow::anyhow!("unknown game ref {ref:?}", ref = r#ref))?;
    let d = draft::for_title(&db, &cfg, g.id)?.expect("the game was just found");
    if json {
        println!("{}", serde_json::to_string_pretty(&d)?);
        return Ok(());
    }
    println!("{}", bold(&d.name));
    if let Some(why) = &d.cant_send {
        println!("{}", dim(why));
    }
    let v = &d.verdict;
    let from = |field: &str| match d.from.get(field) {
        Some(source) => dim(&format!(" (from {})", source_words(*source))),
        None => String::new(),
    };
    let one = |f: &vocab::Field, value: &Option<String>| {
        let answer = value.as_deref().and_then(|c| f.label(c)).unwrap_or("(yours to answer)");
        println!("  {} {answer}{}", f.question, from(f.name));
    };
    one(&vocab::OUTCOME, &v.outcome);
    one(&vocab::WORTH, &v.worth);
    one(&vocab::HOURS, &v.hours);
    one(&vocab::CLICKED, &v.clicked);
    one(&vocab::SESSIONS, &v.sessions);
    one(&vocab::LIMITED_TIME, &v.limited_time);
    one(&vocab::STOP_POINT, &v.stop_point);
    if !v.stopped_for.is_empty() {
        let labels: Vec<&str> =
            v.stopped_for.iter().filter_map(|c| vocab::stop_reasons().find(|r| r.code == c.as_str()).map(|r| r.label)).collect();
        println!("  What stopped you? {}{}", labels.join(", "), from("stopped_for"));
    }
    let left: Vec<String> = d.problems.iter().filter(|p| p.field != "game").map(|p| format!("{} {}", p.field, p.why)).collect();
    if !left.is_empty() {
        println!("{}", dim(&format!("Before it could be sent: {}.", left.join("; "))));
    }
    println!(
        "{}",
        dim(&format!(
            "Nothing was sent: this is what UpNext would fill in. Sending asks first: \"{}\" ({}, consent version {}).",
            consent::TITLE,
            consent::LICENCE,
            consent::CONSENT_VERSION
        ))
    );
    Ok(())
}

fn source_words(s: draft::Source) -> &'static str {
    match s {
        draft::Source::Mark => "your mark",
        draft::Source::Playing => "playing it now",
        draft::Source::Playtime => "your playtime",
        draft::Source::Diary => "your play diary",
        draft::Source::StopReasons => "why you stopped",
        draft::Source::NoEnding => "it having no ending",
    }
}

/// Read a snapshot from disk and say what it holds: a folder or a manifest
/// (`verdicts.json`) with its files beside it, or one data file. With the
/// full file beside the manifest, the summary must be what the full file
/// sums up to, which checks the service's daily run. Nothing is applied.
pub fn check(from: &Path) -> Result<()> {
    let read = |p: &Path| std::fs::read(p).with_context(|| format!("couldn't read {}", p.display()));
    let joined;
    let from = if from.is_dir() {
        joined = from.join(format::MANIFEST);
        joined.as_path()
    } else {
        from
    };
    let name = from.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if name.ends_with(".json") {
        let m = format::read_manifest(&read(from)?)?;
        let summary = format::read_named_summary(&m, &read(&from.with_file_name(&m.summary.file))?)?;
        println!(
            "{} Backlog verdicts of {}: {} with at least {}, {} published, {}",
            green("✓"),
            m.date,
            plural(summary.games.len(), "game", "games"),
            plural(format::SHOW_AT, "verdict", "verdicts"),
            plural(m.verdicts, "verdict", "verdicts"),
            m.licence
        );
        let all_at = from.with_file_name(&m.all.file);
        if all_at.exists() {
            let data = read(&all_at)?;
            format::matches(&m.all, &data)?;
            let all = format::read_all(&data)?;
            if format::summarise(&all.verdicts) != summary.games {
                anyhow::bail!("{} isn't what {} sums up to", m.summary.file, m.all.file);
            }
            println!(
                "{} {}: {}, and the summary is what {} up to",
                green("✓"),
                m.all.file,
                plural(all.verdicts.len(), "verdict", "verdicts"),
                if all.verdicts.len() == 1 { "it sums" } else { "they sum" }
            );
        }
        return Ok(());
    }
    let data = read(from)?;
    if name.contains("-all-") {
        let all = format::read_all(&data)?;
        println!(
            "{} Every backlog verdict of {}: {}, {}",
            green("✓"),
            all.header.date,
            plural(all.verdicts.len(), "verdict", "verdicts"),
            all.header.licence
        );
    } else {
        let s = format::read_summary(&data)?;
        println!(
            "{} Backlog verdicts of {}: {}, {} counted, {}",
            green("✓"),
            s.header.date,
            plural(s.games.len(), "game", "games"),
            plural(s.header.verdicts, "verdict", "verdicts"),
            s.header.licence
        );
    }
    Ok(())
}

/// The answers typed on the command line, over the saved or pre-filled ones.
#[derive(Debug, Default, Clone, clap::Args)]
pub struct AnswerArgs {
    /// Where you left it: finished, playing, shelved, stopped, endless or bounced
    #[arg(long)]
    pub outcome: Option<String>,
    /// Worth it on a backlog: yes, for_some or no
    #[arg(long)]
    pub worth: Option<String>,
    /// About how long you played: under_2, 2_5, 5_10, 10_20, 20_40, 40_80 or 80_plus
    #[arg(long)]
    pub hours: Option<String>,
    /// When it clicked: first_hour, few_hours, later, much_later or never
    #[arg(long)]
    pub clicked: Option<String>,
    /// How long a session needs to be: short, medium or long
    #[arg(long)]
    pub sessions: Option<String>,
    /// Whether it respects limited time: yes, mostly or no
    #[arg(long)]
    pub limited_time: Option<String>,
    /// What helps or hurts (up to 3; repeat the flag), such as save_anywhere or padding
    #[arg(long = "factor")]
    pub factors: Vec<String>,
    /// When it's fine to stop: at_credits, before_endgame, anytime or push_through
    #[arg(long)]
    pub stop_point: Option<String>,
    /// A note on when to stop, up to 80 characters ("after chapter 3")
    #[arg(long)]
    pub stop_note: Option<String>,
    /// What stopped you, for shelved, stopped and bounced (repeat the flag), such as grind
    #[arg(long)]
    pub stopped_for: Vec<String>,
    /// Anything else a busy player should know, up to 500 characters. Public, CC0
    #[arg(long)]
    pub text: Option<String>,
}

impl AnswerArgs {
    fn over(self, mut a: VerdictAnswers) -> VerdictAnswers {
        let take = |new: Option<String>, old: &mut Option<String>| {
            if new.is_some() {
                *old = new;
            }
        };
        take(self.outcome, &mut a.outcome);
        take(self.worth, &mut a.worth);
        take(self.hours, &mut a.hours);
        take(self.clicked, &mut a.clicked);
        take(self.sessions, &mut a.sessions);
        take(self.limited_time, &mut a.limited_time);
        take(self.stop_point, &mut a.stop_point);
        take(self.stop_note, &mut a.stop_note);
        take(self.text, &mut a.text);
        if !self.factors.is_empty() {
            a.factors = self.factors;
        }
        if !self.stopped_for.is_empty() {
            a.stopped_for = self.stopped_for;
        }
        a
    }
}

fn state_words(s: crate::db::VerdictState) -> &'static str {
    use crate::db::VerdictState::*;
    match s {
        Draft => "draft, not sent",
        Queued => "waiting to be sent",
        Sent => "sent",
        Held => "sent, held by the service for now",
        Withdrawn => "taken down",
        Removed => "removed by the service's moderators",
    }
}

fn day(t: i64) -> String {
    chrono::DateTime::from_timestamp(t, 0).map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default()
}

/// The service's refusal or what went wrong, said once.
fn said(e: anyhow::Error) -> anyhow::Error {
    match e.downcast::<mine::Refused>() {
        Ok(r) => anyhow::anyhow!("{r}"),
        Err(e) => e,
    }
}

/// List this player's verdicts as this computer keeps them.
pub fn list(cfg: &Config, json: bool) -> Result<()> {
    let (db, cfg) = open_db(cfg)?;
    let o = mine::overview(&db, &cfg, cfg.whose())?;
    if json {
        println!("{}", serde_json::to_string_pretty(&o)?);
        return Ok(());
    }
    if o.verdicts.is_empty() {
        println!("{}", dim("No backlog verdicts yet. `upnext verdicts draft <game>` shows what UpNext would fill in."));
    }
    for v in &o.verdicts {
        let when = v.sent_at.map(|t| format!(", {}", day(t))).unwrap_or_default();
        println!("{} #{}  {}{}", bold(&v.name), v.title_id, state_words(v.state), when);
        if let Some(e) = &v.error {
            println!("  {}", dim(e));
        }
    }
    if !o.service {
        println!("{}", dim("No verdicts service is set (verdicts_url), so nothing can be sent yet."));
    }
    Ok(())
}

/// Send a verdict about a game: show it and the consent text, ask, send.
/// `yes` skips the question, and only once the consent was accepted.
pub async fn send(cfg: &Config, r#ref: &str, args: AnswerArgs, yes: bool) -> Result<()> {
    let (db, cfg) = open_db(cfg)?;
    let p = cfg.whose();
    let g = db.resolve(p, r#ref)?.ok_or_else(|| anyhow::anyhow!("unknown game ref {ref:?}", ref = r#ref))?;
    let form = mine::form(&db, &cfg, p, g.id).map_err(said)?;
    if let Some(why) = &form.cant_send {
        anyhow::bail!("{why}");
    }
    let answers = args.over(form.answers.clone());
    println!("{}", bold(&form.name));
    println!("{}", serde_json::to_string_pretty(&answers)?);
    let consent = &form.consent;
    if yes && !consent.current {
        anyhow::bail!(
            "accept the consent screen first, in the app or by running this without --yes: everything you send is public under CC0"
        );
    }
    if !yes {
        if !consent.current {
            println!("\n{}", bold(consent.title));
            for line in &consent.paragraphs {
                println!("{line}");
            }
        }
        let question = if consent.current { "Send it? [y/N] " } else { "Publish my verdicts under CC0, and send this one? [y/N] " };
        print!("\n{question}");
        use std::io::Write;
        std::io::stdout().flush()?;
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        if !matches!(line.trim(), "y" | "Y" | "yes") {
            println!("{}", dim("Nothing was sent."));
            return Ok(());
        }
        if !consent.current {
            mine::accept_consent(&db, &cfg, p, consent::CONSENT_VERSION)?;
        }
    }
    let http = crate::http::client()?;
    let sent = mine::send(&db, &http, &cfg, p, g.id, answers).await.map_err(said)?;
    println!("{} {}", green("✓"), sent.message);
    Ok(())
}

/// Take one verdict down, or all of them and forget the key.
pub async fn withdraw(cfg: &Config, r#ref: Option<&str>, all: bool) -> Result<()> {
    let (db, cfg) = open_db(cfg)?;
    let p = cfg.whose();
    let http = crate::http::client()?;
    if all {
        let w = mine::withdraw_all(&db, &http, &cfg, p).await.map_err(said)?;
        println!(
            "{} {} taken down{}.",
            green("✓"),
            plural(w.withdrawn, "verdict", "verdicts"),
            if w.forgot_key { ", and this player's key is forgotten" } else { "" }
        );
        return Ok(());
    }
    let r#ref = r#ref.ok_or_else(|| anyhow::anyhow!("name a game, or --all"))?;
    let g = db.resolve(p, r#ref)?.ok_or_else(|| anyhow::anyhow!("unknown game ref {ref:?}", ref = r#ref))?;
    match mine::withdraw(&db, &http, &cfg, p, g.id).await.map_err(said)? {
        Some(_) => println!("{} Taken down: it's left out of the next daily update and gone from the service within a week.", green("✓")),
        None => println!("{} The draft is forgotten; it was never sent.", green("✓")),
    }
    Ok(())
}

/// Ask the service which verdicts this player's key sent.
pub async fn refresh(cfg: &Config) -> Result<()> {
    let (db, cfg) = open_db(cfg)?;
    let http = crate::http::client()?;
    let r = mine::refresh(&db, &http, &cfg, cfg.whose()).await.map_err(said)?;
    println!(
        "{} The service lists {} of your verdicts: {} new here, {} changed, {} about games not in this library.",
        green("✓"),
        r.listed,
        r.added,
        r.changed,
        r.unmatched
    );
    Ok(())
}
