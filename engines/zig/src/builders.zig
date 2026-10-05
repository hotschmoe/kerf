//! Component builders (SPEC 5): turn a component's JSON params into a `Built` (prisms, anchors,
//! zones) in local coordinates. Builders validate their params and report E_PARAM diagnostics;
//! they return null when the component cannot be built.

const std = @import("std");
const json = @import("json.zig");
const geom = @import("geom.zig");
const model = @import("model.zig");
const units = @import("units.zig");
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

fn fmtNum(a: Allocator, x: f64) []const u8 {
    var b: [40]u8 = undefined;
    return a.dupe(u8, json.fmtNumber(&b, x)) catch "?";
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
    if (a <= 0 or b <= 0) return null;
    return .{ .t = a, .d = b };
}

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
                const b: f64 = if (xy.len == 3) (xy[2].num() orelse 0) else 0;
                if (x == null or y == null) {
                    ctx.p.fail(path, "point {d} of '{s}' has a non-numeric coordinate", .{ i, key });
                    ok = false;
                    continue;
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
                    if (e.get("offset")) |off| if (off.arr()) |oa| if (oa.len >= 2) {
                        q = q.add(V2.init(units.parseLength(oa[0]) orelse 0, units.parseLength(oa[1]) orelse 0));
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

// ---- lumber -------------------------------------------------------------------------------------------

fn buildLumber(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const size = p.str("size", null);
    const product = p.choice("product", "sawn", &.{ "sawn", "lvl", "psl", "lsl", "glulam" });
    const run = p.choice("run", "z", &.{ "z", "x", "y" });
    const orient = p.choice("orient", "upright", &.{ "upright", "flat" });
    const face = p.choice("face", "wide", &.{ "wide", "narrow" });
    const plies_i = p.int("plies", 1, 1, 8);
    const treated = p.boolean("treated", false);
    const blocking = p.boolean("blocking", false);
    _ = p.str("grade", "");
    const mat_over: ?[]const u8 = if (p.has("material")) p.str("material", null) else null;
    if (!p.ok or size == null or run == null) return null;
    const is_sawn = std.mem.eql(u8, product.?, "sawn");
    var sz: SawnSize = undefined;
    if (parseSawn(size.?)) |s| {
        sz = s;
    } else if (parseActual(size.?)) |s| {
        sz = s;
    } else {
        p.fail("size", "size \"{s}\" is not recognised. Use sawn nominal \"2x4\", \"2x6\", \"2x8\", \"2x10\", \"2x12\", \"4x4\".. \"4x12\", \"6x6\".. \"6x12\", or an actual \"thickness x depth\" such as \"1.75x9.25\"", .{size.?});
        return null;
    }
    const n_plies: f64 = @floatFromInt(plies_i.?);
    var length: f64 = 0;
    if (!std.mem.eql(u8, run.?, "z")) {
        length = p.lenPos("length", null, "lumber with run x or y needs its length, e.g. \"length\": 92.625") orelse return null;
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
    const wide = std.mem.eql(u8, face.?, "wide");
    var info_run: []const u8 = undefined;
    if (std.mem.eql(u8, run.?, "z")) {
        const flat = std.mem.eql(u8, orient.?, "flat");
        if (!flat) {
            w = t * n_plies;
            h = d;
            var k: usize = 0;
            while (k < plies_i.?) : (k += 1) {
                const x0 = @as(f64, @floatFromInt(k)) * t;
                try quads.append(a, quadOf(x0, 0, x0 + t, h));
                if (k > 0) try ply_lines.append(a, .{ V2.init(x0, 0), V2.init(x0, h) });
            }
        } else {
            w = d;
            h = t * n_plies;
            var k: usize = 0;
            while (k < plies_i.?) : (k += 1) {
                const y0 = @as(f64, @floatFromInt(k)) * t;
                try quads.append(a, quadOf(0, y0, w, y0 + t));
                if (k > 0) try ply_lines.append(a, .{ V2.init(0, y0), V2.init(w, y0) });
            }
        }
        info_run = if (flat) "flat run z" else "run z";
    } else {
        const along_x = std.mem.eql(u8, run.?, "x");
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
        if (!wide and plies_i.? > 1) {
            var k: usize = 1;
            while (k < plies_i.?) : (k += 1) {
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
    const material: []const u8 = mat_over orelse if (!is_sawn) "wood_engineered" else if (treated) "wood_treated" else "wood";
    if (!materialOk(ctx, "material", material)) return null;
    const loop = try model.rectLoop(a, 0, 0, w, h);
    const prism = Prism{
        .material = material,
        .loops = try model.oneLoop(a, loop),
        .quads = if (std.mem.eql(u8, run.?, "z")) quads.items else &.{},
        .blocking = blocking,
        .ply_lines = ply_lines.items,
    };
    var info: std.ArrayList(u8) = .empty;
    if (plies_i.? > 1) try info.print(a, "({d}) ", .{plies_i.?});
    try info.print(a, "{s}", .{size.?});
    if (!is_sawn) try info.print(a, " {s}", .{product.?});
    if (treated) try info.appendSlice(a, " PT");
    if (blocking) try info.appendSlice(a, " blocking");
    try info.print(a, " {s}", .{info_run});
    return .{
        .prisms = try onePrism(a, prism),
        .box = .{ .x0 = 0, .y0 = 0, .x1 = w, .y1 = h },
        .nat_z = nat,
        .info = info.items,
    };
}

// ---- panel -----------------------------------------------------------------------------------------------

fn buildPanel(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const material = p.choice("material", "osb", &.{ "osb", "plywood", "gypsum", "fiber_cement", "wood_board" });
    const thickness = p.lenPos("thickness", null, "e.g. 0.4375 for 7/16\" OSB");
    const length = p.lenPos("length", null, "the in-plane extent of the panel");
    const run = p.choice("run", "x", &.{ "x", "y" });
    if (!p.ok) return null;
    if (!materialOk(ctx, "material", material.?)) return null;
    const along_x = std.mem.eql(u8, run.?, "x");
    const w = if (along_x) length.? else thickness.?;
    const h = if (along_x) thickness.? else length.?;
    const loop = try model.rectLoop(a, 0, 0, w, h);
    const prism = Prism{
        .material = material.?,
        .loops = try model.oneLoop(a, loop),
        .quads = try a.dupe([4]V2, &.{quadOf(0, 0, w, h)}),
    };
    return .{
        .prisms = try onePrism(a, prism),
        .box = .{ .x0 = 0, .y0 = 0, .x1 = w, .y1 = h },
        .info = try std.fmt.allocPrint(a, "{s} {s}\" thk x {s}", .{ material.?, fmtNum(a, thickness.?), fmtNum(a, length.?) }),
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
        if (v.get(k)) |x| {
            if (units.parseLength(x)) |n| @field(c, k) = n else {
                ctx.p.fail(key, "{s}.{s} must be a length", .{ key, k });
                return null;
            }
        }
    }
    var parts: std.ArrayList(model.PartCover) = .empty;
    if (v.get("parts")) |pv| if (pv == .object) {
        for (pv.object) |m| {
            var pc = c;
            inline for (.{ "bottom", "sides", "top" }) |k| {
                if (m.value.get(k)) |x| if (units.parseLength(x)) |n| {
                    @field(pc, k) = n;
                };
            }
            parts.append(ctx.a, .{ .part = m.key, .cover = pc }) catch return null;
        }
    };
    return .{ .cover = c, .parts = parts.items };
}

// ---- cmu_wall -----------------------------------------------------------------------------------------------

fn buildCmu(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const wv = p.num("width", 8);
    const courses = p.int("courses", null, 1, 200);
    const bb_n = p.int("bond_beam_courses", 0, 0, 200);
    const grout = p.choice("grout", "reinforced", &.{ "solid", "reinforced", "none" });
    const fs = p.lenPos("face_shell", 1.25, "");
    const top_joint = p.boolean("top_joint", false);
    const cov = parseCover(ctx, "cover", .{ .bottom = 0.5, .sides = 1.5, .top = 1.5 });
    if (!p.ok) return null;
    const w: f64 = blk: {
        const x = wv.?;
        if (x == 6) break :blk 5.625;
        if (x == 8) break :blk 7.625;
        if (x == 10) break :blk 9.625;
        if (x == 12) break :blk 11.625;
        if (@abs(x - 5.625) < 1e-3 or @abs(x - 7.625) < 1e-3 or @abs(x - 9.625) < 1e-3 or @abs(x - 11.625) < 1e-3) break :blk x;
        p.fail("width", "param 'width' must be a nominal 6, 8, 10 or 12 (actual 5.625, 7.625, 9.625, 11.625) (got {s})", .{fmtNum(a, x)});
        return null;
    };
    const n: usize = @intCast(courses.?);
    const nbb: usize = @intCast(bb_n.?);
    if (nbb > n) {
        p.fail("bond_beam_courses", "param 'bond_beam_courses' ({d}) cannot exceed 'courses' ({d})", .{ nbb, n });
        return null;
    }
    if (fs.? * 2 >= w) {
        p.fail("face_shell", "face_shell {s} leaves no cell in a {s} wide wall", .{ fmtNum(a, fs.?), fmtNum(a, w) });
        return null;
    }
    const nf: f64 = @floatFromInt(n);
    const total_h = nf * 8.0 - 0.375 + (if (top_joint) @as(f64, 0.375) else 0);
    const g = grout.?;
    var prisms: std.ArrayList(Prism) = .empty;
    var zones: std.ArrayList(model.Zone) = .empty;
    var k: usize = 1;
    while (k <= n) : (k += 1) {
        const y0 = @as(f64, @floatFromInt(k - 1)) * 8.0;
        const y1 = y0 + 7.625;
        const is_bb = k > n - nbb;
        const part = try std.fmt.allocPrint(a, "course_{d}", .{k});
        try prisms.append(a, .{ .part = part, .material = "cmu", .loops = try model.oneLoop(a, try model.rectLoop(a, 0, y0, fs.?, y1)), .cmu_unit = true, .course = @intCast(k) });
        try prisms.append(a, .{ .part = part, .material = "cmu", .loops = try model.oneLoop(a, try model.rectLoop(a, w - fs.?, y0, w, y1)), .cmu_unit = true, .course = @intCast(k) });
        const grouted = !std.mem.eql(u8, g, "none") and (is_bb or std.mem.eql(u8, g, "solid") or std.mem.eql(u8, g, "reinforced"));
        const cell = try model.oneLoop(a, try model.rectLoop(a, fs.?, y0, w - fs.?, y1));
        if (grouted) {
            try prisms.append(a, .{ .part = "grout", .material = "grout", .loops = cell, .cmu_unit = true, .course = @intCast(k) });
        } else {
            try prisms.append(a, .{ .part = part, .material = "cmu", .loops = cell, .kind = .ghost, .pen = "beyond" });
        }
        if (k < n or top_joint) {
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
        if (gy0 < gy1) try zones.append(a, try zoneRect(a, "grout", fs.?, gy0, w - fs.?, gy1));
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
        .info = try std.fmt.allocPrint(a, "{s}\" x {d} courses ({d} bond beam)", .{ fmtNum(a, nom), n, nbb }),
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

fn buildConcrete(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const shape = p.choice("shape", null, &.{ "rect", "footing", "polygon", "slab_edge" });
    const material = p.str("material", "concrete");
    const cov = parseCover(ctx, "cover", .{});
    if (!p.ok) return null;
    if (!materialOk(ctx, "material", material.?)) return null;
    const sh = shape.?;
    if (std.mem.eql(u8, sh, "rect") or std.mem.eql(u8, sh, "footing")) {
        const w = p.lenPos("width", null, "rect/footing need width and height") orelse 0;
        const h = p.lenPos("height", null, "rect/footing need width and height") orelse 0;
        if (!p.ok) return null;
        const loop = try model.rectLoop(a, 0, 0, w, h);
        const part: []const u8 = if (std.mem.eql(u8, sh, "footing")) "footing" else "";
        const prism = Prism{ .part = part, .material = material.?, .loops = try model.oneLoop(a, loop) };
        const zones = if (part.len > 0) try a.dupe(model.Zone, &.{try zoneRect(a, "footing", 0, 0, w, h)}) else &[_]model.Zone{};
        return .{
            .prisms = try onePrism(a, prism),
            .zones = zones,
            .box = .{ .x0 = 0, .y0 = 0, .x1 = w, .y1 = h },
            .host = .{ .outline = loop, .cover = cov.?.cover, .part_cover = cov.?.parts },
            .info = try std.fmt.allocPrint(a, "{s} {s} x {s}", .{ sh, fmtNum(a, w), fmtNum(a, h) }),
        };
    }
    if (std.mem.eql(u8, sh, "polygon")) {
        const pv = p.raw("points") orelse {
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
        const prism = Prism{ .material = material.?, .loops = try model.oneLoop(a, loop) };
        const bx = geom.loopBox(loop);
        return .{
            .prisms = try onePrism(a, prism),
            .box = bx,
            .points_mode = true,
            .host = .{ .outline = loop, .cover = cov.?.cover, .part_cover = cov.?.parts },
            .info = try std.fmt.allocPrint(a, "polygon {d} pts", .{clean.len}),
        };
    }
    // slab_edge
    const exterior = p.choice("exterior", "left", &.{ "left", "right" });
    const st = p.lenPos("slab_thickness", 4, "");
    const sl = p.lenPos("slab_length", 48, "");
    const fw = p.lenPos("footing_width", 12, "");
    const fd = p.lenPos("footing_depth", 18, "");
    const haunch = p.num("haunch", 45);
    var rw: f64 = 0;
    var rd: f64 = 0;
    var re: f64 = 0;
    var has_recess = false;
    if (p.raw("recess")) |rv| {
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
            re = (if (rv.get("from_edge")) |x| units.parseLength(x) else null) orelse 0;
        }
    }
    const rslope = p.len("recess_slope", 0, "");
    if (!p.ok) return null;
    if (fd.? <= st.?) {
        p.fail("footing_depth", "footing_depth ({s}) must exceed slab_thickness ({s}): it is measured from the top of the slab", .{ fmtNum(a, fd.?), fmtNum(a, st.?) });
        return null;
    }
    if (haunch.? <= 0 or haunch.? > 90) {
        p.fail("haunch", "haunch must be an angle in degrees from horizontal, > 0 and <= 90 (got {s})", .{fmtNum(a, haunch.?)});
        return null;
    }
    if (has_recess) {
        if (rw <= 0 or rd <= 0 or rd >= st.?) {
            p.fail("recess", "recess width must be > 0 and depth in (0, slab_thickness {s}) (got width {s}, depth {s})", .{ fmtNum(a, st.?), fmtNum(a, rw), fmtNum(a, rd) });
            return null;
        }
        if (re + rw > sl.?) {
            p.fail("recess", "recess (from_edge {s} + width {s}) extends beyond slab_length {s}", .{ fmtNum(a, re), fmtNum(a, rw), fmtNum(a, sl.?) });
            return null;
        }
    }
    const hx = fw.? + (fd.? - st.?) / @tan(std.math.degreesToRadians(haunch.?));
    if (hx > sl.?) {
        p.fail("haunch", "the haunch meets the slab underside at x={s}, beyond slab_length {s}; lengthen the slab or steepen haunch", .{ fmtNum(a, hx), fmtNum(a, sl.?) });
        return null;
    }
    var pts: std.ArrayList(Pt) = .empty;
    try pts.append(a, .{ .x = 0, .y = -fd.? }); // footing bottom exterior
    const rec_ext_y = -rd - rslope.?;
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
    try pts.append(a, .{ .x = sl.?, .y = 0 });
    try pts.append(a, .{ .x = sl.?, .y = -st.? });
    try pts.append(a, .{ .x = hx, .y = -st.? });
    try pts.append(a, .{ .x = fw.?, .y = -fd.? });
    const loop = try orientedCcw(a, pts.items);
    const prism = Prism{ .material = material.?, .loops = try model.oneLoop(a, loop) };
    var anchors: std.ArrayList(model.NamedAnchor) = .empty;
    try anchors.append(a, .{ .name = "top_exterior", .p = V2.init(0, 0) });
    try anchors.append(a, .{ .name = "slab_top", .p = V2.init(sl.?, 0) });
    try anchors.append(a, .{ .name = "footing_bottom_exterior", .p = V2.init(0, -fd.?) });
    try anchors.append(a, .{ .name = "footing_bottom_interior", .p = V2.init(fw.?, -fd.?) });
    try anchors.append(a, .{ .name = "slab_bottom_interior", .p = V2.init(sl.?, -st.?) });
    try anchors.append(a, .{ .name = "haunch_top", .p = V2.init(hx, -st.?) });
    if (has_recess) {
        try anchors.append(a, .{ .name = "recess_bottom_exterior", .p = V2.init(re, rec_ext_y) });
        try anchors.append(a, .{ .name = "recess_bottom_interior", .p = V2.init(re + rw, -rd) });
        try anchors.append(a, .{ .name = "recess_top_interior", .p = V2.init(re + rw, 0) });
    }
    const zones = try a.dupe(model.Zone, &.{
        try zoneRect(a, "footing", 0, -fd.?, fw.?, -st.?),
        try zoneRect(a, "slab", 0, -st.?, sl.?, 0),
    });
    var built = Built{
        .prisms = try onePrism(a, prism),
        .anchors = anchors.items,
        .zones = zones,
        .box = geom.loopBox(loop),
        .host = .{ .outline = loop, .cover = cov.?.cover, .part_cover = cov.?.parts },
        .info = try std.fmt.allocPrint(a, "slab_edge {s}\" slab, {s}x{s} turndown", .{ fmtNum(a, st.?), fmtNum(a, fw.?), fmtNum(a, fd.?) }),
    };
    if (std.mem.eql(u8, exterior.?, "right")) {
        built = try mirrorBuilt(a, built, geom.Xf.scaling(-1, 1), false);
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

fn buildRebar(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const size = p.str("size", "#4");
    const mode = p.choice("mode", "along_z", &.{ "along_z", "path" });
    _ = p.str("spacing_note", "");
    if (!p.ok) return null;
    const d = rebarDiameter(size.?) orelse {
        p.fail("size", "bar size '{s}' is not recognised. Use #3 (.375), #4 (.5), #5 (.625), #6 (.75), #7 (.875) or #8 (1.0)", .{size.?});
        return null;
    };
    const r = d / 2.0;
    if (std.mem.eql(u8, mode.?, "along_z")) {
        const loop = try model.circleLoop(a, 0, 0, r);
        const prism = Prism{ .material = "rebar", .loops = try model.oneLoop(a, loop), .embedded = true, .sweep_r = r };
        var built = Built{
            .prisms = try onePrism(a, prism),
            .box = .{ .x0 = -r, .y0 = -r, .x1 = r, .y1 = r },
            .bar_d = d,
            .info = try std.fmt.allocPrint(a, "{s} along z", .{size.?}),
        };
        if (p.raw("place")) |pl| {
            built.centers = (try placeRebar(ctx, pl, d)) orelse return null;
        }
        return built;
    }
    // path
    const pv = p.raw("points") orelse {
        p.fail("points", "mode path needs 'points': [[x, y], ...] or Refs", .{});
        return null;
    };
    const pts = (try parsePointList(ctx, "points", pv, true)) orelse return null;
    const clean = try dropDuplicatePoints(a, pts);
    if (clean.len < 2) {
        p.fail("points", "a path bar needs at least 2 distinct points (got {d})", .{clean.len});
        return null;
    }
    const br = p.lenPos("bend_radius", 3.0 * d, "") orelse return null;
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
        .info = try std.fmt.allocPrint(a, "{s} path, {d} pts", .{ size.?, clean.len }),
    };
}

/// Cover-based placement: returns the world centres of the bars.
fn placeRebar(ctx: *Ctx, pl: json.Value, d: f64) BuildError!?[]const V2 {
    const a = ctx.a;
    const p = &ctx.p;
    if (pl != .object) {
        p.fail("place", "param 'place' must be {{\"in\": \"comp[.part]\", \"face\": \"bottom|top|left|right\", \"cover\": 3, \"count\": 2, \"side_cover\": 3}}", .{});
        return null;
    }
    const in_s = (if (pl.get("in")) |x| x.str() else null) orelse {
        p.fail("place/in", "place needs \"in\": \"<component>[.<part>]\", the host zone", .{});
        return null;
    };
    const face = (if (pl.get("face")) |x| x.str() else null) orelse "bottom";
    const cover = (if (pl.get("cover")) |x| units.parseLength(x) else null) orelse 1.5;
    const side_cover = (if (pl.get("side_cover")) |x| units.parseLength(x) else null) orelse cover;
    const count_f = (if (pl.get("count")) |x| x.num() else null) orelse 1;
    if (count_f < 1 or count_f != @round(count_f) or count_f > 200) {
        p.fail("place/count", "place.count must be an integer from 1 to 200 (got {s})", .{fmtNum(a, count_f)});
        return null;
    }
    const count: usize = @intFromFloat(count_f);
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
    if (eq(u8, face, "bottom") or eq(u8, face, "top")) {
        const y = if (eq(u8, face, "bottom")) box.y0 + cover + r else box.y1 - cover - r;
        const xa = box.x0 + side_cover + r;
        const xb = box.x1 - side_cover - r;
        if (xb < xa - 1e-9 and count > 1) {
            p.fail("place", "zone '{s}' is {s} wide: bars at side_cover {s} do not fit ({s} usable); reduce side_cover or count", .{ in_s, fmtNum(a, box.width()), fmtNum(a, side_cover), fmtNum(a, xb - xa) });
            return null;
        }
        var i: usize = 0;
        while (i < count) : (i += 1) {
            const x = if (count == 1) (box.x0 + box.x1) / 2 else xa + (xb - xa) * @as(f64, @floatFromInt(i)) / @as(f64, @floatFromInt(count - 1));
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
            const y = if (count == 1) (box.y0 + box.y1) / 2 else ya + (yb - ya) * @as(f64, @floatFromInt(i)) / @as(f64, @floatFromInt(count - 1));
            try out.append(a, V2.init(x, y));
        }
    } else {
        p.fail("place/face", "place.face must be one of \"bottom\", \"top\", \"left\", \"right\" (got \"{s}\")", .{face});
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

fn buildAnchorBolt(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const d = p.lenPos("diameter", 0.5, "") orelse return null;
    const embed = p.lenPos("embed", 7, "") orelse return null;
    const proj = p.lenPos("projection", 2.5, "") orelse return null;
    const hook = p.choice("hook", "J", &.{ "J", "L", "headed", "none" });
    const nut = p.boolean("nut_washer", true);
    if (!p.ok) return null;
    const r = d / 2.0;
    const rc = 1.5 * d; // centreline bend radius
    const hook_len = 3.0;
    var cl: std.ArrayList(Pt) = .empty;
    try cl.append(a, .{ .x = 0, .y = proj });
    const h = hook.?;
    if (std.mem.eql(u8, h, "J")) {
        const yb = -embed + rc;
        try cl.append(a, .{ .x = 0, .y = yb, .b = geom.bulgeFromSweep(std.math.pi) });
        try cl.append(a, .{ .x = 2 * rc, .y = yb });
        try cl.append(a, .{ .x = 2 * rc, .y = yb + hook_len });
    } else if (std.mem.eql(u8, h, "L")) {
        const yb = -embed + rc;
        try cl.append(a, .{ .x = 0, .y = yb, .b = geom.bulgeFromSweep(std.math.pi / 2.0) });
        try cl.append(a, .{ .x = rc, .y = -embed });
        try cl.append(a, .{ .x = rc + hook_len, .y = -embed });
    } else {
        try cl.append(a, .{ .x = 0, .y = -embed });
    }
    const rib = try path_geom.ribbon(a, cl.items, r, r);
    var prisms: std.ArrayList(Prism) = .empty;
    try prisms.append(a, .{ .part = "shank", .material = "steel", .loops = try model.oneLoop(a, rib), .embedded = true, .centerline = cl.items, .sweep_r = r });
    if (std.mem.eql(u8, h, "headed")) {
        try prisms.append(a, .{ .part = "shank", .material = "steel", .loops = try model.oneLoop(a, try model.rectLoop(a, -d, -embed, d, -embed + 0.5 * d)), .embedded = true, .zhalf = d });
    }
    if (nut) {
        const nut_h = 0.875 * d;
        const wash_t = @min(0.25, 0.4 * d + 0.03);
        const ytop = proj - 0.125;
        const wy0 = ytop - nut_h - wash_t;
        try prisms.append(a, .{ .part = "washer", .material = "steel", .loops = try model.oneLoop(a, try model.rectLoop(a, -1.5, wy0, 1.5, wy0 + wash_t)), .embedded = true, .zhalf = 1.5 });
        try prisms.append(a, .{ .part = "nut", .material = "steel", .loops = try model.oneLoop(a, try model.rectLoop(a, -0.9 * d, wy0 + wash_t, 0.9 * d, ytop)), .embedded = true, .zhalf = 0.9 * d });
    }
    const bx = boxOfPrisms(prisms.items);
    return .{
        .prisms = prisms.items,
        .anchors = try a.dupe(model.NamedAnchor, &.{.{ .name = "top_of_concrete", .p = V2.init(0, 0) }}),
        .box = bx,
        .nat_z = d,
        .info = try std.fmt.allocPrint(a, "{s}\" dia {s}-bolt, {s}\" embed", .{ fmtNum(a, d), h, fmtNum(a, embed) }),
    };
}

// ---- connector ------------------------------------------------------------------------------------------------------

const Hardware = struct { model: []const u8, width: f64, gauge: u32, note: []const u8 };

const hardware = [_]Hardware{
    .{ .model = "CS14", .width = 1.25, .gauge = 14, .note = "coil strap" },
    .{ .model = "CS16", .width = 1.25, .gauge = 16, .note = "coil strap" },
    .{ .model = "CS18", .width = 1.25, .gauge = 18, .note = "coil strap" },
    .{ .model = "CS20", .width = 1.25, .gauge = 20, .note = "coil strap" },
    .{ .model = "CS22", .width = 1.25, .gauge = 22, .note = "coil strap" },
    .{ .model = "MSTA24", .width = 1.25, .gauge = 12, .note = "strap tie" },
    .{ .model = "MSTA30", .width = 1.25, .gauge = 12, .note = "strap tie" },
    .{ .model = "MSTA36", .width = 1.25, .gauge = 12, .note = "strap tie" },
    .{ .model = "MST27", .width = 2.0625, .gauge = 12, .note = "strap tie" },
    .{ .model = "MST37", .width = 2.0625, .gauge = 12, .note = "strap tie" },
    .{ .model = "H1", .width = 1.375, .gauge = 18, .note = "hurricane tie" },
    .{ .model = "H2.5A", .width = 1.375, .gauge = 18, .note = "hurricane tie" },
    .{ .model = "H10A", .width = 1.375, .gauge = 18, .note = "hurricane tie" },
    .{ .model = "META16", .width = 1.25, .gauge = 18, .note = "embedded truss anchor" },
    .{ .model = "META20", .width = 1.25, .gauge = 18, .note = "embedded truss anchor" },
    .{ .model = "HETA12", .width = 1.25, .gauge = 16, .note = "embedded truss anchor" },
    .{ .model = "HETA16", .width = 1.25, .gauge = 16, .note = "embedded truss anchor" },
    .{ .model = "HETA20", .width = 1.25, .gauge = 16, .note = "embedded truss anchor" },
    .{ .model = "HETA24", .width = 1.25, .gauge = 16, .note = "embedded truss anchor" },
    .{ .model = "HHETA16", .width = 1.25, .gauge = 14, .note = "embedded truss anchor" },
    .{ .model = "HHETA20", .width = 1.25, .gauge = 14, .note = "embedded truss anchor" },
};

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
        else => null,
    };
}

fn buildConnector(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const model_name = p.str("model", "");
    const lay = p.choice("lay", "edge", &.{ "edge", "face" });
    const side = p.choice("side", "left", &.{ "left", "right" });
    _ = p.str("fasteners", "");
    var hw: ?Hardware = null;
    if (model_name) |m| if (m.len > 0) {
        for (hardware) |h| if (std.ascii.eqlIgnoreCase(h.model, m)) {
            hw = h;
            break;
        };
    };
    const gauge_default: f64 = if (hw) |h| @floatFromInt(h.gauge) else 18;
    const gauge_n = p.num("gauge", gauge_default);
    const width = p.lenPos("width", if (hw) |h| h.width else 1.25, "");
    const pv = p.raw("points") orelse {
        p.fail("points", "connector needs 'points': the strap polyline, e.g. [\"upper_plate@top_right\", \"beam@top_left\"] (Refs or [x, y])", .{});
        return null;
    };
    if (!p.ok) return null;
    const thickness = gaugeThickness(@intFromFloat(@round(gauge_n.?))) orelse {
        p.fail("gauge", "gauge {s} is not in the table; use 10, 11, 12, 14, 16, 18, 20 or 22", .{fmtNum(a, gauge_n.?)});
        return null;
    };
    const pts = (try parsePointList(ctx, "points", pv, true)) orelse return null;
    const clean = try dropDuplicatePoints(a, pts);
    if (clean.len < 2) {
        p.fail("points", "a connector needs at least 2 distinct points (got {d})", .{clean.len});
        return null;
    }
    const edge = std.mem.eql(u8, lay.?, "edge");
    const rib = if (edge)
        try path_geom.ribbon(a, clean, if (std.mem.eql(u8, side.?, "left")) thickness else 0, if (std.mem.eql(u8, side.?, "right")) thickness else 0)
    else
        try path_geom.ribbon(a, clean, width.? / 2, width.? / 2);
    const prism = Prism{ .material = "steel", .loops = try model.oneLoop(a, rib), .centerline = clean };
    return .{
        .prisms = try onePrism(a, prism),
        .box = geom.loopBox(rib),
        .nat_z = if (edge) width.? else thickness,
        .points_mode = true,
        .info = try std.fmt.allocPrint(a, "{s} {s}ga {s}", .{
            if (model_name != null and model_name.?.len > 0) model_name.? else "strap",
            fmtNum(a, gauge_n.?),
            lay.?,
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
        try prisms.append(a, .{ .part = "plate", .material = "steel", .loops = try model.oneLoop(a, pl), .kind = .ghost, .pen = "hidden", .embedded = true });
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
        .info = try std.fmt.allocPrint(a, "{s} pitch, {s}/{s} chords, {s} heel, {s}\" overhang", .{ if (pitch_v == .string) pitch_v.string else "?", top.?, bot.?, heel.?, fmtNum(a, ov.?) }),
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
        .info = try std.fmt.allocPrint(a, "{s} {s}\" thk, {d} pts", .{ material.?, fmtNum(a, t), clean.len }),
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
        .info = try std.fmt.allocPrint(a, "{s} fill, {d} pts", .{ material.?, clean.len }),
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
        .info = try std.fmt.allocPrint(a, "{s} {s} x {s}", .{ form.?, fmtNum(a, bx.width()), fmtNum(a, bx.height()) }),
    };
}

/// Sinusoidal loop line fitted to the box (batt insulation symbol).
fn battSymbol(a: Allocator, bx: Box) Allocator.Error![]const Pt {
    const horizontal = bx.width() >= bx.height();
    const long = if (horizontal) bx.width() else bx.height();
    const short = if (horizontal) bx.height() else bx.width();
    const loops: f64 = @max(2, @round(long / (short * 0.9)));
    const pad = short * 0.2;
    const pitch = (long - 2 * pad) / loops;
    const loop_w = 1.7 * pitch / (2.0 * std.math.pi);
    const amp = 0.42 * short;
    const steps_per = 20;
    const total: usize = @as(usize, @intFromFloat(loops)) * steps_per;
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
        .info = try std.fmt.allocPrint(a, "{s} {s} x {s}", .{ material.?, fmtNum(a, bx.width()), fmtNum(a, bx.height()) }),
    };
}

// ---- dispatch -----------------------------------------------------------------------------------------------------------------------

pub fn build(ctx: *Ctx) BuildError!?Built {
    const name = ctx.comp.ty.name;
    const eq = std.mem.eql;
    if (eq(u8, name, "lumber")) return buildLumber(ctx);
    if (eq(u8, name, "panel")) return buildPanel(ctx);
    if (eq(u8, name, "cmu_wall")) return buildCmu(ctx);
    if (eq(u8, name, "concrete")) return buildConcrete(ctx);
    if (eq(u8, name, "rebar")) return buildRebar(ctx);
    if (eq(u8, name, "anchor_bolt")) return buildAnchorBolt(ctx);
    if (eq(u8, name, "connector")) return buildConnector(ctx);
    if (eq(u8, name, "truss")) return buildTruss(ctx);
    if (eq(u8, name, "membrane")) return buildMembrane(ctx);
    if (eq(u8, name, "fill")) return buildFill(ctx);
    if (eq(u8, name, "insulation")) return buildInsulation(ctx);
    if (eq(u8, name, "solid")) return buildSolid(ctx);
    unreachable;
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
