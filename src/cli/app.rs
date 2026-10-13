//! `app`: UpNext in its own window, and `install-desktop`, which puts it in
//! the desktop's launcher.
//!
//! The window shows the UpNext already running on the port (the background
//! service) when there is one. Otherwise it runs the server itself, in this
//! process, until the window closes. It doesn't always run its own: the
//! service keeps the daily sync going with no window open, and two servers
//! on one database would both start the scheduled jobs.
//!
//! It can show another computer's UpNext instead: with
//! `--connect` it opens on the connect page, which finds the computers on
//! the network and asks one to allow this one; after that, it opens on the
//! computer chosen there (`computers.json`) until the page is asked for this
//! computer's library again.

use crate::config::Config;
use crate::desktop::{self, AppEvent};
use anyhow::{Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// What answers on the port.
#[derive(Debug, PartialEq)]
pub(crate) enum Probe {
    /// UpNext, with its version and build (the short commit, when it says).
    UpNext { version: String, build: Option<String> },
    /// Something else: another program, or an UpNext from before
    /// `/api/health`, which can't be told apart from one.
    Other,
    /// Nothing.
    Nobody,
}

/// Straight to loopback, whatever proxy the environment names.
fn loopback_client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(2)).build()
}

/// Ask 127.0.0.1:`port` what it is. Only `/api/health` answering
/// `{"app": "upnext"}` counts as UpNext.
pub(crate) async fn probe(port: u16) -> Probe {
    // Nothing listening is told apart first. A listener on loopback accepts
    // at once, but Windows refuses a connection to a closed port only after
    // about 2 s of retries, past the HTTP timeout below (which would read as
    // a program that doesn't answer), and on Windows the app finds nobody
    // there on every start. So a connection not made in half a second means
    // nobody; if something does hold the port, `listen` takes another.
    match tokio::time::timeout(Duration::from_millis(500), tokio::net::TcpStream::connect(("127.0.0.1", port))).await {
        Ok(Err(_)) | Err(_) => return Probe::Nobody,
        Ok(Ok(_)) => {}
    }
    let Ok(http) = loopback_client() else {
        return Probe::Nobody;
    };
    let health = async { http.get(format!("http://127.0.0.1:{port}/api/health")).send().await?.error_for_status()?.json::<Value>().await };
    match health.await {
        Ok(v) if v["app"] == "upnext" => Probe::UpNext {
            version: v["version"].as_str().unwrap_or("unknown").to_string(),
            build: v["build"].as_str().map(str::to_string),
        },
        Err(e) if e.is_connect() => Probe::Nobody,
        _ => Probe::Other,
    }
}

/// The port for the server inside the app: `port` on loopback, or any free
/// one when another program holds it.
pub(crate) async fn listen(port: u16) -> Result<tokio::net::TcpListener> {
    match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
        Ok(l) => Ok(l),
        Err(e) => {
            let l = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.context("finding a free port")?;
            let free = l.local_addr()?.port();
            tracing::warn!("port {port} is taken by another program ({e}); this window's UpNext uses {free}");
            Ok(l)
        }
    }
}

/// Open the window: on the UpNext that answers on the port, or on one
/// started here that stops with the window. Doesn't return until it closes.
/// `big_screen` (`--big-screen`, or Steam's own launch, `desktop::steam_asks_big_screen`)
/// opens it full screen in big screen.
/// `connect` (`--connect`) opens on the page that uses another computer's
/// UpNext.
pub fn run(cfg: Config, port: Option<u16>, big_screen: bool, connect: bool) -> Result<()> {
    let event_loop = desktop::event_loop()?;
    let started = start(&event_loop, cfg, port, false, big_screen || desktop::steam_asks_big_screen(|k| std::env::var_os(k)), connect)?;
    started.open(event_loop)
}

