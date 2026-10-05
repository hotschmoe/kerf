//! Section view (SPEC 8.1): exact 2D cut / beyond / hatch with crop, break lines and dedupe.

const std = @import("std");
const geom = @import("geom.zig");
const clip = @import("clip.zig");
const model = @import("model.zig");
const scene_mod = @import("scene.zig");
const style_mod = @import("style.zig");
const view_mod = @import("view.zig");
const hatch_mod = @import("hatch.zig");
const pathclip = @import("pathclip.zig");
const drawing = @import("drawing.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Pt = geom.Pt;
const Box = geom.Box;
const Prism = model.Prism;

pub const flat_tol: f64 = 0.005;

pub const Class = enum { cut, beyond, drop };

pub const Stroke = struct {
    pts: []const Pt,
    closed: bool,
    pen: []const u8,
    src: []const u8,
    /// Paint rank: lighter first, embedded last.
    rank: f64,
    seq: u32,
};

pub const Occ = struct {
    loops: []const []const V2,
    box: Box,
    z1: f64,
    cut: bool,
    prism: usize,
};

pub const Section = struct {
    a: Allocator,
    scene: *const scene_mod.Scene,
    style: *const style_mod.Style,
    spec: *const view_mod.ViewSpec,
    prisms: []const Prism,
    cls: []Class,
    flat: []?[]const []const V2,
    occs: std.ArrayList(Occ) = .empty,
    strokes: std.ArrayList(Stroke) = .empty,
    hatches: std.ArrayList(drawing.HatchItem) = .empty,
    fills: std.ArrayList(drawing.FillItem) = .empty,
    ghosts: std.ArrayList(Stroke) = .empty,
    breaks: std.ArrayList(drawing.PathItem) = .empty,
    seq: u32 = 0,
    truncated_hatch: bool = false,
    crop_loop: []const V2 = &.{},

    pub fn init(a: Allocator, scene: *const scene_mod.Scene, spec: *const view_mod.ViewSpec, prisms: []const Prism) Allocator.Error!Section {
        const cls = try a.alloc(Class, prisms.len);
        const eps = 1e-9;
        for (prisms, 0..) |p, i| {
            if (p.z0 < spec.cut_z - eps and p.z1 > spec.cut_z + eps) {
                cls[i] = .cut;
            } else if (p.z1 <= spec.cut_z + eps) {
                cls[i] = .beyond;
            } else cls[i] = .drop;
        }
        const flat = try a.alloc(?[]const []const V2, prisms.len);
        @memset(flat, null);
        const c = spec.crop;
        const cl = try a.dupe(V2, &.{ V2.init(c.x0, c.y0), V2.init(c.x1, c.y0), V2.init(c.x1, c.y1), V2.init(c.x0, c.y1) });
        return .{ .a = a, .scene = scene, .style = scene.style, .spec = spec, .prisms = prisms, .cls = cls, .flat = flat, .crop_loop = cl };
    }

    fn srcName(self: *const Section, p: Prism) []const u8 {
        const c = &self.scene.comps[p.comp];
        if (c.arr_count > 1) return std.fmt.allocPrint(self.a, "{s}#{d}", .{ c.id, p.instance / @as(u32, @intCast(c.xfs.len / c.arr_count)) }) catch c.id;
        return c.id;
    }

    pub fn flatOf(self: *Section, i: usize) Allocator.Error![]const []const V2 {
        if (self.flat[i]) |f| return f;
        const f = try clip.fromRegion(self.a, self.prisms[i].loops, flat_tol);
        self.flat[i] = f;
        return f;
    }

    fn penFor(self: *const Section, name: []const u8) []const u8 {
        return if (self.style.pen(name) != null) name else "beyond";
    }

    fn layerFor(self: *const Section, pen: []const u8) []const u8 {
        return self.style.layerForPen(pen);
    }

    fn addStroke(self: *Section, list: *std.ArrayList(Stroke), pts: []const Pt, closed: bool, pen: []const u8, src: []const u8, embedded: bool) Allocator.Error!void {
        const w = self.style.penWidthMm(pen);
        self.seq += 1;
        try list.append(self.a, .{ .pts = pts, .closed = closed, .pen = pen, .src = src, .rank = w + (if (embedded) @as(f64, 10) else 0), .seq = self.seq });
    }

    /// Clip a path to the crop and add the pieces as strokes.
    fn addClipped(self: *Section, list: *std.ArrayList(Stroke), pts: []const Pt, closed: bool, pen: []const u8, src: []const u8, embedded: bool) Allocator.Error!void {
        const pieces = try pathclip.clipPath(self.a, pts, closed, self.spec.crop);
        for (pieces) |pc| try self.addStroke(list, pc.pts, pc.closed, pen, src, embedded);
    }

    fn materialPenName(self: *const Section, mat: []const u8) []const u8 {
        // fill materials draw with the pen of the same name (rebar, steel)
        if (self.style.material(mat)) |m| if (m.fill) return self.penFor(mat);
        return "cut";
    }

    pub fn build(self: *Section) Allocator.Error!void {
        const crop = self.spec.crop;
        const margin = 1e-6;
        // gather occluders (flattened cut/beyond bodies that touch the crop)
        for (self.prisms, 0..) |p, i| {
            if (self.cls[i] == .drop or p.kind != .body) continue;
            const f = try self.flatOf(i);
            const bx = clip.loopsBox(f);
            if (!bx.overlaps(crop, margin)) continue;
            try self.occs.append(self.a, .{ .loops = f, .box = bx, .z1 = p.z1, .cut = self.cls[i] == .cut, .prism = i });
        }
        // embedded cut footprints used as hatch holes
        var holes: std.ArrayList(usize) = .empty;
        for (self.prisms, 0..) |p, i| {
            if (self.cls[i] == .cut and p.embedded and p.kind == .body) try holes.append(self.a, i);
        }
        // draw cut prisms (non-embedded first, then embedded)
        var pass: u8 = 0;
        while (pass < 2) : (pass += 1) {
            for (self.prisms, 0..) |p, i| {
                if (self.cls[i] != .cut) continue;
                if (p.embedded != (pass == 1)) continue;
                try self.drawCut(i, p, holes.items);
            }
        }
        // beyond prisms
        for (self.prisms, 0..) |p, i| {
            if (self.cls[i] != .beyond) continue;
            try self.drawBeyond(i, p);
        }
        // ghosts / lines for non-drop prisms that are not bodies
        for (self.prisms, 0..) |p, i| {
            if (self.cls[i] == .drop) continue;
            switch (p.kind) {
                .ghost => {
                    const pen = self.penFor(p.pen orelse "hidden");
                    for (p.loops) |l| try self.addClipped(&self.ghosts, l, true, pen, self.srcName(p), true);
                },
                .line => {
                    if (p.line_pts.len >= 2) {
                        const pen = self.penFor(if (self.style.material(p.material)) |m| (m.pen orelse "membrane") else "membrane");
                        var lp = p.line_pts;
                        if (std.mem.eql(u8, p.material, "vapor_retarder") and p.centerline.len >= 2) {
                            // keep the dashed line visibly separate from the host edge it follows
                            const c0 = p.centerline[0].v();
                            const d0 = p.centerline[1].v().sub(c0).norm();
                            const off = p.line_pts[0].v().sub(c0);
                            const side: f64 = if (d0.perp().dot(off) >= 0) 1 else -1;
                            const gap = @max(off.len(), 0.03 * self.spec.scale);
                            lp = try @import("pathgeom.zig").offsetOpen(self.a, p.centerline, side * gap);
                        }
                        try self.addClipped(&self.strokes, lp, false, pen, self.srcName(p), p.embedded);
                    }
                },
                .batt => {
                    if (p.line_pts.len >= 2) try self.addClipped(&self.strokes, p.line_pts, false, "profile", self.srcName(p), false);
                },
                .body => {},
            }
        }
        try self.breakLines();
    }

    fn shingleTicks(self: *Section, p: Prism, pen: []const u8) Allocator.Error!void {
        // short ticks perpendicular to the polyline on the thick side
        const S = self.spec.scale;
        const step = 0.14 * S;
        const len = 0.05 * S;
        const flat_line = try geom.flattenPolyline(self.a, p.line_pts, false, flat_tol);
        var carry: f64 = step / 2;
        var i: usize = 0;
        while (i + 1 < flat_line.len) : (i += 1) {
            const a0 = flat_line[i];
            const b0 = flat_line[i + 1];
            const d = b0.sub(a0);
            const l = d.len();
            if (l < 1e-9) continue;
            const dir = d.scale(1.0 / l);
            const nrm = dir.perp();
            var s = carry;
            while (s <= l) : (s += step) {
                const pt = a0.add(dir.scale(s));
                const pts = try self.a.dupe(Pt, &.{ Pt.at(pt, 0), Pt.at(pt.add(nrm.scale(-len)), 0) });
                try self.addClipped(&self.strokes, pts, false, pen, self.srcName(p), false);
            }
            carry = s - l;
        }
    }

    // ---- cut prisms ---------------------------------------------------------------------------------

    fn drawCut(self: *Section, i: usize, p: Prism, holes: []const usize) Allocator.Error!void {
        const crop = self.spec.crop;
        const src = self.srcName(p);
        switch (p.kind) {
            .body => {},
            else => return,
        }
        const flat = try self.flatOf(i);
        const bx = clip.loopsBox(flat);
        if (!bx.overlaps(crop, 1e-6)) return;
        const mat = self.style.material(p.material);
        const is_fill_mat = if (mat) |m| m.fill else false;
        const fully_inside = bx.x0 >= crop.x0 - 1e-9 and bx.x1 <= crop.x1 + 1e-9 and bx.y0 >= crop.y0 - 1e-9 and bx.y1 <= crop.y1 + 1e-9;
        // ----- region for hatch/fill (crop clipped, holes for embedded items) -----
        // Thin cut regions (paper thickness < 2x the cut pen) render as a solid fill plus an outline.
        const cut_in = self.style.penWidthMm("cut") / 25.4;
        var perim: f64 = 0;
        for (flat[0], 0..) |v, vi| perim += v.dist(flat[0][(vi + 1) % flat[0].len]);
        const thick_model = if (perim > 0) 2.0 * @abs(geom.signedAreaV(flat[0])) / perim else 1e9;
        const is_thin = p.outline == .full and isMetal(p.material) and !is_fill_mat and thick_model / self.spec.scale < 2.0 * cut_in;
        const pen_out: []const u8 = if (p.pen) |pp| self.penFor(pp) else if (is_fill_mat) self.penFor(self.materialNameForPen(p.material)) else if (is_thin) "steel" else "cut";
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
        if (want_hatch or ((is_fill_mat or is_thin) and p.kind == .body)) {
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
                                .layer = self.layerFor("hatch"),
                                .pen = "hatch",
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
                        .layer = self.layerFor("hatch"),
                        .pen = "hatch",
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
        switch (p.outline) {
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
                        try self.addStroke(&self.strokes, try self.a.dupe(Pt, &.{ Pt.at(s[0], 0), Pt.at(s[1], 0) }), false, "beyond", src, false);
                    }
                    if (mark == .x) {
                        if (pathclip.clipSeg(q[1], q[3], crop)) |s| {
                            try self.addStroke(&self.strokes, try self.a.dupe(Pt, &.{ Pt.at(s[0], 0), Pt.at(s[1], 0) }), false, "beyond", src, false);
                        }
                    }
                }
            }
        }
        for (p.ply_lines) |pl| {
            if (pathclip.clipSeg(pl[0], pl[1], crop)) |s| {
                try self.addStroke(&self.strokes, try self.a.dupe(Pt, &.{ Pt.at(s[0], 0), Pt.at(s[1], 0) }), false, "beyond", src, false);
            }
        }
    }

    /// Sawn or engineered lumber lying in the section plane (run x/y): cut along its length, not end-on.
    fn isLengthwiseLumber(self: *const Section, p: Prism) bool {
        if (p.quads.len > 0 or p.loops.len == 0 or p.loops[0].len < 3) return false;
        if (p.comp >= self.scene.comps.len) return false;
        return std.mem.eql(u8, self.scene.comps[p.comp].ty.name, "lumber");
    }

    fn materialNameForPen(self: *const Section, mat: []const u8) []const u8 {
        _ = self;
        return mat;
    }

    /// Stroke only the edges whose outward normal points up (grade lines). Loops are CCW.
    fn topEdges(self: *Section, loop: []const Pt, pen: []const u8, src: []const u8) Allocator.Error!void {
        const ccw = geom.signedArea(loop) >= 0;
        var cur: std.ArrayList(Pt) = .empty;
        const n = loop.len;
        // find a start vertex where the previous edge does not qualify so runs are not split
        var start: usize = 0;
        var k: usize = 0;
        while (k < n) : (k += 1) {
            if (!self.edgeIsTop(loop, (k + n - 1) % n, ccw)) {
                start = k;
                break;
            }
        }
        var idx: usize = 0;
        while (idx < n) : (idx += 1) {
            const i = (start + idx) % n;
            const q = loop[(i + 1) % n];
            if (self.edgeIsTop(loop, i, ccw)) {
                if (cur.items.len == 0) {
                    try cur.append(self.a, loop[i]);
                } else cur.items[cur.items.len - 1].b = loop[i].b;
                try cur.append(self.a, Pt.at(q.v(), 0));
            } else if (cur.items.len > 0) {
                try self.addClipped(&self.strokes, cur.items, false, pen, src, false);
                cur = .empty;
            }
        }
        if (cur.items.len > 0) try self.addClipped(&self.strokes, cur.items, false, pen, src, false);
    }

    fn edgeIsTop(self: *const Section, loop: []const Pt, i: usize, ccw: bool) bool {
        _ = self;
        const p = loop[i];
        const q = loop[(i + 1) % loop.len];
        var d = q.v().sub(p.v());
        if (p.b != 0) {
            // use the arc's mid tangent normal via the chord: good enough for grade lines
            d = q.v().sub(p.v());
        }
        const l = d.len();
        if (l < 1e-12) return false;
        const n = if (ccw) V2.init(d.y / l, -d.x / l) else V2.init(-d.y / l, d.x / l);
        return n.y > 0.01;
    }

    // ---- beyond prisms --------------------------------------------------------------------------------

    fn drawBeyond(self: *Section, i: usize, p: Prism) Allocator.Error!void {
        if (p.kind != .body) return;
        const crop = self.spec.crop;
        const f = try self.flatOf(i);
        if (!clip.loopsBox(f).overlaps(crop, 1e-6)) return;
        const src = self.srcName(p);
        const mat = self.style.material(p.material);
        const pen: []const u8 = if (mat != null and mat.?.fill) self.penFor(p.material) else "beyond";
        // occluders
        var occ: std.ArrayList(*const Occ) = .empty;
        if (!p.embedded) {
            for (self.occs.items) |*o| {
                if (o.prism == i) continue;
                if (o.cut or o.z1 > p.z1 + 1e-9) try occ.append(self.a, o);
            }
        }
        for (p.loops) |loop| {
            const pieces = try visiblePieces(self.a, loop, occ.items);
            for (pieces) |pc| try self.addClipped(&self.strokes, pc.pts, pc.closed, pen, src, p.embedded);
        }
    }

    /// Visible (non-occluded, in-crop) region polygons of prism `i` (for note landing points).
    pub fn visibleRegion(self: *Section, i: usize) Allocator.Error![]const []const V2 {
        const p = self.prisms[i];
        if (self.cls[i] == .drop or (p.kind == .ghost and !p.dashed)) return &.{};
        var region: []const []const V2 = try self.flatOf(i);
        const crop = self.spec.crop;
        region = try clip.boolean(self.a, region, &.{self.crop_loop}, .intersect);
        if (self.cls[i] == .beyond and !p.embedded and !p.dashed) {
            for (self.occs.items) |o| {
                if (o.prism == i) continue;
                if (!(o.cut or o.z1 > p.z1 + 1e-9)) continue;
                if (!o.box.overlaps(clip.loopsBox(region), 1e-9)) continue;
                region = try clip.boolean(self.a, region, o.loops, .diff);
            }
        }
        _ = crop;
        return region;
    }

    // ---- break lines ---------------------------------------------------------------------------------

    const Edge = struct { vertical: bool, c: f64, lo: f64, hi: f64, out_sign: f64 };

    fn breakLines(self: *Section) Allocator.Error!void {
        const crop = self.spec.crop;
        const edges = [4]Edge{
            .{ .vertical = true, .c = crop.x0, .lo = crop.y0, .hi = crop.y1, .out_sign = -1 },
            .{ .vertical = true, .c = crop.x1, .lo = crop.y0, .hi = crop.y1, .out_sign = 1 },
            .{ .vertical = false, .c = crop.y0, .lo = crop.x0, .hi = crop.x1, .out_sign = -1 },
            .{ .vertical = false, .c = crop.y1, .lo = crop.x0, .hi = crop.x1, .out_sign = 1 },
        };
        for (edges) |e| {
            const Iv = struct { a: f64, b: f64, comp: u32 };
            var ivs: std.ArrayList(Iv) = .empty;
            for (self.prisms, 0..) |p, i| {
                if (self.cls[i] != .cut or p.kind != .body or p.embedded) continue;
                if (self.style.material(p.material)) |m| {
                    if (isFillMaterial(p.material) or m.fill) continue;
                } else if (isFillMaterial(p.material)) continue;
                const f = try self.flatOf(i);
                const bx = clip.loopsBox(f);
                if (e.vertical) {
                    if (bx.x0 > e.c + 1e-9 or bx.x1 < e.c - 1e-9) continue;
                } else {
                    if (bx.y0 > e.c + 1e-9 or bx.y1 < e.c - 1e-9) continue;
                }
                const xs = try pathclip.scan(self.a, f, e.c, e.vertical);
                var k: usize = 0;
                while (k + 1 < xs.len) : (k += 2) {
                    const lo = @max(xs[k], e.lo);
                    const hi = @min(xs[k + 1], e.hi);
                    if (hi - lo < 1e-6) continue;
                    // is the region really clipped here? probe just outside the edge
                    const mid = (lo + hi) / 2;
                    const probe = if (e.vertical) V2.init(e.c + e.out_sign * 1e-6, mid) else V2.init(mid, e.c + e.out_sign * 1e-6);
                    if (geom.locateEvenOdd(probe, f, 0) != .inside) continue;
                    try ivs.append(self.a, .{ .a = lo, .b = hi, .comp = p.comp });
                }
            }
            if (ivs.items.len == 0) continue;
            std.mem.sort(Iv, ivs.items, {}, struct {
                fn lt(_: void, x: Iv, y: Iv) bool {
                    if (x.a != y.a) return x.a < y.a;
                    return x.comp < y.comp;
                }
            }.lt);
            // merge touching intervals
            var merged: std.ArrayList(Iv) = .empty;
            for (ivs.items) |iv| {
                if (merged.items.len > 0 and iv.a <= merged.items[merged.items.len - 1].b + 1e-3) {
                    const last = &merged.items[merged.items.len - 1];
                    last.b = @max(last.b, iv.b);
                    last.comp = @min(last.comp, iv.comp);
                } else try merged.append(self.a, iv);
            }
            for (merged.items) |m| {
                const pts = try self.breakPolyline(e, m.a, m.b);
                try self.breaks.append(self.a, .{ .layer = self.layerFor("break"), .pen = "break", .src = "crop", .closed = false, .pts = pts });
            }
        }
    }

    fn breakPolyline(self: *Section, e: Edge, a0: f64, b0: f64) Allocator.Error![]const Pt {
        const S = self.spec.scale;
        const over = self.style.break_overshoot_in * S;
        const a_ = a0 - over;
        const b_ = b0 + over;
        const len = b_ - a_;
        const mid = (a_ + b_) / 2;
        const half = @min(self.style.break_period_in * S / 2, len * 0.35);
        const zig = @min(self.style.break_zig_in * S, len * 0.2);
        // along-edge parameter u, perpendicular offset v
        const uv = [_][2]f64{
            .{ a_, 0 },
            .{ mid - half, 0 },
            .{ mid - half * 0.4, zig },
            .{ mid + half * 0.4, -zig },
            .{ mid + half, 0 },
            .{ b_, 0 },
        };
        const out = try self.a.alloc(Pt, uv.len);
        for (uv, 0..) |q, k| {
            out[k] = if (e.vertical) .{ .x = e.c + q[1], .y = q[0] } else .{ .x = q[0], .y = e.c + q[1] };
        }
        return out;
    }

    /// Non-drawn picking regions: one item per visible cut region / beyond face (outer loop + holes).
    pub fn regionItems(self: *Section) Allocator.Error![]const drawing.Item {
        var out: std.ArrayList(drawing.Item) = .empty;
        for (self.prisms, 0..) |p, i| {
            if (self.cls[i] == .drop or (p.kind == .ghost and !p.dashed)) continue;
            const is_cut = self.cls[i] == .cut and !p.dashed;
            const comp = &self.scene.comps[p.comp];
            const inst: u32 = if (comp.arr_count > 1) p.instance / @as(u32, @intCast(comp.xfs.len / comp.arr_count)) else p.instance;
            const part: ?[]const u8 = if (p.part.len > 0) p.part else null;
            // untouched cut region inside the crop keeps its exact bulges
            const bx = geom.Box{};
            _ = bx;
            if (is_cut and p.kind == .body) {
                const f = try self.flatOf(i);
                const fb = clip.loopsBox(f);
                const c = self.spec.crop;
                if (fb.x0 >= c.x0 - 1e-9 and fb.x1 <= c.x1 + 1e-9 and fb.y0 >= c.y0 - 1e-9 and fb.y1 <= c.y1 + 1e-9) {
                    try out.append(self.a, .{ .region = .{ .src = self.srcName(p), .part = part, .instance = inst, .cut = true, .loops = p.loops } });
                    continue;
                }
            }
            const reg = try self.visibleRegion(i);
            const groups = try groupRegion(self.a, reg, null);
            for (groups) |g| {
                try out.append(self.a, .{ .region = .{ .src = self.srcName(p), .part = part, .instance = inst, .cut = is_cut, .loops = g.loops } });
            }
        }
        return out.items;
    }

    // ---- finalize ----------------------------------------------------------------------------------------

    /// Items in paint order, with coincident collinear edges deduplicated.
    pub fn finish(self: *Section) Allocator.Error![]const drawing.Item {
        var items: std.ArrayList(drawing.Item) = .empty;
        for (self.hatches.items) |h| try items.append(self.a, .{ .hatch = h });
        for (self.fills.items) |f| try items.append(self.a, .{ .fill = f });
        const deduped = try chainStrokes(self.a, try dedupe(self.a, self.strokes.items, self.style));
        // stable sort by rank (lighter first)
        std.mem.sort(Stroke, deduped, {}, struct {
            fn lt(_: void, x: Stroke, y: Stroke) bool {
                if (x.rank != y.rank) return x.rank < y.rank;
                return x.seq < y.seq;
            }
        }.lt);
        for (deduped) |s| try items.append(self.a, .{ .path = .{ .layer = self.layerFor(s.pen), .pen = s.pen, .src = s.src, .closed = s.closed, .pts = s.pts } });
        for (self.ghosts.items) |s| try items.append(self.a, .{ .path = .{ .layer = self.layerFor(s.pen), .pen = s.pen, .src = s.src, .closed = s.closed, .pts = s.pts } });
        for (self.breaks.items) |b| try items.append(self.a, .{ .path = b });
        return items.items;
    }
};

