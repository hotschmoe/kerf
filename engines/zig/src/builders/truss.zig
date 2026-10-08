//! truss (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

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
const Box = geom.Box;
const Ctx = common.Ctx;
const BuildError = common.BuildError;
const zoneRect = common.zoneRect;
const ftin = common.ftin;
const fmtNum = common.fmtNum;
const parseSawn = common.parseSawn;
const mirrorBuilt = common.mirrorBuilt;

pub const Params = struct {
    exterior: enum { left, right } = .left,
    pitch: json.Value = .{ .string = "4:12" },
    top_chord: []const u8 = "2x4",
    bottom_chord: []const u8 = "2x4",
    heel: enum { standard, raised } = .standard,
    heel_height: ?f64 = null,
    bearing_width: f64 = 3.5,
    overhang: f64 = 12,
    tail: enum { plumb, square } = .plumb,
    span_shown: f64 = 48,
    plate: bool = true,

    pub const spec = .{
        .exterior = .{ .desc = "side of the heel/overhang (right mirrors)" },
        .pitch = .{ .def = "4:12", .desc = "rise:run" },
        .top_chord = .{ .desc = "sawn nominal size, depth in-plane" },
        .bottom_chord = .{ .desc = "sawn nominal size, depth in-plane" },
        .heel = .{ .desc = "standard | raised" },
        .heel_height = .{ .len = .pos, .desc = "raised heel: vertical height at the bearing outer edge from top of bottom chord to top of top chord" },
        .bearing_width = .{ .len = .pos, .desc = "width of the support under the heel" },
        .overhang = .{ .len = .any, .desc = "horizontal distance from outer face of bearing to the tail end" },
        .tail = .{ .desc = "plumb | square cut" },
        .span_shown = .{ .len = .pos, .desc = "how far into the building to draw (crop/break at the end)" },
        .plate = .{ .desc = "draw the heel truss plate outline (dashed hidden pen)" },
    };
};

