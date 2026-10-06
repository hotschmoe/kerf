//! connector (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

const std = @import("std");
const json = @import("../json.zig");
const geom = @import("../geom.zig");
const model = @import("../model.zig");
const cast = @import("../num.zig");
const path_geom = @import("../pathgeom.zig");
const catalog = @import("../catalog.zig");
const common = @import("common.zig");
const Built = model.Built;
const Prism = model.Prism;
const Hardware = catalog.Hardware;
const hardware = catalog.hardware;
const Ctx = common.Ctx;
const BuildError = common.BuildError;
const onePrism = common.onePrism;
const ftin = common.ftin;
const fmtNum = common.fmtNum;
const parsePointList = common.parsePointList;
const dropDuplicatePoints = common.dropDuplicatePoints;
const gaugeThickness = common.gaugeThickness;

pub const Params = struct {
    model: []const u8 = "",
    points: ?json.Value = null,
    lay: enum { edge, face } = .edge,
    side: enum { left, right } = .left,
    gauge: ?f64 = null,
    width: ?f64 = null,
    fasteners: []const u8 = "",

    pub const spec = .{
        .model = .{ .def = "null", .desc = "e.g. MSTA36, H2.5A, HETA20, CS16, CS14: fills width/gauge from the hardware table" },
        .points = .{ .def = "required", .desc = "polyline [x,y] or Refs of the bearing face (lay edge) or centerline (lay face)" },
        .lay = .{ .desc = "edge: seen edge-on, gauge in-plane growing to `side`, width along Z; face: seen face-on, `width` in-plane centered on the polyline, gauge along Z" },
        .side = .{ .desc = "lay edge: left of the polyline direction (left of a left-to-right line = up) or right" },
        .gauge = .{ .def = "18", .desc = "12 .1046, 14 .0747, 16 .0598, 18 .0478, 20 .0359" },
        .width = .{ .len = .pos, .def = "1.25", .desc = "extent along Z (lay edge) or in-plane (lay face)" },
        .fasteners = .{ .def = "null", .desc = "text for notes, e.g. \"(10) 10d EA. END\"" },
    };
};

pub fn build(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const cp = p.parseAll(Params);
    const model_name = cp.model;
    var hw: ?Hardware = null;
    if (model_name.len > 0) {
        for (hardware) |h| if (std.ascii.eqlIgnoreCase(h.model, model_name)) {
            hw = h;
            break;
        };
    }
    const gauge_default: f64 = if (hw) |h| @floatFromInt(h.gauge) else 18;
    const gauge_n = cp.gauge orelse gauge_default;
    const width: f64 = cp.width orelse if (hw) |h| h.width else 1.25;
    const pv = cp.points orelse {
        p.fail("points", "connector needs 'points': the strap polyline, e.g. [\"upper_plate@top_right\", \"beam@top_left\"] (Refs or [x, y])", .{});
        return null;
    };
    if (!p.ok) return null;
    const gauge_i: u32 = cast.toInt(u32, @round(gauge_n)) orelse 0;
    const thickness = gaugeThickness(gauge_i) orelse {
        p.fail("gauge", "gauge {s} is not in the table; use 10, 11, 12, 14, 16, 18, 20, 22, 24, 26 or 28", .{fmtNum(a, gauge_n)});
        return null;
    };
    const pts = (try parsePointList(ctx, "points", pv, true)) orelse return null;
    const clean = try dropDuplicatePoints(a, pts);
    if (clean.len < 2) {
        p.fail("points", "a connector needs at least 2 distinct points (got {d})", .{clean.len});
        return null;
    }
    const edge = cp.lay == .edge;
    const rib = if (edge)
        try path_geom.ribbon(a, clean, if (cp.side == .left) thickness else 0, if (cp.side == .right) thickness else 0)
    else
        try path_geom.ribbon(a, clean, width / 2, width / 2);
    const prism = Prism{ .material = "steel", .loops = try model.oneLoop(a, rib), .centerline = clean, .face_tie = !edge };
    return .{
        .prisms = try onePrism(a, prism),
        .box = geom.loopBox(rib),
        .nat_z = if (edge) width else thickness,
        .points_mode = true,
        .info = try std.fmt.allocPrint(a, "connector {s}{s}{s}{d} ga x {s} lay {s}", .{
            model_name,
            if (model_name.len > 0) " " else "",
            if (hw) |h| try std.fmt.allocPrint(a, "{s} ", .{h.kind}) else "",
            gauge_i,
            ftin(a, width),
            @tagName(cp.lay),
        }),
    };
}
