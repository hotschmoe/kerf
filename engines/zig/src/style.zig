//! Kerf Style (`*.kerfstyle.json`, SPEC 7): pens, materials, hatch patterns, text/notes/dims
//! settings, layers, sheet. The default style is embedded; a user style is merged over it
//! (RFC 7396) so partial styles work.

const std = @import("std");
const json = @import("json.zig");
const Allocator = std.mem.Allocator;

pub const default_json = @embedFile("kerf_style_json");

pub const Pen = struct {
    name: []const u8,
    width_mm: f64,
    dash_mm: ?[]const f64 = null,
};

pub const HatchSpec = struct {
    pattern: []const u8,
    scale: f64 = 1,
    angle: f64 = 0,
};

/// Wood grain on lumber cut lengthwise (SPEC 19): a straight-line pattern (also the DXF fallback) made wavy by the engine.
pub const GrainSpec = struct {
    pattern: []const u8,
    scale: f64 = 1,
    /// Wave amplitude and wavelength, paper inches.
    amplitude: f64 = 0.010,
    wavelength: f64 = 1.1,
};

pub const CutMark = enum { none, x, diagonal };

pub const Material = struct {
    name: []const u8,
    hatch: []const HatchSpec = &.{},
    grain: ?GrainSpec = null,
    cut_mark: CutMark = .none,
    fill: bool = false,
    batt: bool = false,
    pen: ?[]const u8 = null,
    color3d: []const u8 = "#A0A0A0",
    layer: ?[]const u8 = null,
};

pub const Family = struct {
    angle: f64,
    x0: f64,
    y0: f64,
    dx: f64,
    dy: f64,
    dashes: []const f64,
};

pub const Pattern = struct {
    name: []const u8,
    families: []const Family,
};

pub const Layer = struct {
    key: []const u8,
    name: []const u8,
    lineweight_mm: f64,
    linetype: []const u8 = "CONTINUOUS",
};

pub const Style = struct {
    id: []const u8,
    pens: []const Pen,
    materials: []const Material,
    patterns: []const Pattern,
    layers: []const Layer,

    // text
    text_height_in: f64 = 0.09375,
    title_height_in: f64 = 0.15625,
    label_height_in: f64 = 0.078125,
    text_case_upper: bool = true,
    line_spacing: f64 = 1.6,
    dxf_style: []const u8 = "KERF",
    dxf_font: []const u8 = "romans.shx",

    // notes
    notes_mode_keynote: bool = false,
    wrap_chars: f64 = 28,
    gutter_in: f64 = 0.375,
    shoulder_in: f64 = 0.125,
    note_gap_in: f64 = 0.0625,
    arrow: []const u8 = "closed_filled",
    arrow_len_in: f64 = 0.09375,
    arrow_width_in: f64 = 0.03125,
    keynote_tag: []const u8 = "hex",

    // dims
    dim_terminator: []const u8 = "tick",
    tick_len_in: f64 = 0.0625,
    ext_gap_in: f64 = 0.0625,
    ext_over_in: f64 = 0.0625,
    dim_text_gap_in: f64 = 0.046875,
    dim_precision: f64 = 16,

    // citations
    cite_format: []const u8 = " ({code} {section})",
    cite_flag_unverified: bool = true,
    cite_flag: []const u8 = "*",
    cite_footnote: []const u8 = "* CODE REFERENCE NOT VERIFIED BY DESIGNER",

    // sheet
    sheet_w_in: f64 = 11,
    sheet_h_in: f64 = 8.5,
    margin_in: f64 = 0.375,
    title_block: []const u8 = "kerf-strip",
    title_block_h_in: f64 = 0.75,
    firm: []const u8 = "",
    project: []const u8 = "",

    // break lines
    break_zig_in: f64 = 0.125,
    break_period_in: f64 = 0.5,
    break_overshoot_in: f64 = 0.0625,

    // 3D colors
    color_cut_cap: []const u8 = "#E9D9A6",
    color_edge: []const u8 = "#1A1A1A",
    color_background: []const u8 = "#F2EFE6",

    pub fn pen(self: *const Style, name: []const u8) ?Pen {
        for (self.pens) |p| if (std.mem.eql(u8, p.name, name)) return p;
        return null;
    }
    pub fn penWidthMm(self: *const Style, name: []const u8) f64 {
        return if (self.pen(name)) |p| p.width_mm else 0.25;
    }
    pub fn material(self: *const Style, name: []const u8) ?*const Material {
        for (self.materials) |*m| if (std.mem.eql(u8, m.name, name)) return m;
        return null;
    }
    pub fn pattern(self: *const Style, name: []const u8) ?*const Pattern {
        for (self.patterns) |*p| if (std.mem.eql(u8, p.name, name)) return p;
        return null;
    }
    pub fn layerByKey(self: *const Style, key: []const u8) ?Layer {
        for (self.layers) |l| if (std.mem.eql(u8, l.key, key)) return l;
        return null;
    }
    /// Layer name for a pen (see `layerKeyForPen`).
    pub fn layerForPen(self: *const Style, pen_name: []const u8) []const u8 {
        const key = layerKeyForPen(pen_name);
        return if (self.layerByKey(key)) |l| l.name else "0";
    }
};

