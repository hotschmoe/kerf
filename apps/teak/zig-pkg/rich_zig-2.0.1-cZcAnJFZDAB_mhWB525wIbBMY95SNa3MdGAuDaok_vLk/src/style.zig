const std = @import("std");
const color_mod = @import("color.zig");
const Color = color_mod.Color;
const ColorSystem = color_mod.ColorSystem;

pub const StyleAttribute = enum(u4) {
    bold = 0,
    dim = 1,
    italic = 2,
    underline = 3,
    blink = 4,
    blink2 = 5,
    reverse = 6,
    conceal = 7,
    strike = 8,
    overline = 9,
    underline2 = 10,
    frame = 11,
    encircle = 12,
};

pub const Style = struct {
    color: ?Color = null,
    bgcolor: ?Color = null,
    attributes: u16 = 0, // Bitmask of which attributes are enabled
    set_attributes: u16 = 0, // Bitmask of which attributes have been explicitly set
    link: ?[]const u8 = null,

    pub const empty: Style = .{};

    pub fn bold(self: Style) Style {
        return self.setAttribute(.bold, true);
    }

    pub fn notBold(self: Style) Style {
        return self.setAttribute(.bold, false);
    }

    pub fn dim(self: Style) Style {
        return self.setAttribute(.dim, true);
    }

    pub fn notDim(self: Style) Style {
        return self.setAttribute(.dim, false);
    }

    pub fn italic(self: Style) Style {
        return self.setAttribute(.italic, true);
    }

    pub fn notItalic(self: Style) Style {
        return self.setAttribute(.italic, false);
    }

    pub fn underline(self: Style) Style {
        return self.setAttribute(.underline, true);
    }

    pub fn notUnderline(self: Style) Style {
        return self.setAttribute(.underline, false);
    }

    pub fn blink(self: Style) Style {
        return self.setAttribute(.blink, true);
    }

    pub fn notBlink(self: Style) Style {
        return self.setAttribute(.blink, false);
    }

    pub fn reverse(self: Style) Style {
        return self.setAttribute(.reverse, true);
    }

    pub fn notReverse(self: Style) Style {
        return self.setAttribute(.reverse, false);
    }

    pub fn conceal(self: Style) Style {
        return self.setAttribute(.conceal, true);
    }

    pub fn notConceal(self: Style) Style {
        return self.setAttribute(.conceal, false);
    }

    pub fn strike(self: Style) Style {
        return self.setAttribute(.strike, true);
    }

    pub fn strikethrough(self: Style) Style {
        return self.strike();
    }

    pub fn notStrike(self: Style) Style {
        return self.setAttribute(.strike, false);
    }

    pub fn overline(self: Style) Style {
        return self.setAttribute(.overline, true);
    }

    pub fn notOverline(self: Style) Style {
        return self.setAttribute(.overline, false);
    }

    pub fn underline2(self: Style) Style {
        return self.setAttribute(.underline2, true);
    }

    pub fn notUnderline2(self: Style) Style {
        return self.setAttribute(.underline2, false);
    }

    pub fn frame(self: Style) Style {
        return self.setAttribute(.frame, true);
    }

    pub fn notFrame(self: Style) Style {
        return self.setAttribute(.frame, false);
    }

    pub fn encircle(self: Style) Style {
        return self.setAttribute(.encircle, true);
    }

    pub fn notEncircle(self: Style) Style {
        return self.setAttribute(.encircle, false);
    }

    /// Alias for conceal
    pub fn hidden(self: Style) Style {
        return self.conceal();
    }

    fn setAttribute(self: Style, attr: StyleAttribute, value: bool) Style {
        var s = self;
        const bit: u16 = @as(u16, 1) << @intFromEnum(attr);
        s.set_attributes |= bit;
        if (value) {
            s.attributes |= bit;
        } else {
            s.attributes &= ~bit;
        }
        return s;
    }

    pub fn hasAttribute(self: Style, attr: StyleAttribute) bool {
        const bit: u16 = @as(u16, 1) << @intFromEnum(attr);
        return (self.attributes & bit) != 0;
    }

    pub fn foreground(self: Style, c: Color) Style {
        var s = self;
        s.color = c;
        return s;
    }

    pub fn background(self: Style, c: Color) Style {
        var s = self;
        s.bgcolor = c;
        return s;
    }

    pub fn fg(self: Style, c: Color) Style {
        return self.foreground(c);
    }

    pub fn bg(self: Style, c: Color) Style {
        return self.background(c);
    }

    pub fn hyperlink(self: Style, url: []const u8) Style {
        var s = self;
        s.link = url;
        return s;
    }

    pub fn combine(self: Style, other: Style) Style {
        return .{
            .color = other.color orelse self.color,
            .bgcolor = other.bgcolor orelse self.bgcolor,
            .attributes = (self.attributes & ~other.set_attributes) | other.attributes,
            .set_attributes = self.set_attributes | other.set_attributes,
            .link = other.link orelse self.link,
        };
    }

    pub const ParseError = error{
        UnknownColor,
        UnknownAttribute,
        InvalidHexColor,
        InvalidColorNumber,
    };

    pub fn parse(definition: []const u8) ParseError!Style {
        var style = Style{};
        var on_background = false;
        var not_modifier = false;

        var iter = std.mem.splitScalar(u8, definition, ' ');
        while (iter.next()) |token| {
            if (token.len == 0) continue;

            if (std.mem.eql(u8, token, "on")) {
                on_background = true;
                continue;
            }

            if (std.mem.eql(u8, token, "not")) {
                not_modifier = true;
                continue;
            }

            // Check for attributes
            if (parseAttribute(token)) |attr| {
                style = style.setAttribute(attr, !not_modifier);
                not_modifier = false;
                continue;
            }

            // Try to parse as color
            const parsed_color = parseColor(token) catch |err| {
                return err;
            };

            if (on_background) {
                style.bgcolor = parsed_color;
                on_background = false;
            } else {
                style.color = parsed_color;
            }
            not_modifier = false;
        }

        return style;
    }

    fn parseAttribute(token: []const u8) ?StyleAttribute {
        const attr_map = std.StaticStringMap(StyleAttribute).initComptime(.{
            .{ "bold", .bold },
            .{ "b", .bold },
            .{ "dim", .dim },
            .{ "d", .dim },
            .{ "italic", .italic },
            .{ "i", .italic },
            .{ "underline", .underline },
            .{ "u", .underline },
            .{ "blink", .blink },
            .{ "blink2", .blink2 },
            .{ "reverse", .reverse },
            .{ "r", .reverse },
            .{ "conceal", .conceal },
            .{ "strike", .strike },
            .{ "s", .strike },
            .{ "strikethrough", .strike },
            .{ "overline", .overline },
            .{ "o", .overline },
            .{ "underline2", .underline2 },
            .{ "uu", .underline2 },
            .{ "frame", .frame },
            .{ "encircle", .encircle },
        });

        return attr_map.get(token);
    }

    fn parseColor(token: []const u8) ParseError!Color {
        // Named colors
        if (color_mod.named_colors.get(token)) |c| {
            return c;
        }

        // Hex color (#RRGGBB or RRGGBB)
        if (token.len > 0 and token[0] == '#') {
            return Color.fromHex(token) catch return error.InvalidHexColor;
        }

        // rgb(r,g,b) format
        if (std.mem.startsWith(u8, token, "rgb(") and std.mem.endsWith(u8, token, ")")) {
            const inner = token[4 .. token.len - 1];
            var parts = std.mem.splitScalar(u8, inner, ',');

            const r_str = parts.next() orelse return error.InvalidHexColor;
            const g_str = parts.next() orelse return error.InvalidHexColor;
            const b_str = parts.next() orelse return error.InvalidHexColor;

            const r = std.fmt.parseInt(u8, std.mem.trim(u8, r_str, " "), 10) catch return error.InvalidHexColor;
            const g = std.fmt.parseInt(u8, std.mem.trim(u8, g_str, " "), 10) catch return error.InvalidHexColor;
            const b = std.fmt.parseInt(u8, std.mem.trim(u8, b_str, " "), 10) catch return error.InvalidHexColor;

            return Color.fromRgb(r, g, b);
        }

        // color(N) for 256 palette
        if (std.mem.startsWith(u8, token, "color(") and std.mem.endsWith(u8, token, ")")) {
            const num_str = token[6 .. token.len - 1];
            const num = std.fmt.parseInt(u8, std.mem.trim(u8, num_str, " "), 10) catch return error.InvalidColorNumber;
            return Color.from256(num);
        }

        // Just a number for 256 palette (e.g., "196")
        if (std.fmt.parseInt(u8, token, 10)) |num| {
            return Color.from256(num);
        } else |_| {}

        return error.UnknownColor;
    }

    pub fn renderAnsi(self: Style, color_system: ColorSystem, writer: anytype) !void {
        var first = true;

        try writer.writeAll("\x1b[");

        // SGR enable codes: 1=bold, 2=dim, 3=italic, 4=underline, 5=blink, 6=blink2, 7=reverse, 8=conceal, 9=strike, 53=overline, 21=underline2, 51=frame, 52=encircle
        const sgr_enable = [_]u8{ 1, 2, 3, 4, 5, 6, 7, 8, 9, 53, 21, 51, 52 };
        // SGR disable codes
        const sgr_disable = [_]u8{ 22, 22, 23, 24, 25, 25, 27, 28, 29, 55, 24, 54, 54 };

        inline for (0..13) |i| {
            const bit: u16 = @as(u16, 1) << @intCast(i);
            if ((self.set_attributes & bit) != 0) {
                if (!first) try writer.writeByte(';');
                first = false;

                const code = if ((self.attributes & bit) != 0) sgr_enable[i] else sgr_disable[i];
                try writer.print("{d}", .{code});
            }
        }

        // Foreground color
        if (self.color) |c| {
            const downgraded = c.downgrade(color_system);
            if (!first) try writer.writeByte(';');
            first = false;
            try downgraded.getAnsiCodes(true, writer);
        }

        // Background color
        if (self.bgcolor) |c| {
            const downgraded = c.downgrade(color_system);
            if (!first) try writer.writeByte(';');
            first = false;
            try downgraded.getAnsiCodes(false, writer);
        }

        // If nothing was written, default to reset
        if (first) {
            try writer.writeByte('0');
        }

        try writer.writeByte('m');

        // Hyperlink (OSC 8)
        if (self.link) |url| {
            try writer.print("\x1b]8;;{s}\x1b\\", .{url});
        }
    }

    pub fn renderReset(writer: anytype) !void {
        try writer.writeAll("\x1b[0m");
    }

    pub fn renderHyperlinkEnd(writer: anytype) !void {
        try writer.writeAll("\x1b]8;;\x1b\\");
    }

    pub fn eql(self: Style, other: Style) bool {
        if (self.attributes != other.attributes) return false;
        if (self.set_attributes != other.set_attributes) return false;

        if (!colorsEqual(self.color, other.color)) return false;
        if (!colorsEqual(self.bgcolor, other.bgcolor)) return false;
        if (!optionalStringsEqual(self.link, other.link)) return false;

        return true;
    }

    fn colorsEqual(a: ?Color, b: ?Color) bool {
        if (a == null and b == null) return true;
        if (a == null or b == null) return false;
        return a.?.eql(b.?);
    }

    fn optionalStringsEqual(a: ?[]const u8, b: ?[]const u8) bool {
        if (a == null and b == null) return true;
        if (a == null or b == null) return false;
        return std.mem.eql(u8, a.?, b.?);
    }

    pub fn isEmpty(self: Style) bool {
        return self.color == null and self.bgcolor == null and self.set_attributes == 0 and self.link == null;
    }
};

