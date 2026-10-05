//! `ChatLog`: the console view-model the UI renders. Built incrementally and deterministically
//! from `Session` events (`ingest`); the caller passes the timestamp (no clock inside).
//!
//! Entries (in order): designer messages (text + image thumbnail count + optional edits note),
//! assistant text blocks, tool activity lines (`▸ APPLY 6 OPS ✓ 0 ERR 1 WARN`, `▸ RENDER VIEW A ✓`;
//! expandable: `input_json` is the op JSON, `result_text` the engine reply), notices
//! (`▸ FALLBACK a → b`, retry/backoff lines), errors (red lines) and refusals.
//! `Entry.group` identifies the card: a designer message has its own group; every assistant /
//! tool / notice / error entry after it shares the next group (render as one manila card whose
//! header is `KERF/CLAUDE  hh:mm` using the first entry's `ts_ms`).
//!
//! All strings are copied into the log's arena and stay valid until `clear()`/`deinit()`.

const std = @import("std");
const Allocator = std.mem.Allocator;
const types = @import("types.zig");
const session = @import("session.zig");
const tools = @import("tools.zig");
const jsonw = @import("jsonw.zig");

pub const ToolState = enum { running, ok, failed };

pub const Tool = struct {
    id: []const u8,
    name: []const u8,
    /// Raw op / args JSON (use `jsonw.pretty` to show expanded).
    input_json: []const u8,
    /// `APPLY 6 OPS`, `RENDER VIEW A`, `INSPECT COMPONENT truss`.
    title: []const u8,
    state: ToolState = .running,
    n_err: u32 = 0,
    n_warn: u32 = 0,
    has_image: bool = false,
    /// Engine reply text (expandable).
    result_text: []const u8 = "",
    /// UI-owned expand/collapse flag; toggle with `ChatLog.toggleExpanded`.
    expanded: bool = false,
};

pub const Body = union(enum) {
    designer: struct { text: []const u8, n_images: u32, edits_note: ?[]const u8 },
    assistant_text: []const u8,
    tool: Tool,
    /// Console line (`▸ FALLBACK ...`, retry notices, `CANCELLED`).
    notice: []const u8,
    err: struct { kind: types.ErrorKind, message: []const u8 },
    refusal: []const u8,
};

pub const Entry = struct {
    ts_ms: i64,
    group: u32,
    body: Body,
};

pub const StatusKind = enum { no_key, ready, busy, backoff, failed };

pub const Status = struct {
    kind: StatusKind = .ready,
    round: u32 = 0,
    retry_in_ms: u32 = 0,
};

