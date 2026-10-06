//! Annotations (SPEC 6, 16): leader notes with deterministic column layout, dimensions, labels,
//! and the title block under a view.

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
pub const asciiFold = text_mod.asciiFold;
const layerName = text_mod.layerName;
const textItem = text_mod.textItem;
const pathItem = text_mod.pathItem;
const upperIf = text_mod.upperIf;
pub const textPoly = text_mod.textPoly;
pub const itemsBox = text_mod.itemsBox;
const wrap = text_mod.wrap;
const text_mod = @import("annot/text.zig");
const knockHatch = knockout_mod.knockHatch;
const knockout_mod = @import("annot/knockout.zig");
pub const SheetInfo = title_mod.SheetInfo;
pub const titleItems = title_mod.titleItems;
const title_mod = @import("annot/title.zig");
const DimSpec = dims_mod.DimSpec;
const labelItems = dims_mod.labelItems;
pub const polysOverlap = dims_mod.polysOverlap;
const baseHits = dims_mod.baseHits;
const stackDims = dims_mod.stackDims;
const dims_mod = @import("annot/dims.zig");

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

// ---- small helpers ------------------------------------------------------------------------------------------

// ---- wrapping -------------------------------------------------------------------------------------------------

// ---- note landing ---------------------------------------------------------------------------------------------------

/// E_LIMIT for a note/label text over `limits.max_text_chars` (reports and returns true).
fn textTooLong(env: *Env, what: []const u8, id: []const u8, apath: []const u8, text: []const u8) Allocator.Error!bool {
    if (text.len <= limits.max_text_chars) return false;
    env.diags.addFix(.@"error", "E_LIMIT", id, apath, "{s}", .{try limits.message(env.a, try std.fmt.allocPrint(env.a, "characters in the text of {s} '{s}'", .{ what, id }), text.len, limits.max_text_chars, "A drawing note is a short phrase, not a paragraph.")}, "shorten the text to one phrase (about 130 characters or fewer) or split it into several notes");
    return true;
}

fn targetLanding(env: *Env, target: []const u8) Allocator.Error!?[]const V2 {
    var comp_id = target;
    var part: ?[]const u8 = null;
    var inst: ?u32 = null;
    if (std.mem.indexOfScalar(u8, target, '.')) |d| {
        comp_id = target[0..d];
        part = target[d + 1 ..];
    }
    if (std.mem.indexOfScalar(u8, comp_id, '#')) |h| {
        inst = std.fmt.parseInt(u32, comp_id[h + 1 ..], 10) catch null;
        comp_id = comp_id[0..h];
    }
    const comp = env.scene.find(comp_id) orelse return null;
    if (comp.state != .ok) return null;
    if (compile_mod.isOmitted(env.spec.omit, comp.id)) return null;
    var shapes: std.ArrayList(Shape) = .empty;
    var all_loops: std.ArrayList([]const V2) = .empty;
    switch (env.landing) {
        .section => |sec| {
            for (sec.prisms, 0..) |p, i| {
                if (p.comp != comp.index) continue;
                if (inst) |k| if (p.instance != k) continue;
                if (part) |pn| if (!std.mem.eql(u8, p.part, pn)) continue;
                const reg = try sec.visibleRegion(i);
                try all_loops.appendSlice(env.a, reg);
                try shapes.appendSlice(env.a, try shape_mod.shapesOf(env.a, reg));
            }
            if (shapes.items.len == 0) if (part) |pn| {
                if (scene_mod.Scene.partBox(comp, pn)) |lb| {
                    var bx = Box{};
                    const xf = comp.xfs[inst orelse 0];
                    const cs = [4]V2{ V2.init(lb.x0, lb.y0), V2.init(lb.x1, lb.y0), V2.init(lb.x1, lb.y1), V2.init(lb.x0, lb.y1) };
                    var pts: [4]V2 = undefined;
                    for (cs, 0..) |c, k| {
                        pts[k] = xf.apply(c);
                        bx.addPoint(pts[k].x, pts[k].y);
                    }
                    const loop = try env.a.dupe(V2, &pts);
                    clip.orientCcw(loop);
                    const reg = try clip.boolean(env.a, &.{loop}, &.{sec.crop_loop}, .intersect);
                    try all_loops.appendSlice(env.a, reg);
                    try shapes.appendSlice(env.a, try shape_mod.shapesOf(env.a, reg));
                }
            };
        },
        .iso => |iso| {
            const p = iso.landing(comp, inst, part) orelse return null;
            return try env.a.dupe(V2, &.{p});
        },
    }
    const lp = (try shape_mod.bandedLabelPoint(env.a, shapes.items, env.crop, env.style.text_height_in * env.S)) orelse return null;
    if (env.landing == .section) {
        if (try thinland.candidates(env.a, env.landing.section, comp, inst, part, lp.inset, env.style.text_height_in * env.S, lp.p)) |c| return c;
    }
    return try shape_mod.candidatesFor(env.a, shapes.items, lp.inset, lp.p, env.style.text_height_in * env.S);
}

