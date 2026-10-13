//! This computer's addresses on its network, as another device would open
//! them: the address in a pairing code's QR code and in `upnext pair`.
//!
//! A computer can have several: the home network's, a VPN's, a container
//! bridge's. The one to offer is the interface the system's default route
//! leaves from, then the private ranges a home network uses
//! (192.168/16, then 172.16/12, then 10/8), then anything else, then a
//! tunnel's or an interface that's down, then link-local; loopback never. A
//! VPN or another point-to-point tunnel never counts as the default route
//! and its addresses come after every other but link-local, since a
//! full-tunnel VPN can steer traffic for the outside through itself while
//! the phone on the sofa is still on the home network. The
//! order is fixed (`ranked`), so the same address comes first every run.

use std::net::{IpAddr, Ipv4Addr};

/// One address of one interface, as `ranked` weighs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interface {
    pub name: String,
    pub ip: IpAddr,
    /// Operationally up: a bridge with nothing plugged in isn't.
    pub up: bool,
    /// Point-to-point, as VPNs and other tunnels are.
    pub tunnel: bool,
}

/// This computer's addresses another device could open, best first. Empty
/// when the system won't list its interfaces and no route leaves it.
pub fn addresses() -> Vec<IpAddr> {
    let Ok(list) = if_addrs::get_if_addrs() else {
        return probe().into_iter().collect();
    };
    let interfaces: Vec<Interface> =
        list.into_iter().map(|i| Interface { ip: i.ip(), up: i.is_oper_up(), tunnel: i.is_p2p(), name: i.name }).collect();
    ranked(&interfaces, default_route(&interfaces).as_deref())
}

/// The address to offer first.
pub fn lan_address() -> Option<IpAddr> {
    addresses().into_iter().next()
}

/// The addresses another device could open, best first, from the
/// interfaces and the name of the one the default route leaves from.
/// Ties go by interface name and then address, so the order doesn't
/// depend on the order the system lists them in.
pub fn ranked(interfaces: &[Interface], default_route: Option<&str>) -> Vec<IpAddr> {
    let mut usable: Vec<(u8, bool, &str, IpAddr)> = interfaces
        .iter()
        .filter_map(|i| {
            let class = class(i, default_route)?;
            Some((class, i.ip.is_ipv6(), i.name.as_str(), i.ip))
        })
        .collect();
    usable.sort();
    usable.dedup_by_key(|u| u.3);
    usable.into_iter().map(|u| u.3).collect()
}

/// Where an address goes, lower first; None for one another device can't
/// open. A link-local IPv6 address needs its interface named, which an
/// address in a URL can't carry everywhere.
fn class(i: &Interface, default_route: Option<&str>) -> Option<u8> {
    let ip = i.ip.to_canonical();
    if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
        return None;
    }
    let link_local = match ip {
        IpAddr::V4(v4) => v4.is_link_local(),
        IpAddr::V6(v6) if (v6.segments()[0] & 0xffc0) == 0xfe80 => return None,
        IpAddr::V6(_) => false,
    };
    if link_local {
        return Some(6);
    }
    // After everything else but link-local: a VPN's private address is no
    // nearer than a public one, and a bridge that's down can't be reached.
    if i.tunnel || !i.up {
        return Some(5);
    }
    if default_route == Some(i.name.as_str()) {
        return Some(0);
    }
    Some(match ip {
        IpAddr::V4(v4) => private_range(v4).unwrap_or(4),
        IpAddr::V6(_) => 4,
    })
}

/// 192.168/16 first, then 172.16/12, then 10/8: home routers hand out the
/// first, and VPNs and container bridges tend to use the other two.
fn private_range(v4: Ipv4Addr) -> Option<u8> {
    match v4.octets() {
        [192, 168, ..] => Some(1),
        [172, b, ..] if (16..32).contains(&b) => Some(2),
        [10, ..] => Some(3),
        _ => None,
    }
}

/// The interface the default route leaves from. On Linux that's the main
/// routing table's, which a full-tunnel WireGuard leaves alone (it routes
/// through its own table); elsewhere, the interface holding the address a
/// connection to the outside would leave from. A tunnel never counts.
fn default_route(interfaces: &[Interface]) -> Option<String> {
    #[cfg(target_os = "linux")]
    if let Some(name) = std::fs::read_to_string("/proc/net/route").ok().and_then(|t| default_route_in(&t)) {
        return Some(name);
    }
    let ip = probe()?;
    interfaces.iter().find(|i| i.ip == ip && !i.tunnel).map(|i| i.name.clone())
}

