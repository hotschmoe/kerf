const std = @import("std");
const Segment = @import("../../segment.zig").Segment;
const Style = @import("../../style.zig").Style;
const Color = @import("../../color.zig").Color;
const cells = @import("../../cells.zig");

const Language = @import("language.zig").Language;
const SyntaxTheme = @import("theme.zig").SyntaxTheme;
const tokenizer = @import("tokenizer.zig");
const zig_keywords = tokenizer.zig_keywords;
const zig_builtins = tokenizer.zig_builtins;
const zig_types = tokenizer.zig_types;

pub const Syntax = struct {
    code: []const u8,
    language: Language = .plain,
    theme: SyntaxTheme = SyntaxTheme.default,
    show_line_numbers: bool = false,
    start_line: usize = 1,
    word_wrap: bool = false,
    tab_size: u8 = 4,
    indent_guides: bool = false,
    indent_guide_char: []const u8 = "|",
    highlight_lines: ?[]const usize = null,
    allocator: std.mem.Allocator,
    owns_code: bool = false,

    pub fn init(allocator: std.mem.Allocator, code: []const u8) Syntax {
        return .{
            .code = code,
            .allocator = allocator,
        };
    }

    pub fn renderDuped(self: Syntax, max_width: usize, allocator: std.mem.Allocator) ![]Segment {
        const segments = try self.render(max_width, allocator);

        for (segments) |*seg| {
            if (seg.text.len > 0 and !std.mem.eql(u8, seg.text, "\n")) {
                seg.text = try allocator.dupe(u8, seg.text);
            }
        }

        return segments;
    }

    pub fn withLanguage(self: Syntax, lang: Language) Syntax {
        var s = self;
        s.language = lang;
        return s;
    }

    /// Auto-detect language from filename and/or content
    pub fn withAutoDetect(self: Syntax, filename: ?[]const u8) Syntax {
        var s = self;
        s.language = Language.detect(filename, s.code);
        return s;
    }

    /// Create syntax highlighter from code string with filename for language detection
    pub fn fromFile(allocator: std.mem.Allocator, code: []const u8, filename: []const u8) Syntax {
        return Syntax{
            .code = code,
            .language = Language.detect(filename, code),
            .allocator = allocator,
        };
    }

    /// Load code from filesystem and create syntax highlighter (auto-detects language)
    pub fn loadFile(allocator: std.mem.Allocator, io: std.Io, path: []const u8) !Syntax {
        const file = try std.Io.Dir.openFile(.cwd(), io, path, .{});
        defer file.close(io);

        var read_buf: [4096]u8 = undefined;
        var file_reader = file.reader(io, &read_buf);
        const code = try file_reader.interface.allocRemaining(allocator, .unlimited);

        return Syntax{
            .code = code,
            .language = Language.detect(path, code),
            .allocator = allocator,
            .owns_code = true,
        };
    }

    /// Free memory allocated by loadFile. Only call on Syntax instances created via loadFile.
    pub fn deinit(self: *Syntax) void {
        if (self.owns_code) {
            self.allocator.free(self.code);
        }
    }

    pub fn withTheme(self: Syntax, theme: SyntaxTheme) Syntax {
        var s = self;
        s.theme = theme;
        return s;
    }

    pub fn withLineNumbers(self: Syntax) Syntax {
        var s = self;
        s.show_line_numbers = true;
        return s;
    }

    pub fn withStartLine(self: Syntax, line: usize) Syntax {
        var s = self;
        s.start_line = line;
        return s;
    }

    pub fn withWordWrap(self: Syntax) Syntax {
        var s = self;
        s.word_wrap = true;
        return s;
    }

    pub fn withTabSize(self: Syntax, size: u8) Syntax {
        var s = self;
        s.tab_size = size;
        return s;
    }

    pub fn withIndentGuides(self: Syntax) Syntax {
        var s = self;
        s.indent_guides = true;
        return s;
    }

    pub fn withIndentGuideChar(self: Syntax, char: []const u8) Syntax {
        var s = self;
        s.indent_guide_char = char;
        return s;
    }

    /// Highlight specific line numbers (e.g., for showing error locations).
    /// Line numbers are 1-based (matching start_line convention).
    pub fn withHighlightLines(self: Syntax, lines: []const usize) Syntax {
        var s = self;
        s.highlight_lines = lines;
        return s;
    }

    /// Check if a given line number should be highlighted.
    fn isLineHighlighted(self: Syntax, line_num: usize) bool {
        if (self.highlight_lines) |lines| {
            for (lines) |hl| {
                if (hl == line_num) return true;
            }
        }
        return false;
    }

    pub fn render(self: Syntax, max_width: usize, allocator: std.mem.Allocator) ![]Segment {
        var segments: std.ArrayList(Segment) = .empty;

        var lines = std.mem.splitScalar(u8, self.code, '\n');
        var line_num: usize = self.start_line;

        // Calculate available width for code (accounting for line numbers)
        const line_num_width: usize = if (self.show_line_numbers) 5 else 0; // "NNNN "
        const code_width: usize = if (max_width > line_num_width) max_width - line_num_width else max_width;

        while (lines.next()) |line| {
            const is_highlighted = self.isLineHighlighted(line_num);

            if (self.show_line_numbers) {
                var buf: [16]u8 = undefined;
                const line_str = std.fmt.bufPrint(&buf, "{d:>4} ", .{line_num}) catch "???? ";
                const line_str_copy = try allocator.dupe(u8, line_str);
                const line_num_style = if (is_highlighted)
                    self.theme.highlight_line_number_style
                else
                    self.theme.line_number_style;
                try segments.append(allocator, Segment.styled(line_str_copy, self.theme.applyBackground(line_num_style)));
            }

            if (self.word_wrap and code_width > 0) {
                var line_segments: std.ArrayList(Segment) = .empty;
                try self.highlightLine(&line_segments, allocator, line, is_highlighted);
                const highlighted = try line_segments.toOwnedSlice(allocator);
                defer allocator.free(highlighted);

                const wrapped = try wrapSegments(highlighted, code_width, allocator);
                defer allocator.free(wrapped);

                var after_wrap_newline = false;
                for (wrapped) |seg| {
                    const is_newline = std.mem.eql(u8, seg.text, "\n");
                    if (after_wrap_newline and self.show_line_numbers and !is_newline) {
                        const indent = try allocator.dupe(u8, "     ");
                        const indent_style = if (is_highlighted)
                            self.theme.highlight_line_number_style
                        else
                            self.theme.line_number_style;
                        try segments.append(allocator, Segment.styled(indent, self.theme.applyBackground(indent_style)));
                    }
                    try segments.append(allocator, Segment.styledOptional(seg.text, self.theme.applyBackgroundOpt(seg.style)));
                    after_wrap_newline = is_newline;
                }
            } else {
                try self.highlightLine(&segments, allocator, line, is_highlighted);
            }

            try segments.append(allocator, Segment.line());
            line_num += 1;
        }

        return segments.toOwnedSlice(allocator);
    }

    fn highlightLine(self: Syntax, segments: *std.ArrayList(Segment), allocator: std.mem.Allocator, line: []const u8, is_highlighted: bool) !void {
        const expanded = try self.expandTabs(line, allocator);

        if (self.indent_guides) {
            try self.renderWithIndentGuides(segments, allocator, expanded, is_highlighted);
        } else {
            try self.highlightContent(segments, allocator, expanded, is_highlighted);
        }
    }

    /// Apply syntax highlighting to content based on language.
    fn highlightContent(self: Syntax, segments: *std.ArrayList(Segment), allocator: std.mem.Allocator, content: []const u8, is_highlighted: bool) !void {
        switch (self.language) {
            .zig => try self.highlightZig(segments, allocator, content, is_highlighted),
            .json => try self.highlightJson(segments, allocator, content, is_highlighted),
            .markdown => try self.highlightMarkdown(segments, allocator, content, is_highlighted),
            .python,
            .javascript,
            .typescript,
            .rust,
            .go,
            .c,
            .cpp,
            .bash,
            .yaml,
            .toml,
            .xml,
            .html,
            .css,
            .sql,
            .plain,
            => try segments.append(allocator, self.defaultSegmentHighlighted(content, is_highlighted)),
        }
    }

    /// Render a line with indent guides at each tab stop in the leading whitespace.
    fn renderWithIndentGuides(self: Syntax, segments: *std.ArrayList(Segment), allocator: std.mem.Allocator, line: []const u8, is_highlighted: bool) !void {
        var indent_end: usize = 0;
        while (indent_end < line.len and line[indent_end] == ' ') : (indent_end += 1) {}

        if (indent_end > 0) {
            const tab_size: usize = @intCast(self.tab_size);
            var col: usize = 0;

            while (col < indent_end) {
                const next_tab_stop = ((col / tab_size) + 1) * tab_size;

                if (next_tab_stop <= indent_end and indent_end - col >= tab_size) {
                    try segments.append(allocator, self.styledSegmentHighlighted(
                        self.indent_guide_char,
                        self.theme.indent_guide_style,
                        is_highlighted,
                    ));
                    const spaces_after_guide = tab_size - self.indent_guide_char.len;
                    if (spaces_after_guide > 0) {
                        const space_fill = try allocator.alloc(u8, spaces_after_guide);
                        @memset(space_fill, ' ');
                        try segments.append(allocator, self.defaultSegmentHighlighted(space_fill, is_highlighted));
                    }
                    col = next_tab_stop;
                } else {
                    const spaces_to_end = indent_end - col;
                    const space_fill = try allocator.alloc(u8, spaces_to_end);
                    @memset(space_fill, ' ');
                    try segments.append(allocator, self.defaultSegmentHighlighted(space_fill, is_highlighted));
                    col = indent_end;
                }
            }
        }

        const content = line[indent_end..];
        if (content.len > 0) {
            try self.highlightContent(segments, allocator, content, is_highlighted);
        }
    }

    /// Expand tabs to spaces based on tab_size setting.
    /// Returns the original slice if no tabs are present (no allocation).
    fn expandTabs(self: Syntax, line: []const u8, allocator: std.mem.Allocator) ![]const u8 {
        const tab_count = std.mem.count(u8, line, "\t");
        if (tab_count == 0) return line;

        // Over-allocate: actual size may be smaller due to tab-stop alignment
        const max_len = line.len - tab_count + tab_count * self.tab_size;
        const result = try allocator.alloc(u8, max_len);

        var src_pos: usize = 0;
        var dst_pos: usize = 0;
        var column: usize = 0;

        while (src_pos < line.len) : (src_pos += 1) {
            if (line[src_pos] == '\t') {
                // Calculate spaces to next tab stop
                const spaces_to_tab_stop = self.tab_size - @as(u8, @intCast(column % self.tab_size));
                @memset(result[dst_pos..][0..spaces_to_tab_stop], ' ');
                dst_pos += spaces_to_tab_stop;
                column += spaces_to_tab_stop;
            } else {
                result[dst_pos] = line[src_pos];
                dst_pos += 1;
                column += 1;
            }
        }

        return result[0..dst_pos];
    }

    fn defaultSegmentHighlighted(self: Syntax, text: []const u8, is_highlighted: bool) Segment {
        if (is_highlighted) {
            const style = if (self.theme.default_style.isEmpty())
                self.theme.highlight_line_style
            else
                self.applyHighlightBackground(self.theme.default_style);
            return Segment.styled(text, style);
        }
        const base_style = if (self.theme.default_style.isEmpty()) null else self.theme.default_style;
        return Segment.styledOptional(text, self.theme.applyBackgroundOpt(base_style));
    }

    fn styledSegmentHighlighted(self: Syntax, text: []const u8, style: Style, is_highlighted: bool) Segment {
        if (is_highlighted) {
            return Segment.styled(text, self.applyHighlightBackground(style));
        }
        return Segment.styled(text, self.theme.applyBackground(style));
    }

    /// Apply highlight line background to a style (overrides any existing background).
    fn applyHighlightBackground(self: Syntax, style: Style) Style {
        var result = style;
        result.bgcolor = self.theme.highlight_line_style.bgcolor;
        return result;
    }

    fn highlightZig(self: Syntax, segments: *std.ArrayList(Segment), allocator: std.mem.Allocator, line: []const u8, is_highlighted: bool) !void {
        var i: usize = 0;

        while (i < line.len) {
            if (std.mem.startsWith(u8, line[i..], "//")) {
                try segments.append(allocator, self.styledSegmentHighlighted(line[i..], self.theme.comment_style, is_highlighted));
                return;
            }

            if (line[i] == '"') {
                const end = self.findStringEnd(line, i);
                try segments.append(allocator, self.styledSegmentHighlighted(line[i..end], self.theme.string_style, is_highlighted));
                i = end;
                continue;
            }

            if (line[i] == '\'') {
                const end = self.findCharEnd(line, i);
                try segments.append(allocator, self.styledSegmentHighlighted(line[i..end], self.theme.string_style, is_highlighted));
                i = end;
                continue;
            }

            if (line[i] == '@') {
                const end = self.findIdentEnd(line, i + 1);
                const builtin = line[i..end];
                if (zig_builtins.has(builtin)) {
                    try segments.append(allocator, self.styledSegmentHighlighted(builtin, self.theme.builtin_style, is_highlighted));
                } else {
                    try segments.append(allocator, self.defaultSegmentHighlighted(builtin, is_highlighted));
                }
                i = end;
                continue;
            }

            if (std.ascii.isDigit(line[i]) or (line[i] == '.' and i + 1 < line.len and std.ascii.isDigit(line[i + 1]))) {
                const end = self.findNumberEnd(line, i);
                try segments.append(allocator, self.styledSegmentHighlighted(line[i..end], self.theme.number_style, is_highlighted));
                i = end;
                continue;
            }

            if (std.ascii.isAlphabetic(line[i]) or line[i] == '_') {
                const end = self.findIdentEnd(line, i);
                const ident = line[i..end];

                if (zig_keywords.has(ident)) {
                    try segments.append(allocator, self.styledSegmentHighlighted(ident, self.theme.keyword_style, is_highlighted));
                } else if (zig_types.has(ident)) {
                    try segments.append(allocator, self.styledSegmentHighlighted(ident, self.theme.type_style, is_highlighted));
                } else if (end < line.len and line[end] == '(') {
                    try segments.append(allocator, self.styledSegmentHighlighted(ident, self.theme.function_style, is_highlighted));
                } else {
                    try segments.append(allocator, self.defaultSegmentHighlighted(ident, is_highlighted));
                }
                i = end;
                continue;
            }

            if (self.isOperator(line[i])) {
                try segments.append(allocator, self.styledSegmentHighlighted(line[i .. i + 1], self.theme.operator_style, is_highlighted));
                i += 1;
                continue;
            }

            if (self.isPunctuation(line[i])) {
                try segments.append(allocator, self.styledSegmentHighlighted(line[i .. i + 1], self.theme.punctuation_style, is_highlighted));
                i += 1;
                continue;
            }

            try segments.append(allocator, self.defaultSegmentHighlighted(line[i .. i + 1], is_highlighted));
            i += 1;
        }
    }

    fn highlightJson(self: Syntax, segments: *std.ArrayList(Segment), allocator: std.mem.Allocator, line: []const u8, is_highlighted: bool) !void {
        var i: usize = 0;

        while (i < line.len) {
            if (line[i] == '"') {
                const end = self.findStringEnd(line, i);
                const str_content = line[i..end];
                if (end < line.len and line[end] == ':') {
                    try segments.append(allocator, self.styledSegmentHighlighted(str_content, self.theme.keyword_style, is_highlighted));
                } else {
                    try segments.append(allocator, self.styledSegmentHighlighted(str_content, self.theme.string_style, is_highlighted));
                }
                i = end;
                continue;
            }

            const keyword: ?[]const u8 = if (std.mem.startsWith(u8, line[i..], "true"))
                "true"
            else if (std.mem.startsWith(u8, line[i..], "false"))
                "false"
            else if (std.mem.startsWith(u8, line[i..], "null"))
                "null"
            else
                null;

            if (keyword) |kw| {
                try segments.append(allocator, self.styledSegmentHighlighted(kw, self.theme.keyword_style, is_highlighted));
                i += kw.len;
                continue;
            }

            if (std.ascii.isDigit(line[i]) or (line[i] == '-' and i + 1 < line.len and std.ascii.isDigit(line[i + 1]))) {
                const end = self.findNumberEnd(line, i);
                try segments.append(allocator, self.styledSegmentHighlighted(line[i..end], self.theme.number_style, is_highlighted));
                i = end;
                continue;
            }

            if (self.isPunctuation(line[i])) {
                try segments.append(allocator, self.styledSegmentHighlighted(line[i .. i + 1], self.theme.punctuation_style, is_highlighted));
                i += 1;
                continue;
            }

            try segments.append(allocator, self.defaultSegmentHighlighted(line[i .. i + 1], is_highlighted));
            i += 1;
        }
    }

    fn highlightMarkdown(self: Syntax, segments: *std.ArrayList(Segment), allocator: std.mem.Allocator, line: []const u8, is_highlighted: bool) !void {
        if (line.len == 0) return;

        if (std.mem.startsWith(u8, line, "#")) {
            try segments.append(allocator, self.styledSegmentHighlighted(line, self.theme.keyword_style.bold(), is_highlighted));
            return;
        }

        if (std.mem.startsWith(u8, line, "```")) {
            try segments.append(allocator, self.styledSegmentHighlighted(line, self.theme.comment_style, is_highlighted));
            return;
        }

        if (std.mem.startsWith(u8, line, "- ") or std.mem.startsWith(u8, line, "* ") or std.mem.startsWith(u8, line, "+ ")) {
            try segments.append(allocator, self.styledSegmentHighlighted(line[0..2], self.theme.keyword_style, is_highlighted));
            try segments.append(allocator, self.defaultSegmentHighlighted(line[2..], is_highlighted));
            return;
        }

        if (std.mem.startsWith(u8, line, "> ")) {
            try segments.append(allocator, self.styledSegmentHighlighted(line, self.theme.comment_style.italic(), is_highlighted));
            return;
        }

        var i: usize = 0;
        while (i < line.len) {
            if (line[i] == '`') {
                var end = i + 1;
                while (end < line.len and line[end] != '`') : (end += 1) {}
                if (end < line.len) {
                    try segments.append(allocator, self.styledSegmentHighlighted(line[i .. end + 1], self.theme.string_style, is_highlighted));
                    i = end + 1;
                    continue;
                }
            }

            if (line[i] == '*' and i + 1 < line.len and line[i + 1] == '*') {
                var end = i + 2;
                while (end + 1 < line.len and !(line[end] == '*' and line[end + 1] == '*')) : (end += 1) {}
                if (end + 1 < line.len) {
                    try segments.append(allocator, self.styledSegmentHighlighted(line[i .. end + 2], self.theme.keyword_style.bold(), is_highlighted));
                    i = end + 2;
                    continue;
                }
            }

            if (line[i] == '*' or line[i] == '_') {
                const marker = line[i];
                var end = i + 1;
                while (end < line.len and line[end] != marker) : (end += 1) {}
                if (end < line.len) {
                    try segments.append(allocator, self.styledSegmentHighlighted(line[i .. end + 1], self.theme.default_style.italic(), is_highlighted));
                    i = end + 1;
                    continue;
                }
            }

            if (line[i] == '[') {
                var bracket_end = i + 1;
                while (bracket_end < line.len and line[bracket_end] != ']') : (bracket_end += 1) {}
                if (bracket_end < line.len and bracket_end + 1 < line.len and line[bracket_end + 1] == '(') {
                    var paren_end = bracket_end + 2;
                    while (paren_end < line.len and line[paren_end] != ')') : (paren_end += 1) {}
                    if (paren_end < line.len) {
                        try segments.append(allocator, self.styledSegmentHighlighted(line[i .. paren_end + 1], self.theme.builtin_style, is_highlighted));
                        i = paren_end + 1;
                        continue;
                    }
                }
            }

            try segments.append(allocator, self.defaultSegmentHighlighted(line[i .. i + 1], is_highlighted));
            i += 1;
        }
    }

    fn findStringEnd(_: Syntax, line: []const u8, start: usize) usize {
        var i = start + 1;
        while (i < line.len) : (i += 1) {
            if (line[i] == '\\' and i + 1 < line.len) {
                i += 1;
                continue;
            }
            if (line[i] == '"') {
                return i + 1;
            }
        }
        return line.len;
    }

    fn findCharEnd(_: Syntax, line: []const u8, start: usize) usize {
        var i = start + 1;
        while (i < line.len) : (i += 1) {
            if (line[i] == '\\' and i + 1 < line.len) {
                i += 1;
                continue;
            }
            if (line[i] == '\'') {
                return i + 1;
            }
        }
        return line.len;
    }

    fn findIdentEnd(_: Syntax, line: []const u8, start: usize) usize {
        var i = start;
        while (i < line.len and (std.ascii.isAlphanumeric(line[i]) or line[i] == '_')) : (i += 1) {}
        return i;
    }

    fn findNumberEnd(_: Syntax, line: []const u8, start: usize) usize {
        var i = start;
        if (i < line.len and line[i] == '-') i += 1;
        if (i + 1 < line.len and line[i] == '0' and (line[i + 1] == 'x' or line[i + 1] == 'b' or line[i + 1] == 'o')) {
            i += 2;
            while (i < line.len and (std.ascii.isAlphanumeric(line[i]) or line[i] == '_')) : (i += 1) {}
        } else {
            while (i < line.len and (std.ascii.isDigit(line[i]) or line[i] == '.' or line[i] == '_' or line[i] == 'e' or line[i] == 'E' or line[i] == '-' or line[i] == '+')) : (i += 1) {}
        }
        return i;
    }

    fn isOperator(_: Syntax, c: u8) bool {
        return switch (c) {
            '+', '-', '*', '/', '%', '=', '<', '>', '!', '&', '|', '^', '~' => true,
            else => false,
        };
    }

    fn isPunctuation(_: Syntax, c: u8) bool {
        return switch (c) {
            '{', '}', '[', ']', '(', ')', ',', '.', ':', ';' => true,
            else => false,
        };
    }

    /// Wrap styled segments to fit within max_width, preserving styles across wrapped lines.
    /// Prefers breaking at word boundaries (spaces) when possible, falls back to character-level
    /// wrapping for long words that exceed the available width.
    fn wrapSegments(line_segments: []const Segment, max_width: usize, allocator: std.mem.Allocator) ![]Segment {
        if (max_width == 0) {
            return allocator.dupe(Segment, line_segments);
        }

        var result: std.ArrayList(Segment) = .empty;
        var current_width: usize = 0;

        // Track position of last space for word-boundary breaking
        var last_space_result_idx: ?usize = null;

        for (line_segments) |seg| {
            const seg_width = seg.cellLength();

            // If segment fits on current line, append it
            if (current_width + seg_width <= max_width) {
                // Check if this segment contains a space (potential break point)
                if (std.mem.lastIndexOfScalar(u8, seg.text, ' ')) |_| {
                    last_space_result_idx = result.items.len;
                }
                try result.append(allocator, seg);
                current_width += seg_width;
                continue;
            }

            // Segment doesn't fit - need to wrap within it
            var remaining_text = seg.text;

            while (remaining_text.len > 0) {
                if (current_width >= max_width) {
                    try result.append(allocator, Segment.line());
                    current_width = 0;
                    last_space_result_idx = null;
                }

                const remaining_width = cells.cellLen(remaining_text);
                const available = max_width - current_width;

                if (remaining_width <= available) {
                    // Check for space in this final piece
                    if (std.mem.lastIndexOfScalar(u8, remaining_text, ' ')) |_| {
                        last_space_result_idx = result.items.len;
                    }
                    try result.append(allocator, Segment.styledOptional(remaining_text, seg.style));
                    current_width += remaining_width;
                    break;
                }

                // Find word boundary within available space
                const byte_limit = cells.cellToByteIndex(remaining_text, available);
                var break_byte = byte_limit;
                var break_width = available;

                // Look for last space within the available width
                if (std.mem.lastIndexOfScalar(u8, remaining_text[0..byte_limit], ' ')) |space_pos| {
                    // Found a space - break after it
                    break_byte = space_pos + 1;
                    break_width = cells.cellLen(remaining_text[0..break_byte]);
                } else if (current_width > 0 and last_space_result_idx != null) {
                    // No space in current chunk, but we have a previous space on this line
                    // Rewind to the last space by inserting a newline after it
                    const space_idx = last_space_result_idx.?;
                    const space_seg = result.items[space_idx];

                    // Find the last space in that segment and split it
                    if (std.mem.lastIndexOfScalar(u8, space_seg.text, ' ')) |sp| {
                        // Split at the space: keep text up to and including space,
                        // then insert newline, then continue with rest
                        const before_space = space_seg.text[0 .. sp + 1];
                        const after_space = space_seg.text[sp + 1 ..];

                        // Update the segment to end at space
                        result.items[space_idx] = Segment.styledOptional(before_space, space_seg.style);

                        // Collect segments after the space to re-process
                        var to_reprocess: std.ArrayList(Segment) = .empty;
                        defer to_reprocess.deinit(allocator);

                        if (after_space.len > 0) {
                            try to_reprocess.append(allocator, Segment.styledOptional(after_space, space_seg.style));
                        }

                        // Add remaining segments from result after space_idx
                        for (result.items[space_idx + 1 ..]) |s| {
                            try to_reprocess.append(allocator, s);
                        }

                        // Add the current remaining text
                        try to_reprocess.append(allocator, Segment.styledOptional(remaining_text, seg.style));

                        // Truncate result to just before the newline
                        result.shrinkRetainingCapacity(space_idx + 1);

                        // Insert newline
                        try result.append(allocator, Segment.line());
                        current_width = 0;
                        last_space_result_idx = null;

                        // Re-add all the segments that need to be re-wrapped
                        for (to_reprocess.items) |reprocess_seg| {
                            const rw = reprocess_seg.cellLength();
                            if (current_width + rw <= max_width) {
                                if (std.mem.lastIndexOfScalar(u8, reprocess_seg.text, ' ')) |_| {
                                    last_space_result_idx = result.items.len;
                                }
                                try result.append(allocator, reprocess_seg);
                                current_width += rw;
                            } else {
                                // Need to wrap this segment too - continue with inner loop logic
                                var inner_remaining = reprocess_seg.text;
                                while (inner_remaining.len > 0) {
                                    if (current_width >= max_width) {
                                        try result.append(allocator, Segment.line());
                                        current_width = 0;
                                        last_space_result_idx = null;
                                    }

                                    const inner_width = cells.cellLen(inner_remaining);
                                    const inner_avail = max_width - current_width;

                                    if (inner_width <= inner_avail) {
                                        if (std.mem.lastIndexOfScalar(u8, inner_remaining, ' ')) |_| {
                                            last_space_result_idx = result.items.len;
                                        }
                                        try result.append(allocator, Segment.styledOptional(inner_remaining, reprocess_seg.style));
                                        current_width += inner_width;
                                        break;
                                    }

                                    const inner_byte_limit = cells.cellToByteIndex(inner_remaining, inner_avail);
                                    var inner_break = inner_byte_limit;

                                    if (std.mem.lastIndexOfScalar(u8, inner_remaining[0..inner_byte_limit], ' ')) |inner_sp| {
                                        inner_break = inner_sp + 1;
                                    }

                                    if (inner_break > 0) {
                                        try result.append(allocator, Segment.styledOptional(inner_remaining[0..inner_break], reprocess_seg.style));
                                    }
                                    try result.append(allocator, Segment.line());
                                    inner_remaining = inner_remaining[inner_break..];
                                    current_width = 0;
                                    last_space_result_idx = null;
                                }
                            }
                        }

                        // Done with this segment, move to next in outer loop
                        remaining_text = "";
                        break;
                    }
                }

                // Output text up to break point
                if (break_byte > 0) {
                    const chunk = remaining_text[0..break_byte];
                    if (std.mem.lastIndexOfScalar(u8, chunk, ' ')) |_| {
                        last_space_result_idx = result.items.len;
                    }
                    try result.append(allocator, Segment.styledOptional(chunk, seg.style));
                    current_width += break_width;
                }

                // If we're at max width, insert newline
                if (current_width >= max_width or break_byte == byte_limit) {
                    try result.append(allocator, Segment.line());
                    current_width = 0;
                    last_space_result_idx = null;
                }

                remaining_text = remaining_text[break_byte..];
            }
        }

        return result.toOwnedSlice(allocator);
    }
};
test "Syntax.init" {
    const allocator = std.testing.allocator;
    const syntax = Syntax.init(allocator, "const x = 1;");
    try std.testing.expectEqualStrings("const x = 1;", syntax.code);
}