pub const ChatLog = struct {
    gpa: Allocator,
    arena: std.heap.ArenaAllocator,
    entries: std.ArrayList(Entry) = .empty,
    status: Status = .{},
    group: u32 = 0,
    last_was_designer: bool = false,

    pub fn init(gpa: Allocator) ChatLog {
        return .{ .gpa = gpa, .arena = std.heap.ArenaAllocator.init(gpa) };
    }

    pub fn deinit(self: *ChatLog) void {
        self.arena.deinit();
    }

    pub fn clear(self: *ChatLog) void {
        self.entries = .empty;
        _ = self.arena.reset(.retain_capacity);
        self.status = .{};
        self.group = 0;
        self.last_was_designer = false;
    }

    fn dup(self: *ChatLog, s: []const u8) Allocator.Error![]const u8 {
        return self.arena.allocator().dupe(u8, s);
    }

    fn push(self: *ChatLog, ts_ms: i64, body: Body) Allocator.Error!void {
        const is_designer = body == .designer;
        if (is_designer) {
            self.group += 1;
            self.last_was_designer = true;
        } else if (self.last_was_designer or self.group == 0) {
            self.group += 1;
            self.last_was_designer = false;
        }
        try self.entries.append(self.arena.allocator(), .{ .ts_ms = ts_ms, .group = self.group, .body = body });
    }

    /// Add an app-level console line (e.g. `EXPORTED TRUSS-BEARING-CMU-A.DXF (48 KB)`).
    pub fn addNotice(self: *ChatLog, now_ms: i64, text: []const u8) Allocator.Error!void {
        try self.push(now_ms, .{ .notice = try self.dup(text) });
    }

    pub fn setHasKey(self: *ChatLog, has_key: bool) void {
        if (!has_key) {
            self.status = .{ .kind = .no_key };
        } else if (self.status.kind == .no_key) {
            self.status = .{ .kind = .ready };
        }
    }

    pub fn toggleExpanded(self: *ChatLog, index: usize) void {
        if (index >= self.entries.items.len) return;
        switch (self.entries.items[index].body) {
            .tool => |*t| t.expanded = !t.expanded,
            else => {},
        }
    }

    /// Fold the Session events of ONE call into the log, stamping new entries with `now_ms`.
    pub fn ingest(self: *ChatLog, now_ms: i64, events: []const session.Event) Allocator.Error!void {
        for (events) |e| switch (e) {
            .designer_message => |d| try self.push(now_ms, .{ .designer = .{
                .text = try self.dup(d.text),
                .n_images = d.n_images,
                .edits_note = if (d.edits_note) |n| try self.dup(n) else null,
            } }),
            .assistant_text => |t| try self.push(now_ms, .{ .assistant_text = try self.dup(t) }),
            .thinking => {},
            .tool_call => |tu| {
                const title = try tools.activityTitle(self.arena.allocator(), tu.name, tu.input_json);
                try self.push(now_ms, .{ .tool = .{
                    .id = try self.dup(tu.id),
                    .name = try self.dup(tu.name),
                    .input_json = try self.dup(tu.input_json),
                    .title = title,
                } });
            },
            .tool_done => |d| {
                var i = self.entries.items.len;
                while (i > 0) {
                    i -= 1;
                    switch (self.entries.items[i].body) {
                        .tool => |*t| if (std.mem.eql(u8, t.id, d.id)) {
                            t.state = if (d.is_error) .failed else .ok;
                            t.n_err = d.n_err;
                            t.n_warn = d.n_warn;
                            t.has_image = d.has_image;
                            t.result_text = try self.dup(d.text);
                            break;
                        },
                        else => {},
                    }
                }
            },
            .fallback => |f| {
                const line = try std.fmt.allocPrint(self.arena.allocator(), "▸ FALLBACK {s} → {s}", .{ f.from, f.to });
                try self.push(now_ms, .{ .notice = line });
            },
            .fallbacks_disabled => try self.push(now_ms, .{ .notice = "▸ API REJECTED THE FALLBACKS PARAMETER; CONTINUING WITHOUT IT" }),
            .retry_scheduled => |r| {
                const line = try std.fmt.allocPrint(
                    self.arena.allocator(),
                    "▸ {s}; RETRY {d}/3 IN {d} S",
                    .{ if (r.status == 429) "RATE LIMITED" else "API OVERLOADED", r.attempt, r.delay_ms / 1000 },
                );
                try self.push(now_ms, .{ .notice = line });
            },
            .usage => {},
            .phase => |p| self.status = switch (p) {
                .ready => .{ .kind = .ready },
                .busy => |n| .{ .kind = .busy, .round = n },
                .running_tools => |n| .{ .kind = .busy, .round = n },
                .backoff => |b| .{ .kind = .backoff, .round = self.status.round, .retry_in_ms = b.delay_ms },
                .failed => .{ .kind = .failed },
            },
            .err => |er| try self.push(now_ms, .{ .err = .{ .kind = er.kind, .message = try self.dup(er.message) } }),
            .refusal => |r| try self.push(now_ms, .{ .refusal = try self.dup(r) }),
            .notice => |n| try self.push(now_ms, .{ .notice = try self.dup(n) }),
        };
    }

    /// Status-line text for the LLM segment: `CLAUDE OK`, `CLAUDE BUSY ◐ ROUND 2`, `NO KEY`, ...
    pub fn statusText(self: *const ChatLog, buf: []u8) []const u8 {
        const s = self.status;
        const r = switch (s.kind) {
            .no_key => std.fmt.bufPrint(buf, "NO KEY", .{}),
            .ready => std.fmt.bufPrint(buf, "CLAUDE OK", .{}),
            .busy => std.fmt.bufPrint(buf, "CLAUDE BUSY ◐ ROUND {d}", .{s.round}),
            .backoff => std.fmt.bufPrint(buf, "CLAUDE BUSY ◐ ROUND {d} RETRY IN {d} S", .{ s.round, s.retry_in_ms / 1000 }),
            .failed => std.fmt.bufPrint(buf, "CLAUDE ERROR", .{}),
        };
        return r catch buf[0..0];
    }

    pub fn isBusy(self: *const ChatLog) bool {
        return self.status.kind == .busy or self.status.kind == .backoff;
    }
};

/// One-line tool activity text: `▸ APPLY 6 OPS ✓ 0 ERR 1 WARN`, `▸ RENDER VIEW A ✓`, `▸ INSPECT SUMMARY ◐`.
/// Truncates (never fails) if `buf` is too small; 256 bytes is plenty.
pub fn toolLine(t: Tool, buf: []u8) []const u8 {
    const mark = switch (t.state) {
        .running => "◐",
        .ok => "✓",
        .failed => "✗",
    };
    const is_apply = std.mem.eql(u8, t.name, "kerf_apply");
    const r = if (is_apply and t.state != .running)
        std.fmt.bufPrint(buf, "▸ {s} {s} {d} ERR {d} WARN", .{ t.title, mark, t.n_err, t.n_warn })
    else
        std.fmt.bufPrint(buf, "▸ {s} {s}", .{ t.title, mark });
    return r catch buf;
}

/// `15:42` from a UTC millisecond timestamp and the viewer's UTC offset in minutes.
pub fn formatClock(ts_ms: i64, tz_offset_min: i32, buf: *[5]u8) []const u8 {
    const secs = @divFloor(ts_ms, 1000) + @as(i64, tz_offset_min) * 60;
    const day = @mod(secs, 86400);
    const h: u8 = @intCast(@divFloor(day, 3600));
    const m: u8 = @intCast(@divFloor(@mod(day, 3600), 60));
    buf[0] = '0' + h / 10;
    buf[1] = '0' + h % 10;
    buf[2] = ':';
    buf[3] = '0' + m / 10;
    buf[4] = '0' + m % 10;
    return buf;
}

test "clock" {
    var b: [5]u8 = undefined;
    // 2026-10-05 15:42:00 UTC
    try std.testing.expectEqualStrings("15:42", formatClock(1791214920000, 0, &b));
    try std.testing.expectEqualStrings("08:42", formatClock(1791214920000, -420, &b));
}
