//! concrete (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

const std = @import("std");
const json = @import("../json.zig");
const geom = @import("../geom.zig");
const model = @import("../model.zig");
const units = @import("../units.zig");
const common = @import("common.zig");
const V2 = geom.V2;
const Pt = geom.Pt;
const Built = model.Built;
const Prism = model.Prism;
const Ctx = common.Ctx;
const BuildError = common.BuildError;
const zoneRect = common.zoneRect;
const onePrism = common.onePrism;
const ftin = common.ftin;
const fmtNum = common.fmtNum;
const parsePointList = common.parsePointList;
const dropDuplicatePoints = common.dropDuplicatePoints;
const orientedCcw = common.orientedCcw;
const materialOk = common.materialOk;
const parseCover = common.parseCover;
const mirrorBuilt = common.mirrorBuilt;

pub const Params = struct {
    shape: enum { rect, footing, polygon, slab_edge },
    material: []const u8 = "concrete",
    cover: ?json.Value = null,
    width: ?f64 = null,
    height: ?f64 = null,
    points: ?json.Value = null,
    exterior: enum { left, right } = .left,
    slab_thickness: f64 = 4,
    slab_length: f64 = 48,
    footing_width: f64 = 12,
    footing_depth: f64 = 18,
    haunch: f64 = 45,
    recess: ?json.Value = null,
    recess_slope: f64 = 0,
    base: ?json.Value = null,

    pub const spec = .{
        .shape = .{ .desc = "rect | footing | polygon | slab_edge" },
        .material = .{ .desc = "any style material" },
        .cover = .{ .def = "{bottom:3, sides:3, top:1.5}", .desc = "REQUIRED clear cover for bars in this host (W_COVER); cover.parts.<part> overrides per part zone, e.g. {\"parts\":{\"slab\":{\"bottom\":0.75}}}" },
        .width = .{ .len = .pos, .hint = "rect/footing need width and height", .also = &.{"height"}, .def = "rect/footing: required", .desc = "box size" },
        .height = .{ .len = .pos, .hint = "rect/footing need width and height", .row = false, .desc = "box size" },
        .points = .{ .def = "polygon: required", .desc = "polyline [x,y(,bulge)] relative to the placement point, or absolute Refs" },
        .exterior = .{ .desc = "slab_edge: which side is the exterior edge (right mirrors)" },
        .slab_thickness = .{ .len = .pos, .desc = "slab_edge" },
        .slab_length = .{ .len = .pos, .desc = "slab_edge: slab drawn from exterior face inward" },
        .footing_width = .{ .len = .pos, .desc = "slab_edge: bottom width of the turndown" },
        .footing_depth = .{ .len = .pos, .desc = "slab_edge: top of slab to bottom of footing" },
        .haunch = .{ .desc = "slab_edge: inner face slope from horizontal in degrees; 90 = vertical" },
        .recess = .{ .desc = "slab_edge: {width, depth, from_edge} depression at the top exterior edge (door sill); from_edge 0 = at the exterior face" },
        .recess_slope = .{ .len = .any, .desc = "slab_edge: recess floor falls this many inches toward the exterior over its width" },
        .base = .{ .desc = "slab_edge: {material: gravel|sand|compacted_fill, thickness} uniform base course under the slab soffit and along the haunch (soil side), stopping at the footing bottom; part `base` with the fill hatch. Replaces hand-drawn fill polygons under the slab" },
    };
};

