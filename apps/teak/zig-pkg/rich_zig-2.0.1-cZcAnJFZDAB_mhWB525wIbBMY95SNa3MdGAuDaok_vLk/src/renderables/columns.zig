const std = @import("std");
const Segment = @import("../segment.zig").Segment;
const segment_mod = @import("../segment.zig");
const Style = @import("../style.zig").Style;
const cells = @import("../cells.zig");
const HAlign = @import("align.zig").HAlign;

pub const Columns = struct {
    items: []const []const Segment,
    column_count: ?usize = null,
    equal_width: bool = true,
    expand: bool = false,
    padding: u8 = 2,
    min_column_width: ?usize = null,
    max_column_width: ?usize = null,
    alignment: HAlign = .left,
    allocator: std.mem.Allocator,

    pub fn init(allocator: std.mem.Allocator, items: []const []const Segment) Columns {
        return .{
            .items = items,
            .allocator = allocator,
        };
    }

    pub fn fromText(allocator: std.mem.Allocator, texts: []const []const u8) !Columns {
        const items = try allocator.alloc([]const Segment, texts.len);
        for (texts, 0..) |txt, i| {
            const seg_arr = try allocator.alloc(Segment, 1);
            seg_arr[0] = Segment.plain(txt);
            items[i] = seg_arr;
        }
        return .{
            .items = items,
            .allocator = allocator,
        };
    }

    pub fn withColumnCount(self: Columns, count: usize) Columns {
        var c = self;
        c.column_count = count;
        return c;
    }

    pub fn withPadding(self: Columns, gap: u8) Columns {
        var c = self;
        c.padding = gap;
        return c;
    }

    pub fn withEqualWidth(self: Columns, equal: bool) Columns {
        var c = self;
        c.equal_width = equal;
        return c;
    }

    pub fn withExpand(self: Columns, exp: bool) Columns {
        var c = self;
        c.expand = exp;
        return c;
    }

    pub fn withMinWidth(self: Columns, w: usize) Columns {
        var c = self;
        c.min_column_width = w;
        return c;
    }

    pub fn withMaxWidth(self: Columns, w: usize) Columns {
        var c = self;
        c.max_column_width = w;
        return c;
    }

    pub fn withAlign(self: Columns, a: HAlign) Columns {
        var c = self;
        c.alignment = a;
        return c;
    }

    pub fn left(self: Columns) Columns {
        return self.withAlign(.left);
    }

    pub fn center(self: Columns) Columns {
        return self.withAlign(.center);
    }

    pub fn right(self: Columns) Columns {
        return self.withAlign(.right);
    }

    fn calculateItemWidth(self: Columns, item: []const Segment) usize {
        const lines = segment_mod.splitIntoLines(item, self.allocator) catch return 0;
        defer self.allocator.free(lines);
        return segment_mod.maxLineWidth(lines);
    }

    fn applyWidthConstraints(self: Columns, width: usize) usize {
        var w = width;
        if (self.min_column_width) |min| {
            w = @max(w, min);
        }
        if (self.max_column_width) |max| {
            w = @min(w, max);
        }
        return w;
    }

    fn calculateColumnCount(self: Columns, max_width: usize) usize {
        if (self.column_count) |count| return count;
        if (self.items.len == 0) return 1;

        var max_item_width: usize = 0;
        for (self.items) |item| {
            max_item_width = @max(max_item_width, self.calculateItemWidth(item));
        }

        max_item_width = self.applyWidthConstraints(max_item_width);
        if (max_item_width == 0) return 1;

        var cols: usize = 1;
        while (cols < self.items.len) {
            const next_cols = cols + 1;
            const total_width = next_cols * max_item_width + (next_cols - 1) * self.padding;
            if (total_width > max_width) break;
            cols = next_cols;
        }

        return cols;
    }

    fn calculateColumnWidths(self: Columns, col_count: usize, max_width: usize, allocator: std.mem.Allocator) ![]usize {
        const widths = try allocator.alloc(usize, col_count);
        @memset(widths, 0);

        if (self.equal_width or self.expand) {
            const padding_total = if (col_count > 1) (col_count - 1) * self.padding else 0;
            const available = if (max_width > padding_total) max_width - padding_total else 0;
            const col_width = available / col_count;
            @memset(widths, col_width);
        } else {
            const row_count = (self.items.len + col_count - 1) / col_count;

            for (0..row_count) |row_idx| {
                for (0..col_count) |col_idx| {
                    const item_idx = row_idx * col_count + col_idx;
                    if (item_idx < self.items.len) {
                        const item_width = self.calculateItemWidth(self.items[item_idx]);
                        widths[col_idx] = @max(widths[col_idx], item_width);
                    }
                }
            }
        }

        for (widths) |*w| {
            w.* = self.applyWidthConstraints(w.*);
        }

        return widths;
    }

    pub fn render(self: Columns, max_width: usize, allocator: std.mem.Allocator) ![]Segment {
        var segments: std.ArrayList(Segment) = .empty;

        if (self.items.len == 0) {
            return segments.toOwnedSlice(allocator);
        }

        const col_count = self.calculateColumnCount(max_width);
        const col_widths = try self.calculateColumnWidths(col_count, max_width, allocator);
        defer allocator.free(col_widths);

        const row_count = (self.items.len + col_count - 1) / col_count;

        for (0..row_count) |row_idx| {
            var item_lines: std.ArrayList([]const []const Segment) = .empty;
            defer {
                for (item_lines.items) |lines| {
                    allocator.free(lines);
                }
                item_lines.deinit(allocator);
            }

            var max_line_count: usize = 0;
            for (0..col_count) |col_idx| {
                const item_idx = row_idx * col_count + col_idx;
                if (item_idx < self.items.len) {
                    const lines = try segment_mod.splitIntoLines(self.items[item_idx], allocator);
                    try item_lines.append(allocator, lines);
                    if (lines.len > max_line_count) max_line_count = lines.len;
                } else {
                    const empty_lines = try allocator.alloc([]const Segment, 1);
                    empty_lines[0] = &[_]Segment{};
                    try item_lines.append(allocator, empty_lines);
                }
            }

            for (0..max_line_count) |line_idx| {
                for (0..col_count) |col_idx| {
                    const col_width = col_widths[col_idx];
                    const lines = item_lines.items[col_idx];
                    const line = if (line_idx < lines.len) lines[line_idx] else &[_]Segment{};

                    try self.renderAlignedLine(&segments, allocator, line, col_width);

                    if (col_idx < col_count - 1) {
                        for (0..self.padding) |_| {
                            try segments.append(allocator, Segment.plain(" "));
                        }
                    }
                }
                try segments.append(allocator, Segment.line());
            }
        }

        return segments.toOwnedSlice(allocator);
    }

    fn renderAlignedLine(self: Columns, segments: *std.ArrayList(Segment), allocator: std.mem.Allocator, line: []const Segment, width: usize) !void {
        const line_width = segment_mod.totalCellLength(line);
        const padding_total = if (width > line_width) width - line_width else 0;

        const left_pad: usize = switch (self.alignment) {
            .left => 0,
            .center => padding_total / 2,
            .right => padding_total,
        };
        const right_pad = padding_total - left_pad;

        for (0..left_pad) |_| {
            try segments.append(allocator, Segment.plain(" "));
        }

        for (line) |seg| {
            try segments.append(allocator, seg);
        }

        for (0..right_pad) |_| {
            try segments.append(allocator, Segment.plain(" "));
        }
    }

    pub fn deinit(self: *Columns) void {
        for (self.items) |item| {
            self.allocator.free(item);
        }
        self.allocator.free(self.items);
    }
};

