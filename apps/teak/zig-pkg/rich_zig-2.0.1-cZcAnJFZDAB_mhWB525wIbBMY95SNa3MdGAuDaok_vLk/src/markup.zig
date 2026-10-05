const std = @import("std");
const Style = @import("style.zig").Style;
const Segment = @import("segment.zig").Segment;

pub const Tag = struct {
    name: []const u8,
    parameters: ?[]const u8 = null,
};

pub const MarkupToken = union(enum) {
    text: []const u8,
    open_tag: Tag,
    close_tag: ?[]const u8, // null means [/]
};

pub const MarkupError = error{
    UnbalancedBracket,
    InvalidTag,
    UnclosedTag,
    StyleParseError,
};

pub fn parseMarkup(text: []const u8, allocator: std.mem.Allocator) ![]MarkupToken {
    var tokens: std.ArrayList(MarkupToken) = .empty;
    errdefer tokens.deinit(allocator);

    var i: usize = 0;
    var text_start: usize = 0;

    while (i < text.len) {
        // Escaped bracket: \[ or \]
        if (text[i] == '\\' and i + 1 < text.len and (text[i + 1] == '[' or text[i + 1] == ']')) {
            // Add text before escape
            if (i > text_start) {
                try tokens.append(allocator, .{ .text = text[text_start..i] });
            }
            // Add the escaped character as text
            try tokens.append(allocator, .{ .text = text[i + 1 .. i + 2] });
            i += 2;
            text_start = i;
            continue;
        }

        // Start of tag
        if (text[i] == '[') {
            // Add text before tag
            if (i > text_start) {
                try tokens.append(allocator, .{ .text = text[text_start..i] });
            }

            // Find closing bracket
            const end = std.mem.indexOfScalarPos(u8, text, i + 1, ']') orelse
                return MarkupError.UnbalancedBracket;

            const tag_content = text[i + 1 .. end];

            if (tag_content.len == 0) {
                return MarkupError.InvalidTag;
            }

            if (tag_content[0] == '/') {
                // Close tag: [/] or [/name]
                const tag_name = if (tag_content.len > 1) tag_content[1..] else null;
                try tokens.append(allocator, .{ .close_tag = tag_name });
            } else {
                // Open tag: check for = sign for parameters
                if (std.mem.indexOfScalar(u8, tag_content, '=')) |eq_pos| {
                    try tokens.append(allocator, .{ .open_tag = .{
                        .name = tag_content[0..eq_pos],
                        .parameters = tag_content[eq_pos + 1 ..],
                    } });
                } else {
                    try tokens.append(allocator, .{ .open_tag = .{ .name = tag_content } });
                }
            }

            i = end + 1;
            text_start = i;
            continue;
        }

        i += 1;
    }

    // Remaining text
    if (text_start < text.len) {
        try tokens.append(allocator, .{ .text = text[text_start..] });
    }

    return tokens.toOwnedSlice(allocator);
}

pub fn render(text: []const u8, base_style: Style, allocator: std.mem.Allocator) ![]Segment {
    const tokens = try parseMarkup(text, allocator);
    defer allocator.free(tokens);

    var segments: std.ArrayList(Segment) = .empty;
    errdefer segments.deinit(allocator);

    var style_stack: std.ArrayList(Style) = .empty;
    defer style_stack.deinit(allocator);

    try style_stack.append(allocator, base_style);

    for (tokens) |token| {
        switch (token) {
            .text => |txt| {
                if (txt.len > 0) {
                    try segments.append(allocator, Segment.styled(txt, style_stack.getLast()));
                }
            },
            .open_tag => |tag| {
                const current_style = style_stack.getLast();
                const new_style = Style.parse(tag.name) catch {
                    var literal_buf: [128]u8 = undefined;
                    const literal = std.fmt.bufPrint(&literal_buf, "[{s}]", .{tag.name}) catch "[?]";
                    try segments.append(allocator, Segment.styled(literal, current_style));
                    continue;
                };
                try style_stack.append(allocator, current_style.combine(new_style));
            },
            .close_tag => {
                if (style_stack.items.len > 1) {
                    _ = style_stack.pop();
                }
            },
        }
    }

    return segments.toOwnedSlice(allocator);
}

pub fn escape(text: []const u8, allocator: std.mem.Allocator) ![]u8 {
    var count: usize = 0;
    for (text) |c| {
        if (c == '[' or c == ']') count += 1;
    }

    if (count == 0) {
        return try allocator.dupe(u8, text);
    }

    const result = try allocator.alloc(u8, text.len + count);
    var j: usize = 0;
    for (text) |c| {
        if (c == '[' or c == ']') {
            result[j] = '\\';
            j += 1;
        }
        result[j] = c;
        j += 1;
    }

    return result;
}

