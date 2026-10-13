//! Heroic Games Launcher import: reads Heroic's local library caches for the
//! Epic (legendary), GOG and Amazon (nile) stores. Read-only; no credentials
//! are touched.

use crate::db::ImportedGame;
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The Heroic config directory: the first candidate with a `store_cache`.
pub fn find_config_dir(explicit: Option<&Path>) -> Option<PathBuf> {
    config_dir_candidates(explicit).into_iter().find(|p| p.join("store_cache").is_dir())
}

/// Where Heroic's config may be: the `heroic_dir` setting, then Linux
/// (native and Flatpak), then Windows. Heroic is an Electron app, so on
/// Windows it is in `%APPDATA%\heroic`, with the same layout inside.
pub fn config_dir_candidates(explicit: Option<&Path>) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = explicit.map(Path::to_path_buf).into_iter().collect();
    // Tests never read the machine's own Heroic (as Steam's).
    if !cfg!(test) {
        candidates.extend(machine_config_dirs());
    }
    candidates
}

/// Heroic's usual places on this machine, under `HOME` and `APPDATA`.
fn machine_config_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![];
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        dirs.extend(home_config_dirs(&home).into_iter().take(2));
    }
    if let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) {
        dirs.push(appdata.join("heroic"));
    }
    dirs
}

/// Heroic's places under a home folder: Linux's own, the Flatpak's, and
/// Windows' `AppData\Roaming`.
fn home_config_dirs(home: &Path) -> [PathBuf; 3] {
    [home.join(".config/heroic"), home.join(".var/app/com.heroicgameslauncher.hgl/config/heroic"), home.join("AppData/Roaming/heroic")]
}

/// Heroic's folder in each of `homes` that has one, for setup to offer
/// (`other_users`). Only whether its store cache is there is checked.
pub fn config_dirs_in(homes: &[PathBuf]) -> Vec<PathBuf> {
    homes.iter().filter_map(|home| home_config_dirs(home).into_iter().find(|p| p.join("store_cache").is_dir())).collect()
}

struct Runner {
    file: &'static str,
    list_key: &'static str,
    runner: &'static str,
    source: &'static str,
}

const RUNNERS: [Runner; 3] = [
    Runner { file: "legendary_library.json", list_key: "library", runner: "legendary", source: "epic" },
    Runner { file: "gog_library.json", list_key: "games", runner: "gog", source: "gog" },
    Runner { file: "nile_library.json", list_key: "library", runner: "nile", source: "amazon" },
];

fn s(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::trim).filter(|x| !x.is_empty()).map(str::to_string)
}

/// What Heroic's store caches hold.
#[derive(Debug, Default)]
pub struct Libraries {
    /// `(source, games)` per store whose cache was read.
    pub stores: Vec<(String, Vec<ImportedGame>)>,
    /// Why each cache that is there wasn't read (Heroic halfway through
    /// writing it, or damaged). That store's games stay as they were.
    pub unread: Vec<String>,
}

