//! anchor bolt (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

const std = @import("std");
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
const boxOfPrisms = common.boxOfPrisms;
const ftin = common.ftin;
const fmtNum = common.fmtNum;
const orientedCcw = common.orientedCcw;

pub const Params = struct {
    diameter: f64 = 0.5,
    embed: ?f64 = null,
    projection: f64 = 2.5,
    hook: enum { J, L, headed, none, wedge, screw } = .J,
    hook_len: ?f64 = null,
    nut_washer: bool = true,

    pub const spec = .{
        .diameter = .{ .len = .pos, .desc = "0.5 or 0.625 typical" },
        .embed = .{ .len = .pos, .def = "7 (4 for wedge/screw)", .desc = "length below the placement point (top of concrete); effective embedment for wedge/screw" },
        .projection = .{ .len = .pos, .desc = "length above the placement point" },
        .hook = .{ .desc = "J: 180 degree bend toward +x, inside radius 1.5*d, returning up hook_len from the lowest point; L: 90 degree bend toward +x, horizontal leg ends hook_len from the shaft centerline; headed: square head 2*d wide, 0.5*d thick; none; wedge: post-installed expansion anchor (straight shaft, expansion clip 1.15*d wide x 0.6*embed long at the embedded end, nut+washer); screw: Titen HD style concrete screw (thread ticks along the embedment, hex washer head at the top, no nut)" },
        .hook_len = .{ .len = .pos, .def = "J 2, L 3", .desc = "hook leg length in inches (see hook)" },
        .nut_washer = .{ .desc = "draw nut (1.5*d wide, 0.875*d tall, top at projection - 0.25*d) and washer (2.25*d wide, 0.125 thick) under it" },
    };
};

