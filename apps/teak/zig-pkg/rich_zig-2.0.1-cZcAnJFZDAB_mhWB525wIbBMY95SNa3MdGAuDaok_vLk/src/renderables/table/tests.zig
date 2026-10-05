const std = @import("std");
const table_mod = @import("mod.zig");
const Table = table_mod.Table;
const Column = table_mod.Column;
const Cell = table_mod.Cell;
const CellContent = table_mod.CellContent;
const JustifyMethod = table_mod.JustifyMethod;
const Overflow = table_mod.Overflow;
const RowSpanTracker = table_mod.RowSpanTracker;
const Segment = @import("../../segment.zig").Segment;
const Style = @import("../../style.zig").Style;

test "Table.init" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    try std.testing.expectEqual(@as(usize, 0), table.columns.items.len);
}

test "Table.addColumn" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.addColumn("Name").addColumn("Value");

    try std.testing.expectEqual(@as(usize, 2), table.columns.items.len);
    try std.testing.expectEqualStrings("Name", table.columns.items[0].header);
}

test "Table.addRow" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.addColumn("A").addColumn("B");
    try table.addRow(&.{ "1", "2" });
    try table.addRow(&.{ "3", "4" });

    try std.testing.expectEqual(@as(usize, 2), table.rows.items.len);
}

test "Table.render basic" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.addColumn("Name").addColumn("Value");
    try table.addRow(&.{ "foo", "bar" });

    const segments = try table.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len > 0);
}

test "Column justify methods" {
    const col = Column.init("Test").withJustify(.center).withWidth(10);
    try std.testing.expectEqual(JustifyMethod.center, col.justify);
    try std.testing.expectEqual(@as(?usize, 10), col.width);
}

test "Table.withCaption" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.withCaption("Table Caption");

    try std.testing.expectEqualStrings("Table Caption", table.caption.?);
}

test "Table.withCaptionStyle" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.withCaptionStyle(Style.empty.italic());

    try std.testing.expect(table.caption_style.hasAttribute(.italic));
}

test "Table.withCaptionJustify" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.withCaptionJustify(.right);

    try std.testing.expectEqual(JustifyMethod.right, table.caption_justify);
}

test "Table.render with caption" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.addColumn("A").addColumn("B").withCaption("My Caption");
    try table.addRow(&.{ "1", "2" });

    const segments = try table.render(80, allocator);
    defer allocator.free(segments);

    var found_caption = false;
    for (segments) |seg| {
        if (std.mem.indexOf(u8, seg.text, "My Caption") != null) {
            found_caption = true;
            break;
        }
    }
    try std.testing.expect(found_caption);
}

test "Column.withRatio" {
    const col = Column.init("Test").withRatio(2);
    try std.testing.expectEqual(@as(?u8, 2), col.ratio);
}

test "Column.withOverflow" {
    const col = Column.init("Test").withOverflow(.ellipsis);
    try std.testing.expectEqual(Overflow.ellipsis, col.overflow);
}

test "Column.withEllipsis" {
    const col = Column.init("Test").withEllipsis("..");
    try std.testing.expectEqualStrings("..", col.ellipsis);
}

test "Table.withAlternatingStyles" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.withAlternatingStyles(Style.empty.dim(), Style.empty.bold());

    try std.testing.expect(table.alternating_styles != null);
    try std.testing.expect(table.alternating_styles.?.even.hasAttribute(.dim));
    try std.testing.expect(table.alternating_styles.?.odd.hasAttribute(.bold));
}

test "Table.addRowStyled" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.addColumn("A");
    try table.addRowStyled(&.{"1"}, Style.empty.bold());

    try std.testing.expectEqual(@as(usize, 1), table.rows.items.len);
    try std.testing.expect(table.row_styles.items[0] != null);
    try std.testing.expect(table.row_styles.items[0].?.hasAttribute(.bold));
}

test "Table.withRowStyle" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.addColumn("A");
    try table.addRow(&.{"1"});
    _ = table.withRowStyle(0, Style.empty.italic());

    try std.testing.expect(table.row_styles.items[0] != null);
    try std.testing.expect(table.row_styles.items[0].?.hasAttribute(.italic));
}

