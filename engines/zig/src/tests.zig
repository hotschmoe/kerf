//! Cross-module tests on the reference documents and synthetic scenes.

const std = @import("std");
const json = @import("json.zig");
const geom = @import("geom.zig");
const model = @import("model.zig");
const style_mod = @import("style.zig");
const compile_mod = @import("compile.zig");
const load_mod = @import("load.zig");
const drawview = @import("drawview.zig");
const drawing = @import("drawing.zig");
const annot = @import("annot.zig");
const font_mod = @import("font.zig");
const testdocs = @import("testdocs.zig");
const api = @import("api.zig");
const V2 = geom.V2;

fn near(expected: f64, got: f64) !void {
    try std.testing.expectApproxEqAbs(expected, got, 1e-9);
}

test "slab_edge named anchors have the spec coordinates" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    var err: json.ParseError = undefined;
    const doc = (try json.parse(a, testdocs.slab, &err)).?;
    const st = try style_mod.load(a, null);
    var diags = model.Diags.init(a);
    const scene = try compile_mod.compile(a, doc, &st, &diags);
    const slab = scene.find("slab").?;
    const S = @TypeOf(scene.*);
    const get = struct {
        fn f(c: anytype, n: []const u8) V2 {
            return S.anchorPoint(c, 0, null, n).?;
        }
    }.f;
    const top_ext = get(slab, "top_exterior");
    try near(0, top_ext.x);
    try near(0, top_ext.y);
    const rbi = get(slab, "recess_bottom_interior");
    try near(8, rbi.x);
    try near(-1.5, rbi.y);
    const rbe = get(slab, "recess_bottom_exterior");
    try near(0, rbe.x);
    try near(-1.625, rbe.y); // recess_slope 0.125 falls toward the exterior
    const ht = get(slab, "haunch_top");
    try near(26, ht.x); // 12 + (18-4)/tan(45)
    try near(-4, ht.y);
    try near(-18, get(slab, "footing_bottom_exterior").y);
    // placement chain: top_bar is 6 right, 4.5 below top_exterior (its center anchor)
    const bar = scene.find("top_bar").?;
    const c = @TypeOf(scene.*).anchorPoint(bar, 0, null, "center").?;
    try near(6, c.x);
    try near(-4.5, c.y);
}

test "cover-based rebar placement and cover warnings" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const src =
        \\{"kerf":"0.1","id":"t","run":[-6,6],"components":[
        \\ {"id":"ftg","type":"concrete","shape":"rect","width":16,"height":12,"cover":{"bottom":3,"sides":3,"top":1.5}},
        \\ {"id":"ok_bars","type":"rebar","size":"#4","place":{"in":"ftg","face":"bottom","cover":3,"count":3,"side_cover":3}},
        \\ {"id":"bad_bar","type":"rebar","size":"#5","at":{"anchor":"center","to":{"ref":"ftg@bottom_left","offset":[6,2]}}}
        \\],"views":[]}
    ;
    var err: json.ParseError = undefined;
    const doc = (try json.parse(a, src, &err)).?;
    const st = try style_mod.load(a, null);
    const l = try load_mod.load(a, doc, &st, false);
    var cover_ids: usize = 0;
    for (l.diags.list.items) |d| if (std.mem.eql(u8, d.code, "W_COVER")) {
        cover_ids += 1;
        try std.testing.expectEqualStrings("bad_bar", d.id.?);
    };
    try std.testing.expectEqual(@as(usize, 1), cover_ids);
    // bars placed at clear cover 3 from the bottom: centre y = 3 + d/2, spread between 3+r and 16-3-r
    const ok = l.scene.find("ok_bars").?;
    try std.testing.expectEqual(@as(usize, 3), ok.xfs.len);
    const p0 = ok.xfs[0].apply(V2.init(0, 0));
    try near(3.25, p0.y);
    try near(3.25, p0.x);
    try near(16 - 3.25, ok.xfs[2].apply(V2.init(0, 0)).x);
}

test "reference documents are clean: no errors, no warnings" {
    for (testdocs.all) |src| {
        var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
        defer arena.deinit();
        const a = arena.allocator();
        var err: json.ParseError = undefined;
        const doc = (try json.parse(a, src, &err)).?;
        const st = try style_mod.load(a, null);
        const l = try load_mod.load(a, doc, &st, true);
        for (l.diags.list.items) |d| try std.testing.expect(d.level == .info);
    }
}

