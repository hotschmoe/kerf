const std = @import("std");
const builtin = @import("builtin");
const Style = @import("style.zig").Style;
const Segment = @import("segment.zig").Segment;
const segment = @import("segment.zig");
const Text = @import("text.zig").Text;
const ColorSystem = @import("color.zig").ColorSystem;
const Color = @import("color.zig").Color;
const terminal = @import("terminal.zig");
const markup = @import("markup.zig");
const Theme = @import("theme.zig").Theme;

pub const PagerOptions = struct {
    use_external: bool = true,
    external_command: ?[]const u8 = null,
    prompt: []const u8 = ":",
    quit_keys: []const u8 = "qQ",
    scroll_lines: u16 = 1,
};

pub const Pager = struct {
    allocator: std.mem.Allocator,
    io: std.Io,
    options: PagerOptions,
    terminal_height: u16,
    terminal_width: u16,
    is_tty: bool,

    pub fn init(allocator: std.mem.Allocator, console: *const Console) Pager {
        return initWithOptions(allocator, console, .{});
    }

    pub fn initWithOptions(allocator: std.mem.Allocator, console: *const Console, options: PagerOptions) Pager {
        return .{
            .allocator = allocator,
            .io = console.io,
            .options = options,
            .terminal_height = console.height(),
            .terminal_width = console.width(),
            .is_tty = console.isTty(),
        };
    }

    pub fn page(self: *Pager, content: []const u8) !void {
        if (!self.is_tty) {
            var buf: [4096]u8 = undefined;
            var stdout = std.Io.File.stdout().writer(self.io, &buf);
            try stdout.interface.writeAll(content);
            try stdout.interface.flush();
            return;
        }

        if (self.options.use_external) {
            if (self.tryExternalPager(content)) return;
        }

        try self.internalPager(content);
    }

    pub fn pageSegments(self: *Pager, segments: []const Segment) !void {
        const text = try segment.joinText(segments, self.allocator);
        defer self.allocator.free(text);
        try self.page(text);
    }

    fn tryExternalPager(self: *const Pager, content: []const u8) bool {
        if (builtin.os.tag == .windows) {
            return tryPagerCommand("more", content);
        }

        if (self.options.external_command) |cmd| {
            if (tryPagerCommand(cmd, content)) return true;
        }

        for ([_][]const u8{ "less", "more" }) |pager_cmd| {
            if (tryPagerCommand(pager_cmd, content)) return true;
        }

        return false;
    }

    fn internalPager(self: *Pager, content: []const u8) !void {
        var out_buf: [4096]u8 = undefined;
        var stdout = std.Io.File.stdout().writer(self.io, &out_buf);
        var in_buf: [64]u8 = undefined;
        var stdin_reader = std.Io.File.stdin().readerStreaming(self.io, &in_buf);

        const lines = try self.splitLines(content);
        defer self.allocator.free(lines);

        if (lines.len == 0) return;

        const page_size = self.terminal_height - 1;
        var current_line: usize = 0;

        while (current_line < lines.len) {
            const end_line = @min(current_line + page_size, lines.len);

            for (lines[current_line..end_line]) |line| {
                try stdout.interface.writeAll(line);
                try stdout.interface.writeByte('\n');
            }

            current_line = end_line;

            if (current_line >= lines.len) break;

            const remaining = lines.len - current_line;
            try stdout.interface.print("{s}({d} more lines, press Enter to continue, q to quit)", .{ self.options.prompt, remaining });
            try stdout.interface.flush();

            const ch = stdin_reader.interface.takeByte() catch break;

            try stdout.interface.writeByte('\r');
            for (0..self.terminal_width) |_| {
                try stdout.interface.writeByte(' ');
            }
            try stdout.interface.writeByte('\r');
            try stdout.interface.flush();

            if (std.mem.indexOfScalar(u8, self.options.quit_keys, ch) != null) {
                break;
            }
        }
    }

    fn splitLines(self: *Pager, content: []const u8) ![]const []const u8 {
        var lines_list: std.ArrayList([]const u8) = .empty;

        var iter = std.mem.splitScalar(u8, content, '\n');
        while (iter.next()) |line| {
            try lines_list.append(self.allocator, line);
        }

        return lines_list.toOwnedSlice(self.allocator);
    }
};

fn tryPagerCommand(cmd: []const u8, content: []const u8) bool {
    var child = std.process.Child.init(&.{cmd}, std.heap.page_allocator);
    child.stdin_behavior = .Pipe;

    child.spawn() catch return false;

    if (child.stdin) |stdin| {
        stdin.writeAll(content) catch {};
        stdin.close();
        child.stdin = null;
    }

    _ = child.wait() catch return false;
    return true;
}

pub const LogLevel = enum {
    debug,
    info,
    warn,
    err,

    pub fn toString(self: LogLevel) []const u8 {
        return switch (self) {
            .debug => "DEBUG",
            .info => "INFO",
            .warn => "WARN",
            .err => "ERROR",
        };
    }

    pub fn style(self: LogLevel) Style {
        return switch (self) {
            .debug => Style.empty.dim(),
            .info => Style.empty,
            .warn => Style.empty.foreground(Color.yellow),
            .err => Style.empty.foreground(Color.red),
        };
    }
};

pub const ConsoleOptions = struct {
    width: ?u16 = null,
    height: ?u16 = null,
    color_system: ?ColorSystem = null,
    force_terminal: bool = false,
    no_color: bool = false,
    tab_size: u8 = 8,
    record: bool = false,
    theme: ?Theme = null,
};

