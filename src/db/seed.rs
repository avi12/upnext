//! Summaries from UpNext's shared game data: writing them, taking
//! them out again, and the record of each time the data was applied.

use super::{Db, now, to_json};
use crate::review_tags::ReviewTags;
use anyhow::Result;
use rusqlite::params;
use serde::Serialize;

/// One summary from the shared data, for a title of this library.
pub struct SeededDigest {
    pub title_id: i64,
    /// When the summary was written (the entry's `written` date).
    pub generated_at: i64,
    pub model: String,
    pub digest: String,
    pub vibes: Option<String>,
    pub tags: ReviewTags,
    pub version: i64,
}

/// One time shared data was applied, and what it did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct SeedApply {
    pub applied_at: i64,
    /// The date of the data (YYYY-MM-DD).
    pub seed_date: String,
    /// Where it came from: a URL, a file's name, or the kept copy after a sync.
    pub source: String,
    /// Games in the data.
    pub entries: i64,
    /// Games of this library it has.
    pub matched: i64,
    /// Summaries it wrote.
    pub written: i64,
    /// This install's own summaries it left alone, written with the same
    /// prompt version or a newer one.
    pub kept_own: i64,
}

impl Db {
    /// Write summaries from the shared data of `seed_date`, in one go.
    pub fn put_seeded_digests(&self, seed_date: &str, rows: &[SeededDigest]) -> Result<()> {
        let at = now();
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            {
                let mut put = tx.prepare(
                    "INSERT OR REPLACE INTO digests(title_id,generated_at,model,digest,vibes,tags,prompt_version,seed_date,seed_applied_at)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                )?;
                for r in rows {
                    put.execute(params![
                        r.title_id,
                        r.generated_at,
                        r.model,
                        r.digest,
                        r.vibes,
                        to_json(&r.tags),
                        r.version,
                        seed_date,
                        at
                    ])?;
                }
            }
            tx.commit()?;
            Ok(())
        })
    }

    /// Take out every summary that came from the shared data, so this
    /// install's own AI writes them. Returns how many went.
    pub fn remove_seeded_digests(&self) -> Result<usize> {
        self.with(|c| Ok(c.execute("DELETE FROM digests WHERE seed_date IS NOT NULL", [])?))
    }

    /// How many titles have a summary from the shared data, and the newest
    /// data's date among them.
    pub fn seeded_digests(&self) -> Result<(i64, Option<String>)> {
        self.with(|c| {
            Ok(c.query_row("SELECT COUNT(*), MAX(seed_date) FROM digests WHERE seed_date IS NOT NULL", [], |r| Ok((r.get(0)?, r.get(1)?)))?)
        })
    }

    pub fn record_seed_apply(&self, a: &SeedApply) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO seed_applies(applied_at,seed_date,source,entries,matched,written,kept_own) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![a.applied_at, a.seed_date, a.source, a.entries, a.matched, a.written, a.kept_own],
            )?;
            Ok(())
        })
    }

    /// The latest times shared data was applied, newest first.
    pub fn seed_applies(&self, limit: usize) -> Result<Vec<SeedApply>> {
        self.with(|c| {
            let mut q = c.prepare(
                "SELECT applied_at,seed_date,source,entries,matched,written,kept_own FROM seed_applies ORDER BY applied_at DESC, id DESC LIMIT ?1",
            )?;
            let rows = q.query_map(params![limit as i64], |r| {
                Ok(SeedApply {
                    applied_at: r.get(0)?,
                    seed_date: r.get(1)?,
                    source: r.get(2)?,
                    entries: r.get(3)?,
                    matched: r.get(4)?,
                    written: r.get(5)?,
                    kept_own: r.get(6)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    // -- building the shared data ---------------------------

    /// Copies, removed ones too, that a launcher other than `launcher`
    /// brought in: a library, which a build of the shared data refuses.
    pub fn copies_not_from(&self, launcher: &str) -> Result<i64> {
        self.with(|c| Ok(c.query_row("SELECT COUNT(*) FROM games WHERE launcher<>?1", params![launcher], |r| r.get(0))?))
    }

    /// The model and the time of each summary this database wrote itself,
    /// by title.
    pub fn own_digests_written(&self) -> Result<std::collections::HashMap<i64, (String, i64)>> {
        self.with(|c| {
            let mut q = c.prepare("SELECT title_id, COALESCE(model,''), generated_at FROM digests WHERE seed_date IS NULL")?;
            let rows = q.query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Take one title's summary out, as if it had never been written.
    #[cfg(test)]
    pub fn delete_digest(&self, title_id: i64) -> Result<()> {
        self.with(|c| Ok(c.execute("DELETE FROM digests WHERE title_id=?1", params![title_id]).map(|_| ())?))
    }
}
