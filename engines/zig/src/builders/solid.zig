//! solid (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

const std = @import("std");
const json = @import("../json.zig");
const geom = @import("../geom.zig");
const model = @import("../model.zig");
const units = @import("../units.zig");
const common = @import("common.zig");
const Pt = geom.Pt;
const Built = model.Built;
const Prism = model.Prism;
const Ctx = common.Ctx;
const BuildError = common.BuildError;
const onePrism = common.onePrism;
const parsePointList = common.parsePointList;
const dropDuplicatePoints = common.dropDuplicatePoints;
const orientedCcw = common.orientedCcw;
const materialOk = common.materialOk;

pub const Params = struct {
    profile: ?json.Value = null,
    material: []const u8,

    pub const spec = .{
        .profile = .{ .def = "required", .desc = "{rect:[w,h]} | {circle:d} | {points:[...]}" },
        .material = .{ .desc = "any style material (aluminum, steel, ...)" },
    };
};

pub fn build(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const sp = p.parseAll(Params);
    const prof = sp.profile orelse {
        p.fail("profile", "solid needs 'profile': {{\"rect\": [w, h]}}, {{\"circle\": d}} or {{\"points\": [[x, y], ...]}}", .{});
        return null;
    };
    if (!p.ok) return null;
    if (!materialOk(ctx, "material", sp.material)) return null;
    var loop: []const Pt = undefined;
    var points_mode = false;
    if (prof.get("rect")) |rv| {
        const arr = rv.arr();
        if (arr == null or arr.?.len != 2) {
            p.fail("profile/rect", "profile.rect must be [width, height]", .{});
            return null;
        }
        const w = units.parseLength(arr.?[0]);
        const h = units.parseLength(arr.?[1]);
        if (w == null or h == null or w.? <= 0 or h.? <= 0) {
            p.fail("profile/rect", "profile.rect must be two positive lengths [width, height]", .{});
            return null;
        }
        loop = try model.rectLoop(a, 0, 0, w.?, h.?);
    } else if (prof.get("circle")) |cv| {
        const dd = units.parseLength(cv);
        if (dd == null or dd.? <= 0) {
            p.fail("profile/circle", "profile.circle must be a positive diameter", .{});
            return null;
        }
        loop = try model.circleLoop(a, dd.? / 2, dd.? / 2, dd.? / 2);
    } else if (prof.get("points")) |pv| {
        const pts = (try parsePointList(ctx, "profile/points", pv, true)) orelse return null;
        const clean = try dropDuplicatePoints(a, pts);
        if (clean.len < 3) {
            p.fail("profile/points", "profile.points needs at least 3 points", .{});
            return null;
        }
        loop = try orientedCcw(a, clean);
        points_mode = true;
    } else {
        p.fail("profile", "profile must have one of 'rect', 'circle' or 'points'", .{});
        return null;
    }
    const prism = Prism{ .material = sp.material, .loops = try model.oneLoop(a, loop) };
    const bx = geom.loopBox(loop);
    return .{
        .prisms = try onePrism(a, prism),
        .box = bx,
        .points_mode = points_mode,
        .info = try a.print("solid {s} (escape hatch)", .{sp.material}),
    };
}
