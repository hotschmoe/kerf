//! Note landing on thin layers (SPEC 21): membranes, flashing, thin panels, connectors and path rebar have almost no
//! area, so a landing taken "inside the visible region" can sit beside the line, at a junction with the neighbors that
//! share it. When the regular label point is not on the member's line, or within a text height of a junction with
//! the neighbors, the landing moves onto the member's own line (its centerline, or the midline of a thin panel), on
//! a stretch the view shows: samples along the line are ranked by their clearance from the neighbors' edges that
//! cross it (junctions), capped at two text heights, then by their distance from the regular label point.

const std = @import("std");
const geom = @import("geom.zig");
const units = @import("units.zig");
const scene_mod = @import("scene.zig");
const section = @import("section.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Box = geom.Box;

/// A panel thinner than this (inches) lands on its midline.
const thin_panel = 0.5;

/// A line the landing may sit on: a polyline of the member's own geometry and the half-width of its stroke band.
const Skeleton = struct {
    pts: []const V2,
    /// Half the width of the stroke / member at `pts`: the band around the line that counts as "on the member".
    half_width: f64,
    /// How far the drawn line sits from the member's own region (the vapor retarder's separation from its host).
    gap: f64 = 0,
    prism: usize,
    /// The line is a stroke drawn over everything (membranes): it need not lie inside the member's visible region.
    stroke: bool = false,
    /// Drawn in plain view (not embedded in, or laid over, its neighbors): landing inside a neighbor's body is wrong.
    exposed: bool,
};

fn polyLen(pts: []const V2) f64 {
    var l: f64 = 0;
    for (pts[1..], 0..) |q, i| l += pts[i].dist(q);
    return l;
}

fn distToPoly(p: V2, pts: []const V2) f64 {
    var d = std.math.inf(f64);
    for (pts[1..], 0..) |q, i| d = @min(d, geom.distPointSeg(p, pts[i], q));
    return d;
}

/// Midline of a thin quadrilateral (panel): joins the midpoints of its two short edges. Null when the loop is no quad.
fn quadMidline(a: Allocator, loop: []const V2) Allocator.Error!?[]const V2 {
    if (loop.len != 4) return null;
    const e0 = loop[0].dist(loop[1]);
    const e1 = loop[1].dist(loop[2]);
    const mid = struct {
        fn m(p: V2, q: V2) V2 {
            return V2.init((p.x + q.x) / 2, (p.y + q.y) / 2);
        }
    }.m;
    if (e0 >= e1) return try a.dupe(V2, &.{ mid(loop[1], loop[2]), mid(loop[3], loop[0]) });
    return try a.dupe(V2, &.{ mid(loop[0], loop[1]), mid(loop[2], loop[3]) });
}

/// The skeleton of a thin prism of component type `ty`, or null when the prism is not thin (normal landing applies).
fn skeletonOf(a: Allocator, sec: *section.Section, i: usize, ty: []const u8, panel_thickness: f64) Allocator.Error!?Skeleton {
    const p = sec.prisms[i];
    const eq = std.mem.eql;
    const line_like = eq(u8, ty, "membrane") or eq(u8, ty, "flashing") or eq(u8, ty, "connector") or eq(u8, ty, "rebar");
    if (line_like) {
        if (p.centerline.len < 2) return null;
        // a membrane is drawn along its `line_pts` (offset by the draw-time gap); everything else along its centerline
        const drawn = if (p.kind == .line and p.line_pts.len >= 2) try sec.linePoints(p) else p.centerline;
        const v = try a.alloc(V2, drawn.len);
        for (drawn, 0..) |q, k| v[k] = q.v();
        const gap = if (p.kind == .line) nearestOnPoly(v, p.centerline[0].v()).dist else 0;
        return .{ .pts = v, .half_width = 0.5 * @max(p.sweep_r * 2, 0.0625), .gap = gap, .prism = i, .stroke = p.kind == .line, .exposed = !(p.embedded or p.face_tie) };
    }
    if (eq(u8, ty, "panel") and panel_thickness < thin_panel) {
        const flat = try sec.flatOf(i);
        if (flat.len == 0) return null;
        const mid = (try quadMidline(a, flat[0])) orelse return null;
        return .{ .pts = mid, .half_width = 0.5 * panel_thickness, .prism = i, .exposed = !p.embedded };
    }
    return null;
}

/// Landing candidates for note targets whose geometry is thin (best first), or null when the target is not thin or
/// no point of its line is shown in the view (the regular landing then decides).
pub fn candidates(
    a: Allocator,
    sec: *section.Section,
    comp: *const scene_mod.Comp,
    inst: ?u32,
    part: ?[]const u8,
    inset: Box,
    text_h: f64,
    prefer: V2,
) Allocator.Error!?[]const V2 {
    const panel_t: f64 = if (comp.node.get("thickness")) |t| (units.parseLength(t) orelse 1) else 1;
    var skels: std.ArrayList(Skeleton) = .empty;
    for (sec.prisms, 0..) |p, i| {
        if (p.comp != comp.index) continue;
        if (inst) |k| if (p.instance != k) continue;
        if (part) |pn| if (!std.mem.eql(u8, p.part, pn)) continue;
        if (sec.cls[i] == .drop) continue;
        if (try skeletonOf(a, sec, i, comp.ty.name, panel_t)) |sk| try skels.append(a, sk) else return null; // every prism must be thin
    }
    if (skels.items.len == 0) return null;

    // the neighbors' outlines (for the junction clearance) and the bodies they cut through the view (landing inside one is wrong)
    var edges: std.ArrayList([2]V2) = .empty;
    var bodies: std.ArrayList([]const []const V2) = .empty;
    for (sec.prisms, 0..) |p, i| {
        if (p.comp == comp.index or sec.cls[i] == .drop or p.kind != .body) continue;
        const flat = try sec.flatOf(i);
        for (flat) |loop| for (loop, 0..) |q, k| try edges.append(a, .{ q, loop[(k + 1) % loop.len] });
        if (sec.cls[i] == .cut and !p.embedded) try bodies.append(a, flat);
    }

    // the regular label point is kept when it already sits on the line, clear of the junctions with the neighbors
    for (skels.items) |sk| {
        const near = nearestOnPoly(sk.pts, prefer);
        if (near.dist > sk.half_width * 1.5 + 0.02 or clearance(edges.items, prefer, near.dir, sk.half_width + sk.gap) < text_h) continue;
        if (sk.exposed and insideBody(bodies.items, prefer, sk.half_width + sk.gap)) continue;
        return null;
    }

    const Sample = struct { p: V2, score: f64, d: f64 };
    var samples: std.ArrayList(Sample) = .empty;
    const step = @max(text_h * 0.5, 0.05);
    for (skels.items) |sk| {
        const region = try sec.visibleRegion(sk.prism);
        const total = polyLen(sk.pts);
        const n: usize = @intFromFloat(@min(400, @max(2, @ceil(total / step))));
        var k: usize = 0;
        while (k <= n) : (k += 1) {
            const at = total * @as(f64, @floatFromInt(k)) / @as(f64, @floatFromInt(n));
            const q = pointAt(sk.pts, at);
            if (!inset.contains(q.p)) continue;
            if (!sk.stroke and !shown(region, q.p, sk.half_width * 1.5 + 0.02)) continue;
            if (sk.exposed and insideBody(bodies.items, q.p, sk.half_width + sk.gap)) continue;
            const score = @min(clearance(edges.items, q.p, q.dir, sk.half_width + sk.gap), 2 * text_h);
            try samples.append(a, .{ .p = q.p, .score = score, .d = q.p.dist(prefer) });
        }
    }
    if (samples.items.len == 0) return null;
    std.mem.sort(Sample, samples.items, {}, struct {
        fn lt(_: void, x: Sample, y: Sample) bool {
            if (@abs(x.score - y.score) > 1e-6) return x.score > y.score;
            return x.d < y.d;
        }
    }.lt);
    var out: std.ArrayList(V2) = .empty;
    try out.append(a, samples.items[0].p);
    for (samples.items[1..]) |s| {
        if (out.items.len >= 6) break;
        var far = true;
        for (out.items) |o| if (o.dist(s.p) < text_h) {
            far = false;
        };
        if (far) try out.append(a, s.p);
    }
    return out.items;
}

/// Distance from `p` (on the line, running along `dir`) to the nearest neighbor edge that crosses the line there; edges
/// running along it (shared boundaries) do not count.
fn clearance(edges: []const [2]V2, p: V2, dir: V2, half_width: f64) f64 {
    var clear = std.math.inf(f64);
    for (edges) |e| {
        const d = geom.distPointSeg(p, e[0], e[1]);
        if (d >= clear) continue;
        const ev = e[1].sub(e[0]);
        const el = ev.len();
        if (el > 1e-9 and d < half_width * 2 + 0.05) {
            const cross = @abs(ev.x * dir.y - ev.y * dir.x) / el;
            if (cross < 0.15) continue;
        }
        clear = d;
    }
    return clear;
}

const Nearest = struct { dist: f64, dir: V2 };

/// Distance from `p` to the polyline and the direction of the nearest segment.
fn nearestOnPoly(pts: []const V2, p: V2) Nearest {
    var best = Nearest{ .dist = std.math.inf(f64), .dir = V2.init(1, 0) };
    for (pts[1..], 0..) |q, i| {
        const d = geom.distPointSeg(p, pts[i], q);
        if (d < best.dist) {
            const v = q.sub(pts[i]);
            const l = v.len();
            best = .{ .dist = d, .dir = if (l > 1e-12) V2.init(v.x / l, v.y / l) else V2.init(1, 0) };
        }
    }
    return best;
}

const OnLine = struct { p: V2, dir: V2 };

/// The point at arc length `at` along the polyline and the unit direction of its segment.
fn pointAt(pts: []const V2, at: f64) OnLine {
    var rem = at;
    var i: usize = 0;
    while (i + 1 < pts.len) : (i += 1) {
        const l = pts[i].dist(pts[i + 1]);
        if (rem <= l or i + 2 == pts.len) {
            const t = if (l > 1e-12) @min(rem / l, 1) else 0;
            const d = pts[i + 1].sub(pts[i]);
            return .{ .p = V2.init(pts[i].x + d.x * t, pts[i].y + d.y * t), .dir = if (l > 1e-12) V2.init(d.x / l, d.y / l) else V2.init(1, 0) };
        }
        rem -= l;
    }
    return .{ .p = pts[0], .dir = V2.init(1, 0) };
}

/// True when `p` lies strictly inside one of the bodies (not merely on its outline, where thin layers sit).
fn insideBody(bodies: []const []const []const V2, p: V2, half_width: f64) bool {
    const tol = half_width * 2 + 0.05;
    for (bodies) |b| {
        if (geom.locateEvenOdd(p, b, 1e-9) != .inside) continue;
        var depth = std.math.inf(f64);
        for (b) |loop| for (loop, 0..) |q, k| {
            depth = @min(depth, geom.distPointSeg(p, q, loop[(k + 1) % loop.len]));
        };
        if (depth > tol) return true;
    }
    return false;
}

/// True when `p` lies in (or within `tol` of) the visible region.
fn shown(region: []const []const V2, p: V2, tol: f64) bool {
    if (geom.locateEvenOdd(p, region, 1e-9) != .outside) return true;
    for (region) |loop| for (loop, 0..) |q, k| {
        if (geom.distPointSeg(p, q, loop[(k + 1) % loop.len]) <= tol) return true;
    };
    return false;
}
