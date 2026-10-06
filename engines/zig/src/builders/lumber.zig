//! lumber (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

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
const onePrism = common.onePrism;
const ftin = common.ftin;
const quadOf = common.quadOf;
const parseSawn = common.parseSawn;
const parseActual = common.parseActual;
const materialOk = common.materialOk;
const lengthOrUntil = common.lengthOrUntil;
const materialOr = common.materialOr;

pub const Params = struct {
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

pub fn build(ctx: *Ctx) BuildError!?Built {
    const a = ctx.a;
    const p = &ctx.p;
    const lp = p.parse(Params) orelse return null;
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
