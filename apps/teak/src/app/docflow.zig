//! Document flows: load, designer edits, refreshing every derived view
//! (drawing, mesh, inspector text) after the document changes.

const std = @import("std");
const teak = @import("teak");
const alloc = @import("alloc.zig");
const model = @import("model.zig");
const docops = @import("docops.zig");
const docinfo = @import("docinfo.zig");
const ident = @import("ident.zig");
const draw = @import("../draw/mod.zig");

const Model = model.Model;
const gpa = alloc.gpa;

fn firstLine(s: []const u8) []const u8 {
    const t = std.mem.trim(u8, s, " \r\n");
    const nl = std.mem.indexOfScalar(u8, t, '\n') orelse t.len;
    return t[0..@min(nl, 90)];
}

/// Pull the human message out of an engine error JSON `{"error":{"message":..}}`.
pub fn engineMessage(text: []const u8) []const u8 {
    if (std.mem.indexOf(u8, text, "\"message\":\"")) |i| {
        const rest = text[i + 11 ..];
        const end = std.mem.indexOfScalar(u8, rest, '"') orelse rest.len;
        return rest[0..@min(end, 120)];
    }
    return firstLine(text);
}

pub fn setSource(m: *Model, s: []const u8) void {
    const n = @min(s.len, m.doc_source.len);
    @memcpy(m.doc_source[0..n], s[0..n]);
    m.doc_source_len = @intCast(n);
}

pub fn docNowSec(m: *const Model) i64 {
    return @divFloor(m.nowMs(), 1000);
}

/// Replace the whole document (open / sample).
pub fn loadDoc(m: *Model, json: []const u8, source: []const u8) bool {
    var err: ?[]u8 = null;
    const ok = m.doc.load(json, &err) catch {
        m.setStatus("OUT OF MEMORY LOADING DOCUMENT", .{});
        return false;
    };
    if (!ok) {
        if (err) |e| {
            defer gpa.free(e);
            m.setStatus("NOT A KERF DOCUMENT: {s}", .{engineMessage(e)});
        }
        return false;
    }
    m.ready = true;
    m.tab = 0;
    m.sel = .{};
    m.note_sel = -1;
    m.drawing_doc_rev = std.math.maxInt(u32);
    m.mesh_doc_rev = std.math.maxInt(u32);
    setSource(m, source);
    afterDocChange(m, true);
    // Claude does not see the swap unless told.
    addEditNote(m, "opened a different document (see kerf_inspect doc)");
    m.setStatus("LOADED {s}", .{source});
    return true;
}

pub fn addEditNote(m: *Model, why: []const u8) void {
    const copy = gpa.dupe(u8, why) catch return;
    m.edit_notes.append(gpa, copy) catch gpa.free(copy);
}

/// Apply a designer edit. Returns true on success.
pub fn designerApply(m: *Model, ops_json: []const u8, why: []const u8) bool {
    const r = m.doc.apply(ops_json, why, .designer, docNowSec(m)) catch {
        m.setStatus("OUT OF MEMORY", .{});
        return false;
    };
    defer gpa.free(r.text);
    if (!r.ok) {
        m.setStatus("REJECTED: {s}", .{firstLineAfterFailed(r.text)});
        return false;
    }
    addEditNote(m, why);
    afterDocChange(m, false);
    m.setStatus("{s}", .{why});
    return true;
}

fn firstLineAfterFailed(text: []const u8) []const u8 {
    var it = std.mem.splitScalar(u8, text, '\n');
    _ = it.next();
    while (it.next()) |l| if (l.len > 0 and (std.mem.startsWith(u8, l, "ERROR") or std.mem.indexOf(u8, l, "error") != null)) return l[0..@min(l.len, 100)];
    return firstLine(text);
}

pub fn undo(m: *Model) void {
    if (!m.ready) return;
    const ok = m.doc.undo() catch false;
    if (!ok) {
        m.setStatus("NOTHING TO UNDO", .{});
        return;
    }
    addEditNote(m, "undid the last edit");
    afterDocChange(m, false);
    m.setStatus("UNDONE. REV {d}", .{m.doc.rev});
}

