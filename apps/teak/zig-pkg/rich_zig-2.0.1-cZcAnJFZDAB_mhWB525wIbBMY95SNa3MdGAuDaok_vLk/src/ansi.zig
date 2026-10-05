const std = @import("std");
const Style = @import("style.zig").Style;
const Color = @import("color.zig").Color;
const Text = @import("text.zig").Text;
const Span = @import("text.zig").Span;

/// Parse ANSI-escaped text into a styled Text object.
/// Converts SGR escape sequences into Style spans.
pub fn fromAnsi(allocator: std.mem.Allocator, input: []const u8) !Text {
    var plain_buf: std.ArrayList(u8) = .empty;
    defer plain_buf.deinit(allocator);

    var spans: std.ArrayList(Span) = .empty;
    defer spans.deinit(allocator);

    var current_style = Style.empty;
    var style_start: usize = 0;
    var i: usize = 0;

    while (i < input.len) {
        // Check for ESC [ (CSI sequence)
        if (i + 1 < input.len and input[i] == '\x1b' and input[i + 1] == '[') {
            // Flush any styled text before this escape
            if (!current_style.isEmpty() and plain_buf.items.len > style_start) {
                try spans.append(allocator, .{
                    .start = style_start,
                    .end = plain_buf.items.len,
                    .style = current_style,
                });
            }

            // Parse CSI parameters
            i += 2; // skip ESC [
            var params: std.ArrayList(u16) = .empty;
            defer params.deinit(allocator);

            var current_param: u16 = 0;
            var has_param = false;

            while (i < input.len) {
                const c = input[i];
                if (c >= '0' and c <= '9') {
                    current_param = current_param * 10 + @as(u16, @intCast(c - '0'));
                    has_param = true;
                    i += 1;
                } else if (c == ';') {
                    try params.append(allocator, if (has_param) current_param else 0);
                    current_param = 0;
                    has_param = false;
                    i += 1;
                } else {
                    // End of parameters
                    if (has_param) {
                        try params.append(allocator, current_param);
                    }
                    if (c == 'm') {
                        // SGR sequence
                        current_style = applySgr(current_style, params.items);
                    }
                    i += 1;
                    break;
                }
            }

            style_start = plain_buf.items.len;
            continue;
        }

        // Check for OSC sequences (ESC ]) -- skip them
        if (i + 1 < input.len and input[i] == '\x1b' and input[i + 1] == ']') {
            i += 2;
            // Skip until ST (ESC \) or BEL
            while (i < input.len) {
                if (input[i] == '\x07') {
                    i += 1;
                    break;
                }
                if (i + 1 < input.len and input[i] == '\x1b' and input[i + 1] == '\\') {
                    i += 2;
                    break;
                }
                i += 1;
            }
            continue;
        }

        // Regular character
        try plain_buf.append(allocator, input[i]);
        i += 1;
    }

    // Flush remaining styled text
    if (!current_style.isEmpty() and plain_buf.items.len > style_start) {
        try spans.append(allocator, .{
            .start = style_start,
            .end = plain_buf.items.len,
            .style = current_style,
        });
    }

    return .{
        .plain = try plain_buf.toOwnedSlice(allocator),
        .spans = try spans.toOwnedSlice(allocator),
        .style = Style.empty,
        .allocator = allocator,
        .owns_plain = true,
        .owns_spans = true,
    };
}