// ---- notes ------------------------------------------------------------------------------------------------------------

const NoteIn = struct {
    id: []const u8,
    text: []const u8,
    cands: []const V2,
    /// False when the note has an explicit `at` (never nudged).
    movable: bool,
    place: ?V2,
    /// `column` hint of the note.
    column: ?route.Side = null,
};

/// A dimension text or label box that leaders must keep away from, with what is needed to propose a fix.
const Obstacle = struct {
    poly: [4]V2,
    id: []const u8,
    kind: route.ObstKind,
    text: []const u8,
    /// Index into the dimension specs (kind dim) or label specs (kind label).
    owner: usize = 0,
    /// dim: unit vector along which |offset| grows (before the sign of offset); label: unused
    axis: V2 = .{ .x = 0, .y = 0 },
    /// dim: current offset; label: current dx
    off: f64 = 0,
    /// label: current dy
    off2: f64 = 0,
};

const Meta = struct {
    id: []const u8 = "",
    kind: enum { none, dim, label } = .none,
    axis: V2 = .{ .x = 0, .y = 0 },
    off: f64 = 0,
    off2: f64 = 0,
    /// Index into the dim / label specs.
    owner: usize = 0,
};

const LabelSpec = struct {
    k: usize,
    id: []const u8,
    text: []const u8,
    /// Drawing-space position (author offset applied); null when an iso label does not project.
    base: ?V2,
    off: V2,
};

fn fmtPt(a: Allocator, p: V2) Allocator.Error![]const u8 {
    return std.fmt.allocPrint(a, "[{s}, {s}]", .{ try json.fmtNumberAlloc(a, @round(p.x * 100) / 100), try json.fmtNumberAlloc(a, @round(p.y * 100) / 100) });
}

fn clearOfLeaders(leaders: []const [3]V2, poly: [4]V2, shift: V2, h: f64) bool {
    var q = poly;
    for (&q) |*pt| pt.* = pt.add(shift);
    for (leaders) |l| if (route.polyBoxDist(l, &q) < h) return false;
    return true;
}