// Tests
test "Style.bold" {
    const style = Style.empty.bold();
    try std.testing.expect(style.hasAttribute(.bold));
    try std.testing.expect(!style.hasAttribute(.italic));
}

test "Style.chaining" {
    const style = Style.empty.bold().italic().underline().foreground(Color.red);
    try std.testing.expect(style.hasAttribute(.bold));
    try std.testing.expect(style.hasAttribute(.italic));
    try std.testing.expect(style.hasAttribute(.underline));
    try std.testing.expect(style.color != null);
    try std.testing.expect(style.color.?.eql(Color.red));
}

test "Style.combine" {
    const base = Style.empty.bold().foreground(Color.red);
    const overlay = Style.empty.italic().foreground(Color.blue);

    const combined = base.combine(overlay);
    try std.testing.expect(combined.hasAttribute(.bold));
    try std.testing.expect(combined.hasAttribute(.italic));
    try std.testing.expect(combined.color.?.eql(Color.blue)); // overlay wins
}

test "Style.combine preserves unset attributes" {
    const base = Style.empty.bold().foreground(Color.red);
    const overlay = Style.empty.italic(); // No color set

    const combined = base.combine(overlay);
    try std.testing.expect(combined.hasAttribute(.bold));
    try std.testing.expect(combined.hasAttribute(.italic));
    try std.testing.expect(combined.color.?.eql(Color.red)); // base preserved
}