pub fn stripMarkup(text: []const u8, allocator: std.mem.Allocator) ![]u8 {
    const tokens = try parseMarkup(text, allocator);
    defer allocator.free(tokens);

    var total_len: usize = 0;
    for (tokens) |token| {
        switch (token) {
            .text => |txt| total_len += txt.len,
            else => {},
        }
    }

    const result = try allocator.alloc(u8, total_len);
    var pos: usize = 0;
    for (tokens) |token| {
        switch (token) {
            .text => |txt| {
                @memcpy(result[pos..][0..txt.len], txt);
                pos += txt.len;
            },
            else => {},
        }
    }

    return result;
}

// Tests
test "parseMarkup simple text" {
    const allocator = std.testing.allocator;
    const tokens = try parseMarkup("Hello World", allocator);
    defer allocator.free(tokens);

    try std.testing.expectEqual(@as(usize, 1), tokens.len);
    try std.testing.expectEqualStrings("Hello World", tokens[0].text);
}

test "parseMarkup simple tag" {
    const allocator = std.testing.allocator;
    const tokens = try parseMarkup("[bold]text[/]", allocator);
    defer allocator.free(tokens);

    try std.testing.expectEqual(@as(usize, 3), tokens.len);
    try std.testing.expectEqualStrings("bold", tokens[0].open_tag.name);
    try std.testing.expectEqualStrings("text", tokens[1].text);
    try std.testing.expect(tokens[2].close_tag == null);
}

test "parseMarkup nested tags" {
    const allocator = std.testing.allocator;
    const tokens = try parseMarkup("[bold][red]text[/red][/bold]", allocator);
    defer allocator.free(tokens);

    try std.testing.expectEqual(@as(usize, 5), tokens.len);
    try std.testing.expectEqualStrings("bold", tokens[0].open_tag.name);
    try std.testing.expectEqualStrings("red", tokens[1].open_tag.name);
    try std.testing.expectEqualStrings("text", tokens[2].text);
    try std.testing.expectEqualStrings("red", tokens[3].close_tag.?);
    try std.testing.expectEqualStrings("bold", tokens[4].close_tag.?);
}

test "parseMarkup escaped brackets" {
    const allocator = std.testing.allocator;
    const tokens = try parseMarkup("\\[not a tag\\]", allocator);
    defer allocator.free(tokens);

    // Should have: "[", "not a tag", "]"
    try std.testing.expectEqual(@as(usize, 3), tokens.len);
    try std.testing.expectEqualStrings("[", tokens[0].text);
    try std.testing.expectEqualStrings("not a tag", tokens[1].text);
    try std.testing.expectEqualStrings("]", tokens[2].text);
}

test "parseMarkup tag with parameters" {
    const allocator = std.testing.allocator;
    const tokens = try parseMarkup("[link=https://example.com]click[/link]", allocator);
    defer allocator.free(tokens);

    try std.testing.expectEqual(@as(usize, 3), tokens.len);
    try std.testing.expectEqualStrings("link", tokens[0].open_tag.name);
    try std.testing.expectEqualStrings("https://example.com", tokens[0].open_tag.parameters.?);
}

test "parseMarkup unbalanced bracket error" {
    const allocator = std.testing.allocator;
    try std.testing.expectError(MarkupError.UnbalancedBracket, parseMarkup("[bold", allocator));
}

test "parseMarkup empty tag error" {
    const allocator = std.testing.allocator;
    try std.testing.expectError(MarkupError.InvalidTag, parseMarkup("[]", allocator));
}

test "render basic" {
    const allocator = std.testing.allocator;
    const segments = try render("[bold]Hello[/]", Style.empty, allocator);
    defer allocator.free(segments);

    try std.testing.expectEqual(@as(usize, 1), segments.len);
    try std.testing.expectEqualStrings("Hello", segments[0].text);
    try std.testing.expect(segments[0].style.?.hasAttribute(.bold));
}

test "render nested styles" {
    const allocator = std.testing.allocator;
    const segments = try render("[bold][italic]text[/][/]", Style.empty, allocator);
    defer allocator.free(segments);

    try std.testing.expectEqual(@as(usize, 1), segments.len);
    try std.testing.expect(segments[0].style.?.hasAttribute(.bold));
    try std.testing.expect(segments[0].style.?.hasAttribute(.italic));
}

test "escape" {
    const allocator = std.testing.allocator;

    const escaped = try escape("Hello [World]", allocator);
    defer allocator.free(escaped);
    try std.testing.expectEqualStrings("Hello \\[World\\]", escaped);

    const no_escape = try escape("Hello World", allocator);
    defer allocator.free(no_escape);
    try std.testing.expectEqualStrings("Hello World", no_escape);
}

test "stripMarkup" {
    const allocator = std.testing.allocator;

    const stripped = try stripMarkup("[bold]Hello[/] [red]World[/]", allocator);
    defer allocator.free(stripped);
    try std.testing.expectEqualStrings("Hello World", stripped);
}
