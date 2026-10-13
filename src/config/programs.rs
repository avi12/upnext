//! Where the AI CLIs and the programs they need run from: as set when the
//! setting is a path, else on `PATH`, else in the folders their installers
//! use. Configuration names the programs (`claude_bin`, `codex_bin`);
//! `Config::claude_program`, `process.rs` and the AI CLIs find them here.

use std::path::PathBuf;

/// How [`find_program`] found a program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    /// The setting is a path, and the file is there.
    AsSet,
    OnPath,
    /// Not on `PATH`, but in one of the folders installers put it in.
    Usual,
}

/// Where `name` runs from: itself when it's a path, else the first match
/// on `PATH`, else the folders the Claude and Codex installers use
/// ([`usual_folders`]). The path is as given or as found, not made canonical.
///
/// `PATH` is the one the process started with, so a program installed while
/// UpNext runs is found only in those folders.
pub fn find_program(name: &str) -> Option<(PathBuf, Found)> {
    if name.contains(['/', '\\']) {
        return std::path::Path::new(name).is_file().then(|| (PathBuf::from(name), Found::AsSet));
    }
    let on_path = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default();
    // In a Flatpak, `upnext install-claude` puts Claude Code in the app's own
    // folder, its home there (`flatpak`).
    let homes: Vec<PathBuf> = crate::flatpak::get()
        .map(|f| f.home.clone())
        .into_iter()
        .chain(directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()))
        .collect();
    let windows = if cfg!(windows) { windows_folders(|var| std::env::var_os(var)) } else { Vec::new() };
    find_in(name, &on_path, &usual_folders(&homes, windows), cfg!(windows))
}

/// The folders the Claude and Codex installers use, under each of `homes` in
/// turn (`~/.local/bin`, `~/.claude/local`, `~/.npm-global/bin`), then
/// `windows` (`windows_folders`) and `/usr/local/bin`.
fn usual_folders(homes: &[PathBuf], windows: Vec<PathBuf>) -> Vec<PathBuf> {
    homes
        .iter()
        .flat_map(|h| [h.join(".local").join("bin"), h.join(".claude").join("local"), h.join(".npm-global").join("bin")])
        .chain(windows)
        .chain([PathBuf::from("/usr/local/bin")])
        .collect()
}

