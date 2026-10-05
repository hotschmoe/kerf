//! Mesh IR (`kerf_mesh`, SPEC section 11): typed model + tolerant JSON parser.
//!
//! ```jsonc
//! { "kerf_mesh": "0.1", "parts": [ { "src": "sill_plate", "part": null, "instance": 0,
//!     "material": "wood", "color": "#C9A46A",
//!     "positions": [x,y,z,...], "normals": [...], "indices": [...],
//!     "edges": [x0,y0,z0,x1,y1,z1, ...] } ] }
//! ```
//! `edges` are feature edges (sharp + profile outline) as independent segments,
//! never derived from triangles. Everything is owned by the `Mesh` arena.
//! Missing arrays become empty slices; a part without `src` is skipped.
//! Indices are validated (`index < positions.len / 3`); an invalid triangle is dropped.

const std = @import("std");
const Allocator = std.mem.Allocator;
const jv = @import("jv.zig");

pub const Part = struct {
    src: []const u8,
    part: ?[]const u8 = null,
    instance: u32 = 0,
    material: []const u8 = "",
    /// `#RRGGBB` as written (or "" if absent).
    color_hex: []const u8 = "",
    /// Parsed color, default mid grey.
    color: [3]u8 = .{ 0x9C, 0x97, 0x8C },
    /// xyz triples.
    positions: []const f32 = &.{},
    /// xyz triples (same count as positions when present).
    normals: []const f32 = &.{},
    /// Triangle list (3 indices per triangle).
    indices: []const u32 = &.{},
    /// Segment list: 6 floats (x0 y0 z0 x1 y1 z1) per edge.
    edges: []const f32 = &.{},

    pub fn vertexCount(self: Part) usize {
        return self.positions.len / 3;
    }
    pub fn triangleCount(self: Part) usize {
        return self.indices.len / 3;
    }
    pub fn edgeCount(self: Part) usize {
        return self.edges.len / 6;
    }
};

pub const Box3 = struct {
    min: [3]f32 = .{ std.math.inf(f32), std.math.inf(f32), std.math.inf(f32) },
    max: [3]f32 = .{ -std.math.inf(f32), -std.math.inf(f32), -std.math.inf(f32) },

    pub fn isEmpty(b: Box3) bool {
        return b.min[0] > b.max[0];
    }
    pub fn add(b: *Box3, x: f32, y: f32, z: f32) void {
        b.min = .{ @min(b.min[0], x), @min(b.min[1], y), @min(b.min[2], z) };
        b.max = .{ @max(b.max[0], x), @max(b.max[1], y), @max(b.max[2], z) };
    }
    pub fn center(b: Box3) [3]f32 {
        return .{ (b.min[0] + b.max[0]) * 0.5, (b.min[1] + b.max[1]) * 0.5, (b.min[2] + b.max[2]) * 0.5 };
    }
    pub fn radius(b: Box3) f32 {
        const dx = b.max[0] - b.min[0];
        const dy = b.max[1] - b.min[1];
        const dz = b.max[2] - b.min[2];
        return 0.5 * @sqrt(dx * dx + dy * dy + dz * dz);
    }
};

pub const Mesh = struct {
    arena: std.heap.ArenaAllocator,
    version: []const u8 = "",
    parts: []const Part = &.{},

    pub fn deinit(self: *Mesh) void {
        self.arena.deinit();
        self.* = undefined;
    }

    pub fn bounds(self: *const Mesh) Box3 {
        var b = Box3{};
        for (self.parts) |p| {
            var i: usize = 0;
            while (i + 2 < p.positions.len) : (i += 3) b.add(p.positions[i], p.positions[i + 1], p.positions[i + 2]);
        }
        return b;
    }

    pub fn triangleCount(self: *const Mesh) usize {
        var n: usize = 0;
        for (self.parts) |p| n += p.triangleCount();
        return n;
    }

    pub fn edgeCount(self: *const Mesh) usize {
        var n: usize = 0;
        for (self.parts) |p| n += p.edgeCount();
        return n;
    }

    pub fn findSrc(self: *const Mesh, src: []const u8) ?usize {
        for (self.parts, 0..) |p, i| if (std.mem.eql(u8, p.src, src)) return i;
        return null;
    }
};

