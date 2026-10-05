const std = @import("std");

pub const ColorType = enum {
    default,
    standard, // 16 colors (0-15)
    eight_bit, // 256 colors (0-255)
    truecolor, // RGB 24-bit
};

pub const ColorSystem = enum(u8) {
    standard = 1,
    eight_bit = 2,
    truecolor = 3,

    pub fn supports(self: ColorSystem, other: ColorSystem) bool {
        return @intFromEnum(self) >= @intFromEnum(other);
    }
};

pub const ColorTriplet = struct {
    r: u8,
    g: u8,
    b: u8,

    pub fn hex(self: ColorTriplet) [7]u8 {
        var buf: [7]u8 = undefined;
        _ = std.fmt.bufPrint(&buf, "#{x:0>2}{x:0>2}{x:0>2}", .{ self.r, self.g, self.b }) catch unreachable;
        return buf;
    }

    pub fn blend(c1: ColorTriplet, c2: ColorTriplet, t: f32) ColorTriplet {
        const clamped_t = @max(0.0, @min(1.0, t));
        return .{
            .r = @intFromFloat(@as(f32, @floatFromInt(c1.r)) + (@as(f32, @floatFromInt(c2.r)) - @as(f32, @floatFromInt(c1.r))) * clamped_t),
            .g = @intFromFloat(@as(f32, @floatFromInt(c1.g)) + (@as(f32, @floatFromInt(c2.g)) - @as(f32, @floatFromInt(c1.g))) * clamped_t),
            .b = @intFromFloat(@as(f32, @floatFromInt(c1.b)) + (@as(f32, @floatFromInt(c2.b)) - @as(f32, @floatFromInt(c1.b))) * clamped_t),
        };
    }

    pub fn eql(self: ColorTriplet, other: ColorTriplet) bool {
        return self.r == other.r and self.g == other.g and self.b == other.b;
    }

    pub fn toHsl(self: ColorTriplet) struct { h: f32, s: f32, l: f32 } {
        const r_f: f32 = @as(f32, @floatFromInt(self.r)) / 255.0;
        const g_f: f32 = @as(f32, @floatFromInt(self.g)) / 255.0;
        const b_f: f32 = @as(f32, @floatFromInt(self.b)) / 255.0;

        const max_c = @max(r_f, @max(g_f, b_f));
        const min_c = @min(r_f, @min(g_f, b_f));
        const delta = max_c - min_c;

        const l = (max_c + min_c) / 2.0;

        const s: f32 = if (delta == 0.0) 0.0 else delta / (1.0 - @abs(2.0 * l - 1.0));

        var h: f32 = 0.0;
        if (delta != 0.0) {
            if (max_c == r_f) {
                h = 60.0 * @mod((g_f - b_f) / delta, 6.0);
            } else if (max_c == g_f) {
                h = 60.0 * ((b_f - r_f) / delta + 2.0);
            } else {
                h = 60.0 * ((r_f - g_f) / delta + 4.0);
            }
        }
        if (h < 0.0) h += 360.0;

        return .{ .h = h, .s = s, .l = l };
    }

    pub fn fromHsl(h: f32, s: f32, l: f32) ColorTriplet {
        const c = (1.0 - @abs(2.0 * l - 1.0)) * s;
        const x = c * (1.0 - @abs(@mod(h / 60.0, 2.0) - 1.0));
        const m = l - c / 2.0;

        var r1: f32 = 0.0;
        var g1: f32 = 0.0;
        var b1: f32 = 0.0;

        if (h < 60.0) {
            r1 = c;
            g1 = x;
        } else if (h < 120.0) {
            r1 = x;
            g1 = c;
        } else if (h < 180.0) {
            g1 = c;
            b1 = x;
        } else if (h < 240.0) {
            g1 = x;
            b1 = c;
        } else if (h < 300.0) {
            r1 = x;
            b1 = c;
        } else {
            r1 = c;
            b1 = x;
        }

        return .{
            .r = @intFromFloat(@round((r1 + m) * 255.0)),
            .g = @intFromFloat(@round((g1 + m) * 255.0)),
            .b = @intFromFloat(@round((b1 + m) * 255.0)),
        };
    }

    pub fn blendHsl(c1: ColorTriplet, c2: ColorTriplet, t: f32) ColorTriplet {
        const clamped_t = @max(0.0, @min(1.0, t));
        const hsl1 = c1.toHsl();
        const hsl2 = c2.toHsl();

        var dh = hsl2.h - hsl1.h;
        if (dh > 180.0) dh -= 360.0;
        if (dh < -180.0) dh += 360.0;

        var h = hsl1.h + dh * clamped_t;
        if (h < 0.0) h += 360.0;
        if (h >= 360.0) h -= 360.0;

        return ColorTriplet.fromHsl(
            h,
            hsl1.s + (hsl2.s - hsl1.s) * clamped_t,
            hsl1.l + (hsl2.l - hsl1.l) * clamped_t,
        );
    }

    pub fn luminance(self: ColorTriplet) f64 {
        const r = linearize(@as(f64, @floatFromInt(self.r)) / 255.0);
        const g = linearize(@as(f64, @floatFromInt(self.g)) / 255.0);
        const b = linearize(@as(f64, @floatFromInt(self.b)) / 255.0);
        return 0.2126 * r + 0.7152 * g + 0.0722 * b;
    }

    fn linearize(v: f64) f64 {
        return if (v <= 0.04045)
            v / 12.92
        else
            std.math.pow(f64, (v + 0.055) / 1.055, 2.4);
    }

    pub fn contrastRatio(self: ColorTriplet, other: ColorTriplet) f64 {
        const l1 = self.luminance();
        const l2 = other.luminance();
        const lighter = @max(l1, l2);
        const darker = @min(l1, l2);
        return (lighter + 0.05) / (darker + 0.05);
    }

    pub const WcagLevel = enum {
        fail,
        aa_large,
        aa,
        aaa,
    };

    pub fn wcagLevel(self: ColorTriplet, other: ColorTriplet) WcagLevel {
        const ratio = self.contrastRatio(other);
        if (ratio >= 7.0) return .aaa;
        if (ratio >= 4.5) return .aa;
        if (ratio >= 3.0) return .aa_large;
        return .fail;
    }
};

