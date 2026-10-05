const std = @import("std");
const Segment = @import("../../segment.zig").Segment;
const cells = @import("../../cells.zig");
const Syntax = @import("../syntax/mod.zig").Syntax;
const Language = @import("../syntax/mod.zig").Language;
const theme_mod = @import("theme.zig");
const HeaderLevel = theme_mod.HeaderLevel;
const MarkdownTheme = theme_mod.MarkdownTheme;

pub const Header = struct {
    text: []const u8,
    level: HeaderLevel,
    theme: MarkdownTheme = .default,
    width: ?usize = null,

    pub fn init(text: []const u8, level: HeaderLevel) Header {
        return .{ .text = text, .level = level };
    }

    pub fn h1(text: []const u8) Header {
        return init(text, .h1);
    }

    pub fn h2(text: []const u8) Header {
        return init(text, .h2);
    }

    pub fn h3(text: []const u8) Header {
        return init(text, .h3);
    }

    pub fn h4(text: []const u8) Header {
        return init(text, .h4);
    }

    pub fn h5(text: []const u8) Header {
        return init(text, .h5);
    }

    pub fn h6(text: []const u8) Header {
        return init(text, .h6);
    }

    pub fn withTheme(self: Header, theme: MarkdownTheme) Header {
        var h = self;
        h.theme = theme;
        return h;
    }

    pub fn withWidth(self: Header, w: usize) Header {
        var h = self;
        h.width = w;
        return h;
    }

    pub fn render(self: Header, max_width: usize, allocator: std.mem.Allocator) ![]Segment {
        var segments: std.ArrayList(Segment) = .empty;

        const style = self.theme.styleForLevel(self.level);
        const text_width = cells.cellLen(self.text);
        const effective_width = self.width orelse max_width;

        try segments.append(allocator, Segment.styled(self.text, style));
        try segments.append(allocator, Segment.line());

        if (self.theme.underlineForLevel(self.level)) |underline_char| {
            const underline_len = @min(text_width, effective_width);
            const char_width = cells.cellLen(underline_char);

            if (char_width > 0) {
                const repeat_count = underline_len / char_width;
                for (0..repeat_count) |_| {
                    try segments.append(allocator, Segment.styled(underline_char, style));
                }
            }
            try segments.append(allocator, Segment.line());
        }

        return segments.toOwnedSlice(allocator);
    }
};

pub const CodeBlock = struct {
    code: []const u8,
    language: ?[]const u8 = null,
    theme: MarkdownTheme = .default,

    pub fn init(code: []const u8) CodeBlock {
        return .{ .code = code };
    }

    pub fn withLanguage(self: CodeBlock, lang: []const u8) CodeBlock {
        var cb = self;
        cb.language = lang;
        return cb;
    }

    pub fn withTheme(self: CodeBlock, theme: MarkdownTheme) CodeBlock {
        var cb = self;
        cb.theme = theme;
        return cb;
    }

    fn detectLanguage(self: CodeBlock) Language {
        const lang = self.language orelse return .plain;
        if (lang.len == 0) return .plain;

        const lang_map = std.StaticStringMap(Language).initComptime(.{
            .{ "zig", .zig },
            .{ "json", .json },
            .{ "md", .markdown },
            .{ "markdown", .markdown },
        });

        return lang_map.get(lang) orelse .plain;
    }

    pub fn render(self: CodeBlock, max_width: usize, allocator: std.mem.Allocator) ![]Segment {
        const detected_lang = self.detectLanguage();

        if (detected_lang != .plain) {
            const syntax = Syntax.init(allocator, self.code)
                .withLanguage(detected_lang)
                .withTheme(self.theme.syntax_theme);
            return syntax.renderDuped(max_width, allocator);
        }

        var segments: std.ArrayList(Segment) = .empty;
        var lines = std.mem.splitScalar(u8, self.code, '\n');

        while (lines.next()) |line| {
            if (line.len > 0) {
                const duped = try allocator.dupe(u8, line);
                try segments.append(allocator, Segment.styled(duped, self.theme.code_block_style));
            }
            try segments.append(allocator, Segment.line());
        }

        return segments.toOwnedSlice(allocator);
    }
};

test "Header.h1 creates correct level" {
    const header = Header.h1("Title");
    try std.testing.expectEqual(HeaderLevel.h1, header.level);
    try std.testing.expectEqualStrings("Title", header.text);
}

test "Header.render h1 with underline" {
    const allocator = std.testing.allocator;
    const header = Header.h1("Title");
    const segments = try header.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len >= 2);
    try std.testing.expectEqualStrings("Title", segments[0].text);
    try std.testing.expect(segments[0].style != null);
    try std.testing.expect(segments[0].style.?.hasAttribute(.bold));
}

test "Header.render h3 no underline" {
    const allocator = std.testing.allocator;
    const header = Header.h3("Section");
    const segments = try header.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expectEqual(@as(usize, 2), segments.len);
    try std.testing.expectEqualStrings("Section", segments[0].text);
    try std.testing.expectEqualStrings("\n", segments[1].text);
}

test "CodeBlock.init" {
    const cb = CodeBlock.init("const x = 1;");
    try std.testing.expectEqualStrings("const x = 1;", cb.code);
    try std.testing.expect(cb.language == null);
}

test "CodeBlock.withLanguage" {
    const cb = CodeBlock.init("code").withLanguage("zig");
    try std.testing.expectEqualStrings("zig", cb.language.?);
}

test "CodeBlock.detectLanguage" {
    const plain = CodeBlock.init("code");
    try std.testing.expectEqual(Language.plain, plain.detectLanguage());

    const zig = CodeBlock.init("code").withLanguage("zig");
    try std.testing.expectEqual(Language.zig, zig.detectLanguage());

    const json = CodeBlock.init("code").withLanguage("json");
    try std.testing.expectEqual(Language.json, json.detectLanguage());

    const md = CodeBlock.init("code").withLanguage("markdown");
    try std.testing.expectEqual(Language.markdown, md.detectLanguage());

    const unknown = CodeBlock.init("code").withLanguage("python");
    try std.testing.expectEqual(Language.plain, unknown.detectLanguage());
}

test "CodeBlock.render plain" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const cb = CodeBlock.init("line1\nline2");
    const segments = try cb.render(80, arena.allocator());

    try std.testing.expect(segments.len >= 2);
    var found_line1 = false;
    var found_line2 = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "line1")) found_line1 = true;
        if (std.mem.eql(u8, seg.text, "line2")) found_line2 = true;
    }
    try std.testing.expect(found_line1);
    try std.testing.expect(found_line2);
}

test "CodeBlock.render zig syntax" {
    const allocator = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();

    const cb = CodeBlock.init("const x = 1;").withLanguage("zig");
    const segments = try cb.render(80, arena.allocator());

    try std.testing.expect(segments.len > 0);
    var found_const = false;
    for (segments) |seg| {
        if (std.mem.eql(u8, seg.text, "const")) {
            found_const = true;
            break;
        }
    }
    try std.testing.expect(found_const);
}

test "Header all levels" {
    const allocator = std.testing.allocator;
    const levels = [_]HeaderLevel{ .h1, .h2, .h3, .h4, .h5, .h6 };

    for (levels) |level| {
        const header = Header.init("Test", level);
        const segments = try header.render(80, allocator);
        defer allocator.free(segments);

        try std.testing.expect(segments.len >= 2);
        try std.testing.expect(segments[0].style != null);
        try std.testing.expect(segments[0].style.?.hasAttribute(.bold));
    }
}