/// Recompute everything derived from the document.
pub fn afterDocChange(m: *Model, refit: bool) void {
    const info = &(m.doc.info orelse return);
    // Tab bounds: views, then 3D, then SHEET.
    const max_tab: u8 = @intCast(info.views.len + 1);
    if (m.tab > max_tab) m.tab = 0;
    // Annotation ids that are notes (drag-eligible) in the first/active view.
    rebuildNoteIds(m);
    m.mesh_doc_rev = std.math.maxInt(u32);
    refreshActive(m, refit);
    refreshInspectText(m);
    if (m.note_sel >= 0) syncNoteEditor(m);
}

fn rebuildNoteIds(m: *Model) void {
    if (m.note_ids.len > 0) gpa.free(m.note_ids);
    m.note_ids = &.{};
    const info = &(m.doc.info orelse return);
    const vi: usize = switch (m.tabKind()) {
        .view => |i| i,
        else => 0,
    };
    if (vi >= info.views.len) return;
    const notes = info.views[vi].notes;
    var list = gpa.alloc([]const u8, notes.len) catch return;
    var n: usize = 0;
    for (notes) |nt| {
        if (std.mem.eql(u8, nt.kind, "note")) {
            list[n] = nt.id;
            n += 1;
        }
    }
    m.note_ids = list[0..n];
    m.vp.note_ids = m.note_ids;
}

/// Refresh whatever the active tab shows.
pub fn refreshActive(m: *Model, refit: bool) void {
    switch (m.tabKind()) {
        .view => |i| loadDrawing(m, m.doc.info.?.views[i].id, false, refit),
        .sheet => loadDrawing(m, m.activeViewId() orelse return, true, refit),
        .d3 => refreshMesh(m),
        .none => {},
    }
}

pub fn loadDrawing(m: *Model, view_id: []const u8, sheet: bool, refit: bool) void {
    const res = if (sheet) callSheet(m, view_id) else m.doc.drawingJson(view_id);
    switch (res) {
        .err => |e| {
            defer gpa.free(e);
            m.vp.clearDrawing();
            m.setStatus("VIEW {s}: {s}", .{ view_id, engineMessage(e) });
        },
        .ok => |json| {
            defer gpa.free(json);
            m.vp.grid = !sheet;
            m.vp.setDrawing(json, refit) catch |e| {
                m.setStatus("DRAWING PARSE FAILED: {s}", .{@errorName(e)});
                return;
            };
            m.drawing_doc_rev = m.doc.rev;
            m.drawing_view = ident.Id.from(view_id);
        },
    }
}

fn callSheet(m: *Model, view_id: []const u8) @import("engine.zig").CallResult {
    const eng = @import("engine.zig");
    const req = (eng.Request{ .doc = m.doc.doc, .view = view_id }).build(gpa) catch return .{ .err = gpa.dupe(u8, "out of memory") catch &.{} };
    defer gpa.free(req);
    return m.doc.engine.call(gpa, "sheet_drawing", req);
}

pub fn refreshMesh(m: *Model) void {
    if (m.mesh_doc_rev == m.doc.rev and m.mesh != null) return;
    switch (m.doc.meshJson()) {
        .err => |e| {
            defer gpa.free(e);
            m.setStatus("3D: {s}", .{engineMessage(e)});
        },
        .ok => |json| {
            defer gpa.free(json);
            var mesh = draw.mesh.parse(gpa, json) catch |e| {
                m.setStatus("MESH PARSE FAILED: {s}", .{@errorName(e)});
                return;
            };
            const first = m.mesh == null;
            if (m.mesh) |old| {
                old.deinit();
                gpa.destroy(old);
            }
            const p = gpa.create(draw.Mesh) catch {
                mesh.deinit();
                return;
            };
            p.* = mesh;
            m.mesh = p;
            m.mesh_rev +%= 1;
            m.mesh_doc_rev = m.doc.rev;
            if (first) frameMesh(m);
        },
    }
}

pub fn frameMesh(m: *Model) void {
    const mesh = m.mesh orelse return;
    const b = mesh.bounds();
    if (b.isEmpty()) return;
    m.orbit.frame(.{ b.min[0], b.min[1], b.min[2] }, .{ b.max[0], b.max[1], b.max[2] });
}

pub fn selectTab(m: *Model, tab: u8) void {
    if (!m.ready) return;
    m.tab = tab;
    rebuildNoteIds(m);
    refreshActive(m, true);
}