pub const Color = struct {
    color_type: ColorType,
    number: ?u8 = null,
    triplet: ?ColorTriplet = null,

    // Default color (terminal default)
    pub const default: Color = .{ .color_type = .default };

    // Standard 16 colors (0-15)
    pub const black: Color = .{ .color_type = .standard, .number = 0 };
    pub const red: Color = .{ .color_type = .standard, .number = 1 };
    pub const green: Color = .{ .color_type = .standard, .number = 2 };
    pub const yellow: Color = .{ .color_type = .standard, .number = 3 };
    pub const blue: Color = .{ .color_type = .standard, .number = 4 };
    pub const magenta: Color = .{ .color_type = .standard, .number = 5 };
    pub const cyan: Color = .{ .color_type = .standard, .number = 6 };
    pub const white: Color = .{ .color_type = .standard, .number = 7 };

    // Bright variants (8-15)
    pub const bright_black: Color = .{ .color_type = .standard, .number = 8 };
    pub const bright_red: Color = .{ .color_type = .standard, .number = 9 };
    pub const bright_green: Color = .{ .color_type = .standard, .number = 10 };
    pub const bright_yellow: Color = .{ .color_type = .standard, .number = 11 };
    pub const bright_blue: Color = .{ .color_type = .standard, .number = 12 };
    pub const bright_magenta: Color = .{ .color_type = .standard, .number = 13 };
    pub const bright_cyan: Color = .{ .color_type = .standard, .number = 14 };
    pub const bright_white: Color = .{ .color_type = .standard, .number = 15 };

    pub fn fromRgb(r: u8, g: u8, b: u8) Color {
        return .{
            .color_type = .truecolor,
            .triplet = .{ .r = r, .g = g, .b = b },
        };
    }

    pub fn fromHex(hex_str: []const u8) !Color {
        const start: usize = if (hex_str.len > 0 and hex_str[0] == '#') 1 else 0;
        if (hex_str.len - start != 6) return error.InvalidHexColor;

        const r = std.fmt.parseInt(u8, hex_str[start..][0..2], 16) catch return error.InvalidHexColor;
        const g = std.fmt.parseInt(u8, hex_str[start..][2..4], 16) catch return error.InvalidHexColor;
        const b = std.fmt.parseInt(u8, hex_str[start..][4..6], 16) catch return error.InvalidHexColor;

        return fromRgb(r, g, b);
    }

    pub fn from256(number: u8) Color {
        return .{
            .color_type = .eight_bit,
            .number = number,
        };
    }

    pub fn downgrade(self: Color, target: ColorSystem) Color {
        return switch (self.color_type) {
            .default => self,
            .standard => self,
            .eight_bit => switch (target) {
                .standard => self.toStandard(),
                .eight_bit, .truecolor => self,
            },
            .truecolor => switch (target) {
                .standard => self.toStandard(),
                .eight_bit => self.to256(),
                .truecolor => self,
            },
        };
    }

    fn toStandard(self: Color) Color {
        const triplet = self.getTriplet() orelse return Color.white;

        var best_index: u8 = 0;
        var best_distance: i32 = std.math.maxInt(i32);

        for (standard_color_triplets, 0..) |std_color, i| {
            const dr: i32 = @as(i32, triplet.r) - @as(i32, std_color.r);
            const dg: i32 = @as(i32, triplet.g) - @as(i32, std_color.g);
            const db: i32 = @as(i32, triplet.b) - @as(i32, std_color.b);
            const distance = dr * dr + dg * dg + db * db;

            if (distance < best_distance) {
                best_distance = distance;
                best_index = @intCast(i);
            }
        }

        return .{ .color_type = .standard, .number = best_index };
    }

    fn to256(self: Color) Color {
        const triplet = self.triplet orelse return self;
        const number = rgbTo256(triplet.r, triplet.g, triplet.b);
        return Color.from256(number);
    }

    pub fn getTriplet(self: Color) ?ColorTriplet {
        if (self.triplet) |t| return t;

        // For 256 colors, convert back to RGB approximation
        if (self.color_type == .eight_bit) {
            if (self.number) |n| {
                return tripletFrom256(n);
            }
        }

        // For standard colors, return approximate RGB values
        if (self.color_type == .standard) {
            if (self.number) |n| {
                if (n < standard_color_triplets.len) {
                    return standard_color_triplets[n];
                }
            }
        }

        return null;
    }

    pub fn getAnsiCodes(self: Color, foreground: bool, writer: anytype) !void {
        const base: u8 = if (foreground) 30 else 40;

        switch (self.color_type) {
            .default => try writer.print("{d}", .{if (foreground) @as(u8, 39) else @as(u8, 49)}),
            .standard => {
                const num = self.number orelse return;
                if (num < 8) {
                    try writer.print("{d}", .{base + num});
                } else {
                    try writer.print("{d}", .{base + 60 + num - 8});
                }
            },
            .eight_bit => {
                const num = self.number orelse return;
                try writer.print("{d};5;{d}", .{ if (foreground) @as(u8, 38) else @as(u8, 48), num });
            },
            .truecolor => {
                const t = self.triplet orelse return;
                try writer.print("{d};2;{d};{d};{d}", .{
                    if (foreground) @as(u8, 38) else @as(u8, 48),
                    t.r,
                    t.g,
                    t.b,
                });
            },
        }
    }

    pub fn eql(self: Color, other: Color) bool {
        if (self.color_type != other.color_type) return false;
        if (self.number != other.number) return false;
        if (self.triplet == null and other.triplet == null) return true;
        if (self.triplet == null or other.triplet == null) return false;
        return self.triplet.?.eql(other.triplet.?);
    }

    pub fn paletteToRgb(n: u8) ColorTriplet {
        return tripletFrom256(n);
    }
};

