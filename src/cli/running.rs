//! Commands that ask the running server (`upnext serve`) on this computer,
//! for a container or any server without a desktop:
//! `healthcheck`, the image's health check, and `pair`, a pairing code for
//! a browser when no browser counts as this computer, with `pair requests`,
//! `approve` and `deny` for the devices that ask to be allowed.

use crate::config::Config;
use crate::server::pairing::{Pairing, how_to_use};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::time::Duration;

/// The server on this computer's loopback, straight there whatever proxy
/// the environment names.
fn local(port: u16) -> Result<(reqwest::Client, String)> {
    let http = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(5)).build()?;
    Ok((http, format!("http://127.0.0.1:{port}")))
}

fn unreachable(port: u16, e: reqwest::Error) -> anyhow::Error {
    anyhow::anyhow!("no UpNext answers on 127.0.0.1:{port} ({e}): is `upnext serve` running, on this port?")
}

/// `upnext healthcheck`: `GET /api/health` answers as UpNext. Exits 1
/// otherwise, which marks the container unhealthy. The image has no curl.
pub async fn healthcheck(cfg: &Config, port: Option<u16>) -> Result<()> {
    let port = port.unwrap_or(cfg.port);
    let (http, base) = local(port)?;
    let res = http.get(format!("{base}/api/health")).send().await.map_err(|e| unreachable(port, e))?;
    if !res.status().is_success() {
        bail!("/api/health on port {port} answered {}", res.status());
    }
    let v: Value = res.json().await.with_context(|| format!("/api/health on port {port} isn't UpNext's"))?;
    if v["app"] != "upnext" {
        bail!("port {port} answers, but not as UpNext");
    }
    println!("{}", answer_line(&v));
    Ok(())
}

/// What `healthcheck` prints for `/api/health`'s answer: "ok: UpNext 0.1.0",
/// with the commit after it for a build that knows it. The Linux packages'
/// smoke test (contrib/linux/smoke-test.sh) reads this line.
fn answer_line(health: &Value) -> String {
    format!("ok: UpNext {}", crate::build_id::describe(health["version"].as_str().unwrap_or("?"), health["build"].as_str()))
}

/// `upnext pair`: a new pairing code from the running server, as Settings'
/// Add a device makes one, valid for five minutes; any earlier code ends.
/// This is how a container's next browser pairs: `docker exec upnext upnext pair`.
pub async fn pair(cfg: &Config, port: Option<u16>) -> Result<()> {
    let port = port.unwrap_or(cfg.port);
    let (http, base) = local(port)?;
    let res = http.post(format!("{base}/api/v1/pairing")).send().await.map_err(|e| unreachable(port, e))?;
    let status = res.status();
    let v: Value = res.json().await.unwrap_or_default();
    if !status.is_success() {
        let why = v["error"].as_str().unwrap_or_default();
        bail!("the server on port {port} didn't make a code: {status} {why}");
    }
    let p = Pairing {
        code: v["code"].as_str().context("no code in the answer")?.to_string(),
        expires_at: v["expires_at"].as_i64().unwrap_or_default(),
        address: v["address"].as_str().map(str::to_string),
        qr_svg: None,
    };
    println!("{}", how_to_use(&p, port));
    // The server offers one address. When it listens on all of this
    // computer's and chose the one this command would, the rest are worth
    // a try from a device on another of its networks.
    let every_address = crate::server::listen::wanted_host(cfg).parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_unspecified());
    if !cfg.container
        && every_address
        && let Some(line) = p.address.as_deref().and_then(|a| other_addresses(a, &crate::server::lan::addresses()))
    {
        println!("{line}");
    }
    Ok(())
}

/// "Or try …" with the addresses after the first, when `address` is the
/// first of them (`http://192.168.0.42:8787` with 192.168.0.42 first).
fn other_addresses(address: &str, ips: &[std::net::IpAddr]) -> Option<String> {
    let (first, rest) = ips.split_first()?;
    let host = |ip: &std::net::IpAddr| match ip {
        std::net::IpAddr::V6(v6) => format!("[{v6}]"),
        v4 => v4.to_string(),
    };
    let (scheme, port) = address.split_once("://").and_then(|(scheme, rest)| {
        let (h, port) = rest.trim_end_matches('/').rsplit_once(':')?;
        (h == host(first)).then_some((scheme, port))
    })?;
    if rest.is_empty() {
        return None;
    }
    let others: Vec<String> = rest.iter().map(|ip| format!("{scheme}://{}:{port}", host(ip))).collect();
    Some(format!("If that address doesn't open, this computer is also at {}.", others.join(", ")))
}

