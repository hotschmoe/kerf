//! Hatch pattern line generation (AutoCAD .pat semantics) clipped to a region (even-odd).

const std = @import("std");
const geom = @import("geom.zig");
const cast = @import("num.zig");
const style_mod = @import("style.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;

pub const Line = [4]f64;

pub const max_lines: usize = 150_000;

pub const Result = struct {
    lines: []const Line,
    truncated: bool,
};

/// Generate the lines of one pattern for the region `loops` (flattened, even-odd).
/// `k` = model inches per paper inch times the spec scale; `rot` = extra rotation (radians) of the
/// whole pattern about the model origin.
pub fn generate(
    a: Allocator,
    loops: []const []const V2,
    pattern: *const style_mod.Pattern,
    k: f64,
    rot_deg: f64,
) Allocator.Error!Result {
    var lines: std.ArrayList(Line) = .empty;
    var truncated = false;
    const rot = std.math.degreesToRadians(rot_deg);
    const cr = @cos(rot);
    const sr = @sin(rot);
    for (pattern.families) |fam| {
        const th = std.math.degreesToRadians(fam.angle) + rot;
        const dir = V2.init(@cos(th), @sin(th));
        const nrm = V2.init(-dir.y, dir.x);
        // family origin, rotated by the hatch angle about the model origin
        const ox = (fam.x0 * cr - fam.y0 * sr) * k;
        const oy = (fam.x0 * sr + fam.y0 * cr) * k;
        const o = V2.init(ox, oy);
        const dy = fam.dy * k;
        const dx = fam.dx * k;
        if (@abs(dy) < 1e-9) continue;
        // perpendicular range of the region
        var smin = std.math.inf(f64);
        var smax = -std.math.inf(f64);
        for (loops) |l| for (l) |p| {
            const s = p.dot(nrm);
            smin = @min(smin, s);
            smax = @max(smax, s);
        };
        if (smin > smax) continue;
        const s0 = o.dot(nrm);
        // line i sits at s0 + i*dy
        const qa = (smin - s0) / dy;
        const qb = (smax - s0) / dy;
        const lo_f = @ceil(@min(qa, qb) - 1e-9);
        const hi_f = @floor(@max(qa, qb) + 1e-9);
        if (lo_f > hi_f) continue;
        // compare in f64 first: the line count can exceed any integer type (tiny dy), and NaN must not reach a cast
        if (!(hi_f - lo_f + 1 <= @as(f64, @floatFromInt(max_lines)))) {
            truncated = true;
            continue;
        }
        const i_lo = cast.toInt(i64, lo_f) orelse {
            truncated = true;
            continue;
        };
        const i_hi = cast.toInt(i64, hi_f) orelse {
            truncated = true;
            continue;
        };
        // dash pattern
        var period: f64 = 0;
        for (fam.dashes) |d| period += @abs(d) * k;
        var i = i_lo;
        var ts: std.ArrayList(f64) = .empty; // reused for every line of the family
        while (i <= i_hi) : (i += 1) {
            const fi: f64 = @floatFromInt(i);
            const s = s0 + fi * dy;
            const t_base = o.dot(dir) + fi * dx;
            // crossings
            ts.clearRetainingCapacity();
            for (loops) |l| {
                for (l, 0..) |p, vi| {
                    const q = l[(vi + 1) % l.len];
                    const sa = p.dot(nrm) - s;
                    const sb = q.dot(nrm) - s;
                    if ((sa < 0) != (sb < 0)) {
                        const u = sa / (sa - sb);
                        const ta = p.dot(dir);
                        const tb = q.dot(dir);
                        try ts.append(a, ta + (tb - ta) * u);
                    }
                }
            }
            if (ts.items.len < 2) continue;
            std.mem.sort(f64, ts.items, {}, std.sort.asc(f64));
            var q: usize = 0;
            while (q + 1 < ts.items.len) : (q += 2) {
                const ta = ts.items[q];
                const tb = ts.items[q + 1];
                if (tb - ta < 1e-9 and fam.dashes.len == 0) continue;
                if (lines.items.len >= max_lines) {
                    truncated = true;
                    break;
                }
                if (fam.dashes.len == 0 or period < 1e-12) {
                    try lines.append(a, lineAt(dir, nrm, s, ta, tb));
                } else {
                    try dashSegments(a, &lines, fam.dashes, k, period, t_base, ta, tb, dir, nrm, s);
                }
            }
        }
    }
    return .{ .lines = lines.items, .truncated = truncated };
}

/// Grain look (paper inches): wave amplitude and wavelength, edge inset.
pub const Grain = struct {
    amp: f64 = 0.010,
    wavelength: f64 = 1.1,
    inset: f64 = 0.03,
};

fn mix(x: u64) u64 {
    var z = x +% 0x9E3779B97F4A7C15;
    z = (z ^ (z >> 30)) *% 0xBF58476D1CE4E5B9;
    z = (z ^ (z >> 27)) *% 0x94D049BB133111EB;
    return z ^ (z >> 31);
}

/// 0..1 from the quantised perpendicular position of a line, so every dash of one line shares its wave.
fn unitHash(v_paper: f64, salt: u64) f64 {
    const q: i64 = cast.toInt(i64, @round(v_paper * 1000.0)) orelse 0;
    const h = mix(@as(u64, @bitCast(q)) ^ (salt *% 0x2545F4914F6CDD1D));
    return @as(f64, @floatFromInt(h >> 11)) / 9007199254740992.0;
}

/// Wood grain: the straight dashes of `pattern` (angle along the member, in the pattern's own angle plus `rot_deg`)
/// turned into gentle wavy lines. Each line has its own phase, amplitude and wavelength (from its position, so the
/// result is deterministic and a dash continues the same wave), tapers toward the long edges, and keeps `inset`
/// paper inches away from every boundary of the region (probe points must lie inside it).
pub fn generateGrain(a: Allocator, loops: []const []const V2, pattern: *const style_mod.Pattern, k: f64, rot_deg: f64, grain: Grain) Allocator.Error!Result {
    const base = try generate(a, loops, pattern, k, rot_deg);
    var out: std.ArrayList(Line) = .empty;
    if (pattern.families.len == 0) return base;
    const th = std.math.degreesToRadians(pattern.families[0].angle + rot_deg);
    const dir = V2.init(@cos(th), @sin(th));
    const nrm = V2.init(-dir.y, dir.x);
    var vmin = std.math.inf(f64);
    var vmax = -std.math.inf(f64);
    for (loops) |l| for (l) |p| {
        const v = p.dot(nrm);
        vmin = @min(vmin, v);
        vmax = @max(vmax, v);
    };
    const tau = 2.0 * std.math.pi;
    const inset = grain.inset * k;
    const step = 0.05 * k;
    var truncated = base.truncated;
    for (base.lines) |ln| {
        const p0 = V2.init(ln[0], ln[1]);
        const p1 = V2.init(ln[2], ln[3]);
        const t0 = p0.dot(dir);
        const t1 = p1.dot(dir);
        const v = p0.dot(nrm);
        if (v - vmin < inset or vmax - v < inset) continue; // too close to a long edge
        const vp = v / k;
        const phi = tau * unitHash(vp, 1);
        const phi2 = tau * unitHash(vp, 2);
        const amp = grain.amp * k * (0.55 + 0.45 * unitHash(vp, 3));
        const wl = grain.wavelength * k * (0.75 + 0.5 * unitHash(vp, 4));
        const room = @min(v - vmin, vmax - v) - inset;
        const taper = std.math.clamp(room / (2.0 * amp), 0, 1);
        const n: usize = cast.toIntClamped(usize, @ceil(@abs(t1 - t0) / step), 1, 100_000);
        var prev: ?V2 = null;
        var i: usize = 0;
        while (i <= n) : (i += 1) {
            const f: f64 = @as(f64, @floatFromInt(i)) / @as(f64, @floatFromInt(n));
            const t = t0 + (t1 - t0) * f;
            const d = taper * amp * (@sin(tau * t / wl + phi) + 0.35 * @sin(tau * t / (0.37 * wl) + phi2)) / 1.35;
            const pt = dir.scale(t).add(nrm.scale(v + d));
            const ok = geom.locateEvenOdd(pt, loops, 1e-9) == .inside and
                geom.locateEvenOdd(pt.add(dir.scale(inset)), loops, 1e-9) == .inside and
                geom.locateEvenOdd(pt.sub(dir.scale(inset)), loops, 1e-9) == .inside;
            if (ok) {
                if (prev) |q| {
                    if (out.items.len >= max_lines) {
                        truncated = true;
                        break;
                    }
                    try out.append(a, .{ q.x, q.y, pt.x, pt.y });
                }
                prev = pt;
            } else prev = null;
        }
    }
    return .{ .lines = out.items, .truncated = truncated };
}

fn lineAt(dir: V2, nrm: V2, s: f64, ta: f64, tb: f64) Line {
    return .{
        dir.x * ta + nrm.x * s,
        dir.y * ta + nrm.y * s,
        dir.x * tb + nrm.x * s,
        dir.y * tb + nrm.y * s,
    };
}

fn dashSegments(a: Allocator, lines: *std.ArrayList(Line), dashes: []const f64, k: f64, period: f64, t_base: f64, ta: f64, tb: f64, dir: V2, nrm: V2, s: f64) Allocator.Error!void {
    var m: f64 = @floor((ta - t_base) / period);
    const m_end = @floor((tb - t_base) / period);
    var guard: usize = 0;
    while (m <= m_end and guard < 100000) : (m += 1) {
        guard += 1;
        var t = t_base + m * period;
        for (dashes) |d| {
            const len = @abs(d) * k;
            if (d >= 0) {
                // a dash (or a dot when zero)
                const a0 = t;
                const a1 = t + len;
                if (len == 0) {
                    if (a0 >= ta and a0 <= tb) try lines.append(a, lineAt(dir, nrm, s, a0, a0));
                } else {
                    const c0 = @max(a0, ta);
                    const c1 = @min(a1, tb);
                    if (c1 - c0 > 1e-9) try lines.append(a, lineAt(dir, nrm, s, c0, c1));
                }
            }
            t += len;
            if (lines.items.len >= max_lines) return;
        }
    }
}

test "grain stays inside, is wavy and deterministic" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const r = [_]V2{ V2.init(0, 0), V2.init(96, 0), V2.init(96, 3.5), V2.init(0, 3.5) };
    const fam = [_]style_mod.Family{.{ .angle = 0, .x0 = 0, .y0 = 0, .dx = 3, .dy = 0.6, .dashes = &.{ 20, -6 } }};
    const pat = style_mod.Pattern{ .name = "G", .families = &fam };
    const g1 = try generateGrain(a, &.{&r}, &pat, 1.0, 0, .{});
    const g2 = try generateGrain(a, &.{&r}, &pat, 1.0, 0, .{});
    try std.testing.expect(g1.lines.len > 20);
    try std.testing.expectEqual(g1.lines.len, g2.lines.len);
    var wavy = false;
    for (g1.lines, 0..) |ln, i| {
        try std.testing.expect(ln[1] > 0 and ln[1] < 3.5 and ln[3] > 0 and ln[3] < 3.5);
        try std.testing.expect(ln[0] > 0 and ln[2] < 96);
        try std.testing.expectEqual(ln, g2.lines[i]);
        if (@abs(ln[1] - ln[3]) > 1e-6) wavy = true;
    }
    try std.testing.expect(wavy);
}

