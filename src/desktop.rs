//! The app window (`upnext app`): the web UI in a native window, with tao and
//! wry (WebKitGTK on Linux). `cli::app` decides which server it shows; this
//! is the window, and where the links in it go.
//!
//! The page is loaded over HTTP from 127.0.0.1, so the UI's `fetch('/api/…')`
//! calls, the Steam sign-in's `return_to` and the server's loopback checks
//! all work as they do in a browser.

use reqwest::Url;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy, EventLoopWindowTarget};
use tao::window::{Fullscreen, ResizeDirection, Window, WindowBuilder};
use wry::{NewWindowResponse, PageLoadEvent, WebContext, WebView, WebViewBuilder};

/// The window's Wayland app id and the `.desktop` file's name. Plasma
/// matches the two to show UpNext's icon in the taskbar and group its windows.
pub(crate) const APP_ID: &str = "upnext";

/// The window's app id: [`APP_ID`], or in a Flatpak its own id, which names
/// the launcher entry Flatpak installs for it. Only Linux names windows so.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn window_app_id(sandbox: Option<&crate::flatpak::Flatpak>) -> &str {
    sandbox.map_or(APP_ID, |f| f.id.as_str())
}

/// Told to the page when the window gets the focus back, so it can fetch
/// what may have changed in the browser meanwhile (a Steam sign-in), and
/// when the tray changes a setting Settings shows. `web/src/App.tsx`
/// listens for it.
const FOCUS_SCRIPT: &str = "window.dispatchEvent(new Event('upnext:focus'))";

/// Tells the page it's in the app's window rather than a browser, whichever
/// server the window shows (its own, the service's or `upnext serve`'s):
/// the setup wizard says so, and that the Steam sign-in and other sites open
/// in the browser. `web/src/lib/setup.ts` reads it.
const APP_SCRIPT: &str = "window.upnextApp = true";

/// Tells the page the window was opened in big screen (`--big-screen`),
/// so it starts there whatever the webview remembers
/// from a window opened without it. `web/src/lib/bigscreen.ts` reads it.
const BIG_SCRIPT: &str = "window.upnextBigScreen = true";

/// Whether the window goes without the system's title bar and frame, with
/// the UI drawing its own title bar in their place. Windows and
/// Linux only: macOS isn't scheduled, and tao can't resize a window by
/// hand there.
const FRAMELESS: bool = cfg!(any(windows, target_os = "linux"));

/// Tells the page the window has no title bar of its own, so the UI draws
/// one (`web/src/components/TitleBar.tsx`), and which system it's on: the
/// edges the UI makes for resizing differ (`web/src/lib/frame.ts`). Set
/// only in the frameless window, never in a browser.
#[cfg(windows)]
const FRAME_SCRIPT: &str = "window.upnextFrame = { os: 'windows' }";
#[cfg(not(windows))]
const FRAME_SCRIPT: &str = "window.upnextFrame = { os: 'linux' }";

/// How long the window waits, after a page has loaded, for the UI to say it
/// drew its title bar (`window:framed`) before giving the window the system's
/// frame back. A page that never says so would otherwise leave a window
/// that can't be moved or closed with the mouse: the browser engine's own
/// error page when the server doesn't answer, or the UI of an older UpNext
/// server the window shows (a service not yet restarted after an upgrade).
const FRAME_WAIT: std::time::Duration = std::time::Duration::from_millis(1500);
/// The same, from when a page starts loading, in case the engine never
/// says it finished.
const FRAME_WAIT_LONG: std::time::Duration = std::time::Duration::from_secs(8);

/// The window's keys, caught in the page (the webview has the keyboard, on
/// WebView2 in a window of its own) before the UI's own shortcuts see them,
/// and passed to [`Key`]. Ctrl only, so `/` and 1 to 5 stay the UI's; and
/// F5 and F11, which no page of the UI uses.
const KEYS_SCRIPT: &str = r#"(() => {
  if (window.__upnextKeys) return
  window.__upnextKeys = true
  window.addEventListener('keydown', (e) => {
    const k = e.key.toLowerCase()
    let key = null
    if (e.key === 'F5' && !e.ctrlKey && !e.altKey) key = 'reload'
    else if (e.key === 'F11' && !e.ctrlKey && !e.altKey) key = 'fullscreen'
    else if (e.ctrlKey && !e.altKey && !e.metaKey) {
      if (e.code === 'KeyQ' || k === 'q') key = 'quit'
      else if (e.code === 'KeyR' || k === 'r') key = 'reload'
      else if (k === '+' || k === '=') key = 'zoom-in'
      else if (k === '-' || k === '_') key = 'zoom-out'
      else if (k === '0') key = 'zoom-reset'
    }
    if (!key) return
    e.preventDefault()
    e.stopPropagation()
    if (!e.repeat || key.startsWith('zoom')) window.ipc.postMessage(key)
  }, true)
})()"#;

/// What the window's keys do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Key {
    /// Ctrl+Q: what closing the window does.
    Quit,
    /// Ctrl+R or F5.
    Reload,
    /// F11: full screen, or back to the window as it was.
    Fullscreen,
    /// Ctrl+plus (or Ctrl+=), Ctrl+minus and Ctrl+0.
    ZoomIn,
    ZoomOut,
    ZoomReset,
}

impl Key {
    /// From `KEYS_SCRIPT`'s message; anything else the page sends is ignored.
    fn parse(message: &str) -> Option<Self> {
        Some(match message {
            "quit" => Self::Quit,
            "reload" => Self::Reload,
            "fullscreen" => Self::Fullscreen,
            "zoom-in" => Self::ZoomIn,
            "zoom-out" => Self::ZoomOut,
            "zoom-reset" => Self::ZoomReset,
            _ => return None,
        })
    }
}

/// What the UI's title bar asks of the window (`web/src/lib/frame.ts`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum WindowAction {
    /// The UI drew its title bar. It says so each time a page loads, and is
    /// told the window's state in return.
    Framed,
    /// The pointer moved on the title bar with the left button down: the
    /// window moves with it until the button comes up.
    Drag,
    Minimize,
    /// Maximize, or restore when maximized: the button, or a double click
    /// on the title bar.
    ToggleMaximize,
    /// The close button: what closing the window does.
    Close,
    /// The left button went down on one of the UI's resize edges.
    Resize(ResizeDirection),
}

/// A message from the page, through `window.ipc.postMessage`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Message {
    Key(Key),
    Window(WindowAction),
    /// Open the window on another computer's UpNext, by its id in
    /// `computers.json` (`connect:<id>`), or on this computer's again
    /// (`connect:here`).
    Connect(Option<String>),
    /// The page is in big screen (`big-screen:on`) or isn't
    /// (`big-screen:off`): `web/src/lib/bigscreen.ts` says so when the page
    /// loads and at each switch.
    BigScreen(bool),
}

impl Message {
    /// From `KEYS_SCRIPT` or the title bar; anything else is ignored.
    fn parse(message: &str) -> Option<Self> {
        if let Some(key) = Key::parse(message) {
            return Some(Self::Key(key));
        }
        match message {
            "big-screen:on" => return Some(Self::BigScreen(true)),
            "big-screen:off" => return Some(Self::BigScreen(false)),
            _ => {}
        }
        if let Some(to) = message.strip_prefix("connect:") {
            return match to {
                "here" => Some(Self::Connect(None)),
                id if !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric()) => {
                    Some(Self::Connect(Some(id.into())))
                }
                _ => None,
            };
        }
        let action = match message.strip_prefix("window:")? {
            "framed" => WindowAction::Framed,
            "drag" => WindowAction::Drag,
            "minimize" => WindowAction::Minimize,
            "maximize" => WindowAction::ToggleMaximize,
            "close" => WindowAction::Close,
            other => WindowAction::Resize(match other.strip_prefix("resize:")? {
                "n" => ResizeDirection::North,
                "ne" => ResizeDirection::NorthEast,
                "e" => ResizeDirection::East,
                "se" => ResizeDirection::SouthEast,
                "s" => ResizeDirection::South,
                "sw" => ResizeDirection::SouthWest,
                "w" => ResizeDirection::West,
                "nw" => ResizeDirection::NorthWest,
                _ => return None,
            }),
        };
        Some(Self::Window(action))
    }
}

/// Whether a message through `window.ipc` came from a page of `app`, the
/// UpNext the window shows: only those can press the window's keys or move,
/// resize and close it.
fn from_app(sender: &str, app: &Url) -> bool {
    Url::parse(sender).is_ok_and(|url| url.origin() == app.origin())
}

/// How the window is, for the title bar.
#[derive(Debug, Clone, Copy, PartialEq)]
struct WindowState {
    maximized: bool,
    focused: bool,
    /// Full screen, where the title bar goes away.
    fullscreen: bool,
}

/// The script that tells the title bar how the window is, as the event
/// `upnext:window`, so its maximize button shows the real state (the window
/// can be maximized by a double click, a key or snapping it to the screen's
/// edge, not only by the button), it can dim while the window is behind
/// another, and it steps aside while the window is full screen.
fn window_state_script(state: WindowState) -> String {
    let WindowState { maximized, focused, fullscreen } = state;
    format!(
        "window.dispatchEvent(new CustomEvent('upnext:window', {{ detail: {{ maximized: {maximized}, focused: {focused}, fullscreen: {fullscreen} }} }}))"
    )
}

/// The zoom steps, in percent, as browsers have them.
const ZOOMS: [u32; 13] = [50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300];

/// The zoom after `key`, from `zoom` (percent).
pub(crate) fn zoomed(zoom: u32, key: Key) -> u32 {
    match key {
        Key::ZoomIn => ZOOMS.iter().copied().find(|&z| z > zoom).unwrap_or(zoom.max(ZOOMS[ZOOMS.len() - 1])),
        Key::ZoomOut => ZOOMS.iter().rev().copied().find(|&z| z < zoom).unwrap_or(zoom.min(ZOOMS[0])),
        Key::ZoomReset => 100,
        Key::Quit | Key::Reload | Key::Fullscreen => zoom,
    }
}

/// Whether the window goes full screen (`Some(true)`) or leaves it
/// (`Some(false)`) as the page goes into big screen (`on`) or leaves it,
/// for a window that is `fullscreen` now, `for_big` when that's big
/// screen's doing. Big screen is made for the whole screen, so going into
/// it fills the screen; leaving it gives back the windowed size, but not
/// when the full screen is F11's.
pub(crate) fn fullscreen_for_big_screen(on: bool, fullscreen: bool, for_big: bool) -> Option<bool> {
    match (on, fullscreen) {
        (true, false) => Some(true),
        (false, true) if for_big => Some(false),
        _ => None,
    }
}

