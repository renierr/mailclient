//! Sender badge: the letters and colour of the round avatar both frontends
//! draw for a sender (message rows, search hits, the reader, accounts).
//! Built once here so a sender looks the same in Qt and Flutter; the feed
//! adds the fields to every row that shows one.

use serde::Serialize;

/// Second-level labels skipped when picking the domain's name-bearing
/// label (`shop.example.co.uk` -> `example`).
const SECOND_LEVEL: [&str; 7] = ["co", "com", "net", "org", "ac", "gov", "edu"];

/// Lightness tones and saturations per theme. Three tones times two
/// saturations on top of the hue keep senders that share a hue apart; all
/// stay dark enough for white text.
const LIGHT_LIGHTNESS: [f64; 3] = [0.40, 0.48, 0.56];
const DARK_LIGHTNESS: [f64; 3] = [0.36, 0.44, 0.52];
const LIGHT_SATURATION: [f64; 2] = [0.50, 0.70];
const DARK_SATURATION: [f64; 2] = [0.45, 0.65];

/// What a frontend needs to draw a sender's avatar. The text on it is white
/// on both themes; the frontend picks the colour matching its theme.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SenderBadge {
    /// First letter of the name (or of the address when there is none)
    /// plus the first letter of the address's domain, upper case; `?` when
    /// neither has a letter or digit.
    pub initials: String,
    /// Background colour as `#rrggbb` for a light theme.
    pub avatar_light: String,
    /// Background colour as `#rrggbb` for a dark theme.
    pub avatar_dark: String,
}

impl SenderBadge {
    /// Adds the badge fields to a JSON object row (no-op for non-objects).
    pub fn extend(self, row: &mut serde_json::Value) {
        if let Some(map) = row.as_object_mut() {
            map.insert("initials".into(), self.initials.into());
            map.insert("avatar_light".into(), self.avatar_light.into());
            map.insert("avatar_dark".into(), self.avatar_dark.into());
        }
    }
}

/// Badge for a sender. The colour follows the address alone (case and
/// surrounding blanks ignored), so it never shifts when the display name
/// does; without an address it follows the name.
pub fn sender_badge(name: &str, address: &str) -> SenderBadge {
    let seed = if address.trim().is_empty() {
        name
    } else {
        address
    };
    let (hue, tone, vivid) = color_spec(seed);
    SenderBadge {
        initials: initials(name, address),
        avatar_light: hsl_hex(hue, LIGHT_SATURATION[vivid], LIGHT_LIGHTNESS[tone]),
        avatar_dark: hsl_hex(hue, DARK_SATURATION[vivid], DARK_LIGHTNESS[tone]),
    }
}

fn initials(name: &str, address: &str) -> String {
    let first = first_alnum(name).or_else(|| first_alnum(address));
    let mut out = first.map_or_else(|| "?".to_string(), String::from);
    if let Some((_, domain)) = address.rsplit_once('@') {
        if let Some(c) = first_alnum(domain_label(domain)) {
            out.push(c);
        }
    }
    out
}

fn first_alnum(s: &str) -> Option<char> {
    s.chars()
        .find(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
}

/// The name-bearing label of a domain: `mail.example.co.uk` -> `example`.
fn domain_label(domain: &str) -> &str {
    let mut labels: Vec<&str> = domain
        .trim_end_matches(|c: char| c == '>' || c.is_whitespace())
        .split('.')
        .filter(|l| !l.is_empty())
        .collect();
    if labels.len() > 1 {
        labels.pop();
    }
    if labels.len() > 1
        && SECOND_LEVEL
            .iter()
            .any(|s| labels.last().is_some_and(|l| l.eq_ignore_ascii_case(s)))
    {
        labels.pop();
    }
    labels.last().copied().unwrap_or("")
}

/// Hue 0..359, tone 0..2 and saturation index 0..1 from a well-mixed hash,
/// so near-identical addresses land far apart.
fn color_spec(seed: &str) -> (u16, usize, usize) {
    let h = seed_hash(seed);
    (
        (h % 360) as u16,
        (h / 360 % 3) as usize,
        (h / 1080 % 2) as usize,
    )
}

/// FNV-1a over the lower-cased seed plus the murmur3 finaliser.
fn seed_hash(seed: &str) -> u32 {
    let s = seed.trim().to_lowercase();
    let s = if s.is_empty() { "?" } else { s.as_str() };
    let mut h: u32 = 0x811c_9dc5;
    for b in s.bytes() {
        h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
    }
    h ^= h >> 16;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^ (h >> 16)
}

fn hsl_hex(hue: u16, s: f64, l: f64) -> String {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = f64::from(hue) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let byte = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_are_name_plus_domain() {
        assert_eq!(initials("Alice", "alice@example.com"), "AE");
        assert_eq!(initials("Alice", "news@mail.example.org"), "AE");
        assert_eq!(initials("Alice", "a@shop.example.co.uk"), "AE");
        assert_eq!(initials("", "bob@example.net"), "BE");
        assert_eq!(initials("\"Carol\"", "c@example.com"), "CE");
    }

    #[test]
    fn initials_without_domain_are_one_letter() {
        assert_eq!(initials("Alice", ""), "A");
        assert_eq!(initials("Alice", "alice"), "A");
        assert_eq!(initials("", ""), "?");
    }

    #[test]
    fn colour_follows_the_address_not_the_name() {
        let a = sender_badge("Alice", "alice@example.com");
        let b = sender_badge("Liddell", " Alice@Example.COM ");
        assert_eq!(a.avatar_light, b.avatar_light);
        assert_eq!(a.avatar_dark, b.avatar_dark);
        assert_ne!(a.initials, b.initials);
    }

    #[test]
    fn similar_addresses_spread_apart() {
        // A one-letter difference must not give neighbouring hues.
        let (ha, _, _) = color_spec("a@example.com");
        let (hb, _, _) = color_spec("b@example.com");
        let gap = ha.abs_diff(hb).min(360 - ha.abs_diff(hb));
        assert!(gap >= 30, "hues {ha} and {hb} too close");
    }

    #[test]
    fn without_address_the_name_is_the_seed() {
        assert_eq!(
            sender_badge("Alice", "").avatar_dark,
            sender_badge("x", "alice").avatar_dark
        );
        assert_eq!(sender_badge("", "").initials, "?");
    }

    #[test]
    fn hsl_converts_to_hex() {
        assert_eq!(hsl_hex(0, 1.0, 0.5), "#ff0000");
        assert_eq!(hsl_hex(120, 1.0, 0.5), "#00ff00");
        assert_eq!(hsl_hex(240, 1.0, 0.25), "#000080");
        assert_eq!(hsl_hex(0, 0.0, 1.0), "#ffffff");
    }

    #[test]
    fn colours_are_hex_and_never_too_light_for_white_text() {
        for seed in ["a@example.com", "b@example.org", "news@example.net", ""] {
            let b = sender_badge("", seed);
            for c in [&b.avatar_light, &b.avatar_dark] {
                assert_eq!(c.len(), 7);
                let rgb = u32::from_str_radix(&c[1..], 16).unwrap();
                let max = [rgb >> 16, (rgb >> 8) & 0xff, rgb & 0xff]
                    .into_iter()
                    .max()
                    .unwrap();
                assert!(max < 0xf0, "{c} too light");
            }
        }
    }
}
