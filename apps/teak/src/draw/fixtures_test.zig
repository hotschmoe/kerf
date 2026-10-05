//! Tests that run the whole draw pipeline over `apps/teak/fixtures/*` (real engine output +
//! the hand-written tiny set). Fixtures are read from the package root (`zig build test` runs
//! with cwd = apps/teak); if they cannot be found the tests are skipped, not failed.

const std = @import("std");
const testing = std.testing;
const builtin = @import("builtin");
const ir = @import("ir.zig");
const mesh = @import("mesh.zig");
const font_mod = @import("font.zig");
const tess = @import("tess.zig");
const raster = @import("raster.zig");
const png = @import("png.zig");
const render = @import("render.zig");
const pick = @import("pick.zig");
const geom = @import("geom.zig");

const real_details = [_][]const u8{ "truss-bearing-cmu", "monopour-slab-door-recess", "flush-beam-strap" };

fn readFixture(a: std.mem.Allocator, name: []const u8) ![]u8 {
    var buf: [256]u8 = undefined;
    const path = std.fmt.bufPrint(&buf, "fixtures/{s}", .{name}) catch return error.SkipZigTest;
    return std.Io.Dir.cwd().readFileAlloc(testing.io, path, a, .limited(64 * 1024 * 1024)) catch return error.SkipZigTest;
}

fn loadDrawing(a: std.mem.Allocator, name: []const u8) !ir.Drawing {
    var buf: [128]u8 = undefined;
    const fname = try std.fmt.bufPrint(&buf, "{s}.drawing.json", .{name});
    const bytes = try readFixture(a, fname);
    defer a.free(bytes);
    return ir.parse(a, bytes);
}

test "fixtures: tiny drawing has every item type and parses exactly" {
    var d = try loadDrawing(testing.allocator, "tiny");
    defer d.deinit();
    try testing.expectEqual(@as(usize, 12), d.items.len);
    try testing.expectEqual(@as(usize, 0), d.skipped_items);
    var counts = [_]usize{0} ** 4;
    for (d.items) |it| counts[@intFromEnum(it.kind())] += 1;
    try testing.expect(counts[@intFromEnum(ir.Kind.path)] >= 3);
    try testing.expectEqual(@as(usize, 1), counts[@intFromEnum(ir.Kind.hatch)]);
    try testing.expect(counts[@intFromEnum(ir.Kind.fill)] >= 2);
    try testing.expectEqual(@as(usize, 5), counts[@intFromEnum(ir.Kind.text)]);
    try testing.expectEqual(@as(usize, 1), d.diagnostics.len);
    try testing.expectEqual(@as(usize, 6), d.pens.len);
    try testing.expect(d.pens[d.findPen("hidden").?].dash_mm.len == 2);
}

test "fixtures: tiny mesh" {
    const bytes = try readFixture(testing.allocator, "tiny.mesh.json");
    defer testing.allocator.free(bytes);
    var m = try mesh.parse(testing.allocator, bytes);
    defer m.deinit();
    try testing.expectEqual(@as(usize, 2), m.parts.len);
    try testing.expectEqual(@as(usize, 24), m.triangleCount());
    try testing.expectEqual(@as(usize, 24), m.edgeCount());
    const b = m.bounds();
    try testing.expectEqual(@as(f32, 2), b.max[0]);
    try testing.expectEqual(@as(f32, -1), b.min[1]);
}

