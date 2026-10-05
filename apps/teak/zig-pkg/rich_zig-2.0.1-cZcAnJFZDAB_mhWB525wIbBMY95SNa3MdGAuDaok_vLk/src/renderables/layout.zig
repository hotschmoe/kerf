const std = @import("std");
const Segment = @import("../segment.zig").Segment;
const segment_mod = @import("../segment.zig");
const Style = @import("../style.zig").Style;

pub const SizeConstraint = union(enum) {
    ratio: u8,
    fixed: usize,
    min: usize,
    auto,
};

pub const SplitDirection = enum {
    horizontal,
    vertical,
};

pub const SplitterConfig = struct {
    visible: bool = false,
    char: []const u8 = "\u{2502}",
    horizontal_char: []const u8 = "\u{2500}",
    style: ?Style = null,
};

pub const SplitContent = union(enum) {
    segments: []const Segment,
    split: *Split,
};

pub const SplitChild = struct {
    content: SplitContent,
    constraint: SizeConstraint = .auto,
    name: ?[]const u8 = null,
};

pub const Split = struct {
    direction: SplitDirection,
    children: std.ArrayList(SplitChild),
    allocator: std.mem.Allocator,
    splitter: SplitterConfig = .{},
    name_index: std.StringHashMap(usize),

    pub fn horizontal(allocator: std.mem.Allocator) Split {
        return .{
            .direction = .horizontal,
            .children = std.ArrayList(SplitChild).empty,
            .allocator = allocator,
            .name_index = std.StringHashMap(usize).init(allocator),
        };
    }

    pub fn vertical(allocator: std.mem.Allocator) Split {
        return .{
            .direction = .vertical,
            .children = std.ArrayList(SplitChild).empty,
            .allocator = allocator,
            .name_index = std.StringHashMap(usize).init(allocator),
        };
    }

    pub fn deinit(self: *Split) void {
        self.children.deinit(self.allocator);
        self.name_index.deinit();
    }

    pub fn withSplitter(self: Split) Split {
        var s = self;
        s.splitter.visible = true;
        return s;
    }

    pub fn withSplitterChar(self: Split, char: []const u8) Split {
        var s = self;
        s.splitter.char = char;
        s.splitter.visible = true;
        return s;
    }

    pub fn withSplitterStyle(self: Split, style: Style) Split {
        var s = self;
        s.splitter.style = style;
        s.splitter.visible = true;
        return s;
    }

    pub fn add(self: *Split, content: []const Segment) *Split {
        self.children.append(self.allocator, .{
            .content = .{ .segments = content },
            .constraint = .auto,
        }) catch {};
        return self;
    }

    pub fn addNamed(self: *Split, name: []const u8, content: []const Segment) *Split {
        const index = self.children.items.len;
        self.children.append(self.allocator, .{
            .content = .{ .segments = content },
            .constraint = .auto,
            .name = name,
        }) catch {};
        self.name_index.put(name, index) catch {};
        return self;
    }

    pub fn addSplit(self: *Split, nested: *Split) *Split {
        self.children.append(self.allocator, .{
            .content = .{ .split = nested },
            .constraint = .auto,
        }) catch {};
        return self;
    }

    pub fn addSplitNamed(self: *Split, name: []const u8, nested: *Split) *Split {
        const index = self.children.items.len;
        self.children.append(self.allocator, .{
            .content = .{ .split = nested },
            .constraint = .auto,
            .name = name,
        }) catch {};
        self.name_index.put(name, index) catch {};
        return self;
    }

    pub fn addWithRatio(self: *Split, content: []const Segment, ratio: u8) *Split {
        self.children.append(self.allocator, .{
            .content = .{ .segments = content },
            .constraint = .{ .ratio = ratio },
        }) catch {};
        return self;
    }

    pub fn addWithMinSize(self: *Split, content: []const Segment, min_size: usize) *Split {
        self.children.append(self.allocator, .{
            .content = .{ .segments = content },
            .constraint = .{ .min = min_size },
        }) catch {};
        return self;
    }

    pub fn addWithFixedSize(self: *Split, content: []const Segment, size: usize) *Split {
        self.children.append(self.allocator, .{
            .content = .{ .segments = content },
            .constraint = .{ .fixed = size },
        }) catch {};
        return self;
    }

    pub fn getRegionIndex(self: Split, name: []const u8) ?usize {
        return self.name_index.get(name);
    }

    pub fn getRegion(self: Split, name: []const u8) ?*SplitChild {
        const index = self.name_index.get(name) orelse return null;
        if (index < self.children.items.len) {
            return &self.children.items[index];
        }
        return null;
    }

    pub fn updateRegion(self: *Split, name: []const u8, content: []const Segment) bool {
        const index = self.name_index.get(name) orelse return false;
        if (index < self.children.items.len) {
            self.children.items[index].content = .{ .segments = content };
            return true;
        }
        return false;
    }

    pub fn updateRegionIndex(self: *Split, index: usize, content: []const Segment) void {
        if (index < self.children.items.len) {
            self.children.items[index].content = .{ .segments = content };
        }
    }

    pub fn render(self: Split, max_width: usize, allocator: std.mem.Allocator) std.mem.Allocator.Error![]Segment {
        var segments: std.ArrayList(Segment) = .empty;

        if (self.children.items.len == 0) {
            return segments.toOwnedSlice(allocator);
        }

        switch (self.direction) {
            .vertical => try self.renderVertical(&segments, max_width, allocator),
            .horizontal => try self.renderHorizontal(&segments, max_width, allocator),
        }

        return segments.toOwnedSlice(allocator);
    }

    fn renderVertical(self: Split, segments: *std.ArrayList(Segment), max_width: usize, allocator: std.mem.Allocator) std.mem.Allocator.Error!void {
        for (self.children.items, 0..) |child, i| {
            switch (child.content) {
                .segments => |segs| {
                    for (segs) |seg| {
                        try segments.append(allocator, seg);
                    }
                },
                .split => |nested| {
                    const nested_segs = try nested.render(max_width, allocator);
                    defer allocator.free(nested_segs);
                    for (nested_segs) |seg| {
                        try segments.append(allocator, seg);
                    }
                },
            }
            if (i < self.children.items.len - 1) {
                try segments.append(allocator, Segment.line());
                if (self.splitter.visible) {
                    try segments.append(allocator, Segment.styledOptional(self.splitter.horizontal_char, self.splitter.style));
                    try segments.append(allocator, Segment.line());
                }
            }
        }
    }

    fn renderHorizontal(self: Split, segments: *std.ArrayList(Segment), max_width: usize, allocator: std.mem.Allocator) std.mem.Allocator.Error!void {
        if (self.children.items.len == 0) return;

        const splitter_width: usize = if (self.splitter.visible) 1 else 0;
        const total_splitter_width = if (self.children.items.len > 1) splitter_width * (self.children.items.len - 1) else 0;
        const content_width = if (max_width > total_splitter_width) max_width - total_splitter_width else 0;

        const widths = try self.calculateWidths(content_width, allocator);
        defer allocator.free(widths);

        const child_lines = try allocator.alloc([][]const Segment, self.children.items.len);
        defer allocator.free(child_lines);

        const rendered_nested = try allocator.alloc(?[]const Segment, self.children.items.len);
        defer allocator.free(rendered_nested);

        var max_lines: usize = 0;

        for (self.children.items, 0..) |child, i| {
            rendered_nested[i] = null;
            const content_segs = switch (child.content) {
                .segments => |segs| segs,
                .split => |nested| blk: {
                    const nested_segs = try nested.render(widths[i], allocator);
                    rendered_nested[i] = nested_segs;
                    break :blk nested_segs;
                },
            };
            child_lines[i] = try segment_mod.splitIntoLines(content_segs, allocator);
            if (child_lines[i].len > max_lines) {
                max_lines = child_lines[i].len;
            }
        }
        defer {
            for (child_lines) |lines| {
                allocator.free(lines);
            }
            for (rendered_nested) |maybe_segs| {
                if (maybe_segs) |segs| {
                    allocator.free(segs);
                }
            }
        }

        for (0..max_lines) |line_idx| {
            for (self.children.items, 0..) |_, col| {
                if (col > 0 and self.splitter.visible) {
                    try segments.append(allocator, Segment.styledOptional(self.splitter.char, self.splitter.style));
                }

                const col_width = widths[col];
                const line = if (line_idx < child_lines[col].len) child_lines[col][line_idx] else &[_]Segment{};

                const adjusted = try segment_mod.adjustLineLength(line, col_width, ' ', allocator);
                for (adjusted) |seg| {
                    try segments.append(allocator, seg);
                }
                allocator.free(adjusted);
            }
            try segments.append(allocator, Segment.line());
        }
    }

    fn calculateWidths(self: Split, max_width: usize, allocator: std.mem.Allocator) ![]usize {
        var widths = try allocator.alloc(usize, self.children.items.len);

        var total_ratio: usize = 0;
        var fixed_width: usize = 0;
        var auto_count: usize = 0;

        for (self.children.items, 0..) |child, i| {
            switch (child.constraint) {
                .ratio => |r| total_ratio += r,
                .fixed => |f| {
                    widths[i] = f;
                    fixed_width += f;
                },
                .min => |m| {
                    widths[i] = m;
                    fixed_width += m;
                },
                .auto => auto_count += 1,
            }
        }

        const remaining = if (max_width > fixed_width) max_width - fixed_width else 0;
        var ratio_used: usize = 0;

        if (total_ratio > 0) {
            for (self.children.items, 0..) |child, i| {
                if (child.constraint == .ratio) {
                    widths[i] = (remaining * child.constraint.ratio) / total_ratio;
                    ratio_used += widths[i];
                }
            }
        }

        if (auto_count > 0) {
            const auto_width = (remaining - ratio_used) / auto_count;
            for (self.children.items, 0..) |child, i| {
                if (child.constraint == .auto) {
                    widths[i] = auto_width;
                }
            }
        }

        return widths;
    }
};

