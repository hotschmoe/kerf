//! Whole-loop tests: Chat/Session + FakeEngine + Mock transport (and the Demo script).

const std = @import("std");
const testing = std.testing;
const types = @import("types.zig");
const session = @import("session.zig");
const chat_mod = @import("chat.zig");
const chatlog = @import("chatlog.zig");
const tools = @import("tools.zig");
const fake_engine = @import("fake_engine.zig");
const mock = @import("mock.zig");
const driver = @import("driver.zig");
const demo = @import("demo.zig");
const js = @import("jsonspan.zig");
const request = @import("request.zig");

const a = testing.allocator;

const Fx = struct {
    engine: fake_engine.FakeEngine,
    mock: mock.Mock,
    chat: chat_mod.Chat,
    clock: i64 = 1_000_000,

    fn init(script: []const mock.Scripted, cfg_in: types.Config) Fx {
        var cfg = cfg_in;
        if (cfg.api_key.len == 0) cfg.api_key = "sk-ant-test-key-0000";
        return .{
            .engine = fake_engine.FakeEngine.init(a),
            .mock = mock.Mock.init(a, script),
            .chat = chat_mod.Chat.init(a, cfg),
        };
    }
    fn deinit(self: *Fx) void {
        self.chat.deinit();
        self.mock.deinit();
        self.engine.deinit();
    }
    fn submit(self: *Fx, text: []const u8) !driver.Result {
        return self.submitFull(text, &.{}, null);
    }
    fn submitFull(self: *Fx, text: []const u8, images: []const types.Image, note: ?[]const u8) !driver.Result {
        self.clock += 1000;
        const first = try self.chat.userSubmit(self.clock, text, images, note);
        return driver.run(&self.chat, driver.Transport.fromMock(&self.mock), self.engine.engine(), first, &self.clock);
    }
    fn hist(self: *Fx) []const []u8 {
        return self.chat.session.history();
    }
};

const msg_fmt = "{{\"id\":\"msg_t\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-opus-5-5\",\"content\":[{s}],\"stop_reason\":\"{s}\",\"usage\":{{\"input_tokens\":100,\"output_tokens\":20}}}}";
const txt_fmt = "{{\"type\":\"text\",\"text\":\"{s}\"}}";
const tu_fmt = "{{\"type\":\"tool_use\",\"id\":\"{s}\",\"name\":\"{s}\",\"input\":{s}}}";

fn msgBody(comptime content: []const u8, comptime stop: []const u8) @TypeOf(std.fmt.comptimePrint(msg_fmt, .{ content, stop })) {
    return std.fmt.comptimePrint(msg_fmt, .{ content, stop });
}

fn txt(comptime s: []const u8) @TypeOf(std.fmt.comptimePrint(txt_fmt, .{s})) {
    return std.fmt.comptimePrint(txt_fmt, .{s});
}

fn tu(comptime id: []const u8, comptime name: []const u8, comptime input: []const u8) @TypeOf(std.fmt.comptimePrint(tu_fmt, .{ id, name, input })) {
    return std.fmt.comptimePrint(tu_fmt, .{ id, name, input });
}

const set_doc_input =
    \\{"ops":[{"op":"set","path":"doc","value":{"components":[{"id":"cmu","type":"cmu_wall"},{"id":"sill","type":"lumber"}],"views":[{"id":"A"}]}}],"why":"Build"}
;

fn bodyOf(f: *Fx, i: usize) !std.json.Parsed(std.json.Value) {
    return std.json.parseFromSlice(std.json.Value, a, f.mock.requests.items[i].body, .{});
}

fn messagesOf(v: std.json.Value) []const std.json.Value {
    return v.object.get("messages").?.array.items;
}

test "plain text turn" {
    var f = Fx.init(&.{.{ .body = msgBody(txt("Hello designer."), "end_turn") }}, .{});
    defer f.deinit();
    const r = try f.submit("Hi");
    try testing.expectEqual(driver.End.done, r.end);
    try testing.expectEqual(@as(u32, 1), r.http_calls);
    try testing.expectEqual(@as(usize, 2), f.hist().len);
    try testing.expectEqualStrings("{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"Hi\"}]}", f.hist()[0]);
    try testing.expectEqualStrings("{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Hello designer.\"}]}", f.hist()[1]);
    const es = f.chat.log.entries.items;
    try testing.expectEqual(@as(usize, 2), es.len);
    try testing.expectEqualStrings("Hi", es[0].body.designer.text);
    try testing.expectEqualStrings("Hello designer.", es[1].body.assistant_text);
    try testing.expect(es[0].group != es[1].group);
    var sb: [64]u8 = undefined;
    try testing.expectEqualStrings("CLAUDE OK", f.chat.log.statusText(&sb));
    try testing.expectEqual(@as(u64, 100), f.chat.session.usage_total.input_tokens);
    // request headers of the first request
    try testing.expectEqualStrings("server-side-fallback-2026-07-01", f.mock.requests.items[0].beta.?);
    try testing.expectEqualStrings("sk-ant-test-key-0000", f.mock.requests.items[0].api_key);
}

