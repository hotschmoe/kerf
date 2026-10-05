//! The view: DESIGN.md §2 layout. header | console | viewport | inspector | status.

const std = @import("std");
const teak = @import("teak");
const model = @import("model.zig");
const th = @import("theme.zig");
const wrap = @import("textwrap.zig").wrap;
const units = @import("units.zig");
const flow = @import("docflow.zig");
const chatglue = @import("chatglue.zig");
const llm = @import("../llm/mod.zig");
const docinfo = @import("docinfo.zig");
const session = @import("session.zig");
const editor = @import("editor.zig");

const Model = model.Model;
const Msg = model.Msg;
const Cb = teak.CmdBuffer(Msg);

const COL = 7.8; // px per column of 13px Plex Mono (0.6 em)

fn arena(cb: *Cb) std.mem.Allocator {
    return cb.arena.allocator();
}

fn fmt(cb: *Cb, comptime f: []const u8, args: anytype) []const u8 {
    return std.fmt.allocPrint(arena(cb), f, args) catch "?";
}

fn upper(cb: *Cb, s: []const u8) []const u8 {
    const out = arena(cb).dupe(u8, s) catch return s;
    for (out) |*c| c.* = std.ascii.toUpper(c.*);
    return out;
}

// ── small widgets ──────────────────────────────────────────────────

fn label(cb: *Cb, s: []const u8) void {
    cb.textStyled(s, th.label, th.ink2);
}

fn rule(cb: *Cb, thickness: f32) void {
    cb.dividerStyled(.{ .thickness = thickness, .color = th.ink });
}

fn vrule(cb: *Cb) void {
    cb.pushGroup(.{ .width = 1, .padding = 0, .gap = 0, .bg = th.ink });
    cb.popGroup();
}

fn key(cb: *Cb, msg: Msg, text: []const u8) void {
    cb.buttonStyled(msg, text, th.button);
}

fn tab(cb: *Cb, msg: Msg, text: []const u8, active: bool) void {
    cb.pushGroup(.{ .padding = 0, .gap = 0, .align_cross = .stretch });
    cb.buttonStyled(msg, if (active) fmt(cb, "[{s}]", .{text}) else fmt(cb, " {s} ", .{text}), th.button_tab);
    cb.pushGroup(.{ .padding = 0, .gap = 0, .height = 3, .bg = if (active) th.blue else null });
    cb.popGroup();
    cb.popGroup();
}

/// Rubber-stamp style status tag (square corners; the rotation is faked by the
/// 1.5px border + bold type).
fn stamp(cb: *Cb, text: []const u8, color: th.Color) void {
    cb.pushGroup(.{ .padding = 0, .pad_x = 7, .pad_y = 2, .gap = 0, .border = color, .border_width = 1.5 });
    cb.textStyled(text, th.label, color);
    cb.popGroup();
}

/// Thin ink scrollbar column next to a scroll region (the Model holds the metrics).
fn scrollBar(cb: *Cb, scroll: f32, viewport: f32, content: f32) void {
    cb.pushGroup(.{ .width = 6, .padding = 0, .gap = 0, .align_cross = .stretch });
    if (viewport > 1 and content > viewport + 1) {
        const thumb = @max(18, viewport * viewport / content);
        const top = (viewport - thumb) * std.math.clamp(scroll / (content - viewport), 0, 1);
        cb.pushGroup(.{ .height = top, .padding = 0, .gap = 0 });
        cb.popGroup();
        cb.pushGroup(.{ .height = thumb, .padding = 0, .gap = 0, .bg = th.ink2 });
        cb.popGroup();
    }
    cb.popGroup();
}

fn field(cb: *Cb, lbl: []const u8, ed: *const editor.Editor, focus: model.Focus, focused: bool, cols: usize) void {
    label(cb, lbl);
    const w = ed.window(cols);
    const style = th.field;
    cb.textInputSelected(.{ .focus = focus }, ed.content()[w.start..w.end], w.cursor, if (focused) w.anchor else null, style);
}

// ── root ───────────────────────────────────────────────────────────

pub fn view(m: *const Model, cb: *Cb) void {
    cb.pushGroup(.{ .padding = 0, .gap = 0, .bg = th.paper, .align_cross = .stretch });
    header(m, cb);
    rule(cb, 2);
    cb.pushGroup(.{ .direction = .horizontal, .padding = 0, .gap = 0, .flex = 1, .align_cross = .stretch });
    console(m, cb);
    vrule(cb);
    center(m, cb);
    vrule(cb);
    inspector(m, cb);
    cb.popGroup();
    statusLine(m, cb);
    cb.popGroup();

    menus(m, cb);
}

