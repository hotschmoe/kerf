//! Text helpers of the annotation layout (SPEC 16): glyph folding to the stroke font's ASCII set, character-count wrapping, text boxes and the text/path items every annotation emits.

const std = @import("std");
const geom = @import("../geom.zig");
const pen_mod = @import("../pen.zig");
const font_mod = @import("../font.zig");
const drawing = @import("../drawing.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Pt = geom.Pt;
const Box = geom.Box;
const Pen = pen_mod.Pen;
const Item = drawing.Item;
const annot = @import("../annot.zig");
const Env = annot.Env;

/// Glyph folding (SPEC 16): dashes -> '-', smart quotes -> straight, x-sign -> X, vulgar fractions ->
/// " n/d", anything else non-ASCII -> '?'. `unknown` is set when a '?' substitution happened.
pub fn asciiFoldFlag(a: Allocator, s: []const u8, unknown: ?*bool) Allocator.Error![]const u8 {
    var plain = true;
    for (s) |c| if (c >= 0x80) {
        plain = false;
        break;
    };
    if (plain) return s;
    var out: std.ArrayList(u8) = .empty;
    var it = font_mod.utf8Iter(s);
    while (it.next()) |cp| {
        switch (cp) {
            0x2010...0x2015, 0x2212 => try out.append(a, '-'),
            0xD7 => try out.append(a, 'X'),
            0x201C, 0x201D, 0x201E, 0x2033 => try out.append(a, '"'),
            0x2018, 0x2019, 0x201A, 0x2032 => try out.append(a, '\''),
            0xA0, 0x2009, 0x200A, 0x202F, 0x2002, 0x2003 => try out.append(a, ' '),
            0xBD => try out.appendSlice(a, " 1/2"),
            0xBC => try out.appendSlice(a, " 1/4"),
            0xBE => try out.appendSlice(a, " 3/4"),
            0x215B => try out.appendSlice(a, " 1/8"),
            0x215C => try out.appendSlice(a, " 3/8"),
            0x215D => try out.appendSlice(a, " 5/8"),
            0x215E => try out.appendSlice(a, " 7/8"),
            0...127 => try out.append(a, @intCast(cp)),
            else => {
                try out.append(a, '?');
                if (unknown) |u| u.* = true;
            },
        }
    }
    return out.items;
}

pub fn asciiFold(a: Allocator, s: []const u8) Allocator.Error![]const u8 {
    return asciiFoldFlag(a, s, null);
}

pub fn layerName(env: *const Env, key: pen_mod.LayerKey) []const u8 {
    return env.style.layerName(key);
}

pub fn textItem(env: *Env, layer_key: pen_mod.LayerKey, pen: Pen, src: []const u8, s: []const u8, x: f64, y: f64, h: f64, rot: f64, al: drawing.Align, va: drawing.VAlign) Allocator.Error!Item {
    return .{ .text = .{
        .layer = layerName(env, layer_key),
        .pen = pen,
        .src = src,
        .s = try asciiFoldFlag(env.a, s, &env.unknown_glyph),
        .x = x,
        .y = y,
        .h = h,
        .rot = rot,
        .align_ = al,
        .valign = va,
    } };
}

pub fn pathItem(env: *Env, pen: Pen, src: []const u8, pts: []const V2, closed: bool) Allocator.Error!Item {
    const p = try env.a.alloc(Pt, pts.len);
    for (pts, 0..) |q, i| p[i] = Pt.at(q, 0);
    return .{ .path = .{ .layer = env.style.layerForPen(pen), .pen = pen, .src = src, .closed = closed, .pts = p } };
}

pub fn upperIf(env: *const Env, s: []const u8) Allocator.Error![]const u8 {
    if (env.style.text_case_upper) return font_mod.upperAscii(env.a, s);
    return s;
}

/// Text box polygon (rotated) with padding.
pub fn textPoly(env_font: *const font_mod.Font, t: drawing.TextItem, pad: f64) [4]V2 {
    const w = env_font.width(t.s, t.h);
    const ox: f64 = switch (t.align_) {
        .center => -w * 0.5,
        .right => -w,
        .left => 0,
    };
    const oy: f64 = switch (t.valign) {
        .middle => -t.h * 0.5,
        .top => -t.h,
        .baseline => 0,
    };
    const ang = std.math.degreesToRadians(t.rot);
    const c = @cos(ang);
    const s = @sin(ang);
    const pts = [4][2]f64{ .{ ox - pad, oy - pad }, .{ ox + w + pad, oy - pad }, .{ ox + w + pad, oy + t.h + pad }, .{ ox - pad, oy + t.h + pad } };
    var out: [4]V2 = undefined;
    for (pts, 0..) |q, i| out[i] = V2.init(t.x + q[0] * c - q[1] * s, t.y + q[0] * s + q[1] * c);
    return out;
}

pub fn itemsBox(font: *const font_mod.Font, items: []const Item) Box {
    var b = Box{};
    for (items) |it| switch (it) {
        .path => |p| b.addBox(geom.pointsBox(p.pts)),
        .fill => |f| for (f.loops) |l| b.addBox(geom.pointsBox(l)),
        .hatch => |h| for (h.loops) |l| b.addBox(geom.pointsBox(l)),
        .text => |t| for (textPoly(font, t, 0)) |p| b.addPoint(p.x, p.y),
        .region => {},
    };
    return b;
}

pub fn cpCount(s: []const u8) usize {
    var n: usize = 0;
    var it = font_mod.utf8Iter(s);
    while (it.next()) |_| n += 1;
    return n;
}

/// Greedy word wrap at `chars` characters; words longer than a line are split.
pub fn wrap(a: Allocator, text: []const u8, chars_in: usize) Allocator.Error![]const []const u8 {
    const chars = @max(chars_in, 1); // 0 would never consume a word (REVIEW LAY-2)
    var lines: std.ArrayList([]const u8) = .empty;
    var cur: std.ArrayList(u8) = .empty;
    var it = std.mem.tokenizeAny(u8, text, " \t\r\n");
    while (it.next()) |word| {
        var w = word;
        while (true) {
            const wl = cpCount(w);
            const cl = cpCount(cur.items);
            if (cur.items.len == 0) {
                if (wl > chars) {
                    // split at `chars` code points
                    var n: usize = 0;
                    var idx: usize = 0;
                    var u = font_mod.utf8Iter(w);
                    while (n < chars) : (n += 1) {
                        _ = u.next() orelse break;
                        idx = u.i;
                    }
                    try lines.append(a, w[0..idx]);
                    w = w[idx..];
                    continue;
                }
                try cur.appendSlice(a, w);
                break;
            } else if (cl + 1 + wl <= chars) {
                try cur.append(a, ' ');
                try cur.appendSlice(a, w);
                break;
            } else {
                try lines.append(a, cur.items);
                cur = .empty;
                continue;
            }
        }
    }
    if (cur.items.len > 0) try lines.append(a, cur.items);
    if (lines.items.len == 0) try lines.append(a, "");
    return lines.items;
}

test "wrap by characters" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const l = try wrap(a, "2X8 PT SILL PLATE W/ 5/8\" DIA. ANCHOR BOLTS @ 48\" O.C.", 28);
    for (l) |s| try std.testing.expect(s.len <= 28);
    const w = try wrap(a, "ABCDEFGHIJ", 4);
    try std.testing.expectEqual(@as(usize, 3), w.len);
    try std.testing.expectEqualStrings("EFGH", w[1]);
}
