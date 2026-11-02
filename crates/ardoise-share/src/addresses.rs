//! The IPv4 addresses another PC of the network can type to reach this one
//! (`docs/design/lan-share.md`, "Addresses and QR code").

use std::net::Ipv4Addr;

/// This PC's IPv4 addresses, private LAN ones first; empty when there is no network.
pub fn local_ipv4() -> Vec<Ipv4Addr> {
    unimplemented!("package A")
}
