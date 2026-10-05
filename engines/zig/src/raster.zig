//! PNG raster exporter (SPEC 12): renders exactly the items the SVG exporter shows (same Map, same pens,
//! dashes, round caps/joins, even-odd fills, hatch lines, stroke-font text) into an 8-bit grayscale image
//! (white paper, black ink) and encodes it with `png.zig`.
//!
//! Rasterizer: every item is drawn into a coverage scratch (u8, union by max, so overlapping segments of
//! one stroke never double-darken), then composited once onto the page (`dst *= 1 - coverage`).
//!  * strokes: each flattened segment is a capsule; coverage = clamp(r + 0.5 - distance, 0, 1) per pixel
//!    (a 1 px line is exactly one pixel wide). Row spans are computed analytically, so a long diagonal
//!    hatch line costs O(length), not O(bbox).
//!  * fills: even-odd scanline fill, 8 sub-scanlines per pixel row with exact horizontal coverage.
//! Pure std, all memory from the caller's allocator, deterministic, no I/O (builds for wasm32).

const std = @import("std");
const geom = @import("geom.zig");
const drawing = @import("drawing.zig");
const font_mod = @import("font.zig");
const textgeom = @import("textgeom.zig");
const style_mod = @import("style.zig");
const svg = @import("svg.zig");
const png = @import("png.zig");
const Allocator = std.mem.Allocator;
const Pt = geom.Pt;

pub const default_px: u32 = 1600;
pub const min_px: u32 = 200;
pub const max_px: u32 = 6000;
/// Longest side the page may reach (a tall sheet at 6000 px wide would otherwise be enormous).
pub const max_side: u32 = 8192;

pub const Options = struct {
    /// Requested image width in pixels (clamped to min_px..max_px).
    px: u32 = default_px,
    /// Paper margin around a bare (non-sheet) detail, as in the SVG exporter.
    margin_in: f64 = 0.25,
};

pub const Image = struct {
    width: u32,
    height: u32,
    /// Gray8, row 0 = top, 255 = white paper.
    pixels: []u8,
};

const V = struct { x: f64, y: f64 };

const Edge = struct { x0: f64, y0: f64, x1: f64, y1: f64 };

const sub_scans: usize = 8;

