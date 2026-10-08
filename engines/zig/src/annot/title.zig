//! Title bubble, title text and scale line under a view (SPEC 6.4).

const std = @import("std");
const drawing = @import("../drawing.zig");
const geom = @import("../geom.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Pt = geom.Pt;
const Box = geom.Box;
const Item = drawing.Item;
const layerName = @import("text.zig").layerName;
const textItem = @import("text.zig").textItem;
const pathItem = @import("text.zig").pathItem;
const upperIf = @import("text.zig").upperIf;
const annot = @import("../annot.zig");
const Env = annot.Env;
const asciiFold = annot.asciiFold;

pub const SheetInfo = struct {
    number: []const u8,
    title: []const u8,
    scale_text: []const u8,
    sheet: []const u8,
    unverified: bool,
};

/// Title under the view: bubble, title text with heavy underline, scale (SPEC 6.5). `low` receives the lowest y used.
pub fn titleItems(env: *Env, info: SheetInfo, tcrop: Box, out: *std.ArrayList(Item)) Allocator.Error!f64 {
    const a = env.a;
    const st = env.style;
    const S = env.S;
    const r = 0.3125 * S;
    const top_gap = 0.4 * S;
    const cx = tcrop.x0 + r;
    const cy = tcrop.y0 - top_gap - r;
    const src = try a.print("title:{s}", .{env.spec.id});
    const bubble = try a.dupe(Pt, &.{ .{ .x = cx - r, .y = cy, .b = 1 }, .{ .x = cx + r, .y = cy, .b = 1 } });
    try out.append(a, .{ .path = .{ .layer = layerName(env, .title), .pen = .title, .src = src, .closed = true, .pts = bubble } });
    const th = st.title_height_in * S;
    const nh = st.text_height_in * S;
    if (info.sheet.len == 0) {
        try out.append(a, try textItem(env, .title, .title, src, info.number, cx, cy, th, 0, .center, .middle));
    } else {
        try out.append(a, try pathItem(env, .anno, src, &.{ V2.init(cx - r, cy), V2.init(cx + r, cy) }, false));
        try out.append(a, try textItem(env, .title, .title, src, info.number, cx, cy + r * 0.5, th, 0, .center, .middle));
        try out.append(a, try textItem(env, .title, .anno, src, info.sheet, cx, cy - r * 0.5, st.label_height_in * 0.9 * S, 0, .center, .middle));
    }
    const tx = cx + r + 0.15 * S;
    const title = try upperIf(env, info.title);
    const ty = cy + 0.02 * S;
    try out.append(a, try textItem(env, .title, .title, src, title, tx, ty, th, 0, .left, .baseline));
    const tw = env.font.width(try asciiFold(a, title), th);
    const uy = ty - 0.07 * S;
    try out.append(a, try pathItem(env, .title, src, &.{ V2.init(tx, uy), V2.init(tx + tw, uy) }, false));
    const scale_line = try a.print("SCALE: {s}", .{info.scale_text});
    const sy = uy - 0.06 * S - nh;
    try out.append(a, try textItem(env, .title, .anno, src, scale_line, tx, sy, nh, 0, .left, .baseline));
    var low = @min(cy - r, sy - 0.02 * S);
    if (info.unverified) {
        const fy = low - 0.1 * S - nh;
        try out.append(a, try textItem(env, .title, .anno, "footnote", st.cite_footnote, tcrop.x0, fy, nh * 0.85, 0, .left, .baseline));
        low = fy - 0.02 * S;
    }
    return low;
}
