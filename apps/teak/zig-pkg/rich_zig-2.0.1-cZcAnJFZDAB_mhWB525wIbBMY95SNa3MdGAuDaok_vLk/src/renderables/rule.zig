const std = @import("std");
const Segment = @import("../segment.zig").Segment;
const Style = @import("../style.zig").Style;
const cells = @import("../cells.zig");

pub const Alignment = enum { left, center, right };

pub const Rule = struct {
    title: ?[]const u8 = null,
    characters: []const u8 = "\u{2500}",
    style: Style = Style.empty,
    title_style: Style = Style.empty,
    alignment: Alignment = .center,
    end: []const u8 = "",

    pub fn init() Rule {
        return .{};
    }

    pub fn withTitle(self: Rule, title: []const u8) Rule {
        var r = self;
        r.title = title;
        return r;
    }

    pub fn withStyle(self: Rule, style: Style) Rule {
        var r = self;
        r.style = style;
        return r;
    }

    pub fn withTitleStyle(self: Rule, style: Style) Rule {
        var r = self;
        r.title_style = style;
        return r;
    }

    pub fn withCharacters(self: Rule, chars: []const u8) Rule {
        var r = self;
        r.characters = chars;
        return r;
    }

    pub fn alignLeft(self: Rule) Rule {
        var r = self;
        r.alignment = .left;
        return r;
    }

    pub fn alignRight(self: Rule) Rule {
        var r = self;
        r.alignment = .right;
        return r;
    }

    pub fn alignCenter(self: Rule) Rule {
        var r = self;
        r.alignment = .center;
        return r;
    }

    pub fn measure(self: Rule, max_width: usize, _: std.mem.Allocator) !@import("../measure.zig").Measurement {
        _ = self;
        const Meas = @import("../measure.zig").Measurement;
        return Meas.init(1, max_width);
    }

    pub fn render(self: Rule, width: usize, allocator: std.mem.Allocator) ![]Segment {
        var segments: std.ArrayList(Segment) = .empty;

        if (self.title) |title| {
            const title_len = cells.cellLen(title);
            const rule_len = if (width > title_len + 2) width - title_len - 2 else 0;

            const left_len: usize = switch (self.alignment) {
                .left => 1,
                .center => rule_len / 2,
                .right => if (rule_len > 1) rule_len - 1 else 0,
            };
            const right_len = if (rule_len > left_len) rule_len - left_len else 0;

            try self.renderChars(&segments, allocator, left_len);
            try segments.append(allocator, Segment.plain(" "));
            try segments.append(allocator, Segment.styled(title, self.title_style));
            try segments.append(allocator, Segment.plain(" "));
            try self.renderChars(&segments, allocator, right_len);
        } else {
            try self.renderChars(&segments, allocator, width);
        }

        if (self.end.len > 0) {
            try segments.append(allocator, Segment.plain(self.end));
        }

        try segments.append(allocator, Segment.line());

        return segments.toOwnedSlice(allocator);
    }

    fn renderChars(self: Rule, segments: *std.ArrayList(Segment), allocator: std.mem.Allocator, count: usize) !void {
        const char_len = cells.cellLen(self.characters);
        if (char_len == 0) return;

        const repeat_count = count / char_len;
        for (0..repeat_count) |_| {
            try segments.append(allocator, Segment.styled(self.characters, self.style));
        }
    }
};

// Tests
test "Rule.init" {
    const rule_obj = Rule.init();
    try std.testing.expect(rule_obj.title == null);
    try std.testing.expectEqual(Alignment.center, rule_obj.alignment);
}

test "Rule.withTitle" {
    const rule_obj = Rule.init().withTitle("Section");
    try std.testing.expectEqualStrings("Section", rule_obj.title.?);
}

test "Rule.render no title" {
    const allocator = std.testing.allocator;
    const rule_obj = Rule.init();

    const segments = try rule_obj.render(40, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len > 0);
}

test "Rule.render with title" {
    const allocator = std.testing.allocator;
    const rule_obj = Rule.init().withTitle("Test");

    const segments = try rule_obj.render(40, allocator);
    defer allocator.free(segments);

    var found_title = false;
    for (segments) |seg| {
        if (std.mem.indexOf(u8, seg.text, "Test") != null) {
            found_title = true;
            break;
        }
    }
    try std.testing.expect(found_title);
}

test "Rule.alignment" {
    const left = Rule.init().alignLeft();
    try std.testing.expectEqual(Alignment.left, left.alignment);

    const right = Rule.init().alignRight();
    try std.testing.expectEqual(Alignment.right, right.alignment);
}
