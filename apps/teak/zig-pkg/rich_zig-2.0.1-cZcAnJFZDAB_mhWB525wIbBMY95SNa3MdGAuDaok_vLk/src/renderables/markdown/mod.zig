pub const theme = @import("theme.zig");
pub const HeaderLevel = theme.HeaderLevel;
pub const MarkdownTheme = theme.MarkdownTheme;

pub const elements = @import("elements.zig");
pub const Header = elements.Header;
pub const CodeBlock = elements.CodeBlock;

pub const markdown = @import("markdown.zig");
pub const Markdown = markdown.Markdown;

test {
    _ = @import("theme.zig");
    _ = @import("elements.zig");
    _ = @import("markdown.zig");
}