/// The interface of the lowest-metric default route that's up in Linux's
/// `/proc/net/route`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn default_route_in(table: &str) -> Option<String> {
    const RTF_UP: u32 = 0x1;
    table
        .lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            let (name, destination, flags, metric, mask) = (f.first()?, f.get(1)?, f.get(3)?, f.get(6)?, f.get(7)?);
            let up = u32::from_str_radix(flags, 16).ok()? & RTF_UP != 0;
            if !up || *destination != "00000000" || *mask != "00000000" {
                return None;
            }
            Some((metric.parse::<u32>().ok()?, name.to_string()))
        })
        .min()
        .map(|(_, name)| name)
}

/// The address a connection to the outside would leave from. Nothing is
/// sent: connecting a UDP socket only picks the route.
fn probe() -> Option<IpAddr> {
    let probe = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    probe.connect("192.0.2.1:9").ok()?;
    let ip = probe.local_addr().ok()?.ip();
    (!ip.is_unspecified() && !ip.is_loopback()).then_some(ip)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iface(name: &str, ip: &str) -> Interface {
        Interface { name: name.into(), ip: ip.parse().unwrap(), up: true, tunnel: false }
    }

    fn tunnel(name: &str, ip: &str) -> Interface {
        Interface { tunnel: true, ..iface(name, ip) }
    }

    fn order(interfaces: &[Interface], default_route: Option<&str>) -> Vec<String> {
        ranked(interfaces, default_route).iter().map(IpAddr::to_string).collect()
    }

    #[test]
    fn the_home_network_comes_before_a_vpn() {
        let interfaces = [iface("lo", "127.0.0.1"), tunnel("wg0", "10.0.0.5"), iface("eth0", "192.168.0.42"), iface("lo", "::1")];
        assert_eq!(order(&interfaces, Some("eth0")), ["192.168.0.42", "10.0.0.5"]);
        // Without a default route, the private ranges decide, and a tunnel
        // still comes after them.
        assert_eq!(order(&interfaces, None), ["192.168.0.42", "10.0.0.5"]);
    }

    #[test]
    fn the_default_route_wins_over_the_ranges() {
        let interfaces = [iface("docker0", "172.17.0.1"), iface("enp5s0", "10.1.2.3"), iface("virbr0", "192.168.122.1")];
        assert_eq!(order(&interfaces, Some("enp5s0")), ["10.1.2.3", "192.168.122.1", "172.17.0.1"]);
    }

    #[test]
    fn private_ranges_then_others_then_link_local() {
        let interfaces = [
            iface("f", "169.254.1.1"),
            iface("e", "fd00::42"),
            iface("d", "100.101.102.103"),
            iface("c", "10.1.2.3"),
            iface("b", "172.20.0.1"),
            iface("a", "192.168.0.42"),
            iface("g", "fe80::1"),
            iface("h", "0.0.0.0"),
        ];
        assert_eq!(order(&interfaces, None), ["192.168.0.42", "172.20.0.1", "10.1.2.3", "100.101.102.103", "fd00::42", "169.254.1.1"]);
    }

    #[test]
    fn a_tunnel_or_a_down_interface_after_everything_but_link_local() {
        let down = Interface { up: false, ..iface("br0", "192.168.1.1") };
        let interfaces =
            [iface("eth1", "169.254.1.1"), tunnel("wg0", "10.0.0.5"), down, iface("eth0", "fd00::42"), iface("cgnat0", "100.101.102.103")];
        assert_eq!(order(&interfaces, None), ["100.101.102.103", "fd00::42", "192.168.1.1", "10.0.0.5", "169.254.1.1"]);
    }

    #[test]
    fn ipv4_first_on_the_default_route_and_a_down_interface_waits() {
        let down = Interface { up: false, ..iface("br0", "192.168.1.1") };
        let interfaces = [iface("eth0", "fd00::42"), iface("eth0", "10.1.2.3"), down];
        assert_eq!(order(&interfaces, Some("eth0")), ["10.1.2.3", "fd00::42", "192.168.1.1"]);
    }

    #[test]
    fn the_same_order_whatever_order_they_are_listed_in() {
        let mut interfaces = vec![iface("eth1", "192.168.1.5"), iface("eth0", "192.168.0.42"), iface("wlan0", "192.168.0.43")];
        let first = order(&interfaces, None);
        assert_eq!(first, ["192.168.0.42", "192.168.1.5", "192.168.0.43"]);
        interfaces.reverse();
        assert_eq!(order(&interfaces, None), first);
    }

    #[test]
    fn the_main_tables_default_route() {
        let table = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
            wlan0\t00000000\t0100A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0\n\
            eth0\t00000000\t0100A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0\n\
            wg0\t00000080\t00000000\t0001\t0\t0\t0\t00000080\t0\t0\t0\n\
            down0\t00000000\t00000000\t0000\t0\t0\t0\t00000000\t0\t0\t0\n\
            eth0\t0000A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0\n";
        assert_eq!(default_route_in(table).as_deref(), Some("eth0"));
        assert_eq!(default_route_in("Iface\tDestination\n"), None);
    }
}