/// Offset proposals for a dimension or label that a leader runs into (what the repair loop could not fix).
fn obstacleFix(a: Allocator, o: Obstacle, leaders: []const [3]V2, h: f64) Allocator.Error!?[]const u8 {
    var k: usize = 1;
    switch (o.kind) {
        .dim => {
            const sgn: f64 = if (o.off >= 0) 1 else -1;
            while (k <= 24) : (k += 1) {
                const delta = @ceil(@as(f64, @floatFromInt(k)) * 0.5 * h * 4.0) / 4.0;
                for ([2]f64{ 1, -1 }) |dirn| {
                    const no = o.off + sgn * dirn * delta;
                    if (dirn < 0 and @abs(no) < 2 * h) continue;
                    if (clearOfLeaders(leaders, o.poly, o.axis.scale(sgn * dirn * delta), h)) {
                        return try std.fmt.allocPrint(a, "set dim '{s}' \"offset\": {s} (now {s})", .{ o.id, try json.fmtNumberAlloc(a, no), try json.fmtNumberAlloc(a, o.off) });
                    }
                }
            }
        },
        .label => {
            const dirs = [8]V2{ V2.init(0, 1), V2.init(0, -1), V2.init(1, 0), V2.init(-1, 0), V2.init(1, 1), V2.init(-1, 1), V2.init(1, -1), V2.init(-1, -1) };
            while (k <= 24) : (k += 1) {
                const delta = @ceil(@as(f64, @floatFromInt(k)) * 0.5 * h * 4.0) / 4.0;
                for (dirs) |dv| {
                    const dn = dv.norm().scale(delta);
                    if (clearOfLeaders(leaders, o.poly, dn, h)) {
                        return try std.fmt.allocPrint(a, "set label '{s}' \"offset\": [{s}, {s}] (now [{s}, {s}])", .{ o.id, try json.fmtNumberAlloc(a, @round((o.off + dn.x) * 100) / 100), try json.fmtNumberAlloc(a, @round((o.off2 + dn.y) * 100) / 100), try json.fmtNumberAlloc(a, o.off), try json.fmtNumberAlloc(a, o.off2) });
                    }
                }
            }
        },
    }
    return null;
}

fn noteFixText(a: Allocator, id: []const u8, fa: ?V2, fp: ?V2) Allocator.Error!?[]const u8 {
    if (fa) |p| return try std.fmt.allocPrint(a, "set note '{s}' \"at\": {s} (another point inside its target)", .{ id, try fmtPt(a, p) });
    if (fp) |p| return try std.fmt.allocPrint(a, "set note '{s}' \"place\": {s}", .{ id, try fmtPt(a, p) });
    return null;
}

fn reportHits(env: *Env, notes: []const NoteIn, obsts: []const Obstacle, r: route.Layout, g: route.Geo) Allocator.Error!void {
    const a = env.a;
    const vid = env.spec.id;
    const maxn: usize = 8;
    var shown: usize = 0;
    for (r.hits) |ht| {
        if (shown >= maxn) break;
        shown += 1;
        const me = notes[ht.note];
        const rel = if (ht.dist <= 1e-9) try a.dupe(u8, "crosses") else try std.fmt.allocPrint(a, "comes within {d:.2} in (paper) of", .{ht.dist / env.S});
        var what: []const u8 = undefined;
        var fix: ?[]const u8 = null;
        switch (ht.kind) {
            .leader => {
                const o = notes[ht.other];
                what = try std.fmt.allocPrint(a, "the leader of note '{s}'", .{o.id});
                fix = try noteFixText(a, o.id, ht.other_fix_at, ht.other_fix_place);
                if (fix == null) fix = try noteFixText(a, me.id, ht.fix_at, ht.fix_place);
            },
            .note => {
                const o = notes[ht.other];
                what = try std.fmt.allocPrint(a, "the text of note '{s}'", .{o.id});
                fix = try noteFixText(a, me.id, ht.fix_at, ht.fix_place);
                if (fix == null) fix = try noteFixText(a, o.id, null, null);
            },
            .dim, .label => {
                const o = obsts[ht.other];
                what = if (ht.kind == .dim)
                    try std.fmt.allocPrint(a, "the text '{s}' of dimension '{s}'", .{ o.text, o.id })
                else
                    try std.fmt.allocPrint(a, "label '{s}' ('{s}')", .{ o.id, o.text });
                fix = try obstacleFix(a, o, r.leaders, g.h);
                if (fix == null) fix = try noteFixText(a, me.id, ht.fix_at, ht.fix_place);
            },
        }
        const fx = fix orelse try std.fmt.allocPrint(a, "move the note with \"place\", give it another \"at\" point, or change notes_side", .{});
        env.diags.addFix(.warning, "W_LEADER_HIT", me.id, try std.fmt.allocPrint(a, "views/{s}/annotations/{s}", .{ vid, me.id }), "view {s}: the leader (or arrowhead) of note '{s}' {s} {s}; leaders must stay at least one text height ({d:.3} in paper) clear of other leaders, notes, dimension text and labels", .{ vid, me.id, rel, what, env.style.text_height_in }, try std.fmt.allocPrint(a, "{s}", .{fx}));
    }
    if (r.budget_exhausted) {
        env.diags.addFix(.warning, "W_LEADER_HIT", null, try std.fmt.allocPrint(a, "views/{s}", .{vid}), "view {s}: the layout effort limit was reached ({d} notes, {d} leader hits left): the notes were placed by the basic rule (sorted by landing height, de-crossed) and some leaders may touch each other or other annotations", .{ vid, notes.len, r.hits.len }, "fewer notes per view: split the detail into two views, shorten or merge notes, or set \"place\" on the crowded ones");
    }
    if (r.hits.len > maxn) {
        env.diags.add(.warning, "W_LEADER_HIT", null, try std.fmt.allocPrint(a, "views/{s}", .{vid}), "view {s}: {d} more leader hits not listed; fix the ones above first (dense notes: split the view, shorten notes, or set \"place\" on some)", .{ vid, r.hits.len - maxn });
    }
}

