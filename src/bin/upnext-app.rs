//! `upnext-app`: UpNext's app window as a program of its own.
//!
//! A Windows program is built either for the console, which opens a terminal
//! when it's started from the Start menu or Explorer, or for the desktop,
//! which prints nothing in a terminal. `upnext` is the console one (the CLI,
//! `serve`, and `upnext app` from a terminal); this is the desktop one, for
//! the Start menu. Elsewhere it's the same as `upnext app`.

#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    upnext::cli::app::desktop_main()
}
