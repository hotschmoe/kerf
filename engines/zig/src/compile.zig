//! Document -> Scene: validate components, order the placement DAG, run builders, place
//! (translate / rotate / mirror / arrays / z extents) and produce world-space prisms.

const std = @import("std");
const json = @import("json.zig");
const geom = @import("geom.zig");
const model = @import("model.zig");
const units = @import("units.zig");
const catalog = @import("catalog.zig");
const builders = @import("builders.zig");
const scene_mod = @import("scene.zig");
const style_mod = @import("style.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Scene = scene_mod.Scene;
const Comp = scene_mod.Comp;

pub const default_run = [2]f64{ -24, 24 };

pub fn validId(id: []const u8) bool {
    if (id.len == 0) return false;
    if (!(id[0] >= 'a' and id[0] <= 'z')) return false;
    for (id) |c| {
        if (!((c >= 'a' and c <= 'z') or (c >= '0' and c <= '9') or c == '_')) return false;
    }
    return true;
}

fn refCompId(s: []const u8) ?[]const u8 {
    const at = std.mem.indexOfScalar(u8, s, '@') orelse return null;
    var head = s[0..at];
    if (head.len == 0) return null;
    if (std.mem.indexOfScalar(u8, head, '.')) |d| head = head[0..d];
    if (std.mem.indexOfScalar(u8, head, '#')) |h| head = head[0..h];
    return head;
}

fn addRefDep(a: Allocator, v: json.Value, out: *std.ArrayList([]const u8)) Allocator.Error!void {
    switch (v) {
        .string => |s| if (refCompId(s)) |id| try out.append(a, id),
        .object => if (v.get("ref")) |r| try addRefDep(a, r, out),
        else => {},
    }
}

pub fn collectDeps(a: Allocator, node: json.Value, out: *std.ArrayList([]const u8)) Allocator.Error!void {
    if (node.get("at")) |at| if (at.get("to")) |to| try addRefDep(a, to, out);
    if (node.get("until")) |u| try addRefDep(a, u, out);
    if (node.get("points")) |pts| if (pts.arr()) |arr| for (arr) |e| try addRefDep(a, e, out);
    if (node.get("profile")) |pr| if (pr.get("points")) |pts| if (pts.arr()) |arr| for (arr) |e| try addRefDep(a, e, out);
    if (node.get("place")) |pl| if (pl.get("in")) |inn| if (inn.str()) |s| {
        var head = s;
        if (std.mem.indexOfScalar(u8, s, '.')) |d| head = s[0..d];
        try out.append(a, head);
    };
}

pub fn compile(a: Allocator, doc: json.Value, st: *const style_mod.Style, diags: *model.Diags) Allocator.Error!*Scene {
    const scene = try a.create(Scene);
    scene.* = .{ .a = a, .style = st, .comps = &.{}, .diags = diags, .run = default_run };
    if (doc != .object) {
        diags.add(.@"error", "E_PARAM", null, "", "the document must be a JSON object with \"kerf\", \"id\", \"components\" and \"views\"", .{});
        return scene;
    }
    if (doc.get("run")) |rv| {
        const arr = rv.arr();
        if (arr != null and arr.?.len == 2 and units.parseLength(arr.?[0]) != null and units.parseLength(arr.?[1]) != null) {
            const z0 = units.parseLength(arr.?[0]).?;
            const z1 = units.parseLength(arr.?[1]).?;
            if (z0 < z1) scene.run = .{ z0, z1 } else diags.add(.@"error", "E_PARAM", null, "run", "'run' must be [z0, z1] with z0 < z1 (got [{d}, {d}])", .{ z0, z1 });
        } else diags.add(.@"error", "E_PARAM", null, "run", "'run' must be [z0, z1] in inches, e.g. [-24, 24]", .{});
    }
    const comps_v = doc.get("components") orelse json.Value{ .array = &.{} };
    const items = comps_v.arr() orelse {
        diags.add(.@"error", "E_PARAM", null, "components", "'components' must be an array", .{});
        return scene;
    };

    var comps: std.ArrayList(Comp) = .empty;
    const type_names = try catalog.typeNames(a);
    for (items, 0..) |item, i| {
        const path = try std.fmt.allocPrint(a, "components/{d}", .{i});
        if (item != .object) {
            diags.add(.@"error", "E_PARAM", null, path, "component {d} must be an object with \"id\" and \"type\"", .{i});
            continue;
        }
        const id_v = item.get("id");
        if (id_v == null or id_v.? != .string) {
            diags.add(.@"error", "E_PARAM", null, path, "component {d} needs a string \"id\" matching [a-z][a-z0-9_]*", .{i});
            continue;
        }
        const id = id_v.?.string;
        if (!validId(id)) {
            diags.addFix(.@"error", "E_PARAM", id, path, "id \"{s}\" is invalid: ids must match [a-z][a-z0-9_]* (lowercase letters, digits, underscore)", .{id}, "rename it, e.g. \"sill_plate\"");
            continue;
        }
        var dup = false;
        for (comps.items) |c| if (std.mem.eql(u8, c.id, id)) {
            dup = true;
        };
        if (dup) {
            diags.add(.@"error", "E_DUP_ID", id, path, "duplicate component id '{s}': ids must be unique across components", .{id});
            continue;
        }
        const ty_v = item.get("type");
        if (ty_v == null or ty_v.? != .string) {
            diags.add(.@"error", "E_PARAM", id, try std.fmt.allocPrint(a, "components/{s}/type", .{id}), "component '{s}' needs a \"type\": one of {s}", .{ id, model.joinQuoted(a, type_names) });
            continue;
        }
        const ty = catalog.find(ty_v.?.string) orelse {
            const hint = if (model.nearest(a, ty_v.?.string, type_names)) |n| try std.fmt.allocPrint(a, " Did you mean \"{s}\"?", .{n}) else "";
            diags.add(.@"error", "E_PARAM", id, try std.fmt.allocPrint(a, "components/{s}/type", .{id}), "unknown component type \"{s}\". Types: {s}.{s}", .{ ty_v.?.string, model.joinQuoted(a, type_names), hint });
            continue;
        };
        // unknown keys
        for (item.object) |m| {
            if (!catalog.allowedKey(ty, m.key)) {
                const allowed = try catalog.allowedKeysText(a, ty);
                diags.add(.@"error", "E_PARAM", id, try std.fmt.allocPrint(a, "components/{s}/{s}", .{ id, m.key }), "unknown param '{s}' for type {s}. Allowed: {s}", .{ m.key, ty.name, allowed });
            }
        }
        var label: ?[]const u8 = null;
        if (item.get("label")) |l| label = l.str();
        try comps.append(a, .{
            .index = @intCast(comps.items.len),
            .id = id,
            .ty = ty,
            .node = item,
            .label = label,
        });
    }
    scene.comps = comps.items;
    const n = comps.items.len;

    // Dependencies and topological order (Kahn, ties by document order).
    const deps = try a.alloc([]const usize, n);
    for (scene.comps, 0..) |c, i| {
        var names: std.ArrayList([]const u8) = .empty;
        try collectDeps(a, c.node, &names);
        var idx: std.ArrayList(usize) = .empty;
        for (names.items) |nm| {
            for (scene.comps, 0..) |o, k| if (std.mem.eql(u8, o.id, nm) and k != i) {
                var seen = false;
                for (idx.items) |x| if (x == k) {
                    seen = true;
                };
                if (!seen) try idx.append(a, k);
            };
            // self-reference through `at.to` is a cycle of length 1
            if (std.mem.eql(u8, nm, c.id)) {
                diags.add(.@"error", "E_CYCLE", c.id, try std.fmt.allocPrint(a, "components/{s}", .{c.id}), "placement cycle: '{s}' refers to its own anchors. Give it an absolute position, e.g. \"at\": {{\"to\": [0, 0]}}", .{c.id});
                scene.comps[i].state = .failed;
            }
        }
        deps[i] = idx.items;
    }
    const done = try a.alloc(bool, n);
    @memset(done, false);
    var order: std.ArrayList(usize) = .empty;
    while (order.items.len < n) {
        var picked: ?usize = null;
        for (0..n) |i| {
            if (done[i]) continue;
            var ready = true;
            for (deps[i]) |d| if (!done[d]) {
                ready = false;
                break;
            };
            if (ready) {
                picked = i;
                break;
            }
        }
        if (picked) |p| {
            done[p] = true;
            try order.append(a, p);
        } else {
            // cycle among the remaining: find and report one
            var start: usize = 0;
            for (0..n) |i| if (!done[i]) {
                start = i;
                break;
            };
            var path: std.ArrayList(usize) = .empty;
            var cur = start;
            var guard: usize = 0;
            while (guard <= n) : (guard += 1) {
                // follow the first unfinished dependency
                if (std.mem.indexOfScalar(usize, path.items, cur)) |at| {
                    const cyc = path.items[at..];
                    var txt: std.ArrayList(u8) = .empty;
                    for (cyc) |ci| {
                        try txt.appendSlice(a, scene.comps[ci].id);
                        try txt.appendSlice(a, " -> ");
                    }
                    try txt.appendSlice(a, scene.comps[cyc[0]].id);
                    diags.addFix(.@"error", "E_CYCLE", scene.comps[cyc[0]].id, try std.fmt.allocPrint(a, "components/{s}/at", .{scene.comps[cyc[0]].id}), "placement cycle: {s}. Components may not locate each other", .{txt.items}, "give one of them an absolute \"at\": {\"to\": [x, y]}");
                    break;
                }
                try path.append(a, cur);
                var next: ?usize = null;
                for (deps[cur]) |d| if (!done[d]) {
                    next = d;
                    break;
                };
                cur = next orelse break;
            }
            for (0..n) |i| if (!done[i]) {
                done[i] = true;
                scene.comps[i].state = .failed;
                try order.append(a, i);
            };
        }
    }

    for (order.items) |i| {
        if (scene.comps[i].state == .failed) continue;
        try placeComponent(a, scene, &scene.comps[i]);
    }
    return scene;
}

// ---- placement -----------------------------------------------------------------------------------------

const AtSpec = struct {
    anchor: []const u8 = "bottom_left",
    to: V2 = V2.init(0, 0),
    has_to: bool = false,
};

fn parseAt(a: Allocator, scene: *Scene, comp: *Comp, p: *model.Params) Allocator.Error!?AtSpec {
    const av = p.raw("at") orelse return AtSpec{};
    if (av != .object) {
        p.fail("at", "param 'at' must be {{\"anchor\": \"bottom_left\", \"to\": \"comp@anchor\", \"offset\": [dx, dy]}}", .{});
        return null;
    }
    var spec = AtSpec{};
    if (av.get("anchor")) |anc| {
        if (anc.str()) |s| spec.anchor = s else {
            p.fail("at/anchor", "at.anchor must be an anchor name string", .{});
            return null;
        }
    }
    const base = try std.fmt.allocPrint(a, "components/{s}/at/to", .{comp.id});
    if (av.get("to")) |to| if (to != .null) {
        spec.has_to = true;
        switch (to) {
            .string => |s| spec.to = scene.resolveRefStr(s, comp.id, base) orelse {
                p.ok = false;
                return null;
            },
            .array => |xy| {
                if (xy.len != 2 or units.parseLength(xy[0]) == null or units.parseLength(xy[1]) == null) {
                    p.fail("at/to", "at.to must be a Ref, {{\"ref\": ..., \"offset\": [dx, dy]}} or a point [x, y]", .{});
                    return null;
                }
                spec.to = V2.init(units.parseLength(xy[0]).?, units.parseLength(xy[1]).?);
            },
            .object => {
                const rs = (if (to.get("ref")) |r| r.str() else null) orelse {
                    p.fail("at/to", "at.to object needs a string \"ref\" (and optional \"offset\": [dx, dy])", .{});
                    return null;
                };
                var pt = scene.resolveRefStr(rs, comp.id, base) orelse {
                    p.ok = false;
                    return null;
                };
                if (to.get("offset")) |off| if (off.arr()) |oa| if (oa.len >= 2) {
                    pt = pt.add(V2.init(units.parseLength(oa[0]) orelse 0, units.parseLength(oa[1]) orelse 0));
                };
                spec.to = pt;
            },
            else => {
                p.fail("at/to", "at.to must be a Ref string, {{ref, offset}} or [x, y]", .{});
                return null;
            },
        }
    };
    if (av.get("offset")) |off| if (off.arr()) |oa| {
        if (oa.len != 2 or units.parseLength(oa[0]) == null or units.parseLength(oa[1]) == null) {
            p.fail("at/offset", "at.offset must be [dx, dy] in inches", .{});
            return null;
        }
        spec.to = spec.to.add(V2.init(units.parseLength(oa[0]).?, units.parseLength(oa[1]).?));
    };
    return spec;
}

fn placeComponent(a: Allocator, scene: *Scene, comp: *Comp) Allocator.Error!void {
    var p = model.Params{ .a = a, .diags = scene.diags, .node = comp.node, .id = comp.id, .base = "components", .ty = comp.ty.name };
    comp.state = .failed;

    const at = (try parseAt(a, scene, comp, &p)) orelse return;
    comp.place_pt = at.to;
    // rotation
    var angle: f64 = 0;
    if (p.has("rotate")) {
        angle += std.math.degreesToRadians(p.num("rotate", 0) orelse 0);
    }
    if (p.raw("slope")) |sv| {
        if (units.parseSlope(sv)) |r| angle += r else p.fail("slope", "param 'slope' must be rise:run like \"4:12\" or degrees (got {s})", .{model.kindOrText(a, sv)});
    }
    const mirror = p.boolean("mirror", false);
    const visible = p.boolean("visible", true);
    if (!p.ok) return;

    var bctx = builders.Ctx{
        .a = a,
        .scene = scene,
        .style = scene.style,
        .comp = comp,
        .p = p,
        .origin = at.to,
        .run = scene.run,
    };
    var built = (try builders.build(&bctx)) orelse return;
    p = bctx.p;
    if (!p.ok) return;
    if (mirror and !built.points_mode) built = try builders.mirrorAboutCenter(a, built);

    // z extents
    var zbase: [2]f64 = scene.run;
    var z_explicit = false;
    if (p.raw("z")) |zv| {
        switch (zv) {
            .array => |za| {
                if (za.len == 2 and units.parseLength(za[0]) != null and units.parseLength(za[1]) != null) {
                    zbase = .{ units.parseLength(za[0]).?, units.parseLength(za[1]).? };
                    z_explicit = true;
                    if (zbase[0] >= zbase[1]) {
                        p.fail("z", "z range must have z0 < z1 (got [{d}, {d}])", .{ zbase[0], zbase[1] });
                        return;
                    }
                } else {
                    p.fail("z", "param 'z' must be [z0, z1] or a single z (center)", .{});
                    return;
                }
            },
            .number, .string => {
                const c = units.parseLength(zv) orelse {
                    p.fail("z", "param 'z' must be [z0, z1] or a single z (center)", .{});
                    return;
                };
                const th = built.nat_z orelse (scene.run[1] - scene.run[0]);
                zbase = .{ c - th / 2, c + th / 2 };
                z_explicit = true;
            },
            else => {
                p.fail("z", "param 'z' must be [z0, z1] or a single z (center)", .{});
                return;
            },
        }
    }
    if (!z_explicit) {
        if (built.nat_z) |th| {
            const mid = (scene.run[0] + scene.run[1]) / 2;
            zbase = .{ mid - th / 2, mid + th / 2 };
        }
    }
    const centered = built.nat_z != null;

    // array
    var arr_axis: u8 = 'x';
    var arr_count: usize = 1;
    var arr_spacing: f64 = 0;
    if (p.raw("array")) |av| {
        if (av != .object) {
            p.fail("array", "param 'array' must be {{\"axis\": \"x|y|z\", \"count\": n, \"spacing\": s}}", .{});
            return;
        }
        const ax = (if (av.get("axis")) |x| x.str() else null) orelse "x";
        if (ax.len != 1 or (ax[0] != 'x' and ax[0] != 'y' and ax[0] != 'z')) {
            p.fail("array/axis", "array.axis must be \"x\", \"y\" or \"z\" (got \"{s}\")", .{ax});
            return;
        }
        arr_axis = ax[0];
        const cnt = (if (av.get("count")) |x| x.num() else null) orelse 1;
        if (cnt < 1 or cnt != @round(cnt) or cnt > 500) {
            p.fail("array/count", "array.count must be an integer from 1 to 500", .{});
            return;
        }
        arr_count = @intFromFloat(cnt);
        arr_spacing = (if (av.get("spacing")) |x| units.parseLength(x) else null) orelse {
            p.fail("array/spacing", "array.spacing must be a length (inches between instances; may be negative)", .{});
            return;
        };
    }
    const emb_override: ?bool = if (p.has("embedded")) p.boolean("embedded", false) else null;
    if (!p.ok) return;

    // transforms per instance
    const n_centers = if (built.centers.len > 0) built.centers.len else 1;
    const n_inst = n_centers * arr_count;
    const xfs = try a.alloc(geom.Xf, n_inst);
    const zs = try a.alloc([2]f64, n_inst);
    var base_xf: geom.Xf = undefined;
    if (built.points_mode) {
        base_xf = geom.Xf.translate(at.to.x, at.to.y).mul(geom.Xf.rotate(angle));
    } else {
        const anc = blk: {
            if (model.boxAnchor(built.box, at.anchor)) |v| break :blk v;
            for (built.anchors) |n| if (std.mem.eql(u8, n.name, at.anchor)) break :blk n.p;
            const names = joinAnchorNames(a, built);
            p.fail("at/anchor", "'{s}' is not an anchor of '{s}' ({s}). Anchors: {s}", .{ at.anchor, comp.id, comp.ty.name, names });
            // use the right code
            scene.diags.list.items[scene.diags.list.items.len - 1].code = "E_ANCHOR_UNKNOWN";
            return;
        };
        base_xf = geom.Xf.translate(at.to.x, at.to.y).mul(geom.Xf.rotate(angle)).mul(geom.Xf.translate(-anc.x, -anc.y));
    }
    var k: usize = 0;
    var inst: usize = 0;
    while (k < arr_count) : (k += 1) {
        const kf: f64 = @floatFromInt(k);
        var j: usize = 0;
        while (j < n_centers) : (j += 1) {
            var xf = base_xf;
            if (built.centers.len > 0) {
                const c = built.centers[j];
                xf = geom.Xf.translate(c.x, c.y).mul(geom.Xf.rotate(angle));
            }
            var zr = zbase;
            switch (arr_axis) {
                'x' => xf = geom.Xf.translate(kf * arr_spacing, 0).mul(xf),
                'y' => xf = geom.Xf.translate(0, kf * arr_spacing).mul(xf),
                else => {
                    zr = .{ zbase[0] + kf * arr_spacing, zbase[1] + kf * arr_spacing };
                },
            }
            if (arr_count == 1) {} // no shift
            xfs[inst] = xf;
            zs[inst] = zr;
            inst += 1;
        }
    }
    // world prisms
    var world: std.ArrayList(model.Prism) = .empty;
    for (0..n_inst) |ii| {
        for (built.prisms) |pr| {
            var q = try pr.transform(a, xfs[ii]);
            q.comp = comp.index;
            q.instance = @intCast(ii);
            q.z0 = zs[ii][0];
            q.z1 = zs[ii][1];
            if (centered and pr.zhalf > 0) {
                const zc = (zs[ii][0] + zs[ii][1]) / 2;
                q.z0 = zc - pr.zhalf;
                q.z1 = zc + pr.zhalf;
            }
            if (emb_override) |e| q.embedded = e;
            try world.append(a, q);
        }
    }
    comp.built = built;
    comp.xfs = xfs;
    comp.zs = zs;
    comp.world = world.items;
    comp.visible = visible;
    comp.arr_count = arr_count;
    comp.embedded = if (emb_override) |e| e else (built.prisms.len > 0 and built.prisms[0].embedded);
    comp.state = .ok;
}

fn joinAnchorNames(a: Allocator, b: model.Built) []const u8 {
    var out: std.ArrayList(u8) = .empty;
    for (model.box_anchor_names, 0..) |n, i| {
        if (i > 0) out.appendSlice(a, ", ") catch {};
        out.appendSlice(a, n) catch {};
    }
    for (b.anchors) |n| {
        out.appendSlice(a, ", ") catch {};
        out.appendSlice(a, n.name) catch {};
    }
    return out.items;
}

/// All visible world prisms in document order.
pub fn allPrisms(a: Allocator, scene: *const Scene) Allocator.Error![]const model.Prism {
    var out: std.ArrayList(model.Prism) = .empty;
    for (scene.comps) |c| {
        if (c.state != .ok or !c.visible) continue;
        try out.appendSlice(a, c.world);
    }
    return out.items;
}

test "ids" {
    try std.testing.expect(validId("sill_plate"));
    try std.testing.expect(validId("a1"));
    try std.testing.expect(!validId("1a"));
    try std.testing.expect(!validId("Sill"));
    try std.testing.expect(!validId(""));
    try std.testing.expect(!validId("a-b"));
}

test "reference documents compile without errors" {
    const testdocs = @import("testdocs.zig");
    for (testdocs.all) |src| {
        var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
        defer arena.deinit();
        const a = arena.allocator();
        var err: json.ParseError = undefined;
        const doc = (try json.parse(a, src, &err)).?;
        const st = try style_mod.load(a, null);
        var diags = model.Diags.init(a);
        const scene = try compile(a, doc, &st, &diags);
        try std.testing.expectEqual(@as(usize, 0), diags.errCount());
        for (scene.comps) |c| try std.testing.expect(c.state == .ok);
    }
}

/// Visible world prisms minus the components a view omits.
pub fn viewPrisms(a: Allocator, scene: *const Scene, omit: []const []const u8) Allocator.Error![]const model.Prism {
    var out: std.ArrayList(model.Prism) = .empty;
    for (scene.comps) |c| {
        if (c.state != .ok or !c.visible) continue;
        var skip = false;
        for (omit) |o| if (std.mem.eql(u8, o, c.id)) {
            skip = true;
        };
        if (skip) continue;
        try out.appendSlice(a, c.world);
    }
    return out.items;
}

pub fn isOmitted(omit: []const []const u8, id: []const u8) bool {
    for (omit) |o| if (std.mem.eql(u8, o, id)) return true;
    return false;
}

test "until grows a member from its anchor to a ref" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const src =
        \\{"kerf":"0.1","id":"t","run":[-2,2],"components":[
        \\ {"id":"plate","type":"lumber","size":"2x4","run":"x","face":"narrow","length":48,"at":{"to":[0,0]}},
        \\ {"id":"beam","type":"lumber","size":"2x8","run":"x","length":48,"at":{"to":[0,96]}},
        \\ {"id":"stud","type":"lumber","size":"2x4","run":"y","face":"narrow","until":"plate@top_left",
        \\  "at":{"anchor":"top_left","to":"beam@bottom_left","offset":[10,0]}}
        \\],"views":[]}
    ;
    var err: json.ParseError = undefined;
    const doc = (try json.parse(a, src, &err)).?;
    const st = try style_mod.load(a, null);
    var diags = model.Diags.init(a);
    const scene = try compile(a, doc, &st, &diags);
    try std.testing.expectEqual(@as(usize, 0), diags.errCount());
    const stud = scene.find("stud").?;
    // beam bottom is y=96 (2x8 flat? upright wide: depth 7.25 in plane) -> bottom at 96; plate top = 1.5
    try std.testing.expectApproxEqAbs(96.0 - 1.5, stud.built.box.y1 - stud.built.box.y0, 1e-9);
}

test "until with length is an error and center anchors are rejected" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const src =
        \\{"kerf":"0.1","id":"t","components":[
        \\ {"id":"plate","type":"lumber","size":"2x4","run":"x","length":48,"at":{"to":[0,0]}},
        \\ {"id":"s1","type":"lumber","size":"2x4","run":"y","length":10,"until":"plate@top_left","at":{"anchor":"top_left","to":[5,50]}},
        \\ {"id":"s2","type":"lumber","size":"2x4","run":"y","until":"plate@top_left","at":{"anchor":"middle_left","to":[5,50]}}
        \\],"views":[]}
    ;
    var err: json.ParseError = undefined;
    const doc = (try json.parse(a, src, &err)).?;
    const st = try style_mod.load(a, null);
    var diags = model.Diags.init(a);
    _ = try compile(a, doc, &st, &diags);
    try std.testing.expectEqual(@as(usize, 2), diags.errCount());
    try std.testing.expect(diags.hasCode("E_PARAM"));
}
