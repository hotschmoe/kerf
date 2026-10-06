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
const drawing = @import("drawing.zig");
const units = @import("units.zig");
const cast = @import("num.zig");
const limits = @import("limits.zig");
const route = @import("route.zig");
const thinland = @import("thinland.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Pt = geom.Pt;
const Box = geom.Box;
const Item = drawing.Item;

pub const Landing = union(enum) {
    section: *section.Section,
    iso: *@import("iso.zig").Iso,
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

/// Glyph folding (SPEC 16): dashes -> '-', smart quotes -> straight, x-sign -> X, vulgar fractions ->
/// " n/d", anything else non-ASCII -> '?'. `unknown` is set when a '?' substitution happened.
pub fn asciiFoldFlag(a: Allocator, s: []const u8, unknown: ?*bool) Allocator.Error![]const u8 {
    var plain = true;
    for (s) |c| if (c >= 0x80) {
        plain = false;
        break;
    };
    if (plain) return s;
    var out: std.ArrayList(u8) = .empty;
    var it = font_mod.utf8Iter(s);
    while (it.next()) |cp| {
        switch (cp) {
            0x2010...0x2015, 0x2212 => try out.append(a, '-'),
            0xD7 => try out.append(a, 'X'),
            0x201C, 0x201D, 0x201E, 0x2033 => try out.append(a, '"'),
            0x2018, 0x2019, 0x201A, 0x2032 => try out.append(a, '\''),
            0xA0, 0x2009, 0x200A, 0x202F, 0x2002, 0x2003 => try out.append(a, ' '),
            0xBD => try out.appendSlice(a, " 1/2"),
            0xBC => try out.appendSlice(a, " 1/4"),
            0xBE => try out.appendSlice(a, " 3/4"),
            0x215B => try out.appendSlice(a, " 1/8"),
            0x215C => try out.appendSlice(a, " 3/8"),
            0x215D => try out.appendSlice(a, " 5/8"),
            0x215E => try out.appendSlice(a, " 7/8"),
            0...127 => try out.append(a, @intCast(cp)),
            else => {
                try out.append(a, '?');
                if (unknown) |u| u.* = true;
            },
        }
    }
    return out.items;
}

pub fn asciiFold(a: Allocator, s: []const u8) Allocator.Error![]const u8 {
    return asciiFoldFlag(a, s, null);
}

fn layerName(env: *const Env, key: pen_mod.LayerKey) []const u8 {
    return env.style.layerName(key);
}

fn textItem(env: *Env, layer_key: pen_mod.LayerKey, pen: Pen, src: []const u8, s: []const u8, x: f64, y: f64, h: f64, rot: f64, al: drawing.Align, va: drawing.VAlign) Allocator.Error!Item {
    return .{ .text = .{
        .layer = layerName(env, layer_key),
        .pen = pen,
        .src = src,
        .s = try asciiFoldFlag(env.a, s, &env.unknown_glyph),
        .x = x,
        .y = y,
        .h = h,
        .rot = rot,
        .align_ = al,
        .valign = va,
    } };
}

fn pathItem(env: *Env, pen: Pen, src: []const u8, pts: []const V2, closed: bool) Allocator.Error!Item {
    const p = try env.a.alloc(Pt, pts.len);
    for (pts, 0..) |q, i| p[i] = Pt.at(q, 0);
    return .{ .path = .{ .layer = env.style.layerForPen(pen), .pen = pen, .src = src, .closed = closed, .pts = p } };
}

fn upperIf(env: *const Env, s: []const u8) Allocator.Error![]const u8 {
    if (env.style.text_case_upper) return font_mod.upperAscii(env.a, s);
    return s;
}

/// Text box polygon (rotated) with padding.
pub fn textPoly(env_font: *const font_mod.Font, t: drawing.TextItem, pad: f64) [4]V2 {
    const w = env_font.width(t.s, t.h);
    const ox: f64 = switch (t.align_) {
        .center => -w * 0.5,
        .right => -w,
        .left => 0,
    };
    const oy: f64 = switch (t.valign) {
        .middle => -t.h * 0.5,
        .top => -t.h,
        .baseline => 0,
    };
    const ang = std.math.degreesToRadians(t.rot);
    const c = @cos(ang);
    const s = @sin(ang);
    const pts = [4][2]f64{ .{ ox - pad, oy - pad }, .{ ox + w + pad, oy - pad }, .{ ox + w + pad, oy + t.h + pad }, .{ ox - pad, oy + t.h + pad } };
    var out: [4]V2 = undefined;
    for (pts, 0..) |q, i| out[i] = V2.init(t.x + q[0] * c - q[1] * s, t.y + q[0] * s + q[1] * c);
    return out;
}

pub fn itemsBox(font: *const font_mod.Font, items: []const Item) Box {
    var b = Box{};
    for (items) |it| switch (it) {
        .path => |p| b.addBox(geom.pointsBox(p.pts)),
        .fill => |f| for (f.loops) |l| b.addBox(geom.pointsBox(l)),
        .hatch => |h| for (h.loops) |l| b.addBox(geom.pointsBox(l)),
        .text => |t| for (textPoly(font, t, 0)) |p| b.addPoint(p.x, p.y),
        .region => {},
    };
    return b;
}

// ---- wrapping -------------------------------------------------------------------------------------------------

fn cpCount(s: []const u8) usize {
    var n: usize = 0;
    var it = font_mod.utf8Iter(s);
    while (it.next()) |_| n += 1;
    return n;
}

/// Greedy word wrap at `chars` characters; words longer than a line are split.
pub fn wrap(a: Allocator, text: []const u8, chars_in: usize) Allocator.Error![]const []const u8 {
    const chars = @max(chars_in, 1); // 0 would never consume a word (REVIEW LAY-2)
    var lines: std.ArrayList([]const u8) = .empty;
    var cur: std.ArrayList(u8) = .empty;
    var it = std.mem.tokenizeAny(u8, text, " \t\r\n");
    while (it.next()) |word| {
        var w = word;
        while (true) {
            const wl = cpCount(w);
            const cl = cpCount(cur.items);
            if (cur.items.len == 0) {
                if (wl > chars) {
                    // split at `chars` code points
                    var n: usize = 0;
                    var idx: usize = 0;
                    var u = font_mod.utf8Iter(w);
                    while (n < chars) : (n += 1) {
                        _ = u.next() orelse break;
                        idx = u.i;
                    }
                    try lines.append(a, w[0..idx]);
                    w = w[idx..];
                    continue;
                }
                try cur.appendSlice(a, w);
                break;
            } else if (cl + 1 + wl <= chars) {
                try cur.append(a, ' ');
                try cur.appendSlice(a, w);
                break;
            } else {
                try lines.append(a, cur.items);
                cur = .empty;
                continue;
            }
        }
    }
    if (cur.items.len > 0) try lines.append(a, cur.items);
    if (lines.items.len == 0) try lines.append(a, "");
    return lines.items;
}

// ---- label points ---------------------------------------------------------------------------------------------------

pub const Shape = struct { outer: []const V2, holes: []const []const V2 };

pub fn shapesOf(a: Allocator, loops: []const []const V2) Allocator.Error![]const Shape {
    var out: std.ArrayList(Shape) = .empty;
    for (loops) |l| {
        if (l.len < 3 or geom.signedAreaV(l) <= 0) continue;
        var holes: std.ArrayList([]const V2) = .empty;
        for (loops) |h| {
            if (h.len >= 3 and geom.signedAreaV(h) < 0 and geom.pointInLoopEO(h[0], l)) try holes.append(a, h);
        }
        try out.append(a, .{ .outer = l, .holes = holes.items });
    }
    return out.items;
}

fn shapeArea(s: Shape) f64 {
    var a = geom.signedAreaV(s.outer);
    for (s.holes) |h| a += geom.signedAreaV(h);
    return a;
}

fn shapeContains(s: Shape, p: V2) bool {
    if (!geom.pointInLoopEO(p, s.outer)) return false;
    for (s.holes) |h| if (geom.pointInLoopEO(p, h)) return false;
    return true;
}

/// SPEC 6.3 step 2: label point of the largest visible polygon.
pub fn labelPoint(a: Allocator, shapes: []const Shape) Allocator.Error!?V2 {
    var best: ?Shape = null;
    var best_area: f64 = -1;
    for (shapes) |s| {
        const ar = shapeArea(s);
        if (ar > best_area) {
            best_area = ar;
            best = s;
        }
    }
    const b = best orelse return null;
    const c = geom.centroidV(b.outer);
    if (shapeContains(b, c)) return c;
    var xs: std.ArrayList(f64) = .empty;
    const sets = [_][]const V2{b.outer};
    for (sets) |cont| try crossings(a, &xs, cont, c.y);
    for (b.holes) |h| try crossings(a, &xs, h, c.y);
    std.mem.sort(f64, xs.items, {}, std.sort.asc(f64));
    var best_mid: ?struct { d: f64, m: f64 } = null;
    var i: usize = 0;
    while (i + 1 < xs.items.len) : (i += 2) {
        const x0 = xs.items[i];
        const x1 = xs.items[i + 1];
        const mid = (x0 + x1) * 0.5;
        const d: f64 = if (c.x >= x0 and c.x <= x1) 0 else @abs(c.x - mid);
        if (best_mid == null or d < best_mid.?.d) best_mid = .{ .d = d, .m = mid };
    }
    if (best_mid) |m| return V2.init(m.m, c.y);
    return c;
}

fn crossings(a: Allocator, xs: *std.ArrayList(f64), cont: []const V2, y: f64) Allocator.Error!void {
    for (cont, 0..) |p, i| {
        const q = cont[(i + 1) % cont.len];
        if ((p.y > y) != (q.y > y)) try xs.append(a, p.x + (y - p.y) / (q.y - p.y) * (q.x - p.x));
    }
}

// ---- note landing ---------------------------------------------------------------------------------------------------

fn shapeDepth(s: Shape, p: V2) f64 {
    var d = std.math.inf(f64);
    const conts = [1][]const V2{s.outer};
    for (conts) |c| for (c, 0..) |q, i| {
        d = @min(d, geom.distPointSeg(p, q, c[(i + 1) % c.len]));
    };
    for (s.holes) |c| for (c, 0..) |q, i| {
        d = @min(d, geom.distPointSeg(p, q, c[(i + 1) % c.len]));
    };
    return d;
}

fn onCropEdge(crop: Box, p: V2, q: V2) bool {
    const e = 1e-6;
    return (@abs(p.x - crop.x0) < e and @abs(q.x - crop.x0) < e) or (@abs(p.x - crop.x1) < e and @abs(q.x - crop.x1) < e) or
        (@abs(p.y - crop.y0) < e and @abs(q.y - crop.y0) < e) or (@abs(p.y - crop.y1) < e and @abs(q.y - crop.y1) < e);
}

/// Distance from p to the boundary of a shape, ignoring the edges that lie on the crop (the break lines).
fn shapeDepthNoCrop(s: Shape, p: V2, crop: Box) f64 {
    var d = std.math.inf(f64);
    const conts = [1][]const V2{s.outer};
    for (conts) |c| for (c, 0..) |q, i| {
        const r = c[(i + 1) % c.len];
        if (!onCropEdge(crop, q, r)) d = @min(d, geom.distPointSeg(p, q, r));
    };
    for (s.holes) |c| for (c, 0..) |q, i| {
        const r = c[(i + 1) % c.len];
        if (!onCropEdge(crop, q, r)) d = @min(d, geom.distPointSeg(p, q, r));
    };
    return d;
}

/// SPEC 18 auto landing: the label point stays at least 2 text heights away from the crop edges (and the
/// break lines on them). When the SPEC 6.3 label point is closer than that to a crop edge, take the point of
/// the visible region with the best clearance, where distance to a crop edge counts only up to 2 text
/// heights (a pole of inaccessibility of the region minus the crop band). Returns the point and the inset
/// box that landing candidates must stay in.
fn bandedLabelPoint(env: *Env, shapes: []const Shape) Allocator.Error!?struct { p: V2, inset: Box } {
    const crop = env.crop;
    const prim = (try labelPoint(env.a, shapes)) orelse return null;
    const h = env.style.text_height_in * env.S;
    const band = 2.0 * h;
    const inset = crop.expand(-band);
    if (inset.x1 <= inset.x0 or inset.y1 <= inset.y0) return .{ .p = prim, .inset = crop };
    if (inset.contains(prim)) return .{ .p = prim, .inset = inset };
    var bb = Box{};
    for (shapes) |x| bb.addBox(clip.loopsBox(&.{x.outer}));
    const step = gridStep(bb, h, 6.0) orelse return .{ .p = prim, .inset = inset };
    var best = prim;
    var best_score: f64 = -1;
    var best_d: f64 = std.math.inf(f64);
    var x = bb.x0 + step * 0.5;
    while (x < bb.x1) : (x += step) {
        var y = bb.y0 + step * 0.5;
        while (y < bb.y1) : (y += step) {
            const q = V2.init(x, y);
            for (shapes) |sh| if (shapeContains(sh, q)) {
                const dc = @min(@min(x - crop.x0, crop.x1 - x), @min(y - crop.y0, crop.y1 - y));
                const score = @min(shapeDepthNoCrop(sh, q, crop), @min(dc, band));
                const dp = q.dist(prim);
                if (score > best_score + 1e-9 or (score > best_score - 1e-9 and dp < best_d)) {
                    best_score = score;
                    best_d = dp;
                    best = q;
                }
                break;
            };
        }
    }
    return .{ .p = best, .inset = inset };
}

/// E_LIMIT for a note/label text over `limits.max_text_chars` (reports and returns true).
fn textTooLong(env: *Env, what: []const u8, id: []const u8, apath: []const u8, text: []const u8) Allocator.Error!bool {
    if (text.len <= limits.max_text_chars) return false;
    env.diags.addFix(.@"error", "E_LIMIT", id, apath, "{s}", .{try limits.message(env.a, try std.fmt.allocPrint(env.a, "characters in the text of {s} '{s}'", .{ what, id }), text.len, limits.max_text_chars, "A drawing note is a short phrase, not a paragraph.")}, "shorten the text to one phrase (about 130 characters or fewer) or split it into several notes");
    return true;
}

/// Sampling step for a shape's box: about a text height (or `1/div` of the thin side), coarsened until the grid has at most ~2500
/// cells. Null for a degenerate text height or box, so a zero/NaN style value cannot make the loops below endless (REVIEW LAY-2).
fn gridStep(bb: Box, h: f64, div: f64) ?f64 {
    if (!(h > 0) or !std.math.isFinite(h) or !std.math.isFinite(bb.width()) or !std.math.isFinite(bb.height())) return null;
    var step = @max(@min(h, @min(bb.width(), bb.height()) / div), h / 8.0);
    var guard: u32 = 0;
    while ((bb.width() / step + 1) * (bb.height() / step + 1) > 2500 and guard < 200) : (guard += 1) step *= 1.5;
    return step;
}

/// Landing candidates inside the visible region: the label point first, then alternatives (nearest
/// first, then the extremes in 8 directions) that keep clear of the region boundary.
fn candidatesFor(env: *Env, shapes: []const Shape, inset: Box, primary: V2) Allocator.Error![]const V2 {
    const a = env.a;
    const h = env.style.text_height_in * env.S;
    var out: std.ArrayList(V2) = .empty;
    try out.append(a, primary);
    var bb = Box{};
    for (shapes) |s| bb.addBox(clip.loopsBox(&.{s.outer}));
    if (bb.isEmpty()) return out.items;
    // thin members (straps, flashing) need a grid finer than a text height
    const step = gridStep(bb, h, 3.0) orelse return out.items;
    var pts: std.ArrayList(V2) = .empty;
    var depth: std.ArrayList(f64) = .empty;
    var maxd: f64 = 0;
    var x = bb.x0 + step * 0.5;
    while (x < bb.x1) : (x += step) {
        var y = bb.y0 + step * 0.5;
        while (y < bb.y1) : (y += step) {
            const q = V2.init(x, y);
            if (!inset.contains(q)) continue;
            for (shapes) |s| if (shapeContains(s, q)) {
                const d = shapeDepth(s, q);
                try pts.append(a, q);
                try depth.append(a, d);
                maxd = @max(maxd, d);
                break;
            };
        }
    }
    const thr = @min(0.5 * h, 0.6 * maxd);
    var keep: std.ArrayList(V2) = .empty;
    for (pts.items, depth.items) |q, d| if (d >= thr) try keep.append(a, q);
    const farFromAll = struct {
        fn ok(list: []const V2, q: V2, d: f64) bool {
            for (list) |o| if (o.dist(q) < d) return false;
            return true;
        }
    }.ok;
    var picked: usize = 0;
    while (picked < 4) : (picked += 1) {
        var best: ?V2 = null;
        var bd: f64 = std.math.inf(f64);
        for (keep.items) |q| {
            if (!farFromAll(out.items, q, h)) continue;
            const d = q.dist(primary);
            if (d < bd) {
                bd = d;
                best = q;
            }
        }
        if (best) |q| try out.append(a, q) else break;
    }
    const dirs = [8]V2{ V2.init(-1, 0), V2.init(1, 0), V2.init(0, 1), V2.init(0, -1), V2.init(-1, 1), V2.init(1, 1), V2.init(-1, -1), V2.init(1, -1) };
    for (dirs) |dv| {
        var best: ?V2 = null;
        var bp: f64 = -std.math.inf(f64);
        for (keep.items) |q| {
            const pr = q.dot(dv);
            if (pr > bp + 1e-9) {
                bp = pr;
                best = q;
            }
        }
        if (best) |q| if (farFromAll(out.items, q, 0.5 * h)) try out.append(a, q);
    }
    return out.items;
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
    if (@import("compile.zig").isOmitted(env.spec.omit, comp.id)) return null;
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
                try shapes.appendSlice(env.a, try shapesOf(env.a, reg));
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
                    try shapes.appendSlice(env.a, try shapesOf(env.a, reg));
                }
            };
        },
        .iso => |iso| {
            const p = iso.landing(comp, inst, part) orelse return null;
            return try env.a.dupe(V2, &.{p});
        },
    }
    const lp = (try bandedLabelPoint(env, shapes.items)) orelse return null;
    if (env.landing == .section) {
        if (try thinland.candidates(env.a, env.landing.section, comp, inst, part, lp.inset, env.style.text_height_in * env.S, lp.p)) |c| return c;
    }
    return try candidatesFor(env, shapes.items, lp.inset, lp.p);
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

