//! The one sort of the engine (REVIEW SIZ-1). `std.mem.sort` is a block sort instantiated once per
//! (element type, comparator): 14+ copies of 2-10 KB in the wasm. Here the algorithm exists once, over raw
//! bytes with the comparator behind a function pointer; `stable` only adds a small adapter per call site.
//!
//! Algorithm: binary insertion sort on runs of 20, then bottom-up merging. A merge whose shorter run fits the
//! 4 KB stack buffer is a plain linear buffered merge; a longer one is split SymMerge style (Kim & Kutzner,
//! as Go's `sort.Stable`) with a rotation until the parts fit. Stable, so for any strict weak ordering the
//! result is exactly the one `std.mem.sort` gives (outputs stay byte-identical). O(n log n) comparisons,
//! no allocation, recursion depth O(log n).

const std = @import("std");

/// Largest element type `stable` accepts (one element must fit the merge buffer many times over).
const max_elem_bytes = 256;
const buf_bytes = 4096;
/// Runs sorted by insertion before merging starts.
const block = 20;

/// Sorts `items` in place, keeping equal elements in input order. Same signature as `std.mem.sort`.
pub fn stable(
    comptime T: type,
    items: []T,
    context: anytype,
    comptime lessThan: fn (@TypeOf(context), lhs: T, rhs: T) bool,
) void {
    if (@sizeOf(T) > max_elem_bytes) @compileError("sort.stable: element type too large; sort indices instead");
    if (@alignOf(T) > 16) @compileError("sort.stable: element alignment above 16");
    if (items.len < 2 or @sizeOf(T) == 0) return;
    const Adapter = struct {
        c: @TypeOf(context),
        pad: u8 = 0, // never zero-sized: `&ad` must be a real address
        fn lt(p: *const anyopaque, x: [*]const u8, y: [*]const u8) bool {
            const self: *const @This() = @ptrCast(@alignCast(p));
            const a: *const T = @ptrCast(@alignCast(x));
            const b: *const T = @ptrCast(@alignCast(y));
            return lessThan(self.c, a.*, b.*);
        }
    };
    const ad: Adapter = .{ .c = context };
    sortBytes(@ptrCast(items.ptr), items.len, @sizeOf(T), &ad, Adapter.lt);
}

/// `lessThan` for numbers in ascending order (`std.sort.asc`).
pub fn asc(comptime T: type) fn (void, T, T) bool {
    return struct {
        fn lt(_: void, a: T, b: T) bool {
            return a < b;
        }
    }.lt;
}

const Less = *const fn (ctx: *const anyopaque, a: [*]const u8, b: [*]const u8) bool;

/// The only instantiation of the algorithm.
fn sortBytes(base: [*]u8, n: usize, size: usize, ctx: *const anyopaque, less: Less) void {
    var buf: [buf_bytes]u8 align(16) = undefined;
    const r: Run = .{ .base = base, .size = size, .ctx = ctx, .less = less, .buf = &buf, .cap = buf_bytes / size };
    var a: usize = 0;
    while (a < n) : (a += block) r.insertion(a, @min(a + block, n));
    var width: usize = block;
    while (width < n) : (width *= 2) {
        a = 0;
        while (a + width < n) : (a += 2 * width) r.merge(a, a + width, @min(a + 2 * width, n));
    }
}