test "continuous 45 degree hatch fills a square" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const sq = [_]V2{ V2.init(0, 0), V2.init(10, 0), V2.init(10, 10), V2.init(0, 10) };
    const fam = [_]style_mod.Family{.{ .angle = 45, .x0 = 0, .y0 = 0, .dx = 0, .dy = 1, .dashes = &.{} }};
    const pat = style_mod.Pattern{ .name = "T", .families = &fam };
    const r = try generate(a, &.{&sq}, &pat, 1.0, 0);
    // 45-degree lines spaced 1 apart across a 10x10 square: about 14 lines (diagonal span 14.14)
    try std.testing.expect(r.lines.len >= 13 and r.lines.len <= 15);
    for (r.lines) |ln| {
        const len = std.math.hypot(ln[2] - ln[0], ln[3] - ln[1]);
        try std.testing.expect(len <= 14.2);
        // all endpoints on or inside the square
        try std.testing.expect(ln[0] >= -1e-9 and ln[0] <= 10 + 1e-9 and ln[1] >= -1e-9 and ln[1] <= 10 + 1e-9);
    }
}

test "hole is respected (even-odd)" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const outer = [_]V2{ V2.init(0, 0), V2.init(10, 0), V2.init(10, 10), V2.init(0, 10) };
    const hole = [_]V2{ V2.init(4, 4), V2.init(4, 6), V2.init(6, 6), V2.init(6, 4) };
    const fam = [_]style_mod.Family{.{ .angle = 0, .x0 = 0, .y0 = 0, .dx = 0, .dy = 1, .dashes = &.{} }};
    const pat = style_mod.Pattern{ .name = "T", .families = &fam };
    const r = try generate(a, &.{ &outer, &hole }, &pat, 1.0, 0);
    // horizontal lines at y = 4.. 6 are split into two segments
    var split: usize = 0;
    for (r.lines) |ln| {
        if (@abs(ln[1] - 5) < 1e-9) split += 1;
    }
    try std.testing.expectEqual(@as(usize, 2), split);
}

test "dashes and dots" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const sq = [_]V2{ V2.init(0, 0), V2.init(10, 0), V2.init(10, 1), V2.init(0, 1) };
    const fam = [_]style_mod.Family{.{ .angle = 0, .x0 = 0, .y0 = 0.5, .dx = 0, .dy = 5, .dashes = &.{ 1, -1 } }};
    const pat = style_mod.Pattern{ .name = "T", .families = &fam };
    const r = try generate(a, &.{&sq}, &pat, 1.0, 0);
    try std.testing.expectEqual(@as(usize, 5), r.lines.len);
    const dots = [_]style_mod.Family{.{ .angle = 0, .x0 = 0, .y0 = 0.5, .dx = 0, .dy = 5, .dashes = &.{ 0, -2 } }};
    const pat2 = style_mod.Pattern{ .name = "T", .families = &dots };
    const r2 = try generate(a, &.{&sq}, &pat2, 1.0, 0);
    try std.testing.expectEqual(@as(usize, 6), r2.lines.len); // dots at 0,2,..,10
    try std.testing.expectEqual(r2.lines[0][0], r2.lines[0][2]);
}
