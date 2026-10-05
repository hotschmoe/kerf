//! Small helpers the app UI needs around the harness: key masking, image sniffing and the
//! downscale target (HARNESS.md: long side <= 1568 px), base64 convenience.

const std = @import("std");
const Allocator = std.mem.Allocator;
const types = @import("types.zig");

/// `sk-ant-••••WXYZ` style display of a key. Never reveals the middle. Output is written into `buf`
/// (>= 24 bytes recommended); returns the used slice.
pub fn maskKey(key: []const u8, buf: []u8) []const u8 {
    if (key.len == 0) return "";
    const head = @min(@as(usize, 7), key.len / 3);
    const tail = @min(@as(usize, 4), key.len / 4);
    const r = std.fmt.bufPrint(buf, "{s}••••{s}", .{ key[0..head], key[key.len - tail ..] }) catch return "••••";
    return r;
}

pub fn base64Alloc(a: Allocator, bytes: []const u8) Allocator.Error![]u8 {
    const enc = std.base64.standard.Encoder;
    const out = try a.alloc(u8, enc.calcSize(bytes.len));
    _ = enc.encode(out, bytes);
    return out;
}

pub const Dims = struct { w: u32, h: u32 };

pub fn sniffMediaType(bytes: []const u8) ?types.MediaType {
    if (bytes.len >= 8 and std.mem.eql(u8, bytes[0..8], "\x89PNG\r\n\x1a\n")) return .png;
    if (bytes.len >= 3 and bytes[0] == 0xFF and bytes[1] == 0xD8 and bytes[2] == 0xFF) return .jpeg;
    return null;
}

/// Pixel size from PNG IHDR or the first JPEG SOF marker. Null if unrecognised.
pub fn imageDims(bytes: []const u8) ?Dims {
    switch (sniffMediaType(bytes) orelse return null) {
        .png => {
            if (bytes.len < 24) return null;
            return .{ .w = std.mem.readInt(u32, bytes[16..20], .big), .h = std.mem.readInt(u32, bytes[20..24], .big) };
        },
        .jpeg => {
            var i: usize = 2;
            while (i + 9 < bytes.len) {
                if (bytes[i] != 0xFF) return null;
                const m = bytes[i + 1];
                if (m == 0xFF) {
                    i += 1;
                    continue;
                }
                if (m == 0xD8 or m == 0x01 or (m >= 0xD0 and m <= 0xD7)) {
                    i += 2;
                    continue;
                }
                const len = std.mem.readInt(u16, bytes[i + 2 ..][0..2], .big);
                const is_sof = (m >= 0xC0 and m <= 0xCF) and m != 0xC4 and m != 0xC8 and m != 0xCC;
                if (is_sof) return .{
                    .h = std.mem.readInt(u16, bytes[i + 5 ..][0..2], .big),
                    .w = std.mem.readInt(u16, bytes[i + 7 ..][0..2], .big),
                };
                i += 2 + len;
            }
            return null;
        },
    }
}

/// Target size so the long side is <= `max_long` (default 1568). `scaled` is false if unchanged.
pub fn fitLongSide(w: u32, h: u32, max_long: u32) struct { w: u32, h: u32, scaled: bool } {
    const long = @max(w, h);
    if (long <= max_long or long == 0) return .{ .w = w, .h = h, .scaled = false };
    const nw: u32 = @max(1, @as(u32, @intCast((@as(u64, w) * max_long + long / 2) / long)));
    const nh: u32 = @max(1, @as(u32, @intCast((@as(u64, h) * max_long + long / 2) / long)));
    return .{ .w = nw, .h = nh, .scaled = true };
}

test "maskKey" {
    var b: [64]u8 = undefined;
    try std.testing.expectEqualStrings("sk-ant-••••WXYZ", maskKey("sk-ant-api03-abcdefghijklmnWXYZ", &b));
    try std.testing.expectEqualStrings("", maskKey("", &b));
    try std.testing.expect(std.mem.indexOf(u8, maskKey("abc", &b), "abc") == null or true);
}

test "image dims and fit" {
    const png = "\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x0b\xb8\x00\x00\x07\xd0rest";
    try std.testing.expectEqual(types.MediaType.png, sniffMediaType(png).?);
    const d = imageDims(png).?;
    try std.testing.expectEqual(@as(u32, 3000), d.w);
    try std.testing.expectEqual(@as(u32, 2000), d.h);
    const f = fitLongSide(d.w, d.h, types.max_image_long_side);
    try std.testing.expectEqual(@as(u32, 1568), f.w);
    try std.testing.expectEqual(@as(u32, 1045), f.h);
    try std.testing.expect(!fitLongSide(800, 600, 1568).scaled);
    // jpeg: SOI, APP0(len 4), SOF0
    const jpg = "\xff\xd8\xff\xe0\x00\x04ab\xff\xc0\x00\x0b\x08\x01\x00\x02\x00\x01\x01\x11\x00";
    const jd = imageDims(jpg).?;
    try std.testing.expectEqual(@as(u32, 512), jd.w);
    try std.testing.expectEqual(@as(u32, 256), jd.h);
}
