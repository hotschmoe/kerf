//! Tool execution glue between Claude's `tool_use` blocks and the Kerf engine.
//!
//! The host supplies an `Engine` (fn-pointers + ctx). `executeToolUse` validates the tool input,
//! calls the engine, and returns the `ToolResult` in the HARNESS.md formats:
//!   kerf_apply   -> text `ok` / summary / diagnostics (is_error when the engine rejected the ops)
//!   kerf_inspect -> text
//!   kerf_render  -> [image/png base64 block, caption text]
//! Every failure (bad JSON, missing field, engine error, unknown tool) becomes an `is_error`
//! result with an actionable message so the model can self-correct.
//!
//! Allocation: all result memory comes from the arena `a` you pass (free it after
//! `Session.onToolResults`, which copies what it needs). The engine callbacks receive the same
//! arena and allocate their returned text/png from it.

const std = @import("std");
const Allocator = std.mem.Allocator;
const types = @import("types.zig");
const js = @import("jsonspan.zig");
const jsonw = @import("jsonw.zig");

pub const ApplyOutcome = struct {
    /// false => nothing changed (atomic failure).
    ok: bool,
    /// Engine `apply` output minus the doc: summary + diagnostics, or the error text.
    text: []const u8,
    n_err: u32 = 0,
    n_warn: u32 = 0,
};

pub const TextOutcome = struct {
    ok: bool = true,
    text: []const u8,
};

pub const RenderOutcome = struct {
    ok: bool = true,
    /// PNG file bytes (the view rasterized at ~1400 px wide, white background).
    png: []const u8 = "",
    /// e.g. `view A rendered at 1-1/2"=1'-0"; 9 notes; 0 errors`. Error text when !ok.
    caption: []const u8 = "",
};

pub const Engine = struct {
    ctx: *anyopaque,
    apply_fn: *const fn (ctx: *anyopaque, a: Allocator, ops_json: []const u8, why: []const u8) ApplyOutcome,
    inspect_fn: *const fn (ctx: *anyopaque, a: Allocator, args_json: []const u8) TextOutcome,
    render_fn: *const fn (ctx: *anyopaque, a: Allocator, view: []const u8, mode: []const u8) RenderOutcome,

    pub fn apply(self: Engine, a: Allocator, ops_json: []const u8, why: []const u8) ApplyOutcome {
        return self.apply_fn(self.ctx, a, ops_json, why);
    }
    pub fn inspect(self: Engine, a: Allocator, args_json: []const u8) TextOutcome {
        return self.inspect_fn(self.ctx, a, args_json);
    }
    pub fn render(self: Engine, a: Allocator, view: []const u8, mode: []const u8) RenderOutcome {
        return self.render_fn(self.ctx, a, view, mode);
    }
};

/// Cap on a single text result (a `doc` inspect can be huge).
pub const max_result_text: usize = 100_000;

pub const tool_names = "kerf_apply, kerf_inspect, kerf_render";

fn errResult(a: Allocator, id: []const u8, comptime fmt: []const u8, args: anytype) Allocator.Error!types.ToolResult {
    const text = try std.fmt.allocPrint(a, fmt, args);
    const content = try a.alloc(types.ContentBlock, 1);
    content[0] = .{ .text = text };
    return .{ .id = id, .is_error = true, .content = content };
}

fn textResult(a: Allocator, id: []const u8, is_error: bool, text: []const u8) Allocator.Error!types.ToolResult {
    const content = try a.alloc(types.ContentBlock, 1);
    content[0] = .{ .text = try capText(a, text) };
    return .{ .id = id, .is_error = is_error, .content = content };
}

fn capText(a: Allocator, text: []const u8) Allocator.Error![]const u8 {
    if (text.len <= max_result_text) return text;
    return std.fmt.allocPrint(a, "{s}\n[truncated: {d} more bytes. Use a narrower kerf_inspect query.]", .{ text[0..max_result_text], text.len - max_result_text });
}