/// Note text blocks (word wrap, sizes) and the routing parameters; independent of where dimensions end up.
const NotePrep = struct {
    g: route.Geo,
    gutter: f64,
    tag_r: f64,
    lines_of: []const []const []const u8,
    rin: []route.NoteIn,
};

fn prepNotes(env: *Env, notes: []const NoteIn) Allocator.Error!NotePrep {
    const a = env.a;
    const st = env.style;
    const S = env.S;
    const h = st.text_height_in * S;
    const g = route.Geo{ .h = h, .pitch = h * st.line_spacing, .gap = st.note_gap_in * S, .shoulder = st.shoulder_in * S, .pad = 0.04 * S };
    const wrap_n: usize = st.wrapCols();
    const keynote = st.notes_mode_keynote;
    const tag_r = 0.14 * S;
    const lines_of = try a.alloc([]const []const u8, notes.len);
    const rin = try a.alloc(route.NoteIn, notes.len);
    for (notes, 0..) |n, i| {
        var w: f64 = 2 * tag_r;
        var hgt: f64 = 2 * tag_r;
        if (keynote) {
            const num = try std.fmt.allocPrint(a, "{d}", .{i + 1});
            const ls = try a.alloc([]const u8, 1);
            ls[0] = num;
            lines_of[i] = ls;
        } else {
            const lines = try wrap(a, n.text, wrap_n);
            lines_of[i] = lines;
            w = 0;
            for (lines) |l| w = @max(w, env.font.width(try asciiFold(a, l), h));
            hgt = h + (@as(f64, @floatFromInt(lines.len)) - 1.0) * g.pitch;
        }
        rin[i] = .{ .w = w, .hgt = hgt, .cands = n.cands, .movable = n.movable, .place = n.place, .column = n.column };
    }
    return .{ .g = g, .gutter = st.gutter_in * S, .tag_r = tag_r, .lines_of = lines_of, .rin = rin };
}

fn routeNotes(env: *Env, prep: NotePrep, ext: Box, obsts: []const Obstacle, soft: []const [2]V2, light: bool) Allocator.Error!route.Layout {
    const a = env.a;
    const crop = env.crop;
    const ro = try a.alloc(route.Obst, obsts.len);
    for (obsts, 0..) |o, i| ro[i] = .{ .kind = o.kind, .poly = o.poly };
    const side: route.Side = switch (env.spec.notes_side) {
        .left => .left,
        .right => .right,
        .both => .both,
    };
    return route.route(a, .{ .geo = prep.g, .crop = crop, .xl = @min(crop.x0, ext.x0), .xr = @max(crop.x1, ext.x1), .gutter = prep.gutter, .side = side, .light = light, .work = env.work }, prep.rin, ro, soft);
}

