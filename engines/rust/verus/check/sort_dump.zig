//! Differential oracle for kerf-verified's stable sort: for each input file, the Zig engine's own
//! `sort.stable` result. Mode 0: (u32 key, u32 index) pairs by key. Mode 1: f64 ascending by `<`.
//! Output: one line of hex per file (the sorted records' bytes).

const std = @import("std");
const sort = @import("sort");

const Pair = extern struct { key: u32, idx: u32 };

fn keyLess(_: void, x: Pair, y: Pair) bool {
    return x.key < y.key;
}

pub fn main(init: std.process.Init) !void {
    const gpa = init.gpa;
    const io = init.io;
    var it = try init.minimal.args.iterateAllocator(gpa);
    defer it.deinit();
    _ = it.next();
    var b: [1 << 16]u8 = undefined;
    var out = std.Io.File.stdout().writer(io, &b);
    const w = &out.interface;
    while (it.next()) |path| {
        const raw = try std.Io.Dir.cwd().readFileAlloc(io, path, gpa, .limited(256 << 20));
        defer gpa.free(raw);
        const body = try gpa.alignedAlloc(u8, .@"8", raw.len - 8);
        defer gpa.free(body);
        @memcpy(body, raw[8..]);
        switch (raw[0]) {
            0 => {
                const xs = std.mem.bytesAsSlice(Pair, body);
                sort.stable(Pair, @alignCast(xs), {}, keyLess);
            },
            else => {
                const xs = std.mem.bytesAsSlice(f64, body);
                sort.stable(f64, @alignCast(xs), {}, sort.asc(f64));
            },
        }
        try w.print("{x}\n", .{body});
    }
    try w.flush();
}