test "apply then final; history roles and activity line" {
    var f = Fx.init(&.{
        .{ .body = msgBody(txt("Building.") ++ "," ++ tu("toolu_1", "kerf_apply", set_doc_input), "tool_use") },
        .{ .body = msgBody(txt("Done."), "end_turn") },
    }, .{});
    defer f.deinit();
    const r = try f.submit("truss at cmu");
    try testing.expectEqual(driver.End.done, r.end);
    try testing.expectEqual(@as(u32, 1), r.tool_rounds);
    try testing.expectEqual(@as(usize, 4), f.hist().len);
    try testing.expect(std.mem.startsWith(u8, f.hist()[2], "{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_1\""));
    try testing.expectEqual(@as(usize, 2), f.engine.comps.items.len);
    try testing.expectEqualStrings("Build", f.engine.last_why);
    // second request carries the results; parse and check
    var p = try bodyOf(&f, 1);
    defer p.deinit();
    const ms = messagesOf(p.value);
    try testing.expectEqual(@as(usize, 3), ms.len);
    try testing.expectEqualStrings("user", ms[2].object.get("role").?.string);
    const tr = ms[2].object.get("content").?.array.items[0].object;
    try testing.expectEqualStrings("tool_result", tr.get("type").?.string);
    try testing.expectEqual(false, tr.get("is_error").?.bool);
    try testing.expect(std.mem.startsWith(u8, tr.get("content").?.array.items[0].object.get("text").?.string, "ok\n2 components"));
    // log: designer, text, tool line, text
    const es = f.chat.log.entries.items;
    try testing.expectEqual(@as(usize, 4), es.len);
    var lb: [256]u8 = undefined;
    try testing.expectEqualStrings("▸ APPLY 1 OP ✓ 0 ERR 0 WARN", chatlog.toolLine(es[2].body.tool, &lb));
    try testing.expectEqual(es[1].group, es[3].group);
    try testing.expect(std.mem.indexOf(u8, es[2].body.tool.input_json, "\"op\":\"set\"") != null);
}

test "multiple tool_use blocks: one user message, order preserved" {
    var f = Fx.init(&.{
        .{ .body = msgBody(
            tu("toolu_a", "kerf_apply", set_doc_input) ++ "," ++ tu("toolu_b", "kerf_render", "{\"view\":\"A\"}") ++ "," ++ tu("toolu_c", "kerf_inspect", "{\"q\":\"summary\"}"),
            "tool_use",
        ) },
        .{ .body = msgBody(txt("ok"), "end_turn") },
    }, .{});
    defer f.deinit();
    const r = try f.submit("go");
    try testing.expectEqual(driver.End.done, r.end);
    try testing.expectEqual(@as(usize, 4), f.hist().len); // user, assistant, ONE results user msg, assistant
    var p = try bodyOf(&f, 1);
    defer p.deinit();
    const c = messagesOf(p.value)[2].object.get("content").?.array.items;
    try testing.expectEqual(@as(usize, 3), c.len);
    try testing.expectEqualStrings("toolu_a", c[0].object.get("tool_use_id").?.string);
    try testing.expectEqualStrings("toolu_b", c[1].object.get("tool_use_id").?.string);
    try testing.expectEqualStrings("toolu_c", c[2].object.get("tool_use_id").?.string);
}

test "results are ordered by tool_use order even if host returns them shuffled; missing ones error" {
    var s = session.Session.init(a, .{ .api_key = "k" });
    defer s.deinit();
    _ = try s.userSubmit("x", &.{}, null);
    const body = msgBody(tu("t1", "kerf_inspect", "{\"q\":\"summary\"}") ++ "," ++ tu("t2", "kerf_inspect", "{\"q\":\"doc\"}") ++ "," ++ tu("t3", "kerf_inspect", "{\"q\":\"doc\"}"), "tool_use");
    const st = try s.onHttp(.{ .status = 200, .body = body });
    try testing.expectEqual(@as(usize, 3), st.run_tools.len);
    const rs = [_]types.ToolResult{
        .{ .id = "t2", .content = &.{.{ .text = "two" }} },
        .{ .id = "t1", .content = &.{.{ .text = "one" }} },
    };
    const st2 = try s.onToolResults(&rs);
    try testing.expect(st2 == .send);
    var p = try std.json.parseFromSlice(std.json.Value, a, s.history()[2], .{});
    defer p.deinit();
    const c = p.value.object.get("content").?.array.items;
    try testing.expectEqualStrings("t1", c[0].object.get("tool_use_id").?.string);
    try testing.expectEqualStrings("one", c[0].object.get("content").?.array.items[0].object.get("text").?.string);
    try testing.expectEqualStrings("t2", c[1].object.get("tool_use_id").?.string);
    try testing.expectEqual(true, c[2].object.get("is_error").?.bool);
}