/// Direction (degrees, 0..180) of the longest edge of a member outline: the way its grain runs.
fn memberAngleDeg(loop: []const Pt) f64 {
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

pub fn isFillMaterial(name: []const u8) bool {
    const eq = std.mem.eql;
    return eq(u8, name, "earth") or eq(u8, name, "gravel") or eq(u8, name, "sand") or eq(u8, name, "compacted_fill");
}

// ---- region grouping ----------------------------------------------------------------------------------------

const Group = struct {
    /// Loops with bulges for the IR (outer first, then holes).
    loops: []const []const Pt,
    /// Flattened loops (even-odd) for hatch line generation.
    flat: []const []const V2,
};

/// Split oriented flat loops into connected pieces (outer + its holes). When `exact` is given (the
/// region is the untouched prism profile) the IR loops keep their bulges.
fn groupRegion(a: Allocator, region: []const []const V2, exact: ?[]const []const Pt) Allocator.Error![]const Group {
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

// ---- visibility of a loop against occluders ---------------------------------------------------------------

const PieceOut = struct { pts: []const Pt, closed: bool };

fn pushUnique(a: Allocator, ts: *std.ArrayList(f64), t: f64) Allocator.Error!void {
    if (t > 1e-9 and t < 1 - 1e-9) try ts.append(a, t);
}

/// The parts of `loop` (a closed bulge loop) not hidden inside any occluder.
pub fn visiblePieces(a: Allocator, loop: []const Pt, occ: []const *const Occ) Allocator.Error![]const PieceOut {
    var out: std.ArrayList(PieceOut) = .empty;
    if (occ.len == 0) {
        try out.append(a, .{ .pts = loop, .closed = true });
        return out.items;
    }
    const n = loop.len;
    var cur: std.ArrayList(Pt) = .empty;
    var any_hidden = false;
    var starts_visible_at_0 = false;
    var segs_done: usize = 0;
    var first_flushed: bool = false;
    var i: usize = 0;
    while (i < n) : (i += 1) {
        const p0 = loop[i].v();
        const p1 = loop[(i + 1) % n].v();
        const bulge = loop[i].b;
        var sbox = Box{};
        geom.segBoxInto(&sbox, p0, p1, bulge);
        var ts: std.ArrayList(f64) = .empty;
        defer ts.deinit(a);
        for (occ) |o| {
            if (!o.box.overlaps(sbox, 1e-7)) continue;
            for (o.loops) |ol| {
                for (ol, 0..) |q0, k| {
                    const q1 = ol[(k + 1) % ol.len];
                    var ta: [2]f64 = undefined;
                    var tb: [2]f64 = undefined;
                    if (bulge == 0) {
                        const cnt = geom.segSeg(p0, p1, q0, q1, &ta, &tb);
                        for (0..cnt) |c| try pushUnique(a, &ts, ta[c]);
                    } else {
                        const cnt = geom.arcSeg(p0, p1, bulge, q0, q1, &ta, &tb);
                        for (0..cnt) |c| try pushUnique(a, &ts, ta[c]);
                    }
                }
            }
        }
        std.mem.sort(f64, ts.items, {}, std.sort.asc(f64));
        // sub-segments
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
                    if (!first_flushed and segs_done == 0) first_flushed = true;
                }
            } else {
                if (cur.items.len == 0) {
                    try cur.append(a, .{ .x = sub.a.x, .y = sub.a.y, .b = sub.bulge });
                    if (i == 0 and t_prev == 0) starts_visible_at_0 = true;
                } else cur.items[cur.items.len - 1].b = sub.bulge;
                try cur.append(a, .{ .x = sub.b.x, .y = sub.b.y, .b = 0 });
            }
            t_prev = t_next;
            segs_done += 1;
        }
    }
    if (!any_hidden) {
        out.clearRetainingCapacity();
        try out.append(a, .{ .pts = loop, .closed = true });
        return out.items;
    }
    if (cur.items.len > 0) {
        // join the tail to the head piece when the loop wraps through vertex 0
        if (starts_visible_at_0 and out.items.len > 0 and out.items[0].pts.len > 0) {
            const head = out.orderedRemove(0);
            var joined: std.ArrayList(Pt) = .empty;
            try joined.appendSlice(a, cur.items);
            joined.items[joined.items.len - 1].b = head.pts[0].b;
            try joined.appendSlice(a, head.pts[1..]);
            try out.append(a, .{ .pts = joined.items, .closed = false });
        } else {
            cur.items[cur.items.len - 1].b = 0;
            try out.append(a, .{ .pts = cur.items, .closed = false });
        }
    }
    return out.items;
}

