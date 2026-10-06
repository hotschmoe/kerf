//! Validation (SPEC 9): geometric and construction-practice diagnostics on the compiled scene.
//! Messages are written for an LLM: what is wrong, measured numbers, the concrete fix.

const std = @import("std");
const json = @import("json.zig");
const geom = @import("geom.zig");
const clip = @import("clip.zig");
const model = @import("model.zig");
const scene_mod = @import("scene.zig");
const units = @import("units.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Scene = scene_mod.Scene;

const flat_tol = geom.flat_tol;

const Pf = struct {
    prism: model.Prism,
    flat: []const []const V2,
    box: geom.Box,
    comp: *const scene_mod.Comp,
};

const ftin = units.ftin;

fn boxText(a: Allocator, b: geom.Box) []const u8 {
    return std.fmt.allocPrint(a, "x {s}..{s}, y {s}..{s}", .{ ftin(a, b.x0), ftin(a, b.x1), ftin(a, b.y0), ftin(a, b.y1) }) catch "?";
}

fn segDist(p: V2, q: V2, r: V2, s: V2) f64 {
    var ta: [2]f64 = undefined;
    var tb: [2]f64 = undefined;
    if (geom.segSeg(p, q, r, s, &ta, &tb) > 0) return 0;
    return @min(@min(geom.distPointSeg(p, r, s), geom.distPointSeg(q, r, s)), @min(geom.distPointSeg(r, p, q), geom.distPointSeg(s, p, q)));
}

/// Minimum distance between two regions (0 when they overlap or touch).
fn regionDist(a: []const []const V2, b: []const []const V2, limit: f64) f64 {
    var best: f64 = std.math.inf(f64);
    for (a) |la| for (b) |lb| {
        for (la, 0..) |p, i| {
            const q = la[(i + 1) % la.len];
            const bx0 = @min(p.x, q.x) - limit;
            const bx1 = @max(p.x, q.x) + limit;
            const by0 = @min(p.y, q.y) - limit;
            const by1 = @max(p.y, q.y) + limit;
            for (lb, 0..) |r, k| {
                const s = lb[(k + 1) % lb.len];
                if (@min(r.x, s.x) > bx1 or @max(r.x, s.x) < bx0 or @min(r.y, s.y) > by1 or @max(r.y, s.y) < by0) continue;
                const d = segDist(p, q, r, s);
                if (d < best) {
                    best = d;
                    if (best == 0) return 0;
                }
            }
        }
    };
    // containment (no boundary contact)
    if (a.len > 0 and a[0].len > 0 and b.len > 0 and geom.locate(a[0][0], b, 0) == .inside) return 0;
    if (b.len > 0 and b[0].len > 0 and a.len > 0 and geom.locate(b[0][0], a, 0) == .inside) return 0;
    return best;
}

fn zGap(a: model.Prism, b: model.Prism) f64 {
    return @max(0, @max(a.z0, b.z0) - @min(a.z1, b.z1));
}

fn isMasonry(m: []const u8) bool {
    const eq = std.mem.eql;
    return eq(u8, m, "concrete") or eq(u8, m, "grout") or eq(u8, m, "cmu") or eq(u8, m, "mortar");
}

fn isUntreatedWood(m: []const u8) bool {
    return std.mem.eql(u8, m, "wood") or std.mem.eql(u8, m, "wood_engineered") or std.mem.eql(u8, m, "wood_board");
}

fn isFill(m: []const u8) bool {
    const eq = std.mem.eql;
    return eq(u8, m, "earth") or eq(u8, m, "gravel") or eq(u8, m, "sand") or eq(u8, m, "compacted_fill");
}

pub fn run(a: Allocator, scene: *Scene, doc: json.Value, diags: *model.Diags) Allocator.Error!void {
    var items: std.ArrayList(Pf) = .empty;
    for (scene.comps) |*c| {
        if (c.state != .ok) continue;
        for (c.world) |p| {
            if (p.kind == .ghost) continue;
            const f = try clip.fromRegion(a, p.loops, flat_tol);
            try items.append(a, .{ .prism = p, .flat = f, .box = clip.loopsBox(f), .comp = c });
        }
    }
    try overlaps(a, items.items, diags);
    try floating(scene, items.items, diags);
    try untreated(items.items, diags);
    try cover(a, scene, diags);
    try nearMiss(a, scene, diags);
    try shortSlope(a, scene, items.items, doc, diags);
    // infos
    for (scene.comps) |c| {
        if (c.state == .ok and std.mem.eql(u8, c.ty.name, "solid")) {
            diags.add(.info, "I_SOLID_USED", c.id, null, "{s} uses the `solid` escape hatch (material {s}); a reviewer should confirm no typed component fits.", .{ c.id, if (c.world.len > 0) c.world[0].material else "?" });
        }
    }
    try unverifiedCount(a, doc, diags);
}

fn overlaps(a: Allocator, items: []const Pf, diags: *model.Diags) Allocator.Error!void {
    var reported: std.ArrayList([2]u32) = .empty;
    for (items, 0..) |p, i| {
        if (p.prism.embedded or p.prism.kind != .body) continue;
        if (std.mem.eql(u8, p.comp.ty.name, "connector")) continue;
        for (items[i + 1 ..]) |q| {
            if (q.prism.embedded or q.prism.kind != .body) continue;
            if (std.mem.eql(u8, q.comp.ty.name, "connector")) continue;
            if (p.comp.index == q.comp.index and p.prism.instance == q.prism.instance) continue;
            if (zGap(p.prism, q.prism) > 0 or (@min(p.prism.z1, q.prism.z1) - @max(p.prism.z0, q.prism.z0)) < 1e-6) continue;
            if (!p.box.overlaps(q.box, 0)) continue;
            // fills vs membranes (membranes are lines, already excluded); fills overlap fills is an overlap
            const inter = try clip.boolean(a, p.flat, q.flat, .intersect);
            const area = clip.totalArea(inter);
            if (area <= 0.01) continue;
            const key = [2]u32{ @min(p.comp.index, q.comp.index), @max(p.comp.index, q.comp.index) };
            var seen = false;
            for (reported.items) |r| if (r[0] == key[0] and r[1] == key[1]) {
                seen = true;
            };
            if (seen) continue;
            try reported.append(a, key);
            const b = clip.loopsBox(inter);
            diags.addFix(.warning, "W_OVERLAP", p.comp.id, null, "'{s}' and '{s}' overlap by {d:.2} sq in ({s}) within z {s}..{s}", .{
                p.comp.id,                             q.comp.id,                             area, boxText(a, b),
                ftin(a, @max(p.prism.z0, q.prism.z0)), ftin(a, @min(p.prism.z1, q.prism.z1)),
            }, "move or resize one of them so they only touch, or mark the inner one embedded:true if it is reinforcement or hardware");
        }
    }
}

fn floating(scene: *Scene, items: []const Pf, diags: *model.Diags) Allocator.Error!void {
    const gap = 1.0 / 32.0;
    var n_comps: usize = 0;
    for (scene.comps) |c| if (c.state == .ok and !c.dashed) {
        n_comps += 1;
    };
    if (n_comps < 2) return;
    for (scene.comps) |*c| {
        if (c.state != .ok or c.dashed) continue; // `shown: dashed` ("where occurs") graphics are exempt
        var touches = false;
        outer: for (items) |p| {
            if (p.comp != c) continue;
            for (items) |q| {
                if (q.comp == c) continue;
                if (zGap(p.prism, q.prism) > gap) continue;
                if (!p.box.expand(gap).overlaps(q.box, 0)) continue;
                if (regionDist(p.flat, q.flat, gap) <= gap) {
                    touches = true;
                    break :outer;
                }
            }
        }
        if (!touches) {
            diags.addFix(.warning, "W_FLOATING", c.id, null, "component '{s}' touches nothing (gap > 1/32\" to every other component, in plan and in z); it may be mis-placed", .{c.id}, "check its `at` anchor/offset, or `z` range if it should bear on another member");
        }
    }
}

fn untreated(items: []const Pf, diags: *model.Diags) Allocator.Error!void {
    var reported: std.ArrayList([2]u32) = .empty;
    const gap = 1.0 / 32.0;
    for (items) |p| {
        if (!isUntreatedWood(p.prism.material)) continue;
        for (items) |q| {
            if (!isMasonry(q.prism.material)) continue;
            if (zGap(p.prism, q.prism) > gap) continue;
            if (!p.box.expand(gap).overlaps(q.box, 0)) continue;
            if (regionDist(p.flat, q.flat, gap) > gap) continue;
            const key = [2]u32{ p.comp.index, q.comp.index };
            var seen = false;
            for (reported.items) |r| if (r[0] == key[0] and r[1] == key[1]) {
                seen = true;
            };
            if (seen) continue;
            try reported.append(std.heap.page_allocator, key);
            diags.addFix(.warning, "W_UNTREATED_CONTACT", p.comp.id, null, "untreated wood '{s}' touches {s} '{s}' (IRC R317.1 requires preservative-treated wood or a barrier where wood contacts concrete or masonry)", .{ p.comp.id, q.prism.material, q.comp.id }, "set \"treated\": true on the wood member (or add a sill seal/barrier and note it)");
        }
    }
}

// ---- cover -------------------------------------------------------------------------------------------------

const ZoneBox = struct { name: []const u8, box: geom.Box };
const Edge = struct { a: V2, b: V2, class: u8 }; // 0 bottom 1 top 2 sides

fn hostEdges(a: Allocator, outline: []const geom.Pt, xf: geom.Xf) Allocator.Error![]Edge {
    const world = try xf.applyLoop(a, outline);
    const flat = try geom.flattenLoop(a, world, flat_tol);
    const ccw = geom.signedAreaV(flat) >= 0;
    var out: std.ArrayList(Edge) = .empty;
    for (flat, 0..) |p, i| {
        const q = flat[(i + 1) % flat.len];
        const d = q.sub(p);
        const l = d.len();
        if (l < 1e-9) continue;
        const nrm = if (ccw) V2.init(d.y / l, -d.x / l) else V2.init(-d.y / l, d.x / l);
        const class: u8 = if (nrm.y < -0.5) 0 else if (nrm.y > 0.5) 1 else 2;
        try out.append(a, .{ .a = p, .b = q, .class = class });
    }
    return out.items;
}

fn coverFor(h: model.HostInfo, zones_world: []const ZoneBox, c: V2) model.Cover {
    for (h.part_cover) |pc| {
        for (zones_world) |z| if (std.mem.eql(u8, z.name, pc.part) and z.box.contains(c)) return pc.cover;
    }
    return h.cover;
}

/// One authored leg of a path bar (straight run between bends) or one bend, for naming the failing segment in W_COVER.
const Piece = struct {
    /// flattened sub-segments of this piece (world)
    segs: []const [2]V2,
    /// 1-based leg number of a straight leg; for a bend, the number of the leg it follows
    leg: usize,
    is_bend: bool,
    /// leg ends extended to the authored vertices (legs only)
    p0: V2 = V2.init(0, 0),
    p1: V2 = V2.init(0, 0),
};

fn lineIntersect(p: V2, d: V2, q: V2, e: V2) ?V2 {
    const den = d.cross(e);
    if (@abs(den) < 1e-9) return null;
    const t = q.sub(p).cross(e) / den;
    return p.add(d.scale(t));
}

/// Split a bar centreline (bulge polyline: straight legs have bulge 0, fillets are arcs) into pieces.
fn barPieces(a: Allocator, cl: []const geom.Pt) Allocator.Error![]Piece {
    var out: std.ArrayList(Piece) = .empty;
    var legs: usize = 0;
    for (0..cl.len - 1) |i| {
        var fl: std.ArrayList(V2) = .empty;
        try fl.append(a, cl[i].v());
        try geom.flattenSegInto(&fl, a, cl[i].v(), cl[i + 1].v(), cl[i].b, flat_tol);
        const ss = try a.alloc([2]V2, fl.items.len - 1);
        for (0..ss.len) |k| ss[k] = .{ fl.items[k], fl.items[k + 1] };
        const bend = cl[i].b != 0;
        if (!bend) legs += 1;
        try out.append(a, .{ .segs = ss, .leg = legs, .is_bend = bend, .p0 = cl[i].v(), .p1 = cl[i + 1].v() });
    }
    // extend each leg to the authored vertex where a bend sits at its end
    for (out.items, 0..) |*pc, i| {
        if (pc.is_bend or pc.p0.dist(pc.p1) < 1e-6) continue;
        const d = pc.p1.sub(pc.p0).norm();
        if (i > 0 and out.items[i - 1].is_bend and i >= 2) {
            const prev = out.items[i - 2];
            if (!prev.is_bend and prev.p0.dist(prev.p1) > 1e-6) if (lineIntersect(pc.p0, d, prev.p0, prev.p1.sub(prev.p0).norm())) |x| {
                pc.p0 = x;
            };
        }
        if (i + 2 < out.items.len and out.items[i + 1].is_bend) {
            const next = out.items[i + 2];
            if (!next.is_bend and next.p0.dist(next.p1) > 1e-6) if (lineIntersect(pc.p0, d, next.p0, next.p1.sub(next.p0).norm())) |x| {
                pc.p1 = x;
            };
        }
    }
    return out.items;
}

fn pieceText(a: Allocator, pieces: []const Piece, k: usize) []const u8 {
    const pc = pieces[k];
    var nlegs: usize = 0;
    for (pieces) |q| if (!q.is_bend) {
        nlegs += 1;
    };
    if (pc.is_bend) return std.fmt.allocPrint(a, "the bend after segment {d} of {d} (near x {s}, y {s})", .{ pc.leg, nlegs, ftin(a, pc.segs[pc.segs.len / 2][0].x), ftin(a, pc.segs[pc.segs.len / 2][0].y) }) catch "?";
    return std.fmt.allocPrint(a, "segment {d} of {d} (x {s}, y {s} to x {s}, y {s})", .{ pc.leg, nlegs, ftin(a, pc.p0.x), ftin(a, pc.p0.y), ftin(a, pc.p1.x), ftin(a, pc.p1.y) }) catch "?";
}

fn cover(a: Allocator, scene: *Scene, diags: *model.Diags) Allocator.Error!void {
    for (scene.comps) |*bar| {
        if (bar.state != .ok or !std.mem.eql(u8, bar.ty.name, "rebar")) continue;
        const d = bar.built.bar_d;
        const r = d / 2;
        for (bar.world) |bp| {
            // sample: along_z -> centre; path -> segment midpoints and direction
            var centre: V2 = undefined;
            var dir: ?V2 = null;
            var segs: []const [2]V2 = &.{};
            var pieces: []const Piece = &.{};
            if (bp.centerline.len >= 2) {
                pieces = try barPieces(a, bp.centerline);
                const flat = try geom.flattenPolyline(a, bp.centerline, false, flat_tol);
                const ss = try a.alloc([2]V2, flat.len - 1);
                var longest: usize = 0;
                var ll: f64 = -1;
                for (0..flat.len - 1) |i| {
                    ss[i] = .{ flat[i], flat[i + 1] };
                    const l = flat[i].dist(flat[i + 1]);
                    if (l > ll) {
                        ll = l;
                        longest = i;
                    }
                }
                segs = ss;
                centre = V2.mid(ss[longest][0], ss[longest][1]);
                dir = ss[longest][1].sub(ss[longest][0]).norm();
            } else {
                const bx = geom.loopBox(bp.loops[0]);
                centre = bx.center();
            }
            // host: first concrete/cmu component whose outline contains the bar centre
            var found = false;
            for (scene.comps) |*h| {
                if (h.state != .ok or h.built.host == null) continue;
                if (bp.z0 < h.zs[0][0] - 1e-6 and bp.z1 < h.zs[0][0] - 1e-6) continue;
                for (h.xfs, 0..) |xf, hi| {
                    const hz = h.zs[hi];
                    if (bp.z1 < hz[0] - 1e-6 or bp.z0 > hz[1] + 1e-6) continue;
                    const host = h.built.host.?;
                    const wl = try xf.applyLoop(a, host.outline);
                    const fl = try geom.flattenLoop(a, wl, flat_tol);
                    if (!geom.pointInLoopEO(centre, fl) and geom.distPointSeg(centre, fl[0], fl[0]) > 0) {
                        // boundary test below
                        var onb = false;
                        for (fl, 0..) |p, i| if (geom.distPointSeg(centre, p, fl[(i + 1) % fl.len]) < 1e-6) {
                            onb = true;
                        };
                        if (!onb) continue;
                    }
                    found = true;
                    const edges = try hostEdges(a, host.outline, xf);
                    // zones in world
                    var zw: std.ArrayList(ZoneBox) = .empty;
                    for (h.built.zones) |z| try zw.append(a, .{ .name = z.name, .box = @import("builders.zig").worldBox(xf, z.box) });
                    const req = coverFor(host, zw.items, centre);
                    var min_clear = [3]f64{ std.math.inf(f64), std.math.inf(f64), std.math.inf(f64) };
                    var min_edge: [3]?Edge = .{ null, null, null };
                    var min_piece: [3]usize = .{ 0, 0, 0 };
                    for (edges) |e| {
                        var dist: f64 = undefined;
                        if (segs.len > 0) {
                            // skip host faces roughly perpendicular to the bar (its ends)
                            const ed = e.b.sub(e.a).norm();
                            if (@abs(dir.?.dot(ed.perp())) > 0.7) continue;
                            dist = std.math.inf(f64);
                            var best_key = std.math.inf(f64);
                            var best_pi: usize = 0;
                            for (pieces, 0..) |pc, pi| {
                                var pd = std.math.inf(f64);
                                for (pc.segs) |s| pd = @min(pd, segDist(s[0], s[1], e.a, e.b));
                                // ties (a bend touches the extreme its legs already reach) are blamed on the straight leg
                                const key = pd + (if (pc.is_bend) @as(f64, 1e-4) else 0);
                                if (key < best_key - 1e-12) {
                                    best_key = key;
                                    best_pi = pi;
                                }
                                dist = @min(dist, pd);
                            }
                            if (dist - r < min_clear[e.class]) min_piece[e.class] = best_pi;
                        } else dist = geom.distPointSeg(centre, e.a, e.b);
                        const clear = dist - r;
                        if (clear < min_clear[e.class]) {
                            min_clear[e.class] = clear;
                            min_edge[e.class] = e;
                        }
                    }
                    const reqs = [3]f64{ req.bottom, req.top, req.sides };
                    const names = [3][]const u8{ "bottom", "top", "sides" };
                    for (0..3) |k| {
                        if (min_clear[k] < reqs[k] - 1e-3) {
                            const fix = "move the bar inward (increase place.cover / side_cover or the offset) or lower the host's cover requirement";
                            if (pieces.len > 0) {
                                diags.addFix(.warning, "W_COVER", bar.id, null, "clear cover from {s}, {s}, to the {s} of '{s}' is {s} < required {s}", .{
                                    bar.built.info,
                                    pieceText(a, pieces, min_piece[k]),
                                    names[k],
                                    h.id,
                                    ftin(a, min_clear[k]),
                                    ftin(a, reqs[k]),
                                }, fix);
                            } else {
                                diags.addFix(.warning, "W_COVER", bar.id, null, "clear cover from {s} bar to the {s} of '{s}' is {s} < required {s}; bar center at x {s}, y {s}", .{
                                    bar.built.info,
                                    names[k],
                                    h.id,
                                    ftin(a, min_clear[k]),
                                    ftin(a, reqs[k]),
                                    ftin(a, centre.x),
                                    ftin(a, centre.y),
                                }, fix);
                            }
                        }
                    }
                    break;
                }
                if (found) break;
            }
        }
    }
}

fn unverifiedCount(a: Allocator, doc: json.Value, diags: *model.Diags) Allocator.Error!void {
    var n: usize = 0;
    var ids: std.ArrayList([]const u8) = .empty;
    if (doc.get("views")) |vs| if (vs.arr()) |va| for (va) |v| {
        const vid = if (v.get("id")) |x| (x.str() orelse "?") else "?";
        if (v.get("annotations")) |an| if (an.arr()) |aa| for (aa) |x| {
            var mine = false;
            if (x.get("cite")) |cv| if (cv.arr()) |ca| for (ca) |c| {
                const st = if (c.get("status")) |s| (s.str() orelse "suggested") else "suggested";
                if (!std.mem.eql(u8, st, "verified")) {
                    n += 1;
                    mine = true;
                }
            };
            if (mine) try ids.append(a, try std.fmt.allocPrint(a, "{s}/{s}", .{ vid, if (x.get("id")) |i| (i.str() orelse "?") else "?" }));
        };
    };
    if (n > 0) diags.add(.info, "I_UNVERIFIED_CITE", null, null, "{d} citation(s) await designer verification (notes: {s}). Citations print with a trailing * until verified.", .{ n, scene_mod.joinIds(a, ids.items) });
}

// ---- near miss (SPEC 17) ---------------------------------------------------------------------------------------

const CompBox = struct { c: *const scene_mod.Comp, box: geom.Box, z0: f64, z1: f64 };

fn nearMiss(a: Allocator, scene: *Scene, diags: *model.Diags) Allocator.Error!void {
    var boxes: std.ArrayList(CompBox) = .empty;
    for (scene.comps) |*c| {
        if (c.state != .ok) continue;
        if (std.mem.eql(u8, c.ty.name, "fill")) continue;
        if (!axisAligned(c) or !std.mem.eql(u8, c.ty.name, "lumber")) continue;
        var b = geom.Box{};
        var z0: f64 = std.math.inf(f64);
        var z1: f64 = -std.math.inf(f64);
        for (c.world) |p| {
            if (p.kind == .ghost) continue;
            for (p.loops) |l| b.addBox(geom.loopBox(l));
            z0 = @min(z0, p.z0);
            z1 = @max(z1, p.z1);
        }
        if (b.isEmpty()) continue;
        try boxes.append(a, .{ .c = c, .box = b, .z0 = z0, .z1 = z1 });
    }
    const min_gap = 1.0 / 32.0;
    const max_gap = 3.0;
    for (boxes.items, 0..) |A, i| {
        for (boxes.items[i + 1 ..]) |B| {
            if (@min(A.z1, B.z1) - @max(A.z0, B.z0) <= 1e-6) continue;
            inline for (.{ false, true }) |on_x| {
                // gap along `on_x` axis when the boxes overlap on the other axis
                const a0 = if (on_x) A.box.x0 else A.box.y0;
                const a1 = if (on_x) A.box.x1 else A.box.y1;
                const b0 = if (on_x) B.box.x0 else B.box.y0;
                const b1 = if (on_x) B.box.x1 else B.box.y1;
                const o0 = if (on_x) A.box.y0 else A.box.x0;
                const o1 = if (on_x) A.box.y1 else A.box.x1;
                const q0 = if (on_x) B.box.y0 else B.box.x0;
                const q1 = if (on_x) B.box.y1 else B.box.x1;
                const overlap = @min(o1, q1) - @max(o0, q0);
                const gap = @max(a0, b0) - @min(a1, b1);
                if (overlap > 1e-6 and gap >= min_gap - 1e-9 and gap <= max_gap + 1e-9) {
                    // skip when a third component sits in the gap
                    const g0 = @min(a1, b1);
                    const g1 = @max(a0, b0);
                    const lo = @max(o0, q0);
                    const hi = @min(o1, q1);
                    var blocked = false;
                    for (boxes.items) |C| {
                        if (C.c == A.c or C.c == B.c) continue;
                        const c0 = if (on_x) C.box.x0 else C.box.y0;
                        const c1 = if (on_x) C.box.x1 else C.box.y1;
                        const d0 = if (on_x) C.box.y0 else C.box.x0;
                        const d1 = if (on_x) C.box.y1 else C.box.x1;
                        if (@min(c1, g1) - @max(c0, g0) > 1e-6 and @min(d1, hi) - @max(d0, lo) > 1e-6) {
                            blocked = true;
                            break;
                        }
                    }
                    if (!blocked) {
                        // the member that should grow: the one that is longer along the gap axis than across it
                        const a_len = a1 - a0;
                        const b_len = b1 - b0;
                        const a_across = o1 - o0;
                        const b_across = q1 - q0;
                        var grow = A;
                        var other = B;
                        if (b_len / @max(b_across, 1e-9) > a_len / @max(a_across, 1e-9)) {
                            grow = B;
                            other = A;
                        }
                        const grow_len = if (grow.c == A.c) a_len else b_len;
                        const grow_below_other = if (grow.c == A.c) a1 <= b0 else b1 <= a0; // grow member sits at lower coordinates
                        const edge_name: []const u8 = if (on_x) (if (grow_below_other) "left" else "right") else (if (grow_below_other) "bottom" else "top");
                        const anchor: []const u8 = if (on_x) (if (grow_below_other) "middle_left" else "middle_right") else (if (grow_below_other) "bottom_left" else "top_left");
                        diags.addFix(.warning, "W_NEAR_MISS", grow.c.id, null, "'{s}' and '{s}' leave a {s} gap along {s} ({s}..{s}) between facing edges while overlapping along {s}; '{s}' probably should extend to the {s} edge of '{s}'", .{
                            grow.c.id,
                            other.c.id,
                            ftin(a, gap),
                            if (on_x) "x" else "y",
                            ftin(a, g0),
                            ftin(a, g1),
                            if (on_x) "y" else "x",
                            grow.c.id,
                            edge_name,
                            other.c.id,
                        }, std.fmt.allocPrint(a, "set the length of '{s}' to {s}, or use \"until\": \"{s}@{s}\" so it follows '{s}'; ignore this warning if the gap is intended", .{ grow.c.id, ftin(a, grow_len + gap), other.c.id, anchor, other.c.id }) catch "");
                    }
                }
            }
        }
    }
}

// ---- short slope (SPEC 20) -----------------------------------------------------------------------------------------

/// Crop box of the first section view that sets an explicit crop (auto-fit crops contain every member, so they never cut).
fn explicitCrop(doc: json.Value) ?geom.Box {
    const views = (doc.get("views") orelse return null).arr() orelse return null;
    for (views) |v| {
        if (v.get("kind")) |k| if (k.str()) |ks| if (std.mem.eql(u8, ks, "iso")) continue;
        const cv = v.get("crop") orelse continue;
        if (cv == .null) continue;
        const xr = range2(cv.get("x")) orelse continue;
        const yr = range2(cv.get("y")) orelse continue;
        return .{ .x0 = xr[0], .x1 = xr[1], .y0 = yr[0], .y1 = yr[1] };
    }
    return null;
}

fn range2(v: ?json.Value) ?[2]f64 {
    const arr = (v orelse return null).arr() orelse return null;
    if (arr.len != 2) return null;
    return .{ units.parseLength(arr[0]) orelse return null, units.parseLength(arr[1]) orelse return null };
}

/// Angle of the member's long axis (radians) that a sloped panel/membrane can rest on; null when it is not a candidate host.
fn hostAngle(h: *const scene_mod.Comp) ?f64 {
    const eq = std.mem.eql;
    if (eq(u8, h.ty.name, "truss")) return h.angle + h.pitch_angle;
    if (eq(u8, h.ty.name, "lumber") or eq(u8, h.ty.name, "panel")) return h.angle;
    return null;
}

fn sameSlope(a1: f64, a2: f64) bool {
    var d = @mod(a1 - a2, std.math.pi);
    if (d > std.math.pi / 2.0) d = std.math.pi - d;
    return d < std.math.degreesToRadians(0.5);
}

/// Extent of a component's instance-0 prisms (optionally one part only) projected on `u`; null when it has none.
fn projExtent(items: []const Pf, c: *const scene_mod.Comp, part: ?[]const u8, u: V2) ?[2]f64 {
    var lo: f64 = std.math.inf(f64);
    var hi: f64 = -std.math.inf(f64);
    for (items) |it| {
        if (it.comp != c or it.prism.instance != 0) continue;
        if (part) |pn| if (!std.mem.eql(u8, it.prism.part, pn)) continue;
        for (it.flat) |l| for (l) |v| {
            const t = v.x * u.x + v.y * u.y;
            lo = @min(lo, t);
            hi = @max(hi, t);
        };
    }
    if (lo > hi) return null;
    return .{ lo, hi };
}

/// A sloped panel or membrane that rests on a sloped member (same slope within 0.5 degrees, touching) must reach that
/// member's far end, or the crop edge when that comes first. A literal `length` that goes stale when the pitch changes
/// is the usual cause; the fix is `"until": "<member>@<end anchor>"`.
fn shortSlope(a: Allocator, scene: *Scene, items: []const Pf, doc: json.Value, diags: *model.Diags) Allocator.Error!void {
    const crop = explicitCrop(doc);
    const slack = 0.5;
    const touch = 1.0 / 16.0;
    for (scene.comps) |*c| {
        const is_panel = std.mem.eql(u8, c.ty.name, "panel");
        if (c.state != .ok or c.dashed or !(is_panel or std.mem.eql(u8, c.ty.name, "membrane"))) continue;
        var u = V2.init(@cos(c.angle), @sin(c.angle));
        if (@abs(u.y) < @sin(std.math.degreesToRadians(0.5))) continue; // not sloped
        if (u.y < 0) u = V2.init(-u.x, -u.y);
        // the subject's upper end: the vertex with the largest projection on u
        var pmax: f64 = -std.math.inf(f64);
        var vtx = V2.init(0, 0);
        for (items) |it| {
            if (it.comp != c or it.prism.instance != 0) continue;
            for (it.flat) |l| for (l) |v| {
                const t = v.x * u.x + v.y * u.y;
                if (t > pmax) {
                    pmax = t;
                    vtx = v;
                }
            };
        }
        if (pmax == -std.math.inf(f64)) continue;
        // the host this component rests on that ends farthest up the slope
        var best: ?*const scene_mod.Comp = null;
        var best_end: f64 = 0;
        for (scene.comps) |*h| {
            if (h == c or h.state != .ok or h.dashed) continue;
            if (is_panel and std.mem.eql(u8, h.ty.name, "panel")) continue;
            const ha = hostAngle(h) orelse continue;
            if (!sameSlope(ha, c.angle)) continue;
            const part: ?[]const u8 = if (std.mem.eql(u8, h.ty.name, "truss")) "top_chord" else null;
            var touching = false;
            for (items) |sp| {
                if (sp.comp != c or sp.prism.instance != 0) continue;
                for (items) |hp| {
                    if (hp.comp != h or hp.prism.instance != 0) continue;
                    if (part) |pn| if (!std.mem.eql(u8, hp.prism.part, pn)) continue;
                    if (zGap(sp.prism, hp.prism) > touch or !sp.box.expand(touch).overlaps(hp.box, 0)) continue;
                    if (regionDist(sp.flat, hp.flat, touch) <= touch) touching = true;
                }
            }
            if (!touching) continue;
            const ext = projExtent(items, h, part, u) orelse continue;
            if (best == null or ext[1] > best_end) {
                best = h;
                best_end = ext[1];
            }
        }
        const h = best orelse continue;
        // distance the subject may still grow before it leaves the crop
        var crop_room: f64 = std.math.inf(f64);
        if (crop) |cb| {
            if (u.x > 1e-9) crop_room = @min(crop_room, (cb.x1 - vtx.x) / u.x) else if (u.x < -1e-9) crop_room = @min(crop_room, (cb.x0 - vtx.x) / u.x);
            if (u.y > 1e-9) crop_room = @min(crop_room, (cb.y1 - vtx.y) / u.y);
        }
        const gap_host = best_end - pmax;
        const short = @min(gap_host, crop_room);
        if (short <= slack) continue;
        const by_crop = crop_room < gap_host;
        const anchor: []const u8 = if (std.mem.eql(u8, h.ty.name, "truss")) "top_chord_end" else if (u.x >= 0) "top_right" else "top_left";
        const what = if (by_crop) std.fmt.allocPrint(a, "the view crop (where '{s}' continues)", .{h.id}) catch "the crop" else std.fmt.allocPrint(a, "the end of '{s}'", .{h.id}) catch "the member";
        diags.addFix(.warning, "W_SHORT_SLOPE", c.id, null, "'{s}' rests on the sloped '{s}' (same slope) but its upper end stops {s} short of {s}", .{ c.id, h.id, ftin(a, short), what }, std.fmt.allocPrint(a, "replace the literal length of '{s}' (it does not follow the pitch) with \"until\": \"{s}@{s}\" (grows along the slope to the member's end; pair it with \"slope\": \"@{s}\"), or lengthen it by {s}; acknowledge W_SHORT_SLOPE if it should stop there", .{ c.id, h.id, anchor, h.id, ftin(a, short) }) catch "");
    }
}

fn isMember(c: *const scene_mod.Comp) bool {
    return std.mem.eql(u8, c.ty.name, "lumber") or std.mem.eql(u8, c.ty.name, "panel");
}

/// Only boxy members (every prism an axis-aligned rectangle, none embedded) take part in near-miss checks.
fn axisAligned(c: *const scene_mod.Comp) bool {
    for (c.world) |p| {
        if (p.kind == .ghost) continue;
        if (p.embedded or p.loops.len != 1 or p.loops[0].len != 4) return false;
        const l = p.loops[0];
        for (l, 0..) |v, i| {
            const w = l[(i + 1) % 4];
            if (v.b != 0) return false;
            if (@abs(v.x - w.x) > 1e-9 and @abs(v.y - w.y) > 1e-9) return false;
        }
    }
    return true;
}