pub const ParseError = error{ InvalidJson, NotAMesh, OutOfMemory };

/// Parse `#RRGGBB` / `#RGB` (case-insensitive).
pub fn parseHexColor(s: []const u8) ?[3]u8 {
    if (s.len == 0 or s[0] != '#') return null;
    const h = s[1..];
    if (h.len == 6) {
        const r = std.fmt.parseInt(u8, h[0..2], 16) catch return null;
        const g = std.fmt.parseInt(u8, h[2..4], 16) catch return null;
        const b = std.fmt.parseInt(u8, h[4..6], 16) catch return null;
        return .{ r, g, b };
    }
    if (h.len == 3) {
        const r = std.fmt.parseInt(u8, h[0..1], 16) catch return null;
        const g = std.fmt.parseInt(u8, h[1..2], 16) catch return null;
        const b = std.fmt.parseInt(u8, h[2..3], 16) catch return null;
        return .{ r * 17, g * 17, b * 17 };
    }
    return null;
}

fn floatsOf(a: Allocator, v: ?jv.Value) ![]const f32 {
    const arr = jv.asArray(v orelse return &.{}) orelse return &.{};
    const out = try a.alloc(f32, arr.len);
    var n: usize = 0;
    for (arr) |x| {
        out[n] = @floatCast(jv.num(x) orelse continue);
        n += 1;
    }
    return out[0..n];
}

pub fn parse(allocator: Allocator, json_bytes: []const u8) ParseError!Mesh {
    var tmp = std.heap.ArenaAllocator.init(allocator);
    defer tmp.deinit();
    const root = std.json.parseFromSliceLeaky(jv.Value, tmp.allocator(), json_bytes, .{}) catch |e| switch (e) {
        error.OutOfMemory => return error.OutOfMemory,
        else => return error.InvalidJson,
    };
    const obj = jv.asObject(root) orelse return error.NotAMesh;
    if (obj.get("kerf_mesh") == null and obj.get("parts") == null) return error.NotAMesh;

    var m: Mesh = .{ .arena = std.heap.ArenaAllocator.init(allocator) };
    errdefer m.arena.deinit();
    const a = m.arena.allocator();
    m.version = try a.dupe(u8, jv.getStr(obj, "kerf_mesh") orelse "");

    var parts: std.ArrayList(Part) = .empty;
    if (jv.getArr(obj, "parts")) |pa| {
        for (pa) |pv| {
            const po = jv.asObject(pv) orelse continue;
            const src = jv.getStr(po, "src") orelse continue;
            var p: Part = .{ .src = try a.dupe(u8, src) };
            if (jv.getStr(po, "part")) |s| p.part = try a.dupe(u8, s);
            if (jv.getNum(po, "instance")) |x| p.instance = if (x >= 0) @intFromFloat(x) else 0;
            p.material = try a.dupe(u8, jv.getStr(po, "material") orelse "");
            if (jv.getStr(po, "color")) |c| {
                p.color_hex = try a.dupe(u8, c);
                if (parseHexColor(c)) |rgb| p.color = rgb;
            }
            p.positions = try floatsOf(a, po.get("positions"));
            p.positions = p.positions[0 .. p.positions.len / 3 * 3];
            p.normals = try floatsOf(a, po.get("normals"));
            p.normals = p.normals[0 .. p.normals.len / 3 * 3];
            if (p.normals.len != p.positions.len) p.normals = &.{};
            p.edges = try floatsOf(a, po.get("edges"));
            p.edges = p.edges[0 .. p.edges.len / 6 * 6];
            if (jv.getArr(po, "indices")) |ia| {
                const nv = p.positions.len / 3;
                var idx: std.ArrayList(u32) = .empty;
                try idx.ensureTotalCapacity(a, ia.len);
                var i: usize = 0;
                while (i + 2 < ia.len) : (i += 3) {
                    const i0 = jv.num(ia[i]) orelse continue;
                    const i1 = jv.num(ia[i + 1]) orelse continue;
                    const i2 = jv.num(ia[i + 2]) orelse continue;
                    if (i0 < 0 or i1 < 0 or i2 < 0) continue;
                    const u0: usize = @intFromFloat(i0);
                    const u1: usize = @intFromFloat(i1);
                    const u2: usize = @intFromFloat(i2);
                    if (u0 >= nv or u1 >= nv or u2 >= nv) continue;
                    idx.appendSliceAssumeCapacity(&.{ @intCast(u0), @intCast(u1), @intCast(u2) });
                }
                p.indices = idx.items;
            }
            try parts.append(a, p);
        }
    }
    m.parts = parts.items;
    return m;
}