const Run = struct {
    base: [*]u8,
    size: usize,
    ctx: *const anyopaque,
    less: Less,
    buf: [*]align(16) u8,
    /// Elements that fit in `buf`.
    cap: usize,

    inline fn at(r: Run, i: usize) [*]u8 {
        return r.base + i * r.size;
    }

    inline fn lt(r: Run, i: usize, j: usize) bool {
        return r.ltp(r.at(i), r.at(j));
    }

    inline fn ltp(r: Run, x: [*]const u8, y: [*]const u8) bool {
        return r.less(r.ctx, x, y);
    }

    /// Copies one element (never overlapping).
    inline fn put(r: Run, dst: [*]u8, src: [*]const u8) void {
        switch (r.size) {
            4 => @as(*align(1) u32, @ptrCast(dst)).* = @as(*align(1) const u32, @ptrCast(src)).*,
            8 => @as(*align(1) u64, @ptrCast(dst)).* = @as(*align(1) const u64, @ptrCast(src)).*,
            else => @memcpy(dst[0..r.size], src[0..r.size]),
        }
    }

    /// Copies `count` elements between non-overlapping ranges: short ones by element (a `memory.copy` costs a
    /// runtime call in V8), long ones by `@memcpy`.
    inline fn copy(r: Run, dst: [*]u8, src: [*]const u8, count: usize) void {
        if (count * r.size <= 128) {
            for (0..count) |k| r.put(dst + k * r.size, src + k * r.size);
        } else @memcpy(dst[0 .. count * r.size], src[0 .. count * r.size]);
    }

    /// Element i goes right after the last element of [a, i) that is not greater than it.
    fn insertion(r: Run, a: usize, b: usize) void {
        var i = a + 1;
        while (i < b) : (i += 1) {
            if (!r.lt(i, i - 1)) continue;
            var lo = a;
            var hi = i - 1;
            while (lo < hi) {
                const h = lo + (hi - lo) / 2;
                if (r.lt(i, h)) hi = h else lo = h + 1;
            }
            r.put(r.buf, r.at(i));
            var j = i;
            while (j > lo) : (j -= 1) r.put(r.at(j), r.at(j - 1));
            r.put(r.at(lo), r.buf);
        }
    }

    /// Blocks [a, m) [m, b) -> [m, b) [a, m). The shorter block goes through the buffer; while both are longer than
    /// the buffer, block swaps (Gries-Mills) shrink the problem.
    fn rotate(r: Run, a0: usize, m0: usize, b0: usize) void {
        const sz = r.size;
        var a = a0;
        var m = m0;
        var b = b0;
        while (a < m and m < b) {
            const nl = m - a;
            const nr = b - m;
            if (nl <= r.cap) {
                r.copy(r.buf, r.at(a), nl);
                @memmove(r.at(a)[0 .. nr * sz], r.at(m)[0 .. nr * sz]);
                r.copy(r.at(a + nr), r.buf, nl);
                return;
            }
            if (nr <= r.cap) {
                r.copy(r.buf, r.at(m), nr);
                @memmove(r.at(a + nr)[0 .. nl * sz], r.at(a)[0 .. nl * sz]);
                r.copy(r.at(a), r.buf, nr);
                return;
            }
            if (nl <= nr) {
                // u v1 v2 (|v1| = |u|) -> v1 u v2: v1 is final, rotate u v2
                r.swapBlocks(a, m, nl);
                a = m;
                m += nl;
            } else {
                // u1 u2 v (|u2| = |v|) -> u1 v u2: u2 is final, rotate u1 v
                r.swapBlocks(m - nr, m, nr);
                b = m;
                m -= nr;
            }
        }
    }

    /// Swaps the non-overlapping blocks [x, x + count) and [y, y + count), buffer-sized chunks at a time.
    fn swapBlocks(r: Run, x: usize, y: usize, count: usize) void {
        var done: usize = 0;
        while (done < count) {
            const c = @min(r.cap, count - done);
            r.copy(r.buf, r.at(x + done), c);
            r.copy(r.at(x + done), r.at(y + done), c);
            r.copy(r.at(y + done), r.buf, c);
            done += c;
        }
    }

    /// Merges the sorted runs [a, m) and [m, b) in place; ties keep the left element first.
    fn merge(r: Run, a: usize, m: usize, b: usize) void {
        if (!r.lt(m, m - 1)) return; // already in order
        if (m - a <= r.cap) return r.mergeLow(a, m, b);
        if (b - m <= r.cap) return r.mergeHigh(a, m, b);
        // SymMerge split: find the cut that lets [start, m) and [m, end) swap places, then merge both halves.
        const mid = a + (b - a) / 2;
        const n = mid + m;
        var start: usize = undefined;
        var hi: usize = undefined;
        if (m > mid) {
            start = n - b;
            hi = mid;
        } else {
            start = a;
            hi = m;
        }
        const p = n - 1;
        while (start < hi) {
            const c = start + (hi - start) / 2;
            if (!r.lt(p - c, c)) start = c + 1 else hi = c;
        }
        const end = n - start;
        if (start < m and m < end) r.rotate(start, m, end);
        if (a < start and start < mid) r.merge(a, start, mid);
        if (mid < end and end < b) r.merge(mid, end, b);
    }

    /// Left run into the buffer, merge front to back.
    fn mergeLow(r: Run, a0: usize, m: usize, b: usize) void {
        const sz = r.size;
        // left elements not greater than the first right one are already in place
        var a = a0;
        var hi = m - 1;
        while (a < hi) {
            const h = a + (hi - a) / 2;
            if (r.lt(m, h)) hi = h else a = h + 1;
        }
        const nl = m - a;
        r.copy(r.buf, r.at(a), nl);
        var i: usize = 0;
        var j = m;
        var k = a;
        while (i < nl and j < b) : (k += 1) {
            if (r.ltp(r.at(j), r.buf + i * sz)) {
                r.put(r.at(k), r.at(j));
                j += 1;
            } else {
                r.put(r.at(k), r.buf + i * sz);
                i += 1;
            }
        }
        r.copy(r.at(k), r.buf + i * sz, nl - i);
    }

    /// Right run into the buffer, merge back to front.
    fn mergeHigh(r: Run, a: usize, m: usize, b0: usize) void {
        const sz = r.size;
        // right elements not less than the last left one are already in place
        var lo = m + 1;
        var b = b0;
        while (lo < b) {
            const h = lo + (b - lo) / 2;
            if (r.lt(h, m - 1)) lo = h + 1 else b = h;
        }
        const nr = b - m;
        r.copy(r.buf, r.at(m), nr);
        var i = m;
        var j = nr;
        var k = b;
        while (i > a and j > 0) {
            k -= 1;
            if (r.ltp(r.buf + (j - 1) * sz, r.at(i - 1))) {
                i -= 1;
                r.put(r.at(k), r.at(i));
            } else {
                j -= 1;
                r.put(r.at(k), r.buf + j * sz);
            }
        }
        r.copy(r.at(a), r.buf, j);
    }
};

