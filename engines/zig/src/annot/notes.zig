//! Leader notes (SPEC 6.3): landing points, obstacles, routing of the note column and its legend, hit reporting, and note text with citations.

const std = @import("std");
const geom = @import("../geom.zig");
const json = @import("../json.zig");
const clip = @import("../clip.zig");
const scene_mod = @import("../scene.zig");
const section = @import("../section.zig");
const shape_mod = @import("../shape.zig");
const drawing = @import("../drawing.zig");
const cast = @import("../num.zig");
const limits = @import("../limits.zig");
const route = @import("../route.zig");
const thinland = @import("../thinland.zig");
const compile_mod = @import("../compile.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Pt = geom.Pt;
const Box = geom.Box;
const Shape = shape_mod.Shape;
const Item = drawing.Item;
const layerName = @import("text.zig").layerName;
const textItem = @import("text.zig").textItem;
const pathItem = @import("text.zig").pathItem;
const upperIf = @import("text.zig").upperIf;
const wrap = @import("text.zig").wrap;
const annot = @import("../annot.zig");
const Env = annot.Env;
const asciiFold = annot.asciiFold;
const itemsBox = annot.itemsBox;

/// E_LIMIT for a note/label text over `limits.max_text_chars` (reports and returns true).
pub fn textTooLong(env: *Env, what: []const u8, id: []const u8, apath: []const u8, text: []const u8) Allocator.Error!bool {
    if (text.len <= limits.max_text_chars) return false;
    env.diags.addFix(.@"error", "E_LIMIT", id, apath, "{s}", .{try limits.message(env.a, try env.a.print("characters in the text of {s} '{s}'", .{ what, id }), text.len, limits.max_text_chars, "A drawing note is a short phrase, not a paragraph.")}, "shorten the text to one phrase (about 130 characters or fewer) or split it into several notes");
    return true;
}

pub fn targetLanding(env: *Env, target: []const u8) Allocator.Error!?[]const V2 {
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
            const p = (try iso.landing(comp, inst, part)) orelse return null;
            return try env.a.dupe(V2, &.{p});
        },
    }
    const lp = (try shape_mod.bandedLabelPoint(env.a, shapes.items, env.crop, env.style.text_height_in * env.S)) orelse return null;
    if (env.landing == .section) {
        if (try thinland.candidates(env.a, env.landing.section, comp, inst, part, lp.inset, env.style.text_height_in * env.S, lp.p)) |c| return c;
    }
    return try shape_mod.candidatesFor(env.a, shapes.items, lp.inset, lp.p, env.style.text_height_in * env.S);
}

pub const NoteIn = struct {
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
pub const Obstacle = struct {
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

pub const Meta = struct {
    id: []const u8 = "",
    kind: enum { none, dim, label } = .none,
    axis: V2 = .{ .x = 0, .y = 0 },
    off: f64 = 0,
    off2: f64 = 0,
    /// Index into the dim / label specs.
    owner: usize = 0,
};

pub const LabelSpec = struct {
    k: usize,
    id: []const u8,
    text: []const u8,
    /// Drawing-space position (author offset applied); null when an iso label does not project.
    base: ?V2,
    off: V2,
};

pub fn fmtPt(a: Allocator, p: V2) Allocator.Error![]const u8 {
    return a.print("[{s}, {s}]", .{ try json.fmtNumberAlloc(a, @round(p.x * 100) / 100), try json.fmtNumberAlloc(a, @round(p.y * 100) / 100) });
}

pub fn clearOfLeaders(leaders: []const [3]V2, poly: [4]V2, shift: V2, h: f64) bool {
    var q = poly;
    for (&q) |*pt| pt.* = pt.add(shift);
    for (leaders) |l| if (route.polyBoxDist(l, &q) < h) return false;
    return true;
}

/// Offset proposals for a dimension or label that a leader runs into (what the repair loop could not fix).
pub fn obstacleFix(a: Allocator, o: Obstacle, leaders: []const [3]V2, h: f64) Allocator.Error!?[]const u8 {
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
                        return try a.print("set dim '{s}' \"offset\": {s} (now {s})", .{ o.id, try json.fmtNumberAlloc(a, no), try json.fmtNumberAlloc(a, o.off) });
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
                        return try a.print("set label '{s}' \"offset\": [{s}, {s}] (now [{s}, {s}])", .{ o.id, try json.fmtNumberAlloc(a, @round((o.off + dn.x) * 100) / 100), try json.fmtNumberAlloc(a, @round((o.off2 + dn.y) * 100) / 100), try json.fmtNumberAlloc(a, o.off), try json.fmtNumberAlloc(a, o.off2) });
                    }
                }
            }
        },
    }
    return null;
}

pub fn noteFixText(a: Allocator, id: []const u8, fa: ?V2, fp: ?V2) Allocator.Error!?[]const u8 {
    if (fa) |p| return try a.print("set note '{s}' \"at\": {s} (another point inside its target)", .{ id, try fmtPt(a, p) });
    if (fp) |p| return try a.print("set note '{s}' \"place\": {s}", .{ id, try fmtPt(a, p) });
    return null;
}