pub const Console = struct {
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: std.process.Environ,
    options: ConsoleOptions,
    terminal_info: terminal.TerminalInfo,
    current_style: Style,
    capture_buffer: ?std.ArrayList(u8),
    write_buffer: []u8,
    write_buffer_owned: bool,
    status_line_active: bool = false,
    status_line_length: usize = 0,
    theme: ?Theme,

    const DEFAULT_BUFFER_SIZE = 4096;

    pub fn init(allocator: std.mem.Allocator, io: std.Io, environ: std.process.Environ) Console {
        return initWithOptions(allocator, io, environ, .{});
    }

    pub fn initWithOptions(allocator: std.mem.Allocator, io: std.Io, environ: std.process.Environ, options: ConsoleOptions) Console {
        const info = terminal.detect(environ);
        _ = terminal.enableVirtualTerminal();
        _ = terminal.enableUtf8();

        const buffer = allocator.alloc(u8, DEFAULT_BUFFER_SIZE) catch unreachable;

        return .{
            .allocator = allocator,
            .io = io,
            .environ = environ,
            .options = options,
            .terminal_info = info,
            .current_style = Style.empty,
            .capture_buffer = if (options.record) std.ArrayList(u8).empty else null,
            .write_buffer = buffer,
            .write_buffer_owned = true,
            .theme = options.theme,
        };
    }

    pub fn deinit(self: *Console) void {
        if (self.write_buffer_owned) {
            self.allocator.free(self.write_buffer);
        }
        if (self.capture_buffer) |*buf| {
            buf.deinit(self.allocator);
        }
    }

    pub fn width(self: Console) u16 {
        return self.options.width orelse self.terminal_info.width;
    }

    pub fn height(self: Console) u16 {
        return self.options.height orelse self.terminal_info.height;
    }

    pub fn colorSystem(self: Console) ColorSystem {
        if (self.options.no_color) return .standard;
        return self.options.color_system orelse self.terminal_info.color_system;
    }

    pub fn isTty(self: Console) bool {
        return self.options.force_terminal or self.terminal_info.is_tty;
    }

    fn getWriter(self: *Console) std.Io.File.Writer {
        return std.Io.File.stdout().writer(self.io, self.write_buffer);
    }

    pub fn print(self: *Console, text_markup: []const u8) !void {
        if (self.theme) |t| {
            var txt = try Text.fromMarkupWithTheme(self.allocator, text_markup, t);
            defer txt.deinit();
            try self.printText(txt);
        } else {
            var txt = try Text.fromMarkup(self.allocator, text_markup);
            defer txt.deinit();
            try self.printText(txt);
        }
        try self.writeLine();
    }

    /// Resolve a theme style name. Returns null if no theme or name not found.
    pub fn resolveThemeStyle(self: Console, name: []const u8) ?Style {
        if (self.theme) |t| {
            return t.get(name);
        }
        return null;
    }

    pub fn printPlain(self: *Console, text: []const u8) !void {
        var writer = self.getWriter();
        try writer.interface.writeAll(text);
        try writer.interface.flush();
        try self.writeLine();
    }

    pub fn printStyled(self: *Console, text: []const u8, style: Style) !void {
        var writer = self.getWriter();
        try self.setStyle(style, &writer.interface);
        try writer.interface.writeAll(text);
        try self.resetStyle(&writer.interface);
        try writer.interface.flush();
        try self.writeLine();
    }

    pub fn printText(self: *Console, txt: Text) !void {
        const segments = try txt.render(self.allocator);
        defer self.allocator.free(segments);

        var writer = self.getWriter();
        for (segments) |seg| {
            try self.printSegment(seg, &writer.interface);
        }
        try writer.interface.flush();
    }

    pub fn printSegments(self: *Console, segments: []const Segment) !void {
        var writer = self.getWriter();

        // Use synchronized output on TTY to reduce flicker
        if (self.isTty()) {
            try writer.interface.writeAll(terminal.sync_output_begin);
        }

        for (segments) |seg| {
            try self.printSegment(seg, &writer.interface);
        }

        if (self.isTty()) {
            try writer.interface.writeAll(terminal.sync_output_end);
        }

        try writer.interface.flush();
    }

    /// Renders and prints any type that implements render(width, allocator) -> ![]Segment.
    /// This is a convenience method for printing renderables like Panel, Table, Rule, etc.
    /// Uses an arena allocator to ensure all render allocations (including segment text) are freed.
    pub fn printRenderable(self: *Console, renderable: anytype) !void {
        var arena = std.heap.ArenaAllocator.init(self.allocator);
        defer arena.deinit();

        const segments = try renderable.render(self.width(), arena.allocator());
        try self.printSegments(segments);
    }

    fn printSegment(self: *Console, seg: Segment, writer: anytype) !void {
        if (seg.control) |ctrl| {
            try ctrl.toEscapeSequence(writer);
            return;
        }

        const has_style = if (seg.style) |s| !s.isEmpty() else false;
        if (has_style) {
            try self.setStyle(seg.style.?, writer);
        }

        try writer.writeAll(seg.text);

        if (self.capture_buffer) |*buf| {
            try buf.appendSlice(self.allocator, seg.text);
        }

        if (has_style) {
            try self.resetStyle(writer);
        }
        if (seg.style) |s| {
            if (s.link != null) {
                try Style.renderHyperlinkEnd(writer);
            }
        }
    }

    fn setStyle(self: *Console, style: Style, writer: anytype) !void {
        if (self.options.no_color) return;
        try style.renderAnsi(self.colorSystem(), writer);
        self.current_style = style;
    }

    fn resetStyle(self: *Console, writer: anytype) !void {
        if (self.options.no_color) return;
        try Style.renderReset(writer);
        self.current_style = Style.empty;
    }

    fn writeLine(self: *Console) !void {
        var writer = self.getWriter();
        try writer.interface.writeAll("\n");
        if (self.capture_buffer) |*buf| {
            try buf.append(self.allocator, '\n');
        }
        try writer.interface.flush();
    }

    pub fn rule(self: *Console, title_opt: ?[]const u8) !void {
        var writer = self.getWriter();
        const w = self.width();
        const char = "\u{2500}"; // horizontal line

        if (title_opt) |title| {
            const title_len = @import("cells.zig").cellLen(title);
            const total_rule_len = w - title_len - 2; // 2 for spaces around title
            const left_len = total_rule_len / 2;
            const right_len = total_rule_len - left_len;

            // Left side
            var i: usize = 0;
            while (i < left_len) : (i += 1) {
                try writer.interface.writeAll(char);
            }
            try writer.interface.writeAll(" ");
            try writer.interface.writeAll(title);
            try writer.interface.writeAll(" ");
            // Right side
            i = 0;
            while (i < right_len) : (i += 1) {
                try writer.interface.writeAll(char);
            }
        } else {
            var i: usize = 0;
            while (i < w) : (i += 1) {
                try writer.interface.writeAll(char);
            }
        }

        try writer.interface.writeAll("\n");
        try writer.interface.flush();
    }

    pub fn clear(self: *Console) !void {
        var writer = self.getWriter();
        try writer.interface.writeAll("\x1b[2J\x1b[H");
        try writer.interface.flush();
    }

    pub fn bell(self: *Console) !void {
        var writer = self.getWriter();
        try writer.interface.writeByte(0x07);
        try writer.interface.flush();
    }

    pub fn beginCapture(self: *Console) void {
        self.capture_buffer = std.ArrayList(u8).empty;
    }

    pub fn endCapture(self: *Console) ?[]const u8 {
        if (self.capture_buffer) |*buf| {
            const result = buf.toOwnedSlice(self.allocator) catch null;
            self.capture_buffer = null;
            return result;
        }
        return null;
    }

    pub fn exportText(segments: []const Segment, allocator: std.mem.Allocator) ![]u8 {
        return segment.joinText(segments, allocator);
    }

    pub fn exportCapture(self: *Console) ?[]u8 {
        const buf = self.capture_buffer orelse return null;
        return self.allocator.dupe(u8, buf.items) catch null;
    }

    pub fn setTitle(self: *Console, title: []const u8) !void {
        var writer = self.getWriter();
        try writer.interface.print("\x1b]0;{s}\x07", .{title});
        try writer.interface.flush();
    }

    fn writeEscapeSequence(self: *Console, sequence: []const u8) !void {
        var writer = self.getWriter();
        try writer.interface.writeAll(sequence);
        try writer.interface.flush();
    }

    pub fn showCursor(self: *Console) !void {
        try self.writeEscapeSequence("\x1b[?25h");
    }

    pub fn hideCursor(self: *Console) !void {
        try self.writeEscapeSequence("\x1b[?25l");
    }

    pub fn clearLine(self: *Console) !void {
        try self.writeEscapeSequence("\x1b[K");
    }

    pub fn enterAltScreen(self: *Console) !void {
        try self.writeEscapeSequence("\x1b[?1049h");
    }

    pub fn exitAltScreen(self: *Console) !void {
        try self.writeEscapeSequence("\x1b[?1049l");
    }

    pub fn log(self: *Console, level: LogLevel, comptime fmt: []const u8, args: anytype) !void {
        var writer = self.getWriter();

        // Get current time
        const timestamp = std.Io.Timestamp.now(self.io, .real).toSeconds();
        const epoch_seconds: std.time.epoch.EpochSeconds = .{ .secs = @intCast(timestamp) };
        const day_seconds = epoch_seconds.getDaySeconds();
        const hours = day_seconds.getHoursIntoDay();
        const minutes = day_seconds.getMinutesIntoHour();
        const seconds = day_seconds.getSecondsIntoMinute();

        // Format: [HH:MM:SS] [LEVEL] message
        try writer.interface.print("[{d:0>2}:{d:0>2}:{d:0>2}] ", .{ hours, minutes, seconds });

        // Level with styling
        const level_style = level.style();
        try self.setStyle(level_style, &writer.interface);
        try writer.interface.print("[{s}]", .{level.toString()});
        try self.resetStyle(&writer.interface);

        try writer.interface.writeAll(" ");

        // Message
        try writer.interface.print(fmt, args);
        try writer.interface.writeAll("\n");
        try writer.interface.flush();
    }

    pub fn logDebug(self: *Console, comptime fmt: []const u8, args: anytype) !void {
        try self.log(.debug, fmt, args);
    }

    pub fn logInfo(self: *Console, comptime fmt: []const u8, args: anytype) !void {
        try self.log(.info, fmt, args);
    }

    pub fn logWarn(self: *Console, comptime fmt: []const u8, args: anytype) !void {
        try self.log(.warn, fmt, args);
    }

    pub fn logErr(self: *Console, comptime fmt: []const u8, args: anytype) !void {
        try self.log(.err, fmt, args);
    }

    pub fn status(self: *Console, message: []const u8) !void {
        try self.clearStatus();
        var writer = self.getWriter();
        try writer.interface.writeAll(message);
        try writer.interface.flush();
        self.status_line_active = true;
        self.status_line_length = @import("cells.zig").cellLen(message);
    }

    pub fn statusFmt(self: *Console, comptime fmt: []const u8, args: anytype) !void {
        try self.clearStatus();
        var writer = self.getWriter();

        var buf: [256]u8 = undefined;
        const formatted = std.fmt.bufPrint(&buf, fmt, args) catch fmt;

        try writer.interface.writeAll(formatted);
        try writer.interface.flush();
        self.status_line_active = true;
        self.status_line_length = @import("cells.zig").cellLen(formatted);
    }

    pub fn clearStatus(self: *Console) !void {
        if (!self.status_line_active) return;
        var writer = self.getWriter();
        try writer.interface.writeByte('\r');
        for (0..self.status_line_length) |_| {
            try writer.interface.writeByte(' ');
        }
        try writer.interface.writeByte('\r');
        try writer.interface.flush();
        self.status_line_active = false;
        self.status_line_length = 0;
    }

    pub fn pager(self: *Console) Pager {
        return Pager.init(self.allocator, self);
    }

    pub fn pagerWithOptions(self: *Console, options: PagerOptions) Pager {
        return Pager.initWithOptions(self.allocator, self, options);
    }

    // Input prompt functionality

    pub const InputOptions = struct {
        default: ?[]const u8 = null,
        password: bool = false,
        show_default: bool = true,
        validator: ?*const fn ([]const u8) InputValidationResult = null,
        max_length: ?usize = null,
    };

    pub const InputValidationResult = union(enum) {
        valid,
        invalid: []const u8,
    };

    pub const InputError = error{
        ValidationFailed,
        InputCancelled,
        EndOfStream,
        OutOfMemory,
    };

    pub fn input(self: *Console, prompt_text: []const u8) InputError![]u8 {
        return self.inputWithOptions(prompt_text, .{});
    }

    pub fn inputWithOptions(self: *Console, prompt_text: []const u8, options: InputOptions) InputError![]u8 {
        var writer = self.getWriter();
        writer.interface.writeAll(prompt_text) catch return InputError.OutOfMemory;
        self.writeDefaultSuffix(&writer.interface, options) catch return InputError.OutOfMemory;
        return self.readAndProcessInput(options);
    }

    pub fn prompt(self: *Console, prompt_markup: []const u8) InputError![]u8 {
        return self.promptWithOptions(prompt_markup, .{});
    }

    pub fn promptWithOptions(self: *Console, prompt_markup: []const u8, options: InputOptions) InputError![]u8 {
        var txt = Text.fromMarkup(self.allocator, prompt_markup) catch return InputError.OutOfMemory;
        defer txt.deinit();

        const segments = txt.render(self.allocator) catch return InputError.OutOfMemory;
        defer self.allocator.free(segments);

        var writer = self.getWriter();
        for (segments) |seg| {
            self.printSegment(seg, &writer.interface) catch return InputError.OutOfMemory;
        }

        self.writeDefaultSuffix(&writer.interface, options) catch return InputError.OutOfMemory;
        return self.readAndProcessInput(options);
    }

    fn writeDefaultSuffix(self: *Console, writer: anytype, options: InputOptions) !void {
        _ = self;
        if (options.show_default) {
            if (options.default) |default| {
                try writer.print(" [{s}]", .{default});
            }
        }
        try writer.writeAll(": ");
        try writer.flush();
    }

    fn readAndProcessInput(self: *Console, options: InputOptions) InputError![]u8 {
        var in_buf: [4096]u8 = undefined;
        var stdin_reader = std.Io.File.stdin().readerStreaming(self.io, &in_buf);
        const line = stdin_reader.interface.takeDelimiter('\n') catch |err| {
            return switch (err) {
                error.StreamTooLong => InputError.OutOfMemory,
                error.ReadFailed => InputError.EndOfStream,
            };
        };

        if (line == null) {
            return InputError.EndOfStream;
        }

        var trimmed = line.?;
        if (trimmed.len > 0 and trimmed[trimmed.len - 1] == '\r') {
            trimmed = trimmed[0 .. trimmed.len - 1];
        }

        if (trimmed.len == 0) {
            if (options.default) |default| {
                return self.allocator.dupe(u8, default) catch return InputError.OutOfMemory;
            }
        }

        if (options.max_length) |max_len| {
            if (trimmed.len > max_len) {
                trimmed = trimmed[0..max_len];
            }
        }

        if (options.validator) |validate| {
            const result = validate(trimmed);
            switch (result) {
                .valid => {},
                .invalid => |msg| {
                    self.printValidationError(msg) catch {};
                    return InputError.ValidationFailed;
                },
            }
        }

        return self.allocator.dupe(u8, trimmed) catch return InputError.OutOfMemory;
    }

    fn printValidationError(self: *Console, msg: []const u8) !void {
        var writer = self.getWriter();
        try self.setStyle(Style.empty.foreground(Color.red), &writer.interface);
        try writer.interface.writeAll("Error: ");
        try writer.interface.writeAll(msg);
        try self.resetStyle(&writer.interface);
        try writer.interface.writeAll("\n");
        try writer.interface.flush();
    }

    pub fn printPaged(self: *Console, content: []const u8) !void {
        var p = self.pager();
        try p.page(content);
    }

    pub fn printSegmentsPaged(self: *Console, segments: []const Segment) !void {
        var p = self.pager();
        try p.pageSegments(segments);
    }

    pub fn exportHtml(segments: []const Segment, allocator: std.mem.Allocator) ![]u8 {
        var allocating: std.Io.Writer.Allocating = .init(allocator);
        defer allocating.deinit();
        const writer = &allocating.writer;

        try writer.writeAll("<pre style=\"font-family: monospace;\">");

        for (segments) |seg| {
            if (seg.control != null) continue;

            if (std.mem.eql(u8, seg.text, "\n")) {
                try writer.writeAll("<br>");
                continue;
            }

            const has_style = if (seg.style) |s| !s.isEmpty() else false;

            if (has_style) {
                try writer.writeAll("<span style=\"");
                try writeHtmlStyle(seg.style.?, writer);
                try writer.writeAll("\">");
            }

            try writeXmlEscaped(seg.text, writer);

            if (has_style) {
                try writer.writeAll("</span>");
            }
        }

        try writer.writeAll("</pre>");

        return try allocating.toOwnedSlice();
    }

    fn writeHtmlStyle(style: Style, writer: anytype) !void {
        var needs_sep = false;

        if (style.color) |c| {
            if (c.getTriplet()) |t| {
                try writer.print("color: rgb({d},{d},{d})", .{ t.r, t.g, t.b });
                needs_sep = true;
            }
        }

        if (style.bgcolor) |c| {
            if (c.getTriplet()) |t| {
                if (needs_sep) try writer.writeAll("; ");
                try writer.print("background-color: rgb({d},{d},{d})", .{ t.r, t.g, t.b });
                needs_sep = true;
            }
        }

        inline for (.{
            .{ .bold, "font-weight: bold" },
            .{ .dim, "opacity: 0.5" },
            .{ .italic, "font-style: italic" },
            .{ .underline, "text-decoration: underline" },
            .{ .strike, "text-decoration: line-through" },
            .{ .overline, "text-decoration: overline" },
            .{ .underline2, "text-decoration: underline double" },
            .{ .frame, "border: 1px solid currentColor" },
            .{ .encircle, "border: 1px solid currentColor; border-radius: 50%" },
        }) |pair| {
            if (style.hasAttribute(pair[0])) {
                if (needs_sep) try writer.writeAll("; ");
                try writer.writeAll(pair[1]);
                needs_sep = true;
            }
        }
    }

    fn writeXmlEscaped(text: []const u8, writer: anytype) !void {
        for (text) |c| {
            switch (c) {
                '<' => try writer.writeAll("&lt;"),
                '>' => try writer.writeAll("&gt;"),
                '&' => try writer.writeAll("&amp;"),
                '"' => try writer.writeAll("&quot;"),
                '\'' => try writer.writeAll("&apos;"),
                else => try writer.writeByte(c),
            }
        }
    }

    pub const SvgOptions = struct {
        title: ?[]const u8 = null,
        font_family: []const u8 = "Fira Code,Monaco,Consolas,Liberation Mono,monospace",
        font_size: u16 = 14,
        line_height: f32 = 1.4,
        font_aspect_ratio: f32 = 0.61,
        background_color: []const u8 = "#282a36",
        foreground_color: []const u8 = "#f8f8f2",
        padding: u16 = 20,
        chrome: bool = true,
        chrome_color: []const u8 = "#1e1f29",
        term_width: ?u16 = null,
    };

    pub fn exportSvg(segments: []const Segment, allocator: std.mem.Allocator, options: SvgOptions) ![]u8 {
        const cells = @import("cells.zig");
        var allocating: std.Io.Writer.Allocating = .init(allocator);
        errdefer allocating.deinit();
        const writer = &allocating.writer;

        // Parse segments into lines for layout calculation
        var lines: std.ArrayList(std.ArrayList(LineSegment)) = .empty;
        var current_line: std.ArrayList(LineSegment) = .empty;
        var max_line_width: usize = 0;
        var current_line_width: usize = 0;

        for (segments) |seg| {
            if (seg.control != null) continue;

            if (std.mem.eql(u8, seg.text, "\n")) {
                if (current_line_width > max_line_width) {
                    max_line_width = current_line_width;
                }
                try lines.append(allocator, current_line);
                current_line = .empty;
                current_line_width = 0;
                continue;
            }

            const seg_width = cells.cellLen(seg.text);
            try current_line.append(allocator, .{ .text = seg.text, .style = seg.style, .width = seg_width });
            current_line_width += seg_width;
        }

        // Append the last line if not empty
        if (current_line.items.len > 0 or lines.items.len == 0) {
            if (current_line_width > max_line_width) {
                max_line_width = current_line_width;
            }
            try lines.append(allocator, current_line);
        }
        defer {
            for (lines.items) |*line| {
                line.deinit(allocator);
            }
            lines.deinit(allocator);
        }

        const num_lines = lines.items.len;
        const term_width = options.term_width orelse @as(u16, @intCast(@max(max_line_width, 40)));

        // Calculate dimensions
        const char_height: f32 = @floatFromInt(options.font_size);
        const char_width: f32 = char_height * options.font_aspect_ratio;
        const line_height: f32 = char_height * options.line_height;

        const chrome_height: f32 = if (options.chrome) 36.0 else 0.0;
        const terminal_width: f32 = @as(f32, @floatFromInt(term_width)) * char_width + @as(f32, @floatFromInt(options.padding * 2));
        const terminal_height: f32 = @as(f32, @floatFromInt(num_lines)) * line_height + @as(f32, @floatFromInt(options.padding * 2));

        const total_width: f32 = terminal_width;
        const total_height: f32 = terminal_height + chrome_height;

        // Write SVG header
        try writer.print(
            \\<svg xmlns="http://www.w3.org/2000/svg" width="{d:.0}" height="{d:.0}" viewBox="0 0 {d:.0} {d:.0}">
            \\<style>
            \\  .terminal {{ font-family: {s}; font-size: {d}px; }}
            \\  .terminal text {{ fill: {s}; }}
            \\</style>
            \\
        , .{ total_width, total_height, total_width, total_height, options.font_family, options.font_size, options.foreground_color });

        // Chrome (terminal window decorations)
        if (options.chrome) {
            try writer.print(
                \\<rect width="{d:.0}" height="{d:.0}" rx="8" fill="{s}"/>
                \\<rect y="36" width="{d:.0}" height="{d:.0}" fill="{s}"/>
            , .{ total_width, total_height, options.chrome_color, total_width, terminal_height, options.background_color });

            // Traffic light buttons
            try writer.writeAll(
                \\<circle cx="20" cy="18" r="6" fill="#ff5f56"/>
                \\<circle cx="40" cy="18" r="6" fill="#ffbd2e"/>
                \\<circle cx="60" cy="18" r="6" fill="#27c93f"/>
                \\
            );

            // Title in chrome
            if (options.title) |title| {
                const title_x = total_width / 2;
                try writer.print(
                    \\<text x="{d:.0}" y="23" text-anchor="middle" fill="{s}" font-family="{s}" font-size="13">{s}</text>
                    \\
                , .{ title_x, options.foreground_color, options.font_family, title });
            }
        } else {
            try writer.print(
                \\<rect width="{d:.0}" height="{d:.0}" fill="{s}"/>
                \\
            , .{ total_width, total_height, options.background_color });
        }

        // Terminal content group
        const content_y = chrome_height + @as(f32, @floatFromInt(options.padding));
        const content_x: f32 = @floatFromInt(options.padding);
        try writer.print(
            \\<g class="terminal" transform="translate({d:.1},{d:.1})">
            \\
        , .{ content_x, content_y });

        // Render each line
        for (lines.items, 0..) |line, line_idx| {
            const y = @as(f32, @floatFromInt(line_idx)) * line_height + char_height;
            var x: f32 = 0;

            for (line.items) |line_seg| {
                const has_style = if (line_seg.style) |s| !s.isEmpty() else false;

                if (has_style) {
                    try writer.writeAll("<tspan ");
                    try writeSvgStyle(line_seg.style.?, writer);
                    try writer.print(" x=\"{d:.1}\" y=\"{d:.1}\">", .{ x, y });
                } else {
                    try writer.print("<text x=\"{d:.1}\" y=\"{d:.1}\">", .{ x, y });
                }

                try writeXmlEscaped(line_seg.text, writer);

                if (has_style) {
                    try writer.writeAll("</tspan>");
                } else {
                    try writer.writeAll("</text>");
                }

                x += @as(f32, @floatFromInt(line_seg.width)) * char_width;
            }
        }

        try writer.writeAll("</g>\n</svg>");

        return try allocating.toOwnedSlice();
    }

    const LineSegment = struct {
        text: []const u8,
        style: ?Style,
        width: usize,
    };

    fn writeSvgStyle(style: Style, writer: anytype) !void {
        try writer.writeAll("style=\"");
        var needs_sep = false;

        if (style.color) |c| {
            if (c.getTriplet()) |t| {
                try writer.print("fill:rgb({d},{d},{d})", .{ t.r, t.g, t.b });
                needs_sep = true;
            }
        }

        inline for (.{
            .{ .bold, "font-weight:bold" },
            .{ .dim, "opacity:0.5" },
            .{ .italic, "font-style:italic" },
            .{ .underline, "text-decoration:underline" },
            .{ .strike, "text-decoration:line-through" },
            .{ .overline, "text-decoration:overline" },
            .{ .underline2, "text-decoration:underline" },
            .{ .frame, "stroke:currentColor;stroke-width:1" },
            .{ .encircle, "stroke:currentColor;stroke-width:1" },
        }) |pair| {
            if (style.hasAttribute(pair[0])) {
                if (needs_sep) try writer.writeByte(';');
                try writer.writeAll(pair[1]);
                needs_sep = true;
            }
        }

        try writer.writeByte('"');
    }
};

