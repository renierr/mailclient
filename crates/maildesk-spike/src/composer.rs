//! Spike B: composer view state + UI (throwaway).
//!
//! Exercises the real `mailcore::compose` API offline: `blank_draft` /
//! `answer_draft` prefill, `ComposeForm` validation, `send_format_note`,
//! `resolve_bodies` and `format_draft` (the exact MIME a send would submit).
//! Send and server-draft save need the network and are NOT attempted: the
//! buttons say so out loud instead of failing obscurely.

use mailcore::compose::editor::send_format_note;
use mailcore::compose::{AnswerDraft, ComposeForm, Receipts};
use mailcore::models::Account;
use mailcore::sync::sender::{format_draft, SendFormat, SendPolicy};

use crate::compose::{BlockKind, EditDoc, Mark};
use crate::paint::{self, Colors};

pub struct ComposerState {
    pub to: String,
    pub subject: String,
    pub doc: EditDoc,
    pub notice: String,
    pub source_mode: bool,
    pub source_text: String,
    pub link_url: String,
    pub format: SendFormat,
    pub focused_block: Option<usize>,
    pub message: Option<String>,
    pub mime_preview: Option<String>,
    pub show_preview: bool,
    pub dirty: bool,
}

impl ComposerState {
    pub fn blank() -> Self {
        Self {
            to: String::new(),
            subject: String::new(),
            doc: EditDoc::empty(),
            notice: String::new(),
            source_mode: false,
            source_text: String::new(),
            link_url: String::new(),
            format: SendFormat::Auto,
            focused_block: None,
            message: None,
            mime_preview: None,
            show_preview: false,
            dirty: false,
        }
    }

    pub fn from_answer(draft: &AnswerDraft) -> Self {
        let mut s = Self::blank();
        s.to = draft.to.clone();
        s.subject = draft.subject.clone();
        s.doc = EditDoc::from_html(&draft.body_html);
        s.notice = draft.notice.clone();
        s
    }

    /// Scripted demo content for `--shot-compose`: exercises bold, link,
    /// bullet, quote and the MIME path without keystrokes.
    pub fn preseed_demo(&mut self, account: &Account) {
        self.to = "bob@example.com".to_string();
        self.subject = "Spike B demo".to_string();
        self.doc = EditDoc::empty();
        self.doc.blocks[0].runs = vec![crate::compose::EditRun {
            text: "Hello Bob, some bold and a link.".to_string(),
            marks: crate::compose::Marks::default(),
            link: None,
        }];
        // "bold" is chars 17..21.
        self.doc.toggle_mark(0, 17, 21, Mark::Bold);
        // "a link" is chars 26..32.
        self.doc
            .set_link(0, 26, 32, Some("https://example.com".to_string()));
        self.doc.blocks.push(crate::compose::EditBlock {
            kind: crate::compose::BlockKind::Bullet,
            runs: vec![crate::compose::EditRun {
                text: "first item".to_string(),
                marks: crate::compose::Marks::default(),
                link: None,
            }],
        });
        self.doc.blocks.push(crate::compose::EditBlock {
            kind: crate::compose::BlockKind::Quote,
            runs: vec![crate::compose::EditRun {
                text: "Onwards!".to_string(),
                marks: crate::compose::Marks::default(),
                link: None,
            }],
        });
        self.validate();
        self.build_mime(account);
        self.dirty = true;
    }

    fn html(&self) -> String {
        if self.source_mode {
            self.source_text.clone()
        } else {
            self.doc.to_html()
        }
    }

    fn form(&self) -> ComposeForm {
        ComposeForm {
            to: split_addrs(&self.to),
            cc: Vec::new(),
            bcc: Vec::new(),
            from: String::new(),
            from_name: String::new(),
            reply_to: String::new(),
            subject: self.subject.clone(),
            body: self.html(),
            body_html: self.html(),
            attachments: Vec::new(),
            draft_uid: -1,
            request_mdn: None,
            request_dsn: None,
        }
    }

    fn validate(&mut self) {
        let form = self.form();
        match form.require_recipient() {
            Ok(()) => {
                let note = send_format_note(self.format.as_str(), &self.html());
                self.message = Some(format!("valid — {note}"));
            }
            Err(e) => self.message = Some(format!("not ready: {e}")),
        }
    }