/// The devices waiting to be allowed, as the server lists them.
async fn waiting(http: &reqwest::Client, base: &str, port: u16) -> Result<Vec<Value>> {
    let res = http.get(format!("{base}/api/v1/pairing/requests")).send().await.map_err(|e| unreachable(port, e))?;
    let status = res.status();
    let v: Value = res.json().await.unwrap_or_default();
    if !status.is_success() {
        bail!("the server on port {port} didn't list the devices: {status} {}", v["error"].as_str().unwrap_or_default());
    }
    Ok(v["requests"].as_array().cloned().unwrap_or_default())
}

/// One device as `pair requests` lists it: `4821  Pixel 9 (phone, 192.168.0.42)`,
/// or `(phone, 100.64.0.7 through the proxy at 127.0.0.1)`.
fn request_line(r: &Value) -> String {
    let s = |k: &str| r[k].as_str().unwrap_or_default();
    let from = match r["via"].as_str() {
        Some(proxy) => format!("{} through the proxy at {proxy}", s("from")),
        None => s("from").to_string(),
    };
    format!("{}  {} ({}, {from})", s("match_code"), s("name"), s("kind").replace('_', " "))
}

/// `upnext pair requests`: the devices waiting, with the number each shows.
pub async fn pair_requests(cfg: &Config, port: Option<u16>) -> Result<()> {
    let port = port.unwrap_or(cfg.port);
    let (http, base) = local(port)?;
    let list = waiting(&http, &base, port).await?;
    let mut text = String::new();
    if list.is_empty() {
        text.push_str("No device is waiting to be allowed.\n");
    } else {
        for r in &list {
            text.push_str(&request_line(r));
            text.push('\n');
        }
        text.push_str("Allow one with `upnext pair approve NUMBER`, or refuse it with `upnext pair deny NUMBER`.\n");
    }
    // Written rather than printed: `println!` panics when the reader has
    // gone (`upnext pair requests | head -1`), and a reader that stopped
    // early is no failure.
    use std::io::Write;
    match std::io::stdout().lock().write_all(text.as_bytes()) {
        Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => Err(e.into()),
        _ => Ok(()),
    }
}

