const std = @import("std");
const Segment = @import("../../segment.zig").Segment;
const Style = @import("../../style.zig").Style;
const Color = @import("../../color.zig").Color;
const ProgressBar = @import("bar.zig").ProgressBar;

pub const ProgressGroup = struct {
    bars: std.ArrayList(ProgressBar),
    allocator: std.mem.Allocator,
    io: std.Io,

    pub fn init(allocator: std.mem.Allocator, io: std.Io) ProgressGroup {
        return .{
            .bars = std.ArrayList(ProgressBar).empty,
            .allocator = allocator,
            .io = io,
        };
    }

    pub fn deinit(self: *ProgressGroup) void {
        self.bars.deinit(self.allocator);
    }

    pub fn addTask(self: *ProgressGroup, description: []const u8, total: usize) !*ProgressBar {
        try self.bars.append(self.allocator, ProgressBar.init(self.io)
            .withDescription(description)
            .withTotal(total));
        return &self.bars.items[self.bars.items.len - 1];
    }

    pub fn addTaskWithTiming(self: *ProgressGroup, description: []const u8, total: usize) !*ProgressBar {
        try self.bars.append(self.allocator, ProgressBar.init(self.io)
            .withDescription(description)
            .withTotal(total)
            .withTiming());
        return &self.bars.items[self.bars.items.len - 1];
    }

    pub fn addBar(self: *ProgressGroup, bar: ProgressBar) !*ProgressBar {
        try self.bars.append(self.allocator, bar);
        return &self.bars.items[self.bars.items.len - 1];
    }

    pub fn render(self: ProgressGroup, max_width: usize, allocator: std.mem.Allocator) ![]Segment {
        var segments: std.ArrayList(Segment) = .empty;

        for (self.bars.items, 0..) |bar, i| {
            if (bar.shouldHide()) continue;

            const bar_segments = try bar.render(max_width, allocator);
            defer allocator.free(bar_segments);

            for (bar_segments) |seg| {
                try segments.append(allocator, seg);
            }

            if (i < self.bars.items.len - 1) {
                try segments.append(allocator, Segment.line());
            }
        }

        return segments.toOwnedSlice(allocator);
    }

    pub fn allFinished(self: ProgressGroup) bool {
        for (self.bars.items) |bar| {
            if (!bar.isFinished()) return false;
        }
        return true;
    }

    pub fn visibleCount(self: ProgressGroup) usize {
        var count: usize = 0;
        for (self.bars.items) |bar| {
            if (!bar.shouldHide()) count += 1;
        }
        return count;
    }
};

pub const ProgressDisplay = struct {
    group: *ProgressGroup,
    live: ?*@import("../live.zig").Live = null,
    refresh_thread: ?std.Thread = null,
    should_stop: std.atomic.Value(bool) = std.atomic.Value(bool).init(false),
    refresh_ms: u64 = 100,
    allocator: std.mem.Allocator,
    io: std.Io,
    mutex: std.Io.Mutex = .init,

    pub fn init(allocator: std.mem.Allocator, io: std.Io, group: *ProgressGroup) ProgressDisplay {
        return .{
            .group = group,
            .allocator = allocator,
            .io = io,
        };
    }

    pub fn withRefreshRate(self: ProgressDisplay, ms: u64) ProgressDisplay {
        var pd = self;
        pd.refresh_ms = ms;
        return pd;
    }

    pub fn withLive(self: ProgressDisplay, live: *@import("../live.zig").Live) ProgressDisplay {
        var pd = self;
        pd.live = live;
        return pd;
    }

    pub fn start(self: *ProgressDisplay) !void {
        if (self.refresh_thread != null) return;

        if (self.live) |live| {
            try live.start();
        }

        self.should_stop.store(false, .release);
        self.refresh_thread = try std.Thread.spawn(.{}, autoRefreshThread, .{self});
    }

    pub fn stop(self: *ProgressDisplay) void {
        if (self.refresh_thread) |thread| {
            self.should_stop.store(true, .release);
            thread.join();
            self.refresh_thread = null;

            if (self.live) |live| {
                live.stop() catch {};
            }
        }
    }

    fn autoRefreshThread(self: *ProgressDisplay) void {
        const sleep_duration = std.Io.Duration.fromMilliseconds(@intCast(self.refresh_ms));
        while (!self.should_stop.load(.acquire)) {
            std.Io.sleep(self.io, sleep_duration, .awake) catch {};

            if (self.should_stop.load(.acquire)) break;

            self.mutex.lockUncancelable(self.io);
            const live = self.live;
            self.mutex.unlock(self.io);

            if (live) |l| {
                var arena = std.heap.ArenaAllocator.init(self.allocator);
                defer arena.deinit();

                const segments = self.group.render(80, arena.allocator()) catch continue;
                l.forceUpdate(segments) catch {};
            }
        }
    }

    pub fn advanceSpinners(self: *ProgressDisplay) void {
        self.mutex.lockUncancelable(self.io);
        defer self.mutex.unlock(self.io);

        for (self.group.bars.items) |*bar| {
            if (bar.indeterminate) {
                bar.advancePulse();
            }
        }
    }
};

