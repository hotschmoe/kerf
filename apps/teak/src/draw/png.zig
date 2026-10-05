//! Minimal PNG encoder: 8-bit gray / RGB / RGBA, adaptive scanline filters
//! (None/Sub/Up/Avg/Paeth chosen per row by the minimum-sum-of-absolute-values
//! heuristic) and real DEFLATE compression through `std.compress.flate`
//! (zlib container, level 9, so the Adler-32 is written by std; CRC-32 per chunk
//! by `std.hash.Crc32`). No stored-only blocks: a 1400x1000 mostly-white drawing is a few tens of KB.
//!
//! ```zig
//! const bytes = try png.encode(gpa, w, h, .rgb, rgb_pixels);   // caller frees
//! ```
//! Pure std, allocator-passed, no I/O: builds for wasm32-freestanding.
//! (`decode` is test-only and lives under `test` below.)

const std = @import("std");
const Allocator = std.mem.Allocator;
const flate = std.compress.flate;
const Writer = std.Io.Writer;

pub const ColorType = enum {
    gray,
    rgb,
    rgba,

    pub fn channels(self: ColorType) usize {
        return switch (self) {
            .gray => 1,
            .rgb => 3,
            .rgba => 4,
        };
    }
    fn code(self: ColorType) u8 {
        return switch (self) {
            .gray => 0,
            .rgb => 2,
            .rgba => 6,
        };
    }
};

pub const EncodeError = error{ InvalidDimensions, BadBufferSize, OutOfMemory, CompressFailed };

const signature = [8]u8{ 0x89, 'P', 'N', 'G', 0x0D, 0x0A, 0x1A, 0x0A };

fn paeth(a: u8, b: u8, c: u8) u8 {
    const p: i32 = @as(i32, a) + @as(i32, b) - @as(i32, c);
    const pa = @abs(p - @as(i32, a));
    const pb = @abs(p - @as(i32, b));
    const pc = @abs(p - @as(i32, c));
    if (pa <= pb and pa <= pc) return a;
    if (pb <= pc) return b;
    return c;
}

fn filterRow(kind: u8, row: []const u8, prev: []const u8, bpp: usize, out: []u8) void {
    for (row, 0..) |x, i| {
        const a: u8 = if (i >= bpp) row[i - bpp] else 0;
        const b: u8 = prev[i];
        const c: u8 = if (i >= bpp) prev[i - bpp] else 0;
        out[i] = switch (kind) {
            0 => x,
            1 => x -% a,
            2 => x -% b,
            3 => x -% @as(u8, @intCast((@as(u16, a) + @as(u16, b)) / 2)),
            else => x -% paeth(a, b, c),
        };
    }
}

fn rowScore(buf: []const u8) u64 {
    var s: u64 = 0;
    for (buf) |v| {
        const sv: i8 = @bitCast(v);
        s += @abs(@as(i16, sv));
    }
    return s;
}

fn writeChunk(w: *Writer, tag: *const [4]u8, data: []const u8) Writer.Error!void {
    var len: [4]u8 = undefined;
    std.mem.writeInt(u32, &len, @intCast(data.len), .big);
    try w.writeAll(&len);
    try w.writeAll(tag);
    try w.writeAll(data);
    var crc = std.hash.Crc32.init();
    crc.update(tag);
    crc.update(data);
    var cb: [4]u8 = undefined;
    std.mem.writeInt(u32, &cb, crc.final(), .big);
    try w.writeAll(&cb);
}

