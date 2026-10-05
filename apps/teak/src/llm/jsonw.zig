//! Tiny allocation-backed JSON text builder (no std.Io): `Buf`.
//!
//! Every method returns `error.OutOfMemory` only. Strings are escaped precisely: `"` `\` and all
//! control bytes (< 0x20) are escaped, U+2028/U+2029 are escaped (they break JS eval), valid UTF-8
//! passes through untouched, and any invalid UTF-8 byte is replaced by U+FFFD so the output is
//! always valid JSON text.

const std = @import("std");
const Allocator = std.mem.Allocator;

pub const Buf = struct {
    a: Allocator,
    list: std.ArrayList(u8) = .empty,

    pub fn init(a: Allocator) Buf {
        return .{ .a = a };
    }

    pub fn deinit(self: *Buf) void {
        self.list.deinit(self.a);
    }

    pub fn items(self: *const Buf) []const u8 {
        return self.list.items;
    }

    pub fn toOwnedSlice(self: *Buf) Allocator.Error![]u8 {
        return self.list.toOwnedSlice(self.a);
    }

    pub fn reserve(self: *Buf, n: usize) Allocator.Error!void {
        try self.list.ensureUnusedCapacity(self.a, n);
    }

    /// Append raw bytes (caller guarantees they are valid JSON text).
    pub fn raw(self: *Buf, s: []const u8) Allocator.Error!void {
        try self.list.appendSlice(self.a, s);
    }

    pub fn byte(self: *Buf, c: u8) Allocator.Error!void {
        try self.list.append(self.a, c);
    }

    pub fn print(self: *Buf, comptime fmt: []const u8, args: anytype) Allocator.Error!void {
        var tmp: [64]u8 = undefined;
        const s = std.fmt.bufPrint(&tmp, fmt, args) catch {
            // Rare: fall back to an allocation.
            const big = try std.fmt.allocPrint(self.a, fmt, args);
            defer self.a.free(big);
            return self.raw(big);
        };
        try self.raw(s);
    }

    /// Append `s` as a quoted, escaped JSON string.
    pub fn str(self: *Buf, s: []const u8) Allocator.Error!void {
        try self.byte('"');
        try self.strChars(s);
        try self.byte('"');
    }

    /// Append the escaped characters of `s` (no quotes).
    pub fn strChars(self: *Buf, s: []const u8) Allocator.Error!void {
        const hex = "0123456789abcdef";
        var run: usize = 0;
        var i: usize = 0;
        while (i < s.len) {
            const c = s[i];
            if (c >= 0x20 and c != '"' and c != '\\' and c < 0x80) {
                i += 1;
                continue;
            }
            if (c >= 0x80) {
                const n = std.unicode.utf8ByteSequenceLength(c) catch 0;
                if (n != 0 and i + n <= s.len) {
                    if (std.unicode.utf8Decode(s[i .. i + n])) |cp| {
                        if (cp == 0x2028 or cp == 0x2029) {
                            try self.raw(s[run..i]);
                            try self.raw(if (cp == 0x2028) "\\u2028" else "\\u2029");
                            i += n;
                            run = i;
                        } else {
                            i += n;
                        }
                        continue;
                    } else |_| {}
                }
                // invalid UTF-8: replace
                try self.raw(s[run..i]);
                try self.raw("\u{FFFD}");
                i += 1;
                run = i;
                continue;
            }
            try self.raw(s[run..i]);
            switch (c) {
                '"' => try self.raw("\\\""),
                '\\' => try self.raw("\\\\"),
                '\n' => try self.raw("\\n"),
                '\r' => try self.raw("\\r"),
                '\t' => try self.raw("\\t"),
                0x08 => try self.raw("\\b"),
                0x0C => try self.raw("\\f"),
                else => {
                    try self.raw("\\u00");
                    try self.byte(hex[c >> 4]);
                    try self.byte(hex[c & 15]);
                },
            }
            i += 1;
            run = i;
        }
        try self.raw(s[run..]);
    }

    /// Append `bytes` as base64 text (standard alphabet, padded), no quotes.
    pub fn base64(self: *Buf, bytes: []const u8) Allocator.Error!void {
        const enc = std.base64.standard.Encoder;
        const n = enc.calcSize(bytes.len);
        try self.list.ensureUnusedCapacity(self.a, n);
        const dst = self.list.unusedCapacitySlice()[0..n];
        _ = enc.encode(dst, bytes);
        self.list.items.len += n;
    }

    pub fn uint(self: *Buf, v: anytype) Allocator.Error!void {
        try self.print("{d}", .{v});
    }
};

/// Strip insignificant whitespace from valid JSON text (outside strings). Output is allocated.
pub fn minify(a: Allocator, text: []const u8) Allocator.Error![]u8 {
    var out = Buf.init(a);
    errdefer out.deinit();
    try out.reserve(text.len);
    var i: usize = 0;
    var in_str = false;
    while (i < text.len) : (i += 1) {
        const c = text[i];
        if (in_str) {
            try out.byte(c);
            if (c == '\\' and i + 1 < text.len) {
                i += 1;
                try out.byte(text[i]);
            } else if (c == '"') in_str = false;
        } else switch (c) {
            ' ', '\t', '\n', '\r' => {},
            '"' => {
                in_str = true;
                try out.byte(c);
            },
            else => try out.byte(c),
        }
    }
    return out.toOwnedSlice();
}

