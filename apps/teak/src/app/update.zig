//! `update`: the only place the Model changes.

const std = @import("std");
const teak = @import("teak");
const alloc = @import("alloc.zig");
const model = @import("model.zig");
const flow = @import("docflow.zig");
const chatglue = @import("chatglue.zig");
const docops = @import("docops.zig");
const ident = @import("ident.zig");
const fx = @import("fx.zig");
const llm = @import("../llm/mod.zig");
const viewport = @import("viewport.zig");
const editor = @import("editor.zig");

const Model = model.Model;
const Msg = model.Msg;
const gpa = alloc.gpa;

pub fn focusedEditor(m: *Model) ?*editor.Editor {
    return switch (m.focus) {
        .none => null,
        .chat => &m.chat_ed,
        .key => &m.key_ed,
        .note_text => &m.note_ed,
        .cite_code => &m.cite_code_ed,
        .cite_section => &m.cite_sec_ed,
        .cite_title => &m.cite_title_ed,
    };
}

pub fn update(m: *Model, msg: Msg) void {
    switch (msg) {
        // ── chrome ──
        .menu => |k| m.menu = if (m.menu == k) .none else k,
        .close_menu => m.menu = .none,
        .load_sample => |i| {
            m.menu = .none;
            if (i < model.samples.len) _ = flow.loadDoc(m, model.samples[i].json, model.samples[i].label);
        },
        .open_file => {
            m.menu = .none;
            _ = m.fx.openFile(.open_doc, ".json,.kerf.json");
        },
        .save_file => saveFile(m),
        .export_as => |f| exportAs(m, f),

        // ── viewport ──
        .select_tab => |t| {
            m.menu = .none;
            flow.selectTab(m, t);
        },
        .fit => {
            switch (m.tabKind()) {
                .d3 => flow.frameMesh(m),
                else => {
                    m.vp.fit();
                    m.vp.retess() catch {};
                },
            }
        },
        .zoom => |f| switch (m.tabKind()) {
            .d3 => m.orbit.zoom(f),
            else => {
                m.vp.zoomStep(f);
                m.vp.retess() catch {};
            },
        },
        .preset3d => |p| m.orbit.setPreset(p),
        .toggle_grid => {
            m.vp.grid = !m.vp.grid;
            m.vp.retess() catch {};
        },
        .canvas => |ev| canvasEvent(m, ev),

        // ── selection + inspector ──
        .select => |id| flow.select(m, id.slice()),
        .select_note => |i| {
            const info = &(m.doc.info orelse return);
            const vi: usize = switch (m.tabKind()) {
                .view => |v| v,
                else => 0,
            };
            if (vi < info.views.len and i < info.views[vi].notes.len) flow.select(m, info.views[vi].notes[i].id);
        },
        .insp_tab => |t| m.insp = t,
        .insp_scroll_by => |dy| m.insp_scroll = clamp(m.insp_scroll + dy, m.insp_content - m.insp_viewport),
        .insp_extent => |e| {
            m.insp_viewport = e[0];
            m.insp_content = e[1];
            m.insp_scroll = clamp(m.insp_scroll, e[1] - e[0]);
        },
        .focus => |f| {
            m.menu = .none;
            m.focus = f;
        },
        .char => |c| {
            if (focusedEditor(m)) |ed| ed.insert(&[_]u8{c});
        },
        .key => |k| key(m, k),
        .note_apply => flow.commitNoteText(m),
        .cite_add => citeAdd(m),
        .cite_remove => |i| flow.citeEdit(m, .{ .remove = i }, "Remove citation"),
        .cite_verify => |i| {
            const nt = flow.currentNote(m) orelse return;
            if (i >= nt.cites.len) return;
            const now = !nt.cites[i].verified;
            flow.citeEdit(m, .{ .set_verified = .{ .index = i, .verified = now } }, if (now) "Verify citation" else "Unverify citation");
        },
        .undo => flow.undo(m),

        // ── console ──
        .chat_send => chatglue.submit(m),
        .chat_cancel => chatglue.cancel(m),
        .chat_clear => {
            m.chat.log.clear();
            m.chat.log.setHasKey(chatglue.hasKey(m));
        },
        .toggle_tool => |i| m.chat.log.toggleExpanded(i),
        .model_pick => |p| {
            m.model_pick = p;
            m.menu = .none;
            chatglue.applyConfig(m);
            saveSettings(m);
        },
        .key_save => {
            chatglue.setKey(m, m.key_ed.content());
            m.key_ed.clear();
            m.show_key_card = false;
            m.focus = if (m.key_len > 0) .chat else .none;
            _ = m.fx.storageSet(.key_store, "kerf.key", m.key_buf[0..m.key_len]);
            m.setStatus("{s}", .{if (m.key_len > 0) "API KEY STORED LOCALLY" else "API KEY CLEARED"});
        },
        .key_forget => {
            chatglue.setKey(m, "");
            _ = m.fx.storageSet(.key_store, "kerf.key", "");
            m.setStatus("API KEY FORGOTTEN", .{});
        },
        .toggle_key_card => {
            m.show_key_card = !m.show_key_card;
            m.focus = if (m.show_key_card) .key else .none;
        },
        .toggle_demo => {
            m.demo = !m.demo;
            saveSettings(m);
            m.setStatus("{s}", .{if (m.demo) "DEMO MODE: SCRIPTED CLAUDE, NO KEY NEEDED" else "DEMO MODE OFF"});
        },
        .attach_pick => _ = m.fx.openFile(.attach_image, "image/png,image/jpeg"),
        .attach_remove => |i| {
            if (i >= m.n_attach) return;
            gpa.free(m.attachments[i].bytes);
            var j: usize = i;
            while (j + 1 < m.n_attach) : (j += 1) m.attachments[j] = m.attachments[j + 1];
            m.n_attach -= 1;
        },
        .console_scroll_by => |dy| {
            m.console_scroll = clamp(m.console_scroll + dy, m.console_content - m.console_viewport);
            m.stick_bottom = m.console_scroll >= m.console_content - m.console_viewport - 2;
        },
        .console_extent => |e| {
            m.console_viewport = e[0];
            m.console_content = e[1];
            const max = @max(e[1] - e[0], 0);
            m.console_scroll = if (m.stick_bottom) max else clamp(m.console_scroll, max);
        },

        // ── time + effects ──
        .submit => submit(m),
        .tick => tick(m),
        .fx => |r| effectResult(m, r),
    }
}