pub const ColumnRenderContext = struct {
    completed: usize,
    total: usize,
    percentage: f64,
    elapsed_seconds: u64,
    eta_seconds: ?u64,
    speed: f64,
    description: ?[]const u8,
    is_finished: bool,
    is_indeterminate: bool,
    current_ms: i64 = 0,
};

pub const ColumnWidth = union(enum) {
    fixed: usize,
    ratio: usize,
    auto,
};

pub const BuiltinColumn = enum {
    description,
    bar,
    percentage,
    elapsed,
    eta,
    speed,
    spinner,
    completed,
    total,
    task_count,

    pub fn defaultWidth(self: BuiltinColumn) ColumnWidth {
        return switch (self) {
            .description => .auto,
            .bar => .{ .ratio = 1 },
            .percentage => .{ .fixed = 5 },
            .elapsed => .{ .fixed = 8 },
            .eta => .{ .fixed = 12 },
            .speed => .{ .fixed = 12 },
            .spinner => .{ .fixed = 2 },
            .completed => .{ .fixed = 8 },
            .total => .{ .fixed = 8 },
            .task_count => .{ .fixed = 10 },
        };
    }
};

pub const CustomColumnFn = *const fn (ctx: ColumnRenderContext, allocator: std.mem.Allocator) anyerror![]Segment;

