//! Oracle for kerf-verified's PNG checks: for each PNG, writes `<path>.raw` holding its IDAT zlib
//! stream inflated by Zig's std.compress.flate (the filtered scanlines the encoder compressed).

const std = @import("std");
const flate = std.compress.flate;

pub fn main(init: std.process.Init) !void {
    const gpa = init.gpa;
    const io = init.io;
    var it = try init.minimal.args.iterateAllocator(gpa);
    defer it.deinit();
    _ = it.next();
    while (it.next()) |path| {
        const bytes = try std.Io.Dir.cwd().readFileAlloc(io, path, gpa, .limited(256 << 20));
        defer gpa.free(bytes);
        var idat: std.ArrayList(u8) = .empty;
        defer idat.deinit(gpa);
        var pos: usize = 8;
        while (pos + 12 <= bytes.len) {
            const len = std.mem.readInt(u32, bytes[pos..][0..4], .big);
            if (std.mem.eql(u8, bytes[pos + 4 ..][0..4], "IDAT")) try idat.appendSlice(gpa, bytes[pos + 8 ..][0..len]);
            pos += 12 + len;
        }
        var in: std.Io.Reader = .fixed(idat.items);
        var win: [flate.max_window_len]u8 = undefined;
        var dec = flate.Decompress.init(&in, .zlib, &win);
        const raw = try dec.reader.allocRemaining(gpa, .unlimited);
        defer gpa.free(raw);
        const out = try std.fmt.allocPrint(gpa, "{s}.raw", .{path});
        defer gpa.free(out);
        try std.Io.Dir.cwd().writeFile(io, .{ .sub_path = out, .data = raw });
    }
}
