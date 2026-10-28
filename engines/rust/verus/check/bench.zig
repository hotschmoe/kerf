//! Zig side of `kvcheck bench`: times the engine's own sort.stable and json.parse and std's
//! CRC-32 / Adler-32 on the same input files. Module `kz` is a file next to the engine's sort.zig
//! and json.zig holding `pub const sort = @import("sort.zig"); pub const json = @import("json.zig");`. Prints the best of `rounds` runs, ns per iteration.

const std = @import("std");
const kz = @import("kz");
const sort = kz.sort;
const json = kz.json;

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
    const kind = it.next().?;
    const path = it.next().?;
    const iters = try std.fmt.parseInt(usize, it.next().?, 10);
    const data = try std.Io.Dir.cwd().readFileAlloc(io, path, gpa, .limited(512 << 20));
    defer gpa.free(data);
    const work = try gpa.alignedAlloc(u8, .@"8", data.len);
    defer gpa.free(work);
    var best: i96 = std.math.maxInt(i96);
    var sink: u64 = 0;
    for (0..5) |_| {
        const t0 = std.Io.Timestamp.now(io, .awake).nanoseconds;
        for (0..iters) |_| {
            if (std.mem.eql(u8, kind, "sortpairs")) {
                @memcpy(work, data);
                const xs = std.mem.bytesAsSlice(Pair, work);
                sort.stable(Pair, @alignCast(xs), {}, keyLess);
                sink +%= xs[0].idx;
            } else if (std.mem.eql(u8, kind, "sortf64")) {
                @memcpy(work, data);
                const xs = std.mem.bytesAsSlice(f64, work);
                sort.stable(f64, @alignCast(xs), {}, sort.asc(f64));
                sink +%= @as(u64, @bitCast(xs[0]));
            } else if (std.mem.eql(u8, kind, "json")) {
                var arena = std.heap.ArenaAllocator.init(gpa);
                defer arena.deinit();
                var err: json.ParseError = undefined;
                const v = try json.parse(arena.allocator(), data, &err);
                sink +%= @intFromBool(v != null);
            } else if (std.mem.eql(u8, kind, "crc")) {
                sink +%= std.hash.Crc32.hash(data);
            } else if (std.mem.eql(u8, kind, "adler")) {
                sink +%= std.hash.Adler32.hash(data);
            }
        }
        const dt = std.Io.Timestamp.now(io, .awake).nanoseconds - t0;
        best = @min(best, dt);
    }
    var b: [256]u8 = undefined;
    var out = std.Io.File.stdout().writer(io, &b);
    try out.interface.print("{d} {d}\n", .{ @divTrunc(best, @as(i96, @intCast(iters))), sink % 7 });
    try out.interface.flush();
}