test "Table.render with alternating styles" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.addColumn("A");
    _ = table.withAlternatingStyles(Style.empty.dim(), Style.empty.bold());
    try table.addRow(&.{"row0"});
    try table.addRow(&.{"row1"});
    try table.addRow(&.{"row2"});

    const segments = try table.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len > 0);
}

test "Table.render with ratio columns" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.withColumn(Column.init("A").withRatio(1));
    _ = table.withColumn(Column.init("B").withRatio(2));
    _ = table.withColumn(Column.init("C").withRatio(1));
    try table.addRow(&.{ "one", "two", "three" });

    const segments = try table.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len > 0);
}

test "Table.render with overflow ellipsis" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.withColumn(Column.init("Col").withWidth(8).withNoWrap(true).withOverflow(.ellipsis));
    try table.addRow(&.{"This is a very long text"});

    const segments = try table.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len > 0);

    var found_ellipsis = false;
    for (segments) |seg| {
        if (std.mem.indexOf(u8, seg.text, "...") != null) {
            found_ellipsis = true;
            break;
        }
    }
    try std.testing.expect(found_ellipsis);
}

test "Table.withCollapsePadding" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.withCollapsePadding(true);
    try std.testing.expect(table.collapse_padding);
}

test "Table.render with collapse padding" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.addColumn("A").addColumn("B").withCollapsePadding(true);
    try table.addRow(&.{ "1", "2" });

    const segments = try table.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len > 0);
}

test "Table.withFooter" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.withFooter(&.{ "Total", "100" });
    try std.testing.expect(table.footer != null);
}

test "Table.withFooterStyle" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.withFooterStyle(Style.empty.bold());
    try std.testing.expect(table.footer_style.hasAttribute(.bold));
}

test "Table.render with footer" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.addColumn("Item").addColumn("Price");
    try table.addRow(&.{ "Apple", "1.00" });
    try table.addRow(&.{ "Banana", "0.50" });
    _ = table.withFooter(&.{ "Total", "1.50" });

    const segments = try table.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len > 0);

    var found_total = false;
    for (segments) |seg| {
        if (std.mem.indexOf(u8, seg.text, "Total") != null) {
            found_total = true;
            break;
        }
    }
    try std.testing.expect(found_total);
}

test "CellContent.text" {
    const content = CellContent{ .text = "hello" };
    try std.testing.expectEqualStrings("hello", content.getText());
    try std.testing.expectEqual(@as(usize, 5), content.getCellWidth());
}

test "CellContent.segments" {
    const segs = [_]Segment{
        Segment.plain("hello"),
        Segment.plain(" "),
        Segment.plain("world"),
    };
    const content = CellContent{ .segments = &segs };
    try std.testing.expectEqualStrings("hello", content.getText());
    try std.testing.expectEqual(@as(usize, 11), content.getCellWidth());
}

test "Table.addRowRich" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.addColumn("A").addColumn("B");
    const segs = [_]Segment{Segment.styled("styled", Style.empty.bold())};
    try table.addRowRich(&.{
        CellContent{ .text = "plain" },
        CellContent{ .segments = &segs },
    });

    try std.testing.expectEqual(@as(usize, 1), table.rich_rows.items.len);
}

test "Table.render with rich rows" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.addColumn("Name").addColumn("Status");
    const status_segs = [_]Segment{Segment.styled("OK", Style.empty.bold())};
    try table.addRowRich(&.{
        CellContent{ .text = "Server" },
        CellContent{ .segments = &status_segs },
    });

    const segments = try table.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len > 0);

    var found_server = false;
    var found_ok = false;
    for (segments) |seg| {
        if (std.mem.indexOf(u8, seg.text, "Server") != null) found_server = true;
        if (std.mem.indexOf(u8, seg.text, "OK") != null) found_ok = true;
    }
    try std.testing.expect(found_server);
    try std.testing.expect(found_ok);
}

