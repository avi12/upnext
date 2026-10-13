//! Gamepad layouts the app window adds for WebKitGTK on Linux.
//!
//! WebKitGTK reads gamepads through libmanette, which knows a pad's buttons
//! and axes by its SDL GUID. A pad whose GUID it has no line for gets a
//! guessed layout, and for an Xbox controller through Microsoft's wireless
//! adapter (the `xone` driver, which puts it on the virtual bus) the guess
//! moves the triggers onto stick axes and loses the D-pad. libmanette also
//! reads lines from the user's own file, `libmanette/gamecontrollerdb` in the
//! config folder, so the window adds the lines it knows are missing there
//! before it opens, leaving every line already in the file as it was. Each
//! added line comes after a `# Added by UpNext` comment, so it can be found
//! and removed.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The lines added, in SDL's `gamecontrollerdb` format: SDL's layout for the
/// Xbox Wireless Controller, under the GUID `xone` gives it (bus 6). A
/// device test found that GUID with the wrong layout, and the same Xbox
/// layout under it fixed every button there.
const LAYOUTS: &[&str] = &[
    "060000005e040000ea02000001030000,Xbox Wireless Controller (xone),a:b0,b:b1,back:b6,dpdown:h0.4,dpleft:h0.8,dpright:h0.2,dpup:h0.1,guide:b8,leftshoulder:b4,leftstick:b9,lefttrigger:a2,leftx:a0,lefty:a1,rightshoulder:b5,rightstick:b10,righttrigger:a5,rightx:a3,righty:a4,start:b7,x:b2,y:b3,platform:Linux,",
];

/// The comment before each line UpNext adds.
const MARK: &str = "# Added by UpNext for its app window; delete this line and the next to remove it.";

/// The GUID a line is for, in lower case; none for comments and blank lines.
fn guid(line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    line.split(',').next().map(str::to_ascii_lowercase)
}

/// `file` with `LAYOUTS`' lines for the GUIDs it has no line for added at
/// the end, each after `MARK`, or `None` when it has them all. A layout already there, the
/// person's own included, is never replaced.
pub(crate) fn with_layouts(file: &str) -> Option<String> {
    let known: Vec<String> = file.lines().filter_map(guid).collect();
    let missing: Vec<&str> = LAYOUTS.iter().copied().filter(|l| guid(l).is_some_and(|g| !known.contains(&g))).collect();
    if missing.is_empty() {
        return None;
    }
    let mut out = file.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    for line in missing {
        out.push_str(MARK);
        out.push('\n');
        out.push_str(line);
        out.push('\n');
    }
    Some(out)
}

/// How long a window waits for another that is adding the layouts, before
/// it opens without them rather than hold up the window.
const LOCK_WAIT: Duration = Duration::from_secs(2);

/// The file to write for `link`, the path libmanette reads: the path itself,
/// or the file it points at through any links, so the links stay. A link to
/// a file not there yet names the file to create; one into a folder that
/// isn't there is an error, since writing at `link` would replace the link.
fn target(link: &Path) -> std::io::Result<PathBuf> {
    let mut path = link.to_path_buf();
    // As the kernel does, give up on a loop after 40 links.
    for _ in 0..40 {
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_symlink() => {
                let to = std::fs::read_link(&path)?;
                path = path.parent().map_or(to.clone(), |dir| dir.join(&to));
            }
            Ok(_) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return match path.parent() {
                    Some(dir) if dir.is_dir() => Ok(path),
                    _ => Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("{} links to {}, in a folder that isn't there", link.display(), path.display()),
                    )),
                };
            }
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other(format!("{} is a loop of links", link.display())))
}

