//! The engine API (SPEC 13): `call(fn, input_json) -> output`, identical for CLI, wasm and
//! in-process callers.

const std = @import("std");
const json = @import("json.zig");
const model = @import("model.zig");
const style_mod = @import("style.zig");
const catalog = @import("catalog.zig");
const canon = @import("canon.zig");
const drawview = @import("drawview.zig");
const drawing = @import("drawing.zig");
const font_mod = @import("font.zig");
const svg = @import("svg.zig");
const load_mod = @import("load.zig");
const ops_mod = @import("ops.zig");
const Allocator = std.mem.Allocator;

pub const version = "0.1.0";
pub const engine_name = "kerf-zig";

pub const Result = struct {
    /// false => `bytes` is an error JSON (kerf_call status 1).
    ok: bool,
    /// Owned by the caller's allocator.
    bytes: []u8,
};

const ApiError = Allocator.Error;

fn errJson(a: Allocator, code: []const u8, comptime fmt: []const u8, args: anytype) ApiError![]u8 {
    const msg = try std.fmt.allocPrint(a, fmt, args);
    var out: std.ArrayList(u8) = .empty;
    try out.appendSlice(a, "{\"error\":{\"code\":");
    try json.writeString(&out, a, code);
    try out.appendSlice(a, ",\"message\":");
    try json.writeString(&out, a, msg);
    try out.appendSlice(a, "}}\n");
    return out.items;
}

const Out = struct { ok: bool, bytes: []const u8 };

fn fail(a: Allocator, code: []const u8, comptime fmt: []const u8, args: anytype) ApiError!Out {
    return .{ .ok = false, .bytes = try errJson(a, code, fmt, args) };
}

pub const fn_names = [_][]const u8{ "version", "catalog", "fmt", "check", "apply", "inspect", "drawing", "mesh", "export" };

/// Run API function `name` on `input` (UTF-8 JSON). Output is allocated from `gpa`.
pub fn call(gpa: Allocator, name: []const u8, input: []const u8) ApiError!Result {
    var arena = std.heap.ArenaAllocator.init(gpa);
    defer arena.deinit();
    const a = arena.allocator();
    const r = try dispatch(a, name, input);
    return .{ .ok = r.ok, .bytes = try gpa.dupe(u8, r.bytes) };
}

fn dispatch(a: Allocator, name: []const u8, input: []const u8) ApiError!Out {
    var perr: json.ParseError = undefined;
    const trimmed = std.mem.trim(u8, input, " \t\r\n");
    const inp: json.Value = if (trimmed.len == 0) .{ .object = &.{} } else (try json.parse(a, input, &perr)) orelse
        return fail(a, "E_JSON", "input is not valid JSON: {s} (line {d}, column {d})", .{ perr.msg, perr.line, perr.col });
    if (inp != .object) return fail(a, "E_INPUT", "input must be a JSON object", .{});

    if (std.mem.eql(u8, name, "version")) return versionFn(a);
    if (std.mem.eql(u8, name, "catalog")) return catalogFn(a, inp);
    if (std.mem.eql(u8, name, "fmt")) return fmtFn(a, inp);
    if (std.mem.eql(u8, name, "check")) return checkFn(a, inp);
    if (std.mem.eql(u8, name, "apply")) return applyFn(a, inp);
    if (std.mem.eql(u8, name, "inspect")) return inspectFn(a, inp);
    if (std.mem.eql(u8, name, "drawing")) return drawingFn(a, inp);
    if (std.mem.eql(u8, name, "export")) return exportFn(a, inp);
    return fail(a, "E_FN", "unknown function '{s}'. Functions: version, catalog, fmt, check, apply, inspect, drawing, mesh, export", .{name});
}

fn versionFn(a: Allocator) ApiError!Out {
    return .{ .ok = true, .bytes = try std.fmt.allocPrint(a, "{{\"engine\":\"{s}\",\"version\":\"{s}\",\"spec\":\"0.1\"}}\n", .{ engine_name, version }) };
}

