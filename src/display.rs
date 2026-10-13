//! Whether the computer UpNext runs on has a desktop a window can open on:
//! GOG's sign-in window (`upnext gog-window`), which the server starts as a
//! child process, and what the setup wizard says.
//!
//! The variables alone aren't enough. A systemd user service starts with
//! whatever the user manager had at login, often before the desktop, so it
//! may have no `WAYLAND_DISPLAY` while the session's socket sits in its own
//! runtime folder; and a variable can outlive the session it named. On Linux
//! the sockets are checked, and a service that lacks the variable is given
//! the session's Wayland socket for the window. A socket counts when it
//! takes a connection, so one a crashed compositor left behind doesn't. A
//! Windows service runs in session 0, which has no desktop the user sees.

use std::ffi::OsString;

/// What [`detect`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Desktop {
    /// A window can open.
    pub available: bool,
    /// Variables the window's process needs that UpNext's own environment
    /// lacks: `WAYLAND_DISPLAY`, for a service started before the desktop.
    pub env: Vec<(&'static str, OsString)>,
}

impl Desktop {
    fn yes() -> Desktop {
        Desktop { available: true, env: vec![] }
    }
}

/// The desktop of the computer UpNext runs on, for a server in `mode`
/// (`Config::run_mode`). Never in a container; always inside `upnext app`,
/// whose own window is open. None when the check was given up: it took
/// longer than `DETECT_WAIT`, or an earlier one that did still hasn't
/// returned.
pub fn detect(container: bool, mode: &str) -> Option<Desktop> {
    if container {
        return Some(Desktop::default());
    }
    if mode == "app" {
        return Some(Desktop::yes());
    }
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
    #[cfg(target_os = "linux")]
    {
        // The whole check is bounded, not only each connect: listing a
        // runtime folder on a hung mount blocks too.
        static STILL_CHECKING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        bounded(&STILL_CHECKING, DETECT_WAIT, move || linux(&var, std::path::Path::new("/tmp/.X11-unix")))
    }
    #[cfg(windows)]
    {
        Some(Desktop { available: windows(&var, mode), env: vec![] })
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = var;
        Some(Desktop::yes())
    }
}

/// How long a detection is kept: the setup page asks every two seconds,
/// and each check may take up to `DETECT_WAIT`.
const KEEP: std::time::Duration = std::time::Duration::from_secs(30);

/// How long a check that was given up is kept as "no desktop": long enough
/// that a page polling every two seconds doesn't wait on each, short enough
/// that a desktop that has just appeared is found soon after.
const KEEP_GIVEN_UP: std::time::Duration = std::time::Duration::from_secs(4);

/// The server's detections, for `detected`.
static DETECTIONS: Detections = Detections::new();

/// The last detection, and the lock that lets one check run at a time.
struct Detections {
    checking: tokio::sync::Mutex<()>,
    last: std::sync::Mutex<Option<Kept>>,
}

/// A detection, when it was made, how long it's kept and for which server.
#[derive(Clone)]
struct Kept {
    at: std::time::Instant,
    keep: std::time::Duration,
    container: bool,
    mode: String,
    desktop: Desktop,
}

/// `detect` for the server's handlers: run off the async threads (it may
/// block for `DETECT_WAIT`), one at a time, and kept for `KEEP`, so a setup
/// page polling every two seconds checks the sockets at most every 30.
pub async fn detected(container: bool, mode: &str) -> Desktop {
    DETECTIONS.get(container, mode, detect).await
}

impl Detections {
    const fn new() -> Detections {
        Detections { checking: tokio::sync::Mutex::const_new(()), last: std::sync::Mutex::new(None) }
    }

