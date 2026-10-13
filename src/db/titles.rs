//! Grouping store copies into titles, and manual merge and split.

use super::{Db, Refused, now};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashMap;

pub use crate::names::{loose_key, name_key};

/// Tables keyed by `title_id` holding fetched or generated metadata; moved or
/// dropped when titles are merged.
const TITLE_META_TABLES: [&str; 14] = [
    "igdb_meta",
    "digests",
    "xref",
    "rawg_meta",
    "pcgw_meta",
    "title_series",
    "play_later",
    "trailers",
    "second_chances",
    "hidden_gems",
    // Your rating of the game: the survivor's wins.
    "game_ratings",
    // Favourites; the profile's other rows have no title.
    "profile_statements",
    // "Hide this game": the survivor's choice wins.
    "hidden_titles",
    // Backlog verdicts: the survivor's wins; the other stays with
    // the service, under its own id, until it's taken down there.
    "my_verdicts",
];

// ---------------------------------------------------------------------------
// Title keys
// ---------------------------------------------------------------------------

/// The edition wording that `loose_key` strips, title-cased, or None.
pub fn edition_of(name: &str) -> Option<String> {
    let full = name_key(name);
    let base = loose_key(name);
    if base.len() >= full.len() {
        return None;
    }
    let suffix = full[base.len()..].trim();
    if suffix.is_empty() {
        return None;
    }
    let pretty = match suffix {
        "goty" | "goty edition" | "game of the year" | "game of the year edition" => "Game of the Year Edition".to_string(),
        "director s cut" | "directors cut" => "Director's Cut".to_string(),
        "hd" => "HD".to_string(),
        other => other
            .split_whitespace()
            .map(|w| {
                let mut cs = w.chars();
                match cs.next() {
                    Some(f) if w != "of" && w != "the" => f.to_uppercase().collect::<String>() + cs.as_str(),
                    Some(f) => f.to_string() + cs.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    };
    Some(pretty)
}

impl Db {
    // -- titles ---------------------------------------------------------------

    /// Group every live copy into a title. Automatic rules, in order:
    /// 1. copies with a Steam appid group by appid;
    /// 2. copies without one join a group whose name matches exactly;
    /// 3. otherwise they join a group whose edition-insensitive name matches
    ///    ("Control" ~ "Control Ultimate Edition", "Alan Wake" ~ "Alan Wake Remastered");
    /// 4. otherwise they start a new name-keyed group.
    ///
    /// Then manual rules apply: `split_rules` pull a copy out into its own
    /// title, `merge_rules` union two groups. Titles are reconciled with the
    /// groups; emptied titles are merged (overrides, HLTB, picks follow).
    /// Returns the number of titles merged away.
    pub(super) fn assign_titles_in(c: &Connection) -> Result<usize> {
        struct Cp {
            id: i64,
            name: String,
            source: String,
            appid: Option<u32>,
            title_id: Option<i64>,
        }
        let copies: Vec<Cp> = {
            let mut stmt =
                c.prepare("SELECT id, name, source, appid, title_id FROM games WHERE removed=0 ORDER BY (source<>'steam'), id")?;
            let it = stmt
                .query_map([], |r| Ok(Cp { id: r.get(0)?, name: r.get(1)?, source: r.get(2)?, appid: r.get(3)?, title_id: r.get(4)? }))?;
            it.collect::<std::result::Result<_, _>>()?
        };
        let index_of: HashMap<i64, usize> = copies.iter().enumerate().map(|(i, cp)| (cp.id, i)).collect();
        // Keep editions current (names can change on sync).
        {
            let mut upd = c.prepare("UPDATE games SET edition=?2 WHERE id=?1 AND edition IS NOT ?2")?;
            for cp in &copies {
                upd.execute(params![cp.id, edition_of(&cp.name)])?;
            }
        }
        let splits: std::collections::HashSet<i64> = {
            let mut stmt = c.prepare("SELECT game_id FROM split_rules")?;
            stmt.query_map([], |r| r.get::<_, i64>(0))?.collect::<std::result::Result<_, _>>()?
        };
        let merges: Vec<(i64, i64, i64)> = {
            let mut stmt = c.prepare("SELECT game_id, with_game_id, created_at FROM merge_rules")?;
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<std::result::Result<_, _>>()?
        };

        // --- automatic groups (indices into `copies`) ----------------------
        let mut group_of: Vec<usize> = vec![usize::MAX; copies.len()];
        let mut groups: Vec<Vec<usize>> = vec![];
        let mut by_appid: HashMap<u32, usize> = HashMap::new();
        let mut by_exact: HashMap<String, usize> = HashMap::new();
        let mut by_loose: HashMap<String, usize> = HashMap::new();
        let mut by_namekey: HashMap<String, usize> = HashMap::new();
        fn add(groups: &mut [Vec<usize>], group_of: &mut [usize], gi: usize, i: usize) {
            groups[gi].push(i);
            group_of[i] = gi;
        }
        for (i, cp) in copies.iter().enumerate() {
            if splits.contains(&cp.id) {
                continue;
            }
            let Some(appid) = cp.appid else { continue };
            let gi = *by_appid.entry(appid).or_insert_with(|| {
                groups.push(vec![]);
                groups.len() - 1
            });
            add(&mut groups, &mut group_of, gi, i);
            by_exact.entry(name_key(&cp.name)).or_insert(gi);
            by_loose.entry(loose_key(&cp.name)).or_insert(gi);
        }
        for (i, cp) in copies.iter().enumerate() {
            if splits.contains(&cp.id) || cp.appid.is_some() {
                continue;
            }
            let exact = name_key(&cp.name);
            let loose = loose_key(&cp.name);
            let gi = if let Some(&gi) = by_exact.get(&exact) {
                gi
            } else if let Some(&gi) = by_loose.get(&loose) {
                gi
            } else {
                *by_namekey.entry(exact.clone()).or_insert_with(|| {
                    groups.push(vec![]);
                    groups.len() - 1
                })
            };
            add(&mut groups, &mut group_of, gi, i);
            by_exact.entry(exact).or_insert(gi);
            by_loose.entry(loose).or_insert(gi);
        }
        // Split copies: singleton groups.
        for (i, cp) in copies.iter().enumerate() {
            if splits.contains(&cp.id) {
                groups.push(vec![i]);
                group_of[i] = groups.len() - 1;
            }
        }
        // Manual merges: union-find over groups.
        let mut parent: Vec<usize> = (0..groups.len()).collect();
        fn find(parent: &mut [usize], x: usize) -> usize {
            let mut r = x;
            while parent[r] != r {
                r = parent[r];
            }
            let mut y = x;
            while parent[y] != r {
                let n = parent[y];
                parent[y] = r;
                y = n;
            }
            r
        }
        for (a, b, _) in &merges {
            if let (Some(&ia), Some(&ib)) = (index_of.get(a), index_of.get(b)) {
                let (ra, rb) = (find(&mut parent, group_of[ia]), find(&mut parent, group_of[ib]));
                if ra != rb {
                    parent[rb] = ra;
                }
            }
        }
        let mut final_groups: HashMap<usize, Vec<usize>> = HashMap::new();
        for (gi, members) in groups.iter().enumerate() {
            let root = find(&mut parent, gi);
            final_groups.entry(root).or_default().extend(members);
        }
        // The first title named in a manual merge wins (`merge_titles`): its
        // copy is the rules' anchor, and the anchor's own automatic group
        // decides the merged title's id, name and appid, so a forced merge of
        // unlike games never renames the survivor or moves it to another
        // game's appid. An anchor that was itself merged into something later
        // points on; follow that chain (bounded, in case of a cycle). When
        // one group holds several anchors, the newest merge's wins.
        let next_of: HashMap<i64, i64> = merges.iter().map(|(g, w, _)| (*g, *w)).collect();
        let root_of = |mut g: i64| -> i64 {
            for _ in 0..next_of.len() {
                match next_of.get(&g) {
                    Some(&n) if n != g && index_of.contains_key(&n) => g = n,
                    _ => break,
                }
            }
            g
        };
        let mut anchor_of: HashMap<usize, (i64, i64)> = HashMap::new();
        for (a, b, created) in &merges {
            if !index_of.contains_key(a) {
                continue;
            }
            let anchor = root_of(*b);
            let Some(&ia) = index_of.get(&anchor) else { continue };
            let root = find(&mut parent, group_of[ia]);
            let best = anchor_of.entry(root).or_insert((*created, anchor));
            if (*created, std::cmp::Reverse(anchor)) > (best.0, std::cmp::Reverse(best.1)) {
                *best = (*created, anchor);
            }
        }

        // --- reconcile with title rows --------------------------------------
        let mut title_key: HashMap<i64, String> = HashMap::new();
        let mut title_by_key: HashMap<String, i64> = HashMap::new();
        {
            let mut stmt = c.prepare("SELECT id, key FROM titles")?;
            for r in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
                let (id, key) = r?;
                title_by_key.insert(key.clone(), id);
                title_key.insert(id, key);
            }
        }
        let mut moved_from: Vec<(i64, i64)> = vec![];
        // The copies that left each title, for the snapshot of what it had.
        let mut left: HashMap<i64, Vec<i64>> = HashMap::new();
        let mut claimed: std::collections::HashSet<i64> = std::collections::HashSet::new();
        let mut used_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut roots: Vec<usize> = final_groups.keys().copied().collect();
        // Manually merged groups claim the first title's id before anything
        // else can; then bigger groups claim their existing title first. A
        // title is claimed at most once per pass.
        roots.sort_by_key(|r| (!anchor_of.contains_key(r), std::cmp::Reverse(final_groups[r].len()), *r));
        for root in roots {
            let members = &final_groups[&root];
            let anchor = anchor_of.get(&root).map(|(_, a)| index_of[a]);
            // The copies that name the title: the anchor's own group in a manual merge, else all of them.
            let ident = anchor.map_or(members, |ia| &groups[group_of[ia]]);
            let mem = |i: &usize| &copies[*i];
            // Key: appid of the first Steam copy (or any appid), else the exact name key of the first member.
            let appid = ident
                .iter()
                .map(mem)
                .find(|cp| cp.source == "steam")
                .and_then(|cp| cp.appid)
                .or_else(|| ident.iter().map(mem).find_map(|cp| cp.appid));
            let first_id = members.iter().map(|i| copies[*i].id).min().unwrap_or_default();
            let mut key = match appid {
                // A split copy keeps its own title even when it shares an appid with its old group.
                _ if members.len() == 1 && splits.contains(&first_id) => format!("split:{first_id}"),
                Some(a) => format!("appid:{a}"),
                None => format!("name:{}", name_key(&ident.iter().map(mem).min_by_key(|cp| cp.id).unwrap().name)),
            };
            // Two groups can still land on one key (manual rules against the automatic ones); never collide.
            if !used_keys.insert(key.clone()) {
                key = format!("{key}:{first_id}");
                used_keys.insert(key.clone());
            }
            // Display name: a copy without edition wording (prefer Steam), else Steam's name, else the shortest.
            let name = ident
                .iter()
                .map(mem)
                .filter(|cp| edition_of(&cp.name).is_none())
                .min_by_key(|cp| (cp.source != "steam", cp.name.len()))
                .or_else(|| ident.iter().map(mem).find(|cp| cp.source == "steam"))
                .or_else(|| ident.iter().map(mem).min_by_key(|cp| cp.name.len()))
                .map(|cp| cp.name.clone())
                .unwrap_or_default();
            let mut votes: HashMap<i64, usize> = HashMap::new();
            for i in members {
                if let Some(t) = copies[*i].title_id {
                    *votes.entry(t).or_insert(0) += 1;
                }
            }
            let chosen: Option<i64> = anchor
                .and_then(|ia| copies[ia].title_id)
                .filter(|t| !claimed.contains(t))
                .or_else(|| {
                    votes
                        .iter()
                        .filter(|(t, _)| !claimed.contains(t))
                        .max_by_key(|(t, n)| (**n, title_key.get(t).is_some_and(|k| k == &key), -**t))
                        .map(|(t, _)| *t)
                })
                .or_else(|| title_by_key.get(&key).copied().filter(|t| !claimed.contains(t)));
            let tid = match chosen {
                Some(tid) => {
                    if let Some(&other) = title_by_key.get(&key)
                        && other != tid
                        && !claimed.contains(&other)
                    {
                        let tmp = format!("{key}:merged:{other}");
                        c.execute("UPDATE titles SET key=?2 WHERE id=?1", params![other, tmp])?;
                        title_key.insert(other, tmp.clone());
                        title_by_key.insert(tmp, other);
                    }
                    c.execute("UPDATE titles SET key=?2, name=?3, appid=?4 WHERE id=?1", params![tid, key, name, appid])?;
                    tid
                }
                None => {
                    c.execute("INSERT INTO titles(key,name,appid) VALUES(?1,?2,?3)", params![key, name, appid])?;
                    c.last_insert_rowid()
                }
            };
            claimed.insert(tid);
            if let Some(old_key) = title_key.get(&tid).cloned()
                && old_key != key
            {
                title_by_key.remove(&old_key);
            }
            title_by_key.insert(key.clone(), tid);
            title_key.insert(tid, key.clone());
            for i in members {
                let cp = &copies[*i];
                if cp.title_id != Some(tid) {
                    c.execute("UPDATE games SET title_id=?2 WHERE id=?1", params![cp.id, tid])?;
                    if let Some(old) = cp.title_id {
                        moved_from.push((old, tid));
                        left.entry(old).or_default().push(cp.id);
                    }
                }
                if cp.appid.is_none()
                    && let Some(a) = appid
                {
                    c.execute("UPDATE games SET appid=?2, steam_matched=1 WHERE id=?1", params![cp.id, a])?;
                }
            }
        }
        let mut merged = 0;
        moved_from.sort();
        moved_from.dedup();
        for (old, new) in moved_from {
            let remaining: i64 = c.query_row("SELECT COUNT(*) FROM games WHERE title_id=?1 AND removed=0", params![old], |r| r.get(0))?;
            if remaining > 0 {
                continue;
            }
            keep_snapshot(c, old, left.get(&old).map_or(&[][..], Vec::as_slice))?;
            c.execute("INSERT OR IGNORE INTO overrides(player_id,title_id,status,note,updated_at) SELECT player_id,?2,status,note,updated_at FROM overrides WHERE title_id=?1", params![old, new])?;
            c.execute("INSERT OR IGNORE INTO hltb(title_id,fetched_at,success,hltb_id,hltb_name,similarity,main_story,main_extra,completionist) SELECT ?2,fetched_at,success,hltb_id,hltb_name,similarity,main_story,main_extra,completionist FROM hltb WHERE title_id=?1 AND success=1", params![old, new])?;
            c.execute("INSERT OR IGNORE INTO steam_match(title_id,fetched_at,success,appid,matched_name,similarity) SELECT ?2,fetched_at,success,appid,matched_name,similarity FROM steam_match WHERE title_id=?1 AND success=1", params![old, new])?;
            c.execute("UPDATE picks SET title_id=?2 WHERE title_id=?1", params![old, new])?;
            c.execute("UPDATE stop_reasons SET title_id=?2 WHERE title_id=?1", params![old, new])?;
            c.execute("UPDATE pick_feedback SET title_id=?2 WHERE title_id=?1", params![old, new])?;
            // A lineup that held both keeps the survivor's rating.
            c.execute("UPDATE OR IGNORE pick_ratings SET title_id=?2 WHERE title_id=?1", params![old, new])?;
            c.execute("DELETE FROM pick_ratings WHERE title_id=?1", params![old])?;
            c.execute("UPDATE play_diary SET title_id=?2 WHERE title_id=?1", params![old, new])?;
            c.execute("UPDATE wishlist SET title_id=?2 WHERE title_id=?1", params![old, new])?;
            c.execute(
                "UPDATE titles SET no_ending=(SELECT no_ending FROM titles WHERE id=?1) WHERE id=?2 AND no_ending IS NULL",
                params![old, new],
            )?;
            c.execute("DELETE FROM overrides WHERE title_id=?1", params![old])?;
            c.execute("DELETE FROM hltb WHERE title_id=?1", params![old])?;
            c.execute("DELETE FROM steam_match WHERE title_id=?1", params![old])?;
            // Per-title metadata: keep the survivor's own, adopt the absorbed
            // title's where the survivor has none, then drop the rest so the
            // title row can go (these tables reference titles(id)).
            for table in TITLE_META_TABLES {
                c.execute(&format!("UPDATE OR IGNORE {table} SET title_id=?2 WHERE title_id=?1"), params![old, new])?;
                c.execute(&format!("DELETE FROM {table} WHERE title_id=?1"), params![old])?;
            }
            // Removed copies still point at the old title; they follow the live ones.
            c.execute("UPDATE games SET title_id=?2 WHERE title_id=?1", params![old, new])?;
            c.execute("DELETE FROM titles WHERE id=?1", params![old])?;
            merged += 1;
        }
        Ok(merged)
    }

    /// Manually merge titles: every copy of the other titles is bound to the
    /// first title's first copy, and the first title survives with its name,
    /// appid, status, note, ending switch and Play later (see
    /// `assign_titles_in`). Returns the surviving title id.
    pub fn merge_titles(&self, ids: &[i64]) -> Result<i64> {
        if ids.len() < 2 {
            return Err(Refused("need at least two titles to merge".into()).into());
        }
        let mut seen = std::collections::HashSet::new();
        if let Some(twice) = ids.iter().find(|id| !seen.insert(**id)) {
            return Err(Refused(format!("title #{twice} is named twice; a title can't be merged with itself")).into());
        }
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            // Anchor on a copy that belongs to the first title on its own, not
            // one an earlier merge brought in, so the title keeps its identity.
            let anchor: i64 = tx
                .query_row(
                    "SELECT id FROM games WHERE title_id=?1 AND removed=0
                     ORDER BY id IN (SELECT game_id FROM merge_rules), (source<>'steam'), id LIMIT 1",
                    params![ids[0]],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or_else(|| Refused(format!("title #{} has no copies", ids[0])))?;
            for &other in &ids[1..] {
                let mut stmt = tx.prepare("SELECT id FROM games WHERE title_id=?1 AND removed=0")?;
                let copies: Vec<i64> = stmt.query_map(params![other], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?;
                if copies.is_empty() {
                    return Err(Refused(format!("title #{other} not found")).into());
                }
                for gid in copies {
                    tx.execute("DELETE FROM split_rules WHERE game_id=?1", params![gid])?;
                    tx.execute(
                        "INSERT INTO merge_rules(game_id,with_game_id,created_at) VALUES(?1,?2,?3) ON CONFLICT(game_id) DO UPDATE SET with_game_id=excluded.with_game_id, created_at=excluded.created_at",
                        params![gid, anchor, now()],
                    )?;
                }
            }
            Self::assign_titles_in(&tx)?;
            let tid: i64 = tx.query_row("SELECT title_id FROM games WHERE id=?1", params![anchor], |r| r.get(0))?;
            tx.commit()?;
            Ok(tid)
        })
    }

    /// Detach one copy into its own title (and keep it there across re-syncs).
    /// Returns the new title id of the copy.
    pub fn split_copy(&self, game_id: i64) -> Result<i64> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            let title_id: i64 = tx
                .query_row("SELECT title_id FROM games WHERE id=?1 AND removed=0", params![game_id], |r| r.get(0))
                .optional()?
                .ok_or_else(|| Refused(format!("copy {game_id} not found")))?;
            let siblings: i64 =
                tx.query_row("SELECT COUNT(*) FROM games WHERE title_id=?1 AND removed=0", params![title_id], |r| r.get(0))?;
            if siblings < 2 {
                return Err(Refused("that title has only one copy; nothing to split".into()).into());
            }
            tx.execute("DELETE FROM merge_rules WHERE game_id=?1", params![game_id])?;
            // Merges anchored on this copy stay with the rest of its title:
            // re-anchor them on a copy that belonged there on its own (not
            // one that was merged in), so the title keeps its identity.
            let sibling: Option<i64> = tx
                .query_row(
                    "SELECT id FROM games WHERE title_id=?1 AND removed=0 AND id<>?2
                     ORDER BY id IN (SELECT game_id FROM merge_rules WHERE with_game_id=?2), (source<>'steam'), id LIMIT 1",
                    params![title_id, game_id],
                    |r| r.get(0),
                )
                .optional()?;
            match sibling {
                Some(sib) => tx.execute("UPDATE merge_rules SET with_game_id=?2 WHERE with_game_id=?1", params![game_id, sib])?,
                None => tx.execute("DELETE FROM merge_rules WHERE with_game_id=?1", params![game_id])?,
            };
            tx.execute("DELETE FROM merge_rules WHERE game_id=with_game_id", [])?;
            tx.execute("INSERT OR REPLACE INTO split_rules(game_id,created_at) VALUES(?1,?2)", params![game_id, now()])?;
            // Its appid came from the group; drop it unless it is a native Steam copy so it does not re-group by appid.
            tx.execute("UPDATE games SET appid=NULL, steam_matched=0 WHERE id=?1 AND source<>'steam'", params![game_id])?;
            Self::assign_titles_in(&tx)?;
            let tid: i64 = tx.query_row("SELECT title_id FROM games WHERE id=?1", params![game_id], |r| r.get(0))?;
            // A split non-Steam copy was separated from its Steam match on purpose; don't re-match it.
            tx.execute(
                "INSERT OR REPLACE INTO steam_match(title_id,fetched_at,success)
                 SELECT ?1, ?2, 0 WHERE NOT EXISTS (SELECT 1 FROM games WHERE title_id=?1 AND source='steam' AND removed=0)",
                params![tid, now()],
            )?;
            restore_snapshot(&tx, game_id, tid)?;
            // The play diary's Steam readings name the account and appid they
            // came from, so the split copy's own go with it; launches and the
            // wishlist name only the title, so they stay where they are.
            tx.execute(
                "UPDATE play_diary SET title_id=?2 WHERE title_id=?1 AND source='steam'
                   AND (account_id, appid) IN (SELECT account_id, appid FROM games WHERE id=?3 AND source='steam' AND appid IS NOT NULL)",
                params![title_id, tid, game_id],
            )?;
            tx.commit()?;
            Ok(tid)
        })
    }

    /// Drop Steam matches whose names score below `min_similarity` against
    /// the copy's (`names::score`), so an earlier lax match cannot glue
    /// "Hades II" onto "Hades". Cleared copies are retried by the next
    /// enrich. Returns the number of copies unmatched.
    pub fn prune_bad_matches(&self, min_similarity: f64) -> Result<usize> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            // Only copies whose appid came from a store lookup; titles that also hold a
            // native Steam copy got their appid from grouping, which is authoritative.
            let rows: Vec<(i64, String, i64, String, Option<String>)> = {
                let mut stmt = tx.prepare(
                    "SELECT g.id, g.name, t.id, t.name, m.matched_name FROM games g
                     JOIN titles t ON t.id=g.title_id JOIN steam_match m ON m.title_id=t.id AND m.success=1
                     WHERE g.steam_matched=1 AND g.removed=0
                       AND NOT EXISTS (SELECT 1 FROM games s WHERE s.title_id=t.id AND s.source='steam' AND s.removed=0)",
                )?;
                let it = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?;
                it.collect::<std::result::Result<_, _>>()?
            };
            let mut n = 0;
            for (gid, gname, tid, tname, matched) in rows {
                let sim = crate::names::score(&gname, matched.as_deref().unwrap_or(&tname));
                if sim < min_similarity {
                    tx.execute("UPDATE games SET appid=NULL, steam_matched=0 WHERE id=?1", params![gid])?;
                    tx.execute("DELETE FROM steam_match WHERE title_id=?1", params![tid])?;
                    n += 1;
                }
            }
            if n > 0 {
                Self::assign_titles_in(&tx)?;
            }
            tx.commit()?;
            Ok(n)
        })
    }
}

