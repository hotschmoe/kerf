//! membrane (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

const std = @import("std");
const json = @import("../json.zig");
const geom = @import("../geom.zig");
const model = @import("../model.zig");
const path_geom = @import("../pathgeom.zig");
const common = @import("common.zig");
const V2 = geom.V2;
const Built = model.Built;
const Prism = model.Prism;
const Ctx = common.Ctx;
const BuildError = common.BuildError;
const onePrism = common.onePrism;
const ftin = common.ftin;
const fmtNum = common.fmtNum;
const parsePointList = common.parsePointList;
const dropDuplicatePoints = common.dropDuplicatePoints;
const vsOf = common.vsOf;
const pathLen = common.pathLen;

fn membraneThickness(material: []const u8) f64 {
    const eq = std.mem.eql;
    if (eq(u8, material, "vapor_retarder")) return 0.04;
    if (eq(u8, material, "shingles")) return 0.25;
    if (eq(u8, material, "underlayment")) return 0.06;
    if (eq(u8, material, "wrb")) return 0.04;
    if (eq(u8, material, "flashing_membrane")) return 0.06;
    return 0.05;
}

pub const Params = struct {
    material: enum { membrane, underlayment, vapor_retarder, wrb, shingles, flashing_membrane } = .membrane,
    points: ?json.Value = null,
    thickness: ?f64 = null,
    side: enum { left, right } = .left,
    until: ?json.Value = null,

    pub const spec = .{
        .material = .{ .desc = "underlayment | vapor_retarder | wrb | shingles | flashing_membrane | membrane" },
        .points = .{ .def = "required", .desc = "polyline [x,y] or Refs" },
        .thickness = .{ .len = .pos, .def = "per material", .desc = "draw thickness (vapor retarder 0.04, shingles 0.25 typical)" },
        .side = .{ .desc = "which side of the polyline direction the thickness grows: left of dx,dy is (-dy,dx), right the opposite" },
        .until = .{ .desc = "a Ref (or {ref, offset}): the LAST segment grows or shrinks along its own direction until its end reaches the Ref's coordinate along that direction. With `slope`: \"@truss\" and points [[0,0],[12,0]] a roofing layer follows the roof and stops at e.g. \"truss@top_chord_end\"" },
    };
};

pub fn build(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const mp = p.parseAll(Params);
    const material = @tagName(mp.material);
    const pv = mp.points orelse {
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
    if (mp.until) |uv| {
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
    const left = mp.side == .left;
    const t: f64 = mp.thickness orelse membraneThickness(material);
    const rib = try path_geom.ribbon(a, clean, if (left) t else 0, if (left) 0 else t);
    // The drawn line sits at mid-thickness; vapor retarders keep a minimum separation from the host
    // (0.03 paper inch is applied at draw time via `line_gap`, here only the mid-thickness).
    const line = try path_geom.offsetOpen(a, clean, if (left) t / 2 else -t / 2);
    const prism = Prism{
        .material = material,
        .loops = try model.oneLoop(a, rib),
        .kind = .line,
        .line_pts = line,
        .ticks = std.mem.eql(u8, material, "shingles"),
        .centerline = clean,
    };
    return .{
        .prisms = try onePrism(a, prism),
        .box = geom.loopBox(rib),
        .points_mode = true,
        .info = try std.fmt.allocPrint(a, "membrane {s} {s} thick L={s}{s}", .{ material, ftin(a, t), ftin(a, pathLen(try vsOf(a, clean))), until_note }),
    };
}
