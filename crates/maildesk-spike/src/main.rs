//! maildesk-spike: throwaway egui desktop experiment (Spike A).
//!
//! Offline only. Opens the dev database (`MAILCLIENT_DB`, default
//! `./data/dev.sqlite`) — never the platform mailbox — seeds ~20 fixture
//! mails, and shows folders + a virtualized message list + a prototype
//! reader over `mailcore::html::reader::body()`.
//!
//! Modes:
//! - interactive (default): three-pane eframe window.
//! - `--shot <dir> [--dark]`: scripted capture of every fixture mail to
//!   PNG (plus a `.dump.txt` block tree each), then quit. No keystroke
//!   driving; the app advances itself and exits.
//! - `--dump`: print every fixture's block tree to stdout, no GUI.

mod compose;
mod composer;
mod fixture;
mod paint;
mod render;

use std::path::PathBuf;

use mailcore::compose::{answer_draft_for, blank_draft, AnswerMode, AnswerOptions};
use mailcore::feed;
use mailcore::html::reader::{self, Palette};
use mailcore::html::{has_own_colors, inline_cid_images};
use mailcore::models::Account;
use mailcore::store::messages::CompactMessage;
use mailcore::store::{accounts, folders, messages};
use mailcore::{Db, Result};

fn dev_db_path() -> PathBuf {
    if let Ok(p) = std::env::var("MAILCLIENT_DB") {
        return PathBuf::from(p);
    }
    PathBuf::from("data/dev.sqlite")
}

fn open_dev_db() -> Result<Db> {
    let path = dev_db_path();
    // Hard guard: the spike must never open the real mailbox, even if
    // MAILCLIENT_DB is mispointed at it.
    let s = path.to_string_lossy();
    if s.contains(".local/share/mailclient")
        || s.contains("mailclient.sqlite") && !s.contains("dev.sqlite")
    {
        return Err(mailcore::StoreError::InvalidInput(format!(
            "refusing to open {s}: spike uses the dev database only"
        )));
    }
    Db::open(&path)
}

// ---------------------------------------------------------------------------
// Reader document (the §8.1 contract, minus web-only pieces)
// ---------------------------------------------------------------------------

struct ReaderDoc {
    uid: u32,
    subject: String,
    from_name: String,
    from_addr: String,
    date: String,
    is_html: bool,
    plain: String,
    had_remote: bool,
    missing_inline: usize,
    fit_below: u32,
    paint: reader::Paint,
    blocks: Vec<render::Block>,
    attachments: Vec<AttachmentView>,
    dump: String,
}

struct AttachmentView {
    name: String,
    size_text: String,
    cached: bool,
}

fn build_reader(
    db: &Db,
    folder_id: i64,
    uid: u32,
    allow_remote: bool,
    dark: bool,
) -> Result<ReaderDoc> {
    let m = messages::get_by_uid(db, folder_id, uid)?;
    let (safe, had_remote, is_html, plain) =
        feed::sanitized_bodies(m.body_html.as_deref(), m.body_text.as_deref(), allow_remote);
    let (safe, missing_inline) = if is_html {
        let imgs = messages::inline_images(db, m.id).unwrap_or_default();
        inline_cid_images(&safe, &imgs)
    } else {
        (safe, 0)
    };
    let colored = has_own_colors(&safe);
    let paint = reader::paint_for(colored, dark, false);
    // Narrow-fit is decided at paint time against the live width; the body
    // here is unfitted (fit=false) so the dump reflects full content.
    let body = reader::body(&safe, paint, false);
    let blocks = render::parse(&body);
    let dump = render::dump(&blocks);
    let attachments = messages::list_attachments(db, m.id)
        .unwrap_or_default()
        .into_iter()
        .filter(|a| !a.is_inline)
        .map(|a| AttachmentView {
            name: a
                .filename
                .clone()
                .unwrap_or_else(|| "(unnamed)".to_string()),
            size_text: mailcore::maintenance::format_bytes(a.size),
            cached: a.data.as_ref().is_some_and(|d| !d.is_empty()),
        })
        .collect();
    Ok(ReaderDoc {
        uid,
        subject: m.subject.clone().unwrap_or_default(),
        from_name: m.from_name.clone().unwrap_or_default(),
        from_addr: m.from_addr.clone().unwrap_or_default(),
        date: m.date.clone().unwrap_or_default(),
        is_html,
        plain,
        had_remote,
        missing_inline,
        fit_below: reader::fit_below(&safe),
        paint,
        blocks,
        attachments,
        dump,
    })
}

