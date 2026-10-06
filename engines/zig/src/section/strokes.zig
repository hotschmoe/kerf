//! Section strokes (SPEC 8.1): the Stroke record every drawn line becomes, removal of collinear overlapping edges (the heavier pen wins) and chaining of touching open strokes into polylines.

const std = @import("std");
const geom = @import("../geom.zig");
const style_mod = @import("../style.zig");
const Pen = @import("../pen.zig").Pen;
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Pt = geom.Pt;
const Box = geom.Box;

pub const Stroke = struct {
    pts: []const Pt,
    closed: bool,
    pen: Pen,
    src: []const u8,
    /// Paint rank: lighter first, embedded last.
    rank: f64,
    seq: u32,
};

pub const SegRef = struct {
    a: V2,
    b: V2,
    bulge: f64,
    stroke: u32,
    /// Remaining visible intervals along the segment ([t0,t1]); empty when fully removed.
    keep: std.ArrayList([2]f64) = .empty,
};

pub const dedupe_tol: f64 = 1e-4;

/// Remove collinear overlapping straight edges, keeping the heavier pen's (ties: the earlier).
pub fn dedupe(a: Allocator, strokes: []const Stroke, style: *const style_mod.Style) Allocator.Error![]Stroke {
    var segs: std.ArrayList(SegRef) = .empty;
    var seg_start = try a.alloc(usize, strokes.len + 1);
    for (strokes, 0..) |s, si| {
        seg_start[si] = segs.items.len;
        const nseg = if (s.closed) s.pts.len else s.pts.len -| 1;
        for (0..nseg) |k| {
            const p = s.pts[k];
            const q = s.pts[(k + 1) % s.pts.len];
            var sr = SegRef{ .a = p.v(), .b = q.v(), .bulge = p.b, .stroke = @intCast(si) };
            try sr.keep.append(a, .{ 0, 1 });
            try segs.append(a, sr);
        }
    }
    seg_start[strokes.len] = segs.items.len;
    const items = segs.items;
    var extras: std.ArrayList(Stroke) = .empty;
    // pairwise subtraction
    for (items, 0..) |*s1, i| {
        if (s1.bulge != 0) continue;
        const d1 = s1.b.sub(s1.a);
        const l1 = d1.len();
        if (l1 < 1e-9) continue;
        const ud1 = d1.scale(1.0 / l1);
        const w1 = style.penWidthMm(strokes[s1.stroke].pen);
        const bb1 = Box{ .x0 = @min(s1.a.x, s1.b.x), .y0 = @min(s1.a.y, s1.b.y), .x1 = @max(s1.a.x, s1.b.x), .y1 = @max(s1.a.y, s1.b.y) };
        for (items[i + 1 ..]) |*s2| {
            if (s2.bulge != 0) continue;
            const bb2 = Box{ .x0 = @min(s2.a.x, s2.b.x), .y0 = @min(s2.a.y, s2.b.y), .x1 = @max(s2.a.x, s2.b.x), .y1 = @max(s2.a.y, s2.b.y) };
            if (!bb1.overlaps(bb2, dedupe_tol)) continue;
            const d2 = s2.b.sub(s2.a);
            const l2 = d2.len();
            if (l2 < 1e-9) continue;
            // collinear?
            if (@abs(ud1.cross(d2)) / l2 > 1e-6 and @abs(ud1.cross(d2)) > dedupe_tol) continue;
            const off_a = @abs(ud1.cross(s2.a.sub(s1.a)));
            const off_b = @abs(ud1.cross(s2.b.sub(s1.a)));
            if (off_a > dedupe_tol or off_b > dedupe_tol) continue;
            // overlap interval on s1 parameter
            const ta = s2.a.sub(s1.a).dot(ud1) / l1;
            const tb = s2.b.sub(s1.a).dot(ud1) / l1;
            const lo = @max(0.0, @min(ta, tb));
            const hi = @min(1.0, @max(ta, tb));
            if (hi - lo < dedupe_tol / l1) continue;
            const w2 = style.penWidthMm(strokes[s2.stroke].pen);
            if (s1.stroke != s2.stroke and std.mem.eql(u8, baseId(strokes[s1.stroke].src), baseId(strokes[s2.stroke].src))) {
                // shared edge between prisms of the same component: one line in the beyond pen
                const p_lo = s1.a.add(d1.scale(lo));
                const p_hi = s1.a.add(d1.scale(hi));
                const t2a = p_lo.sub(s2.a).dot(d2) / (l2 * l2);
                const t2b = p_hi.sub(s2.a).dot(d2) / (l2 * l2);
                try subtractInterval(a, &s1.keep, lo, hi);
                try subtractInterval(a, &s2.keep, @max(0.0, @min(t2a, t2b)), @min(1.0, @max(t2a, t2b)));
                try extras.append(a, .{ .pts = try a.dupe(Pt, &.{ Pt.at(p_lo, 0), Pt.at(p_hi, 0) }), .closed = false, .pen = .beyond, .src = strokes[s1.stroke].src, .rank = style.penWidthMm(.beyond), .seq = strokes[s1.stroke].seq });
                continue;
            }
            // the lighter (or later on ties) loses the overlap
            const lose1 = w1 < w2;
            if (lose1) {
                try subtractInterval(a, &s1.keep, lo, hi);
            } else {
                // map [lo,hi] on s1 to s2 parameters
                const p_lo = s1.a.add(d1.scale(lo));
                const p_hi = s1.a.add(d1.scale(hi));
                const t2a = p_lo.sub(s2.a).dot(d2) / (l2 * l2);
                const t2b = p_hi.sub(s2.a).dot(d2) / (l2 * l2);
                try subtractInterval(a, &s2.keep, @max(0.0, @min(t2a, t2b)), @min(1.0, @max(t2a, t2b)));
            }
        }
    }
    // rebuild strokes
    var out: std.ArrayList(Stroke) = .empty;
    for (strokes, 0..) |s, si| {
        const lo = seg_start[si];
        const hi = seg_start[si + 1];
        var untouched = true;
        for (items[lo..hi]) |sr| {
            if (sr.keep.items.len != 1 or sr.keep.items[0][0] != 0 or sr.keep.items[0][1] != 1) {
                untouched = false;
                break;
            }
        }
        if (untouched) {
            try out.append(a, s);
            continue;
        }
        var cur: std.ArrayList(Pt) = .empty;
        var emitted: usize = 0;
        for (items[lo..hi]) |sr| {
            for (sr.keep.items) |iv| {
                const sub = geom.subSeg(sr.a, sr.b, sr.bulge, iv[0], iv[1]);
                if (sub.a.dist(sub.b) < 1e-9) continue;
                const joins = cur.items.len > 0 and V2.eql(cur.items[cur.items.len - 1].v(), sub.a, 1e-7);
                if (!joins) {
                    if (cur.items.len > 1) {
                        try out.append(a, .{ .pts = cur.items, .closed = false, .pen = s.pen, .src = s.src, .rank = s.rank, .seq = s.seq });
                        emitted += 1;
                    }
                    cur = .empty;
                    try cur.append(a, .{ .x = sub.a.x, .y = sub.a.y, .b = sub.bulge });
                } else cur.items[cur.items.len - 1].b = sub.bulge;
                try cur.append(a, .{ .x = sub.b.x, .y = sub.b.y, .b = 0 });
            }
        }
        if (cur.items.len > 1) {
            try out.append(a, .{ .pts = cur.items, .closed = false, .pen = s.pen, .src = s.src, .rank = s.rank, .seq = s.seq });
        }
    }
    try out.appendSlice(a, extras.items);
    return out.items;
}

