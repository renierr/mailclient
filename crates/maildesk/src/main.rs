//! maildesk: egui desktop client over mailcore (product track).
//!
//! Offline-first three-pane shell: accounts + folder trees + virtualised
//! message list + reader. Opens the dev database (`MAILCLIENT_DB`, default
//! `./data/dev.sqlite`) and refuses the platform mailbox, like the spike.
//! No Qt, no cxx-qt, no QML. No fixture seeding: shows what is in the DB.

mod paint;
mod render;

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Local};
use mailcore::feed;
use mailcore::html::reader::{self, Palette};
use mailcore::html::{has_own_colors, inline_cid_images};
use mailcore::models::{Account, Folder};
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
    let s = path.to_string_lossy();
    if s.contains(".local/share/mailclient")
        || s.contains("mailclient.sqlite") && !s.contains("dev.sqlite")
    {
        return Err(mailcore::StoreError::InvalidInput(format!(
            "refusing to open {s}: maildesk dev runs use the dev database only"
        )));
    }
    Db::open(&path)
}

/// Relative date for the list. Mirrors the intent of `feed::short_date`
/// (which is `pub(crate)`): today as clock time, yesterday by word, older
/// by day. Candidate for promotion into `mailcore` so all frontends share
/// one function instead of this UI-local twin.
fn relative_date(raw: Option<&str>) -> String {
    let Some(raw) = raw else { return String::new() };
    let Ok(dt) = raw.parse::<DateTime<chrono::FixedOffset>>() else {
        return raw.to_string();
    };
    let local: DateTime<Local> = dt.with_timezone(&Local);
    let now = Local::now();
    let days = (now.date_naive() - local.date_naive()).num_days();
    if days <= 0 {
        local.format("%H:%M").to_string()
    } else if days == 1 {
        "Yesterday".to_string()
    } else if local.date_naive().year() == now.date_naive().year() {
        local
            .format("%e %b")
            .to_string()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        local.format("%Y-%m-%d").to_string()
    }
}

use chrono::Datelike;

// ---------------------------------------------------------------------------
// Reader document (same core pipeline as the spike: sanitise in core,
// dark rewrite + fit in core, parse + paint here)
// ---------------------------------------------------------------------------

struct ReaderDoc {
    uid: u32,
    subject: String,
    from_name: String,
    from_addr: String,
    to_line: String,
    date: String,
    is_html: bool,
    plain: String,
    had_remote: bool,
    missing_inline: usize,
    paint: reader::Paint,
    blocks: Vec<render::Block>,
    attachments: Vec<AttachmentView>,
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
    let body = reader::body(&safe, paint, false);
    let blocks = render::parse(&body);
    let to_line = m.to_addrs.join(", ");
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
        to_line,
        date: m.date.clone().unwrap_or_default(),
        is_html,
        plain,
        had_remote,
        missing_inline,
        paint,
        blocks,
        attachments,
    })
}

