const std = @import("std");
const Segment = @import("../segment.zig").Segment;
const segment_mod = @import("../segment.zig");
const Console = @import("../console.zig").Console;

pub const OverflowMode = enum {
    visible,
    clip,
    scroll,
};

pub const Live = struct {
    console: *Console,
    min_refresh_ms: u64 = 50,
    last_refresh_time: i64 = 0,
    lines_rendered: usize = 0,
    is_started: bool = false,
    overflow_mode: OverflowMode = .visible,
    max_lines: ?usize = null,
    scroll_offset: usize = 0,
    content: ?[]const Segment = null,
    content_mutex: std.Io.Mutex = .init,
    refresh_thread: ?std.Thread = null,
    should_stop: std.atomic.Value(bool) = std.atomic.Value(bool).init(false),
    allocator: ?std.mem.Allocator = null,

    pub fn init(console: *Console) Live {
        return .{
            .console = console,
        };
    }

    pub fn withRefreshRate(self: Live, ms: u64) Live {
        var l = self;
        l.min_refresh_ms = ms;
        return l;
    }

    pub fn withOverflow(self: Live, mode: OverflowMode) Live {
        var l = self;
        l.overflow_mode = mode;
        return l;
    }

    pub fn withMaxLines(self: Live, lines: ?usize) Live {
        var l = self;
        l.max_lines = lines;
        return l;
    }

    pub fn scrollUp(self: *Live, lines: usize) void {
        self.scroll_offset -|= lines;
    }

    pub fn scrollDown(self: *Live, lines: usize) void {
        self.scroll_offset += lines;
    }

    pub fn scrollToTop(self: *Live) void {
        self.scroll_offset = 0;
    }

    pub fn scrollToBottom(self: *Live, total_lines: usize) void {
        if (self.max_lines) |max| {
            if (total_lines > max) {
                self.scroll_offset = total_lines - max;
            }
        }
    }

    pub fn start(self: *Live) !void {
        if (self.is_started) return;
        try self.console.hideCursor();
        self.is_started = true;
        self.lines_rendered = 0;
    }

    pub fn stop(self: *Live) !void {
        if (!self.is_started) return;
        self.stopAutoRefresh();
        try self.console.showCursor();
        self.is_started = false;
    }

    pub fn setContent(self: *Live, segments: []const Segment) void {
        self.content_mutex.lockUncancelable(self.console.io);
        defer self.content_mutex.unlock(self.console.io);
        self.content = segments;
    }

    pub fn startAutoRefresh(self: *Live, allocator: std.mem.Allocator) !void {
        if (self.refresh_thread != null) return;

        self.allocator = allocator;
        self.should_stop.store(false, .release);
        self.refresh_thread = try std.Thread.spawn(.{}, autoRefreshThread, .{self});
    }

    pub fn stopAutoRefresh(self: *Live) void {
        if (self.refresh_thread) |thread| {
            self.should_stop.store(true, .release);
            thread.join();
            self.refresh_thread = null;
        }
    }

    fn autoRefreshThread(self: *Live) void {
        while (!self.should_stop.load(.acquire)) {
            std.time.sleep(self.min_refresh_ms * std.time.ns_per_ms);

            if (self.should_stop.load(.acquire)) break;

            self.content_mutex.lockUncancelable(self.console.io);
            const segments = self.content;
            self.content_mutex.unlock(self.console.io);

            if (segments) |segs| {
                self.renderSegments(segs) catch {};
                self.last_refresh_time = std.Io.Clock.real.now(self.console.io).toMilliseconds();
            }
        }
    }

    pub fn update(self: *Live, segments: []const Segment) !void {
        if (!self.is_started) return;

        const now = std.Io.Clock.real.now(self.console.io).toMilliseconds();
        if (now - self.last_refresh_time < @as(i64, @intCast(self.min_refresh_ms))) {
            return;
        }
        self.last_refresh_time = now;

        try self.renderSegments(segments);
    }

    pub fn forceUpdate(self: *Live, segments: []const Segment) !void {
        if (!self.is_started) return;

        try self.renderSegments(segments);
        self.last_refresh_time = std.Io.Clock.real.now(self.console.io).toMilliseconds();
    }

    fn renderSegments(self: *Live, segments: []const Segment) !void {
        try self.clearPrevious();

        var total_lines: usize = 1;
        for (segments) |seg| {
            if (std.mem.eql(u8, seg.text, "\n")) {
                total_lines += 1;
            }
        }

        const max = self.max_lines orelse total_lines;

        if (self.overflow_mode == .visible or total_lines <= max) {
            try self.console.printSegments(segments);
            self.lines_rendered = total_lines;
            return;
        }

        switch (self.overflow_mode) {
            .clip => {
                var line_count: usize = 0;
                for (segments) |seg| {
                    if (line_count >= max) break;
                    if (std.mem.eql(u8, seg.text, "\n")) {
                        line_count += 1;
                        if (line_count < max) {
                            try self.console.printSegments(&[_]Segment{seg});
                        }
                    } else {
                        try self.console.printSegments(&[_]Segment{seg});
                    }
                }
                try self.console.printSegments(&[_]Segment{Segment.line()});
                self.lines_rendered = @min(max, total_lines);
            },
            .scroll => {
                const scroll = @min(self.scroll_offset, if (total_lines > max) total_lines - max else 0);
                var line_count: usize = 0;
                var visible_lines: usize = 0;

                for (segments) |seg| {
                    if (line_count >= scroll and visible_lines < max) {
                        if (std.mem.eql(u8, seg.text, "\n")) {
                            visible_lines += 1;
                            if (visible_lines < max) {
                                try self.console.printSegments(&[_]Segment{seg});
                            }
                        } else {
                            try self.console.printSegments(&[_]Segment{seg});
                        }
                    }
                    if (std.mem.eql(u8, seg.text, "\n")) {
                        line_count += 1;
                    }
                }
                try self.console.printSegments(&[_]Segment{Segment.line()});
                self.lines_rendered = visible_lines;
            },
            .visible => unreachable,
        }
    }

    fn clearPrevious(self: *Live) !void {
        if (self.lines_rendered == 0) return;

        const stdout = std.Io.File.stdout();
        var writer = stdout.writer(self.console.io, self.console.write_buffer);

        for (0..self.lines_rendered) |_| {
            try writer.interface.writeAll("\x1b[A");
            try writer.interface.writeAll("\x1b[2K");
        }
        try writer.interface.writeAll("\r");
        try writer.interface.flush();
    }

    pub fn shouldRefresh(self: Live) bool {
        const now = std.Io.Clock.real.now(self.console.io).toMilliseconds();
        return now - self.last_refresh_time >= @as(i64, @intCast(self.min_refresh_ms));
    }
};