test "Split.horizontal" {
    const allocator = std.testing.allocator;
    var split = Split.horizontal(allocator);
    defer split.deinit();

    try std.testing.expectEqual(SplitDirection.horizontal, split.direction);
}

test "Split.vertical" {
    const allocator = std.testing.allocator;
    var split = Split.vertical(allocator);
    defer split.deinit();

    try std.testing.expectEqual(SplitDirection.vertical, split.direction);
}

test "Split.add" {
    const allocator = std.testing.allocator;
    var split = Split.horizontal(allocator);
    defer split.deinit();

    const segs1 = [_]Segment{Segment.plain("Left")};
    const segs2 = [_]Segment{Segment.plain("Right")};

    _ = split.add(&segs1).add(&segs2);

    try std.testing.expectEqual(@as(usize, 2), split.children.items.len);
}

test "Split.addWithRatio" {
    const allocator = std.testing.allocator;
    var split = Split.horizontal(allocator);
    defer split.deinit();

    const segs = [_]Segment{Segment.plain("Content")};
    _ = split.addWithRatio(&segs, 2);

    try std.testing.expectEqual(SizeConstraint{ .ratio = 2 }, split.children.items[0].constraint);
}

test "Split.addWithMinSize" {
    const allocator = std.testing.allocator;
    var split = Split.horizontal(allocator);
    defer split.deinit();

    const segs = [_]Segment{Segment.plain("Content")};
    _ = split.addWithMinSize(&segs, 10);

    try std.testing.expectEqual(SizeConstraint{ .min = 10 }, split.children.items[0].constraint);
}

