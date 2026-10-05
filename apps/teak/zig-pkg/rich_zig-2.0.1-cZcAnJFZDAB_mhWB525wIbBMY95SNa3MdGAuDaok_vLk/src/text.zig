const std = @import("std");
const Style = @import("style.zig").Style;
const Segment = @import("segment.zig").Segment;
const markup = @import("markup.zig");
const cells = @import("cells.zig");
const Theme = @import("theme.zig").Theme;

pub const Span = struct {
    start: usize,
    end: usize,
    style: Style,
};

pub const Text = struct {
    plain: []const u8,
    spans: []const Span,
    style: Style,
    allocator: std.mem.Allocator,
    owns_plain: bool = false,
    owns_spans: bool = false,

    pub fn init(allocator: std.mem.Allocator) Text {
        return .{
            .plain = "",
            .spans = &[_]Span{},
            .style = Style.empty,
            .allocator = allocator,
        };
    }

    pub fn fromPlain(allocator: std.mem.Allocator, text: []const u8) Text {
        return .{
            .plain = text,
            .spans = &[_]Span{},
            .style = Style.empty,
            .allocator = allocator,
        };
    }

    pub fn fromPlainOwned(allocator: std.mem.Allocator, text: []const u8) !Text {
        const owned = try allocator.dupe(u8, text);
        return .{
            .plain = owned,
            .spans = &[_]Span{},
            .style = Style.empty,
            .allocator = allocator,
            .owns_plain = true,
        };
    }

    pub fn fromMarkup(allocator: std.mem.Allocator, text: []const u8) !Text {
        const tokens = try markup.parseMarkup(text, allocator);
        defer allocator.free(tokens);

        var plain_buf: std.ArrayList(u8) = .empty;
        defer plain_buf.deinit(allocator);

        var spans_buf: std.ArrayList(Span) = .empty;
        defer spans_buf.deinit(allocator);

        var style_stack: std.ArrayList(Style) = .empty;
        defer style_stack.deinit(allocator);
        try style_stack.append(allocator, Style.empty);

        for (tokens) |token| {
            switch (token) {
                .text => |txt| {
                    const start = plain_buf.items.len;
                    try plain_buf.appendSlice(allocator, txt);
                    const end = plain_buf.items.len;

                    const current_style = style_stack.getLast();
                    if (!current_style.isEmpty()) {
                        try spans_buf.append(allocator, .{
                            .start = start,
                            .end = end,
                            .style = current_style,
                        });
                    }
                },
                .open_tag => |tag| {
                    const current_style = style_stack.getLast();
                    const new_style = Style.parse(tag.name) catch continue;
                    try style_stack.append(allocator, current_style.combine(new_style));
                },
                .close_tag => {
                    if (style_stack.items.len > 1) {
                        _ = style_stack.pop();
                    }
                },
            }
        }

        return .{
            .plain = try plain_buf.toOwnedSlice(allocator),
            .spans = try spans_buf.toOwnedSlice(allocator),
            .style = Style.empty,
            .allocator = allocator,
            .owns_plain = true,
            .owns_spans = true,
        };
    }

    pub fn fromMarkupWithTheme(allocator: std.mem.Allocator, text: []const u8, theme: Theme) !Text {
        const tokens = try markup.parseMarkup(text, allocator);
        defer allocator.free(tokens);

        var plain_buf: std.ArrayList(u8) = .empty;
        defer plain_buf.deinit(allocator);

        var spans_buf: std.ArrayList(Span) = .empty;
        defer spans_buf.deinit(allocator);

        var style_stack: std.ArrayList(Style) = .empty;
        defer style_stack.deinit(allocator);
        try style_stack.append(allocator, Style.empty);

        for (tokens) |token| {
            switch (token) {
                .text => |txt| {
                    const start = plain_buf.items.len;
                    try plain_buf.appendSlice(allocator, txt);
                    const end = plain_buf.items.len;

                    const current_style = style_stack.getLast();
                    if (!current_style.isEmpty()) {
                        try spans_buf.append(allocator, .{
                            .start = start,
                            .end = end,
                            .style = current_style,
                        });
                    }
                },
                .open_tag => |tag| {
                    const current_style = style_stack.getLast();
                    // Try theme lookup first, then standard style parsing
                    const new_style = theme.get(tag.name) orelse
                        (Style.parse(tag.name) catch continue);
                    try style_stack.append(allocator, current_style.combine(new_style));
                },
                .close_tag => {
                    if (style_stack.items.len > 1) {
                        _ = style_stack.pop();
                    }
                },
            }
        }

        return .{
            .plain = try plain_buf.toOwnedSlice(allocator),
            .spans = try spans_buf.toOwnedSlice(allocator),
            .style = Style.empty,
            .allocator = allocator,
            .owns_plain = true,
            .owns_spans = true,
        };
    }

    pub fn deinit(self: *Text) void {
        if (self.owns_spans and self.spans.len > 0) {
            self.allocator.free(self.spans);
        }
        if (self.owns_plain and self.plain.len > 0) {
            self.allocator.free(@constCast(self.plain));
        }
        self.* = undefined;
    }

    pub fn cellLength(self: Text) usize {
        return cells.cellLen(self.plain);
    }

    pub fn len(self: Text) usize {
        return self.plain.len;
    }

    pub fn isEmpty(self: Text) bool {
        return self.plain.len == 0;
    }

    pub fn render(self: Text, allocator: std.mem.Allocator) ![]Segment {
        if (self.spans.len == 0) {
            const result = try allocator.alloc(Segment, 1);
            result[0] = Segment.styledOptional(self.plain, if (self.style.isEmpty()) null else self.style);
            return result;
        }

        var segments: std.ArrayList(Segment) = .empty;
        var pos: usize = 0;

        for (self.spans) |span| {
            // Text before span (unstyled or with base style)
            if (span.start > pos) {
                try segments.append(allocator, Segment.styledOptional(
                    self.plain[pos..span.start],
                    if (self.style.isEmpty()) null else self.style,
                ));
            }
            // Span text with combined style
            const combined = self.style.combine(span.style);
            try segments.append(allocator, Segment.styled(
                self.plain[span.start..span.end],
                combined,
            ));
            pos = span.end;
        }

        // Remaining text after last span
        if (pos < self.plain.len) {
            try segments.append(allocator, Segment.styledOptional(
                self.plain[pos..],
                if (self.style.isEmpty()) null else self.style,
            ));
        }

        return segments.toOwnedSlice(allocator);
    }

    pub fn withStyle(self: Text, new_style: Style) Text {
        return .{
            .plain = self.plain,
            .spans = self.spans,
            .style = new_style,
            .allocator = self.allocator,
            .owns_plain = false, // Don't take ownership when creating derivative
            .owns_spans = false,
        };
    }

    pub fn append(self: *Text, other: Text) !void {
        const offset = self.plain.len;

        // Create new plain text
        const new_plain = try self.allocator.alloc(u8, self.plain.len + other.plain.len);
        @memcpy(new_plain[0..self.plain.len], self.plain);
        @memcpy(new_plain[self.plain.len..], other.plain);

        // Create new spans with offset adjustment
        const new_spans = try self.allocator.alloc(Span, self.spans.len + other.spans.len);
        @memcpy(new_spans[0..self.spans.len], self.spans);
        for (other.spans, 0..) |span, i| {
            new_spans[self.spans.len + i] = .{
                .start = span.start + offset,
                .end = span.end + offset,
                .style = span.style,
            };
        }

        // Free old data if owned
        if (self.owns_plain and self.plain.len > 0) {
            self.allocator.free(@constCast(self.plain));
        }
        if (self.owns_spans and self.spans.len > 0) {
            self.allocator.free(self.spans);
        }

        self.plain = new_plain;
        self.spans = new_spans;
        self.owns_plain = true;
        self.owns_spans = true;
    }

    pub fn slice(self: Text, start: usize, end: usize) Text {
        const actual_end = @min(end, self.plain.len);
        const actual_start = @min(start, actual_end);

        // Filter and adjust spans
        var relevant_spans: std.ArrayList(Span) = .empty;
        for (self.spans) |span| {
            if (span.end > actual_start and span.start < actual_end) {
                relevant_spans.append(self.allocator, .{
                    .start = if (span.start < actual_start) 0 else span.start - actual_start,
                    .end = @min(span.end, actual_end) - actual_start,
                    .style = span.style,
                }) catch {};
            }
        }

        // Capture count before toOwnedSlice (which sets items.len to 0)
        const has_spans = relevant_spans.items.len > 0;

        return .{
            .plain = self.plain[actual_start..actual_end],
            .spans = relevant_spans.toOwnedSlice(self.allocator) catch &[_]Span{},
            .style = self.style,
            .allocator = self.allocator,
            .owns_plain = false,
            .owns_spans = has_spans,
        };
    }

    pub fn highlight(self: *Text, start: usize, end: usize, style: Style) !void {
        const actual_end = @min(end, self.plain.len);
        const actual_start = @min(start, actual_end);

        var new_spans = try self.allocator.alloc(Span, self.spans.len + 1);
        @memcpy(new_spans[0..self.spans.len], self.spans);
        new_spans[self.spans.len] = .{
            .start = actual_start,
            .end = actual_end,
            .style = style,
        };

        if (self.owns_spans and self.spans.len > 0) {
            self.allocator.free(self.spans);
        }

        self.spans = new_spans;
        self.owns_spans = true;
    }

    /// Create a deep copy of this Text with owned memory
    pub fn clone(self: Text) !Text {
        const new_plain = try self.allocator.dupe(u8, self.plain);
        errdefer self.allocator.free(new_plain);

        const new_spans = try self.allocator.dupe(Span, self.spans);

        return .{
            .plain = new_plain,
            .spans = new_spans,
            .style = self.style,
            .allocator = self.allocator,
            .owns_plain = true,
            .owns_spans = true,
        };
    }

    /// Truncate text to fit within max_cells, appending ellipsis if truncated
    pub fn truncate(self: Text, max_cells: usize, ellipsis: []const u8) !Text {
        const current_len = self.cellLength();
        if (current_len <= max_cells) {
            return self.clone();
        }

        const ellipsis_width = cells.cellLen(ellipsis);
        if (max_cells <= ellipsis_width) {
            // Not enough room for ellipsis, just truncate
            const cutoff = cells.cellToByteIndex(self.plain, max_cells);
            return self.truncateAtByte(cutoff, "");
        }

        const target_cells = max_cells - ellipsis_width;
        const cutoff = cells.cellToByteIndex(self.plain, target_cells);
        return self.truncateAtByte(cutoff, ellipsis);
    }

    fn truncateAtByte(self: Text, cutoff: usize, ellipsis: []const u8) !Text {
        // Build new plain text
        const new_plain = try self.allocator.alloc(u8, cutoff + ellipsis.len);
        errdefer self.allocator.free(new_plain);
        @memcpy(new_plain[0..cutoff], self.plain[0..cutoff]);
        @memcpy(new_plain[cutoff..], ellipsis);

        // Filter and clip spans
        var span_count: usize = 0;
        for (self.spans) |span| {
            if (span.start < cutoff) span_count += 1;
        }

        const new_spans = try self.allocator.alloc(Span, span_count);
        errdefer self.allocator.free(new_spans);

        var i: usize = 0;
        for (self.spans) |span| {
            if (span.start < cutoff) {
                new_spans[i] = .{
                    .start = span.start,
                    .end = @min(span.end, cutoff),
                    .style = span.style,
                };
                i += 1;
            }
        }

        return .{
            .plain = new_plain,
            .spans = new_spans,
            .style = self.style,
            .allocator = self.allocator,
            .owns_plain = true,
            .owns_spans = true,
        };
    }

    /// Pad text on the right to reach exact width
    pub fn alignLeft(self: Text, width: usize) !Text {
        const current_len = self.cellLength();
        if (current_len >= width) {
            return self.clone();
        }

        const padding = width - current_len;
        const new_plain = try self.allocator.alloc(u8, self.plain.len + padding);
        errdefer self.allocator.free(new_plain);
        @memcpy(new_plain[0..self.plain.len], self.plain);
        @memset(new_plain[self.plain.len..], ' ');

        const new_spans = try self.allocator.dupe(Span, self.spans);

        return .{
            .plain = new_plain,
            .spans = new_spans,
            .style = self.style,
            .allocator = self.allocator,
            .owns_plain = true,
            .owns_spans = true,
        };
    }

    /// Pad text on the left to reach exact width, shifting span positions
    pub fn alignRight(self: Text, width: usize) !Text {
        const current_len = self.cellLength();
        if (current_len >= width) {
            return self.clone();
        }

        const padding = width - current_len;
        const new_plain = try self.allocator.alloc(u8, self.plain.len + padding);
        errdefer self.allocator.free(new_plain);
        @memset(new_plain[0..padding], ' ');
        @memcpy(new_plain[padding..], self.plain);

        // Shift all span positions
        const new_spans = try self.allocator.alloc(Span, self.spans.len);
        errdefer self.allocator.free(new_spans);
        for (self.spans, 0..) |span, i| {
            new_spans[i] = .{
                .start = span.start + padding,
                .end = span.end + padding,
                .style = span.style,
            };
        }

        return .{
            .plain = new_plain,
            .spans = new_spans,
            .style = self.style,
            .allocator = self.allocator,
            .owns_plain = true,
            .owns_spans = true,
        };
    }

    /// Pad text on both sides to center within width
    pub fn alignCenter(self: Text, width: usize) !Text {
        const current_len = self.cellLength();
        if (current_len >= width) {
            return self.clone();
        }

        const total_padding = width - current_len;
        const left_padding = total_padding / 2;

        const new_plain = try self.allocator.alloc(u8, self.plain.len + total_padding);
        errdefer self.allocator.free(new_plain);
        @memset(new_plain[0..left_padding], ' ');
        @memcpy(new_plain[left_padding .. left_padding + self.plain.len], self.plain);
        @memset(new_plain[left_padding + self.plain.len ..], ' ');

        // Shift all span positions by left padding
        const new_spans = try self.allocator.alloc(Span, self.spans.len);
        errdefer self.allocator.free(new_spans);
        for (self.spans, 0..) |span, i| {
            new_spans[i] = .{
                .start = span.start + left_padding,
                .end = span.end + left_padding,
                .style = span.style,
            };
        }

        return .{
            .plain = new_plain,
            .spans = new_spans,
            .style = self.style,
            .allocator = self.allocator,
            .owns_plain = true,
            .owns_spans = true,
        };
    }

    /// Split text into multiple lines at word boundaries
    pub fn wrap(self: Text, max_width: usize) ![]Text {
        if (max_width == 0) {
            const result = try self.allocator.alloc(Text, 0);
            return result;
        }

        var lines: std.ArrayList(Text) = .empty;
        errdefer {
            for (lines.items) |*line| line.deinit();
            lines.deinit(self.allocator);
        }

        var line_start: usize = 0; // byte position
        var line_start_cell: usize = 0; // cell position for span adjustment
        var current_cell: usize = 0;
        var last_space_byte: ?usize = null;
        var last_space_cell: ?usize = null;
        var i: usize = 0;

        while (i < self.plain.len) {
            const byte = self.plain[i];
            const cp_len = std.unicode.utf8ByteSequenceLength(byte) catch {
                i += 1;
                continue;
            };

            if (i + cp_len > self.plain.len) break;

            const cp = std.unicode.utf8Decode(self.plain[i..][0..cp_len]) catch {
                i += 1;
                continue;
            };

            const char_width = cells.getCharacterCellSize(cp);

            // Track space positions for word breaking
            if (cp == ' ') {
                last_space_byte = i;
                last_space_cell = current_cell;
            }

            current_cell += char_width;
            i += cp_len;

            // Check if we need to wrap
            if (current_cell - line_start_cell > max_width) {
                var break_byte: usize = undefined;
                var break_cell: usize = undefined;

                if (last_space_byte != null and last_space_byte.? > line_start) {
                    // Break at last space
                    break_byte = last_space_byte.?;
                    break_cell = last_space_cell.?;
                } else {
                    // No space found, break at current position (mid-word)
                    break_byte = i - cp_len;
                    break_cell = current_cell - char_width;
                }

                // Create line from line_start to break_byte
                const line = try self.createLineSlice(line_start, break_byte);
                try lines.append(self.allocator, line);

                // Skip the space if we broke at one
                if (last_space_byte != null and last_space_byte.? > line_start and break_byte == last_space_byte.?) {
                    line_start = break_byte + 1;
                    line_start_cell = break_cell + 1;
                } else {
                    line_start = break_byte;
                    line_start_cell = break_cell;
                }

                last_space_byte = null;
                last_space_cell = null;
            }
        }

        // Add remaining text
        if (line_start < self.plain.len) {
            const line = try self.createLineSlice(line_start, self.plain.len);
            try lines.append(self.allocator, line);
        }

        // Handle empty input
        if (lines.items.len == 0) {
            const empty_line = try self.clone();
            try lines.append(self.allocator, empty_line);
        }

        return lines.toOwnedSlice(self.allocator);
    }

    fn createLineSlice(self: Text, start_byte: usize, end_byte: usize) !Text {
        const line_text = self.plain[start_byte..end_byte];
        const new_plain = try self.allocator.dupe(u8, line_text);
        errdefer self.allocator.free(new_plain);

        // Filter and adjust spans for this line
        var span_count: usize = 0;
        for (self.spans) |span| {
            if (span.end > start_byte and span.start < end_byte) span_count += 1;
        }

        const new_spans = try self.allocator.alloc(Span, span_count);
        errdefer self.allocator.free(new_spans);

        var idx: usize = 0;
        for (self.spans) |span| {
            if (span.end > start_byte and span.start < end_byte) {
                const adj_start = if (span.start < start_byte) 0 else span.start - start_byte;
                const adj_end = @min(span.end, end_byte) - start_byte;
                new_spans[idx] = .{
                    .start = adj_start,
                    .end = adj_end,
                    .style = span.style,
                };
                idx += 1;
            }
        }

        return .{
            .plain = new_plain,
            .spans = new_spans,
            .style = self.style,
            .allocator = self.allocator,
            .owns_plain = true,
            .owns_spans = true,
        };
    }

    /// Justify text to fill width by distributing extra space between words
    pub fn justify(self: Text, width: usize) !Text {
        const current_len = self.cellLength();
        if (current_len >= width) {
            return self.clone();
        }

        // Find word boundaries (spaces)
        var word_count: usize = 0;
        var in_word = false;
        for (self.plain) |c| {
            if (c == ' ') {
                in_word = false;
            } else if (!in_word) {
                word_count += 1;
                in_word = true;
            }
        }

        // Single word or no words: left-align
        if (word_count <= 1) {
            return self.alignLeft(width);
        }

        const gaps = word_count - 1;
        const extra_spaces = width - current_len;
        const base_extra = extra_spaces / gaps;
        const remainder = extra_spaces % gaps;

        // Build new text with distributed spaces
        var new_plain: std.ArrayList(u8) = .empty;
        errdefer new_plain.deinit(self.allocator);

        // Track position mapping for span adjustment: old_byte -> new_byte
        var position_map: std.ArrayList(usize) = .empty;
        defer position_map.deinit(self.allocator);

        var gap_index: usize = 0;
        in_word = false;
        var i: usize = 0;

        while (i < self.plain.len) {
            const c = self.plain[i];
            try position_map.append(self.allocator, new_plain.items.len);

            if (c == ' ') {
                try new_plain.append(self.allocator, ' ');
                // Add extra spaces after this space (before next word)
                if (in_word) {
                    const extra = base_extra + (if (gap_index < remainder) @as(usize, 1) else 0);
                    for (0..extra) |_| {
                        try new_plain.append(self.allocator, ' ');
                    }
                    gap_index += 1;
                }
                in_word = false;
            } else {
                try new_plain.append(self.allocator, c);
                in_word = true;
            }
            i += 1;
        }

        // Add final mapping entry for end position
        try position_map.append(self.allocator, new_plain.items.len);

        // Adjust spans using position map
        const new_spans = try self.allocator.alloc(Span, self.spans.len);
        errdefer self.allocator.free(new_spans);

        for (self.spans, 0..) |span, idx| {
            const new_start = if (span.start < position_map.items.len) position_map.items[span.start] else new_plain.items.len;
            const new_end = if (span.end < position_map.items.len) position_map.items[span.end] else new_plain.items.len;
            new_spans[idx] = .{
                .start = new_start,
                .end = new_end,
                .style = span.style,
            };
        }

        return .{
            .plain = try new_plain.toOwnedSlice(self.allocator),
            .spans = new_spans,
            .style = self.style,
            .allocator = self.allocator,
            .owns_plain = true,
            .owns_spans = true,
        };
    }

    /// Find all occurrences of pattern and highlight them with the given style
    pub fn highlightPattern(self: Text, pattern: []const u8, style: Style) !Text {
        if (pattern.len == 0) {
            return self.clone();
        }

        // Find all matches
        var matches: std.ArrayList(usize) = .empty;
        defer matches.deinit(self.allocator);

        var pos: usize = 0;
        while (pos + pattern.len <= self.plain.len) {
            if (std.mem.indexOf(u8, self.plain[pos..], pattern)) |idx| {
                try matches.append(self.allocator, pos + idx);
                pos = pos + idx + 1; // Allow overlapping matches
            } else {
                break;
            }
        }

        if (matches.items.len == 0) {
            return self.clone();
        }

        // Create new spans: existing spans + new highlight spans
        const new_plain = try self.allocator.dupe(u8, self.plain);
        errdefer self.allocator.free(new_plain);

        const new_spans = try self.allocator.alloc(Span, self.spans.len + matches.items.len);
        errdefer self.allocator.free(new_spans);

        // Copy existing spans
        @memcpy(new_spans[0..self.spans.len], self.spans);

        // Add highlight spans for each match
        for (matches.items, 0..) |match_pos, i| {
            new_spans[self.spans.len + i] = .{
                .start = match_pos,
                .end = match_pos + pattern.len,
                .style = style,
            };
        }

        return .{
            .plain = new_plain,
            .spans = new_spans,
            .style = self.style,
            .allocator = self.allocator,
            .owns_plain = true,
            .owns_spans = true,
        };
    }
};

