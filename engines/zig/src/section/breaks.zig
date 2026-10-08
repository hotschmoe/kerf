//! Break lines on the crop edge where a cut solid is clipped (SPEC 8.1): the zig-zag symbol and which cut regions get one.

const std = @import("std");
const sort = @import("../sort.zig");
const geom = @import("../geom.zig");
const clip = @import("../clip.zig");
const pathclip = @import("../pathclip.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Pt = geom.Pt;
const section = @import("../section.zig");
const Section = section.Section;

const Edge = struct { vertical: bool, c: f64, lo: f64, hi: f64, out_sign: f64 };

pub fn breakLines(self: *Section) Allocator.Error!void {
    const crop = self.spec.crop;
    const edges = [4]Edge{
        .{ .vertical = true, .c = crop.x0, .lo = crop.y0, .hi = crop.y1, .out_sign = -1 },
        .{ .vertical = true, .c = crop.x1, .lo = crop.y0, .hi = crop.y1, .out_sign = 1 },
        .{ .vertical = false, .c = crop.y0, .lo = crop.x0, .hi = crop.x1, .out_sign = -1 },
        .{ .vertical = false, .c = crop.y1, .lo = crop.x0, .hi = crop.x1, .out_sign = 1 },
    };
    for (edges) |e| {
        const Iv = struct { a: f64, b: f64, comp: u32 };
        var ivs: std.ArrayList(Iv) = .empty;
        for (self.prisms, 0..) |p, i| {
            if (self.cls[i] != .cut or p.kind != .body or p.embedded) continue;
            if (p.role == .soil or (if (self.style.material(p.material)) |m| m.fill else false)) continue;
            const f = try self.flatOf(i);
            const bx = clip.loopsBox(f);
            if (e.vertical) {
                if (bx.x0 > e.c + 1e-9 or bx.x1 < e.c - 1e-9) continue;
            } else {
                if (bx.y0 > e.c + 1e-9 or bx.y1 < e.c - 1e-9) continue;
            }
            const xs = try pathclip.scan(self.a, f, e.c, e.vertical);
            var k: usize = 0;
            while (k + 1 < xs.len) : (k += 2) {
                const lo = @max(xs[k], e.lo);
                const hi = @min(xs[k + 1], e.hi);
                if (hi - lo < 1e-6) continue;
                // is the region really clipped here? probe just outside the edge
                const mid = (lo + hi) / 2;
                const probe = if (e.vertical) V2.init(e.c + e.out_sign * 1e-6, mid) else V2.init(mid, e.c + e.out_sign * 1e-6);
                if (geom.locateEvenOdd(probe, f, 0) != .inside) continue;
                try ivs.append(self.a, .{ .a = lo, .b = hi, .comp = p.comp });
            }
        }
        if (ivs.items.len == 0) continue;
        sort.stable(Iv, ivs.items, {}, struct {
            fn lt(_: void, x: Iv, y: Iv) bool {
                if (x.a != y.a) return x.a < y.a;
                return x.comp < y.comp;
            }
        }.lt);
        // merge touching intervals
        var merged: std.ArrayList(Iv) = .empty;
        for (ivs.items) |iv| {
            if (merged.items.len > 0 and iv.a <= merged.items[merged.items.len - 1].b + 1e-3) {
                const last = &merged.items[merged.items.len - 1];
                last.b = @max(last.b, iv.b);
                last.comp = @min(last.comp, iv.comp);
            } else try merged.append(self.a, iv);
        }
        for (merged.items) |m| {
            const pts = try self.breakPolyline(e, m.a, m.b);
            try self.breaks.append(self.a, .{ .layer = self.layerFor(.@"break"), .pen = .@"break", .src = "crop", .closed = false, .pts = pts });
        }
    }
}

pub fn breakPolyline(self: *Section, e: Edge, a0: f64, b0: f64) Allocator.Error![]const Pt {
    const S = self.spec.scale;
    const over = self.style.break_overshoot_in * S;
    const a_ = a0 - over;
    const b_ = b0 + over;
    const len = b_ - a_;
    const mid = (a_ + b_) / 2;
    const half = @min(self.style.break_period_in * S / 2, len * 0.35);
    const zig = @min(self.style.break_zig_in * S, len * 0.2);
    // along-edge parameter u, perpendicular offset v
    const uv = [_][2]f64{
        .{ a_, 0 },
        .{ mid - half, 0 },
        .{ mid - half * 0.4, zig },
        .{ mid + half * 0.4, -zig },
        .{ mid + half, 0 },
        .{ b_, 0 },
    };
    const out = try self.a.alloc(Pt, uv.len);
    for (uv, 0..) |q, k| {
        out[k] = if (e.vertical) .{ .x = e.c + q[1], .y = q[0] } else .{ .x = q[0], .y = e.c + q[1] };
    }
    return out;
}