test "Style.parse basic" {
    const style = try Style.parse("bold red");
    try std.testing.expect(style.hasAttribute(.bold));
    try std.testing.expect(style.color.?.eql(Color.red));
}

test "Style.parse with background" {
    const style = try Style.parse("bold red on white");
    try std.testing.expect(style.hasAttribute(.bold));
    try std.testing.expect(style.color.?.eql(Color.red));
    try std.testing.expect(style.bgcolor.?.eql(Color.white));
}

test "Style.parse multiple attributes" {
    const style = try Style.parse("bold italic underline green");
    try std.testing.expect(style.hasAttribute(.bold));
    try std.testing.expect(style.hasAttribute(.italic));
    try std.testing.expect(style.hasAttribute(.underline));
    try std.testing.expect(style.color.?.eql(Color.green));
}

test "Style.parse short forms" {
    const style = try Style.parse("b i u red");
    try std.testing.expect(style.hasAttribute(.bold));
    try std.testing.expect(style.hasAttribute(.italic));
    try std.testing.expect(style.hasAttribute(.underline));
}

test "Style.parse hex color" {
    const parsed = try Style.parse("bold #ff0000");
    try std.testing.expect(parsed.hasAttribute(.bold));
    try std.testing.expectEqual(color_mod.ColorType.truecolor, parsed.color.?.color_type);
    try std.testing.expectEqual(@as(u8, 255), parsed.color.?.triplet.?.r);
}

