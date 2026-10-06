//! The Markdown-flavoured plain-text composer body, rendered to HTML on send.
//!
//! The touch composers (Flutter, native Android) edit plain text; their
//! toolbar wraps the selection in `**bold**` / `*italic*` / `> quote` /
//! `- bullet` marks and inserts inline images as `![name](inline:N)`
//! tokens. This renders exactly that subset, so formatting arrives instead of
//! literal asterisks, and decides whether a mail needs an HTML part at all:
//! unformatted text without a quoted original goes out plain, as before.
//! (Qt's composer is a WYSIWYG HTML editor and needs none of this.)
//!
//! `sanitize_for_send` cleans `body_html` again, so escaping here is defense
//! in depth, not the security boundary.

use std::collections::BTreeMap;

/// The `body_html` the composer sends: the user's text rendered when it is
/// formatted or a quote rides along, with the quoted original (a reply or
/// forward, already HTML from [`super::answer_draft`]) above or below it.
/// Empty means "send plain text only".
#[must_use]
pub fn body_html(
    text: &str,
    images: &BTreeMap<u32, String>,
    quote_html: &str,
    quote_first: bool,
) -> String {
    let own = if has_formatting(text) || !quote_html.is_empty() {
        to_html(text, images)
    } else {
        String::new()
    };
    match (quote_html.is_empty(), quote_first) {
        (true, _) => own,
        (false, true) => format!("{quote_html}{own}"),
        (false, false) => format!("{own}{quote_html}"),
    }
}

/// What Send will produce for the send-format setting, in words: `auto`
/// depends on whether the text carries formatting.
#[must_use]
pub fn send_format_note(format: &str, formatted: bool) -> &'static str {
    match format {
        "plain" => "Sends as plain text",
        "html" => "Sends as HTML",
        "multipart" => "Sends as plain text and HTML",
        _ if formatted => "Sends formatted (HTML)",
        _ => "Sends as plain text",
    }
}

/// The text token for inline image `id`; brackets in the name would end the
/// token early, so they are dropped.
#[must_use]
pub fn inline_image_token(id: u32, name: &str) -> String {
    let name: String = name.chars().filter(|c| *c != '[' && *c != ']').collect();
    format!("![{name}](inline:{id})")
}

/// The body text after a toolbar action, with the selection to put back.
/// Offsets are UTF-16 code units — what both touch toolkits' text fields
/// use — so a frontend passes its selection through untouched.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Edit {
    pub text: String,
    pub start: usize,
    pub end: usize,
}

