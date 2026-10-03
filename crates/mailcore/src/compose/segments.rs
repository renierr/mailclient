//! The recipient field's current segment. Both composers complete only the
//! address being typed and replace just that slice, so a half-typed list is
//! never clobbered.
//!
//! A segment ends at `,` or `;` outside double quotes: `"Doe, John"
//! <john@example.com>, ja` completes `ja`, and picking a suggestion keeps the
//! quoted name intact. A naive split breaks on the comma inside the quotes.

/// The address currently being typed: the last `,`/`;`-separated segment
/// outside double quotes, trimmed. `""` when the field ends at a separator.
#[must_use]
pub fn recipient_segment(text: &str) -> &str {
    let start = last_separator(text).map_or(0, |i| i + 1);
    text[start..].trim()
}

/// The field after completing its current segment with `replacement`: the
/// text up to and including the last separator outside quotes, a space, then
/// the replacement. No separator yet means the replacement is the field.
#[must_use]
pub fn replace_recipient_segment(text: &str, replacement: &str) -> String {
    match last_separator(text) {
        Some(i) => format!("{} {replacement}", &text[..=i]),
        None => replacement.to_string(),
    }
}

/// Byte index of the last `,`/`;` outside double quotes, if any.
fn last_separator(text: &str) -> Option<usize> {
    let mut quoted = false;
    let mut last = None;
    for (i, c) in text.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ',' | ';' if !quoted => last = Some(i),
            _ => {}
        }
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_is_the_last_unquoted_piece() {
        assert_eq!(recipient_segment("ann@example.com, bo"), "bo");
        assert_eq!(recipient_segment("ann@example.com;"), "");
        assert_eq!(recipient_segment("  bo  "), "bo");
        assert_eq!(recipient_segment(""), "");
    }

    #[test]
    fn quoted_commas_do_not_split() {
        let field = "\"Doe, John\" <john@example.com>, ja";
        assert_eq!(recipient_segment(field), "ja");
        assert_eq!(
            replace_recipient_segment(field, "jane@example.com"),
            "\"Doe, John\" <john@example.com>, jane@example.com"
        );
    }

    #[test]
    fn unterminated_quote_holds_the_rest() {
        assert_eq!(recipient_segment("\"Doe, John, ja"), "\"Doe, John, ja");
    }

    #[test]
    fn replace_keeps_the_head_and_adds_a_space() {
        assert_eq!(
            replace_recipient_segment("a@example.com,b@example.com", "c@example.com"),
            "a@example.com, c@example.com"
        );
        assert_eq!(
            replace_recipient_segment("bo", "bo@example.com"),
            "bo@example.com"
        );
    }
}
