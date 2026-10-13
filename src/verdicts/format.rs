//! The published backlog verdicts (`docs/verdicts/snapshot.md`), version
//! 1, following the shared data's format (`seed.rs`):
//! - `verdicts.json`, the [`Manifest`], under a stable name: the snapshot's
//!   date, counts and licence, and each file's name, size and SHA-256.
//! - `verdicts-summary-<date>.jsonl.gz`: gzip'd JSON lines, a [`Header`]
//!   first, then one [`GameSummary`] per game with at least [`SHOW_AT`]
//!   verdicts. This is what installs download, always whole, so the host
//!   never learns which games anyone owns.
//! - `verdicts-all-<date>.jsonl.gz`: the same header, then every published
//!   verdict, a [`Published`] a line, for anyone who wants the raw data.
//!
//! Each header's `sha256` is of everything after its first line,
//! uncompressed, so a file checks itself wherever it came from. Everything
//! is read strictly: a field the format doesn't list refuses the file. The
//! files carry months, never days, and never a key or its hash, so nobody
//! can collect one person's verdicts or match them against their play.

use super::consent::LICENCE;
use super::verdict::{STOP_NOTE_MAX, TEXT_MAX, is_gog_id, text_refusal};
use super::vocab::{self, VOCAB};
use crate::seed::Refused;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::io::Read;

/// The format this build reads and writes.
pub const FORMAT: i64 = 1;
/// The manifest's stable name, beside the dated files.
pub const MANIFEST: &str = "verdicts.json";
/// What every header and the manifest say about the data.
#[cfg_attr(not(test), allow(dead_code))]
pub const NOTICE: &str = "UpNext backlog verdicts: players' answers about how games fit a backlog and limited time, \
dedicated to the public domain under CC0 1.0 (https://creativecommons.org/publicdomain/zero/1.0/). \
They carry no names, accounts or libraries, and nothing in them links one person's verdicts together.";

/// A game shows in the summary, and on game pages, from this many verdicts,
/// so one key can't make a summary by itself.
pub const SHOW_AT: u32 = 3;
/// Shares and "most players" lines need this many; below it the game page
/// shows plain counts.
pub const PERCENT_AT: u32 = 5;
/// The newest texts a game's summary carries.
pub const TEXTS_PER_GAME: usize = 20;

/// Limits on what is read, so a wrong or hostile file can't fill memory.
const MAX_UNPACKED: usize = 256 << 20;
pub const MAX_MANIFEST: usize = 1 << 20;
const MAX_LINES: usize = 2_000_000;

pub fn summary_file(date: &str) -> String {
    format!("verdicts-summary-{date}.jsonl.gz")
}

pub fn all_file(date: &str) -> String {
    format!("verdicts-all-{date}.jsonl.gz")
}

/// Which of the two data files a header heads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Summary,
    All,
}

/// The first line of a data file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    /// The format's version, [`FORMAT`].
    pub upnext_verdicts: i64,
    pub kind: Kind,
    /// The snapshot's day (YYYY-MM-DD).
    pub date: String,
    /// The newest vocabulary among its verdicts.
    pub vocab: i64,
    /// Lines after the header: games in a summary, verdicts in the full file.
    pub count: usize,
    /// Verdicts counted: the sum of the games' `n` in a summary.
    pub verdicts: u64,
    /// SHA-256 (hex) of everything after the first line, uncompressed.
    pub sha256: String,
    pub licence: String,
    pub notice: String,
}

/// A data file as the manifest names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRef {
    /// A plain file name, beside the manifest.
    pub file: String,
    /// Its size and SHA-256 (hex), as downloaded.
    pub bytes: u64,
    pub sha256: String,
}

/// `verdicts.json`: what the snapshot holds, read before downloading it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub upnext_verdicts: i64,
    pub date: String,
    pub vocab: i64,
    /// Games in the summary.
    pub games: usize,
    /// Verdicts published, in the full file.
    pub verdicts: u64,
    pub licence: String,
    pub notice: String,
    pub summary: FileRef,
    pub all: FileRef,
}

/// How many verdicts gave each answer. Only answers given at least once
/// appear.
pub type Counts = BTreeMap<String, u32>;

