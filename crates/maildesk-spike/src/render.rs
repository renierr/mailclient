//! Prototype sanitized-HTML → egui renderer (Spike A, throwaway).
//!
//! Input is always `mailcore::html` output: sanitizer-normalized tags and
//! attributes, no scripts/styles/forms, `reader::body()` already applied
//! (dark rewrite + narrow fit). The parser therefore covers exactly the
//! allow-list (`tags.rs`: ~55 tags) and degrades gracefully, never errors.
//!
//! Deliberately lossy, per EGUI-DESKTOP.md §5: multi-column table layouts on
//! narrow screens render simplified, `display:none` subtrees are skipped,
//! exotic `display` values are ignored, `float` is approximated (image on
//! its own line).

use std::collections::HashMap;

use mailcore::html::{decode_entities, link_info};

// ---------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------

/// Inline text marks. One run of text carries the full set.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Marks {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub code: bool,
    /// -1 (`small`) .. +1 (`big`).
    pub size_delta: i8,
    /// -1 sub, +1 sup, 0 baseline.
    pub raised: i8,
}

#[derive(Debug, Clone)]
pub enum Inline {
    Text(String, Marks, Option<String>),
    Link {
        href: String,
        label: String,
        safe: bool,
    },
    Break,
}

/// Horizontal alignment surviving the sanitizer (`text-align` / `center`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone)]
pub enum Block {
    Para {
        inlines: Vec<Inline>,
        align: Align,
        bg: Option<String>,
    },
    Heading {
        level: u8,
        inlines: Vec<Inline>,
        align: Align,
        bg: Option<String>,
    },
    Quote(Vec<Block>),
    List {
        ordered: bool,
        items: Vec<Vec<Block>>,
    },
    Code(String),
    Table(Vec<TableRow>),
    Image {
        src: String,
        alt: String,
    },
    Hr,
}

#[derive(Debug, Clone)]
pub struct TableRow {
    pub cells: Vec<TableCell>,
}

#[derive(Debug, Clone)]
pub struct TableCell {
    pub header: bool,
    pub blocks: Vec<Block>,
    /// `bgcolor` / `background-color` surviving the sanitizer, `#rrggbb`.
    pub bg: Option<String>,
}

// ---------------------------------------------------------------------------
// Tokenizer (sanitizer-normalized HTML: lowercase tags, `name="…"` attrs)
// ---------------------------------------------------------------------------

#[derive(Clone)]
enum Token {
    Start {
        name: String,
        attrs: HashMap<String, String>,
    },
    End(String),
    Text(String),
}