fn theme_palette(dark: bool) -> Palette {
    // Spike theme colours (egui defaults), as `#rrggbb` for the core.
    if dark {
        Palette::from_css("#1b1d21", "#e8e8e8", "#7ab3ff", "#9aa0a6", "#3a3d42")
    } else {
        Palette::from_css("#ffffff", "#202124", "#1a5fd0", "#5f6368", "#d0d4da")
    }
}

fn dummy_account() -> Account {
    Account {
        id: -1,
        name: "—".to_string(),
        email_address: fixture::SPIKE_EMAIL.to_string(),
        from_name: String::new(),
        imap_host: "imap.example.com".to_string(),
        imap_port: 993,
        imap_security: "tls".to_string(),
        imap_username: fixture::SPIKE_EMAIL.to_string(),
        smtp_host: "smtp.example.com".to_string(),
        smtp_port: 465,
        smtp_security: "tls".to_string(),
        smtp_username: fixture::SPIKE_EMAIL.to_string(),
        auth_vault_key: "vault-spike-fixture".to_string(),
        check_interval_secs: 300,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

/// Answer options from the stored settings (signature, bottom-posting),
/// like the real adapters' `stored_options`.
fn answer_opts(db: &Db, own: &str) -> AnswerOptions {
    use mailcore::store::settings;
    let sig_on = settings::get_bool(db, settings::SIGNATURE_ENABLED).unwrap_or(false);
    let sig_text = settings::get(db, settings::SIGNATURE_TEXT)
        .unwrap_or_default()
        .unwrap_or_default();
    AnswerOptions {
        own_address: own.to_string(),
        signature: (sig_on && !sig_text.trim().is_empty()).then_some(sig_text),
        reply_below_quote: settings::get_bool(db, settings::REPLY_BELOW_QUOTE).unwrap_or(false),
    }
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

struct FolderEntry {
    id: i64,
    path: String,
}

struct MailApp {
    db: Db,
    account: Account,
    account_name: String,
    folders: Vec<FolderEntry>,
    folder_id: i64,
    rows: Vec<CompactMessage>,
    selected: Option<u32>,
    reader: Option<ReaderDoc>,
    allow_remote_once: bool,
    dark: bool,
    paint: paint::PaintState,
    status: String,
    view: View,
    // Shot mode (scripted, self-advancing).
    shot: Option<ShotMode>,
    shot_compose: Option<ShotCompose>,
}

/// `Box` on the composer: its state is ~216 bytes and the reader variant
/// carries nothing, so the enum would otherwise be that size everywhere.
enum View {
    Read,
    Compose(Box<composer::ComposerState>),
}

struct ShotCompose {
    dir: PathBuf,
    settle: u8,
    shot_requested: bool,
}

struct ShotMode {
    dir: PathBuf,
    queue: Vec<(i64, u32, String)>, // (folder_id, uid, slug)
    next: usize,
    settle: u8,
    shot_requested: bool,
}

impl MailApp {
    fn new(db: Db, dark: bool, shot_dir: Option<PathBuf>) -> Result<Self> {
        fixture::seed(&db)?;
        let stored = accounts::list(&db)?
            .into_iter()
            .find(|a| a.email_address == fixture::SPIKE_EMAIL);
        let account_id = stored.as_ref().map(|a| a.id).unwrap_or(-1);
        let account_name = stored
            .as_ref()
            .map(|a| a.name.clone())
            .unwrap_or_else(|| "—".to_string());
        let account = stored.unwrap_or_else(dummy_account);
        let folders: Vec<FolderEntry> = folders::list_by_account(&db, account_id)
            .unwrap_or_default()
            .into_iter()
            .map(|f| FolderEntry {
                id: f.id,
                path: f.path,
            })
            .collect();
        let mut app = Self {
            db,
            account,
            account_name,
            folders,
            folder_id: -1,
            rows: Vec::new(),
            selected: None,
            reader: None,
            allow_remote_once: false,
            dark,
            paint: paint::PaintState::new(),
            status: "offline spike — dev DB only".to_string(),
            view: View::Read,
            shot: None,
            shot_compose: None,
        };
        if let Some(first) = app.folders.first() {
            app.folder_id = first.id;
        }
        app.reload_list();
        if let Some(dir) = shot_dir {
            std::fs::create_dir_all(&dir).map_err(|e| {
                mailcore::StoreError::InvalidInput(format!("cannot create {}: {e}", dir.display()))
            })?;
            // Queue from every folder's rows, so all fixtures are covered
            // (they span INBOX + Archive).
            let mut queue = Vec::new();
            for f in &app.folders {
                let rows =
                    messages::list_compact_by_folder_sorted(&app.db, f.id, 500, 0, "date", true)
                        .unwrap_or_default();
                for r in &rows {
                    queue.push((f.id, r.uid, slugify(&r.subject.clone().unwrap_or_default())));
                }
            }
            app.shot = Some(ShotMode {
                dir,
                queue,
                next: 0,
                settle: 0,
                shot_requested: false,
            });
        }
        Ok(app)
    }

    fn reload_list(&mut self) {
        self.rows =
            messages::list_compact_by_folder_sorted(&self.db, self.folder_id, 500, 0, "date", true)
                .unwrap_or_default();
    }

    fn select(&mut self, uid: u32) {
        self.selected = Some(uid);
        self.allow_remote_once = false;
        self.reload_reader();
    }

    fn reload_reader(&mut self) {
        if let Some(uid) = self.selected {
            match build_reader(
                &self.db,
                self.folder_id,
                uid,
                self.allow_remote_once,
                self.dark,
            ) {
                Ok(doc) => self.reader = Some(doc),
                Err(e) => self.status = format!("reader: {e}"),
            }
        }
    }

    fn show_once(&mut self) {
        self.allow_remote_once = true;
        self.reload_reader();
        self.status = "remote images allowed for this message only".to_string();
    }

    fn open_compose_blank(&mut self) {
        let opts = answer_opts(&self.db, &self.account.email_address);
        self.view = View::Compose(Box::new(composer::ComposerState::from_answer(
            &blank_draft(&opts),
        )));
        self.status = "composing (nothing is sent)".to_string();
    }

    fn open_reply(&mut self) {
        let (Some(uid), folder_id) = (self.selected, self.folder_id) else {
            self.status = "select a message first".to_string();
            return;
        };
        match answer_draft_for(&self.db, folder_id, uid, AnswerMode::Reply) {
            Ok(draft) => {
                self.view = View::Compose(Box::new(composer::ComposerState::from_answer(&draft)));
                self.status = "reply prefilled by mailcore::compose::answer".to_string();
            }
            Err(e) => self.status = format!("reply prefill: {e}"),
        }
    }
}

const LIST_ROW_H: f32 = 56.0;

impl eframe::App for MailApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.dark {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        // Screenshot replies (shot mode).
        ctx.input(|i| {
            for ev in &i.events {
                if let egui::Event::Screenshot { image, user_data, .. } = ev {
                    if let Some(idx) = user_data
                        .data
                        .as_ref()
                        .and_then(|d| d.downcast_ref::<usize>())
                        .copied()
                    {
                        if let Some(shot) = &mut self.shot {
                            let (_fid, uid, slug) = &shot.queue[idx];
                            let path = shot.dir.join(format!("{idx:02}-{slug}.png"));
                            save_png(&path, image);
                            // Block dump alongside, for structural review.
                            if let Some(doc) = &self.reader {
                                let _ = std::fs::write(
                                    shot.dir.join(format!("{idx:02}-{slug}.dump.txt")),
                                    format!(
                                        "uid={uid} paint={:?} had_remote={} missing_inline={} fit_below={}\n{}",
                                        doc.paint, doc.had_remote, doc.missing_inline, doc.fit_below, doc.dump
                                    ),
                                );
                            }
                            shot.next = idx + 1;
                            shot.settle = 0;
                            shot.shot_requested = false;
                        } else if idx == usize::MAX {
                            // Compose shot: the MIME preview alongside the
                            // window, for the composer grade.
                            if let Some(sc) = &self.shot_compose {
                                save_png(&sc.dir.join("compose-demo.png"), image);
                            }
                        }
                    }
                }
            }
        });

        // Shot driver: select next mail, settle, capture, quit at the end.
        if self.shot.as_ref().is_some_and(|s| s.next >= s.queue.len()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if let Some((fid, uid)) = self
            .shot
            .as_ref()
            .and_then(|s| s.queue.get(s.next).map(|(fid, uid, _)| (*fid, *uid)))
        {
            if self.folder_id != fid {
                self.folder_id = fid;
                self.selected = None;
                self.reader = None;
                self.reload_list();
                if let Some(shot) = &mut self.shot {
                    shot.settle = 0;
                    shot.shot_requested = false;
                }
            } else if self.selected != Some(uid) {
                self.select(uid);
                if let Some(shot) = &mut self.shot {
                    shot.settle = 0;
                    shot.shot_requested = false;
                }
            } else {
                let advance = if let Some(shot) = &mut self.shot {
                    if shot.settle < 8 {
                        shot.settle += 1;
                        false
                    } else if !shot.shot_requested {
                        shot.shot_requested = true;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };
                if advance {
                    let next = self.shot.as_ref().map(|s| s.next).unwrap_or(0);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                        next,
                    )));
                }
                ctx.request_repaint();
            }
        }

        // Compose shot driver: fill the demo once, settle, capture, quit.
        if self.shot_compose.is_some() && !matches!(self.view, View::Compose(_)) {
            let mut c = composer::ComposerState::blank();
            c.preseed_demo(&self.account.clone());
            self.view = View::Compose(Box::new(c));
        }
        if let Some(sc) = &mut self.shot_compose {
            if sc.settle < 8 {
                sc.settle += 1;
                ctx.request_repaint();
            } else if !sc.shot_requested {
                sc.shot_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                    usize::MAX,
                )));
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        egui::Panel::top("bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("maildesk-spike");
                ui.label(format!("account: {}", self.account_name));
                ui.label("(offline, dev DB)");
                if ui
                    .button(if self.dark { "light" } else { "dark" })
                    .clicked()
                {
                    self.dark = !self.dark;
                    self.reload_reader();
                }
                if ui.button("compose").clicked() {
                    self.open_compose_blank();
                }
                if matches!(self.view, View::Compose(_)) && ui.button("reader").clicked() {
                    self.view = View::Read;
                }
            });
        });

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                let s = self
                    .paint
                    .status
                    .take()
                    .unwrap_or_else(|| self.status.clone());
                ui.label(s);
            });
        });

        egui::Panel::left("folders")
            .exact_size(170.0)
            .show(ui, |ui| {
                ui.heading("Folders");
                for i in 0..self.folders.len() {
                    let sel = self.folders[i].id == self.folder_id;
                    let path = self.folders[i].path.clone();
                    if ui.selectable_label(sel, &path).clicked() {
                        self.folder_id = self.folders[i].id;
                        self.selected = None;
                        self.reader = None;
                        self.reload_list();
                    }
                }
            });

        egui::Panel::left("list").exact_size(300.0).show(ui, |ui| {
            ui.heading(format!("Messages ({})", self.rows.len()));
            // Virtualized rows: only the visible window is painted.
            egui::ScrollArea::vertical().auto_shrink(false).show_rows(
                ui,
                LIST_ROW_H,
                self.rows.len(),
                |ui, range| {
                    for i in range {
                        let r = &self.rows[i];
                        let sel = self.selected == Some(r.uid);
                        let subject = r
                            .subject
                            .clone()
                            .unwrap_or_else(|| "(no subject)".to_string());
                        let from = r
                            .from_name
                            .clone()
                            .or(r.from_addr.clone())
                            .unwrap_or_else(|| "?".to_string());
                        if ui
                            .selectable_label(sel, format!("{from}\n{subject}"))
                            .clicked()
                        {
                            self.select(r.uid);
                        }
                    }
                },
            );
        });

        let mut want_reply = false;
        egui::CentralPanel::default().show(ui, |ui| match &mut self.view {
            View::Compose(c) => {
                let account = self.account.clone();
                if c.show(ui, &ctx, &account, &mut self.paint) {
                    self.view = View::Read;
                }
            }
            View::Read => {
                if let Some(doc) = &self.reader {
                    ui.horizontal(|ui| {
                        if ui.button("reply").clicked() {
                            want_reply = true;
                        }
                    });
                    let show_once = reader_ui(ui, doc, self.allow_remote_once, &mut self.paint);
                    if show_once {
                        self.show_once();
                    }
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label("Select a message to read it");
                    });
                }
            }
        });
        if want_reply {
            self.open_reply();
        }
    }
}

