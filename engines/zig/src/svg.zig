//! SVG exporter (SPEC 12): paper inches x 96 as user units, black ink, stroke-font text.

const std = @import("std");
const json = @import("json.zig");
const geom = @import("geom.zig");
const drawing = @import("drawing.zig");
const font_mod = @import("font.zig");
const textgeom = @import("textgeom.zig");
const style_mod = @import("style.zig");
const Pen = @import("pen.zig").Pen;
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Pt = geom.Pt;

pub const px_per_in: f64 = 96.0;

pub const Options = struct {
    /// Paper transform: model (x,y) -> paper inches (x_p, y_p with y down).
    margin_in: f64 = 0.25,
};

/// Model -> paper mapping for a drawing placed with its bounds' lower-left at paper (ox, oy_bottom).
pub const Map = struct {
    s: f64, // model inches per paper inch
    x0: f64, // model x at paper x = ox
    y1: f64, // model y at paper y = oy (top)
    ox: f64,
    oy: f64,

    pub fn px(m: Map, x: f64) f64 {
        return (m.ox + (x - m.x0) / m.s) * px_per_in;
    }
    pub fn py(m: Map, y: f64) f64 {
        return (m.oy + (m.y1 - y) / m.s) * px_per_in;
    }
};

fn num(out: *std.ArrayList(u8), a: Allocator, x: f64) Allocator.Error!void {
    // SVG coordinates: 3 decimals of a px.
    var b: [40]u8 = undefined;
    try out.appendSlice(a, json.fmtFixed(&b, x, 3));
}

fn pathData(out: *std.ArrayList(u8), a: Allocator, m: Map, pts: []const Pt, closed: bool, start_move: bool) Allocator.Error!void {
    if (pts.len == 0) return;
    if (start_move) {
        try out.appendSlice(a, "M");
        try num(out, a, m.px(pts[0].x));
        try out.append(a, ' ');
        try num(out, a, m.py(pts[0].y));
    }
    const nseg = if (closed) pts.len else pts.len - 1;
    for (0..nseg) |i| {
        const p = pts[i];
        const q = pts[(i + 1) % pts.len];
        if (p.b == 0) {
            try out.appendSlice(a, "L");
        } else {
            const arc = geom.arcOf(p.v(), q.v(), p.b);
            const r_px = arc.r / m.s * px_per_in;
            try out.appendSlice(a, "A");
            try num(out, a, r_px);
            try out.append(a, ' ');
            try num(out, a, r_px);
            try out.appendSlice(a, if (@abs(arc.sweep) > std.math.pi) " 0 1 " else " 0 0 ");
            try out.appendSlice(a, if (p.b > 0) "1 " else "0 ");
        }
        try num(out, a, m.px(q.x));
        try out.append(a, ' ');
        try num(out, a, m.py(q.y));
    }
    if (closed) try out.append(a, 'Z');
}

fn strokeAttrs(out: *std.ArrayList(u8), a: Allocator, st: *const style_mod.Style, pen: Pen) Allocator.Error!void {
    const p = st.pen(pen);
    const w_mm = if (p) |x| x.width_mm else 0.25;
    try out.appendSlice(a, " fill=\"none\" stroke=\"#000\" stroke-linecap=\"round\" stroke-linejoin=\"round\" stroke-width=\"");
    try num(out, a, w_mm / 25.4 * px_per_in);
    try out.append(a, '"');
    if (p) |x| if (x.dash_mm) |d| {
        try out.appendSlice(a, " stroke-dasharray=\"");
        for (d, 0..) |v, i| {
            if (i > 0) try out.append(a, ' ');
            try num(out, a, v / 25.4 * px_per_in);
        }
        try out.append(a, '"');
    };
}

fn escapeAttr(out: *std.ArrayList(u8), a: Allocator, s: []const u8) Allocator.Error!void {
    for (s) |c| switch (c) {
        '&' => try out.appendSlice(a, "&amp;"),
        '<' => try out.appendSlice(a, "&lt;"),
        '>' => try out.appendSlice(a, "&gt;"),
        '"' => try out.appendSlice(a, "&quot;"),
        else => try out.append(a, c),
    };
}