test "note layout: text blocks never overlap and notes sit outside the crop" {
    for (testdocs.all) |src| {
        var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
        defer arena.deinit();
        const a = arena.allocator();
        var err: json.ParseError = undefined;
        const doc = (try json.parse(a, src, &err)).?;
        const st = try style_mod.load(a, null);
        var diags = model.Diags.init(a);
        const dr = (try drawview.build(a, doc, &st, "A", &diags)).?;
        const font = try font_mod.Font.parse(a, font_mod.embedded);
        var boxes: std.ArrayList(struct { src: []const u8, q: [4]V2 }) = .empty;
        for (dr.items) |it| {
            if (it != .text or !std.mem.eql(u8, it.text.layer, "S-ANNO-NOTE")) continue;
            if (it.text.rot != 0) continue;
            try boxes.append(a, .{ .src = it.text.src, .q = annot.textPoly(&font, it.text, 0) });
        }
        try std.testing.expect(boxes.items.len > 0);
        for (boxes.items, 0..) |x, i| for (boxes.items[i + 1 ..]) |y| {
            if (std.mem.eql(u8, x.src, y.src)) continue;
            const bx = geom.pointsBox(&.{ .{ .x = x.q[0].x, .y = x.q[0].y }, .{ .x = x.q[2].x, .y = x.q[2].y } });
            const by = geom.pointsBox(&.{ .{ .x = y.q[0].x, .y = y.q[0].y }, .{ .x = y.q[2].x, .y = y.q[2].y } });
            // shrink slightly: touching blocks are fine
            try std.testing.expect(!bx.expand(-1e-6).overlaps(by.expand(-1e-6), 0));
        };
        // a note column is outside the crop
        for (dr.items) |it| {
            if (it != .text or !std.mem.eql(u8, it.text.layer, "S-ANNO-NOTE")) continue;
            if (std.mem.startsWith(u8, it.text.src, "l_")) continue; // labels sit inside the crop
            const c = dr.crop;
            const w = font.width(it.text.s, it.text.h);
            const outside = it.text.x >= c.x1 or it.text.x + w <= c.x0;
            try std.testing.expect(outside);
        }
    }
}

test "determinism: every API function is a pure function of its input" {
    const gpa = std.testing.allocator;
    const doc = std.mem.trim(u8, testdocs.truss, " \n\r\t");
    const cases = [_]struct { f: []const u8, extra: []const u8 }{
        .{ .f = "check", .extra = "" },
        .{ .f = "mesh", .extra = "" },
        .{ .f = "drawing", .extra = ",\"view\":\"A\"" },
        .{ .f = "drawing", .extra = ",\"view\":\"B\"" },
        .{ .f = "export", .extra = ",\"view\":\"A\",\"format\":\"svg\"" },
        .{ .f = "export", .extra = ",\"view\":\"B\",\"format\":\"dxf\"" },
        .{ .f = "export", .extra = ",\"view\":\"A\",\"format\":\"pdf\"" },
    };
    for (cases) |c| {
        const input = try std.fmt.allocPrint(gpa, "{{\"doc\":{s}{s}}}", .{ doc, c.extra });
        defer gpa.free(input);
        const r1 = try api.call(gpa, c.f, input);
        defer gpa.free(r1.bytes);
        const r2 = try api.call(gpa, c.f, input);
        defer gpa.free(r2.bytes);
        try std.testing.expect(r1.ok);
        try std.testing.expectEqualSlices(u8, r1.bytes, r2.bytes);
    }
}

test "apply is atomic and enforces the citation rule" {
    const gpa = std.testing.allocator;
    const doc = std.mem.trim(u8, testdocs.beam, " \n\r\t");
    // a failing second op leaves the document unchanged (ok:false, same doc)
    const bad = try std.fmt.allocPrint(gpa, "{{\"doc\":{s},\"ops\":[{{\"op\":\"update\",\"path\":\"components/beam\",\"value\":{{\"length\":40}}}},{{\"op\":\"remove\",\"path\":\"components/nope\"}}]}}", .{doc});
    defer gpa.free(bad);
    const r = try api.call(gpa, "apply", bad);
    defer gpa.free(r.bytes);
    try std.testing.expect(std.mem.startsWith(u8, r.bytes, "{\"ok\":false"));
    try std.testing.expect(std.mem.indexOf(u8, r.bytes, "\"length\": 42") != null);
    // LLM cannot verify a citation; designer can
    const op = "[{\"op\":\"update\",\"path\":\"views/A/annotations/n_strap\",\"value\":{\"cite\":[{\"code\":\"IRC\",\"section\":\"R1\",\"status\":\"verified\"}]}}]";
    const llm = try std.fmt.allocPrint(gpa, "{{\"doc\":{s},\"ops\":{s}}}", .{ doc, op });
    defer gpa.free(llm);
    const r2 = try api.call(gpa, "apply", llm);
    defer gpa.free(r2.bytes);
    try std.testing.expect(std.mem.indexOf(u8, r2.bytes, "I_CITE_DOWNGRADED") != null);
    try std.testing.expect(std.mem.indexOf(u8, r2.bytes, "\"verified\"") == null);
    const des = try std.fmt.allocPrint(gpa, "{{\"doc\":{s},\"ops\":{s},\"actor\":\"designer\"}}", .{ doc, op });
    defer gpa.free(des);
    const r3 = try api.call(gpa, "apply", des);
    defer gpa.free(r3.bytes);
    try std.testing.expect(std.mem.indexOf(u8, r3.bytes, "\"status\": \"verified\"") != null);
}
