//! Single-line text editor state: a fixed-capacity UTF-8 buffer with a
//! cursor and an optional selection anchor. Pure; lives in the Model.
//!
//! Compared with `teak.TextField`: UTF-8 aware cursor movement, word jumps,
//! Home/End/Delete, bounded clipboard paste, and a visible-window helper for
//! fields wider than their box.

const std = @import("std");

pub const CAP = 4096;

pub const Editor = struct {
    buf: [CAP]u8 = undefined,
    len: usize = 0,
    cursor: usize = 0,
    anchor: ?usize = null,

    pub fn content(self: *const Editor) []const u8 {
        return self.buf[0..self.len];
    }

    pub fn set(self: *Editor, s: []const u8) void {
        const n = @min(s.len, CAP);
        @memcpy(self.buf[0..n], s[0..n]);
        self.len = n;
        self.cursor = n;
        self.anchor = null;
    }

    pub fn clear(self: *Editor) void {
        self.len = 0;
        self.cursor = 0;
        self.anchor = null;
    }

    pub fn hasSelection(self: *const Editor) bool {
        return if (self.anchor) |a| a != self.cursor else false;
    }

    pub fn selection(self: *const Editor) []const u8 {
        const a = self.anchor orelse return "";
        const lo = @min(a, self.cursor);
        const hi = @max(a, self.cursor);
        return self.buf[lo..hi];
    }

    fn deleteSelection(self: *Editor) void {
        const a = self.anchor orelse return;
        self.anchor = null;
        const lo = @min(a, self.cursor);
        const hi = @max(a, self.cursor);
        if (lo == hi) return;
        std.mem.copyForwards(u8, self.buf[lo .. self.len - (hi - lo)], self.buf[hi..self.len]);
        self.len -= hi - lo;
        self.cursor = lo;
    }

    /// Insert raw bytes at the cursor (replacing the selection). Truncates to
    /// capacity on a code point boundary; control characters other than
    /// newline-as-space are dropped.
    pub fn insert(self: *Editor, bytes: []const u8) void {
        self.deleteSelection();
        var i: usize = 0;
        while (i < bytes.len) {
            const n = seqLen(bytes, i);
            const cp = bytes[i..@min(i + n, bytes.len)];
            i += n;
            var one: [1]u8 = undefined;
            var piece = cp;
            if (cp.len == 1 and (cp[0] < 0x20 or cp[0] == 0x7f)) {
                if (cp[0] == '\n' or cp[0] == '\r' or cp[0] == '\t') {
                    one[0] = ' ';
                    piece = &one;
                } else continue;
            }
            if (self.len + piece.len > CAP) break;
            std.mem.copyBackwards(u8, self.buf[self.cursor + piece.len .. self.len + piece.len], self.buf[self.cursor..self.len]);
            @memcpy(self.buf[self.cursor .. self.cursor + piece.len], piece);
            self.len += piece.len;
            self.cursor += piece.len;
        }
    }

    pub fn backspace(self: *Editor) void {
        if (self.anchor != null and self.hasSelection()) return self.deleteSelection();
        self.anchor = null;
        if (self.cursor == 0) return;
        const prev = self.prevBoundary(self.cursor);
        std.mem.copyForwards(u8, self.buf[prev .. self.len - (self.cursor - prev)], self.buf[self.cursor..self.len]);
        self.len -= self.cursor - prev;
        self.cursor = prev;
    }

    pub fn delete(self: *Editor) void {
        if (self.anchor != null and self.hasSelection()) return self.deleteSelection();
        self.anchor = null;
        if (self.cursor >= self.len) return;
        const next = self.nextBoundary(self.cursor);
        std.mem.copyForwards(u8, self.buf[self.cursor .. self.len - (next - self.cursor)], self.buf[next..self.len]);
        self.len -= next - self.cursor;
    }

    pub const Move = enum { left, right, home, end, word_left, word_right };

    pub fn move(self: *Editor, m: Move, extend: bool) void {
        if (extend) {
            if (self.anchor == null) self.anchor = self.cursor;
        } else if (self.hasSelection()) {
            // Collapse toward the direction of travel, like every text box.
            const a = self.anchor.?;
            switch (m) {
                .left, .word_left, .home => self.cursor = @min(a, self.cursor),
                .right, .word_right, .end => self.cursor = @max(a, self.cursor),
            }
            self.anchor = null;
            if (m == .left or m == .right) return;
        } else self.anchor = null;
        self.cursor = switch (m) {
            .left => self.prevBoundary(self.cursor),
            .right => self.nextBoundary(self.cursor),
            .home => 0,
            .end => self.len,
            .word_left => self.wordLeft(self.cursor),
            .word_right => self.wordRight(self.cursor),
        };
    }

    pub fn selectAll(self: *Editor) void {
        self.anchor = 0;
        self.cursor = self.len;
    }

    pub fn deselect(self: *Editor) void {
        self.anchor = null;
    }

    fn prevBoundary(self: *const Editor, i: usize) usize {
        if (i == 0) return 0;
        var j = i - 1;
        while (j > 0 and (self.buf[j] & 0xC0) == 0x80) j -= 1;
        return j;
    }

    fn nextBoundary(self: *const Editor, i: usize) usize {
        if (i >= self.len) return self.len;
        return @min(i + seqLen(self.buf[0..self.len], i), self.len);
    }

    fn wordLeft(self: *const Editor, i: usize) usize {
        var j = i;
        while (j > 0 and self.buf[j - 1] == ' ') j -= 1;
        while (j > 0 and self.buf[j - 1] != ' ') j -= 1;
        return j;
    }

    fn wordRight(self: *const Editor, i: usize) usize {
        var j = i;
        while (j < self.len and self.buf[j] != ' ') j += 1;
        while (j < self.len and self.buf[j] == ' ') j += 1;
        return j;
    }

    /// Bytes [start, end) of the part of the content to display in a box
    /// that fits `max_cols` monospace columns, keeping the cursor visible
    /// (scrolls by whole code points). Returns the window and the cursor
    /// offset within it.
    pub const Window = struct { start: usize, end: usize, cursor: usize, anchor: ?usize };

    pub fn window(self: *const Editor, max_cols: usize) Window {
        const total = columns(self.buf[0..self.len]);
        if (total <= max_cols or max_cols == 0) return .{ .start = 0, .end = self.len, .cursor = self.cursor, .anchor = self.anchor };
        const cur_col = columns(self.buf[0..self.cursor]);
        // Keep the cursor within the last column of the box.
        const first_col = if (cur_col + 1 > max_cols) cur_col + 1 - max_cols else 0;
        const start = byteOfColumn(self.buf[0..self.len], first_col);
        const end = byteOfColumn(self.buf[0..self.len], first_col + max_cols);
        return .{
            .start = start,
            .end = end,
            .cursor = self.cursor - start,
            .anchor = if (self.anchor) |a| (if (a < start) 0 else if (a > end) end - start else a - start) else null,
        };
    }
};

