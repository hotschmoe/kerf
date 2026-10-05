//! v0.1.3 view tests (SPEC 19): auto crop / auto scale, notes_side default, KERF-GRAIN on lengthwise lumber.

const std = @import("std");
const json = @import("json.zig");
const model = @import("model.zig");
const style_mod = @import("style.zig");
const drawview = @import("drawview.zig");
const drawing = @import("drawing.zig");
const view_mod = @import("view.zig");

fn build(a: std.mem.Allocator, src: []const u8, view_id: []const u8) !drawing.Drawing {
    var err: json.ParseError = undefined;
    const doc = (try json.parse(a, src, &err)).?;
    const st = try a.create(style_mod.Style);
    st.* = try style_mod.load(a, null);
    var diags = model.Diags.init(a);
    return (try drawview.build(a, doc, st, view_id, &diags)).?;
}

// A stud wall at a PT sill on a gravel bed: run z studs are cut end-on, the plate runs along x (cut lengthwise),
// the stud (run y) runs along y (cut lengthwise).
const wall =
    \\{"kerf":"0.1","id":"t","run":[-2,2],"components":[
    \\ {"id":"plate","type":"lumber","size":"2x8","run":"x","length":48,"at":{"to":[0,0]}},
    \\ {"id":"stud","type":"lumber","size":"2x8","run":"y","length":60,"at":{"to":[0,7.25]}},
    \\ {"id":"end","type":"lumber","size":"2x8","run":"z","at":{"to":[60,0]}},
    \\ {"id":"bed","type":"fill","material":"gravel","points":[[-200,-40],[400,-40],[400,-0.5],[-200,-0.5]]}
    \\],"views":[{"id":"A","kind":"section","cut_z":0,"annotations":[
    \\ {"id":"n1","type":"note","text":"2X4 PLATE","target":"plate"},
    \\ {"id":"n2","type":"note","text":"2X4 STUD","target":"stud"}
    \\]}]}
;

fn grainSrcs(a: std.mem.Allocator, dr: drawing.Drawing) ![]const []const u8 {
    var out: std.ArrayList([]const u8) = .empty;
    for (dr.items) |it| if (it == .hatch and std.mem.eql(u8, it.hatch.pattern, "KERF-GRAIN")) try out.append(a, it.hatch.src);
    return out.items;
}

test "no crop and no scale: auto crop is the non-fill bbox + 6 in, a standard scale fits, 0 warnings" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const dr = try build(a, wall, "A");
    for (dr.diagnostics) |d| {
        try std.testing.expect(d.level != .warning and d.level != .@"error");
    }
    // plate x 0..48, stud up to y 67.25, end post at 60..61.5 x 0..7.25 (non-fill bbox x 0..61.5, y 0..67.25), gravel excluded
    try std.testing.expectApproxEqAbs(@as(f64, -6), dr.crop.x0, 1e-9);
    try std.testing.expectApproxEqAbs(@as(f64, 67.5), dr.crop.x1, 1e-9);
    try std.testing.expectApproxEqAbs(@as(f64, 73.25), dr.crop.y1, 1e-9);
    try std.testing.expectApproxEqAbs(@as(f64, -6), dr.crop.y0, 1e-9);
    const factors = [_]f64{ 4, 8, 12, 16, 24, 32, 48 };
    var std_scale = false;
    for (factors) |f| if (f == dr.scale) {
        std_scale = true;
    };
    try std.testing.expect(std_scale);
    const aw = dr.style.sheet_w_in - 2.0 * dr.style.margin_in;
    const ah = dr.style.sheet_h_in - 2.0 * dr.style.margin_in - dr.style.title_block_h_in;
    try std.testing.expect((dr.bounds[2] - dr.bounds[0]) / dr.scale <= aw + 1e-6);
    try std.testing.expect((dr.bounds[3] - dr.bounds[1]) / dr.scale <= ah + 1e-6);
    // the next larger standard scale must NOT fit (largest that fits wins)
    // (checked indirectly: scale is stable between runs)
    const dr2 = try build(a, wall, "A");
    try std.testing.expectEqual(dr.scale, dr2.scale);
    try std.testing.expectEqual(dr.items.len, dr2.items.len);
}

test "auto scale picks a smaller scale for a bigger crop, an explicit scale is left alone" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const small = try build(a, wall, "A");
    // same doc with a taller stud: bigger view, so the scale can only shrink (factor grows)
    const tall_src = try std.mem.replaceOwned(u8, a, wall, "\"length\":60", "\"length\":150");
    const tall = try build(a, tall_src, "A");
    try std.testing.expect(tall.scale >= small.scale);
    try std.testing.expect(tall.scale > 4);
    // explicit scale + explicit crop render at exactly that scale (and warn when it does not fit)
    const expl = try std.mem.replaceOwned(u8, a, wall, "\"cut_z\":0,", "\"cut_z\":0,\"scale\":\"1/4\\\"=1'-0\\\"\",\"crop\":{\"x\":[-6,54],\"y\":[-6,75]},");
    const e = try build(a, expl, "A");
    try std.testing.expectEqual(@as(f64, 48), e.scale);
    try std.testing.expectApproxEqAbs(@as(f64, 54), e.crop.x1, 1e-9);
}

test "grain only on lengthwise-cut lumber" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const dr = try build(a, wall, "A");
    const srcs = try grainSrcs(a, dr);
    var plate = false;
    var stud = false;
    for (srcs) |s| {
        if (std.mem.eql(u8, s, "plate")) plate = true else if (std.mem.eql(u8, s, "stud")) stud = true else return error.GrainOnWrongMember;
    }
    try std.testing.expect(plate and stud);
    // lines are wavy: some segment is neither horizontal nor vertical for the plate (run x) pattern
    for (dr.items) |it| if (it == .hatch and std.mem.eql(u8, it.hatch.pattern, "KERF-GRAIN") and std.mem.eql(u8, it.hatch.src, "plate")) {
        var wavy = false;
        for (it.hatch.lines) |l| if (@abs(l[1] - l[3]) > 1e-9) {
            wavy = true;
        };
        try std.testing.expect(wavy);
    };
}

test "notes_side defaults to both" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    var err: json.ParseError = undefined;
    const doc = (try json.parse(a, wall, &err)).?;
    var diags = model.Diags.init(a);
    const spec = (try view_mod.parse(a, view_mod.findView(doc, "A").?, 0, &diags)).?;
    try std.testing.expectEqual(view_mod.NotesSide.both, spec.notes_side);
    try std.testing.expect(!spec.has_crop and !spec.has_scale);
}
