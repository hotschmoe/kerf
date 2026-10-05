const std = @import("std");
const Segment = @import("../../segment.zig").Segment;
const Style = @import("../../style.zig").Style;
const Color = @import("../../color.zig").Color;

const percentage_strings = blk: {
    var strings: [101][5]u8 = undefined;
    for (0..101) |i| {
        const pct: u8 = @intCast(i);
        strings[i][0] = ' ';
        strings[i][1] = if (pct >= 100) '1' else ' ';
        strings[i][2] = if (pct >= 10) '0' + ((pct / 10) % 10) else ' ';
        strings[i][3] = '0' + (pct % 10);
        strings[i][4] = '%';
    }
    break :blk strings;
};

pub fn getPercentageString(pct: f64) []const u8 {
    const clamped = @min(@max(pct, 0.0), 100.0);
    const index: usize = @intFromFloat(@round(clamped));
    return &percentage_strings[index];
}

pub const SpeedUnit = enum {
    items,
    bytes,
    custom,
};

pub const ProgressBar = struct {
    io: std.Io,
    completed: usize = 0,
    total: usize = 100,
    width: usize = 40,
    complete_style: Style = Style.empty.foreground(Color.green),
    finished_style: Style = Style.empty.foreground(Color.bright_green),
    incomplete_style: Style = Style.empty.dim(),
    complete_char: []const u8 = "\u{2501}",
    incomplete_char: []const u8 = "\u{2501}",
    pulse: bool = false,

    description: ?[]const u8 = null,
    description_style: Style = Style.empty,
    start_time: ?i96 = null,
    show_percentage: bool = true,
    show_elapsed: bool = false,
    show_eta: bool = false,
    show_speed: bool = false,
    speed_unit: SpeedUnit = .items,
    speed_suffix: []const u8 = "it/s",

    indeterminate: bool = false,
    pulse_position: usize = 0,
    pulse_width: usize = 4,
    pulse_style: Style = Style.empty.foreground(Color.cyan),
    transient: bool = false,

    pub fn init(io: std.Io) ProgressBar {
        return .{ .io = io };
    }

    pub fn withCompleted(self: ProgressBar, c: usize) ProgressBar {
        var p = self;
        p.completed = c;
        return p;
    }

    pub fn withTotal(self: ProgressBar, t: usize) ProgressBar {
        var p = self;
        p.total = t;
        return p;
    }

    pub fn withWidth(self: ProgressBar, w: usize) ProgressBar {
        var p = self;
        p.width = w;
        return p;
    }

    pub fn withCompleteStyle(self: ProgressBar, s: Style) ProgressBar {
        var p = self;
        p.complete_style = s;
        return p;
    }

    pub fn withIncompleteStyle(self: ProgressBar, s: Style) ProgressBar {
        var p = self;
        p.incomplete_style = s;
        return p;
    }

    pub fn withDescription(self: ProgressBar, text: []const u8) ProgressBar {
        var p = self;
        p.description = text;
        return p;
    }

    pub fn withDescriptionStyle(self: ProgressBar, s: Style) ProgressBar {
        var p = self;
        p.description_style = s;
        return p;
    }

    pub fn withTiming(self: ProgressBar) ProgressBar {
        var p = self;
        p.start_time = std.Io.Timestamp.now(p.io, .awake).toNanoseconds();
        p.show_elapsed = true;
        p.show_eta = true;
        return p;
    }

    pub fn withElapsed(self: ProgressBar) ProgressBar {
        var p = self;
        if (p.start_time == null) {
            p.start_time = std.Io.Timestamp.now(p.io, .awake).toNanoseconds();
        }
        p.show_elapsed = true;
        return p;
    }

    pub fn withEta(self: ProgressBar) ProgressBar {
        var p = self;
        if (p.start_time == null) {
            p.start_time = std.Io.Timestamp.now(p.io, .awake).toNanoseconds();
        }
        p.show_eta = true;
        return p;
    }

    pub fn withSpeed(self: ProgressBar) ProgressBar {
        var p = self;
        if (p.start_time == null) {
            p.start_time = std.Io.Timestamp.now(p.io, .awake).toNanoseconds();
        }
        p.show_speed = true;
        return p;
    }

    pub fn withSpeedUnit(self: ProgressBar, unit: SpeedUnit, suffix: []const u8) ProgressBar {
        var p = self;
        p.speed_unit = unit;
        p.speed_suffix = suffix;
        if (p.start_time == null) {
            p.start_time = std.Io.Timestamp.now(p.io, .awake).toNanoseconds();
        }
        p.show_speed = true;
        return p;
    }

    pub fn asIndeterminate(self: ProgressBar) ProgressBar {
        var p = self;
        p.indeterminate = true;
        return p;
    }

    pub fn withPulseWidth(self: ProgressBar, w: usize) ProgressBar {
        var p = self;
        p.pulse_width = w;
        return p;
    }

    pub fn withPulseStyle(self: ProgressBar, s: Style) ProgressBar {
        var p = self;
        p.pulse_style = s;
        return p;
    }

    pub fn withTransient(self: ProgressBar, t: bool) ProgressBar {
        var p = self;
        p.transient = t;
        return p;
    }

    pub fn shouldHide(self: ProgressBar) bool {
        return self.transient and self.isFinished();
    }

    pub fn calculateElapsed(self: ProgressBar) u64 {
        const start = self.start_time orelse return 0;
        const now = std.Io.Timestamp.now(self.io, .awake).toNanoseconds();
        const diff = now - start;
        if (diff <= 0) return 0;
        return @as(u64, @intCast(diff)) / std.time.ns_per_s;
    }

    pub fn estimateRemaining(self: ProgressBar) ?u64 {
        if (self.completed == 0 or self.start_time == null) return null;
        if (self.completed >= self.total) return 0;

        const elapsed = self.calculateElapsed();
        if (elapsed == 0) return null;

        const rate: f64 = @as(f64, @floatFromInt(self.completed)) / @as(f64, @floatFromInt(elapsed));
        if (rate == 0) return null;

        const remaining: f64 = @as(f64, @floatFromInt(self.total - self.completed)) / rate;
        return @intFromFloat(remaining);
    }

    pub fn calculateSpeed(self: ProgressBar) f64 {
        const elapsed = self.calculateElapsed();
        if (elapsed == 0) return 0;
        return @as(f64, @floatFromInt(self.completed)) / @as(f64, @floatFromInt(elapsed));
    }

    pub fn formatTime(seconds: u64, buf: []u8) []const u8 {
        if (seconds >= 3600) {
            const hours = seconds / 3600;
            const mins = (seconds % 3600) / 60;
            const secs = seconds % 60;
            return std.fmt.bufPrint(buf, "{d:0>2}:{d:0>2}:{d:0>2}", .{ hours, mins, secs }) catch "??:??:??";
        }
        const mins = seconds / 60;
        const secs = seconds % 60;
        return std.fmt.bufPrint(buf, "{d:0>2}:{d:0>2}", .{ mins, secs }) catch "??:??";
    }

    pub fn formatSpeed(rate: f64, unit: SpeedUnit, suffix: []const u8, buf: []u8) []const u8 {
        if (unit != .bytes) {
            return std.fmt.bufPrint(buf, "{d:.1} {s}", .{ rate, suffix }) catch "? ?/s";
        }

        if (rate >= 1_000_000_000) {
            return std.fmt.bufPrint(buf, "{d:.1} GB/s", .{rate / 1_000_000_000}) catch "? GB/s";
        }
        if (rate >= 1_000_000) {
            return std.fmt.bufPrint(buf, "{d:.1} MB/s", .{rate / 1_000_000}) catch "? MB/s";
        }
        if (rate >= 1_000) {
            return std.fmt.bufPrint(buf, "{d:.1} KB/s", .{rate / 1_000}) catch "? KB/s";
        }
        return std.fmt.bufPrint(buf, "{d:.0} B/s", .{rate}) catch "? B/s";
    }

    pub fn advancePulse(self: *ProgressBar) void {
        self.pulse_position = (self.pulse_position + 1) % self.width;
    }

    pub fn reset(self: *ProgressBar) void {
        self.completed = 0;
        self.start_time = std.Io.Timestamp.now(self.io, .awake).toNanoseconds();
        self.pulse_position = 0;
    }

    pub fn render(self: ProgressBar, max_width: usize, allocator: std.mem.Allocator) ![]Segment {
        var segments: std.ArrayList(Segment) = .empty;

        if (self.description) |desc| {
            try segments.append(allocator, Segment.styledOptional(desc, if (self.description_style.isEmpty()) null else self.description_style));
            try segments.append(allocator, Segment.plain("  "));
        }

        const bar_width = @min(self.width, max_width);

        if (self.indeterminate) {
            try self.renderIndeterminate(&segments, allocator, bar_width);
        } else {
            try self.renderDeterminate(&segments, allocator, bar_width);
        }

        if (!self.indeterminate and self.show_percentage) {
            try segments.append(allocator, Segment.plain(getPercentageString(self.percentage())));
        }

        if (self.show_elapsed) {
            var time_buf: [12]u8 = undefined;
            const elapsed = self.calculateElapsed();
            const time_str = formatTime(elapsed, &time_buf);
            try segments.append(allocator, Segment.plain(" "));
            try segments.append(allocator, Segment.plain(try allocator.dupe(u8, time_str)));
        }

        if (self.show_eta and !self.indeterminate) {
            if (self.estimateRemaining()) |eta| {
                var eta_buf: [16]u8 = undefined;
                const eta_str = formatTime(eta, &eta_buf);
                try segments.append(allocator, Segment.plain(" ETA "));
                try segments.append(allocator, Segment.plain(try allocator.dupe(u8, eta_str)));
            }
        }

        if (self.show_speed) {
            var speed_buf: [24]u8 = undefined;
            const rate = self.calculateSpeed();
            const speed_str = formatSpeed(rate, self.speed_unit, self.speed_suffix, &speed_buf);
            try segments.append(allocator, Segment.plain(" "));
            try segments.append(allocator, Segment.plain(try allocator.dupe(u8, speed_str)));
        }

        return segments.toOwnedSlice(allocator);
    }

    fn renderDeterminate(self: ProgressBar, segments: *std.ArrayList(Segment), allocator: std.mem.Allocator, bar_width: usize) !void {
        const ratio: f64 = if (self.total > 0)
            @as(f64, @floatFromInt(self.completed)) / @as(f64, @floatFromInt(self.total))
        else
            0.0;
        const complete_width: usize = @intFromFloat(@min(ratio, 1.0) * @as(f64, @floatFromInt(bar_width)));
        const incomplete_width = bar_width - complete_width;

        const style = if (self.completed >= self.total) self.finished_style else self.complete_style;

        for (0..complete_width) |_| {
            try segments.append(allocator, Segment.styled(self.complete_char, style));
        }

        for (0..incomplete_width) |_| {
            try segments.append(allocator, Segment.styled(self.incomplete_char, self.incomplete_style));
        }
    }

    fn renderIndeterminate(self: ProgressBar, segments: *std.ArrayList(Segment), allocator: std.mem.Allocator, bar_width: usize) !void {
        const pulse_start = self.pulse_position;
        const pulse_end = @min(pulse_start + self.pulse_width, bar_width);

        for (0..bar_width) |i| {
            if (i >= pulse_start and i < pulse_end) {
                try segments.append(allocator, Segment.styled(self.complete_char, self.pulse_style));
            } else {
                try segments.append(allocator, Segment.styled(self.incomplete_char, self.incomplete_style));
            }
        }
    }

    pub fn percentage(self: ProgressBar) f64 {
        if (self.total == 0) return 0.0;
        return @as(f64, @floatFromInt(self.completed)) / @as(f64, @floatFromInt(self.total)) * 100.0;
    }

    pub fn isFinished(self: ProgressBar) bool {
        return self.completed >= self.total;
    }

    pub fn advance(self: *ProgressBar, amount: usize) void {
        self.completed = @min(self.completed + amount, self.total);
    }
};