pub const ProgressColumn = struct {
    column_type: ColumnType,
    width: ColumnWidth = .auto,
    style: Style = Style.empty,
    min_width: ?usize = null,
    max_width: ?usize = null,
    visible: bool = true,

    pub const ColumnType = union(enum) {
        builtin: BuiltinColumn,
        custom: CustomColumnFn,
        text: []const u8,
    };

    pub fn builtin(col: BuiltinColumn) ProgressColumn {
        return .{
            .column_type = .{ .builtin = col },
            .width = col.defaultWidth(),
        };
    }

    pub fn custom(render_fn: CustomColumnFn) ProgressColumn {
        return .{
            .column_type = .{ .custom = render_fn },
        };
    }

    pub fn text(content: []const u8) ProgressColumn {
        return .{
            .column_type = .{ .text = content },
        };
    }

    pub fn withWidth(self: ProgressColumn, w: ColumnWidth) ProgressColumn {
        var col = self;
        col.width = w;
        return col;
    }

    pub fn withFixedWidth(self: ProgressColumn, w: usize) ProgressColumn {
        var col = self;
        col.width = .{ .fixed = w };
        return col;
    }

    pub fn withRatio(self: ProgressColumn, r: usize) ProgressColumn {
        var col = self;
        col.width = .{ .ratio = r };
        return col;
    }

    pub fn withStyle(self: ProgressColumn, s: Style) ProgressColumn {
        var col = self;
        col.style = s;
        return col;
    }

    pub fn withMinWidth(self: ProgressColumn, w: usize) ProgressColumn {
        var col = self;
        col.min_width = w;
        return col;
    }

    pub fn withMaxWidth(self: ProgressColumn, w: usize) ProgressColumn {
        var col = self;
        col.max_width = w;
        return col;
    }

    pub fn hidden(self: ProgressColumn) ProgressColumn {
        var col = self;
        col.visible = false;
        return col;
    }

    pub fn render(self: ProgressColumn, ctx: ColumnRenderContext, bar_width: usize, allocator: std.mem.Allocator) ![]Segment {
        if (!self.visible) {
            return &.{};
        }

        switch (self.column_type) {
            .builtin => |b| return try self.renderBuiltin(b, ctx, bar_width, allocator),
            .custom => |func| {
                const segments = try func(ctx, allocator);
                self.applyStyle(segments);
                return segments;
            },
            .text => |content| {
                const result = try allocator.alloc(Segment, 1);
                result[0] = self.styledSegment(content);
                return result;
            },
        }
    }

    fn styledSegment(self: ProgressColumn, content: []const u8) Segment {
        return if (self.style.isEmpty()) Segment.plain(content) else Segment.styled(content, self.style);
    }

    fn applyStyle(self: ProgressColumn, segments: []Segment) void {
        if (self.style.isEmpty()) return;

        for (segments) |*seg| {
            if (seg.style) |existing| {
                seg.style = existing.combine(self.style);
            } else {
                seg.style = self.style;
            }
        }
    }

    fn renderBuiltin(self: ProgressColumn, col: BuiltinColumn, ctx: ColumnRenderContext, bar_width: usize, allocator: std.mem.Allocator) ![]Segment {
        var segments: std.ArrayList(Segment) = .empty;

        switch (col) {
            .description => {
                if (ctx.description) |desc| {
                    try segments.append(allocator, self.styledSegment(desc));
                }
            },
            .bar => {
                const style = if (ctx.is_finished)
                    Style.empty.foreground(Color.bright_green)
                else
                    Style.empty.foreground(Color.green);
                const incomplete_style = Style.empty.dim();

                if (ctx.is_indeterminate) {
                    for (0..bar_width) |_| {
                        try segments.append(allocator, Segment.styled("\u{2501}", incomplete_style));
                    }
                } else {
                    const ratio: f64 = ctx.percentage / 100.0;
                    const complete_width: usize = @intFromFloat(@min(ratio, 1.0) * @as(f64, @floatFromInt(bar_width)));
                    const incomplete_width = bar_width - complete_width;

                    for (0..complete_width) |_| {
                        try segments.append(allocator, Segment.styled("\u{2501}", style));
                    }
                    for (0..incomplete_width) |_| {
                        try segments.append(allocator, Segment.styled("\u{2501}", incomplete_style));
                    }
                }
            },
            .percentage => {
                const pct_str = @import("bar.zig").getPercentageString(ctx.percentage);
                try segments.append(allocator, self.styledSegment(pct_str));
            },
            .elapsed => {
                var buf: [12]u8 = undefined;
                const time_str = ProgressBar.formatTime(ctx.elapsed_seconds, &buf);
                try segments.append(allocator, self.styledSegment(try allocator.dupe(u8, time_str)));
            },
            .eta => {
                if (ctx.eta_seconds) |eta| {
                    var buf: [16]u8 = undefined;
                    const eta_str = ProgressBar.formatTime(eta, &buf);
                    try segments.append(allocator, Segment.plain("ETA "));
                    try segments.append(allocator, self.styledSegment(try allocator.dupe(u8, eta_str)));
                } else {
                    try segments.append(allocator, Segment.plain("--:--"));
                }
            },
            .speed => {
                var buf: [24]u8 = undefined;
                const speed_str = ProgressBar.formatSpeed(ctx.speed, .items, "it/s", &buf);
                try segments.append(allocator, self.styledSegment(try allocator.dupe(u8, speed_str)));
            },
            .spinner => {
                const frames = [_][]const u8{
                    "\u{280B}", "\u{2819}", "\u{2839}", "\u{2838}",
                    "\u{283C}", "\u{2834}", "\u{2826}", "\u{2827}",
                };
                const frame_idx = @as(usize, @intCast(@mod(@divFloor(ctx.current_ms, 100), @as(i64, frames.len))));
                try segments.append(allocator, self.styledSegment(frames[frame_idx]));
            },
            .completed => {
                var buf: [16]u8 = undefined;
                const str = std.fmt.bufPrint(&buf, "{d}", .{ctx.completed}) catch "?";
                try segments.append(allocator, self.styledSegment(try allocator.dupe(u8, str)));
            },
            .total => {
                var buf: [16]u8 = undefined;
                const str = std.fmt.bufPrint(&buf, "{d}", .{ctx.total}) catch "?";
                try segments.append(allocator, self.styledSegment(try allocator.dupe(u8, str)));
            },
            .task_count => {
                var buf: [24]u8 = undefined;
                const str = std.fmt.bufPrint(&buf, "{d}/{d}", .{ ctx.completed, ctx.total }) catch "?/?";
                try segments.append(allocator, self.styledSegment(try allocator.dupe(u8, str)));
            },
        }

        return segments.toOwnedSlice(allocator);
    }
};

