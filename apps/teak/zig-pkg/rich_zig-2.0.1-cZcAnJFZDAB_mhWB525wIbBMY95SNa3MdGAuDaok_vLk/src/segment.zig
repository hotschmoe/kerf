const std = @import("std");
const Style = @import("style.zig").Style;
const cells = @import("cells.zig");

pub const ControlType = enum {
    bell,
    carriage_return,
    home,
    clear,
    show_cursor,
    hide_cursor,
    enable_alt_screen,
    disable_alt_screen,
    cursor_up,
    cursor_down,
    cursor_forward,
    cursor_backward,
    cursor_move_to_column,
    cursor_move_to,
    erase_in_line,
    set_window_title,
};

pub const ControlCode = union(ControlType) {
    bell: void,
    carriage_return: void,
    home: void,
    clear: void,
    show_cursor: void,
    hide_cursor: void,
    enable_alt_screen: void,
    disable_alt_screen: void,
    cursor_up: u16,
    cursor_down: u16,
    cursor_forward: u16,
    cursor_backward: u16,
    cursor_move_to_column: u16,
    cursor_move_to: struct { x: u16, y: u16 },
    erase_in_line: u8,
    set_window_title: []const u8,

    pub fn toEscapeSequence(self: ControlCode, writer: anytype) !void {
        switch (self) {
            .bell => try writer.writeByte(0x07),
            .carriage_return => try writer.writeByte('\r'),
            .home => try writer.writeAll("\x1b[H"),
            .clear => try writer.writeAll("\x1b[2J"),
            .show_cursor => try writer.writeAll("\x1b[?25h"),
            .hide_cursor => try writer.writeAll("\x1b[?25l"),
            .enable_alt_screen => try writer.writeAll("\x1b[?1049h"),
            .disable_alt_screen => try writer.writeAll("\x1b[?1049l"),
            .cursor_up => |n| try writer.print("\x1b[{d}A", .{n}),
            .cursor_down => |n| try writer.print("\x1b[{d}B", .{n}),
            .cursor_forward => |n| try writer.print("\x1b[{d}C", .{n}),
            .cursor_backward => |n| try writer.print("\x1b[{d}D", .{n}),
            .cursor_move_to_column => |col| try writer.print("\x1b[{d}G", .{col}),
            .cursor_move_to => |pos| try writer.print("\x1b[{d};{d}H", .{ pos.y, pos.x }),
            .erase_in_line => |mode| try writer.print("\x1b[{d}K", .{mode}),
            .set_window_title => |title| try writer.print("\x1b]0;{s}\x07", .{title}),
        }
    }
};

pub const Segment = struct {
    text: []const u8,
    style: ?Style = null,
    control: ?ControlCode = null,

    pub fn plain(text: []const u8) Segment {
        return .{ .text = text };
    }

    pub fn styled(text: []const u8, style: Style) Segment {
        return .{ .text = text, .style = style };
    }

    pub fn styledOptional(text: []const u8, style: ?Style) Segment {
        return .{ .text = text, .style = style };
    }

    pub fn controlSegment(code: ControlCode) Segment {
        return .{ .text = "", .control = code };
    }

    pub fn line() Segment {
        return plain("\n");
    }

    pub fn space() Segment {
        return plain(" ");
    }

    pub fn cellLength(self: Segment) usize {
        if (self.control != null) return 0;
        return cells.cellLen(self.text);
    }

    pub fn isControl(self: Segment) bool {
        return self.control != null;
    }

    pub fn isEmpty(self: Segment) bool {
        return self.text.len == 0 and self.control == null;
    }

    pub fn isWhitespace(self: Segment) bool {
        if (self.control != null) return false;
        if (self.text.len == 0) return false;
        return std.mem.indexOfNone(u8, self.text, " \t\n\r") == null;
    }

    pub fn splitCells(self: Segment, pos: usize) struct { Segment, Segment } {
        if (self.control != null) {
            return .{ self, Segment{ .text = "" } };
        }

        const byte_pos = cells.cellToByteIndex(self.text, pos);

        return .{
            Segment{ .text = self.text[0..byte_pos], .style = self.style },
            Segment{ .text = self.text[byte_pos..], .style = self.style },
        };
    }

    pub fn withStyle(self: Segment, new_style: Style) Segment {
        return .{
            .text = self.text,
            .style = new_style,
            .control = self.control,
        };
    }

    pub fn withoutStyle(self: Segment) Segment {
        return .{
            .text = self.text,
            .style = null,
            .control = self.control,
        };
    }

    pub fn render(self: Segment, writer: anytype, color_system: @import("color.zig").ColorSystem) !void {
        if (self.control) |ctrl| {
            try ctrl.toEscapeSequence(writer);
            return;
        }

        if (self.style) |style| {
            if (!style.isEmpty()) {
                try style.renderAnsi(color_system, writer);
            }
        }

        try writer.writeAll(self.text);

        if (self.style) |style| {
            if (!style.isEmpty()) {
                try Style.renderReset(writer);
            }
            if (style.link != null) {
                try Style.renderHyperlinkEnd(writer);
            }
        }
    }
};