/// Comptime variant of `minify` (used to embed the tools JSON / demo documents).
pub fn minifyComptime(comptime text: []const u8) [minifiedLen(text)]u8 {
    comptime {
        @setEvalBranchQuota(text.len * 40 + 1000);
        var out: [minifiedLen(text)]u8 = undefined;
        var n: usize = 0;
        var i: usize = 0;
        var in_str = false;
        while (i < text.len) : (i += 1) {
            const c = text[i];
            if (in_str) {
                out[n] = c;
                n += 1;
                if (c == '\\' and i + 1 < text.len) {
                    i += 1;
                    out[n] = text[i];
                    n += 1;
                } else if (c == '"') in_str = false;
            } else switch (c) {
                ' ', '\t', '\n', '\r' => {},
                else => {
                    if (c == '"') in_str = true;
                    out[n] = c;
                    n += 1;
                },
            }
        }
        return out;
    }
}

fn minifiedLen(comptime text: []const u8) usize {
    comptime {
        @setEvalBranchQuota(text.len * 40 + 1000);
        var n: usize = 0;
        var i: usize = 0;
        var in_str = false;
        while (i < text.len) : (i += 1) {
            const c = text[i];
            if (in_str) {
                n += 1;
                if (c == '\\' and i + 1 < text.len) {
                    i += 1;
                    n += 1;
                } else if (c == '"') in_str = false;
            } else switch (c) {
                ' ', '\t', '\n', '\r' => {},
                else => {
                    if (c == '"') in_str = true;
                    n += 1;
                },
            }
        }
        return n;
    }
}

test "string escaping" {
    var b = Buf.init(std.testing.allocator);
    defer b.deinit();
    try b.str("a\"b\\c\nd\te\x01f\u{00e9}\u{4e2d}\u{1F600}\u{2028}");
    try std.testing.expectEqualStrings(
        "\"a\\\"b\\\\c\\nd\\te\\u0001f\u{00e9}\u{4e2d}\u{1F600}\\u2028\"",
        b.items(),
    );
}

test "invalid utf8 replaced" {
    var b = Buf.init(std.testing.allocator);
    defer b.deinit();
    try b.str("x\xffy\xc3");
    try std.testing.expectEqualStrings("\"x\u{FFFD}y\u{FFFD}\"", b.items());
}

test "escaped output is valid json and round-trips" {
    const a = std.testing.allocator;
    const src = "q\" b\\ nl\n tab\t ctl\x07 \u{00fc}\u{1F600}";
    var b = Buf.init(a);
    defer b.deinit();
    try b.str(src);
    const parsed = try std.json.parseFromSlice([]const u8, a, b.items(), .{});
    defer parsed.deinit();
    try std.testing.expectEqualStrings(src, parsed.value);
}

test "base64 buf" {
    var b = Buf.init(std.testing.allocator);
    defer b.deinit();
    try b.base64("hello!?");
    try std.testing.expectEqualStrings("aGVsbG8hPw==", b.items());
}

test "minify" {
    const a = std.testing.allocator;
    const m = try minify(a, "{ \"a b\" : [1, 2,\n 3], \"c\": \"x \\\" y\" }");
    defer a.free(m);
    try std.testing.expectEqualStrings("{\"a b\":[1,2,3],\"c\":\"x \\\" y\"}", m);
    const c = comptime minifyComptime("{ \"a b\" : [1, 2,\n 3], \"c\": \"x \\\" y\" }");
    try std.testing.expectEqualStrings("{\"a b\":[1,2,3],\"c\":\"x \\\" y\"}", &c);
}

/// Pretty-print valid JSON text with 2-space indentation (for the expandable op JSON in the
/// console). Strings are copied verbatim; empty `{}`/`[]` stay compact.
pub fn pretty(a: Allocator, text: []const u8) Allocator.Error![]u8 {
    var out = Buf.init(a);
    errdefer out.deinit();
    var depth: usize = 0;
    var i: usize = 0;
    const indent = struct {
        fn f(b: *Buf, d: usize) Allocator.Error!void {
            try b.byte('\n');
            var k: usize = 0;
            while (k < d) : (k += 1) try b.raw("  ");
        }
    }.f;
    while (i < text.len) : (i += 1) {
        const c = text[i];
        switch (c) {
            ' ', '\t', '\n', '\r' => {},
            '"' => {
                const start = i;
                i += 1;
                while (i < text.len) : (i += 1) {
                    if (text[i] == '\\') i += 1 else if (text[i] == '"') break;
                }
                try out.raw(text[start..@min(i + 1, text.len)]);
            },
            '{', '[' => {
                const close: u8 = if (c == '{') '}' else ']';
                var j = i + 1;
                while (j < text.len and (text[j] == ' ' or text[j] == '\n' or text[j] == '\t' or text[j] == '\r')) j += 1;
                if (j < text.len and text[j] == close) {
                    try out.byte(c);
                    try out.byte(close);
                    i = j;
                } else {
                    try out.byte(c);
                    depth += 1;
                    try indent(&out, depth);
                }
            },
            '}', ']' => {
                depth -|= 1;
                try indent(&out, depth);
                try out.byte(c);
            },
            ',' => {
                try out.byte(',');
                try indent(&out, depth);
            },
            ':' => try out.raw(": "),
            else => try out.byte(c),
        }
    }
    return out.toOwnedSlice();
}

test "pretty" {
    const a = std.testing.allocator;
    const p = try pretty(a, "{\"a\":[1,{\"b\":\"x, y\"}],\"c\":{},\"d\":[]}");
    defer a.free(p);
    try std.testing.expectEqualStrings(
        "{\n  \"a\": [\n    1,\n    {\n      \"b\": \"x, y\"\n    }\n  ],\n  \"c\": {},\n  \"d\": []\n}",
        p,
    );
}