/// Every question's counts for one game.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameCounts {
    pub outcome: Counts,
    pub worth: Counts,
    #[serde(default, skip_serializing_if = "Counts::is_empty")]
    pub hours: Counts,
    #[serde(default, skip_serializing_if = "Counts::is_empty")]
    pub clicked: Counts,
    #[serde(default, skip_serializing_if = "Counts::is_empty")]
    pub sessions: Counts,
    #[serde(default, skip_serializing_if = "Counts::is_empty")]
    pub limited_time: Counts,
    #[serde(default, skip_serializing_if = "Counts::is_empty")]
    pub factors: Counts,
    #[serde(default, skip_serializing_if = "Counts::is_empty")]
    pub stop_point: Counts,
    #[serde(default, skip_serializing_if = "Counts::is_empty")]
    pub stopped_for: Counts,
    /// The hours of the verdicts with each outcome, for "Finished: usually
    /// 10 to 20 h".
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub hours_by_outcome: BTreeMap<String, Counts>,
}

/// A verdict's text or stopping note as a game's summary shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShownText {
    /// The verdict's random id, for reporting it.
    pub id: String,
    pub outcome: String,
    pub worth: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hours: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_note: Option<String>,
    /// When it was written or last changed (YYYY-MM).
    pub month: String,
}

/// One game's line in the summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steam_appid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gog_id: Option<String>,
    /// Verdicts counted.
    pub n: u32,
    pub counts: GameCounts,
    /// Up to [`TEXTS_PER_GAME`], newest month first, then by id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub texts: Vec<ShownText>,
}

/// One verdict as the full file publishes it: its random id, the game, the
/// answers, the text and the month, and nothing about who sent it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Published {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steam_appid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gog_id: Option<String>,
    pub outcome: String,
    pub worth: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hours: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clicked: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sessions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limited_time: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub factors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_point: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_note: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stopped_for: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub vocab: i64,
    /// When it was written or last changed (YYYY-MM).
    pub month: String,
}

/// A summary file, read and checked.
#[derive(Debug, Clone)]
pub struct Summary {
    pub header: Header,
    pub games: Vec<GameSummary>,
}

/// The full file, read and checked.
#[derive(Debug, Clone)]
pub struct AllVerdicts {
    pub header: Header,
    pub verdicts: Vec<Published>,
}