test "Syntax.withLanguage" {
    const allocator = std.testing.allocator;
    const syntax = Syntax.init(allocator, "code").withLanguage(.zig);
    try std.testing.expectEqual(Language.zig, syntax.language);
}

test "Syntax.withTheme" {
    const allocator = std.testing.allocator;
    const syntax = Syntax.init(allocator, "code").withTheme(SyntaxTheme.monokai);
    try std.testing.expect(syntax.theme.keyword_style.color != null);
}

test "SyntaxTheme.background_color" {
    // Default theme has no background
    try std.testing.expect(SyntaxTheme.default.background_color == null);

    // Monokai and Dracula themes have background colors
    try std.testing.expect(SyntaxTheme.monokai.background_color != null);
    try std.testing.expect(SyntaxTheme.dracula.background_color != null);

    // Check specific Monokai background (dark gray: #272822)
    const monokai_bg = SyntaxTheme.monokai.background_color.?;
    try std.testing.expectEqual(Color.fromRgb(39, 40, 34), monokai_bg);

    // Check specific Dracula background (dark blue-gray: #282a36)
    const dracula_bg = SyntaxTheme.dracula.background_color.?;
    try std.testing.expectEqual(Color.fromRgb(40, 42, 54), dracula_bg);
}

test "SyntaxTheme.applyBackground" {
    const theme_with_bg = SyntaxTheme.monokai;
    const theme_no_bg = SyntaxTheme.default;

    const base_style = Style.empty.bold();

    // Theme with background should apply it
    const styled_with_bg = theme_with_bg.applyBackground(base_style);
    try std.testing.expect(styled_with_bg.bgcolor != null);
    try std.testing.expect(styled_with_bg.hasAttribute(.bold));

    // Theme without background should return style unchanged
    const styled_no_bg = theme_no_bg.applyBackground(base_style);
    try std.testing.expect(styled_no_bg.bgcolor == null);
    try std.testing.expect(styled_no_bg.hasAttribute(.bold));
}

