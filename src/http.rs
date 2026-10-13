//! Shared HTTP plumbing: a per-host rate limiter and GET/POST with exponential
//! backoff on 429/403/5xx and transport errors.

use anyhow::{Result, anyhow};
use reqwest::{Client, Response, StatusCode};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Names UpNext and where to find it, as Wikimedia's User-Agent policy asks
/// of scripts (docs/THIRD_PARTY.md).
pub const USER_AGENT: &str = concat!("UpNext/", env!("CARGO_PKG_VERSION"), " (+https://github.com/zytocc/upnext)");

/// Redirects a client follows at most, as reqwest's default policy does.
const MAX_REDIRECTS: usize = 10;

pub fn client() -> Result<Client> {
    Ok(Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(45))
        .connect_timeout(Duration::from_secs(15))
        .gzip(true)
        .redirect(reqwest::redirect::Policy::custom(|a| {
            if a.previous().last().is_some_and(|from| downgrade(from, a.url())) {
                a.error("refused a redirect from https to plain http")
            } else if a.previous().len() > MAX_REDIRECTS {
                a.error("too many redirects")
            } else {
                a.follow()
            }
        }))
        .build()?)
}

/// Whether a redirect from `from` to `to` drops https: an https address,
/// such as the verdicts service whose snapshot only the connection
/// authenticates, mustn't be answered over plain http.
fn downgrade(from: &reqwest::Url, to: &reqwest::Url) -> bool {
    from.scheme() == "https" && to.scheme() == "http"
}

