//! rich_zig - A Zig port of Python's Rich library for terminal formatting
//!
//! This library provides beautiful terminal output with:
//! - Styled text with colors and attributes
//! - Unicode-aware text handling
//! - Markup parsing for easy styling (theme-aware)
//! - Renderables: panels, tables, progress bars, trees
//! - Theme system for named, reusable styles
//! - Measurement protocol for smart auto-sizing
//! - Comptime pretty printer for Zig types
//! - Auto-highlighter for numbers, URLs, paths, UUIDs
//! - ANSI escape sequence parsing and stripping
//! - Adaptive colors with automatic terminal downgrading
//! - HSL color space conversion, blending, and gradients
//! - WCAG contrast ratio checking for accessibility
//! - Synchronized output (DEC mode 2026) for flicker-free rendering
//! - Dark/light background detection
//!
//! ## Quick Start
//! ```zig
//! const rich = @import("rich_zig");
//! const std = @import("std");
//!
//! pub fn main() !void {
//!     var gpa = std.heap.GeneralPurposeAllocator(.{}){};
//!     defer _ = gpa.deinit();
//!     const allocator = gpa.allocator();
//!
//!     var console = rich.Console.init(allocator);
//!     defer console.deinit();
//!     try console.print("[bold red]Hello[/] [green]World[/]!");
//! }
//! ```

const std = @import("std");

// Error types
pub const errors = @import("errors.zig");
pub const MarkupError = errors.MarkupError;
pub const RenderError = errors.RenderError;
pub const TableError = errors.TableError;
pub const ConsoleError = errors.ConsoleError;

// Phase 1: Core types
pub const color = @import("color.zig");
pub const Color = color.Color;
pub const ColorType = color.ColorType;
pub const ColorSystem = color.ColorSystem;
pub const ColorTriplet = color.ColorTriplet;
pub const AdaptiveColor = color.AdaptiveColor;
pub const gradient = color.gradient;

pub const cells = @import("cells.zig");

pub const style = @import("style.zig");
pub const Style = style.Style;
pub const StyleAttribute = style.StyleAttribute;

pub const theme = @import("theme.zig");
pub const Theme = theme.Theme;

pub const segment = @import("segment.zig");
pub const Segment = segment.Segment;
pub const ControlCode = segment.ControlCode;
pub const ControlType = segment.ControlType;

pub const measure = @import("measure.zig");
pub const Measurement = measure.Measurement;

// Phase 2: Text and Markup
pub const box = @import("box.zig");
pub const BoxStyle = box.BoxStyle;

pub const markup = @import("markup.zig");

pub const text = @import("text.zig");
pub const Text = text.Text;
pub const Span = text.Span;

pub const highlighter = @import("highlighter.zig");
pub const Highlighter = highlighter.Highlighter;

pub const ansi = @import("ansi.zig");
pub const fromAnsi = ansi.fromAnsi;
pub const stripAnsi = ansi.stripAnsi;

// Phase 3: Terminal and Console
pub const terminal = @import("terminal.zig");
pub const TerminalInfo = terminal.TerminalInfo;
pub const BackgroundMode = terminal.BackgroundMode;
pub const beginSyncOutput = terminal.beginSyncOutput;
pub const endSyncOutput = terminal.endSyncOutput;
pub const sync_output_begin = terminal.sync_output_begin;
pub const sync_output_end = terminal.sync_output_end;

pub const console = @import("console.zig");
pub const Console = console.Console;
pub const ConsoleOptions = console.ConsoleOptions;
pub const LogLevel = console.LogLevel;

// Phase 4: Renderables
pub const renderables = @import("renderables/mod.zig");
pub const Panel = renderables.Panel;
pub const Alignment = renderables.Alignment;
pub const VOverflow = renderables.VOverflow;
pub const Table = renderables.Table;
pub const Column = renderables.Column;
pub const JustifyMethod = renderables.JustifyMethod;
pub const Overflow = renderables.Overflow;
pub const AlternatingStyles = renderables.AlternatingStyles;
pub const Rule = renderables.Rule;
pub const ProgressBar = renderables.ProgressBar;
pub const Spinner = renderables.Spinner;
pub const SpeedUnit = renderables.SpeedUnit;
pub const Tree = renderables.Tree;
pub const TreeNode = renderables.TreeNode;
pub const Padding = renderables.Padding;
pub const Align = renderables.Align;
pub const HAlign = renderables.HAlign;
pub const VAlign = renderables.VAlign;
pub const Columns = renderables.Columns;
pub const Live = renderables.Live;
pub const Split = renderables.Split;
pub const SplitDirection = renderables.SplitDirection;
pub const SizeConstraint = renderables.SizeConstraint;
pub const Json = renderables.Json;
pub const JsonTheme = renderables.JsonTheme;
pub const ProgressGroup = renderables.ProgressGroup;
pub const ProgressDisplay = renderables.ProgressDisplay;
pub const OverflowMode = renderables.OverflowMode;
pub const LabelContent = renderables.LabelContent;
pub const CustomChars = box.CustomChars;
pub const Syntax = renderables.Syntax;
pub const SyntaxTheme = renderables.SyntaxTheme;
pub const SyntaxLanguage = renderables.SyntaxLanguage;
pub const KV = renderables.KV;
pub const Cell = renderables.Cell;
pub const CellContent = renderables.CellContent;
pub const SplitContent = renderables.SplitContent;
pub const SplitterConfig = renderables.SplitterConfig;
pub const Markdown = renderables.Markdown;
pub const MarkdownTheme = renderables.MarkdownTheme;
pub const Header = renderables.Header;
pub const HeaderLevel = renderables.HeaderLevel;

