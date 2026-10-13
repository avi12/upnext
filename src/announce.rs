//! Finding UpNext on the home network: a server that
//! other devices may use says so over mDNS and DNS-SD as a `_upnext._tcp`
//! service, and a client (the app window's "Use the library on another
//! computer", the Android app) browses for it, so nobody types an address.
//!
//! The announcement carries only what a client needs to tell computers
//! apart and reach one: `v=1`, `id` (a random install id, so a renamed
//! computer is still the one it knew), `name` (the computer's name on the
//! network, editable in Settings), `tls` (0 or 1) and `ver` (UpNext's
//! version). Nothing about the library or its people.
//!
//! `mdns-sd` shares port 5353 with Avahi, Bonjour and Windows' own
//! responder. The host name in the service record is UpNext's own,
//! `upnext-<id>.local.`, never the computer's: a second responder claiming
//! the computer's own name with another set of addresses would make Avahi
//! rename the computer.

use anyhow::{Context, Result};
use mdns_sd::{IfKind, ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::Serialize;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The service type, with mDNS's domain.
pub const SERVICE: &str = "_upnext._tcp.local.";
/// The TXT record's version: a client skips a record of a version it
/// doesn't know.
const TXT_VERSION: &str = "1";
/// DNS labels are 63 bytes at most, and a service instance's name is one.
pub const NAME_MAX: usize = 63;

/// What a server says about itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Announcement {
    /// The install id (`network_id`).
    pub id: String,
    /// The computer's name on the network.
    pub name: String,
    pub port: u16,
    pub tls: bool,
    /// The address it listens on when that's one address, not every one:
    /// only that one is announced. None for every address.
    pub only: Option<IpAddr>,
}

impl Announcement {
    /// The TXT record's values.
    pub fn txt(&self) -> Vec<(&'static str, String)> {
        vec![
            ("v", TXT_VERSION.to_string()),
            ("id", self.id.clone()),
            ("name", self.name.clone()),
            ("tls", if self.tls { "1" } else { "0" }.to_string()),
            ("ver", env!("CARGO_PKG_VERSION").to_string()),
        ]
    }

    /// UpNext's own host name in the service record.
    fn host(&self) -> String {
        let short: String = self.id.chars().filter(char::is_ascii_alphanumeric).take(12).collect();
        format!("upnext-{}.local.", short.to_ascii_lowercase())
    }

    fn service(&self) -> Result<ServiceInfo> {
        let txt: HashMap<String, String> = self.txt().into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        let address = self.only.map(|ip| ip.to_string()).unwrap_or_default();
        let info = ServiceInfo::new(SERVICE, &self.name, &self.host(), address.as_str(), self.port, txt)
            .with_context(|| format!("announcing {:?} on the network", self.name))?;
        // Every address of every interface, kept up to date as they change.
        Ok(if self.only.is_none() { info.enable_addr_auto() } else { info })
    }
}

/// A computer found on the network.
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
pub struct Found {
    /// Its install id.
    pub id: String,
    pub name: String,
    /// UpNext's version there.
    pub version: String,
    /// The address to open: `http://192.168.0.42:8787`, or `https://…`.
    pub address: String,
}

impl Found {
    /// What a resolved record says, when it's a record of ours: the right
    /// TXT version, an id, and an address another computer can reach it at
    /// (`reachable`, best first).
    pub fn from_record(txt: &HashMap<String, String>, addresses: &[IpAddr], port: u16) -> Option<Self> {
        if txt.get("v").map(String::as_str) != Some(TXT_VERSION) {
            return None;
        }
        let id = txt.get("id").filter(|id| !id.is_empty())?.clone();
        let ip = addresses.iter().filter_map(|ip| reachable(*ip).map(|rank| (rank, ip))).min_by_key(|(rank, _)| *rank)?.1;
        let scheme = if txt.get("tls").map(String::as_str) == Some("1") { "https" } else { "http" };
        let host = match ip {
            IpAddr::V6(v6) => format!("[{v6}]"),
            v4 => v4.to_string(),
        };
        let name = txt.get("name").filter(|n| !n.trim().is_empty()).cloned().unwrap_or_else(|| host.clone());
        Some(Self { id, name, version: txt.get("ver").cloned().unwrap_or_default(), address: format!("{scheme}://{host}:{port}") })
    }
}