pub const Progress = struct {
    columns: std.ArrayList(ProgressColumn),
    completed: usize = 0,
    total: usize = 100,
    description: ?[]const u8 = null,
    start_time: ?i96 = null,
    indeterminate: bool = false,
    separator: []const u8 = " ",
    allocator: std.mem.Allocator,
    io: std.Io,

    pub fn init(allocator: std.mem.Allocator, io: std.Io) Progress {
        return .{
            .columns = std.ArrayList(ProgressColumn).empty,
            .allocator = allocator,
            .io = io,
        };
    }

    pub fn deinit(self: *Progress) void {
        self.columns.deinit(self.allocator);
    }

    pub fn withDefaultColumns(allocator: std.mem.Allocator, io: std.Io) !Progress {
        var p = Progress.init(allocator, io);
        try p.addColumn(ProgressColumn.builtin(.description));
        try p.addColumn(ProgressColumn.builtin(.bar));
        try p.addColumn(ProgressColumn.builtin(.percentage));
        return p;
    }

    pub fn withDownloadColumns(allocator: std.mem.Allocator, io: std.Io) !Progress {
        var p = Progress.init(allocator, io);
        try p.addColumn(ProgressColumn.builtin(.description));
        try p.addColumn(ProgressColumn.builtin(.bar));
        try p.addColumn(ProgressColumn.builtin(.percentage));
        try p.addColumn(ProgressColumn.builtin(.elapsed));
        try p.addColumn(ProgressColumn.builtin(.speed));
        try p.addColumn(ProgressColumn.builtin(.eta));
        return p;
    }

    pub fn addColumn(self: *Progress, column: ProgressColumn) !void {
        try self.columns.append(self.allocator, column);
    }

    pub fn addCustomColumn(self: *Progress, render_fn: CustomColumnFn) !void {
        try self.columns.append(self.allocator, ProgressColumn.custom(render_fn));
    }

    pub fn withCompleted(self: Progress, c: usize) Progress {
        var p = self;
        p.completed = c;
        return p;
    }

    pub fn withTotal(self: Progress, t: usize) Progress {
        var p = self;
        p.total = t;
        return p;
    }

    pub fn withDescription(self: Progress, desc: []const u8) Progress {
        var p = self;
        p.description = desc;
        return p;
    }

    pub fn withTiming(self: Progress) Progress {
        var p = self;
        p.start_time = std.Io.Timestamp.now(p.io, .awake).toNanoseconds();
        return p;
    }

    pub fn asIndeterminate(self: Progress) Progress {
        var p = self;
        p.indeterminate = true;
        return p;
    }

    pub fn withSeparator(self: Progress, sep: []const u8) Progress {
        var p = self;
        p.separator = sep;
        return p;
    }

    pub fn percentage(self: Progress) f64 {
        if (self.total == 0) return 0.0;
        return @as(f64, @floatFromInt(self.completed)) / @as(f64, @floatFromInt(self.total)) * 100.0;
    }

    pub fn isFinished(self: Progress) bool {
        return self.completed >= self.total;
    }

    pub fn advance(self: *Progress, amount: usize) void {
        self.completed = @min(self.completed + amount, self.total);
    }

    pub fn calculateElapsed(self: Progress) u64 {
        const start = self.start_time orelse return 0;
        const now = std.Io.Timestamp.now(self.io, .awake).toNanoseconds();
        const diff = now - start;
        if (diff <= 0) return 0;
        return @as(u64, @intCast(diff)) / std.time.ns_per_s;
    }

    pub fn estimateRemaining(self: Progress) ?u64 {
        if (self.completed == 0 or self.start_time == null) return null;
        if (self.completed >= self.total) return 0;

        const elapsed = self.calculateElapsed();
        if (elapsed == 0) return null;

        const rate: f64 = @as(f64, @floatFromInt(self.completed)) / @as(f64, @floatFromInt(elapsed));
        if (rate == 0) return null;

        const remaining: f64 = @as(f64, @floatFromInt(self.total - self.completed)) / rate;
        return @intFromFloat(remaining);
    }

    pub fn calculateSpeed(self: Progress) f64 {
        const elapsed = self.calculateElapsed();
        if (elapsed == 0) return 0;
        return @as(f64, @floatFromInt(self.completed)) / @as(f64, @floatFromInt(elapsed));
    }

    fn buildContext(self: Progress) ColumnRenderContext {
        return .{
            .completed = self.completed,
            .total = self.total,
            .percentage = self.percentage(),
            .elapsed_seconds = self.calculateElapsed(),
            .eta_seconds = self.estimateRemaining(),
            .speed = self.calculateSpeed(),
            .description = self.description,
            .is_finished = self.isFinished(),
            .is_indeterminate = self.indeterminate,
            .current_ms = std.Io.Timestamp.now(self.io, .real).toMilliseconds(),
        };
    }

    fn calculateColumnWidths(self: Progress, max_width: usize, allocator: std.mem.Allocator) ![]usize {
        const visible_columns = blk: {
            var count: usize = 0;
            for (self.columns.items) |col| {
                if (col.visible) count += 1;
            }
            break :blk count;
        };

        if (visible_columns == 0) {
            return try allocator.alloc(usize, 0);
        }

        const widths = try allocator.alloc(usize, self.columns.items.len);
        @memset(widths, 0);

        var total_fixed: usize = 0;
        var total_ratio: usize = 0;
        var auto_count: usize = 0;
        const separator_space = if (visible_columns > 1) (visible_columns - 1) * self.separator.len else 0;

        for (self.columns.items, 0..) |col, i| {
            if (!col.visible) continue;

            switch (col.width) {
                .fixed => |w| {
                    widths[i] = w;
                    total_fixed += w;
                },
                .ratio => |r| total_ratio += r,
                .auto => auto_count += 1,
            }
        }

        var remaining = if (max_width > total_fixed + separator_space)
            max_width - total_fixed - separator_space
        else
            0;

        if (total_ratio > 0 and remaining > 0) {
            const ratio_space = remaining;
            for (self.columns.items, 0..) |col, i| {
                if (!col.visible) continue;
                if (col.width == .ratio) {
                    const r = col.width.ratio;
                    const w = (ratio_space * r) / total_ratio;
                    widths[i] = w;
                    remaining -= w;
                }
            }
        }

        if (auto_count > 0 and remaining > 0) {
            const auto_width = remaining / auto_count;
            for (self.columns.items, 0..) |col, i| {
                if (!col.visible) continue;
                if (col.width == .auto) {
                    widths[i] = auto_width;
                }
            }
        }

        for (self.columns.items, 0..) |col, i| {
            if (col.min_width) |min| {
                if (widths[i] < min) widths[i] = min;
            }
            if (col.max_width) |max| {
                if (widths[i] > max) widths[i] = max;
            }
        }

        return widths;
    }

    pub fn render(self: Progress, max_width: usize, allocator: std.mem.Allocator) ![]Segment {
        if (self.columns.items.len == 0) {
            return &.{};
        }

        const ctx = self.buildContext();
        const widths = try self.calculateColumnWidths(max_width, allocator);
        defer allocator.free(widths);

        var segments: std.ArrayList(Segment) = .empty;
        var first = true;

        for (self.columns.items, 0..) |col, i| {
            if (!col.visible) continue;

            if (!first) {
                try segments.append(allocator, Segment.plain(self.separator));
            }
            first = false;

            const col_segments = try col.render(ctx, widths[i], allocator);
            defer allocator.free(col_segments);

            for (col_segments) |seg| {
                try segments.append(allocator, seg);
            }
        }

        return segments.toOwnedSlice(allocator);
    }
};