// Tests
test "Text.init" {
    const allocator = std.testing.allocator;
    var text = Text.init(allocator);
    defer text.deinit();

    try std.testing.expectEqual(@as(usize, 0), text.len());
    try std.testing.expect(text.isEmpty());
}

test "Text.fromPlain" {
    const allocator = std.testing.allocator;
    const text = Text.fromPlain(allocator, "Hello");

    try std.testing.expectEqualStrings("Hello", text.plain);
    try std.testing.expectEqual(@as(usize, 5), text.len());
    try std.testing.expectEqual(@as(usize, 5), text.cellLength());
}

test "Text.fromMarkup basic" {
    const allocator = std.testing.allocator;
    var text = try Text.fromMarkup(allocator, "[bold]Hello[/]");
    defer text.deinit();

    try std.testing.expectEqualStrings("Hello", text.plain);
    try std.testing.expectEqual(@as(usize, 1), text.spans.len);
    try std.testing.expect(text.spans[0].style.hasAttribute(.bold));
}

test "Text.fromMarkup nested" {
    const allocator = std.testing.allocator;
    var text = try Text.fromMarkup(allocator, "[bold][red]Hello[/][/]");
    defer text.deinit();

    try std.testing.expectEqualStrings("Hello", text.plain);
    try std.testing.expectEqual(@as(usize, 1), text.spans.len);
    try std.testing.expect(text.spans[0].style.hasAttribute(.bold));
}