/// `upnext-app`, the app with no console: the GOG login window when
/// the server inside it asks for one (it starts this same program), else the
/// app window on the configured port. With `--background`, which the Run key
/// passes at sign-in (`autostart`), its server starts with only the tray
/// icon on Windows, and nothing happens when UpNext is already running. With
/// no console to print to, an error that stops it is shown in a window
/// instead, or, when not even a window opens, in a message box on Windows;
/// either way it's also written to `upnext-app-error.txt` in the temp folder.
pub fn desktop_main() -> std::process::ExitCode {
    // As `upnext` does first: inside a Flatpak, start again without a
    // host's certificate file the sandbox can't read.
    crate::restart_without_unreadable_cert_env();
    let failed = |e: anyhow::Error| {
        let text = format!("{e:#}");
        eprintln!("{text}");
        let _ = std::fs::write(std::env::temp_dir().join("upnext-app-error.txt"), format!("UpNext couldn't start: {text}\n"));
        // Without WebView2 no window opens, whatever stopped UpNext, so
        // that's what to fix first.
        if desktop::webview2_missing() {
            desktop::webview2_missing_box(&text);
        } else {
            desktop::message_box(&format!("UpNext couldn't start: {text}\n\n{}", advice(&e)));
        }
        std::process::ExitCode::FAILURE
    };
    let first = std::env::args().nth(1);
    if first.as_deref() == Some("gog-window") {
        return crate::run_gog_window().map_or_else(failed, |()| std::process::ExitCode::SUCCESS);
    }
    let background = first.as_deref() == Some(crate::autostart::BACKGROUND_FLAG);
    let big_screen =
        std::env::args().skip(1).any(|a| a == crate::autostart::BIG_SCREEN_FLAG) || desktop::steam_asks_big_screen(|k| std::env::var_os(k));
    let connect = std::env::args().skip(1).any(|a| a == CONNECT_FLAG);
    super::init_logging();
    let event_loop = match desktop::event_loop() {
        Ok(l) => l,
        Err(e) => return failed(e),
    };
    let cfg = Config::load();
    // The error window needs a folder for its webview too, even when the
    // settings didn't load.
    let cache = cfg.as_ref().map_or_else(|_| Config::webview_cache_unloaded(|k| std::env::var(k).ok()), Config::webview_cache);
    let started = cfg.and_then(|cfg| {
        if let Some(why) = super::migrate::unmoved_data(&cfg) {
            anyhow::bail!(why);
        }
        start(&event_loop, cfg, None, background, big_screen, connect)
    });
    // Both only come back when the window couldn't be made.
    let shown = match started {
        Ok(started) => started.open(event_loop),
        Err(e) => desktop::show_error(event_loop, &format!("{e:#}"), &advice(&e), desktop::webview_dir(&cache, "app")).map_err(|window| {
            // Why UpNext didn't start matters more than why its window didn't open.
            eprintln!("the error window couldn't open: {window:#}");
            e
        }),
    };
    shown.map_or_else(failed, |()| std::process::ExitCode::SUCCESS)
}

/// What the error window and message box advise for `e`: where to fix a
/// setting that doesn't parse, since starting again can't help with that
///; otherwise to start again.
fn advice(e: &anyhow::Error) -> String {
    match e.downcast_ref::<crate::config::BadSetting>() {
        Some(bad) => bad.advice(),
        None => desktop::START_AGAIN.to_string(),
    }
}

/// `upnext-app --connect`, as `upnext app --connect`.
const CONNECT_FLAG: &str = "--connect";

/// What the window opens on, and the server it runs, if it runs one.
struct Started {
    url: reqwest::Url,
    /// Opened with `--connect`: on the connect page.
    connect: bool,
    /// Where `computers.json` is kept.
    config_dir: PathBuf,
    /// Started with `--background`: no window until the tray icon opens one.
    hidden: bool,
    /// Started with `--big-screen`: full screen, in big screen.
    big_screen: bool,
    /// Where its webview keeps its data (`desktop::webview_dir`).
    data: PathBuf,
    /// Stops the server inside the app.
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    /// The server's runtime, kept for as long as the window is open.
    rt: tokio::runtime::Runtime,
}