/// Returns true when the user tapped "Show once".
fn reader_ui(
    ui: &mut egui::Ui,
    doc: &ReaderDoc,
    allow_remote: bool,
    pst: &mut paint::PaintState,
) -> bool {
    let mut show_once = false;
    // Per-message scroll state: opening another mail starts at the top
    // instead of inheriting the previous mail's offset.
    egui::ScrollArea::vertical()
        .id_salt(("reader", doc.uid))
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.heading(&doc.subject);
            ui.label(format!(
                "From: {} <{}>  |  {}",
                doc.from_name, doc.from_addr, doc.date
            ));
            ui.separator();

            // Banners (reader contract §A): remote-once, missing inline.
            if doc.is_html && doc.had_remote && !allow_remote {
                ui.horizontal(|ui| {
                    ui.label("Remote images blocked.");
                    if ui.button("Show once").clicked() {
                        show_once = true;
                    }
                });
            }
            if doc.missing_inline > 0 {
                ui.label(format!(
                    "{} inline image(s) not downloaded.",
                    doc.missing_inline
                ));
            }

            if !doc.attachments.is_empty() {
                ui.collapsing(format!("Attachments ({})", doc.attachments.len()), |ui| {
                    for a in &doc.attachments {
                        ui.horizontal(|ui| {
                            ui.label(format!("{}  ({})", a.name, a.size_text));
                            ui.label(if a.cached { "cached" } else { "on server" });
                        });
                    }
                });
            }
            ui.separator();

            if !doc.is_html {
                ui.label(&doc.plain);
            } else {
                let colors = if doc.paint == reader::Paint::Theme {
                    paint::Colors::themed(ui)
                } else {
                    paint::Colors::from_palette(&reader::palette(
                        doc.paint,
                        &theme_palette(ui.visuals().dark_mode),
                    ))
                };
                // Narrow-fit like the web document: loosen fixed widths (already
                // done by reader::body when `fit`; here the layout is fluid, so
                // tables render at the available width — the deliberate lossy
                // simplification for multi-column layouts).
                paint::paint_blocks(ui, &doc.blocks, &colors, pst);
            }
        });
    show_once
}