test "Text.fromMarkup mixed" {
    const allocator = std.testing.allocator;
    var text = try Text.fromMarkup(allocator, "Hello [bold]World[/]!");
    defer text.deinit();

    try std.testing.expectEqualStrings("Hello World!", text.plain);
    try std.testing.expectEqual(@as(usize, 1), text.spans.len);
    try std.testing.expectEqual(@as(usize, 6), text.spans[0].start);
    try std.testing.expectEqual(@as(usize, 11), text.spans[0].end);
}

test "Text.render" {
    const allocator = std.testing.allocator;
    var text = try Text.fromMarkup(allocator, "[bold]Hello[/] World");
    defer text.deinit();

    const segments = try text.render(allocator);
    defer allocator.free(segments);

    try std.testing.expectEqual(@as(usize, 2), segments.len);
    try std.testing.expectEqualStrings("Hello", segments[0].text);
    try std.testing.expect(segments[0].style.?.hasAttribute(.bold));
    try std.testing.expectEqualStrings(" World", segments[1].text);
}

test "Text.cellLength with CJK" {
    const allocator = std.testing.allocator;
    const text = Text.fromPlain(allocator, "\u{4E2D}\u{6587}");

    try std.testing.expectEqual(@as(usize, 4), text.cellLength()); // 2 CJK = 4 cells
}