/// `upnext pair approve 4821` and `deny`: answer the device showing that
/// number. A device allowed here is everyone's, as a paired one is; with
/// two or more players, choose whose it is in the dialog instead.
pub async fn pair_answer(cfg: &Config, port: Option<u16>, number: &str, allow: bool) -> Result<()> {
    let port = port.unwrap_or(cfg.port);
    let (http, base) = local(port)?;
    let number: String = number.chars().filter(|c| !c.is_whitespace()).collect();
    let list = waiting(&http, &base, port).await?;
    let Some(r) = list.iter().find(|r| r["match_code"] == number.as_str()) else {
        bail!("no device showing {number} is waiting: `upnext pair requests` lists the ones that are");
    };
    let id = r["id"].as_str().unwrap_or_default();
    let res = http
        .post(format!("{base}/api/v1/pairing/requests/{id}"))
        .json(&serde_json::json!({ "allow": allow }))
        .send()
        .await
        .map_err(|e| unreachable(port, e))?;
    if !res.status().is_success() {
        let status = res.status();
        let v: Value = res.json().await.unwrap_or_default();
        bail!("the server on port {port} didn't take the answer: {status} {}", v["error"].as_str().unwrap_or_default());
    }
    let name = r["name"].as_str().unwrap_or("the device");
    if allow {
        println!("Allowed {name}: it's in UpNext now, and listed under Settings › Devices and API, where it can be revoked.");
    } else {
        println!("Refused {name}.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_line_names_the_proxy_it_came_through() {
        let r = serde_json::json!({ "match_code": "4821", "name": "Pixel 9", "kind": "phone", "from": "192.168.0.42", "via": null });
        assert_eq!(request_line(&r), "4821  Pixel 9 (phone, 192.168.0.42)");
        let r = serde_json::json!({ "match_code": "4821", "name": "Pixel 9", "kind": "phone", "from": "100.64.0.7", "via": "127.0.0.1" });
        assert_eq!(request_line(&r), "4821  Pixel 9 (phone, 100.64.0.7 through the proxy at 127.0.0.1)");
    }

    /// The Linux packages' smoke test takes the line `healthcheck` prints,
    /// with the commit or without: every release build names its commit, and
    /// the test once wanted the version alone, which would have stopped the
    /// first release.
    #[cfg(unix)]
    #[test]
    fn the_package_smoke_test_takes_the_healthcheck_line() {
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/contrib/linux/smoke-test.sh");
        let takes =
            |line: &str| std::process::Command::new("bash").args([script, "--health-line", "0.1.0", line]).status().unwrap().success();
        for build in [serde_json::json!("abc1234"), Value::Null] {
            let line = answer_line(&serde_json::json!({ "app": "upnext", "version": "0.1.0", "build": build }));
            assert!(takes(&line), "{line}");
        }
        assert!(takes("ok: UpNext 0.1.0 (abc12345)"), "git gives more digits when seven are ambiguous");
        for other in [
            "ok: UpNext 0.1.01",
            "ok: UpNext 0.2.0 (abc1234)",
            "ok: UpNext 0.1.0 ()",
            "ok: UpNext 0.1.0 (a) (b)",
            "ok: UpNext 0.1.0 (abc123)",
            "ok: UpNext 0.1.0 (ABC1234)",
            "ok: UpNext 0.1.0 (abc1234-dirty)",
            "ok: UpNext 0.1.0 (abc1234) and more",
            "ok: UpNext 0x1x0",
            "",
        ] {
            assert!(!takes(other), "{other}");
        }
    }

    #[test]
    fn the_other_addresses_after_the_one_offered() {
        let ips: Vec<std::net::IpAddr> = ["192.168.0.42", "10.0.0.5", "fd00::42"].iter().map(|ip| ip.parse().unwrap()).collect();
        assert_eq!(
            other_addresses("http://192.168.0.42:8787", &ips).as_deref(),
            Some("If that address doesn't open, this computer is also at http://10.0.0.5:8787, http://[fd00::42]:8787.")
        );
        // Another address than this computer's first (`public_url`), or no
        // others: nothing more to say.
        assert_eq!(other_addresses("https://upnext.lan:8787", &ips), None);
        assert_eq!(other_addresses("http://192.168.0.42:8787", &ips[..1]), None);
        assert_eq!(other_addresses("http://192.168.0.42:8787", &[]), None);
    }

    /// Against a real server on a port of its own: `healthcheck` passes, and
    /// `pair` makes a code (the server then has one waiting); with the
    /// server gone, both fail.
    #[tokio::test]
    async fn healthcheck_and_pair_ask_the_running_server() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = crate::testutil::offline_config(dir.path());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        cfg.port = listener.local_addr().unwrap().port();
        let state = super::super::serve::state(cfg.clone()).unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(crate::server::serve::serve_until(listener, state, async move {
            let _ = stopped.await;
        }));

        healthcheck(&cfg, None).await.unwrap();
        pair(&cfg, Some(cfg.port)).await.unwrap();
        let (http, base) = local(cfg.port).unwrap();
        let waiting: Value = http.get(format!("{base}/api/v1/pairing")).send().await.unwrap().json().await.unwrap();
        assert_eq!(waiting["code"].as_str().map(str::len), Some(6), "{waiting}");

        // A device asks, and `pair approve` allows it by its number.
        let asked: Value = http
            .post(format!("{base}/api/v1/pair/request"))
            .json(&serde_json::json!({ "name": "Pixel 9", "kind": "phone", "wants": "token" }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let number = asked["match_code"].as_str().unwrap();
        pair_requests(&cfg, Some(cfg.port)).await.unwrap();
        assert!(pair_answer(&cfg, Some(cfg.port), "0000x", true).await.is_err(), "no device shows that");
        pair_answer(&cfg, Some(cfg.port), number, true).await.unwrap();
        let collected: Value = http
            .get(format!("{base}/api/v1/pair/request/{}", asked["id"].as_str().unwrap()))
            .header("UpNext-Pair-Secret", asked["secret"].as_str().unwrap())
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(collected["status"], "allowed", "{collected}");
        assert!(collected["token"].as_str().is_some_and(|t| t.starts_with("upnext_")), "{collected}");

        stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), server).await.expect("the server stopped").unwrap().unwrap();
        let e = healthcheck(&cfg, None).await.unwrap_err().to_string();
        assert!(e.contains("no UpNext answers"), "{e}");
        assert!(pair(&cfg, None).await.is_err());
    }
}
