//! The 1970s engineering-office look (spec/DESIGN.md): tokens, fonts, a hard-edged egui style,
//! and the handful of hand-painted widgets (buttons, tabs, typed-form fields, stamps, rules)
//! that replace egui's default chrome.

use egui::epaint::{Shadow, StrokeKind};
use egui::{
    Color32, Context, FontData, FontDefinitions, FontFamily, FontId, Galley, Pos2, Rect, Response, Sense, Shape,
    Stroke, Ui, Vec2, text::LayoutJob,
};
use std::sync::Arc;

pub const PAPER: Color32 = Color32::from_rgb(0xF2, 0xEF, 0xE6);
pub const PAPER2: Color32 = Color32::from_rgb(0xE9, 0xE5, 0xD8);
pub const VELLUM: Color32 = Color32::from_rgb(0xFB, 0xFA, 0xF5);
pub const INK: Color32 = Color32::from_rgb(0x1A, 0x1A, 0x1A);
pub const INK2: Color32 = Color32::from_rgb(0x55, 0x52, 0x4B);
pub const GRID: Color32 = Color32::from_rgb(0xA9, 0xC1, 0xDD);
pub const GRID2: Color32 = Color32::from_rgb(0xD3, 0xE0, 0xEE);
pub const BLUE: Color32 = Color32::from_rgb(0x1D, 0x4E, 0x9E);
pub const RED: Color32 = Color32::from_rgb(0xC8, 0x10, 0x2E);
pub const AMBER: Color32 = Color32::from_rgb(0xD9, 0x8E, 0x04);
pub const GREEN: Color32 = Color32::from_rgb(0x2E, 0x7D, 0x32);
pub const MANILA: Color32 = Color32::from_rgb(0xE9, 0xD9, 0xA6);
pub const TERM_BG: Color32 = Color32::from_rgb(0x0E, 0x12, 0x0E);
pub const TERM_FG: Color32 = Color32::from_rgb(0x5C, 0xF2, 0x7A);

pub fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}
pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}
pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