/// Encode an image. `pixels.len` must equal `width * height * channels`. The caller owns the result.
pub fn encode(a: Allocator, width: u32, height: u32, color: ColorType, pixels: []const u8) EncodeError![]u8 {
    if (width == 0 or height == 0 or width > 1 << 24 or height > 1 << 24) return error.InvalidDimensions;
    const ch = color.channels();
    const stride = @as(usize, width) * ch;
    if (pixels.len != stride * height) return error.BadBufferSize;

    // 1. filter
    const filtered = try a.alloc(u8, (stride + 1) * height);
    defer a.free(filtered);
    const zero_row = try a.alloc(u8, stride);
    defer a.free(zero_row);
    @memset(zero_row, 0);
    const cand = try a.alloc(u8, stride);
    defer a.free(cand);
    var y: usize = 0;
    while (y < height) : (y += 1) {
        const row = pixels[y * stride ..][0..stride];
        const prev = if (y == 0) zero_row else pixels[(y - 1) * stride ..][0..stride];
        var best_kind: u8 = 0;
        var best_score: u64 = std.math.maxInt(u64);
        var kind: u8 = 0;
        while (kind < 5) : (kind += 1) {
            filterRow(kind, row, prev, ch, cand);
            const sc = rowScore(cand);
            // prefer Up/None slightly on ties (cheap to decode); strict < keeps the earliest
            if (sc < best_score) {
                best_score = sc;
                best_kind = kind;
            }
        }
        const dst = filtered[y * (stride + 1) ..][0 .. stride + 1];
        dst[0] = best_kind;
        filterRow(best_kind, row, prev, ch, dst[1..]);
    }

    // 2. deflate (zlib container)
    var z = Writer.Allocating.initCapacity(a, 4096) catch return error.OutOfMemory;
    defer z.deinit();
    const window = try a.alloc(u8, flate.max_window_len);
    defer a.free(window);
    const comp = try a.create(flate.Compress);
    defer a.destroy(comp);
    comp.* = flate.Compress.init(&z.writer, window, .zlib, .best) catch return error.CompressFailed;
    comp.writer.writeAll(filtered) catch return error.CompressFailed;
    comp.finish() catch return error.CompressFailed;
    const zbytes = z.written();

    // 3. container
    var out = Writer.Allocating.initCapacity(a, zbytes.len + 128) catch return error.OutOfMemory;
    errdefer out.deinit();
    out.writer.writeAll(&signature) catch return error.OutOfMemory;
    var ihdr: [13]u8 = undefined;
    std.mem.writeInt(u32, ihdr[0..4], width, .big);
    std.mem.writeInt(u32, ihdr[4..8], height, .big);
    ihdr[8] = 8; // bit depth
    ihdr[9] = color.code();
    ihdr[10] = 0; // deflate
    ihdr[11] = 0; // filter method 0
    ihdr[12] = 0; // no interlace
    writeChunk(&out.writer, "IHDR", &ihdr) catch return error.OutOfMemory;
    // split IDAT into 64 KiB chunks (any size is legal; this keeps streaming decoders happy)
    var off: usize = 0;
    while (off < zbytes.len) {
        const n = @min(zbytes.len - off, 65536);
        writeChunk(&out.writer, "IDAT", zbytes[off .. off + n]) catch return error.OutOfMemory;
        off += n;
    }
    writeChunk(&out.writer, "IEND", &.{}) catch return error.OutOfMemory;
    return out.toOwnedSlice() catch error.OutOfMemory;
}

// ---------------------------------------------------------------------------
// Tests (includes a small decoder to verify round trips without external tools)
// ---------------------------------------------------------------------------

const testing = std.testing;

const Decoded = struct {
    width: u32,
    height: u32,
    color: ColorType,
    pixels: []u8,
    idat_blocks: usize,
};

fn decode(a: Allocator, bytes: []const u8) !Decoded {
    try testing.expect(bytes.len > 8 and std.mem.eql(u8, bytes[0..8], &signature));
    var pos: usize = 8;
    var w: u32 = 0;
    var h: u32 = 0;
    var color: ColorType = .rgb;
    var idat: std.ArrayList(u8) = .empty;
    defer idat.deinit(a);
    var nidat: usize = 0;
    var saw_end = false;
    while (pos + 12 <= bytes.len) {
        const len = std.mem.readInt(u32, bytes[pos..][0..4], .big);
        const tag = bytes[pos + 4 ..][0..4];
        const data = bytes[pos + 8 ..][0..len];
        const crc_stored = std.mem.readInt(u32, bytes[pos + 8 + len ..][0..4], .big);
        var crc = std.hash.Crc32.init();
        crc.update(tag);
        crc.update(data);
        try testing.expectEqual(crc_stored, crc.final());
        if (std.mem.eql(u8, tag, "IHDR")) {
            w = std.mem.readInt(u32, data[0..4], .big);
            h = std.mem.readInt(u32, data[4..8], .big);
            try testing.expectEqual(@as(u8, 8), data[8]);
            color = switch (data[9]) {
                0 => .gray,
                2 => .rgb,
                6 => .rgba,
                else => return error.UnsupportedColor,
            };
        } else if (std.mem.eql(u8, tag, "IDAT")) {
            try idat.appendSlice(a, data);
            nidat += 1;
        } else if (std.mem.eql(u8, tag, "IEND")) {
            saw_end = true;
        }
        pos += 12 + len;
    }
    try testing.expect(saw_end);
    try testing.expectEqual(bytes.len, pos);

    var in: std.Io.Reader = .fixed(idat.items);
    var dwin: [flate.max_window_len]u8 = undefined;
    var dec = flate.Decompress.init(&in, .zlib, &dwin);
    const raw = try dec.reader.allocRemaining(a, .unlimited);
    defer a.free(raw);

    const ch = color.channels();
    const stride = @as(usize, w) * ch;
    try testing.expectEqual((stride + 1) * h, raw.len);
    const px = try a.alloc(u8, stride * h);
    var y: usize = 0;
    while (y < h) : (y += 1) {
        const kind = raw[y * (stride + 1)];
        const src = raw[y * (stride + 1) + 1 ..][0..stride];
        const dst = px[y * stride ..][0..stride];
        for (src, 0..) |x, i| {
            const av: u8 = if (i >= ch) dst[i - ch] else 0;
            const bv: u8 = if (y > 0) px[(y - 1) * stride + i] else 0;
            const cv: u8 = if (i >= ch and y > 0) px[(y - 1) * stride + i - ch] else 0;
            dst[i] = switch (kind) {
                0 => x,
                1 => x +% av,
                2 => x +% bv,
                3 => x +% @as(u8, @intCast((@as(u16, av) + @as(u16, bv)) / 2)),
                4 => x +% paeth(av, bv, cv),
                else => return error.BadFilter,
            };
        }
    }
    return .{ .width = w, .height = h, .color = color, .pixels = px, .idat_blocks = nidat };
}

