const std = @import("std");
const Segment = @import("../../segment.zig").Segment;
const Style = @import("../../style.zig").Style;
const syntax_mod = @import("../syntax/mod.zig");
const Syntax = syntax_mod.Syntax;
const SyntaxTheme = syntax_mod.SyntaxTheme;
const Language = syntax_mod.Language;
const Rule = @import("../rule.zig").Rule;
const table_mod = @import("../table/mod.zig");
const Table = table_mod.Table;
const Column = table_mod.Column;
const JustifyMethod = table_mod.JustifyMethod;
const theme_mod = @import("theme.zig");
const HeaderLevel = theme_mod.HeaderLevel;
const MarkdownTheme = theme_mod.MarkdownTheme;
const elements_mod = @import("elements.zig");
const Header = elements_mod.Header;
const CodeBlock = elements_mod.CodeBlock;

pub const Markdown = struct {
    source: []const u8,
    theme: MarkdownTheme = .default,

    pub fn init(source: []const u8) Markdown {
        return .{ .source = source };
    }

    pub fn withTheme(self: Markdown, theme: MarkdownTheme) Markdown {
        var m = self;
        m.theme = theme;
        return m;
    }

    pub fn render(self: Markdown, max_width: usize, allocator: std.mem.Allocator) ![]Segment {
        var segments: std.ArrayList(Segment) = .empty;
        var lines = std.mem.splitScalar(u8, self.source, '\n');

        var in_code_block = false;
        var code_block_lang: ?[]const u8 = null;
        var code_lines: std.ArrayList(u8) = .empty;

        var table_lines: std.ArrayList([]const u8) = .empty;
        defer table_lines.deinit(allocator);

        while (lines.next()) |line| {
            const trimmed = std.mem.trimStart(u8, line, " \t");

            if (parseFenceOpen(trimmed)) |lang| {
                if (!in_code_block) {
                    // Flush any pending table
                    if (table_lines.items.len > 0) {
                        try self.emitTable(&table_lines, max_width, &segments, allocator);
                    }
                    in_code_block = true;
                    code_block_lang = lang;
                    code_lines = .empty;
                    continue;
                }
            }

            if (in_code_block) {
                if (parseFenceClose(trimmed)) {
                    try self.emitCodeBlock(&code_lines, code_block_lang, max_width, &segments, allocator);
                    in_code_block = false;
                    code_block_lang = null;
                    continue;
                }

                if (code_lines.items.len > 0) {
                    try code_lines.append(allocator, '\n');
                }
                try code_lines.appendSlice(allocator, line);
                continue;
            }

            // Check if this could be a table row
            if (isTableRow(trimmed)) {
                try table_lines.append(allocator, trimmed);
                continue;
            }

            // Not a table row - flush any pending table first
            if (table_lines.items.len > 0) {
                try self.emitTable(&table_lines, max_width, &segments, allocator);
            }

            if (parseHeader(trimmed)) |header_info| {
                const header = Header.init(header_info.text, header_info.level)
                    .withTheme(self.theme);
                const header_segments = try header.render(max_width, allocator);
                defer allocator.free(header_segments);
                try segments.appendSlice(allocator, header_segments);
            } else if (parseHorizontalRule(trimmed)) {
                const rule = Rule.init()
                    .withCharacters(self.theme.rule_char)
                    .withStyle(self.theme.rule_style);
                const rule_segments = try rule.render(max_width, allocator);
                defer allocator.free(rule_segments);
                try segments.appendSlice(allocator, rule_segments);
            } else if (parseBlockquote(line)) |blockquote| {
                try self.renderBlockquoteLine(blockquote, &segments, allocator);
                try segments.append(allocator, Segment.line());
            } else if (parseOrderedListItem(line)) |list_item| {
                try self.renderOrderedListItem(list_item, &segments, allocator);
                try segments.append(allocator, Segment.line());
            } else if (parseUnorderedListItem(line)) |list_item| {
                try self.renderUnorderedListItem(list_item, &segments, allocator);
                try segments.append(allocator, Segment.line());
            } else {
                if (line.len > 0) {
                    try self.renderInlineText(line, null, &segments, allocator);
                }
                try segments.append(allocator, Segment.line());
            }
        }

        // Flush any remaining content
        if (in_code_block and code_lines.items.len > 0) {
            try self.emitCodeBlock(&code_lines, code_block_lang, max_width, &segments, allocator);
        }

        if (table_lines.items.len > 0) {
            try self.emitTable(&table_lines, max_width, &segments, allocator);
        }

        return segments.toOwnedSlice(allocator);
    }

    fn emitCodeBlock(
        self: Markdown,
        code_lines: *std.ArrayList(u8),
        lang: ?[]const u8,
        max_width: usize,
        segments: *std.ArrayList(Segment),
        allocator: std.mem.Allocator,
    ) !void {
        const code = try code_lines.toOwnedSlice(allocator);
        defer allocator.free(code);
        var code_block = CodeBlock.init(code).withTheme(self.theme);
        if (lang) |l| {
            code_block = code_block.withLanguage(l);
        }
        const code_segments = try code_block.render(max_width, allocator);
        defer allocator.free(code_segments);
        try segments.appendSlice(allocator, code_segments);
    }

    fn isTableRow(line: []const u8) bool {
        const trimmed = std.mem.trim(u8, line, " \t");
        if (trimmed.len == 0) return false;
        return std.mem.indexOfScalar(u8, trimmed, '|') != null;
    }

    fn isTableSeparator(line: []const u8) bool {
        const trimmed = std.mem.trim(u8, line, " \t|");
        if (trimmed.len == 0) return false;

        var dash_count: usize = 0;
        for (trimmed) |c| {
            switch (c) {
                '-' => dash_count += 1,
                ':', '|', ' ' => {},
                else => return false,
            }
        }
        return dash_count >= 3;
    }

    fn parseTableAlignment(sep_cell: []const u8) JustifyMethod {
        const trimmed = std.mem.trim(u8, sep_cell, " \t");
        if (trimmed.len == 0) return .left;

        const starts_colon = trimmed[0] == ':';
        const ends_colon = trimmed[trimmed.len - 1] == ':';

        if (starts_colon and ends_colon) return .center;
        if (ends_colon) return .right;
        return .left;
    }

    fn splitTableCells(line: []const u8, allocator: std.mem.Allocator) ![][]const u8 {
        var result: std.ArrayList([]const u8) = .empty;

        const content = std.mem.trim(u8, std.mem.trim(u8, line, " \t"), "|");

        var iter = std.mem.splitScalar(u8, content, '|');
        while (iter.next()) |cell| {
            try result.append(allocator, std.mem.trim(u8, cell, " \t"));
        }

        return result.toOwnedSlice(allocator);
    }

    fn emitTable(
        self: Markdown,
        table_lines: *std.ArrayList([]const u8),
        max_width: usize,
        segments: *std.ArrayList(Segment),
        allocator: std.mem.Allocator,
    ) !void {
        defer table_lines.clearRetainingCapacity();

        const is_valid_table = table_lines.items.len >= 2 and isTableSeparator(table_lines.items[1]);

        if (!is_valid_table) {
            for (table_lines.items) |line| {
                try self.renderInlineText(line, null, segments, allocator);
                try segments.append(allocator, Segment.line());
            }
            return;
        }

        const header_cells = try splitTableCells(table_lines.items[0], allocator);
        defer allocator.free(header_cells);

        const sep_cells = try splitTableCells(table_lines.items[1], allocator);
        defer allocator.free(sep_cells);

        var table = Table.init(allocator);
        defer table.deinit();

        for (header_cells, 0..) |header, i| {
            const justify = if (i < sep_cells.len) parseTableAlignment(sep_cells[i]) else .left;
            _ = table.withColumn(Column.init(header).withJustify(justify));
        }

        _ = table.withHeaderStyle(self.theme.table_header_style);
        _ = table.withBorderStyle(self.theme.table_border_style);
        _ = table.withBoxStyle(self.theme.table_box_style);

        for (table_lines.items[2..]) |row_line| {
            const row_cells = try splitTableCells(row_line, allocator);
            defer allocator.free(row_cells);

            const row = try allocator.alloc([]const u8, header_cells.len);
            defer allocator.free(row);

            for (0..header_cells.len) |i| {
                row[i] = if (i < row_cells.len) row_cells[i] else "";
            }

            try table.addRow(row);
        }

        const table_segments = try table.render(max_width, allocator);
        defer allocator.free(table_segments);
        try segments.appendSlice(allocator, table_segments);
    }

    fn parseFenceOpen(line: []const u8) ?[]const u8 {
        if (line.len < 3) return null;
        if (!std.mem.startsWith(u8, line, "```")) return null;

        const rest = std.mem.trimStart(u8, line[3..], " \t");
        if (rest.len == 0) return "";

        const lang_end = std.mem.indexOfAny(u8, rest, " \t") orelse rest.len;
        return rest[0..lang_end];
    }

    fn parseFenceClose(line: []const u8) bool {
        if (line.len < 3) return false;
        const trimmed = std.mem.trimEnd(u8, line, " \t");
        return std.mem.eql(u8, trimmed, "```");
    }

    const HeaderInfo = struct {
        level: HeaderLevel,
        text: []const u8,
    };

    const OrderedListItem = struct {
        number: usize,
        text: []const u8,
        indent_level: usize,
    };

    const TaskCheckbox = enum {
        unchecked,
        checked,
    };

    const UnorderedListItem = struct {
        text: []const u8,
        indent_level: usize,
        checkbox: ?TaskCheckbox = null,
    };

    const BlockquoteItem = struct {
        text: []const u8,
        depth: usize,
    };

    fn countLeadingSpaces(line: []const u8) usize {
        var count: usize = 0;
        while (count < line.len and line[count] == ' ') : (count += 1) {}
        return count;
    }

    fn parseOrderedListItem(line: []const u8) ?OrderedListItem {
        const indent = countLeadingSpaces(line);
        const rest = line[indent..];
        if (rest.len == 0) return null;

        var num_end: usize = 0;
        while (num_end < rest.len and std.ascii.isDigit(rest[num_end])) : (num_end += 1) {}

        if (num_end == 0 or num_end >= rest.len) return null;

        const delimiter = rest[num_end];
        if (delimiter != '.' and delimiter != ')') return null;

        const after_delim = num_end + 1;
        if (after_delim >= rest.len or rest[after_delim] != ' ') return null;

        const number = std.fmt.parseInt(usize, rest[0..num_end], 10) catch return null;

        return .{
            .number = number,
            .text = std.mem.trim(u8, rest[after_delim..], " \t"),
            .indent_level = indent / 2,
        };
    }

    fn parseUnorderedListItem(line: []const u8) ?UnorderedListItem {
        const indent = countLeadingSpaces(line);
        const rest = line[indent..];
        if (rest.len < 2) return null;

        const bullet = rest[0];
        if (bullet != '-' and bullet != '*' and bullet != '+') return null;
        if (rest[1] != ' ') return null;

        const after_bullet = rest[2..];

        // Check for task list syntax: [ ] or [x] or [X]
        if (after_bullet.len >= 3 and after_bullet[0] == '[' and after_bullet[2] == ']') {
            const checkbox_char = after_bullet[1];
            const checkbox: ?TaskCheckbox = if (checkbox_char == ' ')
                .unchecked
            else if (checkbox_char == 'x' or checkbox_char == 'X')
                .checked
            else
                null;

            if (checkbox) |cb| {
                const text_start: usize = if (after_bullet.len > 3 and after_bullet[3] == ' ') 4 else 3;
                return .{
                    .text = std.mem.trim(u8, after_bullet[text_start..], " \t"),
                    .indent_level = indent / 2,
                    .checkbox = cb,
                };
            }
        }

        return .{
            .text = std.mem.trim(u8, after_bullet, " \t"),
            .indent_level = indent / 2,
        };
    }

    fn parseBlockquote(line: []const u8) ?BlockquoteItem {
        var pos: usize = 0;
        var depth: usize = 0;

        while (pos < line.len) {
            while (pos < line.len and line[pos] == ' ') : (pos += 1) {}
            if (pos >= line.len or line[pos] != '>') break;

            depth += 1;
            pos += 1;

            if (pos < line.len and line[pos] == ' ') pos += 1;
        }

        if (depth == 0) return null;

        return .{
            .text = line[pos..],
            .depth = depth,
        };
    }

    fn renderListIndent(
        self: Markdown,
        indent_level: usize,
        segments: *std.ArrayList(Segment),
        allocator: std.mem.Allocator,
    ) !void {
        const base_indent = indent_level * self.theme.list_indent;
        if (base_indent > 0) {
            const indent_str = try allocator.alloc(u8, base_indent);
            @memset(indent_str, ' ');
            try segments.append(allocator, Segment.plain(indent_str));
        }
    }

    fn renderUnorderedListItem(
        self: Markdown,
        item: UnorderedListItem,
        segments: *std.ArrayList(Segment),
        allocator: std.mem.Allocator,
    ) !void {
        try self.renderListIndent(item.indent_level, segments, allocator);

        const marker_char, const marker_style = if (item.checkbox) |checkbox| switch (checkbox) {
            .unchecked => .{ self.theme.task_unchecked_char, self.theme.task_unchecked_style },
            .checked => .{ self.theme.task_checked_char, self.theme.task_checked_style },
        } else .{ self.theme.list_bullet_char, self.theme.list_bullet_style };

        const marker_with_space = try std.fmt.allocPrint(allocator, "{s} ", .{marker_char});
        try segments.append(allocator, Segment.styled(marker_with_space, marker_style));

        try self.renderInlineText(item.text, null, segments, allocator);
    }

    fn renderOrderedListItem(
        self: Markdown,
        item: OrderedListItem,
        segments: *std.ArrayList(Segment),
        allocator: std.mem.Allocator,
    ) !void {
        try self.renderListIndent(item.indent_level, segments, allocator);

        const number_str = try std.fmt.allocPrint(allocator, "{d}. ", .{item.number});
        try segments.append(allocator, Segment.styled(number_str, self.theme.list_number_style));

        try self.renderInlineText(item.text, null, segments, allocator);
    }

    fn renderBlockquoteLine(
        self: Markdown,
        item: BlockquoteItem,
        segments: *std.ArrayList(Segment),
        allocator: std.mem.Allocator,
    ) !void {
        for (0..item.depth) |_| {
            const border_with_space = try std.fmt.allocPrint(allocator, "{s} ", .{self.theme.blockquote_border_char});
            try segments.append(allocator, Segment.styled(border_with_space, self.theme.blockquote_border_style));

            if (self.theme.blockquote_indent > 0) {
                const indent_str = try allocator.alloc(u8, self.theme.blockquote_indent);
                @memset(indent_str, ' ');
                try segments.append(allocator, Segment.plain(indent_str));
            }
        }

        if (item.text.len > 0) {
            try self.renderInlineText(item.text, self.theme.blockquote_style, segments, allocator);
        }
    }

    fn parseHeader(line: []const u8) ?HeaderInfo {
        if (line.len == 0 or line[0] != '#') return null;

        var hash_count: usize = 0;
        while (hash_count < line.len and line[hash_count] == '#') {
            hash_count += 1;
        }

        const level = HeaderLevel.fromCount(hash_count) orelse return null;

        if (hash_count >= line.len) {
            return .{ .level = level, .text = "" };
        }

        if (line[hash_count] != ' ' and line[hash_count] != '\t') {
            return null;
        }

        const text = std.mem.trim(u8, line[hash_count..], " \t");
        return .{ .level = level, .text = text };
    }

    fn parseHorizontalRule(line: []const u8) bool {
        const trimmed = std.mem.trim(u8, line, " \t");
        if (trimmed.len < 3) return false;

        const rule_char = trimmed[0];
        if (rule_char != '-' and rule_char != '*' and rule_char != '_') return false;

        var char_count: usize = 0;
        for (trimmed) |c| {
            if (c == rule_char) {
                char_count += 1;
            } else if (c != ' ') {
                return false;
            }
        }

        return char_count >= 3;
    }

    const InlineSpan = struct {
        text: []const u8,
        style_type: StyleType,
        link_url: ?[]const u8 = null,
        image_url: ?[]const u8 = null,

        const StyleType = enum {
            plain,
            bold,
            italic,
            bold_italic,
            strikethrough,
            link,
            code,
            image,
        };
    };

    fn parseInlineStyles(text: []const u8, allocator: std.mem.Allocator) ![]InlineSpan {
        var spans: std.ArrayList(InlineSpan) = .empty;
        var pos: usize = 0;

        while (pos < text.len) {
            // Try image first: ![alt](url)
            if (tryParseImage(text, pos)) |result| {
                try spans.append(allocator, .{
                    .text = result.text,
                    .style_type = .image,
                    .image_url = result.url,
                });
                pos = result.end_pos;
                continue;
            }

            // Try link: [text](url)
            if (tryParseLink(text, pos)) |result| {
                try spans.append(allocator, .{
                    .text = result.text,
                    .style_type = .link,
                    .link_url = result.url,
                });
                pos = result.end_pos;
                continue;
            }

            if (tryParseInlineCode(text, pos)) |result| {
                try spans.append(allocator, .{
                    .text = result.content,
                    .style_type = .code,
                });
                pos = result.end_pos;
                continue;
            }

            if (tryParseStrikethrough(text, pos)) |result| {
                try spans.append(allocator, .{
                    .text = result.content,
                    .style_type = .strikethrough,
                });
                pos = result.end_pos;
                continue;
            }

            if (tryParseStyledSpan(text, pos)) |result| {
                try spans.append(allocator, .{
                    .text = result.content,
                    .style_type = result.style_type,
                });
                pos = result.end_pos;
                continue;
            }

            // Plain text - find next potential delimiter or end
            const plain_start = pos;
            pos = advanceToNextDelimiter(text, pos);

            // If we hit an unmatched delimiter, skip it and continue
            if (pos < text.len and plain_start == pos) {
                pos += 1;
                pos = advanceToNextDelimiter(text, pos);
            }

            if (pos > plain_start) {
                try spans.append(allocator, .{
                    .text = text[plain_start..pos],
                    .style_type = .plain,
                });
            }
        }

        return spans.toOwnedSlice(allocator);
    }

    const LinkResult = struct {
        text: []const u8,
        url: []const u8,
        end_pos: usize,
    };

    fn tryParseImage(text: []const u8, pos: usize) ?LinkResult {
        // Image syntax: ![alt text](url) - same as link but with ! prefix
        if (pos >= text.len or text[pos] != '!') return null;
        return tryParseLink(text, pos + 1);
    }

    /// Scans for a matching closing delimiter, respecting nesting.
    /// Returns the position of the closing delimiter, or null if not found.
    fn findMatchingDelimiter(text: []const u8, start: usize, open: u8, close: u8) ?usize {
        var depth: usize = 1;
        var pos: usize = start;

        while (pos < text.len and depth > 0) {
            if (text[pos] == open) {
                depth += 1;
            } else if (text[pos] == close) {
                depth -= 1;
            }
            if (depth > 0) pos += 1;
        }

        return if (depth == 0) pos else null;
    }

    fn tryParseLink(text: []const u8, pos: usize) ?LinkResult {
        if (pos >= text.len or text[pos] != '[') return null;

        const text_start = pos + 1;
        const text_end = findMatchingDelimiter(text, text_start, '[', ']') orelse return null;

        const paren_start = text_end + 1;
        if (paren_start >= text.len or text[paren_start] != '(') return null;

        const url_start = paren_start + 1;
        const url_end = findMatchingDelimiter(text, url_start, '(', ')') orelse return null;

        return .{
            .text = text[text_start..text_end],
            .url = text[url_start..url_end],
            .end_pos = url_end + 1,
        };
    }

    const ParseResult = struct {
        content: []const u8,
        style_type: InlineSpan.StyleType,
        end_pos: usize,
    };

    fn tryParseStyledSpan(text: []const u8, pos: usize) ?ParseResult {
        const c = text[pos];
        if (c != '*' and c != '_') return null;

        // Try longest match first (bold+italic: 3, bold: 2, italic: 1)
        const patterns = [_]struct { count: usize, style: InlineSpan.StyleType }{
            .{ .count = 3, .style = .bold_italic },
            .{ .count = 2, .style = .bold },
            .{ .count = 1, .style = .italic },
        };

        for (patterns) |p| {
            if (pos + p.count > text.len) continue;
            if (!isRepeatedChar(text[pos..][0..p.count], c)) continue;
            if (findClosingDelimiter(text, pos + p.count, c, p.count)) |end| {
                return .{
                    .content = text[pos + p.count .. end],
                    .style_type = p.style,
                    .end_pos = end + p.count,
                };
            }
        }
        return null;
    }

    fn isRepeatedChar(slice: []const u8, char: u8) bool {
        for (slice) |c| {
            if (c != char) return false;
        }
        return true;
    }

    const InlineCodeResult = struct {
        content: []const u8,
        end_pos: usize,
    };

    fn tryParseInlineCode(text: []const u8, pos: usize) ?InlineCodeResult {
        if (pos >= text.len or text[pos] != '`') return null;

        // Count opening backticks (inline code uses 1-2, not 3+ like fenced blocks)
        var backtick_count: usize = 0;
        var p = pos;
        while (p < text.len and text[p] == '`') : (p += 1) {
            backtick_count += 1;
        }
        if (backtick_count > 2) return null;

        const content_start = pos + backtick_count;
        if (content_start >= text.len) return null;

        // Find matching closing backticks (exact count, no trailing backticks)
        var search_pos = content_start;
        while (search_pos + backtick_count <= text.len) : (search_pos += 1) {
            if (!isRepeatedChar(text[search_pos..][0..backtick_count], '`')) continue;

            const after_pos = search_pos + backtick_count;
            if (after_pos < text.len and text[after_pos] == '`') continue;

            return .{
                .content = text[content_start..search_pos],
                .end_pos = after_pos,
            };
        }

        return null;
    }

    fn tryParseStrikethrough(text: []const u8, pos: usize) ?InlineCodeResult {
        // Strikethrough uses ~~ (GFM extension)
        if (pos + 2 > text.len) return null;
        if (text[pos] != '~' or text[pos + 1] != '~') return null;

        const content_start = pos + 2;
        // Find closing ~~
        if (findClosingDelimiter(text, content_start, '~', 2)) |end| {
            return .{
                .content = text[content_start..end],
                .end_pos = end + 2,
            };
        }
        return null;
    }

    fn advanceToNextDelimiter(text: []const u8, start: usize) usize {
        var pos = start;
        while (pos < text.len and text[pos] != '*' and text[pos] != '_' and text[pos] != '[' and text[pos] != '`' and text[pos] != '!' and text[pos] != '~') {
            pos += 1;
        }
        return pos;
    }

    fn findClosingDelimiter(text: []const u8, start: usize, delimiter: u8, count: usize) ?usize {
        if (start >= text.len) return null;

        var pos = start;
        while (pos + count <= text.len) : (pos += 1) {
            const slice = text[pos..][0..count];
            const all_match = for (slice) |c| {
                if (c != delimiter) break false;
            } else true;

            if (!all_match) continue;

            const before_ok = pos == start or text[pos - 1] != delimiter;
            const after_ok = pos + count >= text.len or text[pos + count] != delimiter;
            if (before_ok and after_ok) return pos;
        }
        return null;
    }

    fn renderInlineText(
        self: Markdown,
        text: []const u8,
        base_style: ?Style,
        segments: *std.ArrayList(Segment),
        allocator: std.mem.Allocator,
    ) !void {
        const spans = try parseInlineStyles(text, allocator);
        defer allocator.free(spans);

        for (spans) |span| {
            if (span.style_type == .image) {
                // Images: display as "[Image: alt text]" since terminals can't show images
                const final_style: Style = if (base_style) |bs|
                    bs.combine(self.theme.image_style)
                else
                    self.theme.image_style;

                try segments.append(allocator, Segment.styled(self.theme.image_prefix, final_style));
                if (span.text.len > 0) {
                    try segments.append(allocator, Segment.styled(span.text, final_style));
                } else {
                    // If no alt text, show the URL instead
                    if (span.image_url) |url| {
                        try segments.append(allocator, Segment.styled(url, final_style));
                    }
                }
                try segments.append(allocator, Segment.styled(self.theme.image_suffix, final_style));
                continue;
            }

            const inline_style: ?Style = switch (span.style_type) {
                .plain => null,
                .bold => self.theme.bold_style,
                .italic => self.theme.italic_style,
                .bold_italic => self.theme.bold_italic_style,
                .strikethrough => self.theme.strikethrough_style,
                .code => self.theme.inline_code_style,
                .link => blk: {
                    var link_style = self.theme.link_style;
                    if (span.link_url) |url| {
                        link_style = link_style.hyperlink(url);
                    }
                    break :blk link_style;
                },
                .image => unreachable, // Handled above
            };

            const final_style: ?Style = if (inline_style) |is|
                if (base_style) |bs| bs.combine(is) else is
            else
                base_style;

            if (final_style) |s| {
                try segments.append(allocator, Segment.styled(span.text, s));
            } else {
                try segments.append(allocator, Segment.plain(span.text));
            }
        }
    }
};

