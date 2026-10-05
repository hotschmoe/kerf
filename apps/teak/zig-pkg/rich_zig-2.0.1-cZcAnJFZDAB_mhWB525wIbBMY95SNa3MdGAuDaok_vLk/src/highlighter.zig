const std = @import("std");
const Style = @import("style.zig").Style;
const Color = @import("color.zig").Color;
const Text = @import("text.zig").Text;

/// Highlighter that applies styles to recognized patterns in text.
pub const Highlighter = struct {
    rules: []const HighlightRule,

    pub const HighlightRule = struct {
        name: []const u8,
        matcher: *const fn (text: []const u8, pos: usize) ?Match,
        style: Style,
    };

    pub const Match = struct {
        start: usize,
        end: usize,
    };

    /// Create a highlighter with the given rules.
    pub fn init(rules: []const HighlightRule) Highlighter {
        return .{ .rules = rules };
    }

    /// Apply highlighting rules to a Text object, adding spans for each match.
    pub fn highlight(self: Highlighter, text: *Text) !void {
        for (self.rules) |rule| {
            var pos: usize = 0;
            while (pos < text.plain.len) {
                if (rule.matcher(text.plain, pos)) |match| {
                    if (match.end > match.start) {
                        try text.highlight(match.start, match.end, rule.style);
                        pos = match.end;
                    } else {
                        pos += 1;
                    }
                } else {
                    pos += 1;
                }
            }
        }
    }

    /// Create a ReprHighlighter - highlights common programming repr patterns.
    pub fn repr() Highlighter {
        return .{ .rules = &repr_rules };
    }
};

// Matcher functions for the repr highlighter

fn matchInteger(text: []const u8, pos: usize) ?Highlighter.Match {
    // Must be at word boundary
    if (pos > 0 and isWordChar(text[pos - 1])) return null;

    var i = pos;
    // Optional sign
    if (i < text.len and (text[i] == '-' or text[i] == '+')) i += 1;

    if (i >= text.len or !std.ascii.isDigit(text[i])) return null;

    // Hex: 0x...
    if (i + 1 < text.len and text[i] == '0' and (text[i + 1] == 'x' or text[i + 1] == 'X')) {
        i += 2;
        const hex_start = i;
        while (i < text.len and isHexDigit(text[i])) : (i += 1) {}
        if (i == hex_start) return null;
        if (i < text.len and isWordChar(text[i])) return null;
        return .{ .start = pos, .end = i };
    }

    // Decimal digits
    while (i < text.len and std.ascii.isDigit(text[i])) : (i += 1) {}

    // Check it's not a float (no decimal point or exponent)
    if (i < text.len and (text[i] == '.' or text[i] == 'e' or text[i] == 'E')) return null;

    // Must end at word boundary
    if (i < text.len and isWordChar(text[i])) return null;

    if (i > pos) return .{ .start = pos, .end = i };
    return null;
}

fn matchFloat(text: []const u8, pos: usize) ?Highlighter.Match {
    if (pos > 0 and isWordChar(text[pos - 1])) return null;

    var i = pos;
    if (i < text.len and (text[i] == '-' or text[i] == '+')) i += 1;

    if (i >= text.len or !std.ascii.isDigit(text[i])) return null;

    while (i < text.len and std.ascii.isDigit(text[i])) : (i += 1) {}

    var has_decimal = false;
    if (i < text.len and text[i] == '.') {
        has_decimal = true;
        i += 1;
        while (i < text.len and std.ascii.isDigit(text[i])) : (i += 1) {}
    }

    // Exponent
    var has_exp = false;
    if (i < text.len and (text[i] == 'e' or text[i] == 'E')) {
        has_exp = true;
        i += 1;
        if (i < text.len and (text[i] == '+' or text[i] == '-')) i += 1;
        while (i < text.len and std.ascii.isDigit(text[i])) : (i += 1) {}
    }

    if (!has_decimal and !has_exp) return null;
    if (i < text.len and isWordChar(text[i])) return null;

    return .{ .start = pos, .end = i };
}