// Tests
test "Console.init" {
    const allocator = std.testing.allocator;
    var console = Console.init(allocator, std.testing.io, std.testing.environ);
    defer console.deinit();

    try std.testing.expect(console.width() > 0);
    try std.testing.expect(console.height() > 0);
}

test "Console.initWithOptions" {
    const allocator = std.testing.allocator;
    var console = Console.initWithOptions(allocator, std.testing.io, std.testing.environ, .{
        .width = 120,
        .height = 40,
        .color_system = .truecolor,
    });
    defer console.deinit();

    try std.testing.expectEqual(@as(u16, 120), console.width());
    try std.testing.expectEqual(@as(u16, 40), console.height());
    try std.testing.expectEqual(ColorSystem.truecolor, console.colorSystem());
}

test "Console.no_color option" {
    const allocator = std.testing.allocator;
    var console = Console.initWithOptions(allocator, std.testing.io, std.testing.environ, .{
        .no_color = true,
        .color_system = .truecolor,
    });
    defer console.deinit();

    try std.testing.expectEqual(ColorSystem.standard, console.colorSystem());
}

test "Console.exportText" {
    const allocator = std.testing.allocator;
    const segments = [_]Segment{
        Segment.plain("Hello"),
        Segment.plain(" "),
        Segment.plain("World"),
    };

    const text = try Console.exportText(&segments, allocator);
    defer allocator.free(text);

    try std.testing.expectEqualStrings("Hello World", text);
}

