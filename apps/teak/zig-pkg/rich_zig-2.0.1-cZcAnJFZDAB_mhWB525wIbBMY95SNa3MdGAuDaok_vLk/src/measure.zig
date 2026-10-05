const std = @import("std");

/// Measurement represents the minimum and maximum widths a renderable
/// can occupy. Layout containers (Table, Columns, Layout) use this to
/// make smarter sizing decisions.
///
/// Renderables that implement `measure(max_width, allocator) Measurement`
/// alongside `render(max_width, allocator) ![]Segment` allow the layout
/// system to auto-size their containers optimally.
pub const Measurement = struct {
    minimum: usize,
    maximum: usize,

    pub fn init(minimum: usize, maximum: usize) Measurement {
        return .{
            .minimum = minimum,
            .maximum = @max(minimum, maximum),
        };
    }

    /// A zero measurement (for empty content).
    pub const zero: Measurement = .{ .minimum = 0, .maximum = 0 };

    /// Clamp measurement to a maximum width constraint.
    pub fn clamp(self: Measurement, max_width: usize) Measurement {
        return .{
            .minimum = @min(self.minimum, max_width),
            .maximum = @min(self.maximum, max_width),
        };
    }

    /// Create a measurement spanning multiple items by taking the
    /// union (widest min, widest max).
    pub fn union_(a: Measurement, b: Measurement) Measurement {
        return .{
            .minimum = @max(a.minimum, b.minimum),
            .maximum = @max(a.maximum, b.maximum),
        };
    }

    /// Create a measurement spanning multiple items by taking the
    /// intersection (narrowest bounds that satisfies both).
    pub fn intersection(a: Measurement, b: Measurement) Measurement {
        return .{
            .minimum = @max(a.minimum, b.minimum),
            .maximum = @min(a.maximum, b.maximum),
        };
    }

    /// Pad the measurement by a given amount on each side.
    pub fn pad(self: Measurement, amount: usize) Measurement {
        const total = amount * 2;
        return .{
            .minimum = self.minimum + total,
            .maximum = self.maximum + total,
        };
    }

    /// Measure text content by computing its cell width.
    pub fn fromText(text: []const u8) Measurement {
        const cells = @import("cells.zig");
        // For single-line text, min == max == cell width
        // For multi-line, min is widest word, max is widest line
        var max_line: usize = 0;
        var max_word: usize = 0;
        var current_line: usize = 0;
        var current_word: usize = 0;

        var i: usize = 0;
        while (i < text.len) {
            const byte = text[i];
            if (byte == '\n') {
                if (current_line > max_line) max_line = current_line;
                if (current_word > max_word) max_word = current_word;
                current_line = 0;
                current_word = 0;
                i += 1;
                continue;
            }
            if (byte == ' ' or byte == '\t') {
                if (current_word > max_word) max_word = current_word;
                current_word = 0;
                current_line += 1;
                i += 1;
                continue;
            }

            const cp_len = std.unicode.utf8ByteSequenceLength(byte) catch {
                i += 1;
                current_line += 1;
                current_word += 1;
                continue;
            };
            if (i + cp_len > text.len) break;

            const cp = std.unicode.utf8Decode(text[i..][0..cp_len]) catch {
                i += 1;
                current_line += 1;
                current_word += 1;
                continue;
            };

            const w = cells.getCharacterCellSize(cp);
            current_line += w;
            current_word += w;
            i += cp_len;
        }

        if (current_line > max_line) max_line = current_line;
        if (current_word > max_word) max_word = current_word;

        return .{
            .minimum = max_word,
            .maximum = max_line,
        };
    }

    /// Measure a renderable if it supports the measure protocol.
    /// Falls back to rendering and measuring the output if not.
    pub fn measureRenderable(renderable: anytype, max_width: usize, allocator: std.mem.Allocator) !Measurement {
        if (@hasDecl(@TypeOf(renderable), "measure")) {
            return renderable.measure(max_width, allocator);
        }
        const segment_mod = @import("segment.zig");
        const segments = try renderable.render(max_width, allocator);
        defer allocator.free(segments);
        const lines = try segment_mod.splitIntoLines(segments, allocator);
        defer allocator.free(lines);
        const width = segment_mod.maxLineWidth(lines);
        return Measurement.init(width, width);
    }
};

// Tests
test "Measurement.init" {
    const m = Measurement.init(5, 20);
    try std.testing.expectEqual(@as(usize, 5), m.minimum);
    try std.testing.expectEqual(@as(usize, 20), m.maximum);
}

test "Measurement.init ensures min <= max" {
    const m = Measurement.init(20, 5);
    try std.testing.expectEqual(@as(usize, 20), m.minimum);
    try std.testing.expectEqual(@as(usize, 20), m.maximum);
}

test "Measurement.clamp" {
    const m = Measurement.init(5, 20).clamp(10);
    try std.testing.expectEqual(@as(usize, 5), m.minimum);
    try std.testing.expectEqual(@as(usize, 10), m.maximum);
}

test "Measurement.clamp below minimum" {
    const m = Measurement.init(5, 20).clamp(3);
    try std.testing.expectEqual(@as(usize, 3), m.minimum);
    try std.testing.expectEqual(@as(usize, 3), m.maximum);
}

test "Measurement.union_" {
    const a = Measurement.init(3, 10);
    const b = Measurement.init(5, 8);
    const u = Measurement.union_(a, b);
    try std.testing.expectEqual(@as(usize, 5), u.minimum);
    try std.testing.expectEqual(@as(usize, 10), u.maximum);
}

test "Measurement.intersection" {
    const a = Measurement.init(3, 10);
    const b = Measurement.init(5, 8);
    const i = Measurement.intersection(a, b);
    try std.testing.expectEqual(@as(usize, 5), i.minimum);
    try std.testing.expectEqual(@as(usize, 8), i.maximum);
}

test "Measurement.pad" {
    const m = Measurement.init(5, 20).pad(3);
    try std.testing.expectEqual(@as(usize, 11), m.minimum);
    try std.testing.expectEqual(@as(usize, 26), m.maximum);
}

test "Measurement.zero" {
    try std.testing.expectEqual(@as(usize, 0), Measurement.zero.minimum);
    try std.testing.expectEqual(@as(usize, 0), Measurement.zero.maximum);
}

test "Measurement.fromText single line" {
    const m = Measurement.fromText("Hello World");
    try std.testing.expectEqual(@as(usize, 5), m.minimum); // "World" is longest word
    try std.testing.expectEqual(@as(usize, 11), m.maximum);
}

test "Measurement.fromText multi line" {
    const m = Measurement.fromText("Hi\nHello World\nBye");
    try std.testing.expectEqual(@as(usize, 5), m.minimum); // "Hello" or "World"
    try std.testing.expectEqual(@as(usize, 11), m.maximum); // "Hello World"
}

test "Measurement.fromText single word" {
    const m = Measurement.fromText("Hello");
    try std.testing.expectEqual(@as(usize, 5), m.minimum);
    try std.testing.expectEqual(@as(usize, 5), m.maximum);
}

test "Measurement.fromText empty" {
    const m = Measurement.fromText("");
    try std.testing.expectEqual(@as(usize, 0), m.minimum);
    try std.testing.expectEqual(@as(usize, 0), m.maximum);
}
