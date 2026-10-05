//! Drawing IR -> PNG: what `kerf_render` sends to Claude (white background, black ink,
//! true pen weights, ~1400 px wide) plus a "live look" variant (vellum, blue grid,
//! hover/selection) for inspecting the on-screen rendering.
//!
//! ```zig
//! const png_bytes = try render.renderPng(gpa, &drawing, .{ .width_px = 1400, .margin_px = 24 });
//! defer gpa.free(png_bytes);
//! ```
//!
//! Framing: `drawing.bounds` is fitted into `width_px - 2*margin_px`; the image height
//! follows the aspect ratio. If that exceeds `max_height_px` (default 1568, the API's
//! long-side recommendation) the scale is reduced instead and the image gets narrower.
//! Pen weights are `width_mm/25.4 * px_per_model_in * drawing.scale` px, min `min_line_px`:
//! exactly the tessellator's rule, so the PNG matches the live viewport at the same zoom.

const std = @import("std");
const Allocator = std.mem.Allocator;
const ir = @import("ir.zig");
const tess = @import("tess.zig");
const raster = @import("raster.zig");
const png = @import("png.zig");
const font_mod = @import("font.zig");

pub const Options = struct {
    width_px: u32 = 1400,
    margin_px: u32 = 24,
    max_height_px: u32 = 1568,
    selected: ?[]const u8 = null,
    hovered: ?[]const u8 = null,
    palette: tess.Palette = tess.Palette.white_ink(),
    grid: bool = false,
    antialias: bool = true,
    min_line_px: f32 = 1.0,
};

pub const Frame = struct {
    width: u32,
    height: u32,
    view: tess.View,
};

/// Compute image size and view for `drawing.bounds` under `opts`.
pub fn frame(drawing: *const ir.Drawing, opts: Options) Frame {
    const b = drawing.bounds;
    const bw = @max(b[2] - b[0], 1e-6);
    const bh = @max(b[3] - b[1], 1e-6);
    const m: f64 = @floatFromInt(opts.margin_px);
    var w: f64 = @floatFromInt(@max(opts.width_px, 2 * opts.margin_px + 16));
    var scale = (w - 2 * m) / bw;
    var h = bh * scale + 2 * m;
    const maxh: f64 = @floatFromInt(@max(opts.max_height_px, 2 * opts.margin_px + 16));
    if (h > maxh) {
        scale = (maxh - 2 * m) / bh;
        h = maxh;
        w = bw * scale + 2 * m;
    }
    const wi: u32 = @intFromFloat(@max(@round(w), 16));
    const hi: u32 = @intFromFloat(@max(@round(h), 16));
    return .{
        .width = wi,
        .height = hi,
        .view = tess.View.fit(b, @floatFromInt(wi), @floatFromInt(hi), @floatFromInt(opts.margin_px)),
    };
}

/// Rasterize with an explicit view (any window/zoom). Returned image is `view.width x view.height`.
pub fn renderView(a: Allocator, drawing: *const ir.Drawing, font: *const font_mod.Font, view: tess.View, pal: tess.Palette, topts: tess.Options) !raster.Image {
    var t = tess.Tessellator.init(a);
    defer t.deinit();
    try t.build(drawing, font, view, pal, topts);
    const w: u32 = @intFromFloat(@max(@round(view.width), 1));
    const h: u32 = @intFromFloat(@max(@round(view.height), 1));
    var img = try raster.Image.init(a, w, h, .{ 255, 255, 255 });
    errdefer img.deinit(a);
    img.drawTris(t.verts());
    return img;
}

pub fn renderImage(a: Allocator, drawing: *const ir.Drawing, font: *const font_mod.Font, opts: Options) !raster.Image {
    const f = frame(drawing, opts);
    return renderView(a, drawing, font, f.view, opts.palette, .{
        .selected = opts.selected,
        .hovered = opts.hovered,
        .grid = opts.grid,
        .antialias = opts.antialias,
        .min_line_px = opts.min_line_px,
    });
}