test "LogLevel.toString" {
    try std.testing.expectEqualStrings("DEBUG", LogLevel.debug.toString());
    try std.testing.expectEqualStrings("INFO", LogLevel.info.toString());
    try std.testing.expectEqualStrings("WARN", LogLevel.warn.toString());
    try std.testing.expectEqualStrings("ERROR", LogLevel.err.toString());
}

test "LogLevel.style" {
    const debug_style = LogLevel.debug.style();
    try std.testing.expect(debug_style.hasAttribute(.dim));

    const err_style = LogLevel.err.style();
    try std.testing.expect(err_style.color != null);
}

test "Console.status" {
    const allocator = std.testing.allocator;
    var console = Console.init(allocator, std.testing.io, std.testing.environ);
    defer console.deinit();

    try std.testing.expect(!console.status_line_active);
}

test "Console.clearStatus when inactive" {
    const allocator = std.testing.allocator;
    var console = Console.init(allocator, std.testing.io, std.testing.environ);
    defer console.deinit();

    try console.clearStatus();
    try std.testing.expect(!console.status_line_active);
}

test "Console.exportHtml plain" {
    const allocator = std.testing.allocator;
    const segments = [_]Segment{Segment.plain("Hello")};

    const html = try Console.exportHtml(&segments, allocator);
    defer allocator.free(html);

    try std.testing.expect(std.mem.indexOf(u8, html, "Hello") != null);
    try std.testing.expect(std.mem.indexOf(u8, html, "<pre") != null);
    try std.testing.expect(std.mem.indexOf(u8, html, "</pre>") != null);
}

