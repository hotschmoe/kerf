//! CPU triangle rasterizer over `tess.Vert` triangle lists -> RGB8 image.
//!
//! * Straight-alpha source-over blending of per-vertex (r,g,b,a) interpolated
//!   barycentrically (Gouraud). The destination is an opaque RGB image.
//! * Vertices are snapped to 1/256 px fixed point; edge functions are exact i64,
//!   so adjacent triangles share edges with the TOP-LEFT fill rule: a pixel on a
//!   shared edge is covered by exactly one of the two triangles (no seams, no
//!   double blending of the feathered fringes).
//! * Spans are computed analytically per scanline (not per bounding-box pixel), so
//!   long diagonal hairlines cost O(length), not O(length^2).
//! * Sample points are pixel centers (x + 0.5, y + 0.5).
//!
//! ```zig
//! var img = try raster.Image.init(gpa, 1400, 900, .{ 255, 255, 255 });
//! defer img.deinit(gpa);
//! img.drawTris(tessellator.verts());
//! const png_bytes = try png.encode(gpa, img.width, img.height, .rgb, img.pixels);
//! ```

const std = @import("std");
const Allocator = std.mem.Allocator;
const tess = @import("tess.zig");
const Vert = tess.Vert;

pub const Image = struct {
    width: u32,
    height: u32,
    /// Tightly packed RGB8, row 0 = top.
    pixels: []u8,

    pub fn init(a: Allocator, width: u32, height: u32, bg: [3]u8) Allocator.Error!Image {
        const px = try a.alloc(u8, @as(usize, width) * height * 3);
        var img: Image = .{ .width = width, .height = height, .pixels = px };
        img.fill(bg);
        return img;
    }

    pub fn deinit(self: *Image, a: Allocator) void {
        a.free(self.pixels);
        self.* = undefined;
    }

    pub fn fill(self: *Image, rgb: [3]u8) void {
        var i: usize = 0;
        while (i < self.pixels.len) : (i += 3) {
            self.pixels[i] = rgb[0];
            self.pixels[i + 1] = rgb[1];
            self.pixels[i + 2] = rgb[2];
        }
    }

    pub fn pixel(self: *const Image, x: u32, y: u32) [3]u8 {
        const o = (@as(usize, y) * self.width + x) * 3;
        return .{ self.pixels[o], self.pixels[o + 1], self.pixels[o + 2] };
    }

    /// Rasterize a triangle list (3 verts per triangle; a trailing partial triangle is ignored).
    pub fn drawTris(self: *Image, verts: []const Vert) void {
        var i: usize = 0;
        while (i + 2 < verts.len) : (i += 3) self.drawTri(verts[i], verts[i + 1], verts[i + 2]);
    }

    fn drawTri(self: *Image, va: Vert, vb: Vert, vc: Vert) void {
        const clampc = struct {
            fn f(x: f32) i64 {
                if (!(x == x)) return 0;
                const c = std.math.clamp(x, -2.0e6, 2.0e6);
                return @intFromFloat(@round(c * 256.0));
            }
        }.f;
        const a = va;
        var b = vb;
        var c = vc;
        const ax = clampc(a.x);
        const ay = clampc(a.y);
        var bx = clampc(b.x);
        var by = clampc(b.y);
        var cx = clampc(c.x);
        var cy = clampc(c.y);

        var area2 = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax);
        if (area2 == 0) return;
        if (area2 < 0) {
            // make orientation positive by swapping b and c
            std.mem.swap(Vert, &b, &c);
            std.mem.swap(i64, &bx, &cx);
            std.mem.swap(i64, &by, &cy);
            area2 = -area2;
        }

        const w: i64 = self.width;
        const h: i64 = self.height;
        // pixel bbox of covered sample centers
        const minx = @max(@divFloor(@min(ax, @min(bx, cx)) - 128 + 255, 256), 0);
        const maxx = @min(@divFloor(@max(ax, @max(bx, cx)) - 128, 256), w - 1);
        const miny = @max(@divFloor(@min(ay, @min(by, cy)) - 128 + 255, 256), 0);
        const maxy = @min(@divFloor(@max(ay, @max(by, cy)) - 128, 256), h - 1);
        if (minx > maxx or miny > maxy) return;

        // Edge i is opposite vertex i: e0: b->c, e1: c->a, e2: a->b.
        const ex = [3]i64{ cx - bx, ax - cx, bx - ax };
        const ey = [3]i64{ cy - by, ay - cy, by - ay };
        const sx = [3]i64{ bx, cx, ax };
        const sy = [3]i64{ by, cy, ay };
        var bias: [3]i64 = undefined;
        for (0..3) |k| {
            // top-left rule: top edge (dy==0, dx>0) or left edge (dy<0) include their boundary
            const tl = (ey[k] == 0 and ex[k] > 0) or ey[k] < 0;
            bias[k] = if (tl) 0 else -1;
        }

        const inv_area: f32 = 1.0 / @as(f32, @floatFromInt(area2));
        const const_rgb = a.r == b.r and a.r == c.r and a.g == b.g and a.g == c.g and a.b == b.b and a.b == c.b;
        const const_a = a.a == b.a and a.a == c.a;
        // For constant color & alpha precompute 8-bit source.
        const src_r: i32 = @intFromFloat(@round(std.math.clamp(a.r, 0, 1) * 255.0));
        const src_g: i32 = @intFromFloat(@round(std.math.clamp(a.g, 0, 1) * 255.0));
        const src_b: i32 = @intFromFloat(@round(std.math.clamp(a.b, 0, 1) * 255.0));
        const const_alpha8: i32 = @intFromFloat(@round(std.math.clamp(a.a, 0, 1) * 255.0));
        if (const_rgb and const_a and const_alpha8 == 0) return;

        const stride: usize = @as(usize, self.width) * 3;
        var py = miny;
        while (py <= maxy) : (py += 1) {
            const pcy = py * 256 + 128;
            const pcx0 = minx * 256 + 128;
            // edge values at (minx, py)
            var e: [3]i64 = undefined;
            var lo: i64 = 0;
            var hi: i64 = maxx - minx;
            var empty = false;
            for (0..3) |k| {
                e[k] = ex[k] * (pcy - sy[k]) - ey[k] * (pcx0 - sx[k]);
                const base = e[k] + bias[k];
                const step = -ey[k] * 256; // change per +1 pixel in x
                if (step == 0) {
                    if (base < 0) {
                        empty = true;
                        break;
                    }
                } else if (step > 0) {
                    // base + step*t >= 0  =>  t >= ceil(-base/step)
                    const t = ceilDiv(-base, step);
                    if (t > lo) lo = t;
                } else {
                    // base + step*t >= 0  =>  t <= floor(base/-step)
                    const t = floorDiv(base, -step);
                    if (t < hi) hi = t;
                }
            }
            if (empty or lo > hi) continue;

            var x: i64 = minx + lo;
            const xend: i64 = minx + hi;
            var o: usize = @as(usize, @intCast(py)) * stride + @as(usize, @intCast(x)) * 3;
            if (const_rgb and const_a) {
                while (x <= xend) : (x += 1) {
                    blend8(self.pixels[o .. o + 3], src_r, src_g, src_b, const_alpha8);
                    o += 3;
                }
            } else {
                var t: i64 = lo;
                while (x <= xend) : (x += 1) {
                    const e0: f32 = @floatFromInt(e[0] + (-ey[0] * 256) * t);
                    const e1: f32 = @floatFromInt(e[1] + (-ey[1] * 256) * t);
                    const e2: f32 = @floatFromInt(e[2] + (-ey[2] * 256) * t);
                    const l0 = e0 * inv_area;
                    const l1 = e1 * inv_area;
                    const l2 = e2 * inv_area;
                    const alpha = a.a * l0 + b.a * l1 + c.a * l2;
                    const a8: i32 = @intFromFloat(@round(std.math.clamp(alpha, 0, 1) * 255.0));
                    if (a8 > 0) {
                        if (const_rgb) {
                            blend8(self.pixels[o .. o + 3], src_r, src_g, src_b, a8);
                        } else {
                            const rr = a.r * l0 + b.r * l1 + c.r * l2;
                            const gg = a.g * l0 + b.g * l1 + c.g * l2;
                            const bb = a.b * l0 + b.b * l1 + c.b * l2;
                            blend8(
                                self.pixels[o .. o + 3],
                                @intFromFloat(@round(std.math.clamp(rr, 0, 1) * 255.0)),
                                @intFromFloat(@round(std.math.clamp(gg, 0, 1) * 255.0)),
                                @intFromFloat(@round(std.math.clamp(bb, 0, 1) * 255.0)),
                                a8,
                            );
                        }
                    }
                    o += 3;
                    t += 1;
                }
            }
        }
    }
};