test "ProgressBar.init" {
    const bar = ProgressBar.init(std.testing.io);
    try std.testing.expectEqual(@as(usize, 0), bar.completed);
    try std.testing.expectEqual(@as(usize, 100), bar.total);
}

test "ProgressBar.percentage" {
    const bar = ProgressBar.init(std.testing.io).withCompleted(50).withTotal(100);
    try std.testing.expectEqual(@as(f64, 50.0), bar.percentage());
}

test "ProgressBar.percentage zero total" {
    const bar = ProgressBar.init(std.testing.io).withTotal(0);
    try std.testing.expectEqual(@as(f64, 0.0), bar.percentage());
}

test "ProgressBar.isFinished" {
    const incomplete = ProgressBar.init(std.testing.io).withCompleted(50).withTotal(100);
    try std.testing.expect(!incomplete.isFinished());

    const complete = ProgressBar.init(std.testing.io).withCompleted(100).withTotal(100);
    try std.testing.expect(complete.isFinished());
}

test "ProgressBar.advance" {
    var bar = ProgressBar.init(std.testing.io).withCompleted(0).withTotal(100);
    bar.advance(10);
    try std.testing.expectEqual(@as(usize, 10), bar.completed);

    bar.advance(100);
    try std.testing.expectEqual(@as(usize, 100), bar.completed);
}

