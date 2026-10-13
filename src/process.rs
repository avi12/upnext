//! Starting other programs: the AI CLIs (`claude`, `codex`) and the GOG
//! login window. Every child process goes through [`command`] or
//! [`Launch::command`].
//!
//! On Windows a console program started from a program with no console (the
//! app, `upnext-app.exe`) opens a console window of its own, which flashes up
//! for every AI call. Children are started with `CREATE_NO_WINDOW`, which
//! stops that and changes nothing else: a GUI child (the GOG window) still
//! shows its window, and pipes work as before.
//!
//! An npm install on Windows gives `claude.cmd`, a batch file, rather than
//! `claude.exe`. Windows runs a batch file through `cmd.exe`, which can't
//! take an argument with a line break in it (Rust refuses to try), caps the
//! whole command line at 8,191 characters, and survives being killed in
//! place of the program it started. So a `.cmd` that npm wrote is read, and
//! what it would start (`claude.exe`, or `node` with a script) is started
//! directly with the same arguments. Any other batch file is run as it is;
//! [`Launch::batch`] says so, and `claude_cli` then passes the system
//! prompt in a file instead of on the command line.
//!
//! An AI CLI can be a launcher for another program: npm's `codex` is a
//! Node script that runs the native Codex as its child. Killing only the
//! launcher when a call times out or a job is cancelled would leave Codex
//! working on the turn, spending the plan. So [`Launch::command`] starts the
//! CLI in a process group of its own on Unix, and [`feed`] and [`output`]
//! kill the whole group, or on Windows the whole process tree, when they
//! give up on it.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// `CREATE_NO_WINDOW`, from the Windows process creation flags.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A child process that opens no console window on Windows, and is killed
/// if the `Child` is dropped before it ends.
pub fn command(program: impl AsRef<OsStr>) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(program);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.kill_on_drop(true);
    cmd
}

/// How to start a program UpNext was given by name or path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// What is started.
    pub program: PathBuf,
    /// Arguments that come before the caller's (the script `node` runs).
    pub args: Vec<OsString>,
    /// `program` is a batch file, run through `cmd.exe`: no argument may
    /// hold a line break, and the command line stays short.
    pub batch: bool,
}

impl Launch {
    /// `program` as found (`Config::claude_program`): a batch file npm wrote
    /// is replaced by what it starts; anything else is started as it is.
    pub fn of(program: &str) -> Launch {
        let path = Path::new(program);
        let as_is = Launch { program: path.to_path_buf(), args: vec![], batch: is_batch(path) };
        if !as_is.batch {
            return as_is;
        }
        let Some(target) = std::fs::read_to_string(path).ok().and_then(|text| shim_target(&text, path.parent()?)) else {
            return as_is;
        };
        if !target.is_file() {
            return as_is;
        }
        if is_native(&target) {
            return Launch { program: target, args: vec![], batch: false };
        }
        // A script: npm's shim runs it with the `node.exe` beside the shim
        // when there is one, else `node` from PATH.
        let beside = path.parent().map(|d| d.join("node.exe")).filter(|p| p.is_file());
        let node = beside.or_else(|| crate::config::find_program("node").map(|(p, _)| p)).unwrap_or_else(|| PathBuf::from("node"));
        Launch { program: node, args: vec![target.into_os_string()], batch: false }
    }

    /// The command for this launch, with its leading arguments, set up as
    /// [`command`] sets every child up, and on Unix in a process group of
    /// its own, so that [`feed`] and [`output`] can kill what it starts too.
    pub fn command(&self) -> tokio::process::Command {
        let mut cmd = command(&self.program);
        cmd.args(&self.args);
        #[cfg(unix)]
        cmd.process_group(0);
        cmd
    }

    /// A script npm's shim runs with `node`, and no `node.exe` was found
    /// beside the shim, on `PATH` or where Node.js installs it, so starting
    /// it fails for want of Node.js rather than of the program.
    pub fn node_missing(&self) -> bool {
        // `of` names node bare only when it found none.
        !self.args.is_empty() && self.program == Path::new("node")
    }

    /// What `doctor` shows when this isn't simply the program found.
    pub fn describe(&self) -> String {
        let mut parts = vec![self.program.display().to_string()];
        parts.extend(self.args.iter().map(|a| Path::new(a).display().to_string()));
        parts.join(" ")
    }
}

/// Why `bin` couldn't be started, when it's npm's script and Node.js is
/// missing ([`Launch::node_missing`]): said so, rather than that `bin`
/// isn't installed.
pub fn node_missing(bin: &str) -> Option<String> {
    Launch::of(bin).node_missing().then(|| {
        format!(
            "cannot run `{bin}`: it's an npm install, and Node.js's node.exe wasn't found. Install Node.js (nodejs.org), then try again"
        )
    })
}