/// What the window remembers from one run to the next: the `window_size`,
/// `window_maximized` and `window_zoom` settings, read from the server it
/// shows when it opens and saved there when it closes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Remembered {
    /// Width and height, not maximized, in logical pixels. `None` until
    /// the window has closed once.
    pub size: Option<(u32, u32)>,
    pub maximized: bool,
    /// Percent.
    pub zoom: u32,
}

impl Default for Remembered {
    fn default() -> Self {
        Self { size: None, maximized: false, zoom: 100 }
    }
}

/// The size of a window opened for the first time.
const FIRST_SIZE: (u32, u32) = (1280, 860);
/// The smallest the window goes.
const MIN_SIZE: (u32, u32) = (640, 480);

impl Remembered {
    /// From `GET /api/settings`. What isn't there (a server from before
    /// these settings) or doesn't make sense stays at its default.
    pub(crate) fn from_settings(settings: &Value) -> Self {
        let size = settings["window_size"].as_str().and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        });
        let zoom = settings["window_zoom"].as_u64().and_then(|z| u32::try_from(z).ok()).filter(|z| (30..=300).contains(z));
        Self { size, maximized: settings["window_maximized"].as_bool().unwrap_or(false), zoom: zoom.unwrap_or(100) }
    }

    /// The body of `PUT /api/settings` that saves it.
    pub(crate) fn to_settings(self) -> Value {
        let mut v = json!({ "window_maximized": self.maximized, "window_zoom": self.zoom });
        if let Some((w, h)) = self.size {
            v["window_size"] = json!(format!("{w}x{h}"));
        }
        v
    }

    /// The size to open at: the one saved, no smaller than the window
    /// goes and no larger than `screen` (logical pixels) when it's known.
    pub(crate) fn open_size(&self, screen: Option<(u32, u32)>) -> (u32, u32) {
        let (mut w, mut h) = self.size.unwrap_or(FIRST_SIZE);
        if let Some((sw, sh)) = screen {
            (w, h) = (w.min(sw), h.min(sh));
        }
        (w.max(MIN_SIZE.0), h.max(MIN_SIZE.1))
    }
}

/// The screen (logical pixels) the window's size is kept within as it opens:
/// the primary one, where the system names one, which is where Windows and
/// X11 open a new window; otherwise the largest. GTK on Wayland names no
/// primary screen and the compositor picks where the window goes, so the
/// first screen in the list, which could be a small portrait one beside a
/// wide one, would shrink the window for no reason.
pub(crate) fn open_screen(primary: Option<(u32, u32)>, all: impl IntoIterator<Item = (u32, u32)>) -> Option<(u32, u32)> {
    primary.or_else(|| {
        all.into_iter().fold(None, |big: Option<(u32, u32)>, s| match big {
            Some(b) if u64::from(b.0) * u64::from(b.1) >= u64::from(s.0) * u64::from(s.1) => Some(b),
            _ => Some(s),
        })
    })
}

pub(crate) enum AppEvent {
    /// The server inside the app stopped: the window closed, or Ctrl-C or
    /// SIGTERM reached the process. With the error, if it failed.
    ServerStopped(Option<String>),
    /// One of the window's keys was pressed in the page.
    Key(Key),
    /// The UI's title bar asked for something.
    Window(WindowAction),
    /// A page started loading (false) or finished (true) in the window.
    Load(bool),
    /// Time's up for the page that was the window's `n`th to load to say it
    /// drew its title bar (`FRAME_WAIT`).
    FrameCheck(u64),
    /// The tray icon was clicked, or an item in its menu chosen. Only
    /// Windows has the icon.
    #[cfg_attr(not(windows), allow(dead_code))]
    Tray(TrayAction),
    /// The page asked to open the window on another computer, or on this
    /// one's own UpNext again (None).
    Connect(Option<String>),
    /// The page went into big screen (true) or left it.
    BigScreen(bool),
    /// A device asks to be allowed: the window asks for the
    /// person's attention, and one closed to the tray opens.
    Attention,
    /// The window handed `url` to the desktop (`Route::System`), which took
    /// it when `opened`.
    HandedOut { url: String, opened: bool },
    /// `LAUNCH_WAIT` has passed since a game started from a window that
    /// closes when one does (`closes_after_launch`).
    GameStarted,
    /// What answered at the address of the computer the page chose, by id,
    /// checked before its token goes there.
    Reached(String, crate::computers::Reach),
}

/// The UpNext the window shows, and how it opens on it.
#[derive(Clone, PartialEq)]
pub(crate) struct Target {
    /// Its address: links stay on it, and only its pages work the window.
    pub app: Url,
    /// The first page the window loads.
    pub first: Url,
    /// Another computer's token, sent with the first page only: that's
    /// `/api/v1/pair/open` there, which sets the paired-browser cookie.
    pub token: Option<String>,
    /// Another computer's name, told to the page (`REMOTE_SCRIPT`).
    pub remote: Option<String>,
}

// By hand, so a log or a failed test never prints the token.
impl std::fmt::Debug for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Target")
            .field("app", &self.app.as_str())
            .field("first", &self.first.as_str())
            .field("token", &self.token.as_ref().map(|_| "…"))
            .field("remote", &self.remote)
            .finish()
    }
}

impl Target {
    /// This computer's UpNext at `app`, opening on `path`.
    pub(crate) fn here(app: Url, path: &str) -> Self {
        let first = app.join(path).unwrap_or_else(|_| app.clone());
        Self { app, first, token: None, remote: None }
    }

    /// Another computer's, kept in `computers.json`.
    pub(crate) fn on(c: &crate::computers::Computer) -> anyhow::Result<Self> {
        let app: Url = format!("{}/", c.address.trim_end_matches('/')).parse()?;
        let first = app.join("api/v1/pair/open")?;
        Ok(Self { app, first, token: Some(c.token.clone()), remote: Some(c.name.clone()) })
    }

    /// The header the first page goes with.
    fn headers(&self) -> Option<wry::http::HeaderMap> {
        let token = self.token.as_deref()?;
        let mut h = wry::http::HeaderMap::new();
        h.insert(wry::http::header::AUTHORIZATION, wry::http::HeaderValue::from_str(&format!("Bearer {token}")).ok()?);
        Some(h)
    }
}

/// What every page of this computer's UpNext shows along the bottom, once
/// the app finds an UpNext of another build on its port.
static NOTICE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Show `text` on this computer's UpNext's pages (`build_id::notice_script`).
pub(crate) fn notice_on_local(text: String) {
    let _ = NOTICE.set(text);
}

/// Tells a page from another computer which one it is, so Settings can say
/// so and offer this computer's library again (`connect:here`).
/// `web/src/lib/connect.ts` reads it.
fn remote_script(name: &str) -> String {
    format!("window.upnextRemote = {{ name: {} }}", json!(name))
}

/// Where the window can go besides where it opened: this computer's own
/// UpNext, and the config folder holding `computers.json`.
pub(crate) struct Places {
    pub local: Url,
    pub config_dir: PathBuf,
}

/// Where the window goes when the chosen computer's address doesn't answer
/// as that computer: the connect page, saying which and why, with this
/// computer's library a tap away. None when it's the same computer.
pub(crate) fn when_reached(local: &Url, id: &str, reach: &crate::computers::Reach) -> Option<Target> {
    use crate::computers::Reach;
    let why = match reach {
        Reach::Same => return None,
        Reach::Other(_) => "changed",
        Reach::Away(_) => "unreachable",
    };
    Some(Target::here(local.clone(), &format!("connect?{why}={id}")))
}

/// Check the chosen computer from another thread, so the window stays
/// responsive for the few seconds it may take, and come back with
/// `AppEvent::Reached`.
fn check_then_open(places: &Places, id: String, proxy: EventLoopProxy<AppEvent>) {
    let kept = crate::computers::Computers::load(&places.config_dir);
    let Some(c) = kept.ok().and_then(|k| k.get(&id).cloned()) else {
        message_box("UpNext couldn't open that computer's library: it isn't saved here any more.");
        return;
    };
    std::thread::spawn(move || {
        let reach = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt.block_on(crate::computers::reach(&c)),
            Err(e) => crate::computers::Reach::Away(e.to_string()),
        };
        let _ = proxy.send_event(AppEvent::Reached(id, reach));
    });
}

/// The target for a `connect:` message, saving the choice as the one the
/// window opens on next time.
fn connect_to(places: &Places, to: Option<&str>) -> anyhow::Result<Target> {
    let mut kept = crate::computers::Computers::load(&places.config_dir)?;
    let target = match to {
        None => Target::here(places.local.clone(), "/"),
        Some(id) => Target::on(kept.get(id).ok_or_else(|| anyhow::anyhow!("that computer isn't saved here any more"))?)?,
    };
    kept.open = to.map(str::to_string);
    kept.save(&places.config_dir)?;
    Ok(target)
}

/// What the tray icon asks for (Windows).
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) enum TrayAction {
    /// A left click, or Open UpNext: show the window, opening it if it's closed.
    Open,
    /// Start with Windows was chosen, and its checkmark has changed.
    StartWithWindows,
    /// Quit: close the window and stop the server, whatever the settings say.
    Quit,
    /// The menu is about to open: its checkmark follows the setting, which
    /// Settings may have changed since.
    Refresh,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl TrayAction {
    /// The menu's items, by id.
    const MENU: [(&str, TrayAction); 3] = [("open", Self::Open), ("start-with-windows", Self::StartWithWindows), ("quit", Self::Quit)];

    /// From the id of the menu item chosen.
    fn from_menu(id: &str) -> Option<Self> {
        Self::MENU.iter().find(|(item, _)| *item == id).map(|(_, action)| *action)
    }
}

/// The settings of the server the window shows, which it reads and changes
/// over its HTTP API (`GET` and `PUT /api/settings`).
pub(crate) trait ServerSettings {
    /// All of them, or `None` when the server doesn't answer.
    fn get(&self) -> Option<Value>;
    /// Change those in `body`; the server's error when it refuses.
    fn put(&self, body: &Value) -> Result<(), String>;
}

/// The tray's part of the window.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Background {
    /// A tray icon: on Windows, when the app runs its own server. The server
    /// can then outlive the window, and the icon is the way back to it.
    pub tray: bool,
    /// Start with the tray icon only and no window (`upnext-app
    /// --background`, which the Run key starts at sign-in).
    pub hidden: bool,
    /// Whether Start with Windows has its checkmark when the menu is made.
    pub start_with_windows: bool,
    /// Opened by `--big-screen`: full screen, with the
    /// page in big screen. `cli::app` gives such a window no tray icon, so
    /// closing it ends it, as a launcher that started it expects.
    pub big_screen: bool,
}