test "Table.render mixed text and rich rows" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.addColumn("Col");
    try table.addRow(&.{"text row"});

    const rich_segs = [_]Segment{Segment.plain("rich row")};
    try table.addRowRich(&.{CellContent{ .segments = &rich_segs }});

    const segments = try table.render(80, allocator);
    defer allocator.free(segments);

    var found_text = false;
    var found_rich = false;
    for (segments) |seg| {
        if (std.mem.indexOf(u8, seg.text, "text row") != null) found_text = true;
        if (std.mem.indexOf(u8, seg.text, "rich row") != null) found_rich = true;
    }
    try std.testing.expect(found_text);
    try std.testing.expect(found_rich);
}

test "Cell.text creates text cell" {
    const cell = Cell.text("hello");
    try std.testing.expectEqualStrings("hello", cell.content.getText());
    try std.testing.expectEqual(@as(u8, 1), cell.colspan);
    try std.testing.expectEqual(@as(u8, 1), cell.rowspan);
}

test "Cell.withColspan sets column span" {
    const cell = Cell.text("hello").withColspan(3);
    try std.testing.expectEqual(@as(u8, 3), cell.colspan);
}

test "Cell.withRowspan sets row span" {
    const cell = Cell.text("hello").withRowspan(2);
    try std.testing.expectEqual(@as(u8, 2), cell.rowspan);
}

test "Cell.withColspan zero defaults to 1" {
    const cell = Cell.text("hello").withColspan(0);
    try std.testing.expectEqual(@as(u8, 1), cell.colspan);
}

test "Cell.withRowspan zero defaults to 1" {
    const cell = Cell.text("hello").withRowspan(0);
    try std.testing.expectEqual(@as(u8, 1), cell.rowspan);
}

test "Cell.withStyle sets cell style" {
    const cell = Cell.text("hello").withStyle(Style.empty.bold());
    try std.testing.expect(cell.style != null);
    try std.testing.expect(cell.style.?.hasAttribute(.bold));
}

test "Cell.withJustify sets cell justification" {
    const cell = Cell.text("hello").withJustify(.center);
    try std.testing.expectEqual(JustifyMethod.center, cell.justify.?);
}

test "Cell.getCellWidth returns content width" {
    const cell = Cell.text("hello");
    try std.testing.expectEqual(@as(usize, 5), cell.getCellWidth());
}

test "Table.addSpannedRow adds row with cells" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.addColumn("A").addColumn("B").addColumn("C");
    try table.addSpannedRow(&.{
        Cell.text("spanning").withColspan(2),
        Cell.text("single"),
    });

    try std.testing.expectEqual(@as(usize, 1), table.spanned_rows.items.len);
}

test "Table.addSpannedRowStyled adds row with style" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.addColumn("A");
    try table.addSpannedRowStyled(&.{Cell.text("test")}, Style.empty.bold());

    try std.testing.expectEqual(@as(usize, 1), table.spanned_rows.items.len);
    try std.testing.expect(table.spanned_row_styles.items[0] != null);
    try std.testing.expect(table.spanned_row_styles.items[0].?.hasAttribute(.bold));
}

test "Table.render with colspan cells" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.addColumn("A").addColumn("B").addColumn("C");
    try table.addSpannedRow(&.{
        Cell.text("spans two").withColspan(2),
        Cell.text("single"),
    });

    const segments = try table.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len > 0);

    var found_spanning = false;
    var found_single = false;
    for (segments) |seg| {
        if (std.mem.indexOf(u8, seg.text, "spans two") != null) found_spanning = true;
        if (std.mem.indexOf(u8, seg.text, "single") != null) found_single = true;
    }
    try std.testing.expect(found_spanning);
    try std.testing.expect(found_single);
}

test "Table.render with full row colspan" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.addColumn("A").addColumn("B").addColumn("C");
    try table.addSpannedRow(&.{
        Cell.text("spans all three").withColspan(3),
    });

    const segments = try table.render(80, allocator);
    defer allocator.free(segments);

    var found_text = false;
    for (segments) |seg| {
        if (std.mem.indexOf(u8, seg.text, "spans all three") != null) {
            found_text = true;
            break;
        }
    }
    try std.testing.expect(found_text);
}