fn tokenize(html: &str) -> Vec<Token> {
    let bytes = html.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut text = String::new();
    let flush = |text: &mut String, out: &mut Vec<Token>| {
        if !text.is_empty() {
            out.push(Token::Text(std::mem::take(text)));
        }
    };
    while i < bytes.len() {
        if bytes[i] == b'<' {
            if html[i..].starts_with("<!--") {
                flush(&mut text, &mut out);
                if let Some(end) = html[i..].find("-->") {
                    i += end + 3;
                } else {
                    break;
                }
                continue;
            }
            // Tag end, respecting quoted attributes.
            let mut j = i + 1;
            let mut quote = 0u8;
            while j < bytes.len() && (bytes[j] != b'>' || quote != 0) {
                if quote != 0 && bytes[j] == quote {
                    quote = 0;
                } else if quote == 0 && (bytes[j] == b'"' || bytes[j] == b'\'') {
                    quote = bytes[j];
                }
                j += 1;
            }
            if j >= bytes.len() {
                text.push_str(&html[i..]);
                break;
            }
            flush(&mut text, &mut out);
            let raw = html[i + 1..j].trim().trim_end_matches('/');
            i = j + 1;
            if raw.starts_with('!') || raw.starts_with('?') {
                continue;
            }
            if let Some(name) = raw.strip_prefix('/') {
                let tag = name
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if !tag.is_empty() {
                    out.push(Token::End(tag));
                }
                continue;
            }
            let mut parts = raw.split_whitespace();
            let name = parts.next().unwrap_or("").to_ascii_lowercase();
            if name.is_empty() {
                continue;
            }
            let mut attrs = HashMap::new();
            for part in parts {
                if let Some((k, v)) = part.split_once('=') {
                    let v = v.trim_matches('"').trim_matches('\'');
                    attrs.insert(k.to_ascii_lowercase(), decode_entities(v));
                } else if !part.is_empty() && part != "/" {
                    attrs.insert(part.to_ascii_lowercase(), String::new());
                }
            }
            out.push(Token::Start { name, attrs });
        } else {
            // Text until the next tag: slice by `str`, never byte-as-char
            // (multi-byte UTF-8 would become mojibake).
            let next = html[i..].find('<').map(|k| i + k).unwrap_or(html.len());
            text.push_str(&html[i..next]);
            i = next;
        }
    }
    flush(&mut text, &mut out);
    out
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

fn is_void(name: &str) -> bool {
    matches!(name, "br" | "hr" | "img" | "col")
}

fn is_block(name: &str) -> bool {
    matches!(
        name,
        "p" | "div"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "blockquote"
            | "ul"
            | "ol"
            | "pre"
            | "hr"
            | "table"
            | "center"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "main"
            | "figure"
            | "dl"
            | "dt"
            | "dd"
            | "tr"
    )
}

/// `display:none` (preheaders) and other exotic `display` values: skip.
fn is_hidden_style(attrs: &HashMap<String, String>) -> bool {
    attrs.get("style").is_some_and(|s| {
        s.split(';').any(|decl| {
            let mut kv = decl.splitn(2, ':');
            kv.next().is_some_and(|k| k.trim() == "display")
                && kv.next().is_some_and(|v| v.trim() == "none")
        })
    })
}

fn style_align(attrs: &HashMap<String, String>) -> Align {
    if let Some(style) = attrs.get("style") {
        for decl in style.split(';') {
            let mut kv = decl.splitn(2, ':');
            if kv.next().is_some_and(|k| k.trim() == "text-align") {
                match kv.next().unwrap_or("").trim() {
                    "center" => return Align::Center,
                    "right" => return Align::Right,
                    _ => {}
                }
            }
        }
    }
    if let Some(a) = attrs.get("align") {
        match a.as_str() {
            "center" | "middle" => return Align::Center,
            "right" => return Align::Right,
            _ => {}
        }
    }
    Align::Left
}

fn style_color(attrs: &HashMap<String, String>) -> Option<String> {
    attrs.get("color").cloned().or_else(|| {
        attrs.get("style").and_then(|s| {
            s.split(';').find_map(|decl| {
                let mut kv = decl.splitn(2, ':');
                (kv.next().is_some_and(|k| k.trim() == "color"))
                    .then(|| kv.next().unwrap_or("").trim().to_string())
            })
        })
    })
}

fn cell_bg(attrs: &HashMap<String, String>) -> Option<String> {
    attrs.get("bgcolor").cloned().or_else(|| {
        attrs.get("style").and_then(|s| {
            s.split(';').find_map(|decl| {
                let mut kv = decl.splitn(2, ':');
                (kv.next()
                    .is_some_and(|k| k.trim() == "background-color" || k.trim() == "background"))
                .then(|| kv.next().unwrap_or("").trim().to_string())
            })
        })
    })
}

struct Stream {
    tokens: Vec<Token>,
    pos: usize,
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut ws = false;
    for c in s.chars() {
        if c.is_whitespace() {
            ws = true;
        } else {
            if ws && !out.is_empty() {
                out.push(' ');
            }
            ws = false;
            out.push(c);
        }
    }
    out
}

/// Entry point: sanitized HTML → block list. Never fails.
#[must_use]
pub fn parse(html: &str) -> Vec<Block> {
    let mut st = Stream {
        tokens: tokenize(html),
        pos: 0,
    };
    parse_blocks(&mut st, &[], None)
}

fn parse_blocks(st: &mut Stream, stop: &[&str], inherit: Option<String>) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut inlines: Vec<Inline> = Vec::new();
    let flush = |inlines: &mut Vec<Inline>, blocks: &mut Vec<Block>| {
        let kept: Vec<Inline> = std::mem::take(inlines)
            .into_iter()
            .filter(|il| !matches!(il, Inline::Text(t, _, _) if t.trim().is_empty()))
            .collect();
        if !kept.is_empty() {
            blocks.push(Block::Para {
                inlines: kept,
                align: Align::Left,
                bg: None,
            });
        }
    };

    while st.pos < st.tokens.len() {
        match st.tokens[st.pos].clone() {
            Token::End(name) if stop.contains(&name.as_str()) => break,
            Token::End(_) => st.pos += 1, // stray close: ignore
            Token::Text(t) => {
                st.pos += 1;
                let t = decode_entities(&collapse_ws(&t));
                if !t.trim().is_empty() {
                    inlines.push(Inline::Text(t, Marks::default(), inherit.clone()));
                }
            }
            Token::Start { name, attrs } => {
                st.pos += 1;
                if is_hidden_style(&attrs) {
                    skip_subtree(st, &name);
                    continue;
                }
                if name == "br" {
                    inlines.push(Inline::Break);
                } else if name == "hr" {
                    flush(&mut inlines, &mut blocks);
                    blocks.push(Block::Hr);
                } else if name == "img" {
                    // `float` is approximated: the image gets its own line.
                    flush(&mut inlines, &mut blocks);
                    blocks.push(Block::Image {
                        src: attrs.get("src").cloned().unwrap_or_default(),
                        alt: attrs.get("alt").cloned().unwrap_or_default(),
                    });
                } else if name == "a" {
                    let href = attrs.get("href").cloned().unwrap_or_default();
                    let label = collapse_ws(&collect_text(st, &name));
                    if !label.is_empty() {
                        inlines.push(Inline::Link {
                            safe: link_info(&href).safe,
                            href,
                            label,
                        });
                    }
                } else if is_block(&name) {
                    flush(&mut inlines, &mut blocks);
                    blocks.extend(parse_block_tag(st, &name, &attrs, inherit.clone()));
                } else {
                    // Inline mark / span / font: recurse with merged marks.
                    let (m2, c2) = inline_marks(&name, &attrs);
                    inlines.extend(parse_inlines(st, &name, m2, c2.or_else(|| inherit.clone())));
                }
            }
        }
    }
    flush(&mut inlines, &mut blocks);
    blocks
}