impl Started {
    /// Open the window, as it was when it last closed, and save how it was
    /// when it closes again. Doesn't return until it closes.
    fn open(self, event_loop: tao::event_loop::EventLoop<AppEvent>) -> Result<()> {
        let own = self.stop.is_some();
        if self.hidden && !own {
            // Started at sign-in while another UpNext already runs.
            println!("UpNext is already running; nothing to start");
            return Ok(());
        }
        let server = Server { http: loopback_client()?, settings: self.url.join("api/settings")?, rt: self.rt.handle().clone() };
        let first = self.rt.block_on(server.fetch());
        let remembered = remembered(&server, &first);
        // The tray is for a server of the app's own: another one (`upnext
        // serve`, or an UpNext started earlier) stays up without it. A
        // big-screen window has none either: a launcher (Steam, the Xbox app)
        // started it like a game, so closing it ends it, and the launcher
        // sees it end.
        let tray = cfg!(windows) && own && !self.big_screen;
        let start_with_windows = first.as_ref().ok().is_some_and(desktop::starts_with_windows);
        if tray && first.is_ok() {
            // The Run key follows the setting, naming this program as it is
            // now, so an install that moved still starts at sign-in. Another
            // copy's value is left alone (`autostart::change`).
            if let Err(e) = crate::autostart::set(start_with_windows, crate::autostart::Why::Starting) {
                tracing::warn!("Start with Windows: {e:#}");
            }
        }
        let background = desktop::Background { tray, hidden: self.hidden, start_with_windows, big_screen: self.big_screen };
        let target = self.rt.block_on(first_target(&self.url, &self.config_dir, self.connect));
        let places = desktop::Places { local: self.url, config_dir: self.config_dir };
        let shown = desktop::run(event_loop, target, places, self.stop, remembered, server, self.data, background);
        drop(self.rt);
        shown
    }
}

/// Where the window opens: the connect page with `--connect`; else the
/// other computer chosen there, when it answers; else this computer's
/// UpNext. When the chosen computer doesn't answer, the connect page says so
/// and offers it again or this computer's library.
async fn first_target(local: &reqwest::Url, config_dir: &Path, connect: bool) -> desktop::Target {
    if connect {
        return desktop::Target::here(local.clone(), "connect");
    }
    let kept = crate::computers::Computers::load(config_dir).inspect_err(|e| tracing::warn!("{e:#}")).unwrap_or_default();
    let Some(c) = kept.opened() else {
        return desktop::Target::here(local.clone(), "/");
    };
    // Checked with no token: the token goes only to the UpNext that gave it.
    let reach = crate::computers::reach(c).await;
    if let Some(instead) = desktop::when_reached(local, &c.id, &reach) {
        tracing::warn!("{} at {} didn't answer as itself ({reach:?}): opening this computer's", c.name, c.address);
        return instead;
    }
    match desktop::Target::on(c) {
        Ok(target) => {
            println!("Opening the window on {}'s UpNext, at {}", c.name, c.address);
            target
        }
        Err(e) => {
            tracing::warn!("{}'s address {} doesn't work: {e:#}", c.name, c.address);
            desktop::Target::here(local.clone(), &format!("connect?unreachable={}", c.id))
        }
    }
}

/// The settings of the server the window shows, over HTTP.
struct Server {
    http: reqwest::Client,
    /// Its `/api/settings`.
    settings: reqwest::Url,
    rt: tokio::runtime::Handle,
}

impl Server {
    async fn fetch(&self) -> reqwest::Result<Value> {
        self.http.get(self.settings.clone()).send().await?.error_for_status()?.json::<Value>().await
    }

    async fn send(&self, body: &Value) -> std::result::Result<(), String> {
        let res = self.http.put(self.settings.clone()).json(body).send().await.map_err(|e| e.to_string())?;
        if res.status().is_success() {
            return Ok(());
        }
        // An UpNext from before a setting answers 400, with why.
        let status = res.status();
        let body: Value = res.json().await.unwrap_or_default();
        Err(body["error"].as_str().map_or_else(|| format!("{} answered {status}", self.settings), str::to_string))
    }
}

/// From the event loop, which isn't in the runtime: inside it, since
/// reqwest's timeout needs it.
impl desktop::ServerSettings for Server {
    fn get(&self) -> Option<Value> {
        self.rt.block_on(self.fetch()).inspect_err(|e| tracing::warn!("{} didn't answer: {e}", self.settings)).ok()
    }

    fn put(&self, body: &Value) -> std::result::Result<(), String> {
        self.rt.block_on(self.send(body))
    }
}

