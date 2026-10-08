//! Lengths, scales and slopes (SPEC section 1 and 6.1).

const std = @import("std");
const json = @import("json.zig");
const cast = @import("num.zig");
const limits = @import("limits.zig");
const Allocator = std.mem.Allocator;

/// Decimal number in the strict grammar `[+-]? digits [. digits] ([eE] [+-]? digits)?` (at least one digit around the point).
/// Unlike `std.fmt.parseFloat` it never accepts `nan`, `inf`, `infinity`, hex floats or `_` separators, and it returns null
/// when the value overflows to infinity. The one float parser for every document string (REVIEW SAF-5c).
pub fn parseFinite(text: []const u8) ?f64 {
    var i: usize = 0;
    if (i < text.len and (text[i] == '+' or text[i] == '-')) i += 1;
    var digits: usize = 0;
    while (i < text.len and std.ascii.isDigit(text[i])) : (i += 1) digits += 1;
    if (i < text.len and text[i] == '.') {
        i += 1;
        while (i < text.len and std.ascii.isDigit(text[i])) : (i += 1) digits += 1;
    }
    if (digits == 0) return null;
    if (i < text.len and (text[i] == 'e' or text[i] == 'E')) {
        i += 1;
        if (i < text.len and (text[i] == '+' or text[i] == '-')) i += 1;
        var ed: usize = 0;
        while (i < text.len and std.ascii.isDigit(text[i])) : (i += 1) ed += 1;
        if (ed == 0) return null;
    }
    if (i != text.len) return null;
    const v = std.fmt.parseFloat(f64, text) catch return null;
    return if (std.math.isFinite(v)) v else null;
}

pub const length_forms =
    "a number of inches or a string such as 12, \"7 5/8\", \"7-5/8\\\"\", \"3'\", \"3'-4 1/2\\\"\", \"-1'-2\\\"\", \"15/32\"";

const Scan = struct {
    s: []const u8,
    i: usize = 0,

    fn peek(self: *Scan) u8 {
        return if (self.i < self.s.len) self.s[self.i] else 0;
    }
    fn skipSpaces(self: *Scan) void {
        while (self.i < self.s.len and (self.s[self.i] == ' ' or self.s[self.i] == '\t')) self.i += 1;
    }
    /// A run of digits with an optional single decimal point. Null if none.
    fn number(self: *Scan) ?f64 {
        const start = self.i;
        var seen_dot = false;
        var digits: usize = 0;
        while (self.i < self.s.len) : (self.i += 1) {
            const c = self.s[self.i];
            if (std.ascii.isDigit(c)) {
                digits += 1;
            } else if (c == '.' and !seen_dot) {
                seen_dot = true;
            } else break;
        }
        if (digits == 0) {
            self.i = start;
            return null;
        }
        return parseFinite(self.s[start..self.i]);
    }
};

/// Parse inches[.dec] | [whole sep] n/d. `sc` is positioned at the first digit. Returns inches.
fn parseInches(sc: *Scan) ?f64 {
    const first = sc.number() orelse return null;
    if (sc.peek() == '/') {
        // bare fraction n/d
        sc.i += 1;
        const d = sc.number() orelse return null;
        if (d == 0) return null;
        return first / d;
    }
    // optional joiner then fraction: "7 5/8" or "7-5/8"
    const save = sc.i;
    if (sc.peek() == ' ' or sc.peek() == '-') {
        sc.i += 1;
        sc.skipSpaces();
        if (sc.number()) |n| {
            if (sc.peek() == '/') {
                sc.i += 1;
                const d = sc.number() orelse return null;
                if (d == 0) return null;
                return first + n / d;
            }
        }
        sc.i = save;
    }
    return first;
}