const DimDir = enum { h, v, aligned };

/// A parsed dimension. Its drawn offset is `off0` (authored) pushed outward by `push` (layout repair) and by
/// the automatic stacking (SPEC 20).
const DimSpec = struct {
    k: usize,
    id: []const u8,
    from: V2,
    to: V2,
    dir: DimDir,
    off0: f64,
    text: ?[]const u8,

    fn sgn(self: DimSpec) f64 {
        return if (self.off0 >= 0) 1 else -1;
    }

    /// Length measured by the dimension (model units).
    fn span(self: DimSpec) f64 {
        return switch (self.dir) {
            .v => @abs(self.to.y - self.from.y),
            .aligned => self.from.dist(self.to),
            .h => @abs(self.to.x - self.from.x),
        };
    }

    /// Unit vector along which the line moves when |offset| grows (before the sign of the offset).
    fn axis(self: DimSpec) V2 {
        return switch (self.dir) {
            .v => V2.init(1, 0),
            .aligned => self.to.sub(self.from).norm().perp(),
            .h => V2.init(0, 1),
        };
    }
};

/// The drawn pieces of one dimension, for conflict tests (SPEC 20 stacking and outside text).
const DimShape = struct {
    segs: [6][2]V2 = undefined,
    n: usize = 0,
    /// The dimension line proper (between the extension lines).
    line: [2]V2 = undefined,
    /// The text box (padded).
    text: [4]V2 = undefined,
    fits: bool = true,

    fn add(self: *DimShape, a: V2, b: V2) void {
        self.segs[self.n] = .{ a, b };
        self.n += 1;
    }
};