test "Markdown.parseHeader valid headers" {
    const h1 = Markdown.parseHeader("# Title");
    try std.testing.expect(h1 != null);
    try std.testing.expectEqual(HeaderLevel.h1, h1.?.level);
    try std.testing.expectEqualStrings("Title", h1.?.text);

    const h3 = Markdown.parseHeader("### Section Name");
    try std.testing.expect(h3 != null);
    try std.testing.expectEqual(HeaderLevel.h3, h3.?.level);
    try std.testing.expectEqualStrings("Section Name", h3.?.text);
}

test "Markdown.parseHeader invalid headers" {
    try std.testing.expect(Markdown.parseHeader("Not a header") == null);
    try std.testing.expect(Markdown.parseHeader("#NoSpace") == null);
    try std.testing.expect(Markdown.parseHeader("####### Too many") == null);
    try std.testing.expect(Markdown.parseHeader("") == null);
}

test "Markdown.parseHeader edge cases" {
    const empty_h1 = Markdown.parseHeader("# ");
    try std.testing.expect(empty_h1 != null);
    try std.testing.expectEqualStrings("", empty_h1.?.text);

    const h6 = Markdown.parseHeader("###### Deep");
    try std.testing.expect(h6 != null);
    try std.testing.expectEqual(HeaderLevel.h6, h6.?.level);
}

