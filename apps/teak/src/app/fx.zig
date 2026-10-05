//! Pending-effect queue: the app's side of teak's declarative effects.
//!
//! `update` pushes an effect (an id is assigned here); `teak.run` executes
//! every listed id once; the result arrives as a Msg and `done(id)` removes
//! the entry and says what it was for. Strings inside queued effects are
//! copied into a private arena that is recycled whenever the queue drains.

const std = @import("std");
const teak = @import("teak");
const alloc = @import("alloc.zig");

pub const Kind = enum {
    chat_http,
    key_load,
    key_store,
    settings_load,
    settings_store,
    clock,
    query_sample,
    query_demo,
    query_tab,
    query_select,
    query_insp,
    query_prompt,
    open_doc,
    attach_image,
    download,
    copy,
};

pub const CAP = 12;

pub const Queue = struct {
    items: [CAP]teak.Effect = undefined,
    kinds: [CAP]Kind = undefined,
    len: usize = 0,
    next_id: u32 = 1,
    arena: ?std.heap.ArenaAllocator = null,

    fn a(self: *Queue) std.mem.Allocator {
        if (self.arena == null) self.arena = std.heap.ArenaAllocator.init(alloc.gpa);
        return self.arena.?.allocator();
    }

    pub fn list(self: *const Queue) []const teak.Effect {
        return self.items[0..self.len];
    }

    fn take(self: *Queue, kind: Kind) ?usize {
        if (self.len >= CAP) return null;
        const i = self.len;
        self.kinds[i] = kind;
        self.len += 1;
        return i;
    }

    fn newId(self: *Queue) u32 {
        const id = self.next_id;
        self.next_id +%= 1;
        if (self.next_id == 0) self.next_id = 1;
        return id;
    }

    pub fn http(self: *Queue, kind: Kind, method: teak.HttpMethod, url: []const u8, headers: []const teak.Header, body: []const u8, timeout_ms: u32) ?u32 {
        const ar = self.a();
        const i = self.take(kind) orelse return null;
        const id = self.newId();
        const hs = ar.alloc(teak.Header, headers.len) catch return self.drop(i);
        for (headers, 0..) |h, k| hs[k] = .{ .name = ar.dupe(u8, h.name) catch return self.drop(i), .value = ar.dupe(u8, h.value) catch return self.drop(i) };
        self.items[i] = .{ .http = .{
            .id = id,
            .method = method,
            .url = ar.dupe(u8, url) catch return self.drop(i),
            .headers = hs,
            .body = ar.dupe(u8, body) catch return self.drop(i),
            .timeout_ms = timeout_ms,
        } };
        return id;
    }

    pub fn storageGet(self: *Queue, kind: Kind, key: []const u8) ?u32 {
        const i = self.take(kind) orelse return null;
        const id = self.newId();
        self.items[i] = .{ .storage_get = .{ .id = id, .key = self.a().dupe(u8, key) catch return self.drop(i) } };
        return id;
    }

    pub fn storageSet(self: *Queue, kind: Kind, key: []const u8, value: []const u8) ?u32 {
        const i = self.take(kind) orelse return null;
        const id = self.newId();
        const ar = self.a();
        self.items[i] = .{ .storage_set = .{
            .id = id,
            .key = ar.dupe(u8, key) catch return self.drop(i),
            .value = ar.dupe(u8, value) catch return self.drop(i),
        } };
        return id;
    }

    pub fn queryParam(self: *Queue, kind: Kind, name: []const u8) ?u32 {
        const i = self.take(kind) orelse return null;
        const id = self.newId();
        self.items[i] = .{ .query_param = .{ .id = id, .name = self.a().dupe(u8, name) catch return self.drop(i) } };
        return id;
    }

    pub fn clock(self: *Queue) ?u32 {
        const i = self.take(.clock) orelse return null;
        const id = self.newId();
        self.items[i] = .{ .clock = .{ .id = id } };
        return id;
    }

    pub fn openFile(self: *Queue, kind: Kind, accept: []const u8) ?u32 {
        const i = self.take(kind) orelse return null;
        const id = self.newId();
        self.items[i] = .{ .open_file = .{ .id = id, .accept = self.a().dupe(u8, accept) catch return self.drop(i) } };
        return id;
    }

    pub fn download(self: *Queue, name: []const u8, mime: []const u8, bytes: []const u8) ?u32 {
        const i = self.take(.download) orelse return null;
        const id = self.newId();
        const ar = self.a();
        self.items[i] = .{ .download = .{
            .id = id,
            .name = ar.dupe(u8, name) catch return self.drop(i),
            .mime = ar.dupe(u8, mime) catch return self.drop(i),
            .bytes = ar.dupe(u8, bytes) catch return self.drop(i),
        } };
        return id;
    }

    pub fn copy(self: *Queue, text: []const u8) ?u32 {
        const i = self.take(.copy) orelse return null;
        const id = self.newId();
        self.items[i] = .{ .write_clipboard = .{ .id = id, .text = self.a().dupe(u8, text) catch return self.drop(i) } };
        return id;
    }

    fn drop(self: *Queue, i: usize) ?u32 {
        std.debug.assert(i == self.len - 1);
        self.len -= 1;
        return null;
    }

    fn idOf(e: teak.Effect) u32 {
        return switch (e) {
            inline else => |p| p.id,
        };
    }

    /// Remove the effect with `id`; returns what it was for.
    pub fn done(self: *Queue, id: u32) ?Kind {
        var i: usize = 0;
        while (i < self.len) : (i += 1) {
            if (idOf(self.items[i]) != id) continue;
            const k = self.kinds[i];
            var j = i;
            while (j + 1 < self.len) : (j += 1) {
                self.items[j] = self.items[j + 1];
                self.kinds[j] = self.kinds[j + 1];
            }
            self.len -= 1;
            if (self.len == 0) if (self.arena) |*ar| {
                _ = ar.reset(.retain_capacity);
            };
            return k;
        }
        return null;
    }

    pub fn kindOf(self: *const Queue, id: u32) ?Kind {
        for (self.items[0..self.len], 0..) |e, i| if (idOf(e) == id) return self.kinds[i];
        return null;
    }

    /// Drop fire-and-forget entries (no result is delivered for them) once
    /// the runtime has had a frame to issue them.
    pub fn pruneFireAndForget(self: *Queue) void {
        var i: usize = 0;
        while (i < self.len) {
            const ff = switch (self.kinds[i]) {
                .key_store, .settings_store, .copy => true,
                else => false,
            };
            if (ff) {
                _ = self.done(idOf(self.items[i]));
            } else i += 1;
        }
    }
};
