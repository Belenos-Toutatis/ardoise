//! The IPv4 addresses another PC of the network can type to reach this one
//! (`docs/design/lan-share.md`, "Addresses and QR code").
//!
//! Each address is found by "connecting" a UDP socket: the system looks up the route and picks
//! the source address of the interface it would use. No packet is sent. The same code runs on
//! Windows and Linux, without `unsafe` nor a dependency.

use std::net::{IpAddr, Ipv4Addr, UdpSocket};

/// Where the routes are looked up: a public address first (the interface of the default route),
/// then one address of each private range and the two most common home subnets (in case the
/// LAN has no gateway: then only a subnet the PC is on has a route).
const PROBES: [Ipv4Addr; 5] = [
    Ipv4Addr::new(8, 8, 8, 8),
    Ipv4Addr::new(10, 0, 0, 1),
    Ipv4Addr::new(172, 16, 0, 1),
    Ipv4Addr::new(192, 168, 0, 1),
    Ipv4Addr::new(192, 168, 1, 1),
];

/// This PC's IPv4 addresses, private LAN ones first; empty when there is no network.
pub fn local_ipv4() -> Vec<Ipv4Addr> {
    order(PROBES.iter().filter_map(|&target| route_source(target)))
}

/// The address this PC would send from to reach `target`.
fn route_source(target: Ipv4Addr) -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    // The discard port: nothing is sent anyway.
    socket.connect((target, 9)).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(address) => Some(address),
        IpAddr::V6(_) => None,
    }
}

/// Keeps the addresses another PC could reach, without duplicates, private ones (RFC 1918)
/// first, each group in the order found.
pub(crate) fn order(candidates: impl IntoIterator<Item = Ipv4Addr>) -> Vec<Ipv4Addr> {
    let mut kept: Vec<Ipv4Addr> = Vec::new();
    for address in candidates {
        let reachable = !(address.is_loopback()
            || address.is_unspecified()
            || address.is_link_local()
            || address.is_broadcast()
            || address.is_multicast());
        if reachable && !kept.contains(&address) {
            kept.push(address);
        }
    }
    let (mut private, others): (Vec<_>, Vec<_>) = kept.into_iter().partition(Ipv4Addr::is_private);
    private.extend(others);
    private
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> Ipv4Addr {
        text.parse().unwrap()
    }

    #[test]
    fn private_first_without_duplicates() {
        let found = order(
            [
                "100.64.1.2",
                "192.168.1.23",
                "127.0.0.1",
                "0.0.0.0",
                "169.254.10.3",
                "10.1.2.3",
                "192.168.1.23",
                "172.20.0.5",
                "255.255.255.255",
                "224.0.0.1",
                "203.0.113.9",
                "172.32.0.1",
            ]
            .map(ip),
        );
        assert_eq!(
            found,
            [
                "192.168.1.23",
                "10.1.2.3",
                "172.20.0.5",
                "100.64.1.2",
                "203.0.113.9",
                "172.32.0.1",
            ]
            .map(ip)
        );
        assert!(order([]).is_empty());
        assert!(order(["127.0.0.1", "169.254.0.1"].map(ip)).is_empty());
    }

    #[test]
    fn local_addresses_are_usable() {
        // Whatever the machine: never loopback, link-local or a duplicate.
        let found = local_ipv4();
        for address in &found {
            assert!(
                !address.is_loopback() && !address.is_link_local() && !address.is_unspecified()
            );
        }
        let mut unique = found.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), found.len());
        eprintln!("local IPv4 addresses: {found:?}");
    }
}