test "Markdown.render parses headers" {
    const allocator = std.testing.allocator;
    const md = Markdown.init("# Main Title\n\nSome text\n\n## Subtitle");
    const segments = try md.render(80, allocator);
    defer allocator.free(segments);

    var found_main = false;
    var found_subtitle = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "Main Title")) {
            found_main = true;
            try std.testing.expect(seg.style != null);
        }
        if (std.mem.eql(u8, seg.text, "Subtitle")) {
            found_subtitle = true;
            try std.testing.expect(seg.style != null);
        }
    }
    try std.testing.expect(found_main);
    try std.testing.expect(found_subtitle);
}

test "parseInlineStyles plain text" {
    const allocator = std.testing.allocator;
    const spans = try Markdown.parseInlineStyles("Hello world", allocator);
    defer allocator.free(spans);

    try std.testing.expectEqual(@as(usize, 1), spans.len);
    try std.testing.expectEqualStrings("Hello world", spans[0].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.plain, spans[0].style_type);
}

test "parseInlineStyles single asterisk italic" {
    const allocator = std.testing.allocator;
    const spans = try Markdown.parseInlineStyles("Hello *italic* world", allocator);
    defer allocator.free(spans);

    try std.testing.expectEqual(@as(usize, 3), spans.len);
    try std.testing.expectEqualStrings("Hello ", spans[0].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.plain, spans[0].style_type);
    try std.testing.expectEqualStrings("italic", spans[1].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.italic, spans[1].style_type);
    try std.testing.expectEqualStrings(" world", spans[2].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.plain, spans[2].style_type);
}

test "parseInlineStyles underscore italic" {
    const allocator = std.testing.allocator;
    const spans = try Markdown.parseInlineStyles("Hello _italic_ world", allocator);
    defer allocator.free(spans);

    try std.testing.expectEqual(@as(usize, 3), spans.len);
    try std.testing.expectEqualStrings("italic", spans[1].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.italic, spans[1].style_type);
}

test "parseInlineStyles double asterisk bold" {
    const allocator = std.testing.allocator;
    const spans = try Markdown.parseInlineStyles("Hello **bold** world", allocator);
    defer allocator.free(spans);

    try std.testing.expectEqual(@as(usize, 3), spans.len);
    try std.testing.expectEqualStrings("bold", spans[1].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.bold, spans[1].style_type);
}

test "parseInlineStyles double underscore bold" {
    const allocator = std.testing.allocator;
    const spans = try Markdown.parseInlineStyles("Hello __bold__ world", allocator);
    defer allocator.free(spans);

    try std.testing.expectEqual(@as(usize, 3), spans.len);
    try std.testing.expectEqualStrings("bold", spans[1].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.bold, spans[1].style_type);
}

test "parseInlineStyles triple asterisk bold italic" {
    const allocator = std.testing.allocator;
    const spans = try Markdown.parseInlineStyles("Hello ***bold italic*** world", allocator);
    defer allocator.free(spans);

    try std.testing.expectEqual(@as(usize, 3), spans.len);
    try std.testing.expectEqualStrings("bold italic", spans[1].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.bold_italic, spans[1].style_type);
}

test "parseInlineStyles triple underscore bold italic" {
    const allocator = std.testing.allocator;
    const spans = try Markdown.parseInlineStyles("Hello ___bold italic___ world", allocator);
    defer allocator.free(spans);

    try std.testing.expectEqual(@as(usize, 3), spans.len);
    try std.testing.expectEqualStrings("bold italic", spans[1].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.bold_italic, spans[1].style_type);
}

test "parseInlineStyles strikethrough" {
    const allocator = std.testing.allocator;
    const spans = try Markdown.parseInlineStyles("Hello ~~deleted~~ world", allocator);
    defer allocator.free(spans);

    try std.testing.expectEqual(@as(usize, 3), spans.len);
    try std.testing.expectEqualStrings("deleted", spans[1].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.strikethrough, spans[1].style_type);
}

test "parseInlineStyles strikethrough at start" {
    const allocator = std.testing.allocator;
    const spans = try Markdown.parseInlineStyles("~~strike~~ at start", allocator);
    defer allocator.free(spans);

    try std.testing.expectEqual(@as(usize, 2), spans.len);
    try std.testing.expectEqualStrings("strike", spans[0].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.strikethrough, spans[0].style_type);
}

test "parseInlineStyles strikethrough at end" {
    const allocator = std.testing.allocator;
    const spans = try Markdown.parseInlineStyles("end with ~~strike~~", allocator);
    defer allocator.free(spans);

    try std.testing.expectEqual(@as(usize, 2), spans.len);
    try std.testing.expectEqualStrings("strike", spans[1].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.strikethrough, spans[1].style_type);
}

test "parseInlineStyles strikethrough with bold" {
    const allocator = std.testing.allocator;
    const spans = try Markdown.parseInlineStyles("~~strike~~ and **bold**", allocator);
    defer allocator.free(spans);

    try std.testing.expectEqual(@as(usize, 3), spans.len);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.strikethrough, spans[0].style_type);
    try std.testing.expectEqualStrings("strike", spans[0].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.bold, spans[2].style_type);
    try std.testing.expectEqualStrings("bold", spans[2].text);
}

test "parseInlineStyles unclosed strikethrough treated as plain" {
    const allocator = std.testing.allocator;
    const spans = try Markdown.parseInlineStyles("Hello ~~unclosed", allocator);
    defer allocator.free(spans);

    // All text becomes plain since ~~ is not closed
    try std.testing.expect(spans.len >= 1);
    for (spans) |span| {
        try std.testing.expectEqual(Markdown.InlineSpan.StyleType.plain, span.style_type);
    }
}

test "parseInlineStyles multiple styles in one line" {
    const allocator = std.testing.allocator;
    const spans = try Markdown.parseInlineStyles("*italic* and **bold**", allocator);
    defer allocator.free(spans);

    // Expected: italic, " and ", bold (no trailing text)
    try std.testing.expectEqual(@as(usize, 3), spans.len);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.italic, spans[0].style_type);
    try std.testing.expectEqualStrings("italic", spans[0].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.plain, spans[1].style_type);
    try std.testing.expectEqualStrings(" and ", spans[1].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.bold, spans[2].style_type);
    try std.testing.expectEqualStrings("bold", spans[2].text);
}

test "parseInlineStyles at start of line" {
    const allocator = std.testing.allocator;
    const spans = try Markdown.parseInlineStyles("**bold** at start", allocator);
    defer allocator.free(spans);

    try std.testing.expectEqual(@as(usize, 2), spans.len);
    try std.testing.expectEqualStrings("bold", spans[0].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.bold, spans[0].style_type);
}

test "parseInlineStyles at end of line" {
    const allocator = std.testing.allocator;
    const spans = try Markdown.parseInlineStyles("end with *italic*", allocator);
    defer allocator.free(spans);

    try std.testing.expectEqual(@as(usize, 2), spans.len);
    try std.testing.expectEqualStrings("italic", spans[1].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.italic, spans[1].style_type);
}

test "parseInlineStyles unclosed delimiter treated as plain" {
    const allocator = std.testing.allocator;
    const spans = try Markdown.parseInlineStyles("Hello *unclosed", allocator);
    defer allocator.free(spans);

    // Unclosed delimiter should be treated as plain text
    try std.testing.expect(spans.len >= 1);
    // The text should be preserved
    var total_len: usize = 0;
    for (spans) |span| {
        total_len += span.text.len;
    }
    try std.testing.expectEqual(@as(usize, 15), total_len);
}

