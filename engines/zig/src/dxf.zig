//! DXF exporter (SPEC 12): AutoCAD R2000 (AC1015) ASCII, modelspace at 1:1 model inches,
//! $INSUNITS = 1. Entities: LWPOLYLINE (bulges), HATCH (pattern definitions embedded, or SOLID),
//! TEXT (style KERF -> romans.shx). Handles are sequential from a fixed seed; no timestamps.

const std = @import("std");
const json = @import("json.zig");
const geom = @import("geom.zig");
const drawing = @import("drawing.zig");
const style_mod = @import("style.zig");
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
    fn s(self: *Writer, c: u32, v: []const u8) Allocator.Error!void {
        try self.out.print(self.a, "{d}\n{s}\n", .{ c, v });
    }
    fn i(self: *Writer, c: u32, v: i64) Allocator.Error!void {
        try self.out.print(self.a, "{d}\n{d}\n", .{ c, v });
    }
    fn f(self: *Writer, c: u32, v: f64) Allocator.Error!void {
        var b: [48]u8 = undefined;
        try self.out.print(self.a, "{d}\n{s}\n", .{ c, fmtF(&b, v) });
    }
    fn handle(self: *Writer) Allocator.Error!u32 {
        const h = self.next_handle;
        self.next_handle += 1;
        try self.out.print(self.a, "5\n{X}\n", .{h});
        return h;
    }
};

/// Shortest decimal of the value rounded to 1e-6 (deterministic, no exponent).
pub fn fmtF(buf: *[48]u8, x: f64) []const u8 {
    var v = @round(x * 1.0e6) / 1.0e6;
    if (v == 0) v = 0;
    return std.fmt.bufPrint(buf, "{d}", .{v}) catch "0";
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

fn penLt(a: Allocator, st: *const style_mod.Style, pen: []const u8) Allocator.Error!?[]const u8 {
    if (st.pen(pen)) |p| if (p.dash_mm != null) return try linetypeName(a, pen);
    return null;
}

fn entityHeader(w: *Writer, etype: []const u8, layer: []const u8, lw: i32, lt: ?[]const u8) Allocator.Error!void {
    try w.s(0, etype);
    _ = try w.handle();
    try w.s(330, model_space_handle);
    try w.s(100, "AcDbEntity");
    try w.s(8, layer);
    if (lt) |l| try w.s(6, l);
    try w.i(370, lw);
}

fn lwpoly(w: *Writer, layer: []const u8, lw: i32, lt: ?[]const u8, closed: bool, pts: []const Pt) Allocator.Error!void {
    if (pts.len < 2) return;
    try entityHeader(w, "LWPOLYLINE", layer, lw, lt);
    try w.s(100, "AcDbPolyline");
    try w.i(90, @intCast(pts.len));
    try w.i(70, if (closed) 1 else 0);
    try w.f(43, 0);
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
        try w.i(73, 1);
        try w.i(93, @intCast(l.len));
        for (l) |p| {
            try w.f(10, p.x);
            try w.f(20, p.y);
            if (has_b) try w.f(42, p.b);
        }
        try w.i(97, 0);
    }
}