/// Whether closing the window leaves the server running: only with a tray
/// icon to come back through, when the `keep_running` setting is on (read
/// as the window closes, since Settings may have changed it) and the server
/// hasn't failed. Quit from the tray always stops it.
pub(crate) fn keeps_running(tray: bool, quit: bool, failed: bool, settings: Option<&Value>) -> bool {
    // A server that doesn't answer isn't worth keeping.
    let keep = settings.is_some_and(|v| v["keep_running"].as_bool().unwrap_or(true));
    tray && !quit && !failed && keep
}

/// Whether the tray's Start with Windows has its checkmark: the setting, off
/// when the server doesn't say (an UpNext from before it).
pub(crate) fn starts_with_windows(settings: &Value) -> bool {
    settings["start_with_windows"].as_bool().unwrap_or(false)
}

/// The tray's Start with Windows was clicked, and its check already shows
/// `on`. Once the server has it, `page` tells the window to read its
/// settings again: a native menu doesn't take the focus from the window, so
/// Settings showed the old state until it was reloaded (Windows retest).
pub(crate) fn start_with_windows_from_tray(on: bool, server: &impl ServerSettings, page: impl FnOnce()) -> Result<(), String> {
    server.put(&json!({ "start_with_windows": on }))?;
    page();
    Ok(())
}

/// Where a URL the page wants to go to is opened.
#[derive(Debug, PartialEq)]
pub(crate) enum Route {
    /// In the window: one of the UI's own pages.
    Stay,
    /// To the desktop, which opens web pages in the default browser and the
    /// Play buttons' links in their launcher. The browser gets other sites
    /// (store pages, trailers, HLTB) and the server's own links that aren't
    /// pages (the Steam sign-in, downloads), which it handles better than a
    /// window with no address bar or downloads folder.
    System,
    /// Nowhere.
    Block,
}

/// Where a navigation in the window to `to` goes, for a window showing `app`.
pub(crate) fn route(to: &str, app: &Url) -> Route {
    let Ok(url) = Url::parse(to) else { return Route::Block };
    match url.scheme() {
        "http" | "https" if url.origin() == app.origin() => {
            if url.path() == "/api" || url.path().starts_with("/api/") {
                Route::System
            } else {
                Route::Stay
            }
        }
        s if handed_out(s) => Route::System,
        // about:blank, and what WebKit makes of an empty frame.
        "about" => Route::Stay,
        _ => Route::Block,
    }
}

/// Where a link asking for a new window (`target="_blank"`) goes: never a
/// second window, the desktop for anything it can open.
pub(crate) fn route_new_window(to: &str) -> Route {
    match Url::parse(to) {
        Ok(url) if handed_out(url.scheme()) => Route::System,
        _ => Route::Block,
    }
}

/// The schemes the desktop opens: web pages and mail, and the launchers'
/// links behind the Play buttons (`run_url`).
fn handed_out(scheme: &str) -> bool {
    matches!(scheme, "http" | "https" | "mailto") || launcher_link(scheme)
}

/// The launchers' schemes, which the UI uses only for the Play buttons'
/// links. Epic's and Ubisoft's are their launchers' own (`epic::run_url`,
/// `ubisoft::run_url`).
fn launcher_link(scheme: &str) -> bool {
    matches!(scheme, "steam" | "heroic" | "goggalaxy" | "com.epicgames.launcher" | "uplay")
}

/// Whether the Play button's link starts the game. GOG Galaxy's
/// (`goggalaxy://openGameView/…`) only shows the game's page in Galaxy,
/// with Galaxy's own Play button, so nothing has started yet when the
/// desktop takes it.
fn starts_a_game(url: &Url) -> bool {
    launcher_link(url.scheme()) && url.scheme() != "goggalaxy"
}

/// What Steam sets for a program it starts from Big Picture or a Deck's
/// Gaming mode: `SteamGamepadUI` in both, and `SteamTenfoot` in Big
/// Picture, older versions of which set only that. The device test saw both
/// in Big Picture on Linux.
const STEAM_GAMING_MODE: [&str; 2] = ["SteamGamepadUI", "SteamTenfoot"];

/// What Steam sets on a Deck for anything it starts, in Gaming mode and,
/// it is believed, in Desktop mode too. Unverified until the Deck test.
const STEAM_DECK: &str = "SteamDeck";

/// Whether `env` (the process's environment) has `key` set to `1`, as Steam
/// sets its flags.
fn set(env: &impl Fn(&str) -> Option<std::ffi::OsString>, key: &str) -> bool {
    env(key).is_some_and(|v| v == "1")
}

/// Whether Steam started UpNext from Big Picture or a Deck's Gaming mode.
fn under_steams_big_screen(env: &impl Fn(&str) -> Option<std::ffi::OsString>) -> bool {
    STEAM_GAMING_MODE.iter().any(|k| set(env, k))
}

/// Whether the window opens in big screen because of how Steam started it:
/// from Big Picture or a Deck's Gaming mode, or anywhere on a Deck, whose
/// Desktop mode is still a handheld's screen.
pub(crate) fn steam_asks_big_screen(env: impl Fn(&str) -> Option<std::ffi::OsString>) -> bool {
    under_steams_big_screen(&env) || set(&env, STEAM_DECK)
}

/// How long a window that closes when a game starts waits first, so the
/// page's request recording the launch reaches the server, and the launcher
/// has the link, before the window and any server inside it stop.
const LAUNCH_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// Whether the window closes after it handed `to` to the desktop, which
/// took it when `opened`, with `env` the process's environment. Only a
/// Play button's link that starts the game, and only in a window Steam
/// started from Big Picture or a Deck's Gaming mode: Steam counts UpNext, a
/// non-Steam shortcut there, as the game that is running, so a game started
/// from inside it leaves Big Picture waiting on UpNext after that game
/// closes. When the window ends, Steam sees the shortcut finish and shows
/// the game. A Steam game and another launcher's are alike, since Steam
/// waits on UpNext either way. `SteamDeck` alone doesn't count, since a
/// Deck's Desktop mode has no Big Picture to give back to. A link the
/// desktop couldn't open leaves the window as it was, to try again; web
/// pages and the window started any other way never close it.
pub(crate) fn closes_after_launch(to: &str, opened: bool, env: impl Fn(&str) -> Option<std::ffi::OsString>) -> bool {
    let plays = Url::parse(to).is_ok_and(|url| starts_a_game(&url));
    opened && plays && under_steams_big_screen(&env)
}

/// Hand `url` to the desktop; whether it took it. On Linux that means
/// `xdg-open` started, not that a launcher answered.
fn open_outside(url: &str) -> bool {
    open::that_detached(url).inspect_err(|e| tracing::warn!("couldn't open {url}: {e}")).is_ok()
}

/// The window's icon on Windows: the exe's own, which `build.rs` adds as
/// resource 1, so the title bar and the taskbar show UpNext's.
/// Elsewhere none: on Linux the desktop file names the icon, by the app id.
pub(crate) fn window_icon() -> Option<tao::window::Icon> {
    #[cfg(windows)]
    {
        use tao::platform::windows::IconExtWindows;
        tao::window::Icon::from_resource(1, None).inspect_err(|e| tracing::warn!("no window icon: {e}")).ok()
    }
    #[cfg(not(windows))]
    None
}

/// The folder a window's webview keeps its data in (cookies, local storage,
/// its cache): `window` under `webview` in `Config::webview_cache`, UpNext's
/// cache folder (`%LOCALAPPDATA%\upnext\cache\webview\app` on Windows)
/// unless only the data folder was moved. Each window
/// that runs in a process of its own has its own folder, since WebView2
/// refuses to share one between processes started with other options.
pub(crate) fn webview_dir(cache: &Path, window: &str) -> PathBuf {
    cache.join("webview").join(window)
}

/// The webview's context, with its data in `dir` on Windows. Without one,
/// WebView2 makes `upnext-app.exe.WebView2` beside the program, so from a
/// folder the user can't write to (Program Files) the window never opens
///. WebKitGTK keeps its own folders as before, so on Linux what the
/// pages stored stays where it was.
pub(crate) fn web_context(dir: PathBuf) -> WebContext {
    if cfg!(windows) {
        // WebView2 makes the folder, but not always its parents.
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::warn!("couldn't make the window's data folder {}: {e}", dir.display());
        }
        WebContext::new(Some(dir))
    } else {
        WebContext::new(None)
    }
}

/// The event loop. It owns the main thread from here on, and it's made
/// before the server so the server can tell it when it stops.
pub(crate) fn event_loop() -> anyhow::Result<EventLoop<AppEvent>> {
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none() {
            anyhow::bail!(
                "no desktop to open a window on (neither WAYLAND_DISPLAY nor DISPLAY is set). `upnext serve` runs the web UI without one"
            );
        }
        // GTK names the window after the program, and Wayland passes that on
        // as the app id. Set, so a renamed or wrapped binary is still UpNext.
        glib::set_prgname(Some(window_app_id(crate::flatpak::get())));
        glib::set_application_name("UpNext");
        if screen_readers_shut_out(|k| std::env::var_os(k)) {
            tracing::warn!("NO_AT_BRIDGE=1 is set, so screen readers can't read this window: GTK leaves out its accessibility bridge");
        }
    }
    Ok(EventLoopBuilder::<AppEvent>::with_user_event().build())
}

/// Whether `env` (the process's environment) keeps screen readers out of
/// the window: `NO_AT_BRIDGE=1` stops GTK loading its AT-SPI bridge, as
/// AT-SPI reads it (the number at its start is 1). The window's own process
/// then never joins the accessibility bus, while its web process still
/// does, so a screen reader gets the page's events with no program to tie
/// them to and says nothing. That is what Orca did with the window on a
/// test computer. With the bridge, every event names UpNext
/// and the page's whole tree hangs under the window.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn screen_readers_shut_out(env: impl Fn(&str) -> Option<std::ffi::OsString>) -> bool {
    env("NO_AT_BRIDGE").is_some_and(|v| {
        let v = v.to_string_lossy();
        let v = v.trim_start();
        let v = v.strip_prefix('+').unwrap_or(v);
        let digits: String = v.chars().take_while(char::is_ascii_digit).collect();
        digits.parse::<u64>() == Ok(1)
    })
}