/// Parse a length string into inches. Null when malformed.
pub fn parseLengthStr(text: []const u8) ?f64 {
    var sc = Scan{ .s = std.mem.trim(u8, text, " \t") };
    if (sc.s.len == 0) return null;
    var neg = false;
    if (sc.peek() == '-' or sc.peek() == '+') {
        neg = sc.peek() == '-';
        sc.i += 1;
        sc.skipSpaces();
    }
    var total: f64 = 0;
    const start = sc.i;
    const lead = sc.number() orelse return null;
    if (sc.peek() == '\'') {
        // feet
        sc.i += 1;
        total += lead * 12.0;
        sc.skipSpaces();
        if (sc.peek() == '-') {
            sc.i += 1;
            sc.skipSpaces();
        }
        if (sc.i < sc.s.len and sc.peek() != '"') {
            const inch = parseInches(&sc) orelse return null;
            total += inch;
        }
    } else {
        sc.i = start;
        total = parseInches(&sc) orelse return null;
    }
    sc.skipSpaces();
    if (sc.peek() == '"') sc.i += 1;
    sc.skipSpaces();
    if (sc.i != sc.s.len) return null;
    return if (neg) -total else total;
}

/// A length as written, finite but not range-checked (for messages that must tell "out of range" from "not a length").
pub fn parseLengthAny(v: json.Value) ?f64 {
    return switch (v) {
        .number => |n| if (std.math.isFinite(n)) n else null,
        .string => |s| parseLengthStr(s),
        else => null,
    };
}

/// True when `x` is a usable length: finite and at most `limits.max_coord_in` inches in magnitude.
pub fn lengthInRange(x: f64) bool {
    return @abs(x) <= limits.max_coord_in;
}

/// Parse a JSON length (number or string) in inches. Null when it is not a length, not finite or beyond +-`limits.max_coord_in`.
pub fn parseLength(v: json.Value) ?f64 {
    const x = parseLengthAny(v) orelse return null;
    return if (lengthInRange(x)) x else null;
}

/// Architectural feet-inches, nearest 1/16": `0"`, `7 5/8"`, `1'-0"`, `4'-1 1/2"`, `-1'-2"`.
pub fn fmtFtIn(a: Allocator, x: f64) Allocator.Error![]u8 {
    var out: std.ArrayList(u8) = .empty;
    try appendFtIn(&out, a, x);
    return out.items;
}

/// `fmtFtIn` for messages: "?" instead of an error when out of memory.
pub fn ftin(a: Allocator, x: f64) []const u8 {
    return fmtFtIn(a, x) catch "?";
}

pub fn appendFtIn(out: *std.ArrayList(u8), a: Allocator, x: f64) Allocator.Error!void {
    const neg = x < 0;
    const sixteenths: i64 = cast.toInt(i64, @round(@abs(x) * 16.0)) orelse return out.appendSlice(a, "(out of range)");
    if (sixteenths == 0) return out.appendSlice(a, "0\"");
    if (neg) try out.append(a, '-');
    const feet = @divTrunc(sixteenths, 192);
    const rem = @mod(sixteenths, 192);
    const whole = @divTrunc(rem, 16);
    var num: i64 = @mod(rem, 16);
    var den: i64 = 16;
    while (num != 0 and @mod(num, 2) == 0) {
        num = @divTrunc(num, 2);
        den = @divTrunc(den, 2);
    }
    var buf: [48]u8 = undefined;
    if (feet > 0) {
        try out.appendSlice(a, std.fmt.bufPrint(&buf, "{d}'-", .{feet}) catch unreachable);
        try out.appendSlice(a, std.fmt.bufPrint(&buf, "{d}", .{whole}) catch unreachable);
        if (num != 0) try out.appendSlice(a, std.fmt.bufPrint(&buf, " {d}/{d}", .{ num, den }) catch unreachable);
    } else {
        if (whole != 0) {
            try out.appendSlice(a, std.fmt.bufPrint(&buf, "{d}", .{whole}) catch unreachable);
            if (num != 0) try out.appendSlice(a, std.fmt.bufPrint(&buf, " {d}/{d}", .{ num, den }) catch unreachable);
        } else {
            try out.appendSlice(a, std.fmt.bufPrint(&buf, "{d}/{d}", .{ num, den }) catch unreachable);
        }
    }
    try out.append(a, '"');
}

/// Decimal inches in the canonical (1e-4) form.
pub fn fmtDec(a: Allocator, x: f64) Allocator.Error![]u8 {
    var b: [40]u8 = undefined;
    return a.dupe(u8, json.fmtNumber(&b, x));
}

// ---- scales ----------------------------------------------------------------------------------------

