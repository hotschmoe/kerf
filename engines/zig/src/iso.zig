//! Iso view with hidden-line removal on planar faces (SPEC 8.2).
//!
//! Projection (isometric drawing, axes at true length): for camera quadrant (sx, sz)
//!   u = (sz*x - sx*z) * cos30,   v = y - (sx*x + sz*z) * sin30,   depth = sx*x + y + sz*z
//! (larger depth = nearer the viewer). Prisms are tessellated into planar faces (two caps and one
//! side face per profile segment); edges are tested against front-facing faces on a uniform grid.

const std = @import("std");
const cast = @import("num.zig");
const geom = @import("geom.zig");
const clip = @import("clip.zig");
const model = @import("model.zig");
const scene_mod = @import("scene.zig");
const style_mod = @import("style.zig");
const shape_mod = @import("shape.zig");
const Pen = @import("pen.zig").Pen;
const view_mod = @import("view.zig");
const hatch_mod = @import("hatch.zig");
const drawing = @import("drawing.zig");
const compile_mod = @import("compile.zig");
const mesh_mod = @import("mesh.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Pt = geom.Pt;
const Box = geom.Box;

const C30: f64 = 0.8660254037844386;
const S30: f64 = 0.5;

fn proj(sx: f64, sz: f64, p: [3]f64) V2 {
    return V2.init((sz * p[0] - sx * p[2]) * C30, p[1] - (sx * p[0] + sz * p[2]) * S30);
}

fn depthOf(sx: f64, sz: f64, p: [3]f64) f64 {
    return sx * p[0] + p[1] + sz * p[2];
}

const Face = struct {
    n: [3]f64,
    p0: [3]f64,
    outer: []const V2,
    holes: []const []const V2,
    bbox: Box,
    prism: usize,
    front: bool,
    nd: f64,
};

const Edge = struct {
    a: [3]f64,
    b: [3]f64,
    pen: Pen,
    prism: usize,
    f1: usize,
    f2: usize,
    chain: usize,
};

const IsoPrism = struct {
    src: []const u8,
    instance: u32 = 0,
    comp: u32,
    part: []const u8,
    loops: []const []const V2,
    z0: f64,
    z1: f64,
    cap_cut: bool,
    pen: ?Pen,
    material: []const u8,
    embedded: bool,
    is_fill: bool = false,
    outline: model.OutlineMode = .full,
};

pub const Vis = struct {
    comp: u32,
    part: []const u8,
    instance: u32,
    shapes: []const shape_mod.Shape,
    src: []const u8 = "",
    cut: bool = false,
};

pub const Iso = struct {
    a: Allocator,
    sx: f64 = 1,
    sz: f64 = 1,
    cut_z: f64 = 0,
    faces: std.ArrayList(Face) = .empty,
    edges: std.ArrayList(Edge) = .empty,
    prisms: std.ArrayList(IsoPrism) = .empty,
    vis: std.ArrayList(Vis) = .empty,
    // grid
    gx0: f64 = 0,
    gy0: f64 = 0,
    gw: f64 = 1,
    gh: f64 = 1,
    gn: usize = 0,
    cells: [][]u32 = &.{},
    eps: f64 = 1e-3,

    /// Label point of the visible faces of `comp` (null: nothing of it is visible). Out of memory is an error, not "not
    /// visible" (REVIEW LAY-3: it used to surface as W_NOTE_TARGET).
    pub fn landing(self: *Iso, comp: *const scene_mod.Comp, inst: ?u32, part: ?[]const u8) Allocator.Error!?V2 {
        var shapes: std.ArrayList(shape_mod.Shape) = .empty;
        for (self.vis.items) |v| {
            if (v.comp != comp.index) continue;
            if (inst) |k| if (v.instance != k) continue;
            if (part) |p| if (!std.mem.eql(u8, v.part, p)) continue;
            try shapes.appendSlice(self.a, v.shapes);
        }
        return shape_mod.labelPoint(self.a, shapes.items);
    }

    /// Non-drawn picking regions: the visible face chosen per prism (cut cap first).
    pub fn regionItems(self: *Iso) Allocator.Error![]const drawing.Item {
        var out: std.ArrayList(drawing.Item) = .empty;
        for (self.vis.items) |v| {
            for (v.shapes) |sh| {
                const loops = try self.a.alloc([]const Pt, 1 + sh.holes.len);
                const outer = try self.a.alloc(Pt, sh.outer.len);
                for (sh.outer, 0..) |q, i| outer[i] = Pt.at(q, 0);
                loops[0] = outer;
                for (sh.holes, 0..) |h, k| {
                    const hp = try self.a.alloc(Pt, h.len);
                    for (h, 0..) |q, i| hp[i] = Pt.at(q, 0);
                    loops[1 + k] = hp;
                }
                try out.append(self.a, .{ .region = .{ .src = v.src, .part = if (v.part.len > 0) v.part else null, .instance = v.instance, .cut = v.cut, .loops = loops } });
            }
        }
        return out.items;
    }

    pub fn project(self: *Iso, p: V2) ?V2 {
        return proj(self.sx, self.sz, .{ p.x, p.y, self.cut_z });
    }

    fn cellRange(self: *const Iso, b: Box) [4]usize {
        const gn = self.gn;
        const cl = struct {
            fn f(v: f64, o: f64, w: f64, g: usize) usize {
                return cast.toIntClamped(usize, @floor((v - o) / w), 0, g -| 1);
            }
        }.f;
        return .{ cl(b.x0, self.gx0, self.gw, gn), cl(b.x1, self.gx0, self.gw, gn), cl(b.y0, self.gy0, self.gh, gn), cl(b.y1, self.gy0, self.gh, gn) };
    }

    fn buildGrid(self: *Iso) Allocator.Error!void {
        var b = Box{};
        for (self.faces.items) |f| b.addBox(f.bbox);
        if (b.isEmpty()) return;
        self.gn = 24;
        self.gx0 = b.x0;
        self.gy0 = b.y0;
        self.gw = @max(b.width() / 24.0, 1e-6);
        self.gh = @max(b.height() / 24.0, 1e-6);
        const lists = try self.a.alloc(std.ArrayList(u32), 24 * 24);
        for (lists) |*l| l.* = .empty;
        for (self.faces.items, 0..) |f, i| {
            if (!f.front or @abs(f.nd) < 1e-9) continue;
            const r = self.cellRange(f.bbox);
            var row = r[2];
            while (row <= r[3]) : (row += 1) {
                var col = r[0];
                while (col <= r[1]) : (col += 1) try lists[row * 24 + col].append(self.a, @intCast(i));
            }
        }
        const cells = try self.a.alloc([]u32, lists.len);
        for (lists, 0..) |l, i| cells[i] = l.items;
        self.cells = cells;
    }

    fn candidates(self: *const Iso, b: Box, out: *std.ArrayList(u32)) Allocator.Error!void {
        out.clearRetainingCapacity();
        if (self.cells.len == 0) return;
        const r = self.cellRange(b);
        var row = r[2];
        while (row <= r[3]) : (row += 1) {
            var col = r[0];
            while (col <= r[1]) : (col += 1) try out.appendSlice(self.a, self.cells[row * self.gn + col]);
        }
        std.mem.sort(u32, out.items, {}, std.sort.asc(u32));
        var k: usize = 0;
        for (out.items, 0..) |v, i| {
            if (i == 0 or v != out.items[i - 1]) {
                out.items[k] = v;
                k += 1;
            }
        }
        out.shrinkRetainingCapacity(k);
    }

    fn faceDepth(self: *const Iso, f: Face, m: V2) f64 {
        const sx = self.sx;
        const sz = self.sz;
        const uu = m.x / C30;
        const vv = -m.y / S30;
        const x = (sz * uu + sx * vv) * 0.5;
        const z = (-sx * uu + sz * vv) * 0.5;
        const pb = [3]f64{ x, 0, z };
        const d = [3]f64{ sx, 1, sz };
        const t = (f.n[0] * (f.p0[0] - pb[0]) + f.n[1] * (f.p0[1] - pb[1]) + f.n[2] * (f.p0[2] - pb[2])) / f.nd;
        const p = [3]f64{ pb[0] + t * d[0], pb[1] + t * d[1], pb[2] + t * d[2] };
        return depthOf(sx, sz, p);
    }

    fn faceContains(f: Face, m: V2) bool {
        if (!f.bbox.contains(m)) return false;
        if (!geom.pointInLoopEO(m, f.outer)) return false;
        for (f.holes) |h| if (geom.pointInLoopEO(m, h)) return false;
        return true;
    }

    fn occludedAt(self: *const Iso, m: V2, dm: f64, skip: []const usize, cand: []const u32) bool {
        for (cand) |fi| {
            if (std.mem.indexOfScalar(usize, skip, fi) != null) continue;
            const f = self.faces.items[fi];
            if (faceContains(f, m) and self.faceDepth(f, m) > dm + self.eps) return true;
        }
        return false;
    }

    /// Visible parameter intervals of the screen segment a->b whose depth runs da -> db.
    fn visible(self: *const Iso, a: V2, b: V2, da: f64, db: f64, skip: []const usize) Allocator.Error![]const [2]f64 {
        const al = self.a;
        var bb = Box{};
        bb.addPoint(a.x, a.y);
        bb.addPoint(b.x, b.y);
        bb = bb.expand(1e-9);
        var cand: std.ArrayList(u32) = .empty;
        try self.candidates(bb, &cand);
        var keep: std.ArrayList(u32) = .empty;
        for (cand.items) |fi| {
            if (std.mem.indexOfScalar(usize, skip, fi) != null) continue;
            if (!self.faces.items[fi].bbox.overlaps(bb, 0)) continue;
            try keep.append(al, fi);
        }
        var out: std.ArrayList([2]f64) = .empty;
        if (keep.items.len == 0) {
            try out.append(al, .{ 0, 1 });
            return out.items;
        }
        var ts: std.ArrayList(f64) = .empty;
        try ts.append(al, 0);
        try ts.append(al, 1);
        for (keep.items) |fi| {
            const f = self.faces.items[fi];
            try addContourCuts(al, &ts, a, b, f.outer);
            for (f.holes) |h| try addContourCuts(al, &ts, a, b, h);
        }
        std.mem.sort(f64, ts.items, {}, std.sort.asc(f64));
        var i: usize = 0;
        while (i + 1 < ts.items.len) : (i += 1) {
            const t0 = ts.items[i];
            const t1 = ts.items[i + 1];
            if (t1 - t0 < 1e-9) continue;
            const tm = (t0 + t1) * 0.5;
            const m = V2.lerp(a, b, tm);
            const dm = da + (db - da) * tm;
            if (self.occludedAt(m, dm, skip, keep.items)) continue;
            if (out.items.len > 0 and @abs(out.items[out.items.len - 1][1] - t0) < 1e-9) {
                out.items[out.items.len - 1][1] = t1;
            } else try out.append(al, .{ t0, t1 });
        }
        return out.items;
    }
};

fn addContourCuts(a: Allocator, ts: *std.ArrayList(f64), p: V2, q: V2, contour: []const V2) Allocator.Error!void {
    for (contour, 0..) |c0, i| {
        const c1 = contour[(i + 1) % contour.len];
        var ta: [2]f64 = undefined;
        var tb: [2]f64 = undefined;
        const n = geom.segSeg(p, q, c0, c1, &ta, &tb);
        for (0..n) |k| {
            if (ta[k] > 1e-9 and ta[k] < 1 - 1e-9) try ts.append(a, ta[k]);
        }
    }
}

// ---- tessellation ---------------------------------------------------------------------------------------

const FLoop = struct { pts: []const V2, smooth: []const bool };

fn flattenSharp(a: Allocator, loop: []const Pt) Allocator.Error!FLoop {
    var pts: std.ArrayList(V2) = .empty;
    for (loop, 0..) |p, i| {
        const q = loop[(i + 1) % loop.len];
        try pts.append(a, p.v());
        if (p.b != 0) {
            const arc = geom.arcOf(p.v(), q.v(), p.b);
            const by_tol = if (arc.r > 0.004) 2.0 * std.math.acos(1.0 - 0.004 / arc.r) else 1.0;
            const n = geom.stepsForSweep(arc.sweep, if (by_tol > 0) @min(0.22, by_tol) else 0.22);
            var k: usize = 1;
            while (k < n) : (k += 1) try pts.append(a, arc.at(@as(f64, @floatFromInt(k)) / @as(f64, @floatFromInt(n))));
        }
    }
    // smoothness at each vertex: small turning angle
    const n = pts.items.len;
    const sm = try a.alloc(bool, n);
    for (0..n) |i| {
        const prev = pts.items[(i + n - 1) % n];
        const cur = pts.items[i];
        const next = pts.items[(i + 1) % n];
        const u = cur.sub(prev);
        const w = next.sub(cur);
        const ang = @abs(std.math.atan2(u.cross(w), u.dot(w)));
        sm[i] = ang < std.math.degreesToRadians(20.0);
    }
    return .{ .pts = pts.items, .smooth = sm };
}

fn quadrant(from: view_mod.From) [2]f64 {
    return switch (from) {
        .front_right => .{ 1, 1 },
        .front_left => .{ -1, 1 },
        .back_right => .{ 1, -1 },
        .back_left => .{ -1, -1 },
    };
}

fn srcOf(a: Allocator, c: *const scene_mod.Comp, instance: u32) Allocator.Error![]const u8 {
    if (c.arr_count > 1) return a.print("{s}#{d}", .{ c.id, instance / @as(u32, @intCast(c.xfs.len / c.arr_count)) });
    return c.id;
}

fn gather(a: Allocator, scene: *const scene_mod.Scene, spec: *const view_mod.ViewSpec, crop: ?Box, out: *std.ArrayList(IsoPrism)) Allocator.Error!void {
    var crop_loop: ?[]const V2 = null;
    if (crop) |c| crop_loop = try a.dupe(V2, &.{ V2.init(c.x0, c.y0), V2.init(c.x1, c.y0), V2.init(c.x1, c.y1), V2.init(c.x0, c.y1) });
    for (scene.comps) |*c| {
        if (c.state != .ok or !c.visible) continue;
        if (compile_mod.isOmitted(spec.omit, c.id)) continue;
        for (c.world) |p| {
            if (p.kind == .ghost or p.role == .void) continue;
            const fill_mat = p.role == .soil;
            if (fill_mat and !spec.cutaway) continue;
            var segs: []const mesh_mod.ZSeg = &.{.{ .z0 = p.z0, .z1 = p.z1, .mortar = false }};
            if (p.cmu_unit and p.role != .grout) segs = try mesh_mod.cmuSplit(a, p);
            for (segs) |sg| {
                const mat_name: []const u8 = if (sg.mortar) "mortar" else p.material;
                const z0 = sg.z0;
                var z1 = sg.z1;
                var cap = false;
                if (spec.cutaway) {
                    if (z0 >= spec.cut_z) continue;
                    if (z1 > spec.cut_z) {
                        z1 = spec.cut_z;
                        cap = true;
                    }
                }
                if (z1 - z0 < 1e-9) continue;
                if (fill_mat and !cap) continue;
                var region = try clip.fromRegion(a, p.loops, 0.003);
                if (crop_loop) |cl| region = try clip.boolean(a, region, &.{cl}, .intersect);
                // group into shapes (outer + holes) and add one prism per outer loop
                var group_outers: std.ArrayList(usize) = .empty;
                for (region, 0..) |l, i| if (geom.signedAreaV(l) > 0) try group_outers.append(a, i);
                for (group_outers.items) |oi| {
                    var loops: std.ArrayList([]const V2) = .empty;
                    try loops.append(a, region[oi]);
                    for (region) |h| if (geom.signedAreaV(h) < 0 and h.len > 0 and geom.pointInLoopEO(h[0], region[oi])) try loops.append(a, h);
                    try out.append(a, .{
                        .src = try srcOf(a, c, p.instance),
                        .instance = p.instance,
                        .comp = c.index,
                        .part = p.part,
                        .loops = loops.items,
                        .z0 = z0,
                        .z1 = z1,
                        .cap_cut = cap,
                        .pen = if (p.kind == .ghost) p.pen else null,
                        .material = mat_name,
                        .embedded = p.embedded,
                        .is_fill = fill_mat,
                        .outline = p.outline,
                    });
                }
            }
        }
    }
}

fn mkFace(iso: *Iso, n: [3]f64, verts: []const [3]f64, holes3: []const []const [3]f64, pi: usize) Allocator.Error!usize {
    const a = iso.a;
    const outer = try a.alloc(V2, verts.len);
    var bb = Box{};
    for (verts, 0..) |v, i| {
        outer[i] = proj(iso.sx, iso.sz, v);
        bb.addPoint(outer[i].x, outer[i].y);
    }
    const holes = try a.alloc([]const V2, holes3.len);
    for (holes3, 0..) |h, k| {
        const hp = try a.alloc(V2, h.len);
        for (h, 0..) |v, i| hp[i] = proj(iso.sx, iso.sz, v);
        holes[k] = hp;
    }
    const nd = n[0] * iso.sx + n[1] + n[2] * iso.sz;
    try iso.faces.append(a, .{ .n = n, .p0 = verts[0], .outer = outer, .holes = holes, .bbox = bb, .prism = pi, .front = nd > 1e-9, .nd = nd });
    return iso.faces.items.len - 1;
}

fn addPrism(iso: *Iso, pi: usize) Allocator.Error!void {
    const a = iso.a;
    const ip = iso.prisms.items[pi];
    var loops: std.ArrayList(FLoop) = .empty;
    for (ip.loops) |l| {
        const pts_in = try a.alloc(Pt, l.len);
        for (l, 0..) |v, i| pts_in[i] = Pt.at(v, 0);
        try loops.append(a, try flattenSharp(a, pts_in));
    }
    // caps
    const outer = loops.items[0].pts;
    var top_v = try a.alloc([3]f64, outer.len);
    var bot_v = try a.alloc([3]f64, outer.len);
    for (outer, 0..) |p, i| {
        top_v[i] = .{ p.x, p.y, ip.z1 };
        bot_v[i] = .{ p.x, p.y, ip.z0 };
    }
    const top_h = try a.alloc([]const [3]f64, loops.items.len - 1);
    const bot_h = try a.alloc([]const [3]f64, loops.items.len - 1);
    for (loops.items[1..], 0..) |fl, k| {
        const th = try a.alloc([3]f64, fl.pts.len);
        const bh = try a.alloc([3]f64, fl.pts.len);
        for (fl.pts, 0..) |p, i| {
            th[i] = .{ p.x, p.y, ip.z1 };
            bh[i] = .{ p.x, p.y, ip.z0 };
        }
        top_h[k] = th;
        bot_h[k] = bh;
    }
    const ftop = try mkFace(iso, .{ 0, 0, 1 }, top_v, top_h, pi);
    const fbot = try mkFace(iso, .{ 0, 0, -1 }, bot_v, bot_h, pi);
    for (loops.items, 0..) |fl, li| {
        const n = fl.pts.len;
        const side_faces = try a.alloc(usize, n);
        for (0..n) |i| {
            const p = fl.pts[i];
            const q = fl.pts[(i + 1) % n];
            const d = q.sub(p).norm();
            const nn = [3]f64{ d.y, -d.x, 0 };
            const verts = [4][3]f64{ .{ p.x, p.y, ip.z0 }, .{ q.x, q.y, ip.z0 }, .{ q.x, q.y, ip.z1 }, .{ p.x, p.y, ip.z1 } };
            side_faces[i] = try mkFace(iso, nn, &verts, &.{}, pi);
        }
        const chain = pi * 16 + li;
        for (0..n) |i| {
            const p = fl.pts[i];
            const q = fl.pts[(i + 1) % n];
            const sf = side_faces[i];
            const top_pen = penOf(iso, ip, ftop, sf, ip.cap_cut);
            try iso.edges.append(a, .{ .a = .{ p.x, p.y, ip.z1 }, .b = .{ q.x, q.y, ip.z1 }, .pen = top_pen, .prism = pi, .f1 = ftop, .f2 = sf, .chain = chain });
            const bot_pen = penOf(iso, ip, fbot, sf, false);
            try iso.edges.append(a, .{ .a = .{ p.x, p.y, ip.z0 }, .b = .{ q.x, q.y, ip.z0 }, .pen = bot_pen, .prism = pi, .f1 = fbot, .f2 = sf, .chain = chain + 8 });
        }
        for (0..n) |i| {
            const p = fl.pts[i];
            const prev = side_faces[(i + n - 1) % n];
            const cur = side_faces[i];
            const fp = iso.faces.items[prev].front;
            const fc = iso.faces.items[cur].front;
            // smooth vertical edges only where facing differs (silhouettes)
            if (fl.smooth[i] and fp == fc) continue;
            const pen: Pen = if (ip.pen) |x| x else if (fp != fc) .profile else .beyond;
            try iso.edges.append(a, .{ .a = .{ p.x, p.y, ip.z0 }, .b = .{ p.x, p.y, ip.z1 }, .pen = pen, .prism = pi, .f1 = prev, .f2 = cur, .chain = std.math.maxInt(usize) - (pi * 4096 + li * 1024 + i) });
        }
    }
}

fn penOf(iso: *const Iso, ip: IsoPrism, f1: usize, f2: usize, cut_cap_edge: bool) Pen {
    if (ip.pen) |p| return p;
    if (cut_cap_edge) return .cut;
    return if (iso.faces.items[f1].front != iso.faces.items[f2].front) .profile else .beyond;
}

pub const Result = struct {
    items: []const drawing.Item,
    scale: f64,
    crop: Box,
};

/// Build the iso view. `iso` is initialised here and stays alive for annotation landing/projection.
pub fn build(iso: *Iso, scene: *const scene_mod.Scene, spec: *const view_mod.ViewSpec, style: *const style_mod.Style, scale_override: f64) Allocator.Error!Result {
    const a = iso.a;
    const q = quadrant(spec.from);
    iso.sx = q[0];
    iso.sz = q[1];
    iso.cut_z = spec.cut_z;
    var crop: ?Box = if (spec.has_crop) spec.crop else null;
    if (crop == null) {
        var b = Box{};
        for (scene.comps) |c| if (c.state == .ok and c.visible) for (c.world) |p| for (p.loops) |l| b.addBox(geom.loopBox(l));
        if (!b.isEmpty()) crop = b;
    }
    try gather(a, scene, spec, crop, &iso.prisms);
    for (0..iso.prisms.items.len) |i| {
        if (iso.prisms.items[i].is_fill) continue;
        try addPrism(iso, i);
    }
    var sb = Box{};
    for (iso.faces.items) |f| sb.addBox(f.bbox);
    if (sb.isEmpty()) return .{ .items = &.{}, .scale = 12, .crop = .{ .x0 = 0, .y0 = 0, .x1 = 12, .y1 = 12 } };
    iso.eps = 1e-4 * @max(@max(sb.width(), sb.height()), 1.0);
    try iso.buildGrid();
    const s: f64 = if (scale_override > 0) scale_override else if (spec.scale > 0) spec.scale else @max(@ceil(@max(sb.width(), sb.height()) / 5.5 * 2.0) / 2.0, 0.5);
    const sx = iso.sx;
    const sz = iso.sz;

    // dedupe identical 3D edges (heavier pen wins)
    const Key = [6]i64;
    const keyOf = struct {
        fn r(x: f64) i64 {
            return cast.toIntClamped(i64, @round(x * 2000.0), std.math.minInt(i64), std.math.maxInt(i64));
        }
        fn f(ea: [3]f64, eb: [3]f64) Key {
            const ka = [3]i64{ r(ea[0]), r(ea[1]), r(ea[2]) };
            const kb = [3]i64{ r(eb[0]), r(eb[1]), r(eb[2]) };
            const swap = (ka[0] > kb[0]) or (ka[0] == kb[0] and (ka[1] > kb[1] or (ka[1] == kb[1] and ka[2] > kb[2])));
            return if (swap) .{ kb[0], kb[1], kb[2], ka[0], ka[1], ka[2] } else .{ ka[0], ka[1], ka[2], kb[0], kb[1], kb[2] };
        }
    }.f;
    const ne = iso.edges.items.len;
    const keep = try a.alloc(bool, ne);
    @memset(keep, true);
    {
        const Ent = struct { key: Key, idx: usize };
        var ents = try a.alloc(Ent, ne);
        for (iso.edges.items, 0..) |e, i| ents[i] = .{ .key = keyOf(e.a, e.b), .idx = i };
        std.mem.sort(Ent, ents, {}, struct {
            fn lt(_: void, x: Ent, y: Ent) bool {
                return switch (std.mem.order(i64, &x.key, &y.key)) {
                    .lt => true,
                    .gt => false,
                    .eq => x.idx < y.idx,
                };
            }
        }.lt);
        var i: usize = 0;
        while (i < ne) {
            var j = i + 1;
            while (j < ne and std.mem.eql(i64, &ents[i].key, &ents[j].key)) : (j += 1) {}
            // group [i, j): keep the heaviest pen, earliest on ties
            var best = ents[i].idx;
            for (ents[i + 1 .. j]) |en| {
                if (style.penWidthMm(iso.edges.items[en.idx].pen) > style.penWidthMm(iso.edges.items[best].pen)) best = en.idx;
            }
            for (ents[i..j]) |en| if (en.idx != best) {
                keep[en.idx] = false;
            };
            i = j;
        }
    }

    const Piece = struct { edge: usize, segs: []const [2]V2 };
    var pieces: std.ArrayList(Piece) = .empty;
    for (iso.edges.items, 0..) |e, ei| {
        if (!keep[ei]) continue;
        const f1 = iso.faces.items[e.f1];
        const f2 = iso.faces.items[e.f2];
        if (!f1.front and !f2.front) continue;
        const pa = proj(sx, sz, e.a);
        const pb = proj(sx, sz, e.b);
        if (pa.dist(pb) < 1e-9) continue;
        const vis = try iso.visible(pa, pb, depthOf(sx, sz, e.a), depthOf(sx, sz, e.b), &.{ e.f1, e.f2 });
        if (vis.len == 0) continue;
        const segs = try a.alloc([2]V2, vis.len);
        for (vis, 0..) |v, k| segs[k] = .{ V2.lerp(pa, pb, v[0]), V2.lerp(pa, pb, v[1]) };
        try pieces.append(a, .{ .edge = ei, .segs = segs });
    }

    var items: std.ArrayList(drawing.Item) = .empty;
    // hatch on cutaway caps (front-facing only)
    if (spec.cutaway and sz > 0) {
        var cand: std.ArrayList(u32) = .empty;
        for (iso.prisms.items) |ip| {
            if (!ip.cap_cut or ip.embedded) continue;
            const mat = style.material(ip.material) orelse continue;
            for (mat.hatch) |hs| {
                const pat = style.pattern(hs.pattern) orelse continue;
                const res = try hatch_mod.generate(a, ip.loops, pat, s * hs.scale, hs.angle);
                var out_lines: std.ArrayList([4]f64) = .empty;
                for (res.lines) |l| {
                    const pa3 = [3]f64{ l[0], l[1], ip.z1 };
                    const pb3 = [3]f64{ l[2], l[3], ip.z1 };
                    const pa = proj(sx, sz, pa3);
                    const pb = proj(sx, sz, pb3);
                    const da = depthOf(sx, sz, pa3);
                    const db = depthOf(sx, sz, pb3);
                    if (V2.eql(pa, pb, 1e-12)) {
                        var bb = Box{};
                        bb.addPoint(pa.x, pa.y);
                        try iso.candidates(bb.expand(1e-9), &cand);
                        if (!iso.occludedAt(pa, da, &.{}, cand.items)) try out_lines.append(a, .{ pa.x, pa.y, pa.x, pa.y });
                        continue;
                    }
                    for (try iso.visible(pa, pb, da, db, &.{})) |v| {
                        const p0 = V2.lerp(pa, pb, v[0]);
                        const p1 = V2.lerp(pa, pb, v[1]);
                        try out_lines.append(a, .{ p0.x, p0.y, p1.x, p1.y });
                    }
                }
                const loops = try a.alloc([]const Pt, ip.loops.len);
                for (ip.loops, 0..) |l, k| {
                    const pl = try a.alloc(Pt, l.len);
                    for (l, 0..) |v, i| {
                        const r = proj(sx, sz, .{ v.x, v.y, ip.z1 });
                        pl[i] = Pt.at(r, 0);
                    }
                    loops[k] = pl;
                }
                try items.append(a, .{ .hatch = .{ .layer = style.layerForPen(.hatch), .pen = .hatch, .src = ip.src, .pattern = hs.pattern, .scale = hs.scale, .angle = hs.angle, .loops = loops, .lines = out_lines.items } });
            }
            if (ip.is_fill and ip.outline != .none) try fillOutline(a, &items, ip, style, sx, sz);
        }
    }

    // chain visible pieces into paths, lighter pens first
    var pens: std.ArrayList(Pen) = .empty;
    for (pieces.items) |pc| {
        const pen = iso.edges.items[pc.edge].pen;
        var found = false;
        for (pens.items) |p| if (p == pen) {
            found = true;
        };
        if (!found) try pens.append(a, pen);
    }
    std.mem.sort(Pen, pens.items, style, struct {
        fn lt(st: *const style_mod.Style, x: Pen, y: Pen) bool {
            const wx = st.penWidthMm(x);
            const wy = st.penWidthMm(y);
            if (wx != wy) return wx < wy;
            return std.mem.lessThan(u8, @tagName(x), @tagName(y));
        }
    }.lt);
    for (pens.items) |pen| {
        var cur: std.ArrayList(Pt) = .empty;
        var cur_src: []const u8 = "";
        var cur_chain: usize = std.math.maxInt(usize);
        for (pieces.items) |pc| {
            const e = iso.edges.items[pc.edge];
            if (e.pen != pen) continue;
            const src = iso.prisms.items[e.prism].src;
            for (pc.segs) |sg| {
                const continues = cur_chain == e.chain and std.mem.eql(u8, cur_src, src) and cur.items.len > 0 and V2.eql(cur.items[cur.items.len - 1].v(), sg[0], 1e-7);
                if (!continues) {
                    if (cur.items.len >= 2) try items.append(a, .{ .path = .{ .layer = style.layerForPen(pen), .pen = pen, .src = cur_src, .closed = false, .pts = cur.items } });
                    cur = .empty;
                    try cur.append(a, Pt.at(sg[0], 0));
                }
                try cur.append(a, Pt.at(sg[1], 0));
                cur_src = src;
                cur_chain = e.chain;
            }
        }
        if (cur.items.len >= 2) try items.append(a, .{ .path = .{ .layer = style.layerForPen(pen), .pen = pen, .src = cur_src, .closed = false, .pts = cur.items } });
    }

    // visible shapes per prism for note landing points
    var cand: std.ArrayList(u32) = .empty;
    for (iso.prisms.items, 0..) |ip, pi| {
        if (ip.is_fill) {
            var fs: std.ArrayList(shape_mod.Shape) = .empty;
            const lp = try a.alloc(V2, ip.loops[0].len);
            for (ip.loops[0], 0..) |v, i| lp[i] = proj(sx, sz, .{ v.x, v.y, ip.z1 });
            try fs.append(a, .{ .outer = lp, .holes = &.{} });
            var fi: u32 = 0;
            fi = ip.instance;
            try iso.vis.append(a, .{ .comp = ip.comp, .part = ip.part, .instance = fi, .shapes = fs.items, .src = ip.src, .cut = ip.cap_cut });
            continue;
        }
        var faces: std.ArrayList(usize) = .empty;
        for (iso.faces.items, 0..) |f, i| if (f.prism == pi and f.front) try faces.append(a, i);
        const Ctx = struct { iso: *const Iso, cap: bool };
        std.mem.sort(usize, faces.items, Ctx{ .iso = iso, .cap = ip.cap_cut }, struct {
            fn lt(c: Ctx, x: usize, y: usize) bool {
                const fx = c.iso.faces.items[x];
                const fy = c.iso.faces.items[y];
                const cx: i32 = @intFromBool(fx.n[2] > 0.5 and c.cap);
                const cy: i32 = @intFromBool(fy.n[2] > 0.5 and c.cap);
                if (cx != cy) return cx > cy;
                return @abs(geom.signedAreaV(fx.outer)) > @abs(geom.signedAreaV(fy.outer));
            }
        }.lt);
        var shapes: std.ArrayList(shape_mod.Shape) = .empty;
        for (faces.items) |fi| {
            const f = iso.faces.items[fi];
            const sh = shape_mod.Shape{ .outer = f.outer, .holes = f.holes };
            if (try shape_mod.labelPoint(a, &.{sh})) |lp| {
                var bb = Box{};
                bb.addPoint(lp.x, lp.y);
                try iso.candidates(bb.expand(1e-9), &cand);
                if (!iso.occludedAt(lp, iso.faceDepth(f, lp), &.{fi}, cand.items)) {
                    try shapes.append(a, sh);
                    break;
                }
            }
        }
        if (shapes.items.len == 0) {
            fb: for (faces.items[0..@min(faces.items.len, 6)]) |fi| {
                const f = iso.faces.items[fi];
                const c = geom.centroidV(f.outer);
                var best: ?V2 = null;
                var bd: f64 = std.math.inf(f64);
                for (0..7) |gx| for (0..7) |gy| {
                    const qp = V2.init(f.bbox.x0 + f.bbox.width() * (@as(f64, @floatFromInt(gx)) + 0.5) / 7.0, f.bbox.y0 + f.bbox.height() * (@as(f64, @floatFromInt(gy)) + 0.5) / 7.0);
                    if (!Iso.faceContains(f, qp)) continue;
                    var bb = Box{};
                    bb.addPoint(qp.x, qp.y);
                    try iso.candidates(bb.expand(1e-9), &cand);
                    if (iso.occludedAt(qp, iso.faceDepth(f, qp), &.{fi}, cand.items)) continue;
                    const d = qp.dist(c);
                    if (d < bd) {
                        bd = d;
                        best = qp;
                    }
                };
                if (best) |qp| {
                    const e = 1e-3;
                    const tri = try a.dupe(V2, &.{ V2.init(qp.x - e, qp.y - e), V2.init(qp.x + e, qp.y - e), V2.init(qp.x, qp.y + e) });
                    try shapes.append(a, .{ .outer = tri, .holes = &.{} });
                    break :fb;
                }
            }
        }
        try iso.vis.append(a, .{ .comp = ip.comp, .part = ip.part, .instance = ip.instance, .shapes = shapes.items, .src = ip.src, .cut = ip.cap_cut });
    }

    var cropb = Box{};
    for (items.items) |it| switch (it) {
        .path => |p| cropb.addBox(geom.pointsBox(p.pts)),
        .hatch => |h| for (h.loops) |l| cropb.addBox(geom.pointsBox(l)),
        else => {},
    };
    if (cropb.isEmpty()) cropb = sb;
    return .{ .items = items.items, .scale = s, .crop = cropb };
}

/// Grade-line chain (or full outline) of a fill's cut face.
fn fillOutline(a: Allocator, items: *std.ArrayList(drawing.Item), ip: IsoPrism, style: *const style_mod.Style, sx: f64, sz: f64) Allocator.Error!void {
    for (ip.loops) |l| {
        var cur: std.ArrayList(Pt) = .empty;
        const n = l.len;
        const ccw = geom.signedAreaV(l) >= 0;
        var start: usize = 0;
        if (ip.outline == .top) {
            for (0..n) |k| {
                if (!edgeTop(l, (k + n - 1) % n, ccw)) {
                    start = k;
                    break;
                }
            }
        }
        for (0..n) |idx| {
            const i = (start + idx) % n;
            const p = l[i];
            const q = l[(i + 1) % n];
            const draw = ip.outline == .full or edgeTop(l, i, ccw);
            if (draw) {
                const pp = proj(sx, sz, .{ p.x, p.y, ip.z1 });
                const qq = proj(sx, sz, .{ q.x, q.y, ip.z1 });
                if (cur.items.len == 0) try cur.append(a, Pt.at(pp, 0));
                try cur.append(a, Pt.at(qq, 0));
            } else if (cur.items.len > 0) {
                try items.append(a, .{ .path = .{ .layer = style.layerForPen(.cut), .pen = .cut, .src = ip.src, .closed = false, .pts = cur.items } });
                cur = .empty;
            }
        }
        if (cur.items.len > 0) try items.append(a, .{ .path = .{ .layer = style.layerForPen(.cut), .pen = .cut, .src = ip.src, .closed = false, .pts = cur.items } });
    }
}

fn edgeTop(l: []const V2, i: usize, ccw: bool) bool {
    const p = l[i];
    const q = l[(i + 1) % l.len];
    const d = q.sub(p);
    const len = d.len();
    if (len < 1e-12) return false;
    const ny = if (ccw) -d.x / len else d.x / len;
    return ny > 0.01;
}