    /// The kept detection, or a new one by `check`. Requests that miss
    /// together wait for the one check rather than each starting their own.
    async fn get(&self, container: bool, mode: &str, check: fn(bool, &str) -> Option<Desktop>) -> Desktop {
        let _one = self.checking.lock().await;
        if let Some(kept) = fresh(&self.last.lock().unwrap_or_else(|e| e.into_inner()), std::time::Instant::now(), container, mode) {
            return kept;
        }
        let owned = mode.to_string();
        let found = tokio::task::spawn_blocking(move || check(container, &owned)).await.ok().flatten();
        let keep = if found.is_some() { KEEP } else { KEEP_GIVEN_UP };
        let desktop = found.unwrap_or_default();
        let kept = Kept { at: std::time::Instant::now(), keep, container, mode: mode.to_string(), desktop: desktop.clone() };
        *self.last.lock().unwrap_or_else(|e| e.into_inner()) = Some(kept);
        desktop
    }
}

/// The kept detection, when it's for this server and still kept.
fn fresh(kept: &Option<Kept>, now: std::time::Instant, container: bool, mode: &str) -> Option<Desktop> {
    kept.as_ref()
        .filter(|k| k.container == container && k.mode == mode && now.saturating_duration_since(k.at) < k.keep)
        .map(|k| k.desktop.clone())
}

/// `work`'s answer by `within`, or None when it took longer than `wait` or
/// the work `still` marks from an earlier call hasn't returned yet. So work
/// that hangs leaves one thread behind, not one per call.
#[cfg(target_os = "linux")]
fn bounded<T: Send + 'static>(
    still: &'static std::sync::atomic::AtomicBool,
    wait: std::time::Duration,
    work: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    use std::sync::atomic::Ordering;
    if still.swap(true, Ordering::SeqCst) {
        return None;
    }
    within(wait, None, move || {
        let answer = work();
        still.store(false, Ordering::SeqCst);
        Some(answer)
    })
}

/// `work`'s answer, run on a thread of its own, or `otherwise` when it takes
/// longer than `wait`. Work that hangs leaves its thread behind until it
/// returns: a blocking connect or `read_dir` can't be stopped without a
/// socket crate or `unsafe`, and the check must not hold up `GET /api/setup`.
#[cfg(target_os = "linux")]
fn within<T: Send + 'static>(wait: std::time::Duration, otherwise: T, work: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || tx.send(work()));
    rx.recv_timeout(wait).unwrap_or(otherwise)
}

/// How long the whole Linux check may take: a few sockets at
/// `CONNECT_WAIT` each, and the runtime folder's listing.
#[cfg(target_os = "linux")]
const DETECT_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// At most this many entries of the runtime folder are looked at for a
/// `wayland-N` socket. A session's folder holds a few dozen; the bound keeps
/// a folder full of something else from making the check read it all.
#[cfg(target_os = "linux")]
const RUNTIME_ENTRIES: usize = 4096;

/// Linux: a Wayland socket `WAYLAND_DISPLAY` names (in `XDG_RUNTIME_DIR`
/// unless it's a path), else the X server `DISPLAY` names, else the
/// session's own `wayland-N` socket in `XDG_RUNTIME_DIR`. That folder is the
/// user's own (mode 700), so a socket in it is this user's desktop. An X
/// server is never guessed: it needs `XAUTHORITY` as well, which only the
/// session knows.
#[cfg(target_os = "linux")]
fn linux(var: &dyn Fn(&str) -> Option<OsString>, x11_sockets: &std::path::Path) -> Desktop {
    use std::path::PathBuf;
    let runtime = var("XDG_RUNTIME_DIR").map(PathBuf::from);
    if let Some(name) = var("WAYLAND_DISPLAY") {
        let name = PathBuf::from(name);
        let socket = if name.is_absolute() { Some(name) } else { runtime.as_ref().map(|r| r.join(name)) };
        if socket.is_some_and(|s| is_socket(&s)) {
            return Desktop::yes();
        }
    }
    if var("DISPLAY").is_some_and(|d| x11_reachable(&d.to_string_lossy(), x11_sockets)) {
        return Desktop::yes();
    }
    match runtime.and_then(|r| wayland_socket_in(&r)) {
        Some(name) => Desktop { available: true, env: vec![("WAYLAND_DISPLAY", name)] },
        None => Desktop::default(),
    }
}