const dim_variants: usize = 6;

/// Build one dimension. `offset` is the signed dimension-line offset (SPEC 16). When the text does not fit
/// between the extension lines it goes outside (SPEC 20), never smaller and never dropped, joined to the
/// dimension line by a short leader. `variant` picks where: 0/1 on the axis beyond the end of the line, 2-5
/// raised out of the axis (outward / inward of the object) at the far / near end.
fn dimBuild(env: *Env, d: DimSpec, offset: f64, variant: usize, out: *std.ArrayList(Item)) Allocator.Error!DimShape {
    const a = env.a;
    const st = env.style;
    const S = env.S;
    const gap = st.ext_gap_in * S;
    const over = st.ext_over_in * S;
    const tick = st.tick_len_in * S;
    const th = st.text_height_in * S;
    const tgap = st.dim_text_gap_in * S;
    const from = d.from;
    const to = d.to;
    var pa: V2 = undefined;
    var pb: V2 = undefined;
    var la: V2 = undefined; // dimension line endpoints (ordered along u)
    var lb: V2 = undefined;
    var u: V2 = undefined;
    var nrm: V2 = undefined;
    switch (d.dir) {
        .v => {
            const x = if (offset >= 0) @max(from.x, to.x) + offset else @min(from.x, to.x) + offset;
            pa = V2.init(x, from.y);
            pb = V2.init(x, to.y);
            if (from.y <= to.y) {
                la = pa;
                lb = pb;
            } else {
                la = pb;
                lb = pa;
            }
            u = V2.init(0, 1);
            nrm = V2.init(-1, 0);
        },
        .aligned => {
            const dd = to.sub(from).norm();
            const n = dd.perp();
            pa = from.add(n.scale(offset));
            pb = to.add(n.scale(offset));
            la = pa;
            lb = pb;
            u = dd;
            nrm = n;
        },
        .h => {
            const y = if (offset >= 0) @max(from.y, to.y) + offset else @min(from.y, to.y) + offset;
            pa = V2.init(from.x, y);
            pb = V2.init(to.x, y);
            if (from.x <= to.x) {
                la = pa;
                lb = pb;
            } else {
                la = pb;
                lb = pa;
            }
            u = V2.init(1, 0);
            nrm = V2.init(0, 1);
        },
    }
    var sh = DimShape{};
    const pairs = [2][2]V2{ .{ from, pa }, .{ to, pb } };
    for (pairs) |pq| {
        const dv = pq[1].sub(pq[0]);
        if (dv.len() < 1e-9) continue;
        const dn = dv.norm();
        const e0 = pq[0].add(dn.scale(gap));
        const e1 = pq[1].add(dn.scale(over));
        try out.append(a, try pathItem(env, .dim, d.id, &.{ e0, e1 }, false));
        sh.add(e0, e1);
    }
    const dist = d.span();
    const label: []const u8 = d.text orelse try units.fmtFtIn(a, dist);
    const label_f = try asciiFold(a, label);
    const tw = env.font.width(label_f, th);
    const fits = tw + 2.0 * tgap <= lb.sub(la).len() - tick;
    sh.fits = fits;
    sh.line = .{ la, lb };
    try out.append(a, try pathItem(env, .dim, d.id, &.{ la, lb }, false));
    sh.add(la, lb);
    const tdir = u.add(nrm).norm();
    for ([2]V2{ la, lb }) |p| {
        const t0 = p.sub(tdir.scale(tick * 0.5));
        const t1 = p.add(tdir.scale(tick * 0.5));
        try out.append(a, try pathItem(env, .profile, d.id, &.{ t0, t1 }, false));
        sh.add(t0, t1);
    }
    var ang = std.math.radiansToDegrees(std.math.atan2(u.y, u.x));
    if (ang > 90.0 + 1e-9 or ang <= -90.0 + 1e-9) ang += 180.0;
    if (d.dir == .v) ang = 90.0;
    const tn = V2.init(-@sin(std.math.degreesToRadians(ang)), @cos(std.math.degreesToRadians(ang)));
    var center: V2 = undefined;
    var valign: drawing.VAlign = .baseline;
    if (fits) {
        center = V2.mid(la, lb).add(tn.scale(tgap));
    } else {
        // outside: a short leader from the end of the dimension line to the text
        valign = .middle;
        const at_far = variant == 0 or variant == 2 or variant == 4;
        const e = if (at_far) lb else la;
        const s: f64 = if (at_far) 1 else -1;
        const outward = nrm.scale(d.sgn());
        const lift: f64 = switch (variant) {
            2, 3 => th + tgap,
            4, 5 => -(th + tgap),
            else => 0,
        };
        const q = e.add(u.scale(s * 2.0 * tick)).add(outward.scale(lift));
        try out.append(a, try pathItem(env, .dim, d.id, &.{ e, q }, false));
        sh.add(e, q);
        center = q.add(u.scale(s * (tgap + tw * 0.5)));
    }
    const ti = try textItem(env, .dims, .dim, d.id, label, center.x, center.y, th, ang, .center, valign);
    try out.append(a, ti);
    sh.text = textPoly(env.font, ti.text, 0.015 * S);
    return sh;
}

