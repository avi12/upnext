//! Ubisoft Connect's installed games, on Windows. Ubisoft has no
//! sign-in a program like UpNext may use, so its owned library can't be
//! read; what is installed can. Ubisoft Connect keeps each install under
//! `HKLM\SOFTWARE\Ubisoft\Launcher\Installs\<game id>` with its folder, and
//! names it in Windows' Apps list as `Uplay Install <game id>`.

use crate::db::ImportedGame;
use crate::installs::{UninstallEntry, registry_path};

pub const SOURCE: &str = "uplay";
/// Where Ubisoft Connect lists its installs, under HKLM.
pub const INSTALLS_KEY: &str = r"SOFTWARE\Ubisoft\Launcher\Installs";

/// One game Ubisoft Connect installed.
#[derive(Debug, Clone, PartialEq)]
pub struct UbisoftInstall {
    /// Ubisoft's game id, which its launch links take.
    pub id: String,
    pub name: String,
}

/// The games among Ubisoft Connect's install keys, `(game id, InstallDir)`,
/// named from the Apps list, else by their folder. A key whose folder is
/// gone is left out: Ubisoft Connect doesn't always remove it on uninstall.
pub fn installed(installs: &[(String, String)], entries: &[UninstallEntry]) -> Vec<UbisoftInstall> {
    let mut out: Vec<UbisoftInstall> = vec![];
    for (id, dir) in installs {
        let id = id.trim();
        let Some(folder) = registry_path(dir) else { continue };
        if id.is_empty() || !folder.is_dir() || out.iter().any(|g| g.id == id) {
            continue;
        }
        let listed =
            entries.iter().find(|e| e.key.eq_ignore_ascii_case(&format!("Uplay Install {id}"))).and_then(|e| e.display_name.clone());
        let Some(name) = listed.or_else(|| folder.file_name().and_then(|f| f.to_str()).map(String::from)) else { continue };
        out.push(UbisoftInstall { id: id.to_string(), name });
    }
    out
}

/// Ubisoft Connect's link that starts a game.
pub fn run_url(id: &str) -> String {
    format!("uplay://launch/{id}/0")
}

pub fn copy(g: &UbisoftInstall) -> ImportedGame {
    ImportedGame {
        source: SOURCE.into(),
        source_id: g.id.clone(),
        launcher: SOURCE.into(),
        name: g.name.clone(),
        installed: true,
        run_url: Some(run_url(&g.id)),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_are_named_from_the_apps_list_or_their_folder() {
        let dir = tempfile::tempdir().unwrap();
        for f in ["Far Cry 5", "Anno 1800"] {
            std::fs::create_dir_all(dir.path().join(f)).unwrap();
        }
        // Ubisoft Connect writes its folders with forward slashes.
        let at = |f: &str| format!("{}/{f}/", dir.path().display());
        let installs = [
            ("635".to_string(), at("Far Cry 5")),
            ("4553".to_string(), at("Anno 1800")),
            // Uninstalled, and the key left behind.
            ("720".to_string(), at("Gone")),
            ("".to_string(), at("Far Cry 5")),
            ("635".to_string(), at("Far Cry 5")),
        ];
        let entries = [UninstallEntry { key: "Uplay Install 635".into(), display_name: Some("Far Cry® 5".into()), ..Default::default() }];
        let got = installed(&installs, &entries);
        assert_eq!(
            got,
            [
                UbisoftInstall { id: "635".into(), name: "Far Cry® 5".into() },
                UbisoftInstall { id: "4553".into(), name: "Anno 1800".into() },
            ]
        );
        let c = copy(&got[0]);
        assert_eq!((c.source.as_str(), c.launcher.as_str(), c.run_url.as_deref()), ("uplay", "uplay", Some("uplay://launch/635/0")));
    }
}
