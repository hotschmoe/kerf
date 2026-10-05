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

pub const layer_draw_order = [_][]const u8{ "hatch", "beyond", "cut", "steel", "rebar", "hidden", "break", "notes", "dims", "title" };

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
        .text, .region => {},
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

const FitInfo = struct { ann: []const annot.AnnBox = &.{}, title: geom.Box = .{} };

const standard_scales = [_]f64{ 4, 8, 12, 16, 24, 32, 48 };

const Culprit = struct { name: []const u8, ext: f64, kind: ?annot.AnnKind, id: []const u8 };

/// Who extends furthest beyond the crop on one side (extents in model units, > 0 only).
fn sideCulprit(a: Allocator, fit: FitInfo, crop: geom.Box, side: u8) Allocator.Error!Culprit {
    var best = Culprit{ .name = "the crop itself", .ext = 0, .kind = null, .id = "" };
    var notes_ext: f64 = 0;
    var nnotes: usize = 0;
    for (fit.ann) |b| {
        const e = sideExt(b.box, crop, side);
        if (b.kind == .note) {
            nnotes += 1;
            notes_ext = @max(notes_ext, e);
            continue;
        }
        if (e > best.ext + 1e-9) best = .{ .name = try std.fmt.allocPrint(a, "{s} '{s}'", .{ if (b.kind == .dim) "dimension" else "label", b.id }), .ext = e, .kind = b.kind, .id = b.id };
    }
    if (notes_ext > best.ext + 1e-9) best = .{ .name = try std.fmt.allocPrint(a, "the notes column ({d} notes)", .{nnotes}), .ext = notes_ext, .kind = .note, .id = "" };
    const te = sideExt(fit.title, crop, side);
    if (te > best.ext + 1e-9) best = .{ .name = "the title block", .ext = te, .kind = null, .id = "title" };
    return best;
}

fn sideExt(b: geom.Box, crop: geom.Box, side: u8) f64 {
    if (b.isEmpty()) return 0;
    return switch (side) {
        'l' => @max(0, crop.x0 - b.x0),
        'r' => @max(0, b.x1 - crop.x1),
        't' => @max(0, b.y1 - crop.y1),
        else => @max(0, crop.y0 - b.y0),
    };
}

fn fmtNum(a: Allocator, n: f64) Allocator.Error![]const u8 {
    var buf: [40]u8 = undefined;
    return a.dupe(u8, json.fmtNumber(&buf, @round(n * 10) / 10));
}