/// What the window remembers, from the server it shows (`GET
/// /api/settings`); the defaults when that didn't answer in time.
fn remembered(server: &Server, first: &reqwest::Result<Value>) -> desktop::Remembered {
    match first {
        Ok(v) => desktop::Remembered::from_settings(v),
        Err(e) => {
            tracing::warn!("the window opens at its first size: {} didn't answer ({e})", server.settings);
            desktop::Remembered::default()
        }
    }
}

/// Find the UpNext on the port, or start one inside the app.
fn start(
    event_loop: &tao::event_loop::EventLoop<AppEvent>,
    mut cfg: Config,
    port: Option<u16>,
    hidden: bool,
    big_screen: bool,
    connect: bool,
) -> Result<Started> {
    let port = port.unwrap_or(cfg.port);
    let config_dir = cfg.config_dir.clone();
    let data = desktop::webview_dir(&cfg.webview_cache(), "app");
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let (url, stop) = match rt.block_on(probe(port)) {
        Probe::UpNext { version, build } => {
            // Another build is warned about, not refused: a second server
            // on the same library would run its scheduled jobs twice.
            if let Some(text) = crate::build_id::mismatch(port, &version, build.as_deref()) {
                eprintln!("{text}");
                desktop::notice_on_local(text);
            }
            println!("UpNext is running on port {port}; opening a window on it");
            (format!("http://127.0.0.1:{port}/"), None)
        }
        found => {
            if found == Probe::Other {
                eprintln!("{}", other_program(port));
            }
            let listener = rt.block_on(listen(port))?;
            let addr = listener.local_addr()?;
            cfg.host = addr.ip().to_string();
            cfg.port = addr.port();
            let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
            let proxy = event_loop.create_proxy();
            // A device asking to be allowed gets the person's attention.
            let attention = std::sync::Mutex::new(event_loop.create_proxy());
            let on_request: crate::server::approval::OnRequest = std::sync::Arc::new(move || {
                let _ = attention.lock().map(|p| p.send_event(AppEvent::Attention));
            });
            rt.spawn(async move {
                let stop = async move {
                    tokio::select! {
                        _ = stopped => {}
                        _ = crate::server::serve::stop_signal() => {}
                    }
                };
                let served = super::serve::serve_in_app(cfg, listener, stop, on_request).await;
                let _ = proxy.send_event(AppEvent::ServerStopped(served.err().map(|e| format!("{e:#}"))));
            });
            println!("UpNext runs in this window, at http://{addr}, until it closes");
            (format!("http://{addr}/"), Some(stop))
        }
    };
    Ok(Started { url: url.parse()?, connect, config_dir, hidden, big_screen, data, stop, rt })
}

/// What the app says when the port answers, but not as UpNext.
fn other_program(port: u16) -> String {
    format!(
        "Another program is on port {port}: it answers, but not as UpNext, so this window runs its own UpNext on a free port. \
         If that program is an older UpNext (from before /api/health), stop it and start it again on a current build: \
         two UpNext servers on one library would both run its scheduled jobs."
    )
}

/// `contrib/linux`'s launcher entry and icons, in the binary so
/// `install-desktop` works from any folder.
const DESKTOP_FILE: &str = include_str!("../../contrib/linux/upnext.desktop");
const ICON_SVG: &[u8] = include_bytes!("../../contrib/linux/icons/hicolor/scalable/apps/upnext.svg");
const ICON_PNG: &[u8] = include_bytes!("../../contrib/linux/icons/hicolor/256x256/apps/upnext.png");