/// How good an address is for another computer to open, lower first; None
/// for one it can't use. A home network's IPv4 address comes first, since a
/// phone and a computer on one network always share it. Loopback is the
/// other computer itself, and a link-local IPv6 address needs its
/// interface, which an address in a URL can't carry everywhere.
fn reachable(ip: IpAddr) -> Option<u8> {
    match ip {
        IpAddr::V4(v4) if v4.is_loopback() || v4.is_unspecified() => None,
        IpAddr::V4(v4) if v4.is_private() || (v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1])) => Some(0),
        IpAddr::V4(v4) if v4.is_link_local() => Some(3),
        IpAddr::V4(_) => Some(1),
        IpAddr::V6(v6) if v6.is_loopback() || v6.is_unspecified() || (v6.segments()[0] & 0xffc0) == 0xfe80 => None,
        IpAddr::V6(_) => Some(2),
    }
}

/// The announcement in effect, if any. Only a running server announces
/// (`start`); the router tests, with no socket, never do.
#[derive(Default)]
pub struct Announcer {
    started: AtomicBool,
    now: Mutex<Option<(ServiceDaemon, Announcement)>>,
}

impl Announcer {
    /// From now on `set` announces: the server is listening.
    pub fn start(&self) {
        self.started.store(true, Ordering::Relaxed);
    }

    /// Announce `want`, or nothing; what was announced before goes first.
    /// A failure is logged, not returned: UpNext works without it, by
    /// address or QR code.
    pub fn set(&self, want: Option<Announcement>) {
        if !self.started.load(Ordering::Relaxed) {
            return;
        }
        let mut now = self.now.lock().unwrap_or_else(|e| e.into_inner());
        if now.as_ref().map(|(_, a)| a) == want.as_ref() {
            return;
        }
        if let Some((daemon, _)) = now.take() {
            // The goodbye packets go out before the daemon stops.
            let _ = daemon.shutdown();
        }
        let Some(want) = want else {
            tracing::info!("no longer announced on the network");
            return;
        };
        match announce(&want) {
            Ok(daemon) => {
                tracing::info!("announced on the network as {:?} ({SERVICE} on port {})", want.name, want.port);
                *now = Some((daemon, want));
            }
            Err(e) => tracing::warn!("couldn't announce UpNext on the network: {e:#}. Other devices can still use its address or QR code."),
        }
    }
}

fn announce(a: &Announcement) -> Result<ServiceDaemon> {
    let daemon = ServiceDaemon::new().context("starting mDNS")?;
    // Loopback is no use to another computer, and an address of it found
    // first would send a browser on the network to itself.
    for kind in [IfKind::LoopbackV4, IfKind::LoopbackV6] {
        daemon.disable_interface(kind).context("leaving loopback out")?;
    }
    daemon.register(a.service()?).context("registering the service")?;
    Ok(daemon)
}

/// A record as resolved so far: its TXT values, every address and the port.
type Record = (HashMap<String, String>, Vec<IpAddr>, u16);

/// The computers that announce UpNext, as found within `wait`, without
/// the one whose id is `own`. Blocking: the browse runs on mDNS's thread.
pub fn browse(wait: Duration, own: Option<&str>) -> Result<Vec<Found>> {
    let daemon = ServiceDaemon::new().context("starting mDNS")?;
    let events = daemon.browse(SERVICE).context("looking for UpNext on the network")?;
    let until = Instant::now() + wait;
    // Each record by its full name, with every address it was resolved
    // with: they arrive a few at a time, and a computer on two networks
    // answers once per network.
    let mut records: HashMap<String, Record> = HashMap::new();
    while let Some(left) = until.checked_duration_since(Instant::now()) {
        let Ok(event) = events.recv_timeout(left) else { break };
        if let ServiceEvent::ServiceResolved(r) = event {
            let txt: HashMap<String, String> = r.txt_properties.iter().map(|p| (p.key().to_string(), p.val_str().to_string())).collect();
            let record = records.entry(r.fullname.clone()).or_insert_with(|| (HashMap::new(), Vec::new(), r.port));
            record.0 = txt;
            record.1.extend(r.addresses.iter().map(|a| a.to_ip_addr()));
            record.2 = r.port;
        }
    }
    let _ = daemon.shutdown();
    let mut found: Vec<Found> = Vec::new();
    for (txt, addresses, port) in records.values() {
        let Some(f) = Found::from_record(txt, addresses, *port) else { continue };
        if Some(f.id.as_str()) != own && !found.iter().any(|k| k.id == f.id) {
            found.push(f);
        }
    }
    found.sort_by_key(|f| f.name.to_lowercase());
    Ok(found)
}