/// Which style layer an item drawn with `pen` belongs to.
pub fn layerKeyForPen(pen_name: []const u8) []const u8 {
    const eq = std.mem.eql;
    if (eq(u8, pen_name, "cut") or eq(u8, pen_name, "profile") or eq(u8, pen_name, "membrane") or eq(u8, pen_name, "vapor")) return "cut";
    if (eq(u8, pen_name, "beyond")) return "beyond";
    if (eq(u8, pen_name, "hidden")) return "hidden";
    if (eq(u8, pen_name, "hatch")) return "hatch";
    if (eq(u8, pen_name, "rebar") or eq(u8, pen_name, "steel")) return "steel";
    if (eq(u8, pen_name, "anno")) return "notes";
    if (eq(u8, pen_name, "dim")) return "dims";
    if (eq(u8, pen_name, "break")) return "break";
    if (eq(u8, pen_name, "title") or eq(u8, pen_name, "frame")) return "title";
    return "cut";
}

fn numOr(v: ?json.Value, d: f64) f64 {
    if (v) |x| if (x.num()) |n| return n;
    return d;
}
fn strOr(v: ?json.Value, d: []const u8) []const u8 {
    if (v) |x| if (x.str()) |s| return s;
    return d;
}

pub const StyleError = error{ BadStyle, OutOfMemory };

/// Build a Style from the embedded default merged with `user` (may be null).
pub fn load(a: Allocator, user: ?json.Value) StyleError!Style {
    var err: json.ParseError = undefined;
    const base = (try json.parse(a, default_json, &err)) orelse return error.BadStyle;
    var root = base;
    if (user) |u| {
        if (u == .object and u.object.len > 0) root = try json.mergePatch(a, base, u);
    }
    return fromValue(a, root);
}