pub fn stripStyles(segments: []const Segment, allocator: std.mem.Allocator) ![]Segment {
    const result = try allocator.alloc(Segment, segments.len);
    for (segments, 0..) |seg, i| {
        result[i] = seg.withoutStyle();
    }
    return result;
}

pub fn joinText(segments: []const Segment, allocator: std.mem.Allocator) ![]u8 {
    var total_len: usize = 0;
    for (segments) |seg| {
        if (seg.control == null) {
            total_len += seg.text.len;
        }
    }

    const result = try allocator.alloc(u8, total_len);
    var pos: usize = 0;
    for (segments) |seg| {
        if (seg.control == null) {
            @memcpy(result[pos..][0..seg.text.len], seg.text);
            pos += seg.text.len;
        }
    }

    return result;
}

pub fn totalCellLength(segments: []const Segment) usize {
    var total: usize = 0;
    for (segments) |seg| {
        total += seg.cellLength();
    }
    return total;
}

pub fn divide(segments: []const Segment, cuts: []const usize, allocator: std.mem.Allocator) ![][]Segment {
    if (cuts.len == 0) {
        const result = try allocator.alloc([]Segment, 1);
        result[0] = try allocator.dupe(Segment, segments);
        return result;
    }

    var result: std.ArrayList([]Segment) = .empty;
    var current_section: std.ArrayList(Segment) = .empty;

    var seg_idx: usize = 0;
    var seg_pos: usize = 0; // Current position within current segment
    var cell_pos: usize = 0;
    var cut_idx: usize = 0;

    while (seg_idx < segments.len) {
        const seg = segments[seg_idx];

        if (seg.control != null) {
            try current_section.append(allocator, seg);
            seg_idx += 1;
            continue;
        }

        const seg_cell_len = seg.cellLength();
        const seg_start_cell = cell_pos - seg_pos;
        const seg_end_cell = seg_start_cell + seg_cell_len;

        // Check if we need to cut within this segment
        while (cut_idx < cuts.len and cuts[cut_idx] <= seg_end_cell) {
            const cut_cell = cuts[cut_idx];
            const cut_within_seg = cut_cell - seg_start_cell;

            if (cut_within_seg > seg_pos) {
                // Add portion up to the cut
                const split = seg.splitCells(cut_within_seg);
                const portion = split[0].splitCells(seg_pos);
                if (portion[1].text.len > 0) {
                    try current_section.append(allocator, portion[1]);
                }
            }

            // Save current section and start new one
            try result.append(allocator, try current_section.toOwnedSlice(allocator));
            current_section = .empty;

            seg_pos = cut_within_seg;
            cut_idx += 1;
        }

        // Add remaining portion of segment
        if (seg_pos < seg_cell_len) {
            const remaining = seg.splitCells(seg_pos)[1];
            if (remaining.text.len > 0) {
                try current_section.append(allocator, remaining);
            }
        }

        cell_pos = seg_end_cell;
        seg_pos = 0;
        seg_idx += 1;
    }

    // Add final section
    try result.append(allocator, try current_section.toOwnedSlice(allocator));

    return result.toOwnedSlice(allocator);
}

pub fn splitIntoLines(segments: []const Segment, allocator: std.mem.Allocator) ![][]const Segment {
    var lines: std.ArrayList([]const Segment) = .empty;
    var line_start: usize = 0;

    for (segments, 0..) |seg, i| {
        if (std.mem.eql(u8, seg.text, "\n")) {
            try lines.append(allocator, segments[line_start..i]);
            line_start = i + 1;
        }
    }
    if (line_start < segments.len) {
        try lines.append(allocator, segments[line_start..]);
    }
    if (lines.items.len == 0) {
        try lines.append(allocator, &[_]Segment{});
    }

    return lines.toOwnedSlice(allocator);
}