fn clamp(v: f32, max: f32) f32 {
    return std.math.clamp(v, 0, @max(max, 0));
}

fn tick(m: *Model) void {
    m.ticks +%= 1;
    if (m.status_age < 65000) m.status_age += 1;
    m.fx.pruneFireAndForget();
    chatglue.tick(m);
    if (m.ticks % 300 == 0) _ = m.fx.clock(); // re-sync the wall clock every minute
}

fn key(m: *Model, k: teak.SpecialKey) void {
    if (k == .escape) {
        if (m.menu != .none) {
            m.menu = .none;
            return;
        }
        if (m.focus != .none) {
            m.focus = .none;
            return;
        }
        flow.clearSelection(m);
        return;
    }
    if (focusedEditor(m)) |ed| {
        switch (k) {
            .backspace => ed.backspace(),
            .delete => ed.delete(),
            .left => ed.move(.left, false),
            .right => ed.move(.right, false),
            .home => ed.move(.home, false),
            .end => ed.move(.end, false),
            .shift_left => ed.move(.left, true),
            .shift_right => ed.move(.right, true),
            .shift_home => ed.move(.home, true),
            .shift_end => ed.move(.end, true),
            .ctrl_a => ed.selectAll(),
            else => {},
        }
        return;
    }
    switch (k) {
        .ctrl_z => flow.undo(m),
        else => {},
    }
}

/// Enter pressed.
pub fn submit(m: *Model) void {
    switch (m.focus) {
        .chat => chatglue.submit(m),
        .key => update(m, .key_save),
        .note_text => flow.commitNoteText(m),
        .cite_code, .cite_section, .cite_title => citeAdd(m),
        .none => {},
    }
}

fn citeAdd(m: *Model) void {
    const sec = std.mem.trim(u8, m.cite_sec_ed.content(), " ");
    if (sec.len == 0) {
        m.setStatus("ENTER A SECTION NUMBER FOR THE CITATION", .{});
        return;
    }
    const code_in = std.mem.trim(u8, m.cite_code_ed.content(), " ");
    const code = if (code_in.len > 0) code_in else (if (m.doc.info) |i| firstWord(i.jurisdiction) else "IRC");
    const edition: i64 = if (m.doc.info) |i| editionOf(i.jurisdiction) else 2021;
    flow.citeEdit(m, .{ .append = .{
        .code = code,
        .edition = edition,
        .section = sec,
        .title = std.mem.trim(u8, m.cite_title_ed.content(), " "),
    } }, "Add citation");
    m.cite_sec_ed.clear();
    m.cite_title_ed.clear();
}

fn firstWord(s: []const u8) []const u8 {
    const i = std.mem.indexOfScalar(u8, s, ' ') orelse s.len;
    return if (i == 0) "IRC" else s[0..i];
}

