//! insulation (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

const std = @import("std");
const json = @import("../json.zig");
const geom = @import("../geom.zig");
const model = @import("../model.zig");
const cast = @import("../num.zig");
const common = @import("common.zig");
const Allocator = std.mem.Allocator;
const Pt = geom.Pt;
const Built = model.Built;
const Prism = model.Prism;
const Box = geom.Box;
const Ctx = common.Ctx;
const BuildError = common.BuildError;
const onePrism = common.onePrism;
const parsePointList = common.parsePointList;
const dropDuplicatePoints = common.dropDuplicatePoints;
const orientedCcw = common.orientedCcw;

pub const Params = struct {
    form: enum { rigid, batt } = .rigid,
    width: ?f64 = null,
    height: ?f64 = null,
    points: ?json.Value = null,

    pub const spec = .{
        .form = .{ .desc = "rigid | batt" },
        .width = .{ .len = .pos, .hint = "give width and height, or points", .also = &.{"height"}, .def = "rect: required unless points", .desc = "box size" },
        .height = .{ .len = .pos, .hint = "give width and height, or points", .row = false, .desc = "box size" },
        .points = .{ .desc = "polygon alternative to width/height" },
    };
};

pub fn build(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const ip = p.parse(Params) orelse return null;
    var loop: []const Pt = undefined;
    var points_mode = false;
    if (ip.points) |pv| {
        const pts = (try parsePointList(ctx, "points", pv, true)) orelse return null;
        const clean = try dropDuplicatePoints(a, pts);
        if (clean.len < 3) {
            p.fail("points", "insulation polygon needs at least 3 points", .{});
            return null;
        }
        loop = try orientedCcw(a, clean);
        points_mode = true;
    } else {
        const hint = "give width and height, or points";
        const w = ip.width orelse {
            p.missing("width", hint);
            return null;
        };
        const h = ip.height orelse {
            p.missing("height", hint);
            return null;
        };
        loop = try model.rectLoop(a, 0, 0, w, h);
    }
    const batt = ip.form == .batt;
    var prism = Prism{
        .material = if (batt) "insulation_batt" else "insulation_rigid",
        .loops = try model.oneLoop(a, loop),
    };
    const bx = geom.loopBox(loop);
    if (batt) {
        prism.kind = .batt;
        prism.line_pts = try battSymbol(a, bx);
    }
    return .{
        .prisms = try onePrism(a, prism),
        .box = bx,
        .points_mode = points_mode,
        .info = try std.fmt.allocPrint(a, "insulation {s}", .{@tagName(ip.form)}),
    };
}

/// Sinusoidal loop line fitted to the box (batt insulation symbol).
fn battSymbol(a: Allocator, bx: Box) Allocator.Error![]const Pt {
    const horizontal = bx.width() >= bx.height();
    const long = if (horizontal) bx.width() else bx.height();
    const short = if (horizontal) bx.height() else bx.width();
    const loops: f64 = @min(@max(2, @round(long / (short * 0.9))), 2000);
    const pad = short * 0.2;
    const pitch = (long - 2 * pad) / loops;
    const loop_w = 1.7 * pitch / (2.0 * std.math.pi);
    const amp = 0.42 * short;
    const steps_per = 20;
    const total: usize = cast.toIntClamped(usize, loops, 2, 2000) * steps_per;
    var out: std.ArrayList(Pt) = .empty;
    var i: usize = 0;
    while (i <= total) : (i += 1) {
        const t = 2.0 * std.math.pi * loops * @as(f64, @floatFromInt(i)) / @as(f64, @floatFromInt(total));
        const u = pad + (long - 2 * pad) * @as(f64, @floatFromInt(i)) / @as(f64, @floatFromInt(total)) - loop_w * @sin(t);
        const v = amp * @cos(t);
        const along = u;
        if (horizontal) {
            try out.append(a, .{ .x = bx.x0 + along, .y = (bx.y0 + bx.y1) / 2 + v });
        } else {
            try out.append(a, .{ .x = (bx.x0 + bx.x1) / 2 + v, .y = bx.y0 + along });
        }
    }
    return out.items;
}
