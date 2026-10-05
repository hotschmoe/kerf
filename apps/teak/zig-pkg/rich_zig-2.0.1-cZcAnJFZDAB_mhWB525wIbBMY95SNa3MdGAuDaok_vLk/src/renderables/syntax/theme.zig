const Style = @import("../../style.zig").Style;
const Color = @import("../../color.zig").Color;

pub const SyntaxTheme = struct {
    keyword_style: Style = Style.empty.foreground(Color.magenta).bold(),
    builtin_style: Style = Style.empty.foreground(Color.cyan),
    string_style: Style = Style.empty.foreground(Color.green),
    comment_style: Style = Style.empty.foreground(Color.default).dim(),
    number_style: Style = Style.empty.foreground(Color.yellow),
    operator_style: Style = Style.empty.foreground(Color.default),
    punctuation_style: Style = Style.empty.foreground(Color.default).dim(),
    type_style: Style = Style.empty.foreground(Color.blue),
    function_style: Style = Style.empty.foreground(Color.cyan),
    line_number_style: Style = Style.empty.foreground(Color.default).dim(),
    indent_guide_style: Style = Style.empty.foreground(Color.default).dim(),
    highlight_line_style: Style = Style.empty.background(Color.fromRgb(60, 60, 40)),
    highlight_line_number_style: Style = Style.empty.foreground(Color.yellow).bold(),
    default_style: Style = Style.empty,
    background_color: ?Color = null,

    pub const default: SyntaxTheme = .{};

    pub const monokai: SyntaxTheme = .{
        .keyword_style = Style.empty.foreground(Color.fromRgb(249, 38, 114)),
        .builtin_style = Style.empty.foreground(Color.fromRgb(102, 217, 239)),
        .string_style = Style.empty.foreground(Color.fromRgb(230, 219, 116)),
        .comment_style = Style.empty.foreground(Color.fromRgb(117, 113, 94)),
        .number_style = Style.empty.foreground(Color.fromRgb(174, 129, 255)),
        .operator_style = Style.empty.foreground(Color.fromRgb(249, 38, 114)),
        .punctuation_style = Style.empty.foreground(Color.fromRgb(248, 248, 242)),
        .type_style = Style.empty.foreground(Color.fromRgb(102, 217, 239)).italic(),
        .function_style = Style.empty.foreground(Color.fromRgb(166, 226, 46)),
        .line_number_style = Style.empty.foreground(Color.fromRgb(117, 113, 94)),
        .highlight_line_style = Style.empty.background(Color.fromRgb(73, 72, 62)),
        .highlight_line_number_style = Style.empty.foreground(Color.fromRgb(248, 248, 242)).bold(),
        .default_style = Style.empty.foreground(Color.fromRgb(248, 248, 242)),
        .background_color = Color.fromRgb(39, 40, 34),
    };

    pub const dracula: SyntaxTheme = .{
        .keyword_style = Style.empty.foreground(Color.fromRgb(255, 121, 198)),
        .builtin_style = Style.empty.foreground(Color.fromRgb(139, 233, 253)),
        .string_style = Style.empty.foreground(Color.fromRgb(241, 250, 140)),
        .comment_style = Style.empty.foreground(Color.fromRgb(98, 114, 164)),
        .number_style = Style.empty.foreground(Color.fromRgb(189, 147, 249)),
        .operator_style = Style.empty.foreground(Color.fromRgb(255, 121, 198)),
        .punctuation_style = Style.empty.foreground(Color.fromRgb(248, 248, 242)),
        .type_style = Style.empty.foreground(Color.fromRgb(139, 233, 253)).italic(),
        .function_style = Style.empty.foreground(Color.fromRgb(80, 250, 123)),
        .line_number_style = Style.empty.foreground(Color.fromRgb(98, 114, 164)),
        .highlight_line_style = Style.empty.background(Color.fromRgb(68, 71, 90)),
        .highlight_line_number_style = Style.empty.foreground(Color.fromRgb(241, 250, 140)).bold(),
        .default_style = Style.empty.foreground(Color.fromRgb(248, 248, 242)),
        .background_color = Color.fromRgb(40, 42, 54),
    };

    /// Apply theme's background color to a style, if the theme has one
    pub fn applyBackground(self: SyntaxTheme, style: Style) Style {
        if (self.background_color) |bg| {
            return style.background(bg);
        }
        return style;
    }

    /// Apply theme's background color to an optional style, if the theme has one
    pub fn applyBackgroundOpt(self: SyntaxTheme, style: ?Style) ?Style {
        const bg = self.background_color orelse return style;
        return (style orelse Style.empty).background(bg);
    }
};
