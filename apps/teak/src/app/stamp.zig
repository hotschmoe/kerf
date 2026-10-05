//! Rubber-stamp labels (DESIGN §3): UNVERIFIED / VERIFIED drawn as a rotated
//! (-3 degrees) outlined box with stroke-font text, as canvas triangles.

const std = @import("std");
const Allocator = std.mem.Allocator;
const teak = @import("teak");
const draw = @import("../draw/mod.zig");

pub const Tri = teak.CanvasPrimitive.TriVertex;

pub const Size = struct { w: f32, h: f32 };

const cap: f64 = 8.5; // text cap height in px
const pad_x: f32 = 8;
const pad_y: f32 = 5;
const line_w: f32 = 1.5;
const angle: f32 = -3.0 * std.math.pi / 180.0;

pub fn size(font: *const draw.Font, text: []const u8) Size {
    const tw: f32 = @floatCast(font.textWidth(text, cap));
    // Extra margin so the rotated corners stay inside the canvas.
    return .{ .w = tw + 2 * pad_x + 8 + 6 * letterGap(text), .h = @as(f32, @floatCast(cap)) + 2 * pad_y + 10 };
}

fn letterGap(text: []const u8) f32 {
    return @floatFromInt(text.len);
}

const Builder = struct {
    a: Allocator,
    out: std.ArrayList(Tri) = .empty,
    cx: f32,
    cy: f32,
    cos: f32,
    sin: f32,
    color: [4]f32,

    fn rot(self: *const Builder, x: f32, y: f32) [2]f32 {
        const dx = x - self.cx;
        const dy = y - self.cy;
        return .{ self.cx + dx * self.cos - dy * self.sin, self.cy + dx * self.sin + dy * self.cos };
    }

    fn v(self: *const Builder, p: [2]f32) Tri {
        return .{ .x = p[0], .y = p[1], .r = self.color[0], .g = self.color[1], .b = self.color[2], .a = self.color[3] };
    }

    fn seg(self: *Builder, x0: f32, y0: f32, x1: f32, y1: f32, w: f32) !void {
        const dx = x1 - x0;
        const dy = y1 - y0;
        const len = @sqrt(dx * dx + dy * dy);
        if (len < 1e-4) return;
        const nx = -dy / len * w * 0.5;
        const ny = dx / len * w * 0.5;
        const a = self.v(self.rot(x0 + nx, y0 + ny));
        const b = self.v(self.rot(x1 + nx, y1 + ny));
        const c = self.v(self.rot(x1 - nx, y1 - ny));
        const d = self.v(self.rot(x0 - nx, y0 - ny));
        try self.out.appendSlice(self.a, &.{ a, b, c, a, c, d });
    }
};

/// Triangles for the stamp within a `size(...)`-sized canvas. Allocated in `a`.
pub fn build(a: Allocator, font: *const draw.Font, text: []const u8, color: [4]f32) ![]const Tri {
    const sz = size(font, text);
    var b: Builder = .{ .a = a, .cx = sz.w * 0.5, .cy = sz.h * 0.5, .cos = @cos(angle), .sin = @sin(angle), .color = color };
    // Box.
    const x0: f32 = 4;
    const y0: f32 = 5;
    const x1 = sz.w - 4;
    const y1 = sz.h - 5;
    try b.seg(x0 - line_w * 0.5, y0, x1 + line_w * 0.5, y0, line_w);
    try b.seg(x1, y0, x1, y1, line_w);
    try b.seg(x1 + line_w * 0.5, y1, x0 - line_w * 0.5, y1, line_w);
    try b.seg(x0, y1, x0, y0, line_w);
    // Text, centered, baseline in screen coordinates (y down).
    var polys: draw.font.Polylines = .{};
    defer polys.deinit(a);
    const tw: f32 = @floatCast(font.textWidth(text, cap));
    try font.textStrokes(a, &polys, text, cap, 0, 0, 0, .left, .baseline);
    const ox = sz.w * 0.5 - tw * 0.5;
    const oy = sz.h * 0.5 + @as(f32, @floatCast(cap)) * 0.5;
    for (0..polys.count()) |i| {
        const pl = polys.line(i);
        var k: usize = 1;
        while (k < pl.len) : (k += 1) {
            // Stroke font is y-up; flip into screen space.
            try b.seg(ox + @as(f32, @floatCast(pl[k - 1].x)), oy - @as(f32, @floatCast(pl[k - 1].y)), ox + @as(f32, @floatCast(pl[k].x)), oy - @as(f32, @floatCast(pl[k].y)), 1.2);
        }
    }
    return b.out.items;
}
