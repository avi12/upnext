//! UpNext's shared game data: People say, Vibes and review tags for
//! popular games, written once and published under CC0 with each release,
//! so a new install has summaries for most of its library without fetching a
//! review or calling an AI. This is the side that reads and applies the
//! data; `build` makes it.
//!
//! The format, version 1:
//! - `upnext-seed-<date>.jsonl.gz`: gzip'd JSON lines. The first line is the
//!   [`Header`]; every other line is one game, an [`Entry`]. The header's
//!   `sha256` is of everything after the first line, uncompressed, so the
//!   file checks itself wherever it came from.
//! - `upnext-seed.json`: the [`Manifest`] beside it on a release, with the
//!   header's facts and the data file's name, size and SHA-256, so the
//!   first-run wizard can say what it offers before downloading anything.
//!
//! Both are read strictly: a field the format doesn't list refuses the file.
//! That is the allowlist of decision 6 in docs/THIRD_PARTY.md: identifiers, the three
//! AI-written texts and how and when they were written, nothing else.

use crate::config::Config;
use crate::db::PlayerId;
use crate::db::{Db, GameView, SeedApply, SeededDigest, Status, now};
use crate::digest::{DIGEST_MAX_CHARS, VIBES_MAX_CHARS};
use crate::review_tags::{MOOD, Reply, ReviewTags, SKILLS, Verdict};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The format this build reads and writes.
pub const FORMAT: i64 = 1;
pub const LICENCE: &str = "CC0-1.0";
/// What the manifest and the header say about the data's licence.
pub const NOTICE: &str = "UpNext's shared game data: AI-written summaries (People say, Vibes) and review tags, \
dedicated to the public domain under CC0 1.0 (https://creativecommons.org/publicdomain/zero/1.0/). \
It holds no player reviews, store descriptions or personal data. The games' names belong to their owners.";
/// Where the newest release's manifest downloads from: what the wizard and
/// Settings offer by default.
pub const DEFAULT_URL: &str = "https://github.com/zytocc/upnext/releases/latest/download/upnext-seed.json";

/// Limits on what is read, so a wrong or hostile file can't fill memory.
/// The real data is about 3 MB compressed for 10,000 games.
pub const MAX_DOWNLOAD: usize = 64 << 20;
const MAX_UNPACKED: usize = 256 << 20;
const MAX_MANIFEST: usize = 1 << 20;
const MAX_ENTRIES: usize = 200_000;

/// The data file's name for the data of `date`.
pub fn file_name(date: &str) -> String {
    format!("upnext-seed-{date}.jsonl.gz")
}

/// The first line of the data file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    /// The format's version, [`FORMAT`].
    pub upnext_seed: i64,
    /// When the data was built (YYYY-MM-DD).
    pub date: String,
    /// How many games follow.
    pub count: usize,
    /// SHA-256 (hex) of everything after the first line, uncompressed.
    pub sha256: String,
    pub licence: String,
    pub notice: String,
}

/// What a release says about its data, beside the data file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = SeedManifest)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub upnext_seed: i64,
    pub date: String,
    pub count: usize,
    pub licence: String,
    pub notice: String,
    /// The data file's name, beside the manifest.
    pub file: String,
    /// The data file's size in bytes and SHA-256 (hex), as downloaded.
    pub bytes: u64,
    pub sha256: String,
}

/// One game. Every field here is allowed in the data; nothing else is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steam_appid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gog_id: Option<String>,
    pub name: String,
    pub people_say: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vibes: Option<String>,
    #[serde(default)]
    pub tags: ReviewTags,
    /// The `DIGEST_VERSION` the texts were written with.
    pub prompt_version: i64,
    /// The model that wrote them.
    pub model: String,
    /// When they were written (YYYY-MM-DD).
    pub written: String,
    /// The date of the newest review they were written from (YYYY-MM-DD).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviews_until: Option<String>,
}

/// A file or a download this build won't apply, and why, said plainly.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Refused(pub String);

fn refuse<T>(why: impl Into<String>) -> Result<T, Refused> {
    Err(Refused(why.into()))
}