test "render result block shape (image then caption)" {
    var f = Fx.init(&.{
        .{ .body = msgBody(tu("toolu_a", "kerf_apply", set_doc_input), "tool_use") },
        .{ .body = msgBody(tu("toolu_r", "kerf_render", "{\"view\":\"A\"}"), "tool_use") },
        .{ .body = msgBody(txt("fine"), "end_turn") },
    }, .{});
    defer f.deinit();
    const r = try f.submit("go");
    try testing.expectEqual(driver.End.done, r.end);
    var p = try bodyOf(&f, 2);
    defer p.deinit();
    const ms = messagesOf(p.value);
    const tr = ms[4].object.get("content").?.array.items[0].object;
    try testing.expectEqualStrings("toolu_r", tr.get("tool_use_id").?.string);
    const blocks = tr.get("content").?.array.items;
    try testing.expectEqual(@as(usize, 2), blocks.len);
    const img = blocks[0].object;
    try testing.expectEqualStrings("image", img.get("type").?.string);
    const src = img.get("source").?.object;
    try testing.expectEqualStrings("base64", src.get("type").?.string);
    try testing.expectEqualStrings("image/png", src.get("media_type").?.string);
    try testing.expectEqualStrings(fake_engine.tiny_png_b64, src.get("data").?.string);
    try testing.expectEqualStrings("text", blocks[1].object.get("type").?.string);
    try testing.expectEqualStrings("view A rendered; 2 components; 0 errors", blocks[1].object.get("text").?.string);
    // activity line
    var lb: [256]u8 = undefined;
    var found = false;
    for (f.chat.log.entries.items) |e| switch (e.body) {
        .tool => |t| if (std.mem.eql(u8, t.name, "kerf_render")) {
            try testing.expectEqualStrings("▸ RENDER VIEW A ✓", chatlog.toolLine(t, &lb));
            try testing.expect(t.has_image);
            found = true;
        },
        else => {},
    };
    try testing.expect(found);
}

test "refusal shows explanation and keeps history valid for the next message" {
    var f = Fx.init(&.{
        .{ .body = "{\"content\":[" ++ txt("partial") ++ "," ++ tu("toolu_x", "kerf_inspect", "{\"q\":\"summary\"}") ++ "],\"stop_reason\":\"refusal\",\"stop_details\":{\"category\":\"cyber\",\"explanation\":\"Request declined by safeguards.\"}}" },
        .{ .body = msgBody(txt("back"), "end_turn") },
    }, .{});
    defer f.deinit();
    const r = try f.submit("do something");
    try testing.expectEqual(driver.End.refusal, r.end);
    try testing.expectEqualStrings("Request declined by safeguards.", r.message);
    try testing.expectEqual(@as(usize, 2), f.hist().len);
    const es = f.chat.log.entries.items;
    try testing.expectEqualStrings("Request declined by safeguards.", es[es.len - 1].body.refusal);
    // tools were NOT run
    try testing.expectEqual(@as(u32, 0), f.engine.inspect_calls);
    // next designer message must answer the dangling tool_use first
    const r2 = try f.submit("ok never mind");
    try testing.expectEqual(driver.End.done, r2.end);
    var p = try bodyOf(&f, 1);
    defer p.deinit();
    const c = messagesOf(p.value)[2].object.get("content").?.array.items;
    try testing.expectEqualStrings("tool_result", c[0].object.get("type").?.string);
    try testing.expectEqualStrings("toolu_x", c[0].object.get("tool_use_id").?.string);
    try testing.expectEqual(true, c[0].object.get("is_error").?.bool);
    try testing.expectEqualStrings("text", c[1].object.get("type").?.string);
}

test "refusal without explanation uses a default message" {
    var f = Fx.init(&.{.{ .body = "{\"content\":[],\"stop_reason\":\"refusal\"}" }}, .{});
    defer f.deinit();
    const r = try f.submit("x");
    try testing.expectEqual(driver.End.refusal, r.end);
    try testing.expect(r.message.len > 5);
    try testing.expectEqual(@as(usize, 1), f.hist().len); // empty assistant content not stored
}

test "401 is INVALID API KEY" {
    var f = Fx.init(&.{.{ .status = 401, .body = "{\"type\":\"error\",\"error\":{\"type\":\"authentication_error\",\"message\":\"invalid x-api-key\"}}" }}, .{});
    defer f.deinit();
    const r = try f.submit("hi");
    try testing.expectEqual(driver.End.err, r.end);
    try testing.expectEqual(types.ErrorKind.invalid_api_key, r.err_kind.?);
    try testing.expectEqualStrings("INVALID API KEY", r.message);
    var sb: [64]u8 = undefined;
    try testing.expectEqualStrings("CLAUDE ERROR", f.chat.log.statusText(&sb));
    const last = f.chat.log.entries.items[f.chat.log.entries.items.len - 1];
    try testing.expectEqualStrings("INVALID API KEY", last.body.err.message);
}