fn save_png(path: &std::path::Path, image: &egui::ColorImage) {
    let (w, h) = (image.width() as u32, image.height() as u32);
    let mut buf = Vec::with_capacity((w * h * 4) as usize);
    for p in &image.pixels {
        buf.extend_from_slice(&[p.r(), p.g(), p.b(), p.a()]);
    }
    let err = image::RgbaImage::from_raw(w, h, buf)
        .map(|img| image::DynamicImage::ImageRgba8(img).save(path))
        .unwrap_or_else(|| {
            Err(image::ImageError::Parameter(
                image::error::ParameterError::from_kind(
                    image::error::ParameterErrorKind::DimensionMismatch,
                ),
            ))
        })
        .err();
    match err {
        Some(e) => eprintln!("shot failed {}: {e}", path.display()),
        None => println!("{}", path.display()),
    }
}

fn run_dump() -> Result<()> {
    let db = open_dev_db()?;
    let mails = fixture::seed(&db)?;
    for m in &mails {
        // Fixtures live in INBOX except the long thread (Archive): find it.
        let mut found = None;
        for folder in folders::list_by_account(&db, account_id(&db)?).unwrap_or_default() {
            if messages::get_by_uid(&db, folder.id, m.uid).is_ok() {
                found = Some(folder.id);
                break;
            }
        }
        if let Some(fid) = found {
            let doc = build_reader(&db, fid, m.uid, false, false)?;
            println!(
                "=== {:02} {} [{}] paint={:?} remote={} missing_inline={} fit_below={}",
                m.uid, m.slug, m.kind, doc.paint, doc.had_remote, doc.missing_inline, doc.fit_below
            );
            println!("{}", doc.dump);
        }
    }
    Ok(())
}