test "Style.parse rgb color" {
    const style = try Style.parse("rgb(128,64,32)");
    try std.testing.expectEqual(@as(u8, 128), style.color.?.triplet.?.r);
    try std.testing.expectEqual(@as(u8, 64), style.color.?.triplet.?.g);
    try std.testing.expectEqual(@as(u8, 32), style.color.?.triplet.?.b);
}

test "Style.parse 256 color" {
    const parsed = try Style.parse("color(196)");
    try std.testing.expectEqual(color_mod.ColorType.eight_bit, parsed.color.?.color_type);
    try std.testing.expectEqual(@as(u8, 196), parsed.color.?.number.?);
}

test "Style.parse not modifier" {
    const style = try Style.parse("not bold italic");
    try std.testing.expect(!style.hasAttribute(.bold));
    try std.testing.expect(style.hasAttribute(.italic));
}

test "Style.renderAnsi bold red" {
    var buf: [128]u8 = undefined;
    var stream = std.Io.Writer.fixed(&buf);

    const style = Style.empty.bold().foreground(Color.red);
    try style.renderAnsi(.truecolor, &stream);

    try std.testing.expectEqualStrings("\x1b[1;31m", stream.buffered());
}

test "Style.renderAnsi truecolor" {
    var buf: [128]u8 = undefined;
    var stream = std.Io.Writer.fixed(&buf);

    const style = Style.empty.foreground(Color.fromRgb(255, 128, 64));
    try style.renderAnsi(.truecolor, &stream);

    try std.testing.expectEqualStrings("\x1b[38;2;255;128;64m", stream.buffered());
}

test "Style.renderAnsi with background" {
    var buf: [128]u8 = undefined;
    var stream = std.Io.Writer.fixed(&buf);

    const style = Style.empty.foreground(Color.red).background(Color.white);
    try style.renderAnsi(.truecolor, &stream);

    try std.testing.expectEqualStrings("\x1b[31;47m", stream.buffered());
}

test "Style.renderAnsi downgrade" {
    var buf: [128]u8 = undefined;
    var stream = std.Io.Writer.fixed(&buf);

    const style = Style.empty.foreground(Color.fromRgb(255, 0, 0));
    try style.renderAnsi(.standard, &stream);

    // Should be standard red or bright_red
    const written = stream.buffered();
    try std.testing.expect(std.mem.eql(u8, written, "\x1b[31m") or std.mem.eql(u8, written, "\x1b[91m"));
}

test "Style.isEmpty" {
    try std.testing.expect(Style.empty.isEmpty());
    try std.testing.expect(!Style.empty.bold().isEmpty());
    try std.testing.expect(!Style.empty.foreground(Color.red).isEmpty());
}

test "Style.eql" {
    const s1 = Style.empty.bold().foreground(Color.red);
    const s2 = Style.empty.bold().foreground(Color.red);
    const s3 = Style.empty.bold().foreground(Color.blue);

    try std.testing.expect(s1.eql(s2));
    try std.testing.expect(!s1.eql(s3));
}

test "Style.underline2" {
    const style = Style.empty.underline2();
    try std.testing.expect(style.hasAttribute(.underline2));
    try std.testing.expect(!style.hasAttribute(.underline));
}

test "Style.frame" {
    const style = Style.empty.frame();
    try std.testing.expect(style.hasAttribute(.frame));
}

test "Style.encircle" {
    const style = Style.empty.encircle();
    try std.testing.expect(style.hasAttribute(.encircle));
}

test "Style.hidden alias" {
    const style = Style.empty.hidden();
    try std.testing.expect(style.hasAttribute(.conceal));
}

test "Style.parse underline2" {
    const style = try Style.parse("uu red");
    try std.testing.expect(style.hasAttribute(.underline2));
    try std.testing.expect(style.color.?.eql(Color.red));
}

test "Style.renderAnsi underline2" {
    var buf: [128]u8 = undefined;
    var stream = std.Io.Writer.fixed(&buf);
    const style = Style.empty.underline2();
    try style.renderAnsi(.truecolor, &stream);
    try std.testing.expectEqualStrings("\x1b[21m", stream.buffered());
}