/// W_VIEW_FIT (SPEC 18): overflow per edge, the culprit on each overflowing axis and the smallest fix first.
fn viewFit(a: Allocator, st: *const style_mod.Style, spec: *const view_mod.ViewSpec, scale: f64, crop: geom.Box, bounds: geom.Box, fit: FitInfo, aw: f64, ah: f64, diags: *model.Diags) Allocator.Error!void {
    const view_id = spec.id;
    const pw = (bounds.x1 - bounds.x0) / scale;
    const ph = (bounds.y1 - bounds.y0) / scale;
    const ow = @max(0, pw - aw);
    const oh = @max(0, ph - ah);
    const cw = crop.width() / scale;
    const ch = crop.height() / scale;
    var msg: std.ArrayList(u8) = .empty;
    try msg.print(a, "view {s} with notes, dimensions and title needs {d:.2} x {d:.2} paper inches but the sheet area is {d:.2} x {d:.2} at scale {s}. Overflow by edge (sheet centered on the view): left {d:.2}\", right {d:.2}\", top {d:.2}\", bottom {d:.2}\".", .{ view_id, pw, ph, aw, ah, spec.scale_text, ow / 2, ow / 2, oh / 2, oh / 2 });
    var fixes: std.ArrayList(u8) = .empty;
    var nfix: usize = 0;
    const lc = try sideCulprit(a, fit, crop, 'l');
    const rc = try sideCulprit(a, fit, crop, 'r');
    const tc = try sideCulprit(a, fit, crop, 't');
    const bc = try sideCulprit(a, fit, crop, 'b');
    if (ow > 0) {
        try msg.print(a, " Width: crop {d:.2}\" + left side {d:.2}\" ({s}) + right side {d:.2}\" ({s}).", .{ cw, lc.ext / scale, lc.name, rc.ext / scale, rc.name });
        const big = if (lc.ext >= rc.ext) lc else rc;
        const need = ow * scale; // model inches to remove
        if (cw > ow + 0.5) {
            nfix += 1;
            try fixes.print(a, "{d}) narrow crop.x by {s} in (now [{s}, {s}])", .{ nfix, try fmtNum(a, need), try fmtNum(a, crop.x0), try fmtNum(a, crop.x1) });
        }
        if (big.kind) |k| switch (k) {
            .dim => for (fit.ann) |b| if (b.kind == .dim and std.mem.eql(u8, b.id, big.id)) {
                const sg: f64 = if (b.offset >= 0) 1 else -1;
                const no = b.offset - sg * @min(need, @abs(b.offset) - 1);
                nfix += 1;
                try fixes.print(a, "{s}{d}) change dim '{s}' offset from {s} to {s}", .{ if (nfix > 1) "; " else "", nfix, b.id, try fmtNum(a, b.offset), try fmtNum(a, no) });
            },
            .note => if (spec.notes_side == .both) {
                nfix += 1;
                try fixes.print(a, "{s}{d}) set notes_side to \"{s}\" to drop the {s} notes column (saves about {d:.2}\" if the height allows)", .{ if (nfix > 1) "; " else "", nfix, if (lc.ext >= rc.ext) "right" else "left", if (lc.ext >= rc.ext) "left" else "right", @min(lc.ext, rc.ext) / scale });
            },
            else => {},
        };
        if (big.kind != null and big.kind.? == .note) {
            nfix += 1;
            try fixes.print(a, "{s}{d}) shorten the longest notes (they wrap at {d} characters)", .{ if (nfix > 1) "; " else "", nfix, @as(usize, @intFromFloat(st.wrap_chars)) });
        }
    }
    if (oh > 0) {
        try msg.print(a, " Height: crop {d:.2}\" + above {d:.2}\" ({s}) + below {d:.2}\" ({s}).", .{ ch, tc.ext / scale, tc.name, bc.ext / scale, bc.name });
        const need = oh * scale;
        if (ch > oh + 0.5) {
            nfix += 1;
            try fixes.print(a, "{s}{d}) shorten crop.y by {s} in (now [{s}, {s}])", .{ if (nfix > 1) "; " else "", nfix, try fmtNum(a, need), try fmtNum(a, crop.y0), try fmtNum(a, crop.y1) });
        }
        const big = if (tc.ext >= bc.ext) tc else bc;
        if (big.kind) |k| switch (k) {
            .dim => for (fit.ann) |b| if (b.kind == .dim and std.mem.eql(u8, b.id, big.id)) {
                const sg: f64 = if (b.offset >= 0) 1 else -1;
                const no = b.offset - sg * @min(need, @abs(b.offset) - 1);
                nfix += 1;
                try fixes.print(a, "{s}{d}) change dim '{s}' offset from {s} to {s}", .{ if (nfix > 1) "; " else "", nfix, b.id, try fmtNum(a, b.offset), try fmtNum(a, no) });
            },
            .note => if (spec.notes_side != .both) {
                nfix += 1;
                try fixes.print(a, "{s}{d}) set notes_side to \"both\" to split the notes over two columns (adds about {d:.2}\" of width)", .{ if (nfix > 1) "; " else "", nfix, @as(f64, 0.0) + (st.gutter_in + st.wrap_chars * st.text_height_in * 0.75) });
            },
            else => {},
        };
        if (big.kind == null and std.mem.eql(u8, big.id, "title")) {
            nfix += 1;
            try fixes.print(a, "{s}{d}) the title block sits below the lowest annotation: shorten the notes column or the crop", .{ if (nfix > 1) "; " else "", nfix });
        }
    }
    // a scale change only when the overflow is greater than 15% of the frame
    if (ow > 0.15 * aw or oh > 0.15 * ah) {
        const extra_w = pw - cw;
        const extra_h = ph - ch;
        var pick: ?f64 = null;
        for (standard_scales) |s2| {
            if (s2 <= scale) continue;
            if (crop.width() / s2 + extra_w <= aw and crop.height() / s2 + extra_h <= ah) {
                pick = s2;
                break;
            }
        }
        const sc: f64 = pick orelse std.math.ceil((@max(crop.width() / @max(aw - extra_w, 0.5), crop.height() / @max(ah - extra_h, 0.5))));
        nfix += 1;
        const lab = if (pick != null) try units.scaleLabel(a, "", sc) else try std.fmt.allocPrint(a, "1:{d:.0}", .{sc});
        try fixes.print(a, "{s}{d}) use a smaller scale (e.g. {s})", .{ if (nfix > 1) "; " else "", nfix, lab });
    }
    if (nfix == 0) try fixes.appendSlice(a, "shorten the notes or shrink the crop");
    diags.addFix(.warning, "W_VIEW_FIT", view_id, try std.fmt.allocPrint(a, "views/{s}", .{view_id}), "{s}", .{msg.items}, fixes.items);
}

