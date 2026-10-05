//! Small fixed-size identifier (component / annotation / view id) that lives
//! inside the Model without heap ownership.

const std = @import("std");

pub const Id = struct {
    buf: [64]u8 = undefined,
    len: u8 = 0,

    pub fn from(s: []const u8) Id {
        var id: Id = .{};
        const n = @min(s.len, id.buf.len);
        @memcpy(id.buf[0..n], s[0..n]);
        id.len = @intCast(n);
        return id;
    }

    pub fn slice(self: *const Id) []const u8 {
        return self.buf[0..self.len];
    }

    pub fn eql(self: *const Id, s: []const u8) bool {
        return std.mem.eql(u8, self.slice(), s);
    }
};

/// Optional id, comparable and copyable.
pub const MaybeId = struct {
    id: Id = .{},
    set: bool = false,

    pub fn none() MaybeId {
        return .{};
    }
    pub fn of(s: []const u8) MaybeId {
        if (s.len == 0) return .{};
        return .{ .id = Id.from(s), .set = true };
    }
    pub fn get(self: *const MaybeId) ?[]const u8 {
        return if (self.set) self.id.slice() else null;
    }
    pub fn same(self: *const MaybeId, other: *const MaybeId) bool {
        if (self.set != other.set) return false;
        return !self.set or std.mem.eql(u8, self.id.slice(), other.id.slice());
    }
};
