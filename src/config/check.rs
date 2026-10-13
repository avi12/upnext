//! Checks for values typed in the app. Each returns the value to store.

use super::DEFAULT_OPENAI_BASE_URL;
use anyhow::{Result, anyhow, bail};

pub fn any(_key: &str, v: &str) -> Result<String> {
    Ok(v.to_string())
}

/// Galaxy's names for its integrations, comma-separated: `origin,xboxone`.
/// "ea" and "xbox" are taken for those two; GOG's own games can't be left out.
pub fn galaxy_skip(_key: &str, v: &str) -> Result<String> {
    let mut out: Vec<String> = vec![];
    for p in v.split(',').map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty()) {
        let p = match p.as_str() {
            "ea" | "ea app" => "origin".to_string(),
            "xbox" => "xboxone".to_string(),
            _ => p,
        };
        if p == "gog" {
            bail!("GOG's own games are always read from Galaxy");
        }
        if !p.chars().all(|c| c.is_ascii_alphanumeric()) {
            bail!("{p:?} isn't a Galaxy integration's name (origin, xboxone, uplay…)");
        }
        if !out.contains(&p) {
            out.push(p);
        }
    }
    Ok(out.join(","))
}

fn hex32(v: &str) -> bool {
    v.len() == 32 && v.chars().all(|c| c.is_ascii_hexdigit())
}

pub fn steam_api_key(_key: &str, v: &str) -> Result<String> {
    if !hex32(v) {
        bail!("a Steam Web API key is 32 hex characters");
    }
    Ok(v.to_uppercase())
}

pub fn rawg_api_key(_key: &str, v: &str) -> Result<String> {
    if !hex32(v) {
        bail!("a RAWG API key is 32 hex characters (get one at https://rawg.io/apidocs)");
    }
    Ok(v.to_lowercase())
}

pub fn steam_id(_key: &str, v: &str) -> Result<String> {
    if v.len() != 17 || !v.chars().all(|c| c.is_ascii_digit()) {
        bail!("STEAM_ID must be a 17-digit SteamID64");
    }
    Ok(v.to_string())
}

pub fn twitch(key: &str, v: &str) -> Result<String> {
    if v.len() < 10 || v.contains(char::is_whitespace) {
        bail!("{key} looks wrong (copy it from https://dev.twitch.tv/console/apps)");
    }
    Ok(v.to_string())
}

pub fn sampled_minutes(_key: &str, v: &str) -> Result<String> {
    let n: i64 = v.parse().map_err(|_| anyhow!("sampled_minutes must be a number"))?;
    if !(1..=100_000).contains(&n) {
        bail!("sampled_minutes must be between 1 and 100000");
    }
    Ok(n.to_string())
}

pub fn days(key: &str, v: &str) -> Result<String> {
    let n: i64 = v.parse().map_err(|_| anyhow!("{key} must be a number of days"))?;
    if !(1..=3650).contains(&n) {
        bail!("{key} must be between 1 and 3650 days");
    }
    Ok(n.to_string())
}

pub fn staple_hours(_key: &str, v: &str) -> Result<String> {
    let n: i64 = v.parse().map_err(|_| anyhow!("staple_hours must be a number of hours"))?;
    if !(1..=100_000).contains(&n) {
        bail!("staple_hours must be between 1 and 100000");
    }
    Ok(n.to_string())
}

pub fn pattern_min_games(_key: &str, v: &str) -> Result<String> {
    let n: i64 = v.parse().map_err(|_| anyhow!("pattern_min_games must be a number of games"))?;
    if !(2..=50).contains(&n) {
        bail!("pattern_min_games must be between 2 and 50");
    }
    Ok(n.to_string())
}

/// The discovery settings' numbers, each in its own range.
pub fn discovery(key: &str, v: &str) -> Result<String> {
    let (low, high, unit) = match key {
        "gem_min_positive" => (50, 100, "percent"),
        "gem_min_reviews" => (0, 1_000_000, "reviews"),
        "gem_max_hours" | "new_to_you_max_hours" => (1, 200, "hours"),
        "gem_count" => (1, 100, "games"),
        _ => (0, 100, "percent"),
    };
    let n: i64 = v.parse().map_err(|_| anyhow!("{key} must be a number of {unit}"))?;
    if !(low..=high).contains(&n) {
        bail!("{key} must be between {low} and {high} {unit}");
    }
    Ok(n.to_string())
}

pub fn max_candidates(_key: &str, v: &str) -> Result<String> {
    let n: usize = v.parse().map_err(|_| anyhow!("max_candidates must be a number"))?;
    if !(10..=2000).contains(&n) {
        bail!("max_candidates must be between 10 and 2000");
    }
    Ok(n.to_string())
}

pub fn flag(key: &str, v: &str) -> Result<String> {
    match super::parse_flag(v) {
        Some(on) => Ok(on.to_string()),
        None => bail!("{key} must be true or false"),
    }
}

/// `WIDTHxHEIGHT`, each 200 to 16384.
pub fn window_size(_key: &str, v: &str) -> Result<String> {
    let size = v.split_once(['x', 'X', '×']).and_then(|(w, h)| Some((w.trim().parse::<u32>().ok()?, h.trim().parse::<u32>().ok()?)));
    match size {
        Some((w, h)) if (200..=16384).contains(&w) && (200..=16384).contains(&h) => Ok(format!("{w}x{h}")),
        _ => bail!("window_size is WIDTHxHEIGHT, such as 1280x860, each between 200 and 16384"),
    }
}