pub fn maxLineWidth(lines: []const []const Segment) usize {
    var max_width: usize = 0;
    for (lines) |line| {
        const line_width = totalCellLength(line);
        if (line_width > max_width) {
            max_width = line_width;
        }
    }
    return max_width;
}

pub fn adjustLineLength(segments: []const Segment, target_length: usize, pad_char: u8, allocator: std.mem.Allocator) ![]Segment {
    const current_len = totalCellLength(segments);

    if (current_len == target_length) {
        return try allocator.dupe(Segment, segments);
    }

    var result: std.ArrayList(Segment) = .empty;

    if (current_len < target_length) {
        // Pad
        try result.appendSlice(allocator, segments);
        const padding = target_length - current_len;
        const pad_str = try allocator.alloc(u8, padding);
        @memset(pad_str, pad_char);
        try result.append(allocator, Segment.plain(pad_str));
    } else {
        // Truncate
        var remaining = target_length;
        for (segments) |seg| {
            if (seg.control != null) {
                try result.append(allocator, seg);
                continue;
            }

            const seg_len = seg.cellLength();
            if (seg_len <= remaining) {
                try result.append(allocator, seg);
                remaining -= seg_len;
            } else if (remaining > 0) {
                const split = seg.splitCells(remaining);
                try result.append(allocator, split[0]);
                remaining = 0;
                break;
            } else {
                break;
            }
        }
    }

    return result.toOwnedSlice(allocator);
}

// Tests
test "Segment.plain" {
    const seg = Segment.plain("Hello");
    try std.testing.expectEqualStrings("Hello", seg.text);
    try std.testing.expect(seg.style == null);
    try std.testing.expect(seg.control == null);
}

test "Segment.styled" {
    const style = Style.empty.bold();
    const seg = Segment.styled("Hello", style);
    try std.testing.expectEqualStrings("Hello", seg.text);
    try std.testing.expect(seg.style != null);
    try std.testing.expect(seg.style.?.hasAttribute(.bold));
}

test "Segment.cellLength" {
    try std.testing.expectEqual(@as(usize, 5), Segment.plain("Hello").cellLength());
    try std.testing.expectEqual(@as(usize, 4), Segment.plain("\u{4E2D}\u{6587}").cellLength()); // 2 CJK chars
    try std.testing.expectEqual(@as(usize, 0), Segment.controlSegment(.bell).cellLength());
}

test "Segment.isControl" {
    try std.testing.expect(!Segment.plain("Hello").isControl());
    try std.testing.expect(Segment.controlSegment(.bell).isControl());
}

test "Segment.splitCells basic" {
    const seg = Segment.plain("Hello World");
    const split = seg.splitCells(5);
    try std.testing.expectEqualStrings("Hello", split[0].text);
    try std.testing.expectEqualStrings(" World", split[1].text);
}

test "Segment.splitCells preserves style" {
    const style = Style.empty.bold();
    const seg = Segment.styled("Hello World", style);
    const split = seg.splitCells(5);
    try std.testing.expect(split[0].style != null);
    try std.testing.expect(split[1].style != null);
    try std.testing.expect(split[0].style.?.hasAttribute(.bold));
}

test "Segment.splitCells CJK" {
    const seg = Segment.plain("\u{4E2D}\u{6587}\u{5B57}"); // 3 CJK chars, 6 cells
    const split = seg.splitCells(2); // Split after first char (2 cells)
    try std.testing.expectEqual(@as(usize, 2), cells.cellLen(split[0].text));
    try std.testing.expectEqual(@as(usize, 4), cells.cellLen(split[1].text));
}

test "Segment.isEmpty" {
    try std.testing.expect(!Segment.plain("Hello").isEmpty());
    try std.testing.expect(Segment.plain("").isEmpty());
    try std.testing.expect(!Segment.controlSegment(.bell).isEmpty());
}

test "Segment.isWhitespace" {
    try std.testing.expect(Segment.plain(" ").isWhitespace());
    try std.testing.expect(Segment.plain("  \t\n").isWhitespace());
    try std.testing.expect(!Segment.plain("Hello").isWhitespace());
    try std.testing.expect(!Segment.plain("").isWhitespace());
}

