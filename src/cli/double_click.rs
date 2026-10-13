//! `upnext.exe` started on its own, as Explorer starts a program that's
//! double-clicked. With no arguments and a console of its own, it runs
//! the server and opens UpNext in the browser, where it used to print clap's
//! usage error into a console that closed at once. From a terminal, no
//! arguments still prints the usage. Windows only: elsewhere a program is
//! started from a terminal or a launcher entry that names its arguments.

use super::app::{Probe, probe};
use crate::config::{BadSetting, Config};
use anyhow::{Result, bail};
use std::io::IsTerminal;
use std::time::Duration;

/// Whether `upnext` was started with no arguments on a console of its own.
/// Windows tells how many programs share a console (`GetConsoleProcessList`),
/// but only through an unsafe call, which UpNext's code forbids. A new
/// console has its cursor at the top left, while in a terminal it is at least
/// a line below the command that started UpNext, so that is the test. Only
/// a terminal cleared on the same line (`cls & upnext`) fools it, and then
/// UpNext does what a double-click does.
pub fn started_alone() -> bool {
    std::env::args_os().len() == 1
        && std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal()
        && crossterm_winapi::ScreenBuffer::current().and_then(|b| b.info()).is_ok_and(|info| {
            let at = info.cursor_pos();
            (at.x, at.y) == (0, 0)
        })
}

/// What `run` did, when it went well.
enum Ran {
    /// Served until the console's Ctrl-C.
    Served,
    /// Opened the UpNext already running in the browser.
    Opened,
}

/// Run the server and open it in the browser, or open the UpNext already
/// running. The console stays until Enter after anything it should show:
/// an error, or that another UpNext was opened.
pub fn main() -> Result<()> {
    super::init_logging();
    println!("This is UpNext's command-line program. Started on its own, it runs UpNext and opens it in your browser;");
    println!("closing this window stops UpNext. upnext-app.exe opens UpNext in a window of its own instead,");
    println!("and `upnext --help` in a terminal lists the commands.");
    println!();
    let ran =
        tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(anyhow::Error::from).and_then(|rt| rt.block_on(run()));
    match ran {
        Ok(Ran::Served) => Ok(()),
        Ok(Ran::Opened) => {
            wait_for_enter();
            Ok(())
        }
        Err(e) => {
            eprintln!("UpNext couldn't start: {e:#}");
            if let Some(bad) = e.downcast_ref::<BadSetting>() {
                eprintln!("{}", bad.advice());
            }
            wait_for_enter();
            std::process::exit(1)
        }
    }
}

async fn run() -> Result<Ran> {
    let cfg = Config::load()?;
    if let Some(why) = super::migrate::unmoved_data(&cfg) {
        bail!(why);
    }
    let port = cfg.port;
    let url = format!("http://127.0.0.1:{port}/");
    match probe(port).await {
        Probe::UpNext { .. } => {
            println!("UpNext is already running on port {port} (in upnext-app.exe or another upnext), so it opens in your browser.");
            open_browser(&url);
            return Ok(Ran::Opened);
        }
        Probe::Other => bail!(
            "another program is using port {port}. Stop it, or set UPNEXT_PORT to a free port (`upnext config list` shows where settings come from), then start UpNext again"
        ),
        Probe::Nobody => {}
    }
    // The browser opens once the server answers.
    let opener = tokio::spawn(async move {
        for _ in 0..150 {
            if matches!(probe(port).await, Probe::UpNext { .. }) {
                open_browser(&url);
                return;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    });
    let served = super::serve::serve(cfg, None, None, None).await;
    opener.abort();
    served.map(|()| Ran::Served)
}

fn open_browser(url: &str) {
    if let Err(e) = open::that_detached(url) {
        eprintln!("couldn't open the browser ({e}); open {url} yourself");
    }
}

fn wait_for_enter() {
    println!("Press Enter to close this window.");
    let _ = std::io::stdin().read_line(&mut String::new());
}
