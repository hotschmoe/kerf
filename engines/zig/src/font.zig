//! Stroke font (Hershey Simplex derivative, SPEC 15) with metrics and glyph rendering.

const std = @import("std");
const json = @import("json.zig");
const geom = @import("geom.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;

pub const embedded = @embedFile("kerf_font_json");

pub const Glyph = struct {
    adv: f64,
    strokes: []const []const V2,
};

pub const Font = struct {
    cap: f64,
    /// Printable ASCII 32..126.
    ascii: [95]Glyph,
    /// Synthetic glyphs for a few typographic characters used in titles and notes.
    emdash: Glyph,
    endash: Glyph,
    degree: Glyph,
    times: Glyph,
    plusminus: Glyph,

    pub fn parse(a: Allocator, src: []const u8) !Font {
        var err: json.ParseError = undefined;
        const root = (try json.parse(a, src, &err)) orelse return error.BadFont;
        const cap = (root.get("cap_height") orelse return error.BadFont).num() orelse return error.BadFont;
        const gl = root.get("glyphs") orelse return error.BadFont;
        var f: Font = undefined;
        f.cap = cap;
        var question: ?Glyph = null;
        for (&f.ascii, 0..) |*slot, i| {
            const ch = [1]u8{@intCast(32 + i)};
            const gv = gl.get(&ch) orelse {
                slot.* = .{ .adv = 0, .strokes = &.{} };
                continue;
            };
            slot.* = try parseGlyph(a, gv);
            if (ch[0] == '?') question = slot.*;
        }
        // Fill missing glyphs with '?'.
        for (&f.ascii, 0..) |*slot, i| {
            if (slot.adv == 0 and i != 0) slot.* = question orelse slot.*;
        }
        f.emdash = .{ .adv = 24, .strokes = try strokes1(a, &.{ .{ 0, 8 }, .{ 24, 8 } }) };
        f.endash = .{ .adv = 20, .strokes = try strokes1(a, &.{ .{ 0, 8 }, .{ 18, 8 } }) };
        f.times = f.ascii['X' - 32];
        f.plusminus = .{ .adv = 20, .strokes = try strokes2(a, &.{
            &.{ .{ 10, 17 }, .{ 10, 5 } },
            &.{ .{ 4, 11 }, .{ 16, 11 } },
            &.{ .{ 4, 0 }, .{ 16, 0 } },
        }) };
        // degree: a small octagon at the top.
        const deg = try a.alloc(V2, 9);
        for (deg, 0..) |*p, i| {
            const ang = @as(f64, @floatFromInt(i)) * std.math.pi / 4.0;
            p.* = V2.init(5 + 3.2 * @cos(ang), 17.5 + 3.2 * @sin(ang));
        }
        const dstrokes = try a.alloc([]const V2, 1);
        dstrokes[0] = deg;
        f.degree = .{ .adv = 12, .strokes = dstrokes };
        return f;
    }

    fn strokes1(a: Allocator, pts: []const [2]f64) ![]const []const V2 {
        const out = try a.alloc([]const V2, 1);
        const p = try a.alloc(V2, pts.len);
        for (pts, 0..) |q, i| p[i] = V2.init(q[0], q[1]);
        out[0] = p;
        return out;
    }
    fn strokes2(a: Allocator, groups: []const []const [2]f64) ![]const []const V2 {
        const out = try a.alloc([]const V2, groups.len);
        for (groups, 0..) |g, k| {
            const p = try a.alloc(V2, g.len);
            for (g, 0..) |q, i| p[i] = V2.init(q[0], q[1]);
            out[k] = p;
        }
        return out;
    }

    fn parseGlyph(a: Allocator, v: json.Value) !Glyph {
        const adv = (v.get("adv") orelse return error.BadFont).num() orelse return error.BadFont;
        const st = (v.get("strokes") orelse return error.BadFont).arr() orelse return error.BadFont;
        const out = try a.alloc([]const V2, st.len);
        for (st, 0..) |s, i| {
            const pts = s.arr() orelse return error.BadFont;
            const pp = try a.alloc(V2, pts.len);
            for (pts, 0..) |p, k| {
                const xy = p.arr() orelse return error.BadFont;
                if (xy.len < 2) return error.BadFont;
                pp[k] = V2.init(xy[0].num() orelse 0, xy[1].num() orelse 0);
            }
            out[i] = pp;
        }
        return .{ .adv = adv, .strokes = out };
    }

    pub fn glyph(self: *const Font, cp: u21) Glyph {
        return switch (cp) {
            32...126 => self.ascii[cp - 32],
            0x2014 => self.emdash,
            0x2013 => self.endash,
            0xB0 => self.degree,
            0xD7 => self.times,
            0xB1 => self.plusminus,
            0x201C, 0x201D => self.ascii['"' - 32],
            0x2018, 0x2019 => self.ascii['\'' - 32],
            0xA0 => self.ascii[0],
            else => self.ascii['?' - 32],
        };
    }

    /// Mean advance of A-Z in font units; the reference character width for note wrapping.
    pub fn meanCapAdvance(self: *const Font) f64 {
        var s: f64 = 0;
        var c: u8 = 'A';
        while (c <= 'Z') : (c += 1) s += self.ascii[c - 32].adv;
        return s / 26.0;
    }

    /// Advance width of `text` at cap height `h`.
    pub fn width(self: *const Font, text: []const u8, h: f64) f64 {
        return self.widthUnits(text) * h / self.cap;
    }

    pub fn widthUnits(self: *const Font, text: []const u8) f64 {
        var w: f64 = 0;
        var it = utf8Iter(text);
        while (it.next()) |cp| w += self.glyph(cp).adv;
        return w;
    }

    /// Append the stroke polylines of `text` (baseline-left at the origin, y up) scaled to cap height `h`.
    pub fn strokesOf(self: *const Font, a: Allocator, out: *std.ArrayList([]V2), text: []const u8, h: f64) Allocator.Error!void {
        const k = h / self.cap;
        var x: f64 = 0;
        var it = utf8Iter(text);
        while (it.next()) |cp| {
            const g = self.glyph(cp);
            for (g.strokes) |s| {
                const pts = try a.alloc(V2, s.len);
                for (s, 0..) |p, i| pts[i] = V2.init((x + p.x) * k, p.y * k);
                try out.append(a, pts);
            }
            x += g.adv;
        }
    }
};

pub const Utf8Iter = struct {
    s: []const u8,
    i: usize = 0,

    pub fn next(self: *Utf8Iter) ?u21 {
        if (self.i >= self.s.len) return null;
        const c = self.s[self.i];
        const n = std.unicode.utf8ByteSequenceLength(c) catch {
            self.i += 1;
            return '?';
        };
        if (self.i + n > self.s.len) {
            self.i = self.s.len;
            return '?';
        }
        const cp = std.unicode.utf8Decode(self.s[self.i .. self.i + n]) catch {
            self.i += 1;
            return '?';
        };
        self.i += n;
        return cp;
    }
};

pub fn utf8Iter(s: []const u8) Utf8Iter {
    return .{ .s = s };
}

/// ASCII upper-casing (non-ASCII bytes are untouched).
pub fn upperAscii(a: Allocator, s: []const u8) Allocator.Error![]u8 {
    const out = try a.alloc(u8, s.len);
    for (s, 0..) |c, i| out[i] = std.ascii.toUpper(c);
    return out;
}

test "font parses and measures" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const f = try Font.parse(arena.allocator(), embedded);
    try std.testing.expectEqual(@as(f64, 21), f.cap);
    // "0" advance is 20 units, so at h = 21 the width is 20.
    try std.testing.expectApproxEqAbs(20.0, f.width("0", 21), 1e-9);
    try std.testing.expectApproxEqAbs(40.0, f.width("00", 21), 1e-9);
    try std.testing.expect(f.meanCapAdvance() > 19 and f.meanCapAdvance() < 20);
    // non-ASCII falls back or maps
    try std.testing.expectEqual(f.glyph(0x2014).adv, 24);
    try std.testing.expectEqual(f.glyph(0x4e2d).adv, f.ascii['?' - 32].adv);
}