const Canvas = struct {
    w: usize,
    h: usize,
    pix: []u8,
    cov: []u8,
    rmin: []i32,
    rmax: []i32,
    acc: []f32,
    flat: std.ArrayList(V) = .empty,
    edges: std.ArrayList(Edge) = .empty,
    xs: std.ArrayList(f64) = .empty,

    fn init(a: Allocator, w: usize, h: usize) Allocator.Error!Canvas {
        const pix = try a.alloc(u8, w * h);
        @memset(pix, 255);
        const cov = try a.alloc(u8, w * h);
        @memset(cov, 0);
        const rmin = try a.alloc(i32, h);
        const rmax = try a.alloc(i32, h);
        @memset(rmin, std.math.maxInt(i32));
        @memset(rmax, -1);
        const acc = try a.alloc(f32, w + 2);
        return .{ .w = w, .h = h, .pix = pix, .cov = cov, .rmin = rmin, .rmax = rmax, .acc = acc };
    }

    inline fn mark(c: *Canvas, y: usize, x0: usize, x1: usize) void {
        if (@as(i32, @intCast(x0)) < c.rmin[y]) c.rmin[y] = @intCast(x0);
        if (@as(i32, @intCast(x1)) > c.rmax[y]) c.rmax[y] = @intCast(x1);
    }

    /// Composite the coverage scratch onto the page (black ink) and clear it.
    fn flush(c: *Canvas) void {
        for (0..c.h) |y| {
            if (c.rmax[y] < 0) continue;
            const x0: usize = @intCast(c.rmin[y]);
            const x1: usize = @intCast(c.rmax[y]);
            const row = y * c.w;
            for (x0..x1 + 1) |x| {
                const cv: u32 = c.cov[row + x];
                if (cv == 0) continue;
                c.cov[row + x] = 0;
                const d: u32 = c.pix[row + x];
                c.pix[row + x] = @intCast((d * (255 - cv) + 127) / 255);
            }
            c.rmin[y] = std.math.maxInt(i32);
            c.rmax[y] = -1;
        }
    }

    /// Union a round-capped segment of half-width `r` (pixels) into the coverage scratch.
    fn capsule(c: *Canvas, ax: f64, ay: f64, bx: f64, by: f64, r: f64) void {
        const rr = r + 0.5;
        const wf: f64 = @floatFromInt(c.w);
        const hf: f64 = @floatFromInt(c.h);
        const minx = @min(ax, bx) - rr;
        const maxx = @max(ax, bx) + rr;
        const miny = @min(ay, by) - rr;
        const maxy = @max(ay, by) + rr;
        if (!(maxx > 0 and minx < wf and maxy > 0 and miny < hf)) return; // also rejects NaN
        const ry0: usize = @intFromFloat(@max(0.0, @floor(miny - 0.5)));
        const ry1: usize = @intFromFloat(@min(hf - 1.0, @ceil(maxy - 0.5)));
        const dx = bx - ax;
        const dy = by - ay;
        const len2 = dx * dx + dy * dy;
        const len = @sqrt(len2);
        const has_dir = len2 > 1e-12;
        const inv2 = if (has_dir) 1.0 / len2 else 0.0;
        const nx = if (has_dir) -dy / len else 0.0;
        const ny = if (has_dir) dx / len else 1.0;
        var y = ry0;
        while (y <= ry1) : (y += 1) {
            const yc = @as(f64, @floatFromInt(y)) + 0.5;
            var xl = minx;
            var xr = maxx;
            if (has_dir) {
                const t = ny * (yc - ay);
                if (@abs(nx) < 1e-9) {
                    if (@abs(t) > rr) continue;
                } else {
                    const s1 = ax + (-rr - t) / nx;
                    const s2 = ax + (rr - t) / nx;
                    xl = @max(xl, @min(s1, s2));
                    xr = @min(xr, @max(s1, s2));
                }
            }
            if (xl > xr) continue;
            const fx0 = @max(0.0, @floor(xl - 0.5));
            const fx1 = @min(wf - 1.0, @ceil(xr - 0.5));
            if (fx0 > fx1) continue;
            const ix0: usize = @intFromFloat(fx0);
            const ix1: usize = @intFromFloat(fx1);
            const row = y * c.w;
            var x = ix0;
            while (x <= ix1) : (x += 1) {
                const pxc = @as(f64, @floatFromInt(x)) + 0.5;
                var t: f64 = 0;
                if (has_dir) t = std.math.clamp(((pxc - ax) * dx + (yc - ay) * dy) * inv2, 0.0, 1.0);
                const qx = ax + t * dx - pxc;
                const qy = ay + t * dy - yc;
                const cv = rr - @sqrt(qx * qx + qy * qy);
                if (cv <= 0) continue;
                const v: u8 = @intFromFloat(@round(@min(cv, 1.0) * 255.0));
                if (v > c.cov[row + x]) c.cov[row + x] = v;
            }
            c.mark(y, ix0, ix1);
        }
    }

    /// Stroke the flattened polyline in `c.flat` (pixel space), optionally dashed.
    fn strokeFlat(c: *Canvas, r: f64, dash: ?[]const f64) void {
        const p = c.flat.items;
        if (p.len == 0) return;
        if (p.len == 1) {
            c.capsule(p[0].x, p[0].y, p[0].x, p[0].y, r);
            return;
        }
        var pat_sum: f64 = 0;
        if (dash) |d| for (d) |v| {
            pat_sum += @max(v, 0);
        };
        if (dash == null or pat_sum < 1e-6) {
            for (0..p.len - 1) |i| c.capsule(p[i].x, p[i].y, p[i + 1].x, p[i + 1].y, r);
            return;
        }
        const d = dash.?;
        const n = if (d.len % 2 == 1) d.len * 2 else d.len; // SVG: an odd list repeats
        var idx: usize = 0;
        var rem: f64 = @max(d[0], 0);
        var on = true;
        for (0..p.len - 1) |i| {
            const ax = p[i].x;
            const ay = p[i].y;
            const sx = p[i + 1].x - ax;
            const sy = p[i + 1].y - ay;
            const len = @sqrt(sx * sx + sy * sy);
            var pos: f64 = 0;
            while (pos < len) {
                const take = @min(rem, len - pos);
                if (on) {
                    const t0 = pos / len;
                    const t1 = (pos + take) / len;
                    c.capsule(ax + sx * t0, ay + sy * t0, ax + sx * t1, ay + sy * t1, r);
                }
                pos += take;
                rem -= take;
                if (rem <= 1e-9) {
                    idx = (idx + 1) % n;
                    rem = @max(d[idx % d.len], 0);
                    on = !on;
                }
            }
        }
    }

    /// Flatten a bulge polyline (model units) into `out` in pixel space.
    fn flatten(a: Allocator, out: *std.ArrayList(V), m: svg.Map, k: f64, pts: []const Pt, closed: bool) Allocator.Error!void {
        if (pts.len == 0) return;
        try out.append(a, .{ .x = m.px(pts[0].x) * k, .y = m.py(pts[0].y) * k });
        const nseg = if (closed) pts.len else pts.len - 1;
        for (0..nseg) |i| {
            const p = pts[i];
            const q = pts[(i + 1) % pts.len];
            if (p.b != 0) {
                const arc = geom.arcOf(p.v(), q.v(), p.b);
                const r_px = arc.r / m.s * svg.px_per_in * k;
                // chord error <= 0.04 px
                const step = if (r_px > 0.04) 2.0 * std.math.acos(1.0 - 0.04 / r_px) else std.math.pi;
                const nf = @ceil(@abs(arc.sweep) / @max(step, 1e-3));
                const segs: usize = @intFromFloat(std.math.clamp(nf, 2.0, 4096.0));
                for (1..segs) |j| {
                    const pt = arc.at(@as(f64, @floatFromInt(j)) / @as(f64, @floatFromInt(segs)));
                    try out.append(a, .{ .x = m.px(pt.x) * k, .y = m.py(pt.y) * k });
                }
            }
            try out.append(a, .{ .x = m.px(q.x) * k, .y = m.py(q.y) * k });
        }
    }

    /// Even-odd fill of the edges in `c.edges` (pixel space) into the coverage scratch.
    fn fillEdges(c: *Canvas, a: Allocator) Allocator.Error!void {
        const es = c.edges.items;
        if (es.len == 0) return;
        var ymin: f64 = std.math.inf(f64);
        var ymax: f64 = -std.math.inf(f64);
        for (es) |e| {
            ymin = @min(ymin, @min(e.y0, e.y1));
            ymax = @max(ymax, @max(e.y0, e.y1));
        }
        const hf: f64 = @floatFromInt(c.h);
        if (!(ymax > 0 and ymin < hf)) return;
        const wf: f64 = @floatFromInt(c.w);
        const y0: usize = @intFromFloat(@max(0.0, @floor(ymin)));
        const y1: usize = @intFromFloat(@min(hf - 1.0, @floor(ymax)));
        const wgt: f32 = 1.0 / @as(f32, sub_scans);
        var y = y0;
        while (y <= y1) : (y += 1) {
            var amin: usize = c.w;
            var amax: usize = 0;
            var any = false;
            for (0..sub_scans) |s| {
                const sy = @as(f64, @floatFromInt(y)) + (@as(f64, @floatFromInt(s)) + 0.5) / @as(f64, sub_scans);
                c.xs.clearRetainingCapacity();
                for (es) |e| {
                    if ((e.y0 <= sy) != (e.y1 <= sy)) {
                        try c.xs.append(a, e.x0 + (sy - e.y0) * (e.x1 - e.x0) / (e.y1 - e.y0));
                    }
                }
                if (c.xs.items.len < 2) continue;
                std.mem.sort(f64, c.xs.items, {}, std.sort.asc(f64));
                var i: usize = 0;
                while (i + 1 < c.xs.items.len) : (i += 2) {
                    const xa = std.math.clamp(c.xs.items[i], 0.0, wf);
                    const xb = std.math.clamp(c.xs.items[i + 1], 0.0, wf);
                    if (xb <= xa) continue;
                    const ia: usize = @intFromFloat(@floor(xa));
                    const ib: usize = @intFromFloat(@floor(xb));
                    if (!any) {
                        @memset(c.acc, 0);
                        any = true;
                    }
                    amin = @min(amin, ia);
                    amax = @max(amax, ib);
                    if (ia == ib) {
                        c.acc[ia] += @as(f32, @floatCast(xb - xa)) * wgt;
                    } else {
                        c.acc[ia] += @as(f32, @floatCast(@as(f64, @floatFromInt(ia + 1)) - xa)) * wgt;
                        for (ia + 1..ib) |x| c.acc[x] += wgt;
                        c.acc[ib] += @as(f32, @floatCast(xb - @as(f64, @floatFromInt(ib)))) * wgt;
                    }
                }
            }
            if (!any) continue;
            const row = y * c.w;
            const last = @min(amax, c.w - 1);
            if (amin > last) continue;
            for (amin..last + 1) |x| {
                const v: u8 = @intFromFloat(@round(std.math.clamp(c.acc[x], 0.0, 1.0) * 255.0));
                if (v > c.cov[row + x]) c.cov[row + x] = v;
            }
            c.mark(y, amin, last);
        }
    }
};

