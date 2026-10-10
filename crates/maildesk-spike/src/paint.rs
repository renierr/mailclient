//! egui painter for the parsed blocks (Spike A, throwaway).
//!
//! Toolkit code only: layout, widgets, theme lookups. All decisions (what
//! is safe, what counts as colored, dark rewrite) already happened in
//! `mailcore`. Link clicks never leave the app: they land in
//! `PaintState.status` (no browser popups on a shared machine).

use std::collections::HashMap;

use mailcore::html::reader::{Palette, Rgb};

use crate::render::{decode_data_uri, Align, Block, Inline};

pub struct Colors {
    pub ink: egui::Color32,
    pub link: egui::Color32,
    pub quote: egui::Color32,
    pub rule: egui::Color32,
    pub code_bg: egui::Color32,
}

impl Colors {
    pub fn from_palette(p: &Palette) -> Self {
        let c = |rgb: Rgb| {
            let v = rgb.0;
            egui::Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
        };
        Self {
            ink: c(p.ink),
            link: c(p.link),
            quote: c(p.quote),
            rule: c(p.rule),
            code_bg: egui::Color32::from_gray(128).linear_multiply(0.12),
        }
    }

    /// Theme paint: no mail colours, take the toolkit's.
    pub fn themed(ui: &egui::Ui) -> Self {
        let v = ui.visuals();
        Self {
            ink: v.text_color(),
            link: v.hyperlink_color,
            quote: v.weak_text_color(),
            rule: v.widgets.noninteractive.bg_stroke.color,
            code_bg: v.code_bg_color,
        }
    }
}

pub fn parse_hex(s: &str) -> Option<egui::Color32> {
    Rgb::parse(s).map(|rgb| {
        let v = rgb.0;
        egui::Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    })
}

pub struct PaintState {
    textures: HashMap<String, egui::TextureHandle>,
    next_id: u64,
    /// Last link click (`open: <url>`). The spike never opens a browser.
    pub status: Option<String>,
}

impl PaintState {
    pub fn new() -> Self {
        Self {
            textures: HashMap::new(),
            next_id: 0,
            status: None,
        }
    }

    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }
}

pub const BASE_FONT: f32 = 14.0;