fn seqLen(s: []const u8, i: usize) usize {
    const b = s[i];
    const n: usize = if (b < 0x80) 1 else if (b >= 0xF0) 4 else if (b >= 0xE0) 3 else if (b >= 0xC0) 2 else 1;
    return @min(n, s.len - i);
}

pub fn columns(s: []const u8) usize {
    var n: usize = 0;
    var i: usize = 0;
    while (i < s.len) : (n += 1) i += seqLen(s, i);
    return n;
}

fn byteOfColumn(s: []const u8, col: usize) usize {
    var i: usize = 0;
    var n: usize = 0;
    while (i < s.len and n < col) : (n += 1) i += seqLen(s, i);
    return i;
}

test "insert, backspace, delete, utf8" {
    var e: Editor = .{};
    e.insert("ab");
    e.insert("\u{00e9}"); // 2 bytes
    try std.testing.expectEqualStrings("ab\u{00e9}", e.content());
    e.backspace();
    try std.testing.expectEqualStrings("ab", e.content());
    e.move(.home, false);
    e.delete();
    try std.testing.expectEqualStrings("b", e.content());
}

test "selection replace and word jumps" {
    var e: Editor = .{};
    e.insert("hello big world");
    e.move(.word_left, true);
    try std.testing.expectEqualStrings("world", e.selection());
    e.insert("kerf");
    try std.testing.expectEqualStrings("hello big kerf", e.content());
    e.move(.home, false);
    e.move(.word_right, false);
    try std.testing.expectEqual(@as(usize, 6), e.cursor);
}

test "control characters are dropped, newlines become spaces" {
    var e: Editor = .{};
    e.insert("a\nb\x01c");
    try std.testing.expectEqualStrings("a bc", e.content());
}

test "window keeps the cursor visible" {
    var e: Editor = .{};
    e.insert("0123456789");
    const w = e.window(4);
    try std.testing.expectEqualStrings("789", e.content()[w.start..w.end]);
    try std.testing.expectEqual(@as(usize, 3), w.cursor);
}