/// The folders Windows installers put the AI CLIs and Node.js in, from the
/// environment (`var`): npm's global folder (`claude.cmd`, `codex.cmd`), the
/// Codex app's command line, and Node.js, whose `node.exe` npm's scripts
/// need. A program started outside a terminal may not have them on its
/// `PATH`, and one started before they were installed never does.
fn windows_folders(var: impl Fn(&str) -> Option<std::ffi::OsString>) -> Vec<PathBuf> {
    let under = |name: &str, rel: &[&str]| var(name).map(|v| rel.iter().fold(PathBuf::from(v), |p, r| p.join(r)));
    [
        under("APPDATA", &["npm"]),
        under("LOCALAPPDATA", &["Programs", "OpenAI", "Codex", "bin"]),
        under("ProgramFiles", &["nodejs"]),
        under("ProgramFiles(x86)", &["nodejs"]),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// [`find_program`]'s search through `on_path`, then `usual`. On Windows
/// (`windows`) a name without an extension is `name.exe`, else `name.cmd`
/// or `name.bat`, and every folder is searched for the `.exe` before any
/// `.cmd`: the native Claude Code install wins over an npm one wherever each
/// is on `PATH`. npm's extensionless `claude` beside its `claude.cmd` is a
/// shell script, which Windows can't run, so it's never the answer there.
fn find_in(name: &str, on_path: &[PathBuf], usual: &[PathBuf], windows: bool) -> Option<(PathBuf, Found)> {
    let names: Vec<String> = if windows && std::path::Path::new(name).extension().is_none() {
        ["exe", "cmd", "bat"].iter().map(|e| format!("{name}.{e}")).collect()
    } else {
        vec![name.into()]
    };
    for (dirs, how) in [(on_path, Found::OnPath), (usual, Found::Usual)] {
        for n in &names {
            if let Some(p) = dirs.iter().map(|d| d.join(n)).find(|p| p.is_file()) {
                return Some((p, how));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    /// npm's folder, the Codex app's and Node.js's, wherever Windows puts
    /// them for this user.
    #[test]
    fn windows_folders_come_from_the_environment() {
        let env = |name: &str| {
            Some(std::ffi::OsString::from(match name {
                "APPDATA" => "C:/Users/you/AppData/Roaming",
                "LOCALAPPDATA" => "C:/Users/you/AppData/Local",
                "ProgramFiles" => "C:/Program Files",
                _ => return None,
            }))
        };
        let folders = windows_folders(env);
        assert_eq!(
            folders,
            [
                PathBuf::from("C:/Users/you/AppData/Roaming").join("npm"),
                PathBuf::from("C:/Users/you/AppData/Local").join("Programs").join("OpenAI").join("Codex").join("bin"),
                PathBuf::from("C:/Program Files").join("nodejs"),
            ]
        );
        assert!(windows_folders(|_| None).is_empty());
    }

    #[test]
    fn programs_are_found_as_set_or_not_at_all() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("claude");
        std::fs::write(&bin, "").unwrap();
        let set = bin.display().to_string();
        assert_eq!(find_program(&set), Some((bin, Found::AsSet)));
        assert_eq!(find_program(&dir.path().join("codex").display().to_string()), None);
        assert_eq!(find_program("upnext-no-such-program"), None);
        let mut c = Config::built_in();
        c.claude_bin = "upnext-no-such-program".into();
        assert_eq!(c.claude_program(), "upnext-no-such-program", "left as set, so the error names it");
    }

    #[test]
    fn a_flatpaks_own_folder_is_looked_in_first() {
        let homes = [PathBuf::from("/home/you/.var/app/io.github.zytocc.UpNext"), PathBuf::from("/home/you")];
        let usual = usual_folders(&homes, vec![]);
        assert_eq!(usual[0], std::path::Path::new("/home/you/.var/app/io.github.zytocc.UpNext/.local/bin"));
        assert_eq!(usual[3], std::path::Path::new("/home/you/.local/bin"));
        assert_eq!(usual.last().unwrap(), std::path::Path::new("/usr/local/bin"));
    }

    /// Windows' order: an `.exe` anywhere on PATH before a `.cmd`, never
    /// npm's extensionless shell script; elsewhere the name as it is.
    #[test]
    fn programs_on_windows_prefer_the_exe() {
        let dir = tempfile::tempdir().unwrap();
        let (npm, native, usual) = (dir.path().join("npm"), dir.path().join("native"), dir.path().join("usual"));
        for d in [&npm, &native, &usual] {
            std::fs::create_dir_all(d).unwrap();
        }
        for f in [npm.join("claude"), npm.join("claude.cmd"), npm.join("claude.ps1"), usual.join("codex.exe")] {
            std::fs::write(f, "").unwrap();
        }
        let path = [npm.clone(), native.clone()];
        assert_eq!(find_in("claude", &path, &[], true), Some((npm.join("claude.cmd"), Found::OnPath)));
        std::fs::write(native.join("claude.exe"), "").unwrap();
        assert_eq!(find_in("claude", &path, &[], true), Some((native.join("claude.exe"), Found::OnPath)));
        assert_eq!(find_in("claude.cmd", &path, &[], true), Some((npm.join("claude.cmd"), Found::OnPath)));
        assert_eq!(find_in("codex", &path, std::slice::from_ref(&usual), true), Some((usual.join("codex.exe"), Found::Usual)));
        assert_eq!(find_in("claude", &path, &[], false), Some((npm.join("claude"), Found::OnPath)));
        assert_eq!(find_in("codex", &path, &[], false), None);
    }
}