/// Shared data, read and checked.
#[derive(Debug, Clone)]
pub struct Seed {
    pub header: Header,
    pub entries: Vec<Entry>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn is_date(s: &str) -> bool {
    s.len() == 10 && chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok()
}

/// Read a data file (`upnext-seed-<date>.jsonl.gz`) and check it whole: the
/// gzip stream, the header, the checksum, every entry and the count. Any
/// fault refuses the whole file.
pub fn read(gz: &[u8]) -> Result<Seed, Refused> {
    let mut text = Vec::new();
    let mut unpack = flate2::read::GzDecoder::new(gz).take(MAX_UNPACKED as u64 + 1);
    if let Err(e) = unpack.read_to_end(&mut text) {
        return refuse(format!("that file isn't UpNext's shared data, or it's damaged ({e})"));
    }
    if text.len() > MAX_UNPACKED {
        return refuse("that file is far bigger than UpNext's shared data");
    }
    let (first, rest) = match text.iter().position(|&b| b == b'\n') {
        Some(at) => (&text[..at], &text[at + 1..]),
        None => (&text[..], &[][..]),
    };
    let header: Header = match serde_json::from_slice(first) {
        Ok(h) => h,
        Err(e) => return refuse(format!("that file isn't UpNext's shared data: its first line isn't the header ({e})")),
    };
    if header.upnext_seed > FORMAT {
        return refuse(format!(
            "that shared data is in format {}, and this UpNext reads format {FORMAT}; update UpNext to use it",
            header.upnext_seed
        ));
    }
    if header.upnext_seed != FORMAT {
        return refuse(format!("that shared data is in format {}, which this UpNext doesn't read", header.upnext_seed));
    }
    if header.licence != LICENCE {
        return refuse(format!("that shared data says its licence is {:?}; UpNext applies only {LICENCE} data", header.licence));
    }
    if !is_date(&header.date) {
        return refuse(format!("that shared data's date isn't a date: {:?}", header.date));
    }
    if sha256_hex(rest) != header.sha256.to_ascii_lowercase() {
        return refuse("that shared data doesn't match its checksum: it's damaged or was changed");
    }
    let mut entries = vec![];
    for (i, line) in rest.split(|&b| b == b'\n').enumerate() {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let line_no = i + 2;
        let e: Entry = match serde_json::from_slice(line) {
            Ok(e) => e,
            Err(e) => return refuse(format!("line {line_no} of that shared data isn't a game UpNext can use ({e})")),
        };
        if e.steam_appid.is_none() && e.gog_id.as_deref().is_none_or(|g| g.trim().is_empty()) {
            return refuse(format!("line {line_no} of that shared data has neither a Steam appid nor a GOG id"));
        }
        if e.people_say.trim().is_empty() {
            return refuse(format!("line {line_no} of that shared data has no People say"));
        }
        if !is_date(&e.written) || e.reviews_until.as_deref().is_some_and(|d| !is_date(d)) {
            return refuse(format!("line {line_no} of that shared data has a date that isn't one"));
        }
        entries.push(e);
        if entries.len() > MAX_ENTRIES {
            return refuse("that file has far more games than UpNext's shared data");
        }
    }
    if entries.len() != header.count {
        return refuse(format!("that shared data says it has {} games but holds {}", header.count, entries.len()));
    }
    Ok(Seed { header, entries })
}

/// Write a data file for `entries` (the build's export, and the
/// tests' fixtures). The header is made from them.
pub fn write(date: &str, entries: &[Entry]) -> Result<Vec<u8>> {
    use std::io::Write;
    let mut body = Vec::new();
    for e in entries {
        serde_json::to_writer(&mut body, e)?;
        body.push(b'\n');
    }
    let header = Header {
        upnext_seed: FORMAT,
        date: date.into(),
        count: entries.len(),
        sha256: sha256_hex(&body),
        licence: LICENCE.into(),
        notice: NOTICE.into(),
    };
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    serde_json::to_writer(&mut gz, &header)?;
    gz.write_all(b"\n")?;
    gz.write_all(&body)?;
    Ok(gz.finish()?)
}

/// The manifest for a data file written by [`write`].
pub fn manifest(seed: &Header, data: &[u8]) -> Manifest {
    Manifest {
        upnext_seed: seed.upnext_seed,
        date: seed.date.clone(),
        count: seed.count,
        licence: seed.licence.clone(),
        notice: seed.notice.clone(),
        file: file_name(&seed.date),
        bytes: data.len() as u64,
        sha256: sha256_hex(data),
    }
}

/// What applying shared data did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[schema(as = SeedApplied)]
pub struct Applied {
    /// The data's date (YYYY-MM-DD).
    pub date: String,
    /// Games in the data.
    pub entries: usize,
    /// Games of this library it has.
    pub matched: usize,
    /// Summaries written now.
    pub written: usize,
    /// This install's own summaries left alone: written with the same
    /// prompt version or a newer one.
    pub kept_own: usize,
    /// Titles that already had this data's summary, or a newer data's.
    pub already: usize,
}

/// The tags as the model would have given them, so the vocabulary's checks
/// and caps apply to the data too: a newer UpNext's vocabulary may hold
/// values this one doesn't know, and those are dropped.
fn checked_tags(tags: &ReviewTags) -> ReviewTags {
    let set = |name: &str, values: &[String]| values.iter().map(|v| format!("{name}:{v}")).collect::<Vec<_>>();
    let mut facets: Vec<String> = tags.scales.iter().map(|(k, v)| format!("{k}:{v}")).collect();
    facets.extend(set(MOOD.name, &tags.mood));
    facets.extend(set(SKILLS.name, &tags.skills));
    let verdict = |v: Verdict| tags.aspects.iter().filter(|a| a.verdict == v).map(|a| a.aspect.clone()).collect();
    let reply = Reply {
        facets,
        praised: verdict(Verdict::Praised),
        criticized: verdict(Verdict::Criticized),
        divisive: verdict(Verdict::Divisive),
        notes: tags.notes.clone(),
    };
    reply.validate().0
}

/// The data's entry for a title: by Steam appid, then by the GOG id of any
/// of its GOG copies.
fn entry_for<'a>(g: &GameView, by_appid: &HashMap<u32, &'a Entry>, by_gog: &HashMap<&str, &'a Entry>) -> Option<&'a Entry> {
    g.appid
        .and_then(|a| by_appid.get(&a).copied())
        .or_else(|| g.copies.iter().filter(|c| c.source == "gog").find_map(|c| by_gog.get(c.source_id.as_str()).copied()))
}

/// Apply shared data to this library. A title gets the data's summary
/// unless it has its own written with the same prompt version or a newer
/// one (the install's own always wins then), or it already has this data's
/// summary or a newer data's. Nothing is fetched and no AI is called.
pub fn apply(db: &Db, seed: &Seed) -> Result<Applied> {
    let date = &seed.header.date;
    let mut by_appid: HashMap<u32, &Entry> = HashMap::new();
    let mut by_gog: HashMap<&str, &Entry> = HashMap::new();
    for e in &seed.entries {
        if let Some(a) = e.steam_appid {
            by_appid.entry(a).or_insert(e);
        }
        if let Some(g) = e.gog_id.as_deref().map(str::trim).filter(|g| !g.is_empty()) {
            by_gog.entry(g).or_insert(e);
        }
    }
    let mut out = Applied { date: date.clone(), entries: seed.entries.len(), ..Default::default() };
    let mut rows = vec![];
    for g in db.all_games(PlayerId::FIRST)? {
        if g.status == Status::NotGame {
            continue;
        }
        let Some(e) = entry_for(&g, &by_appid, &by_gog) else { continue };
        out.matched += 1;
        match &g.seed_date {
            // Dates are YYYY-MM-DD, so they compare as text.
            Some(had) if had.as_str() >= date.as_str() => {
                out.already += 1;
                continue;
            }
            Some(_) => {}
            None if g.has_digest && g.digest_version.unwrap_or(1) >= e.prompt_version => {
                out.kept_own += 1;
                continue;
            }
            None => {}
        }
        let written = chrono::NaiveDate::parse_from_str(&e.written, "%Y-%m-%d")
            .ok()
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .map_or_else(now, |t| t.and_utc().timestamp());
        let cut = |s: &str, max: usize| s.trim().chars().take(max).collect::<String>();
        rows.push(SeededDigest {
            title_id: g.id,
            generated_at: written,
            model: e.model.clone(),
            digest: cut(&e.people_say, DIGEST_MAX_CHARS),
            vibes: e.vibes.as_deref().map(|v| cut(v, VIBES_MAX_CHARS)).filter(|v| !v.is_empty()),
            tags: checked_tags(&e.tags),
            version: e.prompt_version,
        });
    }
    db.put_seeded_digests(date, &rows)?;
    out.written = rows.len();
    Ok(out)
}

/// Apply shared data and record it: what the CLI and Settings do.
pub fn apply_and_record(db: &Db, seed: &Seed, source: &str) -> Result<Applied> {
    let done = apply(db, seed)?;
    db.record_seed_apply(&SeedApply {
        applied_at: now(),
        seed_date: done.date.clone(),
        source: source.into(),
        entries: done.entries as i64,
        matched: done.matched as i64,
        written: done.written as i64,
        kept_own: done.kept_own as i64,
    })?;
    Ok(done)
}

// -- where data comes from ----------------------------------------------------

/// The client shared data is downloaded with. It follows at most five
/// redirects, and only to https addresses: the release download goes on to
/// GitHub's file host, and a redirect down to plain http would let anyone on
/// the way change what comes back. Nothing checks where an address points,
/// the server's own network included: the server's API takes only an
/// http(s) scheme (`web_address`), and asking it to fetch an address needs
/// a full token or this computer, in a POST, which another website can't
/// send with an image or a link (`access`).
pub fn client() -> Result<reqwest::Client> {
    let https_only =
        reqwest::redirect::Policy::custom(|a| if a.previous().len() < 5 && a.url().scheme() == "https" { a.follow() } else { a.stop() });
    Ok(reqwest::Client::builder()
        .user_agent(crate::http::USER_AGENT)
        .timeout(std::time::Duration::from_secs(300))
        .connect_timeout(std::time::Duration::from_secs(15))
        .redirect(https_only)
        .build()?)
}

fn is_url(from: &str) -> bool {
    from.starts_with("https://") || from.starts_with("http://")
}

/// Download at most `max` bytes, refusing anything bigger.
async fn download(http: &reqwest::Client, url: &str, max: usize) -> Result<Vec<u8>> {
    let mut resp = http.get(url).send().await.with_context(|| format!("couldn't download {url}"))?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        // What the newest release answers until shared data is published.
        return Err(Refused(format!("no shared game data is published at {url} yet (it answered 404)")).into());
    }
    if !resp.status().is_success() {
        return Err(Refused(format!("{url} answered {}", resp.status())).into());
    }
    if resp.content_length().is_some_and(|n| n > max as u64) {
        return Err(Refused(format!("{url} is far bigger than UpNext's shared data")).into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = resp.chunk().await.with_context(|| format!("couldn't download {url}"))? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > max {
            return Err(Refused(format!("{url} is far bigger than UpNext's shared data")).into());
        }
    }
    Ok(bytes)
}

async fn fetch(http: &reqwest::Client, from: &str, max: usize) -> Result<Vec<u8>> {
    if is_url(from) {
        return download(http, from, max).await;
    }
    let bytes = tokio::fs::read(from).await.with_context(|| format!("couldn't read {from}"))?;
    if bytes.len() > max {
        return Err(Refused(format!("{from} is far bigger than UpNext's shared data")).into());
    }
    Ok(bytes)
}

/// Where the data file named in a manifest is: beside the manifest. The
/// name must be a plain file name.
fn beside(manifest_at: &str, file: &str) -> Result<String, Refused> {
    if file.is_empty() || file.contains(['/', '\\']) || file.starts_with('.') {
        return refuse(format!("the manifest names {file:?}, which isn't a file beside it"));
    }
    if is_url(manifest_at) {
        let base = manifest_at.split(['?', '#']).next().unwrap_or(manifest_at);
        let dir = base.rsplit_once('/').map_or(base, |(dir, _)| dir);
        Ok(format!("{dir}/{file}"))
    } else {
        Ok(Path::new(manifest_at).with_file_name(file).display().to_string())
    }
}

/// Data loaded from a file or URL: the checked data and the file's bytes.
#[derive(Debug)]
pub struct Loaded {
    pub seed: Seed,
    pub data: Vec<u8>,
}

/// Read a manifest from a path or an http(s) URL: what the wizard offers
/// before downloading the data.
pub async fn offer(http: &reqwest::Client, at: &str) -> Result<Manifest> {
    let raw = fetch(http, at.trim(), MAX_MANIFEST).await?;
    Ok(serde_json::from_slice(&raw).map_err(|e| Refused(format!("that isn't UpNext's shared-data manifest ({e})")))?)
}

/// Load shared data from a path or an http(s) URL: a manifest (`.json`),
/// whose data file beside it is downloaded and checked against it, or a
/// data file itself.
pub async fn load(http: &reqwest::Client, from: &str) -> Result<Loaded> {
    let from = from.trim();
    let path_part = from.split(['?', '#']).next().unwrap_or(from);
    if path_part.ends_with(".json") {
        let m = offer(http, from).await?;
        if m.upnext_seed > FORMAT {
            return Err(Refused(format!(
                "that shared data is in format {}, and this UpNext reads format {FORMAT}; update UpNext to use it",
                m.upnext_seed
            ))
            .into());
        }
        let at = beside(from, &m.file)?;
        let data = fetch(http, &at, MAX_DOWNLOAD).await?;
        if data.len() as u64 != m.bytes || sha256_hex(&data) != m.sha256.to_ascii_lowercase() {
            return Err(Refused(format!("{} doesn't match its manifest: it's damaged or was changed", m.file)).into());
        }
        let seed = read(&data)?;
        if seed.header.date != m.date || seed.header.count != m.count {
            return Err(Refused(format!("{} isn't the data its manifest describes", m.file)).into());
        }
        return Ok(Loaded { seed, data });
    }
    let data = fetch(http, from, MAX_DOWNLOAD).await?;
    let seed = read(&data)?;
    Ok(Loaded { seed, data })
}

// -- the kept copy --------------------------------------------------------------

/// The folder the applied data is kept in, beside the database, so games
/// that join the library later get their summaries after a sync.
pub fn kept_dir(cfg: &Config) -> PathBuf {
    cfg.db_dir().join("seed")
}

/// The kept data file, if there is one: the newest by date.
pub fn kept(cfg: &Config) -> Option<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(kept_dir(cfg))
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("upnext-seed-") && n.ends_with(".jsonl.gz")))
        .collect();
    files.sort();
    files.pop()
}