fn labelItems(env: *Env, id: []const u8, text: []const u8, at: V2, out: *std.ArrayList(Item)) Allocator.Error!void {
    const t = try upperIf(env, text);
    try out.append(env.a, try textItem(env, .notes, .anno, id, t, at.x, at.y, env.style.label_height_in * env.S, 0, .center, .middle));
}

// ---- dimension conflicts (stacking) --------------------------------------------------------------------------------

fn polyHitsSeg(poly: *const [4]V2, a: V2, b: V2) bool {
    if (geom.pointInLoopEO(a, poly) or geom.pointInLoopEO(b, poly)) return true;
    for (0..4) |i| if (route.segSegDist(a, b, poly[i], poly[(i + 1) % 4]) <= 1e-9) return true;
    return false;
}

pub fn polysOverlap(p: *const [4]V2, q: *const [4]V2) bool {
    for (0..4) |i| if (polyHitsSeg(q, p[i], p[(i + 1) % 4])) return true;
    for (0..4) |i| if (geom.pointInLoopEO(q[i], p)) return true;
    return false;
}

/// Parallel dimension lines that run on top of each other (within `tol`, with a shared stretch).
fn linesCollide(a: [2]V2, b: [2]V2, tol: f64) bool {
    const da = a[1].sub(a[0]);
    const db = b[1].sub(b[0]);
    const la = da.len();
    const lb = db.len();
    if (la < 1e-9 or lb < 1e-9) return false;
    const ua = da.scale(1.0 / la);
    if (@abs(ua.cross(db.scale(1.0 / lb))) > 0.02) return false;
    const t0 = b[0].sub(a[0]).dot(ua);
    const t1 = b[1].sub(a[0]).dot(ua);
    const ov = @min(la, @max(t0, t1)) - @max(0.0, @min(t0, t1));
    if (ov <= 1e-6) return false;
    const dperp = @abs(ua.cross(b[0].sub(a[0])));
    return dperp < tol;
}