test "Console.exportHtml styled" {
    const allocator = std.testing.allocator;
    const segments = [_]Segment{Segment.styled("Bold", Style.empty.bold())};

    const html = try Console.exportHtml(&segments, allocator);
    defer allocator.free(html);

    try std.testing.expect(std.mem.indexOf(u8, html, "font-weight: bold") != null);
    try std.testing.expect(std.mem.indexOf(u8, html, "Bold") != null);
}

test "Console.exportHtml escapes HTML" {
    const allocator = std.testing.allocator;
    const segments = [_]Segment{Segment.plain("<script>")};

    const html = try Console.exportHtml(&segments, allocator);
    defer allocator.free(html);

    try std.testing.expect(std.mem.indexOf(u8, html, "&lt;script&gt;") != null);
    try std.testing.expect(std.mem.indexOf(u8, html, "<script>") == null);
}

test "Console.exportHtml newlines" {
    const allocator = std.testing.allocator;
    const segments = [_]Segment{
        Segment.plain("Line1"),
        Segment.line(),
        Segment.plain("Line2"),
    };

    const html = try Console.exportHtml(&segments, allocator);
    defer allocator.free(html);

    try std.testing.expect(std.mem.indexOf(u8, html, "<br>") != null);
}

test "Console.exportSvg plain" {
    const allocator = std.testing.allocator;
    const segments = [_]Segment{Segment.plain("Hello")};

    const svg = try Console.exportSvg(&segments, allocator, .{});
    defer allocator.free(svg);

    try std.testing.expect(std.mem.indexOf(u8, svg, "<svg") != null);
    try std.testing.expect(std.mem.indexOf(u8, svg, "</svg>") != null);
    try std.testing.expect(std.mem.indexOf(u8, svg, "Hello") != null);
}

