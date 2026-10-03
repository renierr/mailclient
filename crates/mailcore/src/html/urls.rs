//! Which `href` / `src` values are allowed to survive sanitizing.

use super::entities::decode_entities;

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
    let host = low
        .split_once("://")
        .map(|(_, r)| {
            r.split(['/', '?', '#'])
                .next()
                .unwrap_or("")
                .split('@')
                .next_back()
                .unwrap_or("")
        })
        .unwrap_or("");
    let host = host.split(':').next().unwrap_or("");
    if host.is_empty()
        || host == "localhost"
        || host.starts_with("127.")
        || host == "[::1]"
        || host == "::1"
        || host.starts_with("10.")
        || host.starts_with("192.168.")
        || host.ends_with(".local")
        || host.contains("localhost")
    {
        return false;
    }
    if host.starts_with("172.") {
        let parts: Vec<&str> = host.split('.').collect();
        if parts.len() == 4 {
            if let Ok(n) = parts[1].parse::<u8>() {
                if (16..=31).contains(&n) {
                    return false;
                }
            }
        }
    }
    true
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