test "ProgressBar.render" {
    const allocator = std.testing.allocator;
    const bar = ProgressBar.init(std.testing.io).withCompleted(50).withTotal(100).withWidth(10);

    const segments = try bar.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len > 0);
}

test "ProgressBar.withDescription" {
    const bar = ProgressBar.init(std.testing.io).withDescription("Loading");
    try std.testing.expectEqualStrings("Loading", bar.description.?);
}

test "ProgressBar.render with description" {
    const allocator = std.testing.allocator;
    const bar = ProgressBar.init(std.testing.io)
        .withDescription("Loading")
        .withCompleted(50)
        .withTotal(100)
        .withWidth(10);

    const segments = try bar.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len > 0);

    var found_desc = false;
    for (segments) |seg| {
        if (std.mem.indexOf(u8, seg.text, "Loading") != null) {
            found_desc = true;
            break;
        }
    }
    try std.testing.expect(found_desc);
}

test "ProgressBar.formatTime" {
    var buf: [12]u8 = undefined;

    const short = ProgressBar.formatTime(65, &buf);
    try std.testing.expectEqualStrings("01:05", short);

    const long_time = ProgressBar.formatTime(3725, &buf);
    try std.testing.expectEqualStrings("01:02:05", long_time);
}

test "ProgressBar.formatSpeed bytes" {
    var buf: [24]u8 = undefined;

    const kb = ProgressBar.formatSpeed(1500, .bytes, "", &buf);
    try std.testing.expect(std.mem.indexOf(u8, kb, "KB/s") != null);

    const mb = ProgressBar.formatSpeed(1_500_000, .bytes, "", &buf);
    try std.testing.expect(std.mem.indexOf(u8, mb, "MB/s") != null);
}

