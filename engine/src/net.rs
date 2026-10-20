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
        // Site-local (deprecated, but still private where it's used).
        || (first & 0xffc0) == 0xfec0
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

/// A web address that may be fetched: http or https, not this machine or
/// the local network by name, and, for an address written as an IP (which
/// is connected to without resolving), a public one. Checked for the first
/// address and every redirect.
pub fn public_url(url: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(url) else { return false };
    if !matches!(u.scheme(), "http" | "https") {
        return false;
    }
    match u.host() {
        Some(url::Host::Domain(name)) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            !(name == "localhost" || name.ends_with(".localhost") || name.ends_with(".local") || name.ends_with(".internal") || name.ends_with(".lan") || name.ends_with(".home.arpa"))
        }
        Some(url::Host::Ipv4(ip)) => public_ip(ip.into()),
        Some(url::Host::Ipv6(ip)) => public_ip(ip.into()),
        None => false,
    }
}

/// Redirects only to other addresses `public_url` allows, at most ten.
pub fn public_redirects() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 10 || !public_url(attempt.url().as_str()) {
            attempt.stop()
        } else {
            attempt.follow()
        }
    })
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

/// A client builder that resolves through `PublicOnly` and redirects only
/// to public addresses, and never through a proxy (which would resolve the
/// name itself, past these checks). The first address is the caller's to
/// check with `public_url`: an IP written in it is connected to without
/// resolving.
pub fn public_client() -> reqwest::blocking::ClientBuilder {
    reqwest::blocking::Client::builder().no_proxy().dns_resolver(Arc::new(PublicOnly)).redirect(public_redirects())
}

/// Where a link really goes, checked when it's about to be opened: its
/// address and every redirect public, every name resolving only to public
/// addresses. The final address, for the browser.
pub fn checked_link(url: &str) -> anyhow::Result<String> {
    if !public_url(url) {
        anyhow::bail!("that link isn't on the public web");
    }
    let client = public_client().timeout(std::time::Duration::from_secs(15)).build()?;
    let response = client.get(url).send().map_err(|_| anyhow::anyhow!("that link doesn't lead to the public web"))?;
    // A redirect still standing is one the policy refused (or too many).
    if response.status().is_redirection() {
        anyhow::bail!("that link doesn't lead to the public web");
    }
    let last = response.url().to_string();
    if !public_url(&last) {
        anyhow::bail!("that link doesn't lead to the public web");
    }
    Ok(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_addresses() {
        for private in ["127.0.0.1", "10.1.2.3", "172.16.0.1", "192.168.1.1", "169.254.169.254", "100.64.0.1", "0.0.0.0", "::1", "fe80::1", "fd00::1", "::ffff:127.0.0.1", "::ffff:192.168.0.1", "fec0::1"] {
            assert!(!public_ip(private.parse().unwrap()), "{private} should be refused");
        }
        for public in ["93.184.216.34", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(public_ip(public.parse().unwrap()), "{public} should be allowed");
        }
    }

    #[test]
    fn only_public_web_addresses() {
        assert!(public_url("https://example.com/a.jpg"));
        assert!(public_url("http://93.184.216.34/x"));
        for local in ["http://127.0.0.1:8080/", "http://[::1]/", "http://169.254.169.254/latest", "http://10.0.0.1/", "http://localhost/", "http://router.lan/", "http://printer.local/", "file:///etc/passwd", "ftp://example.com/"] {
            assert!(!public_url(local), "{local} should be refused");
        }
    }

    #[test]
    fn names_that_point_home_connect_nowhere() {
        let client = public_client().build().unwrap();
        let err = client.get("http://localhost:9/").send().unwrap_err();
        assert!(format!("{err:?}").contains("public internet"), "{err:?}");
    }
}
