//! Kerf design tokens (spec/DESIGN.md §1) as teak styles.
//! A 1970s engineering office: paper, ink, non-photo-blue, manila, phosphor.

const teak = @import("teak");

pub const Color = [4]f32;

fn hex(comptime rgb: u24) Color {
    return .{
        @as(f32, @floatFromInt((rgb >> 16) & 0xff)) / 255.0,
        @as(f32, @floatFromInt((rgb >> 8) & 0xff)) / 255.0,
        @as(f32, @floatFromInt(rgb & 0xff)) / 255.0,
        1.0,
    };
}

pub const paper = hex(0xF2EFE6);
pub const paper2 = hex(0xE9E5D8);
pub const vellum = hex(0xFBFAF5);
pub const ink = hex(0x1A1A1A);
pub const ink2 = hex(0x55524B);
pub const grid = hex(0xA9C1DD);
pub const grid2 = hex(0xD3E0EE);
pub const blue = hex(0x1D4E9E);
pub const red = hex(0xC8102E);
pub const amber = hex(0xD98E04);
pub const green = hex(0x2E7D32);
pub const manila = hex(0xE9D9A6);
pub const term_bg = hex(0x0E120E);
pub const term_fg = hex(0x5CF27A);
pub const clear: Color = .{ 0, 0, 0, 0 };

/// Selection tint of a table row / hovered row.
pub const row_hover: Color = .{ 0.10, 0.10, 0.10, 0.06 };

// ── Type ───────────────────────────────────────────────────────────
// Sizes per DESIGN §1: 11px field labels (UPPERCASE, tracked), 13px body,
// 15px panel headings, 22px wordmark. Plex Mono Regular/Medium/Bold.

pub const body: teak.FontSpec = .{ .size_px = 13, .family = .mono };
pub const label: teak.FontSpec = .{ .size_px = 11, .family = .mono, .weight = .medium, .letter_spacing = 0.9 };
pub const button_font: teak.FontSpec = .{ .size_px = 12, .family = .mono, .weight = .medium };
pub const heading: teak.FontSpec = .{ .size_px = 15, .family = .mono, .weight = .bold };
pub const wordmark: teak.FontSpec = .{ .size_px = 22, .family = .mono, .weight = .bold, .letter_spacing = 4.4 };
pub const small: teak.FontSpec = .{ .size_px = 11, .family = .mono };
pub const bold: teak.FontSpec = .{ .size_px = 13, .family = .mono, .weight = .bold };

/// Advance of one Plex Mono column (0.6 em) at `size_px`.
pub fn colWidth(font: teak.FontSpec) f32 {
    return font.size_px * 0.6 + font.letter_spacing;
}

// ── Styles ─────────────────────────────────────────────────────────

/// Ink-on-paper key: 1px ink border, inverts on hover, drops 1px on press.
pub const button: teak.ButtonStyle = .{
    .bg = paper,
    .hover_bg = ink,
    .press_bg = ink,
    .fg = ink,
    .hover_fg = paper,
    .press_fg = paper,
    .press_offset_y = 1,
    .border = ink,
    .label_align = .center,
    .height = 28,
    .min_width = 0,
    .h_padding = 12,
    .disabled_bg = paper2,
    .disabled_fg = ink2,
};

pub const button_primary: teak.ButtonStyle = blk: {
    var b = button;
    b.bg = blue;
    b.fg = paper;
    b.hover_bg = ink;
    b.press_bg = ink;
    b.hover_fg = paper;
    b.press_fg = paper;
    b.border = blue;
    break :blk b;
};

pub const button_danger: teak.ButtonStyle = blk: {
    var b = button;
    b.fg = red;
    b.border = red;
    b.hover_bg = red;
    b.hover_fg = paper;
    b.press_bg = red;
    b.press_fg = paper;
    break :blk b;
};

/// Header-bar key (dark bar).
pub const button_header: teak.ButtonStyle = blk: {
    var b = button;
    b.bg = ink;
    b.fg = paper;
    b.border = paper;
    b.hover_bg = paper;
    b.hover_fg = ink;
    b.press_bg = paper;
    b.press_fg = ink;
    break :blk b;
};

/// Flat tab label; the 3px bar under the active one is drawn by the view.
pub const button_tab: teak.ButtonStyle = .{
    .bg = clear,
    .hover_bg = paper2,
    .press_bg = paper2,
    .fg = ink,
    .label_align = .center,
    .height = 26,
    .min_width = 0,
    .h_padding = 8,
};

/// Small text link-like key inside tables.
pub const button_flat: teak.ButtonStyle = blk: {
    var b = button_tab;
    b.height = 22;
    b.h_padding = 4;
    b.label_align = .start;
    break :blk b;
};

/// Typed-form field: label above (drawn by the view), 1px bottom rule, 2px blue when focused.
pub const field: teak.TextInputStyle = .{
    .variant = .underline,
    .fg = ink,
    .border = ink,
    .focus_border = blue,
    .cursor = blue,
    .selection_bg = .{ blue[0], blue[1], blue[2], 0.25 },
    .disabled_fg = ink2,
    .disabled_bg = paper,
    .disabled_border = ink2,
    .height = 28,
    .flex = 0,
};

/// Boxed console input (terminal prompt).
pub const prompt: teak.TextInputStyle = .{
    .variant = .boxed,
    .bg = term_bg,
    .fg = term_fg,
    .border = ink,
    .focus_border = term_fg,
    .cursor = term_fg,
    .selection_bg = .{ term_fg[0], term_fg[1], term_fg[2], 0.3 },
    .border_width = 1,
    .height = 32,
    .flex = 0,
};

pub const theme: teak.Theme = .{
    .palette = .{
        .bg = paper,
        .bg_panel = paper,
        .bg_sunken = paper,
        .bg_raised = paper,
        .bg_hover = ink,
        .bg_press = ink,
        .fg = ink,
        .fg_muted = ink2,
        .accent = blue,
        .danger = red,
        .border = ink,
    },
    .typography = .{ .body = body, .mono = body, .small = small, .heading = heading },
    .text_color = ink,
    .heading_color = ink,
    .muted_color = ink2,
    .danger_color = red,
    .panel_bg = paper,
    .button = button,
    .text_input = prompt,
    .divider = .{ .thickness = 1, .color = ink },
    .card = .{ .padding = 8, .gap = 4, .bg = paper, .border = ink, .align_cross = .stretch },
    .field = field,
};