fn dimConflicts(x: *const DimShape, y: *const DimShape, tol: f64) usize {
    var c: usize = 0;
    if (linesCollide(x.line, y.line, tol)) c += 1;
    if (polysOverlap(&x.text, &y.text)) c += 1;
    for (y.segs[0..y.n]) |sg| if (polyHitsSeg(&x.text, sg[0], sg[1])) {
        c += 1;
        break;
    };
    for (x.segs[0..x.n]) |sg| if (polyHitsSeg(&y.text, sg[0], sg[1])) {
        c += 1;
        break;
    };
    return c;
}

fn baseHits(segs: []const [2]V2, poly: *const [4]V2) usize {
    var bb = Box{};
    for (poly) |p| bb.addPoint(p.x, p.y);
    var n: usize = 0;
    for (segs) |sg| {
        if (@max(sg[0].x, sg[1].x) < bb.x0 or @min(sg[0].x, sg[1].x) > bb.x1 or @max(sg[0].y, sg[1].y) < bb.y0 or @min(sg[0].y, sg[1].y) > bb.y1) continue;
        if (polyHitsSeg(poly, sg[0], sg[1])) n += 1;
    }
    return n;
}

const DimPlaced = struct { offset: f64, shape: DimShape, items: std.ArrayList(Item) };