test "ProgressBar.formatSpeed items" {
    var buf: [24]u8 = undefined;

    const items = ProgressBar.formatSpeed(42.5, .items, "it/s", &buf);
    try std.testing.expect(std.mem.indexOf(u8, items, "it/s") != null);
}

test "ProgressBar.asIndeterminate" {
    const bar = ProgressBar.init(std.testing.io).asIndeterminate();
    try std.testing.expect(bar.indeterminate);
}

test "ProgressBar.advancePulse" {
    var bar = ProgressBar.init(std.testing.io).asIndeterminate().withWidth(10);
    try std.testing.expectEqual(@as(usize, 0), bar.pulse_position);

    bar.advancePulse();
    try std.testing.expectEqual(@as(usize, 1), bar.pulse_position);
}

test "ProgressBar.render indeterminate" {
    const allocator = std.testing.allocator;
    const bar = ProgressBar.init(std.testing.io).asIndeterminate().withWidth(10);

    const segments = try bar.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expectEqual(@as(usize, 10), segments.len);
}

test "ProgressBar.withTiming" {
    const bar = ProgressBar.init(std.testing.io).withTiming();
    try std.testing.expect(bar.start_time != null);
    try std.testing.expect(bar.show_elapsed);
    try std.testing.expect(bar.show_eta);
}