test "Markdown.render with bold text" {
    const allocator = std.testing.allocator;
    const md = Markdown.init("This is **bold** text");
    const segments = try md.render(80, allocator);
    defer allocator.free(segments);

    var found_bold = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "bold")) {
            found_bold = true;
            try std.testing.expect(seg.style != null);
            try std.testing.expect(seg.style.?.hasAttribute(.bold));
        }
    }
    try std.testing.expect(found_bold);
}

test "Markdown.render with italic text" {
    const allocator = std.testing.allocator;
    const md = Markdown.init("This is *italic* text");
    const segments = try md.render(80, allocator);
    defer allocator.free(segments);

    var found_italic = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "italic")) {
            found_italic = true;
            try std.testing.expect(seg.style != null);
            try std.testing.expect(seg.style.?.hasAttribute(.italic));
        }
    }
    try std.testing.expect(found_italic);
}

test "Markdown.render with bold italic text" {
    const allocator = std.testing.allocator;
    const md = Markdown.init("This is ***bold italic*** text");
    const segments = try md.render(80, allocator);
    defer allocator.free(segments);

    var found_bold_italic = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "bold italic")) {
            found_bold_italic = true;
            try std.testing.expect(seg.style != null);
            try std.testing.expect(seg.style.?.hasAttribute(.bold));
            try std.testing.expect(seg.style.?.hasAttribute(.italic));
        }
    }
    try std.testing.expect(found_bold_italic);
}

test "Markdown.render with strikethrough text" {
    const allocator = std.testing.allocator;
    const md = Markdown.init("This is ~~deleted~~ text");
    const segments = try md.render(80, allocator);
    defer allocator.free(segments);

    var found_strike = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "deleted")) {
            found_strike = true;
            try std.testing.expect(seg.style != null);
            try std.testing.expect(seg.style.?.hasAttribute(.strike));
        }
    }
    try std.testing.expect(found_strike);
}

test "MarkdownTheme has bold and italic styles" {
    const theme = MarkdownTheme.default;
    try std.testing.expect(theme.bold_style.hasAttribute(.bold));
    try std.testing.expect(theme.italic_style.hasAttribute(.italic));
    try std.testing.expect(theme.bold_italic_style.hasAttribute(.bold));
    try std.testing.expect(theme.bold_italic_style.hasAttribute(.italic));
}

test "MarkdownTheme has strikethrough style" {
    const theme = MarkdownTheme.default;
    try std.testing.expect(theme.strikethrough_style.hasAttribute(.strike));
}

test "parseFenceOpen detects code fence" {
    try std.testing.expectEqualStrings("", Markdown.parseFenceOpen("```").?);
    try std.testing.expectEqualStrings("zig", Markdown.parseFenceOpen("```zig").?);
    try std.testing.expectEqualStrings("json", Markdown.parseFenceOpen("```json").?);
    try std.testing.expectEqualStrings("zig", Markdown.parseFenceOpen("```zig  ").?);
    try std.testing.expect(Markdown.parseFenceOpen("``") == null);
    try std.testing.expect(Markdown.parseFenceOpen("not a fence") == null);
}

test "parseFenceClose detects closing fence" {
    try std.testing.expect(Markdown.parseFenceClose("```"));
    try std.testing.expect(Markdown.parseFenceClose("```  "));
    try std.testing.expect(!Markdown.parseFenceClose("``"));
    try std.testing.expect(!Markdown.parseFenceClose("```zig"));
}

test "Markdown.render fenced code block plain" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const md = Markdown.init("```\ncode here\n```");
    const segments = try md.render(80, arena.allocator());

    var found_code = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "code here")) {
            found_code = true;
        }
    }
    try std.testing.expect(found_code);
}

test "Markdown.render fenced code block with language" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const md = Markdown.init("```zig\nconst x = 1;\n```");
    const segments = try md.render(80, arena.allocator());

    var found_const = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "const")) {
            found_const = true;
            try std.testing.expect(seg.style != null);
        }
    }
    try std.testing.expect(found_const);
}

test "Markdown.render mixed content with code block" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const source =
        \\# Title
        \\
        \\Some text
        \\
        \\```zig
        \\const x = 42;
        \\```
        \\
        \\More text
    ;
    const md = Markdown.init(source);
    const segments = try md.render(80, arena.allocator());

    var found_title = false;
    var found_code = false;
    var found_more = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "Title")) found_title = true;
        if (std.mem.eql(u8, seg.text, "const")) found_code = true;
        if (std.mem.eql(u8, seg.text, "More text")) found_more = true;
    }
    try std.testing.expect(found_title);
    try std.testing.expect(found_code);
    try std.testing.expect(found_more);
}

test "Markdown.render unclosed code block" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const md = Markdown.init("```zig\ncode without closing");
    const segments = try md.render(80, arena.allocator());

    var found_code = false;
    for (segments) |seg| {
        if (std.mem.indexOf(u8, seg.text, "code") != null) {
            found_code = true;
        }
    }
    try std.testing.expect(found_code);
}

test "Markdown.render multiple code blocks" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const source =
        \\```zig
        \\const a = 1;
        \\```
        \\
        \\```json
        \\{"key": "value"}
        \\```
    ;
    const md = Markdown.init(source);
    const segments = try md.render(80, arena.allocator());

    var found_const = false;
    var found_key = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "const")) found_const = true;
        if (std.mem.indexOf(u8, seg.text, "key") != null) found_key = true;
    }
    try std.testing.expect(found_const);
    try std.testing.expect(found_key);
}

test "tryParseLink basic link" {
    const text = "[click here](https://example.com)";
    const result = Markdown.tryParseLink(text, 0);
    try std.testing.expect(result != null);
    try std.testing.expectEqualStrings("click here", result.?.text);
    try std.testing.expectEqualStrings("https://example.com", result.?.url);
    try std.testing.expectEqual(@as(usize, text.len), result.?.end_pos);
}

test "tryParseLink no link" {
    try std.testing.expect(Markdown.tryParseLink("plain text", 0) == null);
    try std.testing.expect(Markdown.tryParseLink("[unclosed", 0) == null);
    try std.testing.expect(Markdown.tryParseLink("[text]no paren", 0) == null);
    try std.testing.expect(Markdown.tryParseLink("[text](unclosed", 0) == null);
}

test "tryParseLink with offset" {
    const text = "prefix [link](url) suffix";
    const result = Markdown.tryParseLink(text, 7);
    try std.testing.expect(result != null);
    try std.testing.expectEqualStrings("link", result.?.text);
    try std.testing.expectEqualStrings("url", result.?.url);
}

test "tryParseLink nested brackets" {
    const text = "[text [nested]](url)";
    const result = Markdown.tryParseLink(text, 0);
    try std.testing.expect(result != null);
    try std.testing.expectEqualStrings("text [nested]", result.?.text);
}

test "tryParseLink nested parens in url" {
    const text = "[link](https://example.com/path(1))";
    const result = Markdown.tryParseLink(text, 0);
    try std.testing.expect(result != null);
    try std.testing.expectEqualStrings("https://example.com/path(1)", result.?.url);
}

test "parseInlineStyles with link" {
    const allocator = std.testing.allocator;
    const spans = try Markdown.parseInlineStyles("Check [this](https://example.com) out", allocator);
    defer allocator.free(spans);

    try std.testing.expectEqual(@as(usize, 3), spans.len);
    try std.testing.expectEqualStrings("Check ", spans[0].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.plain, spans[0].style_type);
    try std.testing.expectEqualStrings("this", spans[1].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.link, spans[1].style_type);
    try std.testing.expectEqualStrings("https://example.com", spans[1].link_url.?);
    try std.testing.expectEqualStrings(" out", spans[2].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.plain, spans[2].style_type);
}

test "Markdown.render with link" {
    const allocator = std.testing.allocator;
    const md = Markdown.init("Visit [Example](https://example.com) for more info");
    const segments = try md.render(80, allocator);
    defer allocator.free(segments);

    var found_link = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "Example")) {
            found_link = true;
            try std.testing.expect(seg.style != null);
            try std.testing.expect(seg.style.?.link != null);
            try std.testing.expectEqualStrings("https://example.com", seg.style.?.link.?);
            try std.testing.expect(seg.style.?.hasAttribute(.underline));
        }
    }
    try std.testing.expect(found_link);
}

test "Markdown.render link at start" {
    const allocator = std.testing.allocator;
    const md = Markdown.init("[Start](url) of line");
    const segments = try md.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len >= 2);
    try std.testing.expectEqualStrings("Start", segments[0].text);
    try std.testing.expect(segments[0].style.?.link != null);
}

test "Markdown.render link at end" {
    const allocator = std.testing.allocator;
    const md = Markdown.init("End with [link](url)");
    const segments = try md.render(80, allocator);
    defer allocator.free(segments);

    var found_link = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "link")) {
            found_link = true;
            try std.testing.expect(seg.style.?.link != null);
        }
    }
    try std.testing.expect(found_link);
}

test "Markdown.render multiple links" {
    const allocator = std.testing.allocator;
    const md = Markdown.init("[one](url1) and [two](url2)");
    const segments = try md.render(80, allocator);
    defer allocator.free(segments);

    var found_one = false;
    var found_two = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "one")) {
            found_one = true;
            try std.testing.expectEqualStrings("url1", seg.style.?.link.?);
        }
        if (std.mem.eql(u8, seg.text, "two")) {
            found_two = true;
            try std.testing.expectEqualStrings("url2", seg.style.?.link.?);
        }
    }
    try std.testing.expect(found_one);
    try std.testing.expect(found_two);
}

test "MarkdownTheme has link style" {
    const theme = MarkdownTheme.default;
    try std.testing.expect(theme.link_style.hasAttribute(.underline));
    try std.testing.expect(theme.link_style.color != null);
}

test "parseOrderedListItem basic" {
    const item = Markdown.parseOrderedListItem("1. First item");
    try std.testing.expect(item != null);
    try std.testing.expectEqual(@as(usize, 1), item.?.number);
    try std.testing.expectEqualStrings("First item", item.?.text);
    try std.testing.expectEqual(@as(usize, 0), item.?.indent_level);
}

test "parseOrderedListItem various numbers" {
    const item2 = Markdown.parseOrderedListItem("2. Second");
    try std.testing.expect(item2 != null);
    try std.testing.expectEqual(@as(usize, 2), item2.?.number);

    const item10 = Markdown.parseOrderedListItem("10. Tenth");
    try std.testing.expect(item10 != null);
    try std.testing.expectEqual(@as(usize, 10), item10.?.number);

    const item99 = Markdown.parseOrderedListItem("99. Large");
    try std.testing.expect(item99 != null);
    try std.testing.expectEqual(@as(usize, 99), item99.?.number);
}

test "parseOrderedListItem with paren delimiter" {
    const item = Markdown.parseOrderedListItem("1) First item");
    try std.testing.expect(item != null);
    try std.testing.expectEqual(@as(usize, 1), item.?.number);
    try std.testing.expectEqualStrings("First item", item.?.text);
}

