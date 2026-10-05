//! The in-process Zig engine behind the `Engine` boundary.

const std = @import("std");
const kerf = @import("kerf");
const eng = @import("engine.zig");
const Allocator = std.mem.Allocator;

fn call(_: *anyopaque, a: Allocator, name: []const u8, input: []const u8) eng.CallResult {
    if (std.mem.eql(u8, name, "sheet_drawing")) return sheetDrawing(a, input) catch |e| errMsg(a, @errorName(e));
    const r = kerf.call(a, name, input) catch |e| return errMsg(a, @errorName(e));
    return if (r.ok) .{ .ok = r.bytes } else .{ .err = r.bytes };
}

fn errMsg(a: Allocator, msg: []const u8) eng.CallResult {
    return .{ .err = a.dupe(u8, msg) catch &.{} };
}

/// App-level extension (zig engine only): the Drawing IR of a view WITH its
/// sheet frame and title block, i.e. exactly what the PDF page contains.
fn sheetDrawing(a: Allocator, input: []const u8) !eng.CallResult {
    var arena = std.heap.ArenaAllocator.init(a);
    defer arena.deinit();
    const t = arena.allocator();
    var perr: kerf.json.ParseError = undefined;
    const inp = (try kerf.json.parse(t, input, &perr)) orelse return errMsg(a, "bad json");
    const doc = inp.get("doc") orelse return errMsg(a, "missing doc");
    const st = try kerf.style.load(t, if (inp.get("style")) |s| (if (s == .object) s else null) else null);
    const view_id = (if (inp.get("view")) |v| v.str() else null) orelse return errMsg(a, "missing view");
    var diags = kerf.model.Diags.init(t);
    const dr = (try kerf.drawview.build(t, doc, &st, view_id, &diags)) orelse return errMsg(a, "view could not be built");
    const font = try kerf.font.Font.parse(t, kerf.font.embedded);
    const dd = try kerf.sheet.withSheet(t, dr, &font);
    const json = try kerf.drawing.toJson(t, &dd);
    return .{ .ok = try a.dupe(u8, json) };
}

var ctx_dummy: u8 = 0;

pub fn engine() eng.Engine {
    return .{ .ctx = &ctx_dummy, .call_fn = call, .name = "kerf-zig" };
}
