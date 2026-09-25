//! IMAP modified UTF-7 (RFC 3501 §5.1.3) for mailbox names.
//!
//! The wire speaks ASCII: non-ASCII runs travel as `&<modified-base64>-`
//! over UTF-16BE, `&-` is a literal `&`. Without decoding, a German Gmail
//! account shows `[Google Mail]/Entw&APw-rfe` instead of `Entwürfe`.
//!
//! Std-only by design (see AGENT.md): the base64 core is a few lines, and a
//! codec crate would be a dependency decision for ~60 lines of logic.

/// Decode one IMAP modified-UTF-7 string to Unicode.
///
/// Never fails the caller: undecodable `&...-` segments are passed through
/// verbatim so a weird-but-listed mailbox still syncs under its raw name.
#[must_use]
pub fn decode_modified_utf7(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'&' {
            out.push(input[i..].chars().next().expect("char boundary"));
            i += input[i..].chars().next().expect("char boundary").len_utf8();
            continue;
        }
        let Some(rel_end) = input[i..].find('-') else {
            out.push_str(&input[i..]);
            break;
        };
        let end = i + rel_end;
        let inner = &input[i + 1..end];
        if inner.is_empty() {
            out.push('&');
        } else if let Some(decoded) = decode_shift(inner) {
            out.push_str(&decoded);
        } else {
            out.push_str(&input[i..=end]);
        }
        i = end + 1;
    }
    out
}

/// Encode one Unicode mailbox path to IMAP modified UTF-7 for the wire.
///
/// Printable ASCII (except `&`) passes through, so delimiters like `/`
/// survive untouched; `&` becomes `&-`.
#[must_use]
pub fn encode_modified_utf7(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut run: Vec<u16> = Vec::new();
    let flush = |out: &mut String, run: &mut Vec<u16>| {
        if run.is_empty() {
            return;
        }
        let mut bytes = Vec::with_capacity(run.len() * 2);
        for u in run.drain(..) {
            bytes.push((u >> 8) as u8);
            bytes.push((u & 0xff) as u8);
        }
        let mut b64 = base64_encode(&bytes);
        b64 = b64.replace('/', ",");
        let trimmed = b64.trim_end_matches('=');
        out.push('&');
        out.push_str(trimmed);
        out.push('-');
    };
    for ch in input.chars() {
        if ch == '&' {
            flush(&mut out, &mut run);
            out.push_str("&-");
        } else if ch.is_ascii() && (0x20..=0x7e).contains(&(ch as u32)) {
            flush(&mut out, &mut run);
            out.push(ch);
        } else {
            let mut buf = [0u16; 2];
            for u in ch.encode_utf16(&mut buf) {
                run.push(*u);
            }
        }
    }
    flush(&mut out, &mut run);
    out
}

/// Build the wire [`imap_types::mailbox::Mailbox`] for a stored (Unicode)
/// folder path, encoding it back to modified UTF-7.
pub fn mailbox_for_wire(
    path: &str,
) -> Result<imap_types::mailbox::Mailbox<'static>, crate::error::StoreError> {
    let wire = encode_modified_utf7(path);
    imap_types::mailbox::Mailbox::try_from(wire.clone())
        .map_err(|e| crate::error::StoreError::InvalidInput(format!("invalid mailbox {path}: {e}")))
}

fn decode_shift(inner: &str) -> Option<String> {
    if !inner
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b',' || b == b'/')
    {
        return None;
    }
    let standard = inner.replace(',', "/");
    let mut padded = standard;
    padded.extend(std::iter::repeat_n('=', (4 - padded.len() % 4) % 4));
    let bytes = base64_decode(&padded)?;
    if bytes.len() % 2 != 0 {
        return None;
    }
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| (u16::from(c[0]) << 8) | u16::from(c[1]))
        .collect();
    char::decode_utf16(units)
        .collect::<Result<String, _>>()
        .ok()
}

const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let mut n: u32 = 0;
        for (k, b) in chunk.iter().enumerate() {
            n |= (*b as u32) << (16 - 8 * k);
        }
        let pad = 3 - chunk.len();
        for k in 0..4 - pad {
            out.push(B64_ALPHABET[((n >> (18 - 6 * k)) & 63) as usize] as char);
        }
        for _ in 0..pad {
            out.push('=');
        }
    }
    out
}

fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let mut vals = Vec::with_capacity(input.len());
    for b in input.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return None,
        };
        vals.push(v as u32);
    }
    let mut out = Vec::with_capacity(vals.len() * 6 / 8);
    for chunk in vals.chunks(4) {
        let mut n: u32 = 0;
        for (k, v) in chunk.iter().enumerate() {
            n |= *v << (18 - 6 * k);
        }
        let bytes = match chunk.len() {
            4 => 3,
            3 => 2,
            2 => 1,
            _ => return None,
        };
        for k in 0..bytes {
            out.push(((n >> (16 - 8 * k)) & 0xff) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_german_gmail_drafts() {
        assert_eq!(
            decode_modified_utf7("[Google Mail]/Entw&APw-rfe"),
            "[Google Mail]/Entwürfe"
        );
    }

    #[test]
    fn literal_ampersand_round_trips() {
        assert_eq!(decode_modified_utf7("Fish &- Chips"), "Fish & Chips");
        assert_eq!(encode_modified_utf7("Fish & Chips"), "Fish &- Chips");
    }

    #[test]
    fn ascii_and_inbox_pass_through() {
        assert_eq!(decode_modified_utf7("INBOX"), "INBOX");
        assert_eq!(encode_modified_utf7("INBOX"), "INBOX");
        assert_eq!(
            encode_modified_utf7("[Google Mail]/Gesendet"),
            "[Google Mail]/Gesendet"
        );
    }

    #[test]
    fn round_trips_mixed_and_non_latin() {
        for s in [
            "[Google Mail]/Entwürfe",
            "Gelöschte Elemente",
            "Grüße/日本語",
            "A&B",
            "INBOX.Unter&APY-Ordner",
        ] {
            assert_eq!(decode_modified_utf7(&encode_modified_utf7(s)), s, "{s}");
        }
    }

    #[test]
    fn broken_shift_passes_through_verbatim() {
        assert_eq!(decode_modified_utf7("Box&"), "Box&");
        assert_eq!(decode_modified_utf7("A&!!!-B"), "A&!!!-B");
    }
}