fn catalogFn(a: Allocator, inp: json.Value) ApiError!Out {
    const fmt_v = if (inp.get("format")) |f| (f.str() orelse "json") else "json";
    if (std.mem.eql(u8, fmt_v, "markdown") or std.mem.eql(u8, fmt_v, "md")) {
        var o: std.ArrayList(u8) = .empty;
        try json.writeString(&o, a, try catalog.catalogMarkdown(a));
        try o.append(a, '\n');
        return .{ .ok = true, .bytes = o.items };
    }
    if (!std.mem.eql(u8, fmt_v, "json")) return fail(a, "E_INPUT", "catalog format must be \"json\" or \"markdown\"", .{});
    var out: std.ArrayList(u8) = .empty;
    try json.writeCompact(&out, a, try catalog.catalogJson(a));
    try out.append(a, '\n');
    return .{ .ok = true, .bytes = out.items };
}

fn getDoc(a: Allocator, inp: json.Value) ApiError!union(enum) { doc: json.Value, err: Out } {
    const d = inp.get("doc") orelse return .{ .err = try fail(a, "E_INPUT", "missing \"doc\": the Kerf document object", .{}) };
    if (d != .object) return .{ .err = try fail(a, "E_INPUT", "\"doc\" must be a JSON object (the document), not {s}", .{d.kindName()}) };
    return .{ .doc = d };
}

fn getStyle(a: Allocator, inp: json.Value) ApiError!union(enum) { style: style_mod.Style, err: Out } {
    const sv = inp.get("style");
    const user: ?json.Value = if (sv) |s| (if (s == .object) s else null) else null;
    const st = style_mod.load(a, user) catch |e| switch (e) {
        error.OutOfMemory => return error.OutOfMemory,
        error.BadStyle => return .{ .err = try fail(a, "E_STYLE", "the style is not a valid kerfstyle (needs pens and materials objects)", .{}) },
    };
    return .{ .style = st };
}

fn fmtFn(a: Allocator, inp: json.Value) ApiError!Out {
    const d = switch (try getDoc(a, inp)) {
        .doc => |x| x,
        .err => |e| return e,
    };
    const text = try canon.write(a, d);
    // {doc} wrapper
    var out: std.ArrayList(u8) = .empty;
    try out.appendSlice(a, "{\"doc\":");
    try out.appendSlice(a, std.mem.trimEnd(u8, text, "\n"));
    try out.appendSlice(a, ",\"text\":");
    try json.writeString(&out, a, text);
    try out.appendSlice(a, "}\n");
    return .{ .ok = true, .bytes = out.items };
}

fn drawingFn(a: Allocator, inp: json.Value) ApiError!Out {
    const d = switch (try getDoc(a, inp)) {
        .doc => |x| x,
        .err => |e| return e,
    };
    const st = switch (try getStyle(a, inp)) {
        .style => |x| x,
        .err => |e| return e,
    };
    const view_id = (if (inp.get("view")) |v| v.str() else null) orelse return fail(a, "E_INPUT", "missing \"view\": the id of a view in the document", .{});
    var diags = model.Diags.init(a);
    const dr = (try drawview.build(a, d, &st, view_id, &diags)) orelse {
        return fail(a, "E_VIEW", "{s}", .{if (diags.list.items.len > 0) diags.list.items[0].message else "view could not be built"});
    };
    return .{ .ok = true, .bytes = try drawing.toJson(a, &dr) };
}

fn exportFn(a: Allocator, inp: json.Value) ApiError!Out {
    const d = switch (try getDoc(a, inp)) {
        .doc => |x| x,
        .err => |e| return e,
    };
    const st = switch (try getStyle(a, inp)) {
        .style => |x| x,
        .err => |e| return e,
    };
    const view_id = (if (inp.get("view")) |v| v.str() else null) orelse return fail(a, "E_INPUT", "missing \"view\": the id of a view in the document", .{});
    const format = (if (inp.get("format")) |v| v.str() else null) orelse "svg";
    var diags = model.Diags.init(a);
    const dr = (try drawview.build(a, d, &st, view_id, &diags)) orelse {
        return fail(a, "E_VIEW", "{s}", .{if (diags.list.items.len > 0) diags.list.items[0].message else "view could not be built"});
    };
    const font = font_mod.Font.parse(a, font_mod.embedded) catch return fail(a, "E_INTERNAL", "embedded font failed to parse", .{});
    if (std.mem.eql(u8, format, "svg")) return .{ .ok = true, .bytes = try svg.render(a, &dr, &font, .{}) };
    return fail(a, "E_INPUT", "export format must be \"svg\", \"dxf\" or \"pdf\" (got \"{s}\")", .{format});
}