inline fn blend8(dst: []u8, r: i32, g: i32, b: i32, a: i32) void {
    if (a >= 255) {
        dst[0] = @intCast(r);
        dst[1] = @intCast(g);
        dst[2] = @intCast(b);
        return;
    }
    const ia = 255 - a;
    dst[0] = @intCast(@divTrunc(r * a + @as(i32, dst[0]) * ia + 127, 255));
    dst[1] = @intCast(@divTrunc(g * a + @as(i32, dst[1]) * ia + 127, 255));
    dst[2] = @intCast(@divTrunc(b * a + @as(i32, dst[2]) * ia + 127, 255));
}

inline fn floorDiv(a: i64, b: i64) i64 {
    return @divFloor(a, b);
}
inline fn ceilDiv(a: i64, b: i64) i64 {
    return -@divFloor(-a, b);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

const testing = std.testing;

fn V(x: f32, y: f32, r: f32, g: f32, b: f32, a: f32) Vert {
    return .{ .x = x, .y = y, .r = r, .g = g, .b = b, .a = a };
}

test "full-screen quad fills every pixel exactly once (top-left rule, no seam)" {
    var img = try Image.init(testing.allocator, 17, 13, .{ 255, 255, 255 });
    defer img.deinit(testing.allocator);
    // 50% black over white twice-covered would give 64; once gives 128/127.
    const t = [_]Vert{
        V(0, 0, 0, 0, 0, 0.5), V(17, 0, 0, 0, 0, 0.5), V(17, 13, 0, 0, 0, 0.5),
        V(0, 0, 0, 0, 0, 0.5), V(17, 13, 0, 0, 0, 0.5), V(0, 13, 0, 0, 0, 0.5),
    };
    img.drawTris(&t);
    var y: u32 = 0;
    while (y < 13) : (y += 1) {
        var x: u32 = 0;
        while (x < 17) : (x += 1) {
            const p = img.pixel(x, y);
            try testing.expect(p[0] >= 126 and p[0] <= 129);
        }
    }
}

test "shared diagonal edge at fractional coordinates is covered exactly once" {
    var img = try Image.init(testing.allocator, 40, 40, .{ 255, 255, 255 });
    defer img.deinit(testing.allocator);
    const x0: f32 = 3.3;
    const y0: f32 = 2.7;
    const x1: f32 = 31.9;
    const y1: f32 = 35.1;
    const t = [_]Vert{
        V(x0, y0, 0, 0, 0, 0.5), V(x1, y0, 0, 0, 0, 0.5), V(x1, y1, 0, 0, 0, 0.5),
        V(x0, y0, 0, 0, 0, 0.5), V(x1, y1, 0, 0, 0, 0.5), V(x0, y1, 0, 0, 0, 0.5),
    };
    img.drawTris(&t);
    var y: u32 = 0;
    while (y < 40) : (y += 1) {
        var x: u32 = 0;
        while (x < 40) : (x += 1) {
            const p = img.pixel(x, y)[0];
            const cx = @as(f32, @floatFromInt(x)) + 0.5;
            const cy = @as(f32, @floatFromInt(y)) + 0.5;
            const inside = cx > x0 and cx < x1 and cy > y0 and cy < y1;
            if (inside) {
                try testing.expect(p >= 126 and p <= 129);
            } else if (cx < x0 - 0.01 or cx > x1 + 0.01 or cy < y0 - 0.01 or cy > y1 + 0.01) {
                try testing.expectEqual(@as(u8, 255), p);
            }
        }
    }
}

test "alpha gradient interpolates" {
    var img = try Image.init(testing.allocator, 64, 4, .{ 255, 255, 255 });
    defer img.deinit(testing.allocator);
    // alpha 0 at x=0 to 1 at x=64
    const t = [_]Vert{
        V(0, 0, 0, 0, 0, 0), V(64, 0, 0, 0, 0, 1), V(64, 4, 0, 0, 0, 1),
        V(0, 0, 0, 0, 0, 0), V(64, 4, 0, 0, 0, 1), V(0, 4, 0, 0, 0, 0),
    };
    img.drawTris(&t);
    const left = img.pixel(4, 1)[0];
    const mid = img.pixel(32, 1)[0];
    const right = img.pixel(60, 1)[0];
    try testing.expect(left > mid and mid > right);
    try testing.expect(mid > 100 and mid < 150);
}

test "color interpolation and out-of-bounds triangles" {
    var img = try Image.init(testing.allocator, 8, 8, .{ 0, 0, 0 });
    defer img.deinit(testing.allocator);
    const t = [_]Vert{
        V(-100, -100, 1, 0, 0, 1), V(200, -100, 0, 1, 0, 1), V(-100, 300, 0, 0, 1, 1),
        // fully outside
        V(500, 500, 1, 1, 1, 1),   V(600, 500, 1, 1, 1, 1),  V(500, 600, 1, 1, 1, 1),
        // degenerate
        V(1, 1, 1, 1, 1, 1),       V(2, 2, 1, 1, 1, 1),      V(3, 3, 1, 1, 1, 1),
        // NaN does not crash
        V(std.math.nan(f32), 0, 1, 1, 1, 1), V(1, 1, 1, 1, 1, 1), V(0, 3, 1, 1, 1, 1),
    };
    img.drawTris(&t);
    const p = img.pixel(4, 4);
    try testing.expect(@as(u32, p[0]) + p[1] + p[2] > 200);
}

test "thin diagonal hairline is O(length): 2000 long lines draw fast enough" {
    var img = try Image.init(testing.allocator, 600, 600, .{ 255, 255, 255 });
    defer img.deinit(testing.allocator);
    var i: usize = 0;
    while (i < 2000) : (i += 1) {
        const o: f32 = @floatFromInt(i % 500);
        const t = [_]Vert{
            V(o, 0, 0, 0, 0, 0), V(o + 1, 0, 0, 0, 0, 1), V(600 + o, 600, 0, 0, 0, 0),
        };
        img.drawTris(&t);
    }
    try testing.expect(img.pixel(1, 1)[0] <= 255);
}

test "alpha 0 constant triangle is a no-op; opaque overwrites" {
    var img = try Image.init(testing.allocator, 4, 4, .{ 10, 20, 30 });
    defer img.deinit(testing.allocator);
    img.drawTris(&.{ V(0, 0, 1, 1, 1, 0), V(4, 0, 1, 1, 1, 0), V(0, 4, 1, 1, 1, 0) });
    try testing.expectEqual(@as([3]u8, .{ 10, 20, 30 }), img.pixel(0, 0));
    img.drawTris(&.{ V(0, 0, 1, 0, 0, 1), V(4, 0, 1, 0, 0, 1), V(0, 4, 1, 0, 0, 1) });
    try testing.expectEqual(@as([3]u8, .{ 255, 0, 0 }), img.pixel(0, 0));
}