fn emitNotes(env: *Env, notes: []const NoteIn, prep: NotePrep, r: route.Layout, out: []std.ArrayList(Item)) Allocator.Error!void {
    const a = env.a;
    const st = env.style;
    const S = env.S;
    const h = st.text_height_in * S;
    const g = prep.g;
    const keynote = st.notes_mode_keynote;
    const tag_r = prep.tag_r;
    for (notes, 0..) |n, i| {
        const lines = prep.lines_of[i];
        const px = r.x[i];
        const ptop = r.top[i];
        if (keynote) {
            // hexagonal tag with the keynote number
            const cx = px + tag_r;
            const cy = ptop - tag_r;
            var hex: [6]V2 = undefined;
            for (0..6) |k| {
                const ang = std.math.pi / 6.0 + @as(f64, @floatFromInt(k)) * std.math.pi / 3.0;
                hex[k] = V2.init(cx + tag_r * @cos(ang), cy + tag_r * @sin(ang));
            }
            try out[i].append(a, try pathItem(env, .anno, n.id, &hex, true));
            try out[i].append(a, try textItem(env, .notes, .anno, n.id, lines[0], cx, cy, h, 0, .center, .middle));
        } else {
            // SPEC 20: a designer-placed note that sits left of its arrow is right-aligned to place.x + width
            const right_aligned = n.place != null and r.left[i];
            const bw = prep.rin[i].w;
            for (lines, 0..) |line, j| {
                const ty = ptop - h - @as(f64, @floatFromInt(j)) * g.pitch;
                if (right_aligned) {
                    try out[i].append(a, try textItem(env, .notes, .anno, n.id, line, px + bw, ty, h, 0, .right, .baseline));
                } else {
                    try out[i].append(a, try textItem(env, .notes, .anno, n.id, line, px, ty, h, 0, .left, .baseline));
                }
            }
        }
        const l = r.leaders[i];
        const land = r.landing[i];
        const d = land.sub(l[1]).norm();
        const alen = st.arrow_len_in * S;
        const aw = st.arrow_width_in * S;
        const base = land.sub(d.scale(alen));
        const perp = d.perp();
        try out[i].append(a, try pathItem(env, .anno, n.id, &.{ l[0], l[1], base }, false));
        const tri = try a.dupe(Pt, &.{ Pt.at(land, 0), Pt.at(base.add(perp.scale(aw)), 0), Pt.at(base.sub(perp.scale(aw)), 0) });
        const loops = try a.alloc([]const Pt, 1);
        loops[0] = tri;
        try out[i].append(a, .{ .fill = .{ .layer = layerName(env, .notes), .src = n.id, .loops = loops } });
    }
}

/// Keynote legend: numbered full texts in a block under the view (left aligned with the crop).
fn legendItems(env: *Env, notes: []const NoteIn, result: *std.ArrayList(Item)) Allocator.Error!void {
    const a = env.a;
    const st = env.style;
    const S = env.S;
    const h = st.text_height_in * S;
    const pitch = h * st.line_spacing;
    var box = itemsBox(env.font, result.items);
    box.addBox(env.crop);
    const top = env.crop.y1;
    const wrap_n: usize = cast.toIntClamped(usize, st.wrap_chars * 1.25, 5, 250);
    const x0 = box.x1 + 0.35 * S;
    var y = top;
    try result.append(a, try textItem(env, .notes, .anno, "legend", "KEYNOTES", x0, y - h, h, 0, .left, .baseline));
    y -= pitch * 1.4;
    for (notes, 0..) |n, i| {
        const lines = try wrap(a, n.text, wrap_n);
        const num = try std.fmt.allocPrint(a, "{d}", .{i + 1});
        try result.append(a, try textItem(env, .notes, .anno, "legend", num, x0, y - h, h, 0, .left, .baseline));
        for (lines, 0..) |line, j| {
            try result.append(a, try textItem(env, .notes, .anno, "legend", line, x0 + 0.35 * S, y - h - @as(f64, @floatFromInt(j)) * pitch, h, 0, .left, .baseline));
        }
        y -= pitch * @as(f64, @floatFromInt(lines.len)) + 0.25 * pitch;
    }
}

// ---- dimensions ---------------------------------------------------------------------------------------------------------

pub const DimDir = enum { h, v, aligned };

pub const dim_variants: usize = 6;

// ---- dimension conflicts (stacking) --------------------------------------------------------------------------------

// ---- citations --------------------------------------------------------------------------------------------------------------

