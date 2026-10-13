//! Trusted proxies: the allow-list that lets UpNext
//! believe what a proxy in front of it says about a request. From a listed
//! address, the client's address comes from `Forwarded` or
//! `X-Forwarded-For` (for logs and the limits only), HTTPS from
//! `X-Forwarded-Proto` and the host from `X-Forwarded-Host`, so the
//! sign-in's way back and the host check come out right. From any
//! other address those headers are never believed (`access`).
//!
//! It decides access only, never identity: no header from any proxy names
//! a player (`Remote-User`, `X-Forwarded-User` and the like are ignored).
//! With "The proxy does the login" on, a listed proxy's request needs no
//! token and lands on "Who's playing?" (`access::Caller::Proxy`).

use super::AppState;
use crate::db::PlayerId;
use anyhow::{Result, bail};
use axum::http::HeaderMap;
use std::net::IpAddr;

/// One address or range of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Range {
    addr: IpAddr,
    bits: u8,
}

impl Range {
    fn parse(s: &str) -> Option<Self> {
        let (addr, bits) = match s.split_once('/') {
            Some((a, b)) => (a, Some(b.parse::<u8>().ok()?)),
            None => (s, None),
        };
        let addr: IpAddr = addr.trim_matches(['[', ']']).parse().ok()?;
        let max = if addr.is_ipv4() { 32 } else { 128 };
        let bits = bits.unwrap_or(max);
        (bits <= max).then(|| Self { addr: mask(addr, bits), bits })
    }

    fn contains(&self, ip: IpAddr) -> bool {
        let ip = ip.to_canonical();
        ip.is_ipv4() == self.addr.is_ipv4() && mask(ip, self.bits) == self.addr
    }
}

/// `ip` with only its first `bits` bits kept.
fn mask(ip: IpAddr, bits: u8) -> IpAddr {
    match ip {
        IpAddr::V4(v4) => {
            let m = if bits == 0 { 0 } else { u32::MAX << (32 - u32::from(bits)) };
            IpAddr::V4((u32::from(v4) & m).into())
        }
        IpAddr::V6(v6) => {
            let m = if bits == 0 { 0 } else { u128::MAX << (128 - u32::from(bits)) };
            IpAddr::V6((u128::from(v6) & m).into())
        }
    }
}

impl std::fmt::Display for Range {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let max = if self.addr.is_ipv4() { 32 } else { 128 };
        if self.bits == max { write!(f, "{}", self.addr) } else { write!(f, "{}/{}", self.addr, self.bits) }
    }
}

/// The `trusted_proxies` setting: the addresses and ranges whose
/// forwarding headers are believed. Empty by default, which is today's
/// behaviour exactly.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Trusted(Vec<Range>);

impl Trusted {
    /// Addresses and ranges, comma- or space-separated. A range wider than
    /// a /8 (or an IPv6 /32) is refused: trusting most of the internet's
    /// headers is a typo, not a setup.
    pub fn parse(v: &str) -> Result<Self> {
        let mut out: Vec<Range> = vec![];
        for part in v.split([',', ' ', '\t', '\n']).map(str::trim).filter(|p| !p.is_empty()) {
            let Some(r) = Range::parse(part) else {
                bail!("{part:?} isn't an address or a range: trusted_proxies takes ones such as 127.0.0.1, 10.0.0.2 or 172.16.0.0/12");
            };
            if r.bits < if r.addr.is_ipv4() { 8 } else { 32 } {
                bail!("{part} is too wide a range to trust: list the proxy's own address, or a /8 or narrower");
            }
            if !out.contains(&r) {
                out.push(r);
            }
        }
        Ok(Self(out))
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        self.0.iter().any(|r| r.contains(ip))
    }
}

impl std::fmt::Display for Trusted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let parts: Vec<String> = self.0.iter().map(Range::to_string).collect();
        f.write_str(&parts.join(", "))
    }
}

