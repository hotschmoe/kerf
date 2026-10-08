//! DXF exporter (SPEC 12): AutoCAD R2000 (AC1015) ASCII, modelspace at 1:1 model inches,
//! $INSUNITS = 1. Entities: LWPOLYLINE (bulges), HATCH (pattern definitions embedded, or SOLID),
//! TEXT (style KERF -> romans.shx). Handles are sequential from a fixed seed; no timestamps.

const std = @import("std");
const json = @import("json.zig");
const geom = @import("geom.zig");
const drawing = @import("drawing.zig");
const style_mod = @import("style.zig");
const Pen = @import("pen.zig").Pen;
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Pt = geom.Pt;

const model_space_handle = "1F";
const paper_space_handle = "1B";

const Writer = struct {
    a: Allocator,
    out: std.ArrayList(u8) = .empty,
    next_handle: u32 = 0x100,

    fn code(self: *Writer, c: u32, comptime fmt: []const u8, args: anytype) Allocator.Error!void {
        try self.out.print(self.a, "{d}\n", .{c});
        try self.out.print(self.a, fmt, args);
        try self.out.append(self.a, '\n');
    }
    /// A block of group codes built at compile time by `dx`.
    fn lit(self: *Writer, text: []const u8) Allocator.Error!void {
        try self.out.appendSlice(self.a, text);
    }
    fn s(self: *Writer, c: u32, v: []const u8) Allocator.Error!void {
        try self.out.print(self.a, "{d}\n{s}\n", .{ c, v });
    }
    fn i(self: *Writer, c: u32, v: i64) Allocator.Error!void {
        try self.out.print(self.a, "{d}\n{d}\n", .{ c, v });
    }
    fn f(self: *Writer, c: u32, v: f64) Allocator.Error!void {
        var b: [40]u8 = undefined;
        try self.out.print(self.a, "{d}\n{s}\n", .{ c, fmtF(&b, v) });
    }
    fn handle(self: *Writer) Allocator.Error!u32 {
        const h = self.next_handle;
        self.next_handle += 1;
        try self.out.print(self.a, "5\n{X}\n", .{h});
        return h;
    }
};

/// Fixed `.{ code, "value" }` pairs as one DXF text block, at compile time (REVIEW SIZ-1: the ~300 constant
/// `s`/`i`/`f` calls of `render` were 16 KB of wasm). Numbers are written as `fmtF` / `{d}` print them.
inline fn dx(comptime pairs: anytype) []const u8 {
    return comptime blk: {
        @setEvalBranchQuota(100 * pairs.len + 1000);
        var t: []const u8 = "";
        for (pairs) |p| {
            var code: []const u8 = "";
            var c: u32 = p[0];
            while (true) : (c /= 10) {
                code = &[1]u8{'0' + c % 10} ++ code;
                if (c < 10) break;
            }
            t = t ++ code ++ "\n" ++ p[1] ++ "\n";
        }
        break :blk t;
    };
}

/// Shortest decimal of the value rounded to 1e-6 (deterministic, no exponent, never overflows).
pub fn fmtF(buf: *[40]u8, x: f64) []const u8 {
    return json.fmtFixed(buf, x, 6);
}

const std_lineweights = [_]i32{ 0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200, 211 };

pub fn lineweightFor(mm: f64) i32 {
    const target = mm * 100.0;
    var best: i32 = 0;
    var bd: f64 = 1e9;
    for (std_lineweights) |w| {
        const d = @abs(@as(f64, @floatFromInt(w)) - target);
        if (d < bd) {
            bd = d;
            best = w;
        }
    }
    return best;
}

fn linetypeName(a: Allocator, pen: []const u8) Allocator.Error![]const u8 {
    if (std.mem.eql(u8, pen, "hidden")) return "DASHED";
    var out = try a.alloc(u8, 5 + pen.len);
    @memcpy(out[0..5], "KERF_");
    for (pen, 0..) |c, k| out[5 + k] = std.ascii.toUpper(c);
    return out;
}

fn penLt(a: Allocator, st: *const style_mod.Style, pen: Pen) Allocator.Error!?[]const u8 {
    if (st.pen(pen)) |p| if (p.dash_mm != null) return try linetypeName(a, @tagName(pen));
    return null;
}

