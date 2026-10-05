const std = @import("std");
const Segment = @import("../segment.zig").Segment;
const Style = @import("../style.zig").Style;
const cells = @import("../cells.zig");
const segment = @import("../segment.zig");

pub const HAlign = enum {
    left,
    center,
    right,
};

pub const VAlign = enum {
    top,
    middle,
    bottom,
};

pub const Align = struct {
    content: []const Segment,
    horizontal: HAlign = .left,
    vertical: VAlign = .top,
    width: ?usize = null,
    height: ?usize = null,
    pad_style: ?Style = null,

    pub fn init(content: []const Segment) Align {
        return .{ .content = content };
    }

    pub fn left(self: Align) Align {
        var a = self;
        a.horizontal = .left;
        return a;
    }

    pub fn center(self: Align) Align {
        var a = self;
        a.horizontal = .center;
        return a;
    }

    pub fn right(self: Align) Align {
        var a = self;
        a.horizontal = .right;
        return a;
    }

    pub fn top(self: Align) Align {
        var a = self;
        a.vertical = .top;
        return a;
    }

    pub fn middle(self: Align) Align {
        var a = self;
        a.vertical = .middle;
        return a;
    }

    pub fn bottom(self: Align) Align {
        var a = self;
        a.vertical = .bottom;
        return a;
    }

    pub fn withWidth(self: Align, w: usize) Align {
        var a = self;
        a.width = w;
        return a;
    }

    pub fn withHeight(self: Align, h: usize) Align {
        var a = self;
        a.height = h;
        return a;
    }

    pub fn withPadStyle(self: Align, s: Style) Align {
        var a = self;
        a.pad_style = s;
        return a;
    }

    pub fn render(self: Align, max_width: usize, allocator: std.mem.Allocator) ![]Segment {
        var result: std.ArrayList(Segment) = .empty;

        const lines = try segment.splitIntoLines(self.content, allocator);
        defer allocator.free(lines);

        const max_content_width = segment.maxLineWidth(lines);
        const target_width = self.width orelse @min(max_content_width, max_width);
        const content_height = lines.len;
        const target_height = self.height orelse content_height;

        // Calculate vertical padding
        const top_padding: usize = switch (self.vertical) {
            .top => 0,
            .middle => if (target_height > content_height) (target_height - content_height) / 2 else 0,
            .bottom => if (target_height > content_height) target_height - content_height else 0,
        };
        const bottom_padding: usize = if (target_height > content_height + top_padding)
            target_height - content_height - top_padding
        else
            0;

        for (0..top_padding) |_| {
            try self.renderBlankLine(&result, allocator, target_width);
        }

        for (lines) |line| {
            try self.renderAlignedLine(&result, allocator, line, target_width);
        }

        for (0..bottom_padding) |_| {
            try self.renderBlankLine(&result, allocator, target_width);
        }

        return result.toOwnedSlice(allocator);
    }

    fn renderBlankLine(self: Align, result: *std.ArrayList(Segment), allocator: std.mem.Allocator, width: usize) !void {
        try self.renderSpaces(result, allocator, width);
        try result.append(allocator, Segment.line());
    }

    fn renderAlignedLine(self: Align, result: *std.ArrayList(Segment), allocator: std.mem.Allocator, line: []const Segment, target_width: usize) !void {
        const line_width = segment.totalCellLength(line);
        const padding_total = if (target_width > line_width) target_width - line_width else 0;

        const left_padding: usize = switch (self.horizontal) {
            .left => 0,
            .center => padding_total / 2,
            .right => padding_total,
        };
        const right_padding = padding_total - left_padding;

        try self.renderSpaces(result, allocator, left_padding);

        for (line) |seg| {
            try result.append(allocator, seg);
        }

        try self.renderSpaces(result, allocator, right_padding);
        try result.append(allocator, Segment.line());
    }

    fn renderSpaces(self: Align, result: *std.ArrayList(Segment), allocator: std.mem.Allocator, count: usize) !void {
        const space_seg = Segment.styledOptional(" ", self.pad_style);
        for (0..count) |_| {
            try result.append(allocator, space_seg);
        }
    }
};