fn penWidthPx(st: *const style_mod.Style, pen: []const u8, ppi: f64) f64 {
    const w_mm = if (st.pen(pen)) |p| p.width_mm else 0.25;
    return @max(w_mm / 25.4 * ppi, 1.0);
}

fn penDashPx(a: Allocator, st: *const style_mod.Style, pen: []const u8, ppi: f64) Allocator.Error!?[]const f64 {
    const p = st.pen(pen) orelse return null;
    const dm = p.dash_mm orelse return null;
    const out = try a.alloc(f64, dm.len);
    for (dm, 0..) |v, i| out[i] = v / 25.4 * ppi;
    return out;
}

fn layerKnown(d: *const drawing.Drawing, name: []const u8) bool {
    for (d.layers) |l| if (std.mem.eql(u8, l.name, name)) return true;
    return false;
}

/// Page size in pixels and pixels per paper inch for the drawing at the requested width.
pub const Layout = struct { w: u32, h: u32, ppi: f64, margin_in: f64 };

pub fn layout(d: *const drawing.Drawing, opt: Options) Layout {
    const s = d.scale;
    const margin = if (d.page_w > 0) 0 else opt.margin_in;
    const w_in = (d.bounds[2] - d.bounds[0]) / s + 2 * margin;
    const h_in = (d.bounds[3] - d.bounds[1]) / s + 2 * margin;
    const want: f64 = @floatFromInt(std.math.clamp(opt.px, min_px, max_px));
    var ppi = want / @max(w_in, 1e-6);
    ppi = @min(ppi, @as(f64, @floatFromInt(max_side)) / @max(h_in, 1e-6));
    const w: u32 = @intFromFloat(std.math.clamp(@ceil(w_in * ppi - 1e-9), 1.0, @as(f64, @floatFromInt(max_side))));
    const h: u32 = @intFromFloat(std.math.clamp(@ceil(h_in * ppi - 1e-9), 1.0, @as(f64, @floatFromInt(max_side))));
    return .{ .w = w, .h = h, .ppi = ppi, .margin_in = margin };
}

