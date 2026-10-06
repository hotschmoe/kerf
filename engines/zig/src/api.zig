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
const mesh_mod = @import("mesh.zig");
const sheet_mod = @import("sheet.zig");
const dxf_mod = @import("dxf.zig");
const pdf_mod = @import("pdf.zig");
const raster = @import("raster.zig");
const load_mod = @import("load.zig");
const ops_mod = @import("ops.zig");
const lint = @import("lint.zig");
pub const schema = @import("schema.zig");
const limits = @import("limits.zig");
const oom = @import("oom.zig");
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

pub const fn_names = [_][]const u8{ "version", "catalog", "schema", "fmt", "check", "apply", "inspect", "drawing", "mesh", "export", "help" };

/// `kerf call help`: every function with its input and output shape (one JSON object per line).
const help_rows = [_][3][]const u8{
    .{ "version", "{}", "{engine, version, spec}" },
    .{ "help", "{}", "this list" },
    .{ "catalog", "{format?: \"json\"|\"markdown\"}", "the component catalog (json default)" },
    .{ "schema", "{topic?: \"view\"|\"note\"|\"dim\"|\"label\"|\"cite\"|\"ops\"|\"doc\"|<component type>}", "{topic, text}: field reference text; no topic lists the topics" },
    .{ "fmt", "{doc}", "{doc, text}: canonical key order and indentation" },
    .{ "check", "{doc, style?}", "{diagnostics: [{level, code, id?, path?, message, fix?}], summary}" },
    .{ "apply", "{doc, ops: [op...], style?, actor?: \"llm\"|\"designer\"}", "{ok, doc, diagnostics, summary, changed}: ok=false leaves doc unchanged (see diagnostics); `kerf schema ops` for op shapes" },
    .{ "inspect", "{doc, style?, query: {q: \"summary\"|\"component\"|\"anchors\"|\"at\"|\"catalog\"|\"doc\", id?, view?, point?: [x,y], type?}}", "query-specific JSON (anchors with coordinates, parts, what is visible at a point)" },
    .{ "drawing", "{doc, view, style?}", "Drawing IR JSON (SPEC 10) of one view" },
    .{ "mesh", "{doc, style?, include_fills?: bool}", "mesh JSON for 3D viewers" },
    .{ "export", "{doc, view, format: \"svg\"|\"png\"|\"pdf\"|\"dxf\", sheet?: bool, px?: number, style?}", "the file bytes (svg/dxf text, png/pdf binary)" },
};

fn helpFn(a: Allocator) ApiError!Out {
    var out: std.ArrayList(u8) = .empty;
    try out.appendSlice(a, "{\"functions\":[\n");
    for (help_rows, 0..) |row, i| {
        try out.appendSlice(a, "{\"name\":");
        try json.writeString(&out, a, row[0]);
        try out.appendSlice(a, ",\"input\":");
        try json.writeString(&out, a, row[1]);
        try out.appendSlice(a, ",\"output\":");
        try json.writeString(&out, a, row[2]);
        try out.appendSlice(a, if (i + 1 < help_rows.len) "},\n" else "}\n");
    }
    try out.appendSlice(a, "],\"note\":\"input is one JSON object on stdin; doc is the document object; a failure is {error: {code, message}} with exit 1\"}\n");
    return .{ .ok = true, .bytes = out.items };
}

fn schemaFn(a: Allocator, inp: json.Value) ApiError!Out {
    const topic: ?[]const u8 = if (inp.get("topic")) |t| t.str() else null;
    var out: std.ArrayList(u8) = .empty;
    const text: []const u8 = if (topic) |tp| (try schema.render(a, tp)) orelse {
        var names: std.ArrayList([]const u8) = .empty;
        for (schema.objects) |o| try names.append(a, o.name);
        for (catalog.entries) |e| try names.append(a, e.name());
        const hint = if (model.nearest(a, tp, names.items)) |n| try std.fmt.allocPrint(a, " Did you mean \"{s}\"?", .{n}) else "";
        return fail(a, "E_TOPIC", "unknown schema topic \"{s}\".{s} Topics: {s}", .{ tp, hint, schema.topics_hint });
    } else try schema.index(a);
    try out.appendSlice(a, "{\"topic\":");
    try json.writeString(&out, a, topic orelse "");
    try out.appendSlice(a, ",\"text\":");
    try json.writeString(&out, a, text);
    try out.appendSlice(a, "}\n");
    return .{ .ok = true, .bytes = out.items };
}

