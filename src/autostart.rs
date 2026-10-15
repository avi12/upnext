//! Start with Windows: the `UpNext` value under
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, which starts
//! `upnext-app.exe --background` at sign-in, so the server runs with only its
//! tray icon until the window is opened. Windows has no user services, so this
//! is its counterpart of the systemd user service on Linux.
//!
//! The `start_with_windows` setting decides it. `PUT /api/settings` writes or
//! deletes the value before it saves the setting, and the app sets it again
//! from the setting each time it starts its own server, so a value naming a
//! program that moved (an install in another folder) is put right. There is
//! one value for every copy of UpNext on the computer, so a copy changes it
//! only as [`change`] says: a second copy (a scratch or development build
//! with a data folder of its own) never takes the installed copy's value
//! just by starting. Elsewhere the setting is kept but does nothing.
//!
//! The Microsoft Store's copy (`msix`) can't use the Run value: Windows
//! keeps that copy's `HKCU` writes in a registry of the package's own, which
//! it doesn't read at sign-in. Its package declares a startup task, which
//! starts the same `upnext-app.exe --background`, and which the user turns
//! on or off in Windows' own settings (see [`in_package`]).

// Off Windows only the tests build the value; the app reads the flag.
#![cfg_attr(not(windows), allow(dead_code))]

#[cfg(windows)]
use anyhow::Context;
use std::path::{Path, PathBuf};

/// The Run key, under `HKCU`.
#[cfg(all(windows, not(test)))]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// The value's name, which Task Manager's Startup apps list shows beside the
/// program's own description.
pub(crate) const VALUE: &str = "UpNext";

/// What `upnext-app.exe` is started with at sign-in: the tray icon and the
/// server, and no window until one is asked for.
pub(crate) const BACKGROUND_FLAG: &str = "--background";

/// What `upnext-app` (and `upnext app`) take to open full screen in big
/// screen: the Start menu's big-screen entry and the
/// shortcut "Add to Steam" writes (`steam_shortcut`) both pass it.
pub(crate) const BIG_SCREEN_FLAG: &str = "--big-screen";

/// The program the value starts: `upnext-app.exe`, which has no console. It
/// is `exe` itself when that is `upnext-app.exe`, else the one beside it, as
/// the installer and `cargo build --bins` both put them (`upnext serve` and
/// `upnext app` run from `upnext.exe`).
pub(crate) fn app_exe(exe: &Path) -> PathBuf {
    let is_app = exe.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.eq_ignore_ascii_case("upnext-app.exe"));
    if is_app { exe.to_path_buf() } else { exe.with_file_name("upnext-app.exe") }
}

/// The value's data: the program, quoted since the install folder is under
/// the user's profile and a user name can have spaces, then the flag.
pub(crate) fn command(app: &Path) -> String {
    format!("\"{}\" {BACKGROUND_FLAG}", app.display())
}

/// Why the Run value is being set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Why {
    /// The user changed the setting, in Settings or the tray icon's menu.
    Asked,
    /// The app started its own server and makes the value follow the
    /// setting it found.
    Starting,
}

/// What to do with the Run value.
#[derive(Debug, PartialEq)]
pub(crate) enum Change {
    /// Write this copy's command.
    Write,
    Delete,
    Leave,
}

/// The program a Run value starts: the quoted path at its start, or its
/// first word when it isn't quoted.
fn program(value: &str) -> Option<&str> {
    let value = value.trim_start();
    let path = match value.strip_prefix('"') {
        Some(rest) => rest.split('"').next(),
        None => value.split_whitespace().next(),
    };
    path.filter(|p| !p.is_empty())
}

