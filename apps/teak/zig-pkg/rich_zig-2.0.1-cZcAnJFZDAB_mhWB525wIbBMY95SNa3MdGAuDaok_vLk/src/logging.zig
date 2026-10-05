const std = @import("std");
const Style = @import("style.zig").Style;
const Color = @import("color.zig").Color;
const ColorSystem = @import("color.zig").ColorSystem;
const terminal = @import("terminal.zig");
const Segment = @import("segment.zig").Segment;
const box = @import("box.zig");
const Panel = @import("renderables/panel.zig").Panel;
const Syntax = @import("renderables/syntax/mod.zig").Syntax;
const SyntaxTheme = @import("renderables/syntax/mod.zig").SyntaxTheme;
const Language = @import("renderables/syntax/mod.zig").Language;

/// Log level matching std.log.Level for compatibility
pub const Level = enum {
    debug,
    info,
    warn,
    err,

    pub fn toString(self: Level) []const u8 {
        return switch (self) {
            .debug => "DEBUG",
            .info => "INFO",
            .warn => "WARN",
            .err => "ERROR",
        };
    }

    pub fn toStdLevel(self: Level) std.log.Level {
        return switch (self) {
            .debug => .debug,
            .info => .info,
            .warn => .warn,
            .err => .err,
        };
    }

    pub fn fromStdLevel(level: std.log.Level) Level {
        return switch (level) {
            .debug => .debug,
            .info => .info,
            .warn => .warn,
            .err => .err,
        };
    }
};

/// Styles for each log level
pub const LevelStyles = struct {
    debug: Style = Style.empty.dim(),
    info: Style = Style.empty.foreground(Color.blue),
    warn: Style = Style.empty.foreground(Color.yellow).bold(),
    err: Style = Style.empty.foreground(Color.red).bold(),
    timestamp: Style = Style.empty.dim(),
    path: Style = Style.empty.foreground(Color.cyan).dim(),
    message: Style = Style.empty,

    pub const default: LevelStyles = .{};

    pub fn styleFor(self: LevelStyles, level: Level) Style {
        return switch (level) {
            .debug => self.debug,
            .info => self.info,
            .warn => self.warn,
            .err => self.err,
        };
    }
};

/// Format options for log output
pub const FormatOptions = struct {
    show_timestamp: bool = true,
    show_level: bool = true,
    show_path: bool = false,
    timestamp_format: TimestampFormat = .time_only,
    level_width: u8 = 5, // Width for level field (for alignment)
    path_width: ?u16 = null, // Max width for path, null for no limit

    pub const TimestampFormat = enum {
        time_only, // HH:MM:SS
        datetime, // YYYY-MM-DD HH:MM:SS
        iso8601, // YYYY-MM-DDTHH:MM:SSZ
    };
};

/// A log record with all metadata
pub const LogRecord = struct {
    level: Level,
    message: []const u8,
    timestamp: i64,
    path: ?[]const u8 = null,
    line: ?u32 = null,

    pub fn init(io: std.Io, level: Level, message: []const u8) LogRecord {
        return .{
            .level = level,
            .message = message,
            .timestamp = std.Io.Timestamp.now(io, .real).toSeconds(),
        };
    }

    pub fn withPath(self: LogRecord, path: []const u8) LogRecord {
        var r = self;
        r.path = path;
        return r;
    }

    pub fn withLine(self: LogRecord, line: u32) LogRecord {
        var r = self;
        r.line = line;
        return r;
    }
};