/// SPEC 20 dimension stacking: dims are placed shortest first; a dimension whose line would overlap another
/// one, or whose text would overprint another dimension's text or lines, moves out in steps of 0.25 paper
/// inch (outside text first tries the other places before the dimension line moves). `pushes` is the extra
/// outward distance chosen by the layout repair. Returns the effective offsets and fills `items`.
fn stackDims(env: *Env, specs: []const DimSpec, pushes: []const f64, base_segs: []const [2]V2, label_polys: []const [4]V2, items: []std.ArrayList(Item), eff: []f64) Allocator.Error!void {
    const a = env.a;
    const S = env.S;
    const step = 0.25 * S;
    const order = try a.alloc(usize, specs.len);
    for (order, 0..) |*o, i| o.* = i;
    std.mem.sort(usize, order, specs, struct {
        fn lt(sp: []const DimSpec, x: usize, y: usize) bool {
            const sx = sp[x].span();
            const sy = sp[y].span();
            if (@abs(sx - sy) > 1e-9) return sx < sy;
            return x < y;
        }
    }.lt);
    var placed: std.ArrayList(DimShape) = .empty;
    for (order) |i| {
        const d = specs[i];
        const start = d.off0 + d.sgn() * pushes[i];
        var best: ?DimPlaced = null;
        var best_c: usize = std.math.maxInt(usize);
        var k: usize = 0;
        search: while (k <= 12) : (k += 1) {
            const off = start + d.sgn() * @as(f64, @floatFromInt(k)) * step;
            var kbest: ?DimPlaced = null;
            var kbest_hits: usize = std.math.maxInt(usize);
            var kbest_c: usize = std.math.maxInt(usize);
            var v: usize = 0;
            while (v < dim_variants) : (v += 1) {
                var its: std.ArrayList(Item) = .empty;
                const sh = try dimBuild(env, d, off, v, &its);
                var c: usize = 0;
                for (placed.items) |*o| c += dimConflicts(&sh, o, 0.5 * step);
                for (label_polys) |*lp| {
                    if (polysOverlap(&sh.text, lp)) c += 1;
                    for (sh.segs[0..sh.n]) |sg| if (polyHitsSeg(lp, sg[0], sg[1])) {
                        c += 1;
                        break;
                    };
                }
                const bh: usize = if (sh.fits) 0 else baseHits(base_segs, &sh.text);
                if (c < kbest_c or (c == kbest_c and bh < kbest_hits)) {
                    kbest_c = c;
                    kbest_hits = bh;
                    kbest = .{ .offset = off, .shape = sh, .items = its };
                }
                if (sh.fits) break;
            }
            if (kbest_c < best_c) {
                best_c = kbest_c;
                best = kbest;
            }
            if (kbest_c == 0) break :search;
        }
        const pick = best.?;
        eff[i] = pick.offset;
        items[i] = pick.items;
        try placed.append(a, pick.shape);
    }
}

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