fn texture_for(ui: &egui::Ui, st: &mut PaintState, src: &str) -> Option<egui::TextureHandle> {
    // Cache key: length + prefix (sources can be megabytes of base64).
    let key = format!("{}:{}", src.len(), &src[..src.len().min(64)]);
    if let Some(h) = st.textures.get(&key) {
        return Some(h.clone());
    }
    let (_mime, bytes) = decode_data_uri(src)?;
    let img = image::load_from_memory(&bytes).ok()?;
    // Downscale-only cap: `thumbnail` would *upscale* a 1px tracker dot
    // into a 1200px rectangle.
    let img = if img.width() > 1200 || img.height() > 1200 {
        img.thumbnail(1200, 1200)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    if w == 0 || h == 0 {
        return None;
    }
    let pixels = rgba.into_raw();
    let color = egui::ColorImage::from_rgba_unmultiplied([w, h], &pixels);
    let handle = ui
        .ctx()
        .load_texture(key.clone(), color, egui::TextureOptions::LINEAR);
    st.textures.insert(key, handle.clone());
    Some(handle)
}

pub fn paint_blocks(ui: &mut egui::Ui, blocks: &[Block], colors: &Colors, st: &mut PaintState) {
    // Stable widget ids across frames: quotes, grids and tables keep their
    // layout state instead of being re-created every frame (which also
    // trips egui's request_discard perf warning).
    st.next_id = 0;
    for b in blocks {
        paint_block(ui, b, colors, st);
    }
}

fn paint_block(ui: &mut egui::Ui, b: &Block, colors: &Colors, st: &mut PaintState) {
    match b {
        Block::Para { inlines, align, bg } => with_bg(ui, bg.as_deref(), |ui| {
            paint_para(ui, inlines, *align, BASE_FONT, colors, st);
        }),
        Block::Heading {
            level,
            inlines,
            align,
            bg,
        } => {
            let size = [28.0, 23.0, 19.0, 16.5, 15.0, 14.0][(*level).clamp(1, 6) as usize - 1];
            // Headings degrade block children to text already; render bold.
            let mut inlines = inlines.clone();
            for il in &mut inlines {
                if let Inline::Text(_, m, _) = il {
                    m.bold = true;
                }
            }
            with_bg(ui, bg.as_deref(), |ui| {
                paint_para(ui, &inlines, *align, size, colors, st);
            });
            ui.add_space(2.0);
        }
        Block::Quote(inner) => {
            let id = st.id();
            let resp = ui.indent(id, |ui| {
                for c in inner {
                    paint_block(ui, c, colors, st);
                }
            });
            let rect = resp.response.rect;
            let x = rect.left() - 6.0;
            ui.painter()
                .vline(x, rect.y_range(), egui::Stroke::new(3.0, colors.quote));
            ui.add_space(4.0);
        }
        Block::List { ordered, items } => {
            for (i, item) in items.iter().enumerate() {
                let prefix = if *ordered {
                    format!("{}.", i + 1)
                } else {
                    "\u{2022}".to_string()
                };
                ui.horizontal_top(|ui| {
                    ui.label(egui::RichText::new(prefix).color(colors.ink));
                    ui.vertical(|ui| {
                        for c in item {
                            paint_block(ui, c, colors, st);
                        }
                    });
                });
            }
            ui.add_space(4.0);
        }
        Block::Code(text) => {
            egui::Frame::new()
                .fill(colors.code_bg)
                .corner_radius(4.0)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    for line in text.lines() {
                        ui.monospace(egui::RichText::new(line).color(colors.ink));
                    }
                });
            ui.add_space(4.0);
        }
        Block::Table(rows) => {
            let id = st.id();
            let ncols = rows.iter().map(|r| r.cells.len()).max().unwrap_or(1).max(1);
            // Equal-width columns: a grid sizes columns by content, and
            // wrapped text inside a cell shrinks its column to min-content
            // (every word on its own line). Fixing the cell width keeps
            // newsletter columns readable — the deliberate simplification
            // for multi-column layouts (render simplified, not pixel mail).
            let col_w =
                ((ui.available_width() - 12.0 * (ncols as f32 - 1.0)) / ncols as f32).max(60.0);
            egui::Grid::new(id)
                .striped(true)
                .spacing([12.0, 6.0])
                .num_columns(ncols)
                .show(ui, |ui| {
                    for row in rows {
                        for cell in &row.cells {
                            paint_cell(ui, cell, col_w, colors, st);
                        }
                        // Ragged rows: pad to the column count.
                        for _ in row.cells.len()..ncols {
                            ui.label("");
                        }
                        ui.end_row();
                    }
                });
            ui.add_space(4.0);
        }
        Block::Image { src, alt } => {
            paint_image(ui, src, alt, colors, st);
        }
        Block::Hr => {
            ui.separator();
        }
    }
}

/// A designed mail's own background (`bgcolor`, `background-color`): the
/// reason light text on a dark box stays readable.
fn with_bg(ui: &mut egui::Ui, bg: Option<&str>, body: impl FnOnce(&mut egui::Ui)) {
    match bg.and_then(parse_hex) {
        Some(color) => {
            egui::Frame::new()
                .fill(color)
                .corner_radius(4.0)
                .inner_margin(8.0)
                .show(ui, body);
        }
        None => body(ui),
    }
}