test "429 then success: 2 s backoff" {
    var f = Fx.init(&.{
        .{ .status = 429, .body = "{\"type\":\"error\",\"error\":{\"type\":\"rate_limit_error\",\"message\":\"slow down\"}}" },
        .{ .body = msgBody(txt("hi"), "end_turn") },
    }, .{});
    defer f.deinit();
    const r = try f.submit("hi");
    try testing.expectEqual(driver.End.done, r.end);
    try testing.expectEqualSlices(u32, &.{2000}, r.backoffs());
    try testing.expectEqual(@as(u32, 2), r.http_calls);
    // same request resent
    try testing.expectEqualStrings(f.mock.requests.items[0].body, f.mock.requests.items[1].body);
    try testing.expectEqual(@as(usize, 2), f.hist().len);
}

test "429/529 backoff schedule 2/4/8 then shown" {
    const e429 = "{\"type\":\"error\",\"error\":{\"type\":\"rate_limit_error\",\"message\":\"Number of requests has exceeded your rate limit.\"}}";
    var f = Fx.init(&.{
        .{ .status = 429, .body = e429 }, .{ .status = 529, .body = e429 }, .{ .status = 429, .body = e429 }, .{ .status = 429, .body = e429 },
    }, .{});
    defer f.deinit();
    const r = try f.submit("hi");
    try testing.expectEqual(driver.End.err, r.end);
    try testing.expectEqual(types.ErrorKind.rate_limited, r.err_kind.?);
    try testing.expectEqualSlices(u32, &.{ 2000, 4000, 8000 }, r.backoffs());
    try testing.expectEqualStrings("Number of requests has exceeded your rate limit.", r.message);
    try testing.expectEqual(@as(u32, 4), r.http_calls);
    // manual retry afterwards works and resets nothing harmful
    f.mock.script = &.{.{ .body = msgBody(txt("finally"), "end_turn") }};
    f.mock.idx = 0;
    const st = try f.chat.retry(f.clock);
    try testing.expect(st == .send);
}

test "other 4xx shown verbatim; no retry" {
    var f = Fx.init(&.{.{ .status = 400, .body = "{\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"messages: roles must alternate\"}}" }}, .{});
    defer f.deinit();
    const r = try f.submit("hi");
    try testing.expectEqual(driver.End.err, r.end);
    try testing.expectEqual(types.ErrorKind.api_error, r.err_kind.?);
    try testing.expectEqualStrings("messages: roles must alternate", r.message);
    try testing.expectEqual(@as(u32, 1), r.http_calls);
}

test "non-json error body is reported with status" {
    var f = Fx.init(&.{.{ .status = 502, .body = "<html>Bad gateway</html>" }}, .{});
    defer f.deinit();
    const r = try f.submit("hi");
    try testing.expectEqualStrings("HTTP 502: <html>Bad gateway</html>", r.message);
}

test "400 mentioning fallbacks: retry once without it, remembered for the session" {
    var f = Fx.init(&.{
        .{ .status = 400, .body = "{\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"Unexpected value(s) `server-side-fallback-2026-07-01` for the `anthropic-beta` header.\"}}" },
        .{ .body = msgBody(txt("one"), "end_turn") },
        .{ .body = msgBody(txt("two"), "end_turn") },
    }, .{});
    defer f.deinit();
    const r = try f.submit("hi");
    try testing.expectEqual(driver.End.done, r.end);
    try testing.expectEqual(@as(u32, 2), r.http_calls);
    try testing.expect(f.mock.requests.items[0].beta != null);
    try testing.expect(std.mem.indexOf(u8, f.mock.requests.items[0].body, "\"fallbacks\":\"default\"") != null);
    try testing.expect(f.mock.requests.items[1].beta == null);
    try testing.expect(std.mem.indexOf(u8, f.mock.requests.items[1].body, "fallbacks") == null);
    try testing.expect(!f.chat.session.fallbacks_on);
    // remembered: a later turn never sends it
    _ = try f.submit("again");
    try testing.expect(f.mock.requests.items[2].beta == null);
    // a log notice was written
    var saw = false;
    for (f.chat.log.entries.items) |e| switch (e.body) {
        .notice => |n| if (std.mem.indexOf(u8, n, "FALLBACKS") != null) {
            saw = true;
        },
        else => {},
    };
    try testing.expect(saw);
}

test "400 about fallbacks when already off is a normal error (no loop)" {
    var f = Fx.init(&.{.{ .status = 400, .body = "{\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"fallbacks: bad\"}}" }}, .{ .fallbacks = false });
    defer f.deinit();
    const r = try f.submit("hi");
    try testing.expectEqual(driver.End.err, r.end);
    try testing.expectEqual(@as(u32, 1), r.http_calls);
}

