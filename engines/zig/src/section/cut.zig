//! Cut prisms (SPEC 8.1): the cut region of one prism with its hatch, fill, cut mark, ply lines and outline, plus grouping of a region into outer loops with holes.

const std = @import("std");
const geom = @import("../geom.zig");
const model = @import("../model.zig");
const clip = @import("../clip.zig");
const style_mod = @import("../style.zig");
const Pen = @import("../pen.zig").Pen;
const hatch_mod = @import("../hatch.zig");
const pathclip = @import("../pathclip.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Pt = geom.Pt;
const Prism = model.Prism;
const isPathBar = @import("hardware.zig").isPathBar;
const section = @import("../section.zig");
const Section = section.Section;

/// Direction (degrees, 0..180) of the longest edge of a member outline: the way its grain runs.
pub fn memberAngleDeg(loop: []const Pt) f64 {
    var best: f64 = -1;
    var ang: f64 = 0;
    for (loop, 0..) |p, i| {
        const q = loop[(i + 1) % loop.len];
        const d = q.v().sub(p.v());
        const l = d.len();
        if (l > best + 1e-9) {
            best = l;
            ang = std.math.radiansToDegrees(std.math.atan2(d.y, d.x));
        }
    }
    ang = @mod(ang, 180.0);
    // snap run x / run y exactly (tiny float noise would rotate the pattern origin)
    if (ang < 1e-6 or ang > 180.0 - 1e-6) return 0;
    if (@abs(ang - 90.0) < 1e-6) return 90;
    return ang;
}

pub const Group = struct {
    /// Loops with bulges for the IR (outer first, then holes).
    loops: []const []const Pt,
    /// Flattened loops (even-odd) for hatch line generation.
    flat: []const []const V2,
};

/// Split oriented flat loops into connected pieces (outer + its holes). When `exact` is given (the
/// region is the untouched prism profile) the IR loops keep their bulges.
pub fn groupRegion(a: Allocator, region: []const []const V2, exact: ?[]const []const Pt) Allocator.Error![]const Group {
    if (exact) |ex| {
        const g = try a.alloc(Group, 1);
        g[0] = .{ .loops = ex, .flat = region };
        return g;
    }
    var outers: std.ArrayList(usize) = .empty;
    for (region, 0..) |l, i| if (geom.signedAreaV(l) > 0) try outers.append(a, i);
    var groups: std.ArrayList(Group) = .empty;
    for (outers.items) |oi| {
        var flat: std.ArrayList([]const V2) = .empty;
        try flat.append(a, region[oi]);
        for (region, 0..) |l, i| {
            if (geom.signedAreaV(l) < 0 and l.len > 0 and geom.pointInLoopEO(l[0], region[oi])) {
                // point may lie on the outer boundary for degenerate holes; nudge toward the loop centroid
                try flat.append(a, l);
            } else _ = i;
        }
        const loops = try a.alloc([]const Pt, flat.items.len);
        for (flat.items, 0..) |l, k| {
            const pts = try a.alloc(Pt, l.len);
            for (l, 0..) |v, q| pts[q] = Pt.at(v, 0);
            loops[k] = pts;
        }
        try groups.append(a, .{ .loops = loops, .flat = flat.items });
    }
    return groups.items;
}

pub fn drawCut(self: *Section, i: usize, p: Prism, holes: []const usize) Allocator.Error!void {
    const crop = self.spec.crop;
    const src = self.srcName(p);
    switch (p.kind) {
        .body => {},
        else => return,
    }
    if (p.face_tie) return self.drawFaceTie(i, p);
    const flat = try self.flatOf(i);
    const bx = clip.loopsBox(flat);
    if (!bx.overlaps(crop, 1e-6)) return;
    const path_bar = isPathBar(p);
    const mat = self.style.material(p.material);
    const is_fill_mat = if (mat) |m| m.fill else false;
    const fully_inside = bx.x0 >= crop.x0 - 1e-9 and bx.x1 <= crop.x1 + 1e-9 and bx.y0 >= crop.y0 - 1e-9 and bx.y1 <= crop.y1 + 1e-9;
    // ----- region for hatch/fill (crop clipped, holes for embedded items) -----
    // Thin cut regions (paper thickness < 2x the cut pen) render as a solid fill plus an outline.
    const cut_in = self.style.penWidthMm(.cut) / 25.4;
    var perim: f64 = 0;
    for (flat[0], 0..) |v, vi| perim += v.dist(flat[0][(vi + 1) % flat[0].len]);
    const thick_model = if (perim > 0) 2.0 * @abs(geom.signedAreaV(flat[0])) / perim else 1e9;
    const is_thin = p.outline == .full and p.role.isMetal() and !is_fill_mat and thick_model / self.spec.scale < 2.0 * cut_in;
    const pen_out: Pen = if (p.pen) |pp| self.penFor(pp) else if (is_fill_mat) self.penForRole(p.role) else if (is_thin) .steel else .cut;
    var region: []const []const V2 = flat;
    var region_exact: bool = fully_inside;
    if (!fully_inside) {
        region = try clip.boolean(self.a, flat, &.{self.crop_loop}, .intersect);
        region_exact = false;
    }
    if (!p.embedded and holes.len > 0) {
        var changed = false;
        for (holes) |h| {
            if (h == i) continue;
            const hf = try self.flatOf(h);
            const hb = clip.loopsBox(hf);
            if (!hb.overlaps(bx, 1e-9)) continue;
            region = try clip.boolean(self.a, region, hf, .diff);
            changed = true;
        }
        if (changed) region_exact = false;
    }
    // hatch / fill
    const want_hatch = mat != null and mat.?.hatch.len > 0;
    if (want_hatch or (!path_bar and (is_fill_mat or is_thin) and p.kind == .body)) {
        if (region.len > 0) {
            const groups = try groupRegion(self.a, region, if (region_exact) p.loops else null);
            for (groups) |g| {
                if (is_fill_mat or is_thin) {
                    try self.fills.append(self.a, .{ .layer = self.layerFor(pen_out), .src = src, .loops = g.loops });
                }
                if (want_hatch and !is_thin) {
                    for (mat.?.hatch) |hs| {
                        const pat = self.style.pattern(hs.pattern) orelse continue;
                        const res = try hatch_mod.generate(self.a, g.flat, pat, self.spec.scale * hs.scale, hs.angle);
                        if (res.truncated) self.truncated_hatch = true;
                        try self.hatches.append(self.a, .{
                            .layer = self.layerFor(.hatch),
                            .pen = .hatch,
                            .src = src,
                            .pattern = hs.pattern,
                            .scale = hs.scale,
                            .angle = hs.angle,
                            .loops = g.loops,
                            .lines = res.lines,
                        });
                    }
                }
            }
        }
    }
    // wood grain: lumber cut lengthwise (run x or y, no end-grain quads) gets sparse wavy lines along its length
    if (mat != null and mat.?.grain != null and region.len > 0 and self.isLengthwiseLumber(p)) {
        const gs = mat.?.grain.?;
        if (self.style.pattern(gs.pattern)) |pat| {
            const ang = memberAngleDeg(p.loops[0]);
            const groups = try groupRegion(self.a, region, if (region_exact) p.loops else null);
            for (groups) |g| {
                const res = try hatch_mod.generateGrain(self.a, g.flat, pat, self.spec.scale * gs.scale, ang, .{ .amp = gs.amplitude, .wavelength = gs.wavelength });
                if (res.truncated) self.truncated_hatch = true;
                if (res.lines.len == 0) continue;
                try self.hatches.append(self.a, .{
                    .layer = self.layerFor(.hatch),
                    .pen = .hatch,
                    .src = src,
                    .pattern = gs.pattern,
                    .scale = gs.scale,
                    .angle = ang,
                    .loops = g.loops,
                    .lines = res.lines,
                });
            }
        }
    }
    // ----- outline -----
    if (path_bar) {
        // SPEC 20: a path-mode bar is one centerline in the rebar pen (bends stay true arcs); the ribbon only clears hatch.
        try self.addClipped(&self.strokes, p.centerline, false, self.penFor(.rebar), src, true);
    } else switch (p.outline) {
        .none => {},
        .full => for (p.loops) |l| try self.addClipped(&self.strokes, l, true, pen_out, src, p.embedded),
        .top => for (p.loops) |l| try self.topEdges(l, pen_out, src),
    }
    // ----- cut marks and ply lines -----
    if (mat) |m| {
        const mark = if (p.quads.len > 0) (if (p.blocking and m.cut_mark != .none) style_mod.CutMark.diagonal else m.cut_mark) else style_mod.CutMark.none;
        if (mark != .none) {
            for (p.quads) |q| {
                if (pathclip.clipSeg(q[0], q[2], crop)) |s| {
                    try self.addStroke(&self.strokes, try self.a.dupe(Pt, &.{ Pt.at(s[0], 0), Pt.at(s[1], 0) }), false, .beyond, src, false);
                }
                if (mark == .x) {
                    if (pathclip.clipSeg(q[1], q[3], crop)) |s| {
                        try self.addStroke(&self.strokes, try self.a.dupe(Pt, &.{ Pt.at(s[0], 0), Pt.at(s[1], 0) }), false, .beyond, src, false);
                    }
                }
            }
        }
    }
    for (p.ply_lines) |pl| {
        if (pathclip.clipSeg(pl[0], pl[1], crop)) |s| {
            try self.addStroke(&self.strokes, try self.a.dupe(Pt, &.{ Pt.at(s[0], 0), Pt.at(s[1], 0) }), false, .beyond, src, false);
        }
    }
}
