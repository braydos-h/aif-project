//! Remote image fetching over HTTP(S) with a size cap and SSRF guard.
//!
//! `image_url` is client-supplied per request, so it must never reach
//! private network targets: loopback, RFC 1918, link-local (including the
//! `169.254.169.254` cloud-metadata address), or `localhost`-style names.
//! The deterministic `none` backend never fetches (it only hashes the URL
//! string); this guard protects the Ollama path, which downloads the bytes.

use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};

use super::image::{ensure_within_limit, MAX_IMAGE_BYTES};

/// Max `image_url` length in bytes (mirrors the runtime option limit).
pub(crate) const MAX_IMAGE_URL_BYTES: usize = 2 * 1024;

/// Check that an image URL is safe to fetch. Returns the lowercased host on
/// success; returns a human-readable reason when the URL must be refused.
pub(crate) fn check_image_url_allowed(url: &str) -> Result<String, String> {
    if url.is_empty() || url.len() > MAX_IMAGE_URL_BYTES {
        return Err("Image URL must be a non-empty http(s) URL under 2 KiB.".to_string());
    }
    if url
        .bytes()
        .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
    {
        return Err("Image URL must not contain whitespace or control characters.".to_string());
    }
    // Block credential smuggling (`user:pass@host`) and query/fragment
    // exfiltration vectors; image endpoints take a plain path.
    if url.contains('@') || url.contains('?') || url.contains('#') {
        return Err(
            "Image URL must be a plain http(s) URL without credentials or query parameters."
                .to_string(),
        );
    }
    let (scheme, rest) = url
        .strip_prefix("https://")
        .map(|r| ("https", r))
        .or_else(|| url.strip_prefix("http://").map(|r| ("http", r)))
        .ok_or_else(|| "Image URL must start with http:// or https://.".to_string())?;
    let authority = rest.split('/').next().unwrap_or("");
    // Strip optional port; keep the host for the SSRF check.
    let host_port = authority;
    let host_raw = host_port.split(':').next().unwrap_or("");
    let host = host_raw
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host_raw);
    if host.is_empty() {
        return Err("Image URL must contain a hostname.".to_string());
    }
    // Validate an explicit port when present so `host:badport` fails fast.
    if let Some(port_str) = host_port.split(':').nth(1) {
        let port_str = port_str.split('/').next().unwrap_or(port_str);
        if port_str.parse::<u16>().is_err() {
            return Err("Image URL contains an invalid port.".to_string());
        }
    }
    let lower = host.to_ascii_lowercase();
    // Literal IPs are checked directly; hostnames go through the
    // blocklist + best-effort DNS resolution below.
    if let Ok(ip) = lower.parse::<IpAddr>() {
        if !ip_is_public_routable(&ip) {
            return Err("Image URL host is blocked (private or local address).".to_string());
        }
        return Ok(lower);
    }
    if hostname_is_blocked(&lower) {
        return Err("Image URL host is blocked (private or local address).".to_string());
    }
    // Best-effort DNS: if the name resolves to a non-public address, refuse.
    // Resolution failures are allowed through — the fetch itself will then
    // fail with a clear network error instead of a misleading block message.
    let default_port = if scheme == "https" { 443 } else { 80 };
    let port: u16 = host_port
        .split(':')
        .nth(1)
        .and_then(|p| p.split('/').next().unwrap_or(p).parse().ok())
        .unwrap_or(default_port);
    if let Ok(addrs) = (lower.as_str(), port).to_socket_addrs() {
        for addr in addrs {
            if !ip_is_public_routable(&addr.ip()) {
                return Err("Image URL host is blocked (private or local address).".to_string());
            }
        }
    }
    Ok(lower)
}

/// True for globally routable addresses; false for loopback, private,
/// link-local, multicast, unspecified, broadcast, documentation, and
/// reserved ranges.
fn ip_is_public_routable(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => ipv4_is_public(v4),
        IpAddr::V6(v6) => {
            if v6.is_loopback() || v6.is_unspecified() || v6.is_multicast() {
                return false;
            }
            // IPv4-mapped (`::ffff:10.0.0.1`) inherits the v4 verdict.
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return ipv4_is_public(&mapped);
            }
            let seg = v6.segments();
            // Unique-local fc00::/7, link-local fe80::/10.
            if seg[0] & 0xfe00 == 0xfc00 || seg[0] & 0xffc0 == 0xfe80 {
                return false;
            }
            // Documentation 2001:db8::/32.
            if seg[0] == 0x2001 && seg[1] == 0x0db8 {
                return false;
            }
            // Discard prefix 100::/64 (RFC 6666).
            if seg[0] == 0x0100 {
                return false;
            }
            true
        }
    }
}

