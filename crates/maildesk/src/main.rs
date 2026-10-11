//! maildesk: egui desktop client over mailcore (product track).
//!
//! Offline-first shell: accounts + subscribed folder trees + virtualised
//! message list + tabbed reader (draggable/resizable panes via egui_dock).
//! Opens the dev database (`MAILCLIENT_DB`, default `./data/dev.sqlite`)
//! and refuses the platform mailbox. No Qt, no cxx-qt, no QML. No fixture
//! seeding: shows what is in the DB.

mod paint;
mod reader;
mod render;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use chrono::{DateTime, Datelike, Local};
use egui_dock::{DockArea, DockState, Style, TabViewer};
use mailcore::feed;
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
// Tabs
// ---------------------------------------------------------------------------

/// One open message. Equality is `(folder_id, uid)` only: title and star
/// state are snapshots that change without the tab becoming a new tab.
struct MsgTab {
    folder_id: i64,
    uid: u32,
    title: String,
    starred: bool,
}

impl PartialEq for MsgTab {
    fn eq(&self, other: &Self) -> bool {
        self.folder_id == other.folder_id && self.uid == other.uid
    }
}

struct Viewer<'a> {
    data: &'a mut Data,
}

impl TabViewer for Viewer<'_> {
    type Tab = MsgTab;

    fn id(&mut self, tab: &mut MsgTab) -> egui::Id {
        egui::Id::new((tab.folder_id, tab.uid))
    }

    fn title(&mut self, tab: &mut MsgTab) -> egui::WidgetText {
        tab.title.clone().into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut MsgTab) {
        let key = (tab.folder_id, tab.uid);
        let dark = self.data.dark;
        if !self.data.docs.contains_key(&key) {
            let allow_remote = self.data.remote_once.contains(&key);
            match reader::build_reader(&self.data.db, tab.folder_id, tab.uid, allow_remote, dark) {
                Ok(doc) => {
                    self.data.docs.insert(key, doc);
                }
                Err(e) => {
                    self.data.status = format!("reader: {e}");
                    ui.centered_and_justified(|ui| {
                        ui.label("Could not open this message.");
                    });
                    return;
                }
            }
        }
        let remote_allowed = self.data.remote_once.contains(&key);
        let action = egui::ScrollArea::vertical()
            .id_salt(("reader", tab.folder_id, tab.uid))
            .auto_shrink(false)
            .show(ui, |ui| {
                // Scope the doc borrow to the paint pass; actions run after.
                let doc = self.data.docs.get(&key).expect("just built");
                reader::show(ui, doc, remote_allowed, tab.starred, &mut self.data.paint)
            })
            .inner;
        match action {
            reader::ReaderAction::None => {}
            reader::ReaderAction::ToggleStar => self.data.toggle_star(tab),
            reader::ReaderAction::MarkUnread => self.data.mark_unread(tab.folder_id, tab.uid),
            reader::ReaderAction::ShowRemoteOnce => {
                self.data.remote_once.insert(key);
                self.data.docs.remove(&key);
            }
        }
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

/// Everything except the dock tree. Kept separate so the dock pass can
/// borrow `dock` and `data` mutably side by side (`DockArea::new(&mut
/// dock).show_inside(ui, &mut Viewer { data })`).
struct Data {
    db: Db,
    accounts: Vec<Account>,
    account_id: i64,
    folders: Vec<FolderRow>,
    folder_id: i64,
    rows: Vec<CompactMessage>,
    unread_by_folder: HashMap<i64, u64>,
    docs: HashMap<(i64, u32), reader::ReaderDoc>,
    remote_once: HashSet<(i64, u32)>,
    focused: Option<(i64, u32)>,
    dark: bool,
    filter: String,
    paint: paint::PaintState,
    status: String,
}

struct MailApp {
    data: Data,
    dock: DockState<MsgTab>,
}

impl Data {
    fn new(db: Db, dark: bool) -> Result<Self> {
        let accounts = accounts::list(&db).unwrap_or_default();
        let account_id = accounts.first().map(|a| a.id).unwrap_or(-1);
        let mut data = Self {
            db,
            accounts,
            account_id,
            folders: Vec::new(),
            folder_id: -1,
            rows: Vec::new(),
            unread_by_folder: HashMap::new(),
            docs: HashMap::new(),
            remote_once: HashSet::new(),
            focused: None,
            dark,
            filter: String::new(),
            paint: paint::PaintState::new(),
            status: "offline — dev database".to_string(),
        };
        data.reload_folders();
        Ok(data)
    }

    fn reload_folders(&mut self) {
        // Same visible subset as the other frontends: subscribed folders,
        // folded and depth-indented (core computes depth/leaf).
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
        }
        self.reload_list();
    }
}

