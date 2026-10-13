//! GOG Galaxy 2.0 import from `galaxy-2.0.db` (Windows:
//! `C:\ProgramData\GOG.com\Galaxy\storage\galaxy-2.0.db`). Galaxy aggregates
//! GOG plus every linked platform, and tracks playtime. Best-effort: the
//! schema is undocumented. Galaxy keeps the file open while it runs, so it is
//! copied with SQLite's backup API and the copy is read.
//!
//! Galaxy's library holds more than what you own, so these are left out:
//! - releases of an integration the `galaxy_skip` setting names. Galaxy's
//!   other stores come through community plugins that can disappear or
//!   disconnect, and it keeps unlinked integrations "Connected", so its
//!   connection state is reported, never acted on: each integration is
//!   listed with when Galaxy last added a game to it, for the user to judge
//!   (decided 2026-10-09);
//! - releases Galaxy says aren't owned (`LicensedReleases.isOwned`, which
//!   covers Steam Family Sharing's `SubscriptionReleases`), and subscription
//!   titles (an `@subscription` key, as EA Play's);
//! - betas, test servers, playtests, demos and teasers, by name, as the Steam
//!   sync leaves them out.

use crate::db::ImportedGame;
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Map Galaxy release-key prefixes to our source names.
fn source_for(prefix: &str) -> &str {
    prefix
}

/// What a Galaxy database holds that UpNext reads.
#[derive(Debug, Default)]
pub struct GalaxyLibrary {
    pub games: Vec<ImportedGame>,
    /// Every integration Galaxy has games from, GOG's own first, kept or not.
    pub integrations: Vec<Integration>,
    /// Releases Galaxy says aren't owned (family shared) and subscription titles.
    pub not_owned: usize,
    /// Betas, test servers, playtests, demos and teasers.
    pub non_games: usize,
}

/// One store Galaxy brings games from (`gog`, `steam`, `origin`…).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Integration {
    /// Galaxy's name for it, which is the source's id: `origin`, `xboxone`.
    pub platform: String,
    /// Games from it, after the rules above, whether kept or not.
    pub games: usize,
    /// When Galaxy last added a game from it (`YYYY-MM-DD`). Long ago can
    /// mean the integration was unlinked.
    pub last_added: Option<String>,
    /// What Galaxy's `PlatformConnections` says, when it says anything.
    /// Reported only: it isn't reliable either way.
    pub connected: Option<bool>,
    /// The `galaxy_skip` setting leaves its games out.
    pub left_out: bool,
}

/// Read the library, leaving out the integrations `skip` names.
pub fn read_library(path: &Path, skip: &[String]) -> Result<GalaxyLibrary> {
    if !path.is_file() {
        bail!("Galaxy database not found: {}", path.display());
    }
    // Declared first so it's removed after the connection to it closes.
    let copy = TempFile(std::env::temp_dir().join(format!("upnext-galaxy-{}.db", uuid::Uuid::new_v4().simple())));
    let conn = snapshot(path, &copy.0).with_context(|| format!("copying {}", path.display()))?;
    read(&conn, path, skip)
}

/// The `galaxy_skip` setting as a list: `origin,xboxone`.
pub fn skip_list(setting: Option<&str>) -> Vec<String> {
    setting.unwrap_or_default().split(',').map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect()
}

