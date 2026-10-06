//! panel (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

const std = @import("std");
const json = @import("../json.zig");
const geom = @import("../geom.zig");
const model = @import("../model.zig");
const common = @import("common.zig");
const V2 = geom.V2;
const Built = model.Built;
const Prism = model.Prism;
const Ctx = common.Ctx;
const BuildError = common.BuildError;
const onePrism = common.onePrism;
const ftin = common.ftin;
const quadOf = common.quadOf;
const materialOk = common.materialOk;
const lengthOrUntil = common.lengthOrUntil;

pub const Params = struct {
    material: enum { osb, plywood, gypsum, fiber_cement, wood_board } = .osb,
    thickness: f64,
    length: ?json.Value = null,
    until: ?json.Value = null,
    run: enum { x, y } = .x,

    pub const spec = .{
        .material = .{ .desc = "osb | plywood | gypsum | fiber_cement | wood_board" },
        .thickness = .{ .len = .pos, .hint = "e.g. 0.4375 for 7/16\" OSB", .desc = "e.g. 0.4375 (7/16\"), 0.46875 (15/32), 0.5, 0.625, 0.75" },
        .length = .{ .def = "required", .desc = "in-plane extent; alternative: `until` (same rule as lumber)" },
        .until = .{ .desc = "alternative to length: a Ref; the panel grows from its placement anchor along its run axis until its far end reaches the Ref's coordinate. On a sloped panel (`slope`, e.g. \"@truss\") the run axis is the rotated one: the end is cut square where the Ref projects onto the slope. Roof sheathing: \"slope\": \"@truss\", \"until\": \"truss@top_chord_end\" (no literal length or pitch to keep in sync)" },
        .run = .{ .desc = "in-plane direction of length before rotation: x (length x thickness) or y (thickness x length)" },
    };
};

pub fn build(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    // Total parse: a bad thickness must not hide a bad length (every problem is reported in one pass).
    const pp = p.parseAll(Params);
    const run = @tagName(pp.run);
    var until_note: []const u8 = "";
    const length = try lengthOrUntil(ctx, run, "the in-plane extent of the panel: \"length\" or \"until\": \"other@anchor\"", &until_note);
    if (!p.ok) return null;
    const material = @tagName(pp.material);
    if (!materialOk(ctx, "material", material)) return null;
    const along_x = pp.run == .x;
    const w = if (along_x) length.? else pp.thickness;
    const h = if (along_x) pp.thickness else length.?;
    const loop = try model.rectLoop(a, 0, 0, w, h);
    const prism = Prism{
        .material = material,
        .loops = try model.oneLoop(a, loop),
        .quads = try a.dupe([4]V2, &.{quadOf(0, 0, w, h)}),
    };
    return .{
        .prisms = try onePrism(a, prism),
        .box = .{ .x0 = 0, .y0 = 0, .x1 = w, .y1 = h },
        .info = try std.fmt.allocPrint(a, "panel {s} {s} x {s} run {s}{s}", .{ material, ftin(a, pp.thickness), ftin(a, length.?), run, until_note }),
    };
}
