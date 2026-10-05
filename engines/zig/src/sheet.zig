//! Sheet (SPEC 12, DESIGN 5): frame, title block and footer around a detail placed at true scale.
//! `withSheet` returns a Drawing whose bounds are the full page, in model coordinates (the sheet
//! geometry is built in paper inches and mapped through the inverse of the paper scale), so every
//! exporter can treat sheet and detail uniformly.

const std = @import("std");
const geom = @import("geom.zig");
const drawing = @import("drawing.zig");
const font_mod = @import("font.zig");
const style_mod = @import("style.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Pt = geom.Pt;
const Item = drawing.Item;

const Ctx = struct {
    a: Allocator,
    d: *const drawing.Drawing,
    font: *const font_mod.Font,
    st: *const style_mod.Style,
    // paper -> model
    s: f64,
    ox: f64, // model x at paper (0,0)
    oy: f64,
    items: std.ArrayList(Item) = .empty,

    fn m(self: *const Ctx, x: f64, y: f64) V2 {
        return V2.init(self.ox + x * self.s, self.oy + y * self.s);
    }

    fn layer(self: *const Ctx, key: []const u8) []const u8 {
        return if (self.st.layerByKey(key)) |l| l.name else "0";
    }

    fn path(self: *Ctx, pen: []const u8, pts: []const [2]f64, closed: bool) Allocator.Error!void {
        const out = try self.a.alloc(Pt, pts.len);
        for (pts, 0..) |q, i| out[i] = Pt.at(self.m(q[0], q[1]), 0);
        try self.items.append(self.a, .{ .path = .{ .layer = self.st.layerForPen(pen), .pen = pen, .src = "sheet", .closed = closed, .pts = out } });
    }

    fn rect(self: *Ctx, pen: []const u8, x0: f64, y0: f64, x1: f64, y1: f64) Allocator.Error!void {
        try self.path(pen, &.{ .{ x0, y0 }, .{ x1, y0 }, .{ x1, y1 }, .{ x0, y1 } }, true);
    }

    fn fillRect(self: *Ctx, x0: f64, y0: f64, x1: f64, y1: f64) Allocator.Error!void {
        const l = try self.a.alloc(Pt, 4);
        const cs = [4][2]f64{ .{ x0, y0 }, .{ x1, y0 }, .{ x1, y1 }, .{ x0, y1 } };
        for (cs, 0..) |q, i| l[i] = Pt.at(self.m(q[0], q[1]), 0);
        const loops = try self.a.alloc([]const Pt, 1);
        loops[0] = l;
        try self.items.append(self.a, .{ .fill = .{ .layer = self.layer("title"), .src = "sheet", .loops = loops } });
    }

    fn text(self: *Ctx, pen: []const u8, s: []const u8, x: f64, y: f64, h: f64) Allocator.Error!void {
        const p = self.m(x, y);
        try self.items.append(self.a, .{ .text = .{ .layer = self.layer("title"), .pen = pen, .src = "sheet", .s = s, .x = p.x, .y = p.y, .h = h * self.s, .rot = 0, .align_ = .left, .valign = .baseline } });
    }
};

fn fitH(font: *const font_mod.Font, s: []const u8, h: f64, maxw: f64) f64 {
    const w = font.width(s, h);
    return if (w <= maxw) h else @max(h * maxw / w, h * 0.6);
}

pub fn withSheet(a: Allocator, d: drawing.Drawing, font: *const font_mod.Font) Allocator.Error!drawing.Drawing {
    const st = d.style;
    const s = d.scale;
    const W = st.sheet_w_in;
    const H = st.sheet_h_in;
    const m = st.margin_in;
    const tbh = st.title_block_h_in;
    const bw = (d.bounds[2] - d.bounds[0]) / s;
    const bh = (d.bounds[3] - d.bounds[1]) / s;
    const fw = W - 2 * m;
    const fh = H - 2 * m - tbh;
    const px = m + (fw - bw) * 0.5;
    const py = m + tbh + (fh - bh) * 0.5;
    // model coordinate of paper origin
    var c = Ctx{ .a = a, .d = &d, .font = font, .st = st, .s = s, .ox = d.bounds[0] - px * s, .oy = d.bounds[1] - py * s };
    // detail items (drop the footnote: it moves to the footer)
    for (d.items) |it| {
        if (it == .text and std.mem.eql(u8, it.text.src, "footnote")) continue;
        try c.items.append(a, it);
    }
    const fx1 = W - m;
    const fy1 = H - m;
    try c.rect("frame", m, m, fx1, fy1);
    try c.rect("title", m, m, fx1, m + tbh);
    const total = fx1 - m;
    const ratios = [6]f64{ 3.4, 2.0, 1.2, 1.15, 1.2, 1.3 };
    var sum: f64 = 0;
    for (ratios) |r| sum += r;
    const detail_no = if (d.sheet_no.len == 0) d.number else try std.fmt.allocPrint(a, "{s}/{s}", .{ d.number, d.sheet_no });
    const author = if (d.author.len == 0) "KERF" else try font_mod.upperAscii(a, d.author);
    const project = if (d.project.len > 0) try font_mod.upperAscii(a, d.project) else if (st.project.len > 0) try font_mod.upperAscii(a, st.project) else "";
    const title = try font_mod.upperAscii(a, if (d.title.len == 0) d.doc_title else d.title);
    const labels = [6][]const u8{ "DETAIL", "PROJECT", "SCALE", "DRAWN", "DATE", "DETAIL NO" };
    const values = [6][]const u8{ title, project, d.scale_label, author, d.date, detail_no };
    const lh = st.label_height_in;
    var x = m;
    for (0..6) |i| {
        const cw = ratios[i] * total / sum;
        if (i > 0) try c.path("title", &.{ .{ x, m }, .{ x, m + tbh } }, false);
        try c.text("anno", labels[i], x + 0.06, m + tbh - 0.06 - lh * 0.8, lh * 0.8);
        const vh = fitH(font, values[i], 0.125, cw - 0.14);
        try c.text("title", values[i], x + 0.07, m + 0.2, vh);
        x += cw;
    }
    // footer in the margin below the frame
    const fy = m * 0.42;
    const fth = lh;
    try c.text("title", "KERF", m, fy, fth * 1.1);
    const kw = font.width("KERF", fth * 1.1);
    for (0..3) |k| {
        const bx = m + kw + 0.06 + @as(f64, @floatFromInt(k)) * 0.045;
        try c.fillRect(bx, fy, bx + 0.025, fy + fth * 1.1);
    }
    var foot: std.ArrayList(u8) = .empty;
    if (d.code_basis.len > 0) try foot.print(a, "CODE BASIS: {s}", .{try font_mod.upperAscii(a, d.code_basis)});
    if (d.has_unverified) {
        if (foot.items.len > 0) try foot.appendSlice(a, "   ");
        try foot.appendSlice(a, st.cite_footnote);
    }
    if (foot.items.len > 0) try c.text("anno", foot.items, m + kw + 0.3, fy, fth);
    var out = d;
    out.items = c.items.items;
    out.bounds = .{ c.ox, c.oy, c.ox + W * s, c.oy + H * s };
    out.page_w = W;
    out.page_h = H;
    return out;
}
