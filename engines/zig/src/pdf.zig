//! PDF exporter (SPEC 12): PDF 1.7, one page at the style's sheet size, vector only, text as
//! stroked paths (no fonts), deterministic object order, no timestamps.

const std = @import("std");
const geom = @import("geom.zig");
const drawing = @import("drawing.zig");
const style_mod = @import("style.zig");
const font_mod = @import("font.zig");
const textgeom = @import("textgeom.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Pt = geom.Pt;

const pt_per_in: f64 = 72.0;

fn num(out: *std.ArrayList(u8), a: Allocator, x: f64) Allocator.Error!void {
    var v = @round(x * 1000.0) / 1000.0;
    if (v == 0) v = 0;
    try out.print(a, "{d}", .{v});
}

const Cs = struct {
    a: Allocator,
    out: std.ArrayList(u8) = .empty,
    s: f64,
    x0: f64,
    y0: f64,

    fn px(self: *const Cs, x: f64) f64 {
        return (x - self.x0) / self.s * pt_per_in;
    }
    fn py(self: *const Cs, y: f64) f64 {
        return (y - self.y0) / self.s * pt_per_in;
    }
    fn xy(self: *Cs, x: f64, y: f64) Allocator.Error!void {
        try num(&self.out, self.a, self.px(x));
        try self.out.append(self.a, ' ');
        try num(&self.out, self.a, self.py(y));
    }
    fn op(self: *Cs, x: f64, y: f64, o: []const u8) Allocator.Error!void {
        try self.xy(x, y);
        try self.out.append(self.a, ' ');
        try self.out.appendSlice(self.a, o);
        try self.out.append(self.a, '\n');
    }
    fn curve(self: *Cs, p1: V2, p2: V2, p3: V2) Allocator.Error!void {
        try self.xy(p1.x, p1.y);
        try self.out.append(self.a, ' ');
        try self.xy(p2.x, p2.y);
        try self.out.append(self.a, ' ');
        try self.xy(p3.x, p3.y);
        try self.out.appendSlice(self.a, " c\n");
    }

    fn segments(self: *Cs, pts: []const Pt, closed: bool) Allocator.Error!void {
        if (pts.len == 0) return;
        try self.op(pts[0].x, pts[0].y, "m");
        const nseg = if (closed) pts.len else pts.len - 1;
        for (0..nseg) |i| {
            const p = pts[i];
            const q = pts[(i + 1) % pts.len];
            if (p.b == 0) {
                try self.op(q.x, q.y, "l");
            } else {
                const arc = geom.arcOf(p.v(), q.v(), p.b);
                const n: usize = @max(1, @as(usize, @intFromFloat(@ceil(@abs(arc.sweep) / (std.math.pi / 2.0) - 1e-9))));
                const step = arc.sweep / @as(f64, @floatFromInt(n));
                const k = 4.0 / 3.0 * @tan(step / 4.0);
                var a0 = arc.a0;
                for (0..n) |_| {
                    const a1 = a0 + step;
                    const c0 = V2.init(@cos(a0), @sin(a0));
                    const c1 = V2.init(@cos(a1), @sin(a1));
                    const e0 = arc.c.add(c0.scale(arc.r));
                    const e1 = arc.c.add(c1.scale(arc.r));
                    _ = e0;
                    const cp1 = arc.c.add(c0.add(c0.perp().scale(k)).scale(arc.r));
                    const cp2 = arc.c.add(c1.sub(c1.perp().scale(k)).scale(arc.r));
                    try self.curve(cp1, cp2, e1);
                    a0 = a1;
                }
            }
        }
        if (closed) try self.out.appendSlice(self.a, "h\n");
    }

    fn setPen(self: *Cs, st: *const style_mod.Style, pen: []const u8) Allocator.Error!void {
        const p = st.pen(pen);
        const w_mm = if (p) |x| x.width_mm else 0.25;
        try num(&self.out, self.a, w_mm / 25.4 * pt_per_in);
        try self.out.appendSlice(self.a, " w\n");
        if (p) |x| if (x.dash_mm) |d| {
            try self.out.append(self.a, '[');
            for (d, 0..) |v, i| {
                if (i > 0) try self.out.append(self.a, ' ');
                try num(&self.out, self.a, v / 25.4 * pt_per_in);
            }
            try self.out.appendSlice(self.a, "] 0 d\n");
            return;
        };
        try self.out.appendSlice(self.a, "[] 0 d\n");
    }
};

pub fn render(a: Allocator, d: *const drawing.Drawing, font: *const font_mod.Font) Allocator.Error![]u8 {
    const st = d.style;
    var cs = Cs{ .a = a, .s = d.scale, .x0 = d.bounds[0], .y0 = d.bounds[1] };
    const page_w = if (d.page_w > 0) d.page_w else (d.bounds[2] - d.bounds[0]) / d.scale;
    const page_h = if (d.page_h > 0) d.page_h else (d.bounds[3] - d.bounds[1]) / d.scale;
    try cs.out.appendSlice(a, "1 J 1 j 0 0 0 RG 0 0 0 rg\n");
    for (d.layers) |layer| {
        for (d.items) |it| {
            if (!std.mem.eql(u8, it.layer(), layer.name)) continue;
            switch (it) {
                .path => |p| {
                    try cs.setPen(st, p.pen);
                    try cs.segments(p.pts, p.closed);
                    try cs.out.appendSlice(a, "S\n");
                },
                .fill => |f| {
                    for (f.loops) |l| try cs.segments(l, true);
                    try cs.out.appendSlice(a, "f*\n");
                },
                .hatch => |h| {
                    if (h.lines.len == 0) continue;
                    try cs.setPen(st, h.pen);
                    for (h.lines) |ln| {
                        try cs.op(ln[0], ln[1], "m");
                        if (ln[0] == ln[2] and ln[1] == ln[3]) {
                            try cs.op(ln[2] + 0.001 * d.scale / 72.0, ln[3], "l");
                        } else try cs.op(ln[2], ln[3], "l");
                    }
                    try cs.out.appendSlice(a, "S\n");
                },
                .region => {},
                .text => |t| {
                    const sts = try textgeom.strokes(a, font, t);
                    if (sts.len == 0) continue;
                    try cs.setPen(st, t.pen);
                    for (sts) |pl| {
                        for (pl, 0..) |p, k| try cs.op(p.x, p.y, if (k == 0) "m" else "l");
                    }
                    try cs.out.appendSlice(a, "S\n");
                },
            }
        }
    }
    const content = cs.out.items;
    // ---- file ----
    var out: std.ArrayList(u8) = .empty;
    var offsets: [6]usize = undefined;
    try out.appendSlice(a, "%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
    offsets[1] = out.items.len;
    try out.appendSlice(a, "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    offsets[2] = out.items.len;
    try out.appendSlice(a, "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
    offsets[3] = out.items.len;
    try out.print(a, "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {d} {d}] /Contents 4 0 R /Resources << >> >>\nendobj\n", .{ @round(page_w * pt_per_in * 1000) / 1000, @round(page_h * pt_per_in * 1000) / 1000 });
    offsets[4] = out.items.len;
    try out.print(a, "4 0 obj\n<< /Length {d} >>\nstream\n", .{content.len});
    try out.appendSlice(a, content);
    try out.appendSlice(a, "endstream\nendobj\n");
    offsets[5] = out.items.len;
    try out.appendSlice(a, "5 0 obj\n<< /Title (Kerf detail) /Producer (kerf-zig) >>\nendobj\n");
    const xref = out.items.len;
    try out.appendSlice(a, "xref\n0 6\n0000000000 65535 f \n");
    for (1..6) |i| try out.print(a, "{d:0>10} 00000 n \n", .{offsets[i]});
    try out.print(a, "trailer\n<< /Size 6 /Root 1 0 R /Info 5 0 R >>\nstartxref\n{d}\n%%EOF\n", .{xref});
    return out.items;
}