test "Console.exportSvg styled" {
    const allocator = std.testing.allocator;
    const segments = [_]Segment{Segment.styled("Bold", Style.empty.bold())};

    const svg = try Console.exportSvg(&segments, allocator, .{});
    defer allocator.free(svg);

    try std.testing.expect(std.mem.indexOf(u8, svg, "Bold") != null);
    try std.testing.expect(std.mem.indexOf(u8, svg, "font-weight:bold") != null or
        std.mem.indexOf(u8, svg, "font-weight: bold") != null);
}

test "Console.exportSvg escapes XML" {
    const allocator = std.testing.allocator;
    const segments = [_]Segment{Segment.plain("<script>")};

    const svg = try Console.exportSvg(&segments, allocator, .{});
    defer allocator.free(svg);

    try std.testing.expect(std.mem.indexOf(u8, svg, "&lt;script&gt;") != null);
}

test "Console.exportSvg with title" {
    const allocator = std.testing.allocator;
    const segments = [_]Segment{Segment.plain("Content")};

    const svg = try Console.exportSvg(&segments, allocator, .{ .title = "My Title" });
    defer allocator.free(svg);

    try std.testing.expect(std.mem.indexOf(u8, svg, "My Title") != null);
}

test "Console.exportSvg multiple lines" {
    const allocator = std.testing.allocator;
    const segments = [_]Segment{
        Segment.plain("Line1"),
        Segment.line(),
        Segment.plain("Line2"),
    };

    const svg = try Console.exportSvg(&segments, allocator, .{});
    defer allocator.free(svg);

    try std.testing.expect(std.mem.indexOf(u8, svg, "Line1") != null);
    try std.testing.expect(std.mem.indexOf(u8, svg, "Line2") != null);
}