/// One already-consumed block tag → zero or more blocks. `inherit` is the
/// text colour of enclosing blocks (a designed mail's `div` colour reaches
/// every nested run unless a closer tag overrides it).
fn parse_block_tag(
    st: &mut Stream,
    name: &str,
    attrs: &HashMap<String, String>,
    inherit: Option<String>,
) -> Vec<Block> {
    let child_color = style_color(attrs).or(inherit);
    let bg = cell_bg(attrs);
    match name {
        "p" | "div" | "section" | "article" | "header" | "footer" | "main" | "figure" | "dl"
        | "tr" => {
            let inner = parse_blocks(st, &[name], child_color);
            finish_container(inner, style_align(attrs), bg)
        }
        "center" => {
            let inner = parse_blocks(st, &[name], child_color);
            finish_container(inner, Align::Center, bg)
        }
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let level: u8 = name[1..].parse().unwrap_or(3);
            let inner = parse_blocks(st, &[name], child_color);
            // A heading with block children degrades to its text.
            let inlines = flatten_inlines(inner);
            if inlines.iter().all(|il| matches!(il, Inline::Break)) || inlines.is_empty() {
                Vec::new()
            } else {
                vec![Block::Heading {
                    level,
                    inlines,
                    align: style_align(attrs),
                    bg,
                }]
            }
        }
        "blockquote" | "dd" => {
            let inner = parse_blocks(st, &[name], child_color);
            if inner.is_empty() {
                Vec::new()
            } else {
                vec![Block::Quote(inner)]
            }
        }
        "dt" => {
            let inner = parse_blocks(st, &[name], child_color);
            let mut inlines = flatten_inlines(inner);
            for il in &mut inlines {
                if let Inline::Text(_, m, _) = il {
                    m.bold = true;
                }
            }
            vec![Block::Para {
                inlines,
                align: Align::Left,
                bg,
            }]
        }
        "ul" | "ol" => {
            let ordered = name == "ol";
            let mut items = Vec::new();
            loop {
                if st.pos >= st.tokens.len() {
                    break;
                }
                match st.tokens[st.pos].clone() {
                    Token::End(n) if n == name => {
                        st.pos += 1;
                        break;
                    }
                    Token::Start { name: n, .. } if n == "li" => {
                        st.pos += 1;
                        let item = parse_blocks(st, &["li"], child_color.clone());
                        if matches!(st.tokens.get(st.pos), Some(Token::End(n)) if n == "li") {
                            st.pos += 1;
                        }
                        if !item.is_empty() {
                            items.push(item);
                        }
                    }
                    _ => st.pos += 1, // stray text between items: drop
                }
            }
            if items.is_empty() {
                Vec::new()
            } else {
                vec![Block::List { ordered, items }]
            }
        }
        "pre" => {
            let text = decode_entities(collect_text(st, "pre").trim_matches('\n'));
            if text.trim().is_empty() {
                Vec::new()
            } else {
                vec![Block::Code(text)]
            }
        }
        "table" => parse_table(st, child_color).map_or(Vec::new(), |t| vec![Block::Table(t)]),
        _ => {
            let inner = parse_blocks(st, &[name], child_color);
            finish_container(inner, style_align(attrs), bg)
        }
    }
}