const testing = std.testing;

pub const tiny_json =
    \\{ "kerf_mesh": "0.1", "parts": [
    \\  { "src": "sill", "part": null, "instance": 0, "material": "wood", "color": "#C9A46A",
    \\    "positions": [0,0,0, 1,0,0, 1,1,0, 0,1,0, 0,0,1, 1,0,1, 1,1,1, 0,1,1],
    \\    "normals":   [0,0,-1, 0,0,-1, 0,0,-1, 0,0,-1, 0,0,1, 0,0,1, 0,0,1, 0,0,1],
    \\    "indices": [0,2,1, 0,3,2, 4,5,6, 4,6,7, 0,1,99],
    \\    "edges": [0,0,0, 1,0,0, 1,0,0, 1,1,0] },
    \\  { "src": "bolt", "part": "shank", "instance": 2, "positions": [0,0,0], "unknown": 1 },
    \\  { "no_src": true }
    \\] }
;

test "parse tiny mesh" {
    var m = try parse(testing.allocator, tiny_json);
    defer m.deinit();
    try testing.expectEqual(@as(usize, 2), m.parts.len);
    const p = m.parts[0];
    try testing.expectEqualStrings("sill", p.src);
    try testing.expectEqual(@as(usize, 8), p.vertexCount());
    try testing.expectEqual(@as(usize, 4), p.triangleCount()); // 5th (bad index) dropped
    try testing.expectEqual(@as(usize, 2), p.edgeCount());
    try testing.expectEqual(@as([3]u8, .{ 0xC9, 0xA4, 0x6A }), p.color);
    try testing.expectEqual(@as(usize, 24), p.normals.len);
    try testing.expectEqualStrings("shank", m.parts[1].part.?);
    try testing.expectEqual(@as(u32, 2), m.parts[1].instance);
    try testing.expectEqual(@as(usize, 4), m.triangleCount());
    try testing.expectEqual(@as(usize, 0), m.findSrc("sill").?);
    try testing.expect(m.findSrc("nope") == null);
    const b = m.bounds();
    try testing.expectEqual(@as(f32, 1), b.max[2]);
    try testing.expectEqual(@as(f32, 0), b.min[0]);
    try testing.expect(b.radius() > 0.8);
}

test "parse: errors and hex colors" {
    try testing.expectError(error.InvalidJson, parse(testing.allocator, "{"));
    try testing.expectError(error.NotAMesh, parse(testing.allocator, "[]"));
    try testing.expectError(error.NotAMesh, parse(testing.allocator, "{\"a\":1}"));
    try testing.expectEqual(@as([3]u8, .{ 0xFF, 0x00, 0x88 }), parseHexColor("#f08").?);
    try testing.expect(parseHexColor("red") == null);
    try testing.expect(parseHexColor("#12345") == null);
    var m = try parse(testing.allocator, "{\"kerf_mesh\":\"0.1\"}");
    defer m.deinit();
    try testing.expectEqual(@as(usize, 0), m.parts.len);
    try testing.expect(m.bounds().isEmpty());
}