/// What to do with the value, `current` as it is now, so that it follows
/// `on` for the copy whose `upnext-app.exe` is `app`. `exists` says whether
/// a program is there.
///
/// A value naming another program that is still there belongs to another
/// copy of UpNext, often the installed one while a scratch build runs.
/// Starting never writes over it or deletes it, whatever this copy's own
/// setting says; turning the setting on is the user's choice and takes the
/// value over, and turning it off leaves the other copy's alone. A value
/// naming a program that is gone (an install that moved) is put right or
/// cleared either way.
pub(crate) fn change(on: bool, why: Why, current: Option<&str>, app: &Path, exists: impl Fn(&Path) -> bool) -> Change {
    let Some(current) = current else {
        return if on { Change::Write } else { Change::Leave };
    };
    let ours = app.to_string_lossy();
    let named = program(current);
    // Windows doesn't mind the case of a path.
    let another_copys = named.is_some_and(|p| !p.eq_ignore_ascii_case(&ours) && exists(Path::new(p)));
    match (on, why) {
        (true, _) if current == command(app) => Change::Leave,
        (true, Why::Starting) if another_copys => Change::Leave,
        (true, _) => Change::Write,
        (false, _) if another_copys => Change::Leave,
        (false, _) => Change::Delete,
    }
}

/// Make the Run value follow `on` for this program, as [`change`] says:
/// write it, naming the `upnext-app.exe` that goes with this program, delete
/// it, or leave it as it is. Doing any of them twice changes nothing.
#[cfg(windows)]
pub(crate) fn set(on: bool, why: Why) -> anyhow::Result<()> {
    if crate::msix::get().is_some() {
        return in_package(on, why);
    }
    let exe = std::env::current_exe().context("locating this program")?;
    let app = app_exe(&exe);
    let key = run_key();
    match change(on, why, read(&key).as_deref(), &app, Path::is_file) {
        Change::Leave => Ok(()),
        Change::Delete => write(&key, None).context("removing UpNext from the programs Windows starts at sign-in"),
        Change::Write => {
            if !app.is_file() {
                anyhow::bail!("Start with Windows starts upnext-app.exe, and there is none in {}", app.parent().unwrap_or(&app).display());
            }
            write(&key, Some(&command(&app))).context("adding UpNext to the programs Windows starts at sign-in")
        }
    }
}

/// What turning the setting on or off does in the Microsoft Store's copy,
/// whose Start with Windows is the package's startup task. UpNext can't turn
/// that on itself without the Windows Runtime, so turning the setting on
/// says where to do it; turning it off, or starting, changes nothing, so a
/// setting carried over from another copy's library can still be turned
/// off.
pub(crate) fn in_package(on: bool, why: Why) -> anyhow::Result<()> {
    if on && why == Why::Asked {
        anyhow::bail!(
            "this is UpNext from the Microsoft Store, which Windows starts at sign-in from its own list: turn UpNext on \
             in Windows Settings, Apps, Startup"
        );
    }
    Ok(())
}

/// Off Windows there is no Run key; the setting is only stored.
#[cfg(not(windows))]
pub(crate) fn set(_on: bool, _why: Why) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(all(windows, not(test)))]
fn run_key() -> String {
    RUN_KEY.to_string()
}

/// Tests write to a key of their own, never to the real Run key: the Windows
/// CI runner can be the developer's own PC.
#[cfg(all(windows, test))]
fn run_key() -> String {
    format!(r"Software\UpNext-test-{}\Run", std::process::id())
}

/// Write `command` as `key`'s `UpNext` value, or delete the value when it's
/// `None`. A value that is already right isn't written again.
#[cfg(windows)]
fn write(key: &str, command: Option<&str>) -> std::io::Result<()> {
    use std::io::ErrorKind::NotFound;
    use winreg::HKCU;
    use winreg::enums::{KEY_QUERY_VALUE, KEY_SET_VALUE};
    match command {
        Some(command) => {
            let (k, _) = HKCU.create_subkey(key)?;
            if k.get_value::<String, _>(VALUE).ok().as_deref() != Some(command) {
                k.set_value(VALUE, &command)?;
            }
            Ok(())
        }
        None => match HKCU.open_subkey_with_flags(key, KEY_QUERY_VALUE | KEY_SET_VALUE) {
            Ok(k) => match k.delete_value(VALUE) {
                Err(e) if e.kind() != NotFound => Err(e),
                _ => Ok(()),
            },
            Err(e) if e.kind() == NotFound => Ok(()),
            Err(e) => Err(e),
        },
    }
}