// ── header ─────────────────────────────────────────────────────────

fn header(m: *const Model, cb: *Cb) void {
    cb.pushGroup(.{ .direction = .horizontal, .pad_x = 12, .pad_y = 0, .gap = 12, .height = 40, .align_cross = .center });
    cb.textStyled("KERF", th.wordmark, th.ink);
    cb.pushGroup(.{ .direction = .horizontal, .padding = 0, .gap = 3, .align_cross = .center });
    for (0..3) |_| {
        cb.pushGroup(.{ .width = 6, .height = 14, .padding = 0, .gap = 0, .bg = th.ink });
        cb.popGroup();
    }
    cb.popGroup();
    cb.textStyled("DETAIL WORKSTATION", th.label, th.ink2);
    cb.spacer(1);
    if (m.ready) {
        const info = m.doc.info.?;
        cb.textStyled(fmt(cb, "DOC: {s}   REV {d}   STYLE: KERF-STANDARD", .{ upper(cb, info.id), m.doc.rev }), th.label, th.ink);
    } else {
        cb.textStyled("NO DETAIL LOADED", th.label, th.ink2);
    }
    cb.spacer(1);
    key(cb, .{ .menu = .samples }, "OPEN");
    key(cb, .save_file, "SAVE");
    key(cb, .{ .menu = .export_ }, "EXPORT");
    cb.popGroup();
}

// ── console ────────────────────────────────────────────────────────

fn console(m: *const Model, cb: *Cb) void {
    cb.pushGroup(.{ .width = model.CONSOLE_W, .padding = 0, .gap = 0, .align_cross = .stretch });

    cb.pushGroup(.{ .direction = .horizontal, .pad_x = 12, .pad_y = 8, .gap = 6, .align_cross = .center });
    label(cb, "OPERATOR CONSOLE");
    cb.spacer(1);
    cb.buttonStyled(.{ .menu = .model }, fmt(cb, "{s}", .{chatglue.modelLabel(m.model_pick)}), th.button);
    cb.buttonStyled(.toggle_key_card, if (m.demo) "DEMO" else if (chatglue.hasKey(m)) "KEY" else "NO KEY", if (chatglue.hasKey(m) or m.demo) th.button else th.button_danger);
    cb.popGroup();
    rule(cb, 1);

    if (m.show_key_card or (!chatglue.hasKey(m) and !m.demo)) keyCard(m, cb);

    messages(m, cb);
    rule(cb, 1);
    inputRow(m, cb);
    cb.popGroup();
}

fn keyCard(m: *const Model, cb: *Cb) void {
    cb.pushGroup(.{ .padding = 12, .gap = 6, .align_cross = .stretch, .bg = th.paper2 });
    label(cb, "ANTHROPIC API KEY");
    cb.textStyled("STORED LOCALLY. SENT ONLY TO API.ANTHROPIC.COM.", th.small, th.ink2);
    const masked: []u8 = arena(cb).alloc(u8, m.key_ed.len) catch @constCast("");
    @memset(masked, '*');
    const w = m.key_ed.window(34);
    const shown = masked[@min(w.start, masked.len)..@min(w.end, masked.len)];
    cb.textInputSelected(.{ .focus = .key }, shown, w.cursor, null, th.field);
    cb.pushGroup(.{ .direction = .horizontal, .padding = 0, .gap = 6 });
    cb.buttonStyled(.key_save, "STORE KEY", th.button_primary);
    cb.buttonStyled(.toggle_demo, if (m.demo) "DEMO: ON" else "DEMO MODE", th.button);
    if (chatglue.hasKey(m)) cb.buttonStyled(.key_forget, "FORGET", th.button_danger);
    cb.popGroup();
    cb.popGroup();
    rule(cb, 1);
}

const MSG_COLS = 38;

fn messages(m: *const Model, cb: *Cb) void {
    cb.pushGroup(.{ .direction = .horizontal, .padding = 0, .gap = 0, .flex = 1, .align_cross = .stretch });
    cb.pushScroll(.{
        .id = model.CONSOLE_SCROLL,
        .padding = 10,
        .gap = 8,
        .flex = 1,
        .scroll_y = m.console_scroll,
        .align_cross = .stretch,
    });
    const entries = m.chat.log.entries.items;
    if (entries.len == 0) {
        cb.textStyled("DESCRIBE THE DETAIL YOU NEED.", th.body, th.ink2);
        cb.textStyled("PASTE OR DROP A SCREENSHOT TO", th.body, th.ink2);
        cb.textStyled("RECREATE AN EXISTING DETAIL.", th.body, th.ink2);
    }
    var i: usize = 0;
    while (i < entries.len) {
        const e = entries[i];
        switch (e.body) {
            .designer => |d| {
                designerCard(m, cb, e.ts_ms, d.text, d.n_images, d.edits_note);
                i += 1;
            },
            else => {
                // One manila card per group of non-designer entries.
                const g = e.group;
                var j = i;
                while (j < entries.len and entries[j].group == g and entries[j].body != .designer) j += 1;
                claudeCard(m, cb, entries[i..j], i);
                i = j;
            },
        }
    }
    cb.popScroll();
    scrollBar(cb, m.console_scroll, m.console_viewport, m.console_content);
    cb.popGroup();
}