/// The household's list and switch, as the guard reads them: the app's
/// value, else the environment's. A list that doesn't parse (an older
/// value, a hand-edited database) trusts nobody.
pub fn trusted(state: &AppState) -> (Trusted, bool) {
    let get = |key: &str| state.db.setting_get(PlayerId::FIRST, key).ok().flatten();
    let list = get("trusted_proxies").or_else(|| state.base_cfg.trusted_proxies.clone()).unwrap_or_default();
    let list = Trusted::parse(&list).unwrap_or_else(|e| {
        tracing::warn!("trusted_proxies: {e:#}; trusting no proxy");
        Trusted::default()
    });
    let login = get("trusted_proxy_login").map_or(state.base_cfg.trusted_proxy_login, |v| v == "true");
    (list, login)
}

/// What a listed proxy says about a request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Forwarded {
    /// The client's address: the right-most hop in the chain that isn't a
    /// listed proxy itself, since each proxy appends the address it saw and
    /// everything left of the first listed one is the client's own say.
    /// None when that hop's address can't be read (`unknown`, a hidden
    /// name, garbage), which counts as a public client, never as the proxy.
    pub client: Option<IpAddr>,
    /// The proto that hop's proxy recorded said https.
    pub https: Option<bool>,
    /// The host the client asked that hop's proxy for.
    pub host: Option<String>,
}

/// The proxy's two kinds of headers said different things: a client behind
/// a proxy that sets one kind wrote the other itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Disagree;

/// One proxy's record of a request: the address it saw it from (None when
/// it can't be read), and the proto and host it was asked with.
#[derive(Debug, Default)]
struct Hop {
    addr: Option<IpAddr>,
    proto: Option<String>,
    host: Option<String>,
}

/// An address as a forwarding header gives it: `192.168.0.42`,
/// `192.168.0.42:5000`, `[fd7a::1]:5000`, `"[fd7a::1]"`.
fn address(s: &str) -> Option<IpAddr> {
    let s = s.trim().trim_matches('"');
    if let Ok(ip) = s.parse::<IpAddr>() {
        return Some(ip);
    }
    if let Some(rest) = s.strip_prefix('[') {
        return rest.split(']').next()?.parse().ok();
    }
    s.rsplit_once(':').and_then(|(ip, _)| ip.parse::<std::net::Ipv4Addr>().ok()).map(IpAddr::V4)
}

/// Every value of a header, comma-separated values split, in order.
fn values<'a>(headers: &'a HeaderMap, name: &str) -> Vec<&'a str> {
    headers.get_all(name).iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(',')).map(str::trim).collect()
}

/// `Forwarded` (RFC 7239): one element per proxy, each with what it saw.
fn forwarded_hops(headers: &HeaderMap) -> Vec<Hop> {
    let mut hops = vec![];
    for element in values(headers, "forwarded") {
        let mut hop = Hop::default();
        for pair in element.split(';') {
            let Some((k, v)) = pair.split_once('=') else { continue };
            let v = v.trim().trim_matches('"');
            match k.trim().to_ascii_lowercase().as_str() {
                "for" => hop.addr = address(v),
                "proto" => hop.proto = Some(v.to_ascii_lowercase()),
                "host" => hop.host = Some(v.to_string()),
                _ => {}
            }
        }
        hops.push(hop);
    }
    hops
}

/// The `X-Forwarded-*` headers: one address per proxy in
/// `X-Forwarded-For`. A proto or host list as long as the address list
/// lines up with it hop by hop; otherwise its right-most value, the one the
/// listed proxy in front of UpNext wrote, goes with the right-most hop.
fn x_forwarded_hops(headers: &HeaderMap) -> Vec<Hop> {
    let mut hops: Vec<Hop> = values(headers, "x-forwarded-for").into_iter().map(|a| Hop { addr: address(a), ..Hop::default() }).collect();
    let protos = values(headers, "x-forwarded-proto");
    let hosts = values(headers, "x-forwarded-host");
    if hops.is_empty() && (!protos.is_empty() || !hosts.is_empty()) {
        // No addresses: the proxy said only how it was asked, so the client
        // is unknown.
        hops.push(Hop::default());
    }
    let n = hops.len();
    let mut spread = |list: Vec<&str>, set: fn(&mut Hop, String)| {
        if list.len() == n {
            for (hop, v) in hops.iter_mut().zip(list) {
                set(hop, v.to_string());
            }
        } else if let (Some(v), Some(hop)) = (list.last(), hops.last_mut()) {
            set(hop, v.to_string());
        }
    };
    spread(protos, |h, v| h.proto = Some(v.to_ascii_lowercase()));
    spread(hosts, |h, v| h.host = Some(v));
    hops
}

