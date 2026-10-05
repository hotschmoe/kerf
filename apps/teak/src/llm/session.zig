//! `Session`: the non-blocking, host-agnostic Claude tool-loop state machine.
//!
//! The Session performs NO I/O and never blocks. The host (a TEA `update`) calls one of the entry
//! points, interprets the returned `Step`, and later feeds the outcome back:
//!
//!   userSubmit(text, images, edits_note) -> Step      designer sends a message
//!   onHttp(HttpResult)                   -> Step      the HTTP effect finished
//!   onToolResults([]ToolResult)          -> Step      the host ran the `.run_tools` batch
//!   retry()                              -> Step      after `.retry_after_ms` elapsed, or to resend after an error
//!   cancel()                             -> Step      designer pressed stop
//!
//! Steps: `.send` (perform this POST, then call onHttp), `.run_tools` (run each tool synchronously
//! via `tools.executeToolUse`, then call onToolResults with ALL results), `.retry_after_ms`
//! (arm a timer, then call retry()), `.done` (turn finished OK), `.refusal` (model refused),
//! `.err` (turn ended with an error; `retry()` may resend if the kind is retryable), `.ignored`
//! (stale call, e.g. an HTTP result after cancel).
//!
//! Display events: every call also fills `session.events()` (valid until the next call) with
//! `Event`s in order (assistant text, tool calls, tool results, notices, errors, phase changes).
//! `ChatLog.ingest` turns them into the console view-model; see chatlog.zig / chat.zig.
//!
//! Lifetimes: strings in a Step/Event and the `.send` request are owned by the Session and valid
//! until the next call on it. The host must copy what it keeps (a `.send` request must be copied
//! into the effect; it stays valid while the request is in flight because no Session call happens
//! in between except cancel()).
//!
//! History is append-only: each assistant message is stored as the verbatim bytes of the
//! response `content` array (thinking blocks round-trip byte-exact), all tool_results of one
//! assistant turn go into ONE user message, and the history is never edited or truncated.

const std = @import("std");
const Allocator = std.mem.Allocator;
const types = @import("types.zig");
const request = @import("request.zig");
const response = @import("response.zig");
const js = @import("jsonspan.zig");

pub const Phase = union(enum) {
    ready,
    /// Waiting for the API; payload is the 1-based round (tool rounds executed so far + 1).
    busy: u32,
    /// Host is running tools; payload is the round number.
    running_tools: u32,
    /// Waiting out a 429/529 backoff.
    backoff: struct { attempt: u32, delay_ms: u32 },
    failed,
};

pub const ToolDone = struct {
    id: []const u8,
    is_error: bool,
    n_err: u32,
    n_warn: u32,
    has_image: bool,
    /// All text blocks of the result, joined with "\n".
    text: []const u8,
};

pub const Event = union(enum) {
    designer_message: struct { text: []const u8, n_images: u32, edits_note: ?[]const u8 },
    assistant_text: []const u8,
    /// A thinking block arrived (payload: redacted?). Content is never shown.
    thinking: bool,
    tool_call: types.ToolUse,
    tool_done: ToolDone,
    /// `▸ FALLBACK <from> → <to>`
    fallback: struct { from: []const u8, to: []const u8 },
    /// The API rejected the fallbacks param/beta header; resent without it (remembered for the session).
    fallbacks_disabled,
    retry_scheduled: struct { attempt: u32, delay_ms: u32, status: u16 },
    usage: types.Usage,
    phase: Phase,
    err: types.ErrInfo,
    refusal: []const u8,
    notice: []const u8,
};

pub const Step = union(enum) {
    send: types.HttpRequestSpec,
    run_tools: []const types.ToolUse,
    retry_after_ms: struct { ms: u32, attempt: u32 },
    done,
    err: types.ErrInfo,
    refusal: []const u8,
    ignored,
};

pub const State = enum { idle, awaiting_http, awaiting_tools, backoff, failed };

pub const round_limit_message = "ROUND LIMIT: 25 TOOL ROUNDS IN ONE TURN. SEND A MESSAGE TO CONTINUE.";

