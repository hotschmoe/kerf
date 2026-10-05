//! Engine boundary: SPEC §13 as an in-process function.
//!
//! Every engine call is `name + JSON in -> JSON out`. The app never links the
//! engine's internals; it talks through this vtable so a fixture engine (used
//! in tests and before the real engine slice lands) and the real Zig engine
//! are interchangeable.

const std = @import("std");
const Allocator = std.mem.Allocator;

pub const CallResult = union(enum) {
    /// Owned output JSON (or raw bytes for `export`).
    ok: []u8,
    /// Owned error JSON / message.
    err: []u8,

    pub fn deinit(self: CallResult, alloc: Allocator) void {
        switch (self) {
            .ok, .err => |b| alloc.free(b),
        }
    }
};

pub const Engine = struct {
    ctx: *anyopaque,
    call_fn: *const fn (ctx: *anyopaque, alloc: Allocator, name: []const u8, input: []const u8) CallResult,
    name: []const u8 = "engine",

    pub fn call(self: Engine, alloc: Allocator, fn_name: []const u8, input: []const u8) CallResult {
        return self.call_fn(self.ctx, alloc, fn_name, input);
    }
};

/// Writes the JSON request object `{ "doc": <raw>, "style": <raw>, ... }`.
/// `doc` and `style` are spliced in verbatim (they are already JSON text).
pub const Request = struct {
    doc: ?[]const u8 = null,
    style: ?[]const u8 = null,
    /// Raw JSON array text.
    ops: ?[]const u8 = null,
    actor: ?[]const u8 = null,
    view: ?[]const u8 = null,
    format: ?[]const u8 = null,
    sheet: ?bool = null,
    /// Raw JSON object text for `inspect`.
    query: ?[]const u8 = null,

    pub fn build(self: Request, alloc: Allocator) ![]u8 {
        var out: std.Io.Writer.Allocating = .init(alloc);
        errdefer out.deinit();
        const w = &out.writer;
        try w.writeByte('{');
        var first = true;
        inline for (.{ "doc", "style", "ops", "query" }) |k| {
            if (@field(self, k)) |raw| {
                if (!first) try w.writeByte(',');
                first = false;
                try w.print("\"{s}\":{s}", .{ k, raw });
            }
        }
        inline for (.{ "actor", "view", "format" }) |k| {
            if (@field(self, k)) |s| {
                if (!first) try w.writeByte(',');
                first = false;
                try w.print("\"{s}\":", .{k});
                try std.json.Stringify.encodeJsonString(s, .{}, w);
            }
        }
        if (self.sheet) |b| {
            if (!first) try w.writeByte(',');
            try w.print("\"sheet\":{}", .{b});
        }
        try w.writeByte('}');
        return out.toOwnedSlice();
    }
};

test "Request.build splices raw json and escapes strings" {
    const a = std.testing.allocator;
    const body = try (Request{ .doc = "{\"kerf\":\"0.1\"}", .actor = "designer", .view = "A\"" }).build(a);
    defer a.free(body);
    try std.testing.expectEqualStrings("{\"doc\":{\"kerf\":\"0.1\"},\"actor\":\"designer\",\"view\":\"A\\\"\"}", body);
}