test "25-round cap" {
    const loop_body = comptime msgBody(tu("toolu_loop", "kerf_inspect", "{\"q\":\"summary\"}"), "tool_use");
    const script = [_]mock.Scripted{.{ .body = loop_body }} ** 40;
    var f = Fx.init(&script, .{});
    defer f.deinit();
    const r = try f.submit("loop forever");
    try testing.expectEqual(driver.End.err, r.end);
    try testing.expectEqual(types.ErrorKind.round_limit, r.err_kind.?);
    try testing.expectEqual(@as(u32, 25), r.tool_rounds);
    try testing.expectEqual(@as(u32, 26), r.http_calls);
    try testing.expectEqual(@as(u32, 25), f.engine.inspect_calls);
    var sb: [64]u8 = undefined;
    try testing.expectEqualStrings("CLAUDE OK", f.chat.log.statusText(&sb));
    // history is still valid: next designer message answers the dangling tool_use
    f.mock.script = &.{.{ .body = msgBody(txt("ok"), "end_turn") }};
    f.mock.idx = 0;
    const r2 = try f.submit("continue");
    try testing.expectEqual(driver.End.done, r2.end);
    var p = try std.json.parseFromSlice(std.json.Value, a, f.mock.requests.items[f.mock.requests.items.len - 1].body, .{});
    defer p.deinit();
    const ms = messagesOf(p.value);
    const last = ms[ms.len - 1].object.get("content").?.array.items;
    try testing.expectEqualStrings("toolu_loop", last[0].object.get("tool_use_id").?.string);
}

test "status line shows round numbers while busy" {
    var f = Fx.init(&.{.{ .body = msgBody(tu("t1", "kerf_inspect", "{\"q\":\"summary\"}"), "tool_use") }}, .{});
    defer f.deinit();
    var sb: [64]u8 = undefined;
    const s1 = try f.chat.userSubmit(1, "go", &.{}, null);
    try testing.expect(s1 == .send);
    try testing.expectEqualStrings("CLAUDE BUSY ◐ ROUND 1", f.chat.log.statusText(&sb));
    const s2 = try f.chat.onHttp(2, f.mock.respond(s1.send));
    try testing.expect(s2 == .run_tools);
    try testing.expectEqualStrings("CLAUDE BUSY ◐ ROUND 1", f.chat.log.statusText(&sb));
    const s3 = try f.chat.onToolResults(3, &.{.{ .id = "t1", .content = &.{.{ .text = "x" }} }});
    try testing.expect(s3 == .send);
    try testing.expectEqualStrings("CLAUDE BUSY ◐ ROUND 2", f.chat.log.statusText(&sb));
    try testing.expect(f.chat.log.isBusy());
}

test "history is append-only and assistant content is byte-exact (thinking + signature)" {
    const content =
        "[ {\"type\":\"thinking\",\"thinking\":\"Let me \\\"think\\\"\\nabout it \\u00e9\",\"signature\":\"EqQBCkYIAxgCKkB+/+==\"} ,\n" ++
        "  {\"type\":\"redacted_thinking\",\"data\":\"EmwKAhgB\"},\n" ++
        "  {\"type\":\"text\",\"text\":\"Hello\"},\n" ++
        "  {\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"kerf_inspect\",\"input\":{ \"q\" : \"summary\" }} ]";
    const body = "{\"id\":\"m\",\"content\":" ++ content ++ ",\"stop_reason\":\"tool_use\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}";
    const body2 = msgBody(txt("final"), "end_turn");
    var f = Fx.init(&.{ .{ .body = body }, .{ .body = body2 } }, .{});
    defer f.deinit();
    const r = try f.submit("hi");
    try testing.expectEqual(driver.End.done, r.end);
    // stored assistant message contains the content bytes verbatim
    try testing.expectEqualStrings("{\"role\":\"assistant\",\"content\":" ++ content ++ "}", f.hist()[1]);
    // and the next request embeds it verbatim, in order, with earlier messages untouched
    const req2 = f.mock.requests.items[1].body;
    try testing.expect(std.mem.indexOf(u8, req2, "\"messages\":[" ++ "{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"hi\"}]}," ++ "{\"role\":\"assistant\",\"content\":" ++ content ++ "}") != null);
    // prefix property: each request's messages start with the previous request's messages
    const req1 = f.mock.requests.items[0].body;
    const m1 = req1[std.mem.indexOf(u8, req1, "\"messages\":[").? + 12 .. req1.len - 2];
    try testing.expect(std.mem.indexOf(u8, req2, m1) != null);
    // the model-visible thinking blocks survive a second designer turn too
    f.mock.script = &.{.{ .body = msgBody(txt("third"), "end_turn") }};
    f.mock.idx = 0;
    _ = try f.submit("more");
    const req3 = f.mock.requests.items[2].body;
    try testing.expect(std.mem.indexOf(u8, req3, content) != null);
    try testing.expectEqual(@as(usize, 6), f.hist().len);
}

test "history export/import round trip" {
    var f = Fx.init(&.{.{ .body = msgBody(txt("a"), "end_turn") }}, .{});
    defer f.deinit();
    _ = try f.submit("hi");
    const exported = try f.chat.session.exportHistory(a);
    defer a.free(exported);
    var s2 = session.Session.init(a, .{ .api_key = "k" });
    defer s2.deinit();
    try s2.importHistory(exported);
    try testing.expectEqual(@as(usize, 2), s2.history().len);
    try testing.expectEqualStrings(f.hist()[1], s2.history()[1]);
}