test "ProgressGroup.init" {
    const allocator = std.testing.allocator;
    var group = ProgressGroup.init(allocator, std.testing.io);
    defer group.deinit();

    try std.testing.expectEqual(@as(usize, 0), group.bars.items.len);
}

test "ProgressGroup.addTask" {
    const allocator = std.testing.allocator;
    var group = ProgressGroup.init(allocator, std.testing.io);
    defer group.deinit();

    const bar = try group.addTask("Downloading", 100);
    try std.testing.expectEqualStrings("Downloading", bar.description.?);
    try std.testing.expectEqual(@as(usize, 100), bar.total);
}

test "ProgressGroup.allFinished" {
    const allocator = std.testing.allocator;
    var group = ProgressGroup.init(allocator, std.testing.io);
    defer group.deinit();

    _ = try group.addTask("Task 1", 100);
    _ = try group.addTask("Task 2", 100);

    try std.testing.expect(!group.allFinished());

    group.bars.items[0].completed = 100;
    try std.testing.expect(!group.allFinished());

    group.bars.items[1].completed = 100;
    try std.testing.expect(group.allFinished());
}

test "ProgressGroup.render" {
    const allocator = std.testing.allocator;
    var group = ProgressGroup.init(allocator, std.testing.io);
    defer group.deinit();

    _ = try group.addTask("Task 1", 100);
    _ = try group.addTask("Task 2", 100);

    const segments = try group.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len > 0);
}