test "Console.exportSvg with colors" {
    const allocator = std.testing.allocator;
    const segments = [_]Segment{Segment.styled("Red", Style.empty.foreground(Color.fromRgb(255, 0, 0)))};

    const svg = try Console.exportSvg(&segments, allocator, .{});
    defer allocator.free(svg);

    try std.testing.expect(std.mem.indexOf(u8, svg, "Red") != null);
    try std.testing.expect(std.mem.indexOf(u8, svg, "rgb(255,0,0)") != null or
        std.mem.indexOf(u8, svg, "#ff0000") != null);
}

test "Pager.init" {
    const allocator = std.testing.allocator;
    var console = Console.initWithOptions(allocator, std.testing.io, std.testing.environ, .{
        .width = 80,
        .height = 24,
    });
    defer console.deinit();

    const p = Pager.init(allocator, &console);
    try std.testing.expectEqual(@as(u16, 24), p.terminal_height);
    try std.testing.expectEqual(@as(u16, 80), p.terminal_width);
}

test "Pager.initWithOptions" {
    const allocator = std.testing.allocator;
    var console = Console.initWithOptions(allocator, std.testing.io, std.testing.environ, .{
        .width = 100,
        .height = 40,
    });
    defer console.deinit();

    const p = Pager.initWithOptions(allocator, &console, .{
        .use_external = false,
        .prompt = ">>",
        .quit_keys = "xX",
    });
    try std.testing.expectEqual(@as(u16, 40), p.terminal_height);
    try std.testing.expectEqual(false, p.options.use_external);
    try std.testing.expectEqualStrings(">>", p.options.prompt);
    try std.testing.expectEqualStrings("xX", p.options.quit_keys);
}