test "Split.addWithFixedSize" {
    const allocator = std.testing.allocator;
    var split = Split.horizontal(allocator);
    defer split.deinit();

    const segs = [_]Segment{Segment.plain("Content")};
    _ = split.addWithFixedSize(&segs, 20);

    try std.testing.expectEqual(SizeConstraint{ .fixed = 20 }, split.children.items[0].constraint);
}

test "Split.render vertical" {
    const allocator = std.testing.allocator;
    var split = Split.vertical(allocator);
    defer split.deinit();

    const segs1 = [_]Segment{Segment.plain("Top")};
    const segs2 = [_]Segment{Segment.plain("Bottom")};
    _ = split.add(&segs1).add(&segs2);

    const segments = try split.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len > 0);

    var found_top = false;
    var found_bottom = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "Top")) found_top = true;
        if (std.mem.eql(u8, seg.text, "Bottom")) found_bottom = true;
    }
    try std.testing.expect(found_top);
    try std.testing.expect(found_bottom);
}

test "Split.render empty" {
    const allocator = std.testing.allocator;
    var split = Split.horizontal(allocator);
    defer split.deinit();

    const segments = try split.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expectEqual(@as(usize, 0), segments.len);
}

test "Split.addNamed" {
    const allocator = std.testing.allocator;
    var split = Split.horizontal(allocator);
    defer split.deinit();

    const segs1 = [_]Segment{Segment.plain("Header")};
    const segs2 = [_]Segment{Segment.plain("Body")};
    _ = split.addNamed("header", &segs1).addNamed("body", &segs2);

    try std.testing.expectEqual(@as(usize, 2), split.children.items.len);
    try std.testing.expectEqualStrings("header", split.children.items[0].name.?);
    try std.testing.expectEqualStrings("body", split.children.items[1].name.?);
}

