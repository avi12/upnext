//! Receiving backlog verdicts (`docs/verdicts/reading.md`): the
//! daily job downloads the snapshot's manifest from `verdicts_url`, and when
//! it names a newer snapshot, the summary file, which is checked whole and
//! put in place of the one kept in one transaction. Nothing is sent: the
//! request carries no key, and the whole summary is downloaded so the host
//! never learns which games anyone owns.
//!
//! The manifest is asked for with the ETag it last came with, so an
//! unchanged day costs one small request. A failed download leaves the kept
//! snapshot as it was: game pages and picks keep their verdicts offline.
//! `upnext verdicts apply` puts a downloaded file in place the same way.

use super::format::{self, Manifest, Summary};
use crate::config::Config;
use crate::db::{CommunitySnapshot, Db};
use crate::enrich::ProgressSink;
use crate::seed::Refused;
use anyhow::{Context, Result};
use reqwest::{Client, StatusCode, header};
use serde::Serialize;
use std::path::Path;

/// The summary file is at most this big: 20,000 games with texts is a few
/// megabytes.
const MAX_SUMMARY: usize = 64 << 20;

/// What asking for the snapshot did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// A newer snapshot was put in place.
    Applied,
    /// The kept one is the newest.
    Unchanged,
}

/// What a sync or an apply left, with the snapshot now kept.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct Received {
    pub outcome: Outcome,
    pub snapshot: CommunitySnapshot,
}

impl Received {
    /// The job's summary and the command line's line.
    pub fn summary(&self) -> String {
        let s = &self.snapshot;
        let date = s.date.as_deref().unwrap_or("?");
        match self.outcome {
            Outcome::Applied => format!(
                "Backlog verdicts of {date}: {} of its {} {} in this library ({} read)",
                s.matched,
                crate::enrich::plural(s.games, "game", "games"),
                if s.matched == 1 { "is" } else { "are" },
                crate::enrich::plural(s.verdicts, "verdict", "verdicts")
            ),
            Outcome::Unchanged => {
                format!(
                    "No newer backlog verdicts than {date}; {} in this library {} them",
                    crate::enrich::plural(s.matched, "game", "games"),
                    if s.matched == 1 { "has" } else { "have" }
                )
            }
        }
    }
}

fn joined(base: &str, file: &str) -> String {
    format!("{}/{file}", base.trim_end_matches('/'))
}

