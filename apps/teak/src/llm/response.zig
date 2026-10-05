//! Response parsing for /v1/messages (non-streaming) and API error bodies.
//!
//! `parse` allocates only from the arena you pass; all strings borrow from `body` or that arena.
//! `content_raw` is the byte-exact text of the response's `content` array (what goes into history).

const std = @import("std");
const Allocator = std.mem.Allocator;
const types = @import("types.zig");
const js = @import("jsonspan.zig");

pub const StopReason = enum { end_turn, tool_use, max_tokens, stop_sequence, refusal, pause_turn, other, none };

pub const Block = union(enum) {
    text: []const u8,
    thinking,
    redacted_thinking,
    tool_use: types.ToolUse,
    /// A declined model handed off (server-side fallback). Shown only as a console line.
    fallback: struct { from: []const u8, to: []const u8 },
    /// Anything else (server_tool_use, web_search_tool_result, ...): passed through in history only.
    other: []const u8,
};

pub const Response = struct {
    id: []const u8 = "",
    model: []const u8 = "",
    stop_reason: StopReason = .none,
    stop_reason_raw: []const u8 = "",
    /// stop_details.explanation when stop_reason is refusal.
    explanation: ?[]const u8 = null,
    category: ?[]const u8 = null,
    content_raw: []const u8,
    blocks: []const Block,
    usage: types.Usage = .{},

    pub fn toolUses(self: Response, a: Allocator) Allocator.Error![]types.ToolUse {
        var n: usize = 0;
        for (self.blocks) |b| if (b == .tool_use) {
            n += 1;
        };
        const out = try a.alloc(types.ToolUse, n);
        var i: usize = 0;
        for (self.blocks) |b| if (b == .tool_use) {
            out[i] = b.tool_use;
            i += 1;
        };
        return out;
    }
};

pub const ParseError = error{ NotJson, NotMessage, OutOfMemory };

pub fn parse(a: Allocator, body: []const u8) ParseError!Response {
    const ok = js.valid(a, body) catch return error.OutOfMemory;
    if (!ok) return error.NotJson;
    return parseInner(a, body) catch |e| switch (e) {
        error.OutOfMemory => error.OutOfMemory,
        error.Malformed => error.NotMessage,
    };
}

fn parseInner(a: Allocator, body: []const u8) (js.Error || Allocator.Error)!Response {
    const root = js.trim(body);
    if (root.len == 0 or root[0] != '{') return error.Malformed;
    const content = (try js.get(root, "content")) orelse return error.Malformed;
    if (content.len == 0 or content[0] != '[') return error.Malformed;

    var r: Response = .{ .content_raw = content, .blocks = &.{} };
    if (try js.getString(a, root, "id")) |s| r.id = s;
    if (try js.getString(a, root, "model")) |s| r.model = s;
    if (try js.getString(a, root, "stop_reason")) |s| {
        r.stop_reason_raw = s;
        r.stop_reason = std.meta.stringToEnum(StopReason, s) orelse .other;
        if (r.stop_reason == .none) r.stop_reason = .other;
    }
    if (try js.get(root, "stop_details")) |sd| {
        if (sd.len > 0 and sd[0] == '{') {
            r.explanation = try js.getString(a, sd, "explanation");
            r.category = try js.getString(a, sd, "category");
        }
    }
    if (try js.get(root, "usage")) |u| {
        if (u.len > 0 and u[0] == '{') {
            r.usage = .{
                .input_tokens = (try js.getUint(u, "input_tokens")) orelse 0,
                .output_tokens = (try js.getUint(u, "output_tokens")) orelse 0,
                .cache_read_input_tokens = (try js.getUint(u, "cache_read_input_tokens")) orelse 0,
                .cache_creation_input_tokens = (try js.getUint(u, "cache_creation_input_tokens")) orelse 0,
            };
        }
    }

    var blocks: std.ArrayList(Block) = .empty;
    var it = try js.ArrIter.init(content);
    while (try it.next()) |blk| {
        if (blk.len == 0 or blk[0] != '{') return error.Malformed;
        const ty = (try js.getString(a, blk, "type")) orelse return error.Malformed;
        if (std.mem.eql(u8, ty, "text")) {
            try blocks.append(a, .{ .text = (try js.getString(a, blk, "text")) orelse "" });
        } else if (std.mem.eql(u8, ty, "thinking")) {
            try blocks.append(a, .thinking);
        } else if (std.mem.eql(u8, ty, "redacted_thinking")) {
            try blocks.append(a, .redacted_thinking);
        } else if (std.mem.eql(u8, ty, "tool_use")) {
            const id = (try js.getString(a, blk, "id")) orelse return error.Malformed;
            const name = (try js.getString(a, blk, "name")) orelse return error.Malformed;
            const input = (try js.get(blk, "input")) orelse "{}";
            try blocks.append(a, .{ .tool_use = .{ .id = id, .name = name, .input_json = input } });
        } else if (std.mem.eql(u8, ty, "fallback")) {
            var from: []const u8 = "?";
            var to: []const u8 = "?";
            if (try js.get(blk, "from")) |f| if (f.len > 0 and f[0] == '{') {
                from = (try js.getString(a, f, "model")) orelse "?";
            };
            if (try js.get(blk, "to")) |t| if (t.len > 0 and t[0] == '{') {
                to = (try js.getString(a, t, "model")) orelse "?";
            };
            try blocks.append(a, .{ .fallback = .{ .from = from, .to = to } });
        } else {
            try blocks.append(a, .{ .other = ty });
        }
    }
    r.blocks = try blocks.toOwnedSlice(a);
    return r;
}

