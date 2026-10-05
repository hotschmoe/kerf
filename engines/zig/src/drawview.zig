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
const annot = @import("annot.zig");
const font_mod = @import("font.zig");
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
    for (spec.omit) |o| {
        if (scene.find(o) == null) {
            const ids = try scene.compIds(a);
            diags.add(.@"error", "E_REF_UNKNOWN", view_id, try std.fmt.allocPrint(a, "views/{s}/omit", .{view_id}), "view {s} omits '{s}', which is not a component id. Components: {s}", .{ view_id, o, scene_mod.joinIds(a, ids) });
        }
    }
    const prisms = try compile_mod.viewPrisms(a, scene, spec.omit);
    var items: []const drawing.Item = &.{};
    var scale = spec.scale;
    var bounds = geom.Box{};
    var unverified = false;
    var spec_crop_out = spec.crop;
    var detail = geom.Box{};
    const font = try a.create(font_mod.Font);
    font.* = font_mod.Font.parse(a, font_mod.embedded) catch return null;
    if (spec.kind == .section) {
        var sec = try section.Section.init(a, scene, spec, prisms);
        try sec.build();
        const base = try sec.finish();
        var base_items = try a.dupe(drawing.Item, base);
        var env = annot.Env{ .a = a, .style = st, .font = font, .scene = scene, .spec = spec, .S = scale, .crop = spec.crop, .diags = diags, .landing = .{ .section = &sec } };
        const ann = try annot.annotate(&env, base_items);
        unverified = env.unverified;
        var all: std.ArrayList(drawing.Item) = .empty;
        try all.appendSlice(a, base_items);
        try all.appendSlice(a, ann);
        // title below the lowest annotation
        var tcrop = spec.crop;
        const ab = annot.itemsBox(font, all.items);
        if (!ab.isEmpty() and ab.y0 < tcrop.y0) tcrop.y0 = ab.y0;
        const info = annot.SheetInfo{ .number = spec.number, .title = spec.title, .scale_text = try units.scaleLabel(a, spec.scale_text, if (spec.scale == 0) 0 else scale), .sheet = metaString(doc, "sheet"), .unverified = unverified };
        detail = annot.itemsBox(font, all.items);
        detail.addBox(spec.crop);
        _ = try annot.titleItems(&env, info, tcrop, &all);
        if (env.unknown_glyph) diags.add(.info, "I_GLYPH", view_id, null, "some characters are not in the plotter font and were replaced by '?' (dashes, quotes, x-sign and fractions are folded automatically)", .{});
        items = all.items;
        bounds = annot.itemsBox(font, items);
        bounds.addBox(spec.crop);
        _ = &base_items;
    } else {
        var trial: usize = 0;
        var fitted: f64 = 0;
        while (true) : (trial += 1) {
            var tdiags = model.Diags.init(a);
            const iso = try a.create(@import("iso.zig").Iso);
            iso.* = .{ .a = a };
            const res = try @import("iso.zig").build(iso, scene, spec, st, if (trial == 0) 0 else fitted + 0.5 * @as(f64, @floatFromInt(trial)));
            if (trial == 0) fitted = res.scale;
            scale = res.scale;
            var env = annot.Env{ .a = a, .style = st, .font = font, .scene = scene, .spec = spec, .S = scale, .crop = res.crop, .diags = &tdiags, .landing = .{ .iso = iso } };
            const base_items = try a.dupe(drawing.Item, res.items);
            const ann = try annot.annotate(&env, base_items);
            unverified = env.unverified;
            var all: std.ArrayList(drawing.Item) = .empty;
            try all.appendSlice(a, base_items);
            try all.appendSlice(a, ann);
            var tcrop = res.crop;
            const ab = annot.itemsBox(font, all.items);
            if (!ab.isEmpty() and ab.y0 < tcrop.y0) tcrop.y0 = ab.y0;
            const info = annot.SheetInfo{ .number = spec.number, .title = spec.title, .scale_text = try units.scaleLabel(a, spec.scale_text, if (spec.scale == 0) 0 else scale), .sheet = metaString(doc, "sheet"), .unverified = unverified };
            detail = annot.itemsBox(font, all.items);
            detail.addBox(res.crop);
            _ = try annot.titleItems(&env, info, tcrop, &all);
            if (env.unknown_glyph) tdiags.add(.info, "I_GLYPH", view_id, null, "some characters are not in the plotter font and were replaced by '?' (dashes, quotes, x-sign and fractions are folded automatically)", .{});
            items = all.items;
            bounds = annot.itemsBox(font, items);
            bounds.addBox(res.crop);
            spec_crop_out = res.crop;
            // NTS views grow their (internal) fit factor until notes and title fit the sheet frame
            const pw = (bounds.x1 - bounds.x0) / scale;
            const ph = (bounds.y1 - bounds.y0) / scale;
            const fits = pw <= st.sheet_w_in - 2.0 * st.margin_in + 1e-6 and ph <= st.sheet_h_in - 2.0 * st.margin_in - st.title_block_h_in + 1e-6;
            if (fits or spec.scale != 0 or trial >= 16) {
                try diags.list.appendSlice(a, tdiags.list.items);
                break;
            }
        }
    }
    // fit check against the sheet frame
    {
        const pw = (bounds.x1 - bounds.x0) / scale;
        const ph = (bounds.y1 - bounds.y0) / scale;
        const aw = st.sheet_w_in - 2.0 * st.margin_in;
        const ah = st.sheet_h_in - 2.0 * st.margin_in - st.title_block_h_in;
        if (pw > aw + 1e-6 or ph > ah + 1e-6) {
            diags.addFix(.warning, "W_VIEW_FIT", view_id, try std.fmt.allocPrint(a, "views/{s}/scale", .{view_id}), "view {s} with notes and title needs {d:.2} x {d:.2} paper inches but the sheet area is {d:.2} x {d:.2} at scale {s}", .{ view_id, pw, ph, aw, ah, spec.scale_text }, "use a smaller scale (e.g. 3/4\"=1'-0\"), shrink the crop, or shorten the notes");
        }
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
        .scale_label = try units.scaleLabel(a, spec.scale_text, if (spec.scale == 0) 0 else scale),
        .sheet_no = metaString(doc, "sheet"),
        .date = metaString(doc, "date"),
        .code_basis = try codeBasis(a, doc),
        .crop = spec_crop_out,
        .detail_bounds = detail,
        .has_unverified = unverified,
        .author = metaString(doc, "author"),
        .project = metaString(doc, "project"),
        .doc_title = if (doc.get("title")) |t| (t.str() orelse "") else "",
    };
}
