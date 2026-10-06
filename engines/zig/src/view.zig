//! View specs (SPEC 6): parsing and validation of `views[]`.

const std = @import("std");
const json = @import("json.zig");
const geom = @import("geom.zig");
const model = @import("model.zig");
const units = @import("units.zig");
const limits = @import("limits.zig");
const Allocator = std.mem.Allocator;

pub const Kind = enum { section, iso };
pub const From = enum { front_right, front_left, back_right, back_left };
pub const NotesSide = enum { right, left, both };

pub const ViewSpec = struct {
    id: []const u8,
    kind: Kind,
    number: []const u8,
    title: []const u8,
    scale_text: []const u8,
    /// Model inches per paper inch; 0 = NTS (fit).
    scale: f64,
    cut_z: f64,
    crop: geom.Box,
    /// The author set `crop` / `scale` explicitly (SPEC 19). When false, drawview auto-fits them.
    has_crop: bool,
    has_scale: bool = false,
    from: From,
    cutaway: bool,
    notes_side: NotesSide,
    annotations: []const json.Value,
    omit: []const []const u8,
    node: json.Value,
};

pub fn findView(doc: json.Value, id: []const u8) ?json.Value {
    const vs = (doc.get("views") orelse return null).arr() orelse return null;
    for (vs) |v| {
        if (v.get("id")) |i| if (i.str()) |s| if (std.mem.eql(u8, s, id)) return v;
    }
    return null;
}

pub fn viewIds(a: Allocator, doc: json.Value) Allocator.Error![]const []const u8 {
    var out: std.ArrayList([]const u8) = .empty;
    if (doc.get("views")) |vs| if (vs.arr()) |arr| for (arr) |v| {
        if (v.get("id")) |i| if (i.str()) |s| try out.append(a, s);
    };
    return out.items;
}

fn parseRange(v: ?json.Value) ?[2]f64 {
    const arr = (v orelse return null).arr() orelse return null;
    if (arr.len != 2) return null;
    const a = units.parseLength(arr[0]) orelse return null;
    const b = units.parseLength(arr[1]) orelse return null;
    return .{ a, b };
}