fn paint_cell(
    ui: &mut egui::Ui,
    cell: &crate::render::TableCell,
    col_w: f32,
    colors: &Colors,
    st: &mut PaintState,
) {
    // Fixed cell width: the grid column keeps this width instead of
    // collapsing to the wrapped content's minimum.
    ui.allocate_ui_with_layout(
        egui::vec2(col_w, 0.0),
        egui::Layout::top_down(egui::Align::LEFT),
        |ui| {
            ui.set_width(col_w);
            let bg = cell.bg.as_deref().and_then(parse_hex);
            // Header cells render bold.
            let mut paint_one = |ui: &mut egui::Ui, b: &crate::render::Block| {
                if cell.header {
                    with_bold(ui, b, colors, st);
                } else {
                    paint_block(ui, b, colors, st);
                }
            };
            match bg {
                Some(color) => {
                    egui::Frame::new()
                        .fill(color)
                        .inner_margin(4.0)
                        .show(ui, |ui| {
                            for c in &cell.blocks {
                                paint_one(ui, c);
                            }
                        });
                }
                None => {
                    for c in &cell.blocks {
                        paint_one(ui, c);
                    }
                }
            }
        },
    );
}
fn with_bold(ui: &mut egui::Ui, b: &Block, colors: &Colors, st: &mut PaintState) {
    match b {
        Block::Para { inlines, align, bg } => {
            let mut inlines = inlines.clone();
            for il in &mut inlines {
                if let Inline::Text(_, m, _) = il {
                    m.bold = true;
                }
            }
            with_bg(ui, bg.as_deref(), |ui| {
                paint_para(ui, &inlines, *align, BASE_FONT, colors, st);
            });
        }
        _ => paint_block(ui, b, colors, st),
    }
}

fn rich(
    run: &str,
    marks: &crate::render::Marks,
    color: egui::Color32,
    size: f32,
) -> egui::RichText {
    let mut r = egui::RichText::new(run)
        .color(color)
        .size(size + f32::from(marks.size_delta) * 2.0);
    if marks.bold {
        r = r.strong();
    }
    if marks.italic {
        r = r.italics();
    }
    if marks.underline {
        r = r.underline();
    }
    if marks.strike {
        r = r.strikethrough();
    }
    if marks.code {
        r = r.code();
    }
    if marks.raised > 0 {
        r = r.raised().small();
    } else if marks.raised < 0 {
        r = r.small();
    }
    r
}

fn paint_para(
    ui: &mut egui::Ui,
    inlines: &[Inline],
    align: Align,
    size: f32,
    colors: &Colors,
    st: &mut PaintState,
) {
    // `Break` splits the paragraph into wrapped lines.
    let mut lines: Vec<&[Inline]> = Vec::new();
    let mut start = 0;
    for (i, il) in inlines.iter().enumerate() {
        if matches!(il, Inline::Break) {
            lines.push(&inlines[start..i]);
            start = i + 1;
        }
    }
    lines.push(&inlines[start..]);

    let body = |ui: &mut egui::Ui| {
        for line in lines {
            if line.is_empty() {
                continue;
            }
            // NB: `horizontal_wrapped`, not a hand-built
            // `left_to_right(TOP).with_main_wrap(true)` — the latter claims
            // the full remaining height in this context (see layout_tests).
            ui.horizontal_wrapped(|ui| {
                for il in line {
                    match il {
                        Inline::Text(t, m, c) => {
                            let col = c.as_deref().and_then(parse_hex).unwrap_or(colors.ink);
                            ui.label(rich(t, m, col, size));
                        }
                        Inline::Link { href, label, safe } => {
                            if *safe {
                                if ui
                                    .link(
                                        egui::RichText::new(label)
                                            .color(colors.link)
                                            .underline()
                                            .size(size),
                                    )
                                    .clicked()
                                {
                                    st.status = Some(format!("open: {href}"));
                                }
                            } else {
                                ui.label(
                                    egui::RichText::new(format!("{label} (blocked)"))
                                        .color(egui::Color32::DARK_RED)
                                        .size(size),
                                );
                            }
                        }
                        Inline::Break => {}
                    }
                }
            });
        }
    };

    match align {
        Align::Left => body(ui),
        // Right alignment has no wrap-preserving primitive; center is exact,
        // right falls back to left (noted fidelity gap).
        Align::Center => {
            ui.vertical_centered(body);
        }
        Align::Right => body(ui),
    }
    ui.add_space(2.0);
}

