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
const beyond = @import("section/beyond.zig");
const cut = @import("section/cut.zig");
const groupRegion = cut.groupRegion;
const hardware = @import("section/hardware.zig");
const thickenThinHardware = hardware.thickenThinHardware;
const min_metal_paper_in = hardware.min_metal_paper_in;
const isPathBar = hardware.isPathBar;
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

    pub fn srcName(self: *const Section, p: Prism) []const u8 {
        const c = &self.scene.comps[p.comp];
        if (c.arr_count > 1) return self.a.print("{s}#{d}", .{ c.id, p.instance / @as(u32, @intCast(c.xfs.len / c.arr_count)) }) catch c.id;
        return c.id;
    }

    pub fn flatOf(self: *Section, i: usize) Allocator.Error![]const []const V2 {
        if (self.flat[i]) |f| return f;
        const f = try clip.fromRegion(self.a, self.prisms[i].loops, flat_tol);
        self.flat[i] = f;
        return f;
    }

    /// `which` if the style still defines that pen (a user style can delete one), else the beyond pen.
    pub fn penFor(self: *const Section, which: Pen) Pen {
        return if (self.style.pen(which) != null) which else .beyond;
    }

    /// Fill materials draw with the pen of their kind (steel, rebar); every other fill material draws in the beyond pen.
    pub fn penForRole(self: *const Section, role: style_mod.Role) Pen {
        return switch (role) {
            .steel => self.penFor(.steel),
            .rebar => self.penFor(.rebar),
            else => .beyond,
        };
    }

    pub fn layerFor(self: *const Section, pen: Pen) []const u8 {
        return self.style.layerForPen(pen);
    }

    pub fn addStroke(self: *Section, list: *std.ArrayList(Stroke), pts: []const Pt, closed: bool, pen: Pen, src: []const u8, embedded: bool) Allocator.Error!void {
        const w = self.style.penWidthMm(pen);
        self.seq += 1;
        try list.append(self.a, .{ .pts = pts, .closed = closed, .pen = pen, .src = src, .rank = w + (if (embedded) @as(f64, 10) else 0), .seq = self.seq });
    }

    /// Clip a path to the crop and add the pieces as strokes.
    pub fn addClipped(self: *Section, list: *std.ArrayList(Stroke), pts: []const Pt, closed: bool, pen: Pen, src: []const u8, embedded: bool) Allocator.Error!void {
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

    pub const drawCut = cut.drawCut;

    pub const knockOutFaceTies = hardware.knockOutFaceTies;
    pub const drawFaceTie = hardware.drawFaceTie;

    /// Sawn or engineered lumber lying in the section plane (run x/y): cut along its length, not end-on.
    pub fn isLengthwiseLumber(self: *const Section, p: Prism) bool {
        if (p.quads.len > 0 or p.loops.len == 0 or p.loops[0].len < 3) return false;
        if (p.comp >= self.scene.comps.len) return false;
        return self.scene.comps[p.comp].ty.traits.lengthwise_grain;
    }

    pub const drawBeyond = beyond.drawBeyond;
    pub const visibleRegion = beyond.visibleRegion;
    pub const topEdges = beyond.topEdges;
    pub const edgeIsTop = beyond.edgeIsTop;

    // ---- beyond prisms --------------------------------------------------------------------------------

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
