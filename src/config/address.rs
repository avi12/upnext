//! Addresses: the `public_url` setting taken apart, and whether a server's
//! address is on this computer or its network.

/// Whether an address is on this computer or its network: loopback, a
/// private, shared (tailnet) or link-local address, or a name with no dot or
/// a local suffix (`box`, `den.lan`, `box.local`).
pub fn is_local_server(base_url: &str) -> bool {
    let Some(host) = reqwest::Url::parse(base_url).ok().and_then(|u| u.host_str().map(|h| h.trim_matches(['[', ']']).to_ascii_lowercase()))
    else {
        return false;
    };
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => {
            ip.is_loopback() || ip.is_private() || ip.is_link_local() || (ip.octets()[0] == 100 && (64..128).contains(&ip.octets()[1]))
        }
        Ok(std::net::IpAddr::V6(ip)) => ip.is_loopback() || (ip.segments()[0] & 0xfe00) == 0xfc00 || (ip.segments()[0] & 0xffc0) == 0xfe80,
        Err(_) => {
            !host.contains('.') || [".local", ".lan", ".home", ".internal", ".home.arpa", ".ts.net"].iter().any(|s| host.ends_with(s))
        }
    }
}

/// The `public_url` setting, taken apart: the scheme, and the host with its
/// port as a `Host` header names it (no default port, lowercase).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicUrl {
    pub https: bool,
    /// `den.lan:8787`, `upnext.example.org`, `[fd7a::1]:8787`.
    pub authority: String,
}

impl PublicUrl {
    /// `http://` or `https://`, a host and maybe a port, and nothing after
    /// but a `/`. UpNext is served from the root, so a path isn't one.
    pub fn parse(v: &str) -> Option<Self> {
        let v = v.trim();
        let (https, rest) = match v.split_once("://")? {
            (s, rest) if s.eq_ignore_ascii_case("https") => (true, rest),
            (s, rest) if s.eq_ignore_ascii_case("http") => (false, rest),
            _ => return None,
        };
        let authority = rest.strip_suffix('/').unwrap_or(rest).to_ascii_lowercase();
        let (host, port) = if authority.starts_with('[') {
            let end = authority.find(']')?;
            authority[1..end].parse::<std::net::Ipv6Addr>().ok()?;
            let port = match &authority[end + 1..] {
                "" => None,
                rest => Some(rest.strip_prefix(':')?),
            };
            (&authority[..=end], port)
        } else {
            match authority.split_once(':') {
                Some((h, p)) => (h, Some(p)),
                None => (authority.as_str(), None),
            }
        };
        let name_ok = host.starts_with('[')
            || (!host.is_empty() && host.split('.').all(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')));
        let port = match port {
            None => None,
            Some(p) => Some(p.parse::<u16>().ok().filter(|p| *p > 0)?),
        };
        if !name_ok {
            return None;
        }
        let authority = match port {
            Some(p) if p != if https { 443 } else { 80 } => format!("{host}:{p}"),
            _ => host.to_string(),
        };
        Some(Self { https, authority })
    }

    /// `https://upnext.example.org`, as a browser sends it in `Origin`.
    pub fn origin(&self) -> String {
        format!("{}://{}", if self.https { "https" } else { "http" }, self.authority)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, validate_setting};

    #[test]
    fn public_urls() {
        for (typed, saved) in [
            ("http://den.lan:8787", "http://den.lan:8787"),
            ("HTTPS://UpNext.Example.org/", "https://upnext.example.org"),
            ("https://upnext.example.org:443", "https://upnext.example.org"),
            ("http://192.168.0.5:80", "http://192.168.0.5"),
            ("http://[fd7a:115c::1]:8787", "http://[fd7a:115c::1]:8787"),
            ("http://den:8443", "http://den:8443"),
        ] {
            assert_eq!(validate_setting("public_url", typed).ok().as_deref(), Some(saved), "{typed}");
        }
        for bad in [
            "den.lan:8787",
            "ftp://den.lan",
            "http://",
            "http://den.lan/upnext",
            "http://den..lan",
            "http://den:0",
            "http://den:x",
            "http://[nope]:1",
            "http://a b",
        ] {
            assert!(validate_setting("public_url", bad).is_err(), "{bad}");
        }
        let u = PublicUrl::parse("https://upnext.example.org").unwrap();
        assert_eq!((u.https, u.authority.as_str()), (true, "upnext.example.org"));
        assert_eq!(PublicUrl::parse("http://[::1]:8787").unwrap().authority, "[::1]:8787");
    }

    /// Thinking is off by default only for a server
    /// on this computer or network.
    #[test]
    fn thinking_is_off_for_local_servers_by_default() {
        for local in [
            "http://127.0.0.1:11434/v1",
            "http://localhost:8000/v1",
            "http://192.168.0.42:8000/v1",
            "http://10.0.0.5/v1",
            "http://100.101.102.103:8000/v1",
            "http://[::1]:8000/v1",
            "http://box:8000/v1",
            "http://den.lan:8000/v1",
            "https://den.tailnet.ts.net/v1",
        ] {
            assert!(is_local_server(local), "{local}");
        }
        for hosted in ["https://api.openai.com/v1", "https://openrouter.ai/api/v1", "https://8.8.8.8/v1", "not a url"] {
            assert!(!is_local_server(hosted), "{hosted}");
        }
        let mut c = Config::built_in();
        assert_eq!(c.openai_thinking, "auto");
        c.openai_base_url = "http://192.168.0.42:8000/v1".into();
        assert_eq!(c.openai_thinking(), Some(false));
        c.openai_thinking = "on".into();
        assert_eq!(c.openai_thinking(), None, "the model's own default");
        c.openai_base_url = "https://openrouter.ai/api/v1".into();
        c.openai_thinking = "auto".into();
        assert_eq!(c.openai_thinking(), None);
        c.openai_thinking = "off".into();
        assert_eq!(c.openai_thinking(), Some(false));
    }
}