fn paint_image(ui: &mut egui::Ui, src: &str, alt: &str, colors: &Colors, st: &mut PaintState) {
    if src.starts_with("data:") {
        match texture_for(ui, st, src) {
            Some(tex) => {
                let max_w = ui.available_width().min(800.0);
                let mut size = tex.size_vec2();
                if size.x > max_w {
                    size *= max_w / size.x;
                }
                ui.image((tex.id(), size));
            }
            None => {
                placeholder(ui, &format!("[image: {alt}] (undecodable)"), colors);
            }
        }
    } else if src.is_empty() {
        placeholder(ui, &format!("[image: {alt}] (missing source)"), colors);
    } else {
        placeholder(ui, &format!("[image: {alt}] (remote blocked)"), colors);
    }
}

fn placeholder(ui: &mut egui::Ui, text: &str, colors: &Colors) {
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, colors.rule))
        .corner_radius(4.0)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).color(colors.quote).italics());
        });
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use crate::render::parse;

    /// Headless layout probe: how tall is each top-level block at 600px
    /// width? No GPU needed — begin/end_pass only lays out.
    fn block_heights(html: &str, width: f32) -> Vec<f32> {
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 800.0),
            )),
            ..Default::default()
        };
        ctx.begin_pass(raw);
        let mut heights = Vec::new();
        {
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
            let mut ui = egui::Ui::new(
                ctx.clone(),
                egui::Id::new("probe"),
                egui::UiBuilder::new().max_rect(screen),
            );
            ui.set_width(width);
            let colors = Colors {
                ink: egui::Color32::BLACK,
                link: egui::Color32::BLUE,
                quote: egui::Color32::GRAY,
                rule: egui::Color32::GRAY,
                code_bg: egui::Color32::LIGHT_GRAY,
            };
            let mut st = PaintState::new();
            for b in parse(html) {
                let y0 = ui.min_rect().bottom();
                paint_block(&mut ui, &b, &colors, &mut st);
                heights.push(ui.min_rect().bottom() - y0);
            }
        }
        let mut out = ctx.end_pass();
        out.textures_delta.clear();
        heights
    }

    #[test]
    fn two_paras_are_compact() {
        let h = block_heights(
            "<p>Hello, this mail has <b>bold</b> text.</p><p>Second para with <a href=\"https://example.com\">link</a>.</p>",
            600.0,
        );
        assert_eq!(h.len(), 2);
        for (i, height) in h.iter().enumerate() {
            assert!(*height < 120.0, "block {i} too tall: {height}");
        }
    }

    #[test]
    fn bisect_wrap_height() {
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 800.0),
            )),
            ..Default::default()
        };
        ctx.begin_pass(raw);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
        let mut ui = egui::Ui::new(
            ctx.clone(),
            egui::Id::new("probe2"),
            egui::UiBuilder::new().max_rect(screen),
        );
        ui.set_width(600.0);
        let y0 = ui.min_rect().bottom();
        ui.label("hello world, one short line");
        let plain = ui.min_rect().bottom() - y0;
        let y1 = ui.min_rect().bottom();
        ui.horizontal_wrapped(|ui| {
            ui.label("hello");
            ui.label("world, one short line");
        });
        let wrapped = ui.min_rect().bottom() - y1;
        let mut out = ctx.end_pass();
        out.textures_delta.clear();
        println!("plain={plain} wrapped={wrapped}");
        assert!(plain < 60.0, "plain label too tall: {plain}");
        assert!(wrapped < 60.0, "wrapped row too tall: {wrapped}");
    }

    #[test]
    fn probe_image_size() {
        let src = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
        let (mime, bytes) = crate::render::decode_data_uri(src).expect("data uri");
        assert_eq!(mime, "image/png");
        let img = image::load_from_memory(&bytes).expect("decode");
        assert_eq!((img.width(), img.height()), (1, 1));
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput::default());
        let mut st = PaintState::new();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
        let ui = egui::Ui::new(
            ctx.clone(),
            egui::Id::new("probe3"),
            egui::UiBuilder::new().max_rect(screen),
        );
        let tex = texture_for(&ui, &mut st, src).expect("texture");
        println!("tex size={:?}", tex.size_vec2());
        assert_eq!(tex.size_vec2(), egui::vec2(1.0, 1.0));
        let mut out = ctx.end_pass();
        out.textures_delta.clear();
    }
}