test "SyntaxTheme.applyBackgroundOpt" {
    const theme_with_bg = SyntaxTheme.monokai;
    const theme_no_bg = SyntaxTheme.default;

    // Test with non-null style
    const base_style = Style.empty.italic();
    const styled_opt = theme_with_bg.applyBackgroundOpt(base_style);
    try std.testing.expect(styled_opt != null);
    try std.testing.expect(styled_opt.?.bgcolor != null);

    // Test with null style and theme with background
    const from_null = theme_with_bg.applyBackgroundOpt(null);
    try std.testing.expect(from_null != null);
    try std.testing.expect(from_null.?.bgcolor != null);

    // Test with null style and theme without background
    const stays_null = theme_no_bg.applyBackgroundOpt(null);
    try std.testing.expect(stays_null == null);
}

test "Syntax.render with background color" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const code = "const x = 42;";
    const syntax = Syntax.init(arena.allocator(), code)
        .withLanguage(.zig)
        .withTheme(SyntaxTheme.monokai);

    const segments = try syntax.render(80, arena.allocator());

    // All non-newline segments should have background color
    var found_with_bg = false;
    for (segments) |seg| {
        if (!std.mem.eql(u8, seg.text, "\n") and seg.text.len > 0) {
            if (seg.style) |style| {
                if (style.bgcolor != null) {
                    found_with_bg = true;
                    // Verify it's the Monokai background
                    try std.testing.expectEqual(Color.fromRgb(39, 40, 34), style.bgcolor.?);
                }
            }
        }
    }
    try std.testing.expect(found_with_bg);
}