pub const ApiError = struct {
    /// e.g. "invalid_request_error", "authentication_error", "rate_limit_error", "overloaded_error".
    type: []const u8,
    message: []const u8,
};

/// Parse `{"type":"error","error":{"type":..,"message":..}}`. Null if the body is not that shape.
pub fn parseApiError(a: Allocator, body: []const u8) Allocator.Error!?ApiError {
    if (!(try js.valid(a, body))) return null;
    const root = js.trim(body);
    if (root.len == 0 or root[0] != '{') return null;
    const e = (js.get(root, "error") catch return null) orelse return null;
    if (e.len == 0 or e[0] != '{') return null;
    const msg = (js.getString(a, e, "message") catch return null) orelse return null;
    const ty = (js.getString(a, e, "type") catch return null) orelse "error";
    return .{ .type = ty, .message = msg };
}

test "parse tool_use response" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const body =
        \\{"id":"msg_1","type":"message","role":"assistant","model":"claude-opus-5-5","content":[
        \\ {"type":"thinking","thinking":"hmm","signature":"EqQB=="},
        \\ {"type":"text","text":"Building \"it\"\nnow"},
        \\ {"type":"tool_use","id":"toolu_1","name":"kerf_render","input":{"view":"A"}},
        \\ {"type":"server_tool_use","id":"srv_1","name":"web_search","input":{}}
        \\],"stop_reason":"tool_use","usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":3}}
    ;
    const r = try parse(arena.allocator(), body);
    try std.testing.expectEqual(StopReason.tool_use, r.stop_reason);
    try std.testing.expectEqual(@as(usize, 4), r.blocks.len);
    try std.testing.expectEqualStrings("Building \"it\"\nnow", r.blocks[1].text);
    try std.testing.expectEqualStrings("{\"view\":\"A\"}", r.blocks[2].tool_use.input_json);
    try std.testing.expectEqualStrings("server_tool_use", r.blocks[3].other);
    try std.testing.expectEqual(@as(u64, 3), r.usage.cache_read_input_tokens);
    try std.testing.expect(std.mem.startsWith(u8, r.content_raw, "[\n {\"type\":\"thinking\""));
    try std.testing.expect(std.mem.endsWith(u8, r.content_raw, "}\n]"));
}

test "parse refusal and fallback" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const body =
        \\{"content":[{"type":"fallback","from":{"model":"claude-opus-5-5"},"to":{"model":"claude-sonnet-5-5"}}],
        \\"stop_reason":"refusal","stop_details":{"category":"cyber","explanation":"I can't help with that."}}
    ;
    const r = try parse(arena.allocator(), body);
    try std.testing.expectEqual(StopReason.refusal, r.stop_reason);
    try std.testing.expectEqualStrings("I can't help with that.", r.explanation.?);
    try std.testing.expectEqualStrings("claude-sonnet-5-5", r.blocks[0].fallback.to);
}

test "api error body" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const e = (try parseApiError(arena.allocator(), "{\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"fallbacks: Extra inputs are not permitted\"},\"request_id\":\"req_1\"}")).?;
    try std.testing.expectEqualStrings("invalid_request_error", e.type);
    try std.testing.expectEqualStrings("fallbacks: Extra inputs are not permitted", e.message);
    try std.testing.expect((try parseApiError(arena.allocator(), "<html>")) == null);
    try std.testing.expectError(error.NotJson, parse(arena.allocator(), "oops"));
    try std.testing.expectError(error.NotMessage, parse(arena.allocator(), "{\"a\":1}"));
}
