//! Reader document + tab UI for maildesk.
//!
//! Toolkit code only: layout, widgets, theme lookups. All decisions (what
//! is safe, dark rewrite, fit) already happened in `mailcore`; this module
//! parses `reader::body()` output via `render` and paints it via `paint`.
//! Actions the tab cannot take itself (flag writes, doc rebuilds) come back
//! as [`ReaderAction`] for the app to handle after the dock pass.

use chrono::{DateTime, Local};
use egui_extras::{Column, TableBuilder};
use mailcore::feed;
use mailcore::html::reader::{self, Palette};
use mailcore::html::{dropped_summary, has_own_colors, inline_cid_images, sanitize_report};
use mailcore::store::messages;
use mailcore::{Db, Result};

use crate::{paint, render};

pub struct ReaderDoc {
    pub subject: String,
    pub from_name: String,
    pub from_addr: String,
    pub to_line: String,
    pub cc_line: String,
    pub date: String,
    pub is_html: bool,
    pub plain: String,
    pub had_remote: bool,
    pub missing_inline: usize,
    pub paint: reader::Paint,
    pub blocks: Vec<render::Block>,
    pub attachments: Vec<AttachmentView>,
    /// Sanitiser drop summary (`""` when faithful): tag names only, shown
    /// in the status line so "simplified" rendering is never silent.
    pub dropped: String,
}

pub struct AttachmentView {
    pub name: String,
    pub size_text: String,
    pub cached: bool,
}

pub fn build_reader(
    db: &Db,
    folder_id: i64,
    uid: u32,
    allow_remote: bool,
    dark: bool,
) -> Result<ReaderDoc> {
    let m = messages::get_by_uid(db, folder_id, uid)?;
    // Fidelity diagnostics: tag names and counts only, never content.
    let report = sanitize_report(m.body_html.as_deref().unwrap_or_default(), allow_remote);
    log::debug!(
        "reader folder={folder_id} uid={uid} tags={} dropped={:?} dattrs={} remote_imgs={} local_imgs={} links={}",
        report.tags_seen,
        report.dropped_tags,
        report.dropped_attrs,
        report.remote_images,
        report.local_images,
        report.links,
    );
    let dropped = dropped_summary(&report);
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
        subject: m.subject.clone().unwrap_or_default(),
        from_name: m.from_name.clone().unwrap_or_default(),
        from_addr: m.from_addr.clone().unwrap_or_default(),
        to_line: m.to_addrs.join(", "),
        cc_line: m.cc_addrs.join(", "),
        date: m.date.clone().unwrap_or_default(),
        is_html,
        plain,
        had_remote,
        missing_inline,
        paint,
        blocks,
        attachments,
        dropped,
    })
}

/// Reader header date: `1 Sep 2026, 10:00` in local time, raw fallback.
fn full_date(raw: &str) -> String {
    raw.parse::<DateTime<chrono::FixedOffset>>().map_or_else(
        |_| raw.to_string(),
        |dt| {
            dt.with_timezone(&Local)
                .format("%e %b %Y, %H:%M")
                .to_string()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        },
    )
}

pub fn theme_palette(dark: bool) -> Palette {
    if dark {
        Palette::from_css("#1b1d21", "#e8e8e8", "#7ab3ff", "#9aa0a6", "#3a3d42")
    } else {
        Palette::from_css("#ffffff", "#202124", "#1a5fd0", "#5f6368", "#d0d4da")
    }
}

#[derive(Default)]
pub enum ReaderAction {
    #[default]
    None,
    ToggleStar,
    MarkUnread,
    ShowRemoteOnce,
}

/// Paint one reader tab. Returns the action the app must take afterwards
/// (flag writes and doc rebuilds run after the paint pass).
pub fn show(
    ui: &mut egui::Ui,
    doc: &ReaderDoc,
    remote_allowed: bool,
    is_starred: bool,
    pst: &mut paint::PaintState,
) -> ReaderAction {
    let mut action = ReaderAction::None;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            header_card(ui, doc, remote_allowed, is_starred, &mut action);
            ui.add_space(8.0);
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
                paint::paint_blocks(ui, &doc.blocks, &colors, pst);
            }
        });
    action
}