// Emoji support
pub const emoji = @import("emoji.zig");

// Logging
pub const logging = @import("logging.zig");
pub const RichHandler = logging.RichHandler;
pub const LogRecord = logging.LogRecord;
pub const LevelStyles = logging.LevelStyles;

// Traceback support
pub const Traceback = logging.Traceback;
pub const StackFrame = logging.StackFrame;
pub const TracebackTheme = logging.TracebackTheme;
pub const TracebackOptions = logging.TracebackOptions;
pub const traceHere = logging.traceHere;
pub const traceError = logging.traceError;

// Input prompts
pub const prompt = @import("prompt.zig");
pub const Prompt = prompt.Prompt;
pub const IntPrompt = prompt.IntPrompt;
pub const FloatPrompt = prompt.FloatPrompt;
pub const Confirm = prompt.Confirm;
pub const PromptError = prompt.PromptError;
pub const ValidationResult = prompt.ValidationResult;
pub const ValidatorFn = prompt.ValidatorFn;

// Pretty printing
pub const pretty = @import("pretty.zig");
pub const Pretty = pretty.Pretty;
pub const PrettyTheme = pretty.PrettyTheme;
pub const PrettyOptions = pretty.PrettyOptions;

// Prelude: convenience module for rapid prototyping
pub const prelude = @import("prelude.zig");

// Re-export tests from all modules
test {
    // Phase 1
    _ = @import("color.zig");
    _ = @import("cells.zig");
    _ = @import("style.zig");
    _ = @import("segment.zig");

    // Measurement
    _ = @import("measure.zig");

    // Phase 2
    _ = @import("box.zig");
    _ = @import("markup.zig");
    _ = @import("text.zig");

    // Highlighter
    _ = @import("highlighter.zig");

    // ANSI parsing
    _ = @import("ansi.zig");

    // Phase 3
    _ = @import("terminal.zig");
    _ = @import("console.zig");

    // Theme
    _ = @import("theme.zig");

    // Phase 4
    _ = @import("renderables/mod.zig");

    // Emoji support
    _ = @import("emoji.zig");

    // Logging
    _ = @import("logging.zig");

    // Prompts
    _ = @import("prompt.zig");

    // Pretty printing
    _ = @import("pretty.zig");
}

// Basic library functionality tests
test "basic style creation" {
    const s = Style.empty.bold().foreground(Color.red);
    try std.testing.expect(s.hasAttribute(.bold));
    try std.testing.expect(s.color != null);
}

test "basic segment creation" {
    const seg = Segment.styled("Hello", Style.empty.bold());
    try std.testing.expectEqual(@as(usize, 5), seg.cellLength());
}

test "basic cell width" {
    try std.testing.expectEqual(@as(usize, 5), cells.cellLen("Hello"));
    try std.testing.expectEqual(@as(usize, 4), cells.cellLen("\u{4E2D}\u{6587}")); // 2 CJK = 4 cells
}

test "basic text from markup" {
    const allocator = std.testing.allocator;
    var t = try Text.fromMarkup(allocator, "[bold]Hello[/]");
    defer t.deinit();
    try std.testing.expectEqualStrings("Hello", t.plain);
}

test "basic box style" {
    try std.testing.expectEqualStrings("+", BoxStyle.ascii.top_left);
    try std.testing.expectEqualStrings("\u{256D}", BoxStyle.rounded.top_left);
}

test "basic console creation" {
    const allocator = std.testing.allocator;
    var c = Console.init(allocator, std.testing.io, std.testing.environ);
    defer c.deinit();
    try std.testing.expect(c.width() > 0);
}

test "basic panel creation" {
    const allocator = std.testing.allocator;
    const panel = Panel.fromText(allocator, "Test");
    try std.testing.expectEqualStrings("Test", panel.content.text);
}

test "basic table creation" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();
    _ = table.addColumn("A").addColumn("B");
    try std.testing.expectEqual(@as(usize, 2), table.columns.items.len);
}
