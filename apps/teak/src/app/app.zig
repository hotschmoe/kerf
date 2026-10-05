//! The App contract for `teak.run` / `teak.Runtime`: Model, Msg, update, view
//! and the optional hooks.

const std = @import("std");
const teak = @import("teak");
const model = @import("model.zig");
const upd = @import("update.zig");
const vw = @import("view.zig");
const th = @import("theme.zig");
const editor = @import("editor.zig");

pub const Model = model.Model;
pub const Msg = model.Msg;
pub const update = upd.update;

pub fn view(m: *const Model, cb: anytype) void {
    vw.view(m, cb);
}

pub fn themeFor(_: *const Model) teak.Theme {
    return th.theme;
}

pub fn windowTitle(m: *const Model) ?[]const u8 {
    _ = m;
    return "KERF - DETAIL WORKSTATION";
}

// ── input hooks ────────────────────────────────────────────────────

pub fn keyCharMsg(m: *const Model, c: u8) ?Msg {
    if (m.focus == .none) return null;
    return .{ .char = c };
}

pub fn keySpecialMsg(m: *const Model, k: teak.SpecialKey) ?Msg {
    if (m.focus == .none and k != .escape and k != .ctrl_z) return null;
    return .{ .key = k };
}

pub fn submitMsg(m: *const Model) ?Msg {
    if (m.focus == .none) return null;
    return .submit;
}

pub fn focusedMsg(m: *const Model) ?Msg {
    if (m.focus == .none) return null;
    return .{ .focus = m.focus };
}

pub fn keyNeedsClipboard(k: teak.SpecialKey) bool {
    return teak.keyNeedsClipboard(k);
}

/// Cut / copy / paste of the focused field through the host clipboard.
pub fn handleClipboard(m: *Model, k: teak.SpecialKey, cb: teak.Clipboard) void {
    const ed = upd.focusedEditor(m) orelse return;
    switch (k) {
        .ctrl_c => if (ed.hasSelection()) cb.write(ed.selection()),
        .ctrl_x => if (ed.hasSelection()) {
            cb.write(ed.selection());
            ed.backspace();
        },
        .ctrl_v => ed.insert(cb.read()),
        else => {},
    }
}

pub fn canvasMsg(_: *const Model, ev: teak.CanvasEvent) ?Msg {
    return .{ .canvas = ev };
}

pub fn scrollMsg(_: *const Model, id: u32, _: f32, dy: f32) ?Msg {
    return switch (id) {
        model.CONSOLE_SCROLL => .{ .console_scroll_by = dy },
        model.INSPECTOR_SCROLL => .{ .insp_scroll_by = dy },
        else => null,
    };
}

pub fn scrollLayoutMsg(_: *const Model, id: u32, vw_: f32, vh: f32, cw: f32, ch: f32) ?Msg {
    _ = vw_;
    _ = cw;
    return switch (id) {
        model.CONSOLE_SCROLL => .{ .console_extent = .{ vh, ch } },
        model.INSPECTOR_SCROLL => .{ .insp_extent = .{ vh, ch } },
        else => null,
    };
}

pub fn subscribe(_: *const Model) []const teak.Sub(Msg) {
    const subs = [_]teak.Sub(Msg){.{ .every = .{ .interval_ms = @intCast(model.TICK_MS), .msg = .tick } }};
    return &subs;
}

pub fn effects(m: *const Model) []const teak.Effect {
    return m.fx.list();
}

pub fn effectMsg(_: *const Model, r: teak.EffectResult) ?Msg {
    return .{ .fx = r };
}

pub fn resources(m: *const Model) []const teak.Resource {
    return m.res[0..m.res_len];
}

pub fn windowMsg(_: *const Model, w: f32, h: f32) ?Msg {
    return .{ .window = .{ w, h } };
}
