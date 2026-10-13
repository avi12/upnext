//! Running inside a Flatpak (`contrib/flatpak/`). The
//! sandbox shows UpNext the home folder at its real path but lets it write
//! only to the app's own folder, `~/.var/app/<id>`, where Flatpak also points
//! the XDG folders, so UpNext's data, settings and cache land there with no
//! change. A few things need to know: Claude Code, which keeps its program
//! and sign-in under the home folder, is given the app's folder as its home
//! instead; "Add to Steam" can't write Steam's files, which the sandbox
//! shows read-only, so it says how to add UpNext in Steam instead; and
//! setup says where UpNext runs and what it can see.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The Flatpak UpNext runs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flatpak {
    /// Its app id: `io.github.zytocc.UpNext` for the project's own.
    pub id: String,
    /// The app's own folder, `~/.var/app/<id>`: the one place under the home
    /// folder the sandbox can write to, and Claude Code's home here.
    pub home: PathBuf,
}

impl Flatpak {
    /// The command another program (Steam, a launcher) runs to open the app
    /// window: `flatpak run <id>`, since the manifest's command is
    /// `upnext-app`.
    pub fn run_command(&self) -> (PathBuf, String) {
        (PathBuf::from("/usr/bin/flatpak"), format!("run {}", self.id))
    }

    /// The project's own Flatpak for a made-up user, for other modules' tests.
    #[cfg(test)]
    pub fn example() -> Flatpak {
        Flatpak { id: "io.github.zytocc.UpNext".into(), home: "/home/you/.var/app/io.github.zytocc.UpNext".into() }
    }
}

/// The Flatpak this process runs in, or None outside one. Tests never see
/// one, wherever they run, as they never see the machine's Steam.
pub fn get() -> Option<&'static Flatpak> {
    static FOUND: OnceLock<Option<Flatpak>> = OnceLock::new();
    FOUND.get_or_init(|| if cfg!(test) { None } else { detect(|k| std::env::var_os(k), Path::new("/.flatpak-info")) }).as_ref()
}

/// Inside a Flatpak, `flatpak run` sets `FLATPAK_ID` and puts the sandbox's
/// description at `/.flatpak-info` (`info`); both are needed, so a variable
/// left in the environment of a program started from a Flatpak app's
/// terminal isn't taken for one.
fn detect(var: impl Fn(&str) -> Option<OsString>, info: &Path) -> Option<Flatpak> {
    let id = var("FLATPAK_ID")?.into_string().ok()?;
    if id.is_empty() || id.contains(['/', '\\']) || id.starts_with('.') || !info.is_file() {
        return None;
    }
    let home = PathBuf::from(var("HOME").filter(|h| !h.is_empty())?);
    Some(Flatpak { home: home.join(".var").join("app").join(&id), id })
}

/// Set on the program started again by `restart_without_unreadable_cert_env`,
/// naming what it dropped, so the new one logs it and never restarts twice.
const DROPPED: &str = "UPNEXT_DROPPED_CERT_ENV";

/// The certificate variables the sandbox can't follow, with their values.
/// Flatpak passes the host's environment through, so an `SSL_CERT_FILE` or
/// `SSL_CERT_DIR` set on the host (for a company's proxy, say) can name a
/// file or folder the sandbox doesn't show. rustls then reads only that,
/// finds no certificates and refuses to make any HTTPS client ("No CA
/// certificates were loaded from the system"), which stops `upnext serve`.
/// Without the variable it reads the runtime's own certificates. A file
/// that can be opened, or a list of folders of which one can be read, is
/// kept, since rustls reads what it can there. Only the restart, which is
/// for Unix, uses it outside the tests.
#[cfg_attr(not(unix), allow(dead_code))]
fn unreadable_cert_env(var: impl Fn(&str) -> Option<OsString>) -> Vec<(&'static str, OsString)> {
    let mut dropped = vec![];
    if let Some(file) = var("SSL_CERT_FILE") {
        let path = Path::new(&file);
        if !(path.is_file() && std::fs::File::open(path).is_ok()) {
            dropped.push(("SSL_CERT_FILE", file));
        }
    }
    if let Some(dirs) = var("SSL_CERT_DIR")
        && !std::env::split_paths(&dirs).any(|d| !d.as_os_str().is_empty() && std::fs::read_dir(&d).is_ok())
    {
        dropped.push(("SSL_CERT_DIR", dirs));
    }
    dropped
}