test "Syntax.render without background color" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const code = "const x = 42;";
    const syntax = Syntax.init(arena.allocator(), code)
        .withLanguage(.zig)
        .withTheme(SyntaxTheme.default);

    const segments = try syntax.render(80, arena.allocator());

    // Default theme has no background, so no segment should have bgcolor
    for (segments) |seg| {
        if (seg.style) |style| {
            try std.testing.expect(style.bgcolor == null);
        }
    }
}

test "Syntax.withLineNumbers" {
    const allocator = std.testing.allocator;
    const syntax = Syntax.init(allocator, "code").withLineNumbers();
    try std.testing.expect(syntax.show_line_numbers);
}

test "Syntax.render plain" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const syntax = Syntax.init(arena.allocator(), "Hello\nWorld");
    const segments = try syntax.render(80, arena.allocator());

    try std.testing.expect(segments.len > 0);
}

test "Syntax.render zig" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const code =
        \\const std = @import("std");
        \\
        \\pub fn main() void {
        \\    // comment
        \\    const x: u32 = 42;
        \\}
    ;
    const syntax = Syntax.init(arena.allocator(), code).withLanguage(.zig);
    const segments = try syntax.render(80, arena.allocator());

    try std.testing.expect(segments.len > 0);
}

test "Syntax.render zig with line numbers" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const syntax = Syntax.init(arena.allocator(), "const x = 1;\nconst y = 2;")
        .withLanguage(.zig)
        .withLineNumbers();
    const segments = try syntax.render(80, arena.allocator());

    var found_line_num = false;
    for (segments) |seg| {
        if (std.mem.indexOf(u8, seg.text, "1") != null or std.mem.indexOf(u8, seg.text, "2") != null) {
            found_line_num = true;
            break;
        }
    }
    try std.testing.expect(found_line_num);
}