const testing = std.testing;

const Pair = struct { key: u32, idx: u32 };

fn keyLess(_: void, x: Pair, y: Pair) bool {
    return x.key < y.key;
}

const Wide = struct { key: u16, idx: u32, pad: [40]u8 = @splat(0) };

fn wideLess(_: void, x: Wide, y: Wide) bool {
    return x.key < y.key;
}

fn checkAgainstStd(comptime T: type, x: []T, lessThan: fn (void, T, T) bool) !void {
    const y = try testing.allocator.dupe(T, x);
    defer testing.allocator.free(y);
    stable(T, x, {}, lessThan);
    std.mem.sort(T, y, {}, lessThan);
    for (x, y) |p, q| {
        try testing.expectEqual(q.key, p.key);
        try testing.expectEqual(q.idx, p.idx);
    }
}

test "stable equals std.mem.sort (stable) for every length up to 300 and large ones, ties, presorted, reversed, wide elements" {
    var prng = std.Random.DefaultPrng.init(42);
    const rnd = prng.random();
    const a = testing.allocator;
    const lens = [_]usize{ 1000, 1500, 4096, 20000, 70001 };
    var n: usize = 0;
    while (n <= 300 + lens.len) : (n += 1) {
        const len = if (n <= 300) n else lens[n - 301];
        // random keys over small and large ranges, then ascending, descending and sawtooth inputs
        for ([_]u32{ 1, 3, 50, 1 << 30, 0, 1, 2 }, 0..) |range, mode| {
            const x = try a.alloc(Pair, len);
            defer a.free(x);
            for (x, 0..) |*e, i| {
                const k: u32 = switch (mode) {
                    0...3 => rnd.uintLessThan(u32, range),
                    4 => @intCast(i / 3),
                    5 => @intCast(len - i),
                    else => @intCast(i % 97),
                };
                e.* = .{ .key = k, .idx = @intCast(i) };
            }
            try checkAgainstStd(Pair, x, keyLess);
        }
        if (len > 5000) continue;
        const w = try a.alloc(Wide, len);
        defer a.free(w);
        for (w, 0..) |*e, i| e.* = .{ .key = rnd.uintLessThan(u16, 40), .idx = @intCast(i) };
        try checkAgainstStd(Wide, w, wideLess);
    }
}

test "asc u32 / f64 equal std.sort.asc" {
    var prng = std.Random.DefaultPrng.init(7);
    const rnd = prng.random();
    for ([_]usize{ 0, 1, 2, 19, 20, 21, 600, 5000 }) |len| {
        const u = try testing.allocator.alloc(u32, len);
        defer testing.allocator.free(u);
        const f = try testing.allocator.alloc(f64, len);
        defer testing.allocator.free(f);
        for (u, f) |*x, *y| {
            x.* = rnd.uintLessThan(u32, 300);
            y.* = @as(f64, @floatFromInt(rnd.uintLessThan(u32, 300))) - 150;
        }
        const u_ref = try testing.allocator.dupe(u32, u);
        defer testing.allocator.free(u_ref);
        const f_ref = try testing.allocator.dupe(f64, f);
        defer testing.allocator.free(f_ref);
        stable(u32, u, {}, asc(u32));
        stable(f64, f, {}, asc(f64));
        std.mem.sort(u32, u_ref, {}, std.sort.asc(u32));
        std.mem.sort(f64, f_ref, {}, std.sort.asc(f64));
        try testing.expectEqualSlices(u32, u_ref, u);
        try testing.expectEqualSlices(f64, f_ref, f);
    }
}

test "f64 asc, context by value" {
    var v: [100]f64 = undefined;
    for (&v, 0..) |*e, i| e.* = @floatFromInt(100 - i);
    stable(f64, &v, {}, asc(f64));
    for (v, 0..) |e, i| try testing.expectEqual(@as(f64, @floatFromInt(i + 1)), e);
    stable(f64, &v, {}, asc(f64));
    for (v, 0..) |e, i| try testing.expectEqual(@as(f64, @floatFromInt(i + 1)), e);
    const keys = [_]i32{ 5, -1, 3, 3, 0 };
    var idx = [_]usize{ 0, 1, 2, 3, 4 };
    stable(usize, &idx, @as([]const i32, &keys), struct {
        fn lt(k: []const i32, x: usize, y: usize) bool {
            return k[x] < k[y];
        }
    }.lt);
    try testing.expectEqualSlices(usize, &.{ 1, 4, 2, 3, 0 }, &idx);
}