/// Open the window on `app` and run it until it closes. `stop`, when the
/// server runs inside the app, stops that server; the process then waits for
/// it (`AppEvent::ServerStopped`) before it quits, so a running job can
/// record where it got to. The window opens as `remembered` says, and on
/// closing (or Ctrl+Q) saves its size, maximized state and zoom to `server`
/// when they changed, while the server is still there to take them. The
/// webview keeps its data in `data` (`webview_dir`).
///
/// With a tray icon (`background`, on Windows), closing the window can leave
/// the server running instead (`keeps_running`): the window and its webview
/// go, and the icon opens a new one. Quit in its menu ends it all.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run(
    event_loop: EventLoop<AppEvent>,
    target: Target,
    places: Places,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    remembered: Remembered,
    server: impl ServerSettings + 'static,
    data: PathBuf,
    background: Background,
) -> anyhow::Result<()> {
    // Before the webview first asks for gamepads.
    #[cfg(target_os = "linux")]
    crate::pad_layouts::add();
    let proxy = event_loop.create_proxy();
    let mut tray = if background.tray {
        tray::Tray::new(proxy.clone(), background.start_with_windows).inspect_err(|e| tracing::warn!("no tray icon: {e:#}")).ok()
    } else {
        None
    };
    let mut context = web_context(data);
    let mut remembered = remembered;
    let mut target = target;
    // Without a tray icon to open it from, the window opens whatever was asked.
    let mut shown = if background.hidden && tray.is_some() {
        None
    } else {
        Some(Shown::open(&event_loop, &target, &mut context, remembered, background.big_screen, &proxy)?)
    };

    let mut stop = stop;
    let mut failed = false;
    let mut closing = false;
    event_loop.run(move |event, window_target, flow| {
        *flow = ControlFlow::Wait;
        // Some(true) when Quit was chosen in the tray's menu.
        let mut close = None;
        // Where the page asked the window to go instead.
        let mut switch_to: Option<anyhow::Result<Target>> = None;
        match event {
            Event::WindowEvent { event: WindowEvent::Focused(focused), .. } => {
                if let Some(shown) = &mut shown {
                    if focused {
                        let _ = shown.webview.evaluate_script(FOCUS_SCRIPT);
                    }
                    shown.focused = focused;
                    shown.tell_state();
                }
            }
            Event::WindowEvent { event: WindowEvent::Resized(_), .. } => {
                if let Some(shown) = &mut shown {
                    shown.resized();
                    shown.tell_state();
                }
            }
            // Not one for a window already closed to the tray.
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. }
            | Event::UserEvent(AppEvent::Key(Key::Quit) | AppEvent::Window(WindowAction::Close)) => {
                close = shown.is_some().then_some(false);
            }
            Event::UserEvent(AppEvent::Window(action)) => {
                if let Some(shown) = &mut shown {
                    shown.act(action, failed);
                }
            }
            Event::UserEvent(AppEvent::Load(finished)) => {
                if let Some(shown) = &mut shown {
                    shown.loading(finished, &proxy);
                }
            }
            Event::UserEvent(AppEvent::FrameCheck(load)) => {
                if let Some(shown) = &mut shown {
                    shown.check_frame(load);
                }
            }
            Event::UserEvent(AppEvent::Key(Key::Fullscreen)) => {
                if let Some(shown) = shown.as_mut().filter(|_| !failed) {
                    shown.toggle_fullscreen();
                }
            }
            Event::UserEvent(AppEvent::BigScreen(on)) => {
                if let Some(shown) = shown.as_mut().filter(|_| !failed) {
                    shown.follow_big_screen(on);
                }
            }
            Event::UserEvent(AppEvent::Key(Key::Reload)) => {
                // Not the page saying the server stopped: that would go blank.
                if let Some(shown) = shown.as_ref().filter(|_| !failed) {
                    let _ = shown.webview.reload();
                }
            }
            Event::UserEvent(AppEvent::Key(key)) => {
                if let Some(shown) = &mut shown {
                    shown.zoom = zoomed(shown.zoom, key);
                    let _ = shown.webview.zoom(f64::from(shown.zoom) / 100.0);
                }
            }
            Event::UserEvent(AppEvent::Tray(TrayAction::Open)) => match &shown {
                Some(shown) => shown.bring_up(),
                None if !closing => match Shown::open(window_target, &target, &mut context, remembered, background.big_screen, &proxy) {
                    Ok(opened) => shown = Some(opened),
                    Err(e) => message_box(&format!("UpNext's window couldn't open: {e:#}")),
                },
                None => {}
            },
            // Another computer's address is checked first: its token goes
            // only to the UpNext that gave it.
            Event::UserEvent(AppEvent::Connect(Some(id))) => check_then_open(&places, id, proxy.clone()),
            Event::UserEvent(AppEvent::Connect(None)) => switch_to = Some(connect_to(&places, None)),
            Event::UserEvent(AppEvent::Reached(id, reach)) => {
                if let Some(c) = when_reached(&places.local, &id, &reach) {
                    tracing::warn!("the computer chosen ({id}) isn't at its address as itself ({reach:?})");
                    switch_to = Some(Ok(c));
                } else {
                    switch_to = Some(connect_to(&places, Some(&id)));
                }
            }
            Event::UserEvent(AppEvent::Attention) => match &shown {
                Some(shown) => shown.window.request_user_attention(Some(tao::window::UserAttentionType::Informational)),
                // Closed to the tray: the window opens, with the dialog in it.
                None if tray.is_some() && !closing => {
                    shown = Shown::open(window_target, &target, &mut context, remembered, background.big_screen, &proxy).ok();
                }
                None => {}
            },
            Event::UserEvent(AppEvent::Tray(TrayAction::Refresh)) => {
                if let (Some(tray), Some(settings)) = (&tray, server.get()) {
                    tray.show_start_with_windows(starts_with_windows(&settings));
                }
            }
            Event::UserEvent(AppEvent::Tray(TrayAction::StartWithWindows)) => {
                if let Some(tray) = &tray {
                    let on = tray.start_with_windows();
                    let page = || {
                        if let Some(shown) = &shown {
                            let _ = shown.webview.evaluate_script(FOCUS_SCRIPT);
                        }
                    };
                    if let Err(e) = start_with_windows_from_tray(on, &server, page) {
                        tray.show_start_with_windows(!on);
                        message_box(&format!("UpNext couldn't change Start with Windows: {e}"));
                    }
                }
            }
            Event::UserEvent(AppEvent::Tray(TrayAction::Quit)) => close = Some(true),
            Event::UserEvent(AppEvent::HandedOut { url, opened }) => {
                if !closing && closes_after_launch(&url, opened, |k| std::env::var_os(k)) {
                    println!("A game started under Steam: UpNext closes so Steam shows the game");
                    let proxy = proxy.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(LAUNCH_WAIT);
                        let _ = proxy.send_event(AppEvent::GameStarted);
                    });
                }
            }
            // As Quit: the process ends, even with a tray icon, since Steam
            // waits on the process, not the window.
            Event::UserEvent(AppEvent::GameStarted) => close = Some(true),
            Event::UserEvent(AppEvent::ServerStopped(error)) => match error {
                Some(e) => {
                    eprintln!("UpNext's server stopped: {e}");
                    // With a window it says so there: the app started from the
                    // Start menu has no console. One closed to the tray opens again.
                    let page = error_page(&format!("UpNext's server stopped: {e}"), START_AGAIN);
                    let ours = stop.take().is_some();
                    if ours && shown.is_none() {
                        shown = Shown::open(window_target, &target, &mut context, remembered, background.big_screen, &proxy).ok();
                    }
                    if ours && shown.as_ref().is_some_and(|shown| shown.webview.load_html(&page).is_ok()) {
                        failed = true;
                        // The error page draws no title bar: the system's comes back.
                        if let Some(shown) = &mut shown {
                            shown.frame(true);
                        }
                    } else {
                        *flow = ControlFlow::ExitWithCode(1);
                    }
                }
                None => *flow = ControlFlow::Exit,
            },
            _ => {}
        }
        if let Some(next) = switch_to {
            match next {
                Ok(next) if !failed && !closing => {
                    // A new webview for the other UpNext: the old one's
                    // handlers know only the address it opened on.
                    if let Some(old) = shown.take() {
                        remembered = old.hide();
                    }
                    match Shown::open(window_target, &next, &mut context, remembered, background.big_screen, &proxy) {
                        Ok(opened) => {
                            shown = Some(opened);
                            target = next;
                        }
                        Err(e) => {
                            message_box(&format!("UpNext's window couldn't open: {e:#}"));
                            shown = Shown::open(window_target, &target, &mut context, remembered, background.big_screen, &proxy).ok();
                        }
                    }
                }
                Ok(_) => {}
                Err(e) => message_box(&format!("UpNext couldn't open that computer's library: {e:#}")),
            }
        }
        if let Some(quit) = close
            && !closing
        {
            if let Some(now) = shown.as_ref().map(Shown::hide) {
                if !failed
                    && now != remembered
                    && let Err(e) = server.put(&now.to_settings())
                {
                    tracing::warn!("couldn't save the window's size: {e}");
                }
                remembered = now;
            }
            let settings = if tray.is_some() && !quit { server.get() } else { None };
            if keeps_running(tray.is_some(), quit, failed, settings.as_ref()) {
                // The window and its webview go; the tray icon opens a new one.
                shown = None;
            } else {
                // The process goes when the server has stopped.
                closing = true;
                match stop.take() {
                    Some(stop) => {
                        let _ = stop.send(());
                    }
                    None => *flow = if failed { ControlFlow::ExitWithCode(1) } else { ControlFlow::Exit },
                }
            }
        }
        // The icon goes with the process, not after it: Windows leaves an
        // icon its program didn't remove until the pointer passes over it.
        if matches!(*flow, ControlFlow::Exit | ControlFlow::ExitWithCode(_)) {
            tray = None;
            if let Some(shown) = &shown {
                shown.end_web_process();
            }
        }
    })
}

/// The window while it's open, and its webview.
struct Shown {
    // Dropped before the window it's in.
    webview: WebView,
    window: Window,
    /// Percent.
    zoom: u32,
    /// The size it was last at, not maximized: what it opens at next time
    /// after closing maximized.
    size: (u32, u32),
    /// Whether the window has the system's title bar and frame: false while
    /// the UI draws its own (`FRAMELESS`).
    decorated: bool,
    /// How many pages have started loading in it, so a `FrameCheck` for one
    /// that has since been replaced is ignored.
    loads: u64,
    /// Whether the page now in it said it drew its title bar.
    framed: bool,
    focused: bool,
    /// Whether it was maximized when it last wasn't full screen: what it
    /// opens as next time, after closing full screen.
    maximized: bool,
    /// The state the title bar was last told, so it's told only when that
    /// changes.
    told: Option<WindowState>,
    /// Whether it's full screen because big screen is on (opened with
    /// `--big-screen`, or the page went into big screen), rather than by
    /// F11: only then does leaving big screen leave full screen.
    full_for_big: bool,
}