fn entityHeader(w: *Writer, etype: []const u8, layer: []const u8, lw: i32, lt: ?[]const u8) Allocator.Error!void {
    try w.s(0, etype);
    _ = try w.handle();
    try w.s(330, model_space_handle);
    try w.lit(dx(.{.{ 100, "AcDbEntity" }}));
    try w.s(8, layer);
    if (lt) |l| try w.s(6, l);
    try w.i(370, lw);
}

fn lwpoly(w: *Writer, layer: []const u8, lw: i32, lt: ?[]const u8, closed: bool, pts: []const Pt) Allocator.Error!void {
    if (pts.len < 2) return;
    try entityHeader(w, "LWPOLYLINE", layer, lw, lt);
    try w.lit(dx(.{.{ 100, "AcDbPolyline" }}));
    try w.i(90, @intCast(pts.len));
    try w.i(70, if (closed) 1 else 0);
    try w.lit(dx(.{.{ 43, "0" }}));
    for (pts) |p| {
        try w.f(10, p.x);
        try w.f(20, p.y);
        if (p.b != 0) try w.f(42, p.b);
    }
}

fn hatchBoundary(w: *Writer, loops: []const []const Pt) Allocator.Error!void {
    try w.i(91, @intCast(loops.len));
    for (loops, 0..) |l, k| {
        try w.i(92, if (k == 0) 3 else 2);
        var has_b = false;
        for (l) |p| if (p.b != 0) {
            has_b = true;
        };
        try w.i(72, if (has_b) 1 else 0);
        try w.lit(dx(.{.{ 73, "1" }}));
        try w.i(93, @intCast(l.len));
        for (l) |p| {
            try w.f(10, p.x);
            try w.f(20, p.y);
            if (has_b) try w.f(42, p.b);
        }
        try w.lit(dx(.{.{ 97, "0" }}));
    }
}

fn hatchHeader(w: *Writer, layer: []const u8, lw: i32, name: []const u8, solid: bool) Allocator.Error!void {
    try entityHeader(w, "HATCH", layer, lw, null);
    try w.lit(dx(.{
        .{ 100, "AcDbHatch" },
        .{ 10, "0" },
        .{ 20, "0" },
        .{ 30, "0" },
        .{ 210, "0" },
        .{ 220, "0" },
        .{ 230, "1" },
    }));
    try w.s(2, name);
    try w.i(70, if (solid) 1 else 0);
    try w.lit(dx(.{.{ 71, "0" }}));
}