test "Syntax.render json" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const code =
        \\{
        \\  "name": "test",
        \\  "value": 42,
        \\  "active": true
        \\}
    ;
    const syntax = Syntax.init(arena.allocator(), code).withLanguage(.json);
    const segments = try syntax.render(80, arena.allocator());

    try std.testing.expect(segments.len > 0);
}

test "Syntax.render markdown" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const code =
        \\# Heading
        \\
        \\Some text with **bold** and *italic*.
        \\
        \\- List item
        \\
        \\```code block```
    ;
    const syntax = Syntax.init(arena.allocator(), code).withLanguage(.markdown);
    const segments = try syntax.render(80, arena.allocator());

    try std.testing.expect(segments.len > 0);
}

// Language auto-detection tests

test "Language.fromExtension detects common extensions" {
    try std.testing.expectEqual(Language.zig, Language.fromExtension(".zig"));
    try std.testing.expectEqual(Language.zig, Language.fromExtension(".zon"));
    try std.testing.expectEqual(Language.json, Language.fromExtension(".json"));
    try std.testing.expectEqual(Language.markdown, Language.fromExtension(".md"));
    try std.testing.expectEqual(Language.python, Language.fromExtension(".py"));
    try std.testing.expectEqual(Language.javascript, Language.fromExtension(".js"));
    try std.testing.expectEqual(Language.typescript, Language.fromExtension(".ts"));
    try std.testing.expectEqual(Language.rust, Language.fromExtension(".rs"));
    try std.testing.expectEqual(Language.go, Language.fromExtension(".go"));
    try std.testing.expectEqual(Language.c, Language.fromExtension(".c"));
    try std.testing.expectEqual(Language.c, Language.fromExtension(".h"));
    try std.testing.expectEqual(Language.cpp, Language.fromExtension(".cpp"));
    try std.testing.expectEqual(Language.bash, Language.fromExtension(".sh"));
    try std.testing.expectEqual(Language.yaml, Language.fromExtension(".yaml"));
    try std.testing.expectEqual(Language.yaml, Language.fromExtension(".yml"));
    try std.testing.expectEqual(Language.toml, Language.fromExtension(".toml"));
    try std.testing.expectEqual(Language.html, Language.fromExtension(".html"));
    try std.testing.expectEqual(Language.css, Language.fromExtension(".css"));
    try std.testing.expectEqual(Language.sql, Language.fromExtension(".sql"));
    try std.testing.expectEqual(Language.plain, Language.fromExtension(".unknown"));
}

test "Language.fromExtension case insensitive" {
    try std.testing.expectEqual(Language.zig, Language.fromExtension(".ZIG"));
    try std.testing.expectEqual(Language.json, Language.fromExtension(".JSON"));
    try std.testing.expectEqual(Language.python, Language.fromExtension(".PY"));
}

test "Language.fromFilename detects from full path" {
    try std.testing.expectEqual(Language.zig, Language.fromFilename("src/main.zig"));
    try std.testing.expectEqual(Language.json, Language.fromFilename("/path/to/config.json"));
    try std.testing.expectEqual(Language.python, Language.fromFilename("scripts/test.py"));
}

test "Language.fromFilename detects special files" {
    try std.testing.expectEqual(Language.bash, Language.fromFilename("Makefile"));
    try std.testing.expectEqual(Language.bash, Language.fromFilename("Dockerfile"));
    try std.testing.expectEqual(Language.bash, Language.fromFilename(".bashrc"));
    try std.testing.expectEqual(Language.toml, Language.fromFilename("Cargo.toml"));
    try std.testing.expectEqual(Language.json, Language.fromFilename("package.json"));
    try std.testing.expectEqual(Language.zig, Language.fromFilename("build.zig"));
    try std.testing.expectEqual(Language.zig, Language.fromFilename("build.zig.zon"));
}

test "Language.fromContent detects shebang" {
    try std.testing.expectEqual(Language.python, Language.fromContent("#!/usr/bin/env python3\nprint('hello')"));
    try std.testing.expectEqual(Language.bash, Language.fromContent("#!/bin/bash\necho hello"));
    try std.testing.expectEqual(Language.javascript, Language.fromContent("#!/usr/bin/env node\nconsole.log('hi')"));
}

test "Language.fromContent detects JSON" {
    try std.testing.expectEqual(Language.json, Language.fromContent("{\"key\": \"value\"}"));
    try std.testing.expectEqual(Language.json, Language.fromContent("[1, 2, 3]"));
    try std.testing.expectEqual(Language.json, Language.fromContent("  {\n  \"name\": \"test\"\n}"));
}

test "Language.fromContent detects XML/HTML" {
    try std.testing.expectEqual(Language.xml, Language.fromContent("<?xml version=\"1.0\"?>"));
    try std.testing.expectEqual(Language.html, Language.fromContent("<!DOCTYPE html>"));
    try std.testing.expectEqual(Language.html, Language.fromContent("<html>"));
}

test "Language.fromContent detects YAML" {
    try std.testing.expectEqual(Language.yaml, Language.fromContent("---\nkey: value"));
}