/// An X display: `:0`, `:1.0` or `unix:0` is a socket in `/tmp/.X11-unix`;
/// one with a host (`localhost:10.0`, from `ssh -X`) is over the network,
/// and taken as there.
#[cfg(target_os = "linux")]
fn x11_reachable(display: &str, sockets: &std::path::Path) -> bool {
    let Some((host, rest)) = display.rsplit_once(':') else { return false };
    let number = rest.split('.').next().unwrap_or_default();
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if host.is_empty() || host == "unix" { is_socket(&sockets.join(format!("X{number}"))) } else { true }
}

/// The lowest-numbered `wayland-N` socket among the first
/// `RUNTIME_ENTRIES` entries of `dir`.
#[cfg(target_os = "linux")]
fn wayland_socket_in(dir: &std::path::Path) -> Option<OsString> {
    std::fs::read_dir(dir)
        .ok()?
        .take(RUNTIME_ENTRIES)
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name();
            let n: u32 = name.to_str()?.strip_prefix("wayland-")?.parse().ok()?;
            is_socket(&e.path()).then_some((n, name))
        })
        .min_by_key(|(n, _)| *n)
        .map(|(_, name)| name)
}

/// A socket something listens on: a compositor or X server that died
/// leaves its socket file behind. The connect runs on a thread of its own
/// and counts as no for anything slower than `CONNECT_WAIT`: a listener
/// whose queue is full, or a runtime folder on a hung mount, would block it
/// and with it `GET /api/setup`. A connect that hangs leaves its thread
/// behind until it returns.
#[cfg(target_os = "linux")]
fn is_socket(path: &std::path::Path) -> bool {
    let path = path.to_path_buf();
    within(CONNECT_WAIT, false, move || std::os::unix::net::UnixStream::connect(path).is_ok())
}

/// How long a desktop's socket has to take a connection.
#[cfg(target_os = "linux")]
const CONNECT_WAIT: std::time::Duration = std::time::Duration::from_millis(500);