// ── selection + inspector ──────────────────────────────────────────

pub fn refreshInspectText(m: *Model) void {
    if (m.insp_text.len > 0) gpa.free(m.insp_text);
    m.insp_text = &.{};
    m.insp_text_for = .{};
    const sel = m.sel.get() orelse return;
    // Only components have an inspect view.
    const info = &(m.doc.info orelse return);
    var is_comp = false;
    for (info.components) |c| if (std.mem.eql(u8, c.id, sel)) {
        is_comp = true;
    };
    if (!is_comp) return;
    var q: [128]u8 = undefined;
    const query = std.fmt.bufPrint(&q, "{{\"q\":\"component\",\"id\":\"{s}\"}}", .{sel}) catch return;
    switch (m.doc.inspectJson(query)) {
        .ok => |t| m.insp_text = t,
        .err => |t| {
            m.insp_text = t;
        },
    }
    m.insp_text_for = ident.Id.from(sel);
}

pub fn select(m: *Model, id: []const u8) void {
    m.sel = ident.MaybeId.of(id);
    m.vp.setSelection(m.sel) catch {};
    m.cursor_model = m.cursor_model;
    const info = &(m.doc.info orelse return);
    // Note?
    const vi: usize = switch (m.tabKind()) {
        .view => |i| i,
        else => 0,
    };
    m.note_sel = -1;
    if (vi < info.views.len) {
        for (info.views[vi].notes, 0..) |nt, i| if (std.mem.eql(u8, nt.id, id)) {
            m.note_sel = @intCast(i);
            m.insp = .notes;
            syncNoteEditor(m);
        };
    }
    if (m.note_sel < 0) {
        for (info.components) |c| if (std.mem.eql(u8, c.id, id)) {
            m.insp = .parts;
        };
    }
    refreshInspectText(m);
}

pub fn clearSelection(m: *Model) void {
    m.sel = .{};
    m.note_sel = -1;
    m.vp.setSelection(.{}) catch {};
    refreshInspectText(m);
}

pub fn currentNote(m: *const Model) ?docinfo.Note {
    const info = &(m.doc.info orelse return null);
    const vi: usize = switch (m.tabKind()) {
        .view => |i| i,
        else => 0,
    };
    if (vi >= info.views.len or m.note_sel < 0) return null;
    const notes = info.views[vi].notes;
    const i: usize = @intCast(m.note_sel);
    if (i >= notes.len) return null;
    return notes[i];
}

pub fn syncNoteEditor(m: *Model) void {
    const nt = currentNote(m) orelse return;
    m.note_ed.set(nt.text);
}

pub fn activeViewIdOrNull(m: *const Model) ?[]const u8 {
    return m.activeViewId();
}

pub fn commitNoteText(m: *Model) void {
    const nt = currentNote(m) orelse return;
    const vid = m.activeViewId() orelse return;
    const text = std.mem.trim(u8, m.note_ed.content(), " ");
    if (std.mem.eql(u8, text, nt.text)) return;
    const ops = docops.noteText(gpa, vid, nt.id, text) catch return;
    defer gpa.free(ops);
    var why_buf: [96]u8 = undefined;
    const why = std.fmt.bufPrint(&why_buf, "Edit note {s} text", .{nt.id}) catch "Edit note text";
    _ = designerApply(m, ops, why);
}

pub fn commitNotePlace(m: *Model, id: []const u8, x: f64, y: f64) void {
    const vid = m.activeViewId() orelse return;
    const ops = docops.notePlace(gpa, vid, id, x, y) catch return;
    defer gpa.free(ops);
    var why_buf: [96]u8 = undefined;
    const why = std.fmt.bufPrint(&why_buf, "Move note {s}", .{id}) catch "Move note";
    if (!designerApply(m, ops, why)) afterDocChange(m, false); // drop the drag preview
}

pub fn citeEdit(m: *Model, edit: docops.CiteEdit, why: []const u8) void {
    const nt = currentNote(m) orelse return;
    const vid = m.activeViewId() orelse return;
    const ops = docops.noteCites(gpa, vid, nt, edit) catch return;
    defer gpa.free(ops);
    _ = designerApply(m, ops, why);
}
