const std = @import("std");
const Segment = @import("../segment.zig").Segment;
const Style = @import("../style.zig").Style;
const cells = @import("../cells.zig");
const segment = @import("../segment.zig");

pub const Padding = struct {
    content: []const Segment,
    top: u8 = 0,
    right: u8 = 0,
    bottom: u8 = 0,
    left: u8 = 0,
    style: ?Style = null,

    pub fn init(content: []const Segment) Padding {
        return .{ .content = content };
    }

    pub fn uniform(self: Padding, n: u8) Padding {
        var p = self;
        p.top = n;
        p.right = n;
        p.bottom = n;
        p.left = n;
        return p;
    }

    pub fn horizontal(self: Padding, n: u8) Padding {
        var p = self;
        p.left = n;
        p.right = n;
        return p;
    }

    pub fn vertical(self: Padding, n: u8) Padding {
        var p = self;
        p.top = n;
        p.bottom = n;
        return p;
    }

    pub fn withPadding(self: Padding, top: u8, right: u8, bottom: u8, left: u8) Padding {
        var p = self;
        p.top = top;
        p.right = right;
        p.bottom = bottom;
        p.left = left;
        return p;
    }

    pub fn withStyle(self: Padding, s: Style) Padding {
        var p = self;
        p.style = s;
        return p;
    }

    pub fn render(self: Padding, max_width: usize, allocator: std.mem.Allocator) ![]Segment {
        var result: std.ArrayList(Segment) = .empty;

        const lines = try segment.splitIntoLines(self.content, allocator);
        defer allocator.free(lines);

        const max_content_width = segment.maxLineWidth(lines);
        const total_width = max_content_width + self.left + self.right;
        const effective_width = @min(total_width, max_width);

        // Top padding
        for (0..self.top) |_| {
            try self.renderBlankLine(&result, allocator, effective_width);
        }

        // Content lines with left/right padding
        for (lines) |line| {
            try self.renderContentLine(&result, allocator, line, max_content_width);
        }

        // Bottom padding
        for (0..self.bottom) |_| {
            try self.renderBlankLine(&result, allocator, effective_width);
        }

        return result.toOwnedSlice(allocator);
    }

    fn renderBlankLine(self: Padding, result: *std.ArrayList(Segment), allocator: std.mem.Allocator, width: usize) !void {
        try self.renderSpaces(result, allocator, width);
        try result.append(allocator, Segment.line());
    }

    fn renderContentLine(self: Padding, result: *std.ArrayList(Segment), allocator: std.mem.Allocator, line: []const Segment, max_content_width: usize) !void {
        try self.renderSpaces(result, allocator, self.left);

        for (line) |seg| {
            try result.append(allocator, seg);
        }

        const line_width = segment.totalCellLength(line);
        const right_fill = if (max_content_width > line_width) max_content_width - line_width else 0;
        try self.renderSpaces(result, allocator, right_fill + self.right);

        try result.append(allocator, Segment.line());
    }

    fn renderSpaces(self: Padding, result: *std.ArrayList(Segment), allocator: std.mem.Allocator, count: usize) !void {
        const space_seg = Segment.styledOptional(" ", self.style);
        for (0..count) |_| {
            try result.append(allocator, space_seg);
        }
    }
};

// Tests
test "Padding.init" {
    const segments = [_]Segment{Segment.plain("Hello")};
    const pad = Padding.init(&segments);

    try std.testing.expectEqual(@as(u8, 0), pad.top);
    try std.testing.expectEqual(@as(u8, 0), pad.right);
}

test "Padding.uniform" {
    const segments = [_]Segment{Segment.plain("Hello")};
    const pad = Padding.init(&segments).uniform(2);

    try std.testing.expectEqual(@as(u8, 2), pad.top);
    try std.testing.expectEqual(@as(u8, 2), pad.right);
    try std.testing.expectEqual(@as(u8, 2), pad.bottom);
    try std.testing.expectEqual(@as(u8, 2), pad.left);
}

test "Padding.horizontal" {
    const segments = [_]Segment{Segment.plain("Hello")};
    const pad = Padding.init(&segments).horizontal(3);

    try std.testing.expectEqual(@as(u8, 0), pad.top);
    try std.testing.expectEqual(@as(u8, 3), pad.right);
    try std.testing.expectEqual(@as(u8, 0), pad.bottom);
    try std.testing.expectEqual(@as(u8, 3), pad.left);
}

test "Padding.vertical" {
    const segments = [_]Segment{Segment.plain("Hello")};
    const pad = Padding.init(&segments).vertical(1);

    try std.testing.expectEqual(@as(u8, 1), pad.top);
    try std.testing.expectEqual(@as(u8, 0), pad.right);
    try std.testing.expectEqual(@as(u8, 1), pad.bottom);
    try std.testing.expectEqual(@as(u8, 0), pad.left);
}

test "Padding.render basic" {
    const allocator = std.testing.allocator;
    const segments = [_]Segment{Segment.plain("Hi")};
    const pad = Padding.init(&segments).uniform(1);

    const rendered = try pad.render(80, allocator);
    defer allocator.free(rendered);

    var line_count: usize = 0;
    for (rendered) |seg| {
        if (std.mem.eql(u8, seg.text, "\n")) {
            line_count += 1;
        }
    }
    try std.testing.expectEqual(@as(usize, 3), line_count);
}

test "Padding.withPadding" {
    const segments = [_]Segment{Segment.plain("Test")};
    const pad = Padding.init(&segments).withPadding(1, 2, 3, 4);

    try std.testing.expectEqual(@as(u8, 1), pad.top);
    try std.testing.expectEqual(@as(u8, 2), pad.right);
    try std.testing.expectEqual(@as(u8, 3), pad.bottom);
    try std.testing.expectEqual(@as(u8, 4), pad.left);
}

test "Padding.withStyle" {
    const segments = [_]Segment{Segment.plain("Test")};
    const pad = Padding.init(&segments).withStyle(Style.empty.bold());

    try std.testing.expect(pad.style != null);
    try std.testing.expect(pad.style.?.hasAttribute(.bold));
}
