pub const Language = @import("language.zig").Language;
pub const SyntaxTheme = @import("theme.zig").SyntaxTheme;
pub const Syntax = @import("highlighter.zig").Syntax;

test {
    _ = @import("language.zig");
    _ = @import("theme.zig");
    _ = @import("highlighter.zig");
}