fn clock(m: *const Model, cb: *Cb, ts_ms: i64) []const u8 {
    const buf = arena(cb).alloc(u8, 5) catch return "";
    return llm.chatlog.formatClock(ts_ms, m.utc_offset_min, buf[0..5]);
}

fn designerCard(m: *const Model, cb: *Cb, ts_ms: i64, text: []const u8, n_images: u32, edits: ?[]const u8) void {
    cb.pushGroup(.{ .padding = 8, .gap = 3, .bg = th.paper, .border = th.ink, .align_cross = .stretch });
    cb.textStyled(fmt(cb, "DESIGNER  {s}", .{clock(m, cb, ts_ms)}), th.label, th.ink2);
    for (wrap(arena(cb), text, MSG_COLS, 60)) |l| cb.textStyled(l, th.body, th.ink);
    if (n_images > 0) cb.textStyled(fmt(cb, "[{d} IMAGE{s} ATTACHED]", .{ n_images, if (n_images == 1) "" else "S" }), th.small, th.blue);
    if (edits) |ed| for (wrap(arena(cb), ed, MSG_COLS + 4, 4)) |l| cb.textStyled(l, th.small, th.ink2);
    cb.popGroup();
}

fn claudeCard(m: *const Model, cb: *Cb, entries: []const llm.chatlog.Entry, base: usize) void {
    cb.pushGroup(.{ .padding = 8, .gap = 3, .bg = th.manila, .border = th.ink, .align_cross = .stretch });
    cb.textStyled(fmt(cb, "KERF/CLAUDE  {s}", .{clock(m, cb, entries[0].ts_ms)}), th.label, th.ink2);
    for (entries, 0..) |e, k| {
        switch (e.body) {
            .assistant_text => |t| for (wrap(arena(cb), t, MSG_COLS, 200)) |l| cb.textStyled(l, th.body, th.ink),
            .tool => |t| toolLine(m, cb, t, base + k),
            .notice => |t| for (wrap(arena(cb), t, MSG_COLS, 3)) |l| cb.textStyled(l, th.small, th.ink2),
            .err => |er| for (wrap(arena(cb), er.message, MSG_COLS, 6)) |l| cb.textStyled(l, th.body, th.red),
            .refusal => |t| {
                cb.textStyled("REQUEST REFUSED", th.bold, th.red);
                for (wrap(arena(cb), t, MSG_COLS, 8)) |l| cb.textStyled(l, th.body, th.red);
            },
            .designer => {},
        }
    }
    cb.popGroup();
}

fn thumbIndex(m: *const Model, upto: usize) ?usize {
    // The k-th kerf_render result with an image <-> the k-th thumbnail.
    var k: usize = 0;
    for (m.chat.log.entries.items[0..upto]) |e| switch (e.body) {
        .tool => |t| if (t.has_image and std.mem.eql(u8, t.name, "kerf_render")) {
            k += 1;
        },
        else => {},
    };
    return if (k < m.n_thumbs) k else null;
}

fn toolLine(m: *const Model, cb: *Cb, t: llm.chatlog.Tool, idx: usize) void {
    const buf = arena(cb).alloc(u8, 160) catch return;
    const line = llm.chatlog.toolLine(t, buf);
    const style = if (t.state == .failed) th.button_flat_danger else th.button_flat;
    cb.buttonStyled(.{ .toggle_tool = @intCast(idx) }, line, style);
    if (t.has_image and std.mem.eql(u8, t.name, "kerf_render")) {
        if (thumbIndex(m, idx)) |k| {
            const th_ = m.thumbs[k];
            const w: f32 = 300;
            cb.pushGroup(.{ .padding = 1, .gap = 0, .border = th.ink, .align_cross = .start });
            cb.image(model.THUMB_KEY0 + @as(u32, @intCast(k)), .{ .width = w, .height = w * @as(f32, @floatFromInt(th_.h)) / @as(f32, @floatFromInt(th_.w)) });
            cb.popGroup();
        }
    }
    if (!t.expanded) return;
    cb.pushGroup(.{ .padding = 4, .gap = 1, .bg = th.paper, .align_cross = .stretch });
    const pretty = llm.jsonw.pretty(arena(cb), t.input_json) catch t.input_json;
    for (wrap(arena(cb), pretty, MSG_COLS + 2, 28)) |l| cb.textStyled(l, th.small, th.ink);
    if (t.result_text.len > 0) {
        cb.dividerStyled(.{ .thickness = 1, .color = th.ink2 });
        for (wrap(arena(cb), t.result_text, MSG_COLS + 2, 16)) |l| cb.textStyled(l, th.small, th.ink2);
    }
    cb.popGroup();
}