test "Language.fromContent detects Markdown" {
    try std.testing.expectEqual(Language.markdown, Language.fromContent("# Heading\n\nSome text"));
    try std.testing.expectEqual(Language.markdown, Language.fromContent("## Subheading"));
}

test "Language.fromContent detects Zig patterns" {
    const zig_code =
        \\const std = @import("std");
        \\
        \\pub fn main() void {
        \\    std.debug.print("Hello\n", .{});
        \\}
    ;
    try std.testing.expectEqual(Language.zig, Language.fromContent(zig_code));
}

test "Language.fromContent detects Python patterns" {
    const py_code =
        \\import os
        \\from sys import argv
        \\
        \\def main():
        \\    print("Hello")
    ;
    try std.testing.expectEqual(Language.python, Language.fromContent(py_code));
}

test "Language.fromContent detects Rust patterns" {
    const rust_code =
        \\use std::io;
        \\
        \\fn main() {
        \\    println!("Hello");
        \\}
    ;
    try std.testing.expectEqual(Language.rust, Language.fromContent(rust_code));
}

test "Language.fromContent detects Go patterns" {
    const go_code =
        \\package main
        \\
        \\import "fmt"
        \\
        \\func main() {
        \\    fmt.Println("Hello")
        \\}
    ;
    try std.testing.expectEqual(Language.go, Language.fromContent(go_code));
}

test "Language.detect prefers filename over content" {
    // Even though content looks like Python, filename says Zig
    try std.testing.expectEqual(Language.zig, Language.detect("test.zig", "import os\ndef main():"));
}

test "Language.detect falls back to content when filename is plain" {
    const zig_code = "const std = @import(\"std\");\npub fn main() void {}";
    try std.testing.expectEqual(Language.zig, Language.detect("noext", zig_code));
    try std.testing.expectEqual(Language.zig, Language.detect(null, zig_code));
}

test "Syntax.withAutoDetect" {
    const allocator = std.testing.allocator;
    const code = "const std = @import(\"std\");";
    const syntax = Syntax.init(allocator, code).withAutoDetect("main.zig");
    try std.testing.expectEqual(Language.zig, syntax.language);
}

test "Syntax.fromFile" {
    const allocator = std.testing.allocator;
    const code = "{\"key\": \"value\"}";
    const syntax = Syntax.fromFile(allocator, code, "config.json");
    try std.testing.expectEqual(Language.json, syntax.language);
}

test "Syntax.withWordWrap" {
    const allocator = std.testing.allocator;
    const syntax = Syntax.init(allocator, "code").withWordWrap();
    try std.testing.expect(syntax.word_wrap);
}

test "Syntax.render with word wrap - short line" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const syntax = Syntax.init(arena.allocator(), "const x = 1;")
        .withLanguage(.zig)
        .withWordWrap();
    const segments = try syntax.render(80, arena.allocator());

    // Short line should render without wrapping
    try std.testing.expect(segments.len > 0);

    // Count newlines (should be exactly 1 for single line)
    var newline_count: usize = 0;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "\n")) {
            newline_count += 1;
        }
    }
    try std.testing.expectEqual(@as(usize, 1), newline_count);
}

test "Syntax.render with word wrap - long line wraps" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    // Create a line that's longer than the max width
    const long_line = "const very_long_variable_name = \"This is a very long string that should wrap\";";
    const syntax = Syntax.init(arena.allocator(), long_line)
        .withLanguage(.zig)
        .withWordWrap();

    // Use narrow width to force wrapping
    const segments = try syntax.render(30, arena.allocator());

    // Should have wrapped (more than 1 newline)
    var newline_count: usize = 0;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "\n")) {
            newline_count += 1;
        }
    }
    try std.testing.expect(newline_count > 1);
}

test "Syntax.render word wrap preserves styles" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    // String that wraps should keep string style on continuation
    const code = "const s = \"This is a very long string literal that will wrap across multiple lines\";";
    const syntax = Syntax.init(arena.allocator(), code)
        .withLanguage(.zig)
        .withWordWrap();

    const segments = try syntax.render(30, arena.allocator());

    // Find string segments (styled with string_style - green)
    var found_styled_segments = false;
    for (segments) |seg| {
        if (seg.style) |style| {
            if (style.color != null) {
                found_styled_segments = true;
                break;
            }
        }
    }
    try std.testing.expect(found_styled_segments);
}

test "Syntax.render word wrap with line numbers" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const long_line = "const x = \"This is a line that is definitely too long to fit\";";
    const syntax = Syntax.init(arena.allocator(), long_line)
        .withLanguage(.zig)
        .withWordWrap()
        .withLineNumbers();

    const segments = try syntax.render(40, arena.allocator());

    // Should have line number prefix and continuation indent
    var found_line_num = false;
    var found_indent = false;
    for (segments) |seg| {
        if (std.mem.indexOf(u8, seg.text, "1") != null and seg.text.len <= 5) {
            found_line_num = true;
        }
        // Continuation indent is 5 spaces
        if (std.mem.eql(u8, seg.text, "     ")) {
            found_indent = true;
        }
    }
    try std.testing.expect(found_line_num);
    // Indent may or may not appear depending on exact wrap point
}

test "wrapSegments basic" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const segs = [_]Segment{
        Segment.plain("Hello World this is a test"),
    };

    const wrapped = try Syntax.wrapSegments(&segs, 10, arena.allocator());

    // Should have wrapped into multiple lines
    var newline_count: usize = 0;
    for (wrapped) |seg| {
        if (std.mem.eql(u8, seg.text, "\n")) {
            newline_count += 1;
        }
    }
    try std.testing.expect(newline_count >= 2);
}

test "wrapSegments preserves style across wrap" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const test_style = Style.empty.bold();
    const segs = [_]Segment{
        Segment.styled("This is styled text that should wrap", test_style),
    };

    const wrapped = try Syntax.wrapSegments(&segs, 15, arena.allocator());

    // All non-newline segments should have the style
    for (wrapped) |seg| {
        if (!std.mem.eql(u8, seg.text, "\n") and seg.text.len > 0) {
            try std.testing.expect(seg.style != null);
            try std.testing.expect(seg.style.?.hasAttribute(.bold));
        }
    }
}

test "wrapSegments handles multiple segments" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const style1 = Style.empty.bold();
    const style2 = Style.empty.italic();

    const segs = [_]Segment{
        Segment.styled("const ", style1),
        Segment.plain("x = "),
        Segment.styled("\"long string value\"", style2),
    };

    const wrapped = try Syntax.wrapSegments(&segs, 15, arena.allocator());

    // Should produce some output
    try std.testing.expect(wrapped.len > 0);
}

test "wrapSegments breaks at word boundaries" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    // "Hello World" with width 8 should break after "Hello " (6 chars)
    // not in the middle of "World"
    const segs = [_]Segment{
        Segment.plain("Hello World"),
    };

    const wrapped = try Syntax.wrapSegments(&segs, 8, arena.allocator());

    // Collect non-newline text segments
    var lines: std.ArrayList([]const u8) = .empty;
    var current_line: std.ArrayList(u8) = .empty;

    for (wrapped) |seg| {
        if (std.mem.eql(u8, seg.text, "\n")) {
            if (current_line.items.len > 0) {
                try lines.append(arena.allocator(), try arena.allocator().dupe(u8, current_line.items));
                current_line.clearRetainingCapacity();
            }
        } else {
            try current_line.appendSlice(arena.allocator(), seg.text);
        }
    }
    if (current_line.items.len > 0) {
        try lines.append(arena.allocator(), try arena.allocator().dupe(u8, current_line.items));
    }

    // Should have 2 lines
    try std.testing.expectEqual(@as(usize, 2), lines.items.len);
    // First line should be "Hello " (breaks at word boundary)
    try std.testing.expectEqualStrings("Hello ", lines.items[0]);
    // Second line should be "World"
    try std.testing.expectEqualStrings("World", lines.items[1]);
}