fn refuse<T>(why: impl Into<String>) -> Result<T, Refused> {
    Err(Refused(why.into()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn is_date(s: &str) -> bool {
    s.len() == 10 && chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok()
}

fn is_month(s: &str) -> bool {
    s.len() == 7 && is_date(&format!("{s}-01"))
}

/// A verdict's id: 16 random bytes in lowercase base32 without padding.
pub fn is_verdict_id(s: &str) -> bool {
    s.len() == 26 && s.bytes().all(|b| matches!(b, b'a'..=b'z' | b'2'..=b'7'))
}

fn plain_name(file: &str) -> bool {
    !file.is_empty() && !file.contains(['/', '\\']) && !file.starts_with('.')
}

/// Read a manifest and check what it says before anything is downloaded.
pub fn read_manifest(raw: &[u8]) -> Result<Manifest, Refused> {
    if raw.len() > MAX_MANIFEST {
        return refuse("that file is far bigger than a backlog verdicts manifest");
    }
    let m: Manifest = match serde_json::from_slice(raw) {
        Ok(m) => m,
        Err(e) => return refuse(format!("that isn't UpNext's backlog verdicts manifest ({e})")),
    };
    format_ok(m.upnext_verdicts)?;
    if m.licence != LICENCE {
        return refuse(format!("those verdicts say their licence is {:?}; UpNext reads only {LICENCE} verdicts", m.licence));
    }
    if !is_date(&m.date) {
        return refuse(format!("the verdicts' date isn't a date: {:?}", m.date));
    }
    if m.summary.file != summary_file(&m.date) || m.all.file != all_file(&m.date) || !plain_name(&m.summary.file) {
        return refuse("the manifest names files that aren't its snapshot's");
    }
    Ok(m)
}

/// Check a downloaded file against the manifest's entry for it.
pub fn matches(entry: &FileRef, data: &[u8]) -> Result<(), Refused> {
    if data.len() as u64 != entry.bytes || sha256_hex(data) != entry.sha256.to_ascii_lowercase() {
        return refuse(format!("{} doesn't match its manifest: it's damaged or was changed", entry.file));
    }
    Ok(())
}

fn format_ok(version: i64) -> Result<(), Refused> {
    if version > FORMAT {
        return refuse(format!("those verdicts are in format {version}, and this UpNext reads format {FORMAT}; update UpNext to use them"));
    }
    if version != FORMAT {
        return refuse(format!("those verdicts are in format {version}, which this UpNext doesn't read"));
    }
    Ok(())
}

/// A data file's lines after the header, each with its line number.
type Lines = Vec<(usize, Vec<u8>)>;

/// Unpack a data file and check its header and checksum; the lines after
/// the header come back with their line numbers.
fn unpack(gz: &[u8], kind: Kind) -> Result<(Header, Lines), Refused> {
    let mut text = Vec::new();
    let mut unpacked = flate2::read::GzDecoder::new(gz).take(MAX_UNPACKED as u64 + 1);
    if let Err(e) = unpacked.read_to_end(&mut text) {
        return refuse(format!("that file isn't UpNext's backlog verdicts, or it's damaged ({e})"));
    }
    if text.len() > MAX_UNPACKED {
        return refuse("that file is far bigger than UpNext's backlog verdicts");
    }
    let (first, rest) = match text.iter().position(|&b| b == b'\n') {
        Some(at) => (&text[..at], &text[at + 1..]),
        None => (&text[..], &[][..]),
    };
    let header: Header = match serde_json::from_slice(first) {
        Ok(h) => h,
        Err(e) => return refuse(format!("that file isn't UpNext's backlog verdicts: its first line isn't the header ({e})")),
    };
    format_ok(header.upnext_verdicts)?;
    if header.kind != kind {
        return refuse(format!("that is the verdicts' {:?} file, not the {kind:?} file", header.kind));
    }
    if header.licence != LICENCE {
        return refuse(format!("those verdicts say their licence is {:?}; UpNext reads only {LICENCE} verdicts", header.licence));
    }
    if !is_date(&header.date) {
        return refuse(format!("the verdicts' date isn't a date: {:?}", header.date));
    }
    if header.vocab < 1 {
        return refuse(format!("the verdicts' vocabulary {} isn't one", header.vocab));
    }
    if sha256_hex(rest) != header.sha256.to_ascii_lowercase() {
        return refuse("those verdicts don't match their checksum: the file is damaged or was changed");
    }
    let mut lines = vec![];
    for (i, line) in rest.split(|&b| b == b'\n').enumerate() {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        lines.push((i + 2, line.to_vec()));
        if lines.len() > MAX_LINES {
            return refuse("that file has far more lines than UpNext's backlog verdicts");
        }
    }
    if lines.len() != header.count {
        return refuse(format!("those verdicts say they have {} lines after the header but hold {}", header.count, lines.len()));
    }
    Ok((header, lines))
}

/// How strictly a file's answers are read: those from this build's
/// vocabulary or an older one must all be known; a newer vocabulary's
/// unknown answers are dropped, so a new answer doesn't need a new format.
#[derive(Clone, Copy)]
struct Answers {
    newer: bool,
}

impl Answers {
    fn of(header: &Header) -> Self {
        Answers { newer: header.vocab > VOCAB }
    }

    /// Keep the known answers of a count; refuse an unknown one unless the
    /// vocabulary is newer, and any zero.
    fn counts(self, at: usize, name: &str, counts: &mut Counts, known: impl Fn(&str) -> bool) -> Result<(), Refused> {
        if let Some((code, _)) = counts.iter().find(|(_, n)| **n == 0) {
            return refuse(format!("line {at} counts {name} {code:?} zero times"));
        }
        if let Some(code) = counts.keys().find(|c| !known(c)) {
            if !self.newer {
                return refuse(format!("line {at} has {code:?}, which isn't one of {name}'s answers"));
            }
            counts.retain(|c, _| known(c));
        }
        Ok(())
    }

    fn one(self, at: usize, f: &vocab::Field, v: &mut Option<String>) -> Result<(), Refused> {
        if let Some(code) = v.as_deref()
            && !f.has(code)
        {
            if !self.newer {
                return refuse(format!("line {at} has {code:?}, which isn't one of {}'s answers", f.name));
            }
            *v = None;
        }
        Ok(())
    }

    fn many(self, at: usize, name: &str, list: &mut Vec<String>, known: impl Fn(&str) -> bool) -> Result<(), Refused> {
        if let Some(code) = list.iter().find(|c| !known(c)) {
            if !self.newer {
                return refuse(format!("line {at} has {code:?}, which isn't one of {name}'s answers"));
            }
            list.retain(|c| known(c));
        }
        Ok(())
    }
}

fn game_ok(at: usize, steam_appid: Option<u32>, gog_id: Option<&str>) -> Result<(), Refused> {
    if steam_appid.is_none() && gog_id.is_none() {
        return refuse(format!("line {at} has neither a Steam appid nor a GOG id"));
    }
    if steam_appid == Some(0) || gog_id.is_some_and(|g| !is_gog_id(g)) {
        return refuse(format!("line {at} has a game id that isn't one"));
    }
    Ok(())
}

fn typed_ok(at: usize, text: Option<&str>, stop_note: Option<&str>) -> Result<(), Refused> {
    let long = |s: Option<&str>, max: usize| s.is_some_and(|s| s.trim().is_empty() || s.chars().count() > max);
    if long(text, TEXT_MAX) || long(stop_note, STOP_NOTE_MAX) {
        return refuse(format!("line {at} has a text that's empty or too long"));
    }
    // Other players read these texts, so a snapshot is held to the same
    // checks as the form, in case the service let one through.
    if let Some(c) = [text, stop_note].into_iter().flatten().find_map(text_refusal) {
        return refuse(format!("line {at} has a text that {}", c.why));
    }
    Ok(())
}

fn month_ok(at: usize, month: &str, date: &str) -> Result<(), Refused> {
    // Months are YYYY-MM, so they compare as text with the date's month.
    if !is_month(month) || month > &date[..7] {
        return refuse(format!("line {at} has a month that isn't one, or is after the snapshot: {month:?}"));
    }
    Ok(())
}

/// Read a summary file and check it whole. Any fault refuses the file.
pub fn read_summary(gz: &[u8]) -> Result<Summary, Refused> {
    let (header, lines) = unpack(gz, Kind::Summary)?;
    let answers = Answers::of(&header);
    let mut games = Vec::with_capacity(lines.len());
    let mut seen_appid: HashMap<u32, usize> = HashMap::new();
    let mut seen_gog: HashMap<String, usize> = HashMap::new();
    let mut total: u64 = 0;
    for (at, line) in lines {
        let mut g: GameSummary = match serde_json::from_slice(&line) {
            Ok(g) => g,
            Err(e) => return refuse(format!("line {at} of those verdicts isn't a game's summary ({e})")),
        };
        game_ok(at, g.steam_appid, g.gog_id.as_deref())?;
        if let Some(a) = g.steam_appid
            && let Some(was) = seen_appid.insert(a, at)
        {
            return refuse(format!("lines {was} and {at} are the same game"));
        }
        if let Some(gog) = &g.gog_id
            && let Some(was) = seen_gog.insert(gog.clone(), at)
        {
            return refuse(format!("lines {was} and {at} are the same game"));
        }
        if g.n < SHOW_AT {
            return refuse(format!("line {at} summarises {} verdicts; a summary needs at least {SHOW_AT}", g.n));
        }
        check_counts(at, &mut g, answers)?;
        if g.texts.len() > TEXTS_PER_GAME {
            return refuse(format!("line {at} has more than {TEXTS_PER_GAME} texts"));
        }
        for t in &mut g.texts {
            if !is_verdict_id(&t.id) {
                return refuse(format!("line {at} has a text whose verdict id isn't one"));
            }
            if t.text.is_none() && t.stop_note.is_none() {
                return refuse(format!("line {at} has a text with nothing in it"));
            }
            typed_ok(at, t.text.as_deref(), t.stop_note.as_deref())?;
            month_ok(at, &t.month, &header.date)?;
            if !vocab::OUTCOME.has(&t.outcome) || !vocab::WORTH.has(&t.worth) {
                if !answers.newer {
                    return refuse(format!("line {at} has a text with an answer that isn't one"));
                }
                t.outcome.clear();
            }
            answers.one(at, &vocab::HOURS, &mut t.hours)?;
        }
        // A newer vocabulary's text with an outcome this build doesn't know
        // can't be shown with it.
        g.texts.retain(|t| !t.outcome.is_empty());
        total += u64::from(g.n);
        games.push(g);
    }
    if total != header.verdicts {
        return refuse(format!("those verdicts say they count {} but their games add up to {total}", header.verdicts));
    }
    Ok(Summary { header, games })
}

/// Read the summary a manifest names, downloaded beside it: its size and
/// checksum, then the file itself, which must be the snapshot the manifest
/// describes.
pub fn read_named_summary(m: &Manifest, data: &[u8]) -> Result<Summary, Refused> {
    matches(&m.summary, data)?;
    let s = read_summary(data)?;
    if s.header.date != m.date || s.header.count != m.games {
        return refuse(format!("{} isn't the snapshot its manifest describes", m.summary.file));
    }
    Ok(s)
}

/// The counts must fit the game's `n`: every verdict has an outcome and a
/// worth, and no answer is given by more verdicts than there are.
fn check_counts(at: usize, g: &mut GameSummary, answers: Answers) -> Result<(), Refused> {
    let n = g.n;
    let c = &mut g.counts;
    let field = |f: &'static vocab::Field| move |code: &str| f.has(code);
    answers.counts(at, "outcome", &mut c.outcome, field(&vocab::OUTCOME))?;
    answers.counts(at, "worth", &mut c.worth, field(&vocab::WORTH))?;
    answers.counts(at, "hours", &mut c.hours, field(&vocab::HOURS))?;
    answers.counts(at, "clicked", &mut c.clicked, field(&vocab::CLICKED))?;
    answers.counts(at, "sessions", &mut c.sessions, field(&vocab::SESSIONS))?;
    answers.counts(at, "limited_time", &mut c.limited_time, field(&vocab::LIMITED_TIME))?;
    answers.counts(at, "factors", &mut c.factors, vocab::is_factor)?;
    answers.counts(at, "stop_point", &mut c.stop_point, field(&vocab::STOP_POINT))?;
    answers.counts(at, "stopped_for", &mut c.stopped_for, vocab::is_stop_reason)?;
    if c.hours_by_outcome.keys().any(|o| !vocab::OUTCOME.has(o)) {
        if !answers.newer {
            return refuse(format!("line {at} has hours for an outcome that isn't one"));
        }
        c.hours_by_outcome.retain(|o, _| vocab::OUTCOME.has(o));
    }
    for (outcome, hours) in &mut c.hours_by_outcome {
        answers.counts(at, "hours", hours, field(&vocab::HOURS))?;
        if hours.values().sum::<u32>() > c.outcome.get(outcome).copied().unwrap_or(0) {
            return refuse(format!("line {at} has more hours for {outcome:?} than verdicts with it"));
        }
    }
    let sum = |m: &Counts| m.values().map(|v| u64::from(*v)).sum::<u64>();
    // A newer vocabulary's dropped answers leave the sums short.
    let whole = |m: &Counts| if answers.newer { sum(m) <= u64::from(n) } else { sum(m) == u64::from(n) };
    if !whole(&c.outcome) || !whole(&c.worth) {
        return refuse(format!("line {at}'s outcomes or worth don't add up to its {n} verdicts"));
    }
    let singles = [&c.hours, &c.clicked, &c.sessions, &c.limited_time, &c.stop_point];
    if singles.iter().any(|m| sum(m) > u64::from(n)) {
        return refuse(format!("line {at} counts more answers to one question than its {n} verdicts"));
    }
    if c.factors.values().chain(c.stopped_for.values()).any(|v| *v > n) {
        return refuse(format!("line {at} counts a factor or a reason more often than its {n} verdicts"));
    }
    Ok(())
}

/// Read the full file and check it whole. Installs don't download it; this
/// is for tests and for anyone checking the service's output.
pub fn read_all(gz: &[u8]) -> Result<AllVerdicts, Refused> {
    let (header, lines) = unpack(gz, Kind::All)?;
    if header.verdicts != header.count as u64 {
        return refuse("the full file's verdicts aren't its lines");
    }
    let answers = Answers::of(&header);
    let mut verdicts = Vec::with_capacity(lines.len());
    let mut ids = std::collections::HashSet::new();
    for (at, line) in lines {
        let mut v: Published = match serde_json::from_slice(&line) {
            Ok(v) => v,
            Err(e) => return refuse(format!("line {at} of those verdicts isn't a verdict ({e})")),
        };
        if !is_verdict_id(&v.id) || !ids.insert(v.id.clone()) {
            return refuse(format!("line {at} has a verdict id that isn't one, or one used before"));
        }
        game_ok(at, v.steam_appid, v.gog_id.as_deref())?;
        if v.vocab < 1 || v.vocab > header.vocab {
            return refuse(format!("line {at}'s vocabulary {} isn't the header's or older", v.vocab));
        }
        let (mut outcome, mut worth) = (Some(v.outcome.clone()), Some(v.worth.clone()));
        answers.one(at, &vocab::OUTCOME, &mut outcome)?;
        answers.one(at, &vocab::WORTH, &mut worth)?;
        answers.one(at, &vocab::HOURS, &mut v.hours)?;
        answers.one(at, &vocab::CLICKED, &mut v.clicked)?;
        answers.one(at, &vocab::SESSIONS, &mut v.sessions)?;
        answers.one(at, &vocab::LIMITED_TIME, &mut v.limited_time)?;
        answers.one(at, &vocab::STOP_POINT, &mut v.stop_point)?;
        answers.many(at, "factors", &mut v.factors, vocab::is_factor)?;
        answers.many(at, "stopped_for", &mut v.stopped_for, vocab::is_stop_reason)?;
        typed_ok(at, v.text.as_deref(), v.stop_note.as_deref())?;
        month_ok(at, &v.month, &header.date)?;
        // A newer vocabulary's verdict whose required answers this build
        // doesn't know can't be counted.
        if outcome.is_none() || worth.is_none() {
            continue;
        }
        verdicts.push(v);
    }
    Ok(AllVerdicts { header, verdicts })
}

/// Sum up published verdicts into the summary's games, as the service's
/// daily run does: games with fewer than [`SHOW_AT`] verdicts are left
/// out, and each keeps its [`TEXTS_PER_GAME`] newest texts, by month and
/// then by id, so their order says nothing finer than the month.
///
/// Verdicts are one game's when they share a Steam appid, or a GOG id that
/// a verdict with both ids ties to an appid; a verdict with only a GOG id
/// nobody tied to an appid is a game of its own.
pub fn summarise(verdicts: &[Published]) -> Vec<GameSummary> {
    let mut appid_of_gog: HashMap<&str, BTreeMap<u32, u32>> = HashMap::new();
    for v in verdicts {
        if let (Some(a), Some(g)) = (v.steam_appid, v.gog_id.as_deref()) {
            *appid_of_gog.entry(g).or_default().entry(a).or_default() += 1;
        }
    }
    // The appid most verdicts tie a GOG id to, the lowest on a tie.
    let tied = |g: &str| appid_of_gog.get(g).and_then(|m| m.iter().max_by_key(|(a, n)| (**n, std::cmp::Reverse(**a))).map(|(a, _)| *a));
    #[derive(PartialEq, Eq, PartialOrd, Ord, Clone)]
    enum Key {
        Steam(u32),
        Gog(String),
    }
    let mut groups: BTreeMap<Key, Vec<&Published>> = BTreeMap::new();
    for v in verdicts {
        let key = match (v.steam_appid, v.gog_id.as_deref()) {
            (Some(a), _) => Key::Steam(a),
            (None, Some(g)) => tied(g).map_or_else(|| Key::Gog(g.to_string()), Key::Steam),
            (None, None) => continue,
        };
        groups.entry(key).or_default().push(v);
    }
    let mut out = vec![];
    for (key, list) in groups {
        let n = list.len() as u32;
        if n < SHOW_AT {
            continue;
        }
        let mut c = GameCounts::default();
        let add = |m: &mut Counts, v: Option<&String>| {
            if let Some(v) = v {
                *m.entry(v.clone()).or_default() += 1;
            }
        };
        // The GOG id most of the game's verdicts carry, the lowest on a tie.
        let mut gogs: BTreeMap<&str, u32> = BTreeMap::new();
        for v in &list {
            add(&mut c.outcome, Some(&v.outcome));
            add(&mut c.worth, Some(&v.worth));
            add(&mut c.hours, v.hours.as_ref());
            add(&mut c.clicked, v.clicked.as_ref());
            add(&mut c.sessions, v.sessions.as_ref());
            add(&mut c.limited_time, v.limited_time.as_ref());
            add(&mut c.stop_point, v.stop_point.as_ref());
            for f in &v.factors {
                add(&mut c.factors, Some(f));
            }
            for r in &v.stopped_for {
                add(&mut c.stopped_for, Some(r));
            }
            if v.hours.is_some() {
                add(c.hours_by_outcome.entry(v.outcome.clone()).or_default(), v.hours.as_ref());
            }
            if let Some(g) = v.gog_id.as_deref() {
                *gogs.entry(g).or_default() += 1;
            }
        }
        let mut texts: Vec<&&Published> = list.iter().filter(|v| v.text.is_some() || v.stop_note.is_some()).collect();
        texts.sort_by(|a, b| b.month.cmp(&a.month).then_with(|| a.id.cmp(&b.id)));
        let texts = texts
            .into_iter()
            .take(TEXTS_PER_GAME)
            .map(|v| ShownText {
                id: v.id.clone(),
                outcome: v.outcome.clone(),
                worth: v.worth.clone(),
                hours: v.hours.clone(),
                text: v.text.clone(),
                stop_note: v.stop_note.clone(),
                month: v.month.clone(),
            })
            .collect();
        let gog_id = gogs.iter().max_by_key(|(g, n)| (**n, std::cmp::Reverse(*g))).map(|(g, _)| g.to_string());
        let (steam_appid, gog_id) = match key {
            Key::Steam(a) => (Some(a), gog_id),
            Key::Gog(g) => (None, Some(g)),
        };
        out.push(GameSummary { steam_appid, gog_id, n, counts: c, texts });
    }
    out
}

// The writers make the fixtures: the tests' and the mock library's
// snapshot. The service writes the real files.

/// Write a data file of `kind` with these lines, its header made from them.
fn write_lines<T: Serialize>(kind: Kind, date: &str, vocab: i64, verdicts: u64, lines: &[T]) -> Result<(Header, Vec<u8>)> {
    use std::io::Write;
    let mut body = Vec::new();
    for l in lines {
        serde_json::to_writer(&mut body, l)?;
        body.push(b'\n');
    }
    let header = Header {
        upnext_verdicts: FORMAT,
        kind,
        date: date.into(),
        vocab,
        count: lines.len(),
        verdicts,
        sha256: sha256_hex(&body),
        licence: LICENCE.into(),
        notice: NOTICE.into(),
    };
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    serde_json::to_writer(&mut gz, &header)?;
    gz.write_all(b"\n")?;
    gz.write_all(&body)?;
    Ok((header, gz.finish()?))
}

/// A whole snapshot written from published verdicts: the two data files and
/// the manifest that names them. The service writes the same in its daily
/// run; this writes the fixtures the tests and the mock library read.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
pub struct Snapshot {
    pub manifest: Manifest,
    pub summary: Vec<u8>,
    pub all: Vec<u8>,
}

pub fn write_snapshot(date: &str, verdicts: &[Published]) -> Result<Snapshot> {
    let vocab = verdicts.iter().map(|v| v.vocab).max().unwrap_or(VOCAB);
    let games = summarise(verdicts);
    let counted = games.iter().map(|g| u64::from(g.n)).sum();
    let (_, summary) = write_lines(Kind::Summary, date, vocab, counted, &games)?;
    let (_, all) = write_lines(Kind::All, date, vocab, verdicts.len() as u64, verdicts)?;
    let entry = |file: String, data: &[u8]| FileRef { file, bytes: data.len() as u64, sha256: sha256_hex(data) };
    let manifest = Manifest {
        upnext_verdicts: FORMAT,
        date: date.into(),
        vocab,
        games: games.len(),
        verdicts: verdicts.len() as u64,
        licence: LICENCE.into(),
        notice: NOTICE.into(),
        summary: entry(summary_file(date), &summary),
        all: entry(all_file(date), &all),
    };
    Ok(Snapshot { manifest, summary, all })
}
