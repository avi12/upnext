//! Claude Code in UpNext's container: installed on request from
//! Anthropic, never shipped in the image. Claude Code's licence reserves all
//! rights, so a published image can't carry it; each container that asks
//! (`UPNEXT_INSTALL_CLAUDE=1`, or `upnext install-claude`) fetches it from
//! Anthropic's release site instead, into its home folder, which is the
//! `/data` volume in the image.
//!
//! This does what `curl -fsSL https://claude.ai/install.sh | bash` does,
//! because the image has no curl: it reads the version a channel points at,
//! that version's manifest, and the binary for this platform, checks the
//! binary against the manifest's SHA-256, and runs the binary's own
//! `claude install <channel>`. That puts the CLI in `~/.local/bin` and
//! `~/.local/share/claude`, where `Config::claude_program` looks, and Claude
//! Code updates itself there from then on.

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;

/// Anthropic's release site, as Claude Code's own installer and updater use it.
pub const RELEASES: &str = "https://downloads.claude.ai/claude-code-releases";

/// The channel installed and then followed: Anthropic's `stable` one, about a
/// week behind `latest`, skipping releases with known problems.
pub const CHANNEL: &str = "stable";

/// The longest `claude install` may take; it may download its channel's
/// version again, and the binary is over 200 MB.
const INSTALL_TIMEOUT: Duration = Duration::from_secs(600);

/// Where an install stands, for setup's AI step while the server installs.
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
pub struct InstallState {
    /// Downloading or installing now.
    pub installing: bool,
    /// Why the last try failed; null while installing.
    pub error: Option<String>,
}

/// The server's install, if one was started: running, or failed. Null once
/// it succeeds, since `claude` is then found where setup looks.
#[derive(Default)]
pub struct Progress(Mutex<Option<InstallState>>);

impl Progress {
    pub fn now(&self) -> Option<InstallState> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set(&self, state: Option<InstallState>) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = state;
    }
}

/// One install: where from, into which home folder, and where the download
/// waits while it's checked.
pub struct Installer {
    pub releases: String,
    pub channel: String,
    pub home: PathBuf,
    pub work: PathBuf,
}

impl Installer {
    /// From Anthropic's site into `HOME`, or in a Flatpak into the app's own
    /// folder, the only one there it can write to (`flatpak`), downloading
    /// into `work` (UpNext's cache folder: in the image a tmpfs `/tmp` is too
    /// small for the binary).
    /// `UPNEXT_CLAUDE_RELEASES` names a copy of the site instead, for the
    /// smoke test's stand-in, and may be plain http. It opens nothing new:
    /// whoever can set the server's environment or `/data/.env` already
    /// controls what runs in the container.
    pub fn new(work: &Path) -> Result<Installer> {
        let home = install_home(crate::flatpak::get(), || directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()))?;
        let releases = std::env::var("UPNEXT_CLAUDE_RELEASES").ok().filter(|v| !v.trim().is_empty());
        Ok(Installer {
            releases: crate::http::base_url(RELEASES, releases.as_deref()),
            channel: CHANNEL.into(),
            home,
            work: work.to_path_buf(),
        })
    }

    /// Where `claude install` puts the command.
    pub fn launcher(&self) -> PathBuf {
        self.home.join(".local").join("bin").join("claude")
    }

    /// Download, check and install; the installed command and its version.
    pub async fn install(&self) -> Result<(PathBuf, String)> {
        let platform = platform()?;
        let http = client()?;
        let base = &self.releases;

        let version = text(&http, &format!("{base}/{}", self.channel)).await?;
        if !is_version(&version) {
            bail!("{base}/{} didn't name a version: {:?}", self.channel, truncate(&version, 80));
        }
        let manifest: serde_json::Value = get(&http, &format!("{base}/{version}/manifest.json"))
            .await?
            .json()
            .await
            .with_context(|| format!("Claude Code {version}'s manifest isn't JSON"))?;
        let checksum = manifest["platforms"][platform.as_str()]["checksum"]
            .as_str()
            .filter(|c| c.len() == 64 && c.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| anyhow!("Claude Code {version}'s manifest has no checksum for {platform}"))?
            .to_ascii_lowercase();

        std::fs::create_dir_all(&self.work).with_context(|| format!("making {}", self.work.display()))?;
        let file = self.work.join(format!("claude-{version}-{platform}-{}", std::process::id()));
        let _cleanup = Remove(file.clone());
        let got = download(&http, &format!("{base}/{version}/{platform}/claude"), &file).await?;
        if got != checksum {
            bail!("the Claude Code {version} download doesn't match its manifest (SHA-256 {got}, expected {checksum})");
        }
        executable(&file)?;

        // The binary's own installer, as install.sh ends.
        let out = crate::process::command(&file)
            .args(["install", &self.channel])
            .env("HOME", &self.home)
            .current_dir(&self.work)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output();
        let out = tokio::time::timeout(INSTALL_TIMEOUT, out)
            .await
            .map_err(|_| anyhow!("`claude install` took more than {} minutes", INSTALL_TIMEOUT.as_secs() / 60))?
            .context("running the downloaded Claude Code")?;
        let said = format!("{}\n{}", String::from_utf8_lossy(&out.stdout).trim(), String::from_utf8_lossy(&out.stderr).trim());
        let launcher = self.launcher();
        if !out.status.success() || !launcher.is_file() {
            bail!(
                "`claude install {}` exited with {} and left no {}: {}",
                self.channel,
                out.status,
                launcher.display(),
                truncate(said.trim(), 600)
            );
        }
        Ok((launcher, version))
    }
}