pub fn build(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const tp = p.parse(Params) orelse return null;
    const top = tp.top_chord;
    const bot = tp.bottom_chord;
    const theta = units.parseSlope(tp.pitch) orelse {
        p.fail("pitch", "param 'pitch' must be rise:run like \"4:12\" or degrees (got {s})", .{model.kindOrText(a, tp.pitch)});
        return null;
    };
    if (theta <= 0 or theta >= std.math.pi / 2.0) {
        p.fail("pitch", "pitch must slope up (between 0 and 90 degrees)", .{});
        return null;
    }
    const tsz = parseSawn(top) orelse {
        p.fail("top_chord", "top_chord \"{s}\" is not a sawn nominal size like \"2x4\", \"2x6\"", .{top});
        return null;
    };
    const bsz = parseSawn(bot) orelse {
        p.fail("bottom_chord", "bottom_chord \"{s}\" is not a sawn nominal size like \"2x4\", \"2x6\"", .{bot});
        return null;
    };
    const dt = tsz.d;
    const db = bsz.d;
    const s = @tan(theta);
    const c = @cos(theta);
    const raised = tp.heel == .raised;
    const v_thick = dt / c; // vertical thickness of the top chord
    var y_low0 = db; // lower edge of the top chord at x = 0
    if (raised) {
        const hh = tp.heel_height orelse {
            p.fail("heel_height", "heel 'raised' needs 'heel_height' (vertical height at the bearing outer edge from top of bottom chord to top of top chord)", .{});
            return null;
        };
        if (hh < v_thick) {
            p.fail("heel_height", "heel_height {s} is smaller than the top chord's vertical thickness {s}; the raised heel must be at least that tall", .{ fmtNum(a, hh), fmtNum(a, v_thick) });
            return null;
        }
        y_low0 = db + hh - v_thick;
    }
    const x_tail = -tp.overhang;
    const xe = tp.span_shown;
    const lower = struct {
        fn f(x: f64, y0: f64, sl: f64) f64 {
            return y0 + sl * x;
        }
    }.f;
    const tail_bottom = V2.init(x_tail, lower(x_tail, y_low0, s));
    var tail_top = V2.init(x_tail, tail_bottom.y + v_thick);
    var tail_bottom_pt = tail_bottom;
    if (tp.tail == .square) {
        // end cut perpendicular to the chord, hanging through the plumb-cut lower point
        tail_bottom_pt = tail_bottom;
        tail_top = tail_bottom.add(V2.init(-@sin(theta), @cos(theta)).scale(dt));
    }
    const top_loop = try a.dupe(Pt, &.{
        Pt.at(tail_bottom_pt, 0),
        .{ .x = xe, .y = lower(xe, y_low0, s) },
        .{ .x = xe, .y = lower(xe, y_low0, s) + v_thick },
        Pt.at(tail_top, 0),
    });
    const bot_loop = try model.rectLoop(a, 0, 0, xe, db);
    var prisms: std.ArrayList(Prism) = .empty;
    try prisms.append(a, .{ .part = "bottom_chord", .material = "wood", .loops = try model.oneLoop(a, bot_loop) });
    try prisms.append(a, .{ .part = "top_chord", .material = "wood", .loops = try model.oneLoop(a, top_loop) });
    var zones: std.ArrayList(model.Zone) = .empty;
    try zones.append(a, try zoneRect(a, "bottom_chord", 0, 0, xe, db));
    try zones.append(a, .{ .name = "top_chord", .loops = try model.oneLoop(a, top_loop), .box = geom.loopBox(top_loop) });
    if (raised) {
        const web_w = 1.5;
        const web = try a.dupe(Pt, &.{
            .{ .x = 0, .y = db },
            .{ .x = web_w, .y = db },
            .{ .x = web_w, .y = lower(web_w, y_low0, s) },
            .{ .x = 0, .y = y_low0 },
        });
        try prisms.append(a, .{ .part = "heel_web", .material = "wood", .loops = try model.oneLoop(a, web) });
        try zones.append(a, .{ .name = "heel_web", .loops = try model.oneLoop(a, web), .box = geom.loopBox(web) });
    }
    if (tp.plate) {
        const py1 = @max(db + 0.5, y_low0 + 0.5);
        const pl = try model.rectLoop(a, 0.25, 0.25, 5.25, py1);
        try prisms.append(a, .{ .part = "plate", .material = "steel", .loops = try model.oneLoop(a, pl), .kind = .ghost, .pen = .hidden, .embedded = true });
        try zones.append(a, try zoneRect(a, "plate", 0.25, 0.25, 5.25, py1));
    }
    // tail zone: the part of the top chord outside the bearing
    {
        var tb = Box{};
        tb.addPoint(tail_bottom_pt.x, tail_bottom_pt.y);
        tb.addPoint(tail_top.x, tail_top.y);
        tb.addPoint(0, lower(0, y_low0, s));
        tb.addPoint(0, lower(0, y_low0, s) + v_thick);
        try zones.append(a, .{ .name = "tail", .loops = try model.oneLoop(a, try model.rectLoop(a, tb.x0, tb.y0, tb.x1, tb.y1)), .box = tb });
    }
    var anchors: std.ArrayList(model.NamedAnchor) = .empty;
    try anchors.append(a, .{ .name = "bearing_outer", .p = V2.init(0, 0) });
    try anchors.append(a, .{ .name = "bearing_inner", .p = V2.init(tp.bearing_width, 0) });
    try anchors.append(a, .{ .name = "tail_bottom", .p = tail_bottom_pt });
    try anchors.append(a, .{ .name = "tail_top", .p = tail_top });
    try anchors.append(a, .{ .name = "top_chord_at_bearing", .p = V2.init(0, lower(0, y_low0, s) + v_thick) });
    // SPEC 19: where ties and straps land on the heel without literal offsets
    try anchors.append(a, .{ .name = "heel_outer", .p = V2.init(0, (db + lower(0, y_low0, s) + v_thick) / 2) });
    try anchors.append(a, .{ .name = "top_chord_bottom_at_bearing", .p = V2.init(0, lower(0, y_low0, s)) });
    try anchors.append(a, .{ .name = "top_chord_end", .p = V2.init(xe, lower(xe, y_low0, s) + v_thick) });
    try anchors.append(a, .{ .name = "bottom_chord_top_inner", .p = V2.init(xe, db) });
    var box = Box{};
    for (prisms.items) |pr| if (pr.kind != .ghost) {
        for (pr.loops) |l| box.addBox(geom.loopBox(l));
    };
    var built = Built{
        .prisms = prisms.items,
        .anchors = anchors.items,
        .zones = zones.items,
        .box = box,
        .nat_z = tsz.t,
        .info = try a.print("truss {s}:12 {s} heel, {s}+{s} chords, ovh {s}", .{ fmtNum(a, @tan(theta) * 12.0), @tagName(tp.heel), top, bot, ftin(a, tp.overhang) }),
    };
    if (tp.exterior == .right) built = try mirrorBuilt(a, built, geom.Xf.scaling(-1, 1), false);
    return built;
}