pub fn build(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const ap = p.parse(Params) orelse return null;
    const d = ap.diameter;
    const h = ap.hook;
    const post = h == .wedge or h == .screw;
    const embed: f64 = ap.embed orelse if (post) 4 else 7;
    const proj = ap.projection;
    const nut = ap.nut_washer;
    const hook_len: f64 = ap.hook_len orelse if (h == .L) 3.0 else 2.0;
    const r = d / 2.0;
    const rc = 1.5 * d + r; // centreline bend radius (inside radius 1.5 d)
    var cl: std.ArrayList(Pt) = .empty;
    try cl.append(a, .{ .x = 0, .y = proj });
    const y_bottom_cl = -embed + r; // centreline at the lowest point of the bolt
    if (h == .J) {
        const yb = y_bottom_cl + rc;
        try cl.append(a, .{ .x = 0, .y = yb, .b = geom.bulgeFromSweep(std.math.pi) });
        try cl.append(a, .{ .x = 2 * rc, .y = yb });
        try cl.append(a, .{ .x = 2 * rc, .y = -embed + hook_len });
    } else if (h == .L) {
        const yb = y_bottom_cl + rc;
        try cl.append(a, .{ .x = 0, .y = yb, .b = geom.bulgeFromSweep(std.math.pi / 2.0) });
        try cl.append(a, .{ .x = rc, .y = y_bottom_cl });
        try cl.append(a, .{ .x = @max(hook_len, rc + 0.01), .y = y_bottom_cl });
    } else if (h == .headed) {
        try cl.append(a, .{ .x = 0, .y = -embed + 0.5 * d });
    } else {
        try cl.append(a, .{ .x = 0, .y = -embed });
    }
    const rib = try path_geom.ribbon(a, cl.items, r, r);
    var prisms: std.ArrayList(Prism) = .empty;
    try prisms.append(a, .{ .part = "shank", .material = "steel", .loops = try model.oneLoop(a, rib), .embedded = true, .centerline = cl.items, .sweep_r = r });
    if (h == .headed) {
        try prisms.append(a, .{ .part = "shank", .material = "steel", .loops = try model.oneLoop(a, try model.rectLoop(a, -d, -embed, d, -embed + 0.5 * d)), .embedded = true, .zhalf = d });
    }
    if (h == .wedge) {
        // post-installed expansion anchor: expansion clip (sleeve 0.6 embed long, 1.15 d wide) at the embedded end, chamfered tip
        const hw = 0.575 * d;
        const ch = @min(0.35 * d, 0.2 * embed);
        const clip_pts = [_]Pt{
            .{ .x = -hw + ch, .y = -embed },
            .{ .x = hw - ch, .y = -embed },
            .{ .x = hw, .y = -embed + ch },
            .{ .x = hw, .y = -embed + 0.6 * embed },
            .{ .x = -hw, .y = -embed + 0.6 * embed },
            .{ .x = -hw, .y = -embed + ch },
        };
        try prisms.append(a, .{ .part = "clip", .material = "steel", .loops = try model.oneLoop(a, try a.dupe(Pt, &clip_pts)), .embedded = true, .zhalf = hw });
    }
    if (h == .screw) {
        // Titen HD style: thread ticks along the embedded length (exaggerated sawtooth strips) and a hex washer head
        const td = 0.22 * d;
        const pitch = 0.5 * d;
        const n_f = @floor(embed / pitch);
        const n: usize = cast.toIntClamped(usize, n_f, 0, 80);
        if (n >= 1) {
            for ([_]f64{ -1, 1 }) |sgn| {
                var strip: std.ArrayList(Pt) = .empty;
                try strip.append(a, .{ .x = sgn * r, .y = 0 });
                var i: usize = 0;
                while (i < n) : (i += 1) {
                    const fi: f64 = @floatFromInt(i);
                    try strip.append(a, .{ .x = sgn * (r + td), .y = -(fi + 0.5) * pitch });
                    try strip.append(a, .{ .x = sgn * r, .y = -(fi + 1) * pitch });
                }
                const loop = try orientedCcw(a, strip.items);
                try prisms.append(a, .{ .part = "threads", .material = "steel", .loops = try model.oneLoop(a, loop), .embedded = true, .zhalf = r + td });
            }
        }
        const head_h = 0.6 * d;
        const fl_t = 0.1 * d;
        try prisms.append(a, .{ .part = "washer", .material = "steel", .loops = try model.oneLoop(a, try model.rectLoop(a, -0.95 * d, proj - head_h - fl_t, 0.95 * d, proj - head_h)), .embedded = true, .zhalf = 0.95 * d });
        try prisms.append(a, .{ .part = "head", .material = "steel", .loops = try model.oneLoop(a, try model.rectLoop(a, -0.75 * d, proj - head_h, 0.75 * d, proj)), .embedded = true, .zhalf = 0.75 * d });
    } else if (nut) {
        const nut_h = 0.875 * d;
        const ytop = proj - 0.25 * d;
        const nut_bot = ytop - nut_h;
        try prisms.append(a, .{ .part = "washer", .material = "steel", .loops = try model.oneLoop(a, try model.rectLoop(a, -1.125 * d, nut_bot - 0.125, 1.125 * d, nut_bot)), .embedded = true, .zhalf = 1.125 * d });
        try prisms.append(a, .{ .part = "nut", .material = "steel", .loops = try model.oneLoop(a, try model.rectLoop(a, -0.75 * d, nut_bot, 0.75 * d, ytop)), .embedded = true, .zhalf = 0.75 * d });
    }
    const bx = boxOfPrisms(prisms.items);
    return .{
        .prisms = prisms.items,
        .anchors = try a.dupe(model.NamedAnchor, &.{.{ .name = "top_of_concrete", .p = V2.init(0, 0) }}),
        .box = bx,
        .nat_z = d,
        .info = try a.print("anchor_bolt {s}\" dia, embed {s}, proj {s}, {s}{s}", .{ fmtNum(a, d), ftin(a, embed), ftin(a, proj), @tagName(h), if (post) "" else " hook" }),
    };
}