fn ipv4_is_public(v4: &Ipv4Addr) -> bool {
    if v4.is_loopback()
        || v4.is_unspecified()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_multicast()
    {
        return false;
    }
    let o = v4.octets();
    // Broadcast, current-network, and reserved 240.0.0.0/4.
    if *v4 == Ipv4Addr::BROADCAST || o[0] == 0 || o[0] >= 240 {
        return false;
    }
    // Documentation ranges (RFC 5737) + benchmarking (RFC 2544).
    if (o[0] == 192 && o[1] == 0 && o[2] == 2)
        || (o[0] == 198 && o[1] == 51 && o[2] == 100)
        || (o[0] == 203 && o[1] == 0 && o[2] == 113)
        || (o[0] == 198 && (o[1] == 18 || o[1] == 19))
    {
        return false;
    }
    true
}

/// Hostnames that never need DNS to be refused.
fn hostname_is_blocked(lower: &str) -> bool {
    if lower == "localhost"
        || lower.ends_with(".localhost")
        || lower.ends_with(".local")
        || lower.ends_with(".internal")
        || lower.ends_with(".lan")
        || lower.ends_with(".home")
        || lower.ends_with(".corp")
        || lower.ends_with(".intranet")
    {
        return true;
    }
    matches!(
        lower,
        "metadata.google.internal"
            | "metadata.google"
            | "instance-data"
            | "instance-data-compute"
            | "169.254.169.254"
    )
}

/// Fetch bytes from an http(s) URL with a 30s timeout and a UA header.
///
/// Redirects are disabled (`redirects(0)`): a 3xx is refused instead of
/// followed, so a public URL cannot bounce the fetcher onto an internal
/// target. Only 2xx responses are accepted.
pub(crate) fn fetch_url(url: &str) -> Result<Vec<u8>, String> {
    check_image_url_allowed(url)?;
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(30))
        .redirects(0)
        .build();
    let response = agent
        .get(url)
        .set("User-Agent", "aif-project/1.0")
        .call()
        .map_err(|e| format!("Unable to fetch image from URL {}: {}", url, e))?;
    let status = response.status();
    if !(200..300).contains(&status) {
        return Err(format!(
            "Image URL returned HTTP {} (redirects are not followed)",
            status
        ));
    }
    let mut buf: Vec<u8> = Vec::new();
    response
        .into_reader()
        .take((MAX_IMAGE_BYTES + 1) as u64)
        .read_to_end(&mut buf)
        .map_err(|e| format!("Failed to read image from {}: {}", url, e))?;
    ensure_within_limit(buf.len()).map_err(|e| e.0)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_urls_pass_the_guard() {
        assert!(check_image_url_allowed("https://example.com/cow.jpg").is_ok());
        assert!(check_image_url_allowed("http://example.com:8080/a/b.png").is_ok());
        assert!(check_image_url_allowed("https://8.8.8.8/cow.jpg").is_ok());
    }

    #[test]
    fn private_ipv4_hosts_are_blocked() {
        for url in [
            "http://127.0.0.1/cow.jpg",
            "http://127.0.0.1:8080/cow.jpg",
            "http://10.0.0.5/cow.jpg",
            "http://172.16.4.9/cow.jpg",
            "http://172.31.255.1/cow.jpg",
            "http://192.168.1.20/cow.jpg",
            "http://169.254.169.254/latest/meta-data/",
            "http://0.0.0.0/cow.jpg",
            "http://255.255.255.255/cow.jpg",
        ] {
            assert!(
                check_image_url_allowed(url).is_err(),
                "expected block for {}",
                url
            );
        }
    }

    #[test]
    fn loopback_ipv6_is_blocked() {
        assert!(check_image_url_allowed("http://[::1]/cow.jpg").is_err());
        assert!(check_image_url_allowed("http://[fe80::1]/cow.jpg").is_err());
        assert!(check_image_url_allowed("http://[fc00::1]/cow.jpg").is_err());
    }

    #[test]
    fn local_hostnames_are_blocked_without_dns() {
        for url in [
            "http://localhost/cow.jpg",
            "http://LOCALHOST:8080/cow.jpg",
            "http://printer.local/cow.jpg",
            "http://db.internal/cow.jpg",
            "http://nas.lan/cow.jpg",
            "http://metadata.google.internal/cow.jpg",
        ] {
            assert!(
                check_image_url_allowed(url).is_err(),
                "expected block for {}",
                url
            );
        }
    }

    #[test]
    fn malformed_or_sneaky_urls_are_rejected() {
        for url in [
            "",
            "file:///etc/passwd",
            "ftp://example.com/cow.jpg",
            "https://example.com/cow.jpg?key=secret",
            "https://example.com/cow.jpg#frag",
            "https://user:pass@example.com/cow.jpg",
            "https://exam ple.com/cow.jpg",
            "http:///cow.jpg",
            "http://example.com:badport/cow.jpg",
        ] {
            assert!(
                check_image_url_allowed(url).is_err(),
                "expected reject for {:?}",
                url
            );
        }
    }

    #[test]
    fn fetch_url_refuses_blocked_host_without_network() {
        let err = fetch_url("http://127.0.0.1:9/cow.jpg").unwrap_err();
        assert!(err.contains("blocked"), "expected SSRF block, got: {}", err);
    }
}