test "designer edits note is appended after the designer text" {
    var f = Fx.init(&.{.{ .body = msgBody(txt("ok"), "end_turn") }}, .{});
    defer f.deinit();
    const note = try request.formatEditsNote(a, &.{ "Edit note n_roof text", "Move note n3" });
    defer a.free(note);
    _ = try f.submitFull("make the roof note shorter", &.{}, note);
    var p = try bodyOf(&f, 0);
    defer p.deinit();
    const c = messagesOf(p.value)[0].object.get("content").?.array.items;
    try testing.expectEqual(@as(usize, 2), c.len);
    try testing.expectEqualStrings("make the roof note shorter", c[0].object.get("text").?.string);
    try testing.expectEqualStrings("[designer edits since your last turn: Edit note n_roof text; Move note n3]", c[1].object.get("text").?.string);
    const es = f.chat.log.entries.items;
    try testing.expectEqualStrings(note, es[0].body.designer.edits_note.?);
}

test "json escaping: user text, images, tool input and why" {
    const weird_user = "He said \"cut 2x6\"\nline2\ttab \\ back \u{00e9}\u{4e2d}\u{1F600} \x01";
    const input =
        \\{"ops":[{"op":"add","path":"components","value":{"id":"q\"x","type":"lumber","note":"line1\nline2 \u00e9 \ud83d\ude00"}}],"why":"Add \"quoted\"\nwhy \u00e9"}
    ;
    var f = Fx.init(&.{
        .{ .body = msgBody(tu("toolu_e", "kerf_apply", input), "tool_use") },
        .{ .body = msgBody(txt("fin"), "end_turn") },
    }, .{});
    defer f.deinit();
    const r = try f.submit(weird_user);
    try testing.expectEqual(driver.End.done, r.end);
    // user text round-trips exactly through the body JSON
    var p0 = try bodyOf(&f, 0);
    defer p0.deinit();
    const t = messagesOf(p0.value)[0].object.get("content").?.array.items[0].object.get("text").?.string;
    try testing.expectEqualStrings(weird_user, t);
    // tool input reached the engine decoded
    try testing.expectEqualStrings("Add \"quoted\"\nwhy \u{00e9}", f.engine.last_why);
    try testing.expectEqualStrings("q\"x", f.engine.comps.items[0].id);
    // and the assistant message sent back is valid JSON identical to what arrived
    var p1 = try bodyOf(&f, 1);
    defer p1.deinit();
    const inp = messagesOf(p1.value)[1].object.get("content").?.array.items[0].object.get("input").?.object;
    try testing.expectEqualStrings("Add \"quoted\"\nwhy \u{00e9}", inp.get("why").?.string);
}

test "1 MB image: body size sanity and image-before-text ordering" {
    const raw = try a.alloc(u8, 1024 * 1024);
    defer a.free(raw);
    for (raw, 0..) |*b, i| b.* = @truncate(i *% 31 +% 7);
    var f = Fx.init(&.{.{ .body = msgBody(txt("seen"), "end_turn") }}, .{});
    defer f.deinit();
    const img: types.Image = .{ .data = raw, .media_type = .png, .width = 1568, .height = 900, .orig_width = 3136, .orig_height = 1800 };
    const r = try f.submitFull("recreate this", &.{img}, null);
    try testing.expectEqual(driver.End.done, r.end);
    const body = f.mock.requests.items[0].body;
    const b64_len = (raw.len + 2) / 3 * 4;
    try testing.expect(body.len > b64_len);
    try testing.expect(body.len < b64_len + 64 * 1024); // system + tools + wrapper, nothing quadratic
    var p = try bodyOf(&f, 0);
    defer p.deinit();
    const c = messagesOf(p.value)[0].object.get("content").?.array.items;
    try testing.expectEqualStrings("image", c[0].object.get("type").?.string);
    try testing.expectEqual(b64_len, c[0].object.get("source").?.object.get("data").?.string.len);
    try testing.expectEqualStrings("text", c[1].object.get("type").?.string);
    try testing.expect(std.mem.indexOf(u8, c[2].object.get("text").?.string, "downscaled by the app from 3136x1800 to 1568x900") != null);
    try testing.expectEqual(@as(u32, 1), f.chat.log.entries.items[0].body.designer.n_images);
}

test "tool errors: bad apply, unknown tool, engine rejection are is_error and the loop continues" {
    var f = Fx.init(&.{
        .{ .body = msgBody(tu("t1", "kerf_apply", "{\"ops\":[{\"op\":\"update\",\"path\":\"components/nope\",\"value\":{}}],\"why\":\"x\"}") ++ "," ++ tu("t2", "kerf_zap", "{}") ++ "," ++ tu("t3", "kerf_apply", "{\"why\":\"x\"}") ++ "," ++ tu("t4", "kerf_render", "{\"view\":\"Z\"}"), "tool_use") },
        .{ .body = msgBody(txt("sorry"), "end_turn") },
    }, .{});
    defer f.deinit();
    const r = try f.submit("go");
    try testing.expectEqual(driver.End.done, r.end);
    var p = try bodyOf(&f, 1);
    defer p.deinit();
    const c = messagesOf(p.value)[2].object.get("content").?.array.items;
    try testing.expectEqual(@as(usize, 4), c.len);
    for (c) |blk| try testing.expectEqual(true, blk.object.get("is_error").?.bool);
    const t0 = c[0].object.get("content").?.array.items[0].object.get("text").?.string;
    try testing.expect(std.mem.indexOf(u8, t0, "E_REF_UNKNOWN") != null);
    try testing.expect(std.mem.indexOf(u8, t0, "nothing was changed") != null);
    try testing.expect(std.mem.indexOf(u8, c[1].object.get("content").?.array.items[0].object.get("text").?.string, "Unknown tool \"kerf_zap\"") != null);
    try testing.expect(std.mem.indexOf(u8, c[2].object.get("content").?.array.items[0].object.get("text").?.string, "missing required field `ops`") != null);
    var lb: [256]u8 = undefined;
    const es = f.chat.log.entries.items;
    try testing.expectEqualStrings("▸ APPLY 1 OP ✗ 1 ERR 0 WARN", chatlog.toolLine(es[1].body.tool, &lb));
}