/// Read a response's body, refusing more than `max` bytes.
async fn body(mut resp: reqwest::Response, url: &str, max: usize) -> Result<Vec<u8>> {
    if resp.content_length().is_some_and(|n| n > max as u64) {
        return Err(Refused(format!("{url} is far bigger than UpNext's backlog verdicts")).into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = resp.chunk().await.with_context(|| format!("couldn't download {url}"))? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > max {
            return Err(Refused(format!("{url} is far bigger than UpNext's backlog verdicts")).into());
        }
    }
    Ok(bytes)
}

fn answered(url: &str, status: StatusCode) -> anyhow::Error {
    if status == StatusCode::NOT_FOUND {
        // What a new service answers before its first daily run.
        return Refused(format!("no backlog verdicts are published at {url} yet (it answered 404)")).into();
    }
    anyhow::anyhow!("{url} answered {status}")
}

async fn download(http: &Client, url: &str, max: usize) -> Result<Vec<u8>> {
    let resp = http.get(url).send().await.with_context(|| format!("couldn't reach {url}"))?;
    if !resp.status().is_success() {
        return Err(answered(url, resp.status()));
    }
    body(resp, url, max).await
}

/// Ask the service at `base` for its snapshot, and put it in place when
/// it's newer than the one kept.
pub async fn sync(db: &Db, http: &Client, base: &str) -> Result<Received> {
    let (kept_date, etag) = db.community_verdicts_kept()?;
    let url = joined(base, format::MANIFEST);
    let mut req = http.get(&url);
    if let (Some(_), Some(tag)) = (&kept_date, &etag) {
        req = req.header(header::IF_NONE_MATCH, tag);
    }
    let resp = req.send().await.with_context(|| format!("couldn't reach {url}"))?;
    if resp.status() == StatusCode::NOT_MODIFIED {
        db.community_verdicts_checked(None)?;
        return Ok(Received { outcome: Outcome::Unchanged, snapshot: db.community_snapshot()? });
    }
    if !resp.status().is_success() {
        return Err(answered(&url, resp.status()));
    }
    let new_tag = resp.headers().get(header::ETAG).and_then(|v| v.to_str().ok()).map(String::from);
    let m = format::read_manifest(&body(resp, &url, format::MAX_MANIFEST).await?)?;
    // Only a newer day replaces the kept one: an older manifest (a cache, a
    // mirror behind, a service restored from a backup) never rolls it back.
    if !newer(kept_date.as_deref(), &m.date) {
        db.community_verdicts_checked(new_tag.as_deref())?;
        return Ok(Received { outcome: Outcome::Unchanged, snapshot: db.community_snapshot()? });
    }
    let data = download(http, &joined(base, &m.summary.file), MAX_SUMMARY).await?;
    let summary = format::read_named_summary(&m, &data)?;
    apply(db, &summary, base, new_tag.as_deref())
}

/// Whether a snapshot of `date` is newer than the one kept, if any. Days
/// are YYYY-MM-DD, which order as text.
pub fn newer(kept: Option<&str>, date: &str) -> bool {
    kept.is_none_or(|k| date > k)
}

/// Put a checked summary in place of the snapshot kept.
pub fn apply(db: &Db, summary: &Summary, source: &str, etag: Option<&str>) -> Result<Received> {
    db.replace_community_verdicts(&summary.header.date, source, etag, &summary.games)?;
    Ok(Received { outcome: Outcome::Applied, snapshot: db.community_snapshot()? })
}

/// Read a snapshot for `upnext verdicts apply`: a service's address, a
/// manifest's (`…/verdicts.json`) or its summary file's, or the same on
/// disk, or a folder holding them.
pub async fn load(http: &Client, from: &str) -> Result<(Summary, Option<Manifest>)> {
    if from.starts_with("https://") || from.starts_with("http://") {
        let path = from.split(['?', '#']).next().unwrap_or(from);
        // An address is held to `verdicts_url`'s rule: https, or plain http
        // only to this computer, since the connection is what authenticates it.
        crate::config::validate_setting("verdicts_url", path)?;
        if path.ends_with(".jsonl.gz") {
            return Ok((format::read_summary(&download(http, from, MAX_SUMMARY).await?)?, None));
        }
        let base = path.strip_suffix(format::MANIFEST).unwrap_or(path);
        let m = format::read_manifest(&download(http, &joined(base, format::MANIFEST), format::MAX_MANIFEST).await?)?;
        let data = download(http, &joined(base, &m.summary.file), MAX_SUMMARY).await?;
        return Ok((format::read_named_summary(&m, &data)?, Some(m)));
    }
    let read = |p: &Path| std::fs::read(p).with_context(|| format!("couldn't read {}", p.display()));
    let path = Path::new(from);
    let manifest = if path.is_dir() { path.join(format::MANIFEST) } else { path.to_path_buf() };
    if manifest.extension().is_some_and(|e| e == "json") {
        let m = format::read_manifest(&read(&manifest)?)?;
        let data = read(&manifest.with_file_name(&m.summary.file))?;
        return Ok((format::read_named_summary(&m, &data)?, Some(m)));
    }
    Ok((format::read_summary(&read(path)?)?, None))
}

/// Why the job has nothing to do, when it hasn't.
pub fn off(cfg: &Config) -> Option<&'static str> {
    if !cfg.verdicts_receive {
        Some("Backlog verdicts from other players are off (Settings › Library data)")
    } else if cfg.verdicts_url.is_none() {
        Some("No verdicts service is set, so there are no backlog verdicts to download yet")
    } else {
        None
    }
}

/// The "Backlog verdicts" job: one look at the service.
pub async fn run_job(db: &Db, cfg: &Config, http: &Client, sink: &dyn ProgressSink) -> Result<String> {
    if let Some(why) = off(cfg) {
        sink.log(why.to_string());
        return Ok(why.to_string());
    }
    let base = cfg.verdicts_url.as_deref().unwrap_or_default();
    let r = sync(db, http, base).await?;
    let line = r.summary();
    sink.log(line.clone());
    Ok(line)
}
