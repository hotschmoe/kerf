//! Polyline helpers for thickened members: fillets, offsets, ribbons.

const std = @import("std");
const geom = @import("geom.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Pt = geom.Pt;

/// Reverse an open bulge polyline (the last vertex carries bulge 0).
pub fn reverseOpen(a: Allocator, pts: []const Pt) Allocator.Error![]Pt {
    const n = pts.len;
    const out = try a.alloc(Pt, n);
    for (0..n) |i| {
        const src = n - 1 - i;
        // new segment i goes from old vertex src to old vertex src-1 (old segment src-1 reversed)
        const b: f64 = if (src > 0) -pts[src - 1].b else 0;
        out[i] = .{ .x = pts[src].x, .y = pts[src].y, .b = if (i + 1 == n) 0 else b };
    }
    return out;
}

const SegOff = struct { a: V2, b: V2, bulge: f64 };

fn offsetSeg(p0: V2, p1: V2, bulge: f64, d: f64) SegOff {
    if (bulge == 0) {
        const dir = p1.sub(p0).norm();
        const n = dir.perp().scale(d);
        return .{ .a = p0.add(n), .b = p1.add(n), .bulge = 0 };
    }
    const arc = geom.arcOf(p0, p1, bulge);
    const r2 = if (bulge > 0) arc.r - d else arc.r + d;
    const rr = @max(r2, 1e-9);
    const k = rr / arc.r;
    return .{
        .a = arc.c.add(p0.sub(arc.c).scale(k)),
        .b = arc.c.add(p1.sub(arc.c).scale(k)),
        .bulge = bulge,
    };
}

fn lineIntersect(a0: V2, a1: V2, b0: V2, b1: V2) ?V2 {
    const r = a1.sub(a0);
    const s = b1.sub(b0);
    const den = r.cross(s);
    if (@abs(den) < 1e-12 * @max(1.0, r.len() * s.len())) return null;
    const t = b0.sub(a0).cross(s) / den;
    return a0.add(r.scale(t));
}

/// Offset an open bulge polyline to its left by `d` (negative = right). Line-line joints are mitered.
pub fn offsetOpen(a: Allocator, pts: []const Pt, d: f64) Allocator.Error![]Pt {
    const nseg = pts.len -| 1;
    if (nseg == 0) return a.dupe(Pt, pts);
    const segs = try a.alloc(SegOff, nseg);
    for (0..nseg) |i| segs[i] = offsetSeg(pts[i].v(), pts[i + 1].v(), pts[i].b, d);
    // joints
    for (0..nseg - 1) |i| {
        const s0 = &segs[i];
        const s1 = &segs[i + 1];
        if (V2.eql(s0.b, s1.a, 1e-9)) continue;
        if (s0.bulge == 0 and s1.bulge == 0) {
            if (lineIntersect(s0.a, s0.b, s1.a, s1.b)) |m| {
                const lim = 4.0 * @max(@abs(d), 1e-9);
                if (m.dist(pts[i + 1].v()) <= lim) {
                    s0.b = m;
                    s1.a = m;
                }
            }
        }
    }
    var out: std.ArrayList(Pt) = .empty;
    for (0..nseg) |i| {
        try out.append(a, .{ .x = segs[i].a.x, .y = segs[i].a.y, .b = segs[i].bulge });
        if (i + 1 < nseg and !V2.eql(segs[i].b, segs[i + 1].a, 1e-9)) {
            try out.append(a, .{ .x = segs[i].b.x, .y = segs[i].b.y, .b = 0 });
        }
    }
    try out.append(a, .{ .x = segs[nseg - 1].b.x, .y = segs[nseg - 1].b.y, .b = 0 });
    return out.items;
}

/// Closed ribbon around `center`: `left` to the left side, `right` to the right side, flat end caps.
pub fn ribbon(a: Allocator, center: []const Pt, left: f64, right: f64) Allocator.Error![]Pt {
    const l = try offsetOpen(a, center, left);
    const r0 = try offsetOpen(a, center, -right);
    const r = try reverseOpen(a, r0);
    var out: std.ArrayList(Pt) = .empty;
    try out.appendSlice(a, l);
    try out.appendSlice(a, r);
    // collapse coincident neighbours (zero thickness on one side)
    var cleaned: std.ArrayList(Pt) = .empty;
    for (out.items, 0..) |p, i| {
        const prev = out.items[(i + out.items.len - 1) % out.items.len];
        if (cleaned.items.len > 0 and V2.eql(p.v(), prev.v(), 1e-12) and prev.b == 0) continue;
        try cleaned.append(a, p);
    }
    // closing duplicate
    if (cleaned.items.len > 1 and V2.eql(cleaned.items[0].v(), cleaned.items[cleaned.items.len - 1].v(), 1e-12) and cleaned.items[cleaned.items.len - 1].b == 0) {
        _ = cleaned.pop();
    }
    return cleaned.items;
}

/// Fillet the interior vertices of a polyline with radius `R` (reduced where segments are short).
/// Output is an open bulge polyline.
pub fn fillet(a: Allocator, pts: []const V2, R: f64) Allocator.Error![]Pt {
    var out: std.ArrayList(Pt) = .empty;
    if (pts.len == 0) return out.items;
    if (pts.len < 3 or R <= 0) {
        for (pts) |p| try out.append(a, Pt.at(p, 0));
        return out.items;
    }
    try out.append(a, Pt.at(pts[0], 0));
    var i: usize = 1;
    while (i + 1 < pts.len) : (i += 1) {
        const p = pts[i];
        const u = p.sub(pts[i - 1]);
        const w = pts[i + 1].sub(p);
        const lu = u.len();
        const lw = w.len();
        if (lu < 1e-12 or lw < 1e-12) continue;
        const ud = u.scale(1.0 / lu);
        const wd = w.scale(1.0 / lw);
        const cr = ud.cross(wd);
        const dt = ud.dot(wd);
        const phi = std.math.atan2(cr, dt); // signed turning angle
        if (@abs(phi) < 1e-9) {
            try out.append(a, Pt.at(p, 0));
            continue;
        }
        var r = R;
        const t_need = r * @tan(@abs(phi) / 2.0);
        // tangent length may use at most half of each adjacent segment (or all of an end segment)
        const lim_u = if (i == 1) lu else lu / 2.0;
        const lim_w = if (i + 2 == pts.len) lw else lw / 2.0;
        const lim = @min(lim_u, lim_w);
        if (t_need > lim) r = lim / @tan(@abs(phi) / 2.0);
        const t = r * @tan(@abs(phi) / 2.0);
        const p1 = p.sub(ud.scale(t));
        const p2 = p.add(wd.scale(t));
        try out.append(a, Pt.at(p1, geom.bulgeFromSweep(phi)));
        try out.append(a, Pt.at(p2, 0));
    }
    try out.append(a, Pt.at(pts[pts.len - 1], 0));
    return out.items;
}

test "ribbon of a straight line is a rectangle" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const c = [_]Pt{ .{ .x = 0, .y = 0 }, .{ .x = 10, .y = 0 } };
    const r = try ribbon(a, &c, 0.5, 0.5);
    try std.testing.expectEqual(@as(usize, 4), r.len);
    try std.testing.expectApproxEqAbs(10.0, @abs(geom.signedArea(r)), 1e-12);
    // left grows up for a left-to-right line
    const up = try ribbon(a, &c, 1.0, 0.0);
    const bb = geom.loopBox(up);
    try std.testing.expectApproxEqAbs(0.0, bb.y0, 1e-12);
    try std.testing.expectApproxEqAbs(1.0, bb.y1, 1e-12);
}

test "fillet makes a tangent arc" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const pts = [_]V2{ V2.init(0, 0), V2.init(10, 0), V2.init(10, 10) };
    const f = try fillet(a, &pts, 2.0);
    try std.testing.expectEqual(@as(usize, 4), f.len);
    try std.testing.expectApproxEqAbs(8.0, f[1].x, 1e-12);
    try std.testing.expectApproxEqAbs(2.0, f[2].y, 1e-12);
    try std.testing.expectApproxEqAbs(@tan(std.math.pi / 8.0), f[1].b, 1e-12);
    // ribbon around the filleted bend keeps constant width
    const rb = try ribbon(a, f, 0.25, 0.25);
    const area = @abs(geom.signedArea(rb));
    const len = geom.segLength(f[0].v(), f[1].v(), 0) + geom.segLength(f[1].v(), f[2].v(), f[1].b) + geom.segLength(f[2].v(), f[3].v(), 0);
    try std.testing.expectApproxEqAbs(0.5 * len, area, 1e-9);
}