/// Run API function `name` on `input` (UTF-8 JSON). Output is allocated from `gpa`.
pub fn call(gpa: Allocator, name: []const u8, input: []const u8) ApiError!Result {
    // The sensor notices an allocation that was refused anywhere below, including sites that swallow the error to build a
    // message (`Diags.add` ...): such a call must fail with OutOfMemory instead of returning a result with a diagnostic missing.
    var sensor = oom.Sensor.init(gpa);
    var arena = std.heap.ArenaAllocator.init(sensor.allocator());
    defer arena.deinit();
    const a = arena.allocator();
    const r = try dispatch(a, name, input);
    if (sensor.failed) return error.OutOfMemory;
    return .{ .ok = r.ok, .bytes = try gpa.dupe(u8, r.bytes) };
}

/// The API functions callable through `call`; the name is the wire name. Adding a member without a `switch` arm in `dispatch`
/// is a compile error, and the "Functions: ..." list of the E_FN message is generated from the tags.
pub const Fn = enum { version, help, catalog, schema, fmt, check, apply, inspect, drawing, mesh, @"export" };

const fn_list = blk: {
    var list: []const u8 = "";
    for (std.meta.fieldNames(Fn), 0..) |n, i| list = list ++ (if (i > 0) ", " else "") ++ n;
    break :blk list;
};

fn dispatch(a: Allocator, name: []const u8, input: []const u8) ApiError!Out {
    if (input.len > limits.max_json_bytes) return fail(a, "E_LIMIT", "{s}", .{try limits.message(a, "input too large (bytes)", input.len, limits.max_json_bytes, "Split the detail into several documents (one sheet per document) or drop unused components and views.")});
    var perr: json.ParseError = undefined;
    const trimmed = std.mem.trim(u8, input, " \t\r\n");
    const inp: json.Value = if (trimmed.len == 0) .{ .object = &.{} } else (try json.parse(a, input, &perr)) orelse
        return fail(a, "E_JSON", "input is not valid JSON: {s} (line {d}, column {d})", .{ perr.msg, perr.line, perr.col });
    if (inp != .object) return fail(a, "E_INPUT", "input must be a JSON object", .{});

    const f = std.meta.stringToEnum(Fn, name) orelse
        return fail(a, "E_FN", "unknown function '{s}'. Functions: " ++ fn_list ++ " (`kerf call help` lists their input shapes)", .{name});
    return switch (f) {
        .version => versionFn(a),
        .help => helpFn(a),
        .catalog => catalogFn(a, inp),
        .schema => schemaFn(a, inp),
        .fmt => fmtFn(a, inp),
        .check => checkFn(a, inp),
        .apply => applyFn(a, inp),
        .inspect => inspectFn(a, inp),
        .drawing => drawingFn(a, inp),
        .mesh => meshFn(a, inp),
        .@"export" => exportFn(a, inp),
    };
}

fn versionFn(a: Allocator) ApiError!Out {
    return .{ .ok = true, .bytes = try std.fmt.allocPrint(a, "{{\"engine\":\"{s}\",\"version\":\"{s}\",\"spec\":\"0.1\"}}\n", .{ engine_name, version }) };
}