/// One formatting-toolbar action on the body: `bold` / `italic` wrap the
/// selection in their marks (a placeholder word when nothing is selected),
/// `quote` prefixes each selected line with `> ` (every line without a
/// selection), `bullet` puts `- ` at the start of the cursor's line. A
/// negative `start` means "no selection": the end of the text. `None` for
/// an unknown action.
#[must_use]
pub fn apply_edit(action: &str, text: &str, start: i64, end: i64) -> Option<Edit> {
    let total = utf16_len(text);
    let has_selection = start >= 0;
    let clamp = |o: i64| {
        if o < 0 {
            total
        } else {
            (o as usize).min(total)
        }
    };
    let (mut a, mut b) = (clamp(start), clamp(end.max(start)));
    if b < a {
        std::mem::swap(&mut a, &mut b);
    }
    let (ba, bb) = (byte_at(text, a), byte_at(text, b));
    let (before, middle, after) = (&text[..ba], &text[ba..bb], &text[bb..]);
    match action {
        "bold" | "italic" => {
            let mark = if action == "bold" { "**" } else { "*" };
            let insert = if middle.is_empty() { "text" } else { middle };
            let head = format!("{before}{mark}{insert}{mark}");
            let cursor = utf16_len(&head);
            Some(Edit {
                text: format!("{head}{after}"),
                start: cursor,
                end: cursor,
            })
        }
        "quote" => {
            let quote = |s: &str| {
                s.split('\n')
                    .map(|l| format!("> {l}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            if !has_selection {
                let quoted = quote(text);
                let cursor = utf16_len(&quoted);
                return Some(Edit {
                    text: quoted,
                    start: cursor,
                    end: cursor,
                });
            }
            let head = format!("{before}{}", quote(middle));
            let cursor = utf16_len(&head);
            Some(Edit {
                text: format!("{head}{after}"),
                start: cursor,
                end: cursor,
            })
        }
        "bullet" => {
            let line_start = text[..ba].rfind('\n').map_or(0, |i| i + 1);
            let cursor = a + 2;
            Some(Edit {
                text: format!("{}- {}", &text[..line_start], &text[line_start..]),
                start: cursor,
                end: cursor,
            })
        }
        _ => None,
    }
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Byte index of UTF-16 offset `off` (clamped; never inside a char).
fn byte_at(s: &str, off: usize) -> usize {
    let mut units = 0;
    for (i, c) in s.char_indices() {
        if units >= off {
            return i;
        }
        units += c.len_utf16();
    }
    s.len()
}

/// Whether the text carries any formatting [`to_html`] renders.
#[must_use]
pub fn has_formatting(text: &str) -> bool {
    if find_bold(text, 0).is_some() || has_italic(text) || find_image(text, 0).is_some() {
        return true;
    }
    text.split('\n').any(|line| {
        let t = line.trim_start();
        t.starts_with('>') || is_bullet(t) || find_link(t, 0).is_some()
    })
}

/// Render the supported subset to an HTML fragment (no `<html>` wrapper).
/// `images` maps `inline:N` tokens to their `data:` URLs; a token without
/// one renders as its name.
#[must_use]
pub fn to_html(text: &str, images: &BTreeMap<u32, String>) -> String {
    let mut out = String::new();
    let mut quote: Option<Vec<String>> = None;
    let mut bullets: Option<Vec<String>> = None;

    let flush_quote = |out: &mut String, quote: &mut Option<Vec<String>>| {
        if let Some(lines) = quote.take() {
            let body: Vec<String> = lines.iter().map(|l| inline(l, images)).collect();
            out.push_str(&format!("<blockquote>{}</blockquote>", body.join("<br>")));
        }
    };
    let flush_bullets = |out: &mut String, bullets: &mut Option<Vec<String>>| {
        if let Some(items) = bullets.take() {
            out.push_str("<ul>");
            for item in items {
                out.push_str(&format!("<li>{}</li>", inline(&item, images)));
            }
            out.push_str("</ul>");
        }
    };

    for raw in text.split('\n') {
        let line = raw.trim_end();
        let left = line.trim_start();
        if left.starts_with('>') {
            flush_bullets(&mut out, &mut bullets);
            // Like the `^> ?` rule: only a marker at the very start goes.
            let content = line
                .strip_prefix("> ")
                .or_else(|| line.strip_prefix('>'))
                .unwrap_or(line);
            quote.get_or_insert_with(Vec::new).push(content.to_string());
        } else if is_bullet(left) {
            flush_quote(&mut out, &mut quote);
            bullets
                .get_or_insert_with(Vec::new)
                .push(left[2..].to_string());
        } else if line.trim().is_empty() {
            flush_quote(&mut out, &mut quote);
            flush_bullets(&mut out, &mut bullets);
        } else {
            flush_quote(&mut out, &mut quote);
            flush_bullets(&mut out, &mut bullets);
            out.push_str(&format!("<p>{}</p>", inline(line, images)));
        }
    }
    flush_quote(&mut out, &mut quote);
    flush_bullets(&mut out, &mut bullets);
    out
}

/// `[-*] ` at the start.
fn is_bullet(s: &str) -> bool {
    s.starts_with("- ") || s.starts_with("* ")
}

/// Inline spans after HTML-escaping: images, links (web schemes only),
/// bold, then italic.
fn inline(text: &str, images: &BTreeMap<u32, String>) -> String {
    let s = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    // Images first, before the link rule sees `[name](…)`; the `data:` URL
    // is base64, so nothing below can match inside it.
    let s = replace_all(&s, find_image, |s, m| {
        let alt = s[m.groups[0].0..m.groups[0].1].replace('"', "&quot;");
        let id: Option<u32> = s[m.groups[1].0..m.groups[1].1].parse().ok();
        match id.and_then(|id| images.get(&id)) {
            Some(url) => format!("<img alt=\"{alt}\" src=\"{url}\">"),
            None => alt,
        }
    });
    let s = replace_all(&s, find_link, |s, m| {
        let label = &s[m.groups[0].0..m.groups[0].1];
        let url = &s[m.groups[1].0..m.groups[1].1];
        let lower = url.trim().to_ascii_lowercase();
        if lower.starts_with("http://")
            || lower.starts_with("https://")
            || lower.starts_with("mailto:")
        {
            format!("<a href=\"{url}\">{label}</a>")
        } else {
            label.to_string()
        }
    });
    let s = replace_all(&s, find_bold, |s, m| {
        format!("<b>{}</b>", &s[m.groups[0].0..m.groups[0].1])
    });
    render_italic(&s)
}

/// One pattern match: the whole span and its capture groups, as byte ranges.
/// Every pattern here starts and ends on ASCII, so the ranges are char
/// boundaries.
struct Match {
    start: usize,
    end: usize,
    groups: Vec<(usize, usize)>,
}

type Finder = fn(&str, usize) -> Option<Match>;

/// Leftmost non-overlapping replacement, like `replaceAllMapped`.
fn replace_all(s: &str, find: Finder, render: impl Fn(&str, &Match) -> String) -> String {
    let mut out = String::new();
    let mut pos = 0;
    while let Some(m) = find(s, pos) {
        out.push_str(&s[pos..m.start]);
        out.push_str(&render(s, &m));
        pos = m.end;
    }
    out.push_str(&s[pos..]);
    out
}

fn next_byte(s: &str, from: usize, b: u8) -> Option<usize> {
    s.as_bytes()
        .get(from..)?
        .iter()
        .position(|&c| c == b)
        .map(|i| from + i)
}

/// `\*\*([^*]+)\*\*`
fn find_bold(s: &str, from: usize) -> Option<Match> {
    let bytes = s.as_bytes();
    let mut i = from;
    while let Some(open) = next_byte(s, i, b'*') {
        if bytes.get(open + 1) == Some(&b'*') {
            if let Some(close) = next_byte(s, open + 2, b'*') {
                if close > open + 2 && bytes.get(close + 1) == Some(&b'*') {
                    return Some(Match {
                        start: open,
                        end: close + 2,
                        groups: vec![(open + 2, close)],
                    });
                }
            }
        }
        i = open + 1;
    }
    None
}

/// `\*([^*]+)\*`
fn find_star_pair(s: &str, from: usize) -> Option<Match> {
    let mut i = from;
    while let Some(open) = next_byte(s, i, b'*') {
        let close = next_byte(s, open + 1, b'*')?;
        if close > open + 1 {
            return Some(Match {
                start: open,
                end: close + 1,
                groups: vec![(open + 1, close)],
            });
        }
        i = open + 1;
    }
    None
}

/// `[\w*]` on the byte next to a star pair (ASCII word chars only, like the
/// Dart class; any byte of a multi-byte char is not one).
fn is_word_byte(b: Option<&u8>) -> bool {
    b.is_some_and(|&c| c.is_ascii_alphanumeric() || c == b'_' || c == b'*')
}

/// A `*…*` pair is italic only with non-word bytes on both sides, so
/// `2*3=6 and a * b` stays prose and `**bold**`'s inner pair does not count.
fn is_italic_pair(s: &str, m: &Match) -> bool {
    let bytes = s.as_bytes();
    let before = m.start.checked_sub(1).and_then(|i| bytes.get(i));
    !is_word_byte(before) && !is_word_byte(bytes.get(m.end))
}

fn has_italic(text: &str) -> bool {
    let mut pos = 0;
    while let Some(m) = find_star_pair(text, pos) {
        if is_italic_pair(text, &m) {
            return true;
        }
        pos = m.end;
    }
    false
}

fn render_italic(s: &str) -> String {
    let mut out = String::new();
    let mut pos = 0;
    let mut scan = 0;
    while let Some(m) = find_star_pair(s, scan) {
        if is_italic_pair(s, &m) {
            out.push_str(&s[pos..m.start]);
            out.push_str(&format!("<i>{}</i>", &s[m.groups[0].0..m.groups[0].1]));
            pos = m.end;
        }
        scan = m.end;
    }
    out.push_str(&s[pos..]);
    out
}

/// `!\[([^\]]*)\]\(inline:(\d+)\)`
fn find_image(s: &str, from: usize) -> Option<Match> {
    let mut i = from;
    while let Some(bang) = next_byte(s, i, b'!') {
        if let Some(m) = image_at(s, bang) {
            return Some(m);
        }
        i = bang + 1;
    }
    None
}

fn image_at(s: &str, bang: usize) -> Option<Match> {
    let rest = s.get(bang..)?;
    if !rest.starts_with("![") {
        return None;
    }
    let alt_start = bang + 2;
    let alt_end = next_byte(s, alt_start, b']')?;
    let after = s.get(alt_end..)?.strip_prefix("](inline:")?;
    let digits = after.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || after.as_bytes().get(digits) != Some(&b')') {
        return None;
    }
    let id_start = alt_end + "](inline:".len();
    Some(Match {
        start: bang,
        end: id_start + digits + 1,
        groups: vec![(alt_start, alt_end), (id_start, id_start + digits)],
    })
}

/// `\[([^\]]+)\]\(((?:[^()]*|\([^()]*\))*)\)`: a label without `]`, then a
/// URL that may hold one level of balanced parentheses.
fn find_link(s: &str, from: usize) -> Option<Match> {
    let mut i = from;
    while let Some(open) = next_byte(s, i, b'[') {
        if let Some(m) = link_at(s, open) {
            return Some(m);
        }
        i = open + 1;
    }
    None
}

fn link_at(s: &str, open: usize) -> Option<Match> {
    let bytes = s.as_bytes();
    let label_end = next_byte(s, open + 1, b']')?;
    if label_end == open + 1 || bytes.get(label_end + 1) != Some(&b'(') {
        return None;
    }
    let url_start = label_end + 2;
    let mut p = url_start;
    loop {
        match bytes.get(p)? {
            b')' => {
                return Some(Match {
                    start: open,
                    end: p + 1,
                    groups: vec![(open + 1, label_end), (url_start, p)],
                });
            }
            b'(' => {
                // One nested group: up to the next `)` with no `(` inside.
                let mut q = p + 1;
                loop {
                    match bytes.get(q)? {
                        b')' => break,
                        b'(' => return None,
                        _ => q += 1,
                    }
                }
                p = q + 1;
            }
            _ => p += 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> BTreeMap<u32, String> {
        BTreeMap::new()
    }

    #[test]
    fn plain_prose_has_no_formatting() {
        assert!(!has_formatting("Hello,\n\nsee attached.\n"));
        assert!(!has_formatting("2*3=6 and a * b"));
    }

    #[test]
    fn toolbar_syntax_counts_as_formatting() {
        assert!(has_formatting("a **bold** word"));
        assert!(has_formatting("an *italic* word"));
        assert!(has_formatting("> quoted"));
        assert!(has_formatting("- bullet"));
        assert!(has_formatting("[docs](https://example.com)"));
    }

    #[test]
    fn bold_and_italic_render_inline() {
        assert_eq!(
            to_html("a **bold** and *italic* word", &none()),
            "<p>a <b>bold</b> and <i>italic</i> word</p>"
        );
    }

    #[test]
    fn quotes_group_into_one_blockquote() {
        assert_eq!(
            to_html("Hi\n> line one\n> line two\nBye", &none()),
            "<p>Hi</p><blockquote>line one<br>line two</blockquote><p>Bye</p>"
        );
    }

    #[test]
    fn bullets_group_into_a_list() {
        assert_eq!(
            to_html("- one\n- two", &none()),
            "<ul><li>one</li><li>two</li></ul>"
        );
    }

    #[test]
    fn links_allow_web_schemes_only() {
        assert_eq!(
            to_html("[a](https://example.com/x)", &none()),
            "<p><a href=\"https://example.com/x\">a</a></p>"
        );
        assert_eq!(to_html("[a](javascript:alert(1))", &none()), "<p>a</p>");
        assert_eq!(
            to_html("[w](https://example.org/a_(b))", &none()),
            "<p><a href=\"https://example.org/a_(b)\">w</a></p>"
        );
    }

    #[test]
    fn raw_html_is_escaped() {
        let html = to_html("<script>alert(1)</script>", &none());
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn inline_image_tokens_render_their_url_or_name() {
        let url = "data:image/png;base64,Zm9v";
        let text = "see ![logo.png](inline:1) and ![gone](inline:2)";
        assert!(has_formatting(text));
        let images = BTreeMap::from([(1, url.to_string())]);
        assert_eq!(
            to_html(text, &images),
            format!("<p>see <img alt=\"logo.png\" src=\"{url}\"> and gone</p>")
        );
    }

    #[test]
    fn non_ascii_text_survives() {
        assert_eq!(
            to_html("Grüße **ganz** *lieb* – ok", &none()),
            "<p>Grüße <b>ganz</b> <i>lieb</i> – ok</p>"
        );
    }

    #[test]
    fn body_html_is_empty_for_plain_text_without_quote() {
        assert_eq!(body_html("just text", &none(), "", false), "");
    }

    #[test]
    fn body_html_places_the_quote() {
        let q = "<blockquote>orig</blockquote>";
        assert_eq!(body_html("hi", &none(), q, false), format!("<p>hi</p>{q}"));
        assert_eq!(body_html("hi", &none(), q, true), format!("{q}<p>hi</p>"));
    }

    #[test]
    fn bold_wraps_the_selection_or_a_placeholder() {
        let e = apply_edit("bold", "say hi now", 4, 6).unwrap();
        assert_eq!(e.text, "say **hi** now");
        assert_eq!((e.start, e.end), (10, 10));
        let e = apply_edit("italic", "x", -1, -1).unwrap();
        assert_eq!(e.text, "x*text*");
    }

    #[test]
    fn quote_prefixes_selected_lines_or_all() {
        assert_eq!(
            apply_edit("quote", "a\nb\nc", 2, 5).unwrap().text,
            "a\n> b\n> c"
        );
        assert_eq!(
            apply_edit("quote", "a\nb", -1, -1).unwrap().text,
            "> a\n> b"
        );
    }

    #[test]
    fn bullet_goes_to_the_cursor_line_start() {
        let e = apply_edit("bullet", "one\ntwo", 5, 5).unwrap();
        assert_eq!(e.text, "one\n- two");
        assert_eq!(e.start, 7);
    }

    #[test]
    fn edit_offsets_are_utf16() {
        // "😀" is two UTF-16 units: offsets 3..5 select "ok" after "😀 ".
        let e = apply_edit("bold", "😀 ok", 3, 5).unwrap();
        assert_eq!(e.text, "😀 **ok**");
        assert_eq!(e.start, 9);
        assert!(apply_edit("nope", "x", 0, 0).is_none());
    }

    #[test]
    fn token_drops_brackets_from_the_name() {
        assert_eq!(inline_image_token(3, "a[1].png"), "![a1.png](inline:3)");
    }

    #[test]
    fn format_note_follows_setting_and_formatting() {
        assert_eq!(send_format_note("auto", false), "Sends as plain text");
        assert_eq!(send_format_note("auto", true), "Sends formatted (HTML)");
        assert_eq!(
            send_format_note("multipart", false),
            "Sends as plain text and HTML"
        );
    }
}
