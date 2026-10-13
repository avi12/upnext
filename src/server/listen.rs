//! The listener: plain HTTP and, with a certificate, HTTPS on the
//! same port, on loopback or on every address ("Allow other devices"),
//! changed while the server runs.
//!
//! - One port for both: a connection whose first byte starts a TLS
//!   handshake (0x16) gets HTTPS, anything else plain HTTP. This computer's
//!   browser and the desktop app keep using `http://127.0.0.1:8787`.
//! - Each connection carries its [`Peer`]: the address, and whether it came
//!   over TLS (`access` refuses plain HTTP from a public address).
//! - Settings that change what it listens on (`allow_other_devices`,
//!   `tls_cert`, `tls_key`) are applied by [`Control::apply`]: the old socket
//!   closes and the new one opens; open connections carry on.

use super::AppState;
use crate::config::Config;
use anyhow::{Context, Result, bail};
use axum::extract::connect_info::Connected;
use axum::serve::IncomingStream;
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use serde::Serialize;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio_rustls::rustls;
use tokio_rustls::server::TlsStream;

/// Who's on the other end of a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Peer {
    pub addr: SocketAddr,
    /// It came over HTTPS.
    pub tls: bool,
}

impl Peer {
    pub fn plain(addr: SocketAddr) -> Self {
        Self { addr, tls: false }
    }
}

impl Connected<IncomingStream<'_, Listener>> for Peer {
    fn connect_info(stream: IncomingStream<'_, Listener>) -> Self {
        *stream.remote_addr()
    }
}

/// Whether an address is on this computer or a private network: loopback,
/// the private IPv4 ranges, link-local, the 100.64/10 range Tailscale and
/// carrier NAT use, and IPv6's unique local and link-local addresses.
/// Plain HTTP is answered only from these.
pub fn is_private(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            v4.is_loopback() || v4.is_private() || v4.is_link_local() || (a == 100 && (64..128).contains(&b))
        }
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            v6.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    }
}

/// What the server listens on now, for Settings.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Listening {
    pub host: String,
    pub port: u16,
    /// A certificate is loaded: HTTPS works on the same port.
    pub https: bool,
    /// Why the last change didn't take (a port in use, a certificate that
    /// doesn't load). What's listed above is what's in effect.
    pub error: Option<String>,
}

impl Listening {
    pub fn beyond_loopback(&self) -> bool {
        !is_loopback_host(&self.host)
    }
}

/// The listener's state, shared with the handlers.
#[derive(Default)]
pub struct Control {
    now: Mutex<Option<Listening>>,
    /// To the running acceptor: apply the settings, and say when it's done.
    /// None in the router tests, which have no socket.
    acceptor: Mutex<Option<mpsc::Sender<oneshot::Sender<()>>>>,
}

impl Control {
    /// What's in effect. With no socket (the router tests), what the
    /// settings ask for.
    pub fn now(&self, cfg: &Config) -> Listening {
        self.now.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_else(|| Listening {
            host: wanted_host(cfg),
            port: cfg.port,
            https: cfg.tls_cert.is_some() && cfg.tls_key.is_some(),
            error: None,
        })
    }

    fn set(&self, l: Listening) {
        *self.now.lock().unwrap_or_else(|e| e.into_inner()) = Some(l);
    }

    /// Apply the listening settings now, and wait until they're in effect
    /// (or have failed, with `error` saying why).
    pub async fn apply(&self) {
        let acceptor = self.acceptor.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(tx) = acceptor {
            let (done, wait) = oneshot::channel();
            if tx.send(done).await.is_ok() {
                let _ = wait.await;
            }
        }
    }
}

/// Where the server should listen: `host`, or every address when "Allow
/// other devices" is on and `host` is loopback. A `host` beyond loopback
/// (`UPNEXT_HOST=0.0.0.0` in a container) wins.
pub fn wanted_host(cfg: &Config) -> String {
    if cfg.allow_other_devices && is_loopback_host(&cfg.host) { "0.0.0.0".to_string() } else { cfg.host.clone() }
}

/// The certificate and key from the settings, ready to serve. None without
/// them; an error when only one is set or they don't load.
pub fn load_tls(cfg: &Config) -> Result<Option<Arc<rustls::ServerConfig>>> {
    let (cert, key) = match (&cfg.tls_cert, &cfg.tls_key) {
        (None, None) => return Ok(None),
        (Some(c), Some(k)) => (c, k),
        _ => bail!("HTTPS needs both tls_cert and tls_key"),
    };
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(cert)
        .with_context(|| format!("reading the certificate {}", cert.display()))?
        .collect::<std::result::Result<_, _>>()
        .with_context(|| format!("reading the certificate {}", cert.display()))?;
    if certs.is_empty() {
        bail!("{} has no certificate in it (PEM, starting -----BEGIN CERTIFICATE-----)", cert.display());
    }
    let key = PrivateKeyDer::from_pem_file(key).with_context(|| format!("reading the private key {}", key.display()))?;
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let mut tls = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("the certificate and the private key don't go together")?;
    // axum serves HTTP/1.1 here.
    tls.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Some(Arc::new(tls)))
}