/// Alignment + background of a `div`-ish wrapper land on its top-level
/// text blocks (deeper blocks keep their own).
fn finish_container(blocks: Vec<Block>, align: Align, bg: Option<String>) -> Vec<Block> {
    if align == Align::Left && bg.is_none() {
        return blocks;
    }
    blocks
        .into_iter()
        .map(|b| match b {
            Block::Para {
                inlines,
                align: a,
                bg: b,
            } => Block::Para {
                inlines,
                align: if align == Align::Left { a } else { align },
                bg: b.or_else(|| bg.clone()),
            },
            Block::Heading {
                level,
                inlines,
                align: a,
                bg: b,
            } => Block::Heading {
                level,
                inlines,
                align: if align == Align::Left { a } else { align },
                bg: b.or_else(|| bg.clone()),
            },
            other => other,
        })
        .collect()
}

fn flatten_inlines(blocks: Vec<Block>) -> Vec<Inline> {
    let mut out = Vec::new();
    for b in blocks {
        match b {
            Block::Para { mut inlines, .. } | Block::Heading { mut inlines, .. } => {
                out.append(&mut inlines);
                out.push(Inline::Break);
            }
            Block::Quote(inner) => {
                out.extend(flatten_inlines(inner));
                out.push(Inline::Break);
            }
            Block::List { items, .. } => {
                for item in items {
                    out.extend(flatten_inlines(item));
                    out.push(Inline::Break);
                }
            }
            Block::Code(t) => {
                out.push(Inline::Text(
                    t,
                    Marks {
                        code: true,
                        ..Default::default()
                    },
                    None,
                ));
                out.push(Inline::Break);
            }
            Block::Table(rows) => {
                for row in rows {
                    for cell in row.cells {
                        out.extend(flatten_inlines(cell.blocks));
                        out.push(Inline::Text(" | ".to_string(), Marks::default(), None));
                    }
                    out.push(Inline::Break);
                }
            }
            Block::Image { alt, .. } => {
                out.push(Inline::Text(
                    format!("[image: {alt}]"),
                    Marks::default(),
                    None,
                ));
                out.push(Inline::Break);
            }
            Block::Hr => out.push(Inline::Break),
        }
    }
    while matches!(out.last(), Some(Inline::Break)) {
        out.pop();
    }
    out
}

fn parse_table(st: &mut Stream, inherit: Option<String>) -> Option<Vec<TableRow>> {
    let mut rows = Vec::new();
    loop {
        if st.pos >= st.tokens.len() {
            break;
        }
        match st.tokens[st.pos].clone() {
            Token::End(n) if n == "table" => {
                st.pos += 1;
                break;
            }
            Token::Start { name, .. } if name == "tr" => {
                st.pos += 1;
                rows.push(parse_row(st, inherit.clone()));
            }
            _ => st.pos += 1, // thead/tbody/tfoot/caption/colgroup/col: transparent
        }
    }
    // Drop fully-empty rows (spacer rows newsletters love).
    rows.retain(|r| {
        r.cells
            .iter()
            .any(|c| !c.blocks.is_empty() && !is_empty_para(&c.blocks))
    });
    if rows.is_empty() {
        None
    } else {
        Some(rows)
    }
}

fn is_empty_para(blocks: &[Block]) -> bool {
    matches!(blocks, [Block::Para { inlines, .. }] if inlines.is_empty())
}

