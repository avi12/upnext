//! UpNext's Steam copy (`contrib/steam/`). Steam installs
//! it in a Steam library folder, lists it and starts it, so two things
//! UpNext does for a copy installed any other way are left out: "Add to
//! Steam" would give Steam a second entry for UpNext, and `install-desktop`
//! a launcher entry, both of which on Linux would run the program without
//! the launcher that sets up its bundled libraries. Steam has its own "Add
//! desktop shortcut" for that. The depot scripts put `steam-copy.txt`
//! beside the programs, which is how UpNext knows.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The marker file beside the programs in both depots
/// (`contrib/steam/bundle-linux.py` writes it).
pub const MARKER: &str = "steam-copy.txt";

/// The Steam copy this program is part of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamCopy {
    /// What Steam's launch options run to open the app window: the Linux
    /// depot's `upnext.sh`, a folder above the programs, which sets up the
    /// bundled libraries; on Windows `upnext-app.exe` beside them.
    pub launcher: PathBuf,
}

impl SteamCopy {
    /// A Steam copy installed in a made-up user's Steam library, for other
    /// modules' tests.
    #[cfg(test)]
    pub fn example() -> SteamCopy {
        SteamCopy { launcher: "/home/you/.local/share/Steam/steamapps/common/UpNext/upnext.sh".into() }
    }
}

/// The Steam copy this program is part of, or None for any other. Tests
/// never see one, wherever they run.
pub fn get() -> Option<&'static SteamCopy> {
    static FOUND: OnceLock<Option<SteamCopy>> = OnceLock::new();
    FOUND.get_or_init(|| if cfg!(test) { None } else { std::env::current_exe().ok().and_then(|exe| detect(&exe, cfg!(windows))) }).as_ref()
}

/// The Steam copy whose program is `exe`, by the marker beside it.
fn detect(exe: &Path, windows: bool) -> Option<SteamCopy> {
    let bin = exe.parent()?;
    if !bin.join(MARKER).is_file() {
        return None;
    }
    let launcher = if windows { bin.join("upnext-app.exe") } else { bin.parent()?.join("upnext.sh") };
    Some(SteamCopy { launcher })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_marker_beside_the_program_makes_it_the_steam_copy() {
        let depot = tempfile::tempdir().unwrap();
        let bin = depot.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let exe = bin.join("upnext-app");
        assert_eq!(detect(&exe, false), None, "no marker: installed some other way");

        std::fs::write(bin.join(MARKER), "installed by Steam").unwrap();
        assert_eq!(detect(&exe, false), Some(SteamCopy { launcher: depot.path().join("upnext.sh") }), "Linux: the launcher above bin/");
        let exe = bin.join("upnext-app.exe");
        assert_eq!(detect(&exe, true), Some(SteamCopy { launcher: bin.join("upnext-app.exe") }), "Windows: the app's program beside it");

        // A folder named like the marker isn't one.
        let other = tempfile::tempdir().unwrap();
        std::fs::create_dir(other.path().join(MARKER)).unwrap();
        assert_eq!(detect(&other.path().join("upnext"), false), None);
    }

    #[test]
    fn tests_never_run_as_the_steam_copy() {
        assert_eq!(get(), None);
    }
}