test "Text.append" {
    const allocator = std.testing.allocator;
    var text1 = try Text.fromMarkup(allocator, "[bold]Hello[/]");
    defer text1.deinit();

    const text2 = Text.fromPlain(allocator, " World");

    try text1.append(text2);

    try std.testing.expectEqualStrings("Hello World", text1.plain);
    try std.testing.expectEqual(@as(usize, 1), text1.spans.len);
    try std.testing.expectEqual(@as(usize, 0), text1.spans[0].start);
    try std.testing.expectEqual(@as(usize, 5), text1.spans[0].end);
}

test "Text.withStyle" {
    const allocator = std.testing.allocator;
    const base = Text.fromPlain(allocator, "Hello");
    const styled = base.withStyle(Style.empty.bold());

    const segments = try styled.render(allocator);
    defer allocator.free(segments);

    try std.testing.expectEqual(@as(usize, 1), segments.len);
    try std.testing.expect(segments[0].style.?.hasAttribute(.bold));
}

test "Text.highlight" {
    const allocator = std.testing.allocator;
    var text = Text.fromPlain(allocator, "Hello World");

    try text.highlight(6, 11, Style.empty.bold());
    defer {
        if (text.owns_spans) allocator.free(text.spans);
    }

    try std.testing.expectEqual(@as(usize, 1), text.spans.len);
    try std.testing.expectEqual(@as(usize, 6), text.spans[0].start);
    try std.testing.expectEqual(@as(usize, 11), text.spans[0].end);
}

