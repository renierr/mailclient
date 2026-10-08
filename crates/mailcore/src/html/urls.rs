//! Which `href` / `src` values are allowed to survive sanitizing.

use super::entities::decode_entities;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub(super) fn scheme_of(url: &str) -> &str {
    let t = url.trim();
    match t.find(':') {
        Some(i) => &t[..i],
        None => "",
    }
}

pub(super) fn is_http_url(url: &str) -> bool {
    let s = scheme_of(url).to_ascii_lowercase();
    s == "http" || s == "https"
}

/// Authority host of an absolute URL: user info, the `[..]` wrapper of an
/// IPv6 literal and a trailing port all removed.
///
/// Stripping the port on the *first* `:` used to reduce every bracketed
/// IPv6 host to `"["`, which then passed every literal check below — so
/// `[::1]` counted as a public host.
fn http_host(url: &str) -> Option<&str> {
    let (_, rest) = url.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    if let Some(bracketed) = host_port.strip_prefix('[') {
        // `[::1]` / `[::1]:8080` / a zoned `[fe80::1%25eth0]`.
        let inner = bracketed.split(']').next()?;
        return Some(inner.split('%').next().unwrap_or(inner));
    }
    if host_port.matches(':').count() > 1 {
        // A bare IPv6 literal carries no port; leave it whole so every
        // segment survives.
        return Some(host_port);
    }
    match host_port.rsplit_once(':') {
        Some((h, p)) if p.parse::<u16>().is_ok() && !h.is_empty() => Some(h),
        _ => Some(host_port),
    }
}

/// IPv4 ranges a mail must never make the reader fetch. Written out because
/// [`IpAddr::is_global`] is unstable and will not compile on stable.
fn is_public_ipv4(a: Ipv4Addr) -> bool {
    let o = a.octets();
    !(o[0] == 0 // 0.0.0.0/8   "this network" (covers `http://1/`)
        || o[0] == 10 // 10.0.0.0/8  private
        || o[0] == 127 // 127.0.0.0/8 loopback
        || (o[0] == 100 && (o[1] & 0xc0) == 0x40) // 100.64.0.0/10  CGNAT
        || o[0] == 169 && o[1] == 254 // 169.254.0.0/16 link-local + metadata
        || o[0] == 172 && (o[1] & 0xf0) == 0x10 // 172.16.0.0/12 private
        || o[0] == 192 && o[1] == 0 && o[2] == 0 // 192.0.0.0/24
        || o[0] == 192 && o[1] == 168 // 192.168.0.0/16 private
        || o[0] == 198 && (o[1] & 0xfe) == 0x12 // 198.18.0.0/15 benchmarking
        || o[0] >= 240) // 240.0.0.0/4 reserved, 255.255.255.255
}

fn is_public_ipv6(a: Ipv6Addr) -> bool {
    // `::ffff:10.0.0.1` and `::10.0.0.1` are v4 in disguise.
    if let Some(v4) = a.to_ipv4() {
        return is_public_ipv4(v4);
    }
    if let Some(v4) = a.to_ipv4_mapped() {
        return is_public_ipv4(v4);
    }
    let s = a.segments();
    !(a.is_unspecified()
        || a.is_loopback()
        || (s[0] & 0xfe00) == 0xfc00 // fc00::/7 unique local
        || (s[0] & 0xffc0) == 0xfe80 // fe80::/10 link local
        || a.is_multicast())
}

/// The `inet_aton` shorthand a browser resolves even though it is not a
/// dotted quad: `127.1`, `10.1`, `2130706433`, `169.254.169.254`'s numeric
/// twin. Digits and dots only, so a hostname (any letter in it) is never
/// mistaken for a number. Hex and octal forms are deliberately not parsed;
/// a host carrying them falls through to the hostname branch below.
fn loose_ipv4(host: &str) -> Option<Ipv4Addr> {
    if host.is_empty() || !host.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return None;
    }
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() > 4 || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    let last = parts.len() - 1;
    let mut out = 0u32;
    for (i, p) in parts.iter().enumerate() {
        let v: u32 = p.parse().ok()?;
        if i == last {
            // Only the final part may carry more than one octet's worth.
            out |= v;
        } else {
            if v > u8::MAX.into() {
                return None;
            }
            out |= v << (24 - 8 * i);
        }
    }
    Some(Ipv4Addr::from(out))
}