test "fixtures: real details parse, render to a small PNG, and look like drawings" {
    const a = testing.allocator;
    var font = try font_mod.Font.initEmbedded(a);
    defer font.deinit();
    for (real_details) |name| {
        var d = try loadDrawing(a, name);
        defer d.deinit();
        try testing.expect(d.items.len > 50);
        try testing.expectEqual(@as(usize, 0), d.skipped_items);
        try testing.expectEqual(@as(usize, 0), d.errorCount());
        try testing.expect(d.scale == 8 or d.scale == 12);
        try testing.expect(d.pens.len >= 8);

        var img = try render.renderImage(a, &d, &font, .{ .width_px = 1400 });
        defer img.deinit(a);
        try testing.expect(img.width >= 600 and img.width <= 1400);
        try testing.expect(img.height >= 300 and img.height <= 1568);
        // ink statistics
        var dark: usize = 0;
        var i: usize = 0;
        while (i < img.pixels.len) : (i += 3) {
            if (img.pixels[i] < 128) dark += 1;
        }
        const frac = @as(f64, @floatFromInt(dark)) / @as(f64, @floatFromInt(img.pixels.len / 3));
        try testing.expect(frac > 0.004 and frac < 0.2);
        // border is clean white
        try testing.expectEqual(@as([3]u8, .{ 255, 255, 255 }), img.pixel(1, 1));
        try testing.expectEqual(@as([3]u8, .{ 255, 255, 255 }), img.pixel(img.width - 2, img.height - 2));

        const bytes = try render.encodeImage(a, &img);
        defer a.free(bytes);
        try testing.expect(bytes.len < 150 * 1024); // gray8 + deflate; the API limit is 5 MB
        try testing.expect(bytes.len > 5 * 1024);
        // and the RGB path is also well under 300 KB
        const rgb = try png.encode(a, img.width, img.height, .rgb, img.pixels);
        defer a.free(rgb);
        try testing.expect(rgb.len < 300 * 1024);
    }
}

test "fixtures: real meshes parse with valid geometry" {
    const a = testing.allocator;
    for (real_details) |name| {
        var buf: [128]u8 = undefined;
        const fname = try std.fmt.bufPrint(&buf, "{s}.mesh.json", .{name});
        const bytes = try readFixture(a, fname);
        defer a.free(bytes);
        var m = try mesh.parse(a, bytes);
        defer m.deinit();
        try testing.expect(m.parts.len >= 5);
        try testing.expect(m.triangleCount() > 50);
        try testing.expect(m.edgeCount() > 20);
        const b = m.bounds();
        try testing.expect(!b.isEmpty() and b.radius() > 10);
        for (m.parts) |p| {
            try testing.expect(p.src.len > 0);
            try testing.expect(p.normals.len == 0 or p.normals.len == p.positions.len);
            for (p.indices) |ix| try testing.expect(ix < p.vertexCount());
        }
    }
}

test "fixtures: pick finds a src inside every hatch region of the real details" {
    const a = testing.allocator;
    var font = try font_mod.Font.initEmbedded(a);
    defer font.deinit();
    for (real_details) |name| {
        var d = try loadDrawing(a, name);
        defer d.deinit();
        var checked: usize = 0;
        for (d.items) |it| {
            if (it.body != .hatch or it.src.len == 0 or it.body.hatch.loops.len == 0) continue;
            const loops = it.body.hatch.loops;
            const bb = it.bbox;
            var found: ?geom.Vec2 = null;
            var gy: usize = 1;
            while (gy < 24 and found == null) : (gy += 1) {
                var gx: usize = 1;
                while (gx < 24 and found == null) : (gx += 1) {
                    const x = bb.x0 + bb.width() * @as(f64, @floatFromInt(gx)) / 24.0;
                    const y = bb.y0 + bb.height() * @as(f64, @floatFromInt(gy)) / 24.0;
                    if (geom.pointInLoopsEvenOdd(loops, x, y)) found = .{ .x = x, .y = y };
                }
            }
            const q = found orelse continue;
            try testing.expect(pick.pick(&d, &font, q.x, q.y, 0.0) != null);
            checked += 1;
        }
        try testing.expect(checked >= 1);
    }
}