test "Text.clone" {
    const allocator = std.testing.allocator;
    var original = try Text.fromMarkup(allocator, "[bold]Hello[/]");
    defer original.deinit();

    var cloned = try original.clone();
    defer cloned.deinit();

    try std.testing.expectEqualStrings("Hello", cloned.plain);
    try std.testing.expectEqual(@as(usize, 1), cloned.spans.len);
    try std.testing.expect(cloned.spans[0].style.hasAttribute(.bold));
}

test "Text.truncate short text unchanged" {
    const allocator = std.testing.allocator;
    const text = Text.fromPlain(allocator, "Hello");

    var truncated = try text.truncate(10, "...");
    defer truncated.deinit();

    try std.testing.expectEqualStrings("Hello", truncated.plain);
}

test "Text.truncate with ellipsis" {
    const allocator = std.testing.allocator;
    const text = Text.fromPlain(allocator, "Hello World");

    var truncated = try text.truncate(8, "...");
    defer truncated.deinit();

    // 8 - 3 (ellipsis) = 5 cells, "Hello" + "..."
    try std.testing.expectEqualStrings("Hello...", truncated.plain);
}

test "Text.truncate clips spans" {
    const allocator = std.testing.allocator;
    var text = try Text.fromMarkup(allocator, "[bold]Hello World[/]");
    defer text.deinit();

    var truncated = try text.truncate(8, "...");
    defer truncated.deinit();

    try std.testing.expectEqualStrings("Hello...", truncated.plain);
    try std.testing.expectEqual(@as(usize, 1), truncated.spans.len);
    try std.testing.expectEqual(@as(usize, 0), truncated.spans[0].start);
    try std.testing.expectEqual(@as(usize, 5), truncated.spans[0].end);
}