/// Windows: no desktop for a service account (LocalSystem, LocalService,
/// NetworkService: a `NAME$` user, or a profile under `systemprofile` or
/// `ServiceProfiles`), nor for a service (`UPNEXT_MODE=service`) running
/// outside a sign-in: session 0 has no `SESSIONNAME`, which every
/// interactive session has ("Console", "RDP-Tcp#3").
#[cfg(any(windows, test))]
fn windows(var: &dyn Fn(&str) -> Option<OsString>, mode: &str) -> bool {
    let lower = |k: &str| var(k).map(|v| v.to_string_lossy().to_lowercase());
    let service_account = lower("USERNAME").is_some_and(|u| u.ends_with('$'))
        || lower("USERPROFILE").is_some_and(|p| p.ends_with("\\systemprofile") || p.contains("\\serviceprofiles\\"));
    if service_account {
        return false;
    }
    !(mode == "service" && var("SESSIONNAME").is_none())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs.iter().map(|(k, v)| (k.to_string(), OsString::from(v))).collect();
        move |k: &str| map.get(k).cloned()
    }

    #[test]
    fn a_container_has_no_desktop_and_the_app_always_has_one() {
        assert_eq!(detect(true, "app"), Some(Desktop::default()));
        assert_eq!(detect(false, "app"), Some(Desktop::yes()));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_checks_the_sockets_and_finds_the_sessions_wayland() {
        use std::os::unix::net::UnixListener;
        let dir = tempfile::tempdir().unwrap();
        let (runtime, x11) = (dir.path().join("run"), dir.path().join("x11"));
        std::fs::create_dir_all(&runtime).unwrap();
        std::fs::create_dir_all(&x11).unwrap();
        let rt = runtime.to_str().unwrap();
        let none = |v: &dyn Fn(&str) -> Option<OsString>| linux(v, &x11);

        // Nothing there: the variables name sockets that don't exist.
        assert_eq!(none(&vars(&[])), Desktop::default());
        assert_eq!(none(&vars(&[("XDG_RUNTIME_DIR", rt), ("WAYLAND_DISPLAY", "wayland-0"), ("DISPLAY", ":0")])), Desktop::default());

        // X: a local display needs its socket; one over the network is taken as there.
        let _x0 = UnixListener::bind(x11.join("X0")).unwrap();
        assert_eq!(none(&vars(&[("DISPLAY", ":0")])), Desktop::yes());
        assert_eq!(none(&vars(&[("DISPLAY", ":0.0")])), Desktop::yes());
        assert_eq!(none(&vars(&[("DISPLAY", "unix:0")])), Desktop::yes());
        assert_eq!(none(&vars(&[("DISPLAY", ":1")])), Desktop::default());
        assert_eq!(none(&vars(&[("DISPLAY", "localhost:10.0")])), Desktop::yes());
        assert_eq!(none(&vars(&[("DISPLAY", "nonsense")])), Desktop::default());

        // Wayland: the named socket, by name or path.
        let _w1 = UnixListener::bind(runtime.join("wayland-1")).unwrap();
        std::fs::write(runtime.join("wayland-0"), "").unwrap(); // a file, not a socket
        drop(UnixListener::bind(runtime.join("wayland-2")).unwrap()); // left behind, nothing listening
        assert!(!is_socket(&runtime.join("wayland-2")) && is_socket(&runtime.join("wayland-1")));
        std::fs::write(runtime.join("wayland-1.lock"), "").unwrap();
        assert_eq!(none(&vars(&[("XDG_RUNTIME_DIR", rt), ("WAYLAND_DISPLAY", "wayland-1")])), Desktop::yes());
        let path = runtime.join("wayland-1");
        assert_eq!(none(&vars(&[("WAYLAND_DISPLAY", path.to_str().unwrap())])), Desktop::yes());

        // A service with neither variable: the session's own socket, passed to the window.
        let found = none(&vars(&[("XDG_RUNTIME_DIR", rt)]));
        assert_eq!(found, Desktop { available: true, env: vec![("WAYLAND_DISPLAY", OsString::from("wayland-1"))] });
        let _w3 = UnixListener::bind(runtime.join("wayland-3")).unwrap();
        assert_eq!(none(&vars(&[("XDG_RUNTIME_DIR", rt), ("WAYLAND_DISPLAY", "wayland-9")])).env[0].1, "wayland-1", "the lowest");
        assert_eq!(wayland_socket_in(std::path::Path::new("/nonexistent")), None);
    }

    /// A listener that takes no more connections (its queue is full) counts
    /// as no desktop, after `CONNECT_WAIT` rather than never.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_socket_that_takes_no_connection_is_no_desktop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("X0");
        let _busy = std::os::unix::net::UnixListener::bind(&path).unwrap();
        // Connections it never accepts fill its queue, at most `somaxconn` long.
        let full = (0..70_000).find(|_| !is_socket(&path)).expect("the queue never filled");
        let started = std::time::Instant::now();
        assert!(!is_socket(&path), "after {full} connections");
        let took = started.elapsed();
        assert!(took >= CONNECT_WAIT && took < CONNECT_WAIT * 4, "{took:?}");
    }

    /// A detection is kept for `KEEP`, for the same server only, and one
    /// that was given up only for `KEEP_GIVEN_UP`.
    #[test]
    fn a_detection_is_kept_for_half_a_minute() {
        let at = std::time::Instant::now();
        let kept = Some(Kept { at, keep: KEEP, container: false, mode: "serve".into(), desktop: Desktop::yes() });
        assert_eq!(fresh(&kept, at + std::time::Duration::from_secs(29), false, "serve"), Some(Desktop::yes()));
        assert_eq!(fresh(&kept, at + KEEP, false, "serve"), None, "too old");
        assert_eq!(fresh(&kept, at, true, "serve"), None, "another server");
        assert_eq!(fresh(&kept, at, false, "service"), None);
        assert_eq!(fresh(&None, at, false, "serve"), None);
        let given_up = Some(Kept { at, keep: KEEP_GIVEN_UP, container: false, mode: "serve".into(), desktop: Desktop::default() });
        assert_eq!(fresh(&given_up, at + KEEP_GIVEN_UP, false, "serve"), None, "a desktop that appears is found soon");
    }

    /// Requests that miss the kept detection together share one check, and
    /// a check that was given up is tried again a few seconds later.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn requests_that_miss_together_share_one_check() {
        static CHECKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        fn slow(_: bool, _: &str) -> Option<Desktop> {
            CHECKS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(200));
            Some(Desktop::yes())
        }
        let detections: &'static Detections = Box::leak(Box::new(Detections::new()));
        let asks: Vec<_> = (0..8).map(|_| tokio::spawn(detections.get(false, "serve", slow))).collect();
        for ask in asks {
            assert_eq!(ask.await.unwrap(), Desktop::yes());
        }
        assert_eq!(CHECKS.load(std::sync::atomic::Ordering::SeqCst), 1);

        fn given_up(_: bool, _: &str) -> Option<Desktop> {
            None
        }
        let detections = Detections::new();
        assert_eq!(detections.get(false, "serve", given_up).await, Desktop::default());
        let kept = detections.last.lock().unwrap().clone().unwrap();
        assert_eq!(kept.keep, KEEP_GIVEN_UP);
    }

    /// While work that hung hasn't returned, the next call doesn't start
    /// another thread for it; once it returns, calls run again.
    #[cfg(target_os = "linux")]
    #[test]
    fn work_that_hangs_leaves_one_thread_behind() {
        static STILL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        static RAN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let wait = std::time::Duration::from_millis(100);
        let hang = move || {
            RAN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(600));
        };
        assert_eq!(bounded(&STILL, wait, hang), None, "given up");
        assert_eq!(bounded(&STILL, wait, hang), None, "not started again");
        assert_eq!(RAN.load(std::sync::atomic::Ordering::SeqCst), 1);
        std::thread::sleep(std::time::Duration::from_millis(800));
        assert_eq!(bounded(&STILL, wait, || 7), Some(7), "runs again once it returned");
    }

    /// Work that hangs, as listing a folder on a hung mount does, is given
    /// up after the wait with the answer for nothing found.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_check_that_hangs_is_given_up() {
        let wait = std::time::Duration::from_millis(200);
        let started = std::time::Instant::now();
        let found = within(wait, Desktop::default(), || {
            std::thread::sleep(std::time::Duration::from_secs(5));
            Desktop::yes()
        });
        let took = started.elapsed();
        assert_eq!(found, Desktop::default());
        assert!(took >= wait && took < wait * 5, "{took:?}");
        assert_eq!(within(wait, Desktop::default(), Desktop::yes), Desktop::yes(), "an answer in time is kept");
    }

    #[test]
    fn windows_services_have_no_desktop() {
        let user = [("USERNAME", "you"), ("USERPROFILE", r"C:\Users\you")];
        assert!(windows(&vars(&user), "serve"), "a program started by hand");
        assert!(!windows(&vars(&user), "service"), "a service in session 0");
        let signed_in = [user[0], user[1], ("SESSIONNAME", "Console")];
        assert!(windows(&vars(&signed_in), "service"), "started at sign-in, in the user's session");
        for system in [
            [("USERNAME", "DEN$"), ("USERPROFILE", r"C:\Windows\system32\config\systemprofile")],
            [("USERNAME", "LOCAL SERVICE"), ("USERPROFILE", r"C:\Windows\ServiceProfiles\LocalService")],
        ] {
            assert!(!windows(&vars(&system), "serve"), "{system:?}");
        }
    }
}
