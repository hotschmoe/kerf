//! A loaded document: compiled scene + diagnostics, plus summary text and `inspect` queries.

const std = @import("std");
const json = @import("json.zig");
const geom = @import("geom.zig");
const model = @import("model.zig");
const style_mod = @import("style.zig");
const compile_mod = @import("compile.zig");
const scene_mod = @import("scene.zig");
const validate = @import("validate.zig");
const view_mod = @import("view.zig");
const drawview = @import("drawview.zig");
const section = @import("section.zig");
const catalog = @import("catalog.zig");
const units = @import("units.zig");
const builders = @import("builders.zig");
const lint = @import("lint.zig");
const coverage = @import("coverage.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;

pub const Loaded = struct {
    a: Allocator,
    doc: json.Value,
    style: *const style_mod.Style,
    scene: *scene_mod.Scene,
    diags: *model.Diags,
    nviews: usize,
};

/// Compile + validate (+ every view's annotation diagnostics when `with_views`).
pub fn load(a: Allocator, doc: json.Value, st: *const style_mod.Style, with_views: bool) Allocator.Error!Loaded {
    const diags = try a.create(model.Diags);
    diags.* = model.Diags.init(a);
    const scene = try compile_mod.compile(a, doc, st, diags);
    if (doc == .object) try validate.run(a, scene, doc, diags);
    if (doc == .object) try lint.run(a, scene, doc, diags);
    var nviews: usize = 0;
    // dims without `dir` get the dominant-axis default for drawing (the stored document is never changed)
    const vdoc = if (with_views and doc == .object) try lint.withDimDirs(a, scene, doc) else doc;
    if (doc == .object) {
        if (vdoc.get("views")) |vs| if (vs.arr()) |va| {
            nviews = va.len;
            var seen: std.ArrayList([]const u8) = .empty;
            for (va, 0..) |v, i| {
                const vid = if (v.get("id")) |x| (x.str() orelse "") else "";
                var dup = false;
                for (seen.items) |s| if (std.mem.eql(u8, s, vid)) {
                    dup = true;
                };
                if (dup) {
                    diags.add(.@"error", "E_DUP_ID", vid, try std.fmt.allocPrint(a, "views/{d}", .{i}), "duplicate view id '{s}'", .{vid});
                    continue;
                }
                try seen.append(a, vid);
                var vd = model.Diags.init(a);
                const spec_opt = try view_mod.parse(a, v, i, &vd);
                if (spec_opt) |sp| {
                    if (with_views) {
                        const spec = try a.create(view_mod.ViewSpec);
                        spec.* = sp;
                        _ = try drawview.buildFromScene(a, vdoc, st, scene, spec, &vd);
                    }
                }
                try diags.list.appendSlice(a, vd.list.items);
            }
        };
    }
    if (doc == .object) try lint.applyAcknowledge(a, doc, diags);
    return .{ .a = a, .doc = doc, .style = st, .scene = scene, .diags = diags, .nviews = nviews };
}

// ---- summary --------------------------------------------------------------------------------------------

fn padTo(out: *std.ArrayList(u8), a: Allocator, s: []const u8, n: usize) Allocator.Error!void {
    try out.appendSlice(a, s);
    var k = s.len;
    if (k >= n) {
        try out.append(a, ' ');
        return;
    }
    while (k < n) : (k += 1) try out.append(a, ' ');
}

pub fn compBox(c: *const scene_mod.Comp) geom.Box {
    var b = geom.Box{};
    for (c.world) |p| for (p.loops) |l| b.addBox(geom.loopBox(l));
    return b;
}

pub fn diagLine(a: Allocator, d: model.Diag) Allocator.Error![]const u8 {
    const lvl: []const u8 = switch (d.level) {
        .@"error" => "ERROR",
        .warning => "WARN",
        .info => "INFO",
    };
    var out: std.ArrayList(u8) = .empty;
    try out.print(a, "{s} {s}", .{ lvl, d.code });
    if (d.id) |id| if (id.len > 0) try out.print(a, " {s}", .{id});
    try out.print(a, ": {s}", .{d.message});
    if (d.fix) |f| try out.print(a, " Fix: {s}", .{f});
    return out.items;
}

/// The `COVERAGE` block (SPEC 21): one line per `meta.requested` item, `ok <component ids>` or `MISSING`.
fn coverageBlock(a: Allocator, out: *std.ArrayList(u8), doc: json.Value) Allocator.Error!void {
    const items = try coverage.compute(a, doc);
    if (items.len == 0) return;
    var covered: usize = 0;
    for (items) |it| {
        if (it.found.len > 0) covered += 1;
    }
    try out.print(a, "COVERAGE  {d}/{d} requested elements covered{s}\n", .{ covered, items.len, if (covered == items.len) "" else "  (finish only at full coverage)" });
    for (items) |it| {
        try out.append(a, ' ');
        try padTo(out, a, if (it.found.len > 0) "ok" else "MISSING", 9);
        if (it.found.len == 0) {
            try out.appendSlice(a, it.text);
            try out.append(a, '\n');
            continue;
        }
        try padTo(out, a, it.text, 26);
        for (it.found, 0..) |id, i| {
            if (i == 6) {
                try out.print(a, ", +{d} more", .{it.found.len - 6});
                break;
            }
            if (i > 0) try out.appendSlice(a, ", ");
            try out.appendSlice(a, id);
        }
        try out.append(a, '\n');
    }
}

pub fn summary(l: *const Loaded) Allocator.Error![]const u8 {
    const a = l.a;
    var out: std.ArrayList(u8) = .empty;
    const e = l.diags.errCount();
    const w = l.diags.warnCount();
    const id = if (l.doc == .object) (if (l.doc.get("id")) |x| (x.str() orelse "") else "") else "";
    try out.print(a, "DOC {s}  {d} component{s}  {d} view{s}  {d} error{s}  {d} warning{s}\n", .{
        id,
        l.scene.comps.len,
        if (l.scene.comps.len == 1) "" else "s",
        l.nviews,
        if (l.nviews == 1) "" else "s",
        e,
        if (e == 1) "" else "s",
        w,
        if (w == 1) "" else "s",
    });
    for (l.scene.comps) |*c| {
        try out.append(a, ' ');
        try padTo(&out, a, c.id, 14);
        if (c.state != .ok) {
            try padTo(&out, a, c.ty.name, 44);
            try out.appendSlice(a, "(not built: see errors)\n");
            continue;
        }
        var desc: std.ArrayList(u8) = .empty;
        try desc.appendSlice(a, c.built.info);
        if (c.arr_count > 1) {
            if (c.node.get("array")) |av| {
                const ax = if (av.get("axis")) |x| (x.str() orelse "x") else "x";
                const sp = if (av.get("spacing")) |x| (units.parseLength(x) orelse 0) else 0;
                try desc.print(a, " [x{d} {s} @ {s}]", .{ c.arr_count, ax, try units.fmtFtIn(a, sp) });
            }
        }
        if (c.label) |lb| try desc.print(a, " \"{s}\"", .{lb});
        try padTo(&out, a, desc.items, 44);
        const b = compBox(c);
        var xr: std.ArrayList(u8) = .empty;
        try xr.appendSlice(a, "x ");
        try units.appendFtIn(&xr, a, b.x0);
        try xr.appendSlice(a, "..");
        try units.appendFtIn(&xr, a, b.x1);
        try padTo(&out, a, xr.items, 18);
        try out.appendSlice(a, "y ");
        try units.appendFtIn(&out, a, b.y0);
        try out.appendSlice(a, "..");
        try units.appendFtIn(&out, a, b.y1);
        try out.append(a, '\n');
    }
    if (l.doc == .object) try coverageBlock(a, &out, l.doc);
    for (l.diags.list.items) |d| {
        if (d.level == .info and !(std.mem.eql(u8, d.code, "I_SOLID_USED") or std.mem.eql(u8, d.code, "I_UNVERIFIED_CITE") or std.mem.eql(u8, d.code, "I_CITE_DOWNGRADED") or std.mem.eql(u8, d.code, "I_ACK"))) continue;
        try out.appendSlice(a, try diagLine(a, d));
        try out.append(a, '\n');
    }
    return out.items;
}

pub fn diagsJson(a: Allocator, list: []const model.Diag) Allocator.Error!json.Value {
    const out = try a.alloc(json.Value, list.len);
    for (list, 0..) |d, i| out[i] = try model.diagToJson(a, d);
    return .{ .array = out };
}

// ---- inspect ----------------------------------------------------------------------------------------------

fn anchorJson(a: Allocator, name: []const u8, p: V2) Allocator.Error!json.Value {
    return json.obj(a, &.{
        .{ .key = "name", .value = .{ .string = name } },
        .{ .key = "x", .value = .{ .number = p.x } },
        .{ .key = "y", .value = .{ .number = p.y } },
        .{ .key = "x_ft", .value = .{ .string = try units.fmtFtIn(a, p.x) } },
        .{ .key = "y_ft", .value = .{ .string = try units.fmtFtIn(a, p.y) } },
    });
}

fn boxAnchors(a: Allocator, c: *const scene_mod.Comp, part: ?[]const u8) Allocator.Error!json.Value {
    var list: std.ArrayList(json.Value) = .empty;
    for (model.box_anchor_names) |n| {
        if (scene_mod.Scene.anchorPoint(c, 0, part, n)) |p| try list.append(a, try anchorJson(a, n, p));
    }
    if (part == null) for (c.built.anchors) |n| {
        try list.append(a, try anchorJson(a, n.name, c.xfs[0].apply(n.p)));
    };
    return .{ .array = list.items };
}

pub const InspectError = struct { code: []const u8, message: []const u8 };

pub fn inspect(l: *const Loaded, q: json.Value, err: *InspectError) Allocator.Error!?json.Value {
    const a = l.a;
    const kind = (if (q.get("q")) |x| x.str() else null) orelse "summary";
    const eq = std.mem.eql;
    if (eq(u8, kind, "summary")) {
        return try json.obj(a, &.{.{ .key = "summary", .value = .{ .string = try summary(l) } }});
    }
    if (eq(u8, kind, "doc")) {
        return try json.obj(a, &.{.{ .key = "doc", .value = l.doc }});
    }
    if (eq(u8, kind, "catalog")) {
        const t = (if (q.get("type")) |x| x.str() else null) orelse {
            err.* = .{ .code = "E_PARAM", .message = "inspect catalog needs \"type\", e.g. \"truss\"" };
            return null;
        };
        if (catalog.find(t)) |e| return try catalog.entryJson(a, e);
        const names = try catalog.typeNames(a);
        err.* = .{ .code = "E_REF_UNKNOWN", .message = try std.fmt.allocPrint(a, "unknown component type \"{s}\". Types: {s}", .{ t, scene_mod.joinIds(a, names) }) };
        return null;
    }
    if (eq(u8, kind, "component") or eq(u8, kind, "anchors")) {
        const id = (if (q.get("id")) |x| x.str() else null) orelse {
            err.* = .{ .code = "E_PARAM", .message = "inspect component/anchors needs \"id\"" };
            return null;
        };
        const c = l.scene.find(id) orelse {
            const ids = try l.scene.compIds(a);
            const hint = if (model.nearest(a, id, ids)) |n| try std.fmt.allocPrint(a, " Did you mean '{s}'?", .{n}) else "";
            err.* = .{ .code = "E_REF_UNKNOWN", .message = try std.fmt.allocPrint(a, "no component '{s}'.{s} Components: {s}", .{ id, hint, scene_mod.joinIds(a, ids) }) };
            return null;
        };
        if (c.state != .ok) {
            err.* = .{ .code = "E_PARAM", .message = try std.fmt.allocPrint(a, "component '{s}' could not be built; fix its errors (see check)", .{id}) };
            return null;
        }
        const anchors = try boxAnchors(a, c, null);
        if (eq(u8, kind, "anchors")) {
            return try json.obj(a, &.{ .{ .key = "id", .value = .{ .string = id } }, .{ .key = "anchors", .value = anchors } });
        }
        var params: std.ArrayList(json.Member) = .empty;
        for (c.node.object) |m| {
            if (eq(u8, m.key, "id") or eq(u8, m.key, "type")) continue;
            try params.append(a, m);
        }
        var parts: std.ArrayList(json.Value) = .empty;
        for (try l.scene.partNames(c)) |pn| {
            var is_zone = false;
            for (c.built.zones) |z| if (eq(u8, z.name, pn)) {
                is_zone = true;
            };
            try parts.append(a, try json.obj(a, &.{
                .{ .key = "name", .value = .{ .string = pn } },
                .{ .key = "kind", .value = .{ .string = if (is_zone) "zone" else "prism" } },
                .{ .key = "anchors", .value = try boxAnchors(a, c, pn) },
            }));
        }
        const b = compBox(c);
        const mat = if (c.world.len > 0) c.world[0].material else "";
        return try json.obj(a, &.{
            .{ .key = "id", .value = .{ .string = id } },
            .{ .key = "type", .value = .{ .string = c.ty.name } },
            .{ .key = "material", .value = .{ .string = mat } },
            .{ .key = "desc", .value = .{ .string = c.built.info } },
            .{ .key = "params", .value = .{ .object = params.items } },
            .{ .key = "instances", .value = .{ .number = @floatFromInt(c.xfs.len) } },
            .{ .key = "z", .value = try json.numArr(a, &.{ c.zs[0][0], c.zs[0][1] }) },
            .{ .key = "bbox", .value = try json.numArr(a, &.{ b.x0, b.y0, b.x1, b.y1 }) },
            .{ .key = "anchors", .value = anchors },
            .{ .key = "parts", .value = .{ .array = parts.items } },
        });
    }
    if (eq(u8, kind, "at")) {
        const vid = (if (q.get("view")) |x| x.str() else null) orelse {
            err.* = .{ .code = "E_PARAM", .message = "inspect at needs \"view\" and \"point\": [x, y]" };
            return null;
        };
        const pv = (if (q.get("point")) |x| x.arr() else null) orelse {
            err.* = .{ .code = "E_PARAM", .message = "inspect at needs \"point\": [x, y] in inches" };
            return null;
        };
        if (pv.len != 2 or units.parseLength(pv[0]) == null or units.parseLength(pv[1]) == null) {
            err.* = .{ .code = "E_PARAM", .message = "inspect at needs \"point\": [x, y] in inches" };
            return null;
        }
        const pt = V2.init(units.parseLength(pv[0]).?, units.parseLength(pv[1]).?);
        const vnode = view_mod.findView(l.doc, vid) orelse {
            err.* = .{ .code = "E_REF_UNKNOWN", .message = try std.fmt.allocPrint(a, "unknown view '{s}'. Views: {s}", .{ vid, scene_mod.joinIds(a, try view_mod.viewIds(a, l.doc)) }) };
            return null;
        };
        var vd = model.Diags.init(a);
        const spec_v = (try view_mod.parse(a, vnode, 0, &vd)) orelse {
            err.* = .{ .code = "E_PARAM", .message = if (vd.list.items.len > 0) vd.list.items[0].message else "view is invalid" };
            return null;
        };
        const spec = try a.create(view_mod.ViewSpec);
        spec.* = (try drawview.resolveSpec(a, l.doc, l.style, l.scene, &spec_v)).*;
        const prisms = try compile_mod.allPrisms(a, l.scene);
        var sec = try section.Section.init(a, l.scene, spec, prisms);
        try sec.build();
        var hits: std.ArrayList(json.Value) = .empty;
        for (prisms, 0..) |p, i| {
            if (sec.cls[i] == .drop or p.kind != .body) continue;
            const reg = try sec.visibleRegion(i);
            if (reg.len == 0) continue;
            if (geom.locateEvenOdd(pt, reg, 1e-9) == .outside) continue;
            const c = &l.scene.comps[p.comp];
            try hits.append(a, try json.obj(a, &.{
                .{ .key = "id", .value = .{ .string = c.id } },
                .{ .key = "src", .value = .{ .string = if (c.arr_count > 1) try std.fmt.allocPrint(a, "{s}#{d}", .{ c.id, p.instance / @as(u32, @intCast(c.xfs.len / c.arr_count)) }) else c.id } },
                .{ .key = "part", .value = if (p.part.len > 0) json.Value{ .string = p.part } else .null },
                .{ .key = "kind", .value = .{ .string = if (sec.cls[i] == .cut) "cut" else "beyond" } },
            }));
        }
        return try json.obj(a, &.{
            .{ .key = "view", .value = .{ .string = vid } },
            .{ .key = "point", .value = try json.numArr(a, &.{ pt.x, pt.y }) },
            .{ .key = "components", .value = .{ .array = hits.items } },
        });
    }
    err.* = .{ .code = "E_PARAM", .message = try std.fmt.allocPrint(a, "unknown query \"{s}\": use summary, component, anchors, at, catalog, doc", .{kind}) };
    return null;
}