fn inputRow(m: *const Model, cb: *Cb) void {
    if (m.n_attach > 0) {
        cb.pushGroup(.{ .direction = .horizontal, .pad_x = 12, .pad_y = 4, .gap = 6, .align_cross = .center });
        for (m.attachments[0..m.n_attach], 0..) |a, i| {
            if (a.thumb.len > 0) {
                const long: f32 = 40;
                const fw: f32 = @floatFromInt(a.thumb_w);
                const fh: f32 = @floatFromInt(a.thumb_h);
                const sc = long / @max(fw, fh);
                cb.pushGroup(.{ .padding = 1, .gap = 0, .border = th.ink });
                cb.image(model.ATTACH_KEY0 + @as(u32, @intCast(i)), .{ .width = fw * sc, .height = fh * sc });
                cb.popGroup();
            }
            cb.buttonStyled(.{ .attach_remove = @intCast(i) }, fmt(cb, "{s} X", .{if (a.name_len > 0) a.name[0..@min(a.name_len, 14)] else "IMAGE"}), th.button_flat);
        }
        cb.popGroup();
    }
    cb.pushGroup(.{ .direction = .horizontal, .padding = 10, .gap = 6, .align_cross = .stretch });
    const w = m.chat_ed.window(26);
    const focused = m.focus == .chat;
    cb.textInputSelected(.{ .focus = .chat }, m.chat_ed.content()[w.start..w.end], w.cursor, if (focused) w.anchor else null, th.prompt);
    cb.pushGroup(.{ .padding = 0, .gap = 0, .align_cross = .stretch });
    const busy = m.chat.session.isBusy();
    if (busy) cb.buttonStyled(.chat_cancel, "STOP", th.button_danger) else cb.buttonStyled(.chat_send, "SEND", th.button_primary);
    cb.popGroup();
    cb.buttonStyled(.attach_pick, "+IMG", th.button);
    cb.popGroup();
}

// ── center: tabs + viewport ────────────────────────────────────────

fn center(m: *const Model, cb: *Cb) void {
    cb.pushGroup(.{ .padding = 0, .gap = 0, .flex = 1, .align_cross = .stretch });
    tabsBar(m, cb);
    rule(cb, 1);
    if (!m.ready) {
        emptyViewport(cb);
    } else switch (m.tabKind()) {
        .d3 => scene3d(m, cb),
        else => canvas2d(m, cb),
    }
    cb.popGroup();
}

fn tabsBar(m: *const Model, cb: *Cb) void {
    cb.pushGroup(.{ .direction = .horizontal, .pad_x = 12, .pad_y = 4, .gap = 6, .align_cross = .end });
    if (m.ready) {
        const info = m.doc.info.?;
        for (info.views, 0..) |v, i| {
            const kind = if (v.kind.len > 0) v.kind else "VIEW";
            tab(cb, .{ .select_tab = @intCast(i) }, fmt(cb, "{s} {s}", .{ upper(cb, kind), upper(cb, v.id) }), m.tab == i);
        }
        tab(cb, .{ .select_tab = @intCast(info.views.len) }, "3D", m.tab == info.views.len);
        tab(cb, .{ .select_tab = @intCast(info.views.len + 1) }, "SHEET", m.tab == info.views.len + 1);
    }
    cb.spacer(1);
    cb.pushGroup(.{ .direction = .horizontal, .padding = 0, .gap = 4, .align_cross = .center });
    key(cb, .{ .zoom = 0.8 }, "-");
    key(cb, .{ .zoom = 1.25 }, "+");
    key(cb, .fit, "FIT");
    cb.popGroup();
    cb.popGroup();
    // View-cube row for the 3D tab (DESIGN §4): bracketed text keys.
    if (m.ready and m.tabKind() == .d3) {
        rule(cb, 1);
        cb.pushGroup(.{ .direction = .horizontal, .pad_x = 12, .pad_y = 4, .gap = 4, .align_cross = .center });
        label(cb, "VIEW");
        key(cb, .{ .preset3d = .front }, "[FRONT]");
        key(cb, .{ .preset3d = .iso }, "[ISO]");
        key(cb, .{ .preset3d = .top }, "[TOP]");
        key(cb, .{ .preset3d = .right }, "[RIGHT]");
        cb.spacer(1);
        cb.textStyled("DRAG ORBIT  SHIFT-DRAG PAN  WHEEL ZOOM", th.small, th.ink2);
        cb.popGroup();
    }
}

