//! Mesh IR (SPEC §11) -> teak MeshData for the 3D tab: muted flat colors,
//! ink feature edges, a ground grid, selection tint.

const std = @import("std");
const Allocator = std.mem.Allocator;
const teak = @import("teak");
const draw = @import("../draw/mod.zig");
const th = @import("theme.zig");

pub const Built = struct {
    arena: std.heap.ArenaAllocator,
    data: teak.MeshData,

    pub fn deinit(self: *Built) void {
        self.arena.deinit();
    }
};

fn muted(rgb: [3]u8) [3]f32 {
    // Desaturate toward grey by 25% (DESIGN §4: "muted, slightly desaturated").
    const r: f32 = @as(f32, @floatFromInt(rgb[0])) / 255.0;
    const g: f32 = @as(f32, @floatFromInt(rgb[1])) / 255.0;
    const b: f32 = @as(f32, @floatFromInt(rgb[2])) / 255.0;
    const l = 0.3 * r + 0.59 * g + 0.11 * b;
    const k: f32 = 0.75;
    return .{ l + (r - l) * k, l + (g - l) * k, l + (b - l) * k };
}

pub fn build(gpa: Allocator, mesh: *const draw.Mesh, selected: ?[]const u8) !Built {
    var arena = std.heap.ArenaAllocator.init(gpa);
    errdefer arena.deinit();
    const a = arena.allocator();

    var nv: usize = 0;
    var ni: usize = 0;
    var ne: usize = 0;
    for (mesh.parts) |p| {
        nv += p.vertexCount();
        ni += p.indices.len;
        ne += p.edgeCount();
    }
    const grid_lines = 2 * 2 * 41; // up to 41 lines per axis, 2 verts each, 2 axes
    const verts = try a.alloc(teak.MeshVertex, nv);
    const idx = try a.alloc(u32, ni);
    const lines = try a.alloc(teak.LineVertex, ne * 2 + grid_lines);

    var vi: usize = 0;
    var ii: usize = 0;
    var li: usize = 0;
    for (mesh.parts) |p| {
        const base: u32 = @intCast(vi);
        const is_sel = if (selected) |s| draw.ir.srcMatches(p.src, s) else false;
        var c = muted(p.color);
        if (is_sel) {
            c = .{ c[0] * 0.55 + th.blue[0] * 0.45, c[1] * 0.55 + th.blue[1] * 0.45, c[2] * 0.55 + th.blue[2] * 0.45 };
        }
        const n = p.vertexCount();
        for (0..n) |k| {
            verts[vi + k] = .{
                .pos = .{ p.positions[3 * k], p.positions[3 * k + 1], p.positions[3 * k + 2] },
                .normal = if (p.normals.len >= 3 * (k + 1)) .{ p.normals[3 * k], p.normals[3 * k + 1], p.normals[3 * k + 2] } else .{ 0, 1, 0 },
                .color = .{ c[0], c[1], c[2], 1 },
            };
        }
        vi += n;
        for (p.indices) |ix| {
            idx[ii] = base + ix;
            ii += 1;
        }
        const ec: [4]f32 = if (is_sel) th.blue else th.ink;
        var e: usize = 0;
        while (e + 5 < p.edges.len) : (e += 6) {
            lines[li] = .{ .pos = .{ p.edges[e], p.edges[e + 1], p.edges[e + 2] }, .color = ec };
            lines[li + 1] = .{ .pos = .{ p.edges[e + 3], p.edges[e + 4], p.edges[e + 5] }, .color = ec };
            li += 2;
        }
    }

    // Ground grid under the model: 12" spacing, extent = bounds + margin.
    const b = mesh.bounds();
    if (!b.isEmpty()) {
        const step: f32 = 12;
        const x0 = @floor((b.min[0] - 24) / step) * step;
        const x1 = @ceil((b.max[0] + 24) / step) * step;
        const z0 = @floor((b.min[2] - 24) / step) * step;
        const z1 = @ceil((b.max[2] + 24) / step) * step;
        const y = b.min[1] - 0.01;
        var x = x0;
        var count: usize = 0;
        while (x <= x1 and count < 41) : ({
            x += step;
            count += 1;
        }) {
            lines[li] = .{ .pos = .{ x, y, z0 }, .color = th.grid };
            lines[li + 1] = .{ .pos = .{ x, y, z1 }, .color = th.grid };
            li += 2;
        }
        var z = z0;
        count = 0;
        while (z <= z1 and count < 41) : ({
            z += step;
            count += 1;
        }) {
            lines[li] = .{ .pos = .{ x0, y, z }, .color = th.grid };
            lines[li + 1] = .{ .pos = .{ x1, y, z }, .color = th.grid };
            li += 2;
        }
    }
    return .{ .arena = arena, .data = .{ .vertices = verts[0..vi], .indices = idx[0..ii], .lines = lines[0..li] } };
}