/// Add the missing layouts to the file in `config` (the config folder),
/// waiting at most `wait` for another window adding them.
/// The file is the person's, so it's never left half written: the new one
/// is written beside it, with the old one's permissions, and renamed over
/// it. When the file is a link (a dotfiles folder's, say), the file it
/// points at is the one replaced, so the link stays. Two windows opening at
/// once take turns, by a lock on the folder, so neither writes over what
/// the other added.
fn add_to(config: &Path, wait: Duration) -> std::io::Result<bool> {
    let dir = config.join("libmanette");
    std::fs::create_dir_all(&dir)?;
    let path = target(&dir.join("gamecontrollerdb"))?;
    let folder = path.parent().unwrap_or(&dir);
    // flock on the folder itself, which leaves no lock file behind.
    let lock = std::fs::File::open(folder)?;
    let until = Instant::now() + wait;
    loop {
        match lock.try_lock() {
            Ok(()) => break,
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < until => std::thread::sleep(Duration::from_millis(50)),
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "another program held the folder's lock"));
            }
            Err(std::fs::TryLockError::Error(e)) => return Err(e),
        }
    }
    let (file, permissions) = match std::fs::File::open(&path) {
        Ok(mut f) => {
            let mut text = String::new();
            std::io::Read::read_to_string(&mut f, &mut text)?;
            (text, Some(f.metadata()?.permissions()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (String::new(), None),
        Err(e) => return Err(e),
    };
    let Some(new) = with_layouts(&file) else { return Ok(false) };
    let temp = folder.join(format!(".gamecontrollerdb.upnext-{}", std::process::id()));
    // One left by a window that stopped part way, or anything else there,
    // is taken away first, and the new one must be new: never a link
    // followed to somewhere else.
    let _ = std::fs::remove_file(&temp);
    let written = (|| {
        let mut out = std::fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
        if let Some(permissions) = permissions {
            out.set_permissions(permissions)?;
        }
        std::io::Write::write_all(&mut out, new.as_bytes())?;
        out.sync_all()?;
        std::fs::rename(&temp, &path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written.map(|()| true)
}

/// Add the missing layouts to the user's file, where libmanette looks
/// (GLib's config folder). A failure only costs the layouts, so it's logged.
pub(crate) fn add() {
    let config = glib::user_config_dir();
    match add_to(&config, LOCK_WAIT) {
        Ok(true) => tracing::info!("added gamepad layouts to {}", config.join("libmanette/gamecontrollerdb").display()),
        Ok(false) => {}
        Err(e) => tracing::warn!("couldn't add gamepad layouts for WebKitGTK: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_missing_layouts_are_added_and_existing_lines_kept() {
        let new = with_layouts("").unwrap();
        assert_eq!(new, format!("{MARK}\n{}\n", LAYOUTS[0]));
        assert!(new.contains("lefttrigger:a2") && new.contains("dpup:h0.1"));
        // Once there, nothing more.
        assert_eq!(with_layouts(&new), None);

        // The person's own lines stay, and one for the same pad, in any case, wins.
        let own = "# mine\n03000000de2800000512000011010000,Steam Deck,a:b3,\n";
        let added = with_layouts(own).unwrap();
        assert!(added.starts_with(own), "{added}");
        assert!(added.ends_with(&format!("{MARK}\n{}\n", LAYOUTS[0])));
        let theirs = "060000005E040000EA02000001030000,My layout,a:b1,";
        assert_eq!(with_layouts(theirs), None);
        // A file without a last line break gets one before the new line.
        assert_eq!(with_layouts("03000000de2800000512000011010000,Steam Deck,a:b3,").unwrap().lines().count(), 3);
    }

    #[test]
    fn the_file_is_written_in_the_config_folder_once() {
        let config = tempfile::tempdir().unwrap();
        assert!(add_to(config.path(), LOCK_WAIT).unwrap());
        let path = config.path().join("libmanette/gamecontrollerdb");
        let first = std::fs::read_to_string(&path).unwrap();
        assert!(first.contains("060000005e040000ea02000001030000"));
        assert!(!add_to(config.path(), LOCK_WAIT).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), first);
        // Nothing is left beside it.
        let names: Vec<_> = std::fs::read_dir(config.path().join("libmanette")).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["gamecontrollerdb"]);
    }

    /// The added line can be found by its comment, and taking both out
    /// leaves the person's file as it was.
    #[test]
    fn the_added_line_is_marked_for_removal() {
        let own = "# mine\n03000000de2800000512000011010000,Steam Deck,a:b3,\n";
        let added = with_layouts(own).unwrap();
        let mut lines = added.lines();
        let mut kept = String::new();
        while let Some(line) = lines.next() {
            if line == MARK {
                assert_eq!(lines.next(), Some(LAYOUTS[0]));
            } else {
                kept.push_str(line);
                kept.push('\n');
            }
        }
        assert_eq!(kept, own);
    }

    /// A file that's a link keeps being one: the file it points at gets the line.
    #[test]
    fn a_linked_file_stays_linked() {
        let config = tempfile::tempdir().unwrap();
        let dotfiles = tempfile::tempdir().unwrap();
        let real = dotfiles.path().join("gamecontrollerdb");
        std::fs::write(&real, "# mine\n").unwrap();
        std::fs::create_dir_all(config.path().join("libmanette")).unwrap();
        let link = config.path().join("libmanette/gamecontrollerdb");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(add_to(config.path(), LOCK_WAIT).unwrap());
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert!(std::fs::read_to_string(&real).unwrap().starts_with("# mine\n# Added by UpNext"));
    }

    /// Windows opening together each wait for the lock, so the line is added once.
    #[test]
    fn windows_opening_together_add_the_line_once() {
        let config = tempfile::tempdir().unwrap();
        let added: usize = std::thread::scope(|s| {
            let runs: Vec<_> = (0..8).map(|_| s.spawn(|| add_to(config.path(), Duration::from_secs(30)).unwrap())).collect();
            runs.into_iter().map(|r| usize::from(r.join().unwrap())).sum()
        });
        assert_eq!(added, 1);
        let file = std::fs::read_to_string(config.path().join("libmanette/gamecontrollerdb")).unwrap();
        assert_eq!(file.matches(LAYOUTS[0]).count(), 1);
    }

    /// The file keeps its permissions: one only its owner may read stays so.
    #[test]
    fn the_file_keeps_its_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let config = tempfile::tempdir().unwrap();
        let path = config.path().join("libmanette/gamecontrollerdb");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "# mine\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(add_to(config.path(), LOCK_WAIT).unwrap());
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }

    /// A link to a file not made yet gets that file, and stays a link; one
    /// into a folder that isn't there is left alone.
    #[test]
    fn a_link_to_nothing_yet_stays_a_link() {
        let config = tempfile::tempdir().unwrap();
        let dotfiles = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(config.path().join("libmanette")).unwrap();
        let link = config.path().join("libmanette/gamecontrollerdb");
        let real = dotfiles.path().join("gamecontrollerdb");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(add_to(config.path(), LOCK_WAIT).unwrap());
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert!(std::fs::read_to_string(&real).unwrap().contains(LAYOUTS[0]));

        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(dotfiles.path().join("gone/gamecontrollerdb"), &link).unwrap();
        assert!(add_to(config.path(), LOCK_WAIT).is_err());
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    }

    /// Whatever is at the temporary file's name, a link included, is taken
    /// away rather than written through.
    #[test]
    fn a_link_at_the_temporary_name_isnt_followed() {
        let config = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let victim = elsewhere.path().join("victim");
        std::fs::write(&victim, "keep me\n").unwrap();
        std::fs::create_dir_all(config.path().join("libmanette")).unwrap();
        let temp = config.path().join(format!("libmanette/.gamecontrollerdb.upnext-{}", std::process::id()));
        std::os::unix::fs::symlink(&victim, &temp).unwrap();
        assert!(add_to(config.path(), LOCK_WAIT).unwrap());
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep me\n");
        assert!(std::fs::symlink_metadata(&temp).is_err());
        assert!(std::fs::read_to_string(config.path().join("libmanette/gamecontrollerdb")).unwrap().contains(LAYOUTS[0]));
    }

    /// A lock another program never lets go of costs the layouts, not the window.
    #[test]
    fn a_stuck_lock_is_given_up_on() {
        let config = tempfile::tempdir().unwrap();
        let folder = config.path().join("libmanette");
        std::fs::create_dir_all(&folder).unwrap();
        let held = std::fs::File::open(&folder).unwrap();
        held.lock().unwrap();
        let err = add_to(config.path(), Duration::from_millis(200)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        assert!(!folder.join("gamecontrollerdb").exists());
        held.unlock().unwrap();
        assert!(add_to(config.path(), LOCK_WAIT).unwrap());
    }
}
