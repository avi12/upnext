//! Other users of the computer UpNext runs on. Steam and Heroic each keep
//! one sign-in per user, so a second player's own is usually in their own
//! home folder, which UpNext, running as the first player's user, never
//! looks in by itself. Setup lists such folders for a player to choose;
//! finding one only checks that the files are there, and nothing in it is
//! read until a player picks it.

use std::path::{Path, PathBuf};

/// Profiles Windows keeps beside the users' own, which belong to nobody.
const SHARED: [&str; 4] = ["Public", "Default", "Default User", "All Users"];

/// The home folders of this computer's other users: those in `users_dirs`
/// when it's given (as tests give it), else those beside this user's own
/// ([`home_roots`]). A folder this user can't list is skipped.
pub fn homes(users_dirs: Option<&[PathBuf]>) -> Vec<PathBuf> {
    let own = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    if let Some(roots) = users_dirs {
        return homes_under(roots, own.as_deref().unwrap_or(Path::new("")));
    }
    // Tests never look at the users of the machine they run on (as
    // Steam's and Heroic's own folders).
    if cfg!(test) {
        return vec![];
    }
    let Some(own) = own else { return vec![] };
    homes_under(&home_roots(&own), &own)
}

/// Where the other users' homes are: the folder holding this user's own
/// when that is a folder of homes (`/home`, `/Users`, `C:\Users`), and
/// only that, since the other users' homes are beside it. Otherwise, on
/// Linux, `/home`, for a service user whose home is elsewhere
/// (`/var/lib/upnext`). The folder holding any other home (`/` for
/// `/root`) holds no one's, so it isn't looked in. The end-to-end server's
/// home is in a folder of homes of its own, so it never looks at the real
/// `/home` of the machine it runs on.
fn home_roots(own: &Path) -> Vec<PathBuf> {
    let homes_folder =
        |p: &&Path| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| ["home", "users"].iter().any(|h| n.eq_ignore_ascii_case(h)));
    match own.parent().filter(homes_folder) {
        Some(folder) => vec![folder.to_path_buf()],
        None if cfg!(target_os = "linux") => vec![PathBuf::from("/home")],
        None => vec![],
    }
}

/// The folders in `roots` other than `own` and Windows' shared profiles,
/// each once, in order.
pub fn homes_under(roots: &[PathBuf], own: &Path) -> Vec<PathBuf> {
    let own = own.canonicalize().unwrap_or_else(|_| own.to_path_buf());
    let mut homes: Vec<PathBuf> = vec![];
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else { continue };
        let mut found: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter(|e| e.file_name().to_str().is_some_and(|n| !n.starts_with('.') && !SHARED.iter().any(|s| s.eq_ignore_ascii_case(n))))
            .map(|e| e.path())
            .collect();
        found.sort();
        for home in found {
            let real = home.canonicalize().unwrap_or_else(|_| home.clone());
            if real != own && !homes.iter().any(|h| h.canonicalize().unwrap_or_else(|_| h.clone()) == real) {
                homes.push(home);
            }
        }
    }
    homes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_other_homes_are_those_beside_this_one() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("home");
        for name in ["you", "sam", "alex", "Public", ".hidden"] {
            std::fs::create_dir_all(root.join(name)).unwrap();
        }
        std::fs::write(root.join("notes.txt"), "").unwrap();
        // A second root holding the same folders adds nothing twice, and
        // one that isn't there is skipped.
        let roots = [root.clone(), root.clone(), dir.path().join("missing")];
        assert_eq!(homes_under(&roots, &root.join("you")), [root.join("alex"), root.join("sam")]);
    }

    /// Only a folder of homes is looked in, never `/` or `/var/lib`; a
    /// home that is in one is looked beside alone, so a home in a folder of
    /// homes elsewhere (as the end-to-end server's is) never adds `/home`.
    #[test]
    fn only_folders_of_homes_are_looked_in() {
        let service = if cfg!(target_os = "linux") { vec![PathBuf::from("/home")] } else { vec![] };
        assert_eq!(home_roots(Path::new("/home/you")), [PathBuf::from("/home")]);
        assert_eq!(home_roots(Path::new("/Users/you")), [PathBuf::from("/Users")]);
        assert_eq!(home_roots(Path::new("/srv/upnext/e2e/home/you")), [PathBuf::from("/srv/upnext/e2e/home")]);
        assert_eq!(home_roots(Path::new("/root")), service);
        assert_eq!(home_roots(Path::new("/var/lib/upnext")), service);
    }
}