/// Rich-style log handler that formats and outputs log messages
pub const RichHandler = struct {
    io: std.Io,
    allocator: std.mem.Allocator,
    writer: std.Io.File.Writer,
    styles: LevelStyles,
    format: FormatOptions,
    color_system: ColorSystem,
    min_level: Level,
    write_buffer: []u8,

    const BUFFER_SIZE = 4096;

    pub fn init(io: std.Io, allocator: std.mem.Allocator, environ: std.process.Environ) RichHandler {
        return initWithOptions(io, allocator, environ, .{}, .{});
    }

    pub fn initWithOptions(
        io: std.Io,
        allocator: std.mem.Allocator,
        environ: std.process.Environ,
        styles: LevelStyles,
        format: FormatOptions,
    ) RichHandler {
        const term_info = terminal.detect(environ);
        const buffer = allocator.alloc(u8, BUFFER_SIZE) catch @panic("failed to allocate logging buffer");

        return .{
            .io = io,
            .allocator = allocator,
            .writer = std.Io.File.stdout().writer(io, buffer),
            .styles = styles,
            .format = format,
            .color_system = term_info.color_system,
            .min_level = .debug,
            .write_buffer = buffer,
        };
    }

    pub fn deinit(self: *RichHandler) void {
        self.allocator.free(self.write_buffer);
    }

    pub fn setMinLevel(self: *RichHandler, level: Level) *RichHandler {
        self.min_level = level;
        return self;
    }

    pub fn setColorSystem(self: *RichHandler, color_system: ColorSystem) *RichHandler {
        self.color_system = color_system;
        return self;
    }

    pub fn emit(self: *RichHandler, record: LogRecord) !void {
        if (@intFromEnum(record.level) < @intFromEnum(self.min_level)) {
            return;
        }

        const writer = &self.writer.interface;

        // Timestamp
        if (self.format.show_timestamp) {
            try self.writeTimestamp(record.timestamp, writer);
            try writer.writeAll(" ");
        }

        // Level
        if (self.format.show_level) {
            try self.writeLevel(record.level, writer);
            try writer.writeAll(" ");
        }

        // Path
        if (self.format.show_path) {
            if (record.path) |path| {
                try self.writePath(path, record.line, writer);
                try writer.writeAll(" ");
            }
        }

        // Message
        try self.writeMessage(record.message, writer);
        try writer.writeByte('\n');
        try writer.flush();
    }

    fn writeTimestamp(self: *RichHandler, timestamp: i64, writer: anytype) !void {
        const epoch_seconds: std.time.epoch.EpochSeconds = .{ .secs = @intCast(timestamp) };
        const day_seconds = epoch_seconds.getDaySeconds();
        const hours = day_seconds.getHoursIntoDay();
        const minutes = day_seconds.getMinutesIntoHour();
        const seconds = day_seconds.getSecondsIntoMinute();

        try self.styles.timestamp.renderAnsi(self.color_system, writer);

        switch (self.format.timestamp_format) {
            .time_only => {
                try writer.print("{d:0>2}:{d:0>2}:{d:0>2}", .{ hours, minutes, seconds });
            },
            .datetime, .iso8601 => {
                const year_day = epoch_seconds.getEpochDay().calculateYearDay();
                const year = year_day.year;
                const month_day = year_day.calculateMonthDay();
                const month = @intFromEnum(month_day.month) + 1;
                const day = month_day.day_index + 1;

                if (self.format.timestamp_format == .iso8601) {
                    try writer.print("{d:0>4}-{d:0>2}-{d:0>2}T{d:0>2}:{d:0>2}:{d:0>2}Z", .{
                        year, month, day, hours, minutes, seconds,
                    });
                } else {
                    try writer.print("{d:0>4}-{d:0>2}-{d:0>2} {d:0>2}:{d:0>2}:{d:0>2}", .{
                        year, month, day, hours, minutes, seconds,
                    });
                }
            },
        }

        try Style.renderReset(writer);
    }

    fn writeLevel(self: *RichHandler, level: Level, writer: anytype) !void {
        const level_str = level.toString();
        const level_style = self.styles.styleFor(level);

        try level_style.renderAnsi(self.color_system, writer);
        try writer.print("[{s}]", .{level_str});

        // Padding for alignment
        const written_len = level_str.len + 2; // +2 for brackets
        const target_width = self.format.level_width + 2;
        for (written_len..target_width) |_| try writer.writeByte(' ');

        try Style.renderReset(writer);
    }

    fn writePath(self: *RichHandler, path: []const u8, line: ?u32, writer: anytype) !void {
        try self.styles.path.renderAnsi(self.color_system, writer);

        // Truncate path if needed
        var display_path = path;
        if (self.format.path_width) |max_width| {
            if (path.len > max_width) {
                const start = path.len - max_width + 3; // +3 for "..."
                try writer.writeAll("...");
                display_path = path[start..];
            }
        }

        try writer.writeAll(display_path);

        if (line) |ln| {
            try writer.print(":{d}", .{ln});
        }

        try Style.renderReset(writer);
    }

    fn writeMessage(self: *RichHandler, message: []const u8, writer: anytype) !void {
        const styled = !self.styles.message.isEmpty();
        if (styled) try self.styles.message.renderAnsi(self.color_system, writer);
        try writer.writeAll(message);
        if (styled) try Style.renderReset(writer);
    }

    // Convenience methods
    pub fn debug(self: *RichHandler, message: []const u8) !void {
        try self.emit(LogRecord.init(self.io, .debug, message));
    }

    pub fn info(self: *RichHandler, message: []const u8) !void {
        try self.emit(LogRecord.init(self.io, .info, message));
    }

    pub fn warn(self: *RichHandler, message: []const u8) !void {
        try self.emit(LogRecord.init(self.io, .warn, message));
    }

    pub fn err(self: *RichHandler, message: []const u8) !void {
        try self.emit(LogRecord.init(self.io, .err, message));
    }
};

/// A formatted log function that can be used with std.log
/// Usage: pub const std_options = .{ .logFn = logging.stdLogFn };
pub fn stdLogFn(
    comptime level: std.log.Level,
    comptime scope: @TypeOf(.enum_literal),
    comptime format: []const u8,
    args: anytype,
) void {
    // Get the global handler or use default styling
    const level_style = switch (level) {
        .debug => Style.empty.dim(),
        .info => Style.empty.foreground(Color.blue),
        .warn => Style.empty.foreground(Color.yellow).bold(),
        .err => Style.empty.foreground(Color.red).bold(),
    };
    const timestamp_style = Style.empty.dim();

    const scope_str = if (scope == .default) "" else @tagName(scope);

    var buf: [64]u8 = undefined;
    const locked = std.debug.lockStderr(&buf);
    defer std.debug.unlockStderr();
    const stderr = &locked.file_writer.interface;

    const timestamp = std.Io.Timestamp.now(std.Options.debug_io, .real).toSeconds();
    const epoch_seconds: std.time.epoch.EpochSeconds = .{ .secs = @intCast(timestamp) };
    const day_seconds = epoch_seconds.getDaySeconds();

    // Write timestamp
    timestamp_style.renderAnsi(.truecolor, stderr) catch return;
    stderr.print("{d:0>2}:{d:0>2}:{d:0>2}", .{
        day_seconds.getHoursIntoDay(),
        day_seconds.getMinutesIntoHour(),
        day_seconds.getSecondsIntoMinute(),
    }) catch return;
    Style.renderReset(stderr) catch return;
    stderr.writeAll(" ") catch return;

    // Write level
    level_style.renderAnsi(.truecolor, stderr) catch return;
    stderr.print("[{s}]", .{Level.fromStdLevel(level).toString()}) catch return;
    Style.renderReset(stderr) catch return;

    // Write scope if present
    if (scope_str.len > 0) {
        stderr.writeAll(" ") catch return;
        Style.empty.foreground(Color.cyan).dim().renderAnsi(.truecolor, stderr) catch return;
        stderr.print("({s})", .{scope_str}) catch return;
        Style.renderReset(stderr) catch return;
    }

    stderr.writeAll(" ") catch return;

    // Write message
    stderr.print(format, args) catch return;
    stderr.writeByte('\n') catch return;
    stderr.flush() catch return;
}