/// An integration's name as people know it: `origin` is the EA app.
pub fn platform_label(platform: &str) -> String {
    match platform {
        "gog" => "GOG".into(),
        "steam" => "Steam".into(),
        "epic" => "Epic".into(),
        "amazon" => "Amazon".into(),
        "origin" | "eaApp" => "EA app".into(),
        "uplay" => "Ubisoft Connect".into(),
        "xboxone" => "Xbox".into(),
        "battlenet" => "Battle.net".into(),
        "psn" => "PlayStation".into(),
        "itch" => "itch.io".into(),
        "humble" => "Humble".into(),
        "rockstar" => "Rockstar".into(),
        "bethesda" => "Bethesda.net".into(),
        other => {
            let mut c = other.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    }
}

/// A file removed when dropped.
struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Copy the database at `path` to `to` with SQLite's backup API, which
/// reads a consistent copy while Galaxy has the file open (and starts over
/// if Galaxy writes to it meanwhile), and open the copy.
fn snapshot(path: &Path, to: &Path) -> Result<Connection> {
    let src = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    src.busy_timeout(std::time::Duration::from_secs(5))?;
    let mut dst = Connection::open(to)?;
    rusqlite::backup::Backup::new(&src, &mut dst)?.run_to_completion(1024, std::time::Duration::from_millis(50), None)?;
    Ok(dst)
}

/// When Galaxy last saw a game played: a `LastPlayedDates` date, as text
/// (mostly `2026-08-01T22:15:00+0200`, with no colon in the offset; some
/// `2026-08-01 20:15:00`, UTC) or seconds.
fn parse_played_at(v: &rusqlite::types::Value) -> Option<i64> {
    use rusqlite::types::Value as V;
    match v {
        V::Integer(n) => Some(*n),
        V::Text(t) => {
            let t = t.trim();
            chrono::DateTime::parse_from_rfc3339(t)
                .or_else(|_| chrono::DateTime::parse_from_str(t, "%Y-%m-%dT%H:%M:%S%.f%z"))
                .map(|d| d.timestamp())
                .ok()
                .or_else(|| {
                    ["%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S%.f"]
                        .iter()
                        .find_map(|f| chrono::NaiveDateTime::parse_from_str(t, f).ok())
                        .map(|d| d.and_utc().timestamp())
                })
                .or_else(|| t.parse().ok())
        }
        _ => None,
    }
    .filter(|&n| n > 0)
}

fn read(conn: &Connection, path: &Path, skip: &[String]) -> Result<GalaxyLibrary> {
    let has_table = |name: &str| -> Result<bool> {
        Ok(conn
            .query_row("SELECT name FROM sqlite_master WHERE type='table' AND name=?1", params![name], |r| r.get::<_, String>(0))
            .optional()?
            .is_some())
    };
    let has_column = |table: &str, column: &str| -> Result<bool> {
        Ok(conn.query_row("SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2", params![table, column], |_| Ok(())).optional()?.is_some())
    };
    for t in ["LibraryReleases", "GamePieces", "GamePieceTypes"] {
        if !has_table(t)? {
            bail!("{} does not look like a Galaxy 2.0 database (missing table {t})", path.display());
        }
    }

    // Piece type ids by name.
    let mut types: HashMap<String, i64> = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT id, type FROM GamePieceTypes")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
            let (id, ty) = row?;
            types.insert(ty, id);
        }
    }
    let type_id = |name: &str| types.get(name).copied();

    // Release keys in the library, excluding DLC and hidden entries when the
    // property tables exist.
    let mut keys: Vec<String> = vec![];
    {
        let sql = if has_table("ReleaseProperties")? {
            "SELECT DISTINCT lr.releaseKey FROM LibraryReleases lr
             LEFT JOIN ReleaseProperties rp ON rp.releaseKey = lr.releaseKey
             WHERE COALESCE(rp.isDlc, 0) = 0 AND COALESCE(rp.isVisibleInLibrary, 1) = 1"
        } else {
            "SELECT DISTINCT releaseKey FROM LibraryReleases"
        };
        let mut stmt = conn.prepare(sql)?;
        for k in stmt.query_map([], |r| r.get::<_, String>(0))? {
            keys.push(k?);
        }
    }
    let hidden: HashSet<String> = if has_table("UserReleaseProperties")? {
        let mut stmt = conn.prepare("SELECT releaseKey FROM UserReleaseProperties WHERE isHidden = 1")?;
        stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<std::result::Result<_, _>>()?
    } else {
        Default::default()
    };
    // Licences Galaxy says aren't owned: Steam Family Sharing's games, which
    // `SubscriptionReleases` lists beside the owned ones it shares.
    let not_owned: HashSet<String> = if has_table("LicensedReleases")? {
        let mut stmt = conn.prepare(
            "SELECT lr.releaseKey FROM LicensedReleases li JOIN LibraryReleases lr ON lr.id = li.libraryId
             GROUP BY lr.releaseKey HAVING MAX(COALESCE(li.isOwned, 1)) = 0",
        )?;
        stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<std::result::Result<_, _>>()?
    } else {
        Default::default()
    };
    // Each integration's state; Galaxy writes "Connected" and "Disconnected".
    let connected: Option<HashMap<String, bool>> = if has_table("PlatformConnections")? {
        let mut stmt = conn.prepare("SELECT platform, connectionState FROM PlatformConnections")?;
        let mut states: HashMap<String, bool> = HashMap::new();
        for row in stmt.query_map([], |r| Ok((r.get::<_, rusqlite::types::Value>(0)?, r.get::<_, Option<String>>(1)?)))? {
            let (platform, state) = row?;
            let platform = match platform {
                rusqlite::types::Value::Text(t) => t,
                // Not seen, but a number would be a `Platforms` id.
                rusqlite::types::Value::Integer(id) => {
                    match conn.query_row("SELECT name FROM Platforms WHERE id=?1", params![id], |r| r.get::<_, String>(0)).optional()? {
                        Some(name) => name,
                        None => continue,
                    }
                }
                _ => continue,
            };
            let on = state.is_some_and(|s| s.trim().eq_ignore_ascii_case("connected"));
            *states.entry(platform).or_default() |= on;
        }
        Some(states)
    } else {
        None
    };
    // When Galaxy last added a game, by integration.
    let mut last_added: HashMap<String, String> = HashMap::new();
    if has_table("ProductPurchaseDates")? && has_column("ProductPurchaseDates", "addedDate")? {
        let mut stmt = conn.prepare("SELECT gameReleaseKey, addedDate FROM ProductPurchaseDates WHERE addedDate IS NOT NULL")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (key, added) = row?;
            let (Some((prefix, _)), Some(day)) = (key.split_once('_'), added.get(..10)) else { continue };
            let e = last_added.entry(prefix.to_string()).or_default();
            if day > e.as_str() {
                *e = day.to_string();
            }
        }
    }
    // GOG's own games are Galaxy's and can't be left out.
    let left_out = |platform: &str| platform != "gog" && skip.iter().any(|s| s == platform);

    let piece = |key: &str, ty: &str| -> Result<Option<Value>> {
        let Some(tid) = type_id(ty) else { return Ok(None) };
        let v: Option<String> = conn
            .query_row(
                "SELECT value FROM GamePieces WHERE releaseKey=?1 AND gamePieceTypeId=?2 ORDER BY userId DESC LIMIT 1",
                params![key, tid],
                |r| r.get(0),
            )
            .optional()?;
        Ok(v.and_then(|s| serde_json::from_str(&s).ok()))
    };

    // Minutes played, per release. A few versions also keep the end of the
    // last session here; others only in `LastPlayedDates`.
    let mut times: HashMap<String, i64> = HashMap::new();
    let mut last_played: HashMap<String, i64> = HashMap::new();
    if has_table("GameTimes")? {
        let last = if has_column("GameTimes", "lastSessionEnd")? { "lastSessionEnd" } else { "NULL" };
        let mut stmt = conn.prepare(&format!("SELECT releaseKey, minutesInGame, {last} FROM GameTimes"))?;
        for row in
            stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?.unwrap_or(0), r.get::<_, Option<i64>>(2)?)))?
        {
            let (k, m, end) = row?;
            let t = times.entry(k.clone()).or_default();
            *t = (*t).max(m);
            if let Some(end) = end.filter(|&e| e > 0) {
                let l = last_played.entry(k).or_default();
                *l = (*l).max(end);
            }
        }
    }
    if has_table("LastPlayedDates")? {
        let mut stmt = conn.prepare("SELECT gameReleaseKey, lastPlayedDate FROM LastPlayedDates")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, rusqlite::types::Value>(1)?)))? {
            let (k, at) = row?;
            if let Some(at) = parse_played_at(&at) {
                let l = last_played.entry(k).or_default();
                *l = (*l).max(at);
            }
        }
    }
    let mut installed: std::collections::HashSet<String> = Default::default();
    if has_table("InstalledBaseProducts")? {
        let mut stmt = conn.prepare("SELECT productId FROM InstalledBaseProducts")?;
        for id in stmt.query_map([], |r| r.get::<_, i64>(0))? {
            installed.insert(format!("gog_{}", id?));
        }
    }
    if has_table("InstalledExternalProducts")? && has_table("Platforms")? {
        let mut stmt =
            conn.prepare("SELECT p.name, iep.productId FROM InstalledExternalProducts iep JOIN Platforms p ON p.id = iep.platformId")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (platform, pid) = row?;
            installed.insert(format!("{platform}_{pid}"));
        }
    }

    let mut lib = GalaxyLibrary::default();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for key in keys {
        if hidden.contains(&key) {
            continue;
        }
        let Some((prefix, pid)) = key.split_once('_') else { continue };
        if not_owned.contains(&key) || pid.contains("@subscription") {
            lib.not_owned += 1;
            continue;
        }
        let title = piece(&key, "title")?
            .or(piece(&key, "originalTitle")?)
            .and_then(|v| v.get("title").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_default();
        if title.trim().is_empty() {
            continue;
        }
        // Spacewar (480) is the Steam SDK's test app.
        if crate::steam_local::is_test_app(&title) || (prefix == "steam" && pid == "480") {
            lib.non_games += 1;
            continue;
        }
        *counts.entry(prefix.to_string()).or_default() += 1;
        if left_out(prefix) {
            continue;
        }
        let meta = piece(&key, "meta")?.or(piece(&key, "originalMeta")?).unwrap_or(Value::Null);
        // Cover art is in "images" (or "originalImages"); "media" holds it in
        // older shapes.
        let images = piece(&key, "images")?.or(piece(&key, "originalImages")?).or(piece(&key, "media")?).unwrap_or(Value::Null);
        let summary = piece(&key, "summary")?.and_then(|v| v.get("summary").and_then(Value::as_str).map(str::to_string));
        let strs = |v: &Value, k: &str| -> Vec<String> {
            v.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
        };
        let release_date = meta
            .get("releaseDate")
            .and_then(Value::as_i64)
            .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
            .map(|d| d.format("%Y-%m-%d").to_string());
        // No GameTimes row means Galaxy doesn't know, not zero minutes.
        let minutes = times.get(&key).copied();
        let last = last_played.get(&key).copied().unwrap_or(0);
        let source = source_for(prefix).to_string();
        let appid = if source == "steam" { pid.parse::<u32>().ok() } else { None };
        lib.games.push(ImportedGame {
            source,
            source_id: pid.to_string(),
            launcher: "galaxy".to_string(),
            name: title,
            appid,
            installed: installed.contains(&key),
            playtime_minutes: minutes,
            playtime_2weeks: 0,
            last_played: last,
            art_cover: images.get("background").and_then(Value::as_str).map(str::to_string),
            art_square: images.get("verticalCover").and_then(Value::as_str).map(str::to_string),
            description: summary,
            genres: strs(&meta, "genres"),
            developer: strs(&meta, "developers").into_iter().next(),
            release_date,
            store_url: None,
            run_url: Some(format!("goggalaxy://openGameView/{key}")),
        });
    }
    lib.integrations = counts
        .into_iter()
        .map(|(platform, games)| Integration {
            last_added: last_added.get(&platform).cloned(),
            connected: connected.as_ref().and_then(|c| c.get(&platform).copied()),
            left_out: left_out(&platform),
            games,
            platform,
        })
        .collect();
    lib.integrations.sort_by_key(|i| (i.platform != "gog", i.left_out, std::cmp::Reverse(i.games)));
    Ok(lib)
}