pub fn reportHits(env: *Env, notes: []const NoteIn, obsts: []const Obstacle, r: route.Layout, g: route.Geo) Allocator.Error!void {
    const a = env.a;
    const vid = env.spec.id;
    const maxn: usize = 8;
    var shown: usize = 0;
    for (r.hits) |ht| {
        if (shown >= maxn) break;
        shown += 1;
        const me = notes[ht.note];
        const rel = if (ht.dist <= 1e-9) try a.dupe(u8, "crosses") else try a.print("comes within {d:.2} in (paper) of", .{ht.dist / env.S});
        var what: []const u8 = undefined;
        var fix: ?[]const u8 = null;
        switch (ht.kind) {
            .leader => {
                const o = notes[ht.other];
                what = try a.print("the leader of note '{s}'", .{o.id});
                fix = try noteFixText(a, o.id, ht.other_fix_at, ht.other_fix_place);
                if (fix == null) fix = try noteFixText(a, me.id, ht.fix_at, ht.fix_place);
            },
            .note => {
                const o = notes[ht.other];
                what = try a.print("the text of note '{s}'", .{o.id});
                fix = try noteFixText(a, me.id, ht.fix_at, ht.fix_place);
                if (fix == null) fix = try noteFixText(a, o.id, null, null);
            },
            .dim, .label => {
                const o = obsts[ht.other];
                what = if (ht.kind == .dim)
                    try a.print("the text '{s}' of dimension '{s}'", .{ o.text, o.id })
                else
                    try a.print("label '{s}' ('{s}')", .{ o.id, o.text });
                fix = try obstacleFix(a, o, r.leaders, g.h);
                if (fix == null) fix = try noteFixText(a, me.id, ht.fix_at, ht.fix_place);
            },
        }
        const fx = fix orelse try a.print("move the note with \"place\", give it another \"at\" point, or change notes_side", .{});
        env.diags.addFix(.warning, "W_LEADER_HIT", me.id, try a.print("views/{s}/annotations/{s}", .{ vid, me.id }), "view {s}: the leader (or arrowhead) of note '{s}' {s} {s}; leaders must stay at least one text height ({d:.3} in paper) clear of other leaders, notes, dimension text and labels", .{ vid, me.id, rel, what, env.style.text_height_in }, try a.print("{s}", .{fx}));
    }
    if (r.budget_exhausted) {
        env.diags.addFix(.warning, "W_LEADER_HIT", null, try a.print("views/{s}", .{vid}), "view {s}: the layout effort limit was reached ({d} notes, {d} leader hits left): the notes were placed by the basic rule (sorted by landing height, de-crossed) and some leaders may touch each other or other annotations", .{ vid, notes.len, r.hits.len }, "fewer notes per view: split the detail into two views, shorten or merge notes, or set \"place\" on the crowded ones");
    }
    if (r.hits.len > maxn) {
        env.diags.add(.warning, "W_LEADER_HIT", null, try a.print("views/{s}", .{vid}), "view {s}: {d} more leader hits not listed; fix the ones above first (dense notes: split the view, shorten notes, or set \"place\" on some)", .{ vid, r.hits.len - maxn });
    }
}

/// Note text blocks (word wrap, sizes) and the routing parameters; independent of where dimensions end up.
pub const NotePrep = struct {
    g: route.Geo,
    gutter: f64,
    tag_r: f64,
    lines_of: []const []const []const u8,
    rin: []route.NoteIn,
};

pub fn prepNotes(env: *Env, notes: []const NoteIn) Allocator.Error!NotePrep {
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
            const num = try a.print("{d}", .{i + 1});
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

pub fn routeNotes(env: *Env, prep: NotePrep, ext: Box, obsts: []const Obstacle, soft: []const [2]V2, light: bool) Allocator.Error!route.Layout {
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

pub fn emitNotes(env: *Env, notes: []const NoteIn, prep: NotePrep, r: route.Layout, out: []std.ArrayList(Item)) Allocator.Error!void {
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
pub fn legendItems(env: *Env, notes: []const NoteIn, result: *std.ArrayList(Item)) Allocator.Error!void {
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
        const num = try a.print("{d}", .{i + 1});
        try result.append(a, try textItem(env, .notes, .anno, "legend", num, x0, y - h, h, 0, .left, .baseline));
        for (lines, 0..) |line, j| {
            try result.append(a, try textItem(env, .notes, .anno, "legend", line, x0 + 0.35 * S, y - h - @as(f64, @floatFromInt(j)) * pitch, h, 0, .left, .baseline));
        }
        y -= pitch * @as(f64, @floatFromInt(lines.len)) + 0.25 * pitch;
    }
}

pub fn noteText(env: *Env, text: []const u8, cites: []const json.Value) Allocator.Error![]const u8 {
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
