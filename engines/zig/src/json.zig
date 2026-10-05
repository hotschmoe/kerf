//! Minimal order-preserving JSON: a value tree, a strict parser with line/column errors, and a
//! writer that formats numbers by the Kerf rule (round to 1e-4, shortest decimal, `-0` -> `0`).
//! Everything allocates from the caller's (arena) allocator and never frees individually.

const std = @import("std");
const Allocator = std.mem.Allocator;

pub const Member = struct { key: []const u8, value: Value };

pub const Value = union(enum) {
    null,
    bool: bool,
    number: f64,
    string: []const u8,
    array: []Value,
    object: []Member,

    pub fn get(self: Value, key: []const u8) ?Value {
        if (self != .object) return null;
        for (self.object) |m| if (std.mem.eql(u8, m.key, key)) return m.value;
        return null;
    }
    pub fn getPtr(self: *Value, key: []const u8) ?*Value {
        if (self.* != .object) return null;
        for (self.object) |*m| if (std.mem.eql(u8, m.key, key)) return &m.value;
        return null;
    }
    pub fn str(self: Value) ?[]const u8 {
        return if (self == .string) self.string else null;
    }
    pub fn num(self: Value) ?f64 {
        return if (self == .number) self.number else null;
    }
    pub fn arr(self: Value) ?[]Value {
        return if (self == .array) self.array else null;
    }
    pub fn isNull(self: Value) bool {
        return self == .null;
    }
    /// The type name used in error messages.
    pub fn kindName(self: Value) []const u8 {
        return switch (self) {
            .null => "null",
            .bool => "boolean",
            .number => "number",
            .string => "string",
            .array => "array",
            .object => "object",
        };
    }
};

pub const ParseError = struct {
    line: u32,
    col: u32,
    msg: []const u8,
};

