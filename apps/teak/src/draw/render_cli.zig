//! Native dev tool (NOT part of the wasm/lib build): render a Drawing IR JSON to a PNG.
//!
//!   render_cli <in.drawing.json> <out.png> [--live] [--width N] [--hover SRC] [--select SRC]
//!              [--grid] [--no-aa] [--window x0,y0,x1,y1] [--min-line PX]
//!
//! Default is the `kerf_render` look (white, black ink, no grid, 1400 px wide).
//! `--live` uses the on-screen look (vellum, blue grid, hover/selection colours).
//! `--window` frames a model-space rectangle (inches) instead of the drawing bounds,
//! to inspect details zoomed in. Build + run via `apps/teak/tools/gen_pngs.sh`.

const std = @import("std");
const ir = @import("ir.zig");
const tess = @import("tess.zig");
const font_mod = @import("font.zig");
const render = @import("render.zig");

pub fn main(init: std.process.Init) !void {
    const gpa = init.gpa;
    const io = init.io;
    const args = try init.minimal.args.toSlice(init.arena.allocator());
    if (args.len < 3) {
        std.debug.print("usage: render_cli <in.json> <out.png> [--live] [--width N] [--hover SRC] [--select SRC] [--grid] [--no-aa] [--window x0,y0,x1,y1] [--min-line PX]\n", .{});
        return error.Usage;
    }
    var live = false;
    var width: u32 = 0;
    var hover: ?[]const u8 = null;
    var select: ?[]const u8 = null;
    var grid = false;
    var aa = true;
    var window: ?[4]f64 = null;
    var min_line: f32 = 1.0;
    var i: usize = 3;
    while (i < args.len) : (i += 1) {
        const a = args[i];
        if (std.mem.eql(u8, a, "--live")) live = true else if (std.mem.eql(u8, a, "--grid")) grid = true else if (std.mem.eql(u8, a, "--no-aa")) aa = false else if (std.mem.eql(u8, a, "--width")) {
            i += 1;
            width = try std.fmt.parseInt(u32, args[i], 10);
        } else if (std.mem.eql(u8, a, "--hover")) {
            i += 1;
            hover = args[i];
        } else if (std.mem.eql(u8, a, "--select")) {
            i += 1;
            select = args[i];
        } else if (std.mem.eql(u8, a, "--min-line")) {
            i += 1;
            min_line = try std.fmt.parseFloat(f32, args[i]);
        } else if (std.mem.eql(u8, a, "--window")) {
            i += 1;
            var w: [4]f64 = undefined;
            var it = std.mem.splitScalar(u8, args[i], ',');
            for (0..4) |k| w[k] = try std.fmt.parseFloat(f64, it.next() orelse return error.Usage);
            window = w;
        } else {
            std.debug.print("unknown arg {s}\n", .{a});
            return error.Usage;
        }
    }
    const bytes = try std.Io.Dir.cwd().readFileAlloc(io, args[1], gpa, .limited(256 * 1024 * 1024));
    defer gpa.free(bytes);
    var d = try ir.parse(gpa, bytes);
    defer d.deinit();
    var font = try font_mod.Font.initEmbedded(gpa);
    defer font.deinit();

    const w_px: u32 = if (width != 0) width else if (live) 1200 else 1400;
    var out: []u8 = undefined;
    if (window != null or live) {
        const b = window orelse d.bounds;
        const bw = @max(b[2] - b[0], 1e-6);
        const bh = @max(b[3] - b[1], 1e-6);
        const margin: f32 = 24;
        const h_px: f32 = @floatCast(@as(f64, @floatFromInt(w_px)) * bh / bw);
        const view = tess.View.fit(b, @floatFromInt(w_px), @max(h_px, 16), 0);
        _ = margin;
        const pal = if (live) tess.Palette.live() else tess.Palette.white_ink();
        var img = try render.renderView(gpa, &d, &font, view, pal, .{
            .selected = select,
            .hovered = hover,
            .grid = grid or live,
            .antialias = aa,
            .min_line_px = min_line,
        });
        defer img.deinit(gpa);
        out = try render.encodeImage(gpa, &img);
    } else {
        out = try render.renderPngWithFont(gpa, &d, &font, .{
            .width_px = w_px,
            .selected = select,
            .hovered = hover,
            .grid = grid,
            .antialias = aa,
            .min_line_px = min_line,
        });
    }
    defer gpa.free(out);
    try std.Io.Dir.cwd().writeFile(io, .{ .sub_path = args[2], .data = out });
    std.debug.print("{s}: {d} bytes ({d} items, scale {d})\n", .{ args[2], out.len, d.items.len, d.scale });
}