/// Emit the drawing's layers. The caller has opened the <svg>.
pub fn emitItems(out: *std.ArrayList(u8), a: Allocator, d: *const drawing.Drawing, m: Map, font: *const font_mod.Font) Allocator.Error!void {
    const st = d.style;
    for (d.layers) |layer| {
        try out.appendSlice(a, "<g id=\"");
        try escapeAttr(out, a, layer.name);
        try out.appendSlice(a, "\" inkscape:groupmode=\"layer\" inkscape:label=\"");
        try escapeAttr(out, a, layer.name);
        try out.appendSlice(a, "\">\n");
        // group items by src in first-appearance order
        var done = try a.alloc(bool, d.items.len);
        @memset(done, false);
        for (d.items, 0..) |it, i| {
            if (done[i] or !std.mem.eql(u8, it.layer(), layer.name)) continue;
            const src = it.src();
            try out.appendSlice(a, "<g data-src=\"");
            try escapeAttr(out, a, src);
            try out.appendSlice(a, "\">\n");
            for (d.items[i..], i..) |it2, j| {
                if (done[j] or !std.mem.eql(u8, it2.layer(), layer.name) or !std.mem.eql(u8, it2.src(), src)) continue;
                done[j] = true;
                try emitItem(out, a, st, it2, m, font);
            }
            try out.appendSlice(a, "</g>\n");
        }
        try out.appendSlice(a, "</g>\n");
    }
}

fn emitItem(out: *std.ArrayList(u8), a: Allocator, st: *const style_mod.Style, it: drawing.Item, m: Map, font: *const font_mod.Font) Allocator.Error!void {
    switch (it) {
        .path => |p| {
            try out.appendSlice(a, "<path d=\"");
            try pathData(out, a, m, p.pts, p.closed, true);
            try out.append(a, '"');
            try strokeAttrs(out, a, st, p.pen);
            try out.appendSlice(a, "/>\n");
        },
        .fill => |f| {
            try out.appendSlice(a, "<path d=\"");
            for (f.loops) |l| try pathData(out, a, m, l, true, true);
            try out.appendSlice(a, "\" fill=\"#000\" fill-rule=\"evenodd\" stroke=\"none\"/>\n");
        },
        .hatch => |h| {
            if (h.lines.len == 0) return;
            try out.appendSlice(a, "<path d=\"");
            for (h.lines) |ln| {
                try out.appendSlice(a, "M");
                try num(out, a, m.px(ln[0]));
                try out.append(a, ' ');
                try num(out, a, m.py(ln[1]));
                if (ln[0] == ln[2] and ln[1] == ln[3]) {
                    try out.appendSlice(a, "h0");
                } else {
                    try out.appendSlice(a, "L");
                    try num(out, a, m.px(ln[2]));
                    try out.append(a, ' ');
                    try num(out, a, m.py(ln[3]));
                }
            }
            try out.append(a, '"');
            try strokeAttrs(out, a, st, h.pen);
            try out.appendSlice(a, "/>\n");
        },
        .region => {},
        .text => |t| {
            const sts = try textgeom.strokes(a, font, t);
            if (sts.len == 0) return;
            try out.appendSlice(a, "<path d=\"");
            for (sts) |pl| {
                for (pl, 0..) |p, k| {
                    try out.appendSlice(a, if (k == 0) "M" else "L");
                    try num(out, a, m.px(p.x));
                    try out.append(a, ' ');
                    try num(out, a, m.py(p.y));
                }
            }
            try out.append(a, '"');
            try strokeAttrs(out, a, st, t.pen);
            try out.appendSlice(a, "/>\n");
        },
    }
}

/// Bare detail SVG: the drawing's bounds plus a margin, true paper scale.
pub fn render(a: Allocator, d: *const drawing.Drawing, font: *const font_mod.Font, opt: Options) Allocator.Error![]u8 {
    const s = d.scale;
    const margin = if (d.page_w > 0) 0 else opt.margin_in;
    const w_in = (d.bounds[2] - d.bounds[0]) / s + 2 * margin;
    const h_in = (d.bounds[3] - d.bounds[1]) / s + 2 * margin;
    const m = Map{ .s = s, .x0 = d.bounds[0], .y1 = d.bounds[3], .ox = margin, .oy = margin };
    var out: std.ArrayList(u8) = .empty;
    try header(&out, a, w_in, h_in);
    try out.appendSlice(a, "<rect width=\"100%\" height=\"100%\" fill=\"#fff\"/>\n");
    try emitItems(&out, a, d, m, font);
    try out.appendSlice(a, "</svg>\n");
    return out.items;
}

pub fn header(out: *std.ArrayList(u8), a: Allocator, w_in: f64, h_in: f64) Allocator.Error!void {
    try out.appendSlice(a, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\" width=\"");
    try num(out, a, w_in);
    try out.appendSlice(a, "in\" height=\"");
    try num(out, a, h_in);
    try out.appendSlice(a, "in\" viewBox=\"0 0 ");
    try num(out, a, w_in * px_per_in);
    try out.append(a, ' ');
    try num(out, a, h_in * px_per_in);
    try out.appendSlice(a, "\">\n");
}