/// Creates a scoped logger with Rich formatting
pub fn scopedLog(comptime scope: @TypeOf(.enum_literal)) type {
    return struct {
        pub fn debug(comptime format: []const u8, args: anytype) void {
            stdLogFn(.debug, scope, format, args);
        }

        pub fn info(comptime format: []const u8, args: anytype) void {
            stdLogFn(.info, scope, format, args);
        }

        pub fn warn(comptime format: []const u8, args: anytype) void {
            stdLogFn(.warn, scope, format, args);
        }

        pub fn err(comptime format: []const u8, args: anytype) void {
            stdLogFn(.err, scope, format, args);
        }
    };
}

// Tests
test "Level.toString" {
    try std.testing.expectEqualStrings("DEBUG", Level.debug.toString());
    try std.testing.expectEqualStrings("INFO", Level.info.toString());
    try std.testing.expectEqualStrings("WARN", Level.warn.toString());
    try std.testing.expectEqualStrings("ERROR", Level.err.toString());
}

test "Level.toStdLevel" {
    try std.testing.expectEqual(std.log.Level.debug, Level.debug.toStdLevel());
    try std.testing.expectEqual(std.log.Level.info, Level.info.toStdLevel());
    try std.testing.expectEqual(std.log.Level.warn, Level.warn.toStdLevel());
    try std.testing.expectEqual(std.log.Level.err, Level.err.toStdLevel());
}

test "Level.fromStdLevel" {
    try std.testing.expectEqual(Level.debug, Level.fromStdLevel(.debug));
    try std.testing.expectEqual(Level.info, Level.fromStdLevel(.info));
    try std.testing.expectEqual(Level.warn, Level.fromStdLevel(.warn));
    try std.testing.expectEqual(Level.err, Level.fromStdLevel(.err));
}

test "LevelStyles.styleFor" {
    const styles = LevelStyles.default;
    try std.testing.expect(styles.styleFor(.debug).hasAttribute(.dim));
    try std.testing.expect(styles.styleFor(.warn).hasAttribute(.bold));
    try std.testing.expect(styles.styleFor(.err).hasAttribute(.bold));
}

test "LogRecord.init" {
    const io = std.Options.debug_io;
    const record = LogRecord.init(io, .info, "test message");
    try std.testing.expectEqual(Level.info, record.level);
    try std.testing.expectEqualStrings("test message", record.message);
    try std.testing.expect(record.timestamp > 0);
}

test "LogRecord.withPath" {
    const io = std.Options.debug_io;
    const record = LogRecord.init(io, .debug, "msg").withPath("src/main.zig");
    try std.testing.expectEqualStrings("src/main.zig", record.path.?);
}

test "LogRecord.withLine" {
    const io = std.Options.debug_io;
    const record = LogRecord.init(io, .debug, "msg").withLine(42);
    try std.testing.expectEqual(@as(u32, 42), record.line.?);
}

test "RichHandler.init" {
    const io = std.Options.debug_io;
    const allocator = std.testing.allocator;
    var handler = RichHandler.init(io, allocator, std.testing.environ);
    defer handler.deinit();

    try std.testing.expectEqual(Level.debug, handler.min_level);
}

test "RichHandler.setMinLevel" {
    const io = std.Options.debug_io;
    const allocator = std.testing.allocator;
    var handler = RichHandler.init(io, allocator, std.testing.environ);
    defer handler.deinit();

    _ = handler.setMinLevel(.warn);
    try std.testing.expectEqual(Level.warn, handler.min_level);
}

test "FormatOptions defaults" {
    const opts = FormatOptions{};
    try std.testing.expect(opts.show_timestamp);
    try std.testing.expect(opts.show_level);
    try std.testing.expect(!opts.show_path);
    try std.testing.expectEqual(FormatOptions.TimestampFormat.time_only, opts.timestamp_format);
}

test "scopedLog creates scoped logger" {
    const log = scopedLog(.mymodule);
    // Just verify it compiles - actual output goes to stderr
    _ = log;
}

// ============================================================================
// Traceback Support - Formatted stack traces with syntax highlighting
// ============================================================================

/// A single frame in a stack trace
pub const StackFrame = struct {
    /// Source file path (may be null for built-in functions)
    file: ?[]const u8 = null,
    /// Line number (0 means unknown)
    line: u32 = 0,
    /// Column number (0 means unknown)
    column: u32 = 0,
    /// Function or symbol name
    function: ?[]const u8 = null,
    /// Address of the frame
    address: ?usize = null,

    pub fn format(self: StackFrame, allocator: std.mem.Allocator) ![]u8 {
        var allocating: std.Io.Writer.Allocating = .init(allocator);
        defer allocating.deinit();
        const writer = &allocating.writer;

        if (self.function) |func| {
            try writer.writeAll(func);
        } else {
            try writer.writeAll("<unknown>");
        }

        if (self.file) |file| {
            try writer.print(" at {s}", .{file});
            if (self.line > 0) {
                try writer.print(":{d}", .{self.line});
                if (self.column > 0) {
                    try writer.print(":{d}", .{self.column});
                }
            }
        }

        if (self.address) |addr| {
            try writer.print(" (0x{x})", .{addr});
        }

        return allocating.toOwnedSlice();
    }
};