test "round trip rgb gradient + noise" {
    const a = testing.allocator;
    const w = 37;
    const h = 23;
    var px: [w * h * 3]u8 = undefined;
    var prng = std.Random.DefaultPrng.init(42);
    for (&px, 0..) |*v, i| v.* = @truncate((i / 3) * 5 + prng.random().int(u8) % 7);
    const bytes = try encode(a, w, h, .rgb, &px);
    defer a.free(bytes);
    const d = try decode(a, bytes);
    defer a.free(d.pixels);
    try testing.expectEqual(@as(u32, w), d.width);
    try testing.expectEqual(@as(u32, h), d.height);
    try testing.expectEqualSlices(u8, &px, d.pixels);
}

test "round trip gray and rgba" {
    const a = testing.allocator;
    var g: [16 * 9]u8 = undefined;
    for (&g, 0..) |*v, i| v.* = @intCast((i * 7) & 0xff);
    const bg = try encode(a, 16, 9, .gray, &g);
    defer a.free(bg);
    const dg = try decode(a, bg);
    defer a.free(dg.pixels);
    try testing.expectEqual(ColorType.gray, dg.color);
    try testing.expectEqualSlices(u8, &g, dg.pixels);

    var c: [5 * 4 * 4]u8 = undefined;
    for (&c, 0..) |*v, i| v.* = @intCast((i * 13 + 5) & 0xff);
    const bc = try encode(a, 5, 4, .rgba, &c);
    defer a.free(bc);
    const dc = try decode(a, bc);
    defer a.free(dc.pixels);
    try testing.expectEqualSlices(u8, &c, dc.pixels);
}

test "mostly white 1400x1000 compresses to a few KB (not stored)" {
    const a = testing.allocator;
    const w = 1400;
    const h = 1000;
    const px = try a.alloc(u8, w * h * 3);
    defer a.free(px);
    @memset(px, 255);
    // some black lines
    var y: usize = 100;
    while (y < 900) : (y += 50) {
        var x: usize = 100;
        while (x < 1300) : (x += 1) {
            const o = (y * w + x) * 3;
            px[o] = 0;
            px[o + 1] = 0;
            px[o + 2] = 0;
        }
    }
    const bytes = try encode(a, w, h, .rgb, px);
    defer a.free(bytes);
    try testing.expect(bytes.len < 30_000);
    const d = try decode(a, bytes);
    defer a.free(d.pixels);
    try testing.expectEqualSlices(u8, px, d.pixels);
}

test "invalid input" {
    try testing.expectError(error.InvalidDimensions, encode(testing.allocator, 0, 5, .rgb, &.{}));
    try testing.expectError(error.BadBufferSize, encode(testing.allocator, 2, 2, .rgb, &[_]u8{ 1, 2, 3 }));
}

test "1x1 image" {
    const a = testing.allocator;
    const bytes = try encode(a, 1, 1, .rgb, &[_]u8{ 1, 2, 3 });
    defer a.free(bytes);
    const d = try decode(a, bytes);
    defer a.free(d.pixels);
    try testing.expectEqualSlices(u8, &[_]u8{ 1, 2, 3 }, d.pixels);
}
