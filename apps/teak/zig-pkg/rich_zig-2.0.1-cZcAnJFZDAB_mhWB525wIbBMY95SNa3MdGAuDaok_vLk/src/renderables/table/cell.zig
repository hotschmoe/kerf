const std = @import("std");
const Segment = @import("../../segment.zig").Segment;
const segment_mod = @import("../../segment.zig");
const Style = @import("../../style.zig").Style;
const cells = @import("../../cells.zig");

pub const JustifyMethod = enum {
    left,
    center,
    right,
};

pub const CellContent = union(enum) {
    text: []const u8,
    segments: []const Segment,

    pub fn getText(self: CellContent) []const u8 {
        return switch (self) {
            .text => |t| t,
            .segments => |segs| if (segs.len > 0) segs[0].text else "",
        };
    }

    pub fn getCellWidth(self: CellContent) usize {
        return switch (self) {
            .text => |t| cells.cellLen(t),
            .segments => |segs| segment_mod.totalCellLength(segs),
        };
    }
};

pub const Cell = struct {
    content: CellContent,
    colspan: u8 = 1,
    rowspan: u8 = 1,
    style: ?Style = null,
    justify: ?JustifyMethod = null,

    pub fn text(t: []const u8) Cell {
        return .{ .content = .{ .text = t } };
    }

    pub fn segments(segs: []const Segment) Cell {
        return .{ .content = .{ .segments = segs } };
    }

    pub fn withColspan(self: Cell, span: u8) Cell {
        var c = self;
        c.colspan = if (span == 0) 1 else span;
        return c;
    }

    pub fn withRowspan(self: Cell, span: u8) Cell {
        var c = self;
        c.rowspan = if (span == 0) 1 else span;
        return c;
    }

    pub fn withStyle(self: Cell, s: Style) Cell {
        var c = self;
        c.style = s;
        return c;
    }

    pub fn withJustify(self: Cell, j: JustifyMethod) Cell {
        var c = self;
        c.justify = j;
        return c;
    }

    pub fn getCellWidth(self: Cell) usize {
        return self.content.getCellWidth();
    }
};

pub const RowSpanTracker = struct {
    remaining: []u8,
    span_cells: []?Cell,
    allocator: std.mem.Allocator,

    pub fn init(allocator: std.mem.Allocator, num_columns: usize) !RowSpanTracker {
        const remaining = try allocator.alloc(u8, num_columns);
        @memset(remaining, 0);
        const span_cells = try allocator.alloc(?Cell, num_columns);
        @memset(span_cells, null);
        return .{
            .remaining = remaining,
            .span_cells = span_cells,
            .allocator = allocator,
        };
    }

    pub fn deinit(self: *RowSpanTracker) void {
        self.allocator.free(self.remaining);
        self.allocator.free(self.span_cells);
    }

    pub fn isBlocked(self: RowSpanTracker, col_idx: usize) bool {
        return col_idx < self.remaining.len and self.remaining[col_idx] > 0;
    }

    pub fn getSpanningCell(self: RowSpanTracker, col_idx: usize) ?Cell {
        if (col_idx >= self.span_cells.len) return null;
        return self.span_cells[col_idx];
    }

    pub fn registerSpan(self: *RowSpanTracker, col_idx: usize, cell: Cell) void {
        if (col_idx >= self.remaining.len) return;

        const span = cell.colspan;
        const end_col = @min(col_idx + span, self.remaining.len);

        for (col_idx..end_col) |j| {
            if (cell.rowspan > 1) {
                self.remaining[j] = cell.rowspan;
                self.span_cells[j] = cell;
            }
        }
    }

    pub fn advanceRow(self: *RowSpanTracker) void {
        for (0..self.remaining.len) |i| {
            if (self.remaining[i] > 0) {
                self.remaining[i] -= 1;
                if (self.remaining[i] == 0) {
                    self.span_cells[i] = null;
                }
            }
        }
    }

    pub fn hasActiveSpanInRange(self: RowSpanTracker, start: usize, end: usize) bool {
        for (start..@min(end, self.remaining.len)) |i| {
            if (self.remaining[i] > 0) return true;
        }
        return false;
    }
};
