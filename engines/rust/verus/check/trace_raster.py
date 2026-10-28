#!/usr/bin/env python3
"""Instruments a copy of engines/zig/src/raster.zig (never the repo's) so `kerf export --format png`
appends, after the PNG, a trace of every canvas call and the final pixels; see src/raster.rs."""
import sys

path = sys.argv[1]
s = open(path).read()
edits = [
    ("const sub_scans: usize = 8;", """const sub_scans: usize = 8;

var trace: std.ArrayList(u8) = .empty;
var trace_a: ?Allocator = null;
fn tr(bytes: []const u8) void {
    if (trace_a) |a| trace.appendSlice(a, bytes) catch {};
}
fn trU32(x: usize) void {
    var b: [4]u8 = undefined;
    std.mem.writeInt(u32, &b, @intCast(x), .little);
    tr(&b);
}
fn trF64(x: f64) void {
    var b: [8]u8 = undefined;
    std.mem.writeInt(u64, &b, @bitCast(x), .little);
    tr(&b);
}"""),
    ("    fn flush(c: *Canvas) void {", "    fn flush(c: *Canvas) void {\n        tr(\"U\");"),
    ("""    fn strokeFlat(c: *Canvas, r: f64, dash: ?[]const f64) void {
        const p = c.flat.items;""", """    fn strokeFlat(c: *Canvas, r: f64, dash: ?[]const f64) void {
        const p = c.flat.items;
        tr("S");
        trF64(r);
        if (dash) |dd| {
            tr("\\x01");
            trU32(dd.len);
            for (dd) |v| trF64(v);
        } else tr("\\x00");
        trU32(p.len);
        for (p) |q| {
            trF64(q.x);
            trF64(q.y);
        }"""),
    ("""    fn fillEdges(c: *Canvas, a: Allocator) Allocator.Error!void {
        const es = c.edges.items;""", """    fn fillEdges(c: *Canvas, a: Allocator) Allocator.Error!void {
        const es = c.edges.items;
        tr("F");
        trU32(es.len);
        for (es) |e| {
            trF64(e.x0);
            trF64(e.y0);
            trF64(e.x1);
            trF64(e.y1);
        }"""),
    ("""    const lay = layout(d, opt);
    var c = try Canvas.init(a, lay.w, lay.h);""", """    const lay = layout(d, opt);
    trace = .empty;
    trace_a = a;
    tr("N");
    trU32(lay.w);
    trU32(lay.h);
    var c = try Canvas.init(a, lay.w, lay.h);"""),
    ("""    const img = try rasterize(a, d, font, opt);
    return png.encode(a, img.width, img.height, .gray, img.pixels);""", """    const img = try rasterize(a, d, font, opt);
    const enc = try png.encode(a, img.width, img.height, .gray, img.pixels);
    var out: std.ArrayList(u8) = .empty;
    try out.appendSlice(a, enc);
    try out.appendSlice(a, "KVTRACE1");
    try out.appendSlice(a, trace.items);
    try out.appendSlice(a, "P");
    try out.appendSlice(a, img.pixels);
    trace_a = null;
    return out.items;"""),
]
for old, new in edits:
    if old not in s:
        sys.exit(f"trace_raster.py: raster.zig changed, cannot find:\n{old}")
    s = s.replace(old, new)
open(path, "w").write(s)
