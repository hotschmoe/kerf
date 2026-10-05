//! The app's Model and Msg.

const std = @import("std");
const teak = @import("teak");
const alloc = @import("alloc.zig");
const llm = @import("../llm/mod.zig");
const draw = @import("../draw/mod.zig");
const session_mod = @import("session.zig");
const viewport = @import("viewport.zig");
const ident = @import("ident.zig");
const fx = @import("fx.zig");
const cam = @import("cam.zig");
const editor = @import("editor.zig");
const scene3d = @import("scene3d.zig");

pub const Editor = editor.Editor;
pub const MaybeId = ident.MaybeId;

pub const CANVAS_ID: u32 = 1;
pub const MESH_KEY: u32 = 1;
pub const SCENE_ID: u32 = 2;
pub const CONSOLE_SCROLL: u32 = 10;
pub const INSPECTOR_SCROLL: u32 = 11;

pub const Panel = enum { console, view, inspector };
pub const Menu = enum { none, samples, export_, model };
pub const InspTab = enum { parts, notes, diff, diag };
pub const Focus = enum { none, chat, key, note_text, cite_code, cite_section, cite_title };
pub const ExportFmt = enum { dxf, pdf, svg };
pub const Model3 = enum { opus_5_5, sonnet_5_5 };

/// Window-level geometry the view needs for text wrapping etc.
pub const CONSOLE_W: f32 = 360;
pub const INSPECTOR_W: f32 = 320;

pub const Attachment = struct {
    thumb: []u8 = &.{},
    thumb_w: u32 = 0,
    thumb_h: u32 = 0,
    name: [48]u8 = undefined,
    name_len: u8 = 0,
    mime_jpeg: bool = false,
    bytes: []u8 = &.{},
    width: u32 = 0,
    height: u32 = 0,
    orig_width: u32 = 0,
    orig_height: u32 = 0,
};

pub const MAX_ATTACH = 4;
pub const MAX_THUMBS = 16;
pub const THUMB_KEY0: u32 = 100;
pub const ATTACH_KEY0: u32 = 200;
pub const THUMB_W: u32 = 300;

pub const Thumb = struct { rgba: []u8, w: u32, h: u32 };

pub const Sample = struct { id: []const u8, label: []const u8, json: []const u8 };
pub const samples = [_]Sample{
    .{ .id = "truss", .label = "TRUSS-BEARING-CMU", .json = @embedFile("sample_truss_json") },
    .{ .id = "slab", .label = "MONOPOUR-SLAB-DOOR-RECESS", .json = @embedFile("sample_slab_json") },
    .{ .id = "strap", .label = "FLUSH-BEAM-STRAP", .json = @embedFile("sample_strap_json") },
};