// Tests
test "Align.init" {
    const segments = [_]Segment{Segment.plain("Hello")};
    const align_obj = Align.init(&segments);

    try std.testing.expectEqual(HAlign.left, align_obj.horizontal);
    try std.testing.expectEqual(VAlign.top, align_obj.vertical);
}

test "Align.center" {
    const segments = [_]Segment{Segment.plain("Hello")};
    const align_obj = Align.init(&segments).center();

    try std.testing.expectEqual(HAlign.center, align_obj.horizontal);
}

test "Align.right" {
    const segments = [_]Segment{Segment.plain("Hello")};
    const align_obj = Align.init(&segments).right();

    try std.testing.expectEqual(HAlign.right, align_obj.horizontal);
}

test "Align.middle" {
    const segments = [_]Segment{Segment.plain("Hello")};
    const align_obj = Align.init(&segments).middle();

    try std.testing.expectEqual(VAlign.middle, align_obj.vertical);
}

test "Align.bottom" {
    const segments = [_]Segment{Segment.plain("Hello")};
    const align_obj = Align.init(&segments).bottom();

    try std.testing.expectEqual(VAlign.bottom, align_obj.vertical);
}

test "Align.withWidth" {
    const segments = [_]Segment{Segment.plain("Hi")};
    const align_obj = Align.init(&segments).withWidth(10);

    try std.testing.expectEqual(@as(?usize, 10), align_obj.width);
}

test "Align.withHeight" {
    const segments = [_]Segment{Segment.plain("Hi")};
    const align_obj = Align.init(&segments).withHeight(5);

    try std.testing.expectEqual(@as(?usize, 5), align_obj.height);
}

test "Align.render left alignment" {
    const allocator = std.testing.allocator;
    const segments = [_]Segment{Segment.plain("Hi")};
    const align_obj = Align.init(&segments).left().withWidth(5);

    const rendered = try align_obj.render(80, allocator);
    defer allocator.free(rendered);

    try std.testing.expect(rendered.len > 0);
    try std.testing.expectEqualStrings("Hi", rendered[0].text);
}

test "Align.render center alignment" {
    const allocator = std.testing.allocator;
    const segments = [_]Segment{Segment.plain("Hi")};
    const align_obj = Align.init(&segments).center().withWidth(6);

    const rendered = try align_obj.render(80, allocator);
    defer allocator.free(rendered);

    try std.testing.expect(rendered.len >= 5);
}

test "Align.render right alignment" {
    const allocator = std.testing.allocator;
    const segments = [_]Segment{Segment.plain("Hi")};
    const align_obj = Align.init(&segments).right().withWidth(5);

    const rendered = try align_obj.render(80, allocator);
    defer allocator.free(rendered);

    try std.testing.expect(rendered.len >= 4);
    try std.testing.expectEqualStrings(" ", rendered[0].text);
    try std.testing.expectEqualStrings(" ", rendered[1].text);
    try std.testing.expectEqualStrings(" ", rendered[2].text);
    try std.testing.expectEqualStrings("Hi", rendered[3].text);
}

test "Align.render with vertical middle" {
    const allocator = std.testing.allocator;
    const segments = [_]Segment{Segment.plain("Hi")};
    const align_obj = Align.init(&segments).middle().withHeight(3).withWidth(2);

    const rendered = try align_obj.render(80, allocator);
    defer allocator.free(rendered);

    var line_count: usize = 0;
    for (rendered) |seg| {
        if (std.mem.eql(u8, seg.text, "\n")) {
            line_count += 1;
        }
    }
    try std.testing.expectEqual(@as(usize, 3), line_count);
}
