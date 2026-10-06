//! Section view (SPEC 8.1): exact 2D cut / beyond / hatch with crop, break lines and dedupe.

const std = @import("std");
const cast = @import("num.zig");
const geom = @import("geom.zig");
const clip = @import("clip.zig");
const model = @import("model.zig");
const scene_mod = @import("scene.zig");
const style_mod = @import("style.zig");
const Pen = @import("pen.zig").Pen;
const view_mod = @import("view.zig");
const hatch_mod = @import("hatch.zig");
const pathclip = @import("pathclip.zig");
const pathgeom = @import("pathgeom.zig");
const drawing = @import("drawing.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Pt = geom.Pt;
const Box = geom.Box;
const Prism = model.Prism;
const breaks = @import("section/breaks.zig");
const strokes = @import("section/strokes.zig");
const Stroke = strokes.Stroke;
const dedupe = strokes.dedupe;
const chainStrokes = strokes.chainStrokes;
const occlusion = @import("section/occlusion.zig");
const Occ = occlusion.Occ;
const visiblePieces = occlusion.visiblePieces;
const visibleOpen = occlusion.visibleOpen;

pub const flat_tol = geom.flat_tol;

pub const Class = enum { cut, beyond, drop };

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

    pub fn init(a: Allocator, scene: *const scene_mod.Scene, spec: *const view_mod.ViewSpec, prisms_in: []const Prism) Allocator.Error!Section {
        const prisms = try thickenThinHardware(a, prisms_in, spec.scale);
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

    /// `which` if the style still defines that pen (a user style can delete one), else the beyond pen.
    fn penFor(self: *const Section, which: Pen) Pen {
        return if (self.style.pen(which) != null) which else .beyond;
    }

    /// Fill materials draw with the pen of their kind (steel, rebar); every other fill material draws in the beyond pen.
    fn penForRole(self: *const Section, role: style_mod.Role) Pen {
        return switch (role) {
            .steel => self.penFor(.steel),
            .rebar => self.penFor(.rebar),
            else => .beyond,
        };
    }

    pub fn layerFor(self: *const Section, pen: Pen) []const u8 {
        return self.style.layerForPen(pen);
    }

    fn addStroke(self: *Section, list: *std.ArrayList(Stroke), pts: []const Pt, closed: bool, pen: Pen, src: []const u8, embedded: bool) Allocator.Error!void {
        const w = self.style.penWidthMm(pen);
        self.seq += 1;
        try list.append(self.a, .{ .pts = pts, .closed = closed, .pen = pen, .src = src, .rank = w + (if (embedded) @as(f64, 10) else 0), .seq = self.seq });
    }

    /// Clip a path to the crop and add the pieces as strokes.
    fn addClipped(self: *Section, list: *std.ArrayList(Stroke), pts: []const Pt, closed: bool, pen: Pen, src: []const u8, embedded: bool) Allocator.Error!void {
        const pieces = try pathclip.clipPath(self.a, pts, closed, self.spec.crop);
        for (pieces) |pc| try self.addStroke(list, pc.pts, pc.closed, pen, src, embedded);
    }

    pub fn build(self: *Section) Allocator.Error!void {
        const crop = self.spec.crop;
        const margin = 1e-6;
        // gather occluders (flattened cut/beyond bodies that touch the crop)
        for (self.prisms, 0..) |p, i| {
            if (self.cls[i] == .drop or p.kind != .body or p.face_tie) continue;
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
                    const pen = self.penFor(p.pen orelse .hidden);
                    for (p.loops) |l| try self.addClipped(&self.ghosts, l, true, pen, self.srcName(p), true);
                },
                .line => {
                    if (p.line_pts.len >= 2) {
                        const pen = self.penFor(if (self.style.material(p.material)) |m| (m.pen orelse .membrane) else .membrane);
                        try self.addClipped(&self.strokes, try self.linePoints(p), false, pen, self.srcName(p), p.embedded);
                    }
                },
                .batt => {
                    if (p.line_pts.len >= 2) try self.addClipped(&self.strokes, p.line_pts, false, .profile, self.srcName(p), false);
                },
                .body => {},
            }
        }
        try self.breakLines();
    }

    /// The polyline a `line` prism (membrane) is drawn along. A vapor retarder keeps its dashed line visibly separate
    /// from the host edge it follows (0.03 paper inch at least).
    pub fn linePoints(self: *const Section, p: Prism) Allocator.Error![]const Pt {
        if (!(p.role == .vapor_retarder and p.centerline.len >= 2)) return p.line_pts;
        const c0 = p.centerline[0].v();
        const d0 = p.centerline[1].v().sub(c0).norm();
        const off = p.line_pts[0].v().sub(c0);
        const side: f64 = if (d0.perp().dot(off) >= 0) 1 else -1;
        const gap = @max(off.len(), 0.03 * self.spec.scale);
        return pathgeom.offsetOpen(self.a, p.centerline, side * gap);
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

    /// Lines of ordinary members run hidden under a face-on tie (the strap is in front of them): remove the
    /// parts of non-embedded strokes that fall inside a tie outline so the tie and its nail dots read cleanly.
    fn knockOutFaceTies(self: *Section) Allocator.Error!void {
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
    fn drawFaceTie(self: *Section, i: usize, p: Prism) Allocator.Error!void {
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

    /// Sawn or engineered lumber lying in the section plane (run x/y): cut along its length, not end-on.
    fn isLengthwiseLumber(self: *const Section, p: Prism) bool {
        if (p.quads.len > 0 or p.loops.len == 0 or p.loops[0].len < 3) return false;
        if (p.comp >= self.scene.comps.len) return false;
        return self.scene.comps[p.comp].ty.traits.lengthwise_grain;
    }

    /// Stroke only the edges whose outward normal points up (grade lines). Loops are CCW.
    fn topEdges(self: *Section, loop: []const Pt, pen: Pen, src: []const u8) Allocator.Error!void {
        const ccw = geom.signedArea(loop) >= 0;
        var cur: std.ArrayList(Pt) = .empty;
        const n = loop.len;
        // find a start vertex where the previous edge does not qualify so runs are not split
        var start: usize = 0;
        var k: usize = 0;
        while (k < n) : (k += 1) {
            if (!edgeIsTop(loop, (k + n - 1) % n, ccw)) {
                start = k;
                break;
            }
        }
        var idx: usize = 0;
        while (idx < n) : (idx += 1) {
            const i = (start + idx) % n;
            const q = loop[(i + 1) % n];
            if (edgeIsTop(loop, i, ccw)) {
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

    /// Whether the outward normal of edge `i` points up. Arcs use their chord: good enough for grade lines.
    fn edgeIsTop(loop: []const Pt, i: usize, ccw: bool) bool {
        const p = loop[i];
        const q = loop[(i + 1) % loop.len];
        const d = q.v().sub(p.v());
        const l = d.len();
        if (l < 1e-12) return false;
        const n = if (ccw) V2.init(d.y / l, -d.x / l) else V2.init(-d.y / l, d.x / l);
        return n.y > 0.01;
    }

    // ---- beyond prisms --------------------------------------------------------------------------------

    fn drawBeyond(self: *Section, i: usize, p: Prism) Allocator.Error!void {
        if (p.kind != .body) return;
        if (p.face_tie) return self.drawFaceTie(i, p);
        const crop = self.spec.crop;
        const f = try self.flatOf(i);
        if (!clip.loopsBox(f).overlaps(crop, 1e-6)) return;
        const src = self.srcName(p);
        if (isPathBar(p)) return self.addClipped(&self.strokes, p.centerline, false, self.penFor(.rebar), src, true);
        const mat = self.style.material(p.material);
        const pen: Pen = if (mat != null and mat.?.fill) self.penForRole(p.role) else .beyond;
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
        region = try clip.boolean(self.a, region, &.{self.crop_loop}, .intersect);
        if (self.cls[i] == .beyond and !p.embedded and !p.dashed) {
            for (self.occs.items) |o| {
                if (o.prism == i) continue;
                if (!(o.cut or o.z1 > p.z1 + 1e-9)) continue;
                if (!o.box.overlaps(clip.loopsBox(region), 1e-9)) continue;
                region = try clip.boolean(self.a, region, o.loops, .diff);
            }
        }
        return region;
    }

    pub const breakLines = breaks.breakLines;
    pub const breakPolyline = breaks.breakPolyline;

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
        try self.knockOutFaceTies();
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

/// Minimum drawn thickness of sheet metal (straps, flashing) in paper inches: about 0.55 mm, so a CS16 strap is a visible band
/// next to the host member's 0.5 mm outline instead of a hairline merged with it. The true thickness lives in the 3D model.
pub const min_metal_paper_in: f64 = 0.022;

/// SPEC 20 minimum visibility: thin edge-lay sheet metal (a ribbon along a centerline) is widened, on the side(s) it already
/// occupies, to at least `min_metal_paper_in` on paper.
fn thickenThinHardware(a: Allocator, prisms: []const Prism, scale: f64) Allocator.Error![]const Prism {
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

// ---- SPEC 20 drawing conventions ----------------------------------------------------------------------------------

fn testDrawing(a: Allocator, src: []const u8, view_id: []const u8) !drawing.Drawing {
    const json = @import("json.zig");
    const model_ = @import("model.zig");
    const drawview = @import("drawview.zig");
    var err: json.ParseError = undefined;
    const doc = (try json.parse(a, src, &err)).?;
    const st = try a.create(style_mod.Style);
    st.* = try style_mod.load(a, null);
    var diags = model_.Diags.init(a);
    return (try drawview.build(a, doc, st, view_id, &diags)).?;
}

test "path rebar is one open centerline polyline in the rebar pen with true arcs and no fill" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const dr = try testDrawing(a, @import("testdocs.zig").palmer, "A");
    var arcs: usize = 0;
    var paths: usize = 0;
    for (dr.items) |it| switch (it) {
        .path => |p| if (std.mem.eql(u8, p.src, "dowel")) {
            paths += 1;
            try std.testing.expect(!p.closed);
            try std.testing.expectEqual(Pen.rebar, p.pen);
            try std.testing.expectEqualStrings("S-DETL-REBR", p.layer);
            for (p.pts[0 .. p.pts.len - 1]) |q| if (q.b != 0) {
                arcs += 1;
            };
        },
        .fill => |f| try std.testing.expect(!std.mem.eql(u8, f.src, "dowel")),
        else => {},
    };
    try std.testing.expectEqual(@as(usize, 1), paths);
    try std.testing.expect(arcs >= 1);
}

test "a face-on tie is outlined in the steel pen with nail dots at 1 inch pitch" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const dr = try testDrawing(a, @import("testdocs.zig").truss, "A");
    var dots: usize = 0;
    var outline: usize = 0;
    for (dr.items) |it| switch (it) {
        .fill => |f| if (std.mem.eql(u8, f.src, "hurricane_tie")) {
            dots += 1;
        },
        .path => |p| if (std.mem.eql(u8, p.src, "hurricane_tie")) {
            outline += 1;
            try std.testing.expectEqual(Pen.steel, p.pen);
        },
        else => {},
    };
    try std.testing.expect(outline >= 1);
    try std.testing.expect(dots >= 3 and dots <= 5); // 4.4" strap, 1" pitch
}

test "edge-lay straps are never thinner than the minimum metal thickness on paper" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const dr = try testDrawing(a, @import("testdocs.zig").beam, "A");
    var seen = false;
    for (dr.items) |it| switch (it) {
        .region => |r| if (std.mem.eql(u8, r.src, "strap")) {
            seen = true;
            const bb = geom.pointsBox(r.loops[0]);
            try std.testing.expect(@min(bb.width(), bb.height()) * 1.0 / dr.scale >= min_metal_paper_in - 1e-6);
        },
        else => {},
    };
    try std.testing.expect(seen);
}

test "all member linework in every reference section view stays inside the crop" {
    const json = @import("json.zig");
    const testdocs = @import("testdocs.zig");
    for (testdocs.layout_docs) |src| {
        var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
        defer arena.deinit();
        const a = arena.allocator();
        var err: json.ParseError = undefined;
        const doc = (try json.parse(a, src, &err)).?;
        for (doc.get("views").?.array) |v| {
            if (!std.mem.eql(u8, v.get("kind").?.str().?, "section")) continue;
            const dr = try testDrawing(a, src, v.get("id").?.str().?);
            const c = dr.crop;
            const tol = 1e-6;
            for (dr.items) |it| {
                const pts = switch (it) {
                    .path => |p| if (isMemberPen(p.pen) and !std.mem.eql(u8, p.src, "crop")) p.pts else continue,
                    else => continue,
                };
                for (pts) |q| {
                    try std.testing.expect(q.x >= c.x0 - tol and q.x <= c.x1 + tol and q.y >= c.y0 - tol and q.y <= c.y1 + tol);
                }
            }
        }
    }
}

fn isMemberPen(pen: Pen) bool {
    for ([_]Pen{ .cut, .beyond, .hidden, .steel, .rebar, .membrane, .vapor }) |n| if (pen == n) return true;
    return false;
}