test "Table.render with rowspan cells" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.addColumn("A").addColumn("B");
    try table.addSpannedRow(&.{
        Cell.text("spans 2 rows").withRowspan(2),
        Cell.text("row1 col2"),
    });
    try table.addSpannedRow(&.{
        Cell.text("row2 col2"),
    });

    const segments = try table.render(80, allocator);
    defer allocator.free(segments);

    var found_spanning = false;
    var found_row1 = false;
    var found_row2 = false;
    for (segments) |seg| {
        if (std.mem.indexOf(u8, seg.text, "spans 2 rows") != null) found_spanning = true;
        if (std.mem.indexOf(u8, seg.text, "row1 col2") != null) found_row1 = true;
        if (std.mem.indexOf(u8, seg.text, "row2 col2") != null) found_row2 = true;
    }
    try std.testing.expect(found_spanning);
    try std.testing.expect(found_row1);
    try std.testing.expect(found_row2);
}

test "Table.render with combined colspan and rowspan" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.addColumn("A").addColumn("B").addColumn("C");
    try table.addSpannedRow(&.{
        Cell.text("2x2").withColspan(2).withRowspan(2),
        Cell.text("r1c3"),
    });
    try table.addSpannedRow(&.{
        Cell.text("r2c3"),
    });

    const segments = try table.render(80, allocator);
    defer allocator.free(segments);

    var found_2x2 = false;
    var found_r1c3 = false;
    var found_r2c3 = false;
    for (segments) |seg| {
        if (std.mem.indexOf(u8, seg.text, "2x2") != null) found_2x2 = true;
        if (std.mem.indexOf(u8, seg.text, "r1c3") != null) found_r1c3 = true;
        if (std.mem.indexOf(u8, seg.text, "r2c3") != null) found_r2c3 = true;
    }
    try std.testing.expect(found_2x2);
    try std.testing.expect(found_r1c3);
    try std.testing.expect(found_r2c3);
}

test "Table.render spanned row with cell style override" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.addColumn("A").addColumn("B");
    try table.addSpannedRow(&.{
        Cell.text("styled").withStyle(Style.empty.bold()),
        Cell.text("normal"),
    });

    const segments = try table.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len > 0);
}

test "Table.render spanned row with cell justify override" {
    const allocator = std.testing.allocator;
    var table = Table.init(allocator);
    defer table.deinit();

    _ = table.addColumn("A").withColumn(Column.init("B").withJustify(.left));
    try table.addSpannedRow(&.{
        Cell.text("left"),
        Cell.text("centered").withJustify(.center),
    });

    const segments = try table.render(80, allocator);
    defer allocator.free(segments);

    try std.testing.expect(segments.len > 0);
}

test "Cell.segments creates segment cell" {
    const segs = [_]Segment{Segment.plain("hello")};
    const cell = Cell.segments(&segs);
    try std.testing.expectEqual(@as(usize, 5), cell.getCellWidth());
}

test "RowSpanTracker basic functionality" {
    const allocator = std.testing.allocator;
    var tracker = try RowSpanTracker.init(allocator, 3);
    defer tracker.deinit();

    try std.testing.expect(!tracker.isBlocked(0));
    try std.testing.expect(!tracker.isBlocked(1));
    try std.testing.expect(!tracker.isBlocked(2));

    const cell = Cell.text("test").withRowspan(2);
    tracker.registerSpan(0, cell);

    try std.testing.expect(tracker.isBlocked(0));
    try std.testing.expect(!tracker.isBlocked(1));

    tracker.advanceRow();
    try std.testing.expect(tracker.isBlocked(0));

    tracker.advanceRow();
    try std.testing.expect(!tracker.isBlocked(0));
}

test "RowSpanTracker with colspan" {
    const allocator = std.testing.allocator;
    var tracker = try RowSpanTracker.init(allocator, 4);
    defer tracker.deinit();

    const cell = Cell.text("test").withColspan(2).withRowspan(2);
    tracker.registerSpan(1, cell);

    try std.testing.expect(!tracker.isBlocked(0));
    try std.testing.expect(tracker.isBlocked(1));
    try std.testing.expect(tracker.isBlocked(2));
    try std.testing.expect(!tracker.isBlocked(3));
}