/// One line per integration for the sync's log: "EA app: 55 games, last
/// added 2024-05-18", "Xbox: 39 games left out in Settings".
pub fn integration_line(i: &Integration) -> String {
    let added = i.last_added.as_deref().map(|d| format!(", last added {d}")).unwrap_or_default();
    let games = if i.games == 1 { "1 game".to_string() } else { format!("{} games", i.games) };
    let state = match i.connected {
        Some(false) if i.platform != "gog" => ", not connected in Galaxy",
        _ => "",
    };
    let kept = if i.left_out { " left out in Settings" } else { "" };
    format!("{}: {games}{kept}{added}{state}", platform_label(&i.platform))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Galaxy's database in the shape it is described in publicly
    /// (`tests/fixtures/galaxy/galaxy-2.0.sql`).
    fn galaxy_fixture(path: &Path) -> Connection {
        let c = Connection::open(path).unwrap();
        c.execute_batch(&crate::testutil::fixture("galaxy/galaxy-2.0.sql")).unwrap();
        c
    }

    #[test]
    fn reads_a_galaxy_shaped_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("galaxy-2.0.db");
        galaxy_fixture(&path);
        let mut games = read_library(&path, &[]).unwrap().games;
        games.sort_by(|a, b| a.name.cmp(&b.name));
        let names: Vec<&str> = games.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["Hollow Hills", "Portal 2", "The Witcher"], "no DLC or hidden game; the player's own title wins");

        let hh = &games[0];
        assert_eq!((hh.source.as_str(), hh.source_id.as_str(), hh.appid), ("epic", "cf0a7b1c4e8d4f2a9b6c3d5e7f8a9b0c", None));
        assert_eq!((hh.playtime_minutes, hh.last_played, hh.installed), (Some(0), 0, false));
        assert_eq!((hh.release_date.as_deref(), hh.art_cover.as_deref()), (None, None));

        let p2 = &games[1];
        assert_eq!((p2.source.as_str(), p2.appid, p2.installed), ("steam", Some(620), true), "installed through Steam");
        assert_eq!((p2.playtime_minutes, p2.last_played), (None, 1783188000), "no GameTimes row; played 2026-07-04T18:00:00Z");
        assert_eq!(p2.art_cover.as_deref(), Some("https://example.com/my_portal2_bg.jpg"), "the player's images win");
        assert_eq!(p2.genres, ["Puzzle", "Shooter"]);

        let w = &games[2];
        assert_eq!((w.source.as_str(), w.source_id.as_str(), w.installed), ("gog", "1207658924", true));
        assert_eq!((w.playtime_minutes, w.last_played), (Some(754), 1785615300), "the larger of two users' times; 2026-08-01 20:15 UTC");
        assert_eq!((w.release_date.as_deref(), w.developer.as_deref()), (Some("2008-09-16"), Some("CD PROJEKT RED")));
        assert_eq!(
            (w.art_cover.as_deref(), w.art_square.as_deref()),
            (Some("https://images.gog.com/witcher_bg.jpg"), Some("https://images.gog.com/witcher_vc.jpg")),
            "from originalImages, not the screenshots in media"
        );
        assert_eq!(w.description.as_deref(), Some("Hunt monsters for coin."));
        assert_eq!(w.run_url.as_deref(), Some("goggalaxy://openGameView/gog_1207658924"));
    }

    /// Galaxy keeps the file open and writes to it: the copy is read, and
    /// a write that hasn't finished isn't in it.
    #[test]
    fn reads_while_galaxy_is_writing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("galaxy-2.0.db");
        let galaxy = galaxy_fixture(&path);
        galaxy
            .execute_batch("BEGIN IMMEDIATE; UPDATE GamePieces SET value = '{\"title\":\"Renamed\"}' WHERE gamePieceTypeId = 2;")
            .unwrap();
        let games = read_library(&path, &[]).unwrap().games;
        assert!(games.iter().any(|g| g.name == "The Witcher") && !games.iter().any(|g| g.name == "Renamed"));
        galaxy.execute_batch("COMMIT").unwrap();
        assert!(read_library(&path, &[]).unwrap().games.iter().any(|g| g.name == "Renamed"));
    }

    #[test]
    fn played_at() {
        use rusqlite::types::Value as V;
        assert_eq!(parse_played_at(&V::Text("2026-08-01 20:15:00".into())), Some(1785615300));
        assert_eq!(parse_played_at(&V::Text("2026-08-01T20:15:00.250".into())), Some(1785615300));
        assert_eq!(parse_played_at(&V::Text("2026-08-01T22:15:00+02:00".into())), Some(1785615300));
        assert_eq!(parse_played_at(&V::Text("2026-08-01T22:15:00+0200".into())), Some(1785615300), "Galaxy's usual form");
        assert_eq!(parse_played_at(&V::Integer(1785615300)), Some(1785615300));
        assert_eq!(parse_played_at(&V::Text("".into())), None);
        assert_eq!(parse_played_at(&V::Null), None);
    }

    /// The tables as the reader first assumed them: art in "media" and the
    /// last session in `GameTimes`. Kept for those fallbacks.
    fn galaxy_db(path: &Path) {
        let c = Connection::open(path).unwrap();
        c.execute_batch(
            r#"
            CREATE TABLE LibraryReleases (releaseKey TEXT);
            CREATE TABLE GamePieceTypes (id INTEGER, type TEXT);
            CREATE TABLE GamePieces (releaseKey TEXT, gamePieceTypeId INTEGER, userId INTEGER, value TEXT);
            CREATE TABLE ReleaseProperties (releaseKey TEXT, isDlc INTEGER, isVisibleInLibrary INTEGER);
            CREATE TABLE UserReleaseProperties (releaseKey TEXT, isHidden INTEGER);
            CREATE TABLE GameTimes (releaseKey TEXT, minutesInGame INTEGER, lastSessionEnd INTEGER);
            CREATE TABLE InstalledBaseProducts (productId INTEGER);
            CREATE TABLE InstalledExternalProducts (platformId INTEGER, productId TEXT);
            CREATE TABLE Platforms (id INTEGER, name TEXT);
            INSERT INTO GamePieceTypes VALUES (1, 'title'), (2, 'meta'), (3, 'media'), (4, 'summary'), (5, 'originalTitle');
            INSERT INTO LibraryReleases VALUES ('gog_1207658924'), ('steam_620'), ('epic_dlc'), ('gog_2'), ('gog_3'), ('bogus');
            INSERT INTO ReleaseProperties VALUES ('epic_dlc', 1, 1);
            INSERT INTO UserReleaseProperties VALUES ('gog_2', 1);
            INSERT INTO GamePieces VALUES
              ('gog_1207658924', 1, 1, '{"title": "The Witcher"}'),
              ('gog_1207658924', 1, 2, '{"title": "The Witcher: Enhanced Edition"}'),
              ('gog_1207658924', 2, 2, '{"releaseDate": 1221523200, "genres": ["RPG"], "developers": ["CD PROJEKT RED", "Other"]}'),
              ('gog_1207658924', 3, 2, '{"background": "https://img/bg.jpg", "verticalCover": "https://img/vc.jpg"}'),
              ('gog_1207658924', 4, 2, '{"summary": "Monster hunting."}'),
              ('steam_620', 5, 2, '{"title": "Portal 2"}'),
              ('epic_dlc', 1, 2, '{"title": "Some DLC"}'),
              ('gog_2', 1, 2, '{"title": "Hidden Game"}'),
              ('gog_3', 1, 2, '{"title": "   "}'),
              ('bogus', 1, 2, '{"title": "No Prefix"}');
            INSERT INTO GameTimes VALUES ('gog_1207658924', 120, 1790000000);
            INSERT INTO InstalledBaseProducts VALUES (1207658924);
            INSERT INTO Platforms VALUES (7, 'steam');
            INSERT INTO InstalledExternalProducts VALUES (7, '620');
            "#,
        )
        .unwrap();
    }

    #[test]
    fn reads_the_library() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("galaxy-2.0.db");
        galaxy_db(&path);
        let mut games = read_library(&path, &[]).unwrap().games;
        games.sort_by(|a, b| a.name.cmp(&b.name));
        let names: Vec<&str> = games.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["Portal 2", "The Witcher: Enhanced Edition"], "DLC, hidden, untitled and malformed keys are skipped");

        let p2 = &games[0];
        assert_eq!((p2.source.as_str(), p2.source_id.as_str(), p2.appid), ("steam", "620", Some(620)));
        assert!(p2.installed, "installed through an external platform");
        assert_eq!(p2.playtime_minutes, None, "no GameTimes row means unknown, not zero");

        let w = &games[1];
        assert_eq!((w.source.as_str(), w.playtime_minutes, w.last_played), ("gog", Some(120), 1790000000));
        assert!(w.installed);
        assert_eq!(w.release_date.as_deref(), Some("2008-09-16"));
        assert_eq!((w.genres.clone(), w.developer.as_deref()), (vec!["RPG".to_string()], Some("CD PROJEKT RED")));
        assert_eq!((w.art_cover.as_deref(), w.art_square.as_deref()), (Some("https://img/bg.jpg"), Some("https://img/vc.jpg")));
        assert_eq!(w.description.as_deref(), Some("Monster hunting."));
        assert_eq!(w.run_url.as_deref(), Some("goggalaxy://openGameView/gog_1207658924"));
    }

    /// Galaxy's database in the shape a real one had in 2026
    /// (`tests/fixtures/galaxy/galaxy-2.0-2026.sql`).
    fn galaxy_2026(path: &Path) {
        Connection::open(path).unwrap().execute_batch(&crate::testutil::fixture("galaxy/galaxy-2.0-2026.sql")).unwrap();
    }

    #[test]
    fn reads_the_2026_shape() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("galaxy-2.0.db");
        galaxy_2026(&path);
        let lib = read_library(&path, &[]).unwrap();
        let mut keys: Vec<String> = lib.games.iter().map(|g| format!("{}_{}", g.source, g.source_id)).collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "epic_cf0a7b1c4e8d4f2a9b6c3d5e7f8a9b0c",
                "gog_1207658924",
                "origin_71234",
                "steam_413150",
                "steam_620",
                "uplay_635",
                "xboxone_9n1234567890"
            ],
            "every integration kept, connected or not; owned family games kept"
        );
        assert_eq!((lib.not_owned, lib.non_games), (2, 3), "family shared and EA Play; Spacewar, a beta and an open beta");

        let w = lib.games.iter().find(|g| g.source == "gog").unwrap();
        assert_eq!(
            (w.name.as_str(), w.playtime_minutes, w.last_played, w.installed),
            ("The Witcher: Enhanced Edition", Some(754), 1785615300, true)
        );
        assert_eq!(w.art_cover.as_deref(), Some("https://images.gog.com/witcher_bg.jpg"), "from originalImages");
        assert_eq!(w.release_date.as_deref(), Some("2008-09-16"), "from originalMeta");
        let p2 = lib.games.iter().find(|g| g.source_id == "620").unwrap();
        assert_eq!((p2.playtime_minutes, p2.last_played, p2.installed, p2.art_cover.as_deref()), (Some(90), 1783188000, true, None));

        let summary: Vec<(&str, usize, Option<&str>, Option<bool>)> =
            lib.integrations.iter().map(|i| (i.platform.as_str(), i.games, i.last_added.as_deref(), i.connected)).collect();
        assert_eq!(
            summary,
            [
                ("gog", 1, Some("2019-06-01"), None),
                ("steam", 2, Some("2026-10-01"), Some(true)),
                ("epic", 1, Some("2026-10-02"), Some(true)),
                ("origin", 1, Some("2024-05-18"), Some(true)),
                ("uplay", 1, None, Some(false)),
                ("xboxone", 1, Some("2023-03-15"), Some(true)),
            ]
        );
        assert!(lib.integrations.iter().all(|i| !i.left_out));
        assert_eq!(integration_line(&lib.integrations[3]), "EA app: 1 game, last added 2024-05-18");
        assert_eq!(integration_line(&lib.integrations[4]), "Ubisoft Connect: 1 game, not connected in Galaxy");
    }

    /// The integrations the setting names are left out, and say so; GOG's
    /// own games can't be.
    #[test]
    fn leaves_out_the_integrations_named() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("galaxy-2.0.db");
        galaxy_2026(&path);
        let lib = read_library(&path, &skip_list(Some("origin, XBOXONE,gog"))).unwrap();
        assert!(lib.games.iter().all(|g| g.source != "origin" && g.source != "xboxone"));
        assert!(lib.games.iter().any(|g| g.source == "gog"));
        let xbox = lib.integrations.iter().find(|i| i.platform == "xboxone").unwrap();
        assert!(xbox.left_out && xbox.games == 1);
        assert_eq!(integration_line(xbox), "Xbox: 1 game left out in Settings, last added 2023-03-15");
        assert_eq!(lib.integrations.last().map(|i| i.platform.as_str()), Some("xboxone"), "left out ones last");
    }

    /// Both fixtures, so a change has to read both shapes.
    #[test]
    fn the_older_shape_reports_its_integrations_too() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("galaxy-2.0.db");
        galaxy_fixture(&path);
        let lib = read_library(&path, &[]).unwrap();
        let platforms: Vec<(&str, Option<bool>)> = lib.integrations.iter().map(|i| (i.platform.as_str(), i.connected)).collect();
        assert_eq!(platforms, [("gog", None), ("epic", None), ("steam", None)], "no PlatformConnections: nothing to report");
        assert_eq!((lib.not_owned, lib.non_games), (0, 0));
    }

    #[test]
    fn rejects_other_databases() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("other.db");
        Connection::open(&path).unwrap().execute_batch("CREATE TABLE t (x INTEGER);").unwrap();
        assert!(read_library(&path, &[]).unwrap_err().to_string().contains("missing table"));
        assert!(read_library(&dir.path().join("absent.db"), &[]).unwrap_err().to_string().contains("not found"));
    }
}