fn hatchHeader(w: *Writer, layer: []const u8, lw: i32, name: []const u8, solid: bool) Allocator.Error!void {
    try entityHeader(w, "HATCH", layer, lw, null);
    try w.s(100, "AcDbHatch");
    try w.f(10, 0);
    try w.f(20, 0);
    try w.f(30, 0);
    try w.f(210, 0);
    try w.f(220, 0);
    try w.f(230, 1);
    try w.s(2, name);
    try w.i(70, if (solid) 1 else 0);
    try w.i(71, 0);
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
                try hatchHeader(&w, f.layer, lineweightFor(st.penWidthMm("cut")), "SOLID", true);
                try hatchBoundary(&w, f.loops);
                try w.i(75, 0);
                try w.i(76, 1);
                try w.i(98, 0);
                for (f.loops) |l| ext.addBox(geom.pointsBox(l));
            },
            .hatch => |h| {
                const pat = st.pattern(h.pattern);
                if (h.loops.len == 0) continue;
                try hatchHeader(&w, h.layer, lineweightFor(st.penWidthMm(h.pen)), h.pattern, false);
                try hatchBoundary(&w, h.loops);
                try w.i(75, 0);
                try w.i(76, 1);
                try w.f(52, h.angle);
                try w.f(41, 1);
                try w.i(77, 0);
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
                try w.i(98, 0);
                for (h.loops) |l| ext.addBox(geom.pointsBox(l));
            },
            .region => {},
            .text => |t| {
                if (t.s.len == 0) continue;
                try entityHeader(&w, "TEXT", t.layer, lineweightFor(st.penWidthMm(t.pen)), null);
                try w.s(100, "AcDbText");
                try w.f(10, t.x);
                try w.f(20, t.y);
                try w.f(30, 0);
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
                    try w.f(31, 0);
                }
                try w.s(100, "AcDbText");
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
    try o.s(0, "SECTION");
    try o.s(2, "HEADER");
    try o.s(9, "$ACADVER");
    try o.s(1, "AC1015");
    try o.s(9, "$HANDSEED");
    try o.code(5, "{X}", .{seed});
    try o.s(9, "$INSUNITS");
    try o.i(70, 1);
    try o.s(9, "$MEASUREMENT");
    try o.i(70, 0);
    try o.s(9, "$LTSCALE");
    try o.f(40, 1);
    try o.s(9, "$EXTMIN");
    try o.f(10, ext.x0);
    try o.f(20, ext.y0);
    try o.f(30, 0);
    try o.s(9, "$EXTMAX");
    try o.f(10, ext.x1);
    try o.f(20, ext.y1);
    try o.f(30, 0);
    try o.s(0, "ENDSEC");
    try o.s(0, "SECTION");
    try o.s(2, "CLASSES");
    try o.s(0, "ENDSEC");

    // TABLES
    try o.s(0, "SECTION");
    try o.s(2, "TABLES");
    // VPORT
    try o.s(0, "TABLE");
    try o.s(2, "VPORT");
    try o.s(5, "8");
    try o.s(330, "0");
    try o.s(100, "AcDbSymbolTable");
    try o.i(70, 1);
    try o.s(0, "VPORT");
    try o.s(5, "30");
    try o.s(330, "8");
    try o.s(100, "AcDbSymbolTableRecord");
    try o.s(100, "AcDbViewportTableRecord");
    try o.s(2, "*ACTIVE");
    try o.i(70, 0);
    try o.f(10, 0);
    try o.f(20, 0);
    try o.f(11, 1);
    try o.f(21, 1);
    const cx = (ext.x0 + ext.x1) / 2;
    const cy = (ext.y0 + ext.y1) / 2;
    try o.f(12, cx);
    try o.f(22, cy);
    try o.f(13, 0);
    try o.f(23, 0);
    try o.f(14, 10);
    try o.f(24, 10);
    try o.f(15, 10);
    try o.f(25, 10);
    try o.f(16, 0);
    try o.f(26, 0);
    try o.f(36, 1);
    try o.f(17, 0);
    try o.f(27, 0);
    try o.f(37, 0);
    try o.f(40, @max(ext.y1 - ext.y0, 1) * 1.1);
    try o.f(41, @max((ext.x1 - ext.x0) / @max(ext.y1 - ext.y0, 1), 0.1));
    try o.f(42, 50);
    try o.f(43, 0);
    try o.f(44, 0);
    try o.f(50, 0);
    try o.f(51, 0);
    try o.i(71, 0);
    try o.i(72, 100);
    try o.i(73, 1);
    try o.i(74, 3);
    try o.i(75, 0);
    try o.i(76, 0);
    try o.i(77, 0);
    try o.i(78, 0);
    try o.s(0, "ENDTAB");

    // LTYPE
    var lts: std.ArrayList(struct { name: []const u8, pen: style_mod.Pen }) = .empty;
    for (st.pens) |p| if (p.dash_mm != null) {
        // only linetypes actually used
        var used = false;
        for (d.items) |it| if (it == .path and std.mem.eql(u8, it.path.pen, p.name)) {
            used = true;
            break;
        };
        if (used or std.mem.eql(u8, p.name, "hidden")) try lts.append(a, .{ .name = try linetypeName(a, p.name), .pen = p });
    };
    try o.s(0, "TABLE");
    try o.s(2, "LTYPE");
    try o.s(5, "5");
    try o.s(330, "0");
    try o.s(100, "AcDbSymbolTable");
    try o.i(70, @intCast(3 + lts.items.len));
    const fixed_lts = [_][3][]const u8{ .{ "14", "BYBLOCK", "" }, .{ "15", "BYLAYER", "" }, .{ "16", "CONTINUOUS", "Solid line" } };
    for (fixed_lts) |lt| {
        try o.s(0, "LTYPE");
        try o.s(5, lt[0]);
        try o.s(330, "5");
        try o.s(100, "AcDbSymbolTableRecord");
        try o.s(100, "AcDbLinetypeTableRecord");
        try o.s(2, lt[1]);
        try o.i(70, 0);
        try o.s(3, lt[2]);
        try o.i(72, 65);
        try o.i(73, 0);
        try o.f(40, 0);
    }
    for (lts.items, 0..) |lt, idx| {
        try o.s(0, "LTYPE");
        try o.code(5, "{X}", .{0x40 + idx});
        try o.s(330, "5");
        try o.s(100, "AcDbSymbolTableRecord");
        try o.s(100, "AcDbLinetypeTableRecord");
        try o.s(2, lt.name);
        try o.i(70, 0);
        try o.s(3, lt.name);
        try o.i(72, 65);
        const dm = lt.pen.dash_mm.?;
        try o.i(73, @intCast(dm.len));
        var total: f64 = 0;
        for (dm) |x| total += x / 25.4 * d.scale;
        try o.f(40, total);
        for (dm, 0..) |x, k| {
            const len = x / 25.4 * d.scale;
            try o.f(49, if (k % 2 == 0) len else -len);
            try o.i(74, 0);
        }
    }
    try o.s(0, "ENDTAB");

    // LAYER
    try o.s(0, "TABLE");
    try o.s(2, "LAYER");
    try o.s(5, "2");
    try o.s(330, "0");
    try o.s(100, "AcDbSymbolTable");
    try o.i(70, @intCast(1 + d.layers.len));
    try o.s(0, "LAYER");
    try o.s(5, "10");
    try o.s(330, "2");
    try o.s(100, "AcDbSymbolTableRecord");
    try o.s(100, "AcDbLayerTableRecord");
    try o.s(2, "0");
    try o.i(70, 0);
    try o.i(62, 7);
    try o.s(6, "CONTINUOUS");
    try o.i(370, -3);
    for (d.layers, 0..) |l, idx| {
        try o.s(0, "LAYER");
        try o.code(5, "{X}", .{0x50 + idx});
        try o.s(330, "2");
        try o.s(100, "AcDbSymbolTableRecord");
        try o.s(100, "AcDbLayerTableRecord");
        try o.s(2, l.name);
        try o.i(70, 0);
        try o.i(62, 7);
        try o.s(6, if (std.mem.eql(u8, l.linetype, "DASHED")) "DASHED" else "CONTINUOUS");
        try o.i(370, lineweightFor(l.lineweight_mm));
    }
    try o.s(0, "ENDTAB");

    // STYLE
    try o.s(0, "TABLE");
    try o.s(2, "STYLE");
    try o.s(5, "3");
    try o.s(330, "0");
    try o.s(100, "AcDbSymbolTable");
    try o.i(70, 2);
    const styles = [2][3][]const u8{ .{ "11", "STANDARD", "txt" }, .{ "12", st.dxf_style, st.dxf_font } };
    for (styles) |sy| {
        try o.s(0, "STYLE");
        try o.s(5, sy[0]);
        try o.s(330, "3");
        try o.s(100, "AcDbSymbolTableRecord");
        try o.s(100, "AcDbTextStyleTableRecord");
        try o.s(2, sy[1]);
        try o.i(70, 0);
        try o.f(40, 0);
        try o.f(41, 1);
        try o.f(50, 0);
        try o.i(71, 0);
        try o.f(42, 0.2);
        try o.s(3, sy[2]);
        try o.s(4, "");
    }
    try o.s(0, "ENDTAB");

    // VIEW, UCS (empty)
    try o.s(0, "TABLE");
    try o.s(2, "VIEW");
    try o.s(5, "6");
    try o.s(330, "0");
    try o.s(100, "AcDbSymbolTable");
    try o.i(70, 0);
    try o.s(0, "ENDTAB");
    try o.s(0, "TABLE");
    try o.s(2, "UCS");
    try o.s(5, "7");
    try o.s(330, "0");
    try o.s(100, "AcDbSymbolTable");
    try o.i(70, 0);
    try o.s(0, "ENDTAB");
    // APPID
    try o.s(0, "TABLE");
    try o.s(2, "APPID");
    try o.s(5, "9");
    try o.s(330, "0");
    try o.s(100, "AcDbSymbolTable");
    try o.i(70, 1);
    try o.s(0, "APPID");
    try o.s(5, "13");
    try o.s(330, "9");
    try o.s(100, "AcDbSymbolTableRecord");
    try o.s(100, "AcDbRegAppTableRecord");
    try o.s(2, "ACAD");
    try o.i(70, 0);
    try o.s(0, "ENDTAB");
    // DIMSTYLE
    try o.s(0, "TABLE");
    try o.s(2, "DIMSTYLE");
    try o.s(5, "A");
    try o.s(330, "0");
    try o.s(100, "AcDbSymbolTable");
    try o.i(70, 1);
    try o.s(100, "AcDbDimStyleTable");
    try o.i(71, 1);
    try o.s(0, "DIMSTYLE");
    try o.s(105, "17");
    try o.s(330, "A");
    try o.s(100, "AcDbSymbolTableRecord");
    try o.s(100, "AcDbDimStyleTableRecord");
    try o.s(2, "STANDARD");
    try o.i(70, 0);
    try o.i(340, 0x11);
    try o.s(0, "ENDTAB");
    // BLOCK_RECORD
    try o.s(0, "TABLE");
    try o.s(2, "BLOCK_RECORD");
    try o.s(5, "1");
    try o.s(330, "0");
    try o.s(100, "AcDbSymbolTable");
    try o.i(70, 2);
    const brs = [2][2][]const u8{ .{ model_space_handle, "*Model_Space" }, .{ paper_space_handle, "*Paper_Space" } };
    for (brs) |br| {
        try o.s(0, "BLOCK_RECORD");
        try o.s(5, br[0]);
        try o.s(330, "1");
        try o.s(100, "AcDbSymbolTableRecord");
        try o.s(100, "AcDbBlockTableRecord");
        try o.s(2, br[1]);
        try o.i(70, 0);
        try o.i(280, 1);
        try o.i(281, 0);
    }
    try o.s(0, "ENDTAB");
    try o.s(0, "ENDSEC");

    // BLOCKS
    try o.s(0, "SECTION");
    try o.s(2, "BLOCKS");
    const blocks = [2][4][]const u8{
        .{ "20", "21", model_space_handle, "*Model_Space" },
        .{ "22", "23", paper_space_handle, "*Paper_Space" },
    };
    for (blocks, 0..) |b, bi| {
        try o.s(0, "BLOCK");
        try o.s(5, b[0]);
        try o.s(330, b[2]);
        try o.s(100, "AcDbEntity");
        if (bi == 1) try o.i(67, 1);
        try o.s(8, "0");
        try o.s(100, "AcDbBlockBegin");
        try o.s(2, b[3]);
        try o.i(70, 0);
        try o.f(10, 0);
        try o.f(20, 0);
        try o.f(30, 0);
        try o.s(3, b[3]);
        try o.s(1, "");
        try o.s(0, "ENDBLK");
        try o.s(5, b[1]);
        try o.s(330, b[2]);
        try o.s(100, "AcDbEntity");
        if (bi == 1) try o.i(67, 1);
        try o.s(8, "0");
        try o.s(100, "AcDbBlockEnd");
    }
    try o.s(0, "ENDSEC");

    // ENTITIES
    try o.s(0, "SECTION");
    try o.s(2, "ENTITIES");
    try o.out.appendSlice(a, entities);
    try o.s(0, "ENDSEC");

    // OBJECTS
    try o.s(0, "SECTION");
    try o.s(2, "OBJECTS");
    try o.s(0, "DICTIONARY");
    try o.s(5, "C");
    try o.s(330, "0");
    try o.s(100, "AcDbDictionary");
    try o.i(281, 1);
    try o.s(3, "ACAD_GROUP");
    try o.s(350, "D");
    try o.s(0, "DICTIONARY");
    try o.s(5, "D");
    try o.s(330, "C");
    try o.s(100, "AcDbDictionary");
    try o.i(281, 1);
    try o.s(0, "ENDSEC");
    try o.s(0, "EOF");
    return o.out.items;
}