pub const Parser = struct {
    src: []const u8,
    pos: usize = 0,
    alloc: Allocator,
    err: ?ParseError = null,
    depth: u32 = 0,

    pub const Error = error{ Syntax, OutOfMemory };

    fn fail(self: *Parser, msg: []const u8) Error {
        var line: u32 = 1;
        var col: u32 = 1;
        const end = @min(self.pos, self.src.len);
        for (self.src[0..end]) |c| {
            if (c == '\n') {
                line += 1;
                col = 1;
            } else col += 1;
        }
        self.err = .{ .line = line, .col = col, .msg = msg };
        return error.Syntax;
    }

    fn ws(self: *Parser) void {
        while (self.pos < self.src.len) : (self.pos += 1) {
            switch (self.src[self.pos]) {
                ' ', '\t', '\n', '\r' => {},
                else => return,
            }
        }
    }

    pub fn parseDocument(self: *Parser) Error!Value {
        // Skip a UTF-8 BOM.
        if (std.mem.startsWith(u8, self.src, "\xEF\xBB\xBF")) self.pos = 3;
        self.ws();
        const v = try self.value();
        self.ws();
        if (self.pos != self.src.len) return self.fail("unexpected trailing characters after the JSON value");
        return v;
    }

    fn value(self: *Parser) Error!Value {
        if (self.pos >= self.src.len) return self.fail("unexpected end of input");
        const c = self.src[self.pos];
        switch (c) {
            '{' => return self.object(),
            '[' => return self.array(),
            '"' => return .{ .string = try self.string() },
            't' => return self.lit("true", .{ .bool = true }),
            'f' => return self.lit("false", .{ .bool = false }),
            'n' => return self.lit("null", .null),
            '-', '0'...'9' => return self.number(),
            else => return self.fail("unexpected character; expected a JSON value"),
        }
    }

    fn lit(self: *Parser, word: []const u8, v: Value) Error!Value {
        if (std.mem.startsWith(u8, self.src[self.pos..], word)) {
            self.pos += word.len;
            return v;
        }
        return self.fail("invalid literal");
    }

    fn number(self: *Parser) Error!Value {
        const start = self.pos;
        if (self.src[self.pos] == '-') self.pos += 1;
        var digits: usize = 0;
        while (self.pos < self.src.len and std.ascii.isDigit(self.src[self.pos])) : (self.pos += 1) digits += 1;
        if (digits == 0) return self.fail("invalid number");
        if (self.pos < self.src.len and self.src[self.pos] == '.') {
            self.pos += 1;
            var fd: usize = 0;
            while (self.pos < self.src.len and std.ascii.isDigit(self.src[self.pos])) : (self.pos += 1) fd += 1;
            if (fd == 0) return self.fail("invalid number: digits expected after '.'");
        }
        if (self.pos < self.src.len and (self.src[self.pos] == 'e' or self.src[self.pos] == 'E')) {
            self.pos += 1;
            if (self.pos < self.src.len and (self.src[self.pos] == '+' or self.src[self.pos] == '-')) self.pos += 1;
            var ed: usize = 0;
            while (self.pos < self.src.len and std.ascii.isDigit(self.src[self.pos])) : (self.pos += 1) ed += 1;
            if (ed == 0) return self.fail("invalid number exponent");
        }
        const f = std.fmt.parseFloat(f64, self.src[start..self.pos]) catch return self.fail("invalid number");
        return .{ .number = f };
    }

    fn hex4(self: *Parser) Error!u21 {
        if (self.pos + 4 > self.src.len) return self.fail("truncated \\u escape");
        var v: u21 = 0;
        for (self.src[self.pos .. self.pos + 4]) |c| {
            const d: u21 = switch (c) {
                '0'...'9' => c - '0',
                'a'...'f' => c - 'a' + 10,
                'A'...'F' => c - 'A' + 10,
                else => return self.fail("invalid \\u escape"),
            };
            v = v * 16 + d;
        }
        self.pos += 4;
        return v;
    }

    fn string(self: *Parser) Error![]const u8 {
        std.debug.assert(self.src[self.pos] == '"');
        self.pos += 1;
        const start = self.pos;
        // Fast path: no escapes.
        while (self.pos < self.src.len) : (self.pos += 1) {
            const c = self.src[self.pos];
            if (c == '"') {
                const s = self.src[start..self.pos];
                self.pos += 1;
                return s;
            }
            if (c == '\\') break;
            if (c < 0x20) return self.fail("control character in string");
        }
        if (self.pos >= self.src.len) return self.fail("unterminated string");
        var buf: std.ArrayList(u8) = .empty;
        try buf.appendSlice(self.alloc, self.src[start..self.pos]);
        while (self.pos < self.src.len) {
            const c = self.src[self.pos];
            if (c == '"') {
                self.pos += 1;
                return buf.items;
            }
            if (c < 0x20) return self.fail("control character in string");
            if (c != '\\') {
                try buf.append(self.alloc, c);
                self.pos += 1;
                continue;
            }
            self.pos += 1;
            if (self.pos >= self.src.len) break;
            const e = self.src[self.pos];
            self.pos += 1;
            switch (e) {
                '"' => try buf.append(self.alloc, '"'),
                '\\' => try buf.append(self.alloc, '\\'),
                '/' => try buf.append(self.alloc, '/'),
                'b' => try buf.append(self.alloc, 8),
                'f' => try buf.append(self.alloc, 12),
                'n' => try buf.append(self.alloc, '\n'),
                'r' => try buf.append(self.alloc, '\r'),
                't' => try buf.append(self.alloc, '\t'),
                'u' => {
                    var cp = try self.hex4();
                    if (cp >= 0xD800 and cp < 0xDC00) {
                        if (self.pos + 2 <= self.src.len and self.src[self.pos] == '\\' and self.src[self.pos + 1] == 'u') {
                            self.pos += 2;
                            const lo = try self.hex4();
                            if (lo >= 0xDC00 and lo < 0xE000) {
                                cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                            } else return self.fail("invalid surrogate pair");
                        } else return self.fail("lone surrogate in \\u escape");
                    }
                    var tmp: [4]u8 = undefined;
                    const n = std.unicode.utf8Encode(cp, &tmp) catch return self.fail("invalid code point");
                    try buf.appendSlice(self.alloc, tmp[0..n]);
                },
                else => return self.fail("invalid escape sequence"),
            }
        }
        return self.fail("unterminated string");
    }

    fn array(self: *Parser) Error!Value {
        self.depth += 1;
        defer self.depth -= 1;
        if (self.depth > 200) return self.fail("nesting too deep");
        self.pos += 1;
        var items: std.ArrayList(Value) = .empty;
        self.ws();
        if (self.pos < self.src.len and self.src[self.pos] == ']') {
            self.pos += 1;
            return .{ .array = items.items };
        }
        while (true) {
            self.ws();
            try items.append(self.alloc, try self.value());
            self.ws();
            if (self.pos >= self.src.len) return self.fail("unterminated array");
            switch (self.src[self.pos]) {
                ',' => self.pos += 1,
                ']' => {
                    self.pos += 1;
                    return .{ .array = items.items };
                },
                else => return self.fail("expected ',' or ']' in array"),
            }
        }
    }

    fn object(self: *Parser) Error!Value {
        self.depth += 1;
        defer self.depth -= 1;
        if (self.depth > 200) return self.fail("nesting too deep");
        self.pos += 1;
        var items: std.ArrayList(Member) = .empty;
        self.ws();
        if (self.pos < self.src.len and self.src[self.pos] == '}') {
            self.pos += 1;
            return .{ .object = items.items };
        }
        while (true) {
            self.ws();
            if (self.pos >= self.src.len or self.src[self.pos] != '"') return self.fail("expected a string key in object");
            const k = try self.string();
            self.ws();
            if (self.pos >= self.src.len or self.src[self.pos] != ':') return self.fail("expected ':' after object key");
            self.pos += 1;
            self.ws();
            const v = try self.value();
            // Last duplicate key wins (replace in place to keep order stable).
            var dup = false;
            for (items.items) |*m| if (std.mem.eql(u8, m.key, k)) {
                m.value = v;
                dup = true;
                break;
            };
            if (!dup) try items.append(self.alloc, .{ .key = k, .value = v });
            self.ws();
            if (self.pos >= self.src.len) return self.fail("unterminated object");
            switch (self.src[self.pos]) {
                ',' => self.pos += 1,
                '}' => {
                    self.pos += 1;
                    return .{ .object = items.items };
                },
                else => return self.fail("expected ',' or '}' in object"),
            }
        }
    }
};