fn checkFn(a: Allocator, inp: json.Value) ApiError!Out {
    const d = switch (try getDoc(a, inp)) {
        .doc => |x| x,
        .err => |e| return e,
    };
    const st = switch (try getStyle(a, inp)) {
        .style => |x| x,
        .err => |e| return e,
    };
    const l = try load_mod.load(a, d, &st, true);
    var out: std.ArrayList(u8) = .empty;
    try out.appendSlice(a, "{\"diagnostics\":");
    try json.writeCompact(&out, a, try load_mod.diagsJson(a, l.diags.list.items));
    try out.appendSlice(a, ",\"summary\":");
    try json.writeString(&out, a, try load_mod.summary(&l));
    try out.appendSlice(a, "}\n");
    return .{ .ok = true, .bytes = out.items };
}

fn inspectFn(a: Allocator, inp: json.Value) ApiError!Out {
    const d = switch (try getDoc(a, inp)) {
        .doc => |x| x,
        .err => |e| return e,
    };
    const st = switch (try getStyle(a, inp)) {
        .style => |x| x,
        .err => |e| return e,
    };
    const l = try load_mod.load(a, d, &st, false);
    const q = inp.get("query") orelse json.Value{ .object = &.{} };
    var ie: load_mod.InspectError = undefined;
    const r = (try load_mod.inspect(&l, q, &ie)) orelse return fail(a, ie.code, "{s}", .{ie.message});
    var out: std.ArrayList(u8) = .empty;
    try json.writeCompact(&out, a, r);
    try out.append(a, '\n');
    return .{ .ok = true, .bytes = out.items };
}

fn applyFn(a: Allocator, inp: json.Value) ApiError!Out {
    const d = switch (try getDoc(a, inp)) {
        .doc => |x| x,
        .err => |e| return e,
    };
    const st = switch (try getStyle(a, inp)) {
        .style => |x| x,
        .err => |e| return e,
    };
    var ops = inp.get("ops") orelse return fail(a, "E_INPUT", "missing \"ops\": an array of ops (see SPEC 14)", .{});
    if (ops == .object) if (ops.get("ops")) |inner| {
        ops = inner;
    };
    const actor: ops_mod.Actor = if (inp.get("actor")) |x| (if (x.str()) |s| (if (std.mem.eql(u8, s, "designer")) .designer else .llm) else .llm) else .llm;
    var op_diags = model.Diags.init(a);
    const applied = try ops_mod.apply(a, d, ops, actor, &op_diags);
    var ok = false;
    var final_doc = d;
    var changed: []const []const u8 = &.{};
    var all: std.ArrayList(model.Diag) = .empty;
    var summary_text: []const u8 = "";
    if (applied) |ap| {
        const l = try load_mod.load(a, ap.doc, &st, true);
        try all.appendSlice(a, op_diags.list.items);
        try all.appendSlice(a, l.diags.list.items);
        var errs: usize = 0;
        for (all.items) |x| if (x.level == .@"error") {
            errs += 1;
        };
        if (errs == 0) {
            ok = true;
            final_doc = ap.doc;
            changed = ap.changed;
            const dg = try a.create(model.Diags);
            dg.* = .{ .a = a, .list = all };
            summary_text = try load_mod.summary(&.{ .a = a, .doc = l.doc, .style = l.style, .scene = l.scene, .diags = dg, .nviews = l.nviews });
        } else {
            const before = try load_mod.load(a, d, &st, false);
            summary_text = try load_mod.summary(&before);
        }
    } else {
        try all.appendSlice(a, op_diags.list.items);
        const before = try load_mod.load(a, d, &st, false);
        summary_text = try load_mod.summary(&before);
    }
    var out: std.ArrayList(u8) = .empty;
    try out.appendSlice(a, if (ok) "{\"ok\":true,\"doc\":" else "{\"ok\":false,\"doc\":");
    const text = try canon.write(a, final_doc);
    try out.appendSlice(a, std.mem.trimEnd(u8, text, "\n"));
    try out.appendSlice(a, ",\"diagnostics\":");
    try json.writeCompact(&out, a, try load_mod.diagsJson(a, all.items));
    try out.appendSlice(a, ",\"summary\":");
    try json.writeString(&out, a, summary_text);
    try out.appendSlice(a, ",\"changed\":[");
    for (changed, 0..) |c, i| {
        if (i > 0) try out.append(a, ',');
        try json.writeString(&out, a, c);
    }
    try out.appendSlice(a, "]}\n");
    return .{ .ok = true, .bytes = out.items };
}
