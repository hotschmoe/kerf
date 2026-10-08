//! Polygon booleans (intersection, union, difference) on flattened polygons.
//!
//! Method: arrangement + classification. Every boundary edge of A and B is split at all mutual
//! intersections (robust orientation predicates decide crossings, touches and collinear overlaps;
//! cut points are computed once per pair so both edges share the same vertex), coincident vertices
//! are merged within `snap`, each resulting sub-edge is classified against the other operand by its
//! midpoint (inside / outside / on-boundary, with the on-boundary case resolved by edge direction),
//! and the surviving edges are chained into loops. This handles degenerate input (shared edges,
//! touching vertices, T-junctions, identical operands) deterministically, and is O(n*m), which is
//! plenty for detail-sized polygons.
//!
//! Convention for operands and results: loops are CCW when they bound filled area and CW when they
//! bound a hole.

const std = @import("std");
const sort = @import("sort.zig");
const geom = @import("geom.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;

pub const Op = enum { intersect, unite, diff };

/// Boundary band and vertex-merge distance, inches.
pub const snap: f64 = 1e-7;

const Edge = struct { a: V2, b: V2 };

const Cut = struct { t: f64, p: V2 };

fn lessCut(_: void, x: Cut, y: Cut) bool {
    return x.t < y.t;
}

const Sub = struct {
    from: u32,
    to: u32,
    src: u8,
    keep: bool = false,
};

/// Convert a Region (loop 0 outer, others holes; bulge loops) to oriented flattened loops.
pub fn fromRegion(a: Allocator, loops: []const geom.Loop, tol: f64) Allocator.Error![][]V2 {
    const out = try a.alloc([]V2, loops.len);
    for (loops, 0..) |lp, i| {
        const pts = try geom.flattenLoop(a, lp, tol);
        const area = geom.signedAreaV(pts);
        if ((i == 0 and area < 0) or (i > 0 and area > 0)) std.mem.reverse(V2, pts);
        out[i] = pts;
    }
    return out;
}

/// Orient a single loop CCW (filled).
pub fn orientCcw(pts: []V2) void {
    if (geom.signedAreaV(pts) < 0) std.mem.reverse(V2, pts);
}

pub fn totalArea(loops: []const []const V2) f64 {
    var s: f64 = 0;
    for (loops) |l| s += geom.signedAreaV(l);
    return s;
}

pub fn loopsBox(loops: []const []const V2) geom.Box {
    var b = geom.Box{};
    for (loops) |l| for (l) |p| b.addPoint(p.x, p.y);
    return b;
}

pub fn boolean(a: Allocator, A: []const []const V2, B: []const []const V2, op: Op) Allocator.Error![][]V2 {
    // Trivial cases.
    if (A.len == 0) {
        if (op == .unite) return cloneLoops(a, B);
        return &.{};
    }
    if (B.len == 0) {
        if (op == .intersect) return &.{};
        return cloneLoops(a, A);
    }
    // Quick reject: disjoint boxes.
    const ba = loopsBox(A);
    const bb = loopsBox(B);
    if (!ba.overlaps(bb, snap * 4)) {
        return switch (op) {
            .intersect => &.{},
            .unite => blk: {
                const out = try a.alloc([]V2, A.len + B.len);
                for (A, 0..) |l, i| out[i] = try a.dupe(V2, l);
                for (B, 0..) |l, i| out[A.len + i] = try a.dupe(V2, l);
                break :blk out;
            },
            .diff => cloneLoops(a, A),
        };
    }

    // Edge lists.
    var edges: std.ArrayList(Edge) = .empty;
    var src: std.ArrayList(u8) = .empty;
    for (A) |l| try appendLoopEdges(a, &edges, &src, l, 0);
    const na = edges.items.len;
    for (B) |l| try appendLoopEdges(a, &edges, &src, l, 1);
    const ne = edges.items.len;

    const cuts = try a.alloc(std.ArrayList(Cut), ne);
    for (cuts) |*c| c.* = .empty;

    // Pairwise intersections between A and B edges.
    var i: usize = 0;
    while (i < na) : (i += 1) {
        const ea = edges.items[i];
        const bxa = segBox(ea);
        var j: usize = na;
        while (j < ne) : (j += 1) {
            const eb = edges.items[j];
            if (!bxa.overlaps(segBox(eb), snap)) continue;
            var ta: [2]f64 = undefined;
            var tb: [2]f64 = undefined;
            const n = geom.segSeg(ea.a, ea.b, eb.a, eb.b, &ta, &tb);
            if (n == 0) continue;
            if (n == 1) {
                const t = ta[0];
                const u = tb[0];
                const p = if (t <= 0) ea.a else if (t >= 1) ea.b else if (u <= 0) eb.a else if (u >= 1) eb.b else V2.lerp(ea.a, ea.b, t);
                try cuts[i].append(a, .{ .t = t, .p = p });
                try cuts[j].append(a, .{ .t = u, .p = p });
            } else {
                // Collinear overlap: endpoints of the overlap.
                const ab = ea.b.sub(ea.a);
                const l2 = ab.dot(ab);
                const tb0 = eb.a.sub(ea.a).dot(ab) / l2;
                const tb1 = eb.b.sub(ea.a).dot(ab) / l2;
                const p_lo = if (ta[0] <= 0) ea.a else if (tb0 < tb1) eb.a else eb.b;
                const p_hi = if (ta[1] >= 1) ea.b else if (tb0 > tb1) eb.a else eb.b;
                try cuts[i].append(a, .{ .t = ta[0], .p = p_lo });
                try cuts[i].append(a, .{ .t = ta[1], .p = p_hi });
                try cuts[j].append(a, .{ .t = tb[0], .p = p_lo });
                try cuts[j].append(a, .{ .t = tb[1], .p = p_hi });
            }
        }
    }

    // Build point sequences per edge and collect vertices.
    var verts: std.ArrayList(V2) = .empty;
    var seq_start = try a.alloc(usize, ne + 1);
    var seq: std.ArrayList(u32) = .empty; // vertex occurrence indices into `verts`
    for (0..ne) |k| {
        seq_start[k] = seq.items.len;
        const e = edges.items[k];
        try verts.append(a, e.a);
        try seq.append(a, @intCast(verts.items.len - 1));
        sort.stable(Cut, cuts[k].items, {}, lessCut);
        for (cuts[k].items) |c| {
            if (c.t <= 0 or c.t >= 1) continue;
            try verts.append(a, c.p);
            try seq.append(a, @intCast(verts.items.len - 1));
        }
        try verts.append(a, e.b);
        try seq.append(a, @intCast(verts.items.len - 1));
    }
    seq_start[ne] = seq.items.len;

    const vid = try snapVertices(a, verts.items);
    const nv = blk: {
        var m: u32 = 0;
        for (vid) |v| m = @max(m, v + 1);
        break :blk m;
    };
    const pos = try a.alloc(V2, nv);
    {
        const cnt = try a.alloc(f64, nv);
        @memset(cnt, 0);
        @memset(pos, V2.init(0, 0));
        for (verts.items, 0..) |p, k| {
            const id = vid[k];
            // Use the first occurrence as the representative (deterministic and exact for inputs).
            if (cnt[id] == 0) pos[id] = p;
            cnt[id] += 1;
        }
    }

    // Sub-edges.
    var subs: std.ArrayList(Sub) = .empty;
    for (0..ne) |k| {
        const s0 = seq_start[k];
        const s1 = seq_start[k + 1];
        var q = s0;
        while (q + 1 < s1) : (q += 1) {
            const f = vid[seq.items[q]];
            const t = vid[seq.items[q + 1]];
            if (f == t) continue;
            try subs.append(a, .{ .from = f, .to = t, .src = src.items[k] });
        }
    }

    // Classification.
    for (subs.items) |*s| {
        const pa = pos[s.from];
        const pb = pos[s.to];
        const m = V2.mid(pa, pb);
        const other = if (s.src == 0) B else A;
        const loc = geom.locate(m, other, snap);
        const d = pb.sub(pa);
        switch (loc) {
            .inside => s.keep = switch (op) {
                .intersect => true,
                .unite => false,
                .diff => s.src == 1, // B inside A is kept (reversed below); A inside B dropped
            },
            .outside => s.keep = switch (op) {
                .intersect => false,
                .unite => true,
                .diff => s.src == 0,
            },
            .boundary => {
                if (s.src == 1) {
                    s.keep = false;
                } else {
                    const same = sameDirection(other, m, d);
                    s.keep = switch (op) {
                        .intersect, .unite => same,
                        .diff => !same,
                    };
                }
            },
        }
    }
    // In a difference, B edges that are inside A form the cut; they are traversed reversed.
    // (A inside B is dropped; B inside A is kept.) Fix the 'inside' keep decision per operand:
    if (op == .diff) {
        for (subs.items) |*s| {
            if (s.src == 1) {
                const m = V2.mid(pos[s.from], pos[s.to]);
                const loc = geom.locate(m, A, snap);
                s.keep = (loc == .inside);
                if (s.keep) std.mem.swap(u32, &s.from, &s.to);
            }
        }
    }

    // Deduplicate identical directed edges.
    var out_edges: std.ArrayList(Sub) = .empty;
    for (subs.items) |s| {
        if (!s.keep) continue;
        var dup = false;
        for (out_edges.items) |o| if (o.from == s.from and o.to == s.to) {
            dup = true;
            break;
        };
        if (!dup) try out_edges.append(a, s);
    }
    // Cancel exact reversed pairs (zero-width slivers).
    var alive = try a.alloc(bool, out_edges.items.len);
    @memset(alive, true);
    for (out_edges.items, 0..) |e1, x| {
        if (!alive[x]) continue;
        var y = x + 1;
        while (y < out_edges.items.len) : (y += 1) {
            const e2 = out_edges.items[y];
            if (alive[y] and e1.from == e2.to and e1.to == e2.from) {
                alive[x] = false;
                alive[y] = false;
                break;
            }
        }
    }
    var final: std.ArrayList(Sub) = .empty;
    for (out_edges.items, 0..) |e, x| if (alive[x]) try final.append(a, e);

    return chain(a, pos, final.items);
}

fn cloneLoops(a: Allocator, l: []const []const V2) Allocator.Error![][]V2 {
    const out = try a.alloc([]V2, l.len);
    for (l, 0..) |x, i| out[i] = try a.dupe(V2, x);
    return out;
}

fn appendLoopEdges(a: Allocator, edges: *std.ArrayList(Edge), src: *std.ArrayList(u8), l: []const V2, s: u8) Allocator.Error!void {
    if (l.len < 3) return;
    for (l, 0..) |p, i| {
        const q = l[(i + 1) % l.len];
        if (V2.eql(p, q, 1e-12)) continue;
        try edges.append(a, .{ .a = p, .b = q });
        try src.append(a, s);
    }
}

fn segBox(e: Edge) geom.Box {
    return .{ .x0 = @min(e.a.x, e.b.x), .y0 = @min(e.a.y, e.b.y), .x1 = @max(e.a.x, e.b.x), .y1 = @max(e.a.y, e.b.y) };
}

/// Direction agreement of a sub-edge (direction `d`, midpoint `m`) with the operand edge it lies on.
fn sameDirection(loops: []const []const V2, m: V2, d: V2) bool {
    var best: f64 = std.math.inf(f64);
    var dir = V2.init(0, 0);
    for (loops) |lp| {
        for (lp, 0..) |p, i| {
            const q = lp[(i + 1) % lp.len];
            const dd = geom.distPointSeg(m, p, q);
            if (dd < best) {
                best = dd;
                dir = q.sub(p);
            }
        }
    }
    return dir.dot(d) > 0;
}

/// Cluster nearby vertices. Returns the cluster id per input vertex (ids are dense, in first-seen order).
fn snapVertices(a: Allocator, v: []const V2) Allocator.Error![]u32 {
    const n = v.len;
    const order = try a.alloc(u32, n);
    for (order, 0..) |*o, i| o.* = @intCast(i);
    const Ctx = struct {
        v: []const V2,
        fn lt(c: @This(), x: u32, y: u32) bool {
            const p = c.v[x];
            const q = c.v[y];
            if (p.x != q.x) return p.x < q.x;
            if (p.y != q.y) return p.y < q.y;
            return x < y;
        }
    };
    sort.stable(u32, order, Ctx{ .v = v }, Ctx.lt);
    // union-find over x-sorted sweep
    const parent = try a.alloc(u32, n);
    for (parent, 0..) |*p, i| p.* = @intCast(i);
    const find = struct {
        fn f(par: []u32, x: u32) u32 {
            var r = x;
            while (par[r] != r) r = par[r];
            var c = x;
            while (par[c] != r) {
                const nx = par[c];
                par[c] = r;
                c = nx;
            }
            return r;
        }
    }.f;
    var i: usize = 0;
    while (i < n) : (i += 1) {
        const pi = v[order[i]];
        var j = i + 1;
        while (j < n and v[order[j]].x - pi.x <= snap) : (j += 1) {
            const pj = v[order[j]];
            if (@abs(pj.y - pi.y) <= snap) {
                const ra = find(parent, order[i]);
                const rb = find(parent, order[j]);
                if (ra != rb) {
                    if (ra < rb) parent[rb] = ra else parent[ra] = rb;
                }
            }
        }
    }
    // dense ids in first-seen order of the root
    const ids = try a.alloc(u32, n);
    const root_id = try a.alloc(u32, n);
    @memset(root_id, std.math.maxInt(u32));
    var next: u32 = 0;
    for (0..n) |k| {
        const r = find(parent, @intCast(k));
        if (root_id[r] == std.math.maxInt(u32)) {
            root_id[r] = next;
            next += 1;
        }
        ids[k] = root_id[r];
    }
    return ids;
}

fn chain(a: Allocator, pos: []const V2, edges: []Sub) Allocator.Error![][]V2 {
    const ne = edges.len;
    const used = try a.alloc(bool, ne);
    @memset(used, false);
    // out-edge index by vertex (adjacency via sort)
    const by_from = try a.alloc(u32, ne);
    for (by_from, 0..) |*b, i| b.* = @intCast(i);
    const Ctx = struct {
        e: []const Sub,
        fn lt(c: @This(), x: u32, y: u32) bool {
            if (c.e[x].from != c.e[y].from) return c.e[x].from < c.e[y].from;
            return x < y;
        }
    };
    sort.stable(u32, by_from, Ctx{ .e = edges }, Ctx.lt);
    const first = try a.alloc(u32, pos.len + 1);
    @memset(first, 0);
    for (edges) |e| first[e.from + 1] += 1;
    for (1..first.len) |k| first[k] += first[k - 1];

    var loops: std.ArrayList([]V2) = .empty;
    var si: usize = 0;
    while (si < ne) : (si += 1) {
        if (used[si]) continue;
        var pts: std.ArrayList(V2) = .empty;
        const start_v = edges[si].from;
        var cur: usize = si;
        var closed = false;
        var guard: usize = 0;
        while (guard <= ne) : (guard += 1) {
            used[cur] = true;
            try pts.append(a, pos[edges[cur].from]);
            const v = edges[cur].to;
            if (v == start_v) {
                closed = true;
                break;
            }
            // choose next
            const din = pos[v].sub(pos[edges[cur].from]);
            const back = din.scale(-1);
            var best: ?usize = null;
            var best_ang: f64 = std.math.inf(f64);
            var k = first[v];
            while (k < first[v + 1]) : (k += 1) {
                const ei = by_from[k];
                if (used[ei]) continue;
                const d = pos[edges[ei].to].sub(pos[v]);
                var ang = -std.math.atan2(back.cross(d), back.dot(d));
                if (ang <= 1e-12) ang += 2.0 * std.math.pi;
                if (ang < best_ang) {
                    best_ang = ang;
                    best = ei;
                }
            }
            if (best) |b| cur = b else break;
        }
        if (!closed) continue;
        if (pts.items.len < 3) continue;
        const cleaned = try cleanLoop(a, pts.items);
        if (cleaned.len < 3) continue;
        if (@abs(geom.signedAreaV(cleaned)) < 1e-10) continue;
        try loops.append(a, cleaned);
    }
    return loops.items;
}

/// Drop duplicate and collinear vertices.
fn cleanLoop(a: Allocator, pts: []const V2) Allocator.Error![]V2 {
    var cur = try a.dupe(V2, pts);
    var changed = true;
    while (changed and cur.len >= 3) {
        changed = false;
        var out: std.ArrayList(V2) = .empty;
        for (cur, 0..) |p, i| {
            const prev = cur[(i + cur.len - 1) % cur.len];
            const next = cur[(i + 1) % cur.len];
            const area2 = (p.x - prev.x) * (next.y - prev.y) - (p.y - prev.y) * (next.x - prev.x);
            const span = prev.dist(next);
            const near_dup = V2.eql(p, prev, snap);
            if (near_dup or (span > 0 and @abs(area2) / span < snap and p.sub(prev).dot(next.sub(p)) > 0)) {
                changed = true;
                continue;
            }
            try out.append(a, p);
        }
        cur = out.items;
    }
    return cur;
}

// ---- tests -----------------------------------------------------------------------------------------

fn rect(a: Allocator, x0: f64, y0: f64, x1: f64, y1: f64) ![]V2 {
    return a.dupe(V2, &.{ V2.init(x0, y0), V2.init(x1, y0), V2.init(x1, y1), V2.init(x0, y1) });
}

fn areaOf(loops: []const []V2) f64 {
    return totalArea(loops);
}

test "overlapping squares" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const A = [_][]const V2{try rect(a, 0, 0, 2, 2)};
    const B = [_][]const V2{try rect(a, 1, 1, 3, 3)};
    try std.testing.expectApproxEqAbs(1.0, areaOf(try boolean(a, &A, &B, .intersect)), 1e-9);
    try std.testing.expectApproxEqAbs(7.0, areaOf(try boolean(a, &A, &B, .unite)), 1e-9);
    try std.testing.expectApproxEqAbs(3.0, areaOf(try boolean(a, &A, &B, .diff)), 1e-9);
}

