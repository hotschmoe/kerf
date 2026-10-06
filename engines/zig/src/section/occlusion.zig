//! Visibility of a loop or polyline against the cut and beyond bodies in front of it (SPEC 8.1): exact classification of the parts that stay visible.

const std = @import("std");
const geom = @import("../geom.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Pt = geom.Pt;
const Box = geom.Box;

pub const Occ = struct {
    loops: []const []const V2,
    box: Box,
    z1: f64,
    cut: bool,
    prism: usize,
};

pub const PieceOut = struct { pts: []const Pt, closed: bool };

pub fn pushUnique(a: Allocator, ts: *std.ArrayList(f64), t: f64) Allocator.Error!void {
    if (t > 1e-9 and t < 1 - 1e-9) try ts.append(a, t);
}

/// The parts of `loop` (a closed bulge loop) not hidden inside any occluder. A visible run that wraps through vertex 0 is
/// returned as one piece.
pub fn visiblePieces(a: Allocator, loop: []const Pt, occ: []const *const Occ) Allocator.Error![]const PieceOut {
    return visibleImpl(a, loop, true, true, occ);
}

/// Like `visiblePieces` but also for open polylines (arcs allowed): the parts outside every occluder.
/// Closed input is not re-joined across vertex 0.
pub fn visibleOpen(a: Allocator, pts: []const Pt, closed: bool, occ: []const *const Occ) Allocator.Error![]const PieceOut {
    return visibleImpl(a, pts, closed, false, occ);
}

pub fn visibleImpl(a: Allocator, pts: []const Pt, closed: bool, join_wrap: bool, occ: []const *const Occ) Allocator.Error![]const PieceOut {
    var out: std.ArrayList(PieceOut) = .empty;
    const n = pts.len;
    const nseg = if (closed) n else n -| 1;
    var cur: std.ArrayList(Pt) = .empty;
    var any_hidden = false;
    var starts_visible_at_0 = false;
    var ts: std.ArrayList(f64) = .empty;
    for (0..nseg) |i| {
        const p0 = pts[i].v();
        const p1 = pts[(i + 1) % n].v();
        const bulge = pts[i].b;
        var sbox = Box{};
        geom.segBoxInto(&sbox, p0, p1, bulge);
        ts.clearRetainingCapacity();
        for (occ) |o| {
            if (!o.box.overlaps(sbox, 1e-7)) continue;
            for (o.loops) |ol| {
                for (ol, 0..) |q0, k| {
                    const q1 = ol[(k + 1) % ol.len];
                    var ta: [2]f64 = undefined;
                    var tb: [2]f64 = undefined;
                    const cnt = if (bulge == 0) geom.segSeg(p0, p1, q0, q1, &ta, &tb) else geom.arcSeg(p0, p1, bulge, q0, q1, &ta, &tb);
                    for (0..cnt) |c| try pushUnique(a, &ts, ta[c]);
                }
            }
        }
        std.mem.sort(f64, ts.items, {}, std.sort.asc(f64));
        var t_prev: f64 = 0;
        var k: usize = 0;
        while (k <= ts.items.len) : (k += 1) {
            const t_next: f64 = if (k < ts.items.len) ts.items[k] else 1;
            if (t_next - t_prev < 1e-10) {
                t_prev = t_next;
                continue;
            }
            const sub = geom.subSeg(p0, p1, bulge, t_prev, t_next);
            const mid = geom.segPoint(p0, p1, bulge, (t_prev + t_next) / 2);
            var hidden = false;
            for (occ) |o| {
                if (!o.box.contains(mid)) continue;
                if (geom.locate(mid, o.loops, 1e-7) == .inside) {
                    hidden = true;
                    break;
                }
            }
            if (hidden) {
                any_hidden = true;
                if (cur.items.len > 0) {
                    cur.items[cur.items.len - 1].b = 0;
                    try out.append(a, .{ .pts = cur.items, .closed = false });
                    cur = .empty;
                }
            } else {
                const joins = cur.items.len > 0 and V2.eql(cur.items[cur.items.len - 1].v(), sub.a, 1e-9);
                if (!joins) {
                    if (cur.items.len > 1) {
                        cur.items[cur.items.len - 1].b = 0;
                        try out.append(a, .{ .pts = cur.items, .closed = false });
                    }
                    cur = .empty;
                    try cur.append(a, .{ .x = sub.a.x, .y = sub.a.y, .b = sub.bulge });
                    if (i == 0 and t_prev == 0) starts_visible_at_0 = true;
                } else cur.items[cur.items.len - 1].b = sub.bulge;
                try cur.append(a, .{ .x = sub.b.x, .y = sub.b.y, .b = 0 });
            }
            t_prev = t_next;
        }
    }
    if (!any_hidden) {
        out.clearRetainingCapacity();
        try out.append(a, .{ .pts = pts, .closed = closed });
        return out.items;
    }
    if (cur.items.len > 1) {
        if (join_wrap and starts_visible_at_0 and out.items.len > 0 and out.items[0].pts.len > 0) {
            // the loop wraps through vertex 0: tail + head are one run
            const head = out.orderedRemove(0);
            var joined: std.ArrayList(Pt) = .empty;
            try joined.appendSlice(a, cur.items);
            joined.items[joined.items.len - 1].b = head.pts[0].b;
            try joined.appendSlice(a, head.pts[1..]);
            try out.append(a, .{ .pts = joined.items, .closed = false });
        } else {
            try out.append(a, .{ .pts = cur.items, .closed = false });
        }
    }
    return out.items;
}
