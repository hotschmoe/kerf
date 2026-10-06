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

pub const BuildError = Allocator.Error;

pub const Ctx = struct {
    a: Allocator,
    scene: *scene_mod.Scene,
    style: *const style_mod.Style,
    comp: *scene_mod.Comp,
    p: Params,
    /// Placement point (world) used as the origin of point-list builders.
    origin: V2,
    run: [2]f64,
    /// Placement rotation (radians CCW): `until` measures along the member's rotated run direction.
    angle: f64 = 0,
};

// ---- helpers ---------------------------------------------------------------------------------------

fn boxOfPrisms(prisms: []const Prism) Box {
    var b = Box{};
    for (prisms) |p| for (p.loops) |l| b.addBox(geom.loopBox(l));
    return b;
}

fn zoneRect(a: Allocator, name: []const u8, x0: f64, y0: f64, x1: f64, y1: f64) Allocator.Error!model.Zone {
    const l = try model.rectLoop(a, x0, y0, x1, y1);
    return .{ .name = name, .loops = try model.oneLoop(a, l), .box = .{ .x0 = x0, .y0 = y0, .x1 = x1, .y1 = y1 } };
}

fn onePrism(a: Allocator, prism: Prism) Allocator.Error![]Prism {
    const out = try a.alloc(Prism, 1);
    out[0] = prism;
    return out;
}

const ftin = units.ftin;

fn fmtNum(a: Allocator, x: f64) []const u8 {
    return json.fmtNumberAlloc(a, x) catch "?";
}

/// Rect quads for cut marks. Corners CCW from bottom-left.
fn quadOf(x0: f64, y0: f64, x1: f64, y1: f64) [4]V2 {
    return .{ V2.init(x0, y0), V2.init(x1, y0), V2.init(x1, y1), V2.init(x0, y1) };
}

const SawnSize = struct { t: f64, d: f64 };

fn sawnDepth(nominal_t: u32, nd: u32) ?f64 {
    if (nominal_t >= 6) {
        return switch (nd) {
            6 => 5.5,
            8 => 7.5,
            10 => 9.5,
            12 => 11.5,
            else => null,
        };
    }
    return switch (nd) {
        3 => 2.5,
        4 => 3.5,
        6 => 5.5,
        8 => 7.25,
        10 => 9.25,
        12 => 11.25,
        14 => 13.25,
        else => null,
    };
}

fn sawnThickness(nt: u32) ?f64 {
    return switch (nt) {
        1 => 0.75,
        2 => 1.5,
        3 => 2.5,
        4 => 3.5,
        6 => 5.5,
        8 => 7.25,
        else => null,
    };
}

/// Parse `NxM` as a sawn nominal size. Null if it is not a recognised nominal size.
pub fn parseSawn(size: []const u8) ?SawnSize {
    const xi = std.mem.indexOfAny(u8, size, "xX") orelse return null;
    const a = std.fmt.parseInt(u32, std.mem.trim(u8, size[0..xi], " "), 10) catch return null;
    const b = std.fmt.parseInt(u32, std.mem.trim(u8, size[xi + 1 ..], " "), 10) catch return null;
    const t = sawnThickness(a) orelse return null;
    const d = sawnDepth(a, b) orelse return null;
    return .{ .t = t, .d = d };
}

/// Parse `AxB` actual sizes (lengths, e.g. "1.75x11.875" or "1 3/4x9 1/4").
pub fn parseActual(size: []const u8) ?SawnSize {
    const xi = std.mem.indexOfAny(u8, size, "xX\xc3") orelse return null;
    var rest = size[xi + 1 ..];
    if (size[xi] == 0xc3 and rest.len > 0) rest = rest[1..]; // UTF-8 multiplication sign
    const a = units.parseLengthStr(size[0..xi]) orelse return null;
    const b = units.parseLengthStr(rest) orelse return null;
    if (a <= 0 or b <= 0 or !units.lengthInRange(a) or !units.lengthInRange(b)) return null;
    return .{ .t = a, .d = b };
}

/// Accepted magnitude of a polygon bulge: below the minimum the arc is a straight line to within rounding (and its radius overflows
/// the arc flattener), above the maximum it is a near-full circle with an absurd radius (REVIEW SAF-5).
const min_bulge: f64 = 1e-6;
const max_bulge: f64 = 1e3;

fn pointsOr(ctx: *Ctx, key: []const u8, v: json.Value) BuildError!?[]Pt {
    return parsePointList(ctx, key, v, true);
}

/// Parse a points list. Literal [x,y(,b)] entries are relative to the placement point; Refs are
/// absolute. The result is in local coordinates (world - origin for Refs).
pub fn parsePointList(ctx: *Ctx, key: []const u8, v: json.Value, local: bool) BuildError!?[]Pt {
    const arr = v.arr() orelse {
        ctx.p.fail(key, "param '{s}' must be an array of points: [x, y], [x, y, bulge], \"comp@anchor\" or {{\"ref\": \"comp@anchor\", \"offset\": [dx, dy]}}", .{key});
        return null;
    };
    if (arr.len > limits.max_points) {
        ctx.p.failCode("E_LIMIT", key, "{s}. Fix: simplify the outline (a drawing detail rarely needs more than a few dozen vertices), or split it into several components", .{try limits.message(ctx.a, try std.fmt.allocPrint(ctx.a, "points in '{s}'", .{key}), arr.len, limits.max_points, "")});
        return null;
    }
    var out: std.ArrayList(Pt) = .empty;
    var ok = true;
    for (arr, 0..) |e, i| {
        const path = try std.fmt.allocPrint(ctx.a, "{s}/{d}", .{ key, i });
        const fpath = try std.fmt.allocPrint(ctx.a, "{s}/{s}/{s}", .{ ctx.p.base, ctx.p.id, path });
        switch (e) {
            .array => |xy| {
                if (xy.len < 2 or xy.len > 3) {
                    ctx.p.fail(path, "point {d} of '{s}' must be [x, y] or [x, y, bulge]", .{ i, key });
                    ok = false;
                    continue;
                }
                const x = units.parseLength(xy[0]);
                const y = units.parseLength(xy[1]);
                if (x == null or y == null) {
                    const bad = if (x == null) xy[0] else xy[1];
                    ctx.p.fail(path, "point {d} of '{s}': a coordinate must be a length within +-{d} inches (got {s})", .{ i, key, limits.max_coord_in, model.kindOrText(ctx.a, bad) });
                    ok = false;
                    continue;
                }
                var b: f64 = 0;
                if (xy.len == 3) {
                    const bn = xy[2].num() orelse {
                        ctx.p.fail(path, "point {d} of '{s}': the third value is the bulge and must be a number (got {s}); use [x, y] for a straight edge", .{ i, key, model.kindOrText(ctx.a, xy[2]) });
                        ok = false;
                        continue;
                    };
                    if (bn != 0 and !(@abs(bn) >= min_bulge and @abs(bn) <= max_bulge)) {
                        ctx.p.fail(path, "point {d} of '{s}': bulge {s} is out of range. Use 0 (or omit it) for a straight edge, or a value with {d} <= |bulge| <= {d} (bulge = tan(sweep/4): 1 is a half circle, 0.4142 a quarter circle; the sign picks the side)", .{ i, key, model.numText(ctx.a, bn), min_bulge, max_bulge });
                        ok = false;
                        continue;
                    }
                    b = bn;
                }
                try out.append(ctx.a, .{ .x = x.?, .y = y.?, .b = b });
            },
            .string => |s| {
                if (ctx.scene.resolveRefStr(s, ctx.p.id, fpath)) |w| {
                    const q = if (local) w.sub(ctx.origin) else w;
                    try out.append(ctx.a, .{ .x = q.x, .y = q.y });
                } else ok = false;
            },
            .object => {
                const rs = e.get("ref") orelse {
                    ctx.p.fail(path, "point {d} of '{s}': an object point needs a \"ref\"", .{ i, key });
                    ok = false;
                    continue;
                };
                const rstr = rs.str() orelse {
                    ctx.p.fail(path, "point {d} of '{s}': \"ref\" must be a string", .{ i, key });
                    ok = false;
                    continue;
                };
                if (ctx.scene.resolveRefStr(rstr, ctx.p.id, fpath)) |w| {
                    var q = w;
                    if (e.get("offset")) |off| if (off != .null) {
                        q = q.add(ctx.p.offsetPair(try std.fmt.allocPrint(ctx.a, "{s}/offset", .{path}), off) orelse {
                            ok = false;
                            continue;
                        });
                    };
                    if (local) q = q.sub(ctx.origin);
                    try out.append(ctx.a, .{ .x = q.x, .y = q.y });
                } else ok = false;
            },
            else => {
                ctx.p.fail(path, "point {d} of '{s}' must be [x, y], a Ref string or {{ref, offset}}", .{ i, key });
                ok = false;
            },
        }
    }
    if (!ok) return null;
    return out.items;
}