test "Text.alignLeft padding" {
    const allocator = std.testing.allocator;
    const text = Text.fromPlain(allocator, "Hi");

    var aligned = try text.alignLeft(5);
    defer aligned.deinit();

    try std.testing.expectEqualStrings("Hi   ", aligned.plain);
}

test "Text.alignRight padding with span shift" {
    const allocator = std.testing.allocator;
    var text = try Text.fromMarkup(allocator, "[bold]Hi[/]");
    defer text.deinit();

    var aligned = try text.alignRight(5);
    defer aligned.deinit();

    try std.testing.expectEqualStrings("   Hi", aligned.plain);
    try std.testing.expectEqual(@as(usize, 3), aligned.spans[0].start);
    try std.testing.expectEqual(@as(usize, 5), aligned.spans[0].end);
}

test "Text.alignCenter padding" {
    const allocator = std.testing.allocator;
    const text = Text.fromPlain(allocator, "Hi");

    var aligned = try text.alignCenter(6);
    defer aligned.deinit();

    try std.testing.expectEqualStrings("  Hi  ", aligned.plain);
}

test "Text.alignCenter with span shift" {
    const allocator = std.testing.allocator;
    var text = try Text.fromMarkup(allocator, "[bold]Hi[/]");
    defer text.deinit();

    var aligned = try text.alignCenter(6);
    defer aligned.deinit();

    try std.testing.expectEqualStrings("  Hi  ", aligned.plain);
    try std.testing.expectEqual(@as(usize, 2), aligned.spans[0].start);
    try std.testing.expectEqual(@as(usize, 4), aligned.spans[0].end);
}