fn convexInterval(a: V2, b: V2, poly: *const [4]V2) ?[2]f64 {
    const area = geom.signedAreaV(poly);
    const sgn: f64 = if (area >= 0) 1 else -1;
    var t0: f64 = 0;
    var t1: f64 = 1;
    const d = b.sub(a);
    for (poly, 0..) |p, i| {
        const q = poly[(i + 1) % 4];
        const e = q.sub(p);
        const num = e.cross(a.sub(p)) * sgn;
        const den = e.cross(d) * sgn;
        if (@abs(den) < 1e-15) {
            if (num < 0) return null;
        } else {
            const t = -num / den;
            if (den > 0) t0 = @max(t0, t) else t1 = @min(t1, t);
        }
        if (t0 > t1) return null;
    }
    return .{ t0, t1 };
}

fn knockHatch(a: Allocator, items: []Item, boxes: []const [4]V2) Allocator.Error!void {
    if (boxes.len == 0) return;
    for (items) |*it| {
        if (it.* != .hatch) continue;
        var out: std.ArrayList([4]f64) = .empty;
        for (it.hatch.lines) |l| {
            const p = V2.init(l[0], l[1]);
            const q = V2.init(l[2], l[3]);
            var cuts: std.ArrayList([2]f64) = .empty;
            for (boxes) |*bx| if (convexInterval(p, q, bx)) |iv| try cuts.append(a, iv);
            if (cuts.items.len == 0) {
                try out.append(a, l);
                continue;
            }
            const degenerate = V2.eql(p, q, 1e-12);
            if (degenerate) continue;
            std.mem.sort([2]f64, cuts.items, {}, struct {
                fn lt(_: void, x: [2]f64, y: [2]f64) bool {
                    return x[0] < y[0];
                }
            }.lt);
            var t: f64 = 0;
            for (cuts.items) |c| {
                if (c[0] > t + 1e-9) {
                    const s0 = V2.lerp(p, q, t);
                    const s1 = V2.lerp(p, q, c[0]);
                    try out.append(a, .{ s0.x, s0.y, s1.x, s1.y });
                }
                t = @max(t, c[1]);
            }
            if (t < 1.0 - 1e-9) {
                const s0 = V2.lerp(p, q, t);
                try out.append(a, .{ s0.x, s0.y, q.x, q.y });
            }
        }
        it.hatch.lines = out.items;
    }
}