/// Parse `src` (kept alive by the caller; strings without escapes alias it). On failure returns
/// null and fills `err` with a line/column message.
pub fn parse(alloc: Allocator, src: []const u8, err: *ParseError) Allocator.Error!?Value {
    var p = Parser{ .src = src, .alloc = alloc };
    const v = p.parseDocument() catch |e| switch (e) {
        error.Syntax => {
            err.* = p.err.?;
            return null;
        },
        error.OutOfMemory => return error.OutOfMemory,
    };
    return v;
}

// ---- numbers -------------------------------------------------------------------------------------

/// Kerf number format: round to 1e-4, shortest decimal, no exponent, `-0` -> `0`.
pub fn fmtNumber(buf: *[40]u8, x: f64) []const u8 {
    if (!std.math.isFinite(x)) return "0";
    const neg = x < 0;
    const scaled = @round(@abs(x) * 10000.0);
    if (scaled >= 9.0e15) {
        // Out of the exactly-representable range; fall back to the standard shortest form.
        return std.fmt.bufPrint(buf, "{d}", .{x}) catch "0";
    }
    const n: u64 = @intFromFloat(scaled);
    if (n == 0) return "0";
    var i: usize = buf.len;
    var frac = n % 10000;
    var ip = n / 10000;
    if (frac != 0) {
        var digits: usize = 4;
        while (frac % 10 == 0) : (digits -= 1) frac /= 10;
        var d: usize = 0;
        while (d < digits) : (d += 1) {
            i -= 1;
            buf[i] = '0' + @as(u8, @intCast(frac % 10));
            frac /= 10;
        }
        i -= 1;
        buf[i] = '.';
    }
    if (ip == 0) {
        i -= 1;
        buf[i] = '0';
    } else while (ip > 0) {
        i -= 1;
        buf[i] = '0' + @as(u8, @intCast(ip % 10));
        ip /= 10;
    }
    if (neg) {
        i -= 1;
        buf[i] = '-';
    }
    return buf[i..];
}