test "Pager.splitLines" {
    const allocator = std.testing.allocator;
    var console = Console.initWithOptions(allocator, std.testing.io, std.testing.environ, .{
        .width = 80,
        .height = 24,
    });
    defer console.deinit();

    var p = Pager.init(allocator, &console);

    const lines = try p.splitLines("line1\nline2\nline3");
    defer allocator.free(lines);

    try std.testing.expectEqual(@as(usize, 3), lines.len);
    try std.testing.expectEqualStrings("line1", lines[0]);
    try std.testing.expectEqualStrings("line2", lines[1]);
    try std.testing.expectEqualStrings("line3", lines[2]);
}

test "Pager.splitLines empty" {
    const allocator = std.testing.allocator;
    var console = Console.initWithOptions(allocator, std.testing.io, std.testing.environ, .{
        .width = 80,
        .height = 24,
    });
    defer console.deinit();

    var p = Pager.init(allocator, &console);

    const lines = try p.splitLines("");
    defer allocator.free(lines);

    try std.testing.expectEqual(@as(usize, 1), lines.len);
}

test "Console.pager" {
    const allocator = std.testing.allocator;
    var console = Console.initWithOptions(allocator, std.testing.io, std.testing.environ, .{
        .width = 80,
        .height = 24,
    });
    defer console.deinit();

    const p = console.pager();
    try std.testing.expectEqual(@as(u16, 24), p.terminal_height);
    try std.testing.expectEqual(@as(u16, 80), p.terminal_width);
}

test "Console.pagerWithOptions" {
    const allocator = std.testing.allocator;
    var console = Console.initWithOptions(allocator, std.testing.io, std.testing.environ, .{
        .width = 80,
        .height = 24,
    });
    defer console.deinit();

    const p = console.pagerWithOptions(.{ .use_external = false });
    try std.testing.expectEqual(false, p.options.use_external);
}

test "PagerOptions defaults" {
    const opts = PagerOptions{};
    try std.testing.expectEqual(true, opts.use_external);
    try std.testing.expectEqual(@as(?[]const u8, null), opts.external_command);
    try std.testing.expectEqualStrings(":", opts.prompt);
    try std.testing.expectEqualStrings("qQ", opts.quit_keys);
    try std.testing.expectEqual(@as(u16, 1), opts.scroll_lines);
}

test "InputOptions defaults" {
    const opts = Console.InputOptions{};
    try std.testing.expectEqual(@as(?[]const u8, null), opts.default);
    try std.testing.expectEqual(false, opts.password);
    try std.testing.expectEqual(true, opts.show_default);
    try std.testing.expectEqual(@as(?*const fn ([]const u8) Console.InputValidationResult, null), opts.validator);
    try std.testing.expectEqual(@as(?usize, null), opts.max_length);
}

test "InputValidationResult valid" {
    const result = Console.InputValidationResult{ .valid = {} };
    try std.testing.expect(result == .valid);
}

test "InputValidationResult invalid" {
    const result = Console.InputValidationResult{ .invalid = "must be non-empty" };
    switch (result) {
        .valid => unreachable,
        .invalid => |msg| {
            try std.testing.expectEqualStrings("must be non-empty", msg);
        },
    }
}

fn testNonEmptyValidator(input: []const u8) Console.InputValidationResult {
    if (input.len == 0) {
        return .{ .invalid = "Input cannot be empty" };
    }
    return .valid;
}

fn testMinLengthValidator(input: []const u8) Console.InputValidationResult {
    if (input.len < 3) {
        return .{ .invalid = "Input must be at least 3 characters" };
    }
    return .valid;
}

test "validator function type" {
    // Test that our validator functions have the correct signature
    const validator: *const fn ([]const u8) Console.InputValidationResult = &testNonEmptyValidator;
    const result = validator("test");
    try std.testing.expect(result == .valid);

    const empty_result = validator("");
    switch (empty_result) {
        .valid => unreachable,
        .invalid => |msg| {
            try std.testing.expectEqualStrings("Input cannot be empty", msg);
        },
    }
}

test "InputOptions with validator" {
    const opts = Console.InputOptions{
        .default = "default_value",
        .validator = &testMinLengthValidator,
        .max_length = 100,
    };

    try std.testing.expectEqualStrings("default_value", opts.default.?);
    try std.testing.expect(opts.validator != null);
    try std.testing.expectEqual(@as(usize, 100), opts.max_length.?);
}