pub const AdaptiveColor = struct {
    truecolor: Color,
    eight_bit: ?Color = null,
    standard: ?Color = null,

    pub fn resolve(self: AdaptiveColor, system: ColorSystem) Color {
        return switch (system) {
            .truecolor => self.truecolor,
            .eight_bit => self.eight_bit orelse self.truecolor.downgrade(.eight_bit),
            .standard => self.standard orelse self.truecolor.downgrade(.standard),
        };
    }

    pub fn fromRgb(r: u8, g: u8, b: u8) AdaptiveColor {
        return .{ .truecolor = Color.fromRgb(r, g, b) };
    }

    pub fn init(truecolor: Color, eight_bit: ?Color, standard_color: ?Color) AdaptiveColor {
        return .{
            .truecolor = truecolor,
            .eight_bit = eight_bit,
            .standard = standard_color,
        };
    }
};

pub fn gradient(stops: []const ColorTriplet, output: []ColorTriplet, comptime use_hsl: bool) void {
    if (stops.len == 0 or output.len == 0) return;
    if (stops.len == 1) {
        for (output) |*c| c.* = stops[0];
        return;
    }

    const n = output.len;
    for (0..n) |i| {
        const t: f32 = if (n == 1) 0.0 else @as(f32, @floatFromInt(i)) / @as(f32, @floatFromInt(n - 1));
        const scaled = t * @as(f32, @floatFromInt(stops.len - 1));
        const idx = @min(@as(usize, @intFromFloat(scaled)), stops.len - 2);
        const local_t = scaled - @as(f32, @floatFromInt(idx));

        output[i] = if (use_hsl)
            ColorTriplet.blendHsl(stops[idx], stops[idx + 1], local_t)
        else
            ColorTriplet.blend(stops[idx], stops[idx + 1], local_t);
    }
}