fn theme_palette(dark: bool) -> Palette {
    if dark {
        Palette::from_css("#1b1d21", "#e8e8e8", "#7ab3ff", "#9aa0a6", "#3a3d42")
    } else {
        Palette::from_css("#ffffff", "#202124", "#1a5fd0", "#5f6368", "#d0d4da")
    }
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

struct FolderRow {
    folder: Folder,
    unread: u64,
}

struct MailApp {
    db: Db,
    accounts: Vec<Account>,
    account_id: i64,
    folders: Vec<FolderRow>,
    folder_id: i64,
    rows: Vec<CompactMessage>,
    unread_by_folder: HashMap<i64, u64>,
    selected: Option<u32>,
    reader: Option<ReaderDoc>,
    allow_remote_once: bool,
    dark: bool,
    filter: String,
    paint: paint::PaintState,
    status: String,
}

impl MailApp {
    fn new(db: Db, dark: bool) -> Result<Self> {
        let accounts = accounts::list(&db).unwrap_or_default();
        let account_id = accounts.first().map(|a| a.id).unwrap_or(-1);
        let mut app = Self {
            db,
            accounts,
            account_id,
            folders: Vec::new(),
            folder_id: -1,
            rows: Vec::new(),
            unread_by_folder: HashMap::new(),
            selected: None,
            reader: None,
            allow_remote_once: false,
            dark,
            filter: String::new(),
            paint: paint::PaintState::new(),
            status: "offline — dev database".to_string(),
        };
        app.reload_folders();
        Ok(app)
    }

    fn reload_folders(&mut self) {
        let list = folders::list_by_account(&self.db, self.account_id).unwrap_or_default();
        let counts = messages::counts_by_account(&self.db, self.account_id).unwrap_or_default();
        self.unread_by_folder = counts.iter().map(|(k, v)| (*k, v.unread)).collect();
        self.folders = list
            .into_iter()
            .map(|f| {
                let unread = self.unread_by_folder.get(&f.id).copied().unwrap_or(0);
                FolderRow { folder: f, unread }
            })
            .collect();
        if !self.folders.iter().any(|f| f.folder.id == self.folder_id) {
            self.folder_id = self.folders.first().map(|f| f.folder.id).unwrap_or(-1);
            self.selected = None;
            self.reader = None;
        }
        self.reload_list();
    }

    fn reload_list(&mut self) {
        self.rows = messages::list_compact_by_folder_sorted(
            &self.db,
            self.folder_id,
            2000,
            0,
            "date",
            true,
        )
        .unwrap_or_default();
        if let Some(uid) = self.selected {
            if !self.rows.iter().any(|r| r.uid == uid) {
                self.selected = None;
                self.reader = None;
            }
        }
    }

    fn visible_rows(&self) -> Vec<&CompactMessage> {
        let q = self.filter.trim().to_lowercase();
        if q.len() < 2 {
            return self.rows.iter().collect();
        }
        self.rows
            .iter()
            .filter(|r| {
                r.subject
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&q)
                    || r.from_addr
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&q)
                    || r.from_name
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&q)
                    || r.snippet
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&q)
            })
            .collect()
    }

    fn select(&mut self, uid: u32) {
        if self.selected == Some(uid) {
            return;
        }
        self.selected = Some(uid);
        self.allow_remote_once = false;
        // Local mark-read on open (offline write, pushed on next sync).
        let _ = messages::set_read_many_by_uids(&self.db, self.folder_id, &[uid], true);
        if let Some(r) = self.rows.iter_mut().find(|r| r.uid == uid) {
            r.is_read = true;
        }
        self.reload_reader();
        // Refresh the pill without a full folder reload.
        if let Ok(n) = messages::count_unread(&self.db, self.folder_id) {
            self.unread_by_folder.insert(self.folder_id, n);
            for f in &mut self.folders {
                if f.folder.id == self.folder_id {
                    f.unread = n;
                }
            }
        }
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

    fn toggle_star_selected(&mut self) {
        if let Some(uid) = self.selected {
            let cur = self
                .rows
                .iter()
                .find(|r| r.uid == uid)
                .map(|r| r.is_starred)
                .unwrap_or(false);
            if messages::set_star_many_by_uids(&self.db, self.folder_id, &[uid], !cur).is_ok() {
                if let Some(r) = self.rows.iter_mut().find(|r| r.uid == uid) {
                    r.is_starred = !cur;
                }
                self.status = if cur {
                    "unstarred".to_string()
                } else {
                    "starred".to_string()
                };
            }
        }
    }
}

const LIST_ROW_H: f32 = 64.0;