pub fn parse(a: Allocator, node: json.Value, index: usize, diags: *model.Diags) Allocator.Error!?ViewSpec {
    const id = (if (node.get("id")) |x| x.str() else null) orelse {
        diags.add(.@"error", "E_PARAM", null, try std.fmt.allocPrint(a, "views/{d}", .{index}), "view {d} needs a string \"id\"", .{index});
        return null;
    };
    const base = try std.fmt.allocPrint(a, "views/{s}", .{id});
    var ok = true;
    const kind_s = (if (node.get("kind")) |x| x.str() else null) orelse "section";
    var kind: Kind = .section;
    if (std.mem.eql(u8, kind_s, "section")) {
        kind = .section;
    } else if (std.mem.eql(u8, kind_s, "iso")) {
        kind = .iso;
    } else {
        diags.add(.@"error", "E_PARAM", id, try std.fmt.allocPrint(a, "{s}/kind", .{base}), "view kind must be \"section\" or \"iso\" (got \"{s}\")", .{kind_s});
        ok = false;
    }
    var has_scale = false;
    if (node.get("scale")) |sv| if (sv != .null) {
        has_scale = true;
    };
    const scale_text = (if (node.get("scale")) |x| x.str() else null) orelse if (kind == .iso) "NTS" else "1\"=1'-0\"";
    var scale: f64 = 12;
    if (units.parseScale(scale_text)) |s| {
        scale = s.factor;
        if (kind == .section and s.factor == 0) {
            diags.add(.@"error", "E_PARAM", id, try std.fmt.allocPrint(a, "{s}/scale", .{base}), "section views need a numeric scale such as \"1-1/2\\\"=1'-0\\\"\", \"1\\\"=1'-0\\\"\", \"3/4\\\"=1'-0\\\"\" or \"1:20\" (NTS is for iso views)", .{});
            ok = false;
        }
    } else if (units.parseScaleAny(scale_text)) |s| {
        diags.add(.@"error", "E_PARAM", id, try std.fmt.allocPrint(a, "{s}/scale", .{base}), "scale \"{s}\" means {s} model inches per paper inch; the supported range is {d} to {d} (from a 10x enlargement to 1:10000). Use e.g. \"3\\\"=1'-0\\\"\" (4), \"1\\\"=1'-0\\\"\" (12) or \"1:20\"", .{ scale_text, model.numText(a, s.factor), units.min_scale_factor, units.max_scale_factor });
        ok = false;
    } else {
        diags.add(.@"error", "E_PARAM", id, try std.fmt.allocPrint(a, "{s}/scale", .{base}), "unrecognised scale \"{s}\". Use e.g. \"3\\\"=1'-0\\\"\", \"1-1/2\\\"=1'-0\\\"\", \"1\\\"=1'-0\\\"\", \"3/4\\\"=1'-0\\\"\", \"1/2\\\"=1'-0\\\"\", \"3/8\\\"=1'-0\\\"\", \"1/4\\\"=1'-0\\\"\", \"1:N\" or \"NTS\"", .{scale_text});
        ok = false;
    }
    var crop = geom.Box{};
    var has_crop = false;
    if (node.get("crop")) |cv| if (cv != .null) {
        const xr = parseRange(cv.get("x"));
        const yr = parseRange(cv.get("y"));
        if (xr != null and yr != null and xr.?[0] < xr.?[1] and yr.?[0] < yr.?[1]) {
            crop = .{ .x0 = xr.?[0], .x1 = xr.?[1], .y0 = yr.?[0], .y1 = yr.?[1] };
            has_crop = true;
        } else {
            diags.add(.@"error", "E_PARAM", id, try std.fmt.allocPrint(a, "{s}/crop", .{base}), "crop must be {{\"x\": [x0, x1], \"y\": [y0, y1]}} with x0 < x1 and y0 < y1, every value a length of at most {d} inches in magnitude (got {s})", .{ limits.max_coord_in, model.kindOrText(a, cv) });
            ok = false;
        }
    };
    var cut_z: f64 = 0;
    if (node.get("cut_z")) |cz| if (cz != .null) {
        if (units.parseLength(cz)) |z| cut_z = z else {
            diags.add(.@"error", "E_PARAM", id, try std.fmt.allocPrint(a, "{s}/cut_z", .{base}), "cut_z must be a length", .{});
            ok = false;
        }
    };
    var from: From = .front_right;
    if (node.get("from")) |fv| if (fv.str()) |s| {
        if (std.meta.stringToEnum(From, s)) |f| from = f else {
            diags.add(.@"error", "E_PARAM", id, try std.fmt.allocPrint(a, "{s}/from", .{base}), "iso 'from' must be front_right, front_left, back_right or back_left (got \"{s}\")", .{s});
            ok = false;
        }
    };
    var notes_side: NotesSide = .both;
    if (node.get("notes_side")) |fv| if (fv.str()) |s| {
        if (std.meta.stringToEnum(NotesSide, s)) |f| notes_side = f else {
            diags.add(.@"error", "E_PARAM", id, try std.fmt.allocPrint(a, "{s}/notes_side", .{base}), "notes_side must be right, left or both (got \"{s}\")", .{s});
            ok = false;
        }
    };
    var cutaway = false;
    if (node.get("cutaway")) |cv| if (cv == .bool) {
        cutaway = cv.bool;
    };
    if (!ok) return null;
    var omit: std.ArrayList([]const u8) = .empty;
    if (node.get("omit")) |ov| if (ov.arr()) |oa| for (oa) |x| {
        if (x.str()) |sx| try omit.append(a, sx);
    };
    const anns: []const json.Value = if (node.get("annotations")) |av| (av.arr() orelse &.{}) else &.{};
    if (anns.len > limits.max_annotations_per_view) {
        diags.addFix(.@"error", "E_LIMIT", id, try std.fmt.allocPrint(a, "{s}/annotations", .{base}), "{s}", .{try limits.message(a, try std.fmt.allocPrint(a, "annotations (notes, dims, labels) in view {s}", .{id}), anns.len, limits.max_annotations_per_view, "Nothing was laid out: the router cost grows with the square of the note count.")}, "keep the most important notes, or move part of the detail to a second view (a new entry in views[] with its own crop)");
        return null;
    }
    return .{
        .id = id,
        .kind = kind,
        .number = if (node.get("number")) |x| (x.str() orelse "") else try std.fmt.allocPrint(a, "{d}", .{index + 1}),
        .title = if (node.get("title")) |x| (x.str() orelse "") else "",
        .scale_text = scale_text,
        .scale = scale,
        .cut_z = cut_z,
        .crop = crop,
        .has_crop = has_crop,
        .has_scale = has_scale,
        .from = from,
        .cutaway = cutaway,
        .notes_side = notes_side,
        .annotations = anns,
        .omit = omit.items,
        .node = node,
    };
}