fn emptyViewport(cb: *Cb) void {
    cb.pushGroup(.{ .padding = 24, .gap = 6, .flex = 1, .bg = th.vellum, .align_cross = .center, .justify = .center });
    cb.textStyled("NO DETAIL LOADED.", th.heading, th.ink);
    cb.textStyled("DESCRIBE ONE IN THE CONSOLE, OR OPEN A .KERF.JSON.", th.body, th.ink2);
    cb.popGroup();
}

fn canvas2d(m: *const Model, cb: *Cb) void {
    const vp = m.vp;
    const style: teak.CanvasStyle = .{ .width = 200, .height = 200, .flex = 1, .bg = th.vellum };
    const verts: []const teak.CanvasPrimitive.TriVertex = @ptrCast(vp.verts());
    const prims = arena(cb).dupe(teak.CanvasPrimitive, &.{.{ .triangles = .{ .verts = verts, .key = vp.key } }}) catch &.{};
    cb.canvasInteractive(style, prims, model.CANVAS_ID, "drawing viewport");
}

fn scene3d(m: *const Model, cb: *Cb) void {
    if (m.scene == null) {
        cb.pushGroup(.{ .padding = 24, .flex = 1, .bg = th.paper, .align_cross = .center, .justify = .center });
        cb.textStyled("NO 3D GEOMETRY FOR THIS DOCUMENT.", th.body, th.ink2);
        cb.popGroup();
        return;
    }
    const aspect: f32 = if (m.scene_h > 8) m.scene_w / m.scene_h else 1.5;
    cb.scene3d(.{
        .style = .{ .width = 200, .height = 200, .flex = 1 },
        .mesh = model.MESH_KEY,
        .camera = .{ .view_proj = m.orbit.viewProj(aspect), .eye = m.orbit.eye(), .light_dir = .{ -0.35, -0.8, -0.5 } },
        .clear = th.paper,
        .edge_color = .{ 1, 1, 1, 1 },
        .edge_px = 1.25,
        .key = m.mesh_rev,
        .id = model.SCENE_ID,
        .pointer = true,
        .label = "3D view",
    });
}

// ── inspector ──────────────────────────────────────────────────────

fn inspector(m: *const Model, cb: *Cb) void {
    cb.pushGroup(.{ .width = model.INSPECTOR_W, .padding = 0, .gap = 0, .align_cross = .stretch });
    cb.pushGroup(.{ .direction = .horizontal, .pad_x = 12, .pad_y = 4, .gap = 4, .align_cross = .end });
    label(cb, "INSPECTOR");
    cb.spacer(1);
    cb.popGroup();
    cb.pushGroup(.{ .direction = .horizontal, .pad_x = 8, .pad_y = 0, .gap = 2, .align_cross = .end });
    tab(cb, .{ .insp_tab = .parts }, "PARTS", m.insp == .parts);
    tab(cb, .{ .insp_tab = .notes }, "NOTES", m.insp == .notes);
    tab(cb, .{ .insp_tab = .diff }, "DIFF", m.insp == .diff);
    tab(cb, .{ .insp_tab = .diag }, "DIAG", m.insp == .diag);
    cb.popGroup();
    rule(cb, 1);

    cb.pushGroup(.{ .direction = .horizontal, .padding = 0, .gap = 0, .flex = 1, .align_cross = .stretch });
    cb.pushScroll(.{
        .id = model.INSPECTOR_SCROLL,
        .padding = 12,
        .gap = 6,
        .flex = 1,
        .scroll_y = m.insp_scroll,
        .align_cross = .stretch,
    });
    if (m.ready) switch (m.insp) {
        .parts => partsTab(m, cb),
        .notes => notesTab(m, cb),
        .diff => diffTab(m, cb),
        .diag => diagTab(m, cb),
    } else {
        cb.textStyled("NOTHING TO INSPECT.", th.body, th.ink2);
    }
    cb.popScroll();
    scrollBar(cb, m.insp_scroll, m.insp_viewport, m.insp_content);
    cb.popGroup();

    rule(cb, 1);
    cb.pushGroup(.{ .direction = .horizontal, .padding = 10, .gap = 6, .align_cross = .center });
    key(cb, .{ .export_as = .dxf }, "EXPORT DXF");
    key(cb, .{ .export_as = .pdf }, "PDF");
    key(cb, .{ .export_as = .svg }, "SVG");
    cb.popGroup();
    cb.popGroup();
}

