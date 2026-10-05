//! Zero-copy JSON reading by byte spans. All functions assume the input has already been checked
//! with `valid()` (std.json.Scanner.validate); they return `error.Malformed` rather than crash on
//! garbage anyway. The point of spans: a response's `content` array and each tool `input` can be
//! kept as the EXACT bytes the API sent (needed for byte-exact thinking-block round trips).

const std = @import("std");
const Allocator = std.mem.Allocator;

pub const Error = error{Malformed};

pub fn valid(a: Allocator, text: []const u8) Allocator.Error!bool {
    return std.json.Scanner.validate(a, text);
}

fn skipWs(s: []const u8, i_start: usize) usize {
    var i = i_start;
    while (i < s.len and (s[i] == ' ' or s[i] == '\n' or s[i] == '\t' or s[i] == '\r')) i += 1;
    return i;
}

/// `s[start]` must be the first byte of a value; returns the index one past its end.
pub fn valueEnd(s: []const u8, start: usize) Error!usize {
    if (start >= s.len) return error.Malformed;
    switch (s[start]) {
        '"' => return stringEnd(s, start),
        '{', '[' => {
            var depth: usize = 0;
            var i = start;
            while (i < s.len) {
                switch (s[i]) {
                    '"' => {
                        i = try stringEnd(s, i);
                        continue;
                    },
                    '{', '[' => depth += 1,
                    '}', ']' => {
                        depth -= 1;
                        if (depth == 0) return i + 1;
                    },
                    else => {},
                }
                i += 1;
            }
            return error.Malformed;
        },
        else => {
            var i = start;
            while (i < s.len) : (i += 1) switch (s[i]) {
                ',', '}', ']', ' ', '\n', '\t', '\r' => break,
                else => {},
            };
            if (i == start) return error.Malformed;
            return i;
        },
    }
}

fn stringEnd(s: []const u8, start: usize) Error!usize {
    var i = start + 1;
    while (i < s.len) : (i += 1) {
        if (s[i] == '\\') {
            i += 1;
        } else if (s[i] == '"') return i + 1;
    }
    return error.Malformed;
}

/// Trim whitespace around a value span.
pub fn trim(s: []const u8) []const u8 {
    return std.mem.trim(u8, s, " \n\t\r");
}

pub const Member = struct { key: []const u8, value: []const u8 };

/// Iterate the members of an object span. `key` is the raw (still escaped) key text without quotes.
pub const ObjIter = struct {
    s: []const u8,
    i: usize,

    pub fn init(obj: []const u8) Error!ObjIter {
        const s = trim(obj);
        if (s.len < 2 or s[0] != '{') return error.Malformed;
        return .{ .s = s, .i = 1 };
    }

    pub fn next(self: *ObjIter) Error!?Member {
        var i = skipWs(self.s, self.i);
        if (i < self.s.len and self.s[i] == ',') i = skipWs(self.s, i + 1);
        if (i >= self.s.len or self.s[i] == '}') return null;
        if (self.s[i] != '"') return error.Malformed;
        const kend = try stringEnd(self.s, i);
        const key = self.s[i + 1 .. kend - 1];
        i = skipWs(self.s, kend);
        if (i >= self.s.len or self.s[i] != ':') return error.Malformed;
        i = skipWs(self.s, i + 1);
        const vend = try valueEnd(self.s, i);
        self.i = vend;
        return .{ .key = key, .value = self.s[i..vend] };
    }
};

/// Value span of `key` in object `obj`, or null.
pub fn get(obj: []const u8, key: []const u8) Error!?[]const u8 {
    var it = try ObjIter.init(obj);
    while (try it.next()) |m| {
        if (std.mem.eql(u8, m.key, key)) return m.value;
    }
    return null;
}

pub const ArrIter = struct {
    s: []const u8,
    i: usize,

    pub fn init(arr: []const u8) Error!ArrIter {
        const s = trim(arr);
        if (s.len < 2 or s[0] != '[') return error.Malformed;
        return .{ .s = s, .i = 1 };
    }

    pub fn next(self: *ArrIter) Error!?[]const u8 {
        var i = skipWs(self.s, self.i);
        if (i < self.s.len and self.s[i] == ',') i = skipWs(self.s, i + 1);
        if (i >= self.s.len or self.s[i] == ']') return null;
        const vend = try valueEnd(self.s, i);
        self.i = vend;
        return self.s[i..vend];
    }
};

pub fn arrayLen(arr: []const u8) Error!usize {
    var it = try ArrIter.init(arr);
    var n: usize = 0;
    while (try it.next()) |_| n += 1;
    return n;
}

pub fn isString(span: []const u8) bool {
    return span.len >= 2 and span[0] == '"';
}

/// Decode a JSON string span (with quotes) to its text. Borrows from `span` when there are no
/// escapes; otherwise allocates from `a` (use an arena).
pub fn unquote(a: Allocator, span: []const u8) (Error || Allocator.Error)![]const u8 {
    if (!isString(span)) return error.Malformed;
    const inner = span[1 .. span.len - 1];
    if (std.mem.indexOfScalar(u8, inner, '\\') == null) return inner;
    return std.json.parseFromSliceLeaky([]const u8, a, span, .{}) catch |e| switch (e) {
        error.OutOfMemory => return error.OutOfMemory,
        else => return error.Malformed,
    };
}

/// `obj.key` as decoded text if present and a string.
pub fn getString(a: Allocator, obj: []const u8, key: []const u8) (Error || Allocator.Error)!?[]const u8 {
    const v = (try get(obj, key)) orelse return null;
    if (!isString(v)) return null;
    return try unquote(a, v);
}

pub fn getUint(obj: []const u8, key: []const u8) Error!?u64 {
    const v = (try get(obj, key)) orelse return null;
    return std.fmt.parseInt(u64, v, 10) catch null;
}

test "spans" {
    const t = std.testing;
    const src = " {\"a\": [1, {\"x\":\"]}\\\"\"}, 3], \"b\" : \"h\\u00e9llo\", \"c\":null,\"d\":{}} ";
    const a_span = (try get(src, "a")).?;
    try t.expectEqualStrings("[1, {\"x\":\"]}\\\"\"}, 3]", a_span);
    try t.expectEqual(@as(usize, 3), try arrayLen(a_span));
    var arena = std.heap.ArenaAllocator.init(t.allocator);
    defer arena.deinit();
    try t.expectEqualStrings("h\u{e9}llo", (try getString(arena.allocator(), src, "b")).?);
    try t.expectEqualStrings("null", (try get(src, "c")).?);
    try t.expectEqualStrings("{}", (try get(src, "d")).?);
    try t.expect((try get(src, "zzz")) == null);
}