impl eframe::App for MailApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.set_visuals(if self.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
        // Keyboard navigation in the list.
        ctx.input(|i| {
            for ev in &i.events {
                match ev {
                    egui::Event::Key {
                        key: egui::Key::ArrowDown,
                        pressed: true,
                        ..
                    } => {
                        let ids: Vec<u32> = self.rows.iter().map(|r| r.uid).collect();
                        let next =
                            match self.selected.and_then(|s| ids.iter().position(|u| *u == s)) {
                                Some(pos) if pos + 1 < ids.len() => Some(ids[pos + 1]),
                                None if !ids.is_empty() => Some(ids[0]),
                                other => other.and_then(|p| ids.get(p).copied()),
                            };
                        if let Some(uid) = next {
                            self.select(uid);
                        }
                    }
                    egui::Event::Key {
                        key: egui::Key::ArrowUp,
                        pressed: true,
                        ..
                    } => {
                        let ids: Vec<u32> = self.rows.iter().map(|r| r.uid).collect();
                        if let Some(sel) =
                            self.selected.and_then(|s| ids.iter().position(|u| *u == s))
                        {
                            if sel > 0 {
                                self.select(ids[sel - 1]);
                            }
                        } else if !ids.is_empty() {
                            self.select(ids[0]);
                        }
                    }
                    _ => {}
                }
            }
        });
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Link-click feedback from the reader painter.
        if let Some(link) = self.paint.status.take() {
            self.status = link;
        }

        egui::Panel::top("bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("maildesk");
                // Account picker over real accounts.
                let cur_name = self
                    .accounts
                    .iter()
                    .find(|a| a.id == self.account_id)
                    .map(|a| a.name.clone())
                    .unwrap_or_else(|| "—".to_string());
                egui::ComboBox::from_id_salt("account")
                    .selected_text(cur_name)
                    .show_ui(ui, |ui| {
                        for a in &self.accounts {
                            ui.selectable_value(&mut self.account_id, a.id, &a.name);
                        }
                    });
                if ui
                    .button(if self.dark { "☀ light" } else { "🌙 dark" })
                    .clicked()
                {
                    self.dark = !self.dark;
                    self.reload_reader();
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.filter)
                        .hint_text("Search this folder…")
                        .desired_width(180.0),
                );
                if ui.button("⟳ reload").clicked() {
                    self.reload_folders();
                    self.status = "reloaded from dev database".to_string();
                }
            });
        });

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
            });
        });

        egui::Panel::left("folders")
            .exact_size(190.0)
            .show(ui, |ui| {
                ui.heading("Folders");
                ui.separator();
                if self.folders.is_empty() {
                    ui.label("No folders in this database.");
                }
                for i in 0..self.folders.len() {
                    let (id, path, unread) = {
                        let f = &self.folders[i];
                        (f.folder.id, f.folder.path.clone(), f.unread)
                    };
                    let sel = id == self.folder_id;
                    ui.horizontal(|ui| {
                        let label = if unread > 0 {
                            format!("{path}  ({unread})")
                        } else {
                            path.clone()
                        };
                        if ui.selectable_label(sel, label).clicked() {
                            self.folder_id = id;
                            self.selected = None;
                            self.reader = None;
                            self.reload_list();
                        }
                    });
                }
            });

        let visible: Vec<CompactMessage> = self.visible_rows().into_iter().cloned().collect();
        let mut clicked: Option<u32> = None;
        egui::Panel::left("list").exact_size(340.0).show(ui, |ui| {
            ui.heading(format!("Messages ({})", visible.len()));
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink(false).show_rows(
                ui,
                LIST_ROW_H,
                visible.len(),
                |ui, range| {
                    for i in range {
                        let r = &visible[i];
                        let sel = self.selected == Some(r.uid);
                        let from = r
                            .from_name
                            .clone()
                            .or(r.from_addr.clone())
                            .unwrap_or_else(|| "?".to_string());
                        let subject = r
                            .subject
                            .clone()
                            .unwrap_or_else(|| "(no subject)".to_string());
                        let dot = if r.is_read { " " } else { "● " };
                        let star = if r.is_starred { " ★" } else { "" };
                        let clip = if r.has_attachments { " 📎" } else { "" };
                        let date = relative_date(r.date.as_deref());
                        let text = format!("{dot}{from}{star}{clip}\n{subject}\n{date}");
                        let resp = ui.selectable_label(sel, text);
                        if resp.clicked() {
                            clicked = Some(r.uid);
                        }
                    }
                },
            );
        });
        if let Some(uid) = clicked {
            self.select(uid);
        }

        let mut want_star = false;
        let mut want_remote_once = false;
        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(doc) = &self.reader {
                let allow_remote_once = self.allow_remote_once;
                egui::ScrollArea::vertical()
                    .id_salt(("reader", doc.uid))
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        ui.heading(&doc.subject);
                        ui.label(format!(
                            "From: {} <{}>",
                            if doc.from_name.is_empty() {
                                "?"
                            } else {
                                &doc.from_name
                            },
                            doc.from_addr
                        ));
                        if !doc.to_line.is_empty() {
                            ui.label(format!("To: {}", doc.to_line));
                        }
                        ui.label(&doc.date);
                        if ui.button("☆ toggle star").clicked() {
                            want_star = true;
                        }
                        ui.separator();
                        if doc.is_html && doc.had_remote && !allow_remote_once {
                            ui.horizontal(|ui| {
                                ui.label("Remote images blocked.");
                                if ui.button("Show once").clicked() {
                                    want_remote_once = true;
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
                            ui.collapsing(
                                format!("Attachments ({})", doc.attachments.len()),
                                |ui| {
                                    for a in &doc.attachments {
                                        ui.horizontal(|ui| {
                                            ui.label(format!("{}  ({})", a.name, a.size_text));
                                            ui.label(if a.cached { "cached" } else { "on server" });
                                        });
                                    }
                                },
                            );
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
                            paint::paint_blocks(ui, &doc.blocks, &colors, &mut self.paint);
                        }
                    });
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("Select a message to read it");
                });
            }
        });
        if want_star {
            self.toggle_star_selected();
        }
        if want_remote_once {
            self.allow_remote_once = true;
            self.reload_reader();
        }
    }
}

fn main() -> Result<()> {
    let dark = std::env::args().any(|a| a == "--dark");
    let db = open_dev_db()?;
    let app = MailApp::new(db, dark)?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("maildesk")
            .with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native("maildesk", options, Box::new(|_cc| Ok(Box::new(app))))
        .map_err(|e| mailcore::StoreError::InvalidInput(format!("eframe: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_date_handles_today_yesterday_and_empty() {
        assert_eq!(relative_date(None), "");
        assert_eq!(relative_date(Some("not-a-date")), "not-a-date");
        let now = Local::now();
        let today = now.to_rfc3339();
        assert_eq!(relative_date(Some(&today)), now.format("%H:%M").to_string());
        let yesterday = now - chrono::Duration::days(1);
        // Same-clock edge (23:59 -> 00:01) could flip; midday is stable.
        let mid = yesterday
            .date_naive()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap();
        assert_eq!(relative_date(Some(&mid.to_rfc3339())), "Yesterday");
    }
}