/// Round to 1e-4 (the canonical precision).
pub fn round4(x: f64) f64 {
    const r = @round(x * 10000.0) / 10000.0;
    return if (r == 0) 0 else r;
}

// ---- writer --------------------------------------------------------------------------------------

pub fn writeString(out: *std.ArrayList(u8), a: Allocator, s: []const u8) Allocator.Error!void {
    try out.append(a, '"');
    for (s) |c| {
        switch (c) {
            '"' => try out.appendSlice(a, "\\\""),
            '\\' => try out.appendSlice(a, "\\\\"),
            '\n' => try out.appendSlice(a, "\\n"),
            '\r' => try out.appendSlice(a, "\\r"),
            '\t' => try out.appendSlice(a, "\\t"),
            0...8, 11, 12, 14...31 => {
                var tmp: [6]u8 = undefined;
                const t = std.fmt.bufPrint(&tmp, "\\u{x:0>4}", .{c}) catch unreachable;
                try out.appendSlice(a, t);
            },
            else => try out.append(a, c),
        }
    }
    try out.append(a, '"');
}

pub fn writeNumber(out: *std.ArrayList(u8), a: Allocator, x: f64) Allocator.Error!void {
    var b: [40]u8 = undefined;
    try out.appendSlice(a, fmtNumber(&b, x));
}

/// Order-fixing hook for the canonical writer: given the path of keys from the root to an object
/// (e.g. ["components", "*"]) return the schema key order for that object, or null.
pub const KeyOrderFn = *const fn (ctx: *const KeyCtx) []const []const u8;
pub const KeyCtx = struct {
    /// Path of object-valued containers from the root; array elements show as "[]".
    path: []const []const u8,
    /// The object being written.
    obj: []const Member,
};

pub const Pretty = struct {
    out: *std.ArrayList(u8),
    a: Allocator,
    order: ?KeyOrderFn = null,
    path: std.ArrayList([]const u8) = .empty,
    /// Arrays whose elements are all numbers print inline when true (canonical doc form).
    inline_numbers: bool = true,

    pub fn write(self: *Pretty, v: Value, indent: usize) Allocator.Error!void {
        switch (v) {
            .null => try self.out.appendSlice(self.a, "null"),
            .bool => |b| try self.out.appendSlice(self.a, if (b) "true" else "false"),
            .number => |n| try writeNumber(self.out, self.a, n),
            .string => |s| try writeString(self.out, self.a, s),
            .array => |items| {
                if (items.len == 0) return self.out.appendSlice(self.a, "[]");
                var all_num = self.inline_numbers;
                if (all_num) for (items) |it| if (it != .number) {
                    all_num = false;
                    break;
                };
                if (all_num) {
                    try self.out.append(self.a, '[');
                    for (items, 0..) |it, i| {
                        if (i > 0) try self.out.appendSlice(self.a, ", ");
                        try writeNumber(self.out, self.a, it.number);
                    }
                    try self.out.append(self.a, ']');
                    return;
                }
                try self.out.appendSlice(self.a, "[\n");
                try self.path.append(self.a, "[]");
                for (items, 0..) |it, i| {
                    try self.pad(indent + 1);
                    try self.write(it, indent + 1);
                    if (i + 1 < items.len) try self.out.append(self.a, ',');
                    try self.out.append(self.a, '\n');
                }
                _ = self.path.pop();
                try self.pad(indent);
                try self.out.append(self.a, ']');
            },
            .object => |members| {
                if (members.len == 0) return self.out.appendSlice(self.a, "{}");
                const ordered = try self.orderMembers(members);
                try self.out.appendSlice(self.a, "{\n");
                for (ordered, 0..) |m, i| {
                    try self.pad(indent + 1);
                    try writeString(self.out, self.a, m.key);
                    try self.out.appendSlice(self.a, ": ");
                    try self.path.append(self.a, m.key);
                    try self.write(m.value, indent + 1);
                    _ = self.path.pop();
                    if (i + 1 < ordered.len) try self.out.append(self.a, ',');
                    try self.out.append(self.a, '\n');
                }
                try self.pad(indent);
                try self.out.append(self.a, '}');
            },
        }
    }

    fn pad(self: *Pretty, n: usize) Allocator.Error!void {
        var i: usize = 0;
        while (i < n) : (i += 1) try self.out.appendSlice(self.a, "  ");
    }

    fn orderMembers(self: *Pretty, members: []const Member) Allocator.Error![]const Member {
        const f = self.order orelse return members;
        const ctx = KeyCtx{ .path = self.path.items, .obj = members };
        const schema = f(&ctx);
        const out = try self.a.alloc(Member, members.len);
        var n: usize = 0;
        for (schema) |k| {
            for (members) |m| if (std.mem.eql(u8, m.key, k)) {
                out[n] = m;
                n += 1;
                break;
            };
        }
        const start = n;
        for (members) |m| {
            var known = false;
            for (schema) |k| if (std.mem.eql(u8, m.key, k)) {
                known = true;
                break;
            };
            if (!known) {
                out[n] = m;
                n += 1;
            }
        }
        std.mem.sort(Member, out[start..n], {}, struct {
            fn lt(_: void, x: Member, y: Member) bool {
                return std.mem.lessThan(u8, x.key, y.key);
            }
        }.lt);
        return out;
    }
};

