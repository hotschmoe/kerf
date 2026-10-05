const std = @import("std");

pub const Language = enum {
    zig,
    json,
    markdown,
    python,
    javascript,
    typescript,
    rust,
    go,
    c,
    cpp,
    bash,
    yaml,
    toml,
    xml,
    html,
    css,
    sql,
    plain,

    /// Detect language from file extension
    pub fn fromExtension(ext: []const u8) Language {
        const extension_map = std.StaticStringMap(Language).initComptime(.{
            // Zig
            .{ ".zig", .zig },
            .{ ".zon", .zig },
            // JSON
            .{ ".json", .json },
            .{ ".jsonl", .json },
            .{ ".geojson", .json },
            // Markdown
            .{ ".md", .markdown },
            .{ ".markdown", .markdown },
            .{ ".mdown", .markdown },
            // Python
            .{ ".py", .python },
            .{ ".pyi", .python },
            .{ ".pyw", .python },
            .{ ".pyx", .python },
            // JavaScript
            .{ ".js", .javascript },
            .{ ".mjs", .javascript },
            .{ ".cjs", .javascript },
            .{ ".jsx", .javascript },
            // TypeScript
            .{ ".ts", .typescript },
            .{ ".mts", .typescript },
            .{ ".cts", .typescript },
            .{ ".tsx", .typescript },
            // Rust
            .{ ".rs", .rust },
            // Go
            .{ ".go", .go },
            // C
            .{ ".c", .c },
            .{ ".h", .c },
            // C++
            .{ ".cpp", .cpp },
            .{ ".cxx", .cpp },
            .{ ".cc", .cpp },
            .{ ".hpp", .cpp },
            .{ ".hxx", .cpp },
            .{ ".hh", .cpp },
            // Shell/Bash
            .{ ".sh", .bash },
            .{ ".bash", .bash },
            .{ ".zsh", .bash },
            .{ ".fish", .bash },
            // YAML
            .{ ".yaml", .yaml },
            .{ ".yml", .yaml },
            // TOML
            .{ ".toml", .toml },
            // XML
            .{ ".xml", .xml },
            .{ ".xsl", .xml },
            .{ ".xslt", .xml },
            .{ ".svg", .xml },
            // HTML
            .{ ".html", .html },
            .{ ".htm", .html },
            .{ ".xhtml", .html },
            // CSS
            .{ ".css", .css },
            .{ ".scss", .css },
            .{ ".sass", .css },
            .{ ".less", .css },
            // SQL
            .{ ".sql", .sql },
        });

        // Normalize extension to lowercase for comparison
        var lower_ext: [16]u8 = undefined;
        const len = @min(ext.len, lower_ext.len);
        for (0..len) |i| {
            lower_ext[i] = std.ascii.toLower(ext[i]);
        }

        return extension_map.get(lower_ext[0..len]) orelse .plain;
    }

    /// Detect language from filename (extracts extension)
    pub fn fromFilename(filename: []const u8) Language {
        // Handle special filenames without extensions
        const basename = std.fs.path.basename(filename);

        const special_files = std.StaticStringMap(Language).initComptime(.{
            .{ "Makefile", .bash },
            .{ "makefile", .bash },
            .{ "GNUmakefile", .bash },
            .{ "Dockerfile", .bash },
            .{ "dockerfile", .bash },
            .{ "Containerfile", .bash },
            .{ ".bashrc", .bash },
            .{ ".bash_profile", .bash },
            .{ ".zshrc", .bash },
            .{ ".profile", .bash },
            .{ ".gitignore", .plain },
            .{ ".gitattributes", .plain },
            .{ "CMakeLists.txt", .bash },
            .{ "Cargo.toml", .toml },
            .{ "Cargo.lock", .toml },
            .{ "package.json", .json },
            .{ "tsconfig.json", .json },
            .{ "pyproject.toml", .toml },
            .{ "build.zig", .zig },
            .{ "build.zig.zon", .zig },
        });

        if (special_files.get(basename)) |lang| {
            return lang;
        }

        // Extract extension
        if (std.mem.lastIndexOfScalar(u8, filename, '.')) |dot_pos| {
            return fromExtension(filename[dot_pos..]);
        }

        return .plain;
    }

    /// Detect language from content using heuristics
    pub fn fromContent(content: []const u8) Language {
        const trimmed = std.mem.trimStart(u8, content, " \t\n\r");
        if (trimmed.len == 0) return .plain;

        // Check for shebang
        if (std.mem.startsWith(u8, trimmed, "#!")) {
            const shebang = trimmed[0..@min(trimmed.len, 100)];
            if (std.mem.indexOf(u8, shebang, "python") != null) return .python;
            if (std.mem.indexOf(u8, shebang, "node") != null) return .javascript;
            // "sh" matches bash, sh, zsh
            if (std.mem.indexOf(u8, shebang, "sh") != null) return .bash;
            return .bash;
        }

        // Check for XML/HTML declaration
        if (std.mem.startsWith(u8, trimmed, "<?xml")) return .xml;
        if (std.mem.startsWith(u8, trimmed, "<!DOCTYPE html") or
            std.mem.startsWith(u8, trimmed, "<!doctype html") or
            std.mem.startsWith(u8, trimmed, "<html"))
        {
            return .html;
        }

        // Check for JSON (starts with { or [)
        if (trimmed[0] == '{' or trimmed[0] == '[') {
            if (looksLikeJson(trimmed)) return .json;
        }

        // Check for YAML (key: value pattern at start, or ---)
        if (std.mem.startsWith(u8, trimmed, "---")) return .yaml;

        // Check for Markdown (common patterns)
        if (std.mem.startsWith(u8, trimmed, "# ") or
            std.mem.startsWith(u8, trimmed, "## ") or
            std.mem.startsWith(u8, trimmed, "### "))
        {
            return .markdown;
        }

        // Check for common language patterns
        if (containsZigPatterns(content)) return .zig;
        if (containsPythonPatterns(content)) return .python;
        if (containsRustPatterns(content)) return .rust;
        if (containsGoPatterns(content)) return .go;
        if (containsJsPatterns(content)) return .javascript;
        if (containsCPatterns(content)) return .c;

        return .plain;
    }

    /// Auto-detect language from filename and/or content.
    /// Tries filename first, falls back to content heuristics.
    pub fn detect(filename: ?[]const u8, content: []const u8) Language {
        if (filename) |fname| {
            const from_file = fromFilename(fname);
            if (from_file != .plain) return from_file;
        }
        return fromContent(content);
    }

    fn looksLikeJson(content: []const u8) bool {
        var brace_count: i32 = 0;
        var bracket_count: i32 = 0;
        var colon_count: usize = 0;
        var quote_count: usize = 0;
        const check_len = @min(content.len, 500);

        for (content[0..check_len]) |c| {
            switch (c) {
                '{' => brace_count += 1,
                '}' => brace_count -= 1,
                '[' => bracket_count += 1,
                ']' => bracket_count -= 1,
                ':' => colon_count += 1,
                '"' => quote_count += 1,
                else => {},
            }
        }

        const looks_like_object = brace_count == 0 and colon_count > 0 and quote_count >= 2;
        const looks_like_array = content[0] == '[' and bracket_count == 0;

        return looks_like_object or looks_like_array;
    }

    fn containsZigPatterns(content: []const u8) bool {
        const patterns = [_][]const u8{
            "const std = @import",
            "pub fn ",
            "@import(",
            "std.mem.",
            "std.debug.print",
            "catch |err|",
            "try ",
            "defer ",
            "comptime ",
        };
        return matchAnyPattern(content, &patterns, 3);
    }

    fn containsPythonPatterns(content: []const u8) bool {
        const patterns = [_][]const u8{
            "def ",
            "import ",
            "from ",
            "class ",
            "if __name__",
            "self.",
            "    def ",
            "print(",
            "async def ",
        };
        return matchAnyPattern(content, &patterns, 2);
    }

    fn containsRustPatterns(content: []const u8) bool {
        const patterns = [_][]const u8{
            "fn main()",
            "pub fn ",
            "use std::",
            "let mut ",
            "impl ",
            "struct ",
            "enum ",
            "match ",
            "-> Result<",
            "println!(",
            "#[derive(",
        };
        return matchAnyPattern(content, &patterns, 3);
    }

    fn containsGoPatterns(content: []const u8) bool {
        const patterns = [_][]const u8{
            "package ",
            "func main()",
            "import (",
            "func (",
            "type ",
            "fmt.Println",
            "if err != nil",
            ":= ",
        };
        return matchAnyPattern(content, &patterns, 3);
    }

    fn containsJsPatterns(content: []const u8) bool {
        const patterns = [_][]const u8{
            "function ",
            "const ",
            "let ",
            "var ",
            "=> {",
            "console.log",
            "module.exports",
            "require(",
            "export ",
            "import ",
            "async ",
        };
        return matchAnyPattern(content, &patterns, 2);
    }

    fn containsCPatterns(content: []const u8) bool {
        const patterns = [_][]const u8{
            "#include <",
            "#include \"",
            "int main(",
            "void ",
            "printf(",
            "sizeof(",
            "NULL",
            "malloc(",
            "#define ",
        };
        return matchAnyPattern(content, &patterns, 2);
    }

    fn matchAnyPattern(content: []const u8, patterns: []const []const u8, min_matches: usize) bool {
        var matches: usize = 0;
        for (patterns) |pattern| {
            if (std.mem.indexOf(u8, content, pattern) != null) {
                matches += 1;
                if (matches >= min_matches) return true;
            }
        }
        return false;
    }
};