    fn build_mime(&mut self, account: &Account) {
        let form = self.form();
        if let Err(e) = form.require_recipient() {
            self.message = Some(format!("not ready: {e}"));
            return;
        }
        let policy = SendPolicy::Unrestricted;
        let req = form.as_request(account, self.format, true, Receipts::default(), &policy);
        match format_draft(account, &req) {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes).to_string();
                let lines: Vec<&str> = text.lines().take(30).collect();
                self.mime_preview = Some(lines.join("\n"));
                self.message = Some(format!(
                    "MIME built: {} bytes (not sent — offline)",
                    bytes.len()
                ));
            }
            Err(e) => self.message = Some(format!("MIME failed: {e}")),
        }
        self.show_preview = true;
    }

    /// Selection (or caret) block + sorted char range for toolbar ops.
    fn target(&self, ctx: &egui::Context) -> Option<(usize, usize, usize)> {
        let fb = self.focused_block?;
        if fb >= self.doc.blocks.len() {
            return None;
        }
        let id = egui::Id::new(("cedit", fb));
        let range = egui::text_edit::TextEditState::load(ctx, id)?
            .cursor
            .char_range()?;
        let (a, b) = (range.primary.index, range.secondary.index);
        Some((fb, a.min(b).into(), a.max(b).into()))
    }

    fn toggle(&mut self, ctx: &egui::Context, mark: Mark) {
        match self.target(ctx) {
            Some((b, s, e)) if s < e => {
                self.doc.toggle_mark(b, s, e, mark);
                self.dirty = true;
            }
            _ => self.message = Some("select text first, then the mark".to_string()),
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        account: &Account,
        paint: &mut paint::PaintState,
    ) -> bool {
        // Returns true when the user closes the composer.
        let mut close = false;
        ui.heading("Compose (spike — nothing is sent)");
        if !self.notice.is_empty() {
            ui.colored_label(egui::Color32::DARK_RED, &self.notice);
        }
        ui.horizontal(|ui| {
            ui.label("To");
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.to)
                    .desired_width(f32::INFINITY)
                    .hint_text("name@example.com, …"),
            );
            if r.changed() {
                self.dirty = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("Subject");
            let r =
                ui.add(egui::TextEdit::singleline(&mut self.subject).desired_width(f32::INFINITY));
            if r.changed() {
                self.dirty = true;
            }
        });
        ui.separator();

        // --- toolbar (ComposerToolbar.qml ops) ---
        ui.horizontal_wrapped(|ui| {
            self.mark_button(ui, ctx, "B", Mark::Bold, "Bold");
            self.mark_button(ui, ctx, "I", Mark::Italic, "Italic");
            self.mark_button(ui, ctx, "U", Mark::Underline, "Underline");
            ui.separator();
            if ui.button("list").clicked() {
                self.cycle_kind();
            }
            if ui.button("quote").clicked() {
                self.set_kind(BlockKind::Quote);
            }
            ui.separator();
            let link_resp = ui.add(
                egui::TextEdit::singleline(&mut self.link_url)
                    .desired_width(160.0)
                    .hint_text("https://… (select text first)"),
            );
            let _ = link_resp;
            if ui.button("link").clicked() {
                self.apply_link(ctx);
            }
            if ui.button("unlink").clicked() {
                self.clear_link(ctx);
            }
            if ui.button("clear").clicked() {
                self.clear_format(ctx);
            }
            ui.separator();
            if ui
                .button(if self.source_mode { "rich" } else { "source" })
                .clicked()
            {
                self.toggle_source();
            }
            egui::ComboBox::from_id_salt("sendfmt")
                .selected_text(self.format.as_str())
                .show_ui(ui, |ui| {
                    for f in [
                        SendFormat::Auto,
                        SendFormat::Plain,
                        SendFormat::Html,
                        SendFormat::Multipart,
                    ] {
                        ui.selectable_value(&mut self.format, f, f.as_str());
                    }
                });
        });
        ui.separator();

        // --- body ---
        if self.source_mode {
            let r = ui.add(
                egui::TextEdit::multiline(&mut self.source_text)
                    .desired_width(f32::INFINITY)
                    .desired_rows(12)
                    .font(egui::TextStyle::Monospace),
            );
            if r.changed() {
                self.dirty = true;
            }
        } else {
            let n = self.doc.blocks.len();
            let mut split_at: Option<(usize, usize)> = None;
            let mut merge_at: Option<usize> = None;
            for i in 0..n {
                let id = egui::Id::new(("cedit", i));
                let prefix = match self.doc.blocks[i].kind {
                    BlockKind::Para => "",
                    BlockKind::Bullet => "• ",
                    BlockKind::Quote => "❚ ",
                };
                ui.horizontal_top(|ui| {
                    if !prefix.is_empty() {
                        ui.label(prefix);
                    }
                    let mut text = self.doc.blocks[i].plain_text();
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut text)
                            .id(id)
                            .desired_width(f32::INFINITY),
                    );
                    if resp.changed() {
                        self.doc.apply_text_edit(i, &text);
                        self.dirty = true;
                    }
                    if resp.has_focus() {
                        self.focused_block = Some(i);
                        if ctx.input(|st| st.key_pressed(egui::Key::Enter)) {
                            let cur = cursor_char(ctx, id).unwrap_or(0);
                            split_at = Some((i, cur));
                        }
                        if ctx.input(|st| st.key_pressed(egui::Key::Backspace))
                            && cursor_char(ctx, id).unwrap_or(1) == 0
                        {
                            merge_at = Some(i);
                        }
                    }
                });
            }
            if let Some((i, cur)) = split_at {
                self.doc.split_block(i, cur);
                self.focused_block = Some(i + 1);
                set_cursor(ctx, egui::Id::new(("cedit", i + 1)), 0);
                self.dirty = true;
            }
            if let Some(i) = merge_at {
                if let Some(at) = self.doc.merge_into_previous(i) {
                    self.focused_block = Some(i - 1);
                    set_cursor(ctx, egui::Id::new(("cedit", i - 1)), at);
                    self.dirty = true;
                }
            }
        }
        ui.separator();

        // --- format note + actions (resolve_bodies path in core) ---
        let note = send_format_note(self.format.as_str(), &self.html());
        ui.horizontal(|ui| {
            ui.label(format!("sends as: {note}"));
            if self.dirty {
                ui.weak("unsaved changes");
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("validate").clicked() {
                self.validate();
            }
            if ui.button("MIME preview").clicked() {
                self.build_mime(account);
            }
            ui.weak("send + server-draft save need the network — not attempted offline");
            if ui.button("close").clicked() {
                close = true;
            }
        });
        if let Some(m) = &self.message {
            ui.label(m);
        }
        if self.show_preview {
            ui.collapsing("MIME preview (what a send would submit)", |ui| {
                egui::ScrollArea::vertical()
                    .max_height(220.0)
                    .show(ui, |ui| {
                        ui.monospace(self.mime_preview.as_deref().unwrap_or("(build it first)"));
                    });
            });
            ui.collapsing("rendered preview (Spike A painter)", |ui| {
                let colors = if ui.visuals().dark_mode {
                    Colors::from_palette(&mailcore::html::reader::palette(
                        mailcore::html::reader::Paint::Darkened,
                        &crate::theme_palette(true),
                    ))
                } else {
                    Colors::themed(ui)
                };
                let blocks = crate::render::parse(&self.html());
                paint::paint_blocks(ui, &blocks, &colors, paint);
            });
        }
        close
    }

    fn mark_button(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        label: &str,
        mark: Mark,
        tip: &str,
    ) {
        // Active state from the caret/selection (replaces QML's 200ms poll).
        let active = self.target(ctx).is_some_and(|(b, s, e)| {
            let (m, _) = self.doc.marks_at(b, s.min(e));
            match mark {
                Mark::Bold => m.bold,
                Mark::Italic => m.italic,
                Mark::Underline => m.underline,
            }
        });
        let resp = ui.selectable_label(active, label).on_hover_text(tip);
        if resp.clicked() {
            self.toggle(ctx, mark);
        }
    }

    fn cycle_kind(&mut self) {
        let fb = self.focused_block.unwrap_or(0);
        if fb >= self.doc.blocks.len() {
            return;
        }
        let next = match self.doc.blocks[fb].kind {
            BlockKind::Para => BlockKind::Bullet,
            BlockKind::Bullet => BlockKind::Para,
            BlockKind::Quote => BlockKind::Para,
        };
        self.doc.set_kind(fb, next);
        self.dirty = true;
    }

    fn set_kind(&mut self, kind: BlockKind) {
        let fb = self.focused_block.unwrap_or(0);
        if fb < self.doc.blocks.len() {
            let cur = self.doc.blocks[fb].kind;
            self.doc
                .set_kind(fb, if cur == kind { BlockKind::Para } else { kind });
            self.dirty = true;
        }
    }

    fn apply_link(&mut self, ctx: &egui::Context) {
        let url = self.link_url.trim().to_string();
        if url.is_empty() {
            self.message = Some("type a URL first".to_string());
            return;
        }
        match self.target(ctx) {
            Some((b, s, e)) if s < e => {
                self.doc.set_link(b, s, e, Some(url));
                self.dirty = true;
            }
            _ => self.message = Some("select text first to turn it into a link".to_string()),
        }
    }

    fn clear_link(&mut self, ctx: &egui::Context) {
        match self.target(ctx) {
            Some((b, s, e)) if s < e => {
                self.doc.set_link(b, s, e, None);
                self.dirty = true;
            }
            _ => self.message = Some("select a link first".to_string()),
        }
    }

    fn clear_format(&mut self, ctx: &egui::Context) {
        match self.target(ctx) {
            Some((b, s, e)) if s < e => {
                self.doc.clear_range(b, s, e);
                self.dirty = true;
            }
            _ => self.message = Some("select text first".to_string()),
        }
    }

    fn toggle_source(&mut self) {
        if self.source_mode {
            self.doc = EditDoc::from_html(&self.source_text);
            self.source_mode = false;
        } else {
            self.source_text = self.doc.to_html();
            self.source_mode = true;
        }
    }
}