fn editionOf(s: []const u8) i64 {
    const i = std.mem.indexOfScalar(u8, s, ' ') orelse return 2021;
    return std.fmt.parseInt(i64, s[i + 1 ..], 10) catch 2021;
}

// ── viewport events ────────────────────────────────────────────────

fn canvasEvent(m: *Model, ev: teak.CanvasEvent) void {
    if (ev.id == model.SCENE_ID) return scene3dEvent(m, ev);
    if (ev.id != model.CANVAS_ID) return;
    const v: viewport.Event = .{
        .kind = switch (ev.kind) {
            .down => .down,
            .move => .move,
            .up => .up,
            .wheel => .wheel,
            .leave => .leave,
            .layout => .layout,
        },
        .x = ev.x,
        .y = ev.y,
        .dx = ev.dx,
        .dy = ev.dy,
        .button = switch (ev.button) {
            .none => .none,
            .left => .left,
            .middle => .middle,
            .right => .right,
        },
        .ctrl = ev.mods.ctrl,
        .shift = ev.mods.shift,
        .w = ev.w,
        .h = ev.h,
    };
    if (ev.kind == .down) m.focus = .none;
    const out = m.vp.handle(v) catch return;
    m.cursor_model = m.vp.cursor;
    if (out.select) |s| {
        if (s.set) flow.select(m, s.id.slice()) else flow.clearSelection(m);
    }
    if (out.commit_note) |c| flow.commitNotePlace(m, c.id.slice(), c.place[0], c.place[1]);
}

fn scene3dEvent(m: *Model, ev: teak.CanvasEvent) void {
    switch (ev.kind) {
        .layout => {
            m.scene_w = ev.w;
            m.scene_h = ev.h;
        },
        .wheel => {
            const k: f32 = if (ev.mods.ctrl) 0.01 else 0.0015;
            m.orbit.zoom(@exp(ev.dy * k));
        },
        .move => {
            if (ev.buttons.left and !ev.mods.shift) m.orbit.rotate(ev.dx, ev.dy);
            if (ev.buttons.middle or ev.buttons.right or (ev.buttons.left and ev.mods.shift)) m.orbit.panPx(ev.dx, ev.dy, ev.h);
        },
        .down => m.focus = .none,
        else => {},
    }
}

// ── effects ────────────────────────────────────────────────────────

fn effectResult(m: *Model, r: teak.EffectResult) void {
    switch (r) {
        .http => |h| {
            _ = m.fx.done(h.id);
            chatglue.onHttp(m, .{
                .status = h.status,
                .body = h.body,
                .err = if (h.status == 0) (if (h.err.len > 0) h.err else "network error") else null,
            });
        },
        .storage_value => |s| {
            const kind = m.fx.done(s.id) orelse return;
            switch (kind) {
                .key_load => if (s.value) |v| {
                    chatglue.setKey(m, v);
                    if (m.key_len > 0) m.chat.log.setHasKey(true);
                },
                .settings_load => if (s.value) |v| loadSettings(m, v),
                else => {},
            }
        },
        .clock => |c| {
            _ = m.fx.done(c.id);
            m.wall_base_ms = c.unix_ms;
            m.wall_base_tick = m.ticks;
            m.utc_offset_min = c.utc_offset_min;
        },
        .file_opened => |f| {
            const kind = m.fx.done(f.id) orelse return;
            switch (kind) {
                .open_doc => {
                    var label_buf: [48]u8 = undefined;
                    const label = std.fmt.bufPrint(&label_buf, "FILE {s}", .{f.name}) catch "FILE";
                    _ = flow.loadDoc(m, f.bytes, label);
                },
                .attach_image => attachImage(m, f.name, f.mime, f.bytes, 0, 0),
                else => {},
            }
        },
        .file_cancelled => |c| _ = m.fx.done(c.id),
        .downloaded => |d| {
            _ = m.fx.done(d.id);
            if (!d.ok) m.setStatus("DOWNLOAD FAILED", .{});
        },
        .dropped => |d| switch (d.kind) {
            .image => attachImage(m, d.name, d.mime, d.bytes, 0, 0), // TODO(E): d.width/d.height
            .file => {
                if (std.mem.endsWith(u8, d.name, ".json")) {
                    var label_buf: [48]u8 = undefined;
                    const label = std.fmt.bufPrint(&label_buf, "FILE {s}", .{d.name}) catch "FILE";
                    _ = flow.loadDoc(m, d.bytes, label);
                }
            },
            .text => {},
        },
        .pasted_text => |p| {
            if (focusedEditor(m)) |ed| ed.insert(p.text);
        },
    }
}