pub fn rgbTo256(r: u8, g: u8, b: u8) u8 {
    // Check for grayscale
    if (r == g and g == b) {
        if (r < 8) return 16;
        if (r > 248) return 231;
        return @as(u8, @intFromFloat((@as(f32, @floatFromInt(r)) - 8.0) / 247.0 * 24.0)) + 232;
    }

    // Use 6x6x6 color cube (colors 16-231)
    const ri: u8 = @intFromFloat(@round(@as(f32, @floatFromInt(r)) / 255.0 * 5.0));
    const gi: u8 = @intFromFloat(@round(@as(f32, @floatFromInt(g)) / 255.0 * 5.0));
    const bi: u8 = @intFromFloat(@round(@as(f32, @floatFromInt(b)) / 255.0 * 5.0));
    return 16 + 36 * ri + 6 * gi + bi;
}

// RGB values for the 16 standard terminal colors
const standard_color_triplets = [16]ColorTriplet{
    .{ .r = 0, .g = 0, .b = 0 }, // black
    .{ .r = 128, .g = 0, .b = 0 }, // red
    .{ .r = 0, .g = 128, .b = 0 }, // green
    .{ .r = 128, .g = 128, .b = 0 }, // yellow
    .{ .r = 0, .g = 0, .b = 128 }, // blue
    .{ .r = 128, .g = 0, .b = 128 }, // magenta
    .{ .r = 0, .g = 128, .b = 128 }, // cyan
    .{ .r = 192, .g = 192, .b = 192 }, // white
    .{ .r = 128, .g = 128, .b = 128 }, // bright_black
    .{ .r = 255, .g = 0, .b = 0 }, // bright_red
    .{ .r = 0, .g = 255, .b = 0 }, // bright_green
    .{ .r = 255, .g = 255, .b = 0 }, // bright_yellow
    .{ .r = 0, .g = 0, .b = 255 }, // bright_blue
    .{ .r = 255, .g = 0, .b = 255 }, // bright_magenta
    .{ .r = 0, .g = 255, .b = 255 }, // bright_cyan
    .{ .r = 255, .g = 255, .b = 255 }, // bright_white
};

