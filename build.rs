//! Build-time facts. `UPNEXT_VERSION` is the version with the short commit it
//! was built from, for `upnext --version` and doctor, so testers can tell CI
//! builds apart; `UPNEXT_BUILD` is that commit alone, when there is one. On Windows the exes also get UpNext's icon and a
//! version resource, which Explorer's Properties, the taskbar and the window's
//! title bar show, and an application manifest.

use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=contrib/windows/upnext.ico");
    println!("cargo:rerun-if-changed=contrib/windows/upnext.manifest");
    println!("cargo:rerun-if-env-changed=UPNEXT_COMMIT");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    let version = std::env::var("CARGO_PKG_VERSION").expect("Cargo sets CARGO_PKG_VERSION");
    let commit = commit();
    // The build alone, for `GET /api/health` and the app window, which warns
    // when it joins an UpNext of another build.
    if let Some(commit) = &commit {
        println!("cargo:rustc-env=UPNEXT_BUILD={commit}");
    }
    let version = match commit {
        Some(commit) => format!("{version} ({commit})"),
        None => version,
    };
    println!("cargo:rustc-env=UPNEXT_VERSION={version}");

    // The build script runs on the computer that builds, so `cfg(windows)`
    // is that computer; the resource is only for a Windows target.
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        resources(&version);
    }
}

/// The short commit: a build that copies the source without its `.git`
/// names it in `UPNEXT_COMMIT` (the Flatpak's, from
/// `contrib/flatpak/generate-sources.sh`), GitHub Actions in `GITHUB_SHA`,
/// and elsewhere git does when the source is a checkout of UpNext itself. A
/// source tree with none of them (a container build copies no `.git`) gets
/// the plain version, and so does one unpacked inside another checkout,
/// whose commit isn't UpNext's.
fn commit() -> Option<String> {
    for var in ["UPNEXT_COMMIT", "GITHUB_SHA"] {
        if let Ok(sha) = std::env::var(var)
            && sha.len() >= 7
            && sha.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Some(sha[..7].to_string());
        }
    }
    let manifest = std::env::var("CARGO_MANIFEST_DIR").ok()?;
    let top = git(&["rev-parse", "--show-toplevel"])?;
    if Path::new(&top).canonicalize().ok()? != Path::new(&manifest).canonicalize().ok()? {
        return None;
    }
    let commit = git(&["rev-parse", "--short=7", "HEAD"])?;
    // Build again when the checkout moves to another commit: HEAD changes
    // when the branch does, the branch's ref when it gains a commit, and
    // packed-refs when `git pack-refs` (or a gc) moves the ref into it.
    let branch = git(&["symbolic-ref", "-q", "HEAD"]).unwrap_or_default();
    for file in ["HEAD".to_string(), branch, "packed-refs".to_string()] {
        if !file.is_empty()
            && let Some(path) = git(&["rev-parse", "--git-path", &file])
            && Path::new(&path).exists()
        {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    Some(commit)
}

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !text.is_empty()).then_some(text)
}

/// The icon, as resource 1 (`desktop::window_icon` loads it for the app
/// window), the version resource and the manifest
/// (`contrib/windows/upnext.manifest`: no elevation, Windows 10 and 11,
/// per-monitor DPI). Both exes get them.
#[cfg(windows)]
fn resources(version: &str) {
    let mut res = winresource::WindowsResource::new();
    res.set_icon("contrib/windows/upnext.ico")
        .set("ProductName", "UpNext")
        .set("FileDescription", "UpNext")
        .set("ProductVersion", version)
        .set("LegalCopyright", "MIT OR Apache-2.0")
        .set_manifest_file("contrib/windows/upnext.manifest");
    if let Err(e) = res.compile() {
        panic!("couldn't add the icon, version resource and manifest (the Windows SDK's rc.exe builds them): {e}");
    }
}