/// Whether every certificate variable the marker names is still unset. The
/// marker passes on to programs UpNext starts, and to a terminal opened
/// from it, where the variable may be set again or nothing was dropped, and
/// then there's nothing to say.
fn still_dropped(marker: &str, var: impl Fn(&str) -> Option<OsString>) -> bool {
    let named: Vec<&str> = ["SSL_CERT_FILE", "SSL_CERT_DIR"]
        .into_iter()
        .filter(|k| marker.starts_with(&format!("{k}=")) || marker.contains(&format!(", {k}=")))
        .collect();
    !named.is_empty() && named.iter().all(|k| var(k).is_none())
}

/// "SSL_CERT_FILE=/etc/company/ca.pem", for the log. Unix only outside the
/// tests, like `unreadable_cert_env`.
#[cfg_attr(not(unix), allow(dead_code))]
fn describe(dropped: &[(&str, OsString)]) -> String {
    dropped.iter().map(|(k, v)| format!("{k}={}", v.to_string_lossy())).collect::<Vec<_>>().join(", ")
}

/// Inside a Flatpak, starts this program again without the certificate
/// variables the sandbox can't follow (`unreadable_cert_env`). A running
/// program can't remove its own environment variables without `unsafe`, so
/// it runs itself again with them removed; called first thing, before any
/// thread or client exists. Returns only when nothing needs removing, or
/// when starting again failed, and then the HTTPS clients fail as before.
#[cfg(unix)]
pub fn restart_without_unreadable_cert_env() {
    use std::os::unix::process::CommandExt;
    if get().is_none() || std::env::var_os(DROPPED).is_some() {
        return;
    }
    let dropped = unreadable_cert_env(|k| std::env::var_os(k));
    if dropped.is_empty() {
        return;
    }
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => return eprintln!("UpNext couldn't find its own program to start again without {}: {e}", describe(&dropped)),
    };
    let mut args = std::env::args_os();
    let mut again = std::process::Command::new(exe);
    if let Some(name) = args.next() {
        again.arg0(name);
    }
    again.args(args).env(DROPPED, describe(&dropped));
    for (k, _) in &dropped {
        again.env_remove(k);
    }
    let e = again.exec();
    eprintln!("UpNext couldn't start again without {}: {e}", describe(&dropped));
}

/// Nothing to do outside Unix, where there are no Flatpaks.
#[cfg(not(unix))]
pub fn restart_without_unreadable_cert_env() {}