pub const Scale = struct {
    /// Model inches per paper inch. 0 means NTS (fit to frame).
    factor: f64,
    /// Normalised display text, e.g. `1 1/2" = 1'-0"`, `1:20` or `NTS`.
    pub fn isNts(self: Scale) bool {
        return self.factor == 0;
    }
};

/// Model inches per paper inch accepted for a view: 1:10 enlargement .. 1:10000.
pub const min_scale_factor: f64 = 0.1;
pub const max_scale_factor: f64 = 10000;

fn finite(f: f64) ?Scale {
    if (!std.math.isFinite(f)) return null;
    return .{ .factor = f };
}

/// A scale `NTS` or one whose factor lies in `min_scale_factor .. max_scale_factor`.
pub fn parseScale(text: []const u8) ?Scale {
    const s = parseScaleAny(text) orelse return null;
    if (s.factor == 0) return s;
    if (!(s.factor >= min_scale_factor and s.factor <= max_scale_factor)) return null;
    return s;
}

/// Scale text as written, without the range check (so a message can say "out of range" instead of "unrecognised").
pub fn parseScaleAny(text: []const u8) ?Scale {
    const t = std.mem.trim(u8, text, " \t");
    if (std.ascii.eqlIgnoreCase(t, "NTS")) return .{ .factor = 0 };
    if (std.mem.indexOfScalar(u8, t, ':')) |c| {
        const a = parseFinite(std.mem.trim(u8, t[0..c], " ")) orelse return null;
        const b = parseFinite(std.mem.trim(u8, t[c + 1 ..], " ")) orelse return null;
        if (a <= 0 or b <= 0) return null;
        return finite(b / a);
    }
    if (std.mem.indexOfScalar(u8, t, '=')) |e| {
        const l = parseLengthStr(t[0..e]) orelse return null;
        const r = parseLengthStr(t[e + 1 ..]) orelse return null;
        if (l <= 0 or r <= 0) return null;
        return finite(r / l);
    }
    return null;
}

/// Scale text for the title block: `1 1/2" = 1'-0"` (architectural), `1:N`, or `NTS`.
pub fn scaleLabel(a: Allocator, factor: f64) Allocator.Error![]u8 {
    if (factor == 0) return a.dupe(u8, "NTS");
    // Architectural scales: paper inches per foot = 12 / factor.
    const per_foot = 12.0 / factor;
    const sixteenths = per_foot * 16.0;
    if (@abs(sixteenths - @round(sixteenths)) < 1e-6 and per_foot >= 1.0 / 16.0 and per_foot < 1e6) {
        var out: std.ArrayList(u8) = .empty;
        // 1-1/2"=1'-0" style with a space between whole and fraction.
        const s: i64 = cast.toInt(i64, @round(sixteenths)) orelse 0;
        const whole = @divTrunc(s, 16);
        var num = @mod(s, 16);
        var den: i64 = 16;
        while (num != 0 and @mod(num, 2) == 0) {
            num = @divTrunc(num, 2);
            den = @divTrunc(den, 2);
        }
        var buf: [32]u8 = undefined;
        if (whole > 0) try out.appendSlice(a, std.fmt.bufPrint(&buf, "{d}", .{whole}) catch unreachable);
        if (num != 0) {
            if (whole > 0) try out.append(a, ' ');
            try out.appendSlice(a, std.fmt.bufPrint(&buf, "{d}/{d}", .{ num, den }) catch unreachable);
        }
        try out.appendSlice(a, "\" = 1'-0\"");
        return out.items;
    }
    var b: [40]u8 = undefined;
    return a.print("1:{s}", .{json.fmtNumber(&b, factor)});
}

// ---- slopes ----------------------------------------------------------------------------------------

/// `"4:12"` (rise:run) or a number of degrees. Returns radians.
pub fn parseSlope(v: json.Value) ?f64 {
    switch (v) {
        .number => |n| return if (std.math.isFinite(n) and @abs(n) <= 360) std.math.degreesToRadians(n) else null,
        .string => |s| {
            const t = std.mem.trim(u8, s, " \t");
            if (std.mem.indexOfScalar(u8, t, ':')) |c| {
                const rise = parseFinite(std.mem.trim(u8, t[0..c], " ")) orelse return null;
                const run = parseFinite(std.mem.trim(u8, t[c + 1 ..], " ")) orelse return null;
                if (run == 0 or @abs(rise) > 1e6 or @abs(run) > 1e6) return null;
                return std.math.atan2(rise, run);
            }
            const deg = parseFinite(t) orelse return null;
            if (@abs(deg) > 360) return null;
            return std.math.degreesToRadians(deg);
        },
        else => return null,
    }
}