test "stripStyles" {
    const allocator = std.testing.allocator;
    const segments = [_]Segment{
        Segment.styled("Hello", Style.empty.bold()),
        Segment.plain(" "),
        Segment.styled("World", Style.empty.italic()),
    };

    const stripped = try stripStyles(&segments, allocator);
    defer allocator.free(stripped);

    try std.testing.expect(stripped[0].style == null);
    try std.testing.expect(stripped[1].style == null);
    try std.testing.expect(stripped[2].style == null);
}

test "joinText" {
    const allocator = std.testing.allocator;
    const segments = [_]Segment{
        Segment.plain("Hello"),
        Segment.plain(" "),
        Segment.plain("World"),
    };

    const text = try joinText(&segments, allocator);
    defer allocator.free(text);

    try std.testing.expectEqualStrings("Hello World", text);
}

test "totalCellLength" {
    const segments = [_]Segment{
        Segment.plain("Hello"),
        Segment.plain(" "),
        Segment.plain("World"),
    };

    try std.testing.expectEqual(@as(usize, 11), totalCellLength(&segments));
}

test "ControlCode.toEscapeSequence" {
    var buf: [64]u8 = undefined;
    var stream = std.Io.Writer.fixed(&buf);

    try ControlCode.toEscapeSequence(.{ .cursor_up = 5 }, &stream);
    try std.testing.expectEqualStrings("\x1b[5A", stream.buffered());

    stream.end = 0;
    try ControlCode.toEscapeSequence(.{ .cursor_move_to = .{ .x = 10, .y = 5 } }, &stream);
    try std.testing.expectEqualStrings("\x1b[5;10H", stream.buffered());

    stream.end = 0;
    try ControlCode.toEscapeSequence(.clear, &stream);
    try std.testing.expectEqualStrings("\x1b[2J", stream.buffered());
}

test "adjustLineLength padding" {
    const allocator = std.testing.allocator;
    const segments = [_]Segment{Segment.plain("Hi")};

    const adjusted = try adjustLineLength(&segments, 5, ' ', allocator);
    defer {
        for (adjusted) |seg| {
            if (seg.text.len > 0 and seg.style == null) {
                // Check if this is our padding string (not from original segments)
                var is_original = false;
                for (segments) |orig| {
                    if (std.mem.eql(u8, seg.text, orig.text)) {
                        is_original = true;
                        break;
                    }
                }
                if (!is_original) {
                    allocator.free(seg.text);
                }
            }
        }
        allocator.free(adjusted);
    }

    try std.testing.expectEqual(@as(usize, 5), totalCellLength(adjusted));
}

test "adjustLineLength truncating" {
    const allocator = std.testing.allocator;
    const segments = [_]Segment{Segment.plain("Hello World")};

    const adjusted = try adjustLineLength(&segments, 5, ' ', allocator);
    defer allocator.free(adjusted);

    try std.testing.expectEqual(@as(usize, 5), totalCellLength(adjusted));
    try std.testing.expectEqualStrings("Hello", adjusted[0].text);
}

test "splitIntoLines single line" {
    const allocator = std.testing.allocator;
    const segments = [_]Segment{Segment.plain("Hello")};

    const lines = try splitIntoLines(&segments, allocator);
    defer allocator.free(lines);

    try std.testing.expectEqual(@as(usize, 1), lines.len);
    try std.testing.expectEqual(@as(usize, 1), lines[0].len);
}

test "splitIntoLines multiple lines" {
    const allocator = std.testing.allocator;
    const segments = [_]Segment{
        Segment.plain("Hello"),
        Segment.line(),
        Segment.plain("World"),
    };

    const lines = try splitIntoLines(&segments, allocator);
    defer allocator.free(lines);

    try std.testing.expectEqual(@as(usize, 2), lines.len);
    try std.testing.expectEqual(@as(usize, 1), lines[0].len);
    try std.testing.expectEqual(@as(usize, 1), lines[1].len);
}

test "splitIntoLines empty" {
    const allocator = std.testing.allocator;
    const segments = [_]Segment{};

    const lines = try splitIntoLines(&segments, allocator);
    defer allocator.free(lines);

    try std.testing.expectEqual(@as(usize, 1), lines.len);
}

test "maxLineWidth" {
    const line1 = [_]Segment{Segment.plain("Hi")};
    const line2 = [_]Segment{Segment.plain("Hello")};
    const lines = [_][]const Segment{ &line1, &line2 };

    try std.testing.expectEqual(@as(usize, 5), maxLineWidth(&lines));
}