test "ProgressGroup.visibleCount with transient" {
    const allocator = std.testing.allocator;
    var group = ProgressGroup.init(allocator, std.testing.io);
    defer group.deinit();

    _ = try group.addBar(ProgressBar.init(std.testing.io).withDescription("Task 1").withTotal(100).withTransient(true));
    _ = try group.addBar(ProgressBar.init(std.testing.io).withDescription("Task 2").withTotal(100));

    try std.testing.expectEqual(@as(usize, 2), group.visibleCount());

    group.bars.items[0].completed = 100;
    try std.testing.expectEqual(@as(usize, 1), group.visibleCount());
}

test "ProgressDisplay.init" {
    const allocator = std.testing.allocator;
    var group = ProgressGroup.init(allocator, std.testing.io);
    defer group.deinit();

    const display = ProgressDisplay.init(allocator, std.testing.io, &group);
    try std.testing.expectEqual(@as(u64, 100), display.refresh_ms);
}

test "ProgressDisplay.withRefreshRate" {
    const allocator = std.testing.allocator;
    var group = ProgressGroup.init(allocator, std.testing.io);
    defer group.deinit();

    const display = ProgressDisplay.init(allocator, std.testing.io, &group).withRefreshRate(50);
    try std.testing.expectEqual(@as(u64, 50), display.refresh_ms);
}

test "ProgressColumn.builtin" {
    const col = ProgressColumn.builtin(.percentage);
    try std.testing.expectEqual(BuiltinColumn.percentage, col.column_type.builtin);
    try std.testing.expectEqual(@as(usize, 5), col.width.fixed);
}

test "ProgressColumn.custom" {
    const customFn = struct {
        fn render(_: ColumnRenderContext, alloc: std.mem.Allocator) anyerror![]Segment {
            const result = try alloc.alloc(Segment, 1);
            result[0] = Segment.plain("custom");
            return result;
        }
    }.render;

    const col = ProgressColumn.custom(customFn);
    try std.testing.expect(col.column_type == .custom);
}

test "ProgressColumn.text" {
    const col = ProgressColumn.text("[");
    try std.testing.expectEqualStrings("[", col.column_type.text);
}

test "ProgressColumn.withWidth" {
    const col = ProgressColumn.builtin(.bar).withFixedWidth(20);
    try std.testing.expectEqual(@as(usize, 20), col.width.fixed);
}

test "ProgressColumn.render builtin percentage" {
    const allocator = std.testing.allocator;
    const col = ProgressColumn.builtin(.percentage);

    const ctx = ColumnRenderContext{
        .completed = 50,
        .total = 100,
        .percentage = 50.0,
        .elapsed_seconds = 0,
        .eta_seconds = null,
        .speed = 0,
        .description = null,
        .is_finished = false,
        .is_indeterminate = false,
    };

    const segments = try col.render(ctx, 5, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len > 0);
    try std.testing.expectEqualStrings("  50%", segments[0].text);
}

test "ProgressColumn.render custom" {
    const allocator = std.testing.allocator;

    const customFn = struct {
        fn render(_: ColumnRenderContext, alloc: std.mem.Allocator) anyerror![]Segment {
            const result = try alloc.alloc(Segment, 1);
            result[0] = Segment.plain("CUSTOM");
            return result;
        }
    }.render;

    const col = ProgressColumn.custom(customFn);

    const ctx = ColumnRenderContext{
        .completed = 0,
        .total = 100,
        .percentage = 0.0,
        .elapsed_seconds = 0,
        .eta_seconds = null,
        .speed = 0,
        .description = null,
        .is_finished = false,
        .is_indeterminate = false,
    };

    const segments = try col.render(ctx, 10, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len == 1);
    try std.testing.expectEqualStrings("CUSTOM", segments[0].text);
}

test "Progress.init" {
    const allocator = std.testing.allocator;
    var progress = Progress.init(allocator, std.testing.io);
    defer progress.deinit();

    try std.testing.expectEqual(@as(usize, 0), progress.columns.items.len);
}