/// Rasterize to gray8. All allocations (including the returned pixels) come from `a`.
pub fn rasterize(a: Allocator, d: *const drawing.Drawing, font: *const font_mod.Font, opt: Options) Allocator.Error!Image {
    const lay = layout(d, opt);
    var c = try Canvas.init(a, lay.w, lay.h);
    const m = svg.Map{ .s = d.scale, .x0 = d.bounds[0], .y1 = d.bounds[3], .ox = lay.margin_in, .oy = lay.margin_in };
    const k = lay.ppi / svg.px_per_in;
    const st = d.style;
    for (d.items) |it| {
        if (it == .region) continue;
        if (!layerKnown(d, it.layer())) continue; // the SVG exporter only emits declared layers
        switch (it) {
            .region => {},
            .path => |p| {
                c.flat.clearRetainingCapacity();
                try Canvas.flatten(a, &c.flat, m, k, p.pts, p.closed);
                c.strokeFlat(penWidthPx(st, p.pen, lay.ppi) * 0.5, try penDashPx(a, st, p.pen, lay.ppi));
            },
            .fill => |f| {
                c.edges.clearRetainingCapacity();
                for (f.loops) |l| {
                    c.flat.clearRetainingCapacity();
                    try Canvas.flatten(a, &c.flat, m, k, l, true);
                    const q = c.flat.items;
                    if (q.len < 2) continue;
                    for (0..q.len) |i| {
                        const n = q[(i + 1) % q.len];
                        try c.edges.append(a, .{ .x0 = q[i].x, .y0 = q[i].y, .x1 = n.x, .y1 = n.y });
                    }
                }
                try c.fillEdges(a);
            },
            .hatch => |h| {
                const r = penWidthPx(st, h.pen, lay.ppi) * 0.5;
                const dash = try penDashPx(a, st, h.pen, lay.ppi);
                for (h.lines) |ln| {
                    c.flat.clearRetainingCapacity();
                    try c.flat.append(a, .{ .x = m.px(ln[0]) * k, .y = m.py(ln[1]) * k });
                    if (ln[0] != ln[2] or ln[1] != ln[3]) try c.flat.append(a, .{ .x = m.px(ln[2]) * k, .y = m.py(ln[3]) * k });
                    c.strokeFlat(r, dash);
                }
            },
            .text => |t| {
                const sts = try textgeom.strokes(a, font, t);
                const r = penWidthPx(st, t.pen, lay.ppi) * 0.5;
                const dash = try penDashPx(a, st, t.pen, lay.ppi);
                for (sts) |pl| {
                    if (pl.len < 2) continue;
                    c.flat.clearRetainingCapacity();
                    for (pl) |p| try c.flat.append(a, .{ .x = m.px(p.x) * k, .y = m.py(p.y) * k });
                    c.strokeFlat(r, dash);
                }
            },
        }
        c.flush();
    }
    return .{ .width = lay.w, .height = lay.h, .pixels = c.pix };
}