fn slugify(subject: &str) -> String {
    let mut s: String = subject
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "untitled".to_string()
    } else {
        s.chars().take(28).collect()
    }
}

fn account_id(db: &Db) -> Result<i64> {
    accounts::list(db)?
        .into_iter()
        .find(|a| a.email_address == fixture::SPIKE_EMAIL)
        .map(|a| a.id)
        .ok_or_else(|| mailcore::StoreError::InvalidInput("spike account missing".to_string()))
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--dump") {
        return run_dump();
    }
    let shot = args
        .windows(2)
        .find(|w| w[0] == "--shot")
        .map(|w| PathBuf::from(&w[1]));
    let shot_compose = args
        .windows(2)
        .find(|w| w[0] == "--shot-compose")
        .map(|w| PathBuf::from(&w[1]));
    let dark = args.iter().any(|a| a == "--dark");

    let db = open_dev_db()?;
    let mut app = MailApp::new(db, dark, shot)?;
    if let Some(dir) = shot_compose {
        std::fs::create_dir_all(&dir).map_err(|e| {
            mailcore::StoreError::InvalidInput(format!("cannot create {}: {e}", dir.display()))
        })?;
        app.shot_compose = Some(ShotCompose {
            dir,
            settle: 0,
            shot_requested: false,
        });
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("maildesk-spike (auto-closes in shot mode)")
            .with_inner_size([1000.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native("maildesk-spike", options, Box::new(|_cc| Ok(Box::new(app))))
        .map_err(|e| mailcore::StoreError::InvalidInput(format!("eframe: {e}")))
}
