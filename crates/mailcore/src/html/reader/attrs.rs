//! Walking the opening tags of sanitizer output.
//!
//! Text is escaped there, so `<` always opens a tag; attributes are
//! ` name="value"` with `"` escaped inside values, so a value can never hold
//! the sequence that starts another attribute or ends the tag.

/// `html` with every opening tag that has attributes passed through `f`
/// (tag name, attribute text with its leading space). `f` returns the new
/// attribute text, or `None` to keep the tag as it is.
pub(super) fn map_tags(html: &str, mut f: impl FnMut(&str, &str) -> Option<String>) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        let tag = &rest[lt + 1..];
        let name_len = tag
            .bytes()
            .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            .count();
        let opens = name_len > 0 && tag.as_bytes()[0].is_ascii_lowercase();
        let spaced = tag[name_len..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace);
        match tag.find('>') {
            Some(gt) if opens && spaced && gt > name_len => {
                let name = &tag[..name_len];
                let attrs = &tag[name_len..gt];
                out.push('<');
                out.push_str(name);
                match f(name, attrs) {
                    Some(new) => out.push_str(&new),
                    None => out.push_str(attrs),
                }
                out.push('>');
                rest = &tag[gt + 1..];
            }
            _ => {
                out.push('<');
                rest = tag;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Every opening tag with attributes: name and attribute text.
pub(super) fn tags(html: &str) -> Vec<(&str, &str)> {
    let mut found = Vec::new();
    let mut rest = html;
    while let Some(lt) = rest.find('<') {
        let tag = &rest[lt + 1..];
        rest = tag;
        let name_len = tag
            .bytes()
            .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            .count();
        if name_len == 0 || !tag.as_bytes()[0].is_ascii_lowercase() {
            continue;
        }
        if !tag[name_len..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
        {
            continue;
        }
        if let Some(gt) = tag.find('>') {
            found.push((&tag[..name_len], &tag[name_len..gt]));
            rest = &tag[gt + 1..];
        }
    }
    found
}

/// The first ` name="value"` in `attrs`: its byte range and its value.
pub(super) fn find_attr<'a>(
    attrs: &'a str,
    name: &str,
) -> Option<(std::ops::Range<usize>, &'a str)> {
    let needle = format!(" {name}=\"");
    let start = attrs.find(&needle)?;
    let value_at = start + needle.len();
    let len = attrs[value_at..].find('"')?;
    Some((start..value_at + len + 1, &attrs[value_at..value_at + len]))
}

/// `attrs` with `range` replaced by `with`.
pub(super) fn splice(attrs: &str, range: std::ops::Range<usize>, with: &str) -> String {
    format!("{}{with}{}", &attrs[..range.start], &attrs[range.end..])
}