/// Styling configuration for traceback rendering
pub const TracebackTheme = struct {
    /// Style for the error header (e.g., "error: ...")
    error_style: Style = Style.empty.foreground(Color.red).bold(),
    /// Style for the "Traceback" title
    title_style: Style = Style.empty.bold(),
    /// Style for file paths
    file_style: Style = Style.empty.foreground(Color.cyan),
    /// Style for line numbers
    line_number_style: Style = Style.empty.foreground(Color.yellow),
    /// Style for function names
    function_style: Style = Style.empty.foreground(Color.green),
    /// Style for the highlighted source line
    highlight_style: Style = Style.empty.foreground(Color.red),
    /// Style for frame index numbers
    frame_index_style: Style = Style.empty.dim(),
    /// Style for context lines (non-highlighted source)
    context_style: Style = Style.empty.dim(),
    /// Style for the error message
    message_style: Style = Style.empty.foreground(Color.red),
    /// Style for address/pointer values
    address_style: Style = Style.empty.dim(),

    pub const default: TracebackTheme = .{};

    pub const minimal: TracebackTheme = .{
        .error_style = Style.empty.foreground(Color.red),
        .title_style = Style.empty,
        .file_style = Style.empty,
        .line_number_style = Style.empty,
        .function_style = Style.empty,
        .highlight_style = Style.empty.bold(),
        .frame_index_style = Style.empty,
        .context_style = Style.empty,
        .message_style = Style.empty.foreground(Color.red),
        .address_style = Style.empty,
    };
};

/// Options for traceback rendering
pub const TracebackOptions = struct {
    /// Number of source lines to show before the error line
    context_before: u8 = 2,
    /// Number of source lines to show after the error line
    context_after: u8 = 2,
    /// Whether to show line numbers in source context
    show_line_numbers: bool = true,
    /// Whether to show frame addresses
    show_addresses: bool = false,
    /// Whether to show a decorative box around the traceback
    show_box: bool = true,
    /// Maximum number of frames to display (0 = unlimited)
    max_frames: u8 = 0,
    /// Whether to suppress frames from std library
    suppress_std: bool = false,
    /// Whether to attempt to read and display source code
    show_source: bool = true,
    /// Theme for styling
    theme: TracebackTheme = TracebackTheme.default,
    /// Whether to apply syntax highlighting to source code snippets
    syntax_highlighting: bool = true,
    /// Syntax theme for code highlighting (only used when syntax_highlighting is true)
    syntax_theme: SyntaxTheme = SyntaxTheme.default,
};

