//! Clip bulge polylines (lines and arcs, exactly) to an axis-aligned box, and scan loops against
//! axis-parallel lines.

const std = @import("std");
const geom = @import("geom.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Pt = geom.Pt;
const Box = geom.Box;

pub const Piece = struct {
    pts: []const Pt,
    closed: bool,
};

const tol = 1e-9;

fn inBox(b: Box, p: V2) bool {
    return p.x >= b.x0 - tol and p.x <= b.x1 + tol and p.y >= b.y0 - tol and p.y <= b.y1 + tol;
}

/// Liang-Barsky: parameter interval of segment p->q inside box, or null.
fn clipLine(p: V2, q: V2, b: Box) ?[2]f64 {
    var t0: f64 = 0;
    var t1: f64 = 1;
    const d = q.sub(p);
    const ps = [4]f64{ -d.x, d.x, -d.y, d.y };
    const qs = [4]f64{ p.x - b.x0, b.x1 - p.x, p.y - b.y0, b.y1 - p.y };
    for (0..4) |i| {
        if (@abs(ps[i]) < 1e-15) {
            if (qs[i] < -tol) return null;
        } else {
            const r = qs[i] / ps[i];
            if (ps[i] < 0) {
                if (r > t1) return null;
                if (r > t0) t0 = r;
            } else {
                if (r < t0) return null;
                if (r < t1) t1 = r;
            }
        }
    }
    if (t1 - t0 < 1e-12) return null;
    return .{ t0, t1 };
}

const Interval = [2]f64;

fn clipArc(a: Allocator, p0: V2, p1: V2, bulge: f64, b: Box) Allocator.Error![]Interval {
    var ts: std.ArrayList(f64) = .empty;
    try ts.append(a, 0);
    try ts.append(a, 1);
    const corners = [4]V2{ V2.init(b.x0, b.y0), V2.init(b.x1, b.y0), V2.init(b.x1, b.y1), V2.init(b.x0, b.y1) };
    for (0..4) |i| {
        var ta: [2]f64 = undefined;
        var tb: [2]f64 = undefined;
        const n = geom.arcSeg(p0, p1, bulge, corners[i], corners[(i + 1) % 4], &ta, &tb);
        for (0..n) |k| try ts.append(a, ta[k]);
    }
    std.mem.sort(f64, ts.items, {}, std.sort.asc(f64));
    var out: std.ArrayList(Interval) = .empty;
    var i: usize = 0;
    while (i + 1 < ts.items.len) : (i += 1) {
        const t0 = ts.items[i];
        const t1 = ts.items[i + 1];
        if (t1 - t0 < 1e-10) continue;
        const m = geom.segPoint(p0, p1, bulge, (t0 + t1) / 2);
        if (inBox(b, m)) {
            if (out.items.len > 0 and @abs(out.items[out.items.len - 1][1] - t0) < 1e-10) {
                out.items[out.items.len - 1][1] = t1;
            } else try out.append(a, .{ t0, t1 });
        }
    }
    return out.items;
}

/// Clip an open or closed bulge polyline to `box`. Fully-inside closed loops are returned unchanged.
pub fn clipPath(a: Allocator, pts: []const Pt, closed: bool, box: Box) Allocator.Error![]Piece {
    var pieces: std.ArrayList(Piece) = .empty;
    if (pts.len < 2) return pieces.items;
    const nseg = if (closed) pts.len else pts.len - 1;
    // Fast path: everything inside.
    var all_inside = true;
    for (pts, 0..) |p, i| {
        if (!inBox(box, p.v())) {
            all_inside = false;
            break;
        }
        if (p.b != 0 and (closed or i + 1 < pts.len)) {
            const q = pts[(i + 1) % pts.len];
            var bb = Box{};
            geom.segBoxInto(&bb, p.v(), q.v(), p.b);
            if (!inBox(box, V2.init(bb.x0, bb.y0)) or !inBox(box, V2.init(bb.x1, bb.y1))) {
                all_inside = false;
                break;
            }
        }
    }
    if (all_inside) {
        try pieces.append(a, .{ .pts = pts, .closed = closed });
        return pieces.items;
    }
    var cur: std.ArrayList(Pt) = .empty;
    var starts_at_zero_first = false;
    var last_end_full = false; // previous segment's kept interval ended at t = 1
    var i: usize = 0;
    while (i < nseg) : (i += 1) {
        const p = pts[i];
        const q = pts[(i + 1) % pts.len];
        var ivs: []const Interval = undefined;
        var single: [1]Interval = undefined;
        if (p.b == 0) {
            if (clipLine(p.v(), q.v(), box)) |iv| {
                single[0] = iv;
                ivs = single[0..1];
            } else ivs = &.{};
        } else {
            ivs = try clipArc(a, p.v(), q.v(), p.b, box);
        }
        if (ivs.len == 0) {
            if (cur.items.len > 0) {
                try flush(a, &pieces, &cur);
            }
            last_end_full = false;
            continue;
        }
        for (ivs, 0..) |iv, k| {
            const sub = geom.subSeg(p.v(), q.v(), p.b, iv[0], iv[1]);
            const continues = k == 0 and last_end_full and cur.items.len > 0 and iv[0] <= 1e-12;
            if (!continues) {
                if (cur.items.len > 0) {
                    try flush(a, &pieces, &cur);
                }
                if (i == 0 and k == 0 and iv[0] <= 1e-12) starts_at_zero_first = true;
                try cur.append(a, .{ .x = sub.a.x, .y = sub.a.y, .b = sub.bulge });
            } else {
                cur.items[cur.items.len - 1].b = sub.bulge;
            }
            try cur.append(a, .{ .x = sub.b.x, .y = sub.b.y, .b = 0 });
            last_end_full = k + 1 == ivs.len and iv[1] >= 1 - 1e-12;
            if (k + 1 < ivs.len) {
                try flush(a, &pieces, &cur);
            }
        }
    }
    if (cur.items.len > 0) {
        // Closed loop: join the tail with the head when the path runs through vertex 0.
        if (closed and last_end_full and starts_at_zero_first and pieces.items.len > 0) {
            const head = pieces.orderedRemove(0);
            var joined: std.ArrayList(Pt) = .empty;
            try joined.appendSlice(a, cur.items);
            joined.items[joined.items.len - 1].b = head.pts[0].b;
            try joined.appendSlice(a, head.pts[1..]);
            try pieces.append(a, .{ .pts = joined.items, .closed = false });
            cur = .empty;
        } else {
            try flush(a, &pieces, &cur);
        }
    }
    return pieces.items;
}

fn flush(a: Allocator, pieces: *std.ArrayList(Piece), cur: *std.ArrayList(Pt)) Allocator.Error!void {
    if (cur.items.len >= 2) {
        // last vertex carries no bulge
        cur.items[cur.items.len - 1].b = 0;
        try pieces.append(a, .{ .pts = cur.items, .closed = false });
    }
    cur.* = .empty;
}

/// Clip a straight segment to the box.
pub fn clipSeg(p: V2, q: V2, box: Box) ?[2]V2 {
    const iv = clipLine(p, q, box) orelse return null;
    return .{ V2.lerp(p, q, iv[0]), V2.lerp(p, q, iv[1]) };
}

/// Crossings of the loops with the vertical line x = c (returns sorted y values) using the
/// half-open rule, or with the horizontal line y = c when `vertical` is false (sorted x values).
pub fn scan(a: Allocator, loops: []const []const V2, c: f64, vertical: bool) Allocator.Error![]f64 {
    var out: std.ArrayList(f64) = .empty;
    for (loops) |l| {
        for (l, 0..) |p, i| {
            const q = l[(i + 1) % l.len];
            const pa = if (vertical) p.x else p.y;
            const qa = if (vertical) q.x else q.y;
            if ((pa < c) != (qa < c)) {
                const t = (c - pa) / (qa - pa);
                const pb = if (vertical) p.y else p.x;
                const qb = if (vertical) q.y else q.x;
                try out.append(a, pb + (qb - pb) * t);
            }
        }
    }
    std.mem.sort(f64, out.items, {}, std.sort.asc(f64));
    return out.items;
}

test "line clip" {
    const b = Box{ .x0 = 0, .y0 = 0, .x1 = 10, .y1 = 10 };
    const r = clipSeg(V2.init(-5, 5), V2.init(15, 5), b).?;
    try std.testing.expectApproxEqAbs(0.0, r[0].x, 1e-12);
    try std.testing.expectApproxEqAbs(10.0, r[1].x, 1e-12);
    try std.testing.expect(clipSeg(V2.init(-5, 20), V2.init(15, 20), b) == null);
}

test "closed rect clipped by a box becomes open pieces" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const rect = [_]Pt{ .{ .x = -5, .y = 2 }, .{ .x = 5, .y = 2 }, .{ .x = 5, .y = 8 }, .{ .x = -5, .y = 8 } };
    const b = Box{ .x0 = 0, .y0 = 0, .x1 = 10, .y1 = 10 };
    const r = try clipPath(a, &rect, true, b);
    try std.testing.expectEqual(@as(usize, 1), r.len);
    try std.testing.expectEqual(@as(usize, 4), r[0].pts.len);
    try std.testing.expect(!r[0].closed);
    // fully inside: unchanged and closed
    const b2 = Box{ .x0 = -10, .y0 = 0, .x1 = 10, .y1 = 10 };
    const r2 = try clipPath(a, &rect, true, b2);
    try std.testing.expect(r2[0].closed);
}

test "arc clip keeps the inside portion exactly" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    // upper unit semicircle, clip to y >= 0.5
    const pts = [_]Pt{ .{ .x = 1, .y = 0, .b = 1 }, .{ .x = -1, .y = 0 } };
    const b = Box{ .x0 = -2, .y0 = 0.5, .x1 = 2, .y1 = 2 };
    const r = try clipPath(a, &pts, false, b);
    try std.testing.expectEqual(@as(usize, 1), r.len);
    try std.testing.expectEqual(@as(usize, 2), r[0].pts.len);
    const p0 = r[0].pts[0];
    try std.testing.expectApproxEqAbs(0.5, p0.y, 1e-9);
    try std.testing.expectApproxEqAbs(std.math.pi * 2.0 / 3.0, 4.0 * std.math.atan(r[0].pts[0].b), 1e-9);
}
