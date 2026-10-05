const std = @import("std");
const Style = @import("style.zig").Style;
const Color = @import("color.zig").Color;
const Segment = @import("segment.zig").Segment;

/// Theme for pretty-printed output.
pub const PrettyTheme = struct {
    number: Style = Style.empty.foreground(Color.cyan).bold(),
    string: Style = Style.empty.foreground(Color.green),
    boolean: Style = Style.empty.foreground(Color.fromRgb(255, 85, 85)).italic(),
    null_style: Style = Style.empty.foreground(Color.magenta).italic(),
    field_name: Style = Style.empty.foreground(Color.fromRgb(174, 129, 255)),
    punctuation: Style = Style.empty.dim(),
    type_name: Style = Style.empty.foreground(Color.yellow).bold(),
    pointer: Style = Style.empty.foreground(Color.fromRgb(128, 128, 128)),

    pub const default: PrettyTheme = .{};
    pub const minimal: PrettyTheme = .{
        .number = Style.empty,
        .string = Style.empty,
        .boolean = Style.empty,
        .null_style = Style.empty,
        .field_name = Style.empty,
        .punctuation = Style.empty,
        .type_name = Style.empty,
        .pointer = Style.empty,
    };
};

/// Options for pretty printing.
pub const PrettyOptions = struct {
    indent: usize = 2,
    max_depth: usize = 6,
    max_string_length: usize = 80,
    max_items: usize = 30,
    theme: PrettyTheme = PrettyTheme.default,
    expand_all: bool = false,
    single_line_max: usize = 60,
};

