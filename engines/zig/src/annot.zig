//! Annotations (SPEC 6, 16): leader notes with deterministic column layout, dimensions, labels, and the title block under a view.
//!
//! This file holds the layout context (`Env`) and the main entry `annotate` (parse, route, repair, assemble); the parts live in
//! `annot/`: `text` (glyph folding, wrapping, text boxes), `notes` (landing, obstacles, routing, citations), `dims` (one
//! dimension and the stacking of colliding ones), `repair` (the bounded repair loop), `knockout` (hatch under text) and
//! `title` (the title bubble). `shape.zig` has the landing geometry shared with the iso view.

const std = @import("std");
const json = @import("json.zig");
const geom = @import("geom.zig");
const clip = @import("clip.zig");
const model = @import("model.zig");
const scene_mod = @import("scene.zig");
const style_mod = @import("style.zig");
const pen_mod = @import("pen.zig");
const Pen = pen_mod.Pen;
const font_mod = @import("font.zig");
const view_mod = @import("view.zig");
const section = @import("section.zig");
const shape_mod = @import("shape.zig");
pub const Shape = shape_mod.Shape;
const drawing = @import("drawing.zig");
const units = @import("units.zig");
const cast = @import("num.zig");
const limits = @import("limits.zig");
const route = @import("route.zig");
const thinland = @import("thinland.zig");
const compile_mod = @import("compile.zig");
const iso_mod = @import("iso.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Pt = geom.Pt;
const Box = geom.Box;
const Item = drawing.Item;
// annot/text.zig.zig: text helpers
const text_mod = @import("annot/text.zig");
pub const asciiFold = text_mod.asciiFold;
const layerName = text_mod.layerName;
const textItem = text_mod.textItem;
const pathItem = text_mod.pathItem;
const upperIf = text_mod.upperIf;
pub const textPoly = text_mod.textPoly;
pub const itemsBox = text_mod.itemsBox;
const wrap = text_mod.wrap;

// annot/notes.zig.zig: leader notes
const notes_mod = @import("annot/notes.zig");
const textTooLong = notes_mod.textTooLong;
const targetLanding = notes_mod.targetLanding;
const NoteIn = notes_mod.NoteIn;
const Obstacle = notes_mod.Obstacle;
const Meta = notes_mod.Meta;
const LabelSpec = notes_mod.LabelSpec;
const clearOfLeaders = notes_mod.clearOfLeaders;
pub const reportHits = notes_mod.reportHits;
const prepNotes = notes_mod.prepNotes;
const routeNotes = notes_mod.routeNotes;
const emitNotes = notes_mod.emitNotes;
const legendItems = notes_mod.legendItems;
const noteText = notes_mod.noteText;

// annot/dims.zig.zig: dimensions
const dims_mod = @import("annot/dims.zig");
const DimSpec = dims_mod.DimSpec;
const labelItems = dims_mod.labelItems;
pub const polysOverlap = dims_mod.polysOverlap;
const baseHits = dims_mod.baseHits;
const stackDims = dims_mod.stackDims;

// annot/repair.zig.zig: layout repair
const repair_mod = @import("annot/repair.zig");
const DimLab = repair_mod.DimLab;
const renderDimLabels = repair_mod.renderDimLabels;
const repairDim = repair_mod.repairDim;
const repairLabel = repair_mod.repairLabel;
const Best = repair_mod.Best;

// annot/knockout.zig.zig: hatch knockout
const knockout_mod = @import("annot/knockout.zig");
const knockHatch = knockout_mod.knockHatch;

// annot/title.zig.zig: title block
const title_mod = @import("annot/title.zig");
pub const SheetInfo = title_mod.SheetInfo;
pub const titleItems = title_mod.titleItems;

pub const Landing = union(enum) {
    section: *section.Section,
    iso: *iso_mod.Iso,
};

pub const Env = struct {
    a: Allocator,
    style: *const style_mod.Style,
    font: *const font_mod.Font,
    scene: *scene_mod.Scene,
    spec: *const view_mod.ViewSpec,
    S: f64,
    crop: Box,
    diags: *model.Diags,
    landing: Landing,
    unverified: bool = false,
    unknown_glyph: bool = false,
    /// Extents of the annotation groups (model units), filled by `annotate` (used by the view-fit check).
    notes_box: Box = .{},
    dims_box: Box = .{},
    labels_box: Box = .{},
    /// Per-annotation extents (model units) for the view-fit diagnostic.
    ann_boxes: std.ArrayList(AnnBox) = .empty,
    /// Layout effort shared by every pass and trial of one view build (null: each routing call gets its own default budget).
    work: ?*route.Work = null,
};

pub const AnnKind = enum { note, dim, label };
pub const AnnBox = struct { id: []const u8, kind: AnnKind, box: Box, offset: f64 = 0 };

pub const DimDir = enum { h, v, aligned };

pub const dim_variants: usize = 6;

const max_repair_passes: usize = 8;

/// Annotate a view. Returns the annotation items (notes interleaved after their annotation's own
/// items, in view order). Hatch lines in `base_items` are knocked out under dimension text and labels.
///
/// SPEC 20: dimensions are stacked (`stackDims`), notes routed (`route.route`), and when leaders still hit
/// dimension text or labels, a bounded repair loop (at most 8 passes) pushes those dimensions out and
/// moves those labels, re-routing after each pass; the pass with the fewest hits wins.
pub fn annotate(env: *Env, base_items: []Item) Allocator.Error![]const Item {
    const a = env.a;
    const spec = env.spec;
    const vid = spec.id;
    var notes: std.ArrayList(NoteIn) = .empty;
    var note_slot: std.ArrayList(?usize) = .empty;
    var per: std.ArrayList(std.ArrayList(Item)) = .empty;
    var meta: std.ArrayList(Meta) = .empty;
    var dspecs: std.ArrayList(DimSpec) = .empty;
    var lspecs: std.ArrayList(LabelSpec) = .empty;
    var cur_meta = Meta{};
    var seen: std.ArrayList([]const u8) = .empty;
    const types = [_][]const u8{ "note", "dim", "label" };
    for (spec.annotations, 0..) |an, k| {
        const its: std.ArrayList(Item) = .empty;
        var slot: ?usize = null;
        cur_meta = .{};
        defer {
            per.append(a, its) catch {};
            note_slot.append(a, slot) catch {};
            meta.append(a, cur_meta) catch {};
        }
        const id = (if (an.get("id")) |x| x.str() else null) orelse {
            env.diags.add(.@"error", "E_PARAM", null, try a.print("views/{s}/annotations/{d}", .{ vid, k }), "annotation {d} of view {s} needs a string \"id\"", .{ k, vid });
            continue;
        };
        const apath = try a.print("views/{s}/annotations/{s}", .{ vid, id });
        var dup = false;
        for (seen.items) |s| if (std.mem.eql(u8, s, id)) {
            dup = true;
        };
        if (dup) {
            env.diags.add(.@"error", "E_DUP_ID", id, apath, "duplicate annotation id '{s}' in view {s}", .{ id, vid });
            continue;
        }
        try seen.append(a, id);
        const ty = (if (an.get("type")) |x| x.str() else null) orelse "";
        if (std.mem.eql(u8, ty, "note")) {
            const text0 = (if (an.get("text")) |x| x.str() else null) orelse {
                env.diags.add(.@"error", "E_PARAM", id, apath, "note '{s}' needs a string \"text\"", .{id});
                continue;
            };
            if (try textTooLong(env, "note", id, apath, text0)) continue;
            const text = try scene_mod.whereOccursText(a, env.scene, if (an.get("target")) |x| (x.str() orelse "") else "", text0);
            const cites: []const json.Value = if (an.get("cite")) |c| (c.arr() orelse &.{}) else &.{};
            const full = try noteText(env, text, cites);
            const target = (if (an.get("target")) |x| x.str() else null) orelse "";
            var landing: ?[]const V2 = null;
            var movable = true;
            const at_v: ?json.Value = if (an.get("at")) |v| (if (v == .null) null else v) else null;
            if (at_v) |atv| {
                if (env.scene.point(atv, id, try a.print("{s}/at", .{apath}))) |p| landing = try a.dupe(V2, &.{p});
                movable = false;
            } else if (target.len == 0) {
                env.diags.add(.@"error", "E_PARAM", id, apath, "note '{s}' needs a \"target\" (component id or comp.part) or an \"at\" point", .{id});
                continue;
            } else {
                var cid = target;
                if (std.mem.indexOfAny(u8, target, ".#")) |d| cid = target[0..d];
                if (env.scene.find(cid) == null) {
                    const ids = try env.scene.compIds(a);
                    const hint = if (model.nearest(a, cid, ids)) |n| try a.print(" Did you mean '{s}'?", .{n}) else try a.print(" Components: {s}", .{scene_mod.joinIds(a, ids)});
                    env.diags.add(.@"error", "E_REF_UNKNOWN", id, try a.print("{s}/target", .{apath}), "note '{s}' target '{s}' is not a component.{s}", .{ id, target, hint });
                    continue;
                }
                landing = try targetLanding(env, target);
            }
            var place: ?V2 = null;
            if (an.get("place")) |pv| if (pv != .null) {
                place = model.offsetPairOrDiag(a, env.diags, id, try a.print("{s}/place", .{apath}), "place", pv) orelse continue;
            };
            var column: ?route.Side = null;
            if (an.get("column")) |cv| if (cv != .null) {
                const cs = cv.str() orelse "";
                if (std.mem.eql(u8, cs, "left")) {
                    column = .left;
                } else if (std.mem.eql(u8, cs, "right")) {
                    column = .right;
                } else {
                    env.diags.addFix(.warning, "W_PARAM", id, try a.print("{s}/column", .{apath}), "note '{s}' in view {s}: \"column\" must be \"left\" or \"right\" (got {s}); ignored", .{ id, vid, if (cv.str()) |sv| try a.print("\"{s}\"", .{sv}) else "a non-string value" }, "use \"column\": \"left\" or \"right\", or omit it to let the layout choose (view notes_side)");
                }
            };
            if (landing) |l| {
                slot = notes.items.len;
                try notes.append(a, .{ .id = id, .text = full, .cands = l, .movable = movable, .place = place, .column = column });
            } else if (at_v == null) {
                env.diags.addFix(.warning, "W_NOTE_TARGET", id, apath, "note '{s}' in view {s}: target '{s}' is not visible in this view (outside the crop, behind the cut plane, or hidden). The note was not drawn.", .{ id, vid, target }, "move the view crop or cut_z so the target is visible, change target, or give the note an explicit \"at\" Ref");
            }
        } else if (std.mem.eql(u8, ty, "dim")) {
            if (spec.kind == .iso) continue;
            const fv = an.get("from");
            const tv = an.get("to");
            if (fv == null or tv == null) {
                env.diags.add(.@"error", "E_PARAM", id, apath, "dim '{s}' needs \"from\" and \"to\" points (Refs)", .{id});
                continue;
            }
            const from = env.scene.point(fv.?, id, try a.print("{s}/from", .{apath})) orelse continue;
            const to = env.scene.point(tv.?, id, try a.print("{s}/to", .{apath})) orelse continue;
            const dir_s = (if (an.get("dir")) |x| x.str() else null) orelse "h";
            const dir: DimDir = if (std.mem.eql(u8, dir_s, "h")) .h else if (std.mem.eql(u8, dir_s, "v")) .v else if (std.mem.eql(u8, dir_s, "aligned")) .aligned else {
                env.diags.add(.@"error", "E_PARAM", id, try a.print("{s}/dir", .{apath}), "dim dir must be \"h\", \"v\" or \"aligned\" (got \"{s}\")", .{dir_s});
                continue;
            };
            const off = model.lengthOrDiag(a, env.diags, id, try a.print("{s}/offset", .{apath}), an.get("offset"), "dim offset", 0) orelse continue;
            const text: ?[]const u8 = if (an.get("text")) |x| x.str() else null;
            const ds = DimSpec{ .k = k, .id = id, .from = from, .to = to, .dir = dir, .off0 = off, .text = text };
            cur_meta = .{ .id = id, .kind = .dim, .axis = ds.axis(), .off = off, .owner = dspecs.items.len };
            try dspecs.append(a, ds);
        } else if (std.mem.eql(u8, ty, "label")) {
            const text = (if (an.get("text")) |x| x.str() else null) orelse {
                env.diags.add(.@"error", "E_PARAM", id, apath, "label '{s}' needs a string \"text\"", .{id});
                continue;
            };
            if (try textTooLong(env, "label", id, apath, text)) continue;
            const atv = an.get("at") orelse {
                env.diags.add(.@"error", "E_PARAM", id, apath, "label '{s}' needs \"at\": a Ref or [x, y]", .{id});
                continue;
            };
            var p = env.scene.point(atv, id, try a.print("{s}/at", .{apath})) orelse continue;
            var loff = V2.init(0, 0);
            if (an.get("offset")) |ov| if (ov != .null) {
                loff = model.offsetPairOrDiag(a, env.diags, id, try a.print("{s}/offset", .{apath}), "label offset", ov) orelse continue;
                p = p.add(loff);
            };
            cur_meta = .{ .id = id, .kind = .label, .off = loff.x, .off2 = loff.y, .owner = lspecs.items.len };
            const base: ?V2 = switch (env.landing) {
                .section => p,
                .iso => |iso| iso.project(p),
            };
            try lspecs.append(a, .{ .k = k, .id = id, .text = text, .base = base, .off = loff });
        } else {
            env.diags.add(.@"error", "E_PARAM", id, try a.print("{s}/type", .{apath}), "annotation '{s}' has type \"{s}\"; use one of {s}", .{ id, ty, model.joinQuoted(a, &types) });
        }
    }
    var base_segs: std.ArrayList([2]V2) = .empty;
    for (base_items) |it| if (it == .path) {
        const pts = it.path.pts;
        if (pts.len < 2) continue;
        for (pts[0 .. pts.len - 1], 0..) |p, i| try base_segs.append(a, .{ p.v(), pts[i + 1].v() });
        if (it.path.closed) try base_segs.append(a, .{ pts[pts.len - 1].v(), pts[0].v() });
    };
    const dl = DimLab{ .dspecs = dspecs.items, .lspecs = lspecs.items, .base_segs = base_segs.items };
    const dpush = try a.alloc(f64, dspecs.items.len);
    @memset(dpush, 0);
    const lpush = try a.alloc(V2, lspecs.items.len);
    @memset(lpush, V2.init(0, 0));
    const outs = try a.alloc(std.ArrayList(Item), notes.items.len);
    for (outs) |*o| o.* = .empty;
    const prep = try prepNotes(env, notes.items);

    // route, then repair what hits dimension text or labels (bounded, deterministic)
    var best: ?Best = null;
    var stale: usize = 0;
    var pass: usize = 0;
    while (pass < max_repair_passes) : (pass += 1) {
        const rd = try renderDimLabels(env, dl, per.items, meta.items, dpush, lpush);
        const r = try routeNotes(env, prep, rd.ext, rd.obstacles, rd.soft, pass > 0);
        if (best == null or r.hits.len < best.?.r.hits.len) {
            best = .{ .r = r, .rd = rd };
            stale = 0;
        } else {
            stale += 1;
        }
        // stop when clean, or when two passes in a row did not get better
        if (r.hits.len == 0 or stale >= 2) break;
        const touched_d = try a.alloc(bool, dspecs.items.len);
        const touched_l = try a.alloc(bool, lspecs.items.len);
        @memset(touched_d, false);
        @memset(touched_l, false);
        var changed = false;
        for (r.hits) |ht| {
            if (ht.kind != .dim and ht.kind != .label) continue;
            const o = rd.obstacles[ht.other];
            if (o.kind == .dim) {
                if (touched_d[o.owner]) continue;
                touched_d[o.owner] = true;
                if (repairDim(env, o, r.leaders, prep.g.h, dpush, dspecs.items)) changed = true;
            } else {
                if (touched_l[o.owner]) continue;
                touched_l[o.owner] = true;
                if (repairLabel(o, rd.obstacles, r.leaders, prep.g.h, lpush, base_segs.items)) changed = true;
            }
        }
        if (!changed) break;
    }
    const fin = best.?;
    const rd = fin.rd;
    try reportHits(env, notes.items, rd.obstacles, fin.r, prep.g);
    for (rd.per, 0..) |its, k| {
        const bx = itemsBox(env.font, its.items);
        const m = rd.meta[k];
        if (m.kind == .dim) {
            env.dims_box.addBox(bx);
            try env.ann_boxes.append(a, .{ .id = m.id, .kind = .dim, .box = bx, .offset = m.off });
        }
        if (m.kind == .label) {
            env.labels_box.addBox(bx);
            try env.ann_boxes.append(a, .{ .id = m.id, .kind = .label, .box = bx });
        }
    }
    try emitNotes(env, notes.items, prep, fin.r, outs);
    for (outs, 0..) |o, k| {
        const bx = itemsBox(env.font, o.items);
        env.notes_box.addBox(bx);
        try env.ann_boxes.append(a, .{ .id = notes.items[k].id, .kind = .note, .box = bx });
    }
    try knockHatch(a, base_items, rd.knock);
    var result: std.ArrayList(Item) = .empty;
    for (rd.per, 0..) |its, k| {
        try result.appendSlice(a, its.items);
        if (note_slot.items[k]) |sl| try result.appendSlice(a, outs[sl].items);
    }
    if (env.style.notes_mode_keynote and notes.items.len > 0) try legendItems(env, notes.items, &result);
    return result.items;
}