/// The client releases are downloaded with. It follows a redirect only to
/// an https address, as the shared-data downloader does (`seed::client`), so
/// an https release site can't send the binary's download to plain http.
fn client() -> Result<reqwest::Client> {
    let https_only =
        reqwest::redirect::Policy::custom(|a| if a.previous().len() < 5 && a.url().scheme() == "https" { a.follow() } else { a.stop() });
    Ok(reqwest::Client::builder()
        .user_agent(crate::http::USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .redirect(https_only)
        .build()?)
}

/// Removes the download when the install ends, however it ends.
struct Remove(PathBuf);

impl Drop for Remove {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Anthropic's name for this platform's build: `linux-x64`, `linux-arm64`,
/// and `-musl` on Alpine and the like.
pub fn platform() -> Result<String> {
    if !cfg!(target_os = "linux") {
        bail!(
            "installing Claude Code from UpNext is for Linux (its container); elsewhere use Anthropic's installer: https://code.claude.com/docs/en/setup"
        );
    }
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => bail!("Claude Code has no Linux build for {other}"),
    };
    let musl =
        std::fs::read_dir("/lib").map(|d| d.flatten().any(|e| e.file_name().to_string_lossy().starts_with("ld-musl-"))).unwrap_or(false);
    Ok(format!("linux-{arch}{}", if musl { "-musl" } else { "" }))
}

/// `1.2.3`, or with a suffix such as `1.2.3-beta.1`: what a channel names.
fn is_version(v: &str) -> bool {
    let (core, suffix) = v.split_once('-').unwrap_or((v, ""));
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        && suffix.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.')
        && (suffix.is_empty() == !v.contains('-'))
}

async fn get(http: &reqwest::Client, url: &str) -> Result<reqwest::Response> {
    let res = http.get(url).send().await.with_context(|| format!("reaching {url}"))?;
    if !res.status().is_success() {
        bail!("{url} answered {}", res.status());
    }
    Ok(res)
}

async fn text(http: &reqwest::Client, url: &str) -> Result<String> {
    Ok(get(http, url).await?.text().await.with_context(|| format!("reading {url}"))?.trim().to_string())
}

/// Save `url` to `file`; its SHA-256 in hex.
async fn download(http: &reqwest::Client, url: &str, file: &Path) -> Result<String> {
    use tokio::io::AsyncWriteExt;
    let mut res = get(http, url).await?;
    let mut out = tokio::fs::File::create(file).await.with_context(|| format!("writing {}", file.display()))?;
    let mut hash = Sha256::new();
    while let Some(chunk) = res.chunk().await.with_context(|| format!("downloading {url}"))? {
        hash.update(&chunk);
        out.write_all(&chunk).await.with_context(|| format!("writing {}", file.display()))?;
    }
    out.flush().await?;
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(unix)]
fn executable(file: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o755)).with_context(|| format!("making {} executable", file.display()))
}