/// Where each file goes under a data dir (`~/.local/share`, or
/// `/usr/share` for a package).
fn desktop_files() -> [(PathBuf, &'static [u8]); 3] {
    let icons = Path::new("icons/hicolor");
    [
        (Path::new("applications").join(format!("{}.desktop", desktop::APP_ID)), DESKTOP_FILE.as_bytes()),
        (icons.join("scalable/apps").join(format!("{}.svg", desktop::APP_ID)), ICON_SVG),
        (icons.join("256x256/apps").join(format!("{}.png", desktop::APP_ID)), ICON_PNG),
    ]
}

/// A Flatpak has a launcher entry of its own, which Flatpak installs and
/// removes with the app; one written from inside would land in the sandbox.
fn not_in_flatpak(sandbox: Option<&crate::flatpak::Flatpak>) -> Result<()> {
    match sandbox {
        Some(f) => anyhow::bail!("UpNext runs as a Flatpak here, which puts its own entry ({}) in the launcher", f.id),
        None => Ok(()),
    }
}

/// UpNext's Steam copy has no launcher entry of its own: Steam starts it,
/// through the launcher that sets up its bundled libraries on Linux, and
/// Steam's "Add desktop shortcut" gives it one.
fn not_in_steam_copy(copy: Option<&crate::steam_copy::SteamCopy>) -> Result<()> {
    match copy {
        Some(_) => anyhow::bail!(
            "this is UpNext's Steam copy, which Steam starts: for a launcher entry, use Manage › Add desktop shortcut on its page in Steam"
        ),
        None => Ok(()),
    }
}

/// `install-desktop`: put UpNext in the launcher (`$XDG_DATA_HOME`, else
/// `~/.local/share`), started by this binary; `remove` takes it out again.
pub fn install_desktop(remove: bool) -> Result<()> {
    if !cfg!(target_os = "linux") {
        anyhow::bail!("install-desktop is for Linux desktops");
    }
    not_in_flatpak(crate::flatpak::get())?;
    not_in_steam_copy(crate::steam_copy::get())?;
    let data = directories::BaseDirs::new().context("finding the home folder")?.data_dir().to_path_buf();
    if remove {
        for path in uninstall_from(&data)? {
            println!("Removed {}", path.display());
        }
        return Ok(());
    }
    let exe = std::env::current_exe().context("locating the upnext binary")?;
    for path in install_into(&data, &exe)? {
        println!("Wrote {}", path.display());
    }
    println!("UpNext is in the launcher, under Games. It starts `{} app`.", exe.display());
    Ok(())
}

/// Write the launcher entry and icons under `data`, the entry starting
/// `exe`. The files written.
pub(crate) fn install_into(data: &Path, exe: &Path) -> Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    for (rel, contents) in desktop_files() {
        let path = data.join(rel);
        let parent = path.parent().context("a file with no folder")?;
        std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        let contents = if path.extension().is_some_and(|e| e == "desktop") {
            desktop_entry_for(std::str::from_utf8(contents)?, exe).into_bytes()
        } else {
            contents.to_vec()
        };
        std::fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))?;
        written.push(path);
    }
    Ok(written)
}

/// Remove what `install_into` wrote under `data`. The files removed.
pub(crate) fn uninstall_from(data: &Path) -> Result<Vec<PathBuf>> {
    let mut removed = Vec::new();
    for (rel, _) in desktop_files() {
        let path = data.join(rel);
        match std::fs::remove_file(&path) {
            Ok(()) => removed.push(path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("removing {}", path.display())),
        }
    }
    Ok(removed)
}