/// An accepted connection, plain or TLS.
pub enum Conn {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl AsyncRead for Conn {
    fn poll_read(self: Pin<&mut Self>, cx: &mut TaskContext<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Conn::Plain(s) => Pin::new(s).poll_read(cx, buf),
            Conn::Tls(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Conn {
    fn poll_write(self: Pin<&mut Self>, cx: &mut TaskContext<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Conn::Plain(s) => Pin::new(s).poll_write(cx, buf),
            Conn::Tls(s) => Pin::new(s).poll_write(cx, buf),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Conn::Plain(s) => Pin::new(s).poll_flush(cx),
            Conn::Tls(s) => Pin::new(s).poll_flush(cx),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Conn::Plain(s) => Pin::new(s).poll_shutdown(cx),
            Conn::Tls(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
    fn poll_write_vectored(self: Pin<&mut Self>, cx: &mut TaskContext<'_>, bufs: &[std::io::IoSlice<'_>]) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Conn::Plain(s) => Pin::new(s).poll_write_vectored(cx, bufs),
            Conn::Tls(s) => Pin::new(s).poll_write_vectored(cx, bufs),
        }
    }
    fn is_write_vectored(&self) -> bool {
        match self {
            Conn::Plain(s) => s.is_write_vectored(),
            Conn::Tls(s) => s.is_write_vectored(),
        }
    }
}

/// What `axum::serve` accepts from: connections the acceptor task has
/// taken in and, for HTTPS, shaken hands with.
pub struct Listener {
    rx: mpsc::Receiver<(Conn, Peer)>,
    local: SocketAddr,
}

impl axum::serve::Listener for Listener {
    type Io = Conn;
    type Addr = Peer;

    async fn accept(&mut self) -> (Conn, Peer) {
        match self.rx.recv().await {
            Some(c) => c,
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> std::io::Result<Peer> {
        Ok(Peer::plain(self.local))
    }
}

/// How long a new connection gets to send its first byte, and then to
/// finish a TLS handshake.
const FIRST_BYTE: Duration = Duration::from_secs(20);
const HANDSHAKE: Duration = Duration::from_secs(10);

/// Take over `socket`: accept on it, and on whatever the settings move it
/// to, until the returned listener is dropped.
pub fn start(socket: TcpListener, state: &AppState) -> Listener {
    let local = socket.local_addr().unwrap_or_else(|_| SocketAddr::from(([127, 0, 0, 1], state.base_cfg.port)));
    let (tx, rx) = mpsc::channel(64);
    let (control, requests) = mpsc::channel(4);
    let listen = &state.remote.listen;
    listen.set(Listening { host: local.ip().to_string(), port: local.port(), https: false, error: None });
    *listen.acceptor.lock().unwrap_or_else(|e| e.into_inner()) = Some(control);
    // A server that listens may say so on the network (`reconfigure`).
    state.remote.announcer.start();
    tokio::spawn(acceptor(socket, state.clone(), tx, requests));
    Listener { rx, local }
}

async fn acceptor(
    socket: TcpListener,
    state: AppState,
    conns: mpsc::Sender<(Conn, Peer)>,
    mut requests: mpsc::Receiver<oneshot::Sender<()>>,
) {
    let mut socket = Some(socket);
    let mut tls = None;
    // The settings at start: the app's own socket may need moving, and a
    // certificate loading.
    reconfigure(&state, &mut socket, &mut tls).await;
    loop {
        let accept = async {
            match &socket {
                Some(s) => s.accept().await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            accepted = accept => match accepted {
                Ok((tcp, addr)) => {
                    let tls = tls.clone();
                    let conns = conns.clone();
                    tokio::spawn(async move {
                        if let Some(conn) = admit(tcp, addr, tls).await {
                            let _ = conns.send(conn).await;
                        }
                    });
                }
                // Too many open files and the like: wait, as axum does.
                Err(e) => {
                    tracing::warn!("accepting a connection: {e}");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            },
            request = requests.recv() => {
                let Some(done) = request else { continue };
                reconfigure(&state, &mut socket, &mut tls).await;
                let _ = done.send(());
            }
            _ = conns.closed() => break,
        }
    }
}

/// A new connection: plain, or HTTPS when it opens with a TLS handshake and
/// there's a certificate. None when it never says anything or the
/// handshake fails.
async fn admit(tcp: TcpStream, addr: SocketAddr, tls: Option<Arc<rustls::ServerConfig>>) -> Option<(Conn, Peer)> {
    let Some(tls) = tls else { return Some((Conn::Plain(tcp), Peer::plain(addr))) };
    let mut first = [0u8; 1];
    match tokio::time::timeout(FIRST_BYTE, tcp.peek(&mut first)).await {
        Ok(Ok(1)) if first[0] == 0x16 => {}
        Ok(Ok(1)) => return Some((Conn::Plain(tcp), Peer::plain(addr))),
        _ => return None,
    }
    match tokio::time::timeout(HANDSHAKE, tokio_rustls::TlsAcceptor::from(tls).accept(tcp)).await {
        Ok(Ok(stream)) => Some((Conn::Tls(Box::new(stream)), Peer { addr, tls: true })),
        Ok(Err(e)) => {
            tracing::debug!("TLS handshake with {addr} failed: {e}");
            None
        }
        Err(_) => None,
    }
}

/// Bring the socket and the certificate in line with the settings, and
/// record what's in effect.
async fn reconfigure(state: &AppState, socket: &mut Option<TcpListener>, tls: &mut Option<Arc<rustls::ServerConfig>>) {
    let cfg = state.cfg();
    let mut now = state.remote.listen.now(&cfg);
    let mut errors = vec![];

    let port = socket.as_ref().and_then(|s| s.local_addr().ok()).map_or(now.port, |a| a.port());
    let want = wanted_host(&cfg);
    if socket.is_none() || want != now.host {
        // The old socket closes first: on Linux, every address and loopback
        // can't both hold the port.
        let old = now.host.clone();
        drop(socket.take());
        match TcpListener::bind((want.as_str(), port)).await {
            Ok(s) => {
                tracing::info!("listening on {want}:{port}");
                *socket = Some(s);
                now.host = want;
            }
            Err(e) => {
                errors.push(format!("couldn't listen on {want}:{port}: {e}"));
                match TcpListener::bind((old.as_str(), port)).await {
                    Ok(s) => *socket = Some(s),
                    Err(e) => errors.push(format!("and couldn't listen on {old}:{port} again: {e}; restart UpNext")),
                }
            }
        }
    }
    now.port = port;

    match load_tls(&cfg) {
        Ok(loaded) => *tls = loaded,
        Err(e) => {
            *tls = None;
            errors.push(format!("HTTPS is off: {e:#}"));
        }
    }
    now.https = tls.is_some();
    now.error = (!errors.is_empty()).then(|| errors.join("; "));
    if let Some(e) = &now.error {
        tracing::warn!("{e}");
    }
    state.remote.listen.set(now);
    super::network::announce_now(state);
}

pub(super) fn is_loopback_host(host: &str) -> bool {
    let h = host.trim_start_matches('[').trim_end_matches(']');
    h == "localhost" || h.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// Host without the port, IPv6 brackets kept ("[::1]:8787" -> "[::1]").
pub(super) fn host_name(host: &str) -> &str {
    if host.starts_with('[') {
        host.split_inclusive(']').next().unwrap_or(host)
    } else {
        host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_addresses() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.17.0.2",
            "192.168.0.42",
            "169.254.1.1",
            "100.101.102.103",
            "::1",
            "fd7a:115c::1",
            "fe80::1",
            "::ffff:192.168.1.5",
        ] {
            assert!(is_private(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "100.128.0.1", "172.32.0.1", "2001:db8::1", "::ffff:8.8.8.8"] {
            assert!(!is_private(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn other_devices_switch() {
        let mut cfg = Config::default();
        cfg.host = "127.0.0.1".into();
        assert_eq!(wanted_host(&cfg), "127.0.0.1");
        cfg.allow_other_devices = true;
        assert_eq!(wanted_host(&cfg), "0.0.0.0");
        cfg.host = "0.0.0.0".into();
        cfg.allow_other_devices = false;
        assert_eq!(wanted_host(&cfg), "0.0.0.0", "a container's host wins");
    }

    #[test]
    fn certificates_need_both_halves() {
        let mut cfg = Config::default();
        cfg.tls_cert = Some("/nowhere/cert.pem".into());
        assert!(load_tls(&cfg).unwrap_err().to_string().contains("both"));
        assert!(load_tls(&Config::default()).unwrap().is_none());
    }

    #[test]
    fn loopback_hosts() {
        for h in ["127.0.0.1:8787", "localhost:8787", "[::1]:8787", "localhost"] {
            assert!(is_loopback_host(host_name(h)), "{h}");
        }
        for h in ["evil.example:8787", "192.168.0.10:8787", "127.0.0.1.evil.example:8787"] {
            assert!(!is_loopback_host(host_name(h)), "{h}");
        }
    }
}