pub fn install(ctx: &Context) {
    let mut defs = FontDefinitions::empty();
    let faces: [(&str, &'static [u8]); 3] = [
        ("plex-regular", include_bytes!("../assets/IBMPlexMono-Regular.subset.ttf")),
        ("plex-medium", include_bytes!("../assets/IBMPlexMono-Medium.subset.ttf")),
        ("plex-bold", include_bytes!("../assets/IBMPlexMono-Bold.subset.ttf")),
    ];
    for (name, bytes) in faces {
        defs.font_data.insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    }
    for fam in [FontFamily::Proportional, FontFamily::Monospace] {
        defs.families.entry(fam).or_default().push("plex-regular".to_owned());
    }
    defs.families.insert(FontFamily::Name("medium".into()), vec!["plex-medium".to_owned()]);
    defs.families.insert(FontFamily::Name("bold".into()), vec!["plex-bold".to_owned()]);
    ctx.set_fonts(defs);

    ctx.all_styles_mut(|style| {
        use egui::TextStyle::*;
        style.text_styles = [
            (Small, regular(11.0)),
            (Body, regular(13.0)),
            (Button, medium(12.0)),
            (Heading, bold(15.0)),
            (Monospace, regular(13.0)),
        ]
        .into();
        let v = &mut style.visuals;
        v.dark_mode = false;
        v.override_text_color = Some(INK);
        v.panel_fill = PAPER;
        v.window_fill = PAPER;
        v.extreme_bg_color = VELLUM;
        v.faint_bg_color = PAPER2;
        v.code_bg_color = PAPER2;
        v.window_corner_radius = 0.into();
        v.menu_corner_radius = 0.into();
        v.window_stroke = Stroke::new(1.0, INK);
        let hard = Shadow { offset: [2, 2], blur: 0, spread: 0, color: INK };
        v.window_shadow = hard;
        v.popup_shadow = hard;
        v.hyperlink_color = BLUE;
        v.warn_fg_color = AMBER;
        v.error_fg_color = RED;
        v.selection.bg_fill = BLUE.gamma_multiply(0.35);
        v.selection.stroke = Stroke::new(1.0, BLUE);
        let w = &mut v.widgets;
        for wv in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
            wv.corner_radius = 0.into();
            wv.expansion = 0.0;
        }
        w.noninteractive.bg_fill = PAPER;
        w.noninteractive.weak_bg_fill = PAPER;
        w.noninteractive.bg_stroke = Stroke::new(1.0, INK);
        w.noninteractive.fg_stroke = Stroke::new(1.0, INK);
        w.inactive.bg_fill = PAPER;
        w.inactive.weak_bg_fill = PAPER;
        w.inactive.bg_stroke = Stroke::new(1.0, INK);
        w.inactive.fg_stroke = Stroke::new(1.0, INK);
        w.hovered.bg_fill = INK;
        w.hovered.weak_bg_fill = INK;
        w.hovered.bg_stroke = Stroke::new(1.0, INK);
        w.hovered.fg_stroke = Stroke::new(1.0, PAPER);
        w.active.bg_fill = INK;
        w.active.weak_bg_fill = INK;
        w.active.bg_stroke = Stroke::new(1.0, INK);
        w.active.fg_stroke = Stroke::new(1.0, PAPER);
        w.open.bg_fill = PAPER2;
        w.open.weak_bg_fill = PAPER2;
        w.open.bg_stroke = Stroke::new(1.0, INK);
        w.open.fg_stroke = Stroke::new(1.0, INK);
        v.striped = false;
        v.text_cursor.stroke = Stroke::new(2.0, BLUE);

        let s = &mut style.spacing;
        s.item_spacing = Vec2::new(8.0, 6.0);
        s.button_padding = Vec2::new(10.0, 5.0);
        s.interact_size = Vec2::new(28.0, 28.0);
        s.window_margin = 8.into();
        s.menu_margin = 4.into();
        s.indent = 12.0;
        s.scroll.bar_width = 8.0;
        s.scroll.floating = false;
        s.scroll.foreground_color = false;
        style.interaction.selectable_labels = false;
    });
}

// ------------------------------------------------------------------ text helpers

pub fn spaced(text: impl Into<String>, font: FontId, color: Color32, spacing: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.append(
        &text.into(),
        0.0,
        egui::TextFormat { font_id: font, color, extra_letter_spacing: spacing, ..Default::default() },
    );
    job
}

pub fn galley(ui: &Ui, text: impl Into<String>, font: FontId, color: Color32, spacing: f32) -> Arc<Galley> {
    ui.painter().layout_job(spaced(text, font, color, spacing))
}

/// Field label: 11px UPPERCASE ink-2, letter-spacing 0.08em.
pub fn label_caps(ui: &mut Ui, text: &str) {
    let g = galley(ui, text.to_uppercase(), medium(11.0), INK2, 0.9);
    let (rect, _) = ui.allocate_exact_size(g.size(), Sense::hover());
    ui.painter().galley(rect.min, g, INK2);
}

pub fn heading(ui: &mut Ui, text: &str) {
    let g = galley(ui, text.to_uppercase(), bold(15.0), INK, 0.4);
    let (rect, _) = ui.allocate_exact_size(g.size(), Sense::hover());
    ui.painter().galley(rect.min, g, INK);
}

pub fn text(ui: &mut Ui, text: impl Into<String>, color: Color32) -> Response {
    let g = galley(ui, text, regular(13.0), color, 0.0);
    let (rect, r) = ui.allocate_exact_size(g.size(), Sense::hover());
    ui.painter().galley(rect.min, g, color);
    r
}

pub fn wrapped(ui: &mut Ui, text: &str, font: FontId, color: Color32) -> Response {
    let mut job = spaced(text, font, color, 0.0);
    job.wrap.max_width = ui.available_width();
    let g = ui.painter().layout_job(job);
    let (rect, r) = ui.allocate_exact_size(g.size(), Sense::hover());
    ui.painter().galley(rect.min, g, color);
    r
}