/// A formatted traceback with stack frames and optional source context
pub const Traceback = struct {
    io: std.Io,
    allocator: std.mem.Allocator,
    /// The error or exception message
    message: ?[]const u8 = null,
    /// The error name/type
    error_name: ?[]const u8 = null,
    /// Stack frames (most recent first)
    frames: std.ArrayList(StackFrame),
    /// Rendering options
    options: TracebackOptions = .{},
    /// Cached source lines (file path -> lines)
    source_cache: std.StringHashMap([]const []const u8),

    pub fn init(io: std.Io, allocator: std.mem.Allocator) Traceback {
        return .{
            .io = io,
            .allocator = allocator,
            .frames = std.ArrayList(StackFrame).empty,
            .source_cache = std.StringHashMap([]const []const u8).init(allocator),
        };
    }

    pub fn deinit(self: *Traceback) void {
        // Free cached source lines
        var it = self.source_cache.iterator();
        while (it.next()) |entry| {
            for (entry.value_ptr.*) |line| {
                self.allocator.free(line);
            }
            self.allocator.free(entry.value_ptr.*);
            self.allocator.free(entry.key_ptr.*);
        }
        self.source_cache.deinit();
        self.frames.deinit(self.allocator);
    }

    pub fn withMessage(self: Traceback, msg: []const u8) Traceback {
        var t = self;
        t.message = msg;
        return t;
    }

    pub fn withErrorName(self: Traceback, name: []const u8) Traceback {
        var t = self;
        t.error_name = name;
        return t;
    }

    pub fn withOptions(self: Traceback, opts: TracebackOptions) Traceback {
        var t = self;
        t.options = opts;
        return t;
    }

    /// Add a stack frame
    pub fn addFrame(self: *Traceback, frame: StackFrame) !void {
        try self.frames.append(self.allocator, frame);
    }

    /// Capture the current stack trace from this call site
    pub fn captureCurrentTrace(self: *Traceback, skip_frames: usize) !void {
        var stack_trace = std.builtin.StackTrace{
            .instruction_addresses = undefined,
            .index = 0,
        };
        var addresses: [32]usize = undefined;
        stack_trace.instruction_addresses = &addresses;

        std.debug.captureStackTrace(@returnAddress(), &stack_trace);

        const debug_info = std.debug.getSelfDebugInfo() catch return;

        var frame_idx: usize = 0;
        for (stack_trace.instruction_addresses[0..stack_trace.index]) |addr| {
            if (addr == 0) break;

            // Skip requested frames
            if (frame_idx < skip_frames) {
                frame_idx += 1;
                continue;
            }

            var frame = StackFrame{ .address = addr };

            // Try to get symbol information
            if (debug_info.getSymbolAtAddress(self.allocator, addr)) |symbol| {
                if (symbol.symbol_name) |name| {
                    frame.function = name;
                }
                if (symbol.source_location) |loc| {
                    frame.file = loc.file_name;
                    frame.line = @intCast(loc.line);
                    frame.column = @intCast(loc.column);
                }
            } else |_| {}

            try self.addFrame(frame);
            frame_idx += 1;
        }
    }

    /// Parse a Zig stack trace string into frames
    pub fn parseZigTrace(self: *Traceback, trace_str: []const u8) !void {
        var lines = std.mem.splitScalar(u8, trace_str, '\n');

        while (lines.next()) |line| {
            const trimmed = std.mem.trim(u8, line, " \t\r");
            if (trimmed.len == 0) continue;

            // Try to parse different Zig trace formats
            if (parseZigFrameLine(trimmed)) |frame| {
                try self.addFrame(frame);
            }
        }
    }

    fn parseZigFrameLine(line: []const u8) ?StackFrame {
        // Format 1: "file.zig:line:col: 0xaddr in function_name"
        // Format 2: "file.zig:line:col"
        // Format 3: "0xaddr in function_name (file.zig)"

        var frame = StackFrame{};

        // Check for address prefix (0x...)
        if (std.mem.startsWith(u8, line, "0x")) {
            const space_idx = std.mem.indexOfScalar(u8, line, ' ') orelse return null;
            frame.address = std.fmt.parseInt(usize, line[2..space_idx], 16) catch null;

            const rest = line[space_idx + 1 ..];
            if (std.mem.startsWith(u8, rest, "in ")) {
                const func_start = rest[3..];
                const paren_idx = std.mem.indexOfScalar(u8, func_start, '(');
                if (paren_idx) |idx| {
                    frame.function = std.mem.trim(u8, func_start[0..idx], " ");
                    // Parse file from parentheses
                    const file_part = func_start[idx + 1 ..];
                    const close_paren = std.mem.indexOfScalar(u8, file_part, ')') orelse file_part.len;
                    const file_info = file_part[0..close_paren];
                    parseFileLocation(file_info, &frame);
                } else {
                    frame.function = func_start;
                }
            }
            return frame;
        }

        // Try parsing as "file:line:col" format
        parseFileLocation(line, &frame);
        if (frame.file != null) {
            // Look for function after the location
            const in_idx = std.mem.indexOf(u8, line, " in ");
            if (in_idx) |idx| {
                frame.function = std.mem.trim(u8, line[idx + 4 ..], " ");
            }
            return frame;
        }

        return null;
    }

    fn parseFileLocation(text: []const u8, frame: *StackFrame) void {
        // Parse "file.zig:line:col" or "file.zig:line"
        var parts = std.mem.splitScalar(u8, text, ':');
        const file_part = parts.next() orelse return;

        // Validate it looks like a file path
        if (std.mem.indexOf(u8, file_part, ".") == null) return;

        frame.file = file_part;

        if (parts.next()) |line_str| {
            frame.line = std.fmt.parseInt(u32, line_str, 10) catch 0;

            if (parts.next()) |col_str| {
                frame.column = std.fmt.parseInt(u32, col_str, 10) catch 0;
            }
        }
    }

    /// Read source lines for a file and cache them
    fn getSourceLines(self: *Traceback, file_path: []const u8) ?[]const []const u8 {
        // Check cache first
        if (self.source_cache.get(file_path)) |lines| {
            return lines;
        }

        // Try to read the file
        const file = std.Io.Dir.cwd().openFile(self.io, file_path, .{}) catch return null;
        defer file.close(self.io);

        var read_buf: [4096]u8 = undefined;
        var file_reader = file.reader(self.io, &read_buf);
        const content = file_reader.interface.allocRemaining(self.allocator, .limited64(1024 * 1024)) catch return null;
        defer self.allocator.free(content);

        // Split into lines
        var line_list: std.ArrayList([]const u8) = .empty;
        var iter = std.mem.splitScalar(u8, content, '\n');
        while (iter.next()) |line| {
            const duped = self.allocator.dupe(u8, line) catch return null;
            line_list.append(self.allocator, duped) catch {
                self.allocator.free(duped);
                return null;
            };
        }

        const lines = line_list.toOwnedSlice(self.allocator) catch return null;

        // Cache with duped key
        const key_copy = self.allocator.dupe(u8, file_path) catch {
            for (lines) |line| {
                self.allocator.free(line);
            }
            self.allocator.free(lines);
            return null;
        };
        self.source_cache.put(key_copy, lines) catch {
            self.allocator.free(key_copy);
            for (lines) |line| {
                self.allocator.free(line);
            }
            self.allocator.free(lines);
            return null;
        };

        return lines;
    }

    /// Render the traceback to segments
    pub fn render(self: *Traceback, max_width: usize, allocator: std.mem.Allocator) ![]Segment {
        var segments: std.ArrayList(Segment) = .empty;
        const theme = self.options.theme;

        // Title line
        const title = "Traceback (most recent call last):";
        const title_copy = try allocator.dupe(u8, title);
        try segments.append(allocator, Segment.styled(title_copy, theme.title_style));
        try segments.append(allocator, Segment.line());

        // Determine frames to display
        var frames_to_show = self.frames.items;
        if (self.options.max_frames > 0 and frames_to_show.len > self.options.max_frames) {
            frames_to_show = frames_to_show[0..self.options.max_frames];
        }

        // Render each frame (in reverse order - most recent last for Python-style)
        var frame_idx: usize = frames_to_show.len;
        while (frame_idx > 0) {
            frame_idx -= 1;
            const frame = frames_to_show[frame_idx];

            // Skip std library frames if requested
            if (self.options.suppress_std) {
                if (frame.file) |file| {
                    if (std.mem.indexOf(u8, file, "std/") != null) continue;
                }
            }

            try self.renderFrame(&segments, allocator, frame, frames_to_show.len - 1 - frame_idx);
        }

        // Error message
        if (self.error_name != null or self.message != null) {
            try segments.append(allocator, Segment.line());

            if (self.error_name) |name| {
                const name_copy = try allocator.dupe(u8, name);
                try segments.append(allocator, Segment.styled(name_copy, theme.error_style));
            }

            if (self.error_name != null and self.message != null) {
                const sep = try allocator.dupe(u8, ": ");
                try segments.append(allocator, Segment.styled(sep, theme.error_style));
            }

            if (self.message) |msg| {
                const msg_copy = try allocator.dupe(u8, msg);
                try segments.append(allocator, Segment.styled(msg_copy, theme.message_style));
            }
            try segments.append(allocator, Segment.line());
        }

        // Wrap in a box if requested
        if (self.options.show_box) {
            const frame_segments = try segments.toOwnedSlice(allocator);

            var panel = Panel.fromSegments(allocator, frame_segments)
                .withBorderStyle(theme.error_style)
                .withTitle("Error")
                .withTitleStyle(theme.error_style);
            panel.box_style = box.BoxStyle.rounded;

            return panel.render(max_width, allocator);
        }

        return segments.toOwnedSlice(allocator);
    }

    fn renderFrame(
        self: *Traceback,
        segments: *std.ArrayList(Segment),
        allocator: std.mem.Allocator,
        frame: StackFrame,
        index: usize,
    ) !void {
        const theme = self.options.theme;

        // Frame index
        var idx_buf: [16]u8 = undefined;
        const idx_str = std.fmt.bufPrint(&idx_buf, "  #{d} ", .{index}) catch "  #? ";
        const idx_copy = try allocator.dupe(u8, idx_str);
        try segments.append(allocator, Segment.styled(idx_copy, theme.frame_index_style));

        // Function name
        if (frame.function) |func| {
            const func_copy = try allocator.dupe(u8, func);
            try segments.append(allocator, Segment.styled(func_copy, theme.function_style));
        } else {
            const unknown = try allocator.dupe(u8, "<unknown>");
            try segments.append(allocator, Segment.styled(unknown, theme.function_style));
        }

        try segments.append(allocator, Segment.line());

        // File location
        if (frame.file) |file| {
            const indent = try allocator.dupe(u8, "     ");
            try segments.append(allocator, Segment.plain(indent));

            const file_copy = try allocator.dupe(u8, file);
            try segments.append(allocator, Segment.styled(file_copy, theme.file_style));

            if (frame.line > 0) {
                const colon = try allocator.dupe(u8, ":");
                try segments.append(allocator, Segment.plain(colon));

                var line_buf: [16]u8 = undefined;
                const line_str = std.fmt.bufPrint(&line_buf, "{d}", .{frame.line}) catch "?";
                const line_copy = try allocator.dupe(u8, line_str);
                try segments.append(allocator, Segment.styled(line_copy, theme.line_number_style));

                if (frame.column > 0) {
                    const colon2 = try allocator.dupe(u8, ":");
                    try segments.append(allocator, Segment.plain(colon2));

                    var col_buf: [16]u8 = undefined;
                    const col_str = std.fmt.bufPrint(&col_buf, "{d}", .{frame.column}) catch "?";
                    const col_copy = try allocator.dupe(u8, col_str);
                    try segments.append(allocator, Segment.styled(col_copy, theme.line_number_style));
                }
            }

            // Address
            if (self.options.show_addresses) {
                if (frame.address) |addr| {
                    var addr_buf: [32]u8 = undefined;
                    const addr_str = std.fmt.bufPrint(&addr_buf, " (0x{x})", .{addr}) catch "";
                    const addr_copy = try allocator.dupe(u8, addr_str);
                    try segments.append(allocator, Segment.styled(addr_copy, theme.address_style));
                }
            }

            try segments.append(allocator, Segment.line());

            // Source context
            if (self.options.show_source and frame.line > 0) {
                try self.renderSourceContext(segments, allocator, frame);
            }
        }
    }

    fn renderSourceContext(
        self: *Traceback,
        segments: *std.ArrayList(Segment),
        allocator: std.mem.Allocator,
        frame: StackFrame,
    ) !void {
        const theme = self.options.theme;

        const file_path = frame.file orelse return;
        const source_lines = self.getSourceLines(file_path) orelse return;

        const target_line = frame.line;
        if (target_line == 0 or target_line > source_lines.len) return;

        const start_line = if (target_line > self.options.context_before)
            target_line - self.options.context_before
        else
            1;
        const end_line = @min(target_line + self.options.context_after, @as(u32, @intCast(source_lines.len)));

        // Detect language from file path for syntax highlighting
        const language = if (self.options.syntax_highlighting)
            Language.fromFilename(file_path)
        else
            Language.plain;

        var line_num = start_line;
        while (line_num <= end_line) : (line_num += 1) {
            const line_content = source_lines[line_num - 1];
            const is_target = (line_num == target_line);

            // Gutter with line number
            const marker: []const u8 = if (is_target) " > " else "   ";
            const marker_copy = try allocator.dupe(u8, marker);
            const marker_style = if (is_target) theme.highlight_style else theme.context_style;
            try segments.append(allocator, Segment.styled(marker_copy, marker_style));

            if (self.options.show_line_numbers) {
                var line_buf: [16]u8 = undefined;
                const line_str = std.fmt.bufPrint(&line_buf, "{d:>4} | ", .{line_num}) catch "   ? | ";
                const line_copy = try allocator.dupe(u8, line_str);
                try segments.append(allocator, Segment.styled(line_copy, theme.line_number_style));
            }

            // Source line content - with or without syntax highlighting
            if (self.options.syntax_highlighting and language != .plain) {
                // Use syntax highlighting for the line
                const highlighted_segments = try self.highlightSourceLine(
                    allocator,
                    line_content,
                    language,
                );
                defer allocator.free(highlighted_segments);

                for (highlighted_segments) |seg| {
                    // Skip newlines from syntax highlighter (we add our own)
                    if (std.mem.eql(u8, seg.text, "\n")) continue;
                    if (seg.text.len == 0) continue;

                    const text_copy = try allocator.dupe(u8, seg.text);
                    // For target line, blend with highlight style to preserve syntax color
                    const style = if (is_target)
                        blendHighlightStyle(seg.style, theme.highlight_style)
                    else
                        seg.style orelse Style.empty;
                    try segments.append(allocator, Segment.styled(text_copy, style));
                }
            } else {
                // Fall back to plain text with theme styling
                const line_copy = try allocator.dupe(u8, line_content);
                const content_style = if (is_target) theme.highlight_style else theme.context_style;
                try segments.append(allocator, Segment.styled(line_copy, content_style));
            }
            try segments.append(allocator, Segment.line());

            // Column indicator for target line
            if (is_target and frame.column > 0) {
                const prefix_len: usize = 3 + (if (self.options.show_line_numbers) @as(usize, 8) else 0);
                const total_indent = prefix_len + frame.column - 1;

                const indent_str = try allocator.alloc(u8, total_indent);
                @memset(indent_str, ' ');
                try segments.append(allocator, Segment.plain(indent_str));

                const caret = try allocator.dupe(u8, "^");
                try segments.append(allocator, Segment.styled(caret, theme.highlight_style));
                try segments.append(allocator, Segment.line());
            }
        }
    }

    /// Highlight a single line of source code using syntax highlighting
    fn highlightSourceLine(
        self: *Traceback,
        allocator: std.mem.Allocator,
        line: []const u8,
        language: Language,
    ) ![]Segment {
        const syntax = Syntax.init(allocator, line)
            .withLanguage(language)
            .withTheme(self.options.syntax_theme);

        // Render without line numbers (we handle those separately)
        return syntax.render(0, allocator);
    }
};