/// A response's body as text, read a piece at a time and given up past
/// `max` bytes, counted after unzipping. `Response::text` reads everything,
/// so a small gzip answer that unzips to gigabytes would fill memory.
pub async fn text_capped(mut r: Response, max: usize) -> Result<String> {
    let mut body = Vec::new();
    while let Some(chunk) = r.chunk().await? {
        if body.len() + chunk.len() > max {
            return Err(anyhow!("the answer is over {} MB", max >> 20));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// The base URL a client uses for one service: the real one, or `over` in
/// its place. Clients take one override for all the hosts they talk to, so a
/// test can point a whole client at a single local mock server.
pub fn base_url(real: &str, over: Option<&str>) -> String {
    over.unwrap_or(real).trim_end_matches('/').to_string()
}

/// Enforces a minimum interval between calls (per host / per client).
pub struct RateLimiter {
    /// The service, for "Steam asked us to slow down".
    service: &'static str,
    min_interval: Duration,
    next: Mutex<Instant>,
}

impl RateLimiter {
    pub fn new(service: &'static str, min_interval: Duration) -> Self {
        // Tests talk to local mock servers, which need no pacing.
        let min_interval = if cfg!(test) { Duration::ZERO } else { min_interval };
        Self { service, min_interval, next: Mutex::new(Instant::now()) }
    }

    /// Wait until the next call is allowed. False, at once, when the job
    /// running this is asked to stop meanwhile: a backoff can last two
    /// minutes, and a server shutting down gives a job seconds. Callers
    /// stop on false (`stopped`) rather than send the request anyway.
    #[must_use = "on false the job was asked to stop: return `http::stopped()` instead of sending"]
    pub async fn wait(&self) -> bool {
        let mut next = self.next.lock().await;
        loop {
            let now = Instant::now();
            if *next <= now {
                break;
            }
            if crate::jobs::stop_asked() {
                return false;
            }
            tokio::time::sleep((*next - now).min(Duration::from_millis(250))).await;
        }
        *next = Instant::now() + self.min_interval;
        true
    }

    /// How long until the next call is allowed.
    pub async fn ready_in(&self) -> Duration {
        self.next.lock().await.saturating_duration_since(Instant::now())
    }

    /// Push the next allowed call further out, e.g. after a 429.
    pub async fn penalize(&self, extra: Duration) {
        let mut next = self.next.lock().await;
        let candidate = Instant::now() + extra;
        if candidate > *next {
            *next = candidate;
        }
    }
}

tokio::task_local! {
    /// Set while someone waits on the answer (`quick`).
    static QUICK: ();
}

/// In `quick`: how long one request may take, and how long it may wait
/// for its turn with a limiter a backoff pushed out.
const QUICK_TIMEOUT: Duration = Duration::from_secs(8);
const QUICK_TURN: Duration = Duration::from_secs(5);

/// Runs `f` with each request in it tried once, within `QUICK_TIMEOUT`,
/// for someone waiting on the answer (adding a game to the wishlist by
/// name, checking a pick's suggestion): when a service is down, they hear
/// so at once instead of after a backoff of a minute or two, which stays
/// for the background jobs. A limiter that a backoff pushed further out
/// than `QUICK_TURN` fails it at once too. Its failures don't push the
/// limiter out for those jobs, unless the service asked to slow down.
pub async fn quick<F: std::future::Future>(f: F) -> F::Output {
    QUICK.scope((), f).await
}

fn is_quick() -> bool {
    QUICK.try_with(|_| ()).is_ok()
}

const STOPPED: &str = "stopped waiting: the job was asked to stop";

/// The error for a request not sent because `RateLimiter::wait` was cut short.
pub fn stopped() -> anyhow::Error {
    anyhow!(STOPPED)
}

pub struct Backoff {
    pub attempts: u32,
    pub base: Duration,
    pub max: Duration,
    /// Whether a 403 is the host asking to slow down (Steam's store and
    /// community pages, most CDNs) rather than a final no. An API that
    /// answers a refused key with 403 sets it false, so the key isn't asked
    /// again for minutes.
    pub retry_403: bool,
}

impl Default for Backoff {
    fn default() -> Self {
        Self { attempts: 5, base: Duration::from_secs(5), max: Duration::from_secs(120), retry_403: true }
    }
}

fn retryable(status: StatusCode, backoff: &Backoff) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS
        || (status == StatusCode::FORBIDDEN && backoff.retry_403)
        || status == StatusCode::REQUEST_TIMEOUT
        || status.is_server_error()
}

/// A non-success HTTP status, kept typed so callers can tell a 404 from a
/// timeout. The URL is shown without its query string (API keys live there).
#[derive(Debug, thiserror::Error)]
#[error("HTTP {status} for {url}")]
pub struct HttpStatusError {
    pub status: StatusCode,
    pub url: String,
}

/// The URL without query or fragment, safe to log.
pub fn safe_url(url: &reqwest::Url) -> String {
    let mut u = url.clone();
    u.set_query(None);
    u.set_fragment(None);
    u.to_string()
}

/// A response's body as JSON. reqwest names the request's URL, query and
/// all, in an error reading or decoding a body, and the Steam Web API and
/// RAWG take their key in the query, so the URL is left out: such an error
/// reaches the log and a job's message, which a read token can see.
pub async fn json<T: serde::de::DeserializeOwned>(resp: Response) -> Result<T, reqwest::Error> {
    resp.json().await.map_err(reqwest::Error::without_url)
}

/// True when the error is an HTTP 404 from `send_with_backoff`.
pub fn is_not_found(e: &anyhow::Error) -> bool {
    e.downcast_ref::<HttpStatusError>().is_some_and(|h| h.status == StatusCode::NOT_FOUND)
}

/// Send a request builder with rate limiting and backoff. The closure builds a
/// fresh request each attempt (builders are not reusable).
pub async fn send_with_backoff<F>(limiter: &RateLimiter, backoff: &Backoff, mut build: F) -> Result<Response>
where
    F: FnMut() -> reqwest::RequestBuilder,
{
    let quick = is_quick();
    let once = Backoff { attempts: 1, ..*backoff };
    let backoff = if quick { &once } else { backoff };
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 0..backoff.attempts {
        if quick {
            let turn = limiter.ready_in().await;
            if turn > QUICK_TURN {
                return Err(anyhow!(
                    "{} asked us to slow down; try again in {}",
                    limiter.service,
                    crate::enrich::plural(turn.as_secs() + 1, "second", "seconds")
                ));
            }
        }
        if !limiter.wait().await {
            return Err(match last_err {
                Some(e) => e.context(STOPPED),
                None => stopped(),
            });
        }
        let req = if quick { build().timeout(QUICK_TIMEOUT) } else { build() };
        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    return Ok(resp);
                }
                if !retryable(status, backoff) {
                    return Err(HttpStatusError { status, url: safe_url(resp.url()) }.into());
                }
                let retry_after = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| retry_after(v, backoff));
                let url = safe_url(resp.url());
                let delay = retry_after.unwrap_or_else(|| (backoff.base * 2u32.pow(attempt)).min(backoff.max));
                let slow_down = status == StatusCode::TOO_MANY_REQUESTS || status == StatusCode::FORBIDDEN;
                let note = if slow_down {
                    format!("{} asked us to slow down", limiter.service)
                } else {
                    format!("{} is having trouble (HTTP {})", limiter.service, status.as_u16())
                };
                let detail = format!("HTTP {status} for {url}");
                let delay = wait(backoff, attempt, note, delay, detail);
                last_err = Some(HttpStatusError { status, url }.into());
                // A quick try's failure leaves the background jobs' pace
                // alone, unless the service asked everyone to slow down.
                if !quick || slow_down || retry_after.is_some() {
                    limiter.penalize(delay).await;
                }
            }
            Err(e) => {
                let delay = (backoff.base * 2u32.pow(attempt)).min(backoff.max);
                let e = e.without_url();
                let delay = wait(backoff, attempt, format!("{} didn't answer", limiter.service), delay, e.to_string());
                last_err = Some(e.into());
                if !quick {
                    limiter.penalize(delay).await;
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("request failed")))
}