/// Keep what title `old` had of its own (each player's status and note,
/// rating, Play later and hide, and its ending switch) before a merge folds
/// it into another and deletes it, so a split can give it back. It goes on
/// the lowest-id copy that left it without a snapshot of its own, since a
/// copy merged into `old` earlier keeps its first title's; when every copy
/// has one, on the lowest-id copy. A title with nothing of its own leaves no
/// snapshot.
fn keep_snapshot(c: &Connection, old: i64, copies: &[i64]) -> Result<()> {
    let has = |g: i64| -> Result<bool> {
        Ok(c.query_row("SELECT 1 FROM merge_snapshots WHERE game_id=?1", params![g], |_| Ok(())).optional()?.is_some())
    };
    let mut game = None;
    for &g in copies {
        let key = (has(g)?, g);
        if game.is_none_or(|best| key < best) {
            game = Some(key);
        }
    }
    let Some((_, game)) = game else { return Ok(()) };
    let Some((name, no_ending)) = c
        .query_row("SELECT name, no_ending FROM titles WHERE id=?1", params![old], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?))
        })
        .optional()?
    else {
        return Ok(());
    };
    let marks = "SELECT p.id, o.status, o.note, o.updated_at, r.stars, r.at, l.added_at, h.scope, h.created_at FROM players p
         LEFT JOIN overrides o ON o.player_id=p.id AND o.title_id=?1
         LEFT JOIN game_ratings r ON r.player_id=p.id AND r.title_id=?1
         LEFT JOIN play_later l ON l.player_id=p.id AND l.title_id=?1
         LEFT JOIN hidden_titles h ON h.player_id=p.id AND h.title_id=?1
         WHERE o.status IS NOT NULL OR r.stars IS NOT NULL OR l.added_at IS NOT NULL OR h.scope IS NOT NULL";
    let any: bool = c.query_row(&format!("SELECT EXISTS ({marks})"), params![old], |r| r.get(0))?;
    if !any && no_ending.is_none() {
        return Ok(());
    }
    c.execute("DELETE FROM merge_snapshot_marks WHERE game_id=?1", params![game])?;
    c.execute(
        "INSERT OR REPLACE INTO merge_snapshots(game_id,title_name,no_ending,merged_at) VALUES(?1,?2,?3,?4)",
        params![game, name, no_ending, now()],
    )?;
    c.execute(
        &format!(
            "INSERT INTO merge_snapshot_marks(game_id,player_id,status,note,updated_at,stars,rated_at,play_later_at,hidden_scope,hidden_at)
             SELECT ?2, * FROM ({marks})"
        ),
        params![old, game],
    )?;
    Ok(())
}