fn dropDuplicatePoints(a: Allocator, pts: []const Pt) Allocator.Error![]Pt {
    var out: std.ArrayList(Pt) = .empty;
    for (pts) |p| {
        if (out.items.len > 0 and V2.eql(out.items[out.items.len - 1].v(), p.v(), 1e-9)) continue;
        try out.append(a, p);
    }
    return out.items;
}

fn orientedCcw(a: Allocator, loop: []const Pt) Allocator.Error![]const Pt {
    if (geom.signedArea(loop) < 0) return geom.reverseLoop(a, loop);
    return loop;
}

fn materialOk(ctx: *Ctx, key: []const u8, name: []const u8) bool {
    if (ctx.style.material(name) != null) return true;
    var names: std.ArrayList([]const u8) = .empty;
    for (ctx.style.materials) |m| names.append(ctx.a, m.name) catch {};
    ctx.p.fail(key, "material '{s}' is not defined in the style. Available: {s}", .{ name, scene_mod.joinIds(ctx.a, names.items) });
    return false;
}

/// `length` or `until` (SPEC 17) for members that run along x or y. Returns the length in inches.
fn lengthOrUntil(ctx: *Ctx, run: []const u8, hint: []const u8, note: *[]const u8) BuildError!?f64 {
    const p = &ctx.p;
    const a = ctx.a;
    const has_len = p.has("length");
    const uv = p.raw("until");
    if (uv == null) return p.lenPos("length", null, hint);
    if (has_len) {
        p.fail("until", "give either 'length' or 'until', not both (the engine computes length from 'until')", .{});
        return null;
    }
    // anchor of the member
    var anchor: []const u8 = "bottom_left";
    if (p.raw("at")) |at| if (at.get("anchor")) |an| if (an.str()) |s| {
        anchor = s;
    };
    const along_x = std.mem.eql(u8, run, "x");
    var dir: f64 = 0;
    if (along_x) {
        if (std.mem.endsWith(u8, anchor, "_left")) dir = 1 else if (std.mem.endsWith(u8, anchor, "_right")) dir = -1;
    } else {
        if (std.mem.startsWith(u8, anchor, "top_")) dir = -1 else if (std.mem.startsWith(u8, anchor, "bottom_")) dir = 1;
    }
    if (dir == 0) {
        p.fail("until", "'until' needs a placement anchor at one end of the member: for run {s} use {s} (got anchor \"{s}\"); center anchors cannot define a growth direction", .{ run, if (along_x) "*_left or *_right" else "top_* or bottom_*", anchor });
        return null;
    }
    const path = try std.fmt.allocPrint(a, "{s}/{s}/until", .{ p.base, p.id });
    const target = ctx.scene.point(uv.?, p.id, path) orelse {
        p.ok = false;
        return null;
    };
    // distance along the member's own run direction (rotated by `slope`/`rotate`; plain x or y when unrotated)
    const run_dir = if (along_x) V2.init(@cos(ctx.angle), @sin(ctx.angle)) else V2.init(-@sin(ctx.angle), @cos(ctx.angle));
    const start = ctx.origin.x * run_dir.x + ctx.origin.y * run_dir.y;
    const end = target.x * run_dir.x + target.y * run_dir.y;
    const len = (end - start) * dir;
    if (len <= 1e-6) {
        p.fail("until", "'until' target is {s} the anchor along {s} (anchor at {s}, target at {s}); the member would have length {s}. The anchor grows {s}", .{
            if (len < -1e-6) "behind" else "at",
            if (along_x) "x" else "y",
            fmtNum(a, start),
            fmtNum(a, end),
            fmtNum(a, len),
            if (along_x) (if (dir > 0) "right (+x)" else "left (-x)") else (if (dir > 0) "up (+y)" else "down (-y)"),
        });
        return null;
    }
    const ref_txt: []const u8 = switch (uv.?) {
        .string => |t| t,
        .object => if (uv.?.get("ref")) |r| (r.str() orelse "ref") else "ref",
        else => "ref",
    };
    note.* = try std.fmt.allocPrint(a, " (until {s})", .{ref_txt});
    return len;
}

// ---- lumber -------------------------------------------------------------------------------------------

pub const LumberParams = struct {
    size: []const u8,
    product: enum { sawn, lvl, psl, lsl, glulam } = .sawn,
    run: enum { z, x, y } = .z,
    orient: enum { upright, flat } = .upright,
    face: enum { wide, narrow } = .wide,
    length: ?json.Value = null,
    until: ?json.Value = null,
    plies: u8 = 1,
    treated: bool = false,
    blocking: bool = false,
    grade: []const u8 = "",
    barrier: ?enum { sill_seal, membrane } = null,

    pub const spec = .{
        .size = .{ .desc = "sawn nominal \"2x4\"..\"2x12\", \"4x4\"..\"4x12\", \"6x6\"..\"6x12\" (also 1x4..1x12); or actual \"1.75x11.875\" (thickness x depth) for lvl/psl/lsl/glulam" },
        .product = .{ .desc = "sawn | lvl | psl | lsl | glulam" },
        .run = .{ .desc = "axis the length runs along: z (seen end-on in section), x or y" },
        .orient = .{ .desc = "run z only: upright (depth vertical) or flat (depth horizontal)" },
        .face = .{ .desc = "run x/y only: face seen by the viewer: wide (depth in-plane) or narrow (thickness in-plane)" },
        .length = .{ .def = "required for run x/y", .desc = "member length (inches or ft-in string); alternative: `until`" },
        .until = .{ .desc = "run x/y alternative to length: a Ref (or {ref, offset}); the member grows from its placement anchor along its run axis until its far end reaches the Ref's coordinate on that axis (anchor *_left grows right, *_right left, top_* down, bottom_* up; center anchors are an error; length and until together are E_PARAM). Example jack stud: \"at\": {\"anchor\": \"top_left\", \"to\": \"beam@bottom_left\"}, \"until\": \"bottom_plate@top_left\"" },
        .plies = .{ .min = 1, .max = 8, .desc = "built-up members; plies stack along X for run z, along Z otherwise; draws ply lines" },
        .treated = .{ .desc = "preservative treated (material wood_treated; W_UNTREATED_CONTACT checks)" },
        .blocking = .{ .desc = "discontinuous member: section mark is one diagonal instead of an X" },
        .grade = .{ .def = "null", .desc = "free text e.g. \"#2 DF-L\" for notes" },
        .barrier = .{ .desc = "sill_seal | membrane: draws a 1/8\" sealer strip under the member (part `barrier`, material sill_seal) and the member sits on top of it, so bottom_* anchors are the strip underside and the member top is 1/8\" higher than without. Clears W_UNTREATED_CONTACT for untreated wood on concrete/CMU (put it on the wood member that bears on the masonry; add a note, e.g. `SILL SEALER`)" },
    };
};