/// Blend syntax highlighting style with the target line highlight style
/// Preserves the syntax color but adds underline/bold from highlight
fn blendHighlightStyle(syntax_style: ?Style, highlight_style: Style) Style {
    var result = syntax_style orelse Style.empty;

    // Add highlight attributes if they're set
    if (highlight_style.hasAttribute(.bold)) {
        result = result.bold();
    }
    if (highlight_style.hasAttribute(.underline)) {
        result = result.underline();
    }
    // If highlight has a background color and syntax doesn't, use it
    if (highlight_style.bgcolor != null and result.bgcolor == null) {
        result = result.background(highlight_style.bgcolor.?);
    }

    return result;
}

/// Create a traceback from the current location
pub fn traceHere(io: std.Io, allocator: std.mem.Allocator) !Traceback {
    var tb = Traceback.init(io, allocator);
    try tb.captureCurrentTrace(1); // Skip this function
    return tb;
}

/// Create a traceback from an error
pub fn traceError(io: std.Io, allocator: std.mem.Allocator, err: anyerror) !Traceback {
    var tb = Traceback.init(io, allocator);
    tb.error_name = @errorName(err);
    try tb.captureCurrentTrace(1);
    return tb;
}

// Traceback tests
test "StackFrame.format basic" {
    const allocator = std.testing.allocator;
    const frame = StackFrame{
        .file = "test.zig",
        .line = 42,
        .column = 5,
        .function = "testFunc",
    };

    const formatted = try frame.format(allocator);
    defer allocator.free(formatted);

    try std.testing.expect(std.mem.indexOf(u8, formatted, "testFunc") != null);
    try std.testing.expect(std.mem.indexOf(u8, formatted, "test.zig") != null);
    try std.testing.expect(std.mem.indexOf(u8, formatted, "42") != null);
}