fn header_card(
    ui: &mut egui::Ui,
    doc: &ReaderDoc,
    remote_allowed: bool,
    is_starred: bool,
    action: &mut ReaderAction,
) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.heading(if doc.subject.is_empty() {
                "(no subject)"
            } else {
                &doc.subject
            });
        });
        ui.add_space(4.0);
        header_rows(ui, doc);
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(if is_starred {
                    "★ starred"
                } else {
                    "☆ star"
                })
                .clicked()
            {
                *action = ReaderAction::ToggleStar;
            }
            if ui.button("✉ mark unread").clicked() {
                *action = ReaderAction::MarkUnread;
            }
            ui.add_enabled(false, egui::Button::new("↩ reply"))
                .on_disabled_hover_text("Replies need the composer, which lands next");
        });
        banners(ui, doc, remote_allowed, action);
        attachments_table(ui, doc);
    });
}

fn header_rows(ui: &mut egui::Ui, doc: &ReaderDoc) {
    let from = if doc.from_name.is_empty() {
        doc.from_addr.clone()
    } else {
        format!("{} <{}>", doc.from_name, doc.from_addr)
    };
    let rows: Vec<(&str, String)> = [
        ("From", from),
        ("To", doc.to_line.clone()),
        ("Cc", doc.cc_line.clone()),
        ("Date", full_date(&doc.date)),
    ]
    .into_iter()
    .filter(|(_, v)| !v.is_empty())
    .collect();
    if rows.is_empty() {
        return;
    }
    // A plain Grid, not StripBuilder: strip cells take the remainder of the
    // available height, which is infinite inside a ScrollArea — the first
    // strip would swallow the whole pane and push everything below it
    // off-screen. Grid rows size to their content instead.
    egui::Grid::new("headers")
        .num_columns(2)
        .spacing([8.0, 2.0])
        .show(ui, |ui| {
            for (label, value) in &rows {
                ui.label(egui::RichText::new(*label).weak());
                ui.label(value);
                ui.end_row();
            }
        });
}

fn banners(ui: &mut egui::Ui, doc: &ReaderDoc, remote_allowed: bool, action: &mut ReaderAction) {
    if doc.is_html && doc.had_remote && !remote_allowed {
        ui.add_space(6.0);
        egui::Frame::group(ui.style())
            .fill(ui.visuals().warn_fg_color.gamma_multiply(0.12))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("🖼 Remote images blocked (tracking protection).");
                    if ui.button("Show once").clicked() {
                        *action = ReaderAction::ShowRemoteOnce;
                    }
                });
            });
    }
    if doc.missing_inline > 0 {
        ui.add_space(6.0);
        ui.label(format!(
            "⚠ {} inline image(s) not downloaded yet.",
            doc.missing_inline
        ));
    }
}

fn attachments_table(ui: &mut egui::Ui, doc: &ReaderDoc) {
    if doc.attachments.is_empty() {
        return;
    }
    ui.add_space(6.0);
    ui.collapsing(format!("Attachments ({})", doc.attachments.len()), |ui| {
        TableBuilder::new(ui)
            .striped(true)
            .column(Column::remainder().at_least(140.0))
            .column(Column::auto())
            .column(Column::auto())
            .header(22.0, |mut header| {
                header.col(|ui| {
                    ui.label(egui::RichText::new("Name").weak());
                });
                header.col(|ui| {
                    ui.label(egui::RichText::new("Size").weak());
                });
                header.col(|ui| {
                    ui.label(egui::RichText::new("State").weak());
                });
            })
            .body(|mut body| {
                for a in &doc.attachments {
                    body.row(22.0, |mut row| {
                        row.col(|ui| {
                            ui.label(format!("📎 {}", a.name));
                        });
                        row.col(|ui| {
                            ui.label(&a.size_text);
                        });
                        row.col(|ui| {
                            ui.label(if a.cached { "cached" } else { "on server" });
                        });
                    });
                }
            });
    });
}
