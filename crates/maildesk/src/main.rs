//! maildesk: egui desktop client over mailcore (product track).
//!
//! Offline-first three-pane shell like the QML client: accounts +
//! subscribed folder trees + virtualised message list + a single reading
//! pane for the selected message. Opens the dev database (`MAILCLIENT_DB`,
//! default `./data/dev.sqlite`) and refuses the platform mailbox. No Qt,
//! no cxx-qt, no QML. No fixture seeding: shows what is in the DB.
//!
//! UI preferences live in `mailcore::store::settings` so every frontend
//! shares them: theme mode, last active account and last active folder.

mod paint;
mod reader;
mod render;

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Datelike, Local};
use mailcore::feed;
use mailcore::models::{Account, Folder};
use mailcore::store::messages::CompactMessage;
use mailcore::store::{accounts, folders, messages, settings};
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
    // Hard guard: maildesk dev runs must never open the real platform
    // mailbox. The filename alone is not the signal — a repo-local copy
    // (e.g. ./data/mailclient.sqlite via .env) is a legitimate dev DB.
    if s.contains(".local/share/mailclient") {
        return Err(mailcore::StoreError::InvalidInput(format!(
            "refusing to open {s}: maildesk dev runs use a dev database only \
             (override with MAILCLIENT_DB=./data/dev.sqlite)"
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

/// One-line snippet preview for the list delegate.
fn snippet_preview(snippet: Option<&str>, max_chars: usize) -> String {
    let s: String = snippet
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if s.chars().count() <= max_chars {
        s
    } else {
        format!(
            "{}…",
            s.chars()
                .take(max_chars.saturating_sub(1))
                .collect::<String>()
        )
    }
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

struct FolderRow {
    folder: Folder,
    unread: u64,
    depth: u64,
    leaf: String,
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
    reader: Option<reader::ReaderDoc>,
    allow_remote_once: bool,
    theme: String,
    applied_theme: String,
    last_dark: Option<bool>,
    filter: String,
    paint: paint::PaintState,
    status: String,
}

fn stored_id(db: &Db, key: &str) -> Option<i64> {
    settings::get(db, key)
        .ok()?
        .and_then(|v| v.trim().parse().ok())
}

impl MailApp {
    fn new(db: Db) -> Result<Self> {
        let accounts = accounts::list(&db).unwrap_or_default();
        let account_id = stored_id(&db, settings::LAST_ACTIVE_ACCOUNT_ID)
            .filter(|id| accounts.iter().any(|a| a.id == *id))
            .or_else(|| accounts.first().map(|a| a.id))
            .unwrap_or(-1);
        let theme = settings::get_theme_mode(&db);
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
            theme,
            applied_theme: String::new(),
            last_dark: None,
            filter: String::new(),
            paint: paint::PaintState::new(),
            status: "offline — dev database".to_string(),
        };
        app.reload_folders();
        // Restore the persisted folder when it belongs to this account.
        if let Some(fid) = stored_id(&app.db, settings::LAST_ACTIVE_FOLDER_ID) {
            if app.folders.iter().any(|f| f.folder.id == fid) {
                app.folder_id = fid;
                app.reload_list();
            }
        }
        Ok(app)
    }

    fn reload_folders(&mut self) {
        // Same visible subset as the other frontends: subscribed folders,
        // depth-indented (core computes depth/leaf).
        let list = folders::list_by_account(&self.db, self.account_id).unwrap_or_default();
        let counts = messages::counts_by_account(&self.db, self.account_id).unwrap_or_default();
        self.unread_by_folder = counts.iter().map(|(k, v)| (*k, v.unread)).collect();
        self.folders = list
            .into_iter()
            .filter(|f| f.subscribed)
            .map(|f| {
                let unread = self.unread_by_folder.get(&f.id).copied().unwrap_or(0);
                let depth = feed::folder_depth(&f.path, &f.delimiter);
                let leaf = feed::folder_leaf(&f.path, &f.delimiter);
                FolderRow {
                    folder: f,
                    unread,
                    depth,
                    leaf,
                }
            })
            .collect();
        if !self.folders.iter().any(|f| f.folder.id == self.folder_id) {
            self.folder_id = self.folders.first().map(|f| f.folder.id).unwrap_or(-1);
            self.selected = None;
            self.reader = None;
        }
        self.reload_list();
    }

    fn switch_account(&mut self, account_id: i64) {
        self.account_id = account_id;
        let _ = settings::set(
            &self.db,
            settings::LAST_ACTIVE_ACCOUNT_ID,
            &account_id.to_string(),
        );
        self.selected = None;
        self.reader = None;
        self.filter.clear();
        self.reload_folders();
        // Keep the persisted folder when it belongs to the new account.
        if let Some(fid) = stored_id(&self.db, settings::LAST_ACTIVE_FOLDER_ID) {
            if self.folders.iter().any(|f| f.folder.id == fid) {
                self.folder_id = fid;
                self.reload_list();
            }
        }
        self.status = "account switched".to_string();
    }

    fn select_folder(&mut self, folder_id: i64) {
        self.folder_id = folder_id;
        self.selected = None;
        self.reader = None;
        let _ = settings::set(
            &self.db,
            settings::LAST_ACTIVE_FOLDER_ID,
            &folder_id.to_string(),
        );
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

    fn refresh_pills(&mut self, folder_id: i64) {
        if let Ok(n) = messages::count_unread(&self.db, folder_id) {
            self.unread_by_folder.insert(folder_id, n);
            for f in &mut self.folders {
                if f.folder.id == folder_id {
                    f.unread = n;
                }
            }
        }
    }

    /// QML-style: selecting a message shows it in the single reading pane.
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
        self.refresh_pills(self.folder_id);
        self.reload_reader();
    }

    fn reload_reader(&mut self) {
        if let Some(uid) = self.selected {
            let dark = self.last_dark.unwrap_or(false);
            match reader::build_reader(&self.db, self.folder_id, uid, self.allow_remote_once, dark)
            {
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

    fn mark_unread_selected(&mut self) {
        if let Some(uid) = self.selected {
            if messages::set_read_many_by_uids(&self.db, self.folder_id, &[uid], false).is_ok() {
                if let Some(r) = self.rows.iter_mut().find(|r| r.uid == uid) {
                    r.is_read = false;
                }
                let fid = self.folder_id;
                self.refresh_pills(fid);
                self.status = "marked unread".to_string();
            }
        }
    }

    fn cycle_theme(&mut self) {
        self.theme = match self.theme.as_str() {
            "light" => "dark",
            "dark" => "system",
            _ => "light",
        }
        .to_string();
        let _ = settings::set(&self.db, settings::THEME_MODE, &self.theme);
        self.reader = None;
        self.reload_reader();
        self.status = format!("theme: {}", self.theme);
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        if self.applied_theme != self.theme {
            self.applied_theme = self.theme.clone();
            ctx.set_theme(match self.theme.as_str() {
                "light" => egui::ThemePreference::Light,
                "dark" => egui::ThemePreference::Dark,
                _ => egui::ThemePreference::System,
            });
        }
    }
}

const LIST_ROW_H: f32 = 74.0;

impl eframe::App for MailApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_theme(ctx);
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
                        if let Some(pos) =
                            self.selected.and_then(|s| ids.iter().position(|u| *u == s))
                        {
                            if pos > 0 {
                                self.select(ids[pos - 1]);
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
        if let Some(link) = self.paint.status.take() {
            self.status = link;
        }
        // The OS theme can flip under `system`; the reader's dark rewrite
        // must follow it, so rebuild once per change.
        let dark_now = ui.visuals().dark_mode;
        if self.last_dark != Some(dark_now) {
            self.last_dark = Some(dark_now);
            self.reader = None;
            self.reload_reader();
        }

        if let Some(id) = self.top_bar(ui) {
            self.switch_account(id);
        }
        self.status_bar(ui);
        self.folder_panel(ui);

        let mut clicked: Option<u32> = None;
        self.list_panel(ui, &mut clicked);
        if let Some(uid) = clicked {
            self.select(uid);
        }

        let mut action = reader::ReaderAction::None;
        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(doc) = &self.reader {
                let sel = self.selected;
                let fid = self.folder_id;
                action = egui::ScrollArea::vertical()
                    .id_salt(("reader", fid, sel))
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        let starred = sel
                            .and_then(|uid| {
                                self.rows
                                    .iter()
                                    .find(|r| r.uid == uid)
                                    .map(|r| r.is_starred)
                            })
                            .unwrap_or(false);
                        reader::show(ui, doc, self.allow_remote_once, starred, &mut self.paint)
                    })
                    .inner;
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new("Select a message to read it").weak());
                });
            }
        });
        match action {
            reader::ReaderAction::None => {}
            reader::ReaderAction::ToggleStar => self.toggle_star_selected(),
            reader::ReaderAction::MarkUnread => self.mark_unread_selected(),
            reader::ReaderAction::ShowRemoteOnce => {
                self.allow_remote_once = true;
                self.reload_reader();
                self.status = "remote images allowed for this message only".to_string();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Panels
// ---------------------------------------------------------------------------

impl MailApp {
    /// Returns the newly picked account, if the user changed it.
    fn top_bar(&mut self, ui: &mut egui::Ui) -> Option<i64> {
        let mut switched = None;
        egui::Panel::top("bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("maildesk");
                let mut picked = self.account_id;
                let cur_name = self
                    .accounts
                    .iter()
                    .find(|a| a.id == picked)
                    .map(|a| a.name.clone())
                    .unwrap_or_else(|| "—".to_string());
                let combo = egui::ComboBox::from_id_salt("account")
                    .selected_text(cur_name)
                    .show_ui(ui, |ui| {
                        for a in &self.accounts {
                            ui.selectable_value(&mut picked, a.id, &a.name);
                        }
                    });
                if picked != self.account_id {
                    switched = Some(picked);
                }
                let _ = combo;
                if ui
                    .add_enabled(false, egui::Button::new("✎ compose"))
                    .on_disabled_hover_text("The composer lands next")
                    .clicked()
                {
                    // Unreachable while disabled; kept so enabling is one flag.
                }
                if ui.button("⟳ reload").clicked() {
                    self.reload_folders();
                    self.status = "reloaded from dev database".to_string();
                }
                if ui.button(format!("theme: {}", self.theme)).clicked() {
                    self.cycle_theme();
                }
                ui.separator();
                let search = egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("Search this folder…")
                    .desired_width(200.0);
                ui.add(search);
                if !self.filter.is_empty() && ui.button("✕").clicked() {
                    self.filter.clear();
                }
            });
        });
        switched
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let unread: u64 = self.rows.iter().filter(|r| !r.is_read).count() as u64;
                    ui.label(
                        egui::RichText::new(format!(
                            "{} messages · {} unread",
                            self.rows.len(),
                            unread
                        ))
                        .weak(),
                    );
                });
            });
        });
    }

    fn folder_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("folders")
            .resizable(true)
            .default_size(200.0)
            .size_range(egui::Rangef::new(140.0, 340.0))
            .show(ui, |ui| {
                ui.heading("Folders");
                ui.separator();
                if self.folders.is_empty() {
                    ui.label(egui::RichText::new("No subscribed folders.").weak());
                    return;
                }
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        for i in 0..self.folders.len() {
                            let (id, leaf, path, unread, depth) = {
                                let f = &self.folders[i];
                                (
                                    f.folder.id,
                                    f.leaf.clone(),
                                    f.folder.path.clone(),
                                    f.unread,
                                    f.depth,
                                )
                            };
                            let sel = id == self.folder_id;
                            ui.horizontal(|ui| {
                                ui.add_space(depth as f32 * 14.0);
                                let mut label = egui::RichText::new(&leaf);
                                if sel {
                                    label = label.strong();
                                }
                                let resp = ui.selectable_label(sel, label).on_hover_text(path);
                                if unread > 0 {
                                    ui.label(
                                        egui::RichText::new(format!("{unread}"))
                                            .small()
                                            .color(ui.visuals().weak_text_color()),
                                    );
                                }
                                if resp.clicked() {
                                    self.select_folder(id);
                                }
                            });
                        }
                    });
            });
    }

    fn list_panel(&mut self, ui: &mut egui::Ui, clicked: &mut Option<u32>) {
        let visible: Vec<CompactMessage> = self.visible_rows().into_iter().cloned().collect();
        let folder_name = self
            .folders
            .iter()
            .find(|f| f.folder.id == self.folder_id)
            .map(|f| f.leaf.clone())
            .unwrap_or_default();
        egui::Panel::left("list")
            .resizable(true)
            .default_size(360.0)
            .size_range(egui::Rangef::new(240.0, 560.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(&folder_name);
                    ui.label(egui::RichText::new(format!("({})", visible.len())).weak());
                });
                if self.filter.trim().len() >= 2 {
                    ui.label(egui::RichText::new(format!("filter: {}", self.filter.trim())).weak());
                }
                ui.separator();
                if visible.is_empty() {
                    ui.label(egui::RichText::new("No messages.").weak());
                    return;
                }
                egui::ScrollArea::vertical().auto_shrink(false).show_rows(
                    ui,
                    LIST_ROW_H,
                    visible.len(),
                    |ui, range| {
                        for i in range {
                            let r = &visible[i];
                            let sel = self.selected == Some(r.uid);
                            if row_ui(ui, r, sel).clicked() {
                                *clicked = Some(r.uid);
                            }
                            ui.separator();
                        }
                    },
                );
            });
    }
}