fn matchBoolNone(text: []const u8, pos: usize) ?Highlighter.Match {
    if (pos > 0 and isWordChar(text[pos - 1])) return null;

    const keywords = [_][]const u8{ "true", "false", "null", "undefined", "void" };
    for (keywords) |kw| {
        if (pos + kw.len <= text.len and std.mem.eql(u8, text[pos..][0..kw.len], kw)) {
            if (pos + kw.len >= text.len or !isWordChar(text[pos + kw.len])) {
                return .{ .start = pos, .end = pos + kw.len };
            }
        }
    }
    return null;
}

fn matchString(text: []const u8, pos: usize) ?Highlighter.Match {
    if (pos >= text.len) return null;
    const quote = text[pos];
    if (quote != '"' and quote != '\'') return null;

    var i = pos + 1;
    while (i < text.len) {
        if (text[i] == '\\' and i + 1 < text.len) {
            i += 2;
            continue;
        }
        if (text[i] == quote) {
            return .{ .start = pos, .end = i + 1 };
        }
        i += 1;
    }
    return null;
}

fn matchUrl(text: []const u8, pos: usize) ?Highlighter.Match {
    const prefixes = [_][]const u8{ "https://", "http://", "ftp://", "file://" };
    for (prefixes) |prefix| {
        if (pos + prefix.len <= text.len and std.mem.eql(u8, text[pos..][0..prefix.len], prefix)) {
            var i = pos + prefix.len;
            while (i < text.len and !std.ascii.isWhitespace(text[i]) and text[i] != ')' and text[i] != ']' and text[i] != '>') : (i += 1) {}
            if (i > pos + prefix.len) {
                return .{ .start = pos, .end = i };
            }
        }
    }
    return null;
}

fn matchPath(text: []const u8, pos: usize) ?Highlighter.Match {
    // Unix paths: /foo/bar or ./foo/bar
    if (pos > 0 and !std.ascii.isWhitespace(text[pos - 1])) return null;

    if (pos < text.len and (text[pos] == '/' or (text[pos] == '.' and pos + 1 < text.len and text[pos + 1] == '/'))) {
        var i = pos;
        while (i < text.len and !std.ascii.isWhitespace(text[i])) : (i += 1) {}
        // Must contain at least one slash after start
        if (std.mem.indexOfScalar(u8, text[pos + 1 .. i], '/') != null or text[pos] == '/') {
            if (i - pos > 2) { // Must be more than just "/"
                return .{ .start = pos, .end = i };
            }
        }
    }
    return null;
}

fn matchUuid(text: []const u8, pos: usize) ?Highlighter.Match {
    // UUID: 8-4-4-4-12 hex chars
    if (pos + 36 > text.len) return null;
    if (pos > 0 and isWordChar(text[pos - 1])) return null;

    const pattern = [_]u8{ 8, 4, 4, 4, 12 };
    var i = pos;
    for (pattern, 0..) |count, group| {
        for (0..count) |_| {
            if (i >= text.len or !isHexDigit(text[i])) return null;
            i += 1;
        }
        if (group < pattern.len - 1) {
            if (i >= text.len or text[i] != '-') return null;
            i += 1;
        }
    }

    if (i < text.len and isWordChar(text[i])) return null;
    return .{ .start = pos, .end = i };
}

fn isWordChar(c: u8) bool {
    return std.ascii.isAlphanumeric(c) or c == '_';
}

fn isHexDigit(c: u8) bool {
    return std.ascii.isDigit(c) or (c >= 'a' and c <= 'f') or (c >= 'A' and c <= 'F');
}