test "Text.wrap single line no wrap" {
    const allocator = std.testing.allocator;
    const text = Text.fromPlain(allocator, "Hello");

    const lines = try text.wrap(10);
    defer {
        for (lines) |*line| line.deinit();
        allocator.free(lines);
    }

    try std.testing.expectEqual(@as(usize, 1), lines.len);
    try std.testing.expectEqualStrings("Hello", lines[0].plain);
}

test "Text.wrap multi-word" {
    const allocator = std.testing.allocator;
    const text = Text.fromPlain(allocator, "Hello World");

    const lines = try text.wrap(6);
    defer {
        for (lines) |*line| line.deinit();
        allocator.free(lines);
    }

    try std.testing.expectEqual(@as(usize, 2), lines.len);
    try std.testing.expectEqualStrings("Hello", lines[0].plain);
    try std.testing.expectEqualStrings("World", lines[1].plain);
}

test "Text.wrap preserves spans" {
    const allocator = std.testing.allocator;
    var text = try Text.fromMarkup(allocator, "[bold]Hello World[/]");
    defer text.deinit();

    const lines = try text.wrap(6);
    defer {
        for (lines) |*line| line.deinit();
        allocator.free(lines);
    }

    try std.testing.expectEqual(@as(usize, 2), lines.len);
    try std.testing.expect(lines[0].spans.len > 0);
    try std.testing.expect(lines[1].spans.len > 0);
}