/// Read every store Heroic knows about, per store that has a cache file;
/// DLC, redistributables and mods are skipped. A cache that can't be read
/// leaves the others alone; only when none of them can is it an error.
pub fn read_libraries(config_dir: &Path) -> Result<Libraries> {
    let cache = config_dir.join("store_cache");
    let legendary_meta = config_dir.join("legendaryConfig/legendary/metadata");
    let mut out = Libraries::default();
    for r in RUNNERS.iter() {
        let path = cache.join(r.file);
        if !path.is_file() {
            continue;
        }
        let read = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))
            .and_then(|text| serde_json::from_str::<Value>(&text).with_context(|| format!("parsing {}", path.display())));
        let json = match read {
            Ok(json) => json,
            Err(e) => {
                out.unread.push(format!("{e:#}"));
                continue;
            }
        };
        let list = json.get(r.list_key).and_then(Value::as_array).cloned().unwrap_or_default();
        let mut games = vec![];
        for g in &list {
            let Some(app_name) = s(g, "app_name") else { continue };
            let Some(title) = s(g, "title") else { continue };
            if app_name == "gog-redist" || title.to_lowercase().contains("redistributable") {
                continue;
            }
            // Demos, betas and test builds are separate library entries without flags.
            if is_demo_title(&title) || crate::steam_local::is_test_app(&title) {
                continue;
            }
            if g.pointer("/install/is_dlc").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            if r.runner == "legendary" {
                // Legendary's metadata marks DLC via a non-empty mainGameItemList.
                let meta_path = legendary_meta.join(format!("{app_name}.json"));
                if let Ok(t) = std::fs::read_to_string(&meta_path)
                    && let Ok(m) = serde_json::from_str::<Value>(&t)
                {
                    let is_dlc = m.pointer("/metadata/mainGameItemList").and_then(Value::as_array).map(|a| !a.is_empty()).unwrap_or(false);
                    let categories: Vec<String> = m
                        .pointer("/metadata/categories")
                        .and_then(Value::as_array)
                        .map(|a| a.iter().filter_map(|c| c.get("path").and_then(Value::as_str)).map(str::to_string).collect())
                        .unwrap_or_default();
                    if is_dlc || categories.iter().any(|c| c == "addons" || c == "software" || c == "engines") {
                        continue;
                    }
                }
            }
            let extra = g.get("extra").cloned().unwrap_or(Value::Null);
            let description = s(g, "description")
                .or_else(|| extra.pointer("/about/shortDescription").and_then(Value::as_str).map(str::to_string))
                .or_else(|| extra.pointer("/about/description").and_then(Value::as_str).map(str::to_string))
                .filter(|d| !d.is_empty() && d != &title);
            let genres: Vec<String> = extra
                .get("genres")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default();
            let release_date =
                extra.get("releaseDate").and_then(Value::as_str).map(|d| d.chars().take(10).collect::<String>()).filter(|d| !d.is_empty());
            let store_url = s(g, "store_url").or_else(|| extra.get("storeUrl").and_then(Value::as_str).map(str::to_string)).or_else(|| {
                match r.source {
                    "gog" => Some(format!("https://www.gog.com/en/games?query={}", urlencode(&title))),
                    "amazon" => Some("https://gaming.amazon.com/home".to_string()),
                    _ => None,
                }
            });
            games.push(ImportedGame {
                source: r.source.to_string(),
                source_id: app_name.clone(),
                launcher: "heroic".to_string(),
                name: title,
                appid: None,
                installed: g.get("is_installed").and_then(Value::as_bool).unwrap_or(false),
                playtime_minutes: None,
                playtime_2weeks: 0,
                last_played: 0,
                art_cover: s(g, "art_cover").map(|u| epic_resize(&u, 640)),
                art_square: s(g, "art_square").map(|u| epic_resize(&u, 400)),
                description,
                genres,
                developer: s(g, "developer"),
                release_date,
                store_url,
                run_url: Some(format!("heroic://launch/{}/{}", r.runner, app_name)),
            });
        }
        out.stores.push((r.source.to_string(), games));
    }
    if out.stores.is_empty() && !out.unread.is_empty() {
        bail!("{}", out.unread.join("; "));
    }
    Ok(out)
}

/// "Foo Demo", "Foo DEMO", "Foo (Demo)", "Foo - Demo".
pub fn is_demo_title(title: &str) -> bool {
    let t = title.trim().to_lowercase();
    t.ends_with(" demo") || t.ends_with("(demo)") || t.ends_with("- demo") || t.ends_with(": demo")
}

/// Epic's CDN serves multi-megabyte originals; it supports server-side resizing.
pub(crate) fn epic_resize(url: &str, width: u32) -> String {
    if url.contains("epicgames.com") && !url.contains('?') { format!("{url}?resize=1&w={width}") } else { url.to_string() }
}

