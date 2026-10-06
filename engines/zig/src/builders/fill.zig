//! fill (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

const std = @import("std");
const json = @import("../json.zig");
const geom = @import("../geom.zig");
const model = @import("../model.zig");
const common = @import("common.zig");
const Built = model.Built;
const Prism = model.Prism;
const Ctx = common.Ctx;
const BuildError = common.BuildError;
const onePrism = common.onePrism;
const parsePointList = common.parsePointList;
const dropDuplicatePoints = common.dropDuplicatePoints;
const orientedCcw = common.orientedCcw;

pub const Params = struct {
    material: enum { earth, gravel, sand, compacted_fill } = .earth,
    points: ?json.Value = null,
    outline: enum { top, full, none } = .top,
    grade_label: []const u8 = "",

    pub const spec = .{
        .material = .{ .desc = "earth | gravel | sand | compacted_fill" },
        .points = .{ .def = "required", .desc = "polygon [x,y] or Refs" },
        .outline = .{ .desc = "top (stroke only edges with outward normal up: the grade line) | full | none" },
        .grade_label = .{ .def = "null", .desc = "optional text for annotations" },
    };
};

pub fn build(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const fp = p.parseAll(Params);
    const pv = fp.points orelse {
        p.fail("points", "fill needs 'points': a polygon [[x, y], ...] or Refs", .{});
        return null;
    };
    if (!p.ok) return null;
    const pts = (try parsePointList(ctx, "points", pv, true)) orelse return null;
    const clean = try dropDuplicatePoints(a, pts);
    if (clean.len < 3) {
        p.fail("points", "a fill polygon needs at least 3 distinct points (got {d})", .{clean.len});
        return null;
    }
    const loop = try orientedCcw(a, clean);
    const om: model.OutlineMode = switch (fp.outline) {
        .top => .top,
        .full => .full,
        .none => .none,
    };
    const prism = Prism{ .material = @tagName(fp.material), .loops = try model.oneLoop(a, loop), .outline = om };
    return .{
        .prisms = try onePrism(a, prism),
        .box = geom.loopBox(loop),
        .points_mode = true,
        .info = try std.fmt.allocPrint(a, "fill {s}", .{@tagName(fp.material)}),
    };
}