#[cfg(not(unix))]
fn executable(_: &Path) -> Result<()> {
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

/// `serve` with `UPNEXT_INSTALL_CLAUDE=1`: install Claude Code in the
/// background unless `claude_bin` is found already, saying so in the log, and
/// keep `progress` up to date for setup. The server answers meanwhile.
pub fn start(cfg: &crate::config::Config, progress: std::sync::Arc<Progress>) {
    if let Some((found, _)) = crate::config::find_program(&cfg.claude_bin) {
        println!("Claude Code is at {} (UPNEXT_INSTALL_CLAUDE=1; it updates itself).", found.display());
        return;
    }
    let installer = match Installer::new(&cfg.cache_dir) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("Claude Code wasn't installed: {e:#}");
            progress.set(Some(InstallState { installing: false, error: Some(format!("{e:#}")) }));
            return;
        }
    };
    progress.set(Some(InstallState { installing: true, error: None }));
    println!("Installing Claude Code from {} into {} (UPNEXT_INSTALL_CLAUDE=1)…", installer.releases, installer.home.display());
    tokio::spawn(async move {
        // A container's network can lag its start, so a failed try gets two more.
        let mut tries = 1;
        loop {
            match installer.install().await {
                Ok((path, version)) => {
                    println!("Claude Code {version} is installed at {}.", path.display());
                    progress.set(None);
                }
                Err(e) if tries < 3 => {
                    eprintln!("Installing Claude Code: {e:#}; trying again in {} s.", 10 * tries);
                    tokio::time::sleep(Duration::from_secs(10 * tries)).await;
                    tries += 1;
                    continue;
                }
                Err(e) => {
                    eprintln!("Claude Code wasn't installed: {e:#}. `docker exec upnext upnext install-claude` tries again.");
                    progress.set(Some(InstallState { installing: false, error: Some(format!("{e:#}")) }));
                }
            }
            break;
        }
    });
}

/// `upnext install-claude`: install now, in the foreground, whether or not
/// one is found already (a newer one replaces it).
pub async fn run(cfg: &crate::config::Config) -> Result<()> {
    let installer = Installer::new(&cfg.cache_dir)?;
    println!("Installing Claude Code from {} into {}…", installer.releases, installer.home.display());
    let (path, version) = installer.install().await?;
    println!("Claude Code {version} is installed at {}.", path.display());
    if std::env::var_os("CLAUDE_CODE_OAUTH_TOKEN").is_none() {
        println!("{}", sign_in_advice(crate::flatpak::get()));
    }
    Ok(())
}

/// The home folder Claude Code goes into: the app's own folder in a
/// Flatpak, else the user's (`home`).
fn install_home(sandbox: Option<&crate::flatpak::Flatpak>, home: impl FnOnce() -> Option<PathBuf>) -> Result<PathBuf> {
    match sandbox {
        Some(f) => Ok(f.home.clone()),
        None => home().context("no home folder to install Claude Code into"),
    }
}

