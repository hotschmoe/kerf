//! Request building: HTTP headers, the /v1/messages body, and the user/tool_result message JSON
//! that goes into the append-only history. Pure functions over an allocator.

const std = @import("std");
const Allocator = std.mem.Allocator;
const types = @import("types.zig");
const jsonw = @import("jsonw.zig");
const Buf = jsonw.Buf;

pub const edits_prefix = "[designer edits since your last turn: ";

/// `[designer edits since your last turn: a; b; c]` from the log's `why` lines.
pub fn formatEditsNote(a: Allocator, whys: []const []const u8) Allocator.Error![]u8 {
    var b = Buf.init(a);
    errdefer b.deinit();
    try b.raw(edits_prefix);
    for (whys, 0..) |w, i| {
        if (i != 0) try b.raw("; ");
        try b.raw(w);
    }
    try b.byte(']');
    return b.toOwnedSlice();
}

/// Headers for one request. `fallbacks` says whether the beta header is included.
pub fn buildHeaders(a: Allocator, cfg: types.Config, fallbacks: bool) Allocator.Error![]types.Header {
    var list: std.ArrayList(types.Header) = .empty;
    errdefer list.deinit(a);
    try list.append(a, .{ .name = "content-type", .value = "application/json" });
    try list.append(a, .{ .name = "x-api-key", .value = cfg.api_key });
    try list.append(a, .{ .name = "anthropic-version", .value = types.api_version });
    if (cfg.browser_header) try list.append(a, .{ .name = "anthropic-dangerous-direct-browser-access", .value = "true" });
    if (fallbacks) try list.append(a, .{ .name = "anthropic-beta", .value = types.beta_fallbacks });
    return list.toOwnedSlice(a);
}

/// The full request body. `messages` are complete message objects (`{"role":..,"content":[..]}`).
pub fn buildBody(
    a: Allocator,
    cfg: types.Config,
    fallbacks: bool,
    messages: []const []const u8,
) Allocator.Error![]u8 {
    var b = Buf.init(a);
    errdefer b.deinit();
    var total: usize = cfg.system.len + cfg.tools_json.len + 512;
    for (messages) |m| total += m.len + 1;
    try b.reserve(total);

    try b.raw("{\"model\":");
    try b.str(cfg.model);
    try b.raw(",\"max_tokens\":");
    try b.uint(cfg.max_tokens);
    try b.raw(",\"thinking\":{\"type\":\"adaptive\"},\"output_config\":{\"effort\":");
    try b.str(cfg.effort.name());
    try b.byte('}');
    if (fallbacks) try b.raw(",\"fallbacks\":\"default\"");
    try b.raw(",\"system\":[{\"type\":\"text\",\"text\":");
    try b.str(cfg.system);
    try b.raw(",\"cache_control\":{\"type\":\"ephemeral\"}}],\"tools\":");
    const tools = try jsonw.minify(a, cfg.tools_json);
    defer a.free(tools);
    try b.raw(tools);
    try b.raw(",\"messages\":[");
    for (messages, 0..) |m, i| {
        if (i != 0) try b.byte(',');
        try b.raw(m);
    }
    try b.raw("]}");
    return b.toOwnedSlice();
}

fn writeImage(b: *Buf, img: types.Image) Allocator.Error!void {
    try b.raw("{\"type\":\"image\",\"source\":{\"type\":\"base64\",\"media_type\":");
    try b.str(img.media_type.mime());
    try b.raw(",\"data\":\"");
    if (img.is_base64) try b.raw(img.data) else try b.base64(img.data);
    try b.raw("\"}}");
}

fn writeText(b: *Buf, text: []const u8) Allocator.Error!void {
    try b.raw("{\"type\":\"text\",\"text\":");
    try b.str(text);
    try b.byte('}');
}

pub const dangling_text = "Not executed: the previous turn ended before this tool could run (stopped by the harness). Ignore this and continue with the designer's new message.";

/// Designer user message. Block order: error tool_results for any dangling tool_use ids of a turn
/// that ended without results, images, designer text, downscale note, edits note.
pub fn userMessage(
    a: Allocator,
    dangling_ids: []const []const u8,
    images: []const types.Image,
    text: []const u8,
    edits_note: ?[]const u8,
) Allocator.Error![]u8 {
    var b = Buf.init(a);
    errdefer b.deinit();
    var est: usize = text.len + 256;
    for (images) |im| est += im.data.len * 2;
    try b.reserve(est);
    try b.raw("{\"role\":\"user\",\"content\":[");
    var first = true;
    for (dangling_ids) |id| {
        if (!first) try b.byte(',');
        first = false;
        try b.raw("{\"type\":\"tool_result\",\"tool_use_id\":");
        try b.str(id);
        try b.raw(",\"content\":[");
        try writeText(&b, dangling_text);
        try b.raw("],\"is_error\":true}");
    }
    for (images) |im| {
        if (!first) try b.byte(',');
        first = false;
        try writeImage(&b, im);
    }
    if (text.len != 0) {
        if (!first) try b.byte(',');
        first = false;
        try writeText(&b, text);
    }
    // downscale notes
    var note = Buf.init(a);
    defer note.deinit();
    for (images, 0..) |im, i| {
        if (im.orig_width != 0 and (im.orig_width != im.width or im.orig_height != im.height)) {
            if (note.items().len != 0) try note.byte('\n');
            try note.print("[image {d} was downscaled by the app from {d}x{d} to {d}x{d} px]", .{ i + 1, im.orig_width, im.orig_height, im.width, im.height });
        }
    }
    if (note.items().len != 0) {
        if (!first) try b.byte(',');
        first = false;
        try writeText(&b, note.items());
    }
    if (edits_note) |n| {
        if (n.len != 0) {
            if (!first) try b.byte(',');
            first = false;
            if (std.mem.startsWith(u8, n, "[designer edits")) {
                try writeText(&b, n);
            } else {
                const wrapped = try std.fmt.allocPrint(a, "{s}{s}]", .{ edits_prefix, n });
                defer a.free(wrapped);
                try writeText(&b, wrapped);
            }
        }
    }
    try b.raw("]}");
    return b.toOwnedSlice();
}