fn noteText(env: *Env, text: []const u8, cites: []const json.Value) Allocator.Error![]const u8 {
    const a = env.a;
    var s: std.ArrayList(u8) = .empty;
    try s.appendSlice(a, text);
    for (cites) |c| {
        const code = if (c.get("code")) |x| (x.str() orelse "") else "";
        const section_s = if (c.get("section")) |x| (x.str() orelse "") else "";
        var ed: []const u8 = "";
        if (c.get("edition")) |x| if (x.num()) |n| {
            var b: [40]u8 = undefined;
            ed = try a.dupe(u8, json.fmtNumber(&b, n));
        };
        const status = if (c.get("status")) |x| (x.str() orelse "suggested") else "suggested";
        const fmt = env.style.cite_format;
        var i: usize = 0;
        while (i < fmt.len) {
            if (std.mem.startsWith(u8, fmt[i..], "{code}")) {
                try s.appendSlice(a, code);
                i += 6;
            } else if (std.mem.startsWith(u8, fmt[i..], "{section}")) {
                try s.appendSlice(a, section_s);
                i += 9;
            } else if (std.mem.startsWith(u8, fmt[i..], "{edition}")) {
                try s.appendSlice(a, ed);
                i += 9;
            } else {
                try s.append(a, fmt[i]);
                i += 1;
            }
        }
        if (!std.mem.eql(u8, status, "verified") and env.style.cite_flag_unverified) {
            try s.appendSlice(a, env.style.cite_flag);
            env.unverified = true;
        }
    }
    // SPEC 18: the style case transform covers the whole note, citation suffix included
    return upperIf(env, s.items);
}

// ---- layout repair (SPEC 20) --------------------------------------------------------------------------------------------

const max_repair_passes: usize = 8;

/// One rendering of the dimensions and labels (stacked, with the repair pushes applied) plus what the router needs.
const Render = struct {
    per: []std.ArrayList(Item),
    eff: []f64,
    obstacles: []Obstacle,
    soft: []const [2]V2,
    knock: []const [4]V2,
    ext: Box,
    meta: []Meta,
};

const DimLab = struct {
    dspecs: []const DimSpec,
    lspecs: []const LabelSpec,
    base_segs: []const [2]V2,
};

fn renderDimLabels(env: *Env, dl: DimLab, per_in: []const std.ArrayList(Item), meta_in: []const Meta, dpush: []const f64, lpush: []const V2) Allocator.Error!Render {
    const a = env.a;
    const per = try a.dupe(std.ArrayList(Item), per_in);
    const meta = try a.dupe(Meta, meta_in);
    const eff = try a.alloc(f64, dl.dspecs.len);
    const ditems = try a.alloc(std.ArrayList(Item), dl.dspecs.len);
    var label_polys: std.ArrayList([4]V2) = .empty;
    for (dl.lspecs, 0..) |l, i| {
        var its: std.ArrayList(Item) = .empty;
        if (l.base) |p| try labelItems(env, l.id, l.text, p.add(lpush[i]), &its);
        per[l.k] = its;
        meta[l.k].off = l.off.x + lpush[i].x;
        meta[l.k].off2 = l.off.y + lpush[i].y;
        for (its.items) |it| if (it == .text) try label_polys.append(a, textPoly(env.font, it.text, 0.02 * env.S));
    }
    try stackDims(env, dl.dspecs, dpush, dl.base_segs, label_polys.items, ditems, eff);
    for (dl.dspecs, 0..) |d, i| {
        per[d.k] = ditems[i];
        meta[d.k].off = eff[i];
    }
    var ext = env.crop;
    var obstacles: std.ArrayList(Obstacle) = .empty;
    var soft: std.ArrayList([2]V2) = .empty;
    var knock: std.ArrayList([4]V2) = .empty;
    for (per, 0..) |its, k| {
        ext.addBox(itemsBox(env.font, its.items));
        const m = meta[k];
        for (its.items) |it| {
            if (it == .text) {
                try knock.append(a, textPoly(env.font, it.text, 0.02 * env.S));
                try obstacles.append(a, .{
                    .poly = textPoly(env.font, it.text, 0),
                    .id = it.text.src,
                    .kind = if (m.kind == .label) .label else .dim,
                    .text = it.text.s,
                    .owner = m.owner,
                    .axis = m.axis,
                    .off = m.off,
                    .off2 = m.off2,
                });
            } else if (it == .path and m.kind == .dim and it.path.pen == .dim and it.path.pts.len == 2) {
                try soft.append(a, .{ it.path.pts[0].v(), it.path.pts[1].v() });
            }
        }
    }
    return .{ .per = per, .eff = eff, .obstacles = obstacles.items, .soft = soft.items, .knock = knock.items, .ext = ext, .meta = meta };
}