test "Columns.init" {
    const allocator = std.testing.allocator;
    const seg1 = [_]Segment{Segment.plain("One")};
    const seg2 = [_]Segment{Segment.plain("Two")};
    const items = [_][]const Segment{ &seg1, &seg2 };

    const cols = Columns.init(allocator, &items);

    try std.testing.expectEqual(@as(usize, 2), cols.items.len);
    try std.testing.expectEqual(@as(?usize, null), cols.column_count);
    try std.testing.expect(cols.equal_width);
}

test "Columns.withColumnCount" {
    const allocator = std.testing.allocator;
    const items = [_][]const Segment{};
    const cols = Columns.init(allocator, &items).withColumnCount(3);

    try std.testing.expectEqual(@as(?usize, 3), cols.column_count);
}

test "Columns.withPadding" {
    const allocator = std.testing.allocator;
    const items = [_][]const Segment{};
    const cols = Columns.init(allocator, &items).withPadding(4);

    try std.testing.expectEqual(@as(u8, 4), cols.padding);
}

test "Columns.withEqualWidth" {
    const allocator = std.testing.allocator;
    const items = [_][]const Segment{};
    const cols = Columns.init(allocator, &items).withEqualWidth(false);

    try std.testing.expect(!cols.equal_width);
}

test "Columns.withExpand" {
    const allocator = std.testing.allocator;
    const items = [_][]const Segment{};
    const cols = Columns.init(allocator, &items).withExpand(true);

    try std.testing.expect(cols.expand);
}

test "Columns.alignments" {
    const allocator = std.testing.allocator;
    const items = [_][]const Segment{};

    const left_cols = Columns.init(allocator, &items).left();
    try std.testing.expectEqual(HAlign.left, left_cols.alignment);

    const center_cols = Columns.init(allocator, &items).center();
    try std.testing.expectEqual(HAlign.center, center_cols.alignment);

    const right_cols = Columns.init(allocator, &items).right();
    try std.testing.expectEqual(HAlign.right, right_cols.alignment);
}

test "Columns.render empty" {
    const allocator = std.testing.allocator;
    const items = [_][]const Segment{};
    const cols = Columns.init(allocator, &items);

    const segments = try cols.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expectEqual(@as(usize, 0), segments.len);
}

test "Columns.render basic" {
    const allocator = std.testing.allocator;
    const seg1 = [_]Segment{Segment.plain("One")};
    const seg2 = [_]Segment{Segment.plain("Two")};
    const seg3 = [_]Segment{Segment.plain("Three")};
    const seg4 = [_]Segment{Segment.plain("Four")};
    const items = [_][]const Segment{ &seg1, &seg2, &seg3, &seg4 };

    const cols = Columns.init(allocator, &items).withColumnCount(2);
    const segments = try cols.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len > 0);

    var line_count: usize = 0;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "\n")) {
            line_count += 1;
        }
    }
    try std.testing.expectEqual(@as(usize, 2), line_count);
}

test "Columns.render auto column count" {
    const allocator = std.testing.allocator;
    const seg1 = [_]Segment{Segment.plain("A")};
    const seg2 = [_]Segment{Segment.plain("B")};
    const seg3 = [_]Segment{Segment.plain("C")};
    const items = [_][]const Segment{ &seg1, &seg2, &seg3 };

    const cols = Columns.init(allocator, &items);
    const segments = try cols.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len > 0);
}

test "Columns.fromText" {
    const allocator = std.testing.allocator;
    const texts = [_][]const u8{ "One", "Two", "Three" };

    var cols = try Columns.fromText(allocator, &texts);
    defer cols.deinit();

    try std.testing.expectEqual(@as(usize, 3), cols.items.len);

    const segments = try cols.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len > 0);
}
