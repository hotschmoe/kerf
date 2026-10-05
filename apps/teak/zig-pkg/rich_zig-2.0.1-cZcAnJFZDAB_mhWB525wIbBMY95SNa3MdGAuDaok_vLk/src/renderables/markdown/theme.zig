const std = @import("std");
const Style = @import("../../style.zig").Style;
const Color = @import("../../color.zig").Color;
const SyntaxTheme = @import("../syntax/mod.zig").SyntaxTheme;
const BoxStyle = @import("../../box.zig").BoxStyle;

pub const HeaderLevel = enum(u3) {
    h1 = 1,
    h2 = 2,
    h3 = 3,
    h4 = 4,
    h5 = 5,
    h6 = 6,

    pub fn fromCount(count: usize) ?HeaderLevel {
        return switch (count) {
            1 => .h1,
            2 => .h2,
            3 => .h3,
            4 => .h4,
            5 => .h5,
            6 => .h6,
            else => null,
        };
    }
};

pub const MarkdownTheme = struct {
    h1_style: Style = Style.empty.bold().fg(Color.bright_cyan),
    h2_style: Style = Style.empty.bold().fg(Color.bright_blue),
    h3_style: Style = Style.empty.bold().fg(Color.bright_magenta),
    h4_style: Style = Style.empty.bold().fg(Color.cyan),
    h5_style: Style = Style.empty.bold().fg(Color.blue),
    h6_style: Style = Style.empty.bold().fg(Color.magenta).dim(),
    h1_underline: ?[]const u8 = "\u{2550}",
    h2_underline: ?[]const u8 = "\u{2500}",
    bold_style: Style = Style.empty.bold(),
    italic_style: Style = Style.empty.italic(),
    bold_italic_style: Style = Style.empty.bold().italic(),
    strikethrough_style: Style = Style.empty.strike(),
    link_style: Style = Style.empty.fg(Color.bright_blue).underline(),
    code_block_style: Style = Style.empty.foreground(Color.default).dim(),
    syntax_theme: SyntaxTheme = SyntaxTheme.default,
    list_number_style: Style = Style.empty.fg(Color.bright_yellow),
    list_bullet_style: Style = Style.empty.fg(Color.bright_cyan),
    list_bullet_char: []const u8 = "\u{2022}",
    list_indent: usize = 3,
    task_unchecked_char: []const u8 = "\u{2610}",
    task_checked_char: []const u8 = "\u{2611}",
    task_unchecked_style: Style = Style.empty.fg(Color.bright_black),
    task_checked_style: Style = Style.empty.fg(Color.bright_green),
    blockquote_style: Style = Style.empty.fg(Color.bright_black).italic(),
    blockquote_border_style: Style = Style.empty.fg(Color.bright_cyan),
    blockquote_border_char: []const u8 = "\u{2502}",
    blockquote_indent: usize = 2,
    inline_code_style: Style = Style.empty.fg(Color.bright_yellow).dim(),
    image_style: Style = Style.empty.fg(Color.bright_black).italic(),
    image_prefix: []const u8 = "[Image: ",
    image_suffix: []const u8 = "]",
    rule_style: Style = Style.empty.fg(Color.bright_black),
    rule_char: []const u8 = "\u{2500}",
    table_header_style: Style = Style.empty.bold().fg(Color.bright_cyan),
    table_border_style: Style = Style.empty.fg(Color.bright_black),
    table_box_style: BoxStyle = BoxStyle.rounded,

    pub const default: MarkdownTheme = .{};

    pub fn styleForLevel(self: MarkdownTheme, level: HeaderLevel) Style {
        return switch (level) {
            .h1 => self.h1_style,
            .h2 => self.h2_style,
            .h3 => self.h3_style,
            .h4 => self.h4_style,
            .h5 => self.h5_style,
            .h6 => self.h6_style,
        };
    }

    pub fn underlineForLevel(self: MarkdownTheme, level: HeaderLevel) ?[]const u8 {
        return switch (level) {
            .h1 => self.h1_underline,
            .h2 => self.h2_underline,
            else => null,
        };
    }
};

test "HeaderLevel.fromCount" {
    try std.testing.expectEqual(HeaderLevel.h1, HeaderLevel.fromCount(1).?);
    try std.testing.expectEqual(HeaderLevel.h6, HeaderLevel.fromCount(6).?);
    try std.testing.expect(HeaderLevel.fromCount(0) == null);
    try std.testing.expect(HeaderLevel.fromCount(7) == null);
}

test "MarkdownTheme.styleForLevel returns distinct styles" {
    const theme = MarkdownTheme.default;
    const h1_style = theme.styleForLevel(.h1);
    const h6_style = theme.styleForLevel(.h6);

    try std.testing.expect(h1_style.hasAttribute(.bold));
    try std.testing.expect(h6_style.hasAttribute(.bold));
    try std.testing.expect(!h1_style.eql(h6_style));
}