const INSP_COLS = 36;

fn rowButton(cb: *Cb, msg: Msg, text: []const u8, selected: bool) void {
    cb.buttonStyled(msg, text, if (selected) th.button_row_selected else th.button_row);
}

fn padTo(cb: *Cb, s: []const u8, n: usize) []const u8 {
    const out = arena(cb).alloc(u8, n) catch return s;
    @memset(out, ' ');
    const cp = @min(s.len, n);
    @memcpy(out[0..cp], s[0..cp]);
    return out;
}

fn partsTab(m: *const Model, cb: *Cb) void {
    const info = m.doc.info.?;
    label(cb, fmt(cb, "{d} COMPONENTS", .{info.components.len}));
    cb.buttonStyled(.undo, "NO  ID              TYPE", th.button_row_header);
    for (info.components, 0..) |c, i| {
        const sel = m.sel.get() != null and std.mem.eql(u8, m.sel.get().?, c.id);
        const line = fmt(cb, "{d:0>2}  {s}{s}", .{ i + 1, padTo(cb, c.id, 16), upper(cb, c.type[0..@min(c.type.len, 12)]) });
        rowButton(cb, .{ .select = ident_of(c.id) }, line, sel);
    }
    if (m.insp_text.len > 0) {
        rule(cb, 1);
        label(cb, fmt(cb, "COMPONENT {s}", .{upper(cb, m.insp_text_for.slice())}));
        const pretty = llm.jsonw.pretty(arena(cb), m.insp_text) catch m.insp_text;
        for (wrap(arena(cb), pretty, INSP_COLS + 2, 80)) |l| cb.textStyled(l, th.small, th.ink);
    }
}

fn ident_of(s: []const u8) @import("ident.zig").Id {
    return @import("ident.zig").Id.from(s);
}

fn notesTab(m: *const Model, cb: *Cb) void {
    const info = m.doc.info.?;
    const vi: usize = switch (m.tabKind()) {
        .view => |i| i,
        else => 0,
    };
    if (vi >= info.views.len) return;
    const notes = info.views[vi].notes;
    label(cb, fmt(cb, "{d} ANNOTATIONS IN VIEW {s}", .{ notes.len, upper(cb, info.views[vi].id) }));
    for (notes, 0..) |nt, i| {
        const text = if (nt.text.len > 0) nt.text else upper(cb, nt.kind);
        const unv = unverifiedCount(nt);
        const line = fmt(cb, "{d:0>2} {s} {s}", .{ i + 1, padTo(cb, nt.id, 8), text[0..@min(text.len, 22)] });
        rowButton(cb, .{ .select_note = @intCast(i) }, if (unv > 0) fmt(cb, "{s}*", .{line}) else line, m.note_sel == @as(i32, @intCast(i)));
    }
    if (flow.currentNote(m)) |nt| noteEditor(m, cb, nt);
}

fn unverifiedCount(nt: docinfo.Note) usize {
    var n: usize = 0;
    for (nt.cites) |c| if (!c.verified) {
        n += 1;
    };
    return n;
}