/// PNG bytes (gray8) of the drawing. Owned by `a`.
pub fn render(a: Allocator, d: *const drawing.Drawing, font: *const font_mod.Font, opt: Options) ![]u8 {
    const img = try rasterize(a, d, font, opt);
    return png.encode(a, img.width, img.height, .gray, img.pixels);
}

// ---- tests ---------------------------------------------------------------------------------------------

test "capsule coverage: 1 px horizontal line is one pixel tall" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    var c = try Canvas.init(arena.allocator(), 20, 10);
    c.capsule(2, 5.5, 17, 5.5, 0.5);
    c.flush();
    try std.testing.expectEqual(@as(u8, 0), c.pix[5 * 20 + 10]);
    try std.testing.expectEqual(@as(u8, 255), c.pix[4 * 20 + 10]);
    try std.testing.expectEqual(@as(u8, 255), c.pix[6 * 20 + 10]);
}

test "fill: even-odd square with hole, exact edge coverage" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    var c = try Canvas.init(a, 20, 20);
    const loops = [_][4]f64{
        .{ 2, 2, 18, 2 },   .{ 18, 2, 18, 18 }, .{ 18, 18, 2, 18 }, .{ 2, 18, 2, 2 },
        .{ 8, 8, 12, 8 },   .{ 12, 8, 12, 12 }, .{ 12, 12, 8, 12 }, .{ 8, 12, 8, 8 },
    };
    for (loops) |e| try c.edges.append(a, .{ .x0 = e[0], .y0 = e[1], .x1 = e[2], .y1 = e[3] });
    try c.fillEdges(a);
    c.flush();
    try std.testing.expectEqual(@as(u8, 0), c.pix[4 * 20 + 4]);
    try std.testing.expectEqual(@as(u8, 255), c.pix[10 * 20 + 10]); // hole
    try std.testing.expectEqual(@as(u8, 255), c.pix[0]);
}