test "apply warnings counted in the activity line" {
    const input =
        \\{"ops":[{"op":"set","path":"doc","value":{"components":[{"id":"s1","type":"solid"},{"id":"w","type":"lumber"}],"views":[{"id":"A"}]}},{"op":"update","path":"meta","value":{}}],"why":"w"}
    ;
    var f = Fx.init(&.{ .{ .body = msgBody(tu("t1", "kerf_apply", input), "tool_use") }, .{ .body = msgBody(txt("k"), "end_turn") } }, .{});
    defer f.deinit();
    _ = try f.submit("go");
    var lb: [256]u8 = undefined;
    try testing.expectEqualStrings("▸ APPLY 2 OPS ✓ 0 ERR 1 WARN", chatlog.toolLine(f.chat.log.entries.items[1].body.tool, &lb));
}

test "network error then manual retry" {
    var f = Fx.init(&.{
        .{ .status = 0, .err = "offline" },
        .{ .body = msgBody(txt("hi"), "end_turn") },
    }, .{});
    defer f.deinit();
    const first = try f.chat.userSubmit(1, "hello", &.{}, null);
    const st = try f.chat.onHttp(2, f.mock.respond(first.send));
    try testing.expect(st == .err);
    try testing.expectEqual(types.ErrorKind.network, st.err.kind);
    try testing.expectEqualStrings("NETWORK ERROR: offline", st.err.message);
    const st2 = try f.chat.retry(3);
    try testing.expect(st2 == .send);
    const st3 = try f.chat.onHttp(4, f.mock.respond(st2.send));
    try testing.expect(st3 == .done);
    try testing.expectEqual(@as(usize, 2), f.hist().len);
}

test "max_tokens with tool_use: tools are not run, error results keep history valid" {
    var f = Fx.init(&.{
        .{ .body = msgBody(tu("t1", "kerf_apply", "{\"ops\":[{\"op\":\"set\",\"path\":\"doc\",\"value\":{\"components\":[]}}]}"), "max_tokens") },
        .{ .body = msgBody(txt("retrying smaller"), "end_turn") },
    }, .{});
    defer f.deinit();
    const r = try f.submit("big");
    try testing.expectEqual(driver.End.done, r.end);
    try testing.expectEqual(@as(u32, 0), r.tool_rounds);
    try testing.expectEqual(@as(u32, 0), f.engine.apply_calls);
    try testing.expectEqual(@as(usize, 4), f.hist().len);
}

test "max_tokens without tools is a truncation error" {
    var f = Fx.init(&.{.{ .body = msgBody(txt("cut off"), "max_tokens") }}, .{});
    defer f.deinit();
    const r = try f.submit("big");
    try testing.expectEqual(driver.End.err, r.end);
    try testing.expectEqual(types.ErrorKind.truncated, r.err_kind.?);
}

test "pause_turn continues with the assistant turn last" {
    var f = Fx.init(&.{
        .{ .body = msgBody("{\"type\":\"server_tool_use\",\"id\":\"srv_1\",\"name\":\"web_search\",\"input\":{\"query\":\"irc\"}}", "pause_turn") },
        .{ .body = msgBody(txt("done"), "end_turn") },
    }, .{});
    defer f.deinit();
    const r = try f.submit("search");
    try testing.expectEqual(driver.End.done, r.end);
    try testing.expectEqual(@as(u32, 2), r.http_calls);
    try testing.expectEqual(@as(usize, 3), f.hist().len);
    try testing.expect(std.mem.indexOf(u8, f.hist()[1], "server_tool_use") != null);
}

test "fallback block becomes a console line and stays in history" {
    var f = Fx.init(&.{.{ .body = msgBody("{\"type\":\"fallback\",\"from\":{\"model\":\"claude-opus-5-5\"},\"to\":{\"model\":\"claude-sonnet-5-5\"}}," ++ txt("hi"), "end_turn") }}, .{});
    defer f.deinit();
    _ = try f.submit("x");
    const es = f.chat.log.entries.items;
    try testing.expectEqualStrings("▸ FALLBACK claude-opus-5-5 → claude-sonnet-5-5", es[1].body.notice);
    try testing.expect(std.mem.indexOf(u8, f.hist()[1], "\"type\":\"fallback\"") != null);
}