pub(super) fn is_public_remote(src: &str) -> bool {
    // Sender-controlled fetch gate (names only; connection pinning happens
    // in the WebEngine `autoLoadImages=false` default + no-redirect stance).
    // Refuse loopback/private/local/file outright — mirrors omamail policy.
    let t = urldecode_trim(src);
    let low = t.to_ascii_lowercase();
    if low.starts_with("cid:") || low.starts_with("data:image/") {
        return false;
    }
    if !(low.starts_with("http://") || low.starts_with("https://")) {
        return false;
    }
    let Some(host) = http_host(&low) else {
        return false;
    };
    // A fully-qualified name ends in the root dot; it resolves to the same
    // host without it, so the comparison has to ignore it.
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.is_empty()
        || host == "localhost"
        || host.ends_with(".local")
        || host.contains("localhost")
    {
        return false;
    }
    match host.parse::<IpAddr>() {
        Ok(ip) => is_public_ip(ip),
        Err(_) => match loose_ipv4(host) {
            Some(v4) => is_public_ipv4(v4),
            None => true,
        },
    }
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_ipv4(v4),
        IpAddr::V6(v6) => is_public_ipv6(v6),
    }
}

pub(super) fn urldecode_trim(s: &str) -> String {
    decode_entities(s).trim().to_string()
}

pub(super) fn safe_href(href: &str) -> Option<String> {
    let d = urldecode_trim(href);
    if d.is_empty() || d.starts_with('#') {
        return None;
    }
    let low = d.to_ascii_lowercase();
    if low.starts_with("http://") || low.starts_with("https://") || low.starts_with("mailto:") {
        // Reject embedded controls / quotes already handled by attr escaping;
        // reject `javascript:` smuggled after whitespace/entities (decoded above).
        Some(d)
    } else {
        None
    }
}

/// A link as the reader's examine dialog shows it, and whether it may
/// leave the app at all. The same rule as the sanitizer's [`safe_href`], so
/// the reader never offers to open what the sanitizer would have dropped.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LinkInfo {
    /// `http`, `https` or `mailto`: may be opened (user-gated). Everything
    /// else (`javascript:`, `data:`, `file:`, fragments, …) stays inert.
    pub safe: bool,
    /// Lowercase scheme, `""` when there is none.
    pub scheme: String,
    /// The host without user info and port; for `mailto:` the address's
    /// domain. `""` when there is none.
    pub host: String,
    /// Path, query and fragment after the host, `""` when there is none.
    pub path: String,
}

/// Split `url` for display and decide whether it is safe to open.
pub fn link_info(url: &str) -> LinkInfo {
    let s = url.trim();
    let safe = safe_href(s).is_some();
    let (scheme, rest) = match s.find("://") {
        Some(i) if i > 0 => (s[..i].to_ascii_lowercase(), &s[i + 3..]),
        _ => match s.split_once(':') {
            Some((sc, r)) if sc.eq_ignore_ascii_case("mailto") => ("mailto".to_string(), r),
            _ => (String::new(), s),
        },
    };
    let (authority, path) = if scheme == "mailto" {
        // `a@example.com?subject=x`: the address up to its query.
        let end = rest.find(['?', '#']).unwrap_or(rest.len());
        (&rest[..end], "")
    } else {
        match rest.find(['/', '?', '#']) {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, ""),
        }
    };
    let host = authority.rsplit('@').next().unwrap_or_default();
    let host = host.split(':').next().unwrap_or_default();
    LinkInfo {
        safe,
        scheme,
        host: host.to_string(),
        path: path.to_string(),
    }
}

pub(super) fn safe_img_src(src: &str, allow_remote: bool) -> Option<String> {
    let d = urldecode_trim(src);
    if d.is_empty() {
        return None;
    }
    let low = d.to_ascii_lowercase();
    if low.starts_with("cid:") {
        return Some(d);
    }
    if low.starts_with("data:image/png")
        || low.starts_with("data:image/jpeg")
        || low.starts_with("data:image/jpg")
        || low.starts_with("data:image/gif")
        || low.starts_with("data:image/webp")
    {
        if d.len() < 400_000 {
            return Some(d);
        }
        return None;
    }
    if is_http_url(&d) {
        if allow_remote && is_public_remote(&d) {
            return Some(d);
        }
        return None;
    }
    None
}
