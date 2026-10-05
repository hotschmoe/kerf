//! Mock transport: replays a scripted list of HTTP responses and records every request, so tests
//! (and demo mode) can drive the whole loop with no network.

const std = @import("std");
const Allocator = std.mem.Allocator;
const types = @import("types.zig");

pub const Scripted = struct {
    status: u16 = 200,
    body: []const u8 = "",
    err: ?[]const u8 = null,
};

pub const Recorded = struct {
    body: []u8,
    /// Value of the anthropic-beta header, if sent.
    beta: ?[]u8,
    api_key: []u8,
};

pub const Mock = struct {
    gpa: Allocator,
    script: []const Scripted,
    idx: usize = 0,
    requests: std.ArrayList(Recorded) = .empty,

    pub fn init(gpa: Allocator, script: []const Scripted) Mock {
        return .{ .gpa = gpa, .script = script };
    }

    pub fn deinit(self: *Mock) void {
        for (self.requests.items) |r| {
            self.gpa.free(r.body);
            if (r.beta) |b| self.gpa.free(b);
            self.gpa.free(r.api_key);
        }
        self.requests.deinit(self.gpa);
    }

    pub fn respond(self: *Mock, spec: types.HttpRequestSpec) types.HttpResult {
        self.record(spec) catch {};
        if (self.idx >= self.script.len)
            return .{ .status = 500, .body = "{\"type\":\"error\",\"error\":{\"type\":\"api_error\",\"message\":\"mock script exhausted\"}}" };
        const s = self.script[self.idx];
        self.idx += 1;
        return .{ .status = s.status, .body = s.body, .err = s.err };
    }

    fn record(self: *Mock, spec: types.HttpRequestSpec) Allocator.Error!void {
        var beta: ?[]u8 = null;
        var key: []u8 = try self.gpa.dupe(u8, "");
        for (spec.headers) |h| {
            if (std.mem.eql(u8, h.name, "anthropic-beta")) beta = try self.gpa.dupe(u8, h.value);
            if (std.mem.eql(u8, h.name, "x-api-key")) {
                self.gpa.free(key);
                key = try self.gpa.dupe(u8, h.value);
            }
        }
        try self.requests.append(self.gpa, .{ .body = try self.gpa.dupe(u8, spec.body_json), .beta = beta, .api_key = key });
    }
};
