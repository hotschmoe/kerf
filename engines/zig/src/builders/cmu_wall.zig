//! cmu wall (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

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
const zoneRect = common.zoneRect;
const fmtNum = common.fmtNum;
const parseCover = common.parseCover;

pub const Params = struct {
    width: f64 = 8,
    courses: u8,
    bond_beam_courses: u8 = 0,
    grout: enum { solid, reinforced, none } = .reinforced,
    face_shell: f64 = 1.25,
    top_joint: bool = false,
    cover: ?json.Value = null,

    pub const spec = .{
        .width = .{ .desc = "nominal 6, 8, 10, 12 => actual 5.625, 7.625, 9.625, 11.625" },
        .courses = .{ .min = 1, .max = 200, .desc = "number of 8\" courses (7.625 unit + 0.375 mortar joint)" },
        .bond_beam_courses = .{ .min = 0, .max = 200, .desc = "top N courses are bond-beam units (always grouted)" },
        .grout = .{ .desc = "solid | reinforced (bond beams + the cut cell) | none" },
        .face_shell = .{ .len = .pos, .desc = "face shell thickness drawn in section" },
        .top_joint = .{ .desc = "mortar joint above the top course" },
        .cover = .{ .def = "{sides:1.5, top:1.5, bottom:0.5}", .desc = "required clear cover for rebar (W_COVER); supports cover.parts.<part> overrides" },
    };
};

pub fn build(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const cp = p.parseAll(Params);
    const cov = parseCover(ctx, "cover", .{ .bottom = 0.5, .sides = 1.5, .top = 1.5 });
    if (!p.ok) return null;
    const w: f64 = blk: {
        const x = cp.width;
        if (x == 6) break :blk 5.625;
        if (x == 8) break :blk 7.625;
        if (x == 10) break :blk 9.625;
        if (x == 12) break :blk 11.625;
        if (@abs(x - 5.625) < 1e-3 or @abs(x - 7.625) < 1e-3 or @abs(x - 9.625) < 1e-3 or @abs(x - 11.625) < 1e-3) break :blk x;
        p.fail("width", "param 'width' must be a nominal 6, 8, 10 or 12 (actual 5.625, 7.625, 9.625, 11.625) (got {s})", .{fmtNum(a, x)});
        return null;
    };
    const n: usize = cp.courses;
    const nbb: usize = cp.bond_beam_courses;
    if (nbb > n) {
        p.fail("bond_beam_courses", "param 'bond_beam_courses' ({d}) cannot exceed 'courses' ({d})", .{ nbb, n });
        return null;
    }
    if (cp.face_shell * 2 >= w) {
        p.fail("face_shell", "face_shell {s} leaves no cell in a {s} wide wall", .{ fmtNum(a, cp.face_shell), fmtNum(a, w) });
        return null;
    }
    const nf: f64 = @floatFromInt(n);
    const total_h = nf * 8.0 - 0.375 + (if (cp.top_joint) @as(f64, 0.375) else 0);
    const fs = cp.face_shell;
    var prisms: std.ArrayList(Prism) = .empty;
    var zones: std.ArrayList(model.Zone) = .empty;
    var k: usize = 1;
    while (k <= n) : (k += 1) {
        const y0 = @as(f64, @floatFromInt(k - 1)) * 8.0;
        const y1 = y0 + 7.625;
        const is_bb = k > n - nbb;
        const part = try a.print("course_{d}", .{k});
        try prisms.append(a, .{ .part = part, .material = "cmu", .loops = try model.oneLoop(a, try model.rectLoop(a, 0, y0, fs, y1)), .cmu_unit = true, .course = @intCast(k) });
        try prisms.append(a, .{ .part = part, .material = "cmu", .loops = try model.oneLoop(a, try model.rectLoop(a, w - fs, y0, w, y1)), .cmu_unit = true, .course = @intCast(k) });
        const grouted = cp.grout != .none and (is_bb or cp.grout == .solid or cp.grout == .reinforced);
        const cell = try model.oneLoop(a, try model.rectLoop(a, fs, y0, w - fs, y1));
        if (grouted) {
            try prisms.append(a, .{ .part = "grout", .material = "grout", .loops = cell, .cmu_unit = true, .course = @intCast(k) });
        } else {
            try prisms.append(a, .{ .part = part, .material = "cmu", .loops = cell, .kind = .ghost, .pen = .beyond });
        }
        if (k < n or cp.top_joint) {
            try prisms.append(a, .{ .part = try a.print("joint_{d}", .{k}), .material = "mortar", .loops = try model.oneLoop(a, try model.rectLoop(a, 0, y1, w, y1 + 0.375)) });
        }
        try zones.append(a, try zoneRect(a, part, 0, y0, w, y1));
    }
    var bb_y0: f64 = total_h;
    var bb_center = V2.init(w / 2, total_h);
    if (nbb > 0) {
        bb_y0 = @as(f64, @floatFromInt(n - nbb)) * 8.0;
        const y1 = nf * 8.0 - 0.375;
        try zones.append(a, try zoneRect(a, "bond_beam", 0, bb_y0, w, y1));
        bb_center = V2.init(w / 2, (bb_y0 + y1) / 2);
    }
    // grout zone: the cell over the grouted span
    {
        var gy0: f64 = std.math.inf(f64);
        var gy1: f64 = -std.math.inf(f64);
        for (prisms.items) |pr| if (std.mem.eql(u8, pr.part, "grout")) {
            const bx = geom.loopBox(pr.loops[0]);
            gy0 = @min(gy0, bx.y0);
            gy1 = @max(gy1, bx.y1);
        };
        if (gy0 < gy1) try zones.append(a, try zoneRect(a, "grout", fs, gy0, w - fs, gy1));
    }
    const anchors = try a.dupe(model.NamedAnchor, &.{
        .{ .name = "bond_beam_center", .p = bb_center },
        .{ .name = "top_center", .p = V2.init(w / 2, total_h) },
        .{ .name = "cell_center_top", .p = V2.init(w / 2, total_h) },
    });
    const outline = try model.rectLoop(a, 0, 0, w, total_h);
    const nom: f64 = if (@abs(w - 5.625) < 1e-3) 6 else if (@abs(w - 7.625) < 1e-3) 8 else if (@abs(w - 9.625) < 1e-3) 10 else 12;
    return .{
        .prisms = prisms.items,
        .anchors = anchors,
        .zones = zones.items,
        .box = .{ .x0 = 0, .y0 = 0, .x1 = w, .y1 = total_h },
        .host = .{ .outline = outline, .cover = cov.?.cover, .part_cover = cov.?.parts },
        .info = try a.print("cmu_wall {s}\" x {d} courses{s}", .{ fmtNum(a, nom), n, if (nbb > 0) try a.print(" ({d} bond beam)", .{nbb}) else "" }),
    };
}
