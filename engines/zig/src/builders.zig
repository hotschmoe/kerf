//! Component builders (SPEC 5): turn a component's JSON params into a `Built` (prisms, anchors,
//! zones) in local coordinates. Builders validate their params and report E_PARAM diagnostics;
//! they return null when the component cannot be built.

const std = @import("std");
const json = @import("json.zig");
const geom = @import("geom.zig");
const model = @import("model.zig");
const units = @import("units.zig");
const cast = @import("num.zig");
const limits = @import("limits.zig");
const catalog = @import("catalog.zig");
const params_mod = @import("params.zig");
const scene_mod = @import("scene.zig");
const style_mod = @import("style.zig");
const path_geom = @import("pathgeom.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Pt = geom.Pt;
const Built = model.Built;
const Prism = model.Prism;
const Params = model.Params;
const Box = geom.Box;
const common = @import("builders/common.zig");
pub const insulation = @import("builders/insulation.zig");
pub const fill = @import("builders/fill.zig");
pub const membrane = @import("builders/membrane.zig");
pub const truss = @import("builders/truss.zig");
pub const connector = @import("builders/connector.zig");
pub const anchor_bolt = @import("builders/anchor_bolt.zig");
pub const rebar = @import("builders/rebar.zig");
pub const concrete = @import("builders/concrete.zig");
pub const cmu_wall = @import("builders/cmu_wall.zig");
pub const panel = @import("builders/panel.zig");
pub const lumber = @import("builders/lumber.zig");
pub const BuildError = common.BuildError;
pub const Ctx = common.Ctx;
pub const mirrorAboutCenter = common.mirrorAboutCenter;
pub const worldBox = common.worldBox;
const boxOfPrisms = common.boxOfPrisms;
const zoneRect = common.zoneRect;
const onePrism = common.onePrism;
const ftin = common.ftin;
const fmtNum = common.fmtNum;
const quadOf = common.quadOf;
const parseSawn = common.parseSawn;
const parseActual = common.parseActual;
const parsePointList = common.parsePointList;
const dropDuplicatePoints = common.dropDuplicatePoints;
const orientedCcw = common.orientedCcw;
const materialOk = common.materialOk;
const lengthOrUntil = common.lengthOrUntil;
const parseCover = common.parseCover;
const mirrorBuilt = common.mirrorBuilt;
const gaugeThickness = common.gaugeThickness;
const vsOf = common.vsOf;
const pathLen = common.pathLen;
const materialOr = common.materialOr;

// ---- helpers ---------------------------------------------------------------------------------------

// ---- lumber -------------------------------------------------------------------------------------------

// ---- panel -----------------------------------------------------------------------------------------------

// ---- cover parsing ----------------------------------------------------------------------------------------

// ---- cmu_wall -----------------------------------------------------------------------------------------------

// ---- concrete ---------------------------------------------------------------------------------------------------

// ---- rebar --------------------------------------------------------------------------------------------------------

// ---- anchor bolt ----------------------------------------------------------------------------------------------------

// ---- connector ------------------------------------------------------------------------------------------------------

// ---- truss -----------------------------------------------------------------------------------------------------------

// ---- membrane -----------------------------------------------------------------------------------------------------------

// ---- fill ------------------------------------------------------------------------------------------------------------------

// ---- insulation ----------------------------------------------------------------------------------------------------------------

// ---- solid ------------------------------------------------------------------------------------------------------------------------

pub const SolidParams = struct {
    profile: ?json.Value = null,
    material: []const u8,

    pub const spec = .{
        .profile = .{ .def = "required", .desc = "{rect:[w,h]} | {circle:d} | {points:[...]}" },
        .material = .{ .desc = "any style material (aluminum, steel, ...)" },
    };
};

fn buildSolid(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const sp = p.parseAll(SolidParams);
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
        .info = try std.fmt.allocPrint(a, "solid {s} (escape hatch)", .{sp.material}),
    };
}

// ---- flashing -----------------------------------------------------------------------------------------------------