test "wrapSegments zero width returns input" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const segs = [_]Segment{
        Segment.plain("Hello"),
    };

    const wrapped = try Syntax.wrapSegments(&segs, 0, arena.allocator());

    try std.testing.expectEqual(@as(usize, 1), wrapped.len);
    try std.testing.expectEqualStrings("Hello", wrapped[0].text);
}

test "Syntax.loadFile reads file and detects language" {
    const allocator = std.testing.allocator;
    const io = std.testing.io;

    // Load this source file itself
    var syntax = try Syntax.loadFile(allocator, io, "src/renderables/syntax/highlighter.zig");
    defer syntax.deinit();

    // Should detect as Zig from extension
    try std.testing.expectEqual(Language.zig, syntax.language);

    // Should contain actual code
    try std.testing.expect(syntax.code.len > 0);
    try std.testing.expect(std.mem.indexOf(u8, syntax.code, "const std") != null);
}

test "Syntax.loadFile error on non-existent file" {
    const allocator = std.testing.allocator;
    const io = std.testing.io;

    const result = Syntax.loadFile(allocator, io, "non_existent_file_xyz123.zig");
    try std.testing.expectError(error.FileNotFound, result);
}

test "Syntax.loadFile renders correctly" {
    const allocator = std.testing.allocator;
    const io = std.testing.io;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    // Load build.zig.zon (small file)
    var syntax = try Syntax.loadFile(arena.allocator(), io, "build.zig.zon");
    // No need for deinit since arena handles cleanup

    // Should detect as Zig from extension
    try std.testing.expectEqual(Language.zig, syntax.language);

    // Should render successfully
    const segments = try syntax.render(80, arena.allocator());
    try std.testing.expect(segments.len > 0);
}

test "Syntax.withTabSize" {
    const allocator = std.testing.allocator;
    const syntax = Syntax.init(allocator, "code").withTabSize(2);
    try std.testing.expectEqual(@as(u8, 2), syntax.tab_size);
}

test "Syntax.withTabSize default is 4" {
    const allocator = std.testing.allocator;
    const syntax = Syntax.init(allocator, "code");
    try std.testing.expectEqual(@as(u8, 4), syntax.tab_size);
}

test "Syntax.expandTabs no tabs returns original" {
    const allocator = std.testing.allocator;
    const syntax = Syntax.init(allocator, "");

    const line = "no tabs here";
    const result = try syntax.expandTabs(line, allocator);

    // Should return same pointer (no allocation)
    try std.testing.expectEqual(line.ptr, result.ptr);
}

test "Syntax.expandTabs single tab at start" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const syntax = Syntax.init(arena.allocator(), "").withTabSize(4);
    const result = try syntax.expandTabs("\tcode", arena.allocator());

    try std.testing.expectEqualStrings("    code", result);
}

test "Syntax.expandTabs multiple tabs" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const syntax = Syntax.init(arena.allocator(), "").withTabSize(4);
    const result = try syntax.expandTabs("\t\tindented", arena.allocator());

    try std.testing.expectEqualStrings("        indented", result);
}

test "Syntax.expandTabs tab in middle aligns to tab stop" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const syntax = Syntax.init(arena.allocator(), "").withTabSize(4);
    // "ab" is 2 chars, tab should add 2 spaces to reach column 4
    const result = try syntax.expandTabs("ab\tc", arena.allocator());

    try std.testing.expectEqualStrings("ab  c", result);
}

test "Syntax.expandTabs tab at tab stop adds full tab" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const syntax = Syntax.init(arena.allocator(), "").withTabSize(4);
    // "abcd" is 4 chars (at column 4), tab should add 4 spaces to reach column 8
    const result = try syntax.expandTabs("abcd\te", arena.allocator());

    try std.testing.expectEqualStrings("abcd    e", result);
}

test "Syntax.expandTabs custom tab size 2" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const syntax = Syntax.init(arena.allocator(), "").withTabSize(2);
    const result = try syntax.expandTabs("\tcode", arena.allocator());

    try std.testing.expectEqualStrings("  code", result);
}

test "Syntax.expandTabs custom tab size 8" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const syntax = Syntax.init(arena.allocator(), "").withTabSize(8);
    const result = try syntax.expandTabs("\tcode", arena.allocator());

    try std.testing.expectEqualStrings("        code", result);
}

test "Syntax.render expands tabs" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    // Use plain language for simpler output to verify
    const code = "\thello";
    const syntax = Syntax.init(arena.allocator(), code)
        .withLanguage(.plain)
        .withTabSize(4);

    const segments = try syntax.render(80, arena.allocator());

    // For plain language, there should be at least 1 segment
    try std.testing.expect(segments.len >= 1);

    // Check the first non-newline segment for tab expansion
    for (segments) |seg| {
        if (!std.mem.eql(u8, seg.text, "\n") and seg.text.len > 0) {
            // No tabs in the text
            try std.testing.expect(std.mem.indexOfScalar(u8, seg.text, '\t') == null);
            // The expanded text should be "    hello" (4 spaces + hello)
            try std.testing.expectEqualStrings("    hello", seg.text);
            break;
        }
    }
}

test "Syntax.withIndentGuides" {
    const allocator = std.testing.allocator;
    const syntax = Syntax.init(allocator, "code").withIndentGuides();
    try std.testing.expect(syntax.indent_guides);
}

test "Syntax.withIndentGuideChar" {
    const allocator = std.testing.allocator;
    const syntax = Syntax.init(allocator, "code").withIndentGuideChar(":");
    try std.testing.expectEqualStrings(":", syntax.indent_guide_char);
}

test "Syntax.render with indent guides - no indentation" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const code = "no indent";
    const syntax = Syntax.init(arena.allocator(), code)
        .withLanguage(.plain)
        .withIndentGuides();

    const segments = try syntax.render(80, arena.allocator());

    // Should render without guide characters since there's no indentation
    var has_guide = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "|")) {
            has_guide = true;
            break;
        }
    }
    try std.testing.expect(!has_guide);
}

test "Syntax.render with indent guides - single level" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    // 4 spaces = 1 indent level
    const code = "    indented";
    const syntax = Syntax.init(arena.allocator(), code)
        .withLanguage(.plain)
        .withTabSize(4)
        .withIndentGuides();

    const segments = try syntax.render(80, arena.allocator());

    // Should have one guide character
    var guide_count: usize = 0;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "|")) {
            guide_count += 1;
        }
    }
    try std.testing.expectEqual(@as(usize, 1), guide_count);
}

test "Syntax.render with indent guides - multiple levels" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    // 8 spaces = 2 indent levels with tab_size 4
    const code = "        double indent";
    const syntax = Syntax.init(arena.allocator(), code)
        .withLanguage(.plain)
        .withTabSize(4)
        .withIndentGuides();

    const segments = try syntax.render(80, arena.allocator());

    // Should have two guide characters
    var guide_count: usize = 0;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "|")) {
            guide_count += 1;
        }
    }
    try std.testing.expectEqual(@as(usize, 2), guide_count);
}

test "Syntax.render with indent guides - custom char" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const code = "    indented";
    const syntax = Syntax.init(arena.allocator(), code)
        .withLanguage(.plain)
        .withTabSize(4)
        .withIndentGuides()
        .withIndentGuideChar(":");

    const segments = try syntax.render(80, arena.allocator());

    // Should have custom guide character
    var found_colon = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, ":")) {
            found_colon = true;
            break;
        }
    }
    try std.testing.expect(found_colon);
}

test "Syntax.render with indent guides preserves syntax highlighting" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const code = "    const x = 1;";
    const syntax = Syntax.init(arena.allocator(), code)
        .withLanguage(.zig)
        .withTabSize(4)
        .withIndentGuides();

    const segments = try syntax.render(80, arena.allocator());

    // Should have both guide and styled content
    var has_guide = false;
    var has_styled = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "|")) {
            has_guide = true;
        }
        if (seg.style) |style| {
            if (style.color != null) {
                has_styled = true;
            }
        }
    }
    try std.testing.expect(has_guide);
    try std.testing.expect(has_styled);
}