fn parse_row(st: &mut Stream, inherit: Option<String>) -> TableRow {
    let mut cells = Vec::new();
    loop {
        if st.pos >= st.tokens.len() {
            break;
        }
        match st.tokens[st.pos].clone() {
            Token::End(n) if n == "tr" => {
                st.pos += 1;
                break;
            }
            Token::Start { name, attrs } if name == "td" || name == "th" => {
                let header = name == "th";
                let bg = cell_bg(&attrs);
                let color = style_color(&attrs).or_else(|| inherit.clone());
                st.pos += 1;
                let blocks = parse_blocks(st, &[name.as_str()], color);
                if matches!(st.tokens.get(st.pos), Some(Token::End(n)) if n == &name) {
                    st.pos += 1;
                }
                cells.push(TableCell { header, blocks, bg });
            }
            _ => st.pos += 1,
        }
    }
    TableRow { cells }
}

/// Inline marks contributed by one tag.
fn inline_marks(name: &str, attrs: &HashMap<String, String>) -> (Marks, Option<String>) {
    let mut m = Marks::default();
    match name {
        "b" | "strong" => m.bold = true,
        "i" | "em" => m.italic = true,
        "u" | "ins" => m.underline = true,
        "s" | "strike" | "del" => m.strike = true,
        "code" => m.code = true,
        "small" => m.size_delta = -1,
        "big" => m.size_delta = 1,
        "sub" => m.raised = -1,
        "sup" => m.raised = 1,
        _ => {}
    }
    (m, style_color(attrs))
}

/// Inline content until the matching close tag.
fn parse_inlines(st: &mut Stream, stop: &str, marks: Marks, color: Option<String>) -> Vec<Inline> {
    let mut out = Vec::new();
    while st.pos < st.tokens.len() {
        match st.tokens[st.pos].clone() {
            Token::End(n) if n == stop => {
                st.pos += 1;
                break;
            }
            Token::End(_) => st.pos += 1,
            Token::Text(t) => {
                st.pos += 1;
                let t = decode_entities(&collapse_ws(&t));
                if !t.trim().is_empty() {
                    out.push(Inline::Text(t, marks, color.clone()));
                }
            }
            Token::Start { name, attrs } => {
                st.pos += 1;
                if is_hidden_style(&attrs) {
                    skip_subtree(st, &name);
                } else if name == "br" {
                    out.push(Inline::Break);
                } else if name == "img" {
                    let alt = attrs.get("alt").cloned().unwrap_or_default();
                    out.push(Inline::Text(
                        format!("[image: {alt}]"),
                        marks,
                        color.clone(),
                    ));
                } else if name == "a" {
                    let href = attrs.get("href").cloned().unwrap_or_default();
                    let label = collapse_ws(&collect_text(st, &name));
                    if !label.is_empty() {
                        out.push(Inline::Link {
                            safe: link_info(&href).safe,
                            href,
                            label,
                        });
                    }
                } else if is_block(&name) {
                    // Block tag inside inline context: rewind so the block
                    // parser sees it.
                    st.pos -= 1;
                    break;
                } else {
                    let (m2, c2) = inline_marks(&name, &attrs);
                    let merged = Marks {
                        bold: marks.bold || m2.bold,
                        italic: marks.italic || m2.italic,
                        underline: marks.underline || m2.underline,
                        strike: marks.strike || m2.strike,
                        code: marks.code || m2.code,
                        size_delta: (marks.size_delta + m2.size_delta).clamp(-2, 2),
                        raised: if m2.raised != 0 {
                            m2.raised
                        } else {
                            marks.raised
                        },
                    };
                    let c = c2.or_else(|| color.clone());
                    out.extend(parse_inlines(st, &name, merged, c));
                }
            }
        }
    }
    out
}

/// Plain text until the matching close tag (marks flattened, `br` → space).
fn collect_text(st: &mut Stream, stop: &str) -> String {
    let mut out = String::new();
    while st.pos < st.tokens.len() {
        match st.tokens[st.pos].clone() {
            Token::End(n) if n == stop => {
                st.pos += 1;
                break;
            }
            Token::End(_) => st.pos += 1,
            Token::Text(t) => {
                out.push_str(&t);
                st.pos += 1;
            }
            Token::Start { name, .. } => {
                st.pos += 1;
                if name == "br" {
                    out.push(' ');
                } else if !is_void(&name) {
                    out.push_str(&collect_text(st, &name));
                }
            }
        }
    }
    decode_entities(&out)
}

