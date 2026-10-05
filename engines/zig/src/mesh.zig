//! 3D mesh (SPEC 11): each prism extruded over [z0, z1] with flat-shaded faces and explicit
//! feature edges (sharp edges + profile outlines).

const std = @import("std");
const json = @import("json.zig");
const geom = @import("geom.zig");
const model = @import("model.zig");
const scene_mod = @import("scene.zig");
const style_mod = @import("style.zig");
const compile_mod = @import("compile.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Pt = geom.Pt;

pub const Part = struct {
    src: []const u8,
    part: ?[]const u8,
    instance: u32,
    material: []const u8,
    color: []const u8,
    positions: std.ArrayList(f32) = .empty,
    normals: std.ArrayList(f32) = .empty,
    indices: std.ArrayList(u32) = .empty,
    edges: std.ArrayList(f32) = .empty,
};

const Builder = struct {
    a: Allocator,
    p: *Part,

    fn vert(self: *Builder, x: f64, y: f64, z: f64, nx: f64, ny: f64, nz: f64) Allocator.Error!u32 {
        const idx: u32 = @intCast(self.p.positions.items.len / 3);
        try self.p.positions.appendSlice(self.a, &.{ @floatCast(x), @floatCast(y), @floatCast(z) });
        try self.p.normals.appendSlice(self.a, &.{ @floatCast(nx), @floatCast(ny), @floatCast(nz) });
        return idx;
    }
    fn tri(self: *Builder, a: u32, b: u32, c: u32) Allocator.Error!void {
        try self.p.indices.appendSlice(self.a, &.{ a, b, c });
    }
    fn edge(self: *Builder, a: [3]f64, b: [3]f64) Allocator.Error!void {
        try self.p.edges.appendSlice(self.a, &.{ @floatCast(a[0]), @floatCast(a[1]), @floatCast(a[2]), @floatCast(b[0]), @floatCast(b[1]), @floatCast(b[2]) });
    }
};

fn arcStepFor(r: f64) f64 {
    return if (r < 1.0) std.math.pi / 6.0 else std.math.pi * 5.0 / 180.0;
}

/// Flatten a bulge loop for meshing (circles get 12 segments).
fn flattenForMesh(a: Allocator, loop: []const Pt) Allocator.Error![]V2 {
    var out: std.ArrayList(V2) = .empty;
    for (loop, 0..) |p, i| {
        const q = loop[(i + 1) % loop.len];
        try out.append(a, p.v());
        if (p.b != 0) {
            const arc = geom.arcOf(p.v(), q.v(), p.b);
            const step = arcStepFor(arc.r);
            const n = @max(2, @as(usize, @intFromFloat(@ceil(@abs(arc.sweep) / step))));
            var k: usize = 1;
            while (k < n) : (k += 1) try out.append(a, arc.at(@as(f64, @floatFromInt(k)) / @as(f64, @floatFromInt(n))));
        }
    }
    return out.items;
}

/// Ear-clipping triangulation of a simple polygon (any orientation). Returns index triples.
pub fn earClip(a: Allocator, pts_in: []const V2) Allocator.Error![]const u32 {
    const n = pts_in.len;
    var out: std.ArrayList(u32) = .empty;
    if (n < 3) return out.items;
    const idx = try a.alloc(u32, n);
    for (idx, 0..) |*x, i| x.* = @intCast(i);
    const ccw = geom.signedAreaV(pts_in) >= 0;
    var remaining: usize = n;
    var guard: usize = 0;
    var i: usize = 0;
    while (remaining > 3 and guard < n * n + 10) : (guard += 1) {
        const ia = idx[(i + remaining - 1) % remaining];
        const ib = idx[i % remaining];
        const ic = idx[(i + 1) % remaining];
        const pa = pts_in[ia];
        const pb = pts_in[ib];
        const pc = pts_in[ic];
        const cr = pb.sub(pa).cross(pc.sub(pb));
        var ear = if (ccw) cr > 1e-12 else cr < -1e-12;
        if (ear) {
            for (0..remaining) |k| {
                const iv = idx[k];
                if (iv == ia or iv == ib or iv == ic) continue;
                const p = pts_in[iv];
                if (pointInTri(p, pa, pb, pc)) {
                    ear = false;
                    break;
                }
            }
        }
        if (ear) {
            try out.appendSlice(a, &.{ ia, ib, ic });
            // remove ib
            var k = i % remaining;
            while (k + 1 < remaining) : (k += 1) idx[k] = idx[k + 1];
            remaining -= 1;
            if (i >= remaining) i = 0;
        } else {
            i = (i + 1) % remaining;
        }
    }
    if (remaining == 3) try out.appendSlice(a, &.{ idx[0], idx[1], idx[2] });
    return out.items;
}

fn pointInTri(p: V2, a: V2, b: V2, c: V2) bool {
    const d1 = b.sub(a).cross(p.sub(a));
    const d2 = c.sub(b).cross(p.sub(b));
    const d3 = a.sub(c).cross(p.sub(c));
    const neg = d1 < -1e-12 or d2 < -1e-12 or d3 < -1e-12;
    const pos = d1 > 1e-12 or d2 > 1e-12 or d3 > 1e-12;
    return !(neg and pos);
}

fn turnAngle(prev: V2, cur: V2, next: V2) f64 {
    const u = cur.sub(prev);
    const w = next.sub(cur);
    return @abs(std.math.atan2(u.cross(w), u.dot(w)));
}

/// Extrude a flattened loop over [z0, z1].
fn extrude(b: *Builder, loop_in: []const V2, z0: f64, z1: f64, smooth_deg: f64) Allocator.Error!void {
    var loop = loop_in;
    // remove duplicate consecutive points
    var clean: std.ArrayList(V2) = .empty;
    for (loop) |p| {
        if (clean.items.len > 0 and V2.eql(clean.items[clean.items.len - 1], p, 1e-9)) continue;
        try clean.append(b.a, p);
    }
    if (clean.items.len > 1 and V2.eql(clean.items[0], clean.items[clean.items.len - 1], 1e-9)) _ = clean.pop();
    loop = clean.items;
    const n = loop.len;
    if (n < 3) return;
    const ccw = geom.signedAreaV(loop) >= 0;
    // caps
    const tris = try earClip(b.a, loop);
    {
        const base_top = b.p.positions.items.len / 3;
        for (loop) |p| _ = try b.vert(p.x, p.y, z1, 0, 0, 1);
        const base_bot = b.p.positions.items.len / 3;
        for (loop) |p| _ = try b.vert(p.x, p.y, z0, 0, 0, -1);
        var k: usize = 0;
        while (k + 2 < tris.len) : (k += 3) {
            const t0: u32 = @intCast(base_top + tris[k]);
            const t1: u32 = @intCast(base_top + tris[k + 1]);
            const t2: u32 = @intCast(base_top + tris[k + 2]);
            // make top faces counter-clockwise seen from +z
            if (ccw) try b.tri(t0, t1, t2) else try b.tri(t0, t2, t1);
            const b0: u32 = @intCast(base_bot + tris[k]);
            const b1: u32 = @intCast(base_bot + tris[k + 1]);
            const b2: u32 = @intCast(base_bot + tris[k + 2]);
            if (ccw) try b.tri(b0, b2, b1) else try b.tri(b0, b1, b2);
        }
    }
    // sides
    const smooth_rad = std.math.degreesToRadians(smooth_deg);
    for (loop, 0..) |p, i| {
        const q = loop[(i + 1) % n];
        const prev = loop[(i + n - 1) % n];
        const next = loop[(i + 2) % n];
        const d = q.sub(p);
        const l = d.len();
        if (l < 1e-12) continue;
        // outward normal
        const nrm = if (ccw) V2.init(d.y / l, -d.x / l) else V2.init(-d.y / l, d.x / l);
        // smooth shading across shallow turns
        var n0 = nrm;
        var n1 = nrm;
        if (turnAngle(prev, p, q) < smooth_rad) {
            const dp = p.sub(prev).norm();
            const np = if (ccw) V2.init(dp.y, -dp.x) else V2.init(-dp.y, dp.x);
            n0 = np.add(nrm).norm();
        }
        if (turnAngle(p, q, next) < smooth_rad) {
            const dq = next.sub(q).norm();
            const nq = if (ccw) V2.init(dq.y, -dq.x) else V2.init(-dq.y, dq.x);
            n1 = nq.add(nrm).norm();
        }
        const v00 = try b.vert(p.x, p.y, z0, n0.x, n0.y, 0);
        const v10 = try b.vert(q.x, q.y, z0, n1.x, n1.y, 0);
        const v11 = try b.vert(q.x, q.y, z1, n1.x, n1.y, 0);
        const v01 = try b.vert(p.x, p.y, z1, n0.x, n0.y, 0);
        if (ccw) {
            try b.tri(v00, v10, v11);
            try b.tri(v00, v11, v01);
        } else {
            try b.tri(v00, v11, v10);
            try b.tri(v00, v01, v11);
        }
        // outline edges on both caps
        try b.edge(.{ p.x, p.y, z0 }, .{ q.x, q.y, z0 });
        try b.edge(.{ p.x, p.y, z1 }, .{ q.x, q.y, z1 });
        // vertical edge at sharp vertices
        if (turnAngle(prev, p, q) >= std.math.degreesToRadians(20.0)) try b.edge(.{ p.x, p.y, z0 }, .{ p.x, p.y, z1 });
    }
}

/// Swept circular tube along a centerline in the XY plane at height zc.
fn tube(b: *Builder, center: []const V2, r: f64, zc: f64) Allocator.Error!void {
    const m = center.len;
    if (m < 2) return;
    const seg = 12;
    var rings: std.ArrayList(u32) = .empty;
    for (center, 0..) |p, i| {
        var t: V2 = undefined;
        if (i == 0) t = center[1].sub(p) else if (i + 1 == m) t = p.sub(center[i - 1]) else t = center[i + 1].sub(center[i - 1]);
        t = t.norm();
        const nrm = t.perp();
        for (0..seg) |k| {
            const phi = 2.0 * std.math.pi * @as(f64, @floatFromInt(k)) / @as(f64, seg);
            const dir = nrm.scale(@cos(phi));
            const nz = @sin(phi);
            try rings.append(b.a, try b.vert(p.x + r * dir.x, p.y + r * dir.y, zc + r * nz, dir.x, dir.y, nz));
        }
    }
    var i: usize = 0;
    while (i + 1 < m) : (i += 1) {
        for (0..seg) |k| {
            const k2 = (k + 1) % seg;
            const a0 = rings.items[i * seg + k];
            const a1 = rings.items[i * seg + k2];
            const b0 = rings.items[(i + 1) * seg + k];
            const b1 = rings.items[(i + 1) * seg + k2];
            try b.tri(a0, b0, b1);
            try b.tri(a0, b1, a1);
        }
    }
    // feature edges: end rings and four longitudinal lines
    for ([2]usize{ 0, m - 1 }) |ri| {
        for (0..seg) |k| {
            const p0 = b.p.positions.items[rings.items[ri * seg + k] * 3 ..][0..3];
            const p1 = b.p.positions.items[rings.items[ri * seg + (k + 1) % seg] * 3 ..][0..3];
            try b.edge(.{ p0[0], p0[1], p0[2] }, .{ p1[0], p1[1], p1[2] });
        }
    }
    var q: usize = 0;
    while (q < seg) : (q += seg / 4) {
        var j: usize = 0;
        while (j + 1 < m) : (j += 1) {
            const p0 = b.p.positions.items[rings.items[j * seg + q] * 3 ..][0..3];
            const p1 = b.p.positions.items[rings.items[(j + 1) * seg + q] * 3 ..][0..3];
            try b.edge(.{ p0[0], p0[1], p0[2] }, .{ p1[0], p1[1], p1[2] });
        }
    }
}

fn hexColor(st: *const style_mod.Style, material: []const u8) []const u8 {
    if (st.material(material)) |m| return m.color3d;
    return "#A0A0A0";
}

const cmu_unit_len = 15.625;
const cmu_joint = 0.375;

pub fn build(a: Allocator, scene: *const scene_mod.Scene) Allocator.Error![]Part {
    var parts: std.ArrayList(Part) = .empty;
    const st = scene.style;
    for (scene.comps) |*c| {
        if (c.state != .ok or !c.visible) continue;
        for (c.world) |pr| {
            if (pr.kind == .ghost) continue;
            var part = Part{
                .src = c.id,
                .part = if (pr.part.len > 0) pr.part else null,
                .instance = pr.instance,
                .material = pr.material,
                .color = hexColor(st, pr.material),
            };
            var b = Builder{ .a = a, .p = &part };
            if (pr.kind == .line and pr.line_pts.len == 0) continue;
            if (pr.centerline.len >= 2 and pr.sweep_r > 0 and (pr.loops.len > 0)) {
                if (pr.zhalf == 0) {
                    const cl = try geom.flattenPolyline(a, pr.centerline, false, 0.02);
                    try tube(&b, cl, pr.sweep_r, (pr.z0 + pr.z1) / 2);
                    try parts.append(a, part);
                    continue;
                }
            }
            const loop = try flattenForMesh(a, pr.loops[0]);
            if (pr.cmu_unit and !std.mem.eql(u8, pr.material, "grout")) {
                // split along z into 15 5/8" units with 3/8" head joints, running bond
                const offset: f64 = if (pr.course % 2 == 0) 8.0 else 0.0;
                var z = pr.z0 - offset;
                var joints: std.ArrayList([2]f64) = .empty;
                while (z < pr.z1) : (z += cmu_unit_len + cmu_joint) {
                    const uu0 = @max(z, pr.z0);
                    const uu1 = @min(z + cmu_unit_len, pr.z1);
                    if (uu1 - uu0 > 1e-6) try extrude(&b, loop, uu0, uu1, 20);
                    const j0 = z + cmu_unit_len;
                    const j1 = @min(z + cmu_unit_len + cmu_joint, pr.z1);
                    if (j1 - @max(j0, pr.z0) > 1e-6 and j0 < pr.z1) try joints.append(a, .{ @max(j0, pr.z0), j1 });
                }
                try parts.append(a, part);
                if (joints.items.len > 0) {
                    var mortar = Part{
                        .src = c.id,
                        .part = pr.part,
                        .instance = pr.instance,
                        .material = "mortar",
                        .color = hexColor(st, "mortar"),
                    };
                    var mb = Builder{ .a = a, .p = &mortar };
                    for (joints.items) |j| try extrude(&mb, loop, j[0], j[1], 20);
                    try parts.append(a, mortar);
                }
                continue;
            }
            try extrude(&b, loop, pr.z0, pr.z1, 20);
            try parts.append(a, part);
        }
    }
    return parts.items;
}

pub fn toJson(a: Allocator, parts: []const Part) Allocator.Error![]u8 {
    var out: std.ArrayList(u8) = .empty;
    try out.appendSlice(a, "{\"kerf_mesh\":\"0.1\",\"parts\":[\n");
    for (parts, 0..) |p, i| {
        if (i > 0) try out.appendSlice(a, ",\n");
        try out.appendSlice(a, "{\"src\":");
        try json.writeString(&out, a, p.src);
        try out.appendSlice(a, ",\"part\":");
        if (p.part) |x| try json.writeString(&out, a, x) else try out.appendSlice(a, "null");
        try out.print(a, ",\"instance\":{d},\"material\":", .{p.instance});
        try json.writeString(&out, a, p.material);
        try out.appendSlice(a, ",\"color\":");
        try json.writeString(&out, a, p.color);
        try out.appendSlice(a, ",\"positions\":");
        try floats(&out, a, p.positions.items);
        try out.appendSlice(a, ",\"normals\":");
        try floats(&out, a, p.normals.items);
        try out.appendSlice(a, ",\"indices\":[");
        for (p.indices.items, 0..) |x, k| {
            if (k > 0) try out.append(a, ',');
            try out.print(a, "{d}", .{x});
        }
        try out.appendSlice(a, "],\"edges\":");
        try floats(&out, a, p.edges.items);
        try out.append(a, '}');
    }
    try out.appendSlice(a, "\n]}\n");
    return out.items;
}

fn floats(out: *std.ArrayList(u8), a: Allocator, xs: []const f32) Allocator.Error!void {
    try out.append(a, '[');
    for (xs, 0..) |x, i| {
        if (i > 0) try out.append(a, ',');
        try json.writeNumber(out, a, @as(f64, x));
    }
    try out.append(a, ']');
}

test "ear clipping an L" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const l = [_]V2{ V2.init(0, 0), V2.init(2, 0), V2.init(2, 1), V2.init(1, 1), V2.init(1, 2), V2.init(0, 2) };
    const t = try earClip(a, &l);
    try std.testing.expectEqual(@as(usize, 12), t.len);
    var area: f64 = 0;
    var k: usize = 0;
    while (k < t.len) : (k += 3) area += @abs(l[t[k + 1]].sub(l[t[k]]).cross(l[t[k + 2]].sub(l[t[k]]))) / 2;
    try std.testing.expectApproxEqAbs(3.0, area, 1e-9);
}