test "no key, empty input, busy" {
    var s = session.Session.init(a, .{});
    defer s.deinit();
    const st = try s.userSubmit("hi", &.{}, null);
    try testing.expectEqual(types.ErrorKind.no_key, st.err.kind);
    try testing.expectEqual(@as(usize, 0), s.history().len);
    s.cfg.api_key = "k";
    try testing.expectEqual(types.ErrorKind.empty_input, (try s.userSubmit("", &.{}, null)).err.kind);
    const ok = try s.userSubmit("hi", &.{}, null);
    try testing.expect(ok == .send);
    try testing.expectEqual(types.ErrorKind.busy, (try s.userSubmit("again", &.{}, null)).err.kind);
    // a stale http result after cancel is ignored
    try testing.expect((try s.cancel()) == .done);
    try testing.expect((try s.onHttp(.{ .status = 200, .body = "{}" })) == .ignored);
}

test "unreadable 200 body" {
    var f = Fx.init(&.{.{ .body = "not json" }}, .{});
    defer f.deinit();
    const r = try f.submit("hi");
    try testing.expectEqual(types.ErrorKind.bad_response, r.err_kind.?);
}

test "demo script drives the whole loop" {
    var f = Fx.init(&.{}, .{});
    defer f.deinit();
    var d: demo.Demo = .{};
    f.clock += 1000;
    const first = try f.chat.userSubmit(f.clock, "Truss bearing on an 8\" CMU wall, please.", &.{}, null);
    const r = try driver.run(&f.chat, d.transport(), f.engine.engine(), first, &f.clock);
    try testing.expectEqual(driver.End.done, r.end);
    try testing.expectEqual(@as(u32, 4), r.http_calls);
    try testing.expectEqual(@as(u32, 3), r.tool_rounds);
    try testing.expectEqual(@as(usize, 14), f.engine.comps.items.len);
    try testing.expectEqual(@as(u32, 1), f.engine.render_calls);
    try testing.expectEqual(@as(u32, 1), f.engine.inspect_calls);
    // history: user, asst, results, asst, results, asst, results, asst
    try testing.expectEqual(@as(usize, 8), f.hist().len);
    // thinking block with signature is kept verbatim
    try testing.expect(std.mem.indexOf(u8, f.hist()[1], "\"signature\":\"RGVtbyBzaWduYXR1cmUgKG5vdCByZWFsKQ==\"") != null);
    // log
    var lb: [256]u8 = undefined;
    var lines: [3][]const u8 = undefined;
    var n: usize = 0;
    for (f.chat.log.entries.items) |e| switch (e.body) {
        .tool => |t| {
            lines[n] = try a.dupe(u8, chatlog.toolLine(t, &lb));
            n += 1;
        },
        else => {},
    };
    defer for (lines[0..n]) |l| a.free(l);
    try testing.expectEqual(@as(usize, 3), n);
    try testing.expectEqualStrings("▸ APPLY 1 OP ✓ 0 ERR 0 WARN", lines[0]);
    try testing.expectEqualStrings("▸ RENDER VIEW A ✓", lines[1]);
    try testing.expectEqualStrings("▸ INSPECT SUMMARY ✓", lines[2]);
    // a later designer message gets the canned reply, no tools
    f.mock.script = &.{};
    const second = try f.chat.userSubmit(f.clock + 1, "make it taller", &.{}, null);
    const r2 = try driver.run(&f.chat, d.transport(), f.engine.engine(), second, &f.clock);
    try testing.expectEqual(driver.End.done, r2.end);
    try testing.expectEqual(@as(u32, 1), r2.http_calls);
}

test "demo requests are valid and stateless" {
    const r0 = demo.Demo.next("{\"messages\":[{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"x\"}]}]}");
    try testing.expectEqual(@as(u16, 200), r0.status);
    try testing.expect(std.mem.indexOf(u8, r0.body, "toolu_demo_apply") != null);
    var arena = std.heap.ArenaAllocator.init(a);
    defer arena.deinit();
    try testing.expect(try js.valid(arena.allocator(), r0.body));
    const again = demo.Demo.next("{\"messages\":[{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"x\"}]}]}");
    try testing.expectEqualStrings(r0.body, again.body);
}

test "chat log tool detail: pretty op JSON and toggle" {
    var f = Fx.init(&.{ .{ .body = msgBody(tu("t1", "kerf_apply", set_doc_input), "tool_use") }, .{ .body = msgBody(txt("k"), "end_turn") } }, .{});
    defer f.deinit();
    _ = try f.submit("go");
    f.chat.log.toggleExpanded(1);
    const t = f.chat.log.entries.items[1].body.tool;
    try testing.expect(t.expanded);
    const pretty = try @import("jsonw.zig").pretty(a, t.input_json);
    defer a.free(pretty);
    try testing.expect(std.mem.indexOf(u8, pretty, "\n  \"ops\": [") != null);
    try testing.expectEqualStrings("Build", t.title[0..0] ++ "Build");
}