impl Shown {
    /// Open the window on `to`, as `remembered` says.
    fn open(
        target: &EventLoopWindowTarget<AppEvent>,
        to: &Target,
        context: &mut WebContext,
        remembered: Remembered,
        big_screen: bool,
        proxy: &EventLoopProxy<AppEvent>,
    ) -> anyhow::Result<Self> {
        let logical = |m: tao::monitor::MonitorHandle| {
            let size = m.size().to_logical::<u32>(m.scale_factor());
            (size.width, size.height)
        };
        let screen = open_screen(target.primary_monitor().map(logical), target.available_monitors().map(logical));
        let (width, height) = remembered.open_size(screen);
        let builder = WindowBuilder::new()
            .with_title("UpNext")
            .with_window_icon(window_icon())
            .with_inner_size(LogicalSize::new(width, height))
            .with_min_inner_size(LogicalSize::new(MIN_SIZE.0, MIN_SIZE.1))
            .with_maximized(remembered.maximized)
            .with_decorations(!FRAMELESS)
            // On the monitor it opens on; F11 gives back the window as it was.
            .with_fullscreen(big_screen.then_some(Fullscreen::Borderless(None)));
        // Windows keeps the shadow, and with it the resize borders on the
        // left, right and bottom and snapping the window to the screen's
        // edges; the UI makes the top edge (`web/src/lib/frame.ts`).
        #[cfg(windows)]
        let builder = {
            use tao::platform::windows::WindowBuilderExtWindows;
            builder.with_undecorated_shadow(true)
        };
        let window = builder.build(target)?;

        let here = to.app.clone();
        let messages = proxy.clone();
        let sender = to.app.clone();
        let loads = proxy.clone();
        let (handed, handed_new) = (proxy.clone(), proxy.clone());
        let mut builder = WebViewBuilder::new_with_web_context(context)
            .with_url(to.first.as_str())
            // The zoom keys are `KEYS_SCRIPT`'s on every platform, so the zoom
            // can be remembered; this turns WebView2's own off.
            .with_hotkeys_zoom(false)
            .with_initialization_script(APP_SCRIPT)
            .with_initialization_script(KEYS_SCRIPT)
            .with_ipc_handler(move |message| {
                // Only UpNext's own pages work the window: not a frame from
                // another site inside one, which the navigation handler doesn't
                // see on WebView2.
                if !from_app(&message.uri().to_string(), &sender) {
                    return;
                }
                let event = match Message::parse(message.body()) {
                    Some(Message::Key(key)) => AppEvent::Key(key),
                    Some(Message::Window(action)) => AppEvent::Window(action),
                    Some(Message::Connect(to)) => AppEvent::Connect(to),
                    Some(Message::BigScreen(on)) => AppEvent::BigScreen(on),
                    None => return,
                };
                let _ = messages.send_event(event);
            })
            .with_on_page_load_handler(move |event, _| {
                let _ = loads.send_event(AppEvent::Load(matches!(event, PageLoadEvent::Finished)));
            })
            .with_navigation_handler(move |to| match route(&to, &here) {
                Route::Stay => true,
                Route::System => {
                    let opened = open_outside(&to);
                    let _ = handed.send_event(AppEvent::HandedOut { url: to, opened });
                    false
                }
                Route::Block => false,
            })
            .with_new_window_req_handler(move |to, _| {
                if route_new_window(&to) == Route::System {
                    let opened = open_outside(&to);
                    let _ = handed_new.send_event(AppEvent::HandedOut { url: to, opened });
                }
                NewWindowResponse::Deny
            })
            // Downloads go to the browser (the navigation handler sends them there
            // first; this catches any that start anyway).
            .with_download_started_handler(|to, _| {
                if route_new_window(&to) == Route::System {
                    open_outside(&to);
                }
                false
            });
        if FRAMELESS {
            builder = builder.with_initialization_script(FRAME_SCRIPT);
        }
        if big_screen {
            builder = builder.with_initialization_script(BIG_SCRIPT);
        }
        if let Some(name) = &to.remote {
            builder = builder.with_initialization_script(remote_script(name));
        } else if let Some(text) = NOTICE.get() {
            builder = builder.with_initialization_script(crate::build_id::notice_script(text));
        }
        if let Some(headers) = to.headers() {
            builder = builder.with_headers(headers);
        }

        #[cfg(any(target_os = "windows", target_os = "macos"))]
        let webview = builder.build(&window)?;
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let webview = {
            use tao::platform::unix::WindowExtUnix;
            use wry::WebViewBuilderExtUnix;
            let vbox = window.default_vbox().ok_or_else(|| anyhow::anyhow!("no GTK container for the webview"))?;
            builder.build_gtk(vbox)?
        };
        if remembered.zoom != 100 {
            let _ = webview.zoom(f64::from(remembered.zoom) / 100.0);
        }
        let focused = window.is_focused();
        Ok(Self {
            webview,
            window,
            zoom: remembered.zoom,
            size: (width, height),
            decorated: !FRAMELESS,
            loads: 0,
            framed: false,
            focused,
            maximized: remembered.maximized,
            told: None,
            full_for_big: big_screen,
        })
    }

    /// Do what the title bar asked. Nothing once the server has failed: the
    /// window then has the system's frame, and the page is the error's.
    fn act(&mut self, action: WindowAction, failed: bool) {
        if failed {
            return;
        }
        match action {
            WindowAction::Framed => {
                self.framed = true;
                self.frame(false);
                // A new page knows nothing yet.
                self.told = None;
                self.tell_state();
            }
            WindowAction::Drag => {
                let _ = self.window.drag_window();
            }
            WindowAction::Minimize => self.window.set_minimized(true),
            WindowAction::ToggleMaximize => self.window.set_maximized(!self.window.is_maximized()),
            WindowAction::Resize(direction) => {
                if !self.window.is_maximized() {
                    let _ = self.window.drag_resize_window(direction);
                }
            }
            // The event loop closes the window, as for the system's close button.
            WindowAction::Close => {}
        }
    }

    /// Give the window the system's title bar and frame (`true`), or take
    /// them away while the UI draws its own. Never off where the window
    /// isn't `FRAMELESS`.
    fn frame(&mut self, decorated: bool) {
        let decorated = decorated || !FRAMELESS;
        if decorated != self.decorated {
            self.window.set_decorations(decorated);
            self.decorated = decorated;
        }
    }

    /// A page started loading, or finished: it has `FRAME_WAIT` to say it
    /// drew its title bar.
    fn loading(&mut self, finished: bool, proxy: &EventLoopProxy<AppEvent>) {
        if !FRAMELESS {
            return;
        }
        if !finished {
            self.loads += 1;
            self.framed = false;
        }
        let (load, proxy) = (self.loads, proxy.clone());
        let wait = if finished { FRAME_WAIT } else { FRAME_WAIT_LONG };
        std::thread::spawn(move || {
            std::thread::sleep(wait);
            let _ = proxy.send_event(AppEvent::FrameCheck(load));
        });
    }

    /// The page that was the `load`th hasn't said it drew a title bar: the
    /// system's comes back, so the window can still be moved and closed.
    fn check_frame(&mut self, load: u64) {
        if load == self.loads && !self.framed {
            self.frame(true);
        }
    }

    /// Tell the title bar the window's state, when it has changed.
    fn tell_state(&mut self) {
        if !self.framed {
            return;
        }
        let now = WindowState { maximized: self.window.is_maximized(), focused: self.focused, fullscreen: self.fullscreen() };
        if self.told != Some(now) {
            self.told = Some(now);
            let _ = self.webview.evaluate_script(&window_state_script(now));
        }
    }

    fn fullscreen(&self) -> bool {
        self.window.fullscreen().is_some()
    }

    /// F11: full screen on the monitor the window is on, or back.
    fn toggle_fullscreen(&mut self) {
        let to = if self.fullscreen() { None } else { Some(Fullscreen::Borderless(self.window.current_monitor())) };
        self.window.set_fullscreen(to);
        // F11 takes it over: leaving big screen leaves it as it is.
        self.full_for_big = false;
        self.tell_state();
    }

    /// The page went into big screen, or left it: full screen with it, as
    /// F11 does, and back to the window as it was after.
    fn follow_big_screen(&mut self, on: bool) {
        let Some(full) = fullscreen_for_big_screen(on, self.fullscreen(), self.full_for_big) else {
            return;
        };
        let to = full.then(|| Fullscreen::Borderless(self.window.current_monitor()));
        self.window.set_fullscreen(to);
        self.full_for_big = full;
        self.tell_state();
    }

    /// Its size and maximized state change: kept for next time, except
    /// while it's full screen, which isn't the window's own size.
    fn resized(&mut self) {
        if self.fullscreen() {
            return;
        }
        self.maximized = self.window.is_maximized();
        if !self.maximized && !self.window.is_minimized() {
            self.size = logical_size(&self.window);
        }
    }

    /// Show it in front, from the tray.
    fn bring_up(&self) {
        self.window.set_visible(true);
        self.window.set_minimized(false);
        self.window.set_focus();
    }

    /// End the webview's web process as the app quits, on Linux. Left to
    /// itself, WebKitGTK's web process notices the app has gone and exits
    /// through its exit handlers, which tear down the graphics driver, and
    /// with Mesa that teardown once crashed and dumped core as UpNext closed
    /// under Steam's Big Picture. Ended here, it runs none of
    /// them. The web process keeps nothing that needs saving: cookies and
    /// the pages' storage are WebKit's network process's, and the UI saves
    /// nothing as it unloads. After a close the window is already hidden;
    /// when the server stopped by itself or the app exits on an error it is
    /// still on screen for the moment before the process ends.
    fn end_web_process(&self) {
        #[cfg(target_os = "linux")]
        {
            use webkit2gtk::WebViewExt;
            use wry::WebViewExtUnix;
            self.webview.webview().terminate_web_process();
        }
    }

    /// Hide it at once, before its settings are saved or the server stops;
    /// how it was, to save.
    fn hide(&self) -> Remembered {
        // Full screen, it's remembered as it was before.
        let windowed = !self.fullscreen();
        let maximized = if windowed { self.window.is_maximized() } else { self.maximized };
        let size = if windowed && !maximized && !self.window.is_minimized() { logical_size(&self.window) } else { self.size };
        self.window.set_visible(false);
        Remembered { size: Some(size), maximized, zoom: self.zoom }
    }
}