test "StackFrame.format with address" {
    const allocator = std.testing.allocator;
    const frame = StackFrame{
        .function = "main",
        .address = 0x12345,
    };

    const formatted = try frame.format(allocator);
    defer allocator.free(formatted);

    try std.testing.expect(std.mem.indexOf(u8, formatted, "main") != null);
    try std.testing.expect(std.mem.indexOf(u8, formatted, "0x12345") != null);
}

test "StackFrame.format unknown function" {
    const allocator = std.testing.allocator;
    const frame = StackFrame{};

    const formatted = try frame.format(allocator);
    defer allocator.free(formatted);

    try std.testing.expect(std.mem.indexOf(u8, formatted, "<unknown>") != null);
}

test "Traceback.init and deinit" {
    const io = std.Options.debug_io;
    const allocator = std.testing.allocator;
    var tb = Traceback.init(io, allocator);
    defer tb.deinit();

    try tb.addFrame(.{ .function = "test", .line = 1 });
    try std.testing.expectEqual(@as(usize, 1), tb.frames.items.len);
}

test "Traceback.withMessage" {
    const io = std.Options.debug_io;
    const allocator = std.testing.allocator;
    var tb = Traceback.init(io, allocator).withMessage("Something went wrong");
    defer tb.deinit();

    try std.testing.expectEqualStrings("Something went wrong", tb.message.?);
}

test "Traceback.withErrorName" {
    const io = std.Options.debug_io;
    const allocator = std.testing.allocator;
    var tb = Traceback.init(io, allocator).withErrorName("OutOfMemory");
    defer tb.deinit();

    try std.testing.expectEqualStrings("OutOfMemory", tb.error_name.?);
}