test "parseOrderedListItem with indentation" {
    const item1 = Markdown.parseOrderedListItem("  1. Indented once");
    try std.testing.expect(item1 != null);
    try std.testing.expectEqual(@as(usize, 1), item1.?.indent_level);
    try std.testing.expectEqualStrings("Indented once", item1.?.text);

    const item2 = Markdown.parseOrderedListItem("    1. Indented twice");
    try std.testing.expect(item2 != null);
    try std.testing.expectEqual(@as(usize, 2), item2.?.indent_level);
}

test "parseOrderedListItem invalid" {
    try std.testing.expect(Markdown.parseOrderedListItem("Not a list") == null);
    try std.testing.expect(Markdown.parseOrderedListItem("1 No delimiter") == null);
    try std.testing.expect(Markdown.parseOrderedListItem("1.NoSpace") == null);
    try std.testing.expect(Markdown.parseOrderedListItem(". No number") == null);
    try std.testing.expect(Markdown.parseOrderedListItem("") == null);
    try std.testing.expect(Markdown.parseOrderedListItem("a. Letter") == null);
}

test "Markdown.render ordered list" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const md = Markdown.init("1. First\n2. Second\n3. Third");
    const segments = try md.render(80, arena.allocator());

    var found_first = false;
    var found_second = false;
    var found_third = false;
    var found_num_1 = false;
    var found_num_2 = false;
    var found_num_3 = false;

    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "First")) found_first = true;
        if (std.mem.eql(u8, seg.text, "Second")) found_second = true;
        if (std.mem.eql(u8, seg.text, "Third")) found_third = true;
        if (std.mem.eql(u8, seg.text, "1. ")) {
            found_num_1 = true;
            try std.testing.expect(seg.style != null);
        }
        if (std.mem.eql(u8, seg.text, "2. ")) {
            found_num_2 = true;
            try std.testing.expect(seg.style != null);
        }
        if (std.mem.eql(u8, seg.text, "3. ")) {
            found_num_3 = true;
            try std.testing.expect(seg.style != null);
        }
    }

    try std.testing.expect(found_first);
    try std.testing.expect(found_second);
    try std.testing.expect(found_third);
    try std.testing.expect(found_num_1);
    try std.testing.expect(found_num_2);
    try std.testing.expect(found_num_3);
}

test "Markdown.render ordered list with inline styles" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const md = Markdown.init("1. Item with **bold** text");
    const segments = try md.render(80, arena.allocator());

    var found_bold = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "bold")) {
            found_bold = true;
            try std.testing.expect(seg.style != null);
            try std.testing.expect(seg.style.?.hasAttribute(.bold));
        }
    }
    try std.testing.expect(found_bold);
}

test "Markdown.render nested ordered list" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const source =
        \\1. Outer item
        \\  1. Nested item
        \\  2. Another nested
        \\2. Back to outer
    ;
    const md = Markdown.init(source);
    const segments = try md.render(80, arena.allocator());

    var found_outer = false;
    var found_nested = false;
    var found_indent = false;

    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "Outer item")) found_outer = true;
        if (std.mem.eql(u8, seg.text, "Nested item")) found_nested = true;
        // Check for indent spaces (3 spaces per indent level)
        if (seg.text.len == 3) {
            var all_spaces = true;
            for (seg.text) |c| {
                if (c != ' ') all_spaces = false;
            }
            if (all_spaces) found_indent = true;
        }
    }

    try std.testing.expect(found_outer);
    try std.testing.expect(found_nested);
    try std.testing.expect(found_indent);
}

test "Markdown.render ordered list mixed with other elements" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const source =
        \\# Header
        \\
        \\1. First item
        \\2. Second item
        \\
        \\Some text
    ;
    const md = Markdown.init(source);
    const segments = try md.render(80, arena.allocator());

    var found_header = false;
    var found_first = false;
    var found_text = false;

    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "Header")) found_header = true;
        if (std.mem.eql(u8, seg.text, "First item")) found_first = true;
        if (std.mem.eql(u8, seg.text, "Some text")) found_text = true;
    }

    try std.testing.expect(found_header);
    try std.testing.expect(found_first);
    try std.testing.expect(found_text);
}

test "MarkdownTheme has list styles" {
    const theme = MarkdownTheme.default;
    try std.testing.expect(theme.list_number_style.color != null);
    try std.testing.expect(theme.list_bullet_style.color != null);
    try std.testing.expectEqual(@as(usize, 3), theme.list_indent);
}

test "parseUnorderedListItem dash" {
    const item = Markdown.parseUnorderedListItem("- First item");
    try std.testing.expect(item != null);
    try std.testing.expectEqualStrings("First item", item.?.text);
    try std.testing.expectEqual(@as(usize, 0), item.?.indent_level);
}

test "parseUnorderedListItem asterisk" {
    const item = Markdown.parseUnorderedListItem("* Asterisk item");
    try std.testing.expect(item != null);
    try std.testing.expectEqualStrings("Asterisk item", item.?.text);
}

test "parseUnorderedListItem plus" {
    const item = Markdown.parseUnorderedListItem("+ Plus item");
    try std.testing.expect(item != null);
    try std.testing.expectEqualStrings("Plus item", item.?.text);
}

test "parseUnorderedListItem with indentation" {
    const item1 = Markdown.parseUnorderedListItem("  - Indented once");
    try std.testing.expect(item1 != null);
    try std.testing.expectEqual(@as(usize, 1), item1.?.indent_level);
    try std.testing.expectEqualStrings("Indented once", item1.?.text);

    const item2 = Markdown.parseUnorderedListItem("    - Indented twice");
    try std.testing.expect(item2 != null);
    try std.testing.expectEqual(@as(usize, 2), item2.?.indent_level);
}

test "parseUnorderedListItem invalid" {
    try std.testing.expect(Markdown.parseUnorderedListItem("Not a list") == null);
    try std.testing.expect(Markdown.parseUnorderedListItem("-NoSpace") == null);
    try std.testing.expect(Markdown.parseUnorderedListItem("") == null);
    try std.testing.expect(Markdown.parseUnorderedListItem("-") == null);
}

test "Markdown.render unordered list" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const md = Markdown.init("- First\n- Second\n- Third");
    const segments = try md.render(80, arena.allocator());

    var found_first = false;
    var found_second = false;
    var found_third = false;
    var found_bullet = false;

    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "First")) found_first = true;
        if (std.mem.eql(u8, seg.text, "Second")) found_second = true;
        if (std.mem.eql(u8, seg.text, "Third")) found_third = true;
        if (std.mem.indexOf(u8, seg.text, "\u{2022}") != null) {
            found_bullet = true;
            try std.testing.expect(seg.style != null);
        }
    }

    try std.testing.expect(found_first);
    try std.testing.expect(found_second);
    try std.testing.expect(found_third);
    try std.testing.expect(found_bullet);
}

test "Markdown.render unordered list with inline styles" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const md = Markdown.init("- Item with **bold** text");
    const segments = try md.render(80, arena.allocator());

    var found_bold = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "bold")) {
            found_bold = true;
            try std.testing.expect(seg.style != null);
            try std.testing.expect(seg.style.?.hasAttribute(.bold));
        }
    }
    try std.testing.expect(found_bold);
}

test "Markdown.render nested unordered list" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const source =
        \\- Outer item
        \\  - Nested item
        \\  - Another nested
        \\- Back to outer
    ;
    const md = Markdown.init(source);
    const segments = try md.render(80, arena.allocator());

    var found_outer = false;
    var found_nested = false;
    var found_indent = false;

    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "Outer item")) found_outer = true;
        if (std.mem.eql(u8, seg.text, "Nested item")) found_nested = true;
        if (seg.text.len == 3) {
            var all_spaces = true;
            for (seg.text) |c| {
                if (c != ' ') all_spaces = false;
            }
            if (all_spaces) found_indent = true;
        }
    }

    try std.testing.expect(found_outer);
    try std.testing.expect(found_nested);
    try std.testing.expect(found_indent);
}

test "Markdown.render unordered list mixed with other elements" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const source =
        \\# Header
        \\
        \\- First item
        \\- Second item
        \\
        \\Some text
    ;
    const md = Markdown.init(source);
    const segments = try md.render(80, arena.allocator());

    var found_header = false;
    var found_first = false;
    var found_text = false;

    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "Header")) found_header = true;
        if (std.mem.eql(u8, seg.text, "First item")) found_first = true;
        if (std.mem.eql(u8, seg.text, "Some text")) found_text = true;
    }

    try std.testing.expect(found_header);
    try std.testing.expect(found_first);
    try std.testing.expect(found_text);
}

test "Markdown.render mixed ordered and unordered lists" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const source =
        \\1. Ordered first
        \\2. Ordered second
        \\
        \\- Unordered first
        \\- Unordered second
    ;
    const md = Markdown.init(source);
    const segments = try md.render(80, arena.allocator());

    var found_ordered = false;
    var found_unordered = false;
    var found_number = false;
    var found_bullet = false;

    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "Ordered first")) found_ordered = true;
        if (std.mem.eql(u8, seg.text, "Unordered first")) found_unordered = true;
        if (std.mem.eql(u8, seg.text, "1. ")) found_number = true;
        if (std.mem.indexOf(u8, seg.text, "\u{2022}") != null) found_bullet = true;
    }

    try std.testing.expect(found_ordered);
    try std.testing.expect(found_unordered);
    try std.testing.expect(found_number);
    try std.testing.expect(found_bullet);
}

test "parseUnorderedListItem task list unchecked" {
    const item = Markdown.parseUnorderedListItem("- [ ] Unchecked task");
    try std.testing.expect(item != null);
    try std.testing.expectEqualStrings("Unchecked task", item.?.text);
    try std.testing.expectEqual(@as(usize, 0), item.?.indent_level);
    try std.testing.expect(item.?.checkbox != null);
    try std.testing.expectEqual(Markdown.TaskCheckbox.unchecked, item.?.checkbox.?);
}

test "parseUnorderedListItem task list checked lowercase" {
    const item = Markdown.parseUnorderedListItem("- [x] Checked task");
    try std.testing.expect(item != null);
    try std.testing.expectEqualStrings("Checked task", item.?.text);
    try std.testing.expect(item.?.checkbox != null);
    try std.testing.expectEqual(Markdown.TaskCheckbox.checked, item.?.checkbox.?);
}

test "parseUnorderedListItem task list checked uppercase" {
    const item = Markdown.parseUnorderedListItem("- [X] Checked task uppercase");
    try std.testing.expect(item != null);
    try std.testing.expectEqualStrings("Checked task uppercase", item.?.text);
    try std.testing.expect(item.?.checkbox != null);
    try std.testing.expectEqual(Markdown.TaskCheckbox.checked, item.?.checkbox.?);
}

test "parseUnorderedListItem task list with asterisk bullet" {
    const item = Markdown.parseUnorderedListItem("* [ ] Task with asterisk");
    try std.testing.expect(item != null);
    try std.testing.expectEqualStrings("Task with asterisk", item.?.text);
    try std.testing.expect(item.?.checkbox != null);
    try std.testing.expectEqual(Markdown.TaskCheckbox.unchecked, item.?.checkbox.?);
}

test "parseUnorderedListItem task list with indentation" {
    const item = Markdown.parseUnorderedListItem("  - [x] Indented task");
    try std.testing.expect(item != null);
    try std.testing.expectEqualStrings("Indented task", item.?.text);
    try std.testing.expectEqual(@as(usize, 1), item.?.indent_level);
    try std.testing.expect(item.?.checkbox != null);
    try std.testing.expectEqual(Markdown.TaskCheckbox.checked, item.?.checkbox.?);
}

