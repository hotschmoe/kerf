//! Helpers shared by the component builders (SPEC 5): the build context, rectangles/zones/prisms, nominal sizes, point-list parsing,
//! `length`/`until`, cover, mirroring. A component type's own file (`builders/<type>.zig`) holds its `Params` and `build`.

const std = @import("std");
const json = @import("../json.zig");
const geom = @import("../geom.zig");
const model = @import("../model.zig");
const units = @import("../units.zig");
const cast = @import("../num.zig");
const limits = @import("../limits.zig");
const scene_mod = @import("../scene.zig");
const style_mod = @import("../style.zig");
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

pub fn boxOfPrisms(prisms: []const Prism) Box {
    var b = Box{};
    for (prisms) |p| for (p.loops) |l| b.addBox(geom.loopBox(l));
    return b;
}

pub fn zoneRect(a: Allocator, name: []const u8, x0: f64, y0: f64, x1: f64, y1: f64) Allocator.Error!model.Zone {
    const l = try model.rectLoop(a, x0, y0, x1, y1);
    return .{ .name = name, .loops = try model.oneLoop(a, l), .box = .{ .x0 = x0, .y0 = y0, .x1 = x1, .y1 = y1 } };
}

pub fn onePrism(a: Allocator, prism: Prism) Allocator.Error![]Prism {
    const out = try a.alloc(Prism, 1);
    out[0] = prism;
    return out;
}

pub const ftin = units.ftin;

pub fn fmtNum(a: Allocator, x: f64) []const u8 {
    return json.fmtNumberAlloc(a, x) catch "?";
}

/// Rect quads for cut marks. Corners CCW from bottom-left.
pub fn quadOf(x0: f64, y0: f64, x1: f64, y1: f64) [4]V2 {
    return .{ V2.init(x0, y0), V2.init(x1, y0), V2.init(x1, y1), V2.init(x0, y1) };
}

pub const SawnSize = struct { t: f64, d: f64 };

pub fn sawnDepth(nominal_t: u32, nd: u32) ?f64 {
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

pub fn sawnThickness(nt: u32) ?f64 {
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
pub const min_bulge: f64 = 1e-6;

pub const max_bulge: f64 = 1e3;

pub fn pointsOr(ctx: *Ctx, key: []const u8, v: json.Value) BuildError!?[]Pt {
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

pub fn dropDuplicatePoints(a: Allocator, pts: []const Pt) Allocator.Error![]Pt {
    var out: std.ArrayList(Pt) = .empty;
    for (pts) |p| {
        if (out.items.len > 0 and V2.eql(out.items[out.items.len - 1].v(), p.v(), 1e-9)) continue;
        try out.append(a, p);
    }
    return out.items;
}

pub fn orientedCcw(a: Allocator, loop: []const Pt) Allocator.Error![]const Pt {
    if (geom.signedArea(loop) < 0) return geom.reverseLoop(a, loop);
    return loop;
}

pub fn materialOk(ctx: *Ctx, key: []const u8, name: []const u8) bool {
    if (ctx.style.material(name) != null) return true;
    var names: std.ArrayList([]const u8) = .empty;
    for (ctx.style.materials) |m| names.append(ctx.a, m.name) catch {};
    ctx.p.fail(key, "material '{s}' is not defined in the style. Available: {s}", .{ name, scene_mod.joinIds(ctx.a, names.items) });
    return false;
}

/// `length` or `until` (SPEC 17) for members that run along x or y. Returns the length in inches.
pub fn lengthOrUntil(ctx: *Ctx, run: []const u8, hint: []const u8, note: *[]const u8) BuildError!?f64 {
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

pub fn parseCover(ctx: *Ctx, key: []const u8, base: model.Cover) ?struct { cover: model.Cover, parts: []const model.PartCover } {
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

pub fn mirrorBuilt(a: Allocator, b: Built, xf: geom.Xf, keep_box: bool) Allocator.Error!Built {
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

pub fn vsOf(a: Allocator, pts: []const Pt) Allocator.Error![]V2 {
    const out = try a.alloc(V2, pts.len);
    for (pts, 0..) |q, i| out[i] = q.v();
    return out;
}

pub fn pathLen(vs: []const V2) f64 {
    var t: f64 = 0;
    for (vs[0 .. vs.len - 1], 0..) |q, i| t += q.dist(vs[i + 1]);
    return t;
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

/// A style material when the style defines it, else `fallback` (custom styles may lack the v0.1.2 additions).
pub fn materialOr(ctx: *const Ctx, name: []const u8, fallback: []const u8) []const u8 {
    return if (ctx.style.material(name) != null) name else fallback;
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
