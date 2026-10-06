//! rebar (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

const std = @import("std");
const json = @import("../json.zig");
const geom = @import("../geom.zig");
const model = @import("../model.zig");
const units = @import("../units.zig");
const cast = @import("../num.zig");
const scene_mod = @import("../scene.zig");
const path_geom = @import("../pathgeom.zig");
const common = @import("common.zig");
const V2 = geom.V2;
const Built = model.Built;
const Prism = model.Prism;
const Box = geom.Box;
const Ctx = common.Ctx;
const BuildError = common.BuildError;
const onePrism = common.onePrism;
const ftin = common.ftin;
const fmtNum = common.fmtNum;
const parsePointList = common.parsePointList;
const dropDuplicatePoints = common.dropDuplicatePoints;
const worldBox = common.worldBox;
const pathLen = common.pathLen;

pub fn rebarDiameter(size: []const u8) ?f64 {
    if (size.len < 2 or size[0] != '#') return null;
    const n = std.fmt.parseInt(u32, size[1..], 10) catch return null;
    return switch (n) {
        3 => 0.375,
        4 => 0.5,
        5 => 0.625,
        6 => 0.75,
        7 => 0.875,
        8 => 1.0,
        9 => 1.128,
        10 => 1.27,
        11 => 1.41,
        else => null,
    };
}

pub const Params = struct {
    size: []const u8 = "#4",
    mode: enum { along_z, path } = .along_z,
    place: ?json.Value = null,
    points: ?json.Value = null,
    bend_radius: ?f64 = null,
    spacing_note: []const u8 = "",

    pub const spec = .{
        .size = .{ .desc = "#3 .375, #4 .5, #5 .625, #6 .75, #7 .875, #8 1.0 (diameter in)" },
        .mode = .{ .desc = "along_z (continuous bar seen as a dot) or path (bar in the XY plane)" },
        .place = .{ .desc = "cover-based placement (preferred): {in: \"comp[.part]\", face: bottom|top|left|right|center, cover: 3, count: 2, side_cover: cover, axis: x|y, station: in}. bottom/top/left/right: bars at clear `cover` from that face, spread evenly between the zone's adjacent faces at `side_cover` (count 1 centers). center: bars centered in the zone on both axes (e.g. a single #4 in the middle of a stem wall); count > 1 spreads along axis x (default) or y at side_cover. station: ONE bar at that offset from the zone's left face (bottom face with axis y; for bottom/top/left/right faces it sets the along-face position)" },
        .points = .{ .def = "path: required", .desc = "polyline [x,y] or Refs; bends get radius bend_radius, drawn as fillets" },
        .bend_radius = .{ .len = .pos, .def = "3*d_b", .desc = "inside bend radius for path bars" },
        .spacing_note = .{ .def = "null", .desc = "e.g. \"#4 @ 16\\\" O.C.\" for summaries and notes" },
    };
};

pub fn build(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const rp = p.parse(Params) orelse return null;
    const size = rp.size;
    const d = rebarDiameter(size) orelse {
        p.fail("size", "bar size '{s}' is not recognised. Use #3 (.375), #4 (.5), #5 (.625), #6 (.75), #7 (.875) or #8 (1.0)", .{size});
        return null;
    };
    const r = d / 2.0;
    if (rp.mode == .along_z) {
        const loop = try model.circleLoop(a, 0, 0, r);
        const prism = Prism{ .material = "rebar", .loops = try model.oneLoop(a, loop), .embedded = true, .sweep_r = r };
        var built = Built{
            .prisms = try onePrism(a, prism),
            .box = .{ .x0 = -r, .y0 = -r, .x1 = r, .y1 = r },
            .bar_d = d,
            .info = try std.fmt.allocPrint(a, "rebar {s} along z", .{size}),
        };
        if (rp.place) |pl| {
            built.centers = (try placeRebar(ctx, pl, d)) orelse return null;
            const face = if (pl.get("face")) |f| (f.str() orelse "bottom") else "bottom";
            built.info = try std.fmt.allocPrint(a, "rebar ({d}) {s} along z @ {s} face", .{ built.centers.len, size, face });
        }
        return built;
    }
    // path
    const pv = rp.points orelse {
        p.fail("points", "mode path needs 'points': [[x, y], ...] or Refs", .{});
        return null;
    };
    const pts = (try parsePointList(ctx, "points", pv, true)) orelse return null;
    const clean = try dropDuplicatePoints(a, pts);
    if (clean.len < 2) {
        p.fail("points", "a path bar needs at least 2 distinct points (got {d})", .{clean.len});
        return null;
    }
    const br = rp.bend_radius orelse 3.0 * d;
    const vs = try a.alloc(V2, clean.len);
    for (clean, 0..) |q, i| vs[i] = q.v();
    const center = try path_geom.fillet(a, vs, br + r);
    const rib = try path_geom.ribbon(a, center, r, r);
    const prism = Prism{
        .material = "rebar",
        .loops = try model.oneLoop(a, rib),
        .embedded = true,
        .centerline = center,
        .sweep_r = r,
    };
    return .{
        .prisms = try onePrism(a, prism),
        .box = geom.loopBox(rib),
        .nat_z = d,
        .points_mode = true,
        .bar_d = d,
        .info = try std.fmt.allocPrint(a, "rebar {s} path L={s}{s}", .{ size, ftin(a, pathLen(vs)), if (rp.spacing_note.len > 0) try std.fmt.allocPrint(a, " ({s})", .{rp.spacing_note}) else "" }),
    };
}

