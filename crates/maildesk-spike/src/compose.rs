//! Spike B: minimal egui composer document (throwaway).
//!
//! Model: the body is `Vec<EditBlock>`; a block is a kind
//! (paragraph / bullet / quote) plus formatting runs. A per-block `TextEdit`
//! shows the concatenated run text; typing is synced back by prefix/suffix
//! diffing so marks survive edits. Selection-scoped toggles split runs at
//! the selection boundaries (the `execCommand` semantics of `EditorFrame`).
//! HTML out uses only tags the outgoing sanitizer keeps
//! (`<b>/<i>/<u>`, `<code>`, `<a>`, `<p>`, `<ul>/<li>`, `<blockquote>`).

use mailcore::html::escape_text;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Marks {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub code: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditRun {
    pub text: String,
    pub marks: Marks,
    pub link: Option<String>,
}

impl EditRun {
    pub fn plain(text: &str) -> Self {
        Self {
            text: text.to_string(),
            marks: Marks::default(),
            link: None,
        }
    }

    fn len_chars(&self) -> usize {
        self.text.chars().count()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Para,
    Bullet,
    Quote,
}

#[derive(Debug, Clone)]
pub struct EditBlock {
    pub kind: BlockKind,
    pub runs: Vec<EditRun>,
}

impl EditBlock {
    pub fn plain_text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }
}

#[derive(Debug, Clone, Default)]
pub struct EditDoc {
    pub blocks: Vec<EditBlock>,
}

impl EditDoc {
    pub fn empty() -> Self {
        Self {
            blocks: vec![EditBlock {
                kind: BlockKind::Para,
                runs: Vec::new(),
            }],
        }
    }

    /// Char offset → `(run_index, offset_in_run)`.
    fn locate(&self, block: usize, char_at: usize) -> (usize, usize) {
        let mut acc = 0;
        for (i, run) in self.blocks[block].runs.iter().enumerate() {
            let len = run.len_chars();
            if char_at <= acc + len {
                return (i, char_at - acc);
            }
            acc += len;
        }
        let n = self.blocks[block].runs.len();
        (n.saturating_sub(1), usize::MAX)
    }

    /// Split the run containing `char_at` so a boundary lands there.
    fn split_at(&mut self, block: usize, char_at: usize) {
        let total: usize = self.blocks[block].runs.iter().map(|r| r.len_chars()).sum();
        if char_at == 0 || char_at >= total {
            return;
        }
        let (i, off) = self.locate(block, char_at);
        if off == 0 {
            return;
        }
        let mut tail = self.blocks[block].runs[i].clone();
        let head_text: String = tail.text.chars().take(off).collect();
        let tail_text: String = tail.text.chars().skip(off).collect();
        self.blocks[block].runs[i].text = head_text;
        tail.text = tail_text;
        self.blocks[block].runs.insert(i + 1, tail);
    }

    /// Toggle one mark over `[start, end)` (char offsets in the block's
    /// plain text). All-set → clear, else set — `execCommand` semantics.
    pub fn toggle_mark(&mut self, block: usize, start: usize, end: usize, mark: Mark) {
        if block >= self.blocks.len() || start >= end {
            return;
        }
        self.split_at(block, end);
        self.split_at(block, start);
        let covered: Vec<usize> = {
            let mut acc = 0;
            let mut out = Vec::new();
            for (i, run) in self.blocks[block].runs.iter().enumerate() {
                let len = run.len_chars();
                if acc >= start && acc + len <= end && len > 0 {
                    out.push(i);
                }
                acc += len;
            }
            out
        };
        if covered.is_empty() {
            return;
        }
        let all_set = covered
            .iter()
            .all(|&i| get_mark(&self.blocks[block].runs[i].marks, mark));
        for i in covered {
            set_mark(&mut self.blocks[block].runs[i].marks, mark, !all_set);
        }
        self.merge_runs(block);
    }

    /// Set (or clear with `None`) a link over the range.
    pub fn set_link(&mut self, block: usize, start: usize, end: usize, href: Option<String>) {
        if block >= self.blocks.len() || start >= end {
            return;
        }
        self.split_at(block, end);
        self.split_at(block, start);
        let mut acc = 0;
        for run in &mut self.blocks[block].runs {
            let len = run.len_chars();
            if acc >= start && acc + len <= end && len > 0 {
                run.link = href.clone();
            }
            acc += len;
        }
        self.merge_runs(block);
    }

    /// Clear all marks + links over the range.
    pub fn clear_range(&mut self, block: usize, start: usize, end: usize) {
        if block >= self.blocks.len() || start >= end {
            return;
        }
        self.split_at(block, end);
        self.split_at(block, start);
        let mut acc = 0;
        for run in &mut self.blocks[block].runs {
            let len = run.len_chars();
            if acc >= start && acc + len <= end && len > 0 {
                run.marks = Marks::default();
                run.link = None;
            }
            acc += len;
        }
        self.merge_runs(block);
    }

    fn merge_runs(&mut self, block: usize) {
        let mut merged: Vec<EditRun> = Vec::new();
        for run in std::mem::take(&mut self.blocks[block].runs) {
            if run.text.is_empty() {
                continue;
            }
            if let Some(last) = merged.last_mut() {
                if last.marks == run.marks && last.link == run.link {
                    last.text.push_str(&run.text);
                    continue;
                }
            }
            merged.push(run);
        }
        self.blocks[block].runs = merged;
    }

    /// Sync back text typed in the block's `TextEdit`: prefix/suffix diff
    /// against the old plain text, applied to the runs so marks survive.
    pub fn apply_text_edit(&mut self, block: usize, new_text: &str) {
        if block >= self.blocks.len() {
            return;
        }
        let old: Vec<char> = self.blocks[block].plain_text().chars().collect();
        let new: Vec<char> = new_text.chars().collect();
        let mut pre = 0;
        while pre < old.len() && pre < new.len() && old[pre] == new[pre] {
            pre += 1;
        }
        let mut suf = 0;
        while suf < old.len() - pre
            && suf < new.len() - pre
            && old[old.len() - 1 - suf] == new[new.len() - 1 - suf]
        {
            suf += 1;
        }
        if pre + suf >= old.len() && pre + suf >= new.len() {
            return; // identical
        }
        // Char-level splice, then regroup: obviously correct, and blocks
        // are small (one paragraph each).
        let del_end = old.len() - suf;
        let mut chars: Vec<(char, Marks, Option<String>)> = Vec::with_capacity(new.len());
        for run in &self.blocks[block].runs {
            for c in run.text.chars() {
                chars.push((c, run.marks, run.link.clone()));
            }
        }
        let style_at = |i: usize| {
            chars
                .get(i)
                .map(|(_, m, l)| (*m, l.clone()))
                .or_else(|| chars.last().map(|(_, m, l)| (*m, l.clone())))
                .unwrap_or((Marks::default(), None))
        };
        let (ins_marks, ins_link) = style_at(pre.min(chars.len().saturating_sub(1)));
        let mut next: Vec<(char, Marks, Option<String>)> = Vec::with_capacity(new.len());
        next.extend(chars[..pre.min(chars.len())].iter().cloned());
        for c in &new[pre..new.len() - suf] {
            // Newline chars never reach here (blocks are single-line);
            // keep them out rather than smuggling line breaks into a run.
            if *c != '\n' {
                next.push((*c, ins_marks, ins_link.clone()));
            }
        }
        next.extend(chars[del_end.min(chars.len())..].iter().cloned());
        let mut runs: Vec<EditRun> = Vec::new();
        for (c, m, l) in next {
            if let Some(last) = runs.last_mut() {
                let last: &mut EditRun = last;
                if last.marks == m && last.link == l {
                    last.text.push(c);
                    continue;
                }
            }
            runs.push(EditRun {
                text: c.to_string(),
                marks: m,
                link: l,
            });
        }
        if runs.is_empty() {
            runs.push(EditRun::plain(""));
        }
        self.blocks[block].runs = runs;
    }

    /// Split the block at `char_at` (Enter): two blocks of the same kind.
    /// Returns the cursor char offset for the new block (always 0).
    pub fn split_block(&mut self, block: usize, char_at: usize) {
        if block >= self.blocks.len() {
            return;
        }
        self.split_at(block, char_at);
        let mut acc = 0;
        let mut at = 0;
        for (i, run) in self.blocks[block].runs.iter().enumerate() {
            if acc >= char_at {
                at = i;
                break;
            }
            acc += run.len_chars();
            at = i + 1;
        }
        let tail: Vec<EditRun> = {
            let runs = &mut self.blocks[block].runs;
            runs.split_off(at.min(runs.len()))
        };
        let kind = self.blocks[block].kind;
        self.blocks.insert(
            block + 1,
            EditBlock {
                kind,
                runs: if tail.is_empty() {
                    vec![EditRun::plain("")]
                } else {
                    tail
                },
            },
        );
        if self.blocks[block].runs.is_empty() {
            self.blocks[block].runs.push(EditRun::plain(""));
        }
    }

    /// Backspace at offset 0 merges into the previous block. Returns the
    /// cursor char offset in the merged block.
    pub fn merge_into_previous(&mut self, block: usize) -> Option<usize> {
        if block == 0 || block >= self.blocks.len() {
            return None;
        }
        let prev_len: usize = self.blocks[block - 1]
            .runs
            .iter()
            .map(|r| r.len_chars())
            .sum();
        let mut tail = std::mem::take(&mut self.blocks[block].runs);
        self.blocks[block - 1].runs.append(&mut tail);
        self.blocks.remove(block);
        self.merge_runs(block - 1);
        Some(prev_len)
    }

    pub fn set_kind(&mut self, block: usize, kind: BlockKind) {
        if let Some(b) = self.blocks.get_mut(block) {
            b.kind = kind;
        }
    }

    /// Marks at a caret (no selection): the run under the cursor, or the
    /// neighbour's when between runs.
    pub fn marks_at(&self, block: usize, char_at: usize) -> (Marks, Option<String>) {
        let Some(b) = self.blocks.get(block) else {
            return (Marks::default(), None);
        };
        let mut acc = 0;
        for run in &b.runs {
            let len = run.len_chars();
            if char_at >= acc && char_at <= acc + len {
                return (run.marks, run.link.clone());
            }
            acc += len;
        }
        (Marks::default(), None)
    }

    /// Serialize to sendable HTML (tags the outgoing sanitizer keeps).
    pub fn to_html(&self) -> String {
        let mut out = String::new();
        let mut list_open = false;
        for block in &self.blocks {
            let inner: String = block.runs.iter().map(run_html).collect();
            let inner = if inner.trim().is_empty() {
                "<br>".to_string()
            } else {
                inner
            };
            match block.kind {
                BlockKind::Para => {
                    if list_open {
                        out.push_str("</ul>");
                        list_open = false;
                    }
                    out.push_str(&format!("<p>{inner}</p>"));
                }
                BlockKind::Bullet => {
                    if !list_open {
                        out.push_str("<ul>");
                        list_open = true;
                    }
                    out.push_str(&format!("<li>{inner}</li>"));
                }
                BlockKind::Quote => {
                    if list_open {
                        out.push_str("</ul>");
                        list_open = false;
                    }
                    out.push_str(&format!("<blockquote><p>{inner}</p></blockquote>"));
                }
            }
        }
        if list_open {
            out.push_str("</ul>");
        }
        out
    }

    /// Parse editor HTML back (source view round-trip, reply prefills).
    /// Lossy where the model is lossy (tables flatten, images → alt text).
    pub fn from_html(html: &str) -> Self {
        let mut doc = Self { blocks: Vec::new() };
        for block in crate::render::parse(html) {
            match block {
                crate::render::Block::Para { inlines, .. } => doc.blocks.push(EditBlock {
                    kind: BlockKind::Para,
                    runs: inlines_to_runs(&inlines),
                }),
                crate::render::Block::Heading { inlines, .. } => doc.blocks.push(EditBlock {
                    kind: BlockKind::Para,
                    runs: inlines_to_runs(&inlines),
                }),
                crate::render::Block::Quote(inner) => {
                    for b in flatten_to_paras(inner) {
                        doc.blocks.push(EditBlock {
                            kind: BlockKind::Quote,
                            runs: b,
                        });
                    }
                }
                crate::render::Block::List { items, .. } => {
                    for item in items {
                        let mut runs = Vec::new();
                        for line in flatten_to_paras(item) {
                            runs.extend(line);
                        }
                        if runs.is_empty() {
                            runs.push(EditRun::plain(""));
                        }
                        doc.blocks.push(EditBlock {
                            kind: BlockKind::Bullet,
                            runs,
                        });
                    }
                }
                crate::render::Block::Code(text) => doc.blocks.push(EditBlock {
                    kind: BlockKind::Para,
                    runs: text
                        .lines()
                        .map(|l| EditRun {
                            text: l.to_string(),
                            marks: Marks {
                                code: true,
                                ..Default::default()
                            },
                            link: None,
                        })
                        .collect(),
                }),
                crate::render::Block::Table(rows) => {
                    for row in rows {
                        for cell in row.cells {
                            for line in flatten_to_paras(cell.blocks) {
                                doc.blocks.push(EditBlock {
                                    kind: BlockKind::Para,
                                    runs: line,
                                });
                            }
                        }
                    }
                }
                crate::render::Block::Image { alt, .. } => doc.blocks.push(EditBlock {
                    kind: BlockKind::Para,
                    runs: vec![EditRun::plain(&format!("[image: {alt}]"))],
                }),
                crate::render::Block::Hr => doc.blocks.push(EditBlock {
                    kind: BlockKind::Para,
                    runs: vec![EditRun::plain("---")],
                }),
            }
        }
        if doc.blocks.is_empty() {
            return Self::empty();
        }
        for b in &mut doc.blocks {
            if b.runs.is_empty() {
                b.runs.push(EditRun::plain(""));
            }
        }
        doc
    }
}

fn inlines_to_runs(inlines: &[crate::render::Inline]) -> Vec<EditRun> {
    use crate::render::Inline;
    let mut runs = Vec::new();
    for il in inlines {
        match il {
            Inline::Text(t, m, _) => {
                // render::Marks has more fields; map what the editor keeps.
                let marks = Marks {
                    bold: m.bold,
                    italic: m.italic,
                    underline: m.underline,
                    code: m.code,
                };
                runs.push(EditRun {
                    text: t.clone(),
                    marks,
                    link: None,
                });
            }
            Inline::Link { href, label, .. } => runs.push(EditRun {
                text: label.clone(),
                marks: Marks::default(),
                link: Some(href.clone()),
            }),
            Inline::Break => runs.push(EditRun::plain(" ")),
        }
    }
    if runs.is_empty() {
        runs.push(EditRun::plain(""));
    }
    runs
}

fn flatten_to_paras(blocks: Vec<crate::render::Block>) -> Vec<Vec<EditRun>> {
    let mut out = Vec::new();
    for b in blocks {
        match b {
            crate::render::Block::Para { inlines, .. }
            | crate::render::Block::Heading { inlines, .. } => out.push(inlines_to_runs(&inlines)),
            crate::render::Block::Quote(inner) => out.extend(flatten_to_paras(inner)),
            crate::render::Block::List { items, .. } => {
                for item in items {
                    out.extend(flatten_to_paras(item));
                }
            }
            crate::render::Block::Code(text) => {
                for line in text.lines() {
                    out.push(vec![EditRun {
                        text: line.to_string(),
                        marks: Marks {
                            code: true,
                            ..Default::default()
                        },
                        link: None,
                    }]);
                }
            }
            crate::render::Block::Table(rows) => {
                for row in rows {
                    for cell in row.cells {
                        out.extend(flatten_to_paras(cell.blocks));
                    }
                }
            }
            crate::render::Block::Image { alt, .. } => {
                out.push(vec![EditRun::plain(&format!("[image: {alt}]"))]);
            }
            crate::render::Block::Hr => out.push(vec![EditRun::plain("---")]),
        }
    }
    out
}

fn run_html(run: &EditRun) -> String {
    let mut t = escape_text(&run.text);
    if run.marks.code {
        t = format!("<code>{t}</code>");
    }
    if run.marks.bold {
        t = format!("<b>{t}</b>");
    }
    if run.marks.italic {
        t = format!("<i>{t}</i>");
    }
    if run.marks.underline {
        t = format!("<u>{t}</u>");
    }
    if let Some(href) = &run.link {
        // Attribute-escape the URL (core's escape_attr is crate-private).
        let h = escape_text(href).replace('"', "&quot;");
        t = format!("<a href=\"{h}\">{t}</a>");
    }
    t
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Bold,
    Italic,
    Underline,
}

fn get_mark(m: &Marks, mark: Mark) -> bool {
    match mark {
        Mark::Bold => m.bold,
        Mark::Italic => m.italic,
        Mark::Underline => m.underline,
    }
}

fn set_mark(m: &mut Marks, mark: Mark, v: bool) {
    match mark {
        Mark::Bold => m.bold = v,
        Mark::Italic => m.italic = v,
        Mark::Underline => m.underline = v,
    }
}

// ---------------------------------------------------------------------------
// Tests (colocated, per AGENTS.md §3)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> EditDoc {
        EditDoc {
            blocks: vec![EditBlock {
                kind: BlockKind::Para,
                runs: vec![EditRun::plain(text)],
            }],
        }
    }

    #[test]
    fn toggle_splits_and_merges() {
        let mut d = doc("hello world");
        d.toggle_mark(0, 0, 5, Mark::Bold);
        assert_eq!(d.blocks[0].runs.len(), 2);
        assert!(d.blocks[0].runs[0].marks.bold);
        assert!(!d.blocks[0].runs[1].marks.bold);
        // Toggle again clears.
        d.toggle_mark(0, 0, 5, Mark::Bold);
        assert_eq!(d.blocks[0].runs.len(), 1);
        assert!(!d.blocks[0].runs[0].marks.bold);
    }

    #[test]
    fn typing_keeps_marks() {
        let mut d = doc("hello");
        d.toggle_mark(0, 0, 5, Mark::Bold);
        d.apply_text_edit(0, "hello!");
        assert!(d.blocks[0].runs[0].marks.bold);
        assert_eq!(d.blocks[0].plain_text(), "hello!");
        // Typing inside keeps the run's style; the diff is prefix/suffix.
        d.apply_text_edit(0, "hellXo!");
        assert_eq!(d.blocks[0].plain_text(), "hellXo!");
        assert!(d.blocks[0].runs.iter().all(|r| r.marks.bold));
    }

    #[test]
    fn delete_across_runs_merges() {
        let mut d = doc("hello world");
        d.toggle_mark(0, 0, 5, Mark::Bold);
        d.apply_text_edit(0, "held");
        assert_eq!(d.blocks[0].plain_text(), "held");
    }

    #[test]
    fn split_and_merge_blocks() {
        let mut d = doc("hello world");
        d.split_block(0, 5);
        assert_eq!(d.blocks.len(), 2);
        assert_eq!(d.blocks[0].plain_text(), "hello");
        assert_eq!(d.blocks[1].plain_text(), " world");
        let at = d.merge_into_previous(1).unwrap();
        assert_eq!(at, 5);
        assert_eq!(d.blocks.len(), 1);
        assert_eq!(d.blocks[0].plain_text(), "hello world");
    }

    #[test]
    fn html_roundtrip() {
        let mut d = doc("hello world");
        d.toggle_mark(0, 0, 5, Mark::Bold);
        d.set_link(0, 6, 11, Some("https://example.com".to_string()));
        let html = d.to_html();
        assert!(html.contains("<b>hello</b>"), "{html}");
        assert!(
            html.contains("<a href=\"https://example.com\">world</a>"),
            "{html}"
        );
        // And back: marks + link survive the parse.
        let back = EditDoc::from_html(&html);
        assert_eq!(back.blocks.len(), 1);
        assert!(back.blocks[0].runs[0].marks.bold);
        assert_eq!(
            back.blocks[0].runs[1].link.as_deref(),
            Some("https://example.com")
        );
    }

    #[test]
    fn quote_and_bullet_serialize() {
        let mut d = EditDoc::empty();
        d.blocks[0].runs = vec![EditRun::plain("item")];
        d.set_kind(0, BlockKind::Bullet);
        d.blocks.push(EditBlock {
            kind: BlockKind::Quote,
            runs: vec![EditRun::plain("quoted")],
        });
        let html = d.to_html();
        assert!(html.contains("<ul><li>item</li></ul>"), "{html}");
        assert!(
            html.contains("<blockquote><p>quoted</p></blockquote>"),
            "{html}"
        );
    }
}
