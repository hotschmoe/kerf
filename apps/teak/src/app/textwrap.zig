//! Monospace word wrapping for the console and inspector. Teak text cmds are
//! single-line, and Plex Mono has a fixed advance, so wrapping is column
//! arithmetic done in `view` into the frame arena.

const std = @import("std");

/// Split `text` into lines of at most `cols` code points. Honors '\n'.
/// Words longer than a line are broken. Returned slices borrow `text`.
pub fn wrap(a: std.mem.Allocator, text: []const u8, cols: usize, max_lines: usize) []const []const u8 {
    var lines: std.ArrayList([]const u8) = .empty;
    const width = @max(cols, 4);
    var para = std.mem.splitScalar(u8, text, '\n');
    while (para.next()) |p| {
        if (p.len == 0) {
            lines.append(a, "") catch break;
            continue;
        }
        var start: usize = 0; // start of current line (bytes)
        var last_space: ?usize = null; // byte index of the last space seen on this line
        var col: usize = 0;
        var i: usize = 0;
        while (i < p.len) {
            const n = cpLen(p, i);
            if (p[i] == ' ') last_space = i;
            if (col >= width) {
                // Break before i: at the last space if there is one on this line, else hard break.
                if (last_space) |sp| if (sp > start) {
                    lines.append(a, std.mem.trimEnd(u8, p[start..sp], " ")) catch return lines.items;
                    start = sp + 1;
                    col = if (start <= i) cols_of(p[start..i]) else 0;
                    last_space = null;
                    if (start > i) i = start; // the space at `i` was the break
                    continue;
                };
                lines.append(a, p[start..i]) catch return lines.items;
                start = i;
                col = 0;
                last_space = null;
            }
            col += 1;
            i += n;
        }
        lines.append(a, std.mem.trimEnd(u8, p[start..], " ")) catch break;
        if (lines.items.len >= max_lines) break;
    }
    if (lines.items.len > max_lines) lines.shrinkRetainingCapacity(max_lines);
    return lines.items;
}

fn cpLen(s: []const u8, i: usize) usize {
    const b = s[i];
    const n: usize = if (b < 0x80) 1 else if (b >= 0xF0) 4 else if (b >= 0xE0) 3 else if (b >= 0xC0) 2 else 1;
    return @min(n, s.len - i);
}

fn cols_of(s: []const u8) usize {
    var n: usize = 0;
    var i: usize = 0;
    while (i < s.len) : (n += 1) i += cpLen(s, i);
    return n;
}

test "wrap" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const l = wrap(a, "alpha beta gamma delta", 11, 99);
    try std.testing.expectEqual(@as(usize, 2), l.len);
    try std.testing.expectEqualStrings("alpha beta", l[0]);
    try std.testing.expectEqualStrings("gamma delta", l[1]);
    const l2 = wrap(a, "abcdefghijklmnop", 5, 99);
    try std.testing.expectEqual(@as(usize, 4), l2.len);
    try std.testing.expectEqualStrings("abcde", l2[0]);
    const l4 = wrap(a, "abcde fghij", 5, 99);
    try std.testing.expectEqual(@as(usize, 2), l4.len);
    const l3 = wrap(a, "a\n\nb", 10, 99);
    try std.testing.expectEqual(@as(usize, 3), l3.len);
}