pub fn subtractInterval(a: Allocator, keep: *std.ArrayList([2]f64), lo: f64, hi: f64) Allocator.Error!void {
    var out: std.ArrayList([2]f64) = .empty;
    for (keep.items) |iv| {
        if (hi <= iv[0] or lo >= iv[1]) {
            try out.append(a, iv);
            continue;
        }
        if (lo > iv[0]) try out.append(a, .{ iv[0], lo });
        if (hi < iv[1]) try out.append(a, .{ hi, iv[1] });
    }
    keep.* = out;
}

/// Join open strokes (same pen and src) whose end points touch into longer polylines.
pub fn chainStrokes(a: Allocator, in: []Stroke) Allocator.Error![]Stroke {
    var list: std.ArrayList(Stroke) = .empty;
    try list.appendSlice(a, in);
    var changed = true;
    while (changed) {
        changed = false;
        var i: usize = 0;
        while (i < list.items.len) : (i += 1) {
            if (list.items[i].closed) continue;
            var j: usize = i + 1;
            while (j < list.items.len) : (j += 1) {
                const si = list.items[i];
                const sj = list.items[j];
                if (sj.closed or si.pen != sj.pen or !std.mem.eql(u8, si.src, sj.src)) continue;
                const i_end = si.pts[si.pts.len - 1].v();
                const i_start = si.pts[0].v();
                const j_start = sj.pts[0].v();
                const j_end = sj.pts[sj.pts.len - 1].v();
                var merged: ?[]Pt = null;
                // arcs are not reversible cheaply here; only join in travel direction
                if (V2.eql(i_end, j_start, 1e-7)) {
                    var m: std.ArrayList(Pt) = .empty;
                    try m.appendSlice(a, si.pts);
                    m.items[m.items.len - 1].b = sj.pts[0].b;
                    try m.appendSlice(a, sj.pts[1..]);
                    merged = m.items;
                } else if (V2.eql(j_end, i_start, 1e-7)) {
                    var m: std.ArrayList(Pt) = .empty;
                    try m.appendSlice(a, sj.pts);
                    m.items[m.items.len - 1].b = si.pts[0].b;
                    try m.appendSlice(a, si.pts[1..]);
                    merged = m.items;
                }
                if (merged) |mp| {
                    // drop collinear interior vertices of pure-line runs
                    list.items[i].pts = mp;
                    list.items[i].seq = @min(si.seq, sj.seq);
                    _ = list.orderedRemove(j);
                    changed = true;
                    break;
                }
            }
        }
    }
    // close loops whose ends meet
    for (list.items) |*st| {
        if (!st.closed and st.pts.len >= 4 and V2.eql(st.pts[0].v(), st.pts[st.pts.len - 1].v(), 1e-7) and st.pts[st.pts.len - 1].b == 0) {
            st.pts = st.pts[0 .. st.pts.len - 1];
            st.closed = true;
        }
    }
    return list.items;
}

pub fn baseId(src: []const u8) []const u8 {
    if (std.mem.indexOfScalar(u8, src, '#')) |h| return src[0..h];
    return src;
}
