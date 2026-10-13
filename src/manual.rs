//! Games added by hand: a list of titles for a store whose owned library
//! UpNext can't read without the person's password. The EA app and
//! Ubisoft Connect have no sign-in a program of ours may use, and neither
//! do Battle.net or the Xbox app, so a list typed or pasted in is the clean
//! way to have their games picked. GOG Galaxy reads them too, where it's
//! installed and linked to them.
//!
//! A list is the store's copies with the launcher `manual`: saving one
//! replaces the list before, and a game left off it leaves the library as a
//! game a sync no longer sees does. The copies are named only; enrich finds
//! their details on Steam by name, as it does for any store's.

use crate::db::PlayerId;
use crate::db::{Db, ImportedGame, SyncSummary, name_key};
use anyhow::{Result, bail};
use std::collections::{BTreeMap, HashSet};

/// The launcher a list's copies carry.
pub const LAUNCHER: &str = "manual";

/// The stores a list can be kept for, by their source ids (Galaxy's names,
/// so a game listed here and one read through Galaxy are the same store).
pub const STORES: [&str; 4] = ["origin", "uplay", "battlenet", "xboxone"];

/// The most titles a list keeps, far past any library, so a pasted file of
/// the wrong kind is refused rather than written.
pub const MAX_TITLES: usize = 5_000;

/// The longest title taken.
const MAX_TITLE_CHARS: usize = 200;

/// The titles of a pasted list: one a line, trimmed, with bullets, trademark
/// signs and repeats dropped, in the order given.
pub fn titles_from_lines<S: AsRef<str>>(lines: &[S]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = vec![];
    for line in lines.iter().flat_map(|l| l.as_ref().lines()) {
        let t: String = line.trim().trim_start_matches(['-', '*', '•']).chars().filter(|c| !matches!(c, '™' | '®' | '©')).collect();
        let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
        if t.is_empty() || t.chars().count() > MAX_TITLE_CHARS {
            continue;
        }
        let key = name_key(&t);
        if !key.is_empty() && seen.insert(key) {
            out.push(t);
        }
    }
    out
}

/// A listed title as a copy of `store`: its id is the title's own key, so
/// listing it again finds the same copy.
fn copy(store: &str, title: &str) -> ImportedGame {
    ImportedGame {
        source: store.to_string(),
        source_id: name_key(title).replace(' ', "-"),
        launcher: LAUNCHER.to_string(),
        name: title.to_string(),
        ..Default::default()
    }
}

/// Replace `owner`'s list for `store` with `titles` (as `titles_from_lines`
/// reads them). Each player's lists are their own: the copies go
/// into their account for the store.
pub fn save(db: &Db, owner: PlayerId, store: &str, titles: &[String]) -> Result<SyncSummary> {
    if !STORES.contains(&store) {
        bail!("games can be added by hand for {} only", STORES.map(crate::galaxy::platform_label).join(", "));
    }
    let titles = titles_from_lines(titles);
    if titles.len() > MAX_TITLES {
        bail!("that's {} titles; a list takes at most {MAX_TITLES}", titles.len());
    }
    let games: Vec<ImportedGame> = titles.iter().map(|t| copy(store, t)).collect();
    db.sync_source(owner, LAUNCHER, store, &games)
}

/// Every store's list of `owner`'s, by source id, with an empty list for a
/// store that has none.
pub fn lists(db: &Db, owner: PlayerId) -> Result<BTreeMap<String, Vec<String>>> {
    let mut out = BTreeMap::new();
    for store in STORES {
        out.insert(store.to_string(), db.launcher_copies(owner, LAUNCHER, store)?.into_iter().map(|g| g.name).collect());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pasted_list_reads_one_title_a_line() {
        let pasted = ["Mass Effect™ Legendary Edition\n\n  - Far Cry® 5  \n• Dead Space\nmass effect legendary edition", "* Anno 1800"];
        assert_eq!(titles_from_lines(&pasted), ["Mass Effect Legendary Edition", "Far Cry 5", "Dead Space", "Anno 1800"]);
        assert!(titles_from_lines(&["x".repeat(201)]).is_empty(), "a line that long isn't a title");
    }

    #[test]
    fn saving_a_list_replaces_the_one_before() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("upnext.db"), 120).unwrap();
        let s = save(&db, PlayerId::FIRST, "origin", &["Dead Space\nMass Effect Legendary Edition".into()]).unwrap();
        assert_eq!((s.total, s.added), (2, 2));
        let s = save(&db, PlayerId::FIRST, "origin", &["Dead Space".into(), "Battlefield 1".into()]).unwrap();
        assert_eq!((s.added, s.updated, s.removed), (1, 1, 1));
        let lists = lists(&db, PlayerId::FIRST).unwrap();
        assert_eq!(lists["origin"], ["Battlefield 1", "Dead Space"]);
        assert!(lists["uplay"].is_empty());
        let games = db.all_games(PlayerId::FIRST).unwrap();
        let copy = &games.iter().find(|g| g.name == "Dead Space").unwrap().copies[0];
        assert_eq!((copy.source.as_str(), copy.launcher.as_str(), copy.installed), ("origin", "manual", false));
    }

    #[test]
    fn only_the_stores_upnext_cant_read_take_a_list() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("upnext.db"), 120).unwrap();
        let e = save(&db, PlayerId::FIRST, "steam", &["Portal 2".into()]).unwrap_err().to_string();
        assert!(e.contains("EA app, Ubisoft Connect, Battle.net, Xbox"), "{e}");
        let many: Vec<String> = (0..=MAX_TITLES).map(|i| format!("Game {i}")).collect();
        assert!(save(&db, PlayerId::FIRST, "uplay", &many).is_err());
        assert!(db.all_games(PlayerId::FIRST).unwrap().is_empty());
    }
}
