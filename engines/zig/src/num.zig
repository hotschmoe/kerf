//! Checked float -> integer conversion. `@intFromFloat` is illegal behaviour (a panic in safe builds, undefined
//! in ReleaseSmall/Fast) for NaN, infinities and out-of-range values, and every one of those can come from a
//! document, a style or an op. Every conversion of an untrusted float goes through here (REVIEW SAF-1).

const std = @import("std");

/// Float -> integer that never invokes undefined behaviour: null for NaN/inf/out-of-range, truncates toward zero otherwise.
pub fn toInt(comptime T: type, x: f64) ?T {
    if (!std.math.isFinite(x)) return null;
    const bits = @typeInfo(T).int.bits;
    const signed = @typeInfo(T).int.signedness == .signed;
    // 2^bits (or 2^(bits-1)) is exact as an f64; comparing against the open bound avoids "maxInt rounds up".
    const hi: f64 = std.math.ldexp(@as(f64, 1), if (signed) bits - 1 else bits);
    if (x >= hi or (if (signed) x < -hi else x <= -1)) return null;
    return @intFromFloat(x);
}

/// Clamp then convert; NaN maps to `lo`. For counts and step numbers where "too big" should saturate.
pub fn toIntClamped(comptime T: type, x: f64, lo: T, hi: T) T {
    if (std.math.isNan(x)) return lo;
    // `hi` can round up when converted to f64 (maxInt(i64)), so go through the checked conversion once more.
    return toInt(T, std.math.clamp(x, @as(f64, @floatFromInt(lo)), @as(f64, @floatFromInt(hi)))) orelse hi;
}

test "toInt: range and NaN" {
    try std.testing.expect(toInt(i64, 1e30) == null);
    try std.testing.expect(toInt(i64, -1e30) == null);
    try std.testing.expect(toInt(i64, std.math.nan(f64)) == null);
    try std.testing.expect(toInt(i64, std.math.inf(f64)) == null);
    try std.testing.expect(toInt(i64, 9223372036854775808.0) == null);
    try std.testing.expectEqual(@as(?i64, std.math.minInt(i64)), toInt(i64, -9223372036854775808.0));
    try std.testing.expect(toInt(usize, -1) == null);
    try std.testing.expectEqual(@as(?usize, 0), toInt(usize, -0.5));
    try std.testing.expectEqual(@as(?u8, 255), toInt(u8, 255.9));
    try std.testing.expect(toInt(u8, 256) == null);
    try std.testing.expectEqual(@as(?i32, -7), toInt(i32, -7.9));
}

test "toIntClamped saturates and maps NaN to lo" {
    try std.testing.expectEqual(@as(usize, 2), toIntClamped(usize, std.math.nan(f64), 2, 4096));
    try std.testing.expectEqual(@as(usize, 4096), toIntClamped(usize, std.math.inf(f64), 2, 4096));
    try std.testing.expectEqual(@as(usize, 2), toIntClamped(usize, -std.math.inf(f64), 2, 4096));
    try std.testing.expectEqual(@as(usize, 100), toIntClamped(usize, 100.7, 2, 4096));
}
