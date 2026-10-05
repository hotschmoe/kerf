const std = @import("std");
const Segment = @import("../../segment.zig").Segment;
const Style = @import("../../style.zig").Style;
const cells = @import("../../cells.zig");
const BoxStyle = @import("../../box.zig").BoxStyle;

const cell_mod = @import("cell.zig");
const column_mod = @import("column.zig");

pub const Cell = cell_mod.Cell;
pub const CellContent = cell_mod.CellContent;
pub const JustifyMethod = cell_mod.JustifyMethod;
pub const RowSpanTracker = cell_mod.RowSpanTracker;
pub const Column = column_mod.Column;
pub const Overflow = column_mod.Overflow;

pub const AlternatingStyles = struct {
    even: Style,
    odd: Style,
};

/// A flexible table renderer with support for headers, footers, row/column spanning,
/// and alternating row styles.
///
/// Table uses pointer returns (*Table) for configuration methods because it owns
/// allocated collections (columns, rows). This differs from Panel/Rule which use
/// value returns for borrowed content. Returning *Table prevents shallow copies
/// that would share backing array pointers.
///
/// Example:
/// ```
/// var table = Table.init(allocator);
/// defer table.deinit();
/// _ = table.addColumn("Name").addColumn("Value");
/// try table.addRow(&.{ "foo", "123" });
/// const segments = try table.render(80, allocator);
/// defer allocator.free(segments);
/// ```
pub const Table = struct {
    columns: std.ArrayList(Column),
    rows: std.ArrayList([]const []const u8),
    rich_rows: std.ArrayList([]const CellContent),
    spanned_rows: std.ArrayList([]const Cell),
    title: ?[]const u8 = null,
    title_style: Style = Style.empty,
    caption: ?[]const u8 = null,
    caption_style: Style = Style.empty,
    caption_justify: JustifyMethod = .center,
    header_style: Style = Style.empty,
    border_style: Style = Style.empty,
    box_style: BoxStyle = BoxStyle.square,
    show_header: bool = true,
    show_edge: bool = true,
    show_lines: bool = false,
    padding: struct { left: u8, right: u8 } = .{ .left = 1, .right = 1 },
    collapse_padding: bool = false,
    row_styles: std.ArrayList(?Style),
    spanned_row_styles: std.ArrayList(?Style),
    alternating_styles: ?AlternatingStyles = null,
    footer: ?[]const []const u8 = null,
    footer_style: Style = Style.empty,
    allocator: std.mem.Allocator,

    pub fn init(allocator: std.mem.Allocator) Table {
        return .{
            .columns = std.ArrayList(Column).empty,
            .rows = std.ArrayList([]const []const u8).empty,
            .rich_rows = std.ArrayList([]const CellContent).empty,
            .spanned_rows = std.ArrayList([]const Cell).empty,
            .row_styles = std.ArrayList(?Style).empty,
            .spanned_row_styles = std.ArrayList(?Style).empty,
            .allocator = allocator,
        };
    }

    pub fn deinit(self: *Table) void {
        self.columns.deinit(self.allocator);
        for (self.rows.items) |row| {
            self.allocator.free(row);
        }
        self.rows.deinit(self.allocator);
        for (self.rich_rows.items) |row| {
            self.allocator.free(row);
        }
        self.rich_rows.deinit(self.allocator);
        for (self.spanned_rows.items) |row| {
            self.allocator.free(row);
        }
        self.spanned_rows.deinit(self.allocator);
        self.row_styles.deinit(self.allocator);
        self.spanned_row_styles.deinit(self.allocator);
    }

    pub fn withTitle(self: *Table, title: []const u8) *Table {
        self.title = title;
        return self;
    }

    pub fn withColumn(self: *Table, col: Column) *Table {
        self.columns.append(self.allocator, col) catch {};
        return self;
    }

    pub fn addColumn(self: *Table, header: []const u8) *Table {
        return self.withColumn(Column.init(header));
    }

    pub fn addRow(self: *Table, row: []const []const u8) !void {
        const row_copy = try self.allocator.alloc([]const u8, row.len);
        @memcpy(row_copy, row);
        try self.rows.append(self.allocator, row_copy);
        try self.row_styles.append(self.allocator, null);
    }

    pub fn addRowStyled(self: *Table, row: []const []const u8, style: Style) !void {
        const row_copy = try self.allocator.alloc([]const u8, row.len);
        @memcpy(row_copy, row);
        try self.rows.append(self.allocator, row_copy);
        try self.row_styles.append(self.allocator, style);
    }

    pub fn addRowRich(self: *Table, row: []const CellContent) !void {
        const row_copy = try self.allocator.alloc(CellContent, row.len);
        @memcpy(row_copy, row);
        try self.rich_rows.append(self.allocator, row_copy);
        try self.row_styles.append(self.allocator, null);
    }

    pub fn addRowRichStyled(self: *Table, row: []const CellContent, style: Style) !void {
        const row_copy = try self.allocator.alloc(CellContent, row.len);
        @memcpy(row_copy, row);
        try self.rich_rows.append(self.allocator, row_copy);
        try self.row_styles.append(self.allocator, style);
    }

    pub fn addSpannedRow(self: *Table, row: []const Cell) !void {
        const row_copy = try self.allocator.alloc(Cell, row.len);
        @memcpy(row_copy, row);
        try self.spanned_rows.append(self.allocator, row_copy);
        try self.spanned_row_styles.append(self.allocator, null);
    }

    pub fn addSpannedRowStyled(self: *Table, row: []const Cell, style: Style) !void {
        const row_copy = try self.allocator.alloc(Cell, row.len);
        @memcpy(row_copy, row);
        try self.spanned_rows.append(self.allocator, row_copy);
        try self.spanned_row_styles.append(self.allocator, style);
    }

    pub fn withRowStyle(self: *Table, row_index: usize, style: Style) *Table {
        if (row_index < self.row_styles.items.len) {
            self.row_styles.items[row_index] = style;
        }
        return self;
    }

    pub fn withAlternatingStyles(self: *Table, even: Style, odd: Style) *Table {
        self.alternating_styles = .{ .even = even, .odd = odd };
        return self;
    }

    pub fn withBoxStyle(self: *Table, style: BoxStyle) *Table {
        self.box_style = style;
        return self;
    }

    pub fn withHeaderStyle(self: *Table, style: Style) *Table {
        self.header_style = style;
        return self;
    }

    pub fn withBorderStyle(self: *Table, style: Style) *Table {
        self.border_style = style;
        return self;
    }

    pub fn withCaption(self: *Table, caption_text: []const u8) *Table {
        self.caption = caption_text;
        return self;
    }

    pub fn withCaptionStyle(self: *Table, style: Style) *Table {
        self.caption_style = style;
        return self;
    }

    pub fn withCaptionJustify(self: *Table, justify: JustifyMethod) *Table {
        self.caption_justify = justify;
        return self;
    }

    pub fn withCollapsePadding(self: *Table, collapse: bool) *Table {
        self.collapse_padding = collapse;
        return self;
    }

    pub fn withFooter(self: *Table, footer_row: []const []const u8) *Table {
        self.footer = footer_row;
        return self;
    }

    pub fn withFooterStyle(self: *Table, style: Style) *Table {
        self.footer_style = style;
        return self;
    }

    pub fn render(self: Table, max_width: usize, allocator: std.mem.Allocator) ![]Segment {
        var segments: std.ArrayList(Segment) = .empty;

        if (self.columns.items.len == 0) {
            return segments.toOwnedSlice(allocator);
        }

        const col_widths = try self.calculateColumnWidths(max_width, allocator);
        defer allocator.free(col_widths);

        const b = self.box_style;

        if (self.show_edge) {
            try self.renderHorizontalBorder(&segments, allocator, col_widths, b.top_left, b.horizontal, b.top_right, b.top_tee);
        }

        if (self.show_header) {
            try self.renderHeaderRow(&segments, allocator, col_widths, b);
            try self.renderHorizontalBorder(&segments, allocator, col_widths, b.left_tee, b.horizontal, b.right_tee, b.cross);
        }

        const total_text_rows = self.rows.items.len;
        const total_rich_rows = self.rich_rows.items.len;
        const total_spanned_rows = self.spanned_rows.items.len;
        const total_rows = total_text_rows + total_rich_rows + total_spanned_rows;

        for (self.rows.items, 0..) |row, idx| {
            try self.renderDataRow(&segments, allocator, row, col_widths, b, idx);
            if (self.show_lines and idx < total_rows - 1) {
                try self.renderHorizontalBorder(&segments, allocator, col_widths, b.left_tee, b.horizontal, b.right_tee, b.cross);
            }
        }

        for (self.rich_rows.items, 0..) |row, idx| {
            const row_idx = total_text_rows + idx;
            try self.renderRichDataRow(&segments, allocator, row, col_widths, b, row_idx);
            if (self.show_lines and row_idx < total_rows - 1) {
                try self.renderHorizontalBorder(&segments, allocator, col_widths, b.left_tee, b.horizontal, b.right_tee, b.cross);
            }
        }

        if (self.spanned_rows.items.len > 0) {
            var span_tracker = try RowSpanTracker.init(allocator, self.columns.items.len);
            defer span_tracker.deinit();

            for (self.spanned_rows.items, 0..) |row, idx| {
                const row_idx = total_text_rows + total_rich_rows + idx;
                try self.renderSpannedDataRowWithTracker(&segments, allocator, row, col_widths, b, row_idx, &span_tracker);
                span_tracker.advanceRow();

                if (self.show_lines and row_idx < total_rows - 1) {
                    try self.renderHorizontalBorderWithSpans(&segments, allocator, col_widths, b.left_tee, b.horizontal, b.right_tee, b.cross, b.vertical, &span_tracker);
                }
            }
        }

        if (self.footer) |footer_row| {
            try self.renderHorizontalBorder(&segments, allocator, col_widths, b.left_tee, b.horizontal, b.right_tee, b.cross);
            try self.renderFooterRow(&segments, allocator, footer_row, col_widths, b);
        }

        if (self.show_edge) {
            try self.renderHorizontalBorder(&segments, allocator, col_widths, b.bottom_left, b.horizontal, b.bottom_right, b.bottom_tee);
        }

        if (self.caption) |caption_text| {
            try self.renderCaption(&segments, allocator, col_widths, caption_text);
        }

        return segments.toOwnedSlice(allocator);
    }

    fn calculateColumnWidths(self: Table, max_width: usize, allocator: std.mem.Allocator) ![]usize {
        var widths = try allocator.alloc(usize, self.columns.items.len);

        for (self.columns.items, 0..) |col, i| {
            widths[i] = cells.cellLen(col.header);
        }

        for (self.rows.items) |row| {
            for (row, 0..) |cell, i| {
                if (i < widths.len) {
                    const cell_width = cells.cellLen(cell);
                    if (cell_width > widths[i]) {
                        widths[i] = cell_width;
                    }
                }
            }
        }

        for (self.rich_rows.items) |row| {
            for (row, 0..) |cell, i| {
                if (i < widths.len) {
                    const cell_width = cell.getCellWidth();
                    if (cell_width > widths[i]) {
                        widths[i] = cell_width;
                    }
                }
            }
        }

        for (self.spanned_rows.items) |row| {
            var col_idx: usize = 0;
            for (row) |cell| {
                if (col_idx >= widths.len) break;
                const span = @min(cell.colspan, @as(u8, @intCast(widths.len - col_idx)));
                if (span == 1) {
                    const cell_width = cell.getCellWidth();
                    if (cell_width > widths[col_idx]) {
                        widths[col_idx] = cell_width;
                    }
                }
                col_idx += span;
            }
        }

        for (self.columns.items, 0..) |col, i| {
            if (col.width) |w| {
                widths[i] = w;
            } else {
                if (col.min_width) |min| {
                    if (widths[i] < min) widths[i] = min;
                }
                if (col.max_width) |max| {
                    if (widths[i] > max) widths[i] = max;
                }
            }
        }

        var has_ratio = false;
        var total_ratio: usize = 0;
        for (self.columns.items) |col| {
            if (col.ratio) |r| {
                has_ratio = true;
                total_ratio += r;
            }
        }

        if (has_ratio and total_ratio > 0) {
            const borders_width: usize = if (self.show_edge) 2 else 0;
            const separators_width: usize = if (self.columns.items.len > 1) self.columns.items.len - 1 else 0;
            const overhead = borders_width + separators_width;

            var fixed_width: usize = 0;
            for (self.columns.items, 0..) |col, i| {
                if (col.ratio == null) {
                    fixed_width += widths[i] + self.padding.left + self.padding.right;
                }
            }

            const available = if (max_width > overhead + fixed_width) max_width - overhead - fixed_width else 0;

            for (self.columns.items, 0..) |col, i| {
                if (col.ratio) |r| {
                    const ratio_width = (available * r) / total_ratio;
                    const content_width = if (ratio_width > self.padding.left + self.padding.right)
                        ratio_width - self.padding.left - self.padding.right
                    else
                        0;
                    widths[i] = content_width;
                }
            }
        }

        for (self.spanned_rows.items) |row| {
            var col_idx: usize = 0;
            for (row) |cell| {
                if (col_idx >= widths.len) break;
                const span = @min(cell.colspan, @as(u8, @intCast(widths.len - col_idx)));
                if (span > 1) {
                    const cell_width = cell.getCellWidth();
                    var combined_width: usize = 0;
                    for (col_idx..col_idx + span) |j| {
                        combined_width += widths[j];
                    }
                    combined_width += span - 1;

                    if (cell_width > combined_width) {
                        const extra = cell_width - combined_width;
                        const per_col = extra / span;
                        const remainder = extra % span;
                        for (col_idx..col_idx + span) |j| {
                            widths[j] += per_col;
                            if (j - col_idx < remainder) {
                                widths[j] += 1;
                            }
                        }
                    }
                }
                col_idx += span;
            }
        }

        if (!self.collapse_padding) {
            for (widths) |*w| {
                w.* += self.padding.left + self.padding.right;
            }
        }

        return widths;
    }

    fn renderHorizontalBorder(
        self: Table,
        segments: *std.ArrayList(Segment),
        allocator: std.mem.Allocator,
        widths: []usize,
        left: []const u8,
        horizontal: []const u8,
        right: []const u8,
        cross: []const u8,
    ) !void {
        try segments.append(allocator, Segment.styled(left, self.border_style));

        for (widths, 0..) |w, i| {
            for (0..w) |_| {
                try segments.append(allocator, Segment.styled(horizontal, self.border_style));
            }
            if (i < widths.len - 1) {
                try segments.append(allocator, Segment.styled(cross, self.border_style));
            }
        }

        try segments.append(allocator, Segment.styled(right, self.border_style));
        try segments.append(allocator, Segment.line());
    }

    fn renderHeaderRow(self: Table, segments: *std.ArrayList(Segment), allocator: std.mem.Allocator, widths: []usize, b: BoxStyle) !void {
        try segments.append(allocator, Segment.styled(b.left, self.border_style));

        for (self.columns.items, 0..) |col, i| {
            const combined_style = self.header_style.combine(col.header_style);
            try self.renderCell(segments, allocator, col.header, widths[i], col.justify, combined_style, col);
            if (i < self.columns.items.len - 1) {
                try segments.append(allocator, Segment.styled(b.vertical, self.border_style));
            }
        }

        try segments.append(allocator, Segment.styled(b.right, self.border_style));
        try segments.append(allocator, Segment.line());
    }

    fn renderDataRow(self: Table, segments: *std.ArrayList(Segment), allocator: std.mem.Allocator, row: []const []const u8, widths: []usize, b: BoxStyle, row_idx: usize) !void {
        try segments.append(allocator, Segment.styled(b.left, self.border_style));

        const row_style = self.getRowStyle(row_idx);

        for (self.columns.items, 0..) |col, i| {
            const cell = if (i < row.len) row[i] else "";

            var effective_style = col.style;
            if (row_style) |rs| {
                effective_style = rs.combine(col.style);
            }

            try self.renderCell(segments, allocator, cell, widths[i], col.justify, effective_style, col);
            if (i < self.columns.items.len - 1) {
                try segments.append(allocator, Segment.styled(b.vertical, self.border_style));
            }
        }

        try segments.append(allocator, Segment.styled(b.right, self.border_style));
        try segments.append(allocator, Segment.line());
    }

    fn renderRichDataRow(self: Table, segments: *std.ArrayList(Segment), allocator: std.mem.Allocator, row: []const CellContent, widths: []usize, b: BoxStyle, row_idx: usize) !void {
        try segments.append(allocator, Segment.styled(b.left, self.border_style));

        const row_style = self.getRowStyle(row_idx);

        for (self.columns.items, 0..) |col, i| {
            const cell = if (i < row.len) row[i] else CellContent{ .text = "" };

            var effective_style = col.style;
            if (row_style) |rs| {
                effective_style = rs.combine(col.style);
            }

            try self.renderRichCell(segments, allocator, cell, widths[i], col.justify, effective_style, col);
            if (i < self.columns.items.len - 1) {
                try segments.append(allocator, Segment.styled(b.vertical, self.border_style));
            }
        }

        try segments.append(allocator, Segment.styled(b.right, self.border_style));
        try segments.append(allocator, Segment.line());
    }

    fn renderSpannedDataRowWithTracker(self: Table, segments: *std.ArrayList(Segment), allocator: std.mem.Allocator, row: []const Cell, widths: []usize, b: BoxStyle, row_idx: usize, tracker: *RowSpanTracker) !void {
        try segments.append(allocator, Segment.styled(b.left, self.border_style));

        const row_style = self.getSpannedRowStyle(row_idx);
        var col_idx: usize = 0;
        var cell_idx: usize = 0;

        while (col_idx < self.columns.items.len) {
            if (tracker.isBlocked(col_idx)) {
                try self.renderSpaces(segments, allocator, widths[col_idx]);
                col_idx += 1;

                if (col_idx < self.columns.items.len) {
                    try segments.append(allocator, Segment.styled(b.vertical, self.border_style));
                }
                continue;
            }

            const cell = if (cell_idx < row.len) row[cell_idx] else Cell.text("");
            const span = @min(cell.colspan, @as(u8, @intCast(self.columns.items.len - col_idx)));

            if (cell.rowspan > 1) {
                tracker.registerSpan(col_idx, cell);
            }

            var total_width: usize = 0;
            for (col_idx..col_idx + span) |j| {
                total_width += widths[j];
            }
            if (span > 1) {
                total_width += span - 1;
            }

            const col = self.columns.items[col_idx];

            var effective_style = col.style;
            if (row_style) |rs| {
                effective_style = rs.combine(col.style);
            }
            if (cell.style) |cs| {
                effective_style = effective_style.combine(cs);
            }

            const justify = cell.justify orelse col.justify;

            try self.renderSpannedCell(segments, allocator, cell, total_width, justify, effective_style, col);

            col_idx += span;
            cell_idx += 1;

            if (col_idx < self.columns.items.len) {
                try segments.append(allocator, Segment.styled(b.vertical, self.border_style));
            }
        }

        try segments.append(allocator, Segment.styled(b.right, self.border_style));
        try segments.append(allocator, Segment.line());
    }

    fn renderHorizontalBorderWithSpans(
        self: Table,
        segments: *std.ArrayList(Segment),
        allocator: std.mem.Allocator,
        widths: []usize,
        left: []const u8,
        horizontal: []const u8,
        right: []const u8,
        cross: []const u8,
        vertical: []const u8,
        tracker: *RowSpanTracker,
    ) !void {
        try segments.append(allocator, Segment.styled(left, self.border_style));

        for (widths, 0..) |w, i| {
            if (tracker.isBlocked(i)) {
                for (0..w) |_| {
                    try segments.append(allocator, Segment.plain(" "));
                }
            } else {
                for (0..w) |_| {
                    try segments.append(allocator, Segment.styled(horizontal, self.border_style));
                }
            }

            if (i < widths.len - 1) {
                const both_blocked = tracker.isBlocked(i) and tracker.isBlocked(i + 1);
                const char = if (both_blocked) vertical else cross;
                try segments.append(allocator, Segment.styled(char, self.border_style));
            }
        }

        try segments.append(allocator, Segment.styled(right, self.border_style));
        try segments.append(allocator, Segment.line());
    }

    fn getRowStyle(self: Table, row_idx: usize) ?Style {
        if (row_idx < self.row_styles.items.len and self.row_styles.items[row_idx] != null) {
            return self.row_styles.items[row_idx];
        }

        if (self.alternating_styles) |alt| {
            return if (row_idx % 2 == 0) alt.even else alt.odd;
        }

        return null;
    }

    fn getSpannedRowStyle(self: Table, row_idx: usize) ?Style {
        const base_idx = self.rows.items.len + self.rich_rows.items.len;
        if (row_idx >= base_idx) {
            const spanned_idx = row_idx - base_idx;
            if (spanned_idx < self.spanned_row_styles.items.len) {
                if (self.spanned_row_styles.items[spanned_idx]) |style| {
                    return style;
                }
            }
        }

        if (self.alternating_styles) |alt| {
            return if (row_idx % 2 == 0) alt.even else alt.odd;
        }

        return null;
    }

    fn renderCell(self: Table, segments: *std.ArrayList(Segment), allocator: std.mem.Allocator, text: []const u8, width: usize, justify: JustifyMethod, style: Style, col: Column) !void {
        const pad_left: usize = if (self.collapse_padding) 0 else self.padding.left;
        const pad_right: usize = if (self.collapse_padding) 0 else self.padding.right;
        const content_width = if (width > pad_left + pad_right)
            width - pad_left - pad_right
        else
            width;

        var display_text = text;
        const text_width = cells.cellLen(text);

        if (col.no_wrap and text_width > content_width) {
            switch (col.overflow) {
                .fold => {},
                .ellipsis => {
                    const ellipsis_len = cells.cellLen(col.ellipsis);
                    if (content_width > ellipsis_len) {
                        const truncate_width = content_width - ellipsis_len;
                        const byte_pos = cells.cellToByteIndex(text, truncate_width);
                        display_text = text[0..byte_pos];
                    } else {
                        display_text = col.ellipsis;
                    }
                },
                .crop => {
                    const byte_pos = cells.cellToByteIndex(text, content_width);
                    display_text = text[0..byte_pos];
                },
            }
        }

        const display_width = cells.cellLen(display_text);
        const padding_total = if (content_width > display_width) content_width - display_width else 0;

        const needs_ellipsis = col.no_wrap and col.overflow == .ellipsis and text_width > content_width;
        const ellipsis_width = if (needs_ellipsis) cells.cellLen(col.ellipsis) else 0;

        var adjusted_padding: usize = padding_total;
        if (needs_ellipsis) {
            adjusted_padding = if (padding_total >= ellipsis_width) padding_total - ellipsis_width else 0;
        }

        const left_pad: usize = switch (justify) {
            .left => pad_left,
            .right => pad_left + adjusted_padding,
            .center => pad_left + adjusted_padding / 2,
        };
        const right_pad = if (width > left_pad + display_width + (if (needs_ellipsis) ellipsis_width else 0))
            width - left_pad - display_width - (if (needs_ellipsis) ellipsis_width else 0)
        else
            0;

        try self.renderSpaces(segments, allocator, left_pad);
        try segments.append(allocator, Segment.styledOptional(display_text, if (style.isEmpty()) null else style));

        if (needs_ellipsis) {
            try segments.append(allocator, Segment.styledOptional(col.ellipsis, if (style.isEmpty()) null else style));
        }

        try self.renderSpaces(segments, allocator, right_pad);
    }

    fn renderRichCell(self: Table, segments: *std.ArrayList(Segment), allocator: std.mem.Allocator, content: CellContent, width: usize, justify: JustifyMethod, style: Style, _: Column) !void {
        const pad_left: usize = if (self.collapse_padding) 0 else self.padding.left;
        const pad_right: usize = if (self.collapse_padding) 0 else self.padding.right;
        const content_width = if (width > pad_left + pad_right)
            width - pad_left - pad_right
        else
            width;

        const cell_width = content.getCellWidth();
        const padding_total = if (content_width > cell_width) content_width - cell_width else 0;

        const left_pad: usize = switch (justify) {
            .left => pad_left,
            .right => pad_left + padding_total,
            .center => pad_left + padding_total / 2,
        };
        const right_pad = if (width > left_pad + cell_width)
            width - left_pad - cell_width
        else
            0;

        try self.renderSpaces(segments, allocator, left_pad);

        switch (content) {
            .text => |text| {
                try segments.append(allocator, Segment.styledOptional(text, if (style.isEmpty()) null else style));
            },
            .segments => |segs| {
                for (segs) |seg| {
                    const combined_style = if (seg.style) |s| style.combine(s) else style;
                    try segments.append(allocator, Segment.styledOptional(seg.text, if (combined_style.isEmpty()) null else combined_style));
                }
            },
        }

        try self.renderSpaces(segments, allocator, right_pad);
    }

    fn renderSpannedCell(self: Table, segments: *std.ArrayList(Segment), allocator: std.mem.Allocator, cell: Cell, width: usize, justify: JustifyMethod, style: Style, _: Column) !void {
        const pad_left: usize = if (self.collapse_padding) 0 else self.padding.left;
        const pad_right: usize = if (self.collapse_padding) 0 else self.padding.right;
        const content_width = if (width > pad_left + pad_right)
            width - pad_left - pad_right
        else
            width;

        const cell_width = cell.getCellWidth();
        const padding_total = if (content_width > cell_width) content_width - cell_width else 0;

        const left_pad: usize = switch (justify) {
            .left => pad_left,
            .right => pad_left + padding_total,
            .center => pad_left + padding_total / 2,
        };
        const right_pad = if (width > left_pad + cell_width)
            width - left_pad - cell_width
        else
            0;

        try self.renderSpaces(segments, allocator, left_pad);

        switch (cell.content) {
            .text => |text| {
                try segments.append(allocator, Segment.styledOptional(text, if (style.isEmpty()) null else style));
            },
            .segments => |segs| {
                for (segs) |seg| {
                    const combined_style = if (seg.style) |s| style.combine(s) else style;
                    try segments.append(allocator, Segment.styledOptional(seg.text, if (combined_style.isEmpty()) null else combined_style));
                }
            },
        }

        try self.renderSpaces(segments, allocator, right_pad);
    }

    fn renderSpaces(_: Table, segments: *std.ArrayList(Segment), allocator: std.mem.Allocator, count: usize) !void {
        for (0..count) |_| {
            try segments.append(allocator, Segment.plain(" "));
        }
    }

    fn renderFooterRow(self: Table, segments: *std.ArrayList(Segment), allocator: std.mem.Allocator, footer_row: []const []const u8, widths: []usize, b: BoxStyle) !void {
        try segments.append(allocator, Segment.styled(b.left, self.border_style));

        for (self.columns.items, 0..) |col, i| {
            const cell = if (i < footer_row.len) footer_row[i] else "";
            const effective_style = self.footer_style.combine(col.style);

            try self.renderCell(segments, allocator, cell, widths[i], col.justify, effective_style, col);
            if (i < self.columns.items.len - 1) {
                try segments.append(allocator, Segment.styled(b.vertical, self.border_style));
            }
        }

        try segments.append(allocator, Segment.styled(b.right, self.border_style));
        try segments.append(allocator, Segment.line());
    }

    fn renderCaption(self: Table, segments: *std.ArrayList(Segment), allocator: std.mem.Allocator, widths: []usize, caption_text: []const u8) !void {
        var total_width: usize = 0;
        for (widths) |w| {
            total_width += w;
        }
        if (widths.len > 1) {
            total_width += widths.len - 1;
        }
        if (self.show_edge) {
            total_width += 2;
        }

        const caption_len = cells.cellLen(caption_text);
        const padding_total = if (total_width > caption_len) total_width - caption_len else 0;

        const left_pad: usize = switch (self.caption_justify) {
            .left => 0,
            .center => padding_total / 2,
            .right => padding_total,
        };
        const right_pad = padding_total - left_pad;

        try self.renderSpaces(segments, allocator, left_pad);
        try segments.append(allocator, Segment.styledOptional(caption_text, if (self.caption_style.isEmpty()) null else self.caption_style));
        try self.renderSpaces(segments, allocator, right_pad);
        try segments.append(allocator, Segment.line());
    }
};