test "Live.init" {
    const allocator = std.testing.allocator;
    var console = Console.init(allocator, std.testing.io, std.testing.environ);
    defer console.deinit();

    const live = Live.init(&console);
    try std.testing.expectEqual(@as(u64, 50), live.min_refresh_ms);
    try std.testing.expect(!live.is_started);
}

test "Live.withRefreshRate" {
    const allocator = std.testing.allocator;
    var console = Console.init(allocator, std.testing.io, std.testing.environ);
    defer console.deinit();

    const live = Live.init(&console).withRefreshRate(100);
    try std.testing.expectEqual(@as(u64, 100), live.min_refresh_ms);
}

test "Live.start sets is_started" {
    const allocator = std.testing.allocator;
    var console = Console.init(allocator, std.testing.io, std.testing.environ);
    defer console.deinit();

    var live = Live.init(&console);
    live.is_started = true;
    try std.testing.expect(live.is_started);
    live.is_started = false;
    try std.testing.expect(!live.is_started);
}

test "Live.update without start" {
    const allocator = std.testing.allocator;
    var console = Console.init(allocator, std.testing.io, std.testing.environ);
    defer console.deinit();

    var live = Live.init(&console);
    const segments = [_]Segment{Segment.plain("test")};

    try live.update(&segments);
    try std.testing.expectEqual(@as(usize, 0), live.lines_rendered);
}

test "Live.withOverflow" {
    const allocator = std.testing.allocator;
    var console = Console.init(allocator, std.testing.io, std.testing.environ);
    defer console.deinit();

    const live = Live.init(&console).withOverflow(.clip);
    try std.testing.expectEqual(OverflowMode.clip, live.overflow_mode);
}

test "Live.withMaxLines" {
    const allocator = std.testing.allocator;
    var console = Console.init(allocator, std.testing.io, std.testing.environ);
    defer console.deinit();

    const live = Live.init(&console).withMaxLines(10);
    try std.testing.expectEqual(@as(?usize, 10), live.max_lines);
}

test "Live.scroll operations" {
    const allocator = std.testing.allocator;
    var console = Console.init(allocator, std.testing.io, std.testing.environ);
    defer console.deinit();

    var live = Live.init(&console).withOverflow(.scroll).withMaxLines(5);

    try std.testing.expectEqual(@as(usize, 0), live.scroll_offset);

    live.scrollDown(3);
    try std.testing.expectEqual(@as(usize, 3), live.scroll_offset);

    live.scrollUp(1);
    try std.testing.expectEqual(@as(usize, 2), live.scroll_offset);

    live.scrollToTop();
    try std.testing.expectEqual(@as(usize, 0), live.scroll_offset);

    live.scrollToBottom(10);
    try std.testing.expectEqual(@as(usize, 5), live.scroll_offset);
}

test "Live.setContent" {
    const allocator = std.testing.allocator;
    const io = std.testing.io;
    var console = Console.init(allocator, io, std.testing.environ);
    defer console.deinit();

    var live = Live.init(&console);
    const segments = [_]Segment{Segment.plain("content")};

    live.setContent(&segments);

    live.content_mutex.lockUncancelable(io);
    defer live.content_mutex.unlock(io);
    try std.testing.expect(live.content != null);
}
