//! Hardware drawing conventions (SPEC 20): thin sheet metal is thickened to a visible minimum, face-on ties are outlines with nail holes, path rebar is one centerline.

const std = @import("std");
const cast = @import("../num.zig");
const geom = @import("../geom.zig");
const clip = @import("../clip.zig");
const model = @import("../model.zig");
const pathgeom = @import("../pathgeom.zig");
const Allocator = std.mem.Allocator;
const Prism = model.Prism;
const strokes = @import("strokes.zig");
const flat_tol = geom.flat_tol;
const Stroke = @import("strokes.zig").Stroke;
const Occ = @import("occlusion.zig").Occ;
const visibleOpen = @import("occlusion.zig").visibleOpen;
const section = @import("../section.zig");
const Section = section.Section;

/// Minimum drawn thickness of sheet metal (straps, flashing) in paper inches: about 0.55 mm, so a CS16 strap is a visible band
/// next to the host member's 0.5 mm outline instead of a hairline merged with it. The true thickness lives in the 3D model.
pub const min_metal_paper_in: f64 = 0.022;

/// SPEC 20 minimum visibility: thin edge-lay sheet metal (a ribbon along a centerline) is widened, on the side(s) it already
/// occupies, to at least `min_metal_paper_in` on paper.
pub fn thickenThinHardware(a: Allocator, prisms: []const Prism, scale: f64) Allocator.Error![]const Prism {
    var out: ?[]Prism = null;
    const min_t = min_metal_paper_in * scale;
    for (prisms, 0..) |p, i| {
        if (p.kind != .body or p.face_tie or p.sweep_r > 0 or p.centerline.len < 2 or p.loops.len != 1) continue;
        if (!p.role.isMetal() or p.embedded and p.role != .steel) continue;
        const flat = try geom.flattenPolyline(a, p.loops[0], true, flat_tol);
        var len: f64 = 0;
        const cl = try geom.flattenPolyline(a, p.centerline, false, flat_tol);
        for (cl[0 .. cl.len - 1], 0..) |q, k| len += q.dist(cl[k + 1]);
        if (len < 1e-9) continue;
        const t = @abs(geom.signedAreaV(flat)) / len;
        if (t >= min_t - 1e-9) continue;
        // which side(s) of the first segment does the ribbon occupy?
        const d = cl[1].sub(cl[0]).norm();
        const m = cl[0].add(cl[1]).scale(0.5);
        const probe = @min(t, 0.5 * cl[0].dist(cl[1])) * 0.5;
        const on_left = geom.pointInLoopEO(m.add(d.perp().scale(probe)), flat);
        const on_right = geom.pointInLoopEO(m.sub(d.perp().scale(probe)), flat);
        if (!on_left and !on_right) continue;
        if (out == null) out = try a.dupe(Prism, prisms);
        const lt: f64 = if (on_left and on_right) min_t / 2 else if (on_left) min_t else 0;
        const rt: f64 = if (on_left and on_right) min_t / 2 else if (on_right) min_t else 0;
        const rib = try pathgeom.ribbon(a, p.centerline, lt, rt);
        out.?[i].loops = try model.oneLoop(a, rib);
    }
    return out orelse prisms;
}

/// A rebar bar in `path` mode (a swept centerline), as opposed to `along_z` dots.
pub fn isPathBar(p: Prism) bool {
    return p.kind == .body and p.centerline.len >= 2 and p.role == .rebar;
}

/// Lines of ordinary members run hidden under a face-on tie (the strap is in front of them): remove the
/// parts of non-embedded strokes that fall inside a tie outline so the tie and its nail dots read cleanly.
pub fn knockOutFaceTies(self: *Section) Allocator.Error!void {
    var occs: std.ArrayList(Occ) = .empty;
    for (self.prisms, 0..) |p, i| {
        if (self.cls[i] == .drop or !p.face_tie) continue;
        const f = try self.flatOf(i);
        try occs.append(self.a, .{ .loops = f, .box = clip.loopsBox(f), .z1 = p.z1, .cut = false, .prism = i });
    }
    if (occs.items.len == 0) return;
    const refs = try self.a.alloc(*const Occ, occs.items.len);
    for (occs.items, 0..) |*o, i| refs[i] = o;
    var out: std.ArrayList(Stroke) = .empty;
    for (self.strokes.items) |st| {
        if (st.rank >= 10) {
            try out.append(self.a, st);
            continue;
        }
        const pieces = try visibleOpen(self.a, st.pts, st.closed, refs);
        for (pieces) |pc| {
            var q = st;
            q.pts = pc.pts;
            q.closed = pc.closed;
            try out.append(self.a, q);
        }
    }
    self.strokes = out;
}

/// Face-on hardware (SPEC 20): outline in the steel pen plus nail-hole dots at 1" pitch along the centerline,
/// drawn over everything so H2.5A / HETA style ties read at small scales.
pub fn drawFaceTie(self: *Section, i: usize, p: Prism) Allocator.Error!void {
    const crop = self.spec.crop;
    const src = self.srcName(p);
    const S = self.spec.scale;
    const pen = self.penFor(.steel);
    for (p.loops) |l| try self.addClipped(&self.strokes, l, true, pen, src, true);
    if (p.centerline.len < 2) return;
    const flat = try geom.flattenPolyline(self.a, p.centerline, false, flat_tol);
    var total: f64 = 0;
    for (flat[0 .. flat.len - 1], 0..) |q, k| total += q.dist(flat[k + 1]);
    // dot size follows the paper (0.016" radius), never below a true 0.05" hole; pitch is whole inches, >= 0.07" on paper
    const strap_w = if (total > 0) @abs(geom.signedAreaV((try self.flatOf(i))[0])) / total else 0;
    const r = @min(@max(0.05, 0.016 * S), 0.15 * strap_w + 0.02);
    const pitch = @ceil(@max(1.0, 0.07 * S));
    if (total < 0.5 * pitch) return;
    const n: usize = cast.toIntClamped(usize, @floor(total / pitch + 1e-9), 1, 5000);
    const first = (total - @as(f64, @floatFromInt(n - 1)) * pitch) / 2;
    var seg: usize = 0;
    var seg_start: f64 = 0;
    var k: usize = 0;
    while (k < n) : (k += 1) {
        const d = first + @as(f64, @floatFromInt(k)) * pitch;
        while (seg + 2 < flat.len and d > seg_start + flat[seg].dist(flat[seg + 1])) {
            seg_start += flat[seg].dist(flat[seg + 1]);
            seg += 1;
        }
        const a0 = flat[seg];
        const b0 = flat[seg + 1];
        const sl = a0.dist(b0);
        const c = if (sl < 1e-12) a0 else a0.add(b0.sub(a0).scale((d - seg_start) / sl));
        if (c.x - r < crop.x0 or c.x + r > crop.x1 or c.y - r < crop.y0 or c.y + r > crop.y1) continue;
        try self.fills.append(self.a, .{ .layer = self.layerFor(pen), .src = src, .loops = try model.oneLoop(self.a, try model.circleLoop(self.a, c.x, c.y, r)) });
    }
}
