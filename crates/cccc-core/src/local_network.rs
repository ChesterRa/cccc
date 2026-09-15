//! Local interface suggestions, not remote reachability checks or discovery traffic.
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, UdpSocket};

#[derive(Debug, PartialEq, Eq)]
pub struct LocalAddress {
    pub interface: String,
    pub ip: IpAddr,
}

pub fn detect_lan_ipv4() -> Option<Ipv4Addr> {
    // UDP connect asks the OS to select a route; no datagram is sent.
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(8, 8, 8, 8), 80)).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if usable(ip.into()) => Some(ip),
        _ => None,
    }
}

fn usable(ip: IpAddr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
        return false;
    }
    match ip {
        IpAddr::V4(ip) => !ip.is_link_local() && !ip.is_broadcast(),
        // A link-local IPv6 scope ID is local to each machine and cannot be shared.
        IpAddr::V6(ip) => !ip.is_unicast_link_local() && ip.to_ipv4_mapped().is_none(),
    }
}

fn ordered(mut addresses: Vec<LocalAddress>, preferred: Option<Ipv4Addr>) -> Vec<LocalAddress> {
    addresses.retain(|a| usable(a.ip));
    addresses.sort_by_key(|a| {
        (
            Some(a.ip) != preferred.map(IpAddr::V4),
            a.ip.is_ipv6(),
            a.ip,
            a.interface.clone(),
        )
    });
    addresses.dedup_by_key(|a| a.ip);
    addresses
}

pub fn addresses() -> std::io::Result<Vec<LocalAddress>> {
    let addresses = if_addrs::get_if_addrs()?
        .into_iter()
        .filter(|a| a.is_oper_up())
        .map(|a| LocalAddress {
            ip: a.ip(),
            interface: a.name,
        })
        .collect();
    Ok(ordered(addresses, detect_lan_ipv4()))
}

pub fn wildcard(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => Ipv4Addr::UNSPECIFIED.into(),
        IpAddr::V6(_) => Ipv6Addr::UNSPECIFIED.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_addresses_keep_vpn_and_ipv6_but_exclude_unshareable_addresses() {
        let ips = [
            "127.0.0.1",
            "::1",
            "0.0.0.0",
            "::",
            "ff02::1",
            "224.0.0.1",
            "169.254.2.1",
            "fe90::1",
            "::ffff:127.0.0.1",
            "255.255.255.255",
            "192.168.1.9",
            "100.64.0.9",
            "fd00::9",
            "192.168.1.9",
        ];
        let result = ordered(
            ips.iter()
                .map(|ip| LocalAddress {
                    interface: "ethernet".into(),
                    ip: ip.parse().expect("fixture IP"),
                })
                .collect(),
            Some("192.168.1.9".parse().expect("preferred IP")),
        );
        assert_eq!(
            result.iter().map(|a| a.ip.to_string()).collect::<Vec<_>>(),
            ["192.168.1.9", "100.64.0.9", "fd00::9"]
        );
        assert!(ordered(Vec::new(), None).is_empty());
        assert_eq!(wildcard(result[0].ip).to_string(), "0.0.0.0");
        assert_eq!(wildcard(result[2].ip).to_string(), "::");
    }
}