impl MailApp {
    fn new(db: Db, dark: bool) -> Result<Self> {
        Ok(Self {
            data: Data::new(db, dark)?,
            dock: DockState::new(vec![]),
        })
    }

    fn switch_account(&mut self, account_id: i64) {
        self.data.account_id = account_id;
        self.dock = DockState::new(vec![]);
        self.data.docs.clear();
        self.data.remote_once.clear();
        self.data.focused = None;
        self.data.filter.clear();
        self.data.reload_folders();
        self.data.status = "account switched".to_string();
    }

    fn open_tab(&mut self, folder_id: i64, uid: u32) {
        // Local mark-read on open (offline write, pushed on next sync).
        let _ = messages::set_read_many_by_uids(&self.data.db, folder_id, &[uid], true);
        if folder_id == self.data.folder_id {
            if let Some(r) = self.data.rows.iter_mut().find(|r| r.uid == uid) {
                r.is_read = true;
            }
        }
        self.data.refresh_pills(folder_id);
        self.data.focused = Some((folder_id, uid));
        let probe = MsgTab {
            folder_id,
            uid,
            title: String::new(),
            starred: false,
        };
        if let Some(path) = self.dock.find_tab(&probe) {
            let _ = self.dock.set_active_tab(path);
            return;
        }
        let row = (folder_id == self.data.folder_id)
            .then(|| self.data.rows.iter().find(|r| r.uid == uid))
            .flatten();
        let title = row
            .and_then(|r| r.subject.clone())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "(no subject)".to_string());
        let starred = row.map(|r| r.is_starred).unwrap_or_else(|| {
            messages::get_by_uid(&self.data.db, folder_id, uid)
                .map(|m| m.is_starred)
                .unwrap_or(false)
        });
        self.dock.push_to_focused_leaf(MsgTab {
            folder_id,
            uid,
            title,
            starred,
        });
    }

    fn prune_docs(&mut self) {
        let open: HashSet<(i64, u32)> = self
            .dock
            .iter_all_tabs()
            .map(|(_, t)| (t.folder_id, t.uid))
            .collect();
        self.data.docs.retain(|k, _| open.contains(k));
    }
}