pub const Model = struct {
    // ── window ──
    win_w: f32 = 1440,
    win_h: f32 = 900,
    /// Narrow windows (< 900 px) show one panel at a time (DESIGN §2).
    panel: Panel = .view,

    // ── document ──
    doc: *session_mod.Session = undefined,
    ready: bool = false,
    /// Where the current doc came from ("SAMPLE TRUSS", "FILE x.kerf.json", "CLAUDE").
    doc_source: [48]u8 = undefined,
    doc_source_len: u8 = 0,

    // ── viewport ──
    /// 0..nviews-1 = document views, nviews = 3D, nviews+1 = SHEET.
    tab: u8 = 0,
    vp: *viewport.Vp = undefined,
    vp_key: u64 = 0,
    orbit: cam.Orbit = .{},
    mesh: ?*draw.Mesh = null,
    mesh_rev: u32 = 0,
    scene: ?*scene3d.Built = null,
    res: [1 + MAX_THUMBS + MAX_ATTACH]teak.Resource = undefined,
    res_len: usize = 0,
    /// Thumbnails of kerf_render results, in chat order (resource keys THUMB_KEY0 + i).
    thumbs: [MAX_THUMBS]Thumb = undefined,
    n_thumbs: u8 = 0,
    mesh_doc_rev: u32 = std.math.maxInt(u32),
    scene_w: f32 = 0,
    scene_h: f32 = 0,
    drawing_doc_rev: u32 = std.math.maxInt(u32),
    note_ids: []const []const u8 = &.{},
    drawing_view: ident.Id = .{},
    cursor_model: ?[2]f64 = null,

    // ── selection / inspector ──
    sel: MaybeId = .{},
    insp: InspTab = .parts,
    insp_text: []u8 = &.{},
    insp_text_for: ident.Id = .{},
    note_sel: i32 = -1,
    focus: Focus = .none,
    note_ed: Editor = .{},
    cite_code_ed: Editor = .{},
    cite_sec_ed: Editor = .{},
    cite_title_ed: Editor = .{},
    insp_scroll: f32 = 0,
    insp_viewport: f32 = 0,
    insp_content: f32 = 0,

    // ── console ──
    chat: *llm.Chat = undefined,
    chat_ed: Editor = .{},
    key_ed: Editor = .{},
    /// The key in use (the Session borrows this buffer).
    key_buf: [256]u8 = undefined,
    key_len: u8 = 0,
    show_key_card: bool = false,
    model_pick: Model3 = .opus_5_5,
    demo: bool = false,
    /// The Session needs a non-empty key; demo mode uses a placeholder.
    demo_key: bool = false,
    console_scroll: f32 = 0,
    console_viewport: f32 = 0,
    console_content: f32 = 0,
    stick_bottom: bool = true,
    attachments: [MAX_ATTACH]Attachment = undefined,
    n_attach: u8 = 0,
    demo_pending: ?llm.types.HttpResult = null,
    retry_in_ticks: i32 = -1,
    /// "why" lines of designer edits since the last chat turn.
    edit_notes: std.ArrayList([]u8) = .empty,

    // ── chrome ──
    menu: Menu = .none,
    status: [96]u8 = undefined,
    status_len: u8 = 0,
    status_age: u16 = 0,

    // ── effects / time ──
    fx: fx.Queue = .{},
    ticks: u32 = 0,
    wall_base_ms: i64 = 0,
    wall_base_tick: u32 = 0,
    utc_offset_min: i32 = 0,
    /// Startup parameters that need the document to exist first (applied when it loads).
    boot_tab: [8]u8 = undefined,
    boot_tab_len: u8 = 0,
    boot_select: ident.MaybeId = .{},
    boot_insp: InspTab = .parts,
    boot_prompt: ?[]u8 = null,
    api_url: [160]u8 = undefined,
    api_url_len: u8 = 0,

    pub fn init() Model {
        return @import("boot.zig").init();
    }

    pub fn narrow(self: *const Model) bool {
        return self.win_w < 900;
    }

    pub fn nowMs(self: *const Model) i64 {
        return self.wall_base_ms + @as(i64, self.ticks - self.wall_base_tick) * TICK_MS;
    }

    pub fn setStatus(self: *Model, comptime fmt: []const u8, args: anytype) void {
        const s = std.fmt.bufPrint(&self.status, fmt, args) catch self.status[0..0];
        self.status_len = @intCast(s.len);
        self.status_age = 0;
    }

    pub fn statusText(self: *const Model) []const u8 {
        return self.status[0..self.status_len];
    }

    pub fn nViews(self: *const Model) usize {
        return if (self.ready) (self.doc.info.?.views.len) else 0;
    }

    pub const TabKind = union(enum) { view: usize, d3, sheet, none };

    pub fn tabKind(self: *const Model) TabKind {
        const n = self.nViews();
        if (!self.ready) return .none;
        if (self.tab < n) return .{ .view = self.tab };
        if (self.tab == n) return .d3;
        return .sheet;
    }

    pub fn activeViewId(self: *const Model) ?[]const u8 {
        return switch (self.tabKind()) {
            .view => |i| self.doc.info.?.views[i].id,
            else => if (self.nViews() > 0) self.doc.info.?.views[0].id else null,
        };
    }
};

pub const TICK_MS: i64 = 200;

pub const Msg = union(enum) {
    // chrome
    menu: Menu,
    close_menu,
    load_sample: u8,
    open_file,
    save_file,
    export_as: ExportFmt,
    // viewport
    select_tab: u8,
    fit,
    zoom: f32,
    preset3d: cam.Orbit.Preset,
    toggle_grid,
    canvas: teak.CanvasEvent,
    // selection + inspector
    select: ident.Id,
    select_note: u32,
    insp_tab: InspTab,
    insp_scroll_by: f32,
    insp_extent: [2]f32,
    focus: Focus,
    key: teak.SpecialKey,
    char: u8,
    note_apply,
    cite_add,
    cite_remove: u32,
    cite_verify: u32,
    undo,
    // console
    chat_send,
    chat_cancel,
    chat_clear,
    toggle_tool: u32,
    model_pick: Model3,
    key_save,
    key_forget,
    toggle_key_card,
    toggle_demo,
    attach_pick,
    attach_remove: u32,
    console_scroll_by: f32,
    console_extent: [2]f32,
    // time + effects
    window: [2]f32,
    show_panel: Panel,
    submit,
    tick,
    fx: teak.EffectResult,
};