pub fn tripletFrom256(n: u8) ColorTriplet {
    if (n < 16) {
        return standard_color_triplets[n];
    } else if (n < 232) {
        // 6x6x6 color cube
        const cube_index = n - 16;
        const ri = cube_index / 36;
        const gi = (cube_index % 36) / 6;
        const bi = cube_index % 6;
        return .{
            .r = if (ri > 0) @as(u8, @intCast(ri * 40 + 55)) else 0,
            .g = if (gi > 0) @as(u8, @intCast(gi * 40 + 55)) else 0,
            .b = if (bi > 0) @as(u8, @intCast(bi * 40 + 55)) else 0,
        };
    } else {
        // Grayscale (232-255)
        const gray = @as(u8, @intCast((n - 232) * 10 + 8));
        return .{ .r = gray, .g = gray, .b = gray };
    }
}

// Named color lookup table for parsing
pub const named_colors = std.StaticStringMap(Color).initComptime(.{
    .{ "default", Color.default },
    .{ "black", Color.black },
    .{ "red", Color.red },
    .{ "green", Color.green },
    .{ "yellow", Color.yellow },
    .{ "blue", Color.blue },
    .{ "magenta", Color.magenta },
    .{ "cyan", Color.cyan },
    .{ "white", Color.white },
    .{ "bright_black", Color.bright_black },
    .{ "bright_red", Color.bright_red },
    .{ "bright_green", Color.bright_green },
    .{ "bright_yellow", Color.bright_yellow },
    .{ "bright_blue", Color.bright_blue },
    .{ "bright_magenta", Color.bright_magenta },
    .{ "bright_cyan", Color.bright_cyan },
    .{ "bright_white", Color.bright_white },
    .{ "grey", Color.bright_black },
    .{ "gray", Color.bright_black },
});

// Tests
test "ColorTriplet.hex" {
    const triplet = ColorTriplet{ .r = 255, .g = 128, .b = 0 };
    try std.testing.expectEqualStrings("#ff8000", &triplet.hex());
}

test "ColorTriplet.blend" {
    const c1 = ColorTriplet{ .r = 0, .g = 0, .b = 0 };
    const c2 = ColorTriplet{ .r = 255, .g = 255, .b = 255 };

    const mid = ColorTriplet.blend(c1, c2, 0.5);
    try std.testing.expectEqual(@as(u8, 127), mid.r);
    try std.testing.expectEqual(@as(u8, 127), mid.g);
    try std.testing.expectEqual(@as(u8, 127), mid.b);

    const start = ColorTriplet.blend(c1, c2, 0.0);
    try std.testing.expectEqual(@as(u8, 0), start.r);

    const end = ColorTriplet.blend(c1, c2, 1.0);
    try std.testing.expectEqual(@as(u8, 255), end.r);
}

test "Color.fromHex valid" {
    const c1 = try Color.fromHex("#ff0000");
    try std.testing.expectEqual(ColorType.truecolor, c1.color_type);
    try std.testing.expectEqual(@as(u8, 255), c1.triplet.?.r);
    try std.testing.expectEqual(@as(u8, 0), c1.triplet.?.g);
    try std.testing.expectEqual(@as(u8, 0), c1.triplet.?.b);

    const c2 = try Color.fromHex("00ff00");
    try std.testing.expectEqual(@as(u8, 0), c2.triplet.?.r);
    try std.testing.expectEqual(@as(u8, 255), c2.triplet.?.g);

    const c3 = try Color.fromHex("#abcdef");
    try std.testing.expectEqual(@as(u8, 0xab), c3.triplet.?.r);
    try std.testing.expectEqual(@as(u8, 0xcd), c3.triplet.?.g);
    try std.testing.expectEqual(@as(u8, 0xef), c3.triplet.?.b);
}

test "Color.fromHex invalid" {
    try std.testing.expectError(error.InvalidHexColor, Color.fromHex("#ff00"));
    try std.testing.expectError(error.InvalidHexColor, Color.fromHex("ff"));
    try std.testing.expectError(error.InvalidHexColor, Color.fromHex("#gggggg"));
}

test "Color.downgrade truecolor to 256" {
    const truecolor = Color.fromRgb(255, 0, 0);
    const downgraded = truecolor.downgrade(.eight_bit);
    try std.testing.expectEqual(ColorType.eight_bit, downgraded.color_type);
    try std.testing.expect(downgraded.number != null);
}