impl Data {
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
        if let Some((fid, uid)) = self.focused {
            if fid != self.folder_id || !self.rows.iter().any(|r| r.uid == uid) {
                self.focused = None;
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

    fn toggle_star(&mut self, tab: &mut MsgTab) {
        let next = !tab.starred;
        if messages::set_star_many_by_uids(&self.db, tab.folder_id, &[tab.uid], next).is_ok() {
            tab.starred = next;
            if tab.folder_id == self.folder_id {
                if let Some(r) = self.rows.iter_mut().find(|r| r.uid == tab.uid) {
                    r.is_starred = next;
                }
            }
            self.status = if next {
                "starred".to_string()
            } else {
                "unstarred".to_string()
            };
        }
    }

    fn mark_unread(&mut self, folder_id: i64, uid: u32) {
        if messages::set_read_many_by_uids(&self.db, folder_id, &[uid], false).is_ok() {
            if folder_id == self.folder_id {
                if let Some(r) = self.rows.iter_mut().find(|r| r.uid == uid) {
                    r.is_read = false;
                }
            }
            self.refresh_pills(folder_id);
            self.status = "marked unread".to_string();
        }
    }
}

const LIST_ROW_H: f32 = 74.0;

impl eframe::App for MailApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.set_visuals(if self.data.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
        ctx.input(|i| {
            for ev in &i.events {
                match ev {
                    egui::Event::Key {
                        key: egui::Key::ArrowDown,
                        pressed: true,
                        ..
                    } => {
                        let ids: Vec<u32> = self.data.rows.iter().map(|r| r.uid).collect();
                        let next = match self
                            .data
                            .focused
                            .and_then(|(_, s)| ids.iter().position(|u| *u == s))
                        {
                            Some(pos) if pos + 1 < ids.len() => Some(ids[pos + 1]),
                            None if !ids.is_empty() => Some(ids[0]),
                            other => other.and_then(|p| ids.get(p).copied()),
                        };
                        if let Some(uid) = next {
                            let fid = self.data.folder_id;
                            self.open_tab(fid, uid);
                        }
                    }
                    egui::Event::Key {
                        key: egui::Key::ArrowUp,
                        pressed: true,
                        ..
                    } => {
                        let ids: Vec<u32> = self.data.rows.iter().map(|r| r.uid).collect();
                        if let Some((_, sel)) = self.data.focused.and_then(|(f, s)| {
                            (f == self.data.folder_id)
                                .then(|| ids.iter().position(|u| *u == s))
                                .flatten()
                                .map(|p| (f, p))
                        }) {
                            if sel > 0 {
                                let fid = self.data.folder_id;
                                self.open_tab(fid, ids[sel - 1]);
                            }
                        } else if !ids.is_empty() {
                            let fid = self.data.folder_id;
                            self.open_tab(fid, ids[0]);
                        }
                    }
                    _ => {}
                }
            }
        });
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(link) = self.data.paint.status.take() {
            self.data.status = link;
        }

        if let Some(id) = self.data.top_bar(ui) {
            self.switch_account(id);
        }
        self.data.status_bar(ui);
        self.data.folder_panel(ui);

        let mut clicked: Option<u32> = None;
        self.data.list_panel(ui, &mut clicked);
        if let Some(uid) = clicked {
            let fid = self.data.folder_id;
            self.open_tab(fid, uid);
        }

        egui::CentralPanel::default().show(ui, |ui| {
            if self.dock.iter_all_tabs().next().is_none() {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new(
                            "Select a message to read it\nTabs can be dragged, split and undocked.",
                        )
                        .weak(),
                    );
                });
                return;
            }
            DockArea::new(&mut self.dock)
                .style(Style::from_egui(ui.style()))
                .show_close_buttons(true)
                .show_add_buttons(false)
                .draggable_tabs(true)
                .show_leaf_close_all_buttons(true)
                .show_inside(
                    ui,
                    &mut Viewer {
                        data: &mut self.data,
                    },
                );
        });
        self.prune_docs();
    }
}

// ---------------------------------------------------------------------------
// Panels
// ---------------------------------------------------------------------------

impl Data {
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
                    .on_disabled_hover_text("The composer lands next — after this polish pass")
                    .clicked()
                {
                    // Unreachable while disabled; kept so enabling is one flag.
                }
                if ui.button("⟳ reload").clicked() {
                    self.reload_folders();
                    self.status = "reloaded from dev database".to_string();
                }
                if ui
                    .button(if self.dark { "☀ light" } else { "🌙 dark" })
                    .clicked()
                {
                    self.dark = !self.dark;
                    self.docs.clear();
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
                                    self.folder_id = id;
                                    self.reload_list();
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
                            let sel = self.focused == Some((self.folder_id, r.uid));
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
    let dark = std::env::args().any(|a| a == "--dark");
    let db = open_dev_db()?;
    let app = MailApp::new(db, dark)?;
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
        assert_eq!(snippet_preview(Some("abcdefghij"), 5), "abcd…");
    }
}
