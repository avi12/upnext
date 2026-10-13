//! The EA app's installed games, on Windows. EA has no sign-in a
//! program like UpNext may use, so its owned library can't be read; what is
//! installed can, from Windows' Apps list. EA's installer leaves two marks
//! there: an uninstall command run through `EAInstaller`, and the game's
//! own `__Installer\installerdata.xml`, which holds its title and content id.

use crate::db::ImportedGame;
use crate::installs::{UninstallEntry, registry_path};
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

pub const SOURCE: &str = "origin";

/// One game the EA app installed.
#[derive(Debug, Clone, PartialEq)]
pub struct EaInstall {
    /// EA's content id from the game's installer data, or the folder's
    /// name when there's none.
    pub id: String,
    pub name: String,
}

/// The EA app and Origin themselves, which their own Apps entries list
/// alongside the games.
fn is_the_launcher(name: &str) -> bool {
    let n = name.trim().to_lowercase();
    ["ea app", "ea desktop", "origin", "ea"].contains(&n.as_str())
}

/// The games among the Apps list's entries that EA's installer put there.
pub fn installed(entries: &[UninstallEntry]) -> Vec<EaInstall> {
    let mut out: Vec<EaInstall> = vec![];
    for e in entries {
        let Some(folder) = e.install_location.as_deref().and_then(registry_path) else { continue };
        let data = std::fs::read_to_string(folder.join("__Installer").join("installerdata.xml")).ok();
        let by_ea = e.uninstall_string.as_deref().is_some_and(|u| u.to_lowercase().contains("eainstaller"));
        if data.is_none() && !by_ea {
            continue;
        }
        let parsed = data.as_deref().map(installer_data).unwrap_or_default();
        let Some(name) = parsed.title.or_else(|| e.display_name.clone()) else { continue };
        if is_the_launcher(&name) {
            continue;
        }
        let id = parsed.content_id.unwrap_or_else(|| folder_id(&folder, &name));
        if !out.iter().any(|g| g.id == id) {
            out.push(EaInstall { id, name });
        }
    }
    out
}

/// A stable id for a game whose installer data has no content id: its
/// folder's name, else its title, lowercased.
fn folder_id(folder: &Path, name: &str) -> String {
    let base = folder.file_name().and_then(|f| f.to_str()).filter(|f| !f.is_empty()).unwrap_or(name);
    format!("dir-{}", base.to_lowercase().split_whitespace().collect::<Vec<_>>().join("-"))
}

#[derive(Debug, Default, PartialEq)]
struct InstallerData {
    title: Option<String>,
    content_id: Option<String>,
}

/// The English title and first content id from `installerdata.xml`. Newer
/// files have `<gameTitle locale="en_US">`, older ones a `<localeInfo
/// locale="en_US">` holding a `<title>`; both are matched with patterns, as
/// nothing else needs an XML parser.
fn installer_data(xml: &str) -> InstallerData {
    static GAME_TITLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?s)<gameTitle\b([^>]*)>\s*([^<]+?)\s*</gameTitle>"#).unwrap());
    static LOCALE_TITLE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"(?s)<localeInfo\b([^>]*)>.*?<title>\s*([^<]+?)\s*</title>"#).unwrap());
    static CONTENT_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<contentID>\s*([^<\s]+)\s*</contentID>").unwrap());
    let titles: Vec<(bool, String)> =
        GAME_TITLE.captures_iter(xml).chain(LOCALE_TITLE.captures_iter(xml)).map(|c| (c[1].contains("en_US"), unescape(&c[2]))).collect();
    let title = titles.iter().find(|(en, _)| *en).or(titles.first()).map(|(_, t)| t.clone());
    let content_id = CONTENT_ID.captures(xml).map(|c| c[1].to_string());
    InstallerData { title, content_id }
}

/// XML's escapes, numeric ones (`&#8482;`, ™) included; `&amp;` goes last
/// so an escaped escape stays as written.
fn unescape(s: &str) -> String {
    static NUMERIC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"&#(x[0-9a-fA-F]+|[0-9]+);").unwrap());
    let s = NUMERIC.replace_all(s, |c: &regex::Captures| {
        let n = match c[1].strip_prefix('x') {
            Some(hex) => u32::from_str_radix(hex, 16).ok(),
            None => c[1].parse().ok(),
        };
        n.and_then(char::from_u32).map(String::from).unwrap_or_else(|| c[0].to_string())
    });
    s.replace("&apos;", "'").replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