/// One message row. Returns the click response.
fn row_ui(ui: &mut egui::Ui, r: &CompactMessage, sel: bool) -> egui::Response {
    use egui::text::{LayoutJob, TextFormat};

    let from = r
        .from_name
        .clone()
        .or(r.from_addr.clone())
        .unwrap_or_else(|| "?".to_string());
    let subject = r
        .subject
        .clone()
        .unwrap_or_else(|| "(no subject)".to_string());
    let date = relative_date(r.date.as_deref());
    let marks = format!(
        "{}{}{}",
        if r.is_read { "" } else { "● " },
        if r.is_starred { " ★" } else { "" },
        if r.has_attachments { " 📎" } else { "" }
    );

    let accent = ui.visuals().strong_text_color();
    let faint = ui.visuals().weak_text_color();
    let mut job = LayoutJob::default();
    job.append(
        &format!("{marks}{from}"),
        0.0,
        TextFormat {
            color: if r.is_read { faint } else { accent },
            ..Default::default()
        },
    );
    job.append("\n", 0.0, TextFormat::default());
    job.append(
        &subject,
        0.0,
        TextFormat {
            color: accent,
            ..Default::default()
        },
    );
    job.append("\n", 0.0, TextFormat::default());
    let tail = format!("{date} · {}", snippet_preview(r.snippet.as_deref(), 72));
    job.append(
        &tail,
        0.0,
        TextFormat {
            color: faint,
            ..Default::default()
        },
    );

    ui.selectable_label(sel, job)
}

fn main() -> Result<()> {
    let db = open_dev_db()?;
    let mut app = MailApp::new(db)?;
    // Dev/testing hook (mirrors `mailapp --open`): open one message at
    // startup so screenshots and manual checks can land on the reader.
    let select_uid = std::env::args()
        .skip_while(|a| a != "--select-uid")
        .nth(1)
        .and_then(|v| v.parse::<u32>().ok());
    if let Some(uid) = select_uid {
        app.select(uid);
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("maildesk")
            .with_inner_size([1360.0, 860.0]),
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

    #[test]
    fn snippet_preview_collapses_whitespace_and_truncates() {
        assert_eq!(snippet_preview(None, 10), "");
        assert_eq!(snippet_preview(Some("a  b\nc"), 10), "a b c");
        assert_eq!(snippet_preview(Some("abcdefghij"), 10), "abcdefghij");
        assert_eq!(snippet_preview(Some("abcdefghij"), 5), "abcd…");
    }
}
