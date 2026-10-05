//! Synchronous driver: runs a `Chat` to the end of a designer turn against a `Transport` (mock or
//! demo) and a `tools.Engine`. This is what tests use, and what a host can use for demo mode when
//! it does not need per-step async effects. Real hosts instead interpret each `Step` themselves
//! (`.send` -> HTTP effect, `.retry_after_ms` -> timer) and call back into `Chat`.
//! Backoff delays are RECORDED, never slept.

const std = @import("std");
const Allocator = std.mem.Allocator;
const types = @import("types.zig");
const session = @import("session.zig");
const chat_mod = @import("chat.zig");
const tools = @import("tools.zig");
const mock = @import("mock.zig");

pub const Transport = struct {
    ctx: *anyopaque,
    perform_fn: *const fn (ctx: *anyopaque, spec: types.HttpRequestSpec) types.HttpResult,

    pub fn perform(self: Transport, spec: types.HttpRequestSpec) types.HttpResult {
        return self.perform_fn(self.ctx, spec);
    }

    pub fn fromMock(m: *mock.Mock) Transport {
        return .{ .ctx = m, .perform_fn = struct {
            fn f(ctx: *anyopaque, spec: types.HttpRequestSpec) types.HttpResult {
                const mm: *mock.Mock = @ptrCast(@alignCast(ctx));
                return mm.respond(spec);
            }
        }.f };
    }
};

pub const End = enum { done, refusal, err, stalled };

pub const Result = struct {
    end: End,
    err_kind: ?types.ErrorKind = null,
    message: []const u8 = "",
    delays: [16]u32 = [_]u32{0} ** 16,
    n_delays: u32 = 0,
    http_calls: u32 = 0,
    tool_rounds: u32 = 0,

    pub fn backoffs(self: *const Result) []const u32 {
        return self.delays[0..self.n_delays];
    }
};

/// Drive from `first` (the Step returned by userSubmit etc.) to the end of the turn. `clock_ms` is
/// advanced by 10 ms per step so ChatLog timestamps are deterministic.
pub fn run(
    chat: *chat_mod.Chat,
    transport: Transport,
    engine: tools.Engine,
    first: session.Step,
    clock_ms: *i64,
) Allocator.Error!Result {
    var res: Result = .{ .end = .stalled };
    var step = first;
    var guard: u32 = 0;
    while (guard < 500) : (guard += 1) {
        clock_ms.* += 10;
        switch (step) {
            .send => |spec| {
                res.http_calls += 1;
                const r = transport.perform(spec);
                step = try chat.onHttp(clock_ms.*, r);
            },
            .run_tools => |tus| {
                res.tool_rounds += 1;
                var arena = std.heap.ArenaAllocator.init(chat.session.gpa);
                defer arena.deinit();
                const results = try tools.executeAll(arena.allocator(), engine, tus);
                step = try chat.onToolResults(clock_ms.*, results);
            },
            .retry_after_ms => |r| {
                if (res.n_delays < res.delays.len) {
                    res.delays[res.n_delays] = r.ms;
                    res.n_delays += 1;
                }
                clock_ms.* += r.ms;
                step = try chat.retry(clock_ms.*);
            },
            .done => {
                res.end = .done;
                return res;
            },
            .refusal => |why| {
                res.end = .refusal;
                res.message = why;
                return res;
            },
            .err => |e| {
                res.end = .err;
                res.err_kind = e.kind;
                res.message = e.message;
                return res;
            },
            .ignored => return res,
        }
    }
    return res;
}