fn catalogFn(a: Allocator, inp: json.Value) ApiError!Out {
    const fmt_v = if (inp.get("format")) |f| (f.str() orelse "json") else "json";
    if (std.mem.eql(u8, fmt_v, "markdown") or std.mem.eql(u8, fmt_v, "md")) {
        return .{ .ok = true, .bytes = try catalog.catalogMarkdown(a) };
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
    var why: []const u8 = "";
    const st = style_mod.loadWhy(a, user, &why) catch |e| switch (e) {
        error.OutOfMemory => return error.OutOfMemory,
        error.BadStyle => return .{ .err = if (why.len > 0)
            try fail(a, "E_STYLE", "the style has invalid values: {s}. Fix: remove those keys to use the defaults (shown above), or give values inside the stated ranges.", .{why})
        else
            try fail(a, "E_STYLE", "the style is not a valid kerfstyle (needs pens and materials objects)", .{}) },
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
    const dr = (try drawview.build(a, try lint.withDimDirsFromDoc(a, d, &st), &st, view_id, &diags)) orelse {
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
    const dr = (try drawview.build(a, try lint.withDimDirsFromDoc(a, d, &st), &st, view_id, &diags)) orelse {
        return fail(a, "E_VIEW", "{s}", .{if (diags.list.items.len > 0) diags.list.items[0].message else "view could not be built"});
    };
    const font = font_mod.Font.parse(a, font_mod.embedded) catch return fail(a, "E_INTERNAL", "embedded font failed to parse", .{});
    const want_sheet = if (inp.get("sheet")) |sv| (sv == .bool and sv.bool) else false;
    if (std.mem.eql(u8, format, "svg")) {
        const dd = if (want_sheet) try sheet_mod.withSheet(a, dr, &font) else dr;
        return .{ .ok = true, .bytes = try svg.render(a, &dd, &font, .{}) };
    }
    if (std.mem.eql(u8, format, "pdf")) {
        const dd = try sheet_mod.withSheet(a, dr, &font);
        return .{ .ok = true, .bytes = try pdf_mod.render(a, &dd, &font) };
    }
    if (std.mem.eql(u8, format, "dxf")) {
        const dd = if (want_sheet) try sheet_mod.withSheet(a, dr, &font) else dr;
        return .{ .ok = true, .bytes = try dxf_mod.render(a, &dd) };
    }
    if (std.mem.eql(u8, format, "png")) {
        var opt = raster.Options{};
        if (inp.get("px")) |pv| {
            const n = pv.num() orelse return fail(a, "E_INPUT", "export \"px\" must be a number (output width in pixels, {d}-{d}, default {d})", .{ raster.min_px, raster.max_px, raster.default_px });
            opt.px = @intFromFloat(std.math.clamp(@round(if (n == n) n else 1600), @as(f64, raster.min_px), @as(f64, raster.max_px)));
        }
        const dd = if (want_sheet) try sheet_mod.withSheet(a, dr, &font) else dr;
        return .{ .ok = true, .bytes = raster.render(a, &dd, &font, opt) catch |e| switch (e) {
            error.OutOfMemory => return error.OutOfMemory,
            else => return fail(a, "E_INTERNAL", "png encoding failed ({s})", .{@errorName(e)}),
        } };
    }
    return fail(a, "E_INPUT", "export format must be \"svg\", \"dxf\", \"pdf\" or \"png\" (got \"{s}\")", .{format});
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
    const ops = inp.get("ops") orelse return fail(a, "E_INPUT", "missing \"ops\": an array of ops (see SPEC 14)", .{});
    const actor: ops_mod.Actor = if (inp.get("actor")) |x| (if (x.str()) |s| (if (std.mem.eql(u8, s, "designer")) .designer else .llm) else .llm) else .llm;
    var op_diags = model.Diags.init(a);
    const applied = try ops_mod.apply(a, d, ops, actor, &op_diags);
    var ok = false;
    var final_doc = d;
    var changed: []const []const u8 = &.{};
    var all: std.ArrayList(model.Diag) = .empty;
    var summary_text: []const u8 = "";
    if (applied) |ap| {
        const prior = try load_mod.load(a, d, &st, false);
        const l = try load_mod.loadAfterEdit(a, ap.doc, &st, true, &prior);
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
            summary_text = try load_mod.summary(&prior);
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

test "api: every function runs on the reference documents without leaking" {
    const testdocs = @import("testdocs.zig");
    const gpa = std.testing.allocator;
    for (testdocs.all) |doc| {
        const trimmed = std.mem.trim(u8, doc, " \n\r\t");
        inline for (.{ "check", "inspect" }) |fname| {
            const input = try std.fmt.allocPrint(gpa, "{{\"doc\":{s}}}", .{trimmed});
            defer gpa.free(input);
            const r = try call(gpa, fname, input);
            defer gpa.free(r.bytes);
            try std.testing.expect(r.ok);
        }
        for ([_][]const u8{"A"}) |v| {
            for ([_][]const u8{ "drawing", "export" }) |fname| {
                const input = try std.fmt.allocPrint(gpa, "{{\"doc\":{s},\"view\":\"{s}\",\"format\":\"svg\"}}", .{ trimmed, v });
                defer gpa.free(input);
                const r = try call(gpa, fname, input);
                defer gpa.free(r.bytes);
                try std.testing.expect(r.ok);
            }
        }
        const finput = try std.fmt.allocPrint(gpa, "{{\"doc\":{s}}}", .{trimmed});
        defer gpa.free(finput);
        const r = try call(gpa, "fmt", finput);
        gpa.free(r.bytes);
    }
    const bad = try call(gpa, "nope", "{}");
    defer gpa.free(bad.bytes);
    try std.testing.expect(!bad.ok);
}

fn meshFn(a: Allocator, inp: json.Value) ApiError!Out {
    const d = switch (try getDoc(a, inp)) {
        .doc => |x| x,
        .err => |e| return e,
    };
    const st = switch (try getStyle(a, inp)) {
        .style => |x| x,
        .err => |e| return e,
    };
    const l = try load_mod.load(a, d, &st, false);
    const incl = if (inp.get("include_fills")) |x| (x == .bool and x.bool) else false;
    const parts = try mesh_mod.build(a, l.scene, incl);
    return .{ .ok = true, .bytes = try mesh_mod.toJson(a, parts) };
}

test {
    _ = @import("ergo_tests.zig");
}
