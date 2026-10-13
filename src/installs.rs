//! What Windows' registry says is installed, for the launchers that keep
//! their games' records there: the EA app and Ubisoft Connect. Reading is
//! Windows only; everywhere else there is nothing to read, and the readers
//! take these lists as plain data so their tests run anywhere.

use std::path::PathBuf;

/// One program in Windows' Apps list (an `Uninstall` key).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UninstallEntry {
    /// The key's own name: a GUID, or a name such as `Uplay Install 635`.
    pub key: String,
    pub display_name: Option<String>,
    /// As the registry holds it; `registry_path` makes it a folder.
    pub install_location: Option<String>,
    pub uninstall_string: Option<String>,
    pub publisher: Option<String>,
}

/// A folder as the registry holds it: trimmed, and with the forward slashes
/// some launchers write (`C:/Games/Far Cry 5/`) turned into the platform's own.
pub fn registry_path(raw: &str) -> Option<PathBuf> {
    let raw = raw.trim().trim_matches('"');
    (!raw.is_empty()).then(|| PathBuf::from(raw.replace('/', std::path::MAIN_SEPARATOR_STR)))
}

/// Every program in the Apps list: the machine's 64-bit and 32-bit views
/// and the user's own. Tests never read the machine's registry (as with
/// Steam's folder).
pub fn uninstall_entries() -> Vec<UninstallEntry> {
    if cfg!(test) { vec![] } else { read_uninstall_entries() }
}

/// The subkeys of `path` under HKLM, in the 32-bit view (where 32-bit
/// launchers write, `WOW6432Node` on 64-bit Windows) and then the 64-bit
/// one, with the string `value` each holds: `(subkey, value)`.
pub fn machine_subkey_values(path: &str, value: &str) -> Vec<(String, String)> {
    if cfg!(test) { vec![] } else { read_machine_subkey_values(path, value) }
}

#[cfg(windows)]
const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";

#[cfg(windows)]
fn read_uninstall_entries() -> Vec<UninstallEntry> {
    use winreg::enums::{KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};
    use winreg::{HKCU, HKLM};
    let mut out = vec![];
    for (root, flags) in [(HKLM, KEY_READ | KEY_WOW64_64KEY), (HKLM, KEY_READ | KEY_WOW64_32KEY), (HKCU, KEY_READ)] {
        let Ok(list) = root.open_subkey_with_flags(UNINSTALL, flags) else { continue };
        for name in list.enum_keys().filter_map(Result::ok) {
            let Ok(k) = list.open_subkey_with_flags(&name, flags) else { continue };
            let text = |v: &str| k.get_value::<String, _>(v).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
            let entry = UninstallEntry {
                key: name.clone(),
                display_name: text("DisplayName"),
                install_location: text("InstallLocation"),
                uninstall_string: text("UninstallString"),
                publisher: text("Publisher"),
            };
            // A 64-bit Windows shows some keys in both views.
            if !out.iter().any(|e: &UninstallEntry| e.key == entry.key && e.display_name == entry.display_name) {
                out.push(entry);
            }
        }
    }
    out
}

#[cfg(not(windows))]
fn read_uninstall_entries() -> Vec<UninstallEntry> {
    vec![]
}

#[cfg(windows)]
fn read_machine_subkey_values(path: &str, value: &str) -> Vec<(String, String)> {
    use winreg::HKLM;
    use winreg::enums::{KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};
    let mut out: Vec<(String, String)> = vec![];
    for flags in [KEY_READ | KEY_WOW64_32KEY, KEY_READ | KEY_WOW64_64KEY] {
        let Ok(list) = HKLM.open_subkey_with_flags(path, flags) else { continue };
        for name in list.enum_keys().filter_map(Result::ok) {
            let Some(v) = list.open_subkey_with_flags(&name, flags).ok().and_then(|k| k.get_value::<String, _>(value).ok()) else {
                continue;
            };
            if !out.iter().any(|(n, _)| n == &name) {
                out.push((name, v));
            }
        }
    }
    out
}

#[cfg(not(windows))]
fn read_machine_subkey_values(_path: &str, _value: &str) -> Vec<(String, String)> {
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_paths_take_the_platforms_separators() {
        let sep = std::path::MAIN_SEPARATOR_STR;
        assert_eq!(registry_path(" C:/Games/Far Cry 5/ "), Some(PathBuf::from(format!("C:{sep}Games{sep}Far Cry 5{sep}"))));
        assert_eq!(registry_path("\"\""), None);
        assert_eq!(registry_path("  "), None);
    }

    #[test]
    fn tests_never_read_this_machines_registry() {
        assert!(uninstall_entries().is_empty());
        assert!(machine_subkey_values(r"SOFTWARE\Ubisoft\Launcher\Installs", "InstallDir").is_empty());
    }
}