/// Pretty printer that formats Zig values into styled Segments.
pub const Pretty = struct {
    allocator: std.mem.Allocator,
    options: PrettyOptions,
    segments: std.ArrayList(Segment),
    current_depth: usize,

    pub fn init(allocator: std.mem.Allocator) Pretty {
        return initWithOptions(allocator, .{});
    }

    pub fn initWithOptions(allocator: std.mem.Allocator, options: PrettyOptions) Pretty {
        return .{
            .allocator = allocator,
            .options = options,
            .segments = .empty,
            .current_depth = 0,
        };
    }

    /// Format any value into styled segments.
    pub fn format(self: *Pretty, value: anytype) ![]Segment {
        self.segments.clearRetainingCapacity();
        try self.formatValue(value);
        return self.segments.toOwnedSlice(self.allocator);
    }

    /// Render as a renderable (matches the render(width, allocator) protocol).
    pub fn render(self: Pretty, max_width: usize, allocator: std.mem.Allocator) ![]Segment {
        _ = max_width;
        var p = Pretty.initWithOptions(allocator, self.options);
        return p.formatStored(self);
    }

    fn emit(self: *Pretty, text: []const u8, style: ?Style) !void {
        const owned = try self.allocator.dupe(u8, text);
        if (style) |s| {
            try self.segments.append(self.allocator, Segment.styled(owned, s));
        } else {
            try self.segments.append(self.allocator, Segment.plain(owned));
        }
    }

    fn emitIndent(self: *Pretty) !void {
        if (self.current_depth == 0) return;
        const indent_size = self.current_depth * self.options.indent;
        const spaces = try self.allocator.alloc(u8, indent_size);
        @memset(spaces, ' ');
        try self.segments.append(self.allocator, Segment.plain(spaces));
    }

    fn formatValue(self: *Pretty, value: anytype) !void {
        const T = @TypeOf(value);
        const info = @typeInfo(T);

        if (self.current_depth > self.options.max_depth) {
            try self.emit("...", self.options.theme.punctuation);
            return;
        }

        switch (info) {
            .void => try self.emit("void", self.options.theme.null_style),
            .bool => {
                if (value) {
                    try self.emit("true", self.options.theme.boolean);
                } else {
                    try self.emit("false", self.options.theme.boolean);
                }
            },
            .int, .comptime_int => {
                var buf: [32]u8 = undefined;
                const text = std.fmt.bufPrint(&buf, "{d}", .{value}) catch "?";
                try self.emit(text, self.options.theme.number);
            },
            .float, .comptime_float => {
                var buf: [64]u8 = undefined;
                const text = std.fmt.bufPrint(&buf, "{d:.4}", .{value}) catch "?";
                try self.emit(text, self.options.theme.number);
            },
            .null => try self.emit("null", self.options.theme.null_style),
            .optional => {
                if (value) |v| {
                    try self.formatValue(v);
                } else {
                    try self.emit("null", self.options.theme.null_style);
                }
            },
            .@"enum" => {
                try self.emit(".", self.options.theme.punctuation);
                try self.emit(@tagName(value), self.options.theme.field_name);
            },
            .error_set => {
                try self.emit("error.", self.options.theme.type_name);
                try self.emit(@errorName(value), self.options.theme.field_name);
            },
            .@"union" => |union_info| {
                if (union_info.tag_type) |_| {
                    const tag_name = @tagName(value);
                    try self.emit("{ .", self.options.theme.punctuation);
                    try self.emit(tag_name, self.options.theme.field_name);
                    try self.emit(" = ", self.options.theme.punctuation);

                    inline for (union_info.fields) |field| {
                        if (std.mem.eql(u8, field.name, tag_name)) {
                            if (field.type == void) {
                                try self.emit("{}", self.options.theme.punctuation);
                            } else {
                                try self.formatValue(@field(value, field.name));
                            }
                            break;
                        }
                    }

                    try self.emit(" }", self.options.theme.punctuation);
                } else {
                    try self.emit("@union", self.options.theme.type_name);
                }
            },
            .@"struct" => |struct_info| {
                if (struct_info.fields.len == 0) {
                    try self.emit("{}", self.options.theme.punctuation);
                    return;
                }

                try self.emit(".{", self.options.theme.punctuation);

                if (self.options.expand_all or struct_info.fields.len > 3) {
                    try self.emit("\n", null);
                    self.current_depth += 1;

                    inline for (struct_info.fields, 0..) |field, i| {
                        if (i >= self.options.max_items) {
                            try self.emitIndent();
                            var buf: [32]u8 = undefined;
                            const remaining = std.fmt.bufPrint(&buf, "... {d} more fields", .{struct_info.fields.len - i}) catch "...";
                            try self.emit(remaining, self.options.theme.punctuation);
                            try self.emit("\n", null);
                            break;
                        }
                        try self.emitIndent();
                        try self.emit(".", self.options.theme.punctuation);
                        try self.emit(field.name, self.options.theme.field_name);
                        try self.emit(" = ", self.options.theme.punctuation);
                        try self.formatValue(@field(value, field.name));
                        if (i < struct_info.fields.len - 1) {
                            try self.emit(",", self.options.theme.punctuation);
                        }
                        try self.emit("\n", null);
                    }

                    self.current_depth -= 1;
                    try self.emitIndent();
                } else {
                    try self.emit(" ", null);
                    inline for (struct_info.fields, 0..) |field, i| {
                        try self.emit(".", self.options.theme.punctuation);
                        try self.emit(field.name, self.options.theme.field_name);
                        try self.emit(" = ", self.options.theme.punctuation);
                        try self.formatValue(@field(value, field.name));
                        if (i < struct_info.fields.len - 1) {
                            try self.emit(", ", self.options.theme.punctuation);
                        }
                    }
                    try self.emit(" ", null);
                }

                try self.emit("}", self.options.theme.punctuation);
            },
            .pointer => |ptr| {
                switch (ptr.size) {
                    .slice => {
                        if (ptr.child == u8) {
                            try self.formatString(value);
                            return;
                        }
                        try self.formatSlice(value);
                    },
                    .one => {
                        if (ptr.child == anyopaque) {
                            try self.emit("*opaque", self.options.theme.pointer);
                        } else {
                            try self.emit("&", self.options.theme.pointer);
                            try self.formatValue(value.*);
                        }
                    },
                    .many, .c => {
                        try self.emit("[*]...", self.options.theme.pointer);
                    },
                }
            },
            .array => |arr| {
                if (arr.child == u8) {
                    try self.formatString(&value);
                    return;
                }
                try self.formatSlice(&value);
            },
            .type => {
                try self.emit(@typeName(value), self.options.theme.type_name);
            },
            else => {
                try self.emit(@typeName(T), self.options.theme.type_name);
            },
        }
    }

    fn formatString(self: *Pretty, str: []const u8) !void {
        try self.emit("\"", self.options.theme.string);

        if (str.len > self.options.max_string_length) {
            const truncated = str[0..self.options.max_string_length];
            try self.emit(truncated, self.options.theme.string);

            var buf: [32]u8 = undefined;
            const remaining = std.fmt.bufPrint(&buf, "... +{d}", .{str.len - self.options.max_string_length}) catch "...";
            try self.emit(remaining, self.options.theme.punctuation);
        } else {
            try self.emit(str, self.options.theme.string);
        }

        try self.emit("\"", self.options.theme.string);
    }

    fn formatSlice(self: *Pretty, slice: anytype) !void {
        if (slice.len == 0) {
            try self.emit("[]", self.options.theme.punctuation);
            return;
        }

        try self.emit("[", self.options.theme.punctuation);

        if (self.options.expand_all or slice.len > 5) {
            try self.emit("\n", null);
            self.current_depth += 1;

            for (slice, 0..) |item, i| {
                if (i >= self.options.max_items) {
                    try self.emitIndent();
                    var buf: [32]u8 = undefined;
                    const remaining = std.fmt.bufPrint(&buf, "... {d} more", .{slice.len - i}) catch "...";
                    try self.emit(remaining, self.options.theme.punctuation);
                    try self.emit("\n", null);
                    break;
                }
                try self.emitIndent();
                try self.formatValue(item);
                if (i < slice.len - 1) {
                    try self.emit(",", self.options.theme.punctuation);
                }
                try self.emit("\n", null);
            }

            self.current_depth -= 1;
            try self.emitIndent();
        } else {
            for (slice, 0..) |item, i| {
                try self.formatValue(item);
                if (i < slice.len - 1) {
                    try self.emit(", ", self.options.theme.punctuation);
                }
            }
        }

        try self.emit("]", self.options.theme.punctuation);
    }

    fn formatStored(self: *Pretty, other: Pretty) ![]Segment {
        _ = other;
        // Placeholder for stored value rendering
        try self.emit("Pretty{...}", self.options.theme.type_name);
        return self.segments.toOwnedSlice(self.allocator);
    }
};