/// Keep `data` as the applied data, unless newer data is kept already, and
/// remove the older copies.
pub fn keep(cfg: &Config, seed: &Seed, data: &[u8]) -> Result<()> {
    let dir = kept_dir(cfg);
    let name = file_name(&seed.header.date);
    if kept(cfg).and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).is_some_and(|k| k > name) {
        return Ok(());
    }
    std::fs::create_dir_all(&dir).with_context(|| format!("couldn't make {}", dir.display()))?;
    let part = dir.join(format!("{name}.part"));
    std::fs::write(&part, data).with_context(|| format!("couldn't write {}", part.display()))?;
    std::fs::rename(&part, dir.join(&name))?;
    for entry in std::fs::read_dir(&dir)?.flatten() {
        let n = entry.file_name().to_string_lossy().into_owned();
        if n.starts_with("upnext-seed-") && n != name {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    Ok(())
}

/// Apply the kept data again for titles that have joined the library since
/// (after a sync, and before the AI writes summaries). Recorded only when
/// it wrote something. None when nothing is kept.
pub fn apply_kept(db: &Db, cfg: &Config, why: &str) -> Result<Option<Applied>> {
    let Some(path) = kept(cfg) else { return Ok(None) };
    let data = std::fs::read(&path).with_context(|| format!("couldn't read {}", path.display()))?;
    let seed = read(&data).with_context(|| format!("the kept shared data, {}", path.display()))?;
    let done = apply(db, &seed)?;
    if done.written > 0 {
        db.record_seed_apply(&SeedApply {
            applied_at: now(),
            seed_date: done.date.clone(),
            source: why.into(),
            entries: done.entries as i64,
            matched: done.matched as i64,
            written: done.written as i64,
            kept_own: done.kept_own as i64,
        })?;
    }
    Ok(Some(done))
}

/// Stop using shared data: its summaries go, and so does the kept copy, so
/// this install's own AI writes them. Returns how many summaries went.
pub fn remove(db: &Db, cfg: &Config) -> Result<usize> {
    if let Ok(files) = std::fs::read_dir(kept_dir(cfg)) {
        for f in files.flatten() {
            if f.file_name().to_string_lossy().starts_with("upnext-seed-") {
                std::fs::remove_file(f.path()).with_context(|| format!("couldn't remove {}", f.path().display()))?;
            }
        }
    }
    db.remove_seeded_digests()
}

/// What Settings shows: the summaries from shared data, the kept data, and
/// the latest times it was applied.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct SeedStatus {
    /// Titles whose summary came from shared data.
    pub seeded: i64,
    /// The newest shared data's date among them (YYYY-MM-DD).
    pub seed_date: Option<String>,
    /// The kept data file's name, applied again after each sync.
    pub kept: Option<String>,
    /// The latest times shared data was applied, newest first.
    pub applies: Vec<SeedApply>,
    /// Where the newest release's data is.
    pub default_url: String,
}

pub fn status(db: &Db, cfg: &Config) -> Result<SeedStatus> {
    let (seeded, seed_date) = db.seeded_digests()?;
    Ok(SeedStatus {
        seeded,
        seed_date,
        kept: kept(cfg).and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())),
        applies: db.seed_applies(5)?,
        default_url: DEFAULT_URL.into(),
    })
}

pub mod build;
pub mod verbatim;

#[cfg(test)]
mod tests;