/// The `UpNext` value under `key`, if there is one.
#[cfg(windows)]
fn read(key: &str) -> Option<String> {
    winreg::HKCU.open_subkey(key).ok()?.get_value(VALUE).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_value_starts_the_app_in_the_background() {
        let installed = Path::new(r"C:\Users\Some One\AppData\Local\Programs\UpNext\upnext-app.exe");
        assert_eq!(command(installed), r#""C:\Users\Some One\AppData\Local\Programs\UpNext\upnext-app.exe" --background"#);
    }

    #[test]
    fn the_store_copy_sends_the_user_to_windows_settings() {
        let e = in_package(true, Why::Asked).unwrap_err().to_string();
        assert!(e.contains("Windows Settings, Apps, Startup"), "{e}");
        // Starting never fails over it, and the setting can still go off.
        assert!(in_package(true, Why::Starting).is_ok());
        assert!(in_package(false, Why::Asked).is_ok());
        assert!(in_package(false, Why::Starting).is_ok());
    }

    #[test]
    fn the_app_is_this_program_or_the_one_beside_it() {
        let folder = Path::new("programs").join("UpNext");
        assert_eq!(app_exe(&folder.join("upnext-app.exe")), folder.join("upnext-app.exe"));
        // Windows doesn't mind the case, and neither does the value.
        assert_eq!(app_exe(&folder.join("UpNext-App.EXE")), folder.join("UpNext-App.EXE"));
        assert_eq!(app_exe(&folder.join("upnext.exe")), folder.join("upnext-app.exe"));
    }

    #[test]
    fn the_program_is_the_quoted_path_or_the_first_word() {
        assert_eq!(program(r#""C:\Users\Some One\UpNext\upnext-app.exe" --background"#), Some(r"C:\Users\Some One\UpNext\upnext-app.exe"));
        assert_eq!(program(r"C:\UpNext\upnext-app.exe --background"), Some(r"C:\UpNext\upnext-app.exe"));
        assert_eq!(program(r#""C:\UpNext\upnext-app.exe""#), Some(r"C:\UpNext\upnext-app.exe"));
        assert_eq!(program(""), None);
        assert_eq!(program(r#""" --background"#), None);
    }

    /// The installed copy and a scratch build beside it, both there.
    const INSTALLED: &str = r"C:\Users\you\AppData\Local\Programs\UpNext\upnext-app.exe";
    const SCRATCH: &str = r"C:\upnext-scratch\target\release\upnext-app.exe";

    fn both_there(p: &Path) -> bool {
        [INSTALLED, SCRATCH].iter().any(|there| p.as_os_str().eq_ignore_ascii_case(there))
    }

    #[test]
    fn starting_a_second_copy_leaves_the_installed_copys_value() {
        let installed = command(Path::new(INSTALLED));
        let scratch = Path::new(SCRATCH);
        // The scratch copy's own setting may be either: a data folder of its
        // own says off, a copied one may say on.
        assert_eq!(change(false, Why::Starting, Some(&installed), scratch, both_there), Change::Leave);
        assert_eq!(change(true, Why::Starting, Some(&installed), scratch, both_there), Change::Leave);
    }

    #[test]
    fn starting_makes_this_copys_own_value_follow_the_setting() {
        let app = Path::new(INSTALLED);
        let ours = command(app);
        assert_eq!(change(true, Why::Starting, Some(&ours), app, both_there), Change::Leave);
        assert_eq!(change(true, Why::Starting, None, app, both_there), Change::Write);
        assert_eq!(change(false, Why::Starting, Some(&ours), app, both_there), Change::Delete);
        assert_eq!(change(false, Why::Starting, None, app, both_there), Change::Leave);
        // Written by hand, in other letters or without the flag: still ours,
        // and written again as it should be.
        let by_hand = format!("\"{}\"", INSTALLED.to_uppercase());
        assert_eq!(change(true, Why::Starting, Some(&by_hand), app, both_there), Change::Write);
        assert_eq!(change(false, Why::Starting, Some(&by_hand), app, both_there), Change::Delete);
    }

    #[test]
    fn a_value_naming_a_program_that_is_gone_is_put_right_or_cleared() {
        let moved = command(Path::new(r"D:\Old\UpNext\upnext-app.exe"));
        let app = Path::new(INSTALLED);
        for why in [Why::Asked, Why::Starting] {
            assert_eq!(change(true, why, Some(&moved), app, both_there), Change::Write);
            assert_eq!(change(false, why, Some(&moved), app, both_there), Change::Delete);
        }
        // A value that names no program at all is no copy's either.
        assert_eq!(change(true, Why::Starting, Some(""), app, both_there), Change::Write);
        assert_eq!(change(false, Why::Starting, Some(""), app, both_there), Change::Delete);
    }

    #[test]
    fn turning_it_on_takes_the_value_over_and_turning_it_off_leaves_another_copys() {
        let installed = command(Path::new(INSTALLED));
        let scratch = Path::new(SCRATCH);
        assert_eq!(change(true, Why::Asked, Some(&installed), scratch, both_there), Change::Write);
        assert_eq!(change(false, Why::Asked, Some(&installed), scratch, both_there), Change::Leave);
        let ours = command(scratch);
        assert_eq!(change(true, Why::Asked, Some(&ours), scratch, both_there), Change::Leave);
        assert_eq!(change(false, Why::Asked, Some(&ours), scratch, both_there), Change::Delete);
        assert_eq!(change(true, Why::Asked, None, scratch, both_there), Change::Write);
        assert_eq!(change(false, Why::Asked, None, scratch, both_there), Change::Leave);
    }

    #[cfg(not(windows))]
    #[test]
    fn off_windows_it_does_nothing() {
        for why in [Why::Asked, Why::Starting] {
            set(true, why).unwrap();
            set(false, why).unwrap();
        }
    }

    /// On and off, twice each, against a key of this test's own.
    #[cfg(windows)]
    #[test]
    fn the_run_value_follows_the_setting() {
        let key = format!(r"Software\UpNext-test-{}\Run", uuid::Uuid::new_v4().simple());
        let value = command(Path::new(r"C:\Programs\UpNext\upnext-app.exe"));
        // Nothing there yet: deleting is fine.
        write(&key, None).unwrap();
        assert_eq!(read(&key), None);
        write(&key, Some(&value)).unwrap();
        write(&key, Some(&value)).unwrap();
        assert_eq!(read(&key).as_deref(), Some(value.as_str()));
        // A program that moved is written over.
        let moved = command(Path::new(r"D:\UpNext\upnext-app.exe"));
        write(&key, Some(&moved)).unwrap();
        assert_eq!(read(&key).as_deref(), Some(moved.as_str()));
        write(&key, None).unwrap();
        write(&key, None).unwrap();
        assert_eq!(read(&key), None);
        winreg::HKCU.delete_subkey_all(key.trim_end_matches(r"\Run")).unwrap();
    }

    /// The test binary has no `upnext-app.exe` beside it, so turning it on
    /// says so and writes nothing; turning it off works.
    #[cfg(windows)]
    #[test]
    fn turning_it_on_needs_the_app_beside_this_program() {
        let err = set(true, Why::Asked).unwrap_err().to_string();
        assert!(err.contains("Start with Windows starts upnext-app.exe, and there is none in"), "{err}");
        set(false, Why::Asked).unwrap();
        assert_eq!(read(&run_key()), None);
    }
}