/// One complete section pass at a fixed (resolved) crop and scale: geometry, annotations, title.
const SecPass = struct {
    items: []const drawing.Item,
    bounds: geom.Box,
    detail: geom.Box,
    fit: FitInfo,
    unverified: bool,
};

fn sectionPass(a: Allocator, st: *const style_mod.Style, scene: *scene_mod.Scene, spec: *const view_mod.ViewSpec, prisms: []const model.Prism, font: *const font_mod.Font, doc: json.Value, diags: *model.Diags) Allocator.Error!SecPass {
    const scale = spec.scale;
    const sec = try a.create(section.Section);
    sec.* = try section.Section.init(a, scene, spec, prisms);
    try sec.build();
    const base_items = try a.dupe(drawing.Item, try sec.finish());
    const regions = try sec.regionItems();
    var env = annot.Env{ .a = a, .style = st, .font = font, .scene = scene, .spec = spec, .S = scale, .crop = spec.crop, .diags = diags, .landing = .{ .section = sec } };
    const ann = try annot.annotate(&env, base_items);
    const unverified = env.unverified;
    var all: std.ArrayList(drawing.Item) = .empty;
    try all.appendSlice(a, base_items);
    try all.appendSlice(a, regions);
    try all.appendSlice(a, ann);
    // title below the lowest annotation
    var tcrop = spec.crop;
    const ab = annot.itemsBox(font, all.items);
    if (!ab.isEmpty() and ab.y0 < tcrop.y0) tcrop.y0 = ab.y0;
    const info = annot.SheetInfo{ .number = spec.number, .title = spec.title, .scale_text = try units.scaleLabel(a, spec.scale_text, if (spec.scale == 0) 0 else scale), .sheet = metaString(doc, "sheet"), .unverified = unverified };
    var detail = annot.itemsBox(font, all.items);
    detail.addBox(spec.crop);
    const title_from = all.items.len;
    _ = try annot.titleItems(&env, info, tcrop, &all);
    const fit = FitInfo{ .ann = env.ann_boxes.items, .title = annot.itemsBox(font, all.items[title_from..]) };
    if (env.unknown_glyph) diags.add(.info, "I_GLYPH", spec.id, null, "some characters are not in the plotter font and were replaced by '?' (dashes, quotes, x-sign and fractions are folded automatically)", .{});
    var bounds = annot.itemsBox(font, all.items);
    bounds.addBox(spec.crop);
    return .{ .items = all.items, .bounds = bounds, .detail = detail, .fit = fit, .unverified = unverified };
}

/// Auto crop (SPEC 19): bounding box of the non-fill components visible in the section, plus 6".
/// Fills (earth, gravel, sand, compacted fill) are left out and get clipped by the resulting crop.
pub fn autoCrop(prisms: []const model.Prism, cut_z: f64) geom.Box {
    var solid = geom.Box{};
    var any = geom.Box{};
    for (prisms) |p| {
        const is_cut = p.z0 < cut_z - 1e-9 and p.z1 > cut_z + 1e-9;
        if (!is_cut and p.z1 > cut_z + 1e-9) continue; // dropped: above the cut plane
        var b = geom.Box{};
        for (p.loops) |l| b.addBox(geom.pointsBox(l));
        b.addBox(geom.pointsBox(p.line_pts));
        b.addBox(geom.pointsBox(p.centerline));
        if (b.isEmpty()) continue;
        any.addBox(b);
        if (!section.isFillMaterial(p.material)) solid.addBox(b);
    }
    var b = if (solid.isEmpty()) any else solid;
    if (b.isEmpty()) b = .{ .x0 = -6, .y0 = -6, .x1 = 6, .y1 = 6 };
    const m = 6.0;
    return .{ .x0 = b.x0 - m, .y0 = b.y0 - m, .x1 = b.x1 + m, .y1 = b.y1 + m };
}

const SecResolved = struct { spec: *const view_mod.ViewSpec, pass: SecPass };