/// Logs what `restart_without_unreadable_cert_env` dropped, once logging is
/// up, with the way to share the file with the sandbox instead.
pub fn log_dropped_cert_env() {
    let (Some(dropped), Some(sandbox)) = (std::env::var_os(DROPPED), get()) else { return };
    if !still_dropped(&dropped.to_string_lossy(), |k| std::env::var_os(k)) {
        return;
    }
    tracing::warn!(
        "{} can't be read inside the Flatpak, so UpNext uses the runtime's own certificates instead. To use it, share it with UpNext: flatpak override --user --filesystem=<its folder>:ro {}",
        dropped.to_string_lossy(),
        sandbox.id
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
        move |k| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| OsString::from(v))
    }

    #[test]
    fn a_flatpak_needs_its_id_and_its_info_file() {
        let dir = tempfile::tempdir().unwrap();
        let info = dir.path().join(".flatpak-info");
        let vars = env(&[("FLATPAK_ID", "io.github.zytocc.UpNext"), ("HOME", "/home/you")]);
        assert_eq!(detect(&vars, &info), None, "no info file: not a sandbox");
        std::fs::write(&info, "[Application]\nname=io.github.zytocc.UpNext\n").unwrap();
        let found = detect(&vars, &info).unwrap();
        assert_eq!(found.id, "io.github.zytocc.UpNext");
        assert_eq!(found.home, Path::new("/home/you/.var/app/io.github.zytocc.UpNext"));
        assert_eq!(found.run_command(), (PathBuf::from("/usr/bin/flatpak"), "run io.github.zytocc.UpNext".to_string()));

        assert_eq!(detect(env(&[("HOME", "/home/you")]), &info), None, "no id");
        assert_eq!(detect(env(&[("FLATPAK_ID", ""), ("HOME", "/home/you")]), &info), None);
        assert_eq!(detect(env(&[("FLATPAK_ID", "../x"), ("HOME", "/home/you")]), &info), None, "an id is never a path");
        assert_eq!(detect(env(&[("FLATPAK_ID", "io.github.zytocc.UpNext")]), &info), None, "no home folder");
    }

    /// A host's certificate file or folder the sandbox doesn't show is
    /// dropped; one it can read is kept.
    #[test]
    fn certificate_variables_the_sandbox_cant_read_are_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ca.pem");
        std::fs::write(&file, "-----BEGIN CERTIFICATE-----\n").unwrap();
        let (file, folder) = (file.into_os_string(), dir.path().as_os_str().to_owned());
        let missing = OsString::from("/home/you/company/ca.pem");
        let vars = |pairs: Vec<(&'static str, OsString)>| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone());
        let dropped = |pairs| unreadable_cert_env(vars(pairs));

        assert_eq!(dropped(vec![]), [], "nothing set, nothing dropped");
        assert_eq!(dropped(vec![("SSL_CERT_FILE", file.clone())]), []);
        assert_eq!(dropped(vec![("SSL_CERT_FILE", missing.clone())]), [("SSL_CERT_FILE", missing.clone())]);
        assert_eq!(dropped(vec![("SSL_CERT_FILE", "".into())]), [("SSL_CERT_FILE", OsString::new())]);
        assert_eq!(dropped(vec![("SSL_CERT_FILE", folder.clone())]), [("SSL_CERT_FILE", folder.clone())], "a folder isn't a file");

        let some_readable = std::env::join_paths([Path::new("/home/you/company/certs"), Path::new(&folder)]).unwrap();
        assert_eq!(dropped(vec![("SSL_CERT_DIR", some_readable)]), [], "rustls reads the folders it can");
        let none_readable = OsString::from("/home/you/company/certs:/home/you/more-certs");
        assert_eq!(dropped(vec![("SSL_CERT_DIR", none_readable.clone())]), [("SSL_CERT_DIR", none_readable.clone())]);
        assert_eq!(
            dropped(vec![("SSL_CERT_FILE", missing.clone()), ("SSL_CERT_DIR", none_readable.clone())]),
            [("SSL_CERT_FILE", missing.clone()), ("SSL_CERT_DIR", none_readable.clone())]
        );
        assert_eq!(
            describe(&[("SSL_CERT_FILE", missing), ("SSL_CERT_DIR", none_readable)]),
            "SSL_CERT_FILE=/home/you/company/ca.pem, SSL_CERT_DIR=/home/you/company/certs:/home/you/more-certs"
        );
    }

    /// The warning is logged only while what the marker names is still unset.
    #[test]
    fn the_drop_is_logged_only_while_the_variable_is_gone() {
        let set = |names: &'static [&'static str]| move |k: &str| names.contains(&k).then(|| OsString::from("/etc/ssl/ca.pem"));
        let both = "SSL_CERT_FILE=/home/you/ca.pem, SSL_CERT_DIR=/home/you/certs";
        assert!(still_dropped(both, set(&[])));
        assert!(!still_dropped(both, set(&["SSL_CERT_DIR"])), "set again");
        assert!(still_dropped("SSL_CERT_FILE=/home/you/ca.pem", set(&["SSL_CERT_DIR"])), "another variable doesn't count");
        assert!(!still_dropped("SSL_CERT_FILE=/home/you/ca.pem", set(&["SSL_CERT_FILE"])));
        assert!(!still_dropped("", set(&[])), "a marker naming nothing");
        assert!(!still_dropped("something else", set(&[])));
    }

    #[test]
    fn tests_never_run_in_a_flatpak() {
        assert_eq!(get(), None);
    }
}
