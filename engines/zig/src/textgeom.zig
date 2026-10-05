//! Stroke-font text to model-space polylines (shared by SVG, DXF-less fallbacks and PDF).

const std = @import("std");
const geom = @import("geom.zig");
const font_mod = @import("font.zig");
const drawing = @import("drawing.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;

/// Polylines for a text item (anchor at item.x, item.y; rotation about the anchor, degrees CCW).
pub fn strokes(a: Allocator, font: *const font_mod.Font, t: drawing.TextItem) Allocator.Error![][]V2 {
    var list: std.ArrayList([]V2) = .empty;
    try font.strokesOf(a, &list, t.s, t.h);
    const w = font.width(t.s, t.h);
    const dx: f64 = switch (t.align_) {
        .left => 0,
        .center => -w / 2,
        .right => -w,
    };
    const dy: f64 = switch (t.valign) {
        .baseline => 0,
        .middle => -t.h / 2,
        .top => -t.h,
    };
    const r = std.math.degreesToRadians(t.rot);
    const c = @cos(r);
    const s = @sin(r);
    for (list.items) |pl| {
        for (pl) |*p| {
            const x = p.x + dx;
            const y = p.y + dy;
            p.* = V2.init(t.x + x * c - y * s, t.y + x * s + y * c);
        }
    }
    return list.items;
}
