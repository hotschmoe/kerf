# rich_zig

A full-featured Zig port of [Python Rich](https://github.com/Textualize/rich), targeting **100% feature parity** with [rich_rust](https://github.com/Dicklesworthstone/rich_rust).

```
+--------------------------------------------------+
|  rich_zig: Beautiful Terminal Output in Pure Zig |
|                                                  |
|  [bold green]Tables[/] | [cyan]Panels[/] | [yellow]Progress[/] | [magenta]Trees[/]  |
+--------------------------------------------------+
```

[![CI](https://github.com/hotschmoe/rich_zig/actions/workflows/ci.yml/badge.svg)](https://github.com/hotschmoe/rich_zig/actions/workflows/ci.yml)
[![Zig](https://img.shields.io/badge/Zig-0.15.2-orange)](https://ziglang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

## Overview

rich_zig brings rich text and beautiful formatting to Zig terminal applications. Create visually sophisticated CLI apps with styled text, tables, panels, progress bars, trees, syntax highlighting, and more.

### Design Goals

- **100% Feature Parity**: Complete port of Rich/rich_rust functionality
- **Zero Dependencies**: Core features use only Zig standard library
- **Pure Zig**: No unsafe code, explicit memory management
- **Terminal-Aware**: Automatic color detection and downgrading
- **Unicode-Correct**: Proper CJK, emoji, and combining mark handling
- **Allocator-Explicit**: All allocations visible and controllable
- **Cross-Platform**: Linux, macOS, Windows with consistent behavior

## Features

### Core (Phase 1-3) - Complete

| Component | Description | Status |
|-----------|-------------|--------|
| Color | 4-bit, 8-bit, 24-bit with auto-downgrade | Complete |
| Style | Bold, italic, underline, dim, reverse, strike, hyperlinks | Complete |
| Segment | Atomic rendering unit with text + style + control codes | Complete |
| Cells | Unicode width calculation (CJK, emoji, zero-width) | Complete |
| Markup | BBCode-like syntax `[bold red]text[/]` | Complete |
| Text | Styled text with spans, wrapping, alignment | Complete |
| Terminal | TTY, color, size, unicode detection | Complete |
| Console | Print, log, capture, export, status | Complete |

### Renderables (Phase 4) - Complete

| Component | Description | Status |
|-----------|-------------|--------|
| Panel | Bordered boxes with title/subtitle, all box styles | Complete |
| Table | Unicode tables with alignment, headers, footer, caption | Complete |
| Rule | Horizontal rules with title alignment | Complete |
| Progress | Progress bars, spinners, timing, speed, groups | Complete |
| Tree | Tree structures with guides, styling, collapse | Complete |
| Padding | Uniform and per-side padding with styling | Complete |
| Align | Horizontal and vertical alignment | Complete |
| Columns | Multi-column layout | Complete |
| Layout | Split views (horizontal/vertical) | Complete |
| Live | Real-time updating display | Complete |

### Optional (Phase 5) - Complete

| Component | Description | Status |
|-----------|-------------|--------|
| JSON | Pretty-printed JSON with themes | Complete |
| Syntax | Syntax highlighting | Complete |
| Markdown | Terminal markdown rendering | Complete |

### Color & Terminal (v1.4.0+)

| Component | Description | Status |
|-----------|-------------|--------|
| AdaptiveColor | Colors that auto-downgrade across color systems | Complete |
| HSL Blending | Perceptually smooth color transitions via HSL | Complete |
| Multi-stop Gradient | Generate N colors across arbitrary color stops | Complete |
| WCAG Contrast | Accessibility-aware contrast ratio checking | Complete |
| Synchronized Output | Flicker-free rendering via DEC mode 2026 | Complete |
| Background Detection | Dark/light terminal background heuristic | Complete |

## Installation

### Using Zig Package Manager (Recommended)

Add rich_zig to your project using `zig fetch`:

```bash
zig fetch --save git+https://github.com/hotschmoe/rich_zig.git
```

Or add manually to your `build.zig.zon`:

```zig
.dependencies = .{
    .rich_zig = .{
        .url = "git+https://github.com/hotschmoe/rich_zig.git",
        .hash = "...",  // Will be filled by zig fetch
    },
},
```

Then in your `build.zig`:

```zig
const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});

    // Fetch the rich_zig dependency
    const rich_zig = b.dependency("rich_zig", .{
        .target = target,
        .optimize = optimize,
    });

    // Create your executable
    const exe = b.addExecutable(.{
        .name = "my_app",
        .root_module = b.createModule(.{
            .root_source_file = b.path("src/main.zig"),
            .target = target,
            .optimize = optimize,
            .imports = &.{
                .{ .name = "rich_zig", .module = rich_zig.module("rich_zig") },
            },
        }),
    });

    b.installArtifact(exe);
}
```

### Using a Specific Version (Recommended)

```bash
zig fetch --save git+https://github.com/hotschmoe/rich_zig.git#v1.4.1
```

Or manually in `build.zig.zon`:

```zig
.dependencies = .{
    .rich_zig = .{
        .url = "git+https://github.com/hotschmoe/rich_zig.git#v1.4.1",
        .hash = "...",
    },
},
```

See [Releases](https://github.com/hotschmoe/rich_zig/releases) for all available versions.

### Local Development

Clone and reference locally:

```zig
.dependencies = .{
    .rich_zig = .{
        .path = "../rich_zig",
    },
},
```

### Verify Installation

Test your installation with this simple program:

```zig
const rich = @import("rich_zig");
const std = @import("std");

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    const allocator = gpa.allocator();

    var console = rich.Console.init(allocator);
    try console.print("[bold green]rich_zig v1.4.1 installed successfully![/]");
}
```

## Usage

### Basic Styled Output

```zig
const std = @import("std");
const rich = @import("rich_zig");

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    var console = rich.Console.init(allocator);

    // Markup syntax
    try console.print("[bold green]Success![/] Operation completed.");
    try console.print("[red on white]Error:[/] [italic]File not found[/]");

    // Horizontal rule
    try console.rule("Configuration");
}
```

### Tables

```zig
var table = rich.Table.init(allocator)
    .withTitle("Server Status")
    .withColumn(rich.Column.init("Service"))
    .withColumn(rich.Column.init("Status").withJustify(.center))
    .withColumn(rich.Column.init("Uptime").withJustify(.right));

try table.addRowCells(.{ "nginx", "Running", "14d 3h" });
try table.addRowCells(.{ "postgres", "Running", "14d 3h" });
try table.addRowCells(.{ "redis", "Stopped", "0s" });

try console.printRenderable(table);
```

Output:
```
+----------+---------+---------+
| Service  | Status  |  Uptime |
+----------+---------+---------+
| nginx    | Running |  14d 3h |
| postgres | Running |  14d 3h |
| redis    | Stopped |      0s |
+----------+---------+---------+
```

### Panels

```zig
const panel = rich.Panel.fromText(allocator, "Welcome to the system!")
    .withTitle("Message")
    .withSubtitle("v1.4.1")
    .withWidth(50)
    .rounded();

try console.printRenderable(panel);
```

Output:
```
+-- Message --------------------------+
| Welcome to the system!             |
+------------------------- v1.0.0 ---+
```

### Progress Bars

```zig
var i: usize = 0;
while (i <= 100) : (i += 5) {
    const bar = rich.ProgressBar.init()
        .withCompleted(i)
        .withTotal(100)
        .withWidth(40);

    try console.writer.writeAll("\r");
    try console.printRenderable(bar);
    std.time.sleep(100 * std.time.ns_per_ms);
}
```

### Trees

```zig
var root = rich.TreeNode.init(allocator, "project");

var src = try root.addChildLabel("src");
_ = try src.addChildLabel("main.zig");
_ = try src.addChildLabel("lib.zig");

_ = try root.addChildLabel("build.zig");
_ = try root.addChildLabel("README.md");

const tree = rich.Tree.init(root);
try console.printRenderable(tree);
```

Output:
```
project
+-- src
|   +-- main.zig
|   +-- lib.zig
+-- build.zig
+-- README.md
```

## Quick Reference

| Task | Code |
|------|------|
| Print styled text | `try console.print("[bold red]Error![/]")` |
| Create panel | `Panel.fromText(alloc, "text").withTitle("T")` |
| Info panel | `Panel.info(alloc, "message")` |
| Create table | `Table.init(alloc).withColumn(Column.init("A"))` |
| Add table row | `try table.addRowCells(.{ "a", "b", "c" })` |
| Progress bar | `ProgressBar.init().withCompleted(50).withTotal(100)` |
| Horizontal rule | `Rule.init().withTitle("Section")` |
| Tree node | `TreeNode.init(alloc, "root")` |
| Style composition | `Style.empty.bold().fg(Color.red)` |
| Adaptive color | `AdaptiveColor.fromRgb(255, 128, 0).resolve(.standard)` |
| HSL gradient | `gradient(&stops, &output, true)` |
| WCAG contrast | `color1.contrastRatio(color2)` |
| Prelude import | `const rich = @import("rich_zig").prelude;` |

## Architecture

```
+-------------------+
|     Console       |  Central output manager
+-------------------+
         |
         v
+-------------------+
|   Renderables     |  Panel, Table, Tree, etc.
+-------------------+
         |
         v
+-------------------+
|     Segment       |  Text + Style atomic unit
+-------------------+
         |
    +----+----+
    v         v
+-------+ +-------+
| Style | | Text  |
+-------+ +-------+
    |
    v
+-------+
| Color |
+-------+
```

### Module Structure

```
src/
+-- root.zig           # Library entry point (public API)
+-- main.zig           # CLI executable
+-- color.zig          # Color types and conversions
+-- style.zig          # Text styling attributes
+-- segment.zig        # Atomic render unit
+-- cells.zig          # Unicode width calculations
+-- text.zig           # Text with styled spans
+-- markup.zig         # Markup parser [bold]...[/]
+-- terminal.zig       # Terminal detection
+-- console.zig        # Output management
+-- measure.zig        # Measurement protocol
+-- theme.zig          # Named style registry
+-- highlighter.zig    # Auto-highlighting
+-- ansi.zig           # ANSI escape parsing
+-- emoji.zig          # Emoji shortcodes
+-- pretty.zig         # Comptime pretty printer
+-- box.zig            # Box drawing characters
+-- renderables/
    +-- panel.zig      # Bordered panels
    +-- table.zig      # Tables with columns
    +-- rule.zig       # Horizontal rules
    +-- progress.zig   # Progress bars, spinners
    +-- tree.zig       # Tree structures
    +-- json.zig       # JSON pretty-printing
```

## Color System

rich_zig automatically detects terminal color capabilities and downgrades colors as needed:

| System | Colors | Detection |
|--------|--------|-----------|
| Standard | 16 (4-bit) | Default fallback |
| 256 | 256 (8-bit) | TERM contains "256" |
| Truecolor | 16M (24-bit) | COLORTERM=truecolor |

```zig
// Explicit color specification
const style = Style.empty
    .foreground(Color.fromRgb(255, 128, 0))  // Truecolor
    .background(Color.from256(42));           // 256-color

// Named colors
const warning = Style.empty.foreground(Color.yellow).bold();
```

### Adaptive Colors

Colors that automatically downgrade to match terminal capabilities:

```zig
const rich = @import("rich_zig");

// Bundle colors for different terminal capabilities
const sunset = rich.AdaptiveColor.init(
    rich.Color.fromRgb(255, 100, 50),  // Truecolor
    rich.Color.from256(208),            // 256-color fallback
    rich.Color.yellow,                  // 16-color fallback
);

// Resolve for the current terminal
const color = sunset.resolve(console.colorSystem());

// Quick adaptive from RGB (auto-computes fallbacks)
const sky = rich.AdaptiveColor.fromRgb(0, 180, 255);
```

### HSL Blending & Gradients

Perceptually smooth color transitions through HSL color space:

```zig
const rich = @import("rich_zig");

// HSL blend between two colors
const red = rich.ColorTriplet{ .r = 255, .g = 0, .b = 0 };
const blue = rich.ColorTriplet{ .r = 0, .g = 0, .b = 255 };
const mid = rich.ColorTriplet.blendHsl(red, blue, 0.5);

// Multi-stop gradient
const stops = [_]rich.ColorTriplet{
    .{ .r = 255, .g = 0, .b = 0 },
    .{ .r = 0, .g = 255, .b = 0 },
    .{ .r = 0, .g = 0, .b = 255 },
};
var output: [30]rich.ColorTriplet = undefined;
rich.gradient(&stops, &output, true); // true = HSL interpolation
```

### WCAG Contrast Checking

Validate color pair accessibility per WCAG 2.0 guidelines:

```zig
const fg = rich.ColorTriplet{ .r = 255, .g = 255, .b = 255 };
const bg = rich.ColorTriplet{ .r = 0, .g = 0, .b = 0 };

const ratio = fg.contrastRatio(bg);  // 21.0:1
const level = fg.wcagLevel(bg);      // .aaa
```

## Building

```bash
# Build library and executable
zig build

# Run tests
zig build test

# Run executable
zig build run

# Build with optimizations
zig build -Doptimize=ReleaseFast
```

## Testing

See [docs/testing.md](docs/testing.md) for the comprehensive testing strategy.

```bash
# Run all tests
zig build test

# Run with specific optimization
zig build test -Doptimize=ReleaseSafe

# Run fuzz tests
zig build test --fuzz
```

## Feature Parity

We track 100% feature parity with Rich/rich_rust. See [docs/FEATURE_PARITY.md](docs/FEATURE_PARITY.md) for detailed progress.

| Phase | Components | Status |
|-------|------------|--------|
| Core (P0) | Color, Style, Segment, Cells, Markup, Text | 100% Complete |
| Console (P0) | Terminal detection, Console I/O | 100% Complete |
| Color/Terminal (P0) | AdaptiveColor, HSL, WCAG, Gradients, Sync Output | 100% Complete |
| Renderables (P1) | Panel, Table, Rule, Progress, Tree, Layout | 100% Complete |
| Optional (P2) | JSON, Syntax, Markdown | 100% Complete |

## CI/CD

Automated testing via GitHub Actions on PRs and release tags:

| Trigger | Tests | Release |
|---------|-------|---------|
| Pull Request | Yes | No |
| Tag `v*` | Yes | Yes (if tests pass) |

**Test Matrix**:
- **Platforms**: Linux, macOS, Windows
- **Optimization Levels**: Debug, ReleaseSafe, ReleaseFast, ReleaseSmall
- **Fuzz Testing**: Parser robustness (Linux)
- **Package Validation**: Consumer integration test

**Release Workflow**:
1. Update version in `build.zig.zon`
2. Commit and push to master
3. Create and push tag: `git tag v1.4.1 && git push origin v1.4.1`
4. CI runs full test suite
5. If tests pass, GitHub Release is created automatically

## Compatibility

| Requirement | Version |
|-------------|---------|
| Zig | 0.15.2+ |
| Platforms | Linux, macOS, Windows |
| Terminals | Any with ANSI support |

## Contributing

1. Fork the repository
2. Create a feature branch
3. Ensure tests pass: `zig build test`
4. Ensure formatting: `zig fmt src/`
5. Submit a pull request

See [docs/FEATURE_PARITY.md](docs/FEATURE_PARITY.md) for what needs implementation.

## References

- [Python Rich](https://github.com/Textualize/rich) - Original library
- [rich_rust](https://github.com/Dicklesworthstone/rich_rust) - Rust port
- [Unicode East Asian Width](https://www.unicode.org/reports/tr11/) - UAX #11
- [ANSI Escape Codes](https://en.wikipedia.org/wiki/ANSI_escape_code)

## License

MIT License - See LICENSE file for details.