/// Give split copy `game`'s new title `tid` what its first title had before
/// a merge folded it in ([`keep_snapshot`]), where the new title has none,
/// and drop the snapshot. Picks, stop reasons, feedback, launches in the
/// play diary and the wishlist stay with the title it was merged into, since
/// nothing records which copy they were about (`split_copy` moves the
/// diary's Steam readings, which do); the metadata comes back with the next
/// enrich.
fn restore_snapshot(c: &Connection, game: i64, tid: i64) -> Result<()> {
    let Some(no_ending) =
        c.query_row("SELECT no_ending FROM merge_snapshots WHERE game_id=?1", params![game], |r| r.get::<_, Option<i64>>(0)).optional()?
    else {
        return Ok(());
    };
    c.execute("UPDATE titles SET no_ending=?2 WHERE id=?1 AND no_ending IS NULL", params![tid, no_ending])?;
    for sql in [
        "INSERT OR IGNORE INTO overrides(player_id,title_id,status,note,updated_at)
         SELECT player_id,?2,status,note,updated_at FROM merge_snapshot_marks WHERE game_id=?1 AND status IS NOT NULL",
        "INSERT OR IGNORE INTO game_ratings(player_id,title_id,stars,at)
         SELECT player_id,?2,stars,rated_at FROM merge_snapshot_marks WHERE game_id=?1 AND stars IS NOT NULL",
        "INSERT OR IGNORE INTO play_later(player_id,title_id,added_at)
         SELECT player_id,?2,play_later_at FROM merge_snapshot_marks WHERE game_id=?1 AND play_later_at IS NOT NULL",
        "INSERT OR IGNORE INTO hidden_titles(player_id,title_id,scope,created_at)
         SELECT player_id,?2,hidden_scope,hidden_at FROM merge_snapshot_marks WHERE game_id=?1 AND hidden_scope IS NOT NULL",
    ] {
        c.execute(sql, params![game, tid])?;
    }
    c.execute("DELETE FROM merge_snapshot_marks WHERE game_id=?1", params![game])?;
    c.execute("DELETE FROM merge_snapshots WHERE game_id=?1", params![game])?;
    Ok(())
}