/// Strip all ANSI escape sequences from text, returning plain text.
pub fn stripAnsi(allocator: std.mem.Allocator, input: []const u8) ![]u8 {
    var result: std.ArrayList(u8) = .empty;
    var i: usize = 0;

    while (i < input.len) {
        if (input[i] == '\x1b') {
            i += 1;
            if (i >= input.len) break;

            if (input[i] == '[') {
                // CSI: skip until letter
                i += 1;
                while (i < input.len and input[i] >= 0x20 and input[i] < 0x40) : (i += 1) {}
                if (i < input.len) i += 1; // skip final byte
                continue;
            }
            if (input[i] == ']') {
                // OSC: skip until ST or BEL
                i += 1;
                while (i < input.len) {
                    if (input[i] == '\x07') {
                        i += 1;
                        break;
                    }
                    if (i + 1 < input.len and input[i] == '\x1b' and input[i + 1] == '\\') {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
                continue;
            }
            // Other escape sequences: skip next char
            i += 1;
            continue;
        }

        try result.append(allocator, input[i]);
        i += 1;
    }

    return result.toOwnedSlice(allocator);
}

fn applySgr(base: Style, params: []const u16) Style {
    var style = base;

    if (params.len == 0) {
        return Style.empty;
    }

    const standard_colors = [_]Color{
        Color.black, Color.red,     Color.green, Color.yellow,
        Color.blue,  Color.magenta, Color.cyan,  Color.white,
    };
    const bright_colors = [_]Color{
        Color.bright_black, Color.bright_red,     Color.bright_green, Color.bright_yellow,
        Color.bright_blue,  Color.bright_magenta, Color.bright_cyan,  Color.bright_white,
    };

    var i: usize = 0;
    while (i < params.len) {
        const code = params[i];
        switch (code) {
            0 => style = Style.empty,
            1 => style = style.bold(),
            2 => style = style.dim(),
            3 => style = style.italic(),
            4 => style = style.underline(),
            5 => style = style.blink(),
            7 => style = style.reverse(),
            8 => style = style.conceal(),
            9 => style = style.strike(),
            21 => style = style.underline2(),
            22 => style = style.notBold().notDim(),
            23 => style = style.notItalic(),
            24 => style = style.notUnderline(),
            25 => style = style.notBlink(),
            27 => style = style.notReverse(),
            28 => style = style.notConceal(),
            29 => style = style.notStrike(),
            53 => style = style.overline(),
            55 => style = style.notOverline(),

            30...37 => style.color = standard_colors[code - 30],
            40...47 => style.bgcolor = standard_colors[code - 40],
            90...97 => style.color = bright_colors[code - 90],
            100...107 => style.bgcolor = bright_colors[code - 100],

            38 => {
                if (i + 1 < params.len) {
                    if (params[i + 1] == 5 and i + 2 < params.len) {
                        style.color = Color.from256(@truncate(params[i + 2]));
                        i += 2;
                    } else if (params[i + 1] == 2 and i + 4 < params.len) {
                        style.color = Color.fromRgb(@truncate(params[i + 2]), @truncate(params[i + 3]), @truncate(params[i + 4]));
                        i += 4;
                    }
                }
            },
            48 => {
                if (i + 1 < params.len) {
                    if (params[i + 1] == 5 and i + 2 < params.len) {
                        style.bgcolor = Color.from256(@truncate(params[i + 2]));
                        i += 2;
                    } else if (params[i + 1] == 2 and i + 4 < params.len) {
                        style.bgcolor = Color.fromRgb(@truncate(params[i + 2]), @truncate(params[i + 3]), @truncate(params[i + 4]));
                        i += 4;
                    }
                }
            },

            39 => style.color = null,
            49 => style.bgcolor = null,
            else => {},
        }
        i += 1;
    }

    return style;
}

// Tests
test "fromAnsi plain text" {
    const allocator = std.testing.allocator;
    var text = try fromAnsi(allocator, "Hello World");
    defer text.deinit();

    try std.testing.expectEqualStrings("Hello World", text.plain);
    try std.testing.expectEqual(@as(usize, 0), text.spans.len);
}

test "fromAnsi bold text" {
    const allocator = std.testing.allocator;
    var text = try fromAnsi(allocator, "\x1b[1mHello\x1b[0m");
    defer text.deinit();

    try std.testing.expectEqualStrings("Hello", text.plain);
    try std.testing.expectEqual(@as(usize, 1), text.spans.len);
    try std.testing.expect(text.spans[0].style.hasAttribute(.bold));
}

test "fromAnsi colored text" {
    const allocator = std.testing.allocator;
    var text = try fromAnsi(allocator, "\x1b[31mRed\x1b[0m");
    defer text.deinit();

    try std.testing.expectEqualStrings("Red", text.plain);
    try std.testing.expectEqual(@as(usize, 1), text.spans.len);
    try std.testing.expect(text.spans[0].style.color != null);
    try std.testing.expect(text.spans[0].style.color.?.eql(Color.red));
}

test "fromAnsi multiple styles" {
    const allocator = std.testing.allocator;
    var text = try fromAnsi(allocator, "\x1b[1mBold\x1b[0m \x1b[3mItalic\x1b[0m");
    defer text.deinit();

    try std.testing.expectEqualStrings("Bold Italic", text.plain);
    try std.testing.expectEqual(@as(usize, 2), text.spans.len);
    try std.testing.expect(text.spans[0].style.hasAttribute(.bold));
    try std.testing.expect(text.spans[1].style.hasAttribute(.italic));
}

test "fromAnsi 256 color" {
    const allocator = std.testing.allocator;
    var text = try fromAnsi(allocator, "\x1b[38;5;196mRed\x1b[0m");
    defer text.deinit();

    try std.testing.expectEqualStrings("Red", text.plain);
    try std.testing.expectEqual(@as(usize, 1), text.spans.len);
}

test "fromAnsi truecolor" {
    const allocator = std.testing.allocator;
    var text = try fromAnsi(allocator, "\x1b[38;2;255;128;64mOrange\x1b[0m");
    defer text.deinit();

    try std.testing.expectEqualStrings("Orange", text.plain);
    try std.testing.expectEqual(@as(usize, 1), text.spans.len);
}

test "fromAnsi reset" {
    const allocator = std.testing.allocator;
    var text = try fromAnsi(allocator, "\x1b[1;31mBold Red\x1b[0m Normal");
    defer text.deinit();

    try std.testing.expectEqualStrings("Bold Red Normal", text.plain);
    // "Bold Red" should be styled, " Normal" should not (or minimally)
    try std.testing.expect(text.spans.len >= 1);
    try std.testing.expect(text.spans[0].style.hasAttribute(.bold));
}

test "fromAnsi background color" {
    const allocator = std.testing.allocator;
    var text = try fromAnsi(allocator, "\x1b[42mGreen BG\x1b[0m");
    defer text.deinit();

    try std.testing.expectEqualStrings("Green BG", text.plain);
    try std.testing.expectEqual(@as(usize, 1), text.spans.len);
    try std.testing.expect(text.spans[0].style.bgcolor != null);
}

test "fromAnsi osc stripped" {
    const allocator = std.testing.allocator;
    var text = try fromAnsi(allocator, "\x1b]0;Title\x07Normal");
    defer text.deinit();

    try std.testing.expectEqualStrings("Normal", text.plain);
}

test "stripAnsi basic" {
    const allocator = std.testing.allocator;
    const result = try stripAnsi(allocator, "\x1b[1;31mHello\x1b[0m World");
    defer allocator.free(result);

    try std.testing.expectEqualStrings("Hello World", result);
}

test "stripAnsi no escapes" {
    const allocator = std.testing.allocator;
    const result = try stripAnsi(allocator, "Plain text");
    defer allocator.free(result);

    try std.testing.expectEqualStrings("Plain text", result);
}

test "stripAnsi osc" {
    const allocator = std.testing.allocator;
    const result = try stripAnsi(allocator, "\x1b]8;;https://example.com\x1b\\Link\x1b]8;;\x1b\\");
    defer allocator.free(result);

    try std.testing.expectEqualStrings("Link", result);
}

test "applySgr reset" {
    const style = applySgr(Style.empty.bold(), &.{0});
    try std.testing.expect(style.isEmpty());
}

test "applySgr combined" {
    const style = applySgr(Style.empty, &.{ 1, 31 });
    try std.testing.expect(style.hasAttribute(.bold));
    try std.testing.expect(style.color != null);
}