test "Text.justify single word" {
    const allocator = std.testing.allocator;
    const text = Text.fromPlain(allocator, "Hello");

    var justified = try text.justify(10);
    defer justified.deinit();

    // Single word should be left-aligned
    try std.testing.expectEqualStrings("Hello     ", justified.plain);
}

test "Text.justify two words" {
    const allocator = std.testing.allocator;
    const text = Text.fromPlain(allocator, "Hi World");

    var justified = try text.justify(12);
    defer justified.deinit();

    // 8 cells original, need 4 extra spaces between words
    try std.testing.expectEqualStrings("Hi     World", justified.plain);
}

test "Text.justify multiple words" {
    const allocator = std.testing.allocator;
    const text = Text.fromPlain(allocator, "A B C");

    var justified = try text.justify(9);
    defer justified.deinit();

    // 5 cells original (A B C), need 4 extra spaces distributed between 2 gaps
    // = 2 extra per gap, result: "A   B   C"
    try std.testing.expectEqualStrings("A   B   C", justified.plain);
}

test "Text.highlightPattern single match" {
    const allocator = std.testing.allocator;
    const text = Text.fromPlain(allocator, "Hello World");

    var highlighted = try text.highlightPattern("World", Style.empty.bold());
    defer highlighted.deinit();

    try std.testing.expectEqualStrings("Hello World", highlighted.plain);
    try std.testing.expectEqual(@as(usize, 1), highlighted.spans.len);
    try std.testing.expectEqual(@as(usize, 6), highlighted.spans[0].start);
    try std.testing.expectEqual(@as(usize, 11), highlighted.spans[0].end);
}

test "Text.highlightPattern multiple matches" {
    const allocator = std.testing.allocator;
    const text = Text.fromPlain(allocator, "test test test");

    var highlighted = try text.highlightPattern("test", Style.empty.bold());
    defer highlighted.deinit();

    try std.testing.expectEqual(@as(usize, 3), highlighted.spans.len);
}

test "Text.highlightPattern no match" {
    const allocator = std.testing.allocator;
    const text = Text.fromPlain(allocator, "Hello World");

    var highlighted = try text.highlightPattern("xyz", Style.empty.bold());
    defer highlighted.deinit();

    try std.testing.expectEqualStrings("Hello World", highlighted.plain);
    try std.testing.expectEqual(@as(usize, 0), highlighted.spans.len);
}

test "Text.highlightPattern preserves existing spans" {
    const allocator = std.testing.allocator;
    var text = try Text.fromMarkup(allocator, "[italic]Hello World[/]");
    defer text.deinit();

    var highlighted = try text.highlightPattern("World", Style.empty.bold());
    defer highlighted.deinit();

    try std.testing.expectEqual(@as(usize, 2), highlighted.spans.len);
    // First span is the original italic
    try std.testing.expect(highlighted.spans[0].style.hasAttribute(.italic));
    // Second span is the new bold highlight
    try std.testing.expect(highlighted.spans[1].style.hasAttribute(.bold));
}