/// The wait a `Retry-After` header asks for, in seconds, up to twice the
/// backoff's longest: one that asks for a day would otherwise hold the
/// host's lane, and the job with it, for a day. A date, which no service
/// UpNext talks to sends, counts as no header.
fn retry_after(value: &str, backoff: &Backoff) -> Option<Duration> {
    value.trim().parse::<u64>().ok().map(|s| Duration::from_secs(s).min(backoff.max * 2))
}

/// Report a wait to the job's lane, or log it outside a job. Returns the
/// wait, which tests shorten: their mock servers don't need the time.
fn wait(backoff: &Backoff, attempt: u32, note: String, delay: Duration, detail: String) -> Duration {
    let tries = backoff.attempts;
    let last = attempt + 1 == tries;
    let detail = if last {
        format!("{detail}, gave up after {}", crate::enrich::plural(tries, "try", "tries"))
    } else {
        format!("{detail}, try {} of {tries}", attempt + 1)
    };
    let report = crate::jobs::Wait { note, delay, detail, last };
    if !crate::jobs::waiting(&report) {
        tracing::warn!("{}", report.text());
    }
    if cfg!(test) { Duration::from_millis(5) } else { delay }
}

/// Write a file only the current user can read (tokens, backups with
/// settings), atomically: a temp file next to it is renamed over it, so a
/// crash never leaves half a token behind. Sign-ins live in the database
/// now (`crate::signin`); the clients' own tests keep theirs in files.
#[cfg(test)]
pub fn write_private(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&tmp)?;
    f.write_all(contents)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, path)
}

/// Make an existing file readable by the current user only (no-op off Unix).
/// On Windows the files are under `%LOCALAPPDATA%`, which only the user (and
/// administrators) can read already.
pub fn make_private(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The code-health review's B-STORES-10: a `Retry-After` is honoured up to twice
    /// the backoff's longest wait, so one that asks for a day can't hold a
    /// lane for a day; a date or nonsense counts as no header.
    #[test]
    fn retry_after_is_capped() {
        let b = Backoff::default();
        assert_eq!(retry_after("30", &b), Some(Duration::from_secs(30)));
        assert_eq!(retry_after(" 7 ", &b), Some(Duration::from_secs(7)));
        assert_eq!(retry_after("86400", &b), Some(Duration::from_secs(240)));
        let short = Backoff { max: Duration::from_secs(30), ..Default::default() };
        assert_eq!(retry_after("86400", &short), Some(Duration::from_secs(60)));
        assert_eq!(retry_after("Wed, 21 Oct 2026 07:28:00 GMT", &b), None);
        assert_eq!(retry_after("-5", &b), None);
    }

    /// A quick try that fails leaves the limiter where it was, so the
    /// background jobs sharing it aren't held back by someone's one try.
    #[tokio::test]
    async fn a_quick_failure_leaves_the_limiter_alone() {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::any};
        let s = MockServer::start().await;
        Mock::given(any()).respond_with(ResponseTemplate::new(500)).mount(&s).await;
        let (http, limiter) = (client().unwrap(), RateLimiter::new("Steam", Duration::ZERO));
        for url in [s.uri(), "http://127.0.0.1:1".to_string()] {
            assert!(quick(send_with_backoff(&limiter, &Backoff::default(), || http.get(&url))).await.is_err());
            assert_eq!(limiter.ready_in().await, Duration::ZERO, "{url}");
        }
        assert_eq!(s.received_requests().await.unwrap().len(), 1);

        assert!(send_with_backoff(&limiter, &Backoff::default(), || http.get(s.uri())).await.is_err());
        assert_eq!(s.received_requests().await.unwrap().len(), 6, "a job's request is tried five times");
    }

    /// A redirect from https to plain http is refused; any other is followed.
    #[tokio::test]
    async fn redirects_never_drop_https() {
        use reqwest::Url;
        let u = |s: &str| Url::parse(s).unwrap();
        assert!(downgrade(&u("https://verdicts.example.org/verdicts.json"), &u("http://verdicts.example.org/verdicts.json")));
        assert!(!downgrade(&u("https://a.example.org/"), &u("https://b.example.org/")));
        assert!(!downgrade(&u("http://127.0.0.1:1/"), &u("http://127.0.0.1:2/")));

        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};
        let s = MockServer::start().await;
        let to = format!("{}/there", s.uri());
        Mock::given(path("/here")).respond_with(ResponseTemplate::new(302).insert_header("location", to.as_str())).mount(&s).await;
        Mock::given(path("/there")).respond_with(ResponseTemplate::new(200).set_body_string("ok")).mount(&s).await;
        let r = client().unwrap().get(format!("{}/here", s.uri())).send().await.unwrap();
        assert_eq!((r.status().as_u16(), r.url().path()), (200, "/there"));
    }
}