fn buildLumber(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const lp = p.parse(LumberParams) orelse return null;
    const size = lp.size;
    const run = @tagName(lp.run);
    const treated = lp.treated;
    const is_sawn = lp.product == .sawn;
    const sz = parseSawn(size) orelse parseActual(size) orelse {
        p.fail("size", "size \"{s}\" is not recognised. Use sawn nominal \"2x4\", \"2x6\", \"2x8\", \"2x10\", \"2x12\", \"4x4\".. \"4x12\", \"6x6\".. \"6x12\", or an actual \"thickness x depth\" such as \"1.75x9.25\"", .{size});
        return null;
    };
    const plies: usize = lp.plies;
    const n_plies: f64 = @floatFromInt(lp.plies);
    var length: f64 = 0;
    var until_note: []const u8 = "";
    if (lp.run == .z and p.has("until")) {
        p.fail("until", "'until' only applies to lumber with run x or y (run z spans the document run; set 'z' instead)", .{});
        return null;
    }
    if (lp.run != .z) {
        length = (try lengthOrUntil(ctx, run, "lumber with run x or y needs its length, e.g. \"length\": 92.625, or \"until\": \"other@anchor\"", &until_note)) orelse return null;
    } else if (p.has("length")) {
        // run z spans the z extent; length is ignored
    }
    if (!p.ok) return null;

    const t = sz.t;
    const d = sz.d;
    var w: f64 = undefined; // in-plane width
    var h: f64 = undefined; // in-plane height
    var nat: ?f64 = null;
    var quads: std.ArrayList([4]V2) = .empty;
    var ply_lines: std.ArrayList([2]V2) = .empty;
    const wide = lp.face == .wide;
    var info_run: []const u8 = undefined;
    if (lp.run == .z) {
        const flat = lp.orient == .flat;
        if (!flat) {
            w = t * n_plies;
            h = d;
            var k: usize = 0;
            while (k < plies) : (k += 1) {
                const x0 = @as(f64, @floatFromInt(k)) * t;
                try quads.append(a, quadOf(x0, 0, x0 + t, h));
                if (k > 0) try ply_lines.append(a, .{ V2.init(x0, 0), V2.init(x0, h) });
            }
        } else {
            w = d;
            h = t * n_plies;
            var k: usize = 0;
            while (k < plies) : (k += 1) {
                const y0 = @as(f64, @floatFromInt(k)) * t;
                try quads.append(a, quadOf(0, y0, w, y0 + t));
                if (k > 0) try ply_lines.append(a, .{ V2.init(0, y0), V2.init(w, y0) });
            }
        }
        info_run = if (flat) "flat run z" else "run z";
    } else {
        const along_x = lp.run == .x;
        const in_plane = if (wide) d else t * n_plies;
        const out_of_plane = if (wide) t * n_plies else d;
        nat = out_of_plane;
        if (along_x) {
            w = length;
            h = in_plane;
        } else {
            w = in_plane;
            h = length;
        }
        // Plies stacked in-plane (narrow face) show their separation lines.
        if (!wide and plies > 1) {
            var k: usize = 1;
            while (k < plies) : (k += 1) {
                const o = @as(f64, @floatFromInt(k)) * t;
                if (along_x) {
                    try ply_lines.append(a, .{ V2.init(0, o), V2.init(w, o) });
                } else {
                    try ply_lines.append(a, .{ V2.init(o, 0), V2.init(o, h) });
                }
            }
        }
        info_run = if (along_x) (if (wide) "run x" else "run x narrow") else (if (wide) "run y" else "run y narrow");
    }
    const material: []const u8 = if (!is_sawn) "wood_engineered" else if (treated) "wood_treated" else "wood";
    if (!materialOk(ctx, "material", material)) return null;
    const loop = try model.rectLoop(a, 0, 0, w, h);
    const prism = Prism{
        .material = material,
        .loops = try model.oneLoop(a, loop),
        .quads = if (lp.run == .z) quads.items else &.{},
        .blocking = lp.blocking,
        .ply_lines = ply_lines.items,
    };
    var info: std.ArrayList(u8) = .empty;
    try info.appendSlice(a, "lumber ");
    if (plies > 1) try info.print(a, "({d}) ", .{plies});
    if (lp.run == .z) {
        try info.print(a, "{s}{s} {s} run z", .{ size, if (treated) " PT" else "", @tagName(lp.orient) });
    } else {
        try info.print(a, "{s} {s}{s} {s} run {s} L={s}{s}", .{ size, run, if (treated) " PT" else "", @tagName(lp.face), run, ftin(a, length), until_note });
    }
    if (lp.barrier) |bk| {
        // SPEC 19: a 1/8" sealer strip under the member; the member sits on top of it, so the box (and its bottom anchors)
        // starts at the strip's underside.
        const seal_t: f64 = 0.125;
        const raised = try prism.transform(a, geom.Xf.translate(0, seal_t));
        const strip = Prism{
            .part = "barrier",
            .material = materialOr(ctx, "sill_seal", "generic"),
            .loops = try model.oneLoop(a, try model.rectLoop(a, 0, 0, w, seal_t)),
        };
        const prisms = try a.alloc(Prism, 2);
        prisms[0] = strip;
        prisms[1] = raised;
        try info.print(a, " + {s}", .{@tagName(bk)});
        return .{
            .prisms = prisms,
            .box = .{ .x0 = 0, .y0 = 0, .x1 = w, .y1 = h + seal_t },
            .nat_z = nat,
            .info = info.items,
        };
    }
    return .{
        .prisms = try onePrism(a, prism),
        .box = .{ .x0 = 0, .y0 = 0, .x1 = w, .y1 = h },
        .nat_z = nat,
        .info = info.items,
    };
}

// ---- panel -----------------------------------------------------------------------------------------------