fn attachImage(m: *Model, name: []const u8, mime: []const u8, bytes: []const u8, w: u32, h: u32) void {
    if (m.n_attach >= model.MAX_ATTACH) {
        m.setStatus("AT MOST {d} IMAGES PER MESSAGE", .{model.MAX_ATTACH});
        return;
    }
    if (bytes.len > 4_500_000) {
        m.setStatus("IMAGE TOO LARGE (4.5 MB LIMIT)", .{});
        return;
    }
    const copy = gpa.dupe(u8, bytes) catch return;
    var a: model.Attachment = .{ .bytes = copy, .width = w, .height = h, .mime_jpeg = std.mem.indexOf(u8, mime, "jpeg") != null or std.mem.indexOf(u8, mime, "jpg") != null };
    const n = @min(name.len, a.name.len);
    @memcpy(a.name[0..n], name[0..n]);
    a.name_len = @intCast(n);
    m.attachments[m.n_attach] = a;
    m.n_attach += 1;
    m.setStatus("IMAGE ATTACHED ({d} KB)", .{bytes.len / 1024});
}

fn bootSample(m: *Model, id: []const u8) void {
    for (model.samples, 0..) |s, i| {
        if (std.mem.eql(u8, s.id, id)) {
            _ = flow.loadDoc(m, model.samples[i].json, s.label);
            return;
        }
    }
}

fn saveSettings(m: *Model) void {
    var buf: [48]u8 = undefined;
    const s = std.fmt.bufPrint(&buf, "model={s};demo={d}", .{ @tagName(m.model_pick), @intFromBool(m.demo) }) catch return;
    _ = m.fx.storageSet(.settings_store, "kerf.settings", s);
}

fn loadSettings(m: *Model, v: []const u8) void {
    var it = std.mem.splitScalar(u8, v, ';');
    while (it.next()) |kv| {
        if (std.mem.startsWith(u8, kv, "model=")) {
            if (std.mem.eql(u8, kv[6..], "sonnet_5_5")) m.model_pick = .sonnet_5_5;
            if (std.mem.eql(u8, kv[6..], "opus_5_5")) m.model_pick = .opus_5_5;
        }
        if (std.mem.startsWith(u8, kv, "demo=")) m.demo = kv.len > 5 and kv[5] == '1';
    }
    chatglue.applyConfig(m);
}

// ── files ──────────────────────────────────────────────────────────

fn baseName(m: *const Model, buf: []u8) []const u8 {
    const info = m.doc.info orelse return "kerf";
    return std.fmt.bufPrint(buf, "{s}", .{info.id}) catch "kerf";
}

fn saveFile(m: *Model) void {
    m.menu = .none;
    if (!m.ready) return;
    var nb: [96]u8 = undefined;
    var name_buf: [128]u8 = undefined;
    const name = std.fmt.bufPrint(&name_buf, "{s}.kerf.json", .{baseName(m, &nb)}) catch "doc.kerf.json";
    if (m.fx.download(name, "application/json", m.doc.doc) != null) m.setStatus("SAVED {s} ({d} KB)", .{ name, (m.doc.doc.len + 1023) / 1024 });
}

fn exportAs(m: *Model, f: model.ExportFmt) void {
    m.menu = .none;
    if (!m.ready) return;
    const vid = m.activeViewId() orelse return;
    const fmt = @tagName(f);
    const res = m.doc.exportBytes(vid, fmt, f == .pdf);
    switch (res) {
        .err => |e| {
            defer gpa.free(e);
            m.setStatus("EXPORT {s} FAILED: {s}", .{ fmt, flow.engineMessage(e) });
        },
        .ok => |bytes| {
            defer gpa.free(bytes);
            var nb: [96]u8 = undefined;
            var name_buf: [160]u8 = undefined;
            const name = std.fmt.bufPrint(&name_buf, "{s}-{s}.{s}", .{ baseName(m, &nb), vid, fmt }) catch "export";
            const mime = switch (f) {
                .dxf => "application/dxf",
                .pdf => "application/pdf",
                .svg => "image/svg+xml",
            };
            for (name_buf[0..name.len]) |*c| c.* = std.ascii.toUpper(c.*);
            if (m.fx.download(name, mime, bytes) != null) {
                m.setStatus("EXPORTED {s} ({d} KB)", .{ name, (bytes.len + 1023) / 1024 });
                m.chat.log.addNotice(m.nowMs(), std.fmt.bufPrint(&nb, "EXPORTED {s}", .{name}) catch "EXPORTED") catch {};
            } else m.setStatus("EFFECT QUEUE FULL", .{});
        },
    }
}