pub fn render(a: Allocator, d: *const drawing.Drawing) Allocator.Error![]u8 {
    const st = d.style;
    var w = Writer{ .a = a };
    // ---- entities first (so the handle seed and extents are known) ----
    var ext = geom.Box{};
    for (d.items) |it| {
        switch (it) {
            .path => |p| {
                const lw = lineweightFor(st.penWidthMm(p.pen));
                try lwpoly(&w, p.layer, lw, try penLt(a, st, p.pen), p.closed, p.pts);
                ext.addBox(geom.pointsBox(p.pts));
            },
            .fill => |f| {
                try hatchHeader(&w, f.layer, lineweightFor(st.penWidthMm(.cut)), "SOLID", true);
                try hatchBoundary(&w, f.loops);
                try w.lit(dx(.{
                    .{ 75, "0" },
                    .{ 76, "1" },
                    .{ 98, "0" },
                }));
                for (f.loops) |l| ext.addBox(geom.pointsBox(l));
            },
            .hatch => |h| {
                const pat = st.pattern(h.pattern);
                if (h.loops.len == 0) continue;
                try hatchHeader(&w, h.layer, lineweightFor(st.penWidthMm(h.pen)), h.pattern, false);
                try hatchBoundary(&w, h.loops);
                try w.lit(dx(.{
                    .{ 75, "0" },
                    .{ 76, "1" },
                }));
                try w.f(52, h.angle);
                try w.lit(dx(.{
                    .{ 41, "1" },
                    .{ 77, "0" },
                }));
                const k = d.scale * h.scale;
                if (pat) |p| {
                    try w.i(78, @intCast(p.families.len));
                    for (p.families) |fam| {
                        try w.f(53, fam.angle);
                        try w.f(43, fam.x0 * k);
                        try w.f(44, fam.y0 * k);
                        try w.f(45, fam.dx * k);
                        try w.f(46, fam.dy * k);
                        try w.i(79, @intCast(fam.dashes.len));
                        for (fam.dashes) |dd| try w.f(49, dd * k);
                    }
                } else try w.i(78, 0);
                try w.lit(dx(.{.{ 98, "0" }}));
                for (h.loops) |l| ext.addBox(geom.pointsBox(l));
            },
            .region => {},
            .text => |t| {
                if (t.s.len == 0) continue;
                try entityHeader(&w, "TEXT", t.layer, lineweightFor(st.penWidthMm(t.pen)), null);
                try w.lit(dx(.{.{ 100, "AcDbText" }}));
                try w.f(10, t.x);
                try w.f(20, t.y);
                try w.lit(dx(.{.{ 30, "0" }}));
                try w.f(40, t.h);
                try w.s(1, t.s);
                if (t.rot != 0) try w.f(50, t.rot);
                try w.s(7, st.dxf_style);
                const ha: i64 = switch (t.align_) {
                    .left => 0,
                    .center => 1,
                    .right => 2,
                };
                const va: i64 = switch (t.valign) {
                    .baseline => 0,
                    .middle => 2,
                    .top => 3,
                };
                if (ha != 0 or va != 0) {
                    try w.i(72, ha);
                    try w.f(11, t.x);
                    try w.f(21, t.y);
                    try w.lit(dx(.{.{ 31, "0" }}));
                }
                try w.lit(dx(.{.{ 100, "AcDbText" }}));
                if (va != 0) try w.i(73, va);
                ext.addPoint(t.x, t.y);
            },
        }
    }
    if (ext.isEmpty()) ext = .{ .x0 = 0, .y0 = 0, .x1 = 1, .y1 = 1 };
    const entities = w.out.items;
    const seed = w.next_handle;

    // ---- assemble ----
    var o = Writer{ .a = a };
    try o.lit(dx(.{
        .{ 0, "SECTION" },
        .{ 2, "HEADER" },
        .{ 9, "$ACADVER" },
        .{ 1, "AC1015" },
        .{ 9, "$HANDSEED" },
    }));
    try o.code(5, "{X}", .{seed});
    try o.lit(dx(.{
        .{ 9, "$INSUNITS" },
        .{ 70, "1" },
        .{ 9, "$MEASUREMENT" },
        .{ 70, "0" },
        .{ 9, "$LTSCALE" },
        .{ 40, "1" },
        .{ 9, "$EXTMIN" },
    }));
    try o.f(10, ext.x0);
    try o.f(20, ext.y0);
    try o.lit(dx(.{
        .{ 30, "0" },
        .{ 9, "$EXTMAX" },
    }));
    try o.f(10, ext.x1);
    try o.f(20, ext.y1);
    try o.lit(dx(.{
        .{ 30, "0" },
        .{ 0, "ENDSEC" },
        .{ 0, "SECTION" },
        .{ 2, "CLASSES" },
        .{ 0, "ENDSEC" },
    }));

    // TABLES
    try o.lit(dx(.{
        .{ 0, "SECTION" },
        .{ 2, "TABLES" },
    }));
    // VPORT
    try o.lit(dx(.{
        .{ 0, "TABLE" },
        .{ 2, "VPORT" },
        .{ 5, "8" },
        .{ 330, "0" },
        .{ 100, "AcDbSymbolTable" },
        .{ 70, "1" },
        .{ 0, "VPORT" },
        .{ 5, "30" },
        .{ 330, "8" },
        .{ 100, "AcDbSymbolTableRecord" },
        .{ 100, "AcDbViewportTableRecord" },
        .{ 2, "*ACTIVE" },
        .{ 70, "0" },
        .{ 10, "0" },
        .{ 20, "0" },
        .{ 11, "1" },
        .{ 21, "1" },
    }));
    const cx = (ext.x0 + ext.x1) / 2;
    const cy = (ext.y0 + ext.y1) / 2;
    try o.f(12, cx);
    try o.f(22, cy);
    try o.lit(dx(.{
        .{ 13, "0" },
        .{ 23, "0" },
        .{ 14, "10" },
        .{ 24, "10" },
        .{ 15, "10" },
        .{ 25, "10" },
        .{ 16, "0" },
        .{ 26, "0" },
        .{ 36, "1" },
        .{ 17, "0" },
        .{ 27, "0" },
        .{ 37, "0" },
    }));
    try o.f(40, @max(ext.y1 - ext.y0, 1) * 1.1);
    try o.f(41, @max((ext.x1 - ext.x0) / @max(ext.y1 - ext.y0, 1), 0.1));
    try o.lit(dx(.{
        .{ 42, "50" },
        .{ 43, "0" },
        .{ 44, "0" },
        .{ 50, "0" },
        .{ 51, "0" },
        .{ 71, "0" },
        .{ 72, "100" },
        .{ 73, "1" },
        .{ 74, "3" },
        .{ 75, "0" },
        .{ 76, "0" },
        .{ 77, "0" },
        .{ 78, "0" },
        .{ 0, "ENDTAB" },
    }));

    // LTYPE
    var lts: std.ArrayList(struct { name: []const u8, pen: style_mod.PenDef }) = .empty;
    for (st.pens) |p| if (p.dash_mm != null) {
        // only linetypes actually used
        var used = false;
        for (d.items) |it| if (it == .path and std.mem.eql(u8, @tagName(it.path.pen), p.name)) {
            used = true;
            break;
        };
        if (used or std.mem.eql(u8, p.name, "hidden")) try lts.append(a, .{ .name = try linetypeName(a, p.name), .pen = p });
    };
    try o.lit(dx(.{
        .{ 0, "TABLE" },
        .{ 2, "LTYPE" },
        .{ 5, "5" },
        .{ 330, "0" },
        .{ 100, "AcDbSymbolTable" },
    }));
    try o.i(70, @intCast(3 + lts.items.len));
    try o.lit(dx(.{
        .{ 0, "LTYPE" },
        .{ 5, "14" },
        .{ 330, "5" },
        .{ 100, "AcDbSymbolTableRecord" },
        .{ 100, "AcDbLinetypeTableRecord" },
        .{ 2, "BYBLOCK" },
        .{ 70, "0" },
        .{ 3, "" },
        .{ 72, "65" },
        .{ 73, "0" },
        .{ 40, "0" },
        .{ 0, "LTYPE" },
        .{ 5, "15" },
        .{ 330, "5" },
        .{ 100, "AcDbSymbolTableRecord" },
        .{ 100, "AcDbLinetypeTableRecord" },
        .{ 2, "BYLAYER" },
        .{ 70, "0" },
        .{ 3, "" },
        .{ 72, "65" },
        .{ 73, "0" },
        .{ 40, "0" },
        .{ 0, "LTYPE" },
        .{ 5, "16" },
        .{ 330, "5" },
        .{ 100, "AcDbSymbolTableRecord" },
        .{ 100, "AcDbLinetypeTableRecord" },
        .{ 2, "CONTINUOUS" },
        .{ 70, "0" },
        .{ 3, "Solid line" },
        .{ 72, "65" },
        .{ 73, "0" },
        .{ 40, "0" },
    }));
    for (lts.items, 0..) |lt, idx| {
        try o.lit(dx(.{.{ 0, "LTYPE" }}));
        try o.code(5, "{X}", .{0x40 + idx});
        try o.lit(dx(.{
            .{ 330, "5" },
            .{ 100, "AcDbSymbolTableRecord" },
            .{ 100, "AcDbLinetypeTableRecord" },
        }));
        try o.s(2, lt.name);
        try o.lit(dx(.{.{ 70, "0" }}));
        try o.s(3, lt.name);
        try o.lit(dx(.{.{ 72, "65" }}));
        const dm = lt.pen.dash_mm.?;
        try o.i(73, @intCast(dm.len));
        var total: f64 = 0;
        for (dm) |x| total += x / 25.4 * d.scale;
        try o.f(40, total);
        for (dm, 0..) |x, k| {
            const len = x / 25.4 * d.scale;
            try o.f(49, if (k % 2 == 0) len else -len);
            try o.lit(dx(.{.{ 74, "0" }}));
        }
    }
    try o.lit(dx(.{.{ 0, "ENDTAB" }}));

    // LAYER
    try o.lit(dx(.{
        .{ 0, "TABLE" },
        .{ 2, "LAYER" },
        .{ 5, "2" },
        .{ 330, "0" },
        .{ 100, "AcDbSymbolTable" },
    }));
    try o.i(70, @intCast(1 + d.layers.len));
    try o.lit(dx(.{
        .{ 0, "LAYER" },
        .{ 5, "10" },
        .{ 330, "2" },
        .{ 100, "AcDbSymbolTableRecord" },
        .{ 100, "AcDbLayerTableRecord" },
        .{ 2, "0" },
        .{ 70, "0" },
        .{ 62, "7" },
        .{ 6, "CONTINUOUS" },
        .{ 370, "-3" },
    }));
    for (d.layers, 0..) |l, idx| {
        try o.lit(dx(.{.{ 0, "LAYER" }}));
        try o.code(5, "{X}", .{0x50 + idx});
        try o.lit(dx(.{
            .{ 330, "2" },
            .{ 100, "AcDbSymbolTableRecord" },
            .{ 100, "AcDbLayerTableRecord" },
        }));
        try o.s(2, l.name);
        try o.lit(dx(.{
            .{ 70, "0" },
            .{ 62, "7" },
        }));
        try o.s(6, if (std.mem.eql(u8, l.linetype, "DASHED")) "DASHED" else "CONTINUOUS");
        try o.i(370, lineweightFor(l.lineweight_mm));
    }
    try o.lit(dx(.{.{ 0, "ENDTAB" }}));

    // STYLE
    try o.lit(dx(.{
        .{ 0, "TABLE" },
        .{ 2, "STYLE" },
        .{ 5, "3" },
        .{ 330, "0" },
        .{ 100, "AcDbSymbolTable" },
        .{ 70, "2" },
        .{ 0, "STYLE" },
        .{ 5, "11" },
        .{ 330, "3" },
        .{ 100, "AcDbSymbolTableRecord" },
        .{ 100, "AcDbTextStyleTableRecord" },
        .{ 2, "STANDARD" },
        .{ 70, "0" },
        .{ 40, "0" },
        .{ 41, "1" },
        .{ 50, "0" },
        .{ 71, "0" },
        .{ 42, "0.2" },
        .{ 3, "txt" },
        .{ 4, "" },
        .{ 0, "STYLE" },
        .{ 5, "12" },
        .{ 330, "3" },
        .{ 100, "AcDbSymbolTableRecord" },
        .{ 100, "AcDbTextStyleTableRecord" },
    }));
    try o.s(2, st.dxf_style);
    try o.lit(dx(.{
        .{ 70, "0" },
        .{ 40, "0" },
        .{ 41, "1" },
        .{ 50, "0" },
        .{ 71, "0" },
        .{ 42, "0.2" },
    }));
    try o.s(3, st.dxf_font);
    try o.lit(dx(.{
        .{ 4, "" },
        .{ 0, "ENDTAB" },
    }));

    // VIEW, UCS (empty)
    try o.lit(dx(.{
        .{ 0, "TABLE" },
        .{ 2, "VIEW" },
        .{ 5, "6" },
        .{ 330, "0" },
        .{ 100, "AcDbSymbolTable" },
        .{ 70, "0" },
        .{ 0, "ENDTAB" },
        .{ 0, "TABLE" },
        .{ 2, "UCS" },
        .{ 5, "7" },
        .{ 330, "0" },
        .{ 100, "AcDbSymbolTable" },
        .{ 70, "0" },
        .{ 0, "ENDTAB" },
    }));
    // APPID
    try o.lit(dx(.{
        .{ 0, "TABLE" },
        .{ 2, "APPID" },
        .{ 5, "9" },
        .{ 330, "0" },
        .{ 100, "AcDbSymbolTable" },
        .{ 70, "1" },
        .{ 0, "APPID" },
        .{ 5, "13" },
        .{ 330, "9" },
        .{ 100, "AcDbSymbolTableRecord" },
        .{ 100, "AcDbRegAppTableRecord" },
        .{ 2, "ACAD" },
        .{ 70, "0" },
        .{ 0, "ENDTAB" },
    }));
    // DIMSTYLE
    try o.lit(dx(.{
        .{ 0, "TABLE" },
        .{ 2, "DIMSTYLE" },
        .{ 5, "A" },
        .{ 330, "0" },
        .{ 100, "AcDbSymbolTable" },
        .{ 70, "1" },
        .{ 100, "AcDbDimStyleTable" },
        .{ 71, "1" },
        .{ 0, "DIMSTYLE" },
        .{ 105, "17" },
        .{ 330, "A" },
        .{ 100, "AcDbSymbolTableRecord" },
        .{ 100, "AcDbDimStyleTableRecord" },
        .{ 2, "STANDARD" },
        .{ 70, "0" },
        .{ 340, "17" },
        .{ 0, "ENDTAB" },
    }));
    // BLOCK_RECORD
    try o.lit(dx(.{
        .{ 0, "TABLE" },
        .{ 2, "BLOCK_RECORD" },
        .{ 5, "1" },
        .{ 330, "0" },
        .{ 100, "AcDbSymbolTable" },
        .{ 70, "2" },
        .{ 0, "BLOCK_RECORD" },
        .{ 5, model_space_handle },
        .{ 330, "1" },
        .{ 100, "AcDbSymbolTableRecord" },
        .{ 100, "AcDbBlockTableRecord" },
        .{ 2, "*Model_Space" },
        .{ 70, "0" },
        .{ 280, "1" },
        .{ 281, "0" },
        .{ 0, "BLOCK_RECORD" },
        .{ 5, paper_space_handle },
        .{ 330, "1" },
        .{ 100, "AcDbSymbolTableRecord" },
        .{ 100, "AcDbBlockTableRecord" },
        .{ 2, "*Paper_Space" },
        .{ 70, "0" },
        .{ 280, "1" },
        .{ 281, "0" },
        .{ 0, "ENDTAB" },
        .{ 0, "ENDSEC" },
    }));

    // BLOCKS
    try o.lit(dx(.{
        .{ 0, "SECTION" },
        .{ 2, "BLOCKS" },
        .{ 0, "BLOCK" },
        .{ 5, "20" },
        .{ 330, model_space_handle },
        .{ 100, "AcDbEntity" },
        .{ 8, "0" },
        .{ 100, "AcDbBlockBegin" },
        .{ 2, "*Model_Space" },
        .{ 70, "0" },
        .{ 10, "0" },
        .{ 20, "0" },
        .{ 30, "0" },
        .{ 3, "*Model_Space" },
        .{ 1, "" },
        .{ 0, "ENDBLK" },
        .{ 5, "21" },
        .{ 330, model_space_handle },
        .{ 100, "AcDbEntity" },
        .{ 8, "0" },
        .{ 100, "AcDbBlockEnd" },
        .{ 0, "BLOCK" },
        .{ 5, "22" },
        .{ 330, paper_space_handle },
        .{ 100, "AcDbEntity" },
        .{ 67, "1" },
        .{ 8, "0" },
        .{ 100, "AcDbBlockBegin" },
        .{ 2, "*Paper_Space" },
        .{ 70, "0" },
        .{ 10, "0" },
        .{ 20, "0" },
        .{ 30, "0" },
        .{ 3, "*Paper_Space" },
        .{ 1, "" },
        .{ 0, "ENDBLK" },
        .{ 5, "23" },
        .{ 330, paper_space_handle },
        .{ 100, "AcDbEntity" },
        .{ 67, "1" },
        .{ 8, "0" },
        .{ 100, "AcDbBlockEnd" },
        .{ 0, "ENDSEC" },
    }));

    // ENTITIES
    try o.lit(dx(.{
        .{ 0, "SECTION" },
        .{ 2, "ENTITIES" },
    }));
    try o.out.appendSlice(a, entities);
    try o.lit(dx(.{.{ 0, "ENDSEC" }}));

    // OBJECTS
    try o.lit(dx(.{
        .{ 0, "SECTION" },
        .{ 2, "OBJECTS" },
        .{ 0, "DICTIONARY" },
        .{ 5, "C" },
        .{ 330, "0" },
        .{ 100, "AcDbDictionary" },
        .{ 281, "1" },
        .{ 3, "ACAD_GROUP" },
        .{ 350, "D" },
        .{ 0, "DICTIONARY" },
        .{ 5, "D" },
        .{ 330, "C" },
        .{ 100, "AcDbDictionary" },
        .{ 281, "1" },
        .{ 0, "ENDSEC" },
        .{ 0, "EOF" },
    }));
    return o.out.items;
}