pub const PanelParams = struct {
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

fn buildPanel(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    // Total parse: a bad thickness must not hide a bad length (every problem is reported in one pass).
    const pp = p.parseAll(PanelParams);
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

// ---- cover parsing ----------------------------------------------------------------------------------------

fn parseCover(ctx: *Ctx, key: []const u8, base: model.Cover) ?struct { cover: model.Cover, parts: []const model.PartCover } {
    const v = ctx.p.raw(key) orelse return .{ .cover = base, .parts = &.{} };
    if (v != .object) {
        ctx.p.fail(key, "param '{s}' must be an object like {{\"bottom\": 3, \"sides\": 3, \"top\": 1.5}}", .{key});
        return null;
    }
    var c = base;
    inline for (.{ "bottom", "sides", "top" }) |k| {
        @field(c, k) = ctx.p.fieldLen(key, v, k, @field(c, k)) orelse return null;
    }
    var parts: std.ArrayList(model.PartCover) = .empty;
    if (v.get("parts")) |pv| if (pv == .object) {
        for (pv.object) |m| {
            var pc = c;
            if (m.value != .object) {
                ctx.p.fail(key, "{s}.parts.{s} must be an object like {{\"bottom\": 0.75}} (got {s})", .{ key, m.key, model.kindOrText(ctx.a, m.value) });
                return null;
            }
            const part_key = std.fmt.allocPrint(ctx.a, "{s}/parts/{s}", .{ key, m.key }) catch return null;
            inline for (.{ "bottom", "sides", "top" }) |k| {
                @field(pc, k) = ctx.p.fieldLen(part_key, m.value, k, @field(pc, k)) orelse return null;
            }
            parts.append(ctx.a, .{ .part = m.key, .cover = pc }) catch return null;
        }
    };
    return .{ .cover = c, .parts = parts.items };
}

// ---- cmu_wall -----------------------------------------------------------------------------------------------

pub const CmuParams = struct {
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

fn buildCmu(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const cp = p.parseAll(CmuParams);
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
        const part = try std.fmt.allocPrint(a, "course_{d}", .{k});
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
            try prisms.append(a, .{ .part = try std.fmt.allocPrint(a, "joint_{d}", .{k}), .material = "mortar", .loops = try model.oneLoop(a, try model.rectLoop(a, 0, y1, w, y1 + 0.375)) });
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
        .info = try std.fmt.allocPrint(a, "cmu_wall {s}\" x {d} courses{s}", .{ fmtNum(a, nom), n, if (nbb > 0) try std.fmt.allocPrint(a, " ({d} bond beam)", .{nbb}) else "" }),
    };
}

// ---- concrete ---------------------------------------------------------------------------------------------------

fn mirrorBuilt(a: Allocator, b: Built, xf: geom.Xf, keep_box: bool) Allocator.Error!Built {
    var out = b;
    const prisms = try a.alloc(Prism, b.prisms.len);
    for (b.prisms, 0..) |pr, i| prisms[i] = try pr.transform(a, xf);
    out.prisms = prisms;
    const an = try a.alloc(model.NamedAnchor, b.anchors.len);
    for (b.anchors, 0..) |x, i| an[i] = .{ .name = x.name, .p = xf.apply(x.p) };
    out.anchors = an;
    const zs = try a.alloc(model.Zone, b.zones.len);
    for (b.zones, 0..) |z, i| {
        const loops = try a.alloc([]const Pt, z.loops.len);
        var bx = Box{};
        for (z.loops, 0..) |l, k| {
            loops[k] = try xf.applyLoop(a, l);
            bx.addBox(geom.loopBox(loops[k]));
        }
        zs[i] = .{ .name = z.name, .loops = loops, .box = bx };
    }
    out.zones = zs;
    if (b.host) |h| {
        var nh = h;
        nh.outline = try xf.applyLoop(a, h.outline);
        out.host = nh;
    }
    if (!keep_box) out.box = boxOfPrisms(prisms);
    return out;
}

pub const ConcreteParams = struct {
    shape: enum { rect, footing, polygon, slab_edge },
    material: []const u8 = "concrete",
    cover: ?json.Value = null,
    width: ?f64 = null,
    height: ?f64 = null,
    points: ?json.Value = null,
    exterior: enum { left, right } = .left,
    slab_thickness: f64 = 4,
    slab_length: f64 = 48,
    footing_width: f64 = 12,
    footing_depth: f64 = 18,
    haunch: f64 = 45,
    recess: ?json.Value = null,
    recess_slope: f64 = 0,
    base: ?json.Value = null,

    pub const spec = .{
        .shape = .{ .desc = "rect | footing | polygon | slab_edge" },
        .material = .{ .desc = "any style material" },
        .cover = .{ .def = "{bottom:3, sides:3, top:1.5}", .desc = "REQUIRED clear cover for bars in this host (W_COVER); cover.parts.<part> overrides per part zone, e.g. {\"parts\":{\"slab\":{\"bottom\":0.75}}}" },
        .width = .{ .len = .pos, .hint = "rect/footing need width and height", .also = &.{"height"}, .def = "rect/footing: required", .desc = "box size" },
        .height = .{ .len = .pos, .hint = "rect/footing need width and height", .row = false, .desc = "box size" },
        .points = .{ .def = "polygon: required", .desc = "polyline [x,y(,bulge)] relative to the placement point, or absolute Refs" },
        .exterior = .{ .desc = "slab_edge: which side is the exterior edge (right mirrors)" },
        .slab_thickness = .{ .len = .pos, .desc = "slab_edge" },
        .slab_length = .{ .len = .pos, .desc = "slab_edge: slab drawn from exterior face inward" },
        .footing_width = .{ .len = .pos, .desc = "slab_edge: bottom width of the turndown" },
        .footing_depth = .{ .len = .pos, .desc = "slab_edge: top of slab to bottom of footing" },
        .haunch = .{ .desc = "slab_edge: inner face slope from horizontal in degrees; 90 = vertical" },
        .recess = .{ .desc = "slab_edge: {width, depth, from_edge} depression at the top exterior edge (door sill); from_edge 0 = at the exterior face" },
        .recess_slope = .{ .len = .any, .desc = "slab_edge: recess floor falls this many inches toward the exterior over its width" },
        .base = .{ .desc = "slab_edge: {material: gravel|sand|compacted_fill, thickness} uniform base course under the slab soffit and along the haunch (soil side), stopping at the footing bottom; part `base` with the fill hatch. Replaces hand-drawn fill polygons under the slab" },
    };
};

fn buildConcrete(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const cp = p.parseAll(ConcreteParams);
    const cov = parseCover(ctx, "cover", .{});
    if (!p.ok) return null;
    const material = cp.material;
    if (!materialOk(ctx, "material", material)) return null;
    const sh = cp.shape;
    if (sh == .rect or sh == .footing) {
        const hint = "rect/footing need width and height";
        const w = cp.width orelse blk: {
            p.missing("width", hint);
            break :blk 0;
        };
        const h = cp.height orelse blk: {
            p.missing("height", hint);
            break :blk 0;
        };
        if (!p.ok) return null;
        const loop = try model.rectLoop(a, 0, 0, w, h);
        const part: []const u8 = if (sh == .footing) "footing" else "";
        const prism = Prism{ .part = part, .material = material, .loops = try model.oneLoop(a, loop) };
        const zones = if (part.len > 0) try a.dupe(model.Zone, &.{try zoneRect(a, "footing", 0, 0, w, h)}) else &[_]model.Zone{};
        return .{
            .prisms = try onePrism(a, prism),
            .zones = zones,
            .box = .{ .x0 = 0, .y0 = 0, .x1 = w, .y1 = h },
            .host = .{ .outline = loop, .cover = cov.?.cover, .part_cover = cov.?.parts },
            .info = try std.fmt.allocPrint(a, "concrete {s} {s} x {s}", .{ @tagName(sh), ftin(a, w), ftin(a, h) }),
        };
    }
    if (sh == .polygon) {
        const pv = cp.points orelse {
            p.fail("points", "shape polygon needs 'points': [[x, y], ...] (relative to the placement point) or Refs", .{});
            return null;
        };
        const pts = (try parsePointList(ctx, "points", pv, true)) orelse return null;
        const clean = try dropDuplicatePoints(a, pts);
        if (clean.len < 3) {
            p.fail("points", "polygon needs at least 3 distinct points (got {d})", .{clean.len});
            return null;
        }
        const loop = try orientedCcw(a, clean);
        const prism = Prism{ .material = material, .loops = try model.oneLoop(a, loop) };
        const bx = geom.loopBox(loop);
        return .{
            .prisms = try onePrism(a, prism),
            .box = bx,
            .points_mode = true,
            .host = .{ .outline = loop, .cover = cov.?.cover, .part_cover = cov.?.parts },
            .info = "concrete polygon",
        };
    }
    // slab_edge
    const st = cp.slab_thickness;
    const sl = cp.slab_length;
    const fw = cp.footing_width;
    const fd = cp.footing_depth;
    const haunch = cp.haunch;
    var rw: f64 = 0;
    var rd: f64 = 0;
    var re: f64 = 0;
    var has_recess = false;
    if (cp.recess) |rv| {
        if (rv != .object) {
            p.fail("recess", "param 'recess' must be {{\"width\": w, \"depth\": d, \"from_edge\": e}} or null", .{});
        } else {
            has_recess = true;
            rw = (if (rv.get("width")) |x| units.parseLength(x) else null) orelse blk: {
                p.fail("recess/width", "recess needs a numeric 'width'", .{});
                break :blk 0;
            };
            rd = (if (rv.get("depth")) |x| units.parseLength(x) else null) orelse blk: {
                p.fail("recess/depth", "recess needs a numeric 'depth'", .{});
                break :blk 0;
            };
            re = p.fieldLen("recess", rv, "from_edge", 0) orelse 0;
        }
    }
    const rslope = cp.recess_slope;
    var base_mat: []const u8 = "";
    var base_t: f64 = 0;
    if (cp.base) |bv| {
        if (bv != .object) {
            p.fail("base", "param 'base' must be {{\"material\": \"gravel\"|\"sand\"|\"compacted_fill\", \"thickness\": 4}} or null", .{});
        } else {
            base_mat = (if (bv.get("material")) |x| x.str() else null) orelse "gravel";
            var ok_mat = false;
            for ([_][]const u8{ "gravel", "sand", "compacted_fill" }) |m| {
                if (std.mem.eql(u8, m, base_mat)) ok_mat = true;
            }
            if (!ok_mat) p.fail("base/material", "base.material must be \"gravel\", \"sand\" or \"compacted_fill\" (got \"{s}\")", .{base_mat});
            base_t = (if (bv.get("thickness")) |x| units.parseLength(x) else null) orelse blk: {
                p.fail("base/thickness", "base needs a numeric 'thickness' (inches), e.g. {{\"material\": \"gravel\", \"thickness\": 4}}", .{});
                break :blk 0;
            };
            if (p.ok and base_t <= 0) p.fail("base/thickness", "base.thickness must be greater than 0", .{});
        }
    }
    if (!p.ok) return null;
    if (fd <= st) {
        p.fail("footing_depth", "footing_depth ({s}) must exceed slab_thickness ({s}): it is measured from the top of the slab", .{ fmtNum(a, fd), fmtNum(a, st) });
        return null;
    }
    if (haunch <= 0 or haunch > 90) {
        p.fail("haunch", "haunch must be an angle in degrees from horizontal, > 0 and <= 90 (got {s})", .{fmtNum(a, haunch)});
        return null;
    }
    if (has_recess) {
        if (rw <= 0 or rd <= 0 or rd >= st) {
            p.fail("recess", "recess width must be > 0 and depth in (0, slab_thickness {s}) (got width {s}, depth {s})", .{ fmtNum(a, st), fmtNum(a, rw), fmtNum(a, rd) });
            return null;
        }
        if (re + rw > sl) {
            p.fail("recess", "recess (from_edge {s} + width {s}) extends beyond slab_length {s}", .{ fmtNum(a, re), fmtNum(a, rw), fmtNum(a, sl) });
            return null;
        }
    }
    const hx = fw + (fd - st) / @tan(std.math.degreesToRadians(haunch));
    if (hx > sl) {
        p.fail("haunch", "the haunch meets the slab underside at x={s}, beyond slab_length {s}; lengthen the slab or steepen haunch", .{ fmtNum(a, hx), fmtNum(a, sl) });
        return null;
    }
    var pts: std.ArrayList(Pt) = .empty;
    try pts.append(a, .{ .x = 0, .y = -fd }); // footing bottom exterior
    const rec_ext_y = -rd - rslope;
    if (has_recess) {
        if (re > 0) {
            try pts.append(a, .{ .x = 0, .y = 0 });
            try pts.append(a, .{ .x = re, .y = 0 });
        }
        try pts.append(a, .{ .x = re, .y = rec_ext_y }); // recess bottom exterior
        try pts.append(a, .{ .x = re + rw, .y = -rd }); // recess bottom interior
        try pts.append(a, .{ .x = re + rw, .y = 0 }); // recess top interior
    } else {
        try pts.append(a, .{ .x = 0, .y = 0 });
    }
    try pts.append(a, .{ .x = sl, .y = 0 });
    try pts.append(a, .{ .x = sl, .y = -st });
    try pts.append(a, .{ .x = hx, .y = -st });
    try pts.append(a, .{ .x = fw, .y = -fd });
    const loop = try orientedCcw(a, pts.items);
    const prism = Prism{ .material = material, .loops = try model.oneLoop(a, loop) };
    var anchors: std.ArrayList(model.NamedAnchor) = .empty;
    try anchors.append(a, .{ .name = "top_exterior", .p = V2.init(0, 0) });
    try anchors.append(a, .{ .name = "slab_top", .p = V2.init(sl, 0) });
    try anchors.append(a, .{ .name = "footing_bottom_exterior", .p = V2.init(0, -fd) });
    try anchors.append(a, .{ .name = "footing_bottom_interior", .p = V2.init(fw, -fd) });
    try anchors.append(a, .{ .name = "slab_bottom_interior", .p = V2.init(sl, -st) });
    try anchors.append(a, .{ .name = "haunch_top", .p = V2.init(hx, -st) });
    if (has_recess) {
        try anchors.append(a, .{ .name = "recess_bottom_exterior", .p = V2.init(re, rec_ext_y) });
        try anchors.append(a, .{ .name = "recess_bottom_interior", .p = V2.init(re + rw, -rd) });
        try anchors.append(a, .{ .name = "recess_top_interior", .p = V2.init(re + rw, 0) });
    }
    const zones = try a.dupe(model.Zone, &.{
        try zoneRect(a, "footing", 0, -fd, fw, -st),
        try zoneRect(a, "slab", 0, -st, sl, 0),
    });
    var prisms_out: std.ArrayList(Prism) = .empty;
    try prisms_out.append(a, prism);
    var zones_out: std.ArrayList(model.Zone) = .empty;
    try zones_out.appendSlice(a, zones);
    if (base_t > 0) {
        // uniform base course under the slab soffit and along the haunch, stopping at the footing bottom
        const dx = hx - fw;
        const dy = fd - st;
        const dl = @sqrt(dx * dx + dy * dy);
        const ux = dx / dl;
        const uy = dy / dl;
        const nx = uy; // soil-side normal of the haunch line (down and toward the interior)
        const ny = -ux;
        const o = V2.init(fw + nx * base_t, -fd + ny * base_t);
        if (st + base_t >= fd - 1e-9) {
            p.fail("base/thickness", "base.thickness {s} must be less than footing_depth - slab_thickness ({s}) so the base stops above the footing bottom", .{ fmtNum(a, base_t), fmtNum(a, fd - st) });
            return null;
        }
        const s1 = (-st - base_t - o.y) / uy; // along the offset haunch line to the soffit-offset level
        const s2 = (-fd - o.y) / uy; // ... to the footing-bottom level
        const q1 = V2.init(o.x + ux * s1, -st - base_t);
        const q2 = V2.init(o.x + ux * s2, -fd);
        if (q1.x >= sl - 1e-9) {
            p.fail("base/thickness", "the base course meets the slab end before the haunch offset does; lengthen slab_length or reduce base.thickness", .{});
            return null;
        }
        const bpts = [_]Pt{
            .{ .x = fw, .y = -fd },
            .{ .x = hx, .y = -st },
            .{ .x = sl, .y = -st },
            .{ .x = sl, .y = -st - base_t },
            .{ .x = q1.x, .y = q1.y },
            .{ .x = q2.x, .y = q2.y },
        };
        const bloop = try orientedCcw(a, try dropDuplicatePoints(a, &bpts));
        try prisms_out.append(a, .{ .part = "base", .material = base_mat, .loops = try model.oneLoop(a, bloop) });
        const bb = geom.loopBox(bloop);
        try zones_out.append(a, .{ .name = "base", .loops = try model.oneLoop(a, bloop), .box = bb });
        try anchors.append(a, .{ .name = "base_bottom_interior", .p = V2.init(sl, -st - base_t) });
        try anchors.append(a, .{ .name = "base_bottom_footing", .p = V2.init(q2.x, -fd) });
    }
    var built = Built{
        .prisms = prisms_out.items,
        .anchors = anchors.items,
        .zones = zones_out.items,
        .box = geom.loopBox(loop),
        .host = .{ .outline = loop, .cover = cov.?.cover, .part_cover = cov.?.parts },
        .info = try std.fmt.allocPrint(a, "concrete slab_edge {s} slab, ftg {s} x {s}{s}{s}", .{
            ftin(a, st),
            ftin(a, fw),
            ftin(a, fd),
            if (has_recess) try std.fmt.allocPrint(a, ", recess {s} x {s}", .{ ftin(a, rw), ftin(a, rd) }) else "",
            if (base_t > 0) try std.fmt.allocPrint(a, ", {s} {s} base", .{ ftin(a, base_t), base_mat }) else "",
        }),
    };
    if (cp.exterior == .right) {
        built = try mirrorBuilt(a, built, geom.Xf.scaling(-1, 1), false);
        built.box = geom.loopBox(built.host.?.outline); // the base course does not move the 9 box anchors
    }
    return built;
}

// ---- rebar --------------------------------------------------------------------------------------------------------

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

fn vsOf(a: Allocator, pts: []const Pt) Allocator.Error![]V2 {
    const out = try a.alloc(V2, pts.len);
    for (pts, 0..) |q, i| out[i] = q.v();
    return out;
}

fn pathLen(vs: []const V2) f64 {
    var t: f64 = 0;
    for (vs[0 .. vs.len - 1], 0..) |q, i| t += q.dist(vs[i + 1]);
    return t;
}

pub const RebarParams = struct {
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

fn buildRebar(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const rp = p.parse(RebarParams) orelse return null;
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

pub fn worldBox(xf: geom.Xf, b: Box) Box {
    var out = Box{};
    const cs = [_]V2{ V2.init(b.x0, b.y0), V2.init(b.x1, b.y0), V2.init(b.x1, b.y1), V2.init(b.x0, b.y1) };
    for (cs) |c| {
        const w = xf.apply(c);
        out.addPoint(w.x, w.y);
    }
    return out;
}

// ---- anchor bolt ----------------------------------------------------------------------------------------------------

pub const AnchorBoltParams = struct {
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

fn buildAnchorBolt(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const ap = p.parse(AnchorBoltParams) orelse return null;
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
        .info = try std.fmt.allocPrint(a, "anchor_bolt {s}\" dia, embed {s}, proj {s}, {s}{s}", .{ fmtNum(a, d), ftin(a, embed), ftin(a, proj), @tagName(h), if (post) "" else " hook" }),
    };
}

// ---- connector ------------------------------------------------------------------------------------------------------

const Hardware = catalog.Hardware;
const hardware = catalog.hardware;

pub fn gaugeThickness(g: u32) ?f64 {
    return switch (g) {
        10 => 0.1345,
        11 => 0.1196,
        12 => 0.1046,
        14 => 0.0747,
        16 => 0.0598,
        18 => 0.0478,
        20 => 0.0359,
        22 => 0.0299,
        24 => 0.0239,
        26 => 0.0179,
        28 => 0.0149,
        else => null,
    };
}

pub const ConnectorParams = struct {
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

fn buildConnector(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const cp = p.parseAll(ConnectorParams);
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

// ---- truss -----------------------------------------------------------------------------------------------------------

fn buildTruss(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const exterior = p.choice("exterior", "left", &.{ "left", "right" });
    const pitch_v = p.raw("pitch") orelse json.Value{ .string = "4:12" };
    const top = p.str("top_chord", "2x4");
    const bot = p.str("bottom_chord", "2x4");
    const heel = p.choice("heel", "standard", &.{ "standard", "raised" });
    const heel_h = if (p.has("heel_height")) p.lenPos("heel_height", null, "") else null;
    const bw = p.lenPos("bearing_width", 3.5, "");
    const ov = p.len("overhang", 12, "");
    const tail = p.choice("tail", "plumb", &.{ "plumb", "square" });
    const span = p.lenPos("span_shown", 48, "");
    const plate = p.boolean("plate", true);
    if (!p.ok) return null;
    const theta = units.parseSlope(pitch_v) orelse {
        p.fail("pitch", "param 'pitch' must be rise:run like \"4:12\" or degrees (got {s})", .{model.kindOrText(a, pitch_v)});
        return null;
    };
    if (theta <= 0 or theta >= std.math.pi / 2.0) {
        p.fail("pitch", "pitch must slope up (between 0 and 90 degrees)", .{});
        return null;
    }
    const tsz = parseSawn(top.?) orelse {
        p.fail("top_chord", "top_chord \"{s}\" is not a sawn nominal size like \"2x4\", \"2x6\"", .{top.?});
        return null;
    };
    const bsz = parseSawn(bot.?) orelse {
        p.fail("bottom_chord", "bottom_chord \"{s}\" is not a sawn nominal size like \"2x4\", \"2x6\"", .{bot.?});
        return null;
    };
    const dt = tsz.d;
    const db = bsz.d;
    const s = @tan(theta);
    const c = @cos(theta);
    const raised = std.mem.eql(u8, heel.?, "raised");
    const v_thick = dt / c; // vertical thickness of the top chord
    var y_low0 = db; // lower edge of the top chord at x = 0
    if (raised) {
        const hh = heel_h orelse {
            p.fail("heel_height", "heel 'raised' needs 'heel_height' (vertical height at the bearing outer edge from top of bottom chord to top of top chord)", .{});
            return null;
        };
        if (hh < v_thick) {
            p.fail("heel_height", "heel_height {s} is smaller than the top chord's vertical thickness {s}; the raised heel must be at least that tall", .{ fmtNum(a, hh), fmtNum(a, v_thick) });
            return null;
        }
        y_low0 = db + hh - v_thick;
    }
    const x_tail = -ov.?;
    const xe = span.?;
    const lower = struct {
        fn f(x: f64, y0: f64, sl: f64) f64 {
            return y0 + sl * x;
        }
    }.f;
    const tail_bottom = V2.init(x_tail, lower(x_tail, y_low0, s));
    var tail_top = V2.init(x_tail, tail_bottom.y + v_thick);
    var tail_bottom_pt = tail_bottom;
    if (std.mem.eql(u8, tail.?, "square")) {
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
    if (plate) {
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
    try anchors.append(a, .{ .name = "bearing_inner", .p = V2.init(bw.?, 0) });
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
        .info = try std.fmt.allocPrint(a, "truss {s}:12 {s} heel, {s}+{s} chords, ovh {s}", .{ fmtNum(a, @tan(theta) * 12.0), heel.?, top.?, bot.?, ftin(a, ov.?) }),
    };
    if (std.mem.eql(u8, exterior.?, "right")) built = try mirrorBuilt(a, built, geom.Xf.scaling(-1, 1), false);
    return built;
}

// ---- membrane -----------------------------------------------------------------------------------------------------------

fn membraneThickness(material: []const u8) f64 {
    const eq = std.mem.eql;
    if (eq(u8, material, "vapor_retarder")) return 0.04;
    if (eq(u8, material, "shingles")) return 0.25;
    if (eq(u8, material, "underlayment")) return 0.06;
    if (eq(u8, material, "wrb")) return 0.04;
    if (eq(u8, material, "flashing_membrane")) return 0.06;
    return 0.05;
}

fn buildMembrane(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const material = p.choice("material", "membrane", &.{ "membrane", "underlayment", "vapor_retarder", "wrb", "shingles", "flashing_membrane" });
    const side = p.choice("side", "left", &.{ "left", "right" });
    const thick = p.lenPos("thickness", if (material) |m| membraneThickness(m) else 0.05, "");
    const pv = p.raw("points") orelse {
        p.fail("points", "membrane needs 'points': the layer polyline, e.g. [\"roof_sheathing@top_left\", \"roof_sheathing@top_right\"]", .{});
        return null;
    };
    if (!p.ok) return null;
    const pts = (try parsePointList(ctx, "points", pv, true)) orelse return null;
    const clean = try dropDuplicatePoints(a, pts);
    if (clean.len < 2) {
        p.fail("points", "a membrane needs at least 2 distinct points (got {d})", .{clean.len});
        return null;
    }
    var until_note: []const u8 = "";
    if (p.raw("until")) |uv| {
        // SPEC 19: the last segment grows or shrinks along its own direction until its end reaches the Ref's coordinate along that direction
        const path = try std.fmt.allocPrint(a, "{s}/{s}/until", .{ p.base, p.id });
        const target = ctx.scene.point(uv, p.id, path) orelse {
            p.ok = false;
            return null;
        };
        const last = clean.len - 1;
        const seg = V2.init(clean[last].x - clean[last - 1].x, clean[last].y - clean[last - 1].y);
        if (seg.len() < 1e-9) {
            p.fail("until", "'until' needs a last segment with a direction (the last two points coincide)", .{});
            return null;
        }
        const u = seg.norm();
        // the polyline is local (world - origin, then rotated by the placement angle): bring the target into that frame
        const rel = target.sub(ctx.origin);
        const ca = @cos(-ctx.angle);
        const sa = @sin(-ctx.angle);
        const tl = V2.init(rel.x * ca - rel.y * sa, rel.x * sa + rel.y * ca);
        const new_len = (tl.x - clean[last - 1].x) * u.x + (tl.y - clean[last - 1].y) * u.y;
        if (new_len <= 1e-6) {
            p.fail("until", "'until' target lies at or behind the start of the last segment along its direction (length would be {s})", .{fmtNum(a, new_len)});
            return null;
        }
        clean[last].x = clean[last - 1].x + u.x * new_len;
        clean[last].y = clean[last - 1].y + u.y * new_len;
        const ref_txt: []const u8 = switch (uv) {
            .string => |t| t,
            .object => if (uv.get("ref")) |r| (r.str() orelse "ref") else "ref",
            else => "ref",
        };
        until_note = try std.fmt.allocPrint(a, " (until {s})", .{ref_txt});
    }
    const left = std.mem.eql(u8, side.?, "left");
    const t = thick.?;
    const rib = try path_geom.ribbon(a, clean, if (left) t else 0, if (left) 0 else t);
    // The drawn line sits at mid-thickness; vapor retarders keep a minimum separation from the host
    // (0.03 paper inch is applied at draw time via `line_gap`, here only the mid-thickness).
    const line = try path_geom.offsetOpen(a, clean, if (left) t / 2 else -t / 2);
    const prism = Prism{
        .material = material.?,
        .loops = try model.oneLoop(a, rib),
        .kind = .line,
        .line_pts = line,
        .ticks = std.mem.eql(u8, material.?, "shingles"),
        .centerline = clean,
    };
    return .{
        .prisms = try onePrism(a, prism),
        .box = geom.loopBox(rib),
        .points_mode = true,
        .info = try std.fmt.allocPrint(a, "membrane {s} {s} thick L={s}{s}", .{ material.?, ftin(a, t), ftin(a, pathLen(try vsOf(a, clean))), until_note }),
    };
}

// ---- fill ------------------------------------------------------------------------------------------------------------------

fn buildFill(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const material = p.choice("material", "earth", &.{ "earth", "gravel", "sand", "compacted_fill" });
    const outline = p.choice("outline", "top", &.{ "top", "full", "none" });
    _ = p.str("grade_label", "");
    const pv = p.raw("points") orelse {
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
    const om: model.OutlineMode = if (std.mem.eql(u8, outline.?, "top")) .top else if (std.mem.eql(u8, outline.?, "full")) .full else .none;
    const prism = Prism{ .material = material.?, .loops = try model.oneLoop(a, loop), .outline = om };
    return .{
        .prisms = try onePrism(a, prism),
        .box = geom.loopBox(loop),
        .points_mode = true,
        .info = try std.fmt.allocPrint(a, "fill {s}", .{material.?}),
    };
}

// ---- insulation ----------------------------------------------------------------------------------------------------------------

fn buildInsulation(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const form = p.choice("form", "rigid", &.{ "rigid", "batt" });
    if (!p.ok) return null;
    var loop: []const Pt = undefined;
    var points_mode = false;
    if (p.raw("points")) |pv| {
        const pts = (try parsePointList(ctx, "points", pv, true)) orelse return null;
        const clean = try dropDuplicatePoints(a, pts);
        if (clean.len < 3) {
            p.fail("points", "insulation polygon needs at least 3 points", .{});
            return null;
        }
        loop = try orientedCcw(a, clean);
        points_mode = true;
    } else {
        const w = p.lenPos("width", null, "give width and height, or points") orelse return null;
        const h = p.lenPos("height", null, "give width and height, or points") orelse return null;
        loop = try model.rectLoop(a, 0, 0, w, h);
    }
    const batt = std.mem.eql(u8, form.?, "batt");
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
        .info = try std.fmt.allocPrint(a, "insulation {s}", .{form.?}),
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

// ---- solid ------------------------------------------------------------------------------------------------------------------------

fn buildSolid(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const material = p.str("material", null);
    const prof = p.raw("profile") orelse {
        p.fail("profile", "solid needs 'profile': {{\"rect\": [w, h]}}, {{\"circle\": d}} or {{\"points\": [[x, y], ...]}}", .{});
        return null;
    };
    if (!p.ok) return null;
    if (!materialOk(ctx, "material", material.?)) return null;
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
    const prism = Prism{ .material = material.?, .loops = try model.oneLoop(a, loop) };
    const bx = geom.loopBox(loop);
    return .{
        .prisms = try onePrism(a, prism),
        .box = bx,
        .points_mode = points_mode,
        .info = try std.fmt.allocPrint(a, "solid {s} (escape hatch)", .{material.?}),
    };
}

// ---- flashing -----------------------------------------------------------------------------------------------------

/// A style material when the style defines it, else `fallback` (custom styles may lack the v0.1.2 additions).
fn materialOr(ctx: *const Ctx, name: []const u8, fallback: []const u8) []const u8 {
    return if (ctx.style.material(name) != null) name else fallback;
}

fn buildFlashing(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const prof = p.choice("profile", "z", &.{ "z", "l", "drip", "weep_screed", "points" });
    const exterior = p.choice("exterior", "left", &.{ "left", "right" });
    const gauge_n = p.num("gauge", 26);
    if (!p.ok) return null;
    const pr = prof.?;
    const is = struct {
        fn f(x: []const u8, y: []const u8) bool {
            return std.mem.eql(u8, x, y);
        }
    }.f;
    const flange_def: f64 = if (is(pr, "weep_screed")) 3.5 else 2;
    const leg_def: f64 = if (is(pr, "l")) 2 else 1;
    const drop_def: f64 = if (is(pr, "drip")) 1.5 else if (is(pr, "weep_screed")) 0.5 else 2;
    const flange = p.lenPos("flange", flange_def, "") orelse return null;
    const leg = p.lenPos("leg", leg_def, "") orelse return null;
    const drop = p.lenPos("drop", drop_def, "") orelse return null;
    const kick = p.lenPos("kick", 0.5, "") orelse return null;
    const gauge_i: u32 = cast.toInt(u32, @round(gauge_n.?)) orelse 0;
    const thickness = gaugeThickness(gauge_i) orelse {
        p.fail("gauge", "gauge {s} is not in the table; use 20, 22, 24, 26 (default, 0.0179\") or 28", .{fmtNum(a, gauge_n.?)});
        return null;
    };
    var pts: std.ArrayList(Pt) = .empty;
    var points_mode = false;
    // local frame: corner (the first bend) at (0,0); the wall surface is x = 0 and the exterior is -x
    if (is(pr, "z") or is(pr, "weep_screed")) {
        try pts.appendSlice(a, &.{ .{ .x = 0, .y = flange }, .{ .x = 0, .y = 0 }, .{ .x = -leg, .y = 0 }, .{ .x = -leg, .y = -drop } });
    } else if (is(pr, "l")) {
        try pts.appendSlice(a, &.{ .{ .x = 0, .y = flange }, .{ .x = 0, .y = 0 }, .{ .x = -leg, .y = 0 } });
    } else if (is(pr, "drip")) {
        try pts.appendSlice(a, &.{ .{ .x = flange, .y = 0 }, .{ .x = 0, .y = 0 }, .{ .x = 0, .y = -drop }, .{ .x = -kick, .y = -drop - 0.5 * kick } });
    } else {
        const pv = p.raw("points") orelse {
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
        .info = try std.fmt.allocPrint(a, "flashing {s} {d} ga ({s}\" thick)", .{ pr, gauge_i, fmtNum(a, thickness) }),
    };
    if (!points_mode and std.mem.eql(u8, exterior.?, "right")) built = try mirrorBuilt(a, built, geom.Xf.scaling(-1, 1), false);
    return built;
}

// ---- joint --------------------------------------------------------------------------------------------------------

fn buildJoint(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const kind = p.choice("kind", null, &.{ "expansion", "control", "tooled_edge", "sealant" });
    if (!p.ok) return null;
    const k = kind.?;
    const is = struct {
        fn f(x: []const u8, y: []const u8) bool {
            return std.mem.eql(u8, x, y);
        }
    }.f;
    // optional host zone: default depth = slab thickness (expansion) or a quarter of it (control)
    var host_h: f64 = 0;
    if (p.raw("in")) |iv| {
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
    const width_def: f64 = if (is(k, "control")) 0.25 else 0.5;
    const width = p.lenPos("width", width_def, "") orelse return null;
    const depth_def: f64 = if (is(k, "control")) (if (host_h > 0) host_h / 4.0 else 1.0) else if (is(k, "sealant")) @max(0.25, 0.5 * width) else if (host_h > 0) host_h else 4.0;
    const depth = p.lenPos("depth", depth_def, "") orelse return null;
    const radius = p.lenPos("radius", 0.25, "") orelse return null;
    const cap = p.len("cap", 0, "") orelse return null;
    const rod = p.boolean("backer_rod", true);
    const corner = p.choice("corner", "top_right", &.{ "top_right", "top_left", "bottom_right", "bottom_left" });
    if (!p.ok) return null;
    var prisms: std.ArrayList(Prism) = .empty;
    var anchors: std.ArrayList(model.NamedAnchor) = .empty;
    try anchors.append(a, .{ .name = "joint_top", .p = V2.init(0, 0) });
    const void_mat = materialOr(ctx, "void", "generic");
    if (is(k, "expansion")) {
        if (cap < 0 or cap >= depth) {
            p.fail("cap", "cap (sealant depth at the top of the joint) must be from 0 to less than depth {s} (got {s})", .{ fmtNum(a, depth), fmtNum(a, cap) });
            return null;
        }
        const hw = width / 2;
        try prisms.append(a, .{ .part = "filler", .material = materialOr(ctx, "joint_filler", "generic"), .loops = try model.oneLoop(a, try model.rectLoop(a, -hw, -depth, hw, -cap)) });
        if (cap > 0) try prisms.append(a, .{ .part = "sealant", .material = materialOr(ctx, "sealant", "steel"), .loops = try model.oneLoop(a, try model.rectLoop(a, -hw, -cap, hw, 0)) });
    } else if (is(k, "control")) {
        const tri = [_]Pt{ .{ .x = -width / 2, .y = 0 }, .{ .x = 0, .y = -depth }, .{ .x = width / 2, .y = 0 } };
        try prisms.append(a, .{ .part = "notch", .material = void_mat, .loops = try model.oneLoop(a, try orientedCcw(a, &tri)), .embedded = true });
    } else if (is(k, "tooled_edge")) {
        // the sliver between the sharp corner and the radius, drawn for the top-right corner then flipped into place
        const r = radius;
        const loop0 = [_]Pt{ .{ .x = 0, .y = 0 }, .{ .x = -r, .y = 0, .b = geom.bulgeFromSweep(-std.math.pi / 2.0) }, .{ .x = 0, .y = -r } };
        const c = corner.?;
        const sx: f64 = if (is(c, "top_left") or is(c, "bottom_left")) -1 else 1;
        const sy: f64 = if (is(c, "bottom_left") or is(c, "bottom_right")) -1 else 1;
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
        .info = try std.fmt.allocPrint(a, "joint {s} {s} wide x {s} deep", .{ k, ftin(a, width), ftin(a, depth) }),
    };
}

// ---- dispatch -----------------------------------------------------------------------------------------------------------------------

pub fn build(ctx: *Ctx) BuildError!?Built {
    return switch (ctx.comp.ty.type) {
        .lumber => buildLumber(ctx),
        .panel => buildPanel(ctx),
        .cmu_wall => buildCmu(ctx),
        .concrete => buildConcrete(ctx),
        .rebar => buildRebar(ctx),
        .anchor_bolt => buildAnchorBolt(ctx),
        .connector => buildConnector(ctx),
        .truss => buildTruss(ctx),
        .membrane => buildMembrane(ctx),
        .fill => buildFill(ctx),
        .insulation => buildInsulation(ctx),
        .solid => buildSolid(ctx),
        .flashing => buildFlashing(ctx),
        .joint => buildJoint(ctx),
    };
}

pub fn mirrorAboutCenter(a: Allocator, b: Built) Allocator.Error!Built {
    const cx = (b.box.x0 + b.box.x1) / 2;
    const xf = geom.Xf.translate(2 * cx, 0).mul(geom.Xf.scaling(-1, 1));
    return mirrorBuilt(a, b, xf, true);
}

test "sawn sizes" {
    try std.testing.expectEqual(@as(f64, 7.25), parseSawn("2x8").?.d);
    try std.testing.expectEqual(@as(f64, 1.5), parseSawn("2x8").?.t);
    try std.testing.expectEqual(@as(f64, 7.5), parseSawn("6x8").?.d);
    try std.testing.expectEqual(@as(f64, 3.5), parseSawn("4x4").?.t);
    try std.testing.expect(parseSawn("2x7") == null);
    try std.testing.expectEqual(@as(f64, 1.75), parseActual("1.75x11.875").?.t);
    try std.testing.expectEqual(@as(f64, 9.25), parseActual("1 3/4x9 1/4").?.d);
}

test "rebar sizes" {
    try std.testing.expectEqual(@as(f64, 0.625), rebarDiameter("#5").?);
    try std.testing.expect(rebarDiameter("5") == null);
}