test "length parsing: every SPEC form" {
    const cases = [_]struct { s: []const u8, v: f64 }{
        .{ .s = "12", .v = 12 },
        .{ .s = "7 5/8", .v = 7.625 },
        .{ .s = "7-5/8", .v = 7.625 },
        .{ .s = "7-5/8\"", .v = 7.625 },
        .{ .s = "3'", .v = 36 },
        .{ .s = "3'-0\"", .v = 36 },
        .{ .s = "3'-4 1/2\"", .v = 40.5 },
        .{ .s = "3' 4.5\"", .v = 40.5 },
        .{ .s = "-1'-2\"", .v = -14 },
        .{ .s = "0.4375", .v = 0.4375 },
        .{ .s = "15/32", .v = 0.46875 },
        .{ .s = "  4'-1 1/2\"  ", .v = 49.5 },
        .{ .s = "1'-0", .v = 12 },
        .{ .s = "-3/4", .v = -0.75 },
    };
    for (cases) |c| {
        const got = parseLengthStr(c.s) orelse {
            std.debug.print("failed to parse '{s}'\n", .{c.s});
            return error.TestUnexpectedResult;
        };
        try std.testing.expectApproxEqAbs(c.v, got, 1e-12);
    }
    const bad = [_][]const u8{ "", "abc", "7-", "1/0", "3'-x", "7 5/", "1.2.3", "--2" };
    for (bad) |s| try std.testing.expect(parseLengthStr(s) == null);
}

test "ft-in formatting" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const cases = [_]struct { v: f64, s: []const u8 }{
        .{ .v = 0, .s = "0\"" },
        .{ .v = 7.625, .s = "7 5/8\"" },
        .{ .v = 12, .s = "1'-0\"" },
        .{ .v = 49.5, .s = "4'-1 1/2\"" },
        .{ .v = -14, .s = "-1'-2\"" },
        .{ .v = 0.5, .s = "1/2\"" },
        .{ .v = 0.03, .s = "0\"" },
        .{ .v = 11.99, .s = "1'-0\"" },
        .{ .v = 0.4375, .s = "7/16\"" },
        .{ .v = 12.5, .s = "1'-0 1/2\"" },
        .{ .v = 7.25, .s = "7 1/4\"" },
        .{ .v = 97.125, .s = "8'-1 1/8\"" },
    };
    for (cases) |c| {
        const got = try fmtFtIn(a, c.v);
        try std.testing.expectEqualStrings(c.s, got);
    }
}

test "scales" {
    try std.testing.expectEqual(@as(f64, 8), parseScale("1-1/2\"=1'-0\"").?.factor);
    try std.testing.expectEqual(@as(f64, 4), parseScale("3\"=1'-0\"").?.factor);
    try std.testing.expectEqual(@as(f64, 12), parseScale("1\"=1'-0\"").?.factor);
    try std.testing.expectEqual(@as(f64, 16), parseScale("3/4\"=1'-0\"").?.factor);
    try std.testing.expectEqual(@as(f64, 24), parseScale("1/2\"=1'-0\"").?.factor);
    try std.testing.expectEqual(@as(f64, 32), parseScale("3/8\"=1'-0\"").?.factor);
    try std.testing.expectEqual(@as(f64, 48), parseScale("1/4\"=1'-0\"").?.factor);
    try std.testing.expectEqual(@as(f64, 20), parseScale("1:20").?.factor);
    try std.testing.expect(parseScale("NTS").?.isNts());
    try std.testing.expect(parseScale("bogus") == null);
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const l = try scaleLabel(arena.allocator(), 8);
    try std.testing.expectEqualStrings("1 1/2\" = 1'-0\"", l);
}

test "slopes" {
    const r = parseSlope(.{ .string = "4:12" }).?;
    try std.testing.expectApproxEqAbs(18.4349488, std.math.radiansToDegrees(r), 1e-6);
}