/// Push a dimension out by whole dimension spacings (0.25 paper inch) until its text clears every leader.
fn repairDim(env: *Env, o: Obstacle, leaders: []const [3]V2, h: f64, dpush: []f64, dspecs: []const DimSpec) bool {
    const step = 0.25 * env.S;
    const cap = 8.0 * step;
    if (dpush[o.owner] + step > cap + 1e-9) return false;
    const sgn = dspecs[o.owner].sgn();
    var chosen: f64 = 1;
    var k: f64 = 1;
    while (k <= 6) : (k += 1) {
        if (dpush[o.owner] + k * step > cap + 1e-9) break;
        if (clearOfLeaders(leaders, o.poly, o.axis.scale(sgn * k * step), h)) {
            chosen = k;
            break;
        }
    }
    dpush[o.owner] += chosen * step;
    return true;
}

/// Move a label (smallest move first, up/down before sideways) to where no leader comes within a text height
/// and it overprints no other label or dimension text; prefers a spot that does not sit on drawn outlines.
fn repairLabel(o: Obstacle, obsts: []const Obstacle, leaders: []const [3]V2, h: f64, lpush: []V2, base_segs: []const [2]V2) bool {
    const dirs = [8]V2{ V2.init(0, 1), V2.init(0, -1), V2.init(1, 0), V2.init(-1, 0), V2.init(1, 1), V2.init(-1, 1), V2.init(1, -1), V2.init(-1, -1) };
    const cap = 12.0 * h;
    var relax: usize = 0;
    while (relax < 2) : (relax += 1) {
        var k: f64 = 1;
        while (k * 0.5 * h <= cap) : (k += 1) {
            const delta = @ceil(k * 0.5 * h * 4.0) / 4.0;
            for (dirs) |dv| {
                const dn = dv.norm().scale(delta);
                if (@abs(lpush[o.owner].x + dn.x) > cap or @abs(lpush[o.owner].y + dn.y) > cap) continue;
                if (!clearOfLeaders(leaders, o.poly, dn, h)) continue;
                var q = o.poly;
                for (&q) |*pt| pt.* = pt.add(dn);
                var ok = true;
                for (obsts) |ob| {
                    if (ob.kind == .label and ob.owner == o.owner) continue;
                    if (polysOverlap(&q, &ob.poly)) {
                        ok = false;
                        break;
                    }
                }
                if (!ok) continue;
                if (relax == 0 and baseHits(base_segs, &q) > 0) continue;
                lpush[o.owner] = lpush[o.owner].add(dn);
                return true;
            }
        }
    }
    return false;
}

const Best = struct { r: route.Layout, rd: Render };