pub(crate) fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn write(root: &Path, rel: &str, v: serde_json::Value) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, v.to_string()).unwrap();
    }

    #[test]
    fn reads_each_store_and_skips_non_games() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "store_cache/legendary_library.json",
            json!({"library": [
                {"app_name": "Fortnite", "title": "Space Game", "developer": "Studio", "is_installed": true,
                 "art_cover": "https://cdn1.epicgames.com/cover.jpg", "art_square": "https://example.com/sq.jpg",
                 "extra": {"about": {"shortDescription": "A game in space."}, "genres": ["Action"], "releaseDate": "2020-01-02T00:00:00.000Z"}},
                {"app_name": "SpaceDLC", "title": "Space Game: Expansion"},
                {"app_name": "Engine", "title": "Unreal Engine"},
                {"app_name": "NoTitle"},
            ]}),
        );
        write(root, "legendaryConfig/legendary/metadata/SpaceDLC.json", json!({"metadata": {"mainGameItemList": [{"id": "x"}]}}));
        write(root, "legendaryConfig/legendary/metadata/Engine.json", json!({"metadata": {"categories": [{"path": "engines"}]}}));
        write(
            root,
            "store_cache/gog_library.json",
            json!({"games": [
                {"app_name": "1207658924", "title": "The Witcher: Enhanced Edition", "is_installed": false},
                {"app_name": "gog-redist", "title": "GOG Redist"},
                {"app_name": "1", "title": "Some Game Demo"},
                {"app_name": "2", "title": "Some DLC", "install": {"is_dlc": true}},
            ]}),
        );
        write(root, "store_cache/nile_library.json", json!({"library": [{"app_name": "amzn1", "title": "Prime Game"}]}));

        let libs = read_libraries(root).unwrap();
        assert!(libs.unread.is_empty());
        let by: std::collections::HashMap<&str, &Vec<ImportedGame>> = libs.stores.iter().map(|(s, g)| (s.as_str(), g)).collect();

        let epic = by["epic"];
        assert_eq!(epic.len(), 1, "DLC, engines and untitled entries are skipped");
        let g = &epic[0];
        assert_eq!((g.name.as_str(), g.source_id.as_str(), g.installed), ("Space Game", "Fortnite", true));
        assert_eq!(g.art_cover.as_deref(), Some("https://cdn1.epicgames.com/cover.jpg?resize=1&w=640"), "Epic art is resized");
        assert_eq!(g.art_square.as_deref(), Some("https://example.com/sq.jpg"), "other hosts are left alone");
        assert_eq!(g.description.as_deref(), Some("A game in space."));
        assert_eq!((g.genres.clone(), g.release_date.as_deref()), (vec!["Action".to_string()], Some("2020-01-02")));
        assert_eq!(g.run_url.as_deref(), Some("heroic://launch/legendary/Fortnite"));
        assert_eq!(g.store_url, None);

        let gog = by["gog"];
        assert_eq!(gog.len(), 1, "redistributables, demos and DLC are skipped");
        assert_eq!(gog[0].store_url.as_deref(), Some("https://www.gog.com/en/games?query=The+Witcher%3A+Enhanced+Edition"));

        assert_eq!(by["amazon"][0].store_url.as_deref(), Some("https://gaming.amazon.com/home"));
    }

    #[test]
    fn missing_caches_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "store_cache/nile_library.json", json!({"library": []}));
        let libs = read_libraries(dir.path()).unwrap();
        assert_eq!(libs.stores.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>(), ["amazon"]);
    }

    /// The code-health review's B-STORES-7: a store cache Heroic is halfway through
    /// writing (or that's damaged) is left out with why, and the other
    /// stores' caches are still read; only when none can be is it an error.
    #[test]
    fn one_damaged_cache_leaves_the_other_stores() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("store_cache")).unwrap();
        std::fs::write(dir.path().join("store_cache/legendary_library.json"), "{\"library\": [").unwrap();
        write(dir.path(), "store_cache/gog_library.json", json!({"games": [{"app_name": "1", "title": "Good Game"}]}));
        let libs = read_libraries(dir.path()).unwrap();
        assert_eq!(libs.stores.iter().map(|(s, g)| (s.as_str(), g.len())).collect::<Vec<_>>(), [("gog", 1)]);
        assert_eq!(libs.unread.len(), 1);
        assert!(libs.unread[0].contains("legendary_library.json"), "{:?}", libs.unread);

        std::fs::write(dir.path().join("store_cache/gog_library.json"), "").unwrap();
        let err = read_libraries(dir.path()).unwrap_err();
        assert!(format!("{err:#}").contains("legendary_library.json") && format!("{err:#}").contains("gog_library.json"), "{err:#}");
    }

    #[test]
    fn demo_titles() {
        for t in ["Foo Demo", "Foo DEMO", "Foo (Demo)", "Foo - Demo", "Foo: Demo"] {
            assert!(is_demo_title(t), "{t}");
        }
        assert!(!is_demo_title("Demolition Crew"));
    }

    /// Heroic's Windows folder is `%APPDATA%\heroic` (Electron's userData).
    #[cfg(windows)]
    #[test]
    fn heroic_on_windows_is_in_roaming_app_data() {
        let appdata = PathBuf::from(std::env::var_os("APPDATA").expect("APPDATA is set"));
        assert!(machine_config_dirs().contains(&appdata.join("heroic")));
    }
}