test "Color.downgrade truecolor to standard" {
    const truecolor = Color.fromRgb(255, 0, 0);
    const downgraded = truecolor.downgrade(.standard);
    try std.testing.expectEqual(ColorType.standard, downgraded.color_type);
    // Should map to red (1) or bright_red (9)
    try std.testing.expect(downgraded.number.? == 9 or downgraded.number.? == 1);
}

test "Color.downgrade standard stays standard" {
    const standard = Color.red;
    const downgraded = standard.downgrade(.standard);
    try std.testing.expect(standard.eql(downgraded));
}

test "Color.getAnsiCodes" {
    var buf: [64]u8 = undefined;
    var stream = std.Io.Writer.fixed(&buf);

    // Default foreground
    try Color.default.getAnsiCodes(true, &stream);
    try std.testing.expectEqualStrings("39", stream.buffered());

    stream.end = 0;

    // Standard red foreground
    try Color.red.getAnsiCodes(true, &stream);
    try std.testing.expectEqualStrings("31", stream.buffered());

    stream.end = 0;

    // Standard red background
    try Color.red.getAnsiCodes(false, &stream);
    try std.testing.expectEqualStrings("41", stream.buffered());

    stream.end = 0;

    // Bright red foreground (number 9)
    try Color.bright_red.getAnsiCodes(true, &stream);
    try std.testing.expectEqualStrings("91", stream.buffered());

    stream.end = 0;

    // 256 color
    try Color.from256(196).getAnsiCodes(true, &stream);
    try std.testing.expectEqualStrings("38;5;196", stream.buffered());

    stream.end = 0;

    // Truecolor
    try Color.fromRgb(255, 128, 64).getAnsiCodes(true, &stream);
    try std.testing.expectEqualStrings("38;2;255;128;64", stream.buffered());
}

test "rgbTo256 grayscale" {
    // Pure black
    try std.testing.expectEqual(@as(u8, 16), rgbTo256(0, 0, 0));
    // Pure white
    try std.testing.expectEqual(@as(u8, 231), rgbTo256(255, 255, 255));
    // Mid gray
    const mid_gray = rgbTo256(128, 128, 128);
    try std.testing.expect(mid_gray >= 232 and mid_gray <= 255);
}

test "rgbTo256 color cube" {
    // Pure red should be near color 196 (5,0,0 in cube)
    const pure_red = rgbTo256(255, 0, 0);
    try std.testing.expectEqual(@as(u8, 196), pure_red);

    // Pure green should be near color 46 (0,5,0 in cube)
    const pure_green = rgbTo256(0, 255, 0);
    try std.testing.expectEqual(@as(u8, 46), pure_green);

    // Pure blue should be near color 21 (0,0,5 in cube)
    const pure_blue = rgbTo256(0, 0, 255);
    try std.testing.expectEqual(@as(u8, 21), pure_blue);
}

test "ColorSystem.supports" {
    try std.testing.expect(ColorSystem.truecolor.supports(.truecolor));
    try std.testing.expect(ColorSystem.truecolor.supports(.eight_bit));
    try std.testing.expect(ColorSystem.truecolor.supports(.standard));

    try std.testing.expect(!ColorSystem.eight_bit.supports(.truecolor));
    try std.testing.expect(ColorSystem.eight_bit.supports(.eight_bit));
    try std.testing.expect(ColorSystem.eight_bit.supports(.standard));

    try std.testing.expect(!ColorSystem.standard.supports(.truecolor));
    try std.testing.expect(!ColorSystem.standard.supports(.eight_bit));
    try std.testing.expect(ColorSystem.standard.supports(.standard));
}

test "named_colors lookup" {
    try std.testing.expect(named_colors.get("red").?.eql(Color.red));
    try std.testing.expect(named_colors.get("green").?.eql(Color.green));
    try std.testing.expect(named_colors.get("gray").?.eql(Color.bright_black));
    try std.testing.expect(named_colors.get("invalid") == null);
}

