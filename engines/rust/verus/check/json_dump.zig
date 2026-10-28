//! Differential oracle for kerf-verified's JSON parser: one line per input file, the Zig engine's
//! own `json.parse` result in a canonical form (numbers as f64 bit patterns, strings as hex).

const std = @import("std");
const json = @import("json");

fn dump(w: *std.Io.Writer, v: json.Value) !void {
    switch (v) {
        .null => try w.writeAll("n"),
        .bool => |b| try w.writeAll(if (b) "t" else "f"),
        .number => |x| try w.print("d{x:0>16}", .{@as(u64, @bitCast(x))}),
        .string => |s| try w.print("s{x}", .{s}),
        .array => |xs| {
            try w.writeAll("[");
            for (xs) |x| {
                try dump(w, x);
                try w.writeAll(",");
            }
            try w.writeAll("]");
        },
        .object => |ms| {
            try w.writeAll("{");
            for (ms) |m| {
                try w.print("s{x}:", .{m.key});
                try dump(w, m.value);
                try w.writeAll(",");
            }
            try w.writeAll("}");
        },
    }
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
        const src = try std.Io.Dir.cwd().readFileAlloc(io, path, gpa, .limited(64 << 20));
        defer gpa.free(src);
        var arena = std.heap.ArenaAllocator.init(gpa);
        defer arena.deinit();
        var err: json.ParseError = undefined;
        if (try json.parse(arena.allocator(), src, &err)) |v| {
            try w.writeAll("OK ");
            try dump(w, v);
        } else try w.print("ERR {d}:{d} {s}", .{ err.line, err.col, err.msg });
        try w.writeAll("\n");
    }
    try w.flush();
}