fn skip_subtree(st: &mut Stream, stop: &str) {
    let mut depth = 1usize;
    while st.pos < st.tokens.len() && depth > 0 {
        match &st.tokens[st.pos] {
            Token::Start { name, .. } if !is_void(name) => depth += 1,
            Token::End(n) if n == stop => depth -= 1,
            _ => {}
        }
        st.pos += 1;
    }
}

// ---------------------------------------------------------------------------
// base64 (data: URIs) — std only
// ---------------------------------------------------------------------------

fn b64val(c: u8) -> Option<u32> {
    match c {
        b'A'..=b'Z' => Some((c - b'A') as u32),
        b'a'..=b'z' => Some((c - b'a' + 26) as u32),
        b'0'..=b'9' => Some((c - b'0' + 52) as u32),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

pub fn decode_base64(s: &str) -> Option<Vec<u8>> {
    let clean: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if clean.is_empty() || !clean.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    for chunk in clean.chunks(4) {
        let mut n = 0u32;
        let mut pad = 0;
        for (i, &c) in chunk.iter().enumerate() {
            if c == b'=' {
                if i < 2 {
                    return None;
                }
                pad += 1;
                n <<= 6;
            } else {
                if pad > 0 {
                    return None;
                }
                n = (n << 6) | b64val(c)?;
            }
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Some(out)
}

/// Split a `data:` URI into `(mime, bytes)`. Only `;base64` is supported.
pub fn decode_data_uri(src: &str) -> Option<(&str, Vec<u8>)> {
    let rest = src.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    if !meta.ends_with(";base64") {
        return None;
    }
    decode_base64(data).map(|b| (meta.trim_end_matches(";base64"), b))
}

// ---------------------------------------------------------------------------
// Text dump (structural verification + --dump mode)
// ---------------------------------------------------------------------------

/// Indented text tree of the parsed document. Used by `--dump` and tests.
#[must_use]
pub fn dump(blocks: &[Block]) -> String {
    let mut s = String::new();
    for b in blocks {
        dump_block(b, 0, &mut s);
    }
    s
}

fn dump_inline(il: &Inline) -> String {
    match il {
        Inline::Text(t, m, c) => {
            let mut tags = String::new();
            if m.bold {
                tags.push('B');
            }
            if m.italic {
                tags.push('I');
            }
            if m.underline {
                tags.push('U');
            }
            if m.strike {
                tags.push('S');
            }
            if m.code {
                tags.push('C');
            }
            if m.size_delta != 0 {
                tags.push_str(&format!("z{}", m.size_delta));
            }
            if m.raised != 0 {
                tags.push_str(&format!("r{}", m.raised));
            }
            let col = c.as_deref().unwrap_or("");
            format!("[t:{tags}:{col}:{t}]")
        }
        Inline::Link { href, label, safe } => {
            format!("[link safe={safe}:{label} -> {href}]")
        }
        Inline::Break => "[br]".to_string(),
    }
}

fn dump_block(b: &Block, indent: usize, s: &mut String) {
    let pad = "  ".repeat(indent);
    match b {
        Block::Para { inlines, align, bg } => {
            let bg = bg.as_deref().unwrap_or("");
            s.push_str(&format!(
                "{pad}P({align:?} bg={bg}): {}\n",
                inlines
                    .iter()
                    .map(dump_inline)
                    .collect::<Vec<_>>()
                    .join(" ")
            ));
        }
        Block::Heading {
            level,
            inlines,
            align,
            bg,
        } => {
            let bg = bg.as_deref().unwrap_or("");
            s.push_str(&format!(
                "{pad}H{level}({align:?} bg={bg}): {}\n",
                inlines
                    .iter()
                    .map(dump_inline)
                    .collect::<Vec<_>>()
                    .join(" ")
            ));
        }
        Block::Quote(inner) => {
            s.push_str(&format!("{pad}QUOTE:\n"));
            for c in inner {
                dump_block(c, indent + 1, s);
            }
        }
        Block::List { ordered, items } => {
            s.push_str(&format!(
                "{pad}LIST({}):\n",
                if *ordered { "ol" } else { "ul" }
            ));
            for item in items {
                s.push_str(&format!("{pad} - item:\n"));
                for c in item {
                    dump_block(c, indent + 2, s);
                }
            }
        }
        Block::Code(t) => {
            s.push_str(&format!("{pad}CODE:\n"));
            for line in t.lines() {
                s.push_str(&format!("{pad}  |{line}\n"));
            }
        }
        Block::Table(rows) => {
            let cols = rows.iter().map(|r| r.cells.len()).max().unwrap_or(0);
            s.push_str(&format!("{pad}TABLE({}x{cols}):\n", rows.len()));
            for row in rows {
                for cell in &row.cells {
                    let bg = cell.bg.as_deref().unwrap_or("");
                    s.push_str(&format!("{pad}  cell(h={} bg={bg}):\n", cell.header));
                    for c in &cell.blocks {
                        dump_block(c, indent + 2, s);
                    }
                }
            }
        }
        Block::Image { src, alt } => {
            let kind = if src.starts_with("data:") {
                "data"
            } else if src.is_empty() {
                "empty"
            } else {
                "remote"
            };
            s.push_str(&format!("{pad}IMG({kind} alt={alt:?} len={})\n", src.len()));
        }
        Block::Hr => s.push_str(&format!("{pad}HR\n")),
    }
}

// ---------------------------------------------------------------------------
// Tests (colocated, per AGENTS.md §3)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_nest() {
        let d = dump(&parse(
            "<p>plain <b>bold <i>both</i></b> <code>code</code></p>",
        ));
        assert!(d.contains("[t:::plain]"), "{d}");
        assert!(d.contains("[t:B::bold]"), "{d}");
        assert!(d.contains("[t:BI::both]"), "{d}");
        assert!(d.contains("[t:C::code]"), "{d}");
    }

    #[test]
    fn hidden_subtree_skipped() {
        let d = dump(&parse(
            "<div><span style=\"display:none\">preheader secret</span><p>visible</p></div>",
        ));
        assert!(!d.contains("secret"), "{d}");
        assert!(d.contains("visible"), "{d}");
    }

    #[test]
    fn table_cells_and_bg() {
        let d = dump(&parse(
            "<table><tr><th>Head</th></tr>\
             <tr><td bgcolor=\"#ff0000\">A</td><td>B</td></tr></table>",
        ));
        assert!(d.contains("TABLE(2x"), "{d}");
        assert!(d.contains("h=true"), "{d}");
        assert!(d.contains("bg=#ff0000"), "{d}");
    }

    #[test]
    fn nested_quotes() {
        let d = dump(&parse(
            "<blockquote>outer<blockquote>inner</blockquote></blockquote>",
        ));
        assert_eq!(d.matches("QUOTE:").count(), 2, "{d}");
        assert!(d.contains("inner"), "{d}");
    }

    #[test]
    fn links_flagged() {
        let d = dump(&parse(
            "<p><a href=\"https://example.com/x\">ok</a> \
             <a href=\"javascript:alert(1)\">bad</a></p>",
        ));
        assert!(
            d.contains("[link safe=true:ok -> https://example.com/x]"),
            "{d}"
        );
        assert!(
            d.contains("[link safe=false:bad -> javascript:alert(1)]"),
            "{d}"
        );
    }

    #[test]
    fn pre_keeps_lines() {
        let d = dump(&parse("<pre>line1\n  indented\nline3</pre>"));
        assert!(d.contains("|line1") && d.contains("|  indented"), "{d}");
    }

    #[test]
    fn base64_roundtrip() {
        let bytes = decode_base64("SGVsbG8=").unwrap();
        assert_eq!(bytes, b"Hello");
        assert!(decode_base64("!!!").is_none());
    }

    #[test]
    fn image_alt_kept() {
        let d = dump(&parse(
            "<p>see <img src=\"data:image/png;base64,AAAA\" alt=\"logo\"></p>",
        ));
        assert!(d.contains("IMG(data"), "{d}");
    }

    #[test]
    fn utf8_text_survives() {
        // No byte-as-char mojibake: multi-byte text round-trips intact.
        let d = dump(&parse("<p>Grüße &amp; 你好</p>"));
        assert!(d.contains("Grüße & 你好"), "{d}");
    }
}