test "ProgressBar.withSpeed" {
    const bar = ProgressBar.init(std.testing.io).withSpeed();
    try std.testing.expect(bar.start_time != null);
    try std.testing.expect(bar.show_speed);
}

test "ProgressBar.withSpeedUnit" {
    const bar = ProgressBar.init(std.testing.io).withSpeedUnit(.bytes, "B/s");
    try std.testing.expectEqual(SpeedUnit.bytes, bar.speed_unit);
    try std.testing.expectEqualStrings("B/s", bar.speed_suffix);
}

test "ProgressBar.withTransient" {
    const bar = ProgressBar.init(std.testing.io).withTransient(true);
    try std.testing.expect(bar.transient);
}

test "ProgressBar.shouldHide" {
    const incomplete = ProgressBar.init(std.testing.io).withTransient(true).withCompleted(50).withTotal(100);
    try std.testing.expect(!incomplete.shouldHide());

    const complete = ProgressBar.init(std.testing.io).withTransient(true).withCompleted(100).withTotal(100);
    try std.testing.expect(complete.shouldHide());

    const non_transient = ProgressBar.init(std.testing.io).withCompleted(100).withTotal(100);
    try std.testing.expect(!non_transient.shouldHide());
}

test "ProgressBar.render with timing enabled" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const bar = ProgressBar.init(std.testing.io)
        .withCompleted(50)
        .withTotal(100)
        .withWidth(10)
        .withTiming();

    const segments = try bar.render(80, arena.allocator());

    const text = try @import("../../segment.zig").joinText(segments, arena.allocator());

    try std.testing.expect(std.mem.indexOf(u8, text, ":") != null);
    for (text) |c| {
        try std.testing.expect(c >= 0x20 or c == '\n');
    }
}

test "ProgressBar.render with elapsed only" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const bar = ProgressBar.init(std.testing.io)
        .withCompleted(75)
        .withTotal(100)
        .withWidth(10)
        .withElapsed();

    const segments = try bar.render(80, arena.allocator());

    const text = try @import("../../segment.zig").joinText(segments, arena.allocator());

    try std.testing.expect(std.mem.indexOf(u8, text, "00:00") != null);
}

test "ProgressBar.render with speed" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const bar = ProgressBar.init(std.testing.io)
        .withCompleted(100)
        .withTotal(100)
        .withWidth(10)
        .withSpeed();

    const segments = try bar.render(80, arena.allocator());

    try std.testing.expect(segments.len > 0);
}

test "ProgressBar percentage string lookup" {
    try std.testing.expectEqualStrings("   0%", getPercentageString(0.0));
    try std.testing.expectEqualStrings("  50%", getPercentageString(50.0));
    try std.testing.expectEqualStrings(" 100%", getPercentageString(100.0));
    try std.testing.expectEqualStrings("  75%", getPercentageString(75.4));
    try std.testing.expectEqualStrings("   0%", getPercentageString(-5.0));
    try std.testing.expectEqualStrings(" 100%", getPercentageString(150.0));
}