/// Encode an RGB image; if every pixel is gray (white-ink palette) an 8-bit gray PNG is written,
/// which is ~3x smaller raw and compresses better.
pub fn encodeImage(a: Allocator, img: *const raster.Image) ![]u8 {
    var gray = true;
    var i: usize = 0;
    while (i < img.pixels.len) : (i += 3) {
        if (img.pixels[i] != img.pixels[i + 1] or img.pixels[i] != img.pixels[i + 2]) {
            gray = false;
            break;
        }
    }
    if (!gray) return png.encode(a, img.width, img.height, .rgb, img.pixels);
    const g = try a.alloc(u8, @as(usize, img.width) * img.height);
    defer a.free(g);
    for (g, 0..) |*v, k| v.* = img.pixels[k * 3];
    return png.encode(a, img.width, img.height, .gray, g);
}

pub fn renderPngWithFont(a: Allocator, drawing: *const ir.Drawing, font: *const font_mod.Font, opts: Options) ![]u8 {
    var img = try renderImage(a, drawing, font, opts);
    defer img.deinit(a);
    return encodeImage(a, &img);
}

/// Render with the embedded stroke font (needs the `spec_font_json` import).
pub fn renderPng(a: Allocator, drawing: *const ir.Drawing, opts: Options) ![]u8 {
    var font = try font_mod.Font.initEmbedded(a);
    defer font.deinit();
    return renderPngWithFont(a, drawing, &font, opts);
}

/// Live-UI look (vellum, blue grid, hover/selection) at an explicit view.
pub fn renderLivePng(a: Allocator, drawing: *const ir.Drawing, font: *const font_mod.Font, view: tess.View, selected: ?[]const u8, hovered: ?[]const u8) ![]u8 {
    var img = try renderView(a, drawing, font, view, tess.Palette.live(), .{ .selected = selected, .hovered = hovered });
    defer img.deinit(a);
    return encodeImage(a, &img);
}

const testing = std.testing;

test "frame: aspect, margin, and max height clamp" {
    var d = try ir.parse(testing.allocator, ir.tiny_json); // bounds 10 x 8
    defer d.deinit();
    const f = frame(&d, .{ .width_px = 1000, .margin_px = 20 });
    try testing.expectEqual(@as(u32, 1000), f.width);
    // (1000-40)/10 = 96 px/in ; height = 8*96+40 = 808
    try testing.expectEqual(@as(u32, 808), f.height);
    try testing.expectApproxEqAbs(@as(f32, 96), f.view.px_per_model_in, 1e-3);
    const g = frame(&d, .{ .width_px = 1000, .margin_px = 20, .max_height_px = 500 });
    try testing.expectEqual(@as(u32, 500), g.height);
    try testing.expect(g.width < 1000);
    // bounds inside the image
    const bb = d.boundsBox();
    try testing.expect(f.view.sx(bb.x0) >= 19.9 and f.view.sx(bb.x1) <= 1000 - 19.9);
    try testing.expect(f.view.sy(bb.y1) >= 19.9 and f.view.sy(bb.y0) <= 808 - 19.9);
}

test "renderPng on the tiny drawing: valid PNG, ink present, white margins" {
    var d = try ir.parse(testing.allocator, ir.tiny_json);
    defer d.deinit();
    var font = try font_mod.Font.initEmbedded(testing.allocator);
    defer font.deinit();
    var img = try renderImage(testing.allocator, &d, &font, .{ .width_px = 600 });
    defer img.deinit(testing.allocator);
    try testing.expectEqual(@as(u32, 600), img.width);
    // corner is white
    try testing.expectEqual(@as([3]u8, .{ 255, 255, 255 }), img.pixel(2, 2));
    var dark: usize = 0;
    var i: usize = 0;
    while (i < img.pixels.len) : (i += 3) {
        if (img.pixels[i] < 100) dark += 1;
    }
    try testing.expect(dark > 200);
    const bytes = try renderPng(testing.allocator, &d, .{ .width_px = 600 });
    defer testing.allocator.free(bytes);
    try testing.expect(bytes.len > 100 and bytes.len < 100_000);
    try testing.expectEqualSlices(u8, &[_]u8{ 0x89, 'P', 'N', 'G' }, bytes[0..4]);
}