// ---- main entry ---------------------------------------------------------------------------------------------------------------

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
            env.diags.add(.@"error", "E_PARAM", null, try std.fmt.allocPrint(a, "views/{s}/annotations/{d}", .{ vid, k }), "annotation {d} of view {s} needs a string \"id\"", .{ k, vid });
            continue;
        };
        const apath = try std.fmt.allocPrint(a, "views/{s}/annotations/{s}", .{ vid, id });
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
                if (env.scene.point(atv, id, try std.fmt.allocPrint(a, "{s}/at", .{apath}))) |p| landing = try a.dupe(V2, &.{p});
                movable = false;
            } else if (target.len == 0) {
                env.diags.add(.@"error", "E_PARAM", id, apath, "note '{s}' needs a \"target\" (component id or comp.part) or an \"at\" point", .{id});
                continue;
            } else {
                var cid = target;
                if (std.mem.indexOfAny(u8, target, ".#")) |d| cid = target[0..d];
                if (env.scene.find(cid) == null) {
                    const ids = try env.scene.compIds(a);
                    const hint = if (model.nearest(a, cid, ids)) |n| try std.fmt.allocPrint(a, " Did you mean '{s}'?", .{n}) else try std.fmt.allocPrint(a, " Components: {s}", .{scene_mod.joinIds(a, ids)});
                    env.diags.add(.@"error", "E_REF_UNKNOWN", id, try std.fmt.allocPrint(a, "{s}/target", .{apath}), "note '{s}' target '{s}' is not a component.{s}", .{ id, target, hint });
                    continue;
                }
                landing = try targetLanding(env, target);
            }
            var place: ?V2 = null;
            if (an.get("place")) |pv| if (pv != .null) {
                place = model.offsetPairOrDiag(a, env.diags, id, try std.fmt.allocPrint(a, "{s}/place", .{apath}), "place", pv) orelse continue;
            };
            var column: ?route.Side = null;
            if (an.get("column")) |cv| if (cv != .null) {
                const cs = cv.str() orelse "";
                if (std.mem.eql(u8, cs, "left")) {
                    column = .left;
                } else if (std.mem.eql(u8, cs, "right")) {
                    column = .right;
                } else {
                    env.diags.addFix(.warning, "W_PARAM", id, try std.fmt.allocPrint(a, "{s}/column", .{apath}), "note '{s}' in view {s}: \"column\" must be \"left\" or \"right\" (got {s}); ignored", .{ id, vid, if (cv.str()) |sv| try std.fmt.allocPrint(a, "\"{s}\"", .{sv}) else "a non-string value" }, "use \"column\": \"left\" or \"right\", or omit it to let the layout choose (view notes_side)");
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
            const from = env.scene.point(fv.?, id, try std.fmt.allocPrint(a, "{s}/from", .{apath})) orelse continue;
            const to = env.scene.point(tv.?, id, try std.fmt.allocPrint(a, "{s}/to", .{apath})) orelse continue;
            const dir_s = (if (an.get("dir")) |x| x.str() else null) orelse "h";
            const dir: DimDir = if (std.mem.eql(u8, dir_s, "h")) .h else if (std.mem.eql(u8, dir_s, "v")) .v else if (std.mem.eql(u8, dir_s, "aligned")) .aligned else {
                env.diags.add(.@"error", "E_PARAM", id, try std.fmt.allocPrint(a, "{s}/dir", .{apath}), "dim dir must be \"h\", \"v\" or \"aligned\" (got \"{s}\")", .{dir_s});
                continue;
            };
            const off = model.lengthOrDiag(a, env.diags, id, try std.fmt.allocPrint(a, "{s}/offset", .{apath}), an.get("offset"), "dim offset", 0) orelse continue;
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
            var p = env.scene.point(atv, id, try std.fmt.allocPrint(a, "{s}/at", .{apath})) orelse continue;
            var loff = V2.init(0, 0);
            if (an.get("offset")) |ov| if (ov != .null) {
                loff = model.offsetPairOrDiag(a, env.diags, id, try std.fmt.allocPrint(a, "{s}/offset", .{apath}), "label offset", ov) orelse continue;
                p = p.add(loff);
            };
            cur_meta = .{ .id = id, .kind = .label, .off = loff.x, .off2 = loff.y, .owner = lspecs.items.len };
            const base: ?V2 = switch (env.landing) {
                .section => p,
                .iso => |iso| iso.project(p),
            };
            try lspecs.append(a, .{ .k = k, .id = id, .text = text, .base = base, .off = loff });
        } else {
            env.diags.add(.@"error", "E_PARAM", id, try std.fmt.allocPrint(a, "{s}/type", .{apath}), "annotation '{s}' has type \"{s}\"; use one of {s}", .{ id, ty, model.joinQuoted(a, &types) });
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

// ---- hatch knockout --------------------------------------------------------------------------------------------------------

// ---- title -------------------------------------------------------------------------------------------------------------------------