/// Compact one-line JSON (no whitespace), using the Kerf number format.
pub fn writeCompact(out: *std.ArrayList(u8), a: Allocator, v: Value) Allocator.Error!void {
    switch (v) {
        .null => try out.appendSlice(a, "null"),
        .bool => |b| try out.appendSlice(a, if (b) "true" else "false"),
        .number => |n| try writeNumber(out, a, n),
        .string => |s| try writeString(out, a, s),
        .array => |items| {
            try out.append(a, '[');
            for (items, 0..) |it, i| {
                if (i > 0) try out.append(a, ',');
                try writeCompact(out, a, it);
            }
            try out.append(a, ']');
        },
        .object => |ms| {
            try out.append(a, '{');
            for (ms, 0..) |m, i| {
                if (i > 0) try out.append(a, ',');
                try writeString(out, a, m.key);
                try out.append(a, ':');
                try writeCompact(out, a, m.value);
            }
            try out.append(a, '}');
        },
    }
}

// ---- builders for constructing values ---------------------------------------------------------

pub fn obj(a: Allocator, members: []const Member) Allocator.Error!Value {
    return .{ .object = try a.dupe(Member, members) };
}
pub fn arrOf(a: Allocator, items: []const Value) Allocator.Error!Value {
    return .{ .array = try a.dupe(Value, items) };
}
pub fn numArr(a: Allocator, xs: []const f64) Allocator.Error!Value {
    const out = try a.alloc(Value, xs.len);
    for (xs, 0..) |x, i| out[i] = .{ .number = x };
    return .{ .array = out };
}

/// Deep copy of a value into `a`.
pub fn clone(a: Allocator, v: Value) Allocator.Error!Value {
    return switch (v) {
        .null, .bool, .number => v,
        .string => |s| .{ .string = try a.dupe(u8, s) },
        .array => |items| blk: {
            const out = try a.alloc(Value, items.len);
            for (items, 0..) |it, i| out[i] = try clone(a, it);
            break :blk .{ .array = out };
        },
        .object => |ms| blk: {
            const out = try a.alloc(Member, ms.len);
            for (ms, 0..) |m, i| out[i] = .{ .key = try a.dupe(u8, m.key), .value = try clone(a, m.value) };
            break :blk .{ .object = out };
        },
    };
}