// ---- dedupe ---------------------------------------------------------------------------------------------------------

const SegRef = struct {
    a: V2,
    b: V2,
    bulge: f64,
    stroke: u32,
    /// Remaining visible intervals along the segment ([t0,t1]); empty when fully removed.
    keep: std.ArrayList([2]f64) = .empty,
};

const dedupe_tol: f64 = 1e-4;

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
            if (s2.bulge != 0 or s2.stroke == s1.stroke and false) continue;
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
                try extras.append(a, .{ .pts = try a.dupe(Pt, &.{ Pt.at(p_lo, 0), Pt.at(p_hi, 0) }), .closed = false, .pen = "beyond", .src = strokes[s1.stroke].src, .rank = style.penWidthMm("beyond"), .seq = strokes[s1.stroke].seq });
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
        const nseg = hi - lo;
        for (items[lo..hi], 0..) |sr, k| {
            _ = k;
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
        _ = nseg;
        if (cur.items.len > 1) {
            try out.append(a, .{ .pts = cur.items, .closed = false, .pen = s.pen, .src = s.src, .rank = s.rank, .seq = s.seq });
        }
    }
    try out.appendSlice(a, extras.items);
    return out.items;
}

fn subtractInterval(a: Allocator, keep: *std.ArrayList([2]f64), lo: f64, hi: f64) Allocator.Error!void {
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
                if (sj.closed or !std.mem.eql(u8, si.pen, sj.pen) or !std.mem.eql(u8, si.src, sj.src)) continue;
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

fn baseId(src: []const u8) []const u8 {
    if (std.mem.indexOfScalar(u8, src, '#')) |h| return src[0..h];
    return src;
}

/// Materials the thin-region rule applies to (SPEC 16 parity decisions).
pub fn isMetal(name: []const u8) bool {
    const eq = std.mem.eql;
    return eq(u8, name, "steel") or eq(u8, name, "aluminum") or eq(u8, name, "flashing_membrane");
}