/// This computer's name, as the network already shows it: the host name
/// (`COMPUTERNAME` on Windows), else "UpNext".
pub fn host_name() -> String {
    let from_env = std::env::var("COMPUTERNAME").ok().filter(|_| cfg!(windows));
    let from_file = || std::fs::read_to_string("/proc/sys/kernel/hostname").or_else(|_| std::fs::read_to_string("/etc/hostname")).ok();
    let name = from_env.or_else(from_file).or_else(|| std::env::var("HOSTNAME").ok()).unwrap_or_default();
    let name = name.trim().split('.').next().unwrap_or_default().to_string();
    if name.is_empty() || name == "localhost" { "UpNext".to_string() } else { name }
}

/// A new install id: 16 hex digits from the system's randomness.
pub fn new_id() -> Result<String> {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).map_err(|e| anyhow::anyhow!("no randomness for an install id: {e}"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn den() -> Announcement {
        Announcement { id: "4f2a9c01d3e85b67".into(), name: "den".into(), port: 8787, tls: false, only: None }
    }

    #[test]
    fn the_txt_record_says_only_what_a_client_needs() {
        let txt = den().txt();
        let keys: Vec<&str> = txt.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, ["v", "id", "name", "tls", "ver"]);
        assert_eq!(txt[0].1, "1");
        assert_eq!(txt[3].1, "0");
        assert_eq!(Announcement { tls: true, ..den() }.txt()[3].1, "1");
        assert_eq!(den().host(), "upnext-4f2a9c01d3e8.local.");
        let info = den().service().unwrap();
        assert_eq!(info.get_fullname(), "den._upnext._tcp.local.");
        assert_eq!(info.get_port(), 8787);
        assert_eq!(info.get_property_val_str("id"), Some("4f2a9c01d3e85b67"));
    }

    #[test]
    fn a_record_becomes_an_address_to_open() {
        let txt: HashMap<String, String> = den().txt().into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        let v6: IpAddr = "fe80::1".parse().unwrap();
        let v4: IpAddr = "192.168.0.42".parse().unwrap();
        let f = Found::from_record(&txt, &[v6, v4], 8787).unwrap();
        assert_eq!(f.address, "http://192.168.0.42:8787");
        assert_eq!((f.id.as_str(), f.name.as_str()), ("4f2a9c01d3e85b67", "den"));
        let mut tls = txt.clone();
        tls.insert("tls".into(), "1".into());
        let ula: IpAddr = "fd00::42".parse().unwrap();
        assert_eq!(Found::from_record(&tls, &[v6, ula], 8443).unwrap().address, "https://[fd00::42]:8443");
        // Another version's record, or one with no id or address, is skipped.
        let mut v2 = txt.clone();
        v2.insert("v".into(), "2".into());
        assert_eq!(Found::from_record(&v2, &[v4], 8787), None);
        let mut no_id = txt.clone();
        no_id.remove("id");
        assert_eq!(Found::from_record(&no_id, &[v4], 8787), None);
        assert_eq!(Found::from_record(&txt, &[], 8787), None);
        // Loopback is never another computer's address; a home network's
        // comes before any other.
        let lo: IpAddr = "127.0.0.1".parse().unwrap();
        let public: IpAddr = "203.0.113.9".parse().unwrap();
        assert_eq!(Found::from_record(&txt, &[lo, "::1".parse().unwrap(), v6], 8787), None);
        assert_eq!(Found::from_record(&txt, &[lo, public, v4], 8787).unwrap().address, "http://192.168.0.42:8787");
        assert_eq!(Found::from_record(&txt, &[lo, ula, public], 8787).unwrap().address, "http://203.0.113.9:8787");
    }

    #[test]
    fn a_host_name_and_an_id() {
        assert!(!host_name().is_empty());
        let id = new_id().unwrap();
        assert_eq!(id.len(), 16);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// Announce, then find it, over this machine's network. Containers and
    /// CI runners often carry no multicast, so this runs by hand:
    /// `cargo test --lib announce -- --ignored`.
    #[test]
    #[ignore = "needs multicast on this machine's network"]
    fn announced_then_found() {
        let announcer = Announcer::default();
        announcer.start();
        let a = Announcement { id: new_id().unwrap(), name: "den test".into(), ..den() };
        announcer.set(Some(a.clone()));
        let found = browse(Duration::from_secs(5), None).unwrap();
        assert!(found.iter().any(|f| f.id == a.id && f.name == "den test"), "{found:?}");
        assert!(browse(Duration::from_secs(2), Some(&a.id)).unwrap().iter().all(|f| f.id != a.id), "its own id is left out");
        announcer.set(None);
    }
}