// ---- title -------------------------------------------------------------------------------------------------------------------------

pub const SheetInfo = struct {
    number: []const u8,
    title: []const u8,
    scale_text: []const u8,
    sheet: []const u8,
    unverified: bool,
};

/// Title under the view: bubble, title text with heavy underline, scale (SPEC 6.5). `low` receives the lowest y used.
pub fn titleItems(env: *Env, info: SheetInfo, tcrop: Box, out: *std.ArrayList(Item)) Allocator.Error!f64 {
    const a = env.a;
    const st = env.style;
    const S = env.S;
    const r = 0.3125 * S;
    const top_gap = 0.4 * S;
    const cx = tcrop.x0 + r;
    const cy = tcrop.y0 - top_gap - r;
    const src = try std.fmt.allocPrint(a, "title:{s}", .{env.spec.id});
    const bubble = try a.dupe(Pt, &.{ .{ .x = cx - r, .y = cy, .b = 1 }, .{ .x = cx + r, .y = cy, .b = 1 } });
    try out.append(a, .{ .path = .{ .layer = layerName(env, .title), .pen = .title, .src = src, .closed = true, .pts = bubble } });
    const th = st.title_height_in * S;
    const nh = st.text_height_in * S;
    if (info.sheet.len == 0) {
        try out.append(a, try textItem(env, .title, .title, src, info.number, cx, cy, th, 0, .center, .middle));
    } else {
        try out.append(a, try pathItem(env, .anno, src, &.{ V2.init(cx - r, cy), V2.init(cx + r, cy) }, false));
        try out.append(a, try textItem(env, .title, .title, src, info.number, cx, cy + r * 0.5, th, 0, .center, .middle));
        try out.append(a, try textItem(env, .title, .anno, src, info.sheet, cx, cy - r * 0.5, st.label_height_in * 0.9 * S, 0, .center, .middle));
    }
    const tx = cx + r + 0.15 * S;
    const title = try upperIf(env, info.title);
    const ty = cy + 0.02 * S;
    try out.append(a, try textItem(env, .title, .title, src, title, tx, ty, th, 0, .left, .baseline));
    const tw = env.font.width(try asciiFold(a, title), th);
    const uy = ty - 0.07 * S;
    try out.append(a, try pathItem(env, .title, src, &.{ V2.init(tx, uy), V2.init(tx + tw, uy) }, false));
    const scale_line = try std.fmt.allocPrint(a, "SCALE: {s}", .{info.scale_text});
    const sy = uy - 0.06 * S - nh;
    try out.append(a, try textItem(env, .title, .anno, src, scale_line, tx, sy, nh, 0, .left, .baseline));
    var low = @min(cy - r, sy - 0.02 * S);
    if (info.unverified) {
        const fy = low - 0.1 * S - nh;
        try out.append(a, try textItem(env, .title, .anno, "footnote", st.cite_footnote, tcrop.x0, fy, nh * 0.85, 0, .left, .baseline));
        low = fy - 0.02 * S;
    }
    return low;
}

test "wrap by characters" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const l = try wrap(a, "2X8 PT SILL PLATE W/ 5/8\" DIA. ANCHOR BOLTS @ 48\" O.C.", 28);
    for (l) |s| try std.testing.expect(s.len <= 28);
    const w = try wrap(a, "ABCDEFGHIJ", 4);
    try std.testing.expectEqual(@as(usize, 3), w.len);
    try std.testing.expectEqualStrings("EFGH", w[1]);
}