/// Why [`output`] got no output from a child.
#[derive(Debug)]
pub enum RunError {
    /// It couldn't be started, or waiting for it failed.
    Start(std::io::Error),
    /// It took longer than the time it was given, and was killed.
    TimedOut,
}

/// Run `cmd` with no input, read what it prints and wait for it to exit,
/// all within `timeout`: the short status commands of the AI CLIs
/// (`--version`, `auth status`, `/usage`). The child goes with the
/// returned future, so a timeout kills it and what it started ([`Tree`]).
pub async fn output(mut cmd: tokio::process::Command, timeout: std::time::Duration) -> Result<std::process::Output, RunError> {
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let mut tree = Tree::new(cmd.spawn().map_err(RunError::Start)?);
    match tokio::time::timeout(timeout, tree.output()).await {
        Ok(Ok(out)) => Ok(out),
        Ok(Err(e)) => Err(RunError::Start(e)),
        Err(_) => Err(RunError::TimedOut),
    }
}

/// A child, and everything it starts. Dropped before the child has been
/// waited for (a timeout, a cancelled job), it kills them all, not only the
/// child as `kill_on_drop` does: on Unix the process group the child leads
/// ([`Launch::command`]), on Windows the process tree under it.
struct Tree {
    child: tokio::process::Child,
    /// The child's id, kept: Tokio forgets it once the child has exited,
    /// and what the child started may still run then.
    pid: Option<u32>,
    waited: bool,
}

impl Tree {
    fn new(child: tokio::process::Child) -> Tree {
        Tree { pid: child.id(), child, waited: false }
    }

    /// What the child prints, read until it exits and its output ends, as
    /// `Child::wait_with_output` does, but keeping the child here.
    async fn output(&mut self) -> std::io::Result<std::process::Output> {
        use tokio::io::{AsyncRead, AsyncReadExt};
        async fn read_all(pipe: Option<impl AsyncRead + Unpin>) -> std::io::Result<Vec<u8>> {
            let mut buf = Vec::new();
            if let Some(mut pipe) = pipe {
                pipe.read_to_end(&mut buf).await?;
            }
            Ok(buf)
        }
        let (stdout, stderr) = (self.child.stdout.take(), self.child.stderr.take());
        let (status, stdout, stderr) = tokio::try_join!(self.child.wait(), read_all(stdout), read_all(stderr))?;
        self.waited = true;
        Ok(std::process::Output { status, stdout, stderr })
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        if let (false, Some(pid)) = (self.waited, self.pid) {
            kill_tree(pid);
        }
        // The child itself is killed next, by `kill_on_drop`.
    }
}

/// Kill the process group `pid` leads. It was started with
/// `process_group(0)`, so its group's id is its own; a group outlives its
/// leader while any of its processes run, so this reaches them even after
/// the child has exited.
#[cfg(unix)]
fn kill_tree(pid: u32) {
    // Never 1, which `kill` reads as every process there is.
    let Some(group) = i32::try_from(pid).ok().filter(|p| *p > 1).and_then(rustix::process::Pid::from_raw) else { return };
    let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
}

/// Kill the process tree under `pid`, with `taskkill /T`. It finds the tree
/// through the child, so it runs, and is waited for, while the child is
/// still alive, before `kill_on_drop` ends it.
#[cfg(windows)]
fn kill_tree(pid: u32) {
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;
    let _ = std::process::Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(any(unix, windows)))]
fn kill_tree(_pid: u32) {}

/// Why [`feed`] got no output from a child.
#[derive(Debug)]
pub enum FeedError {
    /// Writing its input failed (it exited without reading all of it).
    Write(std::io::Error),
    /// Reading its output or waiting for it to exit failed.
    Wait(std::io::Error),
    /// It took longer than the time it was given, and was killed.
    TimedOut,
}