pub const Session = struct {
    gpa: Allocator,
    /// Mutable between turns (model picker, effort, key, system text with a fresh catalog).
    cfg: types.Config,
    state: State = .idle,
    /// Whether requests still ask for `fallbacks: "default"` + beta header.
    fallbacks_on: bool,
    /// Tool rounds executed in the current designer turn.
    rounds: u32 = 0,
    /// Backoff attempts used for the current request.
    attempt: u32 = 0,
    usage_total: types.Usage = .{},
    /// Number of designer messages sent so far.
    turns: u32 = 0,

    hist: std.ArrayList([]u8) = .empty,
    dangling: std.ArrayList([]u8) = .empty,
    pending: []const types.ToolUse = &.{},
    pending_arena: std.heap.ArenaAllocator,
    req_arena: std.heap.ArenaAllocator,
    ev_arena: std.heap.ArenaAllocator,
    ev_list: std.ArrayList(Event) = .empty,

    pub fn init(gpa: Allocator, cfg: types.Config) Session {
        return .{
            .gpa = gpa,
            .cfg = cfg,
            .fallbacks_on = cfg.fallbacks,
            .pending_arena = std.heap.ArenaAllocator.init(gpa),
            .req_arena = std.heap.ArenaAllocator.init(gpa),
            .ev_arena = std.heap.ArenaAllocator.init(gpa),
        };
    }

    pub fn deinit(self: *Session) void {
        for (self.hist.items) |m| self.gpa.free(m);
        self.hist.deinit(self.gpa);
        for (self.dangling.items) |m| self.gpa.free(m);
        self.dangling.deinit(self.gpa);
        self.pending_arena.deinit();
        self.req_arena.deinit();
        self.ev_arena.deinit();
    }

    // ---- inspection ----

    /// Display events produced by the most recent call.
    pub fn events(self: *const Session) []const Event {
        return self.ev_list.items;
    }

    /// The append-only history: one complete message object JSON per entry.
    pub fn history(self: *const Session) []const []u8 {
        return self.hist.items;
    }

    pub fn isBusy(self: *const Session) bool {
        return self.state == .awaiting_http or self.state == .awaiting_tools or self.state == .backoff;
    }

    /// Current 1-based round for the status line (`CLAUDE BUSY ◐ ROUND n`).
    pub fn round(self: *const Session) u32 {
        return if (self.state == .awaiting_tools) self.rounds else self.rounds + 1;
    }

    /// JSON array text of the whole history (for saving a conversation). Caller frees.
    pub fn exportHistory(self: *const Session, a: Allocator) Allocator.Error![]u8 {
        var b = @import("jsonw.zig").Buf.init(a);
        errdefer b.deinit();
        try b.byte('[');
        for (self.hist.items, 0..) |m, i| {
            if (i != 0) try b.byte(',');
            try b.raw(m);
        }
        try b.byte(']');
        return b.toOwnedSlice();
    }

    /// Replace an EMPTY history with a previously exported one.
    pub fn importHistory(self: *Session, json_array: []const u8) !void {
        if (self.hist.items.len != 0 or self.isBusy()) return error.NotEmpty;
        if (!try js.valid(self.gpa, json_array)) return error.Malformed;
        var it = try js.ArrIter.init(json_array);
        while (try it.next()) |m| {
            const copy = try self.gpa.dupe(u8, m);
            errdefer self.gpa.free(copy);
            try self.hist.append(self.gpa, copy);
        }
    }

    // ---- entry points ----

    pub fn userSubmit(
        self: *Session,
        text: []const u8,
        images: []const types.Image,
        designer_edits_note: ?[]const u8,
    ) Allocator.Error!Step {
        self.beginCall();
        if (self.isBusy()) return self.reject(.busy, "CLAUDE IS BUSY. WAIT FOR THE CURRENT TURN OR CANCEL IT.");
        if (self.cfg.api_key.len == 0) return self.reject(.no_key, "NO API KEY — ENTER KEY TO ENABLE CLAUDE.");
        const note = if (designer_edits_note) |n| (if (n.len == 0) null else n) else null;
        if (text.len == 0 and images.len == 0) return self.reject(.empty_input, "EMPTY MESSAGE.");

        const ev = self.ev_arena.allocator();
        const ids = try ev.alloc([]const u8, self.dangling.items.len);
        for (self.dangling.items, 0..) |d, i| ids[i] = d;
        const msg = try request.userMessage(self.gpa, ids, images, text, note);
        errdefer self.gpa.free(msg);
        try self.hist.append(self.gpa, msg);
        for (self.dangling.items) |d| self.gpa.free(d);
        self.dangling.clearRetainingCapacity();

        self.rounds = 0;
        self.attempt = 0;
        self.turns += 1;
        try self.emit(.{ .designer_message = .{
            .text = try ev.dupe(u8, text),
            .n_images = @intCast(images.len),
            .edits_note = if (note) |n| try ev.dupe(u8, n) else null,
        } });
        return self.buildSend();
    }

    pub fn onHttp(self: *Session, r: types.HttpResult) Allocator.Error!Step {
        self.beginCall();
        if (self.state != .awaiting_http) return .ignored;
        if (r.err) |e| {
            const msg = try std.fmt.allocPrint(self.ev_arena.allocator(), "NETWORK ERROR: {s}", .{e});
            return self.fail(.network, msg, 0);
        }
        const body = try self.ev_arena.allocator().dupe(u8, r.body);
        if (r.status >= 200 and r.status < 300) return self.onOk(body);
        return self.onHttpError(r.status, body);
    }

    pub fn onToolResults(self: *Session, results: []const types.ToolResult) Allocator.Error!Step {
        self.beginCall();
        if (self.state != .awaiting_tools) return .ignored;
        const ev = self.ev_arena.allocator();

        // Order by the assistant's tool_use order; synthesize an error for any missing id.
        const ordered = try ev.alloc(types.ToolResult, self.pending.len);
        for (self.pending, 0..) |tu, i| {
            ordered[i] = for (results) |r| {
                if (std.mem.eql(u8, r.id, tu.id)) break r;
            } else .{
                .id = tu.id,
                .is_error = true,
                .content = &.{.{ .text = "No result was returned for this tool call." }},
            };
        }
        const msg = try request.toolResultsMessage(self.gpa, ordered);
        errdefer self.gpa.free(msg);
        try self.hist.append(self.gpa, msg);

        for (ordered) |r| try self.emitToolDone(r);
        _ = self.pending_arena.reset(.retain_capacity);
        self.pending = &.{};
        return self.buildSend();
    }

    /// Resend after a backoff timer, or after a retryable error (network etc.).
    pub fn retry(self: *Session) Allocator.Error!Step {
        self.beginCall();
        if (self.state != .backoff and self.state != .failed) return .ignored;
        if (self.cfg.api_key.len == 0) return self.reject(.no_key, "NO API KEY — ENTER KEY TO ENABLE CLAUDE.");
        return self.buildSend();
    }

    pub fn cancel(self: *Session) Allocator.Error!Step {
        self.beginCall();
        if (self.state == .idle) return .ignored;
        if (self.state == .awaiting_tools) try self.markDangling(self.pending);
        _ = self.pending_arena.reset(.retain_capacity);
        self.pending = &.{};
        self.state = .idle;
        try self.emit(.{ .notice = "CANCELLED" });
        try self.emit(.{ .phase = .ready });
        return .done;
    }

    // ---- internals ----

    fn beginCall(self: *Session) void {
        _ = self.ev_arena.reset(.retain_capacity);
        self.ev_list = .empty;
    }

    fn emit(self: *Session, e: Event) Allocator.Error!void {
        try self.ev_list.append(self.ev_arena.allocator(), e);
    }

    fn reject(self: *Session, kind: types.ErrorKind, msg: []const u8) Allocator.Error!Step {
        const info: types.ErrInfo = .{ .kind = kind, .message = msg };
        try self.emit(.{ .err = info });
        return .{ .err = info };
    }

    fn fail(self: *Session, kind: types.ErrorKind, msg: []const u8, status: u16) Allocator.Error!Step {
        self.state = .failed;
        const info: types.ErrInfo = .{ .kind = kind, .message = msg, .status = status };
        try self.emit(.{ .err = info });
        try self.emit(.{ .phase = .failed });
        return .{ .err = info };
    }

    /// Terminal error that leaves nothing to retry (round limit, truncation).
    fn finishErr(self: *Session, kind: types.ErrorKind, msg: []const u8) Allocator.Error!Step {
        self.state = .idle;
        const info: types.ErrInfo = .{ .kind = kind, .message = msg };
        try self.emit(.{ .err = info });
        try self.emit(.{ .phase = .ready });
        return .{ .err = info };
    }

    fn markDangling(self: *Session, tool_uses: []const types.ToolUse) Allocator.Error!void {
        for (tool_uses) |tu| {
            const id = try self.gpa.dupe(u8, tu.id);
            errdefer self.gpa.free(id);
            try self.dangling.append(self.gpa, id);
        }
    }

    fn buildSend(self: *Session) Allocator.Error!Step {
        _ = self.req_arena.reset(.retain_capacity);
        const ra = self.req_arena.allocator();
        const headers = try request.buildHeaders(ra, self.cfg, self.fallbacks_on);
        const body = try request.buildBody(ra, self.cfg, self.fallbacks_on, self.hist.items);
        self.state = .awaiting_http;
        try self.emit(.{ .phase = .{ .busy = self.round() } });
        return .{ .send = .{ .url = self.cfg.url, .headers = headers, .body_json = body } };
    }

    fn onHttpError(self: *Session, status: u16, body: []const u8) Allocator.Error!Step {
        const ev = self.ev_arena.allocator();
        const api = try response.parseApiError(ev, body);
        const msg: []const u8 = if (api) |e| e.message else blk: {
            const cut = body[0..@min(body.len, 300)];
            break :blk try std.fmt.allocPrint(ev, "HTTP {d}: {s}", .{ status, cut });
        };
        switch (status) {
            401 => return self.fail(.invalid_api_key, "INVALID API KEY", 401),
            400 => if (self.fallbacks_on and mentionsFallbacks(msg, body)) {
                self.fallbacks_on = false;
                try self.emit(.fallbacks_disabled);
                return self.buildSend();
            },
            429, 529 => {
                if (types.backoffMs(self.attempt)) |ms| {
                    self.attempt += 1;
                    self.state = .backoff;
                    try self.emit(.{ .retry_scheduled = .{ .attempt = self.attempt, .delay_ms = ms, .status = status } });
                    try self.emit(.{ .phase = .{ .backoff = .{ .attempt = self.attempt, .delay_ms = ms } } });
                    return .{ .retry_after_ms = .{ .ms = ms, .attempt = self.attempt } };
                }
                return self.fail(if (status == 429) .rate_limited else .overloaded, msg, status);
            },
            else => {},
        }
        return self.fail(.api_error, msg, status);
    }

    fn onOk(self: *Session, body: []const u8) Allocator.Error!Step {
        const ev = self.ev_arena.allocator();
        const resp = response.parse(ev, body) catch |e| switch (e) {
            error.OutOfMemory => return error.OutOfMemory,
            else => return self.fail(.bad_response, "UNREADABLE RESPONSE FROM API.", 200),
        };
        self.attempt = 0;
        self.usage_total.add(resp.usage);
        try self.emit(.{ .usage = resp.usage });

        // Display events first, in order.
        for (resp.blocks) |b| switch (b) {
            .text => |t| if (t.len != 0) try self.emit(.{ .assistant_text = t }),
            .thinking => try self.emit(.{ .thinking = false }),
            .redacted_thinking => try self.emit(.{ .thinking = true }),
            .tool_use => |tu| try self.emit(.{ .tool_call = tu }),
            .fallback => |f| try self.emit(.{ .fallback = .{ .from = f.from, .to = f.to } }),
            .other => {},
        };

        // Append VERBATIM (empty content arrays are not valid history entries).
        if (resp.blocks.len != 0) {
            const msg = try request.assistantMessage(self.gpa, resp.content_raw);
            errdefer self.gpa.free(msg);
            try self.hist.append(self.gpa, msg);
        }

        const tool_uses = try resp.toolUses(ev);

        switch (resp.stop_reason) {
            .refusal => {
                try self.markDangling(tool_uses);
                self.state = .idle;
                const why = resp.explanation orelse "CLAUDE DECLINED THIS REQUEST.";
                try self.emit(.{ .refusal = why });
                try self.emit(.{ .phase = .ready });
                return .{ .refusal = why };
            },
            .tool_use, .max_tokens => {
                if (tool_uses.len != 0) return self.toolRound(tool_uses, resp.stop_reason == .max_tokens);
                if (resp.stop_reason == .max_tokens)
                    return self.finishErr(.truncated, "RESPONSE CUT OFF AT MAX_TOKENS.");
                return self.done();
            },
            .pause_turn => {
                if (self.rounds >= self.cfg.max_rounds) return self.finishErr(.round_limit, round_limit_message);
                self.rounds += 1;
                return self.buildSend();
            },
            .other => {
                if (resp.blocks.len == 0) return self.finishErr(.bad_response, "EMPTY RESPONSE FROM API.");
                return self.finishErr(.truncated, try std.fmt.allocPrint(ev, "STOPPED: {s}", .{resp.stop_reason_raw}));
            },
            .end_turn, .stop_sequence, .none => {
                if (resp.blocks.len == 0) return self.finishErr(.bad_response, "EMPTY RESPONSE FROM API.");
                return self.done();
            },
        }
    }

    fn done(self: *Session) Allocator.Error!Step {
        self.state = .idle;
        try self.emit(.{ .phase = .ready });
        return .done;
    }

    fn toolRound(self: *Session, tool_uses: []const types.ToolUse, truncated: bool) Allocator.Error!Step {
        if (self.rounds >= self.cfg.max_rounds) {
            try self.markDangling(tool_uses);
            return self.finishErr(.round_limit, round_limit_message);
        }
        self.rounds += 1;
        if (truncated) {
            // The tool_use input may be cut off: never run it. Answer every call with an error so
            // the history stays valid, and let the model retry with smaller ops.
            const ev = self.ev_arena.allocator();
            const results = try ev.alloc(types.ToolResult, tool_uses.len);
            for (tool_uses, 0..) |tu, i| results[i] = .{
                .id = tu.id,
                .is_error = true,
                .content = &.{.{ .text = "Your response hit max_tokens before this tool call finished, so it was NOT executed. Retry with a smaller call (fewer ops per kerf_apply; for a first build, add components in several batches)." }},
            };
            const msg = try request.toolResultsMessage(self.gpa, results);
            errdefer self.gpa.free(msg);
            try self.hist.append(self.gpa, msg);
            try self.emit(.{ .notice = "RESPONSE TRUNCATED AT MAX_TOKENS; TOOL CALLS NOT RUN." });
            for (results) |r| try self.emitToolDone(r);
            return self.buildSend();
        }
        _ = self.pending_arena.reset(.retain_capacity);
        const pa = self.pending_arena.allocator();
        const copy = try pa.alloc(types.ToolUse, tool_uses.len);
        for (tool_uses, 0..) |tu, i| copy[i] = .{
            .id = try pa.dupe(u8, tu.id),
            .name = try pa.dupe(u8, tu.name),
            .input_json = try pa.dupe(u8, tu.input_json),
        };
        self.pending = copy;
        self.state = .awaiting_tools;
        try self.emit(.{ .phase = .{ .running_tools = self.rounds } });
        return .{ .run_tools = copy };
    }

    fn emitToolDone(self: *Session, r: types.ToolResult) Allocator.Error!void {
        const ev = self.ev_arena.allocator();
        var text: std.ArrayList(u8) = .empty;
        var has_image = false;
        for (r.content) |c| switch (c) {
            .text => |t| {
                if (text.items.len != 0) try text.append(ev, '\n');
                try text.appendSlice(ev, t);
            },
            .image_png_b64 => has_image = true,
        };
        try self.emit(.{ .tool_done = .{
            .id = try ev.dupe(u8, r.id),
            .is_error = r.is_error,
            .n_err = r.n_err,
            .n_warn = r.n_warn,
            .has_image = has_image,
            .text = text.items,
        } });
    }
};

fn mentionsFallbacks(msg: []const u8, body: []const u8) bool {
    const needles = [_][]const u8{ "fallback", "anthropic-beta", "server-side-fallback" };
    for (needles) |n| {
        if (std.ascii.indexOfIgnoreCase(msg, n) != null) return true;
        if (std.ascii.indexOfIgnoreCase(body, n) != null) return true;
    }
    return false;
}