/// The packaged entry, `Exec=upnext app` (and the big-screen action's
/// `Exec=upnext app --big-screen`), made to start `exe` instead: a
/// launcher's `PATH` often lacks `~/.cargo/bin`.
fn desktop_entry_for(entry: &str, exe: &Path) -> String {
    let exe = exe.to_string_lossy();
    entry
        .lines()
        .map(|line| {
            if let Some(command) = line.strip_prefix("Exec=") {
                let args = command.split_once(' ').map_or("", |(_, args)| args);
                format!("Exec={} {args}", exec_arg(&exe))
            } else if line.starts_with("TryExec=") {
                format!("TryExec={}", exe.replace('\\', "\\\\"))
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// One argument of a `.desktop` file's `Exec` line, quoted when the
/// specification asks for it.
fn exec_arg(arg: &str) -> String {
    const RESERVED: &[char] = &[' ', '\t', '\n', '"', '\'', '\\', '>', '<', '~', '|', '&', ';', '$', '*', '?', '#', '(', ')', '`'];
    let arg = arg.replace('%', "%%");
    if !arg.contains(RESERVED) {
        return arg;
    }
    let mut quoted = String::from("\"");
    for c in arg.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    // The whole value is a string, whose own escape is a backslash too.
    quoted.replace('\\', "\\\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flatpak_keeps_its_own_launcher_entry() {
        assert!(not_in_flatpak(None).is_ok());
        let sandbox = crate::flatpak::Flatpak::example();
        let e = not_in_flatpak(Some(&sandbox)).unwrap_err().to_string();
        assert_eq!(e, "UpNext runs as a Flatpak here, which puts its own entry (io.github.zytocc.UpNext) in the launcher");
    }

    #[test]
    fn the_steam_copy_leaves_its_launcher_entry_to_steam() {
        assert!(not_in_steam_copy(None).is_ok());
        let e = not_in_steam_copy(Some(&crate::steam_copy::SteamCopy::example())).unwrap_err().to_string();
        assert!(e.starts_with("this is UpNext's Steam copy") && e.contains("Add desktop shortcut"), "{e}");
    }

    /// A port nothing listens on.
    async fn free_port() -> u16 {
        tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port()
    }

    /// Serve `app` on a port of its own; the port.
    async fn serve(app: axum::Router) -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        port
    }

    #[tokio::test]
    async fn probe_tells_upnext_from_other_programs() {
        // Without waiting out Windows' slow refusal of a closed port.
        let asked = std::time::Instant::now();
        assert_eq!(probe(free_port().await).await, Probe::Nobody);
        assert!(asked.elapsed() < Duration::from_millis(1500), "took {:?}", asked.elapsed());

        let other = axum::Router::new().fallback(|| async { "<html>some other program</html>" });
        assert_eq!(probe(serve(other).await).await, Probe::Other);

        let not_json = axum::Router::new().fallback(|| async { (axum::http::StatusCode::NOT_FOUND, "no") });
        assert_eq!(probe(serve(not_json).await).await, Probe::Other);

        // Any JSON from /api/config isn't enough: an UpNext from before
        // /api/health can't be told from another program by it.
        let older = axum::Router::new()
            .route("/api/config", axum::routing::get(|| async { axum::Json(serde_json::json!({ "llm_backend": "claude" })) }))
            .fallback(|| async { axum::response::Html("<!doctype html><title>UpNext</title>") });
        assert_eq!(probe(serve(older).await).await, Probe::Other);
        // Nor is JSON from /api/health that doesn't say UpNext.
        let health =
            axum::Router::new().route("/api/health", axum::routing::get(|| async { axum::Json(serde_json::json!({ "status": "ok" })) }));
        assert_eq!(probe(serve(health).await).await, Probe::Other);
        assert!(other_program(8787).contains("Another program is on port 8787"));
    }

    /// The server `app` runs answers the probe with its version, and stops
    /// when the window closes.
    #[tokio::test]
    async fn the_server_inside_the_app_answers_and_stops_with_the_window() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = crate::testutil::offline_config(dir.path());
        let listener = listen(0).await.unwrap();
        let addr = listener.local_addr().unwrap();
        (cfg.host, cfg.port) = (addr.ip().to_string(), addr.port());
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(super::super::serve::serve_in_app(
            cfg,
            listener,
            async move {
                let _ = stopped.await;
            },
            std::sync::Arc::new(|| {}),
        ));

        let build = crate::build_id::BUILD.map(str::to_string);
        assert_eq!(probe(addr.port()).await, Probe::UpNext { version: env!("CARGO_PKG_VERSION").to_string(), build: build.clone() });
        let health: Value = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{addr}/api/health"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            (&health["app"], &health["version"], &health["container"]),
            (&serde_json::json!("upnext"), &serde_json::json!(env!("CARGO_PKG_VERSION")), &serde_json::json!(false))
        );
        assert_eq!(health["id"].as_str().map(str::len), Some(16), "the install id: {health}");
        assert_eq!(health["build"].as_str(), build.as_deref(), "the build, or null: {health}");

        stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), server).await.expect("the server stopped").unwrap().unwrap();
        assert_eq!(probe(addr.port()).await, Probe::Nobody);
    }

    /// The window's size and zoom go to the server it shows and come back
    /// from it the next time it opens.
    #[tokio::test]
    async fn the_window_remembers_through_the_server() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = crate::testutil::offline_config(dir.path());
        let listener = listen(0).await.unwrap();
        let addr = listener.local_addr().unwrap();
        (cfg.host, cfg.port) = (addr.ip().to_string(), addr.port());
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(super::super::serve::serve_in_app(
            cfg,
            listener,
            async move {
                let _ = stopped.await;
            },
            std::sync::Arc::new(|| {}),
        ));

        let settings = Server {
            http: loopback_client().unwrap(),
            settings: format!("http://{addr}/api/settings").parse().unwrap(),
            rt: tokio::runtime::Handle::current(),
        };
        assert_eq!(remembered(&settings, &settings.fetch().await), desktop::Remembered::default());
        let closed = desktop::Remembered { size: Some((1500, 950)), maximized: true, zoom: 110 };
        settings.send(&closed.to_settings()).await.unwrap();
        assert_eq!(remembered(&settings, &settings.fetch().await), closed);
        // A refusal comes back with the server's reason, for the tray's message.
        let refused = settings.send(&serde_json::json!({ "keep_running": "maybe" })).await.unwrap_err();
        assert_eq!(refused, "keep_running must be true or false");

        stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), server).await.expect("the server stopped").unwrap().unwrap();
        // Nobody there: the first size.
        assert_eq!(remembered(&settings, &settings.fetch().await), desktop::Remembered::default());
    }

    #[tokio::test]
    async fn a_taken_port_means_another_one() {
        let taken = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = taken.local_addr().unwrap().port();
        let listener = listen(port).await.unwrap();
        assert_ne!(listener.local_addr().unwrap().port(), port);
        assert!(listener.local_addr().unwrap().ip().is_loopback());

        let free = free_port().await;
        assert_eq!(listen(free).await.unwrap().local_addr().unwrap().port(), free);
    }

    #[cfg(target_os = "linux")] // install-desktop is for Linux desktops
    #[test]
    fn install_writes_the_entry_and_icons_and_remove_takes_them_out() {
        let data = tempfile::tempdir().unwrap();
        let exe = Path::new("/home/you/.cargo/bin/upnext");
        let written = install_into(data.path(), exe).unwrap();
        let names: Vec<String> = written.iter().map(|p| p.strip_prefix(data.path()).unwrap().display().to_string()).collect();
        assert_eq!(
            names,
            ["applications/upnext.desktop", "icons/hicolor/scalable/apps/upnext.svg", "icons/hicolor/256x256/apps/upnext.png"]
        );

        let entry = std::fs::read_to_string(&written[0]).unwrap();
        assert!(entry.starts_with("[Desktop Entry]\n"), "{entry}");
        assert!(entry.contains("\nExec=/home/you/.cargo/bin/upnext app\n"), "{entry}");
        // The action's line keeps its own arguments.
        assert!(entry.contains("\nExec=/home/you/.cargo/bin/upnext app --big-screen\n"), "{entry}");
        assert!(entry.contains("\n[Desktop Action big-screen]\nName=Big screen\n"), "{entry}");
        assert!(entry.contains("\nTryExec=/home/you/.cargo/bin/upnext\n"), "{entry}");
        assert!(entry.contains("\nIcon=upnext\n"), "{entry}");
        assert!(entry.contains("\nStartupWMClass=upnext\n"), "{entry}");
        assert!(entry.contains("\nCategories=Game;\n"), "{entry}");
        assert_eq!(std::fs::read(&written[1]).unwrap(), ICON_SVG);
        assert_eq!(&std::fs::read(&written[2]).unwrap()[..8], b"\x89PNG\r\n\x1a\n");

        // Again: the same files, rewritten.
        assert_eq!(install_into(data.path(), exe).unwrap(), written);
        assert_eq!(uninstall_from(data.path()).unwrap(), written);
        assert!(written.iter().all(|p| !p.exists()));
        assert!(uninstall_from(data.path()).unwrap().is_empty());
    }

    #[test]
    fn exec_lines_quote_what_the_spec_reserves() {
        assert_eq!(exec_arg("/usr/bin/upnext"), "/usr/bin/upnext");
        assert_eq!(exec_arg("/home/you/My Apps/upnext"), r#""/home/you/My Apps/upnext""#);
        // `$` and `\` are escaped for the Exec line, and that backslash again for the string.
        assert_eq!(exec_arg("/opt/$x/up\\next"), r#""/opt/\\$x/up\\\\next""#);
        assert_eq!(exec_arg("/opt/100%/upnext"), "/opt/100%%/upnext");
    }
}