/// The icon in the notification area, with its menu: Open UpNext, Start with
/// Windows and Quit. Windows only; Linux keeps the systemd
/// service, and closing the window there closes it.
#[cfg(windows)]
mod tray {
    use super::{AppEvent, TrayAction};
    use tao::event_loop::EventLoopProxy;
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

    pub(crate) struct Tray {
        _icon: TrayIcon,
        start_with_windows: CheckMenuItem,
    }

    impl Tray {
        /// Add the icon. Clicks on it and choices in its menu come to the
        /// event loop as `AppEvent::Tray`.
        pub(crate) fn new(proxy: EventLoopProxy<AppEvent>, start_with_windows: bool) -> anyhow::Result<Self> {
            let [(open, _), (start, _), (quit, _)] = TrayAction::MENU;
            let open = MenuItem::with_id(open, "Open UpNext", true, None);
            let start = CheckMenuItem::with_id(start, "Start with Windows", true, start_with_windows, None);
            let quit = MenuItem::with_id(quit, "Quit", true, None);
            let menu = Menu::with_items(&[&open, &PredefinedMenuItem::separator(), &start, &PredefinedMenuItem::separator(), &quit])?;
            // The exe's own icon, resource 1, as the window's.
            let icon = tray_icon::Icon::from_resource(1, None)?;
            let built = TrayIconBuilder::new()
                .with_tooltip("UpNext")
                .with_icon(icon)
                .with_menu(Box::new(menu))
                // A left click opens the window; the menu is the right click's.
                .with_menu_on_left_click(false)
                .build()?;

            let menu_events = proxy.clone();
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                if let Some(action) = TrayAction::from_menu(e.id.as_ref()) {
                    let _ = menu_events.send_event(AppEvent::Tray(action));
                }
            }));
            // The menu opens when the right button comes up, so the press is
            // the moment to bring its checkmark up to date.
            TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
                let action = match e {
                    TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } => TrayAction::Open,
                    TrayIconEvent::Click { button: MouseButton::Right, button_state: MouseButtonState::Down, .. } => TrayAction::Refresh,
                    _ => return,
                };
                let _ = proxy.send_event(AppEvent::Tray(action));
            }));
            Ok(Self { _icon: built, start_with_windows: start })
        }

        /// Whether Start with Windows has its checkmark. The menu turns it
        /// over itself when it's chosen.
        pub(crate) fn start_with_windows(&self) -> bool {
            self.start_with_windows.is_checked()
        }

        pub(crate) fn show_start_with_windows(&self, on: bool) {
            self.start_with_windows.set_checked(on);
        }
    }
}

/// No tray elsewhere: `Background::tray` is never set off Windows.
#[cfg(not(windows))]
mod tray {
    use super::AppEvent;
    use tao::event_loop::EventLoopProxy;

    pub(crate) struct Tray;

    impl Tray {
        pub(crate) fn new(_proxy: EventLoopProxy<AppEvent>, _start_with_windows: bool) -> anyhow::Result<Self> {
            anyhow::bail!("the tray icon is for Windows")
        }

        pub(crate) fn start_with_windows(&self) -> bool {
            false
        }

        pub(crate) fn show_start_with_windows(&self, _on: bool) {}
    }
}

/// The window's size in logical pixels, as it's set when it opens.
fn logical_size(window: &tao::window::Window) -> (u32, u32) {
    // GTK draws its own title bar and shadow on Wayland (and asks for room
    // for them), and tao's size counts them, so saving that would grow the
    // window each time it opens. GTK's own size leaves them out.
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::GtkWindowExt;
        use tao::platform::unix::WindowExtUnix;
        let (w, h) = window.gtk_window().size();
        (w.max(0) as u32, h.max(0) as u32)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let size = window.inner_size().to_logical::<f64>(window.scale_factor());
        (size.width.round() as u32, size.height.round() as u32)
    }
}

/// What the error windows advise when nothing better is known.
pub(crate) const START_AGAIN: &str = "Close this window and start UpNext again. `upnext doctor` in a terminal checks the setup.";

/// A window that says why UpNext couldn't start and what to do (`advice`),
/// for the app with no console (`upnext-app`), until it's closed. Its
/// webview keeps its data in `data`. Comes back only when the window can't
/// be made.
pub(crate) fn show_error(event_loop: EventLoop<AppEvent>, message: &str, advice: &str, data: PathBuf) -> anyhow::Result<()> {
    let window = WindowBuilder::new()
        .with_title("UpNext")
        .with_window_icon(window_icon())
        .with_inner_size(LogicalSize::new(640.0, 360.0))
        .build(&event_loop)?;
    let mut context = web_context(data);
    let builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_html(error_page(&format!("UpNext couldn't start: {message}"), advice))
        .with_navigation_handler(|to| to.starts_with("about:") || to.starts_with("data:"));
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let webview = builder.build(&window)?;
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window.default_vbox().ok_or_else(|| anyhow::anyhow!("no GTK container for the webview"))?;
        builder.build_gtk(vbox)?
    };
    event_loop.run(move |event, _, flow| {
        let _ = (&webview, &context);
        *flow = ControlFlow::Wait;
        if let Event::WindowEvent { event: WindowEvent::CloseRequested, .. } = event {
            *flow = ControlFlow::ExitWithCode(1);
        }
    })
}

/// A page with `message` on it, and `advice` under it, in the window's
/// light or dark.
fn error_page(message: &str, advice: &str) -> String {
    let escape = |text: &str| text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!(
        r#"<!doctype html><meta charset="utf-8"><title>UpNext</title>
<style>:root{{color-scheme:light dark;font:15px/1.5 system-ui,sans-serif}}body{{margin:2rem}}pre{{white-space:pre-wrap;font:inherit}}</style>
<h1 style="font-size:1.2rem">Something went wrong</h1><pre>{}</pre>
<p>{}</p>"#,
        escape(message),
        escape(advice)
    )
}

/// A message box with `text`, for when not even the error window opens
/// (WebView2 missing or broken, or no folder it can write to): the app has
/// no console, so on Windows this is the one place left to say why. Elsewhere
/// the error goes to the terminal, as it always has.
pub(crate) fn message_box(text: &str) {
    #[cfg(windows)]
    {
        use rfd::{MessageButtons, MessageDialog, MessageLevel};
        let _ = MessageDialog::new()
            .set_level(MessageLevel::Error)
            .set_title("UpNext")
            .set_description(text)
            .set_buttons(MessageButtons::Ok)
            .show();
    }
    #[cfg(not(windows))]
    let _ = text;
}

/// Microsoft's download page for the WebView2 Runtime, whose Evergreen
/// Bootstrapper installs it for the whole PC and keeps it up to date.
pub(crate) const WEBVIEW2_PAGE: &str = "https://developer.microsoft.com/microsoft-edge/webview2/";

/// Whether the WebView2 Runtime, which draws every UpNext window on Windows,
/// is missing. Windows 11 includes it and the installer carries no copy
///, so this is a PC where it was taken out; nothing else can be
/// shown there but a message box. Elsewhere it's never missing.
pub(crate) fn webview2_missing() -> bool {
    #[cfg(windows)]
    return wry::webview_version().is_err();
    #[cfg(not(windows))]
    false
}

/// What the message box says when WebView2 is missing, with the error that
/// stopped UpNext last, for a bug report.
fn webview2_missing_text(error: &str) -> String {
    format!(
        "UpNext couldn't start: its window needs Microsoft's WebView2 Runtime, which isn't installed on this PC.\n\n\
         Install it with the Evergreen Bootstrapper from {WEBVIEW2_PAGE}, then start UpNext again. Open that page now?\n\n\
         ({error})"
    )
}