/// A zoom in percent, 30 to 300 ("125%" too).
pub fn window_zoom(_key: &str, v: &str) -> Result<String> {
    let n: i64 = v.trim_end_matches('%').trim().parse().map_err(|_| anyhow!("window_zoom must be a percentage, such as 125"))?;
    if !(30..=300).contains(&n) {
        bail!("window_zoom must be between 30 and 300 (percent)");
    }
    Ok(n.to_string())
}

pub fn openai_base_url(_key: &str, v: &str) -> Result<String> {
    if !(v.starts_with("http://") || v.starts_with("https://")) || v.contains(char::is_whitespace) {
        bail!("the server address must start with http:// or https:// (for Ollama: {DEFAULT_OPENAI_BASE_URL})");
    }
    Ok(v.trim_end_matches('/').to_string())
}

/// The verdicts service's address: https://, a host, maybe a port and a
/// path, with no query. Saved without a trailing `/`. Plain http:// only
/// to this computer (tests, a local `workerd`): the snapshot has no
/// signature, so the https connection is all that says where it came from.
pub fn verdicts_url(_key: &str, v: &str) -> Result<String> {
    let ok = reqwest::Url::parse(v).ok().filter(|u| {
        let local = matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        (u.scheme() == "https" || (u.scheme() == "http" && local))
            && u.host_str().is_some()
            && u.query().is_none()
            && u.fragment().is_none()
    });
    if ok.is_none() || v.contains(char::is_whitespace) {
        bail!(
            "the verdicts service's address is https://, a name and maybe a port and a path (such as https://verdicts.example.org); \
             http:// only for localhost or 127.0.0.1"
        );
    }
    Ok(v.trim_end_matches('/').to_string())
}

/// The computer's name on the network: one to 63 bytes, a DNS label's
/// most, with no control characters, each run of spaces made one.
pub fn network_name(_key: &str, v: &str) -> Result<String> {
    let name = crate::db::clean_name(v);
    if name.is_empty() || name.len() > crate::announce::NAME_MAX {
        bail!("the name on the network is 1 to 63 bytes, such as \"den\" or \"Living room PC\"");
    }
    Ok(name)
}

/// The identity provider's issuer: https://, or plain http:// to this
/// computer or its network (a provider in the same homelab, or a test's
/// mock), with no query. Kept as typed, trailing `/` and all, since the
/// ID token's `iss` must be exactly the provider's own.
pub fn oidc_issuer(_key: &str, v: &str) -> Result<String> {
    let ok = reqwest::Url::parse(v).ok().filter(|u| {
        (u.scheme() == "https" || (u.scheme() == "http" && super::is_local_server(v)))
            && u.host_str().is_some()
            && u.query().is_none()
            && u.fragment().is_none()
    });
    if ok.is_none() || v.contains(char::is_whitespace) {
        bail!(
            "the issuer is the provider's https:// address, such as https://sso.den.example.org (http:// only on \
             this computer or its network)"
        );
    }
    Ok(v.to_string())
}

/// The provider's name on the "Sign in with" button: 1 to 40 characters.
pub fn oidc_name(_key: &str, v: &str) -> Result<String> {
    let name = crate::db::clean_name(v);
    if name.is_empty() || name.chars().count() > 40 {
        bail!("the provider's name is 1 to 40 characters, such as \"Authelia\"");
    }
    Ok(name)
}

/// Group names, comma-separated, as the provider names them.
pub fn groups(key: &str, v: &str) -> Result<String> {
    let mut out: Vec<&str> = vec![];
    for g in v.split(',').map(str::trim).filter(|g| !g.is_empty()) {
        if g.chars().any(char::is_control) {
            bail!("{key} is group names separated by commas");
        }
        if !out.contains(&g) {
            out.push(g);
        }
    }
    Ok(out.join(","))
}

/// Addresses and ranges, comma- or space-separated: `127.0.0.1`,
/// `10.0.0.2`, `172.16.0.0/12`, `fd7a:115c:a1e0::/48`.
pub fn trusted_proxies(_key: &str, v: &str) -> Result<String> {
    let list = crate::server::proxies::Trusted::parse(v)?;
    Ok(list.to_string())
}

pub fn public_url(_key: &str, v: &str) -> Result<String> {
    match super::PublicUrl::parse(v) {
        Some(u) => Ok(u.origin()),
        None => bail!(
            "the address other devices use is http:// or https://, a name and maybe a port, with no path \
             (such as http://den.lan:8787 or https://upnext.example.org)"
        ),
    }
}

pub fn no_spaces(key: &str, v: &str) -> Result<String> {
    if v.contains(char::is_whitespace) {
        bail!("{key} cannot contain spaces");
    }
    Ok(v.to_string())
}

pub fn openai_key(_key: &str, v: &str) -> Result<String> {
    if v.starts_with("sk-ant-") {
        bail!("that's an Anthropic key: it goes under Anthropic API key");
    }
    if !v.starts_with("sk-") || v.len() < 20 || v.contains(char::is_whitespace) {
        bail!("an OpenAI API key starts with sk- (create one at https://platform.openai.com/api-keys)");
    }
    Ok(v.to_string())
}

pub fn anthropic_api_key(_key: &str, v: &str) -> Result<String> {
    if !v.starts_with("sk-ant-") || v.contains(char::is_whitespace) {
        bail!("an Anthropic API key starts with sk-ant- (create one at https://platform.claude.com/settings/keys)");
    }
    Ok(v.to_string())
}