/// The library copy for an installed EA game. EA's launch links take offer
/// ids, which the installer data doesn't hold, so there's no `run_url`.
pub fn copy(g: &EaInstall) -> ImportedGame {
    ImportedGame {
        source: SOURCE.into(),
        source_id: g.id.clone(),
        launcher: SOURCE.into(),
        name: g.name.clone(),
        installed: true,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn game(root: &Path, folder: &str, xml: Option<&str>) -> PathBuf {
        let dir = root.join(folder);
        std::fs::create_dir_all(dir.join("__Installer")).unwrap();
        if let Some(xml) = xml {
            std::fs::write(dir.join("__Installer").join("installerdata.xml"), xml).unwrap();
        }
        dir
    }

    fn entry(key: &str, name: &str, folder: Option<PathBuf>, uninstall: Option<&str>) -> UninstallEntry {
        let folder = folder.map(|f| f.display().to_string());
        UninstallEntry {
            key: key.into(),
            display_name: Some(name.into()),
            install_location: folder,
            uninstall_string: uninstall.map(String::from),
            publisher: Some("Electronic Arts".into()),
        }
    }

    #[test]
    fn the_installer_data_names_the_game_in_either_layout() {
        let new = r#"<DiPManifest version="4.0"><gameTitles>
            <gameTitle locale="de_DE">Die Sims 4</gameTitle>
            <gameTitle locale="en_US">The Sims&#8482; 4 &amp; Friends</gameTitle>
          </gameTitles><contentIDs><contentID>71820</contentID><contentID>71821</contentID></contentIDs></DiPManifest>"#;
        assert_eq!(installer_data(new), InstallerData { title: Some("The Sims™ 4 & Friends".into()), content_id: Some("71820".into()) });
        let old = r#"<game><metadata><localeInfo locale="en_US"><title>Mass Effect</title></localeInfo></metadata>
            <contentIDs><contentID> 70377 </contentID></contentIDs></game>"#;
        assert_eq!(installer_data(old), InstallerData { title: Some("Mass Effect".into()), content_id: Some("70377".into()) });
        assert_eq!(installer_data("<nothing/>"), InstallerData::default());
    }

    #[test]
    fn eas_games_are_found_among_the_apps_and_nothing_else_is() {
        let dir = tempfile::tempdir().unwrap();
        let bf = game(
            dir.path(),
            "Battlefield 1",
            Some(r#"<gameTitle locale="en_US">Battlefield™ 1</gameTitle><contentID>1026023</contentID>"#),
        );
        let plain = game(dir.path(), "Unravel Two", None);
        let other = dir.path().join("Some Editor");
        let entries = [
            entry("{A1}", "Battlefield 1", Some(bf.clone()), None),
            // The 32-bit view lists it again.
            entry("{A1-32}", "Battlefield 1", Some(bf), None),
            entry(
                "{B2}",
                "Unravel Two",
                Some(plain),
                Some(r#""C:\Program Files\Common Files\EAInstaller\Unravel Two\Cleanup.exe" uninstall_game"#),
            ),
            entry("{C3}", "Some Editor", Some(other), Some(r#""C:\Program Files\Some Editor\uninstall.exe""#)),
            entry(
                "{D4}",
                "EA app",
                Some(dir.path().join("EA Desktop")),
                Some(r#""C:\Program Files\Electronic Arts\EA Desktop\EAInstaller.exe""#),
            ),
            entry("{E5}", "No folder", None, Some("EAInstaller")),
        ];
        let got = installed(&entries);
        assert_eq!(
            got,
            [
                EaInstall { id: "1026023".into(), name: "Battlefield™ 1".into() },
                EaInstall { id: "dir-unravel-two".into(), name: "Unravel Two".into() },
            ]
        );
        let c = copy(&got[0]);
        assert_eq!((c.source.as_str(), c.launcher.as_str(), c.installed, c.run_url), ("origin", "origin", true, None));
    }
}