/// One user message carrying every tool_result of a turn, in the order given.
pub fn toolResultsMessage(a: Allocator, results: []const types.ToolResult) Allocator.Error![]u8 {
    var b = Buf.init(a);
    errdefer b.deinit();
    try b.raw("{\"role\":\"user\",\"content\":[");
    for (results, 0..) |r, i| {
        if (i != 0) try b.byte(',');
        try b.raw("{\"type\":\"tool_result\",\"tool_use_id\":");
        try b.str(r.id);
        try b.raw(",\"content\":[");
        if (r.content.len == 0) try writeText(&b, "(empty)");
        for (r.content, 0..) |c, j| {
            if (j != 0) try b.byte(',');
            switch (c) {
                .text => |t| try writeText(&b, if (t.len == 0) "(empty)" else t),
                .image_png_b64 => |d| {
                    try b.raw("{\"type\":\"image\",\"source\":{\"type\":\"base64\",\"media_type\":\"image/png\",\"data\":\"");
                    try b.raw(d);
                    try b.raw("\"}}");
                },
            }
        }
        try b.raw("],\"is_error\":");
        try b.raw(if (r.is_error) "true" else "false");
        try b.byte('}');
    }
    try b.raw("]}");
    return b.toOwnedSlice();
}

/// Assistant message wrapping the VERBATIM content array text from the response.
pub fn assistantMessage(a: Allocator, content_raw: []const u8) Allocator.Error![]u8 {
    var b = Buf.init(a);
    errdefer b.deinit();
    try b.reserve(content_raw.len + 48);
    try b.raw("{\"role\":\"assistant\",\"content\":");
    try b.raw(content_raw);
    try b.byte('}');
    return b.toOwnedSlice();
}

test "body shape" {
    const a = std.testing.allocator;
    const cfg: types.Config = .{ .api_key = "sk-test", .system = "SYS \"q\"\n" };
    const um = try userMessage(a, &.{}, &.{}, "hello", null);
    defer a.free(um);
    const body = try buildBody(a, cfg, true, &.{um});
    defer a.free(body);
    const parsed = try std.json.parseFromSlice(std.json.Value, a, body, .{});
    defer parsed.deinit();
    const o = parsed.value.object;
    try std.testing.expectEqualStrings("claude-opus-5-5", o.get("model").?.string);
    try std.testing.expectEqual(@as(i64, 32000), o.get("max_tokens").?.integer);
    try std.testing.expectEqualStrings("adaptive", o.get("thinking").?.object.get("type").?.string);
    try std.testing.expectEqualStrings("high", o.get("output_config").?.object.get("effort").?.string);
    try std.testing.expectEqualStrings("default", o.get("fallbacks").?.string);
    const sys = o.get("system").?.array.items[0].object;
    try std.testing.expectEqualStrings("SYS \"q\"\n", sys.get("text").?.string);
    try std.testing.expectEqualStrings("ephemeral", sys.get("cache_control").?.object.get("type").?.string);
    try std.testing.expectEqual(@as(usize, 3), o.get("tools").?.array.items.len);
    try std.testing.expectEqualStrings("kerf_apply", o.get("tools").?.array.items[0].object.get("name").?.string);
    try std.testing.expect(o.get("temperature") == null);
    try std.testing.expect(o.get("tool_choice") == null);

    const nb = try buildBody(a, cfg, false, &.{um});
    defer a.free(nb);
    try std.testing.expect(std.mem.indexOf(u8, nb, "fallbacks") == null);
}

test "headers" {
    const a = std.testing.allocator;
    const h = try buildHeaders(a, .{ .api_key = "k" }, true);
    defer a.free(h);
    try std.testing.expectEqual(@as(usize, 5), h.len);
    try std.testing.expectEqualStrings("anthropic-beta", h[4].name);
    try std.testing.expectEqualStrings("server-side-fallback-2026-07-01", h[4].value);
    const h2 = try buildHeaders(a, .{ .api_key = "k" }, false);
    defer a.free(h2);
    try std.testing.expectEqual(@as(usize, 4), h2.len);
}

test "user message ordering: images before text, edits note last" {
    const a = std.testing.allocator;
    const img: types.Image = .{ .data = "abc", .width = 100, .height = 50, .orig_width = 400, .orig_height = 200 };
    const m = try userMessage(a, &.{"toolu_1"}, &.{img}, "make it", "added HETA20; moved note n3");
    defer a.free(m);
    const p = try std.json.parseFromSlice(std.json.Value, a, m, .{});
    defer p.deinit();
    const c = p.value.object.get("content").?.array.items;
    try std.testing.expectEqual(@as(usize, 5), c.len);
    try std.testing.expectEqualStrings("tool_result", c[0].object.get("type").?.string);
    try std.testing.expectEqualStrings("image", c[1].object.get("type").?.string);
    try std.testing.expectEqualStrings("YWJj", c[1].object.get("source").?.object.get("data").?.string);
    try std.testing.expectEqualStrings("make it", c[2].object.get("text").?.string);
    try std.testing.expect(std.mem.indexOf(u8, c[3].object.get("text").?.string, "downscaled") != null);
    try std.testing.expectEqualStrings("[designer edits since your last turn: added HETA20; moved note n3]", c[4].object.get("text").?.string);
}
