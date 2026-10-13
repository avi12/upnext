//! Which build of UpNext this is: the version and the short commit it was
//! built from (`build.rs`). Two builds can both say 0.1.0, so the app window
//! compares the build too before it opens on an UpNext already running on
//! its port: an installed copy started at sign-in answered a
//! window from a newer checkout, which showed the older pages without a word.

/// The package version, as `GET /api/health` reports it.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The short commit this was built from: `GITHUB_SHA` in CI, else git in a
/// checkout. None for a source tree with neither (a container build copies
/// no `.git`), which can't be told apart from another such build.
pub const BUILD: Option<&str> = option_env!("UPNEXT_BUILD");

/// "0.1.0 (abc1234)", or the version alone for a build without a commit.
pub fn describe(version: &str, build: Option<&str>) -> String {
    match build {
        Some(b) => format!("{version} ({b})"),
        None => version.to_string(),
    }
}

/// What the window says when the UpNext on `port` is another build than its
/// own, or None when it's the same one.
#[cfg(any(feature = "desktop", test))]
pub fn mismatch(port: u16, version: &str, build: Option<&str>) -> Option<String> {
    mismatch_with(VERSION, BUILD, port, version, build)
}

/// `mismatch`, for a window of `mine`/`my_build`. Another version always
/// differs. With the same version, a window that knows its commit compares
/// it, and a server that names none (one from before builds were named, or
/// built without git) counts as another build; a window that knows none of
/// its own can't tell, and says nothing.
#[cfg(any(feature = "desktop", test))]
fn mismatch_with(mine: &str, my_build: Option<&str>, port: u16, version: &str, build: Option<&str>) -> Option<String> {
    let differs = version != mine || my_build.is_some_and(|m| build != Some(m));
    if !differs {
        return None;
    }
    let theirs = match build {
        Some(_) => describe(version, build),
        None => format!("{version}, a build that doesn't say which commit it's from"),
    };
    Some(format!(
        "This window is UpNext {}, but the UpNext already running on port {port} is {theirs}, so these are that one's pages. \
         To use this build, quit the other UpNext (or stop its service) and open this window again.",
        describe(mine, my_build)
    ))
}

/// A script that shows `text` in a bar along the bottom of every page the
/// window loads, whatever build served the page: the older server's own UI
/// knows nothing of this check. The bar can be closed.
#[cfg(any(feature = "desktop", test))]
pub fn notice_script(text: &str) -> String {
    let text = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"(() => {{
  const text = {text};
  const show = () => {{
    if (document.getElementById('upnext-build-notice')) return;
    const bar = document.createElement('div');
    bar.id = 'upnext-build-notice';
    bar.setAttribute('role', 'alert');
    bar.style.cssText = 'position:fixed;left:12px;right:12px;bottom:12px;z-index:2147483647;padding:10px 44px 10px 14px;border-radius:10px;background:#7a4b00;color:#fff;font:14px/1.45 system-ui,sans-serif;box-shadow:0 4px 18px rgba(0,0,0,.45)';
    bar.textContent = text;
    const close = document.createElement('button');
    close.type = 'button';
    close.textContent = '×';
    close.setAttribute('aria-label', 'Close');
    close.style.cssText = 'position:absolute;top:4px;right:8px;background:none;border:0;color:inherit;font-size:22px;cursor:pointer';
    close.onclick = () => bar.remove();
    bar.appendChild(close);
    document.body.appendChild(bar);
  }};
  if (document.body) show(); else document.addEventListener('DOMContentLoaded', show);
}})();"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_build_says_nothing_and_another_says_which() {
        assert_eq!(mismatch_with("0.1.0", Some("abc1234"), 8787, "0.1.0", Some("abc1234")), None);
        let other = mismatch_with("0.1.0", Some("abc1234"), 8787, "0.1.0", Some("def5678")).unwrap();
        assert!(other.contains("This window is UpNext 0.1.0 (abc1234)"), "{other}");
        assert!(other.contains("on port 8787 is 0.1.0 (def5678)"), "{other}");
        assert!(other.contains("quit the other UpNext"), "{other}");
        // An UpNext from before /api/health named its build: another build.
        let older = mismatch_with("0.1.0", Some("abc1234"), 8787, "0.1.0", None).unwrap();
        assert!(older.contains("is 0.1.0, a build that doesn't say which commit"), "{older}");
        // Another version, whatever the commits say.
        assert!(mismatch_with("0.2.0", None, 8787, "0.1.0", None).unwrap().contains("This window is UpNext 0.2.0, but"));
        // A window that doesn't know its own commit can only compare versions.
        assert_eq!(mismatch_with("0.1.0", None, 8787, "0.1.0", Some("def5678")), None);
        assert_eq!(mismatch_with("0.1.0", None, 8787, "0.1.0", None), None);
    }

    #[test]
    fn this_build_matches_itself_and_names_its_commit() {
        assert_eq!(mismatch(8787, VERSION, BUILD), None);
        assert_eq!(describe(VERSION, BUILD), env!("UPNEXT_VERSION"), "as --version says it");
        if let Some(b) = BUILD {
            assert!(b.len() == 7 && b.bytes().all(|c| c.is_ascii_hexdigit()), "{b}");
        }
    }

    #[test]
    fn the_notice_carries_its_text_as_a_string() {
        let script = notice_script("port 8787 is \"old\" </script>\n");
        assert!(script.contains(r#"const text = "port 8787 is \"old\" </script>\n";"#), "{script}");
        assert!(script.contains("upnext-build-notice"));
    }
}