const repr_rules = [_]Highlighter.HighlightRule{
    .{
        .name = "url",
        .matcher = &matchUrl,
        .style = Style.empty.foreground(Color.blue).underline(),
    },
    .{
        .name = "uuid",
        .matcher = &matchUuid,
        .style = Style.empty.foreground(Color.fromRgb(174, 129, 255)),
    },
    .{
        .name = "path",
        .matcher = &matchPath,
        .style = Style.empty.foreground(Color.fromRgb(174, 129, 255)),
    },
    .{
        .name = "string",
        .matcher = &matchString,
        .style = Style.empty.foreground(Color.green),
    },
    .{
        .name = "float",
        .matcher = &matchFloat,
        .style = Style.empty.foreground(Color.cyan).bold(),
    },
    .{
        .name = "integer",
        .matcher = &matchInteger,
        .style = Style.empty.foreground(Color.cyan).bold(),
    },
    .{
        .name = "bool_none",
        .matcher = &matchBoolNone,
        .style = Style.empty.foreground(Color.fromRgb(255, 85, 85)).italic(),
    },
};

// Tests
test "matchInteger basic" {
    const m = matchInteger("42", 0);
    try std.testing.expect(m != null);
    try std.testing.expectEqual(@as(usize, 0), m.?.start);
    try std.testing.expectEqual(@as(usize, 2), m.?.end);
}

test "matchInteger hex" {
    const m = matchInteger("0xFF", 0);
    try std.testing.expect(m != null);
    try std.testing.expectEqual(@as(usize, 4), m.?.end);
}

test "matchInteger negative" {
    const m = matchInteger("-123", 0);
    try std.testing.expect(m != null);
    try std.testing.expectEqual(@as(usize, 4), m.?.end);
}

test "matchInteger not at word boundary" {
    try std.testing.expect(matchInteger("abc123", 3) == null);
}

test "matchFloat basic" {
    const m = matchFloat("3.14", 0);
    try std.testing.expect(m != null);
    try std.testing.expectEqual(@as(usize, 4), m.?.end);
}

test "matchFloat exponent" {
    const m = matchFloat("1e10", 0);
    try std.testing.expect(m != null);
    try std.testing.expectEqual(@as(usize, 4), m.?.end);
}

test "matchBoolNone" {
    try std.testing.expect(matchBoolNone("true", 0) != null);
    try std.testing.expect(matchBoolNone("false", 0) != null);
    try std.testing.expect(matchBoolNone("null", 0) != null);
    try std.testing.expect(matchBoolNone("truex", 0) == null);
}

test "matchString double quote" {
    const m = matchString("\"hello\"", 0);
    try std.testing.expect(m != null);
    try std.testing.expectEqual(@as(usize, 7), m.?.end);
}

test "matchString with escape" {
    const m = matchString("\"he\\\"llo\"", 0);
    try std.testing.expect(m != null);
    try std.testing.expectEqual(@as(usize, 9), m.?.end);
}

test "matchUrl https" {
    const m = matchUrl("https://example.com/path", 0);
    try std.testing.expect(m != null);
    try std.testing.expectEqual(@as(usize, 24), m.?.end);
}

test "matchUrl not at start" {
    const m = matchUrl("visit https://example.com", 6);
    try std.testing.expect(m != null);
    try std.testing.expectEqual(@as(usize, 6), m.?.start);
}

test "matchPath unix" {
    const m = matchPath("/usr/local/bin", 0);
    try std.testing.expect(m != null);
    try std.testing.expectEqual(@as(usize, 14), m.?.end);
}

test "matchPath relative" {
    const m = matchPath("./src/main.zig", 0);
    try std.testing.expect(m != null);
    try std.testing.expectEqual(@as(usize, 14), m.?.end);
}

test "matchUuid" {
    const m = matchUuid("550e8400-e29b-41d4-a716-446655440000", 0);
    try std.testing.expect(m != null);
    try std.testing.expectEqual(@as(usize, 36), m.?.end);
}

test "matchUuid invalid" {
    try std.testing.expect(matchUuid("550e8400-e29b-41d4", 0) == null);
}

test "Highlighter.repr creation" {
    const h = Highlighter.repr();
    try std.testing.expect(h.rules.len > 0);
}

test "Highlighter.highlight text" {
    const allocator = std.testing.allocator;
    var text = try Text.fromPlainOwned(allocator, "count is 42 and pi is 3.14");
    defer text.deinit();

    const h = Highlighter.repr();
    try h.highlight(&text);

    // Should have found at least the integer 42 and float 3.14
    try std.testing.expect(text.spans.len >= 2);
}