pub fn fromValue(a: Allocator, root: json.Value) StyleError!Style {
    var s: Style = undefined;
    s.id = strOr(root.get("id"), "custom");

    // pens
    {
        const pv = root.get("pens") orelse return error.BadStyle;
        if (pv != .object) return error.BadStyle;
        const out = try a.alloc(Pen, pv.object.len);
        for (pv.object, 0..) |m, i| {
            var dash: ?[]const f64 = null;
            if (m.value.get("dash_mm")) |d| if (d == .array and d.array.len > 0) {
                const dd = try a.alloc(f64, d.array.len);
                for (d.array, 0..) |x, k| dd[k] = x.num() orelse 1;
                dash = dd;
            };
            out[i] = .{ .name = m.key, .width_mm = numOr(m.value.get("width_mm"), 0.25), .dash_mm = dash };
        }
        s.pens = out;
    }
    // materials
    {
        const mv = root.get("materials") orelse return error.BadStyle;
        if (mv != .object) return error.BadStyle;
        const out = try a.alloc(Material, mv.object.len);
        for (mv.object, 0..) |m, i| {
            var mat = Material{ .name = m.key };
            if (m.value.get("hatch")) |h| if (h == .array) {
                const hs = try a.alloc(HatchSpec, h.array.len);
                for (h.array, 0..) |x, k| hs[k] = .{
                    .pattern = strOr(x.get("pattern"), "ANSI31"),
                    .scale = numOr(x.get("scale"), 1),
                    .angle = numOr(x.get("angle"), 0),
                };
                mat.hatch = hs;
            };
            if (m.value.get("grain")) |g| if (g == .object) {
                mat.grain = .{
                    .pattern = strOr(g.get("pattern"), "KERF-GRAIN"),
                    .scale = numOr(g.get("scale"), 1),
                    .amplitude = numOr(g.get("amplitude"), 0.010),
                    .wavelength = numOr(g.get("wavelength"), 1.1),
                };
            };
            if (m.value.get("cut_mark")) |cm| if (cm.str()) |t| {
                if (std.mem.eql(u8, t, "x")) mat.cut_mark = .x else if (std.mem.eql(u8, t, "diagonal")) mat.cut_mark = .diagonal;
            };
            if (m.value.get("fill")) |f| if (f == .bool) {
                mat.fill = f.bool;
            };
            if (m.value.get("batt")) |f| if (f == .bool) {
                mat.batt = f.bool;
            };
            if (m.value.get("pen")) |p| mat.pen = p.str();
            if (m.value.get("color3d")) |c| mat.color3d = c.str() orelse mat.color3d;
            if (m.value.get("layer")) |c| mat.layer = c.str();
            out[i] = mat;
        }
        s.materials = out;
    }
    // patterns
    {
        var list: std.ArrayList(Pattern) = .empty;
        if (root.get("patterns")) |pv| if (pv == .object) {
            for (pv.object) |m| {
                if (m.key.len > 0 and m.key[0] == '_') continue;
                const fams = m.value.arr() orelse continue;
                const fo = try a.alloc(Family, fams.len);
                var nf: usize = 0;
                for (fams) |f| {
                    const arr = f.arr() orelse continue;
                    if (arr.len < 5) continue;
                    const dashes = try a.alloc(f64, arr.len - 5);
                    for (arr[5..], 0..) |d, k| dashes[k] = d.num() orelse 0;
                    fo[nf] = .{
                        .angle = numOr(arr[0], 0),
                        .x0 = numOr(arr[1], 0),
                        .y0 = numOr(arr[2], 0),
                        .dx = numOr(arr[3], 0),
                        .dy = numOr(arr[4], 0.125),
                        .dashes = dashes,
                    };
                    nf += 1;
                }
                try list.append(a, .{ .name = m.key, .families = fo[0..nf] });
            }
        };
        s.patterns = list.items;
    }
    // layers
    {
        var list: std.ArrayList(Layer) = .empty;
        if (root.get("layers")) |lv| if (lv == .object) {
            for (lv.object) |m| {
                switch (m.value) {
                    .string => |nm| try list.append(a, .{ .key = m.key, .name = nm, .lineweight_mm = 0.25 }),
                    .object => try list.append(a, .{
                        .key = m.key,
                        .name = strOr(m.value.get("name"), m.key),
                        .lineweight_mm = numOr(m.value.get("lineweight_mm"), 0.25),
                        .linetype = strOr(m.value.get("linetype"), "CONTINUOUS"),
                    }),
                    else => {},
                }
            }
        };
        s.layers = list.items;
    }
    // Defaults for the scalar groups.
    const d = Style{ .id = "", .pens = &.{}, .materials = &.{}, .patterns = &.{}, .layers = &.{} };
    inline for (std.meta.fields(Style)) |f| {
        if (comptime !(std.mem.eql(u8, f.name, "id") or std.mem.eql(u8, f.name, "pens") or std.mem.eql(u8, f.name, "materials") or std.mem.eql(u8, f.name, "patterns") or std.mem.eql(u8, f.name, "layers"))) {
            @field(s, f.name) = @field(d, f.name);
        }
    }
    if (root.get("text")) |t| {
        s.text_height_in = numOr(t.get("height_in"), s.text_height_in);
        s.title_height_in = numOr(t.get("title_height_in"), s.title_height_in);
        s.label_height_in = numOr(t.get("label_height_in"), s.label_height_in);
        s.text_case_upper = !std.mem.eql(u8, strOr(t.get("case"), "upper"), "as_is");
        s.line_spacing = numOr(t.get("line_spacing"), s.line_spacing);
        s.dxf_style = strOr(t.get("dxf_style"), s.dxf_style);
        s.dxf_font = strOr(t.get("dxf_font"), s.dxf_font);
    }
    if (root.get("notes")) |t| {
        s.notes_mode_keynote = std.mem.eql(u8, strOr(t.get("mode"), "leader"), "keynote");
        s.wrap_chars = numOr(t.get("wrap_chars"), s.wrap_chars);
        s.gutter_in = numOr(t.get("gutter_in"), s.gutter_in);
        s.shoulder_in = numOr(t.get("shoulder_in"), s.shoulder_in);
        s.note_gap_in = numOr(t.get("note_gap_in"), s.note_gap_in);
        s.arrow = strOr(t.get("arrow"), s.arrow);
        s.arrow_len_in = numOr(t.get("arrow_len_in"), s.arrow_len_in);
        s.arrow_width_in = numOr(t.get("arrow_width_in"), s.arrow_width_in);
        s.keynote_tag = strOr(t.get("keynote_tag"), s.keynote_tag);
    }
    if (root.get("dims")) |t| {
        s.dim_terminator = strOr(t.get("terminator"), s.dim_terminator);
        s.tick_len_in = numOr(t.get("tick_len_in"), s.tick_len_in);
        s.ext_gap_in = numOr(t.get("ext_gap_in"), s.ext_gap_in);
        s.ext_over_in = numOr(t.get("ext_over_in"), s.ext_over_in);
        s.dim_text_gap_in = numOr(t.get("text_gap_in"), s.dim_text_gap_in);
        s.dim_precision = numOr(t.get("precision"), s.dim_precision);
    }
    if (root.get("citations")) |t| {
        s.cite_format = strOr(t.get("format"), s.cite_format);
        s.cite_flag_unverified = std.mem.eql(u8, strOr(t.get("unverified"), "flag"), "flag");
        s.cite_flag = strOr(t.get("flag"), s.cite_flag);
        s.cite_footnote = strOr(t.get("footnote"), s.cite_footnote);
    }
    if (root.get("sheet")) |t| {
        if (t.get("size_in")) |sz| if (sz.arr()) |arr| if (arr.len >= 2) {
            s.sheet_w_in = numOr(arr[0], s.sheet_w_in);
            s.sheet_h_in = numOr(arr[1], s.sheet_h_in);
        };
        s.margin_in = numOr(t.get("margin_in"), s.margin_in);
        s.title_block = strOr(t.get("title_block"), s.title_block);
        s.title_block_h_in = numOr(t.get("title_block_height_in"), s.title_block_h_in);
        s.firm = strOr(t.get("firm"), s.firm);
        s.project = strOr(t.get("project"), s.project);
    }
    if (root.get("break_line")) |t| {
        s.break_zig_in = numOr(t.get("zig_in"), s.break_zig_in);
        s.break_period_in = numOr(t.get("period_in"), s.break_period_in);
        s.break_overshoot_in = numOr(t.get("overshoot_in"), s.break_overshoot_in);
    }
    if (root.get("colors3d")) |t| {
        s.color_cut_cap = strOr(t.get("cut_cap"), s.color_cut_cap);
        s.color_edge = strOr(t.get("edge"), s.color_edge);
        s.color_background = strOr(t.get("background"), s.color_background);
    }
    return s;
}

test "default style loads" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const s = try load(arena.allocator(), null);
    try std.testing.expectEqualStrings("kerf-standard", s.id);
    try std.testing.expectEqual(@as(f64, 0.5), s.penWidthMm("cut"));
    try std.testing.expect(s.pen("hidden").?.dash_mm.?.len == 2);
    try std.testing.expectEqual(CutMark.x, s.material("wood").?.cut_mark);
    try std.testing.expectEqualStrings("S-DETL-CUT", s.layerForPen("cut"));
    try std.testing.expect(s.pattern("KERF-CONC").?.families.len == 5);
    try std.testing.expectEqual(@as(f64, 28), s.wrap_chars);
}

test "partial user style merges" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    var err: json.ParseError = undefined;
    const u = (try json.parse(a, "{\"notes\":{\"wrap_chars\":20},\"pens\":{\"cut\":{\"width_mm\":0.7}}}", &err)).?;
    const s = try load(a, u);
    try std.testing.expectEqual(@as(f64, 20), s.wrap_chars);
    try std.testing.expectEqual(@as(f64, 0.7), s.penWidthMm("cut"));
    try std.testing.expectEqual(@as(f64, 0.35), s.penWidthMm("profile"));
}
