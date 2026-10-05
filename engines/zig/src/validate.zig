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

const flat_tol: f64 = 0.005;

const Pf = struct {
    prism: model.Prism,
    flat: []const []const V2,
    box: geom.Box,
    comp: *const scene_mod.Comp,
};

fn ftin(a: Allocator, x: f64) []const u8 {
    return units.fmtFtIn(a, x) catch "?";
}

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
            const f = try @import("clip.zig").fromRegion(a, p.loops, flat_tol);
            try items.append(a, .{ .prism = p, .flat = f, .box = clip.loopsBox(f), .comp = c });
        }
    }
    try overlaps(a, items.items, diags);
    try floating(a, scene, items.items, diags);
    try untreated(a, items.items, diags);
    try cover(a, scene, diags);
    try nearMiss(a, scene, diags);
    // infos
    for (scene.comps) |c| {
        if (c.state == .ok and std.mem.eql(u8, c.ty.name, "solid")) {
            diags.add(.info, "I_SOLID_USED", c.id, null, "component '{s}' uses the 'solid' escape hatch (material {s}); prefer a typed component when one fits so reviewers can read its intent", .{ c.id, if (c.world.len > 0) c.world[0].material else "?" });
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
                p.comp.id,                                  q.comp.id,                                  area,                                              boxText(a, b),
                ftin(a, @max(p.prism.z0, q.prism.z0)), ftin(a, @min(p.prism.z1, q.prism.z1)),
            }, "move or resize one of them so they only touch, or mark the inner one embedded:true if it is reinforcement or hardware");
        }
    }
}

fn floating(a: Allocator, scene: *Scene, items: []const Pf, diags: *model.Diags) Allocator.Error!void {
    _ = a;
    const gap = 1.0 / 32.0;
    var n_comps: usize = 0;
    for (scene.comps) |c| if (c.state == .ok) {
        n_comps += 1;
    };
    if (n_comps < 2) return;
    for (scene.comps) |*c| {
        if (c.state != .ok) continue;
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

fn untreated(a: Allocator, items: []const Pf, diags: *model.Diags) Allocator.Error!void {
    _ = a;
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
            if (bp.centerline.len >= 2) {
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
                    for (edges) |e| {
                        var dist: f64 = undefined;
                        if (segs.len > 0) {
                            // skip host faces roughly perpendicular to the bar (its ends)
                            const ed = e.b.sub(e.a).norm();
                            if (@abs(dir.?.dot(ed.perp())) > 0.7) continue;
                            dist = std.math.inf(f64);
                            for (segs) |s| dist = @min(dist, segDist(s[0], s[1], e.a, e.b));
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
                            diags.addFix(.warning, "W_COVER", bar.id, null, "clear cover from {s} {s} to the {s} of '{s}' is {s} < required {s}; bar center at x {s}, y {s}", .{
                                bar.built.info,
                                "bar",
                                names[k],
                                h.id,
                                ftin(a, min_clear[k]),
                                ftin(a, reqs[k]),
                                ftin(a, centre.x),
                                ftin(a, centre.y),
                            }, "move the bar inward (increase place.cover / side_cover or the offset) or lower the host's cover requirement");
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
    _ = a;
    var n: usize = 0;
    if (doc.get("views")) |vs| if (vs.arr()) |va| for (va) |v| {
        if (v.get("annotations")) |an| if (an.arr()) |aa| for (aa) |x| {
            if (x.get("cite")) |cv| if (cv.arr()) |ca| for (ca) |c| {
                const st = if (c.get("status")) |s| (s.str() orelse "suggested") else "suggested";
                if (!std.mem.eql(u8, st, "verified")) n += 1;
            };
        };
    };
    if (n > 0) diags.add(.info, "I_UNVERIFIED_CITE", null, null, "{d} code citation(s) await designer verification; they print with a trailing * and a footnote until verified", .{n});
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
                    const other_lo = if (grow.c == A.c) b0 else a0;
                    const grow_below_other = if (grow.c == A.c) a1 <= b0 else b1 <= a0; // grow member sits at lower coordinates
                    _ = other_lo;
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