fn split_addrs(s: &str) -> Vec<String> {
    s.split([',', ';'])
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

fn cursor_char(ctx: &egui::Context, id: egui::Id) -> Option<usize> {
    egui::text_edit::TextEditState::load(ctx, id)?
        .cursor
        .char_range()
        .map(|r| r.primary.index.into())
}

fn set_cursor(ctx: &egui::Context, id: egui::Id, index: usize) {
    if let Some(mut st) = egui::text_edit::TextEditState::load(ctx, id) {
        st.cursor.set_char_range(Some(egui::text::CCursorRange::one(
            egui::text::CCursor::new(index),
        )));
        st.store(ctx, id);
    }
}

// ---------------------------------------------------------------------------
// Tests: the Spike B contract (colocated, per AGENTS.md §3)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::*;

    fn test_account() -> Account {
        Account {
            id: -1,
            name: "Spike".to_string(),
            email_address: "spike@example.com".to_string(),
            from_name: String::new(),
            imap_host: "imap.example.com".to_string(),
            imap_port: 993,
            imap_security: "tls".to_string(),
            imap_username: "spike@example.com".to_string(),
            smtp_host: "smtp.example.com".to_string(),
            smtp_port: 465,
            smtp_security: "tls".to_string(),
            smtp_username: "spike@example.com".to_string(),
            auth_vault_key: "vault-spike".to_string(),
            check_interval_secs: 300,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    /// The whole Spike B promise: what an egui editor builds must survive
    /// `resolve_bodies` → `format_draft` into correct MIME.
    ///
    /// NB: `format_draft` is the **draft-save** path — core deliberately
    /// keeps it multipart/alternative always, so a saved draft never loses
    /// its rich text. The send path is `compose::send::{prepare_send,
    /// deliver}`, which resolves the *user's* format preference via
    /// `effective_format` + `needs_html_formatting` and needs the network.
    /// Both are covered here: the MIME shape offline, the format decision
    /// by the same calls the bridge makes.
    #[test]
    fn editor_output_becomes_sendable_mime() {
        let db = mailcore::db::Db::open_in_memory().unwrap();
        let _ = db; // validation/settings are read-only where needed
        let mut c = ComposerState::blank();
        c.to = "bob@example.com".to_string();
        c.subject = "Spike B".to_string();
        // Body with a bold run, a link, a bullet and a quote — the ops the
        // toolbar exposes.
        c.doc.blocks[0].runs = vec![EditRun {
            text: "Hello Bob, bold here.".to_string(),
            marks: Marks::default(),
            link: None,
        }];
        c.doc.toggle_mark(0, 11, 15, Mark::Bold);
        c.doc
            .set_link(0, 0, 5, Some("https://example.com".to_string()));
        c.doc.blocks.push(EditBlock {
            kind: BlockKind::Bullet,
            runs: vec![EditRun::plain("a bullet")],
        });
        c.doc.blocks.push(EditBlock {
            kind: BlockKind::Quote,
            runs: vec![EditRun::plain("a quote")],
        });

        let form = c.form();
        form.require_recipient().expect("To is set");
        let account = test_account();
        let policy = SendPolicy::Unrestricted;
        // 1. The outgoing sanitized HTML (what the MIME part will hold).
        let (plain, html) = mailcore::sync::sender::resolve_bodies(
            &form.body,
            Some(&form.body_html),
            mailcore::sync::sender::SendFormat::Auto,
        );
        let html = html.expect("formatted body keeps an html side");
        assert!(html.contains("<b>bold</b>"), "{html}");
        // Core's outgoing sanitizer adds rel=noopener to links; keep it.
        assert!(
            html.contains("<a href=\"https://example.com\" rel=\"noopener\">Hello</a>"),
            "{html}"
        );
        assert!(html.contains("<ul><li>a bullet</li></ul>"), "{html}");
        assert!(
            html.contains("<blockquote><p>a quote</p></blockquote>"),
            "{html}"
        );
        // The plain twin is derived, never literal tags.
        assert!(plain.contains("Hello Bob, bold here."), "{plain}");
        assert!(!plain.contains("<b>"), "{plain}");

        // 2. The real send format: Auto resolves by the body's formatting.
        assert!(mailcore::html::needs_html_formatting(&html));
        let format = mailcore::sync::sender::effective_format(
            mailcore::sync::sender::SendFormat::Auto,
            true,
            true,
        );
        assert_eq!(format, mailcore::sync::sender::SendFormat::Multipart);

        // Draft MIME (offline): headers + multipart + a derived plain twin.
        let req = form.as_request(&account, format, true, Receipts::default(), &policy);
        let bytes = format_draft(&account, &req).expect("MIME builds");
        let mime = String::from_utf8_lossy(&bytes);
        assert!(mime.contains("To: bob@example.com"), "{mime}");
        assert!(mime.contains("Subject: Spike B"), "{mime}");
        assert!(mime.contains("multipart/alternative"), "{mime}");
        assert!(mime.contains("Hello Bob, bold here."), "{mime}");
        assert!(mime.contains("a quote"), "{mime}");
    }

    /// A plain-only body must go out as text/plain (the Auto contract).
    #[test]
    fn plain_body_sends_as_plain() {
        let mut c = ComposerState::blank();
        c.to = "bob@example.com".to_string();
        c.subject = "Plain".to_string();
        c.doc.blocks[0].runs = vec![EditRun::plain("just text")];
        let form = c.form();
        let account = test_account();
        let policy = SendPolicy::Unrestricted;
        // A body with only paragraph structure carries no formatting, so
        // core resolves Auto to plain on Send — the same decision the Qt
        // bridge's `send_format_note` shows in the composer header.
        assert!(!mailcore::html::needs_html_formatting(&form.body_html));
        assert_eq!(
            mailcore::sync::sender::effective_format(
                mailcore::sync::sender::SendFormat::Auto,
                false,
                true
            ),
            mailcore::sync::sender::SendFormat::Plain
        );
        // Draft keeps the rich side either way (core is explicit about it);
        // the *plain* alternative must hold plain text, no literal tags.
        let req = form.as_request(
            &account,
            mailcore::sync::sender::SendFormat::Plain,
            true,
            Receipts::default(),
            &policy,
        );
        let bytes = format_draft(&account, &req).unwrap();
        let mime = String::from_utf8_lossy(&bytes);
        // The plain alternative is the first MIME part; check that part only.
        let boundary = mime
            .split("boundary=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap_or_default()
            .to_string();
        let plain_part = mime
            .split(&format!("--{boundary}"))
            .nth(1)
            .unwrap_or_default();
        assert!(plain_part.contains("just text"), "{plain_part}");
        assert!(!plain_part.contains("<p>"), "plain part carries no html");
    }

    /// Validation gate: no recipient is refused before anything is sent.
    #[test]
    fn no_recipient_is_refused() {
        let c = ComposerState::blank();
        assert!(c.form().require_recipient().is_err());
        let mut c2 = ComposerState::blank();
        c2.to = "a@example.com, b@example.com".to_string();
        let form = c2.form();
        assert_eq!(form.to, ["a@example.com", "b@example.com"]);
        form.require_recipient().unwrap();
    }
}