/// How to sign in the Claude Code just installed. A Flatpak's environment
/// is set with `flatpak override`, which keeps it for every start.
fn sign_in_advice(sandbox: Option<&crate::flatpak::Flatpak>) -> String {
    let token = "Sign it in with a token: `claude setup-token` on any computer where you use Claude prints one";
    match sandbox {
        Some(f) => format!(
            "{token}; give it to UpNext with `flatpak override --user --env=CLAUDE_CODE_OAUTH_TOKEN=<the token> {}`, then start UpNext again.",
            f.id
        ),
        None => format!("{token}; give it to UpNext as CLAUDE_CODE_OAUTH_TOKEN."),
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// A stand-in for the Claude Code binary: `install` copies itself to
    /// where the real one puts its command, as `claude install` does.
    const FAKE: &str = "#!/bin/sh\ncase \"$1\" in\n  install) mkdir -p \"$HOME/.local/bin\" && cp \"$0\" \"$HOME/.local/bin/claude\" && echo \"installed $2\" ;;\n  --version) echo '2.1.285 (Claude Code)' ;;\nesac\n";

    async fn releases(binary: &[u8], checksum: &str) -> MockServer {
        let server = MockServer::start().await;
        let platform = platform().unwrap();
        Mock::given(method("GET"))
            .and(path("/stable"))
            .respond_with(ResponseTemplate::new(200).set_body_string("2.1.285\n"))
            .mount(&server)
            .await;
        let manifest =
            serde_json::json!({"version": "2.1.285", "platforms": {platform.clone(): {"checksum": checksum, "size": binary.len()}}});
        Mock::given(method("GET"))
            .and(path("/2.1.285/manifest.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(manifest))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/2.1.285/{platform}/claude")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(binary.to_vec()))
            .mount(&server)
            .await;
        server
    }

    fn installer(server: &MockServer, dir: &tempfile::TempDir) -> Installer {
        Installer { releases: server.uri(), channel: CHANNEL.into(), home: dir.path().join("home"), work: dir.path().join("cache") }
    }

    #[tokio::test]
    async fn installs_a_checked_download_with_its_own_installer() {
        let dir = tempfile::tempdir().unwrap();
        let sum = Sha256::digest(FAKE.as_bytes()).iter().map(|b| format!("{b:02x}")).collect::<String>();
        let server = releases(FAKE.as_bytes(), &sum).await;
        let i = installer(&server, &dir);
        let (path, version) = i.install().await.unwrap();
        assert_eq!((path.clone(), version.as_str()), (dir.path().join("home/.local/bin/claude"), "2.1.285"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), FAKE);
        assert_eq!(std::fs::read_dir(dir.path().join("cache")).unwrap().count(), 0, "the download is removed");
    }

    #[tokio::test]
    async fn a_download_unlike_its_manifest_is_never_run() {
        let dir = tempfile::tempdir().unwrap();
        let server = releases(FAKE.as_bytes(), &"0".repeat(64)).await;
        let err = installer(&server, &dir).install().await.unwrap_err().to_string();
        assert!(err.contains("doesn't match its manifest"), "{err}");
        assert!(!dir.path().join("home/.local/bin/claude").exists());
        assert_eq!(std::fs::read_dir(dir.path().join("cache")).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn a_channel_that_names_no_version_stops_it() {
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/stable"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>"))
            .mount(&server)
            .await;
        let err = installer(&server, &dir).install().await.unwrap_err().to_string();
        assert!(err.contains("didn't name a version"), "{err}");
        let gone = installer(&MockServer::start().await, &dir).install().await.unwrap_err().to_string();
        assert!(gone.contains("answered 404"), "{gone}");
    }

    /// A redirect is followed only to https, so a release site can't send
    /// the download on to plain http (here, back to the mock server).
    #[tokio::test]
    async fn a_redirect_to_plain_http_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let sum = Sha256::digest(FAKE.as_bytes()).iter().map(|b| format!("{b:02x}")).collect::<String>();
        let server = releases(FAKE.as_bytes(), &sum).await;
        let moved = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/stable"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", format!("{}/stable", server.uri()).as_str()))
            .mount(&moved)
            .await;
        let err = installer(&moved, &dir).install().await.unwrap_err().to_string();
        assert!(err.contains("answered 302"), "{err}");
        assert!(!dir.path().join("home/.local/bin/claude").exists());
    }

    #[test]
    fn a_flatpak_installs_into_its_own_folder_and_is_told_how_to_set_the_token() {
        let sandbox = crate::flatpak::Flatpak::example();
        let home = || Some(PathBuf::from("/home/you"));
        assert_eq!(install_home(None, home).unwrap(), Path::new("/home/you"));
        assert_eq!(install_home(Some(&sandbox), home).unwrap(), sandbox.home);
        assert!(install_home(None, || None).is_err());

        assert!(sign_in_advice(None).ends_with("give it to UpNext as CLAUDE_CODE_OAUTH_TOKEN."));
        assert!(
            sign_in_advice(Some(&sandbox))
                .contains("`flatpak override --user --env=CLAUDE_CODE_OAUTH_TOKEN=<the token> io.github.zytocc.UpNext`")
        );
    }

    #[test]
    fn versions() {
        for ok in ["2.1.285", "10.0.1", "2.1.0-beta.1"] {
            assert!(is_version(ok), "{ok}");
        }
        for bad in ["", "2.1", "2.1.x", "../x/1.2.3", "1.2.3-", "1.2.3 ", "1.2.3-a/b"] {
            assert!(!is_version(bad), "{bad}");
        }
    }
}