fn noteEditor(m: *const Model, cb: *Cb, nt: docinfo.Note) void {
    rule(cb, 1);
    label(cb, fmt(cb, "ANNOTATION {s}  ({s})", .{ upper(cb, nt.id), upper(cb, nt.kind) }));
    field(cb, "TEXT", &m.note_ed, .note_text, m.focus == .note_text, INSP_COLS - 2);
    cb.pushGroup(.{ .direction = .horizontal, .padding = 0, .gap = 6 });
    cb.buttonStyled(.note_apply, "APPLY TEXT", th.button_primary);
    cb.popGroup();
    if (!std.mem.eql(u8, nt.kind, "note")) return;

    label(cb, "CODE CITATIONS");
    if (nt.cites.len == 0) cb.textStyled("NONE", th.small, th.ink2);
    for (nt.cites, 0..) |c, i| {
        cb.pushGroup(.{ .padding = 4, .gap = 3, .border = th.ink2, .align_cross = .stretch });
        const ed = if (c.edition) |e| fmt(cb, " {d}", .{e}) else "";
        cb.textStyled(fmt(cb, "{s}{s} {s}", .{ c.code, ed, c.section }), th.bold, th.ink);
        if (c.title.len > 0) for (wrap(arena(cb), c.title, INSP_COLS - 4, 2)) |l| cb.textStyled(l, th.small, th.ink2);
        cb.pushGroup(.{ .direction = .horizontal, .padding = 0, .gap = 6, .align_cross = .center });
        if (c.verified) stamp(cb, "VERIFIED", th.green) else stamp(cb, "UNVERIFIED", th.red);
        cb.spacer(1);
        cb.buttonStyled(.{ .cite_verify = @intCast(i) }, if (c.verified) "UNVERIFY" else "VERIFY", th.button);
        cb.buttonStyled(.{ .cite_remove = @intCast(i) }, "X", th.button_danger);
        cb.popGroup();
        cb.popGroup();
    }
    label(cb, "ADD CITATION");
    field(cb, "CODE (DEFAULT: DOC JURISDICTION)", &m.cite_code_ed, .cite_code, m.focus == .cite_code, INSP_COLS - 2);
    field(cb, "SECTION", &m.cite_sec_ed, .cite_section, m.focus == .cite_section, INSP_COLS - 2);
    field(cb, "TITLE", &m.cite_title_ed, .cite_title, m.focus == .cite_title, INSP_COLS - 2);
    cb.buttonStyled(.cite_add, "ADD (SUGGESTED)", th.button);
}

fn diffTab(m: *const Model, cb: *Cb) void {
    cb.pushGroup(.{ .direction = .horizontal, .padding = 0, .gap = 6, .align_cross = .center });
    label(cb, fmt(cb, "REV {d}   {d} EDITS", .{ m.doc.rev, m.doc.log.items.len }));
    cb.spacer(1);
    if (m.doc.canUndo()) cb.buttonStyled(.undo, "UNDO LAST", th.button) else cb.buttonStyled(.undo, "UNDO LAST", th.button_disabled);
    cb.popGroup();
    const log = m.doc.log.items;
    var i = log.len;
    while (i > 0) {
        i -= 1;
        const e = log[i];
        cb.pushGroup(.{ .padding = 6, .gap = 2, .border = th.ink2, .align_cross = .stretch });
        const t = arena(cb).alloc(u8, 5) catch break;
        const ops = std.mem.count(u8, e.ops_json, "\"op\"");
        cb.textStyled(fmt(cb, "R{d} {s} {s}  {d} OP{s}{s}", .{
            e.rev_after,
            e.who.label(),
            llm.chatlog.formatClock(e.time_s * 1000, m.utc_offset_min, t[0..5]),
            ops,
            if (ops == 1) "" else "S",
            if (e.undone) "  UNDONE" else "",
        }), th.small, if (e.undone) th.ink2 else th.ink);
        for (wrap(arena(cb), e.why, INSP_COLS - 2, 3)) |l| cb.textStyled(l, th.body, if (e.undone) th.ink2 else th.ink);
        cb.popGroup();
    }
    if (log.len == 0) cb.textStyled("NO EDITS YET.", th.body, th.ink2);
}

fn diagTab(m: *const Model, cb: *Cb) void {
    label(cb, fmt(cb, "{d} ERR  {d} WARN  {d} TOTAL", .{ m.doc.errorCount(), m.doc.warnCount(), m.doc.diags.len }));
    for (m.doc.diags) |d| {
        const color = switch (d.level) {
            .@"error" => th.red,
            .warning => th.amber,
            .info => th.ink2,
        };
        cb.pushGroup(.{ .padding = 0, .gap = 0, .align_cross = .stretch });
        const head = fmt(cb, "{c}  {s}{s}{s}", .{ d.level.letter(), d.code, if (d.id.len > 0) "  " else "", d.id });
        if (d.id.len > 0) cb.buttonStyled(.{ .select = ident_of(d.id) }, head, th.button_flat) else cb.textStyled(head, th.bold, color);
        for (wrap(arena(cb), d.message, INSP_COLS, 6)) |l| cb.textStyled(l, th.small, th.ink);
        cb.popGroup();
    }
    if (m.doc.diags.len == 0) cb.textStyled("NO DIAGNOSTICS.", th.body, th.ink2);
}

// ── status line ────────────────────────────────────────────────────

