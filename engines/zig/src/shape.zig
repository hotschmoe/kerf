//! Visible-region shapes and where a label or a note's arrow can land inside them (SPEC 6.3, SPEC 18): pure geometry on
//! polygons with holes, used by both the section and the iso annotation code (it is the module `annot.zig` and `iso.zig`
//! share, so they no longer import each other).

const std = @import("std");
const geom = @import("geom.zig");
const clip = @import("clip.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Box = geom.Box;

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
pub fn bandedLabelPoint(a: Allocator, shapes: []const Shape, crop: Box, h: f64) Allocator.Error!?struct { p: V2, inset: Box } {
    const prim = (try labelPoint(a, shapes)) orelse return null;
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
pub fn candidatesFor(a: Allocator, shapes: []const Shape, inset: Box, primary: V2, h: f64) Allocator.Error![]const V2 {
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