pub fn eql(x: Value, y: Value) bool {
    if (std.meta.activeTag(x) != std.meta.activeTag(y)) return false;
    return switch (x) {
        .null => true,
        .bool => |b| b == y.bool,
        .number => |n| n == y.number,
        .string => |s| std.mem.eql(u8, s, y.string),
        .array => |xs| blk: {
            if (xs.len != y.array.len) break :blk false;
            for (xs, y.array) |p, q| if (!eql(p, q)) break :blk false;
            break :blk true;
        },
        .object => |xs| blk: {
            if (xs.len != y.object.len) break :blk false;
            for (xs) |m| {
                const o = y.get(m.key) orelse break :blk false;
                if (!eql(m.value, o)) break :blk false;
            }
            break :blk true;
        },
    };
}

/// RFC 7396 merge patch. Returns the patched value (new containers allocated from `a`).
pub fn mergePatch(a: Allocator, target: Value, patch: Value) Allocator.Error!Value {
    if (patch != .object) return clone(a, patch);
    var members: std.ArrayList(Member) = .empty;
    if (target == .object) {
        for (target.object) |m| try members.append(a, m);
    }
    for (patch.object) |pm| {
        var idx: ?usize = null;
        for (members.items, 0..) |m, i| if (std.mem.eql(u8, m.key, pm.key)) {
            idx = i;
            break;
        };
        if (pm.value == .null) {
            if (idx) |i| _ = members.orderedRemove(i);
            continue;
        }
        if (idx) |i| {
            members.items[i].value = try mergePatch(a, members.items[i].value, pm.value);
        } else {
            try members.append(a, .{ .key = pm.key, .value = try mergePatch(a, .null, pm.value) });
        }
    }
    return .{ .object = members.items };
}

test "number format" {
    var b: [40]u8 = undefined;
    try std.testing.expectEqualStrings("7.625", fmtNumber(&b, 7.625));
    try std.testing.expectEqualStrings("0.4375", fmtNumber(&b, 0.4375));
    try std.testing.expectEqualStrings("12", fmtNumber(&b, 12.0));
    try std.testing.expectEqualStrings("0", fmtNumber(&b, -0.0));
    try std.testing.expectEqualStrings("0", fmtNumber(&b, -0.00001));
    try std.testing.expectEqualStrings("-1.5", fmtNumber(&b, -1.5));
    try std.testing.expectEqualStrings("0.3333", fmtNumber(&b, 1.0 / 3.0));
    try std.testing.expectEqualStrings("97.125", fmtNumber(&b, 97.125));
    try std.testing.expectEqualStrings("0.1", fmtNumber(&b, 0.1 + 0.2 - 0.2));
    try std.testing.expectEqualStrings("18.4349", fmtNumber(&b, 18.43494882292201));
}

test "parse and canonical write" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    var err: ParseError = undefined;
    const v = (try parse(a, "{\"b\": [1, 2.50, 3], \"a\": {\"x\": \"q\\u00e9\\n\"}, \"c\": []}", &err)).?;
    var out: std.ArrayList(u8) = .empty;
    var pw = Pretty{ .out = &out, .a = a };
    try pw.write(v, 0);
    try std.testing.expectEqualStrings(
        "{\n  \"b\": [1, 2.5, 3],\n  \"a\": {\n    \"x\": \"q\xc3\xa9\\n\"\n  },\n  \"c\": []\n}",
        out.items,
    );
}

test "parse errors have positions" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    var err: ParseError = undefined;
    const r = try parse(arena.allocator(), "{\n  \"a\": [1, 2,]\n}", &err);
    try std.testing.expect(r == null);
    try std.testing.expectEqual(@as(u32, 2), err.line);
}

test "merge patch" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    var err: ParseError = undefined;
    const t = (try parse(a, "{\"a\":1,\"b\":{\"c\":2,\"d\":3}}", &err)).?;
    const p = (try parse(a, "{\"b\":{\"c\":null,\"e\":5},\"f\":[1]}", &err)).?;
    const r = try mergePatch(a, t, p);
    var out: std.ArrayList(u8) = .empty;
    try writeCompact(&out, a, r);
    try std.testing.expectEqualStrings("{\"a\":1,\"b\":{\"d\":3,\"e\":5},\"f\":[1]}", out.items);
}