/// The client's hop, and what it says: the right-most hop whose address
/// isn't a listed proxy (an address that can't be read is no proxy), else,
/// when every hop is a listed proxy, the first.
fn judge(hops: &[Hop], list: &Trusted) -> Forwarded {
    let Some(hop) = hops.iter().rev().find(|h| h.addr.is_none_or(|ip| !list.contains(ip))).or(hops.first()) else {
        return Forwarded::default();
    };
    let host = hop.host.clone().filter(|h| !h.is_empty() && h.chars().all(|c| c.is_ascii_alphanumeric() || "-.:[]".contains(c)));
    Forwarded { client: hop.addr, https: hop.proto.as_deref().map(|p| p == "https"), host }
}

/// Read what a listed proxy says. Each kind of header is read on its own;
/// when a request carries both, they must say the same, or a visitor behind
/// a proxy that sets only one kind could say what they liked with the other.
pub fn read(headers: &HeaderMap, list: &Trusted) -> Result<Forwarded, Disagree> {
    let rfc = forwarded_hops(headers);
    let x = x_forwarded_hops(headers);
    match (rfc.is_empty(), x.is_empty()) {
        (true, true) => Ok(Forwarded::default()),
        (false, true) => Ok(judge(&rfc, list)),
        (true, false) => Ok(judge(&x, list)),
        (false, false) => {
            let (a, b) = (judge(&rfc, list), judge(&x, list));
            if a == b { Ok(a) } else { Err(Disagree) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.append(*k, HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn the_list() {
        let t = Trusted::parse("127.0.0.1, 10.0.0.2 172.16.0.0/12,fd7a:115c:a1e0::/48, 127.0.0.1").unwrap();
        assert_eq!(t.to_string(), "127.0.0.1, 10.0.0.2, 172.16.0.0/12, fd7a:115c:a1e0::/48");
        assert!(t.contains("127.0.0.1".parse().unwrap()));
        assert!(t.contains("::ffff:127.0.0.1".parse().unwrap()), "a mapped address");
        assert!(!t.contains("127.0.0.2".parse().unwrap()));
        assert!(t.contains("172.31.255.1".parse().unwrap()));
        assert!(!t.contains("172.32.0.1".parse().unwrap()));
        assert!(t.contains("fd7a:115c:a1e0::5".parse().unwrap()));
        assert!(!t.contains("10.0.0.3".parse().unwrap()));
        assert_eq!(Trusted::parse("172.16.5.9/12").unwrap().to_string(), "172.16.0.0/12");
        assert_eq!(Trusted::parse("").unwrap(), Trusted::default());
        for bad in ["localhost", "10.0.0.0/33", "0.0.0.0/0", "::/0", "10.0.0.2/x"] {
            assert!(Trusted::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_client_is_the_right_most_unlisted_address() {
        let list = Trusted::parse("10.0.0.2, 10.0.0.3").unwrap();
        let read = |h: &HeaderMap| read(h, &list).unwrap();
        // A client that wrote its own X-Forwarded-For gets it believed no further than the first listed proxy.
        let h = headers(&[("x-forwarded-for", "203.0.113.9, 192.168.0.42"), ("x-forwarded-for", "10.0.0.3")]);
        assert_eq!(read(&h).client, Some("192.168.0.42".parse().unwrap()));
        let h = headers(&[("x-forwarded-for", "10.0.0.3")]);
        assert_eq!(read(&h).client, Some("10.0.0.3".parse().unwrap()), "only proxies: the first");
        let h = headers(&[("forwarded", "for=192.168.0.43;proto=https;host=upnext.lan, for=\"[fd7a::1]:5000\"")]);
        let f = read(&h);
        assert_eq!(f, Forwarded { client: Some("fd7a::1".parse().unwrap()), https: None, host: None }, "the client's own hop's");
        let h = headers(&[
            ("x-forwarded-proto", "http"),
            ("x-forwarded-host", "den.example.org:8443"),
            ("x-forwarded-for", "192.168.0.44:51000"),
        ]);
        let f = read(&h);
        assert_eq!(
            f,
            Forwarded { client: Some("192.168.0.44".parse().unwrap()), https: Some(false), host: Some("den.example.org:8443".into()) }
        );
        assert_eq!(read(&headers(&[("x-forwarded-host", "evil.example/path")])).host, None, "not a host");
        assert_eq!(read(&HeaderMap::new()), Forwarded::default());
    }

    #[test]
    fn proto_and_host_are_the_trusted_hops() {
        let list = Trusted::parse("10.0.0.2").unwrap();
        let read = |h: &HeaderMap| read(h, &list).unwrap();
        // The client wrote "https" and a host; the listed proxy appended what it saw.
        let h = headers(&[
            ("x-forwarded-for", "203.0.113.5, 198.51.100.7"),
            ("x-forwarded-proto", "https, http"),
            ("x-forwarded-host", "upnext.den.example.org, den.lan"),
        ]);
        let f = read(&h);
        assert_eq!((f.client, f.https, f.host.as_deref()), (Some("198.51.100.7".parse().unwrap()), Some(false), Some("den.lan")));
        // A proxy that sets one proto while the client wrote addresses: the proxy's, the right-most.
        let h = headers(&[("x-forwarded-for", "203.0.113.5, 198.51.100.7"), ("x-forwarded-proto", "https, http, http")]);
        assert_eq!(read(&h).https, Some(false));
        // Two listed proxies: the one that saw the client says how it was asked.
        let two = Trusted::parse("10.0.0.2, 10.0.0.3").unwrap();
        let h =
            headers(&[("forwarded", "for=192.168.0.42;proto=https;host=upnext.den.example.org, for=10.0.0.3;proto=http;host=10.0.0.2")]);
        let f = super::read(&h, &two).unwrap();
        assert_eq!((f.https, f.host.as_deref()), (Some(true), Some("upnext.den.example.org")));
    }

    #[test]
    fn an_address_that_cant_be_read_is_no_proxy() {
        let list = Trusted::parse("10.0.0.3").unwrap();
        let h = headers(&[("x-forwarded-for", "192.168.0.42, not-an-address, 10.0.0.3")]);
        assert_eq!(read(&h, &list).unwrap().client, None, "never skipped to the address left of it");
        let h = headers(&[("forwarded", "for=unknown")]);
        assert_eq!(read(&h, &list).unwrap().client, None);
        assert_eq!(
            read(&headers(&[("x-forwarded-proto", "https")]), &list).unwrap(),
            Forwarded { https: Some(true), ..Forwarded::default() }
        );
    }

    #[test]
    fn both_kinds_must_agree() {
        let list = Trusted::parse("10.0.0.2").unwrap();
        // Behind a proxy that sets only X-Forwarded-*, a visitor's own Forwarded can't say otherwise.
        let h =
            headers(&[("x-forwarded-for", "203.0.113.9"), ("x-forwarded-proto", "http"), ("forwarded", "for=192.168.0.42;proto=https")]);
        assert_eq!(read(&h, &list), Err(Disagree));
        let h = headers(&[("x-forwarded-for", "203.0.113.9"), ("forwarded", "for=203.0.113.9;proto=https")]);
        assert_eq!(read(&h, &list), Err(Disagree), "a proto only one kind gives");
        // A proxy that sets both, the same.
        let h =
            headers(&[("x-forwarded-for", "192.168.0.42"), ("x-forwarded-proto", "https"), ("forwarded", "for=192.168.0.42;proto=https")]);
        assert_eq!(read(&h, &list).unwrap().https, Some(true));
    }
}