test "Traceback.parseZigTrace basic" {
    const io = std.Options.debug_io;
    const allocator = std.testing.allocator;
    var tb = Traceback.init(io, allocator);
    defer tb.deinit();

    const trace =
        \\src/main.zig:42:5
        \\src/lib.zig:100:10
    ;

    try tb.parseZigTrace(trace);

    try std.testing.expect(tb.frames.items.len >= 2);
    try std.testing.expectEqualStrings("src/main.zig", tb.frames.items[0].file.?);
    try std.testing.expectEqual(@as(u32, 42), tb.frames.items[0].line);
}

test "TracebackTheme.default" {
    const theme = TracebackTheme.default;
    try std.testing.expect(theme.error_style.hasAttribute(.bold));
    try std.testing.expect(theme.title_style.hasAttribute(.bold));
}

test "TracebackTheme.minimal" {
    const theme = TracebackTheme.minimal;
    try std.testing.expect(!theme.title_style.hasAttribute(.bold));
    try std.testing.expect(theme.highlight_style.hasAttribute(.bold));
}

test "TracebackOptions defaults" {
    const opts = TracebackOptions{};
    try std.testing.expectEqual(@as(u8, 2), opts.context_before);
    try std.testing.expectEqual(@as(u8, 2), opts.context_after);
    try std.testing.expect(opts.show_line_numbers);
    try std.testing.expect(opts.show_box);
}

test "Traceback.render basic" {
    const io = std.Options.debug_io;
    const allocator = std.testing.allocator;
    var tb = Traceback.init(io, allocator);
    defer tb.deinit();

    tb = tb.withMessage("Test error").withErrorName("TestError");
    tb.options.show_box = false;
    tb.options.show_source = false;

    try tb.addFrame(.{ .function = "main", .file = "main.zig", .line = 10 });

    const segments = try tb.render(80, allocator);
    defer {
        for (segments) |seg| {
            if (seg.text.len > 0 and !std.mem.eql(u8, seg.text, "\n")) {
                allocator.free(seg.text);
            }
        }
        allocator.free(segments);
    }

    try std.testing.expect(segments.len > 0);

    // Verify we have some content
    var found_traceback = false;
    var found_error = false;
    for (segments) |seg| {
        if (std.mem.indexOf(u8, seg.text, "Traceback") != null) found_traceback = true;
        if (std.mem.indexOf(u8, seg.text, "TestError") != null) found_error = true;
    }
    try std.testing.expect(found_traceback);
    try std.testing.expect(found_error);
}

test "TracebackOptions.syntax_highlighting defaults" {
    const opts = TracebackOptions{};
    try std.testing.expect(opts.syntax_highlighting);
    try std.testing.expect(opts.syntax_theme.keyword_style.hasAttribute(.bold));
}

test "TracebackOptions.syntax_highlighting can be disabled" {
    const opts = TracebackOptions{ .syntax_highlighting = false };
    try std.testing.expect(!opts.syntax_highlighting);
}

test "TracebackOptions.syntax_theme customization" {
    const opts = TracebackOptions{
        .syntax_theme = SyntaxTheme.monokai,
    };
    try std.testing.expect(opts.syntax_theme.background_color != null);
}

test "blendHighlightStyle preserves syntax color" {
    const syntax_style = Style.empty.foreground(Color.green);
    const highlight_style = Style.empty.bold();

    const blended = blendHighlightStyle(syntax_style, highlight_style);

    // Should have the syntax color
    try std.testing.expect(blended.color != null);
    try std.testing.expectEqual(Color.green, blended.color.?);
    // Should also be bold from highlight
    try std.testing.expect(blended.hasAttribute(.bold));
}

test "blendHighlightStyle adds highlight attributes" {
    const syntax_style = Style.empty.foreground(Color.cyan);
    const highlight_style = Style.empty.underline().bold();

    const blended = blendHighlightStyle(syntax_style, highlight_style);

    try std.testing.expect(blended.hasAttribute(.bold));
    try std.testing.expect(blended.hasAttribute(.underline));
}

test "blendHighlightStyle uses highlight background when syntax has none" {
    const syntax_style = Style.empty.foreground(Color.magenta);
    const highlight_style = Style.empty.background(Color.red);

    const blended = blendHighlightStyle(syntax_style, highlight_style);

    try std.testing.expect(blended.bgcolor != null);
    try std.testing.expectEqual(Color.red, blended.bgcolor.?);
}

test "blendHighlightStyle handles null syntax style" {
    const highlight_style = Style.empty.bold().underline();

    const blended = blendHighlightStyle(null, highlight_style);

    try std.testing.expect(blended.hasAttribute(.bold));
    try std.testing.expect(blended.hasAttribute(.underline));
}

test "Traceback.highlightSourceLine basic" {
    const io = std.Options.debug_io;
    const allocator = std.testing.allocator;
    var tb = Traceback.init(io, allocator);
    defer tb.deinit();

    const segments = try tb.highlightSourceLine(
        allocator,
        "const x = 42;",
        .zig,
    );
    defer allocator.free(segments);

    try std.testing.expect(segments.len > 0);

    // Should have produced some styled segments
    var has_styled = false;
    for (segments) |seg| {
        if (seg.style != null and !std.mem.eql(u8, seg.text, "\n")) {
            has_styled = true;
            break;
        }
    }
    try std.testing.expect(has_styled);
}

test "Traceback.highlightSourceLine with custom theme" {
    const io = std.Options.debug_io;
    const allocator = std.testing.allocator;
    var tb = Traceback.init(io, allocator);
    defer tb.deinit();

    tb.options.syntax_theme = SyntaxTheme.monokai;

    const segments = try tb.highlightSourceLine(
        allocator,
        "pub fn main() void {}",
        .zig,
    );
    defer allocator.free(segments);

    try std.testing.expect(segments.len > 0);
}