fn statusLine(m: *const Model, cb: *Cb) void {
    cb.pushGroup(.{ .direction = .horizontal, .pad_x = 12, .pad_y = 0, .gap = 8, .height = 24, .bg = th.term_bg, .align_cross = .center });
    const f: teak.FontSpec = .{ .size_px = 12, .family = .mono, .weight = .medium };
    const seg = struct {
        fn sep(c: *Cb) void {
            c.pushGroup(.{ .width = 5, .height = 11, .padding = 0, .gap = 0, .bg = th.term_fg });
            c.popGroup();
        }
    };
    const busy = m.chat.session.isBusy();
    cb.textStyled(if (m.status_len > 0 and m.status_age < 40) m.statusText() else "READY", f, th.term_fg);
    seg.sep(cb);
    if (m.ready) {
        const info = m.doc.info.?;
        cb.textStyled(fmt(cb, "{d} COMPONENTS", .{info.components.len}), f, th.term_fg);
        seg.sep(cb);
        cb.textStyled(fmt(cb, "{d} ERR {d} WARN", .{ m.doc.errorCount(), m.doc.warnCount() }), f, if (m.doc.errorCount() > 0) th.red_on_dark else th.term_fg);
        seg.sep(cb);
        if (m.activeViewId()) |vid| {
            var scale: []const u8 = "";
            for (info.views) |v| if (std.mem.eql(u8, v.id, vid)) {
                scale = v.scale;
            };
            cb.textStyled(fmt(cb, "VIEW {s} {s}", .{ upper(cb, vid), scale }), f, th.term_fg);
            seg.sep(cb);
        }
        if (m.cursor_model) |c| {
            var xb: [32]u8 = undefined;
            var yb: [32]u8 = undefined;
            cb.textStyled(fmt(cb, "X {s} Y {s}", .{ units.formatFeetInches(&xb, c[0]), units.formatFeetInches(&yb, c[1]) }), f, th.term_fg);
        }
    } else {
        cb.textStyled("0 COMPONENTS", f, th.term_fg);
    }
    cb.spacer(1);
    const sb = arena(cb).alloc(u8, 64) catch return;
    const st = m.chat.log.statusText(sb);
    const spinner = "|/-\\";
    cb.textStyled(if (busy) fmt(cb, "{s} {c}", .{ st, spinner[m.ticks % 4] }) else st, f, th.term_fg);
    cb.popGroup();
}

// ── overlays ───────────────────────────────────────────────────────

/// Full-window transparent modal scrim: a click anywhere outside the panel closes the menu.
fn scrim(m: *const Model, cb: *Cb) void {
    cb.pushOverlay(.{ .x = 0, .y = 0, .width = m.win_w, .height = m.win_h, .padding = 0, .backdrop = th.clear, .modal = true, .backdrop_msg = .close_menu });
    cb.popOverlay();
}

fn panel(x: f32, y: f32, anchor_x: f32) teak.OverlayStyle(Msg) {
    return .{
        .x = x,
        .y = y,
        .padding = 6,
        .gap = 4,
        .backdrop = th.paper,
        .border = th.ink,
        .shadow = th.ink,
        .shadow_offset = .{ 2, 2 },
        .align_cross = .stretch,
        .anchor_x_frac = anchor_x,
    };
}

fn menus(m: *const Model, cb: *Cb) void {
    if (m.menu == .none) return;
    scrim(m, cb);
    switch (m.menu) {
        .samples => {
            cb.pushOverlay(panel(m.win_w - 12, 44, 1.0));
            label(cb, "OPEN SAMPLE DETAIL");
            for (model.samples, 0..) |s, i| cb.buttonStyled(.{ .load_sample = @intCast(i) }, s.label, th.button_row);
            rule(cb, 1);
            cb.buttonStyled(.open_file, "OPEN .KERF.JSON FILE...", th.button_row);
            cb.popOverlay();
        },
        .export_ => {
            cb.pushOverlay(panel(m.win_w - 12, 44, 1.0));
            label(cb, "EXPORT ACTIVE VIEW");
            cb.buttonStyled(.{ .export_as = .dxf }, "DXF  (ACAD R2000)", th.button_row);
            cb.buttonStyled(.{ .export_as = .pdf }, "PDF  (SHEET, VECTOR)", th.button_row);
            cb.buttonStyled(.{ .export_as = .svg }, "SVG", th.button_row);
            cb.popOverlay();
        },
        .model => {
            cb.pushOverlay(panel(12, 78, 0));
            label(cb, "CLAUDE MODEL");
            cb.buttonStyled(.{ .model_pick = .opus_5_5 }, "OPUS 5.5 (DEFAULT)", th.button_row);
            cb.buttonStyled(.{ .model_pick = .sonnet_5_5 }, "SONNET 5.5 (FASTER)", th.button_row);
            cb.popOverlay();
        },
        .none => {},
    }
}