/// Convenience function: format a value and return segments.
pub fn pretty(allocator: std.mem.Allocator, value: anytype) ![]Segment {
    var p = Pretty.init(allocator);
    return p.format(value);
}

/// Convenience function with options.
pub fn prettyWithOptions(allocator: std.mem.Allocator, value: anytype, options: PrettyOptions) ![]Segment {
    var p = Pretty.initWithOptions(allocator, options);
    return p.format(value);
}

// Tests
test "pretty print integer" {
    const allocator = std.testing.allocator;
    const segments = try pretty(allocator, @as(i32, 42));
    defer {
        for (segments) |seg| {
            allocator.free(seg.text);
        }
        allocator.free(segments);
    }

    try std.testing.expectEqual(@as(usize, 1), segments.len);
    try std.testing.expectEqualStrings("42", segments[0].text);
}

test "pretty print bool" {
    const allocator = std.testing.allocator;
    const segments = try pretty(allocator, true);
    defer {
        for (segments) |seg| {
            allocator.free(seg.text);
        }
        allocator.free(segments);
    }

    try std.testing.expectEqual(@as(usize, 1), segments.len);
    try std.testing.expectEqualStrings("true", segments[0].text);
}

test "pretty print null" {
    const allocator = std.testing.allocator;
    const val: ?i32 = null;
    const segments = try pretty(allocator, val);
    defer {
        for (segments) |seg| {
            allocator.free(seg.text);
        }
        allocator.free(segments);
    }

    try std.testing.expectEqual(@as(usize, 1), segments.len);
    try std.testing.expectEqualStrings("null", segments[0].text);
}

test "pretty print optional with value" {
    const allocator = std.testing.allocator;
    const val: ?i32 = 42;
    const segments = try pretty(allocator, val);
    defer {
        for (segments) |seg| {
            allocator.free(seg.text);
        }
        allocator.free(segments);
    }

    try std.testing.expectEqual(@as(usize, 1), segments.len);
    try std.testing.expectEqualStrings("42", segments[0].text);
}

test "pretty print string" {
    const allocator = std.testing.allocator;
    const segments = try pretty(allocator, @as([]const u8, "hello"));
    defer {
        for (segments) |seg| {
            allocator.free(seg.text);
        }
        allocator.free(segments);
    }

    // Should have: " hello "
    try std.testing.expect(segments.len >= 3);
    try std.testing.expectEqualStrings("\"", segments[0].text);
}

test "pretty print enum" {
    const TestEnum = enum { foo, bar };
    const allocator = std.testing.allocator;
    const segments = try pretty(allocator, TestEnum.foo);
    defer {
        for (segments) |seg| {
            allocator.free(seg.text);
        }
        allocator.free(segments);
    }

    try std.testing.expectEqual(@as(usize, 2), segments.len);
    try std.testing.expectEqualStrings(".", segments[0].text);
    try std.testing.expectEqualStrings("foo", segments[1].text);
}

test "pretty print struct small" {
    const allocator = std.testing.allocator;
    const val = .{ .x = @as(i32, 1), .y = @as(i32, 2) };
    const segments = try pretty(allocator, val);
    defer {
        for (segments) |seg| {
            allocator.free(seg.text);
        }
        allocator.free(segments);
    }

    try std.testing.expect(segments.len > 0);
    try std.testing.expectEqualStrings(".{", segments[0].text);
}

test "pretty print void" {
    const allocator = std.testing.allocator;
    const segments = try pretty(allocator, {});
    defer {
        for (segments) |seg| {
            allocator.free(seg.text);
        }
        allocator.free(segments);
    }

    try std.testing.expectEqual(@as(usize, 1), segments.len);
    try std.testing.expectEqualStrings("void", segments[0].text);
}

test "pretty print empty slice" {
    const allocator = std.testing.allocator;
    const empty: []const i32 = &.{};
    const segments = try pretty(allocator, empty);
    defer {
        for (segments) |seg| {
            allocator.free(seg.text);
        }
        allocator.free(segments);
    }

    try std.testing.expectEqual(@as(usize, 1), segments.len);
    try std.testing.expectEqualStrings("[]", segments[0].text);
}

test "PrettyTheme.default" {
    const theme = PrettyTheme.default;
    try std.testing.expect(theme.number.hasAttribute(.bold));
    try std.testing.expect(theme.boolean.hasAttribute(.italic));
}

test "PrettyOptions defaults" {
    const opts = PrettyOptions{};
    try std.testing.expectEqual(@as(usize, 2), opts.indent);
    try std.testing.expectEqual(@as(usize, 6), opts.max_depth);
    try std.testing.expectEqual(@as(usize, 80), opts.max_string_length);
}
