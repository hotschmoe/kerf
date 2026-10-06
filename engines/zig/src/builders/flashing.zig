//! flashing (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

const std = @import("std");
const json = @import("../json.zig");
const geom = @import("../geom.zig");
const model = @import("../model.zig");
const cast = @import("../num.zig");
const path_geom = @import("../pathgeom.zig");
const common = @import("common.zig");
const V2 = geom.V2;
const Pt = geom.Pt;
const Built = model.Built;
const Prism = model.Prism;
const Ctx = common.Ctx;
const BuildError = common.BuildError;
const onePrism = common.onePrism;
const fmtNum = common.fmtNum;
const parsePointList = common.parsePointList;
const dropDuplicatePoints = common.dropDuplicatePoints;
const mirrorBuilt = common.mirrorBuilt;
const gaugeThickness = common.gaugeThickness;
const materialOr = common.materialOr;

pub const Params = struct {
    profile: enum { z, l, drip, weep_screed, points } = .z,
    flange: ?f64 = null,
    leg: ?f64 = null,
    drop: ?f64 = null,
    kick: f64 = 0.5,
    gauge: f64 = 26,
    exterior: enum { left, right } = .left,
    points: ?json.Value = null,

    pub const spec = .{
        .profile = .{ .desc = "z: back flange up the wall, horizontal leg out, drop at the nose; l: flange + horizontal leg; drip: flange on the deck, drop, outward kick; weep_screed: nailing flange up the wall, ledge, small drip drop; points: free centerline polyline" },
        .flange = .{ .len = .pos, .def = "2 (weep_screed 3.5)", .desc = "vertical back/nailing flange length (drip: horizontal flange on the deck)" },
        .leg = .{ .len = .pos, .def = "1 (l 2)", .desc = "horizontal leg length toward the exterior" },
        .drop = .{ .len = .pos, .def = "2 (drip 1.5, weep_screed 0.5)", .desc = "downturned leg at the nose" },
        .kick = .{ .len = .pos, .desc = "drip only: outward kick at the bottom of the drop" },
        .gauge = .{ .desc = "20 .0359, 22 .0299, 24 .0239, 26 .0179, 28 .0149" },
        .exterior = .{ .desc = "side the nose faces (right mirrors); presets only" },
        .points = .{ .def = "profile points: required", .desc = "centerline polyline [x,y] relative to the placement point, or Refs" },
    };
};

pub fn build(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const fp = p.parse(Params) orelse return null;
    const pr = fp.profile;
    const flange: f64 = fp.flange orelse if (pr == .weep_screed) 3.5 else 2;
    const leg: f64 = fp.leg orelse if (pr == .l) 2 else 1;
    const drop: f64 = fp.drop orelse switch (pr) {
        .drip => 1.5,
        .weep_screed => 0.5,
        else => 2,
    };
    const kick = fp.kick;
    const gauge_i: u32 = cast.toInt(u32, @round(fp.gauge)) orelse 0;
    const thickness = gaugeThickness(gauge_i) orelse {
        p.fail("gauge", "gauge {s} is not in the table; use 20, 22, 24, 26 (default, 0.0179\") or 28", .{fmtNum(a, fp.gauge)});
        return null;
    };
    var pts: std.ArrayList(Pt) = .empty;
    var points_mode = false;
    // local frame: corner (the first bend) at (0,0); the wall surface is x = 0 and the exterior is -x
    if (pr == .z or pr == .weep_screed) {
        try pts.appendSlice(a, &.{ .{ .x = 0, .y = flange }, .{ .x = 0, .y = 0 }, .{ .x = -leg, .y = 0 }, .{ .x = -leg, .y = -drop } });
    } else if (pr == .l) {
        try pts.appendSlice(a, &.{ .{ .x = 0, .y = flange }, .{ .x = 0, .y = 0 }, .{ .x = -leg, .y = 0 } });
    } else if (pr == .drip) {
        try pts.appendSlice(a, &.{ .{ .x = flange, .y = 0 }, .{ .x = 0, .y = 0 }, .{ .x = 0, .y = -drop }, .{ .x = -kick, .y = -drop - 0.5 * kick } });
    } else {
        const pv = fp.points orelse {
            p.fail("points", "profile \"points\" needs 'points': the sheet-metal centerline polyline, e.g. [[0,2],[0,0],[-1,0],[-1,-2]] (or Refs)", .{});
            return null;
        };
        const pl = (try parsePointList(ctx, "points", pv, true)) orelse return null;
        const clean = try dropDuplicatePoints(a, pl);
        if (clean.len < 2) {
            p.fail("points", "flashing points need at least 2 distinct points (got {d})", .{clean.len});
            return null;
        }
        try pts.appendSlice(a, clean);
        points_mode = true;
    }
    const rib = try path_geom.ribbon(a, pts.items, thickness / 2, thickness / 2);
    const mat = materialOr(ctx, "steel", "generic");
    const prism = Prism{ .material = mat, .loops = try model.oneLoop(a, rib), .embedded = true, .centerline = pts.items };
    const anchors = try a.dupe(model.NamedAnchor, &.{
        .{ .name = "corner", .p = if (points_mode) pts.items[0].v() else V2.init(0, 0) },
        .{ .name = "start", .p = pts.items[0].v() },
        .{ .name = "end", .p = pts.items[pts.items.len - 1].v() },
    });
    var built = Built{
        .prisms = try onePrism(a, prism),
        .anchors = anchors,
        .box = geom.loopBox(rib),
        .points_mode = points_mode,
        .info = try std.fmt.allocPrint(a, "flashing {s} {d} ga ({s}\" thick)", .{ @tagName(pr), gauge_i, fmtNum(a, thickness) }),
    };
    if (!points_mode and fp.exterior == .right) built = try mirrorBuilt(a, built, geom.Xf.scaling(-1, 1), false);
    return built;
}