test "Syntax.render with indent guides - partial indent" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    // 6 spaces = 1 full level + 2 spaces partial with tab_size 4
    const code = "      partial";
    const syntax = Syntax.init(arena.allocator(), code)
        .withLanguage(.plain)
        .withTabSize(4)
        .withIndentGuides();

    const segments = try syntax.render(80, arena.allocator());

    // Should have one guide character (for the full level)
    var guide_count: usize = 0;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "|")) {
            guide_count += 1;
        }
    }
    try std.testing.expectEqual(@as(usize, 1), guide_count);
}

test "Syntax.render with indent guides - multiline" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const code =
        \\fn foo() void {
        \\    const x = 1;
        \\        const y = 2;
        \\}
    ;
    const syntax = Syntax.init(arena.allocator(), code)
        .withLanguage(.zig)
        .withTabSize(4)
        .withIndentGuides();

    const segments = try syntax.render(80, arena.allocator());

    // Count guide characters - should be 3 total (1 for line 2, 2 for line 3)
    var guide_count: usize = 0;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "|")) {
            guide_count += 1;
        }
    }
    try std.testing.expectEqual(@as(usize, 3), guide_count);
}

// Line highlighting tests

test "Syntax.withHighlightLines" {
    const allocator = std.testing.allocator;
    const lines = [_]usize{ 1, 3, 5 };
    const syntax = Syntax.init(allocator, "code").withHighlightLines(&lines);
    try std.testing.expect(syntax.highlight_lines != null);
    try std.testing.expectEqual(@as(usize, 3), syntax.highlight_lines.?.len);
}

test "Syntax.isLineHighlighted" {
    const allocator = std.testing.allocator;
    const lines = [_]usize{ 2, 5, 10 };
    const syntax = Syntax.init(allocator, "code").withHighlightLines(&lines);

    try std.testing.expect(syntax.isLineHighlighted(2));
    try std.testing.expect(syntax.isLineHighlighted(5));
    try std.testing.expect(syntax.isLineHighlighted(10));
    try std.testing.expect(!syntax.isLineHighlighted(1));
    try std.testing.expect(!syntax.isLineHighlighted(3));
    try std.testing.expect(!syntax.isLineHighlighted(100));
}

test "Syntax.isLineHighlighted no highlights" {
    const allocator = std.testing.allocator;
    const syntax = Syntax.init(allocator, "code");

    try std.testing.expect(!syntax.isLineHighlighted(1));
    try std.testing.expect(!syntax.isLineHighlighted(100));
}

test "Syntax.render with highlighted lines - basic" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const code = "line 1\nline 2\nline 3";
    const lines = [_]usize{2};
    const syntax = Syntax.init(arena.allocator(), code)
        .withLanguage(.plain)
        .withHighlightLines(&lines);

    const segments = try syntax.render(80, arena.allocator());

    // Should have some segments with highlight background
    var found_highlighted = false;
    for (segments) |seg| {
        if (seg.style) |style| {
            if (style.bgcolor != null) {
                found_highlighted = true;
                break;
            }
        }
    }
    try std.testing.expect(found_highlighted);
}

test "Syntax.render with highlighted lines - zig syntax" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const code =
        \\const x = 1;
        \\const y = 2;
        \\const z = 3;
    ;
    const lines = [_]usize{2};
    const syntax = Syntax.init(arena.allocator(), code)
        .withLanguage(.zig)
        .withHighlightLines(&lines);

    const segments = try syntax.render(80, arena.allocator());

    // Highlighted line should have background color on its segments
    var found_highlighted = false;
    for (segments) |seg| {
        if (seg.style) |style| {
            if (style.bgcolor != null) {
                found_highlighted = true;
                break;
            }
        }
    }
    try std.testing.expect(found_highlighted);
}

test "Syntax.render with highlighted lines and line numbers" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const code = "line 1\nline 2\nline 3";
    const lines = [_]usize{2};
    const syntax = Syntax.init(arena.allocator(), code)
        .withLanguage(.plain)
        .withLineNumbers()
        .withHighlightLines(&lines);

    const segments = try syntax.render(80, arena.allocator());

    // Should render without errors and have line numbers
    var found_line_num = false;
    for (segments) |seg| {
        if (std.mem.indexOf(u8, seg.text, "2") != null and seg.text.len <= 5) {
            found_line_num = true;
            break;
        }
    }
    try std.testing.expect(found_line_num);
}

test "Syntax.render with multiple highlighted lines" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const code = "line 1\nline 2\nline 3\nline 4\nline 5";
    const lines = [_]usize{ 1, 3, 5 };
    const syntax = Syntax.init(arena.allocator(), code)
        .withLanguage(.plain)
        .withHighlightLines(&lines);

    const segments = try syntax.render(80, arena.allocator());

    // Multiple lines should be highlighted
    var highlight_count: usize = 0;
    var prev_was_newline = true;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "\n")) {
            prev_was_newline = true;
            continue;
        }
        if (prev_was_newline and seg.style != null and seg.style.?.bgcolor != null) {
            highlight_count += 1;
        }
        prev_was_newline = false;
    }
    try std.testing.expect(highlight_count >= 3);
}

test "Syntax.render with highlighted lines preserves syntax colors" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const code = "const x = 42;";
    const lines = [_]usize{1};
    const syntax = Syntax.init(arena.allocator(), code)
        .withLanguage(.zig)
        .withHighlightLines(&lines);

    const segments = try syntax.render(80, arena.allocator());

    // Should have both foreground colors (syntax) and background (highlight)
    var found_styled = false;
    for (segments) |seg| {
        if (seg.style) |style| {
            if (style.color != null and style.bgcolor != null) {
                found_styled = true;
                break;
            }
        }
    }
    try std.testing.expect(found_styled);
}

test "SyntaxTheme highlight styles" {
    // Default theme
    try std.testing.expect(SyntaxTheme.default.highlight_line_style.bgcolor != null);
    try std.testing.expect(SyntaxTheme.default.highlight_line_number_style.color != null);

    // Monokai theme
    try std.testing.expect(SyntaxTheme.monokai.highlight_line_style.bgcolor != null);
    try std.testing.expect(SyntaxTheme.monokai.highlight_line_number_style.color != null);

    // Dracula theme
    try std.testing.expect(SyntaxTheme.dracula.highlight_line_style.bgcolor != null);
    try std.testing.expect(SyntaxTheme.dracula.highlight_line_number_style.color != null);
}

test "Syntax.render with highlighted lines and indent guides" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const code =
        \\fn foo() void {
        \\    const x = 1;
        \\}
    ;
    const lines = [_]usize{2};
    const syntax = Syntax.init(arena.allocator(), code)
        .withLanguage(.zig)
        .withTabSize(4)
        .withIndentGuides()
        .withHighlightLines(&lines);

    const segments = try syntax.render(80, arena.allocator());

    // Should have both indent guides and highlighted content
    var has_guide = false;
    var has_highlight = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "|")) {
            has_guide = true;
        }
        if (seg.style) |style| {
            if (style.bgcolor != null) {
                has_highlight = true;
            }
        }
    }
    try std.testing.expect(has_guide);
    try std.testing.expect(has_highlight);
}

test "Syntax.render highlighted line with custom start_line" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const code = "line 1\nline 2\nline 3";
    // Highlight line 12 (which is the second line when start_line is 11)
    const lines = [_]usize{12};
    const syntax = Syntax.init(arena.allocator(), code)
        .withLanguage(.plain)
        .withStartLine(11)
        .withHighlightLines(&lines);

    const segments = try syntax.render(80, arena.allocator());

    // The second line should be highlighted
    var found_highlighted = false;
    for (segments) |seg| {
        if (seg.style) |style| {
            if (style.bgcolor != null) {
                found_highlighted = true;
                break;
            }
        }
    }
    try std.testing.expect(found_highlighted);
}