test "Split.getRegionIndex" {
    const allocator = std.testing.allocator;
    var split = Split.horizontal(allocator);
    defer split.deinit();

    const segs1 = [_]Segment{Segment.plain("A")};
    const segs2 = [_]Segment{Segment.plain("B")};
    _ = split.addNamed("first", &segs1).addNamed("second", &segs2);

    try std.testing.expectEqual(@as(?usize, 0), split.getRegionIndex("first"));
    try std.testing.expectEqual(@as(?usize, 1), split.getRegionIndex("second"));
    try std.testing.expectEqual(@as(?usize, null), split.getRegionIndex("nonexistent"));
}

test "Split.getRegion" {
    const allocator = std.testing.allocator;
    var split = Split.horizontal(allocator);
    defer split.deinit();

    const segs = [_]Segment{Segment.plain("Content")};
    _ = split.addNamed("main", &segs);

    const region = split.getRegion("main");
    try std.testing.expect(region != null);
    try std.testing.expectEqualStrings("main", region.?.name.?);
}

test "Split.updateRegion" {
    const allocator = std.testing.allocator;
    var split = Split.horizontal(allocator);
    defer split.deinit();

    const segs1 = [_]Segment{Segment.plain("Original")};
    _ = split.addNamed("content", &segs1);

    const segs2 = [_]Segment{Segment.plain("Updated")};
    const updated = split.updateRegion("content", &segs2);

    try std.testing.expect(updated);
    try std.testing.expectEqualStrings("Updated", split.children.items[0].content.segments[0].text);
}

test "Split.updateRegionIndex" {
    const allocator = std.testing.allocator;
    var split = Split.horizontal(allocator);
    defer split.deinit();

    const segs1 = [_]Segment{Segment.plain("Original")};
    _ = split.add(&segs1);

    const segs2 = [_]Segment{Segment.plain("New")};
    split.updateRegionIndex(0, &segs2);

    try std.testing.expectEqualStrings("New", split.children.items[0].content.segments[0].text);
}

test "Split.withSplitter" {
    const allocator = std.testing.allocator;
    var split = Split.horizontal(allocator).withSplitter();
    defer split.deinit();

    try std.testing.expect(split.splitter.visible);
}

test "Split.withSplitterChar" {
    const allocator = std.testing.allocator;
    var split = Split.horizontal(allocator).withSplitterChar("|");
    defer split.deinit();

    try std.testing.expect(split.splitter.visible);
    try std.testing.expectEqualStrings("|", split.splitter.char);
}

test "Split.withSplitterStyle" {
    const allocator = std.testing.allocator;
    var split = Split.horizontal(allocator).withSplitterStyle(Style.empty.bold());
    defer split.deinit();

    try std.testing.expect(split.splitter.visible);
    try std.testing.expect(split.splitter.style != null);
}

test "Split.render with splitter" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    var split = Split.horizontal(arena.allocator()).withSplitterChar("|");
    defer split.deinit();

    const segs1 = [_]Segment{Segment.plain("Left")};
    const segs2 = [_]Segment{Segment.plain("Right")};
    _ = split.add(&segs1).add(&segs2);

    const segments = try split.render(80, arena.allocator());

    var found_splitter = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "|")) {
            found_splitter = true;
            break;
        }
    }
    try std.testing.expect(found_splitter);
}

test "Split.addSplit nested" {
    const allocator = std.testing.allocator;
    var outer = Split.vertical(allocator);
    defer outer.deinit();

    var inner = Split.horizontal(allocator);
    defer inner.deinit();

    const segs1 = [_]Segment{Segment.plain("Inner1")};
    const segs2 = [_]Segment{Segment.plain("Inner2")};
    _ = inner.add(&segs1).add(&segs2);

    const segs3 = [_]Segment{Segment.plain("Outer")};
    _ = outer.add(&segs3).addSplit(&inner);

    try std.testing.expectEqual(@as(usize, 2), outer.children.items.len);
    try std.testing.expect(outer.children.items[1].content == .split);
}

test "Split.render nested" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    var outer = Split.vertical(arena.allocator());
    defer outer.deinit();

    var inner = Split.horizontal(arena.allocator());
    defer inner.deinit();

    const inner1 = [_]Segment{Segment.plain("A")};
    const inner2 = [_]Segment{Segment.plain("B")};
    _ = inner.add(&inner1).add(&inner2);

    const outer1 = [_]Segment{Segment.plain("Header")};
    _ = outer.add(&outer1).addSplit(&inner);

    const segments = try outer.render(40, arena.allocator());
    const text = try segment_mod.joinText(segments, arena.allocator());

    try std.testing.expect(std.mem.indexOf(u8, text, "Header") != null);
    try std.testing.expect(std.mem.indexOf(u8, text, "A") != null);
    try std.testing.expect(std.mem.indexOf(u8, text, "B") != null);
}