test "coincident edges: abutting squares" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const A = [_][]const V2{try rect(a, 0, 0, 2, 2)};
    const B = [_][]const V2{try rect(a, 2, 0, 4, 2)};
    try std.testing.expectEqual(@as(usize, 0), (try boolean(a, &A, &B, .intersect)).len);
    const u = try boolean(a, &A, &B, .unite);
    try std.testing.expectEqual(@as(usize, 1), u.len);
    try std.testing.expectApproxEqAbs(8.0, areaOf(u), 1e-9);
    try std.testing.expectApproxEqAbs(4.0, areaOf(try boolean(a, &A, &B, .diff)), 1e-9);
}

test "coincident edges: shared partial edge and identical polygons" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const A = [_][]const V2{try rect(a, 0, 0, 4, 2)};
    const B = [_][]const V2{try rect(a, 1, 0, 3, 2)}; // shares top and bottom edges partially
    try std.testing.expectApproxEqAbs(4.0, areaOf(try boolean(a, &A, &B, .intersect)), 1e-9);
    try std.testing.expectApproxEqAbs(8.0, areaOf(try boolean(a, &A, &B, .unite)), 1e-9);
    const d = try boolean(a, &A, &B, .diff);
    try std.testing.expectApproxEqAbs(4.0, areaOf(d), 1e-9);
    try std.testing.expectEqual(@as(usize, 2), d.len); // two disjoint pieces
    // identical
    try std.testing.expectApproxEqAbs(8.0, areaOf(try boolean(a, &A, &A, .intersect)), 1e-9);
    try std.testing.expectApproxEqAbs(8.0, areaOf(try boolean(a, &A, &A, .unite)), 1e-9);
    try std.testing.expectEqual(@as(usize, 0), (try boolean(a, &A, &A, .diff)).len);
}