/// The message box for a missing WebView2, in place of `message_box`: Yes
/// opens Microsoft's page in the browser.
pub(crate) fn webview2_missing_box(error: &str) {
    let text = webview2_missing_text(error);
    #[cfg(windows)]
    {
        use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
        let answer = MessageDialog::new()
            .set_level(MessageLevel::Error)
            .set_title("UpNext")
            .set_description(text)
            .set_buttons(MessageButtons::YesNo)
            .show();
        if answer == MessageDialogResult::Yes {
            open_outside(WEBVIEW2_PAGE);
        }
    }
    #[cfg(not(windows))]
    let _ = text;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flatpaks_window_is_named_after_its_launcher_entry() {
        assert_eq!(window_app_id(None), "upnext");
        let sandbox = crate::flatpak::Flatpak::example();
        assert_eq!(window_app_id(Some(&sandbox)), "io.github.zytocc.UpNext");
    }

    #[test]
    fn the_ui_stays_and_everything_else_goes_to_the_desktop() {
        let app = Url::parse("http://127.0.0.1:8787/").unwrap();
        for stay in
            ["http://127.0.0.1:8787/", "http://127.0.0.1:8787/library?view=table", "http://127.0.0.1:8787/settings#data", "about:blank"]
        {
            assert_eq!(route(stay, &app), Route::Stay, "{stay}");
        }
        for browser in [
            // Other sites.
            "https://store.steampowered.com/app/620/",
            "https://www.youtube.com/watch?v=abc",
            "https://howlongtobeat.com/game/123",
            "mailto:someone@example.com",
            // Play buttons.
            "steam://run/620",
            "heroic://launch/legendary/Fortnite",
            "goggalaxy://openGameView/gog_1207658924",
            "com.epicgames.launcher://apps/ns%3Aitem%3AFortnite?action=launch&silent=true",
            "uplay://launch/635/0",
            // The server's links that aren't pages: the Steam sign-in, downloads.
            "http://127.0.0.1:8787/api/steam/login",
            "http://127.0.0.1:8787/api/export",
            "http://127.0.0.1:8787/api/jobs/12/log.txt",
            "http://127.0.0.1:8787/api/backups/upnext-2026-10-02.db/download?keys=true",
            // Another UpNext, or this one by another name, is another site.
            "http://127.0.0.1:9999/",
            "http://localhost:8787/",
            "https://127.0.0.1:8787/",
        ] {
            assert_eq!(route(browser, &app), Route::System, "{browser}");
        }
        // Schemes that only look like a launcher's are blocked too.
        for block in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,hi",
            "not a url",
            "epicgames://launch/Fortnite",
            "com.epicgames.launcher.evil://apps/x",
            "uplayx://launch/635/0",
            "steam2://run/620",
        ] {
            assert_eq!(route(block, &app), Route::Block, "{block}");
        }
        // A page whose name starts like the API's is still a page.
        assert_eq!(route("http://127.0.0.1:8787/apis", &app), Route::Stay);
    }

    #[test]
    fn the_zoom_keys_step_like_a_browser_and_stop_at_the_ends() {
        assert_eq!(zoomed(100, Key::ZoomIn), 110);
        assert_eq!(zoomed(110, Key::ZoomIn), 125);
        assert_eq!(zoomed(100, Key::ZoomOut), 90);
        assert_eq!(zoomed(67, Key::ZoomOut), 50);
        assert_eq!(zoomed(50, Key::ZoomOut), 50);
        assert_eq!(zoomed(300, Key::ZoomIn), 300);
        // A zoom set by hand between the steps goes to the next one.
        assert_eq!(zoomed(105, Key::ZoomIn), 110);
        assert_eq!(zoomed(105, Key::ZoomOut), 100);
        assert_eq!(zoomed(30, Key::ZoomOut), 30);
        assert_eq!(zoomed(175, Key::ZoomReset), 100);
        assert_eq!(zoomed(175, Key::Reload), 175);
        assert_eq!(zoomed(175, Key::Fullscreen), 175);
    }

    #[test]
    fn only_the_keys_script_messages_are_keys() {
        for (message, key) in [
            ("quit", Key::Quit),
            ("reload", Key::Reload),
            ("fullscreen", Key::Fullscreen),
            ("zoom-in", Key::ZoomIn),
            ("zoom-out", Key::ZoomOut),
            ("zoom-reset", Key::ZoomReset),
        ] {
            assert_eq!(Key::parse(message), Some(key));
            assert!(KEYS_SCRIPT.contains(&format!("'{message}'")), "the script sends {message}");
        }
        assert_eq!(Key::parse("Quit"), None);
        assert_eq!(Key::parse("eval:alert(1)"), None);
    }

    #[test]
    fn the_title_bar_messages_are_window_actions() {
        for (message, action) in [
            ("window:framed", WindowAction::Framed),
            ("window:drag", WindowAction::Drag),
            ("window:minimize", WindowAction::Minimize),
            ("window:maximize", WindowAction::ToggleMaximize),
            ("window:close", WindowAction::Close),
            ("window:resize:n", WindowAction::Resize(ResizeDirection::North)),
            ("window:resize:ne", WindowAction::Resize(ResizeDirection::NorthEast)),
            ("window:resize:e", WindowAction::Resize(ResizeDirection::East)),
            ("window:resize:se", WindowAction::Resize(ResizeDirection::SouthEast)),
            ("window:resize:s", WindowAction::Resize(ResizeDirection::South)),
            ("window:resize:sw", WindowAction::Resize(ResizeDirection::SouthWest)),
            ("window:resize:w", WindowAction::Resize(ResizeDirection::West)),
            ("window:resize:nw", WindowAction::Resize(ResizeDirection::NorthWest)),
        ] {
            assert_eq!(Message::parse(message), Some(Message::Window(action)), "{message}");
        }
        // The keys still come through as keys.
        assert_eq!(Message::parse("quit"), Some(Message::Key(Key::Quit)));
        assert_eq!(Message::parse("zoom-in"), Some(Message::Key(Key::ZoomIn)));
        for nothing in
            ["window:", "window:Close", "window:resize:", "window:resize:up", "window:resize:n ", "drag", "close", "", "window:eval"]
        {
            assert_eq!(Message::parse(nothing), None, "{nothing}");
        }
    }

    #[test]
    fn the_page_can_open_another_computer_or_this_one() {
        assert_eq!(Message::parse("connect:here"), Some(Message::Connect(None)));
        assert_eq!(Message::parse("connect:4f2a9c01d3e85b67"), Some(Message::Connect(Some("4f2a9c01d3e85b67".into()))));
        for nothing in ["connect:", "connect:../x", "connect:a b", &format!("connect:{}", "a".repeat(65))] {
            assert_eq!(Message::parse(nothing), None, "{nothing}");
        }
    }

    #[test]
    fn another_computer_opens_with_its_token_on_the_first_page_only() {
        let den = crate::computers::Computer {
            id: "4f2a9c01d3e85b67".into(),
            name: "den \"main\"".into(),
            address: "http://192.168.0.42:8787".into(),
            token: "upnext_abc".into(),
            added_at: 1,
        };
        let t = Target::on(&den).unwrap();
        assert_eq!(t.app.as_str(), "http://192.168.0.42:8787/");
        assert_eq!(t.first.as_str(), "http://192.168.0.42:8787/api/v1/pair/open");
        assert_eq!(t.headers().unwrap()[wry::http::header::AUTHORIZATION], "Bearer upnext_abc");
        assert_eq!(remote_script(&den.name), r#"window.upnextRemote = { name: "den \"main\"" }"#);
        let here = Target::here("http://127.0.0.1:8787/".parse().unwrap(), "connect");
        assert_eq!(here.first.as_str(), "http://127.0.0.1:8787/connect");
        assert!(here.headers().is_none() && here.remote.is_none());

        // Choosing one saves it as the one the window opens on; this
        // computer's again forgets that, not the computer.
        let dir = tempfile::tempdir().unwrap();
        let places = Places { local: "http://127.0.0.1:8787/".parse().unwrap(), config_dir: dir.path().to_path_buf() };
        let mut kept = crate::computers::Computers::default();
        kept.add(den.clone());
        kept.save(dir.path()).unwrap();
        assert_eq!(connect_to(&places, Some(&den.id)).unwrap(), t);
        assert_eq!(crate::computers::Computers::load(dir.path()).unwrap().open.as_deref(), Some(den.id.as_str()));
        assert_eq!(connect_to(&places, None).unwrap().app.as_str(), "http://127.0.0.1:8787/");
        let back = crate::computers::Computers::load(dir.path()).unwrap();
        assert_eq!((back.open, back.computers.len()), (None, 1));
        assert!(connect_to(&places, Some("gone")).is_err());

        // Checked first: where it goes when the address answers otherwise.
        use crate::computers::Reach;
        assert_eq!(when_reached(&places.local, &den.id, &Reach::Same), None);
        let moved = when_reached(&places.local, &den.id, &Reach::Other("0123456789abcdef".into())).unwrap();
        assert_eq!(moved.first.as_str(), format!("http://127.0.0.1:8787/connect?changed={}", den.id));
        assert!(moved.token.is_none(), "no token for another UpNext");
        let away = when_reached(&places.local, &den.id, &Reach::Away("refused".into())).unwrap();
        assert_eq!(away.first.as_str(), format!("http://127.0.0.1:8787/connect?unreachable={}", den.id));
        assert!(!format!("{t:?}").contains("upnext_abc"), "no token in a log");
    }

    #[test]
    fn only_the_apps_own_pages_work_the_window() {
        let app = Url::parse("http://127.0.0.1:8787/").unwrap();
        for ours in ["http://127.0.0.1:8787/", "http://127.0.0.1:8787/library?view=table", "http://127.0.0.1:8787/settings#data"] {
            assert!(from_app(ours, &app), "{ours}");
        }
        // Another site in a frame, another UpNext, this one by another name,
        // and what isn't a page of a site at all.
        for other in [
            "https://www.youtube.com/embed/abc",
            "http://127.0.0.1:9999/",
            "http://localhost:8787/",
            "https://127.0.0.1:8787/",
            "about:blank",
            "data:text/html,hi",
            "",
        ] {
            assert!(!from_app(other, &app), "{other}");
        }
    }

    #[test]
    fn the_title_bar_is_told_the_window_state() {
        let script = window_state_script(WindowState { maximized: true, focused: false, fullscreen: false });
        assert_eq!(
            script,
            "window.dispatchEvent(new CustomEvent('upnext:window', { detail: { maximized: true, focused: false, fullscreen: false } }))"
        );
        let full = window_state_script(WindowState { maximized: false, focused: true, fullscreen: true });
        assert!(full.contains("fullscreen: true"), "{full}");
        // A window opened in big screen says so to the page.
        assert_eq!(BIG_SCRIPT, "window.upnextBigScreen = true");
        // The page reads which system it's on, to make the resize edges.
        assert!(FRAME_SCRIPT.starts_with("window.upnextFrame = { os: '"), "{FRAME_SCRIPT}");
        #[cfg(any(windows, target_os = "linux"))]
        assert!(FRAME_SCRIPT.contains(std::env::consts::OS), "{FRAME_SCRIPT}");
    }

    #[test]
    fn the_window_remembers_its_settings() {
        // Never closed yet, or a server from before these settings.
        assert_eq!(Remembered::from_settings(&json!({ "theme": "arcade" })), Remembered::default());
        let saved = Remembered { size: Some((1440, 900)), maximized: true, zoom: 125 };
        let body = saved.to_settings();
        assert_eq!(body, json!({ "window_size": "1440x900", "window_maximized": true, "window_zoom": 125 }));
        // GET /api/settings answers with the same names and kinds.
        assert_eq!(Remembered::from_settings(&body), saved);
        let never_sized = Remembered { size: None, maximized: false, zoom: 90 };
        assert_eq!(never_sized.to_settings(), json!({ "window_maximized": false, "window_zoom": 90 }));
        // Nonsense stays at the default.
        let odd = Remembered::from_settings(&json!({ "window_size": "big", "window_zoom": 5000, "window_maximized": "yes" }));
        assert_eq!(odd, Remembered::default());
    }

    #[test]
    fn the_tray_menu_items_are_actions() {
        assert_eq!(TrayAction::from_menu("open"), Some(TrayAction::Open));
        assert_eq!(TrayAction::from_menu("start-with-windows"), Some(TrayAction::StartWithWindows));
        assert_eq!(TrayAction::from_menu("quit"), Some(TrayAction::Quit));
        assert_eq!(TrayAction::from_menu("Quit"), None);
        assert_eq!(TrayAction::from_menu(""), None);
    }

    #[test]
    fn closing_the_window_keeps_the_server_only_with_a_tray_and_the_setting() {
        let on = json!({ "keep_running": true });
        let off = json!({ "keep_running": false });
        // An UpNext from before the setting: its default, on.
        let older = json!({ "window_zoom": 100 });
        assert!(keeps_running(true, false, false, Some(&on)));
        assert!(keeps_running(true, false, false, Some(&older)));
        assert!(!keeps_running(true, false, false, Some(&off)));
        // No tray (Linux, or a window on another UpNext's server): close means close.
        assert!(!keeps_running(false, false, false, Some(&on)));
        // Quit from the tray, a server that failed, or one that doesn't answer.
        assert!(!keeps_running(true, true, false, Some(&on)));
        assert!(!keeps_running(true, false, true, Some(&on)));
        assert!(!keeps_running(true, false, false, None));
    }

    /// A server that keeps what it was sent, and refuses when told to.
    struct Sent(std::cell::RefCell<Vec<Value>>, bool);

    impl ServerSettings for Sent {
        fn get(&self) -> Option<Value> {
            None
        }
        fn put(&self, body: &Value) -> Result<(), String> {
            self.0.borrow_mut().push(body.clone());
            if self.1 { Err("refused".into()) } else { Ok(()) }
        }
    }

    #[test]
    fn the_tray_tells_the_page_once_the_server_has_start_with_windows() {
        let server = Sent(Default::default(), false);
        let mut told = 0;
        start_with_windows_from_tray(true, &server, || told += 1).unwrap();
        assert_eq!(*server.0.borrow(), [json!({ "start_with_windows": true })]);
        assert_eq!(told, 1);
        // Refused: the page isn't told, and the tray puts its check back.
        let refusing = Sent(Default::default(), true);
        let mut told = 0;
        assert_eq!(start_with_windows_from_tray(false, &refusing, || told += 1), Err("refused".into()));
        assert_eq!(told, 0);
    }

    #[test]
    fn the_checkmark_follows_the_setting() {
        assert!(starts_with_windows(&json!({ "start_with_windows": true })));
        assert!(!starts_with_windows(&json!({ "start_with_windows": false })));
        assert!(!starts_with_windows(&json!({ "theme": "arcade" })));
    }

    #[test]
    fn the_window_opens_at_its_size_within_the_screen() {
        assert_eq!(Remembered::default().open_size(None), FIRST_SIZE);
        let saved = Remembered { size: Some((1920, 1200)), ..Remembered::default() };
        assert_eq!(saved.open_size(None), (1920, 1200));
        assert_eq!(saved.open_size(Some((2560, 1440))), (1920, 1200));
        // Saved on a bigger screen.
        assert_eq!(saved.open_size(Some((1366, 768))), (1366, 768));
        let tiny = Remembered { size: Some((300, 200)), ..Remembered::default() };
        assert_eq!(tiny.open_size(Some((1366, 768))), MIN_SIZE);
    }

    #[test]
    fn the_window_is_kept_within_the_primary_screen_or_else_the_largest() {
        let portrait = (720, 1280);
        let wide = (2560, 1440);
        let laptop = (1536, 960);
        // Wayland: no primary, and a portrait screen first in the list.
        assert_eq!(open_screen(None, [portrait, wide, laptop]), Some(wide));
        let saved = Remembered { size: Some((1920, 1080)), ..Remembered::default() };
        assert_eq!(saved.open_size(open_screen(None, [portrait, wide])), (1920, 1080));
        // The first of two the same size.
        assert_eq!(open_screen(None, [(1920, 1080), (1080, 1920)]), Some((1920, 1080)));
        // Where the system names a primary screen, the window opens on it.
        assert_eq!(open_screen(Some(laptop), [portrait, wide, laptop]), Some(laptop));
        // A screen that's gone isn't listed, so the size fits one that's there.
        assert_eq!(saved.open_size(open_screen(None, [laptop])), (1536, 960));
        assert_eq!(open_screen(None, []), None);
    }

    #[test]
    fn a_missing_webview2_says_where_to_get_it() {
        let text = webview2_missing_text("WebView2 error: class not registered");
        assert!(text.contains("WebView2 Runtime, which isn't installed"), "{text}");
        assert!(text.contains(WEBVIEW2_PAGE), "{text}");
        assert!(text.ends_with("(WebView2 error: class not registered)"), "{text}");
        // Off Windows the system's webview is a package the app depends on.
        #[cfg(not(windows))]
        assert!(!webview2_missing());
    }

    #[test]
    fn error_pages_show_the_message_as_text() {
        let page = error_page("UpNext couldn't start: <script>alert(1)</script> & more", START_AGAIN);
        assert!(page.contains("&lt;script&gt;alert(1)&lt;/script&gt; &amp; more"), "{page}");
        assert!(!page.contains("<script>"));
        assert!(page.contains("<p>Close this window and start UpNext again."), "{page}");
        let advice = error_page("UPNEXT_PORT=\"abc\"", "Fix or remove UPNEXT_PORT in C:\\Users\\you\\<odd>\\.env");
        assert!(advice.contains("<p>Fix or remove UPNEXT_PORT in C:\\Users\\you\\&lt;odd&gt;\\.env</p>"), "{advice}");
    }

    /// The webview's data goes under UpNext's cache folder, one folder per
    /// window, never beside the program.
    #[test]
    fn the_webview_keeps_its_data_in_the_cache_folder() {
        let cache = Path::new("/home/you/.cache/upnext");
        assert_eq!(webview_dir(cache, "app"), cache.join("webview").join("app"));
        assert_eq!(webview_dir(cache, "gog"), cache.join("webview").join("gog"));
        assert!(webview_dir(cache, "app").starts_with(cache));
    }

    /// WebView2 is told the folder, and makes it there.
    #[cfg(windows)]
    #[test]
    fn webview2_gets_the_data_folder() {
        let cache = tempfile::tempdir().unwrap();
        let dir = webview_dir(cache.path(), "app");
        let context = web_context(dir.clone());
        assert_eq!(context.data_directory(), Some(dir.as_path()));
        assert!(dir.is_dir());
    }

    #[test]
    fn no_at_bridge_set_to_one_shuts_screen_readers_out() {
        let env = |value: Option<&'static str>| move |k: &str| (k == "NO_AT_BRIDGE").then_some(value).flatten().map(Into::into);
        for one in ["1", " 1", "+1", "01", "1 "] {
            assert!(screen_readers_shut_out(env(Some(one))), "{one:?}");
        }
        for not in [None, Some(""), Some("0"), Some("2"), Some("yes"), Some("-1"), Some("10")] {
            assert!(!screen_readers_shut_out(env(not)), "{not:?}");
        }
    }

    #[test]
    fn a_game_started_under_steams_big_screen_closes_the_window() {
        let env =
            |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.into());
        let big_picture = &[("SteamGamepadUI", "1"), ("SteamTenfoot", "1"), ("SteamClientLaunch", "1")];
        // Steam's games and other launchers' alike: Steam waits on UpNext either way.
        for play in [
            "steam://run/620",
            "heroic://launch/legendary/Fortnite",
            "com.epicgames.launcher://apps/ns%3Aitem%3AFortnite?action=launch&silent=true",
            "uplay://launch/635/0",
        ] {
            assert!(closes_after_launch(play, true, env(big_picture)), "{play}");
            // A link the desktop couldn't open leaves the window to try again.
            assert!(!closes_after_launch(play, false, env(big_picture)), "{play}");
            // Outside Steam's big-screen modes nothing changes.
            assert!(!closes_after_launch(play, true, env(&[])), "{play}");
        }
        // Galaxy's link only shows the game's page there, so nothing has started.
        assert!(!closes_after_launch("goggalaxy://openGameView/gog_1207658924", true, env(big_picture)));
        // Either of Big Picture's variables is enough; its older versions set only SteamTenfoot.
        for one in [&[("SteamGamepadUI", "1")], &[("SteamTenfoot", "1")]] {
            assert!(closes_after_launch("steam://run/620", true, env(one)), "{one:?}");
        }
        // SteamDeck alone may be a Deck's Desktop mode, with no Big Picture to give back to.
        assert!(!closes_after_launch("steam://run/620", true, env(&[("SteamDeck", "1")])));
        assert!(closes_after_launch("steam://run/620", true, env(&[("SteamDeck", "1"), ("SteamGamepadUI", "1")])));
        // Steam's desktop client sets these for anything it starts; the window stays there.
        assert!(!closes_after_launch("steam://run/620", true, env(&[("SteamClientLaunch", "1"), ("SteamAppId", "4235876455")])));
        assert!(!closes_after_launch("steam://run/620", true, env(&[("SteamDeck", "0"), ("SteamGamepadUI", "")])));
        // Web pages, mail and the server's own links never close it.
        for other in ["https://store.steampowered.com/app/620/", "mailto:a@b.example", "http://127.0.0.1:8787/api/export", "not a url"] {
            assert!(!closes_after_launch(other, true, env(big_picture)), "{other}");
        }
    }

    #[test]
    fn steam_turns_big_screen_on_from_its_own_big_screen_modes() {
        let env =
            |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.into());
        // Every variable that closes the window after a launch also opens it in big screen.
        for k in STEAM_GAMING_MODE {
            assert!(steam_asks_big_screen(|n: &str| (n == k).then(|| "1".into())), "{k}");
        }
        // On a Deck, even in Desktop mode, big screen without closing.
        assert!(steam_asks_big_screen(env(&[("SteamDeck", "1")])));
        assert!(!steam_asks_big_screen(env(&[])));
        assert!(!steam_asks_big_screen(env(&[("SteamDeck", "0"), ("SteamGamepadUI", ""), ("SteamTenfoot", "0")])));
        // Steam's other variables, set for anything it starts, don't count.
        assert!(!steam_asks_big_screen(env(&[("SteamAppId", "1"), ("SteamClientLaunch", "1")])));
    }

    #[test]
    fn the_window_fills_the_screen_in_big_screen_and_goes_back_after() {
        assert_eq!(Message::parse("big-screen:on"), Some(Message::BigScreen(true)));
        assert_eq!(Message::parse("big-screen:off"), Some(Message::BigScreen(false)));
        assert_eq!(Message::parse("big-screen:"), None);
        assert_eq!(Message::parse("big-screen:yes"), None);
        // Going into big screen in a window: full screen.
        assert_eq!(fullscreen_for_big_screen(true, false, false), Some(true));
        // Leaving it: back to the window.
        assert_eq!(fullscreen_for_big_screen(false, true, true), Some(false));
        // Already full screen, by F11 or by big screen: stays so.
        assert_eq!(fullscreen_for_big_screen(true, true, false), None);
        assert_eq!(fullscreen_for_big_screen(true, true, true), None);
        // F11's full screen stays when big screen is left, and a window
        // already windowed stays so.
        assert_eq!(fullscreen_for_big_screen(false, true, false), None);
        assert_eq!(fullscreen_for_big_screen(false, false, false), None);
    }

    #[test]
    fn new_windows_open_on_the_desktop() {
        for browser in
            ["https://store.steampowered.com/app/620/", "http://127.0.0.1:8787/api/export", "mailto:a@b.example", "steam://run/620"]
        {
            assert_eq!(route_new_window(browser), Route::System, "{browser}");
        }
        for block in ["file:///etc/passwd", "javascript:alert(1)", "blob:http://127.0.0.1:8787/x", "nope"] {
            assert_eq!(route_new_window(block), Route::Block, "{block}");
        }
    }
}
