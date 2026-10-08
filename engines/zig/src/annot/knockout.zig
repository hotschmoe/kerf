//! Hatch knockout (SPEC 18): hatch lines are cut where a note, dimension text or label sits on them.

const std = @import("std");
const sort = @import("../sort.zig");
const drawing = @import("../drawing.zig");
const geom = @import("../geom.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Item = drawing.Item;
const annot = @import("../annot.zig");
const Env = annot.Env;

pub fn convexInterval(a: V2, b: V2, poly: *const [4]V2) ?[2]f64 {
    const area = geom.signedAreaV(poly);
    const sgn: f64 = if (area >= 0) 1 else -1;
    var t0: f64 = 0;
    var t1: f64 = 1;
    const d = b.sub(a);
    for (poly, 0..) |p, i| {
        const q = poly[(i + 1) % 4];
        const e = q.sub(p);
        const num = e.cross(a.sub(p)) * sgn;
        const den = e.cross(d) * sgn;
        if (@abs(den) < 1e-15) {
            if (num < 0) return null;
        } else {
            const t = -num / den;
            if (den > 0) t0 = @max(t0, t) else t1 = @min(t1, t);
        }
        if (t0 > t1) return null;
    }
    return .{ t0, t1 };
}

pub fn knockHatch(a: Allocator, items: []Item, boxes: []const [4]V2) Allocator.Error!void {
    if (boxes.len == 0) return;
    for (items) |*it| {
        if (it.* != .hatch) continue;
        var out: std.ArrayList([4]f64) = .empty;
        for (it.hatch.lines) |l| {
            const p = V2.init(l[0], l[1]);
            const q = V2.init(l[2], l[3]);
            var cuts: std.ArrayList([2]f64) = .empty;
            for (boxes) |*bx| if (convexInterval(p, q, bx)) |iv| try cuts.append(a, iv);
            if (cuts.items.len == 0) {
                try out.append(a, l);
                continue;
            }
            const degenerate = V2.eql(p, q, 1e-12);
            if (degenerate) continue;
            sort.stable([2]f64, cuts.items, {}, struct {
                fn lt(_: void, x: [2]f64, y: [2]f64) bool {
                    return x[0] < y[0];
                }
            }.lt);
            var t: f64 = 0;
            for (cuts.items) |c| {
                if (c[0] > t + 1e-9) {
                    const s0 = V2.lerp(p, q, t);
                    const s1 = V2.lerp(p, q, c[0]);
                    try out.append(a, .{ s0.x, s0.y, s1.x, s1.y });
                }
                t = @max(t, c[1]);
            }
            if (t < 1.0 - 1e-9) {
                const s0 = V2.lerp(p, q, t);
                try out.append(a, .{ s0.x, s0.y, q.x, q.y });
            }
        }
        it.hatch.lines = out.items;
    }
}