pub fn executeToolUse(a: Allocator, engine: Engine, tu: types.ToolUse) Allocator.Error!types.ToolResult {
    if (!(js.valid(a, tu.input_json) catch return error.OutOfMemory) or js.trim(tu.input_json).len == 0 or js.trim(tu.input_json)[0] != '{') {
        return errResult(a, tu.id, "{s}: the tool input was not a valid JSON object. Send an object matching the tool's input_schema.", .{tu.name});
    }
    if (std.mem.eql(u8, tu.name, "kerf_apply")) return execApply(a, engine, tu);
    if (std.mem.eql(u8, tu.name, "kerf_inspect")) return execInspect(a, engine, tu);
    if (std.mem.eql(u8, tu.name, "kerf_render")) return execRender(a, engine, tu);
    return errResult(a, tu.id, "Unknown tool \"{s}\". Available tools: " ++ tool_names ++ ".", .{tu.name});
}

/// Run every tool use in order; results are in the same order.
pub fn executeAll(a: Allocator, engine: Engine, tool_uses: []const types.ToolUse) Allocator.Error![]types.ToolResult {
    const out = try a.alloc(types.ToolResult, tool_uses.len);
    for (tool_uses, 0..) |tu, i| out[i] = try executeToolUse(a, engine, tu);
    return out;
}

fn execApply(a: Allocator, engine: Engine, tu: types.ToolUse) Allocator.Error!types.ToolResult {
    const ops = (js.get(tu.input_json, "ops") catch null) orelse
        return errResult(a, tu.id, "kerf_apply: missing required field `ops` (an array of op objects such as {{\"op\":\"update\",\"path\":\"components/sill_plate\",\"value\":{{...}}}}). Nothing was changed.", .{});
    if (ops.len == 0 or ops[0] != '[')
        return errResult(a, tu.id, "kerf_apply: `ops` must be an array of op objects, got {s}. Nothing was changed.", .{kindOf(ops)});
    const n = js.arrayLen(ops) catch 0;
    if (n == 0) return errResult(a, tu.id, "kerf_apply: `ops` is empty. Send at least one op. Nothing was changed.", .{});
    var it = js.ArrIter.init(ops) catch unreachable;
    var idx: usize = 0;
    while (it.next() catch null) |op| : (idx += 1) {
        const ok_kind = blk: {
            const k = (js.getString(a, op, "op") catch null) orelse break :blk false;
            for ([_][]const u8{ "add", "update", "remove", "set" }) |name| if (std.mem.eql(u8, k, name)) break :blk true;
            break :blk false;
        };
        if (op.len == 0 or op[0] != '{' or !ok_kind)
            return errResult(a, tu.id, "kerf_apply: ops[{d}] must be an object with \"op\" one of add|update|remove|set and a \"path\". Nothing was changed.", .{idx});
        if ((js.get(op, "path") catch null) == null)
            return errResult(a, tu.id, "kerf_apply: ops[{d}] is missing \"path\". Nothing was changed.", .{idx});
    }
    const why = (js.getString(a, tu.input_json, "why") catch null) orelse "(no why given)";

    const out = engine.apply(a, ops, why);
    var text: []const u8 = out.text;
    if (out.ok) {
        if (text.len == 0) text = "ok" else if (!std.mem.startsWith(u8, text, "ok")) text = try std.fmt.allocPrint(a, "ok\n{s}", .{text});
    } else {
        text = try std.fmt.allocPrint(a, "error: nothing was changed.\n{s}", .{if (out.text.len == 0) "The engine rejected the ops without a message." else out.text});
    }
    var r = try textResult(a, tu.id, !out.ok, text);
    r.n_err = out.n_err;
    r.n_warn = out.n_warn;
    return r;
}

fn kindOf(span: []const u8) []const u8 {
    if (span.len == 0) return "nothing";
    return switch (span[0]) {
        '{' => "an object",
        '"' => "a string",
        '[' => "an array",
        else => "a scalar",
    };
}

