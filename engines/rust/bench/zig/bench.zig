const std = @import("std");
const kerf = @import("kerf");

const Case = struct { label: []const u8, func: []const u8, input: []const u8 };

pub fn main(init: std.process.Init) !void {
    const gpa = init.gpa;
    const io = init.io;
    var it = try init.minimal.args.iterateAllocator(gpa);
    defer it.deinit();
    var args: std.ArrayList([]const u8) = .empty;
    _ = it.next();
    while (it.next()) |a| try args.append(gpa, a);
    const dir = args.items[0];
    var filter: []const u8 = "";
    var exclude: ?[]const u8 = null;
    var min_ms: i96 = 300;
    var min_reps: usize = 10;
    var i: usize = 1;
    while (i + 1 < args.items.len) : (i += 2) {
        const k = args.items[i];
        const v = args.items[i + 1];
        if (std.mem.eql(u8, k, "--filter")) filter = v;
        if (std.mem.eql(u8, k, "--exclude")) exclude = v;
        if (std.mem.eql(u8, k, "--min-ms")) min_ms = try std.fmt.parseInt(i96, v, 10);
        if (std.mem.eql(u8, k, "--min-reps")) min_reps = try std.fmt.parseInt(usize, v, 10);
    }
    const cwd = std.Io.Dir.cwd();
    const mpath = try std.fmt.allocPrint(gpa, "{s}/manifest.tsv", .{dir});
    const manifest = try cwd.readFileAlloc(io, mpath, gpa, .limited(1 << 20));
    var cases: std.ArrayList(Case) = .empty;
    var lines = std.mem.tokenizeScalar(u8, manifest, '\n');
    while (lines.next()) |l| {
        var f = std.mem.splitScalar(u8, l, '\t');
        const label = f.next().?;
        const func = f.next().?;
        const file = f.next().?;
        if (std.mem.indexOf(u8, label, filter) == null) continue;
        if (exclude) |x| if (std.mem.indexOf(u8, label, x) != null) continue;
        const p = try std.fmt.allocPrint(gpa, "{s}/{s}", .{ dir, file });
        try cases.append(gpa, .{ .label = label, .func = func, .input = try cwd.readFileAlloc(io, p, gpa, .limited(64 << 20)) });
    }
    var buf: [4096]u8 = undefined;
    var w = std.Io.File.stdout().writer(io, &buf);
    const out = &w.interface;
    try out.print("label\tfn\tmedian_us\tmin_us\treps\tbytes\tstatus\n", .{});
    var times: std.ArrayList(f64) = .empty;
    for (cases.items) |c| {
        for (0..2) |_| {
            const r = try kerf.api.call(gpa, c.func, c.input);
            gpa.free(r.bytes);
        }
        times.clearRetainingCapacity();
        const start = std.Io.Timestamp.now(io, .awake);
        var bytes: usize = 0;
        var ok = true;
        while (times.items.len < min_reps or (start.durationTo(std.Io.Timestamp.now(io, .awake)).nanoseconds < min_ms * std.time.ns_per_ms and times.items.len < 5000)) {
            const t = std.Io.Timestamp.now(io, .awake);
            const r = try kerf.api.call(gpa, c.func, c.input);
            const dt = t.durationTo(std.Io.Timestamp.now(io, .awake)).nanoseconds;
            try times.append(gpa, @as(f64, @floatFromInt(dt)) / 1e3);
            bytes = r.bytes.len;
            ok = r.ok;
            gpa.free(r.bytes);
        }
        std.mem.sort(f64, times.items, {}, std.sort.asc(f64));
        try out.print("{s}\t{s}\t{d:.1}\t{d:.1}\t{d}\t{d}\t{s}\n", .{ c.label, c.func, times.items[times.items.len / 2], times.items[0], times.items.len, bytes, if (ok) "ok" else "err" });
        try out.flush();
    }
}