test "AdaptiveColor auto-downgrade" {
    const ac = AdaptiveColor.fromRgb(255, 100, 50);
    const resolved = ac.resolve(.standard);
    try std.testing.expectEqual(ColorType.standard, resolved.color_type);
}

test "AdaptiveColor explicit fallback" {
    const ac = AdaptiveColor.init(
        Color.fromRgb(255, 100, 50),
        Color.from256(208),
        Color.yellow,
    );
    try std.testing.expect(ac.resolve(.standard).eql(Color.yellow));
    try std.testing.expect(ac.resolve(.eight_bit).eql(Color.from256(208)));
}

test "AdaptiveColor truecolor passthrough" {
    const ac = AdaptiveColor.fromRgb(100, 200, 50);
    const resolved = ac.resolve(.truecolor);
    try std.testing.expectEqual(ColorType.truecolor, resolved.color_type);
    try std.testing.expectEqual(@as(u8, 100), resolved.triplet.?.r);
    try std.testing.expectEqual(@as(u8, 200), resolved.triplet.?.g);
    try std.testing.expectEqual(@as(u8, 50), resolved.triplet.?.b);
}

test "ColorTriplet HSL round-trip" {
    const original = ColorTriplet{ .r = 200, .g = 100, .b = 50 };
    const hsl = original.toHsl();
    const recovered = ColorTriplet.fromHsl(hsl.h, hsl.s, hsl.l);
    try std.testing.expect(@abs(@as(i16, original.r) - @as(i16, recovered.r)) <= 1);
    try std.testing.expect(@abs(@as(i16, original.g) - @as(i16, recovered.g)) <= 1);
    try std.testing.expect(@abs(@as(i16, original.b) - @as(i16, recovered.b)) <= 1);
}

test "blendHsl red to green goes through yellow" {
    const red = ColorTriplet{ .r = 255, .g = 0, .b = 0 };
    const green_c = ColorTriplet{ .r = 0, .g = 255, .b = 0 };
    const mid = ColorTriplet.blendHsl(red, green_c, 0.5);
    try std.testing.expect(mid.r > 100);
    try std.testing.expect(mid.g > 100);
    try std.testing.expect(mid.b < 50);
}

test "gradient produces correct count" {
    const stops = [_]ColorTriplet{
        .{ .r = 255, .g = 0, .b = 0 },
        .{ .r = 0, .g = 255, .b = 0 },
        .{ .r = 0, .g = 0, .b = 255 },
    };
    var output: [5]ColorTriplet = undefined;
    gradient(&stops, &output, false);
    try std.testing.expectEqual(@as(u8, 255), output[0].r);
    try std.testing.expectEqual(@as(u8, 255), output[4].b);
}

test "gradient HSL mode" {
    const stops = [_]ColorTriplet{
        .{ .r = 255, .g = 0, .b = 0 },
        .{ .r = 0, .g = 0, .b = 255 },
    };
    var output: [3]ColorTriplet = undefined;
    gradient(&stops, &output, true);
    try std.testing.expectEqual(@as(u8, 255), output[0].r);
    try std.testing.expectEqual(@as(u8, 255), output[2].b);
}

test "contrastRatio black on white is ~21" {
    const black = ColorTriplet{ .r = 0, .g = 0, .b = 0 };
    const white = ColorTriplet{ .r = 255, .g = 255, .b = 255 };
    const ratio = black.contrastRatio(white);
    try std.testing.expect(ratio > 20.5 and ratio < 21.5);
}

test "contrastRatio same color is 1" {
    const c = ColorTriplet{ .r = 128, .g = 128, .b = 128 };
    const ratio = c.contrastRatio(c);
    try std.testing.expect(ratio > 0.99 and ratio < 1.01);
}

test "wcagLevel black on white is AAA" {
    const black = ColorTriplet{ .r = 0, .g = 0, .b = 0 };
    const white = ColorTriplet{ .r = 255, .g = 255, .b = 255 };
    try std.testing.expectEqual(ColorTriplet.WcagLevel.aaa, black.wcagLevel(white));
}
