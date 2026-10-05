const std = @import("std");

pub const CustomChars = struct {
    top_left: []const u8 = "+",
    top_right: []const u8 = "+",
    bottom_left: []const u8 = "+",
    bottom_right: []const u8 = "+",
    horizontal: []const u8 = "-",
    vertical: []const u8 = "|",
    left: []const u8 = "|",
    right: []const u8 = "|",
    cross: []const u8 = "+",
    top_tee: []const u8 = "+",
    bottom_tee: []const u8 = "+",
    left_tee: []const u8 = "+",
    right_tee: []const u8 = "+",
};

pub const BoxStyle = struct {
    top_left: []const u8,
    top_right: []const u8,
    bottom_left: []const u8,
    bottom_right: []const u8,
    horizontal: []const u8,
    vertical: []const u8,
    left: []const u8,
    right: []const u8,
    cross: []const u8,
    top_tee: []const u8,
    bottom_tee: []const u8,
    left_tee: []const u8,
    right_tee: []const u8,

    pub fn custom(chars: CustomChars) BoxStyle {
        return .{
            .top_left = chars.top_left,
            .top_right = chars.top_right,
            .bottom_left = chars.bottom_left,
            .bottom_right = chars.bottom_right,
            .horizontal = chars.horizontal,
            .vertical = chars.vertical,
            .left = chars.left,
            .right = chars.right,
            .cross = chars.cross,
            .top_tee = chars.top_tee,
            .bottom_tee = chars.bottom_tee,
            .left_tee = chars.left_tee,
            .right_tee = chars.right_tee,
        };
    }

    pub const rounded: BoxStyle = .{
        .top_left = "\u{256D}", // rounded corners
        .top_right = "\u{256E}",
        .bottom_left = "\u{2570}",
        .bottom_right = "\u{256F}",
        .horizontal = "\u{2500}",
        .vertical = "\u{2502}",
        .left = "\u{2502}",
        .right = "\u{2502}",
        .cross = "\u{253C}",
        .top_tee = "\u{252C}",
        .bottom_tee = "\u{2534}",
        .left_tee = "\u{251C}",
        .right_tee = "\u{2524}",
    };

    pub const square: BoxStyle = .{
        .top_left = "\u{250C}",
        .top_right = "\u{2510}",
        .bottom_left = "\u{2514}",
        .bottom_right = "\u{2518}",
        .horizontal = "\u{2500}",
        .vertical = "\u{2502}",
        .left = "\u{2502}",
        .right = "\u{2502}",
        .cross = "\u{253C}",
        .top_tee = "\u{252C}",
        .bottom_tee = "\u{2534}",
        .left_tee = "\u{251C}",
        .right_tee = "\u{2524}",
    };

    pub const heavy: BoxStyle = .{
        .top_left = "\u{250F}",
        .top_right = "\u{2513}",
        .bottom_left = "\u{2517}",
        .bottom_right = "\u{251B}",
        .horizontal = "\u{2501}",
        .vertical = "\u{2503}",
        .left = "\u{2503}",
        .right = "\u{2503}",
        .cross = "\u{254B}",
        .top_tee = "\u{2533}",
        .bottom_tee = "\u{253B}",
        .left_tee = "\u{2523}",
        .right_tee = "\u{252B}",
    };

    pub const double: BoxStyle = .{
        .top_left = "\u{2554}",
        .top_right = "\u{2557}",
        .bottom_left = "\u{255A}",
        .bottom_right = "\u{255D}",
        .horizontal = "\u{2550}",
        .vertical = "\u{2551}",
        .left = "\u{2551}",
        .right = "\u{2551}",
        .cross = "\u{256C}",
        .top_tee = "\u{2566}",
        .bottom_tee = "\u{2569}",
        .left_tee = "\u{2560}",
        .right_tee = "\u{2563}",
    };

    pub const ascii: BoxStyle = .{
        .top_left = "+",
        .top_right = "+",
        .bottom_left = "+",
        .bottom_right = "+",
        .horizontal = "-",
        .vertical = "|",
        .left = "|",
        .right = "|",
        .cross = "+",
        .top_tee = "+",
        .bottom_tee = "+",
        .left_tee = "+",
        .right_tee = "+",
    };

    pub const minimal: BoxStyle = .{
        .top_left = " ",
        .top_right = " ",
        .bottom_left = " ",
        .bottom_right = " ",
        .horizontal = "\u{2500}",
        .vertical = " ",
        .left = " ",
        .right = " ",
        .cross = " ",
        .top_tee = " ",
        .bottom_tee = " ",
        .left_tee = " ",
        .right_tee = " ",
    };

    pub const simple: BoxStyle = .{
        .top_left = " ",
        .top_right = " ",
        .bottom_left = " ",
        .bottom_right = " ",
        .horizontal = "\u{2500}",
        .vertical = "\u{2502}",
        .left = "\u{2502}",
        .right = "\u{2502}",
        .cross = "\u{253C}",
        .top_tee = "\u{252C}",
        .bottom_tee = "\u{2534}",
        .left_tee = "\u{251C}",
        .right_tee = "\u{2524}",
    };

    pub const none: BoxStyle = .{
        .top_left = " ",
        .top_right = " ",
        .bottom_left = " ",
        .bottom_right = " ",
        .horizontal = " ",
        .vertical = " ",
        .left = " ",
        .right = " ",
        .cross = " ",
        .top_tee = " ",
        .bottom_tee = " ",
        .left_tee = " ",
        .right_tee = " ",
    };

    pub const horizontals: BoxStyle = .{
        .top_left = " ",
        .top_right = " ",
        .bottom_left = " ",
        .bottom_right = " ",
        .horizontal = "\u{2500}",
        .vertical = " ",
        .left = " ",
        .right = " ",
        .cross = "\u{2500}",
        .top_tee = "\u{2500}",
        .bottom_tee = "\u{2500}",
        .left_tee = " ",
        .right_tee = " ",
    };

    pub const markdown: BoxStyle = .{
        .top_left = "|",
        .top_right = "|",
        .bottom_left = "|",
        .bottom_right = "|",
        .horizontal = "-",
        .vertical = "|",
        .left = "|",
        .right = "|",
        .cross = "|",
        .top_tee = "|",
        .bottom_tee = "|",
        .left_tee = "|",
        .right_tee = "|",
    };

    pub fn getHorizontal(self: BoxStyle, count: usize, allocator: std.mem.Allocator) ![]u8 {
        const char_len = self.horizontal.len;
        const result = try allocator.alloc(u8, count * char_len);
        for (0..count) |i| {
            @memcpy(result[i * char_len ..][0..char_len], self.horizontal);
        }
        return result;
    }

    pub fn getVertical(self: BoxStyle, count: usize, allocator: std.mem.Allocator) ![]u8 {
        const char_len = self.vertical.len;
        const line_len = char_len + 1;
        const result = try allocator.alloc(u8, count * line_len);
        for (0..count) |i| {
            @memcpy(result[i * line_len ..][0..char_len], self.vertical);
            result[i * line_len + char_len] = '\n';
        }
        return result;
    }
};