pub fn build(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const cp = p.parseAll(Params);
    const cov = try parseCover(ctx, "cover", .{});
    if (!p.ok) return null;
    const material = cp.material;
    if (!materialOk(ctx, "material", material)) return null;
    const sh = cp.shape;
    if (sh == .rect or sh == .footing) {
        const hint = "rect/footing need width and height";
        const w = cp.width orelse blk: {
            p.missing("width", hint);
            break :blk 0;
        };
        const h = cp.height orelse blk: {
            p.missing("height", hint);
            break :blk 0;
        };
        if (!p.ok) return null;
        const loop = try model.rectLoop(a, 0, 0, w, h);
        const part: []const u8 = if (sh == .footing) "footing" else "";
        const prism = Prism{ .part = part, .material = material, .loops = try model.oneLoop(a, loop) };
        const zones = if (part.len > 0) try a.dupe(model.Zone, &.{try zoneRect(a, "footing", 0, 0, w, h)}) else &[_]model.Zone{};
        return .{
            .prisms = try onePrism(a, prism),
            .zones = zones,
            .box = .{ .x0 = 0, .y0 = 0, .x1 = w, .y1 = h },
            .host = .{ .outline = loop, .cover = cov.?.cover, .part_cover = cov.?.parts },
            .info = try a.print("concrete {s} {s} x {s}", .{ @tagName(sh), ftin(a, w), ftin(a, h) }),
        };
    }
    if (sh == .polygon) {
        const pv = cp.points orelse {
            p.fail("points", "shape polygon needs 'points': [[x, y], ...] (relative to the placement point) or Refs", .{});
            return null;
        };
        const pts = (try parsePointList(ctx, "points", pv, true)) orelse return null;
        const clean = try dropDuplicatePoints(a, pts);
        if (clean.len < 3) {
            p.fail("points", "polygon needs at least 3 distinct points (got {d})", .{clean.len});
            return null;
        }
        const loop = try orientedCcw(a, clean);
        const prism = Prism{ .material = material, .loops = try model.oneLoop(a, loop) };
        const bx = geom.loopBox(loop);
        return .{
            .prisms = try onePrism(a, prism),
            .box = bx,
            .points_mode = true,
            .host = .{ .outline = loop, .cover = cov.?.cover, .part_cover = cov.?.parts },
            .info = "concrete polygon",
        };
    }
    // slab_edge
    const st = cp.slab_thickness;
    const sl = cp.slab_length;
    const fw = cp.footing_width;
    const fd = cp.footing_depth;
    const haunch = cp.haunch;
    var rw: f64 = 0;
    var rd: f64 = 0;
    var re: f64 = 0;
    var has_recess = false;
    if (cp.recess) |rv| {
        if (rv != .object) {
            p.fail("recess", "param 'recess' must be {{\"width\": w, \"depth\": d, \"from_edge\": e}} or null", .{});
        } else {
            has_recess = true;
            rw = (if (rv.get("width")) |x| units.parseLength(x) else null) orelse blk: {
                p.fail("recess/width", "recess needs a numeric 'width'", .{});
                break :blk 0;
            };
            rd = (if (rv.get("depth")) |x| units.parseLength(x) else null) orelse blk: {
                p.fail("recess/depth", "recess needs a numeric 'depth'", .{});
                break :blk 0;
            };
            re = p.fieldLen("recess", rv, "from_edge", 0) orelse 0;
        }
    }
    const rslope = cp.recess_slope;
    var base_mat: []const u8 = "";
    var base_t: f64 = 0;
    if (cp.base) |bv| {
        if (bv != .object) {
            p.fail("base", "param 'base' must be {{\"material\": \"gravel\"|\"sand\"|\"compacted_fill\", \"thickness\": 4}} or null", .{});
        } else {
            base_mat = (if (bv.get("material")) |x| x.str() else null) orelse "gravel";
            var ok_mat = false;
            for ([_][]const u8{ "gravel", "sand", "compacted_fill" }) |m| {
                if (std.mem.eql(u8, m, base_mat)) ok_mat = true;
            }
            if (!ok_mat) p.fail("base/material", "base.material must be \"gravel\", \"sand\" or \"compacted_fill\" (got \"{s}\")", .{base_mat});
            base_t = (if (bv.get("thickness")) |x| units.parseLength(x) else null) orelse blk: {
                p.fail("base/thickness", "base needs a numeric 'thickness' (inches), e.g. {{\"material\": \"gravel\", \"thickness\": 4}}", .{});
                break :blk 0;
            };
            if (p.ok and base_t <= 0) p.fail("base/thickness", "base.thickness must be greater than 0", .{});
        }
    }
    if (!p.ok) return null;
    if (fd <= st) {
        p.fail("footing_depth", "footing_depth ({s}) must exceed slab_thickness ({s}): it is measured from the top of the slab", .{ fmtNum(a, fd), fmtNum(a, st) });
        return null;
    }
    if (haunch <= 0 or haunch > 90) {
        p.fail("haunch", "haunch must be an angle in degrees from horizontal, > 0 and <= 90 (got {s})", .{fmtNum(a, haunch)});
        return null;
    }
    if (has_recess) {
        if (rw <= 0 or rd <= 0 or rd >= st) {
            p.fail("recess", "recess width must be > 0 and depth in (0, slab_thickness {s}) (got width {s}, depth {s})", .{ fmtNum(a, st), fmtNum(a, rw), fmtNum(a, rd) });
            return null;
        }
        if (re + rw > sl) {
            p.fail("recess", "recess (from_edge {s} + width {s}) extends beyond slab_length {s}", .{ fmtNum(a, re), fmtNum(a, rw), fmtNum(a, sl) });
            return null;
        }
    }
    const hx = fw + (fd - st) / @tan(std.math.degreesToRadians(haunch));
    if (hx > sl) {
        p.fail("haunch", "the haunch meets the slab underside at x={s}, beyond slab_length {s}; lengthen the slab or steepen haunch", .{ fmtNum(a, hx), fmtNum(a, sl) });
        return null;
    }
    var pts: std.ArrayList(Pt) = .empty;
    try pts.append(a, .{ .x = 0, .y = -fd }); // footing bottom exterior
    const rec_ext_y = -rd - rslope;
    if (has_recess) {
        if (re > 0) {
            try pts.append(a, .{ .x = 0, .y = 0 });
            try pts.append(a, .{ .x = re, .y = 0 });
        }
        try pts.append(a, .{ .x = re, .y = rec_ext_y }); // recess bottom exterior
        try pts.append(a, .{ .x = re + rw, .y = -rd }); // recess bottom interior
        try pts.append(a, .{ .x = re + rw, .y = 0 }); // recess top interior
    } else {
        try pts.append(a, .{ .x = 0, .y = 0 });
    }
    try pts.append(a, .{ .x = sl, .y = 0 });
    try pts.append(a, .{ .x = sl, .y = -st });
    try pts.append(a, .{ .x = hx, .y = -st });
    try pts.append(a, .{ .x = fw, .y = -fd });
    const loop = try orientedCcw(a, pts.items);
    const prism = Prism{ .material = material, .loops = try model.oneLoop(a, loop) };
    var anchors: std.ArrayList(model.NamedAnchor) = .empty;
    try anchors.append(a, .{ .name = "top_exterior", .p = V2.init(0, 0) });
    try anchors.append(a, .{ .name = "slab_top", .p = V2.init(sl, 0) });
    try anchors.append(a, .{ .name = "footing_bottom_exterior", .p = V2.init(0, -fd) });
    try anchors.append(a, .{ .name = "footing_bottom_interior", .p = V2.init(fw, -fd) });
    try anchors.append(a, .{ .name = "slab_bottom_interior", .p = V2.init(sl, -st) });
    try anchors.append(a, .{ .name = "haunch_top", .p = V2.init(hx, -st) });
    if (has_recess) {
        try anchors.append(a, .{ .name = "recess_bottom_exterior", .p = V2.init(re, rec_ext_y) });
        try anchors.append(a, .{ .name = "recess_bottom_interior", .p = V2.init(re + rw, -rd) });
        try anchors.append(a, .{ .name = "recess_top_interior", .p = V2.init(re + rw, 0) });
    }
    const zones = try a.dupe(model.Zone, &.{
        try zoneRect(a, "footing", 0, -fd, fw, -st),
        try zoneRect(a, "slab", 0, -st, sl, 0),
    });
    var prisms_out: std.ArrayList(Prism) = .empty;
    try prisms_out.append(a, prism);
    var zones_out: std.ArrayList(model.Zone) = .empty;
    try zones_out.appendSlice(a, zones);
    if (base_t > 0) {
        // uniform base course under the slab soffit and along the haunch, stopping at the footing bottom
        const dx = hx - fw;
        const dy = fd - st;
        const dl = @sqrt(dx * dx + dy * dy);
        const ux = dx / dl;
        const uy = dy / dl;
        const nx = uy; // soil-side normal of the haunch line (down and toward the interior)
        const ny = -ux;
        const o = V2.init(fw + nx * base_t, -fd + ny * base_t);
        if (st + base_t >= fd - 1e-9) {
            p.fail("base/thickness", "base.thickness {s} must be less than footing_depth - slab_thickness ({s}) so the base stops above the footing bottom", .{ fmtNum(a, base_t), fmtNum(a, fd - st) });
            return null;
        }
        const s1 = (-st - base_t - o.y) / uy; // along the offset haunch line to the soffit-offset level
        const s2 = (-fd - o.y) / uy; // ... to the footing-bottom level
        const q1 = V2.init(o.x + ux * s1, -st - base_t);
        const q2 = V2.init(o.x + ux * s2, -fd);
        if (q1.x >= sl - 1e-9) {
            p.fail("base/thickness", "the base course meets the slab end before the haunch offset does; lengthen slab_length or reduce base.thickness", .{});
            return null;
        }
        const bpts = [_]Pt{
            .{ .x = fw, .y = -fd },
            .{ .x = hx, .y = -st },
            .{ .x = sl, .y = -st },
            .{ .x = sl, .y = -st - base_t },
            .{ .x = q1.x, .y = q1.y },
            .{ .x = q2.x, .y = q2.y },
        };
        const bloop = try orientedCcw(a, try dropDuplicatePoints(a, &bpts));
        try prisms_out.append(a, .{ .part = "base", .material = base_mat, .loops = try model.oneLoop(a, bloop) });
        const bb = geom.loopBox(bloop);
        try zones_out.append(a, .{ .name = "base", .loops = try model.oneLoop(a, bloop), .box = bb });
        try anchors.append(a, .{ .name = "base_bottom_interior", .p = V2.init(sl, -st - base_t) });
        try anchors.append(a, .{ .name = "base_bottom_footing", .p = V2.init(q2.x, -fd) });
    }
    var built = Built{
        .prisms = prisms_out.items,
        .anchors = anchors.items,
        .zones = zones_out.items,
        .box = geom.loopBox(loop),
        .host = .{ .outline = loop, .cover = cov.?.cover, .part_cover = cov.?.parts },
        .info = try a.print("concrete slab_edge {s} slab, ftg {s} x {s}{s}{s}", .{
            ftin(a, st),
            ftin(a, fw),
            ftin(a, fd),
            if (has_recess) try a.print(", recess {s} x {s}", .{ ftin(a, rw), ftin(a, rd) }) else "",
            if (base_t > 0) try a.print(", {s} {s} base", .{ ftin(a, base_t), base_mat }) else "",
        }),
    };
    if (cp.exterior == .right) {
        built = try mirrorBuilt(a, built, geom.Xf.scaling(-1, 1), false);
        built.box = geom.loopBox(built.host.?.outline); // the base course does not move the 9 box anchors
    }
    return built;
}
