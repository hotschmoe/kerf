pub const bar = @import("bar.zig");
pub const ProgressBar = bar.ProgressBar;
pub const SpeedUnit = bar.SpeedUnit;

pub const spinner = @import("spinner.zig");
pub const Spinner = spinner.Spinner;

pub const group = @import("group.zig");
pub const ProgressGroup = group.ProgressGroup;
pub const ProgressDisplay = group.ProgressDisplay;
pub const ColumnRenderContext = group.ColumnRenderContext;
pub const ColumnWidth = group.ColumnWidth;
pub const BuiltinColumn = group.BuiltinColumn;
pub const CustomColumnFn = group.CustomColumnFn;
pub const ProgressColumn = group.ProgressColumn;
pub const Progress = group.Progress;

test {
    _ = @import("bar.zig");
    _ = @import("spinner.zig");
    _ = @import("group.zig");
}