test "Progress.withDefaultColumns" {
    const allocator = std.testing.allocator;
    var progress = try Progress.withDefaultColumns(allocator, std.testing.io);
    defer progress.deinit();

    try std.testing.expectEqual(@as(usize, 3), progress.columns.items.len);
}

test "Progress.addColumn" {
    const allocator = std.testing.allocator;
    var progress = Progress.init(allocator, std.testing.io);
    defer progress.deinit();

    try progress.addColumn(ProgressColumn.builtin(.description));
    try progress.addColumn(ProgressColumn.builtin(.bar));

    try std.testing.expectEqual(@as(usize, 2), progress.columns.items.len);
}

test "Progress.addCustomColumn" {
    const allocator = std.testing.allocator;
    var progress = Progress.init(allocator, std.testing.io);
    defer progress.deinit();

    const customFn = struct {
        fn render(_: ColumnRenderContext, alloc: std.mem.Allocator) anyerror![]Segment {
            const result = try alloc.alloc(Segment, 1);
            result[0] = Segment.plain("test");
            return result;
        }
    }.render;

    try progress.addCustomColumn(customFn);
    try std.testing.expectEqual(@as(usize, 1), progress.columns.items.len);
}

test "Progress.render" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    var progress = try Progress.withDefaultColumns(arena.allocator(), std.testing.io);
    progress = progress.withCompleted(50).withTotal(100).withDescription("Loading");

    const segments = try progress.render(80, arena.allocator());

    try std.testing.expect(segments.len > 0);
}

test "Progress.render with custom column" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    var progress = Progress.init(arena.allocator(), std.testing.io);

    const customFn = struct {
        fn render(ctx: ColumnRenderContext, alloc: std.mem.Allocator) anyerror![]Segment {
            const result = try alloc.alloc(Segment, 1);
            var buf: [32]u8 = undefined;
            const str = std.fmt.bufPrint(&buf, "Items: {d}/{d}", .{ ctx.completed, ctx.total }) catch "?";
            result[0] = Segment.plain(try alloc.dupe(u8, str));
            return result;
        }
    }.render;

    try progress.addCustomColumn(customFn);
    progress = progress.withCompleted(25).withTotal(50);

    const segments = try progress.render(80, arena.allocator());

    try std.testing.expect(segments.len > 0);

    const text = try @import("../../segment.zig").joinText(segments, arena.allocator());
    try std.testing.expect(std.mem.indexOf(u8, text, "25/50") != null);
}

test "Progress.percentage" {
    const allocator = std.testing.allocator;
    var progress = Progress.init(allocator, std.testing.io);
    defer progress.deinit();

    progress = progress.withCompleted(50).withTotal(100);
    try std.testing.expectEqual(@as(f64, 50.0), progress.percentage());
}

test "Progress.isFinished" {
    const allocator = std.testing.allocator;
    var progress = Progress.init(allocator, std.testing.io);
    defer progress.deinit();

    progress = progress.withCompleted(50).withTotal(100);
    try std.testing.expect(!progress.isFinished());

    progress = progress.withCompleted(100);
    try std.testing.expect(progress.isFinished());
}

test "Progress.advance" {
    const allocator = std.testing.allocator;
    var progress = Progress.init(allocator, std.testing.io);
    defer progress.deinit();

    progress = progress.withTotal(100);
    progress.advance(25);
    try std.testing.expectEqual(@as(usize, 25), progress.completed);

    progress.advance(100);
    try std.testing.expectEqual(@as(usize, 100), progress.completed);
}

test "ProgressColumn.hidden" {
    const col = ProgressColumn.builtin(.percentage).hidden();
    try std.testing.expect(!col.visible);
}

test "Progress.render with hidden column" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    var progress = Progress.init(arena.allocator(), std.testing.io);
    try progress.addColumn(ProgressColumn.builtin(.description));
    try progress.addColumn(ProgressColumn.builtin(.percentage).hidden());
    progress = progress.withDescription("Test");

    const segments = try progress.render(80, arena.allocator());

    const text = try @import("../../segment.zig").joinText(segments, arena.allocator());
    try std.testing.expect(std.mem.indexOf(u8, text, "%") == null);
}

test "Progress.withDownloadColumns" {
    const allocator = std.testing.allocator;
    var progress = try Progress.withDownloadColumns(allocator, std.testing.io);
    defer progress.deinit();

    try std.testing.expectEqual(@as(usize, 6), progress.columns.items.len);
}
