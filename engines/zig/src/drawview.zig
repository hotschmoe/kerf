//! Orchestrates a view into a Drawing: scene compile, section/iso geometry, annotations, title.

const std = @import("std");
const json = @import("json.zig");
const geom = @import("geom.zig");
const model = @import("model.zig");
const compile_mod = @import("compile.zig");
const scene_mod = @import("scene.zig");
const style_mod = @import("style.zig");
const view_mod = @import("view.zig");
const section = @import("section.zig");
const drawing = @import("drawing.zig");
const units = @import("units.zig");
const Allocator = std.mem.Allocator;

pub const layer_draw_order = [_][]const u8{ "hatch", "beyond", "cut", "steel", "hidden", "break", "notes", "dims", "title" };

pub fn collectLayers(a: Allocator, items: []const drawing.Item, st: *const style_mod.Style) Allocator.Error![]const drawing.LayerDef {
    var out: std.ArrayList(drawing.LayerDef) = .empty;
    for (layer_draw_order) |key| {
        const l = st.layerByKey(key) orelse continue;
        var used = false;
        for (items) |it| if (std.mem.eql(u8, it.layer(), l.name)) {
            used = true;
            break;
        };
        if (used) try out.append(a, .{ .name = l.name, .lineweight_mm = l.lineweight_mm, .linetype = l.linetype });
    }
    return out.items;
}

pub fn itemBounds(items: []const drawing.Item) geom.Box {
    var b = geom.Box{};
    for (items) |it| switch (it) {
        .path => |p| b.addBox(geom.pointsBox(p.pts)),
        .fill => |f| for (f.loops) |l| b.addBox(geom.pointsBox(l)),
        .hatch => |h| for (h.loops) |l| b.addBox(geom.pointsBox(l)),
        .text => {},
    };
    return b;
}

pub fn metaString(doc: json.Value, key: []const u8) []const u8 {
    if (doc.get("meta")) |m| if (m.get(key)) |v| if (v.str()) |s| return s;
    return "";
}

pub fn codeBasis(a: Allocator, doc: json.Value) Allocator.Error![]const u8 {
    if (doc.get("meta")) |m| if (m.get("jurisdiction")) |j| {
        const code = if (j.get("code")) |c| (c.str() orelse "") else "";
        if (code.len > 0) {
            if (j.get("edition")) |e| if (e.num()) |n| return std.fmt.allocPrint(a, "{s} {d}", .{ code, @as(i64, @intFromFloat(n)) });
            return code;
        }
    };
    return "";
}

/// Build the Drawing for `view_id`. Returns null (with diagnostics) if the view does not exist or is invalid.
pub fn build(a: Allocator, doc: json.Value, st: *const style_mod.Style, view_id: []const u8, diags: *model.Diags) Allocator.Error!?drawing.Drawing {
    const vnode = view_mod.findView(doc, view_id) orelse {
        const ids = try view_mod.viewIds(a, doc);
        diags.add(.@"error", "E_REF_UNKNOWN", view_id, "views", "no view with id '{s}'. Views: {s}", .{ view_id, scene_mod.joinIds(a, ids) });
        return null;
    };
    var idx: usize = 0;
    if (doc.get("views")) |vs| if (vs.arr()) |arr| for (arr, 0..) |v, i| {
        if (v.get("id")) |x| if (x.str()) |s| if (std.mem.eql(u8, s, view_id)) {
            idx = i;
        };
    };
    const spec_opt = try view_mod.parse(a, vnode, idx, diags);
    const spec = try a.create(view_mod.ViewSpec);
    spec.* = spec_opt orelse return null;
    const scene = try compile_mod.compile(a, doc, st, diags);
    return buildFromScene(a, doc, st, scene, spec, diags);
}

/// Build a view's Drawing from an already compiled scene. `diags` receives view-level diagnostics.
pub fn buildFromScene(a: Allocator, doc: json.Value, st: *const style_mod.Style, scene: *scene_mod.Scene, spec: *const view_mod.ViewSpec, diags: *model.Diags) Allocator.Error!?drawing.Drawing {
    const view_id = spec.id;
    const prisms = try compile_mod.allPrisms(a, scene);
    var items: []const drawing.Item = &.{};
    var scale = spec.scale;
    var bounds = geom.Box{};
    if (spec.kind == .section) {
        var sec = try section.Section.init(a, scene, spec, prisms);
        try sec.build();
        items = try sec.finish();
        bounds = spec.crop;
        bounds.addBox(itemBounds(items));
    } else {
        diags.add(.info, "I_ISO_PENDING", view_id, "views", "iso view rendering is not implemented in this engine build yet", .{});
        scale = 12;
        bounds = spec.crop;
    }
    const layers = try collectLayers(a, items, st);
    return .{
        .doc = if (doc.get("id")) |x| (x.str() orelse "") else "",
        .view = spec.id,
        .kind = @tagName(spec.kind),
        .scale = scale,
        .bounds = .{ bounds.x0, bounds.y0, bounds.x1, bounds.y1 },
        .items = items,
        .layers = layers,
        .diagnostics = diags.list.items,
        .style = st,
        .number = spec.number,
        .title = spec.title,
        .scale_label = try units.scaleLabel(a, spec.scale_text, scale),
        .sheet_no = metaString(doc, "sheet"),
        .date = metaString(doc, "date"),
        .code_basis = try codeBasis(a, doc),
        .crop = spec.crop,
        .detail_bounds = bounds,
    };
}
