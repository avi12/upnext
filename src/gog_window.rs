//! Native GOG login window (`upnext gog-window`). GOG's OAuth client only
//! redirects to its own blank `on_login_success` page, which a browser tab
//! cannot hand back to UpNext; like Heroic, we open the login in a webview we
//! control, watch for that page, and print the code on stdout:
//!   `CODE <code>` on success, `CLOSED` if the window was closed first.
//! Runs as its own process because the GUI event loop needs the main thread.
//!
//! GOG's page signs in through another account (Discord, Facebook, a
//! console's) in a popup, which hands the sign-in back to the page that
//! opened it. A webview drops popups unless told what to do with them, so
//! the window opens each https one on GOG's or a sign-in provider's site as
//! a window of its own, related to this one so the two can talk.
//! Google refuses to sign in inside an embedded webview whatever the window
//! does, so for Google the Accounts row offers pasting from your own
//! browser instead.

use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::{NewWindowResponse, WebViewBuilder};

const SUCCESS_PREFIX: &str = "https://embed.gog.com/on_login_success";

/// The sites the sign-in page may open a popup on, with their subdomains:
/// GOG's own, and the accounts its page offers to sign in with (Google,
/// Facebook, Discord, Apple, PlayStation, Xbox and Steam). Google turns an
/// embedded webview away, but its own page saying so beats a button that
/// does nothing.
const POPUP_SITES: &[&str] = &[
    "gog.com",
    "google.com",
    "facebook.com",
    "discord.com",
    "discordapp.com",
    "apple.com",
    "playstation.com",
    "sonyentertainmentnetwork.com",
    "live.com",
    "microsoft.com",
    "microsoftonline.com",
    "xbox.com",
    "steampowered.com",
    "steamcommunity.com",
];

/// Whether the sign-in page may open `url` as a popup: https, on one of
/// `POPUP_SITES` or a subdomain of one. Anything else (another site, another
/// scheme, `about:blank`) is dropped, as every popup was before.
fn popup_allowed(url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(url) else { return false };
    let Some(host) = url.host_str().filter(|_| url.scheme() == "https") else { return false };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    POPUP_SITES.iter().any(|site| host == *site || host.strip_suffix(site).is_some_and(|rest| rest.ends_with('.')))
}

pub fn run() -> anyhow::Result<()> {
    // The server that starts this window passes on its environment, so this
    // finds the same cache folder. The login is private (incognito), but
    // WebView2 still needs a folder it can write to, and beside the program
    // may not be one.
    let cache =
        crate::Config::load().map_or_else(|_| crate::Config::webview_cache_unloaded(|k| std::env::var(k).ok()), |c| c.webview_cache());
    let event_loop = EventLoopBuilder::<String>::with_user_event().build();
    let window = WindowBuilder::new()
        .with_title("Connect GOG · UpNext")
        .with_window_icon(crate::desktop::window_icon())
        .with_inner_size(tao::dpi::LogicalSize::new(480.0, 720.0))
        .build(&event_loop)?;
    let mut context = crate::desktop::web_context(crate::desktop::webview_dir(&cache, "gog"));

    // Both hooks watch for the landing page; server redirects surface in one
    // or the other depending on the platform webview.
    let found = move |url: &str, proxy: &tao::event_loop::EventLoopProxy<String>| {
        if url.starts_with(SUCCESS_PREFIX)
            && let Some(code) = crate::gog::extract_code(url)
        {
            let _ = proxy.send_event(code);
        }
    };
    let (p1, p2) = (event_loop.create_proxy(), event_loop.create_proxy());
    let builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_url(crate::gog::auth_url())
        .with_incognito(true)
        .with_navigation_handler(move |url| {
            found(&url, &p1);
            true
        })
        .with_on_page_load_handler(move |_, url| found(&url, &p2))
        .with_new_window_req_handler(|url, _| if popup_allowed(&url) { NewWindowResponse::Allow } else { NewWindowResponse::Deny });

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let _webview = builder.build(&window)?;
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let _webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window.default_vbox().ok_or_else(|| anyhow::anyhow!("no GTK container for the webview"))?;
        builder.build_gtk(vbox)?
    };

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(code) => {
                println!("CODE {code}");
                std::process::exit(0);
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                println!("CLOSED");
                std::process::exit(0);
            }
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_providers_sign_in_opens_and_nothing_else_does() {
        assert!(popup_allowed("https://discord.com/oauth2/authorize?client_id=1"));
        assert!(popup_allowed("https://login.gog.com/social/facebook"));
        assert!(popup_allowed("https://www.Facebook.com./dialog/oauth"));
        assert!(popup_allowed("https://accounts.google.com/o/oauth2/v2/auth"));
        for no in [
            "https://example.org/",
            "https://evilgog.com/",
            "https://gog.com.example.org/",
            "https://discord.com@example.org/",
            "http://login.gog.com/",
            "about:blank",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "steam://run/620",
            "not a url",
        ] {
            assert!(!popup_allowed(no), "{no}");
        }
    }
}