// ------------------------------------------------------------------ rules

pub fn hrule(ui: &mut Ui, weight: f32) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, weight), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, INK);
}

pub fn hline_at(ui: &Ui, rect: Rect, y: f32, weight: f32, color: Color32) {
    ui.painter().rect_filled(Rect::from_min_size(Pos2::new(rect.left(), y), Vec2::new(rect.width(), weight)), 0.0, color);
}

// ------------------------------------------------------------------ buttons

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Normal,
    Primary,
    #[allow(dead_code)]
    Danger,
}

pub fn button(ui: &mut Ui, label: &str) -> Response {
    button_k(ui, label, Kind::Normal, true)
}

pub fn button_k(ui: &mut Ui, label: &str, kind: Kind, enabled: bool) -> Response {
    let label = label.to_uppercase();
    let font = medium(12.0);
    let measure = galley(ui, &label, font.clone(), INK, 0.5);
    let size = Vec2::new((measure.size().x + 20.0).max(28.0), 28.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let hovered = enabled && resp.hovered();
    let pressed = enabled && resp.is_pointer_button_down_on();
    let (fill, fg, border) = match (kind, hovered) {
        (_, false) if !enabled => (PAPER, INK2.gamma_multiply(0.6), INK2.gamma_multiply(0.6)),
        (Kind::Primary, false) => (BLUE, PAPER, BLUE),
        (Kind::Primary, true) => (INK, PAPER, INK),
        (Kind::Danger, false) => (PAPER, RED, RED),
        (Kind::Danger, true) => (RED, PAPER, RED),
        (Kind::Normal, false) => (PAPER, INK, INK),
        (Kind::Normal, true) => (INK, PAPER, INK),
    };
    let rect = if pressed { rect.translate(Vec2::new(0.0, 1.0)) } else { rect };
    let p = ui.painter();
    p.rect_filled(rect, 0.0, fill);
    p.rect_stroke(rect, 0.0, Stroke::new(1.0, border), StrokeKind::Inside);
    let g = galley(ui, label, font, fg, 0.5);
    let pos = rect.center() - g.size() / 2.0;
    ui.painter().galley(pos, g, fg);
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Bracketed tab: `[SECTION A]`; the active tab is bold with a 3px blue bar.
pub fn tab(ui: &mut Ui, label: &str, active: bool) -> Response {
    let txt = format!("[{}]", label.to_uppercase());
    let font = if active { bold(12.0) } else { medium(12.0) };
    let color = if active { INK } else { INK2 };
    let g = galley(ui, &txt, font.clone(), color, 0.4);
    let size = Vec2::new(g.size().x + 14.0, 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hovered = resp.hovered();
    let color = if hovered && !active { INK } else { color };
    let g = galley(ui, &txt, font, color, 0.4);
    let p = ui.painter();
    p.galley(Pos2::new(rect.left() + 7.0, rect.center().y - g.size().y / 2.0 - 1.0), g, color);
    if active {
        p.rect_filled(Rect::from_min_max(Pos2::new(rect.left() + 4.0, rect.bottom() - 3.0), Pos2::new(rect.right() - 4.0, rect.bottom())), 0.0, BLUE);
    }
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Small text-only button used in viewport toolbars: `FIT`, `+`, `-`.
pub fn small_button(ui: &mut Ui, label: &str, active: bool) -> Response {
    let g = galley(ui, label.to_uppercase(), medium(11.0), INK, 0.5);
    let size = Vec2::new((g.size().x + 14.0).max(24.0), 22.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hovered = resp.hovered();
    let (fill, fg) = if hovered || active { (INK, PAPER) } else { (PAPER, INK) };
    ui.painter().rect_filled(rect, 0.0, fill);
    ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.0, INK), StrokeKind::Inside);
    let g = galley(ui, label.to_uppercase(), medium(11.0), fg, 0.5);
    ui.painter().galley(rect.center() - g.size() / 2.0, g, fg);
    resp
}

// ------------------------------------------------------------------ fields

pub fn field_cell(ui: &mut Ui, value: &mut String, hint: &str, password: bool, multiline: Option<usize>) -> Response {
    let width = ui.available_width();
    let mut te = match multiline {
        Some(rows) => egui::TextEdit::multiline(value).desired_rows(rows),
        None => egui::TextEdit::singleline(value),
    };
    te = te
        .frame(egui::Frame::NONE)
        .desired_width(width)
        .font(regular(13.0))
        .margin(egui::Margin::symmetric(0, 3))
        .hint_text(egui::RichText::new(hint).color(INK2.gamma_multiply(0.7)))
        .password(password)
        .text_color(INK);
    let resp = ui.add(te);
    let focused = resp.has_focus();
    let rect = resp.rect;
    if focused {
        ui.painter().rect_filled(Rect::from_min_size(Pos2::new(rect.left(), rect.bottom() - 1.0), Vec2::new(rect.width(), 2.0)), 0.0, BLUE);
    } else {
        ui.painter().rect_filled(Rect::from_min_size(Pos2::new(rect.left(), rect.bottom() - 1.0), Vec2::new(rect.width(), 1.0)), 0.0, INK);
    }
    resp
}

// ------------------------------------------------------------------ stamps

/// Rubber-stamp label rotated -3 degrees with a 1.5px outline.
pub fn stamp(ui: &mut Ui, label: &str, color: Color32) -> Response {
    let font = bold(11.0);
    let g = galley(ui, label.to_uppercase(), font, color, 1.0);
    let pad = Vec2::new(5.0, 2.0);
    let size = g.size() + pad * 2.0;
    let (rect, resp) = ui.allocate_exact_size(size + Vec2::new(4.0, 6.0), Sense::click());
    let c = rect.center();
    let ang = -3.0_f32.to_radians();
    let (s, co) = ang.sin_cos();
    let rot = |p: Vec2| Pos2::new(c.x + p.x * co - p.y * s, c.y + p.x * s + p.y * co);
    let h = size / 2.0;
    let pts = vec![rot(Vec2::new(-h.x, -h.y)), rot(Vec2::new(h.x, -h.y)), rot(Vec2::new(h.x, h.y)), rot(Vec2::new(-h.x, h.y))];
    let painter = ui.painter();
    painter.add(Shape::closed_line(pts, Stroke::new(1.5, color)));
    let top_left = rot(Vec2::new(-g.size().x / 2.0, -g.size().y / 2.0));
    let mut ts = egui::epaint::TextShape::new(top_left, g, color);
    ts.angle = ang;
    painter.add(ts);
    resp
}

/// Square check box (typed-form style). Returns true when toggled.
pub fn check_box(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let g = galley(ui, label.to_uppercase(), medium(11.0), INK, 0.5);
    let size = Vec2::new(18.0 + g.size().x + 6.0, 20.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let b = Rect::from_min_size(Pos2::new(rect.left(), rect.center().y - 6.0), Vec2::splat(12.0));
    ui.painter().rect_filled(b, 0.0, VELLUM);
    ui.painter().rect_stroke(b, 0.0, Stroke::new(1.0, INK), StrokeKind::Inside);
    if *on {
        ui.painter().rect_filled(b.shrink(3.0), 0.0, INK);
    }
    ui.painter().galley(Pos2::new(rect.left() + 18.0, rect.center().y - g.size().y / 2.0), g, INK);
    if resp.clicked() {
        *on = !*on;
    }
    resp
}

pub fn shadowed_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(PAPER)
        .stroke(Stroke::new(1.0, INK))
        .inner_margin(8)
        .shadow(Shadow { offset: [2, 2], blur: 0, spread: 0, color: INK })
}