fn execInspect(a: Allocator, engine: Engine, tu: types.ToolUse) Allocator.Error!types.ToolResult {
    const q = (js.getString(a, tu.input_json, "q") catch null) orelse
        return errResult(a, tu.id, "kerf_inspect: missing required field `q` (summary | component | anchors | at | catalog | doc).", .{});
    const known = [_][]const u8{ "summary", "component", "anchors", "at", "catalog", "doc" };
    var ok = false;
    for (known) |k| if (std.mem.eql(u8, k, q)) {
        ok = true;
    };
    if (!ok) return errResult(a, tu.id, "kerf_inspect: unknown q \"{s}\". Use one of: summary, component, anchors, at, catalog, doc.", .{q});
    const needs = if (std.mem.eql(u8, q, "component") or std.mem.eql(u8, q, "anchors")) "id" else if (std.mem.eql(u8, q, "catalog")) "type" else if (std.mem.eql(u8, q, "at")) "point" else "";
    if (needs.len != 0 and ((js.get(tu.input_json, needs) catch null) == null))
        return errResult(a, tu.id, "kerf_inspect: q \"{s}\" requires `{s}`.", .{ q, needs });
    const out = engine.inspect(a, tu.input_json);
    return textResult(a, tu.id, !out.ok, out.text);
}

fn execRender(a: Allocator, engine: Engine, tu: types.ToolUse) Allocator.Error!types.ToolResult {
    const view = (js.getString(a, tu.input_json, "view") catch null) orelse
        return errResult(a, tu.id, "kerf_render: missing required field `view` (a view id such as \"A\").", .{});
    const mode = (js.getString(a, tu.input_json, "mode") catch null) orelse "view";
    if (!std.mem.eql(u8, mode, "view") and !std.mem.eql(u8, mode, "sheet"))
        return errResult(a, tu.id, "kerf_render: mode must be \"view\" or \"sheet\", got \"{s}\".", .{mode});
    const out = engine.render(a, view, mode);
    if (!out.ok) return textResult(a, tu.id, true, if (out.caption.len == 0) "render failed" else out.caption);
    if (out.png.len == 0) return errResult(a, tu.id, "kerf_render: the engine produced no image for view \"{s}\".", .{view});
    var b = jsonw.Buf.init(a);
    try b.base64(out.png);
    const content = try a.alloc(types.ContentBlock, 2);
    content[0] = .{ .image_png_b64 = b.items() };
    content[1] = .{ .text = if (out.caption.len == 0) "rendered" else out.caption };
    return .{ .id = tu.id, .is_error = false, .content = content };
}

/// Activity-line title for a tool call, e.g. `APPLY 6 OPS`, `RENDER VIEW A`, `INSPECT COMPONENT truss`.
/// Allocated from `a`.
pub fn activityTitle(a: Allocator, name: []const u8, input_json: []const u8) Allocator.Error![]const u8 {
    if (std.mem.eql(u8, name, "kerf_apply")) {
        const ops = (js.get(input_json, "ops") catch null) orelse return "APPLY";
        const n = js.arrayLen(ops) catch 0;
        return std.fmt.allocPrint(a, "APPLY {d} {s}", .{ n, if (n == 1) "OP" else "OPS" });
    }
    if (std.mem.eql(u8, name, "kerf_render")) {
        const view = (js.getString(a, input_json, "view") catch null) orelse "?";
        const mode = (js.getString(a, input_json, "mode") catch null) orelse "view";
        return std.fmt.allocPrint(a, "RENDER {s} {s}", .{ if (std.mem.eql(u8, mode, "sheet")) "SHEET" else "VIEW", view });
    }
    if (std.mem.eql(u8, name, "kerf_inspect")) {
        const q = (js.getString(a, input_json, "q") catch null) orelse "?";
        const up = try a.dupe(u8, q);
        for (up) |*c| c.* = std.ascii.toUpper(c.*);
        const arg = (js.getString(a, input_json, "id") catch null) orelse (js.getString(a, input_json, "type") catch null);
        if (arg) |x| return std.fmt.allocPrint(a, "INSPECT {s} {s}", .{ up, x });
        return std.fmt.allocPrint(a, "INSPECT {s}", .{up});
    }
    const up = try a.dupe(u8, name);
    for (up) |*c| c.* = std.ascii.toUpper(c.*);
    return up;
}