/// Resolve an omitted crop and/or scale (SPEC 19) and run the section pass. The scale is the first of the
/// standard scales (largest first) at which view + notes + dimensions + title fit the sheet frame; every
/// candidate is evaluated in order, so the result is stable. Explicit crop and scale are used as given.
fn resolveSection(a: Allocator, st: *const style_mod.Style, scene: *scene_mod.Scene, spec: *const view_mod.ViewSpec, prisms: []const model.Prism, font: *const font_mod.Font, doc: json.Value, diags: *model.Diags) Allocator.Error!SecResolved {
    const rs0 = try a.create(view_mod.ViewSpec);
    rs0.* = spec.*;
    if (!spec.has_crop) rs0.crop = autoCrop(prisms, spec.cut_z);
    if (spec.has_scale) return .{ .spec = rs0, .pass = try sectionPass(a, st, scene, rs0, prisms, font, doc, diags) };
    const aw = st.sheet_w_in - 2.0 * st.margin_in;
    const ah = st.sheet_h_in - 2.0 * st.margin_in - st.title_block_h_in;
    var last: ?SecResolved = null;
    var last_diags: model.Diags = model.Diags.init(a);
    for (standard_scales, 0..) |sc, k| {
        const is_last = k + 1 == standard_scales.len;
        // the crop alone must fit (necessary condition; skips hopeless candidates without a full pass)
        if (!is_last and (rs0.crop.width() / sc > aw + 1e-6 or rs0.crop.height() / sc > ah + 1e-6)) continue;
        const rs = try a.create(view_mod.ViewSpec);
        rs.* = rs0.*;
        rs.scale = sc;
        rs.scale_text = try units.scaleLabel(a, "", sc);
        var td = model.Diags.init(a);
        const pass = try sectionPass(a, st, scene, rs, prisms, font, doc, &td);
        last = .{ .spec = rs, .pass = pass };
        last_diags = td;
        const pw = (pass.bounds.x1 - pass.bounds.x0) / sc;
        const ph = (pass.bounds.y1 - pass.bounds.y0) / sc;
        if (pw <= aw + 1e-6 and ph <= ah + 1e-6) break;
    }
    try diags.list.appendSlice(a, last_diags.list.items);
    return last.?;
}

/// The view spec with an omitted crop/scale resolved (what the drawing of this view actually uses).
/// Section views only; other views and fully explicit specs come back unchanged.
pub fn resolveSpec(a: Allocator, doc: json.Value, st: *const style_mod.Style, scene: *scene_mod.Scene, spec: *const view_mod.ViewSpec) Allocator.Error!*const view_mod.ViewSpec {
    if (spec.kind != .section or (spec.has_crop and spec.has_scale)) return spec;
    const prisms = try compile_mod.viewPrisms(a, scene, spec.omit);
    const font = try a.create(font_mod.Font);
    font.* = font_mod.Font.parse(a, font_mod.embedded) catch return spec;
    var d = model.Diags.init(a);
    return (try resolveSection(a, st, scene, spec, prisms, font, doc, &d)).spec;
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
    var fit = FitInfo{};
    var resolved_spec: *const view_mod.ViewSpec = spec;
    const font = try a.create(font_mod.Font);
    font.* = font_mod.Font.parse(a, font_mod.embedded) catch return null;
    if (spec.kind == .section) {
        const rs = try resolveSection(a, st, scene, spec, prisms, font, doc, diags);
        scale = rs.spec.scale;
        spec_crop_out = rs.spec.crop;
        const pass = rs.pass;
        unverified = pass.unverified;
        items = pass.items;
        bounds = pass.bounds;
        detail = pass.detail;
        fit = pass.fit;
        resolved_spec = rs.spec;
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
            try all.appendSlice(a, try iso.regionItems());
            try all.appendSlice(a, ann);
            var tcrop = res.crop;
            const ab = annot.itemsBox(font, all.items);
            if (!ab.isEmpty() and ab.y0 < tcrop.y0) tcrop.y0 = ab.y0;
            const info = annot.SheetInfo{ .number = spec.number, .title = spec.title, .scale_text = try units.scaleLabel(a, spec.scale_text, if (spec.scale == 0) 0 else scale), .sheet = metaString(doc, "sheet"), .unverified = unverified };
            detail = annot.itemsBox(font, all.items);
            detail.addBox(res.crop);
            const title_from = all.items.len;
            _ = try annot.titleItems(&env, info, tcrop, &all);
            fit = .{ .ann = env.ann_boxes.items, .title = annot.itemsBox(font, all.items[title_from..]) };
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
        if (pw > aw + 1e-6 or ph > ah + 1e-6) try viewFit(a, st, resolved_spec, scale, spec_crop_out, bounds, fit, aw, ah, diags);
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
