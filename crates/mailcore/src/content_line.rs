//! Content lines shared by iCalendar (RFC 5545) and vCard (RFC 6350):
//! unfolding, the name/parameter/value split and text and parameter
//! unescaping.

/// Unfold RFC 5545 lines: CRLF or LF followed by a space or tab is deleted.
pub(crate) fn unfold(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        let newline = match c {
            '\r' if chars.peek() == Some(&'\n') => {
                chars.next();
                true
            }
            '\n' => true,
            _ => false,
        };
        if !newline {
            out.push(c);
            continue;
        }
        if matches!(chars.peek(), Some(' ') | Some('\t')) {
            chars.next();
            continue;
        }
        out.push('\n');
    }
    out
}

/// Unescape RFC 5545 text value: `\,` -> `,`, `\;` -> `;`, `\n`/`\N` -> newline, `\\` -> `\`.
pub(crate) fn unescape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Decode a parameter value: strip surrounding quotes and apply RFC 6868
/// caret escapes (`^n`, `^'`, `^^`).
fn param_value(raw: &str) -> String {
    let raw = raw.trim();
    let raw = raw
        .strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .unwrap_or(raw);
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '^' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') | Some('N') => out.push('\n'),
            Some('\'') => out.push('"'),
            Some('^') => out.push('^'),
            Some(other) => {
                out.push('^');
                out.push(other);
            }
            None => out.push('^'),
        }
    }
    out
}

/// One content line: upper-cased name, parameters (upper-cased names,
/// decoded values) and the raw value. A parameter without `=` (vCard 2.1's
/// bare `TEL;CELL:` or `QUOTED-PRINTABLE`) is kept with an empty value.
pub(crate) struct ContentLine<'a> {
    pub(crate) name: String,
    pub(crate) params: Vec<(String, String)>,
    pub(crate) value: &'a str,
}

impl ContentLine<'_> {
    pub(crate) fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Split a content line at the first `:` outside a double-quoted parameter
/// value, and its name/parameter part at each such `;`.
pub(crate) fn parse_content_line(line: &str) -> Option<ContentLine<'_>> {
    let mut in_quotes = false;
    let mut segments: Vec<&str> = Vec::new();
    let mut seg_start = 0;
    let mut colon = None;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            ';' if !in_quotes => {
                segments.push(&line[seg_start..i]);
                seg_start = i + 1;
            }
            ':' if !in_quotes => {
                colon = Some(i);
                break;
            }
            _ => {}
        }
    }
    let colon = colon?;
    segments.push(&line[seg_start..colon]);
    let mut segments = segments.into_iter();
    let name = segments.next()?.trim().to_ascii_uppercase();
    if name.is_empty() {
        return None;
    }
    let params = segments
        .filter(|p| !p.trim().is_empty())
        .map(|p| match p.split_once('=') {
            Some((n, v)) => (n.trim().to_ascii_uppercase(), param_value(v)),
            None => (p.trim().to_ascii_uppercase(), String::new()),
        })
        .collect();
    Some(ContentLine {
        name,
        params,
        value: &line[colon + 1..],
    })
}