test "hole creation and touching vertex" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const A = [_][]const V2{try rect(a, 0, 0, 10, 10)};
    const B = [_][]const V2{try rect(a, 4, 4, 6, 6)};
    const d = try boolean(a, &A, &B, .diff);
    try std.testing.expectEqual(@as(usize, 2), d.len);
    try std.testing.expectApproxEqAbs(96.0, areaOf(d), 1e-9);
    // diagonal-touching corner
    const C = [_][]const V2{try rect(a, 10, 10, 12, 12)};
    try std.testing.expectEqual(@as(usize, 0), (try boolean(a, &A, &C, .intersect)).len);
    try std.testing.expectApproxEqAbs(100.0, areaOf(try boolean(a, &A, &C, .diff)), 1e-9);
}

test "concave subject" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    // L shape area 3
    const L = [_][]const V2{try a.dupe(V2, &.{ V2.init(0, 0), V2.init(2, 0), V2.init(2, 1), V2.init(1, 1), V2.init(1, 2), V2.init(0, 2) })};
    const R = [_][]const V2{try rect(a, 0.5, 0.5, 1.5, 1.5)};
    try std.testing.expectApproxEqAbs(0.75, areaOf(try boolean(a, &L, &R, .intersect)), 1e-9);
    try std.testing.expectApproxEqAbs(3.25, areaOf(try boolean(a, &L, &R, .unite)), 1e-9);
    try std.testing.expectApproxEqAbs(2.25, areaOf(try boolean(a, &L, &R, .diff)), 1e-9);
}

test "circle flattened vs rect" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const circ = [_]geom.Pt{ .{ .x = 1, .y = 0, .b = 1 }, .{ .x = -1, .y = 0, .b = 1 } };
    const c = try fromRegion(a, &.{&circ}, 0.0005);
    const R = [_][]const V2{try rect(a, 0, -2, 2, 2)};
    const inter = try boolean(a, c, &R, .intersect);
    try std.testing.expectApproxEqAbs(std.math.pi / 2.0, areaOf(inter), 2e-3);
}

test "near-coincident sliver does not explode" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const A = [_][]const V2{try rect(a, 0, 0, 2, 2)};
    const B = [_][]const V2{try rect(a, 2.00000001, 0, 4, 2)};
    const u = try boolean(a, &A, &B, .unite);
    try std.testing.expectApproxEqAbs(8.0, areaOf(u), 1e-6);
}