test "parseUnorderedListItem regular list no checkbox" {
    const item = Markdown.parseUnorderedListItem("- Regular item");
    try std.testing.expect(item != null);
    try std.testing.expectEqualStrings("Regular item", item.?.text);
    try std.testing.expect(item.?.checkbox == null);
}

test "MarkdownTheme has task list styles" {
    const theme = MarkdownTheme.default;
    try std.testing.expect(theme.task_unchecked_style.color != null);
    try std.testing.expect(theme.task_checked_style.color != null);
    try std.testing.expectEqualStrings("\u{2610}", theme.task_unchecked_char);
    try std.testing.expectEqualStrings("\u{2611}", theme.task_checked_char);
}

test "Markdown.render task list" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const source =
        \\- [ ] Unchecked task
        \\- [x] Checked task
        \\- Regular item
    ;
    const md = Markdown.init(source);
    const segments = try md.render(80, arena.allocator());

    var found_unchecked_box = false;
    var found_checked_box = false;
    var found_regular_bullet = false;
    var found_unchecked_text = false;
    var found_checked_text = false;
    var found_regular_text = false;

    for (segments) |seg| {
        if (std.mem.indexOf(u8, seg.text, "\u{2610}") != null) found_unchecked_box = true;
        if (std.mem.indexOf(u8, seg.text, "\u{2611}") != null) found_checked_box = true;
        if (std.mem.indexOf(u8, seg.text, "\u{2022}") != null) found_regular_bullet = true;
        if (std.mem.eql(u8, seg.text, "Unchecked task")) found_unchecked_text = true;
        if (std.mem.eql(u8, seg.text, "Checked task")) found_checked_text = true;
        if (std.mem.eql(u8, seg.text, "Regular item")) found_regular_text = true;
    }

    try std.testing.expect(found_unchecked_box);
    try std.testing.expect(found_checked_box);
    try std.testing.expect(found_regular_bullet);
    try std.testing.expect(found_unchecked_text);
    try std.testing.expect(found_checked_text);
    try std.testing.expect(found_regular_text);
}

test "Markdown.render nested task list" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const source =
        \\- [ ] Parent task
        \\  - [x] Completed subtask
        \\  - [ ] Pending subtask
    ;
    const md = Markdown.init(source);
    const segments = try md.render(80, arena.allocator());

    var found_parent = false;
    var found_completed = false;
    var found_pending = false;
    var found_indent = false;

    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "Parent task")) found_parent = true;
        if (std.mem.eql(u8, seg.text, "Completed subtask")) found_completed = true;
        if (std.mem.eql(u8, seg.text, "Pending subtask")) found_pending = true;
        if (seg.text.len == 3) {
            var all_spaces = true;
            for (seg.text) |c| {
                if (c != ' ') all_spaces = false;
            }
            if (all_spaces) found_indent = true;
        }
    }

    try std.testing.expect(found_parent);
    try std.testing.expect(found_completed);
    try std.testing.expect(found_pending);
    try std.testing.expect(found_indent);
}

test "parseBlockquote basic" {
    const item = Markdown.parseBlockquote("> Quote text");
    try std.testing.expect(item != null);
    try std.testing.expectEqual(@as(usize, 1), item.?.depth);
    try std.testing.expectEqualStrings("Quote text", item.?.text);
}

test "parseBlockquote no space after marker" {
    const item = Markdown.parseBlockquote(">Quote text");
    try std.testing.expect(item != null);
    try std.testing.expectEqual(@as(usize, 1), item.?.depth);
    try std.testing.expectEqualStrings("Quote text", item.?.text);
}

test "parseBlockquote nested" {
    const item = Markdown.parseBlockquote(">> Nested quote");
    try std.testing.expect(item != null);
    try std.testing.expectEqual(@as(usize, 2), item.?.depth);
    try std.testing.expectEqualStrings("Nested quote", item.?.text);
}

test "parseBlockquote deeply nested" {
    const item = Markdown.parseBlockquote(">>> Deep quote");
    try std.testing.expect(item != null);
    try std.testing.expectEqual(@as(usize, 3), item.?.depth);
    try std.testing.expectEqualStrings("Deep quote", item.?.text);
}

test "parseBlockquote with spaces between markers" {
    const item = Markdown.parseBlockquote("> > Spaced nested");
    try std.testing.expect(item != null);
    try std.testing.expectEqual(@as(usize, 2), item.?.depth);
    try std.testing.expectEqualStrings("Spaced nested", item.?.text);
}

test "parseBlockquote empty line" {
    const item = Markdown.parseBlockquote(">");
    try std.testing.expect(item != null);
    try std.testing.expectEqual(@as(usize, 1), item.?.depth);
    try std.testing.expectEqualStrings("", item.?.text);
}

test "parseBlockquote leading spaces" {
    const item = Markdown.parseBlockquote("  > Indented quote");
    try std.testing.expect(item != null);
    try std.testing.expectEqual(@as(usize, 1), item.?.depth);
    try std.testing.expectEqualStrings("Indented quote", item.?.text);
}

test "parseBlockquote not a quote" {
    try std.testing.expect(Markdown.parseBlockquote("Not a quote") == null);
    try std.testing.expect(Markdown.parseBlockquote("") == null);
    try std.testing.expect(Markdown.parseBlockquote("- List item") == null);
}

test "Markdown.render blockquote basic" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const md = Markdown.init("> This is a quote");
    const segments = try md.render(80, arena.allocator());

    var found_border = false;
    var found_text = false;

    for (segments) |seg| {
        if (std.mem.indexOf(u8, seg.text, "\u{2502}") != null) {
            found_border = true;
            try std.testing.expect(seg.style != null);
        }
        if (std.mem.eql(u8, seg.text, "This is a quote")) {
            found_text = true;
            try std.testing.expect(seg.style != null);
            try std.testing.expect(seg.style.?.hasAttribute(.italic));
        }
    }

    try std.testing.expect(found_border);
    try std.testing.expect(found_text);
}

test "Markdown.render multiline blockquote" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const source =
        \\> First line
        \\> Second line
        \\> Third line
    ;
    const md = Markdown.init(source);
    const segments = try md.render(80, arena.allocator());

    var found_first = false;
    var found_second = false;
    var found_third = false;
    var border_count: usize = 0;

    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "First line")) found_first = true;
        if (std.mem.eql(u8, seg.text, "Second line")) found_second = true;
        if (std.mem.eql(u8, seg.text, "Third line")) found_third = true;
        if (std.mem.indexOf(u8, seg.text, "\u{2502}") != null) border_count += 1;
    }

    try std.testing.expect(found_first);
    try std.testing.expect(found_second);
    try std.testing.expect(found_third);
    try std.testing.expect(border_count >= 3);
}

test "Markdown.render nested blockquote" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const source =
        \\> Outer quote
        \\>> Nested quote
        \\> Back to outer
    ;
    const md = Markdown.init(source);
    const segments = try md.render(80, arena.allocator());

    var found_outer = false;
    var found_nested = false;
    var max_border_count: usize = 0;
    var current_border_count: usize = 0;

    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "\n")) {
            if (current_border_count > max_border_count) {
                max_border_count = current_border_count;
            }
            current_border_count = 0;
        }
        if (std.mem.eql(u8, seg.text, "Outer quote") or std.mem.eql(u8, seg.text, "Back to outer")) {
            found_outer = true;
        }
        if (std.mem.eql(u8, seg.text, "Nested quote")) {
            found_nested = true;
        }
        if (std.mem.indexOf(u8, seg.text, "\u{2502}") != null) {
            current_border_count += 1;
        }
    }

    try std.testing.expect(found_outer);
    try std.testing.expect(found_nested);
    try std.testing.expect(max_border_count >= 2);
}

test "Markdown.render blockquote with inline styles" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const md = Markdown.init("> Quote with **bold** text");
    const segments = try md.render(80, arena.allocator());

    var found_bold = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "bold")) {
            found_bold = true;
            try std.testing.expect(seg.style != null);
            try std.testing.expect(seg.style.?.hasAttribute(.bold));
        }
    }
    try std.testing.expect(found_bold);
}

test "Markdown.render blockquote mixed with other elements" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const source =
        \\# Header
        \\
        \\> A blockquote
        \\
        \\Normal paragraph
    ;
    const md = Markdown.init(source);
    const segments = try md.render(80, arena.allocator());

    var found_header = false;
    var found_quote = false;
    var found_paragraph = false;

    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "Header")) found_header = true;
        if (std.mem.eql(u8, seg.text, "A blockquote")) found_quote = true;
        if (std.mem.eql(u8, seg.text, "Normal paragraph")) found_paragraph = true;
    }

    try std.testing.expect(found_header);
    try std.testing.expect(found_quote);
    try std.testing.expect(found_paragraph);
}

test "MarkdownTheme has blockquote styles" {
    const theme = MarkdownTheme.default;
    try std.testing.expect(theme.blockquote_style.hasAttribute(.italic));
    try std.testing.expect(theme.blockquote_border_style.color != null);
    try std.testing.expectEqualStrings("\u{2502}", theme.blockquote_border_char);
    try std.testing.expectEqual(@as(usize, 2), theme.blockquote_indent);
}

test "tryParseInlineCode basic" {
    const result = Markdown.tryParseInlineCode("`code`", 0);
    try std.testing.expect(result != null);
    try std.testing.expectEqualStrings("code", result.?.content);
    try std.testing.expectEqual(@as(usize, 6), result.?.end_pos);
}

test "tryParseInlineCode double backticks" {
    const result = Markdown.tryParseInlineCode("``code with `backtick` inside``", 0);
    try std.testing.expect(result != null);
    try std.testing.expectEqualStrings("code with `backtick` inside", result.?.content);
    try std.testing.expectEqual(@as(usize, 31), result.?.end_pos);
}

test "tryParseInlineCode with offset" {
    const text = "text `code` more";
    const result = Markdown.tryParseInlineCode(text, 5);
    try std.testing.expect(result != null);
    try std.testing.expectEqualStrings("code", result.?.content);
    try std.testing.expectEqual(@as(usize, 11), result.?.end_pos);
}

test "tryParseInlineCode unclosed" {
    try std.testing.expect(Markdown.tryParseInlineCode("`unclosed", 0) == null);
    try std.testing.expect(Markdown.tryParseInlineCode("``unclosed`", 0) == null);
}

test "tryParseInlineCode not at backtick" {
    try std.testing.expect(Markdown.tryParseInlineCode("text", 0) == null);
    try std.testing.expect(Markdown.tryParseInlineCode("", 0) == null);
}

test "tryParseInlineCode triple backticks ignored" {
    try std.testing.expect(Markdown.tryParseInlineCode("```code```", 0) == null);
}

test "parseInlineStyles with inline code" {
    const allocator = std.testing.allocator;
    const spans = try Markdown.parseInlineStyles("Use `code` here", allocator);
    defer allocator.free(spans);

    try std.testing.expectEqual(@as(usize, 3), spans.len);
    try std.testing.expectEqualStrings("Use ", spans[0].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.plain, spans[0].style_type);
    try std.testing.expectEqualStrings("code", spans[1].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.code, spans[1].style_type);
    try std.testing.expectEqualStrings(" here", spans[2].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.plain, spans[2].style_type);
}

