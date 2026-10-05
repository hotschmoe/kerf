//! Annotations (SPEC 6, 16): leader notes with deterministic column layout, dimensions, labels,
//! and the title block under a view.

const std = @import("std");
const json = @import("json.zig");
const geom = @import("geom.zig");
const clip = @import("clip.zig");
const model = @import("model.zig");
const scene_mod = @import("scene.zig");
const style_mod = @import("style.zig");
const font_mod = @import("font.zig");
const view_mod = @import("view.zig");
const section = @import("section.zig");
const drawing = @import("drawing.zig");
const units = @import("units.zig");
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
};

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

fn layerName(env: *const Env, key: []const u8) []const u8 {
    return if (env.style.layerByKey(key)) |l| l.name else "0";
}

fn textItem(env: *Env, layer_key: []const u8, pen: []const u8, src: []const u8, s: []const u8, x: f64, y: f64, h: f64, rot: f64, al: drawing.Align, va: drawing.VAlign) Allocator.Error!Item {
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

fn pathItem(env: *Env, pen: []const u8, src: []const u8, pts: []const V2, closed: bool) Allocator.Error!Item {
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
pub fn wrap(a: Allocator, text: []const u8, chars: usize) Allocator.Error![]const []const u8 {
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

fn targetLanding(env: *Env, target: []const u8) Allocator.Error!?V2 {
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
    switch (env.landing) {
        .section => |sec| {
            for (sec.prisms, 0..) |p, i| {
                if (p.comp != comp.index) continue;
                if (inst) |k| if (p.instance != k) continue;
                if (part) |pn| if (!std.mem.eql(u8, p.part, pn)) continue;
                const reg = try sec.visibleRegion(i);
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
                    try shapes.appendSlice(env.a, try shapesOf(env.a, reg));
                }
            };
        },
        .iso => |iso| {
            return iso.landing(comp, inst, part);
        },
    }
    return labelPoint(env.a, shapes.items);
}

// ---- notes ------------------------------------------------------------------------------------------------------------

const NoteIn = struct {
    id: []const u8,
    text: []const u8,
    landing: V2,
    place: ?V2,
};

const Placed = struct {
    lines: []const []const u8,
    width: f64,
    height: f64,
    landing: V2,
    top: f64,
    x: f64,
    left_side: bool,
    fixed: bool,
};

const Geo = struct { h: f64, pitch: f64, gap: f64, shoulder: f64, pad: f64 };

fn leaderOf(p: Placed, g: Geo) [3]V2 {
    const ymid = p.top - p.height * 0.5;
    const edge_x: f64 = if (p.left_side) p.x + p.width + g.pad else p.x - g.pad;
    const dir: f64 = if (p.left_side) 1 else -1;
    return .{ V2.init(edge_x, ymid), V2.init(edge_x + dir * g.shoulder, ymid), p.landing };
}

fn stack(order: []const usize, placed: []Placed, crop: Box, g: Geo) void {
    var prev_bottom: f64 = std.math.inf(f64);
    for (order) |i| {
        const p = &placed[i];
        p.top = p.landing.y + p.height * 0.5;
        if (p.top > prev_bottom - g.gap) p.top = prev_bottom - g.gap;
        prev_bottom = p.top - p.height;
    }
    if (order.len > 0) {
        const last = order[order.len - 1];
        const bottom = placed[last].top - placed[last].height;
        if (bottom < crop.y0) {
            const d = crop.y0 - bottom;
            for (order) |i| placed[i].top += d;
        }
        const top = placed[order[0]].top;
        if (top > crop.y1) {
            const d = top - crop.y1;
            for (order) |i| placed[i].top -= d;
        }
    }
}

fn segsCross(a0: V2, a1: V2, b0: V2, b1: V2) bool {
    var ta: [2]f64 = undefined;
    var tb: [2]f64 = undefined;
    return geom.segSeg(a0, a1, b0, b1, &ta, &tb) > 0;
}

fn polylinesCross(a: []const V2, b: []const V2) bool {
    for (0..a.len - 1) |i| for (0..b.len - 1) |j| {
        if (segsCross(a[i], a[i + 1], b[j], b[j + 1])) return true;
    };
    return false;
}

fn segHitsPoly(a: V2, b: V2, poly: []const V2) bool {
    for (poly, 0..) |p, i| {
        if (segsCross(a, b, p, poly[(i + 1) % poly.len])) return true;
    }
    return geom.pointInLoopEO(a, poly) or geom.pointInLoopEO(b, poly);
}

fn hitsObstacle(p: Placed, g: Geo, obstacles: []const [4]V2) bool {
    const l = leaderOf(p, g);
    for (obstacles) |o| {
        if (segHitsPoly(l[0], l[1], &o) or segHitsPoly(l[1], l[2], &o)) return true;
    }
    return false;
}

fn layoutNotes(env: *Env, notes: []const NoteIn, ext: Box, obstacles: []const [4]V2, out: []std.ArrayList(Item)) Allocator.Error!void {
    const a = env.a;
    const st = env.style;
    const S = env.S;
    const crop = env.crop;
    const h = st.text_height_in * S;
    const g = Geo{ .h = h, .pitch = h * st.line_spacing, .gap = st.note_gap_in * S, .shoulder = st.shoulder_in * S, .pad = 0.04 * S };
    const gutter = st.gutter_in * S;
    const xr = @max(crop.x1, ext.x1);
    const xl = @min(crop.x0, ext.x0);
    const wrap_n: usize = @intFromFloat(st.wrap_chars);
    var placed = try a.alloc(Placed, notes.len);
    const keynote = st.notes_mode_keynote;
    const tag_r = 0.14 * S;
    for (notes, 0..) |n, i| {
        if (keynote) {
            const num = try std.fmt.allocPrint(a, "{d}", .{i + 1});
            const left_side_k = switch (env.spec.notes_side) {
                .left => true,
                .both => @abs(n.landing.x - crop.x0) < @abs(crop.x1 - n.landing.x),
                .right => false,
            };
            const ls = try a.alloc([]const u8, 1);
            ls[0] = num;
            placed[i] = .{ .lines = ls, .width = 2 * tag_r, .height = 2 * tag_r, .landing = n.landing, .top = n.landing.y + tag_r, .x = 0, .left_side = left_side_k, .fixed = n.place != null };
            continue;
        }
        const lines = try wrap(a, n.text, wrap_n);
        var width: f64 = 0;
        for (lines) |l| width = @max(width, env.font.width(try asciiFold(a, l), h));
        const height = h + (@as(f64, @floatFromInt(lines.len)) - 1.0) * g.pitch;
        const left_side = switch (env.spec.notes_side) {
            .left => true,
            .both => @abs(n.landing.x - crop.x0) < @abs(crop.x1 - n.landing.x),
            .right => false,
        };
        placed[i] = .{ .lines = lines, .width = width, .height = height, .landing = n.landing, .top = n.landing.y + height * 0.5, .x = 0, .left_side = left_side, .fixed = n.place != null };
    }
    for ([2]bool{ false, true }) |want_left| {
        var order: std.ArrayList(usize) = .empty;
        for (placed, 0..) |p, i| if (p.left_side == want_left and !p.fixed) try order.append(a, i);
        std.mem.sort(usize, order.items, placed, struct {
            fn lt(pl: []Placed, x: usize, y: usize) bool {
                if (pl[x].landing.y != pl[y].landing.y) return pl[x].landing.y > pl[y].landing.y;
                return x < y;
            }
        }.lt);
        for (order.items) |i| {
            placed[i].x = if (want_left) xl - gutter - placed[i].width else xr + gutter;
        }
        if (order.items.len == 0) continue;
        stack(order.items, placed, crop, g);
        // swap adjacent notes whose leaders cross (bounded, deterministic)
        var iter: usize = 0;
        while (iter < order.items.len * 4 + 8) : (iter += 1) {
            var swapped = false;
            var k: usize = 0;
            while (k + 1 < order.items.len) : (k += 1) {
                const ia = order.items[k];
                const ib = order.items[k + 1];
                const la = leaderOf(placed[ia], g);
                const lb = leaderOf(placed[ib], g);
                if (polylinesCross(&la, &lb)) {
                    std.mem.swap(usize, &order.items[k], &order.items[k + 1]);
                    stack(order.items, placed, crop, g);
                    swapped = true;
                    break;
                }
            }
            if (!swapped) break;
        }
        // nudge notes whose leaders run through dimension/label text
        for (order.items, 0..) |i, k| {
            if (!hitsObstacle(placed[i], g, obstacles)) continue;
            const base = placed[i].top;
            var step: usize = 1;
            search: while (step <= 16) : (step += 1) {
                for ([2]f64{ 1, -1 }) |sign| {
                    const t = base + sign * @as(f64, @floatFromInt(step)) * g.pitch * 0.5;
                    const hgt = placed[i].height;
                    if (k > 0 and t > placed[order.items[k - 1]].top - placed[order.items[k - 1]].height - g.gap) continue;
                    if (k + 1 < order.items.len and placed[order.items[k + 1]].top > t - hgt - g.gap) continue;
                    const saved = placed[i].top;
                    placed[i].top = t;
                    var ok = !hitsObstacle(placed[i], g, obstacles);
                    if (ok) for (order.items) |o| {
                        if (o == i) continue;
                        const l1 = leaderOf(placed[i], g);
                        const l2 = leaderOf(placed[o], g);
                        if (polylinesCross(&l1, &l2)) {
                            ok = false;
                            break;
                        }
                    };
                    if (ok) break :search;
                    placed[i].top = saved;
                }
            }
        }
    }
    for (placed, 0..) |*p, i| {
        if (notes[i].place) |pl| {
            p.x = pl.x;
            p.top = pl.y;
            p.left_side = pl.x + p.width * 0.5 < p.landing.x;
        }
    }
    for (placed, 0..) |p, i| {
        const n = notes[i];
        if (keynote) {
            // hexagonal tag with the keynote number
            const cx = p.x + tag_r;
            const cy = p.top - tag_r;
            var hex: [6]V2 = undefined;
            for (0..6) |k| {
                const ang = std.math.pi / 6.0 + @as(f64, @floatFromInt(k)) * std.math.pi / 3.0;
                hex[k] = V2.init(cx + tag_r * @cos(ang), cy + tag_r * @sin(ang));
            }
            try out[i].append(a, try pathItem(env, "anno", n.id, &hex, true));
            try out[i].append(a, try textItem(env, "notes", "anno", n.id, p.lines[0], cx, cy, h, 0, .center, .middle));
        } else for (p.lines, 0..) |line, j| {
            try out[i].append(a, try textItem(env, "notes", "anno", n.id, line, p.x, p.top - h - @as(f64, @floatFromInt(j)) * g.pitch, h, 0, .left, .baseline));
        }
        const l = leaderOf(p, g);
        const land = p.landing;
        const d = land.sub(l[1]).norm();
        const alen = st.arrow_len_in * S;
        const aw = st.arrow_width_in * S;
        const base = land.sub(d.scale(alen));
        const perp = d.perp();
        try out[i].append(a, try pathItem(env, "anno", n.id, &.{ l[0], l[1], base }, false));
        const tri = try a.dupe(Pt, &.{ Pt.at(land, 0), Pt.at(base.add(perp.scale(aw)), 0), Pt.at(base.sub(perp.scale(aw)), 0) });
        const loops = try a.alloc([]const Pt, 1);
        loops[0] = tri;
        try out[i].append(a, .{ .fill = .{ .layer = layerName(env, "notes"), .src = n.id, .loops = loops } });
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
    const wrap_n: usize = @intFromFloat(st.wrap_chars * 1.25);
    const x0 = box.x1 + 0.35 * S;
    var y = top;
    try result.append(a, try textItem(env, "notes", "anno", "legend", "KEYNOTES", x0, y - h, h, 0, .left, .baseline));
    y -= pitch * 1.4;
    for (notes, 0..) |n, i| {
        const lines = try wrap(a, n.text, wrap_n);
        const num = try std.fmt.allocPrint(a, "{d}", .{i + 1});
        try result.append(a, try textItem(env, "notes", "anno", "legend", num, x0, y - h, h, 0, .left, .baseline));
        for (lines, 0..) |line, j| {
            try result.append(a, try textItem(env, "notes", "anno", "legend", line, x0 + 0.35 * S, y - h - @as(f64, @floatFromInt(j)) * pitch, h, 0, .left, .baseline));
        }
        y -= pitch * @as(f64, @floatFromInt(lines.len)) + 0.25 * pitch;
    }
}

// ---- dimensions ---------------------------------------------------------------------------------------------------------

fn dimItems(env: *Env, id: []const u8, from: V2, to: V2, dir: []const u8, offset: f64, text: ?[]const u8, out: *std.ArrayList(Item)) Allocator.Error!void {
    const a = env.a;
    const st = env.style;
    const S = env.S;
    const gap = st.ext_gap_in * S;
    const over = st.ext_over_in * S;
    const tick = st.tick_len_in * S;
    const th = st.text_height_in * S;
    const tgap = st.dim_text_gap_in * S;
    var pa: V2 = undefined;
    var pb: V2 = undefined;
    var la: V2 = undefined; // dimension line endpoints (ordered along u)
    var lb: V2 = undefined;
    var u: V2 = undefined;
    var nrm: V2 = undefined;
    const eq = std.mem.eql;
    if (eq(u8, dir, "v")) {
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
    } else if (eq(u8, dir, "aligned")) {
        const d = to.sub(from).norm();
        const n = d.perp();
        pa = from.add(n.scale(offset));
        pb = to.add(n.scale(offset));
        la = pa;
        lb = pb;
        u = d;
        nrm = n;
    } else {
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
    }
    const pairs = [2][2]V2{ .{ from, pa }, .{ to, pb } };
    for (pairs) |pq| {
        const d = pq[1].sub(pq[0]);
        if (d.len() < 1e-9) continue;
        const dn = d.norm();
        try out.append(a, try pathItem(env, "dim", id, &.{ pq[0].add(dn.scale(gap)), pq[1].add(dn.scale(over)) }, false));
    }
    const dist: f64 = if (eq(u8, dir, "v")) @abs(to.y - from.y) else if (eq(u8, dir, "aligned")) from.dist(to) else @abs(to.x - from.x);
    const label: []const u8 = text orelse try units.fmtFtIn(a, dist);
    const label_f = try asciiFold(a, label);
    const tw = env.font.width(label_f, th);
    const fits = tw + 2.0 * tgap <= lb.sub(la).len() - tick;
    var lend = lb;
    if (!fits) lend = lb.add(u.scale(tw + 4.0 * tgap));
    try out.append(a, try pathItem(env, "dim", id, &.{ la, lend }, false));
    const tdir = u.add(nrm).norm();
    for ([2]V2{ la, lb }) |p| {
        try out.append(a, try pathItem(env, "profile", id, &.{ p.sub(tdir.scale(tick * 0.5)), p.add(tdir.scale(tick * 0.5)) }, false));
    }
    var ang = std.math.radiansToDegrees(std.math.atan2(u.y, u.x));
    if (ang > 90.0 + 1e-9 or ang <= -90.0 + 1e-9) ang += 180.0;
    if (eq(u8, dir, "v")) ang = 90.0;
    const tn = V2.init(-@sin(std.math.degreesToRadians(ang)), @cos(std.math.degreesToRadians(ang)));
    const center = if (fits) V2.mid(la, lb).add(tn.scale(tgap)) else lb.add(u.scale(2.0 * tgap + tw * 0.5 + tick)).add(tn.scale(tgap));
    try out.append(a, try textItem(env, "dims", "dim", id, label, center.x, center.y, th, ang, .center, .baseline));
}

fn labelItems(env: *Env, id: []const u8, text: []const u8, at: V2, out: *std.ArrayList(Item)) Allocator.Error!void {
    const t = try upperIf(env, text);
    try out.append(env.a, try textItem(env, "notes", "anno", id, t, at.x, at.y, env.style.label_height_in * env.S, 0, .center, .middle));
}

// ---- citations --------------------------------------------------------------------------------------------------------------

fn noteText(env: *Env, text: []const u8, cites: []const json.Value) Allocator.Error![]const u8 {
    const a = env.a;
    var s: std.ArrayList(u8) = .empty;
    try s.appendSlice(a, try upperIf(env, text));
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
    return s.items;
}

// ---- main entry ---------------------------------------------------------------------------------------------------------------

/// Annotate a view. Returns the annotation items (notes interleaved after their annotation's own
/// items, in view order). Hatch lines in `base_items` are knocked out under dimension text and labels.
pub fn annotate(env: *Env, base_items: []Item) Allocator.Error![]const Item {
    const a = env.a;
    const spec = env.spec;
    const vid = spec.id;
    var notes: std.ArrayList(NoteIn) = .empty;
    var note_slot: std.ArrayList(?usize) = .empty;
    var per: std.ArrayList(std.ArrayList(Item)) = .empty;
    var seen: std.ArrayList([]const u8) = .empty;
    const types = [_][]const u8{ "note", "dim", "label" };
    for (spec.annotations, 0..) |an, k| {
        var its: std.ArrayList(Item) = .empty;
        var slot: ?usize = null;
        defer {
            per.append(a, its) catch {};
            note_slot.append(a, slot) catch {};
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
            const text = (if (an.get("text")) |x| x.str() else null) orelse {
                env.diags.add(.@"error", "E_PARAM", id, apath, "note '{s}' needs a string \"text\"", .{id});
                continue;
            };
            const cites: []const json.Value = if (an.get("cite")) |c| (c.arr() orelse &.{}) else &.{};
            const full = try noteText(env, text, cites);
            const target = (if (an.get("target")) |x| x.str() else null) orelse "";
            var landing: ?V2 = null;
            if (an.get("at")) |atv| {
                if (atv != .null) landing = env.scene.point(atv, id, try std.fmt.allocPrint(a, "{s}/at", .{apath}));
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
            if (an.get("place")) |pv| if (pv.arr()) |pa| if (pa.len >= 2) {
                if (units.parseLength(pa[0])) |x| if (units.parseLength(pa[1])) |y| {
                    place = V2.init(x, y);
                };
            };
            if (landing) |l| {
                slot = notes.items.len;
                try notes.append(a, .{ .id = id, .text = full, .landing = l, .place = place });
            } else if (an.get("at") == null) {
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
            const dir = (if (an.get("dir")) |x| x.str() else null) orelse "h";
            if (!(std.mem.eql(u8, dir, "h") or std.mem.eql(u8, dir, "v") or std.mem.eql(u8, dir, "aligned"))) {
                env.diags.add(.@"error", "E_PARAM", id, try std.fmt.allocPrint(a, "{s}/dir", .{apath}), "dim dir must be \"h\", \"v\" or \"aligned\" (got \"{s}\")", .{dir});
                continue;
            }
            const off = if (an.get("offset")) |x| (units.parseLength(x) orelse 0) else 0;
            const text: ?[]const u8 = if (an.get("text")) |x| x.str() else null;
            try dimItems(env, id, from, to, dir, off, text, &its);
        } else if (std.mem.eql(u8, ty, "label")) {
            const text = (if (an.get("text")) |x| x.str() else null) orelse {
                env.diags.add(.@"error", "E_PARAM", id, apath, "label '{s}' needs a string \"text\"", .{id});
                continue;
            };
            const atv = an.get("at") orelse {
                env.diags.add(.@"error", "E_PARAM", id, apath, "label '{s}' needs \"at\": a Ref or [x, y]", .{id});
                continue;
            };
            var p = env.scene.point(atv, id, try std.fmt.allocPrint(a, "{s}/at", .{apath})) orelse continue;
            if (an.get("offset")) |ov| if (ov.arr()) |oa| if (oa.len >= 2) {
                p = p.add(V2.init(units.parseLength(oa[0]) orelse 0, units.parseLength(oa[1]) orelse 0));
            };
            switch (env.landing) {
                .section => try labelItems(env, id, text, p, &its),
                .iso => |iso| if (iso.project(p)) |pp| try labelItems(env, id, text, pp, &its),
            }
        } else {
            env.diags.add(.@"error", "E_PARAM", id, try std.fmt.allocPrint(a, "{s}/type", .{apath}), "annotation '{s}' has type \"{s}\"; use one of {s}", .{ id, ty, model.joinQuoted(a, &types) });
        }
    }
    // extents and obstacles
    var ext = env.crop;
    var obstacles: std.ArrayList([4]V2) = .empty;
    var knock: std.ArrayList([4]V2) = .empty;
    for (per.items) |its| {
        ext.addBox(itemsBox(env.font, its.items));
        for (its.items) |it| if (it == .text) {
            try obstacles.append(a, textPoly(env.font, it.text, 0.03 * env.S));
            try knock.append(a, textPoly(env.font, it.text, 0.02 * env.S));
        };
    }
    const outs = try a.alloc(std.ArrayList(Item), notes.items.len);
    for (outs) |*o| o.* = .empty;
    try layoutNotes(env, notes.items, ext, obstacles.items, outs);
    try knockHatch(a, base_items, knock.items);
    var result: std.ArrayList(Item) = .empty;
    for (per.items, 0..) |its, k| {
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
    try out.append(a, .{ .path = .{ .layer = layerName(env, "title"), .pen = "title", .src = src, .closed = true, .pts = bubble } });
    const th = st.title_height_in * S;
    const nh = st.text_height_in * S;
    if (info.sheet.len == 0) {
        try out.append(a, try textItem(env, "title", "title", src, info.number, cx, cy, th, 0, .center, .middle));
    } else {
        try out.append(a, try pathItem(env, "anno", src, &.{ V2.init(cx - r, cy), V2.init(cx + r, cy) }, false));
        try out.append(a, try textItem(env, "title", "title", src, info.number, cx, cy + r * 0.5, th, 0, .center, .middle));
        try out.append(a, try textItem(env, "title", "anno", src, info.sheet, cx, cy - r * 0.5, st.label_height_in * 0.9 * S, 0, .center, .middle));
    }
    const tx = cx + r + 0.15 * S;
    const title = try upperIf(env, info.title);
    const ty = cy + 0.02 * S;
    try out.append(a, try textItem(env, "title", "title", src, title, tx, ty, th, 0, .left, .baseline));
    const tw = env.font.width(try asciiFold(a, title), th);
    const uy = ty - 0.07 * S;
    try out.append(a, try pathItem(env, "title", src, &.{ V2.init(tx, uy), V2.init(tx + tw, uy) }, false));
    const scale_line = try std.fmt.allocPrint(a, "SCALE: {s}", .{info.scale_text});
    const sy = uy - 0.06 * S - nh;
    try out.append(a, try textItem(env, "title", "anno", src, scale_line, tx, sy, nh, 0, .left, .baseline));
    var low = @min(cy - r, sy - 0.02 * S);
    if (info.unverified) {
        const fy = low - 0.1 * S - nh;
        try out.append(a, try textItem(env, "title", "anno", "footnote", st.cite_footnote, tcrop.x0, fy, nh * 0.85, 0, .left, .baseline));
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