// Tests
test "BoxStyle.rounded characters" {
    const rounded = BoxStyle.rounded;
    try std.testing.expectEqualStrings("\u{256D}", rounded.top_left);
    try std.testing.expectEqualStrings("\u{256E}", rounded.top_right);
    try std.testing.expectEqualStrings("\u{2500}", rounded.horizontal);
}

test "BoxStyle.ascii characters" {
    const ascii = BoxStyle.ascii;
    try std.testing.expectEqualStrings("+", ascii.top_left);
    try std.testing.expectEqualStrings("-", ascii.horizontal);
    try std.testing.expectEqualStrings("|", ascii.vertical);
}

test "BoxStyle.getHorizontal" {
    const allocator = std.testing.allocator;
    const line = try BoxStyle.ascii.getHorizontal(5, allocator);
    defer allocator.free(line);
    try std.testing.expectEqualStrings("-----", line);
}

test "BoxStyle.getHorizontal unicode" {
    const allocator = std.testing.allocator;
    const line = try BoxStyle.heavy.getHorizontal(3, allocator);
    defer allocator.free(line);
    try std.testing.expectEqualStrings("\u{2501}\u{2501}\u{2501}", line);
}

test "BoxStyle.horizontals" {
    const h = BoxStyle.horizontals;
    try std.testing.expectEqualStrings("\u{2500}", h.horizontal);
    try std.testing.expectEqualStrings(" ", h.vertical);
    try std.testing.expectEqualStrings(" ", h.top_left);
}

test "BoxStyle.markdown" {
    const m = BoxStyle.markdown;
    try std.testing.expectEqualStrings("-", m.horizontal);
    try std.testing.expectEqualStrings("|", m.vertical);
    try std.testing.expectEqualStrings("|", m.top_left);
}

test "BoxStyle.custom" {
    const custom_box = BoxStyle.custom(.{
        .top_left = "*",
        .top_right = "*",
        .bottom_left = "*",
        .bottom_right = "*",
        .horizontal = "=",
        .vertical = "!",
    });
    try std.testing.expectEqualStrings("*", custom_box.top_left);
    try std.testing.expectEqualStrings("=", custom_box.horizontal);
    try std.testing.expectEqualStrings("!", custom_box.vertical);
    try std.testing.expectEqualStrings("+", custom_box.cross);
}

test "BoxStyle.custom defaults" {
    const default_custom = BoxStyle.custom(.{});
    try std.testing.expectEqualStrings("+", default_custom.top_left);
    try std.testing.expectEqualStrings("-", default_custom.horizontal);
    try std.testing.expectEqualStrings("|", default_custom.vertical);
}