test "parseInlineStyles inline code at start" {
    const allocator = std.testing.allocator;
    const spans = try Markdown.parseInlineStyles("`start` of text", allocator);
    defer allocator.free(spans);

    try std.testing.expectEqual(@as(usize, 2), spans.len);
    try std.testing.expectEqualStrings("start", spans[0].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.code, spans[0].style_type);
}

test "parseInlineStyles inline code at end" {
    const allocator = std.testing.allocator;
    const spans = try Markdown.parseInlineStyles("end with `code`", allocator);
    defer allocator.free(spans);

    try std.testing.expectEqual(@as(usize, 2), spans.len);
    try std.testing.expectEqualStrings("code", spans[1].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.code, spans[1].style_type);
}

test "parseInlineStyles multiple inline codes" {
    const allocator = std.testing.allocator;
    const spans = try Markdown.parseInlineStyles("`one` and `two`", allocator);
    defer allocator.free(spans);

    try std.testing.expectEqual(@as(usize, 3), spans.len);
    try std.testing.expectEqualStrings("one", spans[0].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.code, spans[0].style_type);
    try std.testing.expectEqualStrings(" and ", spans[1].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.plain, spans[1].style_type);
    try std.testing.expectEqualStrings("two", spans[2].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.code, spans[2].style_type);
}

test "parseInlineStyles code with bold" {
    const allocator = std.testing.allocator;
    const spans = try Markdown.parseInlineStyles("Use `code` with **bold**", allocator);
    defer allocator.free(spans);

    try std.testing.expectEqual(@as(usize, 4), spans.len);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.code, spans[1].style_type);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.bold, spans[3].style_type);
}

test "Markdown.render with inline code" {
    const allocator = std.testing.allocator;
    const md = Markdown.init("Use `variable` in your code");
    const segments = try md.render(80, allocator);
    defer allocator.free(segments);

    var found_code = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "variable")) {
            found_code = true;
            try std.testing.expect(seg.style != null);
            try std.testing.expect(seg.style.?.hasAttribute(.dim));
        }
    }
    try std.testing.expect(found_code);
}

test "Markdown.render inline code at start" {
    const allocator = std.testing.allocator;
    const md = Markdown.init("`code` at start");
    const segments = try md.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len >= 2);
    try std.testing.expectEqualStrings("code", segments[0].text);
    try std.testing.expect(segments[0].style != null);
}

test "Markdown.render inline code at end" {
    const allocator = std.testing.allocator;
    const md = Markdown.init("end with `code`");
    const segments = try md.render(80, allocator);
    defer allocator.free(segments);

    var found_code = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "code")) {
            found_code = true;
            try std.testing.expect(seg.style != null);
        }
    }
    try std.testing.expect(found_code);
}

test "Markdown.render multiple inline codes" {
    const allocator = std.testing.allocator;
    const md = Markdown.init("Use `foo` and `bar`");
    const segments = try md.render(80, allocator);
    defer allocator.free(segments);

    var found_foo = false;
    var found_bar = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "foo")) {
            found_foo = true;
            try std.testing.expect(seg.style != null);
        }
        if (std.mem.eql(u8, seg.text, "bar")) {
            found_bar = true;
            try std.testing.expect(seg.style != null);
        }
    }
    try std.testing.expect(found_foo);
    try std.testing.expect(found_bar);
}

test "Markdown.render inline code with other styles" {
    const allocator = std.testing.allocator;
    const md = Markdown.init("Use `code` with **bold** and *italic*");
    const segments = try md.render(80, allocator);
    defer allocator.free(segments);

    var found_code = false;
    var found_bold = false;
    var found_italic = false;

    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "code")) {
            found_code = true;
            try std.testing.expect(seg.style != null);
        }
        if (std.mem.eql(u8, seg.text, "bold")) {
            found_bold = true;
            try std.testing.expect(seg.style != null);
            try std.testing.expect(seg.style.?.hasAttribute(.bold));
        }
        if (std.mem.eql(u8, seg.text, "italic")) {
            found_italic = true;
            try std.testing.expect(seg.style != null);
            try std.testing.expect(seg.style.?.hasAttribute(.italic));
        }
    }

    try std.testing.expect(found_code);
    try std.testing.expect(found_bold);
    try std.testing.expect(found_italic);
}

test "Markdown.render inline code in list item" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const md = Markdown.init("- Use `code` here");
    const segments = try md.render(80, arena.allocator());

    var found_code = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "code")) {
            found_code = true;
            try std.testing.expect(seg.style != null);
        }
    }
    try std.testing.expect(found_code);
}

test "Markdown.render inline code in blockquote" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const md = Markdown.init("> Use `code` here");
    const segments = try md.render(80, arena.allocator());

    var found_code = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "code")) {
            found_code = true;
            try std.testing.expect(seg.style != null);
        }
    }
    try std.testing.expect(found_code);
}

test "MarkdownTheme has inline code style" {
    const theme = MarkdownTheme.default;
    try std.testing.expect(theme.inline_code_style.color != null);
    try std.testing.expect(theme.inline_code_style.hasAttribute(.dim));
}

test "tryParseImage basic" {
    const text = "![alt text](https://example.com/image.png)";
    const result = Markdown.tryParseImage(text, 0);
    try std.testing.expect(result != null);
    try std.testing.expectEqualStrings("alt text", result.?.text);
    try std.testing.expectEqualStrings("https://example.com/image.png", result.?.url);
    try std.testing.expectEqual(@as(usize, text.len), result.?.end_pos);
}

test "tryParseImage empty alt" {
    const text = "![](https://example.com/image.png)";
    const result = Markdown.tryParseImage(text, 0);
    try std.testing.expect(result != null);
    try std.testing.expectEqualStrings("", result.?.text);
    try std.testing.expectEqualStrings("https://example.com/image.png", result.?.url);
}

test "tryParseImage no image" {
    try std.testing.expect(Markdown.tryParseImage("plain text", 0) == null);
    try std.testing.expect(Markdown.tryParseImage("[link](url)", 0) == null);
    try std.testing.expect(Markdown.tryParseImage("![unclosed", 0) == null);
    try std.testing.expect(Markdown.tryParseImage("![alt]no paren", 0) == null);
    try std.testing.expect(Markdown.tryParseImage("![alt](unclosed", 0) == null);
}

test "tryParseImage with offset" {
    const text = "prefix ![img](url) suffix";
    const result = Markdown.tryParseImage(text, 7);
    try std.testing.expect(result != null);
    try std.testing.expectEqualStrings("img", result.?.text);
    try std.testing.expectEqualStrings("url", result.?.url);
}

test "tryParseImage nested brackets in alt" {
    const text = "![text [nested]](url)";
    const result = Markdown.tryParseImage(text, 0);
    try std.testing.expect(result != null);
    try std.testing.expectEqualStrings("text [nested]", result.?.text);
}

test "tryParseImage nested parens in url" {
    const text = "![img](https://example.com/path(1).png)";
    const result = Markdown.tryParseImage(text, 0);
    try std.testing.expect(result != null);
    try std.testing.expectEqualStrings("https://example.com/path(1).png", result.?.url);
}

test "parseInlineStyles with image" {
    const allocator = std.testing.allocator;
    const spans = try Markdown.parseInlineStyles("Check ![photo](url) out", allocator);
    defer allocator.free(spans);

    try std.testing.expectEqual(@as(usize, 3), spans.len);
    try std.testing.expectEqualStrings("Check ", spans[0].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.plain, spans[0].style_type);
    try std.testing.expectEqualStrings("photo", spans[1].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.image, spans[1].style_type);
    try std.testing.expectEqualStrings("url", spans[1].image_url.?);
    try std.testing.expectEqualStrings(" out", spans[2].text);
    try std.testing.expectEqual(Markdown.InlineSpan.StyleType.plain, spans[2].style_type);
}

test "Markdown.render with image" {
    const allocator = std.testing.allocator;
    const md = Markdown.init("See ![a cat](https://example.com/cat.jpg) here");
    const segments = try md.render(80, allocator);
    defer allocator.free(segments);

    var found_prefix = false;
    var found_alt = false;
    var found_suffix = false;

    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "[Image: ")) {
            found_prefix = true;
            try std.testing.expect(seg.style != null);
            try std.testing.expect(seg.style.?.hasAttribute(.italic));
        }
        if (std.mem.eql(u8, seg.text, "a cat")) {
            found_alt = true;
            try std.testing.expect(seg.style != null);
            try std.testing.expect(seg.style.?.hasAttribute(.italic));
        }
        if (std.mem.eql(u8, seg.text, "]")) {
            found_suffix = true;
        }
    }
    try std.testing.expect(found_prefix);
    try std.testing.expect(found_alt);
    try std.testing.expect(found_suffix);
}

test "Markdown.render image at start" {
    const allocator = std.testing.allocator;
    const md = Markdown.init("![Start](url) of line");
    const segments = try md.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len >= 3);
    try std.testing.expectEqualStrings("[Image: ", segments[0].text);
    try std.testing.expectEqualStrings("Start", segments[1].text);
    try std.testing.expectEqualStrings("]", segments[2].text);
}

test "Markdown.render image at end" {
    const allocator = std.testing.allocator;
    const md = Markdown.init("End with ![image](url)");
    const segments = try md.render(80, allocator);
    defer allocator.free(segments);

    var found_alt = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "image")) {
            found_alt = true;
            try std.testing.expect(seg.style != null);
        }
    }
    try std.testing.expect(found_alt);
}

test "Markdown.render multiple images" {
    const allocator = std.testing.allocator;
    const md = Markdown.init("![one](url1) and ![two](url2)");
    const segments = try md.render(80, allocator);
    defer allocator.free(segments);

    var found_one = false;
    var found_two = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "one")) {
            found_one = true;
        }
        if (std.mem.eql(u8, seg.text, "two")) {
            found_two = true;
        }
    }
    try std.testing.expect(found_one);
    try std.testing.expect(found_two);
}

test "Markdown.render image with empty alt shows url" {
    const allocator = std.testing.allocator;
    const md = Markdown.init("![](https://example.com/img.png)");
    const segments = try md.render(80, allocator);
    defer allocator.free(segments);

    var found_url = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "https://example.com/img.png")) {
            found_url = true;
            try std.testing.expect(seg.style != null);
        }
    }
    try std.testing.expect(found_url);
}

test "Markdown.render image in list item" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const md = Markdown.init("- Item with ![icon](url)");
    const segments = try md.render(80, arena.allocator());

    var found_alt = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "icon")) {
            found_alt = true;
            try std.testing.expect(seg.style != null);
        }
    }
    try std.testing.expect(found_alt);
}

test "Markdown.render image in blockquote" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const md = Markdown.init("> Quote with ![photo](url)");
    const segments = try md.render(80, arena.allocator());

    var found_alt = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "photo")) {
            found_alt = true;
            try std.testing.expect(seg.style != null);
        }
    }
    try std.testing.expect(found_alt);
}