/// Write `input` to a child started with piped stdin, stdout and stderr,
/// read what it prints, and wait for it to exit, all within `timeout`. The
/// write is inside the timeout and runs beside the reads: a child that
/// doesn't read its input fills the pipe (64 KB on Linux), and a write
/// made on its own before the timeout starts would then never end. The
/// child goes with the returned future, so a timeout, or a caller that
/// drops the future (a cancelled job), kills it and what it started
/// ([`Tree`]).
pub async fn feed(child: tokio::process::Child, input: &[u8], timeout: std::time::Duration) -> Result<std::process::Output, FeedError> {
    use tokio::io::AsyncWriteExt;
    let mut tree = Tree::new(child);
    let stdin = tree.child.stdin.take();
    let write = async move {
        let Some(mut stdin) = stdin else { return Err(std::io::Error::other("no stdin")) };
        // Stdin is dropped when this ends, which closes it: the child sees the end of its input.
        stdin.write_all(input).await
    };
    let both = async { tokio::join!(write, tree.output()) };
    match tokio::time::timeout(timeout, both).await {
        Err(_) => Err(FeedError::TimedOut),
        Ok((Err(e), _)) => Err(FeedError::Write(e)),
        Ok((Ok(()), Err(e))) => Err(FeedError::Wait(e)),
        Ok((Ok(()), Ok(output))) => Ok(output),
    }
}

fn has_extension(path: &Path, wanted: &[&str]) -> bool {
    path.extension().and_then(OsStr::to_str).is_some_and(|e| wanted.iter().any(|w| e.eq_ignore_ascii_case(w)))
}

fn is_batch(path: &Path) -> bool {
    has_extension(path, &["cmd", "bat"])
}

fn is_native(path: &Path) -> bool {
    has_extension(path, &["exe", "com"])
}

