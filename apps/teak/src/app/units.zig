//! Display formatting for model inches: feet-inch strings with fractions
//! (status line cursor read-out, dimensions in the inspector).

const std = @import("std");

/// `2'-3 1/2"`, `0"`, `7 5/8"`, `-4'-0"`. Rounded to the nearest 1/16".
pub fn formatFeetInches(buf: []u8, inches: f64) []const u8 {
    const neg = inches < 0;
    const sixteenths_total: u64 = @intFromFloat(@round(@abs(inches) * 16));
    const whole_in = sixteenths_total / 16;
    var frac = sixteenths_total % 16;
    const ft = whole_in / 12;
    const inch = whole_in % 12;

    var w: std.Io.Writer = .fixed(buf);
    if (neg and sixteenths_total != 0) w.writeByte('-') catch {};
    if (ft > 0) w.print("{d}'-", .{ft}) catch {};
    w.print("{d}", .{inch}) catch {};
    if (frac != 0) {
        var den: u64 = 16;
        while (frac % 2 == 0) : ({
            frac /= 2;
            den /= 2;
        }) {}
        w.print(" {d}/{d}", .{ frac, den }) catch {};
    }
    w.writeByte('"') catch {};
    return w.buffered();
}

test "feet-inches" {
    var b: [32]u8 = undefined;
    try std.testing.expectEqualStrings("0\"", formatFeetInches(&b, 0));
    try std.testing.expectEqualStrings("2'-3 1/2\"", formatFeetInches(&b, 27.5));
    try std.testing.expectEqualStrings("7 5/8\"", formatFeetInches(&b, 7.625));
    try std.testing.expectEqualStrings("-4'-0\"", formatFeetInches(&b, -48));
    try std.testing.expectEqualStrings("1'-0\"", formatFeetInches(&b, 12.01));
}
