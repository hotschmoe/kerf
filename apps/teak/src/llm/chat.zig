//! `Chat`: convenience facade = `Session` + `ChatLog`. Each call forwards to the session and folds
//! the resulting events into the log with the caller-supplied timestamp, so the app has one place
//! to call from `update`. The wrapped `session` / `log` stay public for direct use.

const std = @import("std");
const Allocator = std.mem.Allocator;
const types = @import("types.zig");
const session_mod = @import("session.zig");
const chatlog = @import("chatlog.zig");

pub const Chat = struct {
    session: session_mod.Session,
    log: chatlog.ChatLog,

    pub fn init(gpa: Allocator, cfg: types.Config) Chat {
        var c: Chat = .{ .session = session_mod.Session.init(gpa, cfg), .log = chatlog.ChatLog.init(gpa) };
        c.log.setHasKey(cfg.api_key.len != 0);
        return c;
    }

    pub fn deinit(self: *Chat) void {
        self.session.deinit();
        self.log.deinit();
    }

    fn fold(self: *Chat, now_ms: i64, step: session_mod.Step) Allocator.Error!session_mod.Step {
        try self.log.ingest(now_ms, self.session.events());
        return step;
    }

    pub fn setApiKey(self: *Chat, key: []const u8) void {
        self.session.cfg.api_key = key;
        self.log.setHasKey(key.len != 0);
    }

    pub fn userSubmit(self: *Chat, now_ms: i64, text: []const u8, images: []const types.Image, edits_note: ?[]const u8) Allocator.Error!session_mod.Step {
        return self.fold(now_ms, try self.session.userSubmit(text, images, edits_note));
    }

    pub fn onHttp(self: *Chat, now_ms: i64, r: types.HttpResult) Allocator.Error!session_mod.Step {
        return self.fold(now_ms, try self.session.onHttp(r));
    }

    pub fn onToolResults(self: *Chat, now_ms: i64, results: []const types.ToolResult) Allocator.Error!session_mod.Step {
        return self.fold(now_ms, try self.session.onToolResults(results));
    }

    pub fn retry(self: *Chat, now_ms: i64) Allocator.Error!session_mod.Step {
        return self.fold(now_ms, try self.session.retry());
    }

    pub fn cancel(self: *Chat, now_ms: i64) Allocator.Error!session_mod.Step {
        return self.fold(now_ms, try self.session.cancel());
    }
};
