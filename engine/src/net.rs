//! Fetching from addresses that feeds and directories hand us (cover art,
//! podcast feeds and episodes) only ever reaches the public internet.
//!
//! A hostname is checked where it's resolved, not just as written: these
//! clients resolve through `PublicOnly`, which drops loopback, private,
//! link-local, and other non-public addresses, so a name that points (or
//! later re-points) at this machine or the local network connects nowhere.
//! That holds for redirects too, since every connection resolves here.

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};
use std::sync::Arc;

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || a == 0
        // Shared address space (carrier NAT), benchmarking, and reserved.
        || (a == 100 && (b & 0xc0) == 64)
        || (a == 198 && (b & 0xfe) == 18)
        || a >= 240
        || (a == 192 && b == 0 && ip.octets()[2] == 0))
}

fn public_v6(ip: Ipv6Addr) -> bool {
    // An IPv4 address carried in IPv6 is judged as itself.
    if let Some(v4) = ip.to_ipv4_mapped() {
        return public_v4(v4);
    }
    let first = ip.segments()[0];
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        || (first & 0xfe00) == 0xfc00
        || (first & 0xffc0) == 0xfe80
        // NAT64 and documentation prefixes.
        || (first == 0x64 && ip.segments()[1] == 0xff9b)
        || (first == 0x2001 && ip.segments()[1] == 0x0db8))
}

/// An address on the public internet.
pub fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => public_v6(v6),
    }
}

/// The system's resolver, keeping only public addresses.
pub struct PublicOnly;

impl Resolve for PublicOnly {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let found = tokio::task::spawn_blocking(move || (host.as_str(), 0).to_socket_addrs().map(|a| a.collect::<Vec<SocketAddr>>())).await??;
            let public: Vec<SocketAddr> = found.into_iter().filter(|a| public_ip(a.ip())).collect();
            if public.is_empty() {
                return Err("that address isn't on the public internet".into());
            }
            Ok(Box::new(public.into_iter()) as Addrs)
        })
    }
}

/// A client builder that resolves through `PublicOnly`.
pub fn public_client() -> reqwest::blocking::ClientBuilder {
    reqwest::blocking::Client::builder().dns_resolver(Arc::new(PublicOnly))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_addresses() {
        for private in ["127.0.0.1", "10.1.2.3", "172.16.0.1", "192.168.1.1", "169.254.169.254", "100.64.0.1", "0.0.0.0", "::1", "fe80::1", "fd00::1", "::ffff:127.0.0.1", "::ffff:192.168.0.1"] {
            assert!(!public_ip(private.parse().unwrap()), "{private} should be refused");
        }
        for public in ["93.184.216.34", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(public_ip(public.parse().unwrap()), "{public} should be allowed");
        }
    }

    #[test]
    fn names_that_point_home_connect_nowhere() {
        let client = public_client().build().unwrap();
        let err = client.get("http://localhost:9/").send().unwrap_err();
        assert!(format!("{err:?}").contains("public internet"), "{err:?}");
    }
}