/// The program an npm (or pnpm) `.cmd` shim starts, from its text, with
/// the shim in `dir`. The shim's last line runs
/// `"%dp0%\node_modules\…\claude.exe" %*` (or `"%_prog%" "%dp0%\…\cli.js" %*`
/// for a script; pnpm writes `%~dp0` for `%dp0%`). The target is the last
/// quoted path under the shim's folder on a line that passes `%*` on, other
/// than a `node.exe`.
fn shim_target(text: &str, dir: &Path) -> Option<PathBuf> {
    let line = text.lines().find(|l| l.contains("%*"))?;
    let before_args = &line[..line.find("%*")?];
    let quoted = before_args.split('"').skip(1).step_by(2);
    let rest = quoted
        .filter_map(|q| q.strip_prefix("%dp0%").or_else(|| q.strip_prefix("%~dp0")))
        .filter(|rest| !rest.to_ascii_lowercase().ends_with("node.exe"))
        .last()?;
    let mut target = dir.to_path_buf();
    for part in rest.split(['\\', '/']).filter(|p| !p.is_empty() && *p != ".") {
        target.push(part);
    }
    (target != dir).then_some(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shims npm 10 writes (its cmd-shim 7.0.0) for the native package
    /// (`bin/claude.exe`, Claude Code 2.x) and for a package whose bin is a
    /// node script (older Claude Code, Codex).
    #[test]
    fn npm_shims_name_what_they_start() {
        let dir = Path::new("npm");
        let native = crate::testutil::fixture("process/claude_npm_native.cmd");
        assert_eq!(
            shim_target(&native, dir),
            Some(dir.join("node_modules").join("@anthropic-ai").join("claude-code").join("bin").join("claude.exe"))
        );
        let script = crate::testutil::fixture("process/claude_npm_node.cmd");
        assert_eq!(shim_target(&script, dir), Some(dir.join("node_modules").join("@anthropic-ai").join("claude-code").join("cli.js")));
    }

    /// pnpm's shim: `%~dp0`, and `node.exe` beside it named first.
    #[test]
    fn pnpm_shims_too() {
        let dir = Path::new("pnpm");
        let text = "@SETLOCAL\r\n@IF EXIST \"%~dp0\\node.exe\" (\r\n  \"%~dp0\\node.exe\"  \"%~dp0\\..\\@anthropic-ai\\claude-code\\cli.js\" %*\r\n) ELSE (\r\n  @SET PATHEXT=%PATHEXT:;.JS;=;%\r\n  node  \"%~dp0\\..\\@anthropic-ai\\claude-code\\cli.js\" %*\r\n)\r\n";
        assert_eq!(shim_target(text, dir), Some(dir.join("..").join("@anthropic-ai").join("claude-code").join("cli.js")));
    }

    #[test]
    fn other_batch_files_name_nothing() {
        let dir = Path::new("x");
        assert_eq!(shim_target("@echo off\r\nclaude-real.exe %*\r\n", dir), None);
        assert_eq!(shim_target("@echo off\r\necho hi\r\n", dir), None);
        assert_eq!(shim_target("\"%dp0%\\\" %*", dir), None);
    }

    /// A shim is replaced by what it starts, when that exists; a batch file
    /// that isn't a shim, or whose target is gone, runs as it is.
    #[test]
    fn launches() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let put = |rel: &str, body: &str| {
            let p = d.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, body).unwrap();
            p
        };
        let shim = put("claude.cmd", &crate::testutil::fixture("process/claude_npm_native.cmd"));
        let shim = shim.to_str().unwrap();
        assert_eq!(Launch::of(shim), Launch { program: shim.into(), args: vec![], batch: true }, "the target isn't installed");
        let exe = put("node_modules/@anthropic-ai/claude-code/bin/claude.exe", "MZ");
        assert_eq!(Launch::of(shim), Launch { program: exe, args: vec![], batch: false });

        let old = put("old.CMD", &crate::testutil::fixture("process/claude_npm_node.cmd"));
        let script = put("node_modules/@anthropic-ai/claude-code/cli.js", "");
        let node = put("node.exe", "MZ");
        let launch = Launch::of(old.to_str().unwrap());
        assert_eq!((&launch.program, launch.batch), (&node, false));
        assert_eq!(launch.args.iter().map(PathBuf::from).collect::<Vec<_>>(), [script]);
        assert!(launch.describe().ends_with("cli.js"), "{}", launch.describe());

        let own = put("mine.bat", "@echo off\r\nC:\\tools\\claude.exe %*\r\n");
        assert_eq!(Launch::of(own.to_str().unwrap()), Launch { program: own.clone(), args: vec![], batch: true });

        for plain in ["claude", "/usr/bin/claude", "C:\\Users\\you\\.local\\bin\\claude.exe"] {
            assert_eq!(Launch::of(plain), Launch { program: plain.into(), args: vec![], batch: false }, "{plain}");
        }
    }

    /// Only a script with no `node.exe` found is missing Node.js; the
    /// program itself, or a script with a node, is not.
    #[test]
    fn node_missing_only_for_a_script_without_node() {
        let script = Launch { program: "node".into(), args: vec!["cli.js".into()], batch: false };
        assert!(script.node_missing());
        let found = Launch { program: "C:\\Program Files\\nodejs\\node.exe".into(), ..script.clone() };
        assert!(!found.node_missing());
        let plain = Launch { program: "node".into(), args: vec![], batch: false };
        assert!(!plain.node_missing(), "`node` itself, run as it is");
        assert_eq!(node_missing("claude"), None, "not a shim");
    }

    /// A child that never reads its input, given more than the pipe holds:
    /// `feed` gives up at its timeout instead of waiting on the write for
    /// ever, and the child is killed.
    #[cfg(target_os = "linux")] // The stand-in is a shell script, and /proc says whether it still runs.
    #[tokio::test]
    async fn feed_times_out_on_a_child_that_never_reads() {
        use std::os::unix::fs::PermissionsExt;
        use std::time::{Duration, Instant};
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("stuck");
        let pid_file = dir.path().join("pid");
        std::fs::write(&bin, format!("#!/bin/sh\necho $$ > '{}'\nexec sleep 60\n", pid_file.display())).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut cmd = command(&bin);
        cmd.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        let input = vec![b'x'; 1024 * 1024];

        let started = Instant::now();
        let fed = tokio::time::timeout(Duration::from_secs(10), feed(cmd.spawn().unwrap(), &input, Duration::from_millis(500)));
        let result = fed.await.expect("feed ended on its own");
        assert!(matches!(result, Err(FeedError::TimedOut)), "{result:?}");
        assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());

        // Killed: gone, or a zombie until Tokio reaps it.
        let pid = std::fs::read_to_string(&pid_file).unwrap().trim().to_string();
        let running = || {
            std::fs::read_to_string(format!("/proc/{pid}/stat"))
                .is_ok_and(|stat| stat.rsplit(')').next().and_then(|rest| rest.split_whitespace().next()) != Some("Z"))
        };
        let killed = Instant::now();
        while running() {
            assert!(killed.elapsed() < Duration::from_secs(5), "the child is still running");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// A stand-in CLI that starts a program of its own, as npm's `codex`
    /// starts the native Codex, and waits for it: its path, and the file it
    /// writes that program's id to.
    #[cfg(target_os = "linux")]
    fn launcher(dir: &Path) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let bin = dir.join("codex");
        let pid_file = dir.join("child-pid");
        std::fs::write(&bin, format!("#!/bin/sh\nsleep 60 &\necho $! > '{}'\nwait\n", pid_file.display())).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        (bin, pid_file)
    }

    /// Waits, up to five seconds, for the program the stand-in started to
    /// be gone (or a zombie until something reaps it).
    #[cfg(target_os = "linux")]
    async fn assert_gone(pid_file: &Path) {
        use std::time::{Duration, Instant};
        let started = Instant::now();
        let pid = loop {
            match std::fs::read_to_string(pid_file) {
                Ok(p) if !p.trim().is_empty() => break p.trim().to_string(),
                _ if started.elapsed() > Duration::from_secs(5) => panic!("the stand-in never started its program"),
                _ => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        };
        let running = || {
            std::fs::read_to_string(format!("/proc/{pid}/stat"))
                .is_ok_and(|stat| stat.rsplit(')').next().and_then(|rest| rest.split_whitespace().next()) != Some("Z"))
        };
        while running() {
            if started.elapsed() > Duration::from_secs(5) {
                let _ = std::process::Command::new("kill").arg(&pid).status();
                panic!("the program the CLI started ({pid}) still runs after the call gave up");
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// A call that times out kills what the CLI started, not only the CLI
    /// (B-AI-5).
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_timed_out_call_kills_what_the_cli_started() {
        use std::time::Duration;
        let dir = tempfile::tempdir().unwrap();
        let (bin, pid_file) = launcher(dir.path());
        let mut cmd = Launch::of(bin.to_str().unwrap()).command();
        cmd.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        let fed = feed(cmd.spawn().unwrap(), b"prompt", Duration::from_millis(500)).await;
        assert!(matches!(fed, Err(FeedError::TimedOut)), "{fed:?}");
        assert_gone(&pid_file).await;
    }

    /// A call dropped before it ends (a cancelled job) does the same.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_cancelled_call_kills_what_the_cli_started() {
        use std::time::Duration;
        let dir = tempfile::tempdir().unwrap();
        let (bin, pid_file) = launcher(dir.path());
        let mut cmd = Launch::of(bin.to_str().unwrap()).command();
        cmd.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        let call = feed(cmd.spawn().unwrap(), b"prompt", Duration::from_secs(60));
        assert!(tokio::time::timeout(Duration::from_millis(500), call).await.is_err(), "the call ended on its own");
        assert_gone(&pid_file).await;
    }

    /// A status command that times out does the same, and a quick one
    /// answers as before.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_timed_out_status_command_kills_what_the_cli_started() {
        use std::time::Duration;
        let dir = tempfile::tempdir().unwrap();
        let (bin, pid_file) = launcher(dir.path());
        let out = output(Launch::of(bin.to_str().unwrap()).command(), Duration::from_millis(500)).await;
        assert!(matches!(out, Err(RunError::TimedOut)), "{out:?}");
        assert_gone(&pid_file).await;

        let mut echo = Launch::of("sh").command();
        echo.args(["-c", "echo upnext; echo oops >&2"]);
        let out = output(echo, Duration::from_secs(10)).await.unwrap();
        assert!(out.status.success());
        assert_eq!((out.stdout.as_slice(), out.stderr.as_slice()), (&b"upnext\n"[..], &b"oops\n"[..]));
    }

    /// On Windows: npm's shim is read with Windows' own paths and its exe
    /// (here a copy of `cmd.exe`) started directly, while a batch file can't
    /// be given an argument with a line break at all, which is why.
    #[cfg(windows)]
    #[tokio::test]
    async fn on_windows_a_shim_starts_its_exe() {
        let dir = tempfile::tempdir().unwrap();
        let shim = dir.path().join("claude.cmd");
        std::fs::write(&shim, crate::testutil::fixture("process/claude_npm_native.cmd")).unwrap();
        let bin = dir.path().join(r"node_modules\@anthropic-ai\claude-code\bin");
        std::fs::create_dir_all(&bin).unwrap();
        let system = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join(r"System32\cmd.exe");
        std::fs::copy(system, bin.join("claude.exe")).unwrap();

        let launch = Launch::of(shim.to_str().unwrap());
        assert_eq!(launch, Launch { program: bin.join("claude.exe"), args: vec![], batch: false });
        let out = launch.command().args(["/C", "echo upnext"]).output().await.unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "upnext");
        assert!(launch.command().args(["/C", "rem", "two\nlines"]).output().await.is_ok(), "an exe takes a line break");

        let err = command(&shim).args(["/C", "rem", "two\nlines"]).output().await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput, "{err}");
    }

    /// The child starts and its output comes back through the pipes, with
    /// the no-window flag set on Windows.
    #[tokio::test]
    async fn children_run_and_pipe() {
        let mut cmd = if cfg!(windows) { command("cmd") } else { command("sh") };
        if cfg!(windows) {
            cmd.args(["/C", "echo upnext"]);
        } else {
            cmd.args(["-c", "echo upnext"]);
        }
        let out = cmd.output().await.unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "upnext");
    }
}