pub const FlashingParams = struct {
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

fn buildFlashing(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const fp = p.parse(FlashingParams) orelse return null;
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

// ---- joint --------------------------------------------------------------------------------------------------------

pub const JointParams = struct {
    kind: enum { expansion, control, tooled_edge, sealant },
    width: ?f64 = null,
    depth: ?f64 = null,
    in: ?json.Value = null,
    cap: f64 = 0,
    radius: f64 = 0.25,
    corner: enum { top_right, top_left, bottom_right, bottom_left } = .top_right,
    backer_rod: bool = true,

    pub const spec = .{
        .kind = .{ .desc = "expansion | control | tooled_edge | sealant" },
        .width = .{ .len = .pos, .def = "0.5 (control 0.25)", .desc = "expansion: filler thickness; control: notch width at the top; sealant: joint gap width" },
        .depth = .{ .len = .pos, .def = "expansion 4, control 1, sealant 0.25", .desc = "expansion: filler depth below the top (set to the slab thickness, or give `in`); control: notch depth (default 1/4 of the `in` zone height); sealant: bead depth" },
        .in = .{ .desc = "optional host zone \"comp[.part]\" whose height sets the default depth (expansion: full height; control: 1/4)" },
        .cap = .{ .len = .any, .desc = "expansion: depth of a sealant cap at the top of the filler (part `sealant`)" },
        .radius = .{ .len = .pos, .desc = "tooled_edge: radius of the rounded corner" },
        .corner = .{ .desc = "tooled_edge: which corner of the concrete the point is: top_right (concrete lies left and below), top_left, bottom_right, bottom_left" },
        .backer_rod = .{ .desc = "sealant: draw the backer rod circle (diameter 1.25*width) below the bead" },
    };
};

fn buildJoint(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const jp = p.parse(JointParams) orelse return null;
    const k = jp.kind;
    // optional host zone: default depth = slab thickness (expansion) or a quarter of it (control)
    var host_h: f64 = 0;
    if (jp.in) |iv| {
        const in_s = iv.str() orelse {
            p.fail("in", "param 'in' must be \"<component>[.<part>]\", the concrete zone the joint cuts (its height sets the default depth)", .{});
            return null;
        };
        var hid = in_s;
        var part: ?[]const u8 = null;
        if (std.mem.indexOfScalar(u8, in_s, '.')) |dot| {
            hid = in_s[0..dot];
            part = in_s[dot + 1 ..];
        }
        const host = ctx.scene.find(hid) orelse {
            const ids = try ctx.scene.compIds(a);
            p.fail("in", "no component '{s}'. Known ids: {s}", .{ hid, scene_mod.joinIds(a, ids) });
            return null;
        };
        if (host.state != .ok) {
            p.fail("in", "host '{s}' did not build; fix its errors first", .{hid});
            return null;
        }
        const bx = if (part) |pt| (scene_mod.Scene.partBox(host, pt) orelse {
            p.fail("in", "component '{s}' has no part '{s}'", .{ hid, pt });
            return null;
        }) else host.built.box;
        host_h = bx.height();
    }
    const width: f64 = jp.width orelse if (k == .control) 0.25 else 0.5;
    const depth_def: f64 = switch (k) {
        .control => if (host_h > 0) host_h / 4.0 else 1.0,
        .sealant => @max(0.25, 0.5 * width),
        else => if (host_h > 0) host_h else 4.0,
    };
    const depth: f64 = jp.depth orelse depth_def;
    const radius = jp.radius;
    const cap = jp.cap;
    const rod = jp.backer_rod;
    var prisms: std.ArrayList(Prism) = .empty;
    var anchors: std.ArrayList(model.NamedAnchor) = .empty;
    try anchors.append(a, .{ .name = "joint_top", .p = V2.init(0, 0) });
    const void_mat = materialOr(ctx, "void", "generic");
    if (k == .expansion) {
        if (cap < 0 or cap >= depth) {
            p.fail("cap", "cap (sealant depth at the top of the joint) must be from 0 to less than depth {s} (got {s})", .{ fmtNum(a, depth), fmtNum(a, cap) });
            return null;
        }
        const hw = width / 2;
        try prisms.append(a, .{ .part = "filler", .material = materialOr(ctx, "joint_filler", "generic"), .loops = try model.oneLoop(a, try model.rectLoop(a, -hw, -depth, hw, -cap)) });
        if (cap > 0) try prisms.append(a, .{ .part = "sealant", .material = materialOr(ctx, "sealant", "steel"), .loops = try model.oneLoop(a, try model.rectLoop(a, -hw, -cap, hw, 0)) });
    } else if (k == .control) {
        const tri = [_]Pt{ .{ .x = -width / 2, .y = 0 }, .{ .x = 0, .y = -depth }, .{ .x = width / 2, .y = 0 } };
        try prisms.append(a, .{ .part = "notch", .material = void_mat, .loops = try model.oneLoop(a, try orientedCcw(a, &tri)), .embedded = true });
    } else if (k == .tooled_edge) {
        // the sliver between the sharp corner and the radius, drawn for the top-right corner then flipped into place
        const r = radius;
        const loop0 = [_]Pt{ .{ .x = 0, .y = 0 }, .{ .x = -r, .y = 0, .b = geom.bulgeFromSweep(-std.math.pi / 2.0) }, .{ .x = 0, .y = -r } };
        const c = jp.corner;
        const sx: f64 = if (c == .top_left or c == .bottom_left) -1 else 1;
        const sy: f64 = if (c == .bottom_left or c == .bottom_right) -1 else 1;
        const xf = geom.Xf.scaling(sx, sy);
        const loop = try orientedCcw(a, try xf.applyLoop(a, &loop0));
        try prisms.append(a, .{ .part = "radius", .material = void_mat, .loops = try model.oneLoop(a, loop), .embedded = true });
        try anchors.append(a, .{ .name = "corner", .p = V2.init(0, 0) });
    } else {
        // sealant bead over a backer rod, in a gap of `width`
        const hw = width / 2;
        try prisms.append(a, .{ .part = "bead", .material = materialOr(ctx, "sealant", "steel"), .loops = try model.oneLoop(a, try model.rectLoop(a, -hw, -depth, hw, 0)) });
        if (rod) {
            const rd = 1.25 * width;
            const loop = try model.circleLoop(a, 0, -depth - rd / 2, rd / 2);
            try prisms.append(a, .{ .part = "rod", .material = materialOr(ctx, "backer_rod", "generic"), .loops = try model.oneLoop(a, loop) });
        }
    }
    return .{
        .prisms = prisms.items,
        .anchors = anchors.items,
        .box = boxOfPrisms(prisms.items),
        .info = try std.fmt.allocPrint(a, "joint {s} {s} wide x {s} deep", .{ @tagName(k), ftin(a, width), ftin(a, depth) }),
    };
}

// ---- dispatch -----------------------------------------------------------------------------------------------------------------------

pub fn build(ctx: *Ctx) BuildError!?Built {
    return switch (ctx.comp.ty.type) {
        .lumber => lumber.build(ctx),
        .panel => panel.build(ctx),
        .cmu_wall => cmu_wall.build(ctx),
        .concrete => concrete.build(ctx),
        .rebar => rebar.build(ctx),
        .anchor_bolt => anchor_bolt.build(ctx),
        .connector => connector.build(ctx),
        .truss => truss.build(ctx),
        .membrane => membrane.build(ctx),
        .fill => fill.build(ctx),
        .insulation => insulation.build(ctx),
        .solid => buildSolid(ctx),
        .flashing => buildFlashing(ctx),
        .joint => buildJoint(ctx),
    };
}

/// The parameter struct of every component type, in `catalog.Type` order.
pub const param_structs = .{ lumber.Params, panel.Params, cmu_wall.Params, concrete.Params, rebar.Params, anchor_bolt.Params, connector.Params, truss.Params, membrane.Params, fill.Params, insulation.Params, SolidParams, FlashingParams, JointParams };

comptime {
    if (param_structs.len != std.meta.tags(catalog.Type).len) @compileError("builders.param_structs must have one struct per catalog.Type tag");
}

test "every choice of an enum parameter is mentioned in its catalog row (docs cannot drift from the parser)" {
    inline for (param_structs) |S| {
        const rows = params_mod.rows(S);
        inline for (@typeInfo(S).@"struct".fields) |f| {
            const Base = switch (@typeInfo(f.type)) {
                .optional => |o| o.child,
                else => f.type,
            };
            if (@typeInfo(Base) == .@"enum") {
                var found_row = false;
                for (rows) |r| {
                    if (!std.mem.eql(u8, r.names[0], f.name)) continue;
                    found_row = true;
                    inline for (@typeInfo(Base).@"enum".fields) |ef| {
                        if (std.mem.indexOf(u8, r.desc, ef.name) == null and std.mem.indexOf(u8, r.def, ef.name) == null) {
                            std.debug.print("{s}.{s}: choice '{s}' is not in its catalog text\n", .{ @typeName(S), f.name, ef.name });
                            return error.TestUnexpectedResult;
                        }
                    }
                }
                try std.testing.expect(found_row);
            }
        }
    }
}