test "fixtures: notes are pickable by their text and report an origin" {
    const a = testing.allocator;
    var font = try font_mod.Font.initEmbedded(a);
    defer font.deinit();
    var d = try loadDrawing(a, "truss-bearing-cmu");
    defer d.deinit();
    var n: usize = 0;
    for (d.items) |it| {
        if (it.body != .text or it.src.len == 0) continue;
        const t = it.body.text;
        if (t.rot != 0) continue;
        const w = font.textWidth(t.s, t.h);
        const cx = switch (t.halign) {
            .left => t.x + w * 0.5,
            .center => t.x,
            .right => t.x - w * 0.5,
        };
        const cy = t.y + t.h * 0.4;
        const got = pick.pick(&d, &font, cx, cy, 0.0).?;
        // another text item may overlap the sample (wrapped lines are close); must still be a src
        try testing.expect(got.len > 0);
        const o = pick.textOrigin(&d, it.src).?;
        try testing.expect(o.y >= t.y - 1e-9);
        n += 1;
    }
    try testing.expect(n > 10);
}

test "fixtures: tessellation of the real details (live look, hover + select) is finite and fast enough" {
    const a = testing.allocator;
    var font = try font_mod.Font.initEmbedded(a);
    defer font.deinit();
    var t = tess.Tessellator.init(a);
    defer t.deinit();
    for (real_details) |name| {
        var d = try loadDrawing(a, name);
        defer d.deinit();
        const view = tess.View.fit(d.bounds, 1200, 800, 24);
        const first_src = for (d.items) |it| {
            if (it.src.len > 0 and it.body == .path) break it.src;
        } else "";
        try t.build(&d, &font, view, tess.Palette.live(), .{ .hovered = first_src, .selected = first_src });
        try testing.expect(t.buf.triangleCount() > 2000);
        for (t.verts()) |v| {
            try testing.expect(std.math.isFinite(v.x) and std.math.isFinite(v.y));
            try testing.expect(v.a >= 0 and v.a <= 1.0001);
        }
        // zoomed way in on the middle: still sane and much smaller
        const z = view.zoomAt(600, 400, 20.0);
        try t.build(&d, &font, z, tess.Palette.live(), .{});
        for (t.verts()) |v| try testing.expect(std.math.isFinite(v.x) and std.math.isFinite(v.y));
    }
}

test "fixtures: render with the live palette and with hover/select produces a valid PNG" {
    const a = testing.allocator;
    var font = try font_mod.Font.initEmbedded(a);
    defer font.deinit();
    var d = try loadDrawing(a, "truss-bearing-cmu");
    defer d.deinit();
    const view = tess.View.fit(d.bounds, 1000, 700, 20);
    const bytes = try render.renderLivePng(a, &d, &font, view, "cmu", "sill_plate");
    defer a.free(bytes);
    try testing.expect(bytes.len > 10_000 and bytes.len < 400_000);
    try testing.expectEqualSlices(u8, &[_]u8{ 0x89, 'P', 'N', 'G' }, bytes[0..4]);
}

test "fixtures: tint of a selected component is visible (blue-ish pixel inside the CMU wall)" {
    const a = testing.allocator;
    var font = try font_mod.Font.initEmbedded(a);
    defer font.deinit();
    var d = try loadDrawing(a, "truss-bearing-cmu");
    defer d.deinit();
    const view = tess.View.fit(d.bounds, 1400, 1000, 20);
    var plain = try render.renderView(a, &d, &font, view, tess.Palette.live(), .{ .grid = false });
    defer plain.deinit(a);
    var sel = try render.renderView(a, &d, &font, view, tess.Palette.live(), .{ .grid = false, .selected = "cmu" });
    defer sel.deinit(a);
    // somewhere in the wall the selected render is bluer (b - r larger) than the plain render
    const bb = pick.bboxOfSrc(&d, &font, "cmu").?;
    var bluer: usize = 0;
    var y = bb.y0 + 1;
    while (y < bb.y1 - 1) : (y += 0.5) {
        const px: u32 = @intFromFloat(view.sx(bb.x0 + 1.0));
        const py: u32 = @intFromFloat(view.sy(y));
        const p0 = plain.pixel(px, py);
        const p1 = sel.pixel(px, py);
        if (@as(i32, p1[2]) - @as(i32, p1[0]) > @as(i32, p0[2]) - @as(i32, p0[0]) + 5) bluer += 1;
    }
    try testing.expect(bluer > 5);
    _ = builtin;
}