test "Markdown.render image with other inline styles" {
    const allocator = std.testing.allocator;
    const md = Markdown.init("![image](url) with **bold** and *italic*");
    const segments = try md.render(80, allocator);
    defer allocator.free(segments);

    var found_image = false;
    var found_bold = false;
    var found_italic = false;

    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "image")) {
            found_image = true;
        }
        if (std.mem.eql(u8, seg.text, "bold")) {
            found_bold = true;
            try std.testing.expect(seg.style != null);
            try std.testing.expect(seg.style.?.hasAttribute(.bold));
        }
        if (std.mem.eql(u8, seg.text, "italic")) {
            found_italic = true;
            try std.testing.expect(seg.style != null);
            try std.testing.expect(seg.style.?.hasAttribute(.italic));
        }
    }

    try std.testing.expect(found_image);
    try std.testing.expect(found_bold);
    try std.testing.expect(found_italic);
}

test "MarkdownTheme has image style" {
    const theme = MarkdownTheme.default;
    try std.testing.expect(theme.image_style.hasAttribute(.italic));
    try std.testing.expect(theme.image_style.color != null);
    try std.testing.expectEqualStrings("[Image: ", theme.image_prefix);
    try std.testing.expectEqualStrings("]", theme.image_suffix);
}

test "parseHorizontalRule basic dashes" {
    try std.testing.expect(Markdown.parseHorizontalRule("---"));
    try std.testing.expect(Markdown.parseHorizontalRule("----"));
    try std.testing.expect(Markdown.parseHorizontalRule("-----"));
    try std.testing.expect(Markdown.parseHorizontalRule("----------"));
}

test "parseHorizontalRule basic asterisks" {
    try std.testing.expect(Markdown.parseHorizontalRule("***"));
    try std.testing.expect(Markdown.parseHorizontalRule("****"));
    try std.testing.expect(Markdown.parseHorizontalRule("*****"));
}

test "parseHorizontalRule basic underscores" {
    try std.testing.expect(Markdown.parseHorizontalRule("___"));
    try std.testing.expect(Markdown.parseHorizontalRule("____"));
    try std.testing.expect(Markdown.parseHorizontalRule("_____"));
}

test "parseHorizontalRule with spaces" {
    try std.testing.expect(Markdown.parseHorizontalRule("- - -"));
    try std.testing.expect(Markdown.parseHorizontalRule("* * *"));
    try std.testing.expect(Markdown.parseHorizontalRule("_ _ _"));
    try std.testing.expect(Markdown.parseHorizontalRule("  ---  "));
    try std.testing.expect(Markdown.parseHorizontalRule("- - - -"));
}

test "parseHorizontalRule invalid" {
    try std.testing.expect(!Markdown.parseHorizontalRule("--"));
    try std.testing.expect(!Markdown.parseHorizontalRule("**"));
    try std.testing.expect(!Markdown.parseHorizontalRule("__"));
    try std.testing.expect(!Markdown.parseHorizontalRule("- * -"));
    try std.testing.expect(!Markdown.parseHorizontalRule("abc"));
    try std.testing.expect(!Markdown.parseHorizontalRule(""));
    try std.testing.expect(!Markdown.parseHorizontalRule("- -a-"));
}

test "Markdown.render horizontal rule" {
    const allocator = std.testing.allocator;
    const md = Markdown.init("Above\n\n---\n\nBelow");
    const segments = try md.render(40, allocator);
    defer allocator.free(segments);

    var found_above = false;
    var found_below = false;
    var found_rule_char = false;

    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "Above")) found_above = true;
        if (std.mem.eql(u8, seg.text, "Below")) found_below = true;
        if (std.mem.eql(u8, seg.text, "\u{2500}")) {
            found_rule_char = true;
            try std.testing.expect(seg.style != null);
        }
    }

    try std.testing.expect(found_above);
    try std.testing.expect(found_below);
    try std.testing.expect(found_rule_char);
}

test "Markdown.render horizontal rule asterisks" {
    const allocator = std.testing.allocator;
    const md = Markdown.init("***");
    const segments = try md.render(20, allocator);
    defer allocator.free(segments);

    var found_rule = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "\u{2500}")) {
            found_rule = true;
        }
    }
    try std.testing.expect(found_rule);
}

test "Markdown.render horizontal rule underscores" {
    const allocator = std.testing.allocator;
    const md = Markdown.init("___");
    const segments = try md.render(20, allocator);
    defer allocator.free(segments);

    var found_rule = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "\u{2500}")) {
            found_rule = true;
        }
    }
    try std.testing.expect(found_rule);
}

test "MarkdownTheme has rule style" {
    const theme = MarkdownTheme.default;
    try std.testing.expect(theme.rule_style.color != null);
    try std.testing.expectEqualStrings("\u{2500}", theme.rule_char);
}

// GFM Table tests

test "isTableRow basic" {
    try std.testing.expect(Markdown.isTableRow("| Header 1 | Header 2 |"));
    try std.testing.expect(Markdown.isTableRow("|---|---|"));
    try std.testing.expect(Markdown.isTableRow("| cell | cell |"));
    try std.testing.expect(Markdown.isTableRow("Header 1 | Header 2"));
    try std.testing.expect(!Markdown.isTableRow(""));
    try std.testing.expect(!Markdown.isTableRow("No pipes here"));
}

test "isTableSeparator basic" {
    try std.testing.expect(Markdown.isTableSeparator("|---|---|"));
    try std.testing.expect(Markdown.isTableSeparator("| --- | --- |"));
    try std.testing.expect(Markdown.isTableSeparator("|:---:|---:|"));
    try std.testing.expect(Markdown.isTableSeparator("|:---|:---:|---:|"));
    try std.testing.expect(Markdown.isTableSeparator("| -------- |"));
    try std.testing.expect(!Markdown.isTableSeparator("| Header |"));
    try std.testing.expect(!Markdown.isTableSeparator("|--|"));
    try std.testing.expect(!Markdown.isTableSeparator(""));
}

test "parseTableAlignment" {
    try std.testing.expectEqual(JustifyMethod.left, Markdown.parseTableAlignment("---"));
    try std.testing.expectEqual(JustifyMethod.left, Markdown.parseTableAlignment(":---"));
    try std.testing.expectEqual(JustifyMethod.center, Markdown.parseTableAlignment(":---:"));
    try std.testing.expectEqual(JustifyMethod.right, Markdown.parseTableAlignment("---:"));
    try std.testing.expectEqual(JustifyMethod.left, Markdown.parseTableAlignment("  ---  "));
    try std.testing.expectEqual(JustifyMethod.center, Markdown.parseTableAlignment("  :---:  "));
}

test "splitTableCells basic" {
    const allocator = std.testing.allocator;

    const cells1 = try Markdown.splitTableCells("| A | B | C |", allocator);
    defer allocator.free(cells1);
    try std.testing.expectEqual(@as(usize, 3), cells1.len);
    try std.testing.expectEqualStrings("A", cells1[0]);
    try std.testing.expectEqualStrings("B", cells1[1]);
    try std.testing.expectEqualStrings("C", cells1[2]);

    const cells2 = try Markdown.splitTableCells("| Header 1 | Header 2 |", allocator);
    defer allocator.free(cells2);
    try std.testing.expectEqual(@as(usize, 2), cells2.len);
    try std.testing.expectEqualStrings("Header 1", cells2[0]);
    try std.testing.expectEqualStrings("Header 2", cells2[1]);
}

test "splitTableCells no leading pipe" {
    const allocator = std.testing.allocator;

    const parsed_cells = try Markdown.splitTableCells("A | B | C", allocator);
    defer allocator.free(parsed_cells);
    try std.testing.expectEqual(@as(usize, 3), parsed_cells.len);
    try std.testing.expectEqualStrings("A", parsed_cells[0]);
    try std.testing.expectEqualStrings("B", parsed_cells[1]);
    try std.testing.expectEqualStrings("C", parsed_cells[2]);
}

test "Markdown.render basic GFM table" {
    const allocator = std.testing.allocator;
    const source =
        \\| Name | Age |
        \\|------|-----|
        \\| Alice | 30 |
        \\| Bob | 25 |
    ;
    const md = Markdown.init(source);
    const segments = try md.render(80, allocator);
    defer allocator.free(segments);

    var found_name = false;
    var found_age = false;
    var found_alice = false;
    var found_bob = false;

    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "Name")) found_name = true;
        if (std.mem.eql(u8, seg.text, "Age")) found_age = true;
        if (std.mem.eql(u8, seg.text, "Alice")) found_alice = true;
        if (std.mem.eql(u8, seg.text, "Bob")) found_bob = true;
    }

    try std.testing.expect(found_name);
    try std.testing.expect(found_age);
    try std.testing.expect(found_alice);
    try std.testing.expect(found_bob);
}

test "Markdown.render GFM table with alignment" {
    const allocator = std.testing.allocator;
    const source =
        \\| Left | Center | Right |
        \\|:-----|:------:|------:|
        \\| L | C | R |
    ;
    const md = Markdown.init(source);
    const segments = try md.render(80, allocator);
    defer allocator.free(segments);

    var found_left = false;
    var found_center = false;
    var found_right = false;

    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "Left")) found_left = true;
        if (std.mem.eql(u8, seg.text, "Center")) found_center = true;
        if (std.mem.eql(u8, seg.text, "Right")) found_right = true;
    }

    try std.testing.expect(found_left);
    try std.testing.expect(found_center);
    try std.testing.expect(found_right);
}

test "Markdown.render GFM table header style" {
    const allocator = std.testing.allocator;
    const source =
        \\| Header |
        \\|--------|
        \\| Data |
    ;
    const md = Markdown.init(source);
    const segments = try md.render(80, allocator);
    defer allocator.free(segments);

    var found_header = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "Header")) {
            found_header = true;
            try std.testing.expect(seg.style != null);
            try std.testing.expect(seg.style.?.hasAttribute(.bold));
        }
    }
    try std.testing.expect(found_header);
}

test "Markdown.render table mixed with other elements" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const source =
        \\# Title
        \\
        \\| Col1 | Col2 |
        \\|------|------|
        \\| A | B |
        \\
        \\Some text after
    ;
    const md = Markdown.init(source);
    const segments = try md.render(80, arena.allocator());

    var found_title = false;
    var found_col1 = false;
    var found_text = false;

    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "Title")) found_title = true;
        if (std.mem.eql(u8, seg.text, "Col1")) found_col1 = true;
        if (std.mem.eql(u8, seg.text, "Some text after")) found_text = true;
    }

    try std.testing.expect(found_title);
    try std.testing.expect(found_col1);
    try std.testing.expect(found_text);
}

test "Markdown.render invalid table falls back to text" {
    const allocator = std.testing.allocator;
    // Only one line - not a valid table
    const source = "| Header 1 | Header 2 |";
    const md = Markdown.init(source);
    const segments = try md.render(80, allocator);
    defer allocator.free(segments);

    // Should still render the text (as inline text, not table)
    try std.testing.expect(segments.len >= 1);
}

test "Markdown.render table without separator falls back to text" {
    const allocator = std.testing.allocator;
    const source =
        \\| Header 1 | Header 2 |
        \\| Not a sep | Also not |
    ;
    const md = Markdown.init(source);
    const segments = try md.render(80, allocator);
    defer allocator.free(segments);

    // Should still render as text (inline), not as a table
    try std.testing.expect(segments.len >= 1);
}

test "MarkdownTheme has table styles" {
    const theme = MarkdownTheme.default;
    try std.testing.expect(theme.table_header_style.hasAttribute(.bold));
    try std.testing.expect(theme.table_border_style.color != null);
}