/// Cover-based placement: returns the world centres of the bars.
fn placeRebar(ctx: *Ctx, pl: json.Value, d: f64) BuildError!?[]const V2 {
    const a = ctx.a;
    const p = &ctx.p;
    if (pl != .object) {
        p.fail("place", "param 'place' must be {{\"in\": \"comp[.part]\", \"face\": \"bottom|top|left|right|center\", \"cover\": 3, \"count\": 2, \"side_cover\": 3}}", .{});
        return null;
    }
    const in_s = (if (pl.get("in")) |x| x.str() else null) orelse {
        p.fail("place/in", "place needs \"in\": \"<component>[.<part>]\", the host zone", .{});
        return null;
    };
    const face = (if (pl.get("face")) |x| x.str() else null) orelse "bottom";
    const cover = p.fieldLen("place", pl, "cover", 1.5) orelse return null;
    const side_cover = p.fieldLen("place", pl, "side_cover", cover) orelse return null;
    const count_i = p.fieldInt("place", pl, "count", 1, 1, 200) orelse return null;
    const count: usize = cast.toIntClamped(usize, @floatFromInt(count_i), 1, 200);
    const axis = (if (pl.get("axis")) |x| x.str() else null) orelse "x";
    if (!std.mem.eql(u8, axis, "x") and !std.mem.eql(u8, axis, "y")) {
        p.fail("place/axis", "place.axis must be \"x\" or \"y\" (got \"{s}\"): the direction a multi-bar row spreads for face \"center\"", .{axis});
        return null;
    }
    const station: ?f64 = if (pl.get("station")) |x| (units.parseLength(x) orelse {
        p.fail("place/station", "place.station must be a length measured from the zone's left face (bottom face with axis \"y\"), e.g. 4 or \"4\\\"\"", .{});
        return null;
    }) else null;
    if (station != null and count > 1) {
        p.fail("place/station", "place.station positions ONE bar; use count 1 (or drop station to spread {d} bars between the faces)", .{count});
        return null;
    }
    var host_id = in_s;
    var part: ?[]const u8 = null;
    if (std.mem.indexOfScalar(u8, in_s, '.')) |dot| {
        host_id = in_s[0..dot];
        part = in_s[dot + 1 ..];
    }
    const host = ctx.scene.find(host_id) orelse {
        const ids = try ctx.scene.compIds(a);
        const path = try std.fmt.allocPrint(a, "{s}/{s}/place/in", .{ p.base, p.id });
        if (model.nearest(a, host_id, ids)) |nn| {
            ctx.scene.diags.addFix(.@"error", "E_REF_UNKNOWN", p.id, path, "place.in: no component '{s}'; did you mean '{s}'?", .{ host_id, nn }, nn);
        } else {
            ctx.scene.diags.add(.@"error", "E_REF_UNKNOWN", p.id, path, "place.in: no component '{s}'. Known ids: {s}", .{ host_id, scene_mod.joinIds(a, ids) });
        }
        p.ok = false;
        return null;
    };
    if (host.state != .ok) {
        p.fail("place/in", "place.in host '{s}' did not build; fix its errors first", .{host_id});
        return null;
    }
    var box: Box = undefined;
    if (part) |pt| {
        const lb = scene_mod.Scene.partBox(host, pt) orelse {
            const parts = try ctx.scene.partNames(host);
            const path = try std.fmt.allocPrint(a, "{s}/{s}/place/in", .{ p.base, p.id });
            ctx.scene.diags.add(.@"error", "E_REF_UNKNOWN", p.id, path, "place.in: component '{s}' has no part '{s}'. Parts: {s}", .{ host_id, pt, if (parts.len == 0) "(none)" else scene_mod.joinIds(a, parts) });
            p.ok = false;
            return null;
        };
        box = worldBox(host.xfs[0], lb);
    } else {
        box = worldBox(host.xfs[0], host.built.box);
    }
    const r = d / 2.0;
    var out: std.ArrayList(V2) = .empty;
    const eq = std.mem.eql;
    if (eq(u8, face, "center")) {
        // Centered in the zone on both axes; a row of bars spreads along `axis` at side_cover; station moves one bar.
        const along_y = eq(u8, axis, "y");
        const lo = (if (along_y) box.y0 else box.x0) + side_cover + r;
        const hi = (if (along_y) box.y1 else box.x1) - side_cover - r;
        if (hi < lo - 1e-9 and count > 1) {
            p.fail("place", "zone '{s}' is {s} {s}: bars at side_cover {s} do not fit; reduce side_cover or count", .{ in_s, fmtNum(a, if (along_y) box.height() else box.width()), if (along_y) "tall" else "wide", fmtNum(a, side_cover) });
            return null;
        }
        const mid_x = (box.x0 + box.x1) / 2;
        const mid_y = (box.y0 + box.y1) / 2;
        var i: usize = 0;
        while (i < count) : (i += 1) {
            const t: f64 = if (count == 1) 0.5 else @as(f64, @floatFromInt(i)) / @as(f64, @floatFromInt(count - 1));
            if (along_y) {
                const y = if (station) |st| box.y0 + st else lo + (hi - lo) * t;
                try out.append(a, V2.init(mid_x, y));
            } else {
                const x = if (station) |st| box.x0 + st else lo + (hi - lo) * t;
                try out.append(a, V2.init(x, mid_y));
            }
        }
    } else if (eq(u8, face, "bottom") or eq(u8, face, "top")) {
        const y = if (eq(u8, face, "bottom")) box.y0 + cover + r else box.y1 - cover - r;
        const xa = box.x0 + side_cover + r;
        const xb = box.x1 - side_cover - r;
        if (xb < xa - 1e-9 and count > 1) {
            p.fail("place", "zone '{s}' is {s} wide: bars at side_cover {s} do not fit ({s} usable); reduce side_cover or count", .{ in_s, fmtNum(a, box.width()), fmtNum(a, side_cover), fmtNum(a, xb - xa) });
            return null;
        }
        var i: usize = 0;
        while (i < count) : (i += 1) {
            const x = if (station) |st| box.x0 + st else if (count == 1) (box.x0 + box.x1) / 2 else xa + (xb - xa) * @as(f64, @floatFromInt(i)) / @as(f64, @floatFromInt(count - 1));
            try out.append(a, V2.init(x, y));
        }
    } else if (eq(u8, face, "left") or eq(u8, face, "right")) {
        const x = if (eq(u8, face, "left")) box.x0 + cover + r else box.x1 - cover - r;
        const ya = box.y0 + side_cover + r;
        const yb = box.y1 - side_cover - r;
        if (yb < ya - 1e-9 and count > 1) {
            p.fail("place", "zone '{s}' is {s} tall: bars at side_cover {s} do not fit; reduce side_cover or count", .{ in_s, fmtNum(a, box.height()), fmtNum(a, side_cover) });
            return null;
        }
        var i: usize = 0;
        while (i < count) : (i += 1) {
            const y = if (station) |st| box.y0 + st else if (count == 1) (box.y0 + box.y1) / 2 else ya + (yb - ya) * @as(f64, @floatFromInt(i)) / @as(f64, @floatFromInt(count - 1));
            try out.append(a, V2.init(x, y));
        }
    } else {
        p.fail("place/face", "place.face must be one of \"bottom\", \"top\", \"left\", \"right\", \"center\" (got \"{s}\")", .{face});
        return null;
    }
    return out.items;
}

test "rebar sizes" {
    try std.testing.expectEqual(@as(f64, 0.625), rebarDiameter("#5").?);
    try std.testing.expect(rebarDiameter("5") == null);
}
