//! Kerf Style (`*.kerfstyle.json`, SPEC 7): pens, materials, hatch patterns, text/notes/dims
//! settings, layers, sheet. The default style is embedded; a user style is merged over it
//! (RFC 7396) so partial styles work.

const std = @import("std");
const json = @import("json.zig");
const cast = @import("num.zig");
const pen_mod = @import("pen.zig");
pub const Pen = pen_mod.Pen;
pub const LayerKey = pen_mod.LayerKey;
const Allocator = std.mem.Allocator;

/// The default style, whitespace stripped at compile time (REVIEW SIZ-1: 12.1 KB -> 6.4 KB in the wasm).
pub const default_json = json.minify(@embedFile("kerf_style_json"));

/// A pen as the style defines it: name, width and dash pattern. The engine reaches pens through `pen.Pen`.
pub const PenDef = struct {
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

/// What a material is, for the rules the engine applies by kind (REVIEW ARC-1). A style sets it with `"role": "steel"` on a
/// material; without the key the role follows from the material name (`defaultRole`: what the embedded style's materials
/// are), so a custom style that renames or adds materials keeps working and can opt a new material into a rule.
pub const Role = enum {
    generic,
    /// Loose fill (earth, gravel, sand, compacted fill): not part of the view's auto-fit, never break-lined, drawn only in cutaway iso.
    soil,
    /// Wood that W_UNTREATED_CONTACT checks against masonry.
    wood,
    /// Concrete, CMU and mortar.
    masonry,
    /// Grout: masonry that is not split into units in 3D.
    grout,
    /// Structural steel and hardware: steel pen, thin sections are thickened.
    steel,
    /// Reinforcing bar: drawn along its centerline when it is a path bar.
    rebar,
    /// Thin sheet metal (aluminum, flashing membrane): thin sections render as a solid fill plus outline.
    sheet_metal,
    /// Vapor retarder: its dashed line keeps a minimum gap from the host edge it follows.
    vapor_retarder,
    /// Negative space (joint notches): not drawn as a body, not in 3D.
    void,

    pub fn isMetal(self: Role) bool {
        return self == .steel or self == .sheet_metal;
    }

    pub fn isMasonry(self: Role) bool {
        return self == .masonry or self == .grout;
    }
};

/// The role a material name has when the style does not say (the embedded style's materials).
pub fn defaultRole(name: []const u8) Role {
    const eq = std.mem.eql;
    if (eq(u8, name, "earth") or eq(u8, name, "gravel") or eq(u8, name, "sand") or eq(u8, name, "compacted_fill")) return .soil;
    if (eq(u8, name, "wood") or eq(u8, name, "wood_engineered") or eq(u8, name, "wood_board")) return .wood;
    if (eq(u8, name, "concrete") or eq(u8, name, "cmu") or eq(u8, name, "mortar")) return .masonry;
    if (eq(u8, name, "grout")) return .grout;
    if (eq(u8, name, "steel")) return .steel;
    if (eq(u8, name, "rebar")) return .rebar;
    if (eq(u8, name, "aluminum") or eq(u8, name, "flashing_membrane")) return .sheet_metal;
    if (eq(u8, name, "vapor_retarder")) return .vapor_retarder;
    if (eq(u8, name, "void")) return .void;
    return .generic;
}

pub const Material = struct {
    name: []const u8,
    hatch: []const HatchSpec = &.{},
    grain: ?GrainSpec = null,
    cut_mark: CutMark = .none,
    fill: bool = false,
    batt: bool = false,
    pen: ?Pen = null,
    role: Role = .generic,
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
    pens: []const PenDef,
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

    /// Characters per note line (validated to 4..200 at load; clamped again so a hand-built Style cannot reach a bad cast).
    pub fn wrapCols(self: *const Style) usize {
        return cast.toIntClamped(usize, self.wrap_chars, 4, 200);
    }

    pub fn pen(self: *const Style, which: Pen) ?PenDef {
        for (self.pens) |p| if (std.mem.eql(u8, p.name, @tagName(which))) return p;
        return null;
    }
    pub fn penWidthMm(self: *const Style, which: Pen) f64 {
        return if (self.pen(which)) |p| p.width_mm else 0.25;
    }
    /// The role of a material by name: the style's `role` when it defines the material, else the name's default.
    pub fn roleOf(self: *const Style, name: []const u8) Role {
        if (self.material(name)) |m| return m.role;
        return defaultRole(name);
    }
    pub fn material(self: *const Style, name: []const u8) ?*const Material {
        for (self.materials) |*m| if (std.mem.eql(u8, m.name, name)) return m;
        return null;
    }
    pub fn pattern(self: *const Style, name: []const u8) ?*const Pattern {
        for (self.patterns) |*p| if (std.mem.eql(u8, p.name, name)) return p;
        return null;
    }
    pub fn layerByKey(self: *const Style, key: LayerKey) ?Layer {
        for (self.layers) |l| if (std.mem.eql(u8, l.key, @tagName(key))) return l;
        return null;
    }
    /// Name of a layer in this style ("0" when the style does not define it).
    pub fn layerName(self: *const Style, key: LayerKey) []const u8 {
        return if (self.layerByKey(key)) |l| l.name else "0";
    }
    /// Layer name for a pen (`Pen.layer`).
    pub fn layerForPen(self: *const Style, which: Pen) []const u8 {
        const key = which.layer();
        if (self.layerByKey(key)) |l| return l.name;
        // a user style without a `rebar` layer keeps rebar on the steel layer
        if (key == .rebar) if (self.layerByKey(.steel)) |l| return l.name;
        return "0";
    }
};

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
    var why: []const u8 = "";
    return loadWhy(a, user, &why);
}

/// Like `load`; on `error.BadStyle` `why` is a message naming every offending key and the accepted range.
pub fn loadWhy(a: Allocator, user: ?json.Value, why: *[]const u8) StyleError!Style {
    var err: json.ParseError = undefined;
    const base = (try json.parse(a, default_json, &err)) orelse return error.BadStyle;
    var root = base;
    if (user) |u| {
        if (u == .object and u.object.len > 0) root = try json.mergePatch(a, base, u);
    }
    if (try validate(a, root)) |msg| {
        why.* = msg;
        return error.BadStyle;
    }
    return fromValue(a, root);
}

// ---- validation (REVIEW LAY-2, SAF-1, SAF-4) -----------------------------------------------------------------------------------
// Every number that later becomes a loop step, a character count or an array size is range-checked here once, so the
// layout and exporters can trust the Style. A wrong type or an out-of-range value is an error that names the key.

const Check = struct {
    a: Allocator,
    msg: std.ArrayList(u8) = .empty,
    count: usize = 0,

    fn bad(self: *Check, comptime fmt: []const u8, args: anytype) Allocator.Error!void {
        self.count += 1;
        if (self.count > 12) return; // keep the message short: the first dozen problems are plenty
        if (self.msg.items.len > 0) try self.msg.appendSlice(self.a, "; ");
        try self.msg.print(self.a, fmt, args);
    }

    fn gotText(self: *Check, v: json.Value) Allocator.Error![]const u8 {
        return switch (v) {
            .number => |n| blk: {
                var b: [40]u8 = undefined;
                break :blk try self.a.dupe(u8, json.fmtNumber(&b, n));
            },
            .string => |s| try self.a.print("\"{s}\"", .{s[0..@min(s.len, 24)]}),
            else => v.kindName(),
        };
    }

    /// `v` (if present and not null) must be a number in [lo, hi].
    fn range(self: *Check, path: []const u8, v: ?json.Value, lo: f64, hi: f64, def: f64) Allocator.Error!void {
        const x = v orelse return;
        if (x == .null) return;
        if (x == .number and x.number >= lo and x.number <= hi) return;
        var b1: [40]u8 = undefined;
        var b2: [40]u8 = undefined;
        var b3: [40]u8 = undefined;
        try self.bad("style key '{s}' must be a number from {s} to {s} (got {s}; default {s})", .{ path, json.fmtNumber(&b1, lo), json.fmtNumber(&b2, hi), try self.gotText(x), json.fmtNumber(&b3, def) });
    }

    fn section(self: *Check, root: json.Value, key: []const u8) Allocator.Error!?json.Value {
        const v = root.get(key) orelse return null;
        if (v == .null) return null;
        if (v != .object) {
            try self.bad("style key '{s}' must be an object (got {s})", .{ key, v.kindName() });
            return null;
        }
        return v;
    }
};

const RangeRow = struct { sec: []const u8, key: []const u8, field: []const u8, lo: f64, hi: f64 };
const range_rows = [_]RangeRow{
    .{ .sec = "text", .key = "height_in", .field = "text_height_in", .lo = 1.0 / 64.0, .hi = 2 },
    .{ .sec = "text", .key = "title_height_in", .field = "title_height_in", .lo = 1.0 / 64.0, .hi = 4 },
    .{ .sec = "text", .key = "label_height_in", .field = "label_height_in", .lo = 1.0 / 64.0, .hi = 2 },
    .{ .sec = "text", .key = "line_spacing", .field = "line_spacing", .lo = 0.5, .hi = 4 },
    .{ .sec = "notes", .key = "wrap_chars", .field = "wrap_chars", .lo = 4, .hi = 200 },
    .{ .sec = "notes", .key = "gutter_in", .field = "gutter_in", .lo = 0, .hi = 10 },
    .{ .sec = "notes", .key = "shoulder_in", .field = "shoulder_in", .lo = 0, .hi = 10 },
    .{ .sec = "notes", .key = "note_gap_in", .field = "note_gap_in", .lo = 0, .hi = 10 },
    .{ .sec = "notes", .key = "arrow_len_in", .field = "arrow_len_in", .lo = 0, .hi = 2 },
    .{ .sec = "notes", .key = "arrow_width_in", .field = "arrow_width_in", .lo = 0, .hi = 2 },
    .{ .sec = "dims", .key = "tick_len_in", .field = "tick_len_in", .lo = 0, .hi = 2 },
    .{ .sec = "dims", .key = "ext_gap_in", .field = "ext_gap_in", .lo = 0, .hi = 2 },
    .{ .sec = "dims", .key = "ext_over_in", .field = "ext_over_in", .lo = 0, .hi = 2 },
    .{ .sec = "dims", .key = "text_gap_in", .field = "dim_text_gap_in", .lo = 0, .hi = 2 },
    .{ .sec = "dims", .key = "precision", .field = "dim_precision", .lo = 1, .hi = 64 },
    .{ .sec = "sheet", .key = "margin_in", .field = "margin_in", .lo = 0, .hi = 10 },
    .{ .sec = "sheet", .key = "title_block_height_in", .field = "title_block_h_in", .lo = 0, .hi = 20 },
    .{ .sec = "break_line", .key = "zig_in", .field = "break_zig_in", .lo = 0.001, .hi = 5 },
    .{ .sec = "break_line", .key = "period_in", .field = "break_period_in", .lo = 0.01, .hi = 20 },
    .{ .sec = "break_line", .key = "overshoot_in", .field = "break_overshoot_in", .lo = 0, .hi = 5 },
};

/// Null when `root` is acceptable, else a message listing the problems (allocated from `a`).
fn validate(a: Allocator, root: json.Value) Allocator.Error!?[]const u8 {
    var c = Check{ .a = a };
    const d = Style{ .id = "", .pens = &.{}, .materials = &.{}, .patterns = &.{}, .layers = &.{} };
    inline for (range_rows) |r| {
        if (try c.section(root, r.sec)) |sec| {
            const path = r.sec ++ "." ++ r.key;
            try c.range(path, sec.get(r.key), r.lo, r.hi, @field(d, r.field));
        }
    }
    if (try c.section(root, "sheet")) |sec| {
        if (sec.get("size_in")) |sz| if (sz != .null) {
            if (sz != .array or sz.array.len != 2) {
                try c.bad("style key 'sheet.size_in' must be [width, height] in inches (got {s})", .{try c.gotText(sz)});
            } else {
                try c.range("sheet.size_in[0]", sz.array[0], 2, 200, d.sheet_w_in);
                try c.range("sheet.size_in[1]", sz.array[1], 2, 200, d.sheet_h_in);
            }
        };
    }
    try validatePens(&c, root);
    try validateMaterials(&c, root);
    try validatePatterns(&c, root);
    if (root.get("layers")) |lv| if (lv == .object) {
        for (lv.object) |m| if (m.value == .object) {
            const path = try a.print("layers.{s}.lineweight_mm", .{m.key});
            try c.range(path, m.value.get("lineweight_mm"), 0, 10, 0.25);
        };
    };
    if (c.count == 0) {
        // The page must keep a drawing area after margins and the title strip.
        const sh: json.Value = root.get("sheet") orelse .null;
        var w = d.sheet_w_in;
        var h = d.sheet_h_in;
        if (sh.get("size_in")) |sz| if (sz.arr()) |arr| if (arr.len == 2) {
            w = arr[0].num() orelse w;
            h = arr[1].num() orelse h;
        };
        const m = if (sh.get("margin_in")) |x| x.num() orelse d.margin_in else d.margin_in;
        const tb = if (sh.get("title_block_height_in")) |x| x.num() orelse d.title_block_h_in else d.title_block_h_in;
        if (w - 2 * m < 1 or h - 2 * m - tb < 1) {
            try c.bad("style key 'sheet': margin_in {d} and title_block_height_in {d} leave no drawing area on a {d} x {d} in sheet (need at least 1 in each way)", .{ m, tb, w, h });
        }
    }
    if (c.count == 0) return null;
    if (c.count > 12) try c.msg.print(a, "; and {d} more", .{c.count - 12});
    return c.msg.items;
}

fn validatePens(c: *Check, root: json.Value) Allocator.Error!void {
    const pv = root.get("pens") orelse return;
    if (pv != .object) return; // reported as BadStyle by fromValue (needs pens and materials objects)
    for (pv.object) |m| {
        if (m.value != .object) continue;
        const path = try c.a.print("pens.{s}.width_mm", .{m.key});
        try c.range(path, m.value.get("width_mm"), 0, 10, 0.25);
        if (m.value.get("dash_mm")) |dv| if (dv == .array) {
            for (dv.array, 0..) |x, i| {
                const dp = try c.a.print("pens.{s}.dash_mm[{d}]", .{ m.key, i });
                try c.range(dp, x, 0.01, 100, 1);
            }
        };
    }
}

const pen_names = blk: {
    var list: []const u8 = "";
    for (@typeInfo(Pen).@"enum".field_names, 0..) |n, i| list = list ++ (if (i > 0) ", " else "") ++ n;
    break :blk list;
};

const role_names = blk: {
    var list: []const u8 = "";
    for (@typeInfo(Role).@"enum".field_names, 0..) |n, i| list = list ++ (if (i > 0) ", " else "") ++ n;
    break :blk list;
};

fn validateMaterials(c: *Check, root: json.Value) Allocator.Error!void {
    const mv = root.get("materials") orelse return;
    if (mv != .object) return;
    for (mv.object) |m| {
        if (m.value != .object) continue;
        if (m.value.get("pen")) |pv| if (pv != .null) {
            if (pv.str()) |t| {
                if (std.meta.stringToEnum(Pen, t) == null) try c.bad("style key 'materials.{s}.pen' must be one of the engine's pens ({s}) (got \"{s}\")", .{ m.key, pen_names, t[0..@min(t.len, 24)] });
            } else try c.bad("style key 'materials.{s}.pen' must be a pen name string (got {s})", .{ m.key, pv.kindName() });
        };
        if (m.value.get("role")) |rv| if (rv != .null) {
            if (rv.str()) |t| {
                if (std.meta.stringToEnum(Role, t) == null) try c.bad("style key 'materials.{s}.role' must be one of {s} (got \"{s}\")", .{ m.key, role_names, t[0..@min(t.len, 24)] });
            } else try c.bad("style key 'materials.{s}.role' must be a role name string (got {s})", .{ m.key, rv.kindName() });
        };
        if (m.value.get("hatch")) |h| if (h == .array) {
            for (h.array, 0..) |x, i| {
                const base = try c.a.print("materials.{s}.hatch[{d}]", .{ m.key, i });
                try c.range(try c.a.print("{s}.scale", .{base}), x.get("scale"), 0.01, 100, 1);
                try c.range(try c.a.print("{s}.angle", .{base}), x.get("angle"), -3600, 3600, 0);
            }
        };
        if (m.value.get("grain")) |g| if (g == .object) {
            const base = try c.a.print("materials.{s}.grain", .{m.key});
            try c.range(try c.a.print("{s}.scale", .{base}), g.get("scale"), 0.01, 100, 1);
            try c.range(try c.a.print("{s}.amplitude", .{base}), g.get("amplitude"), 0, 1, 0.010);
            try c.range(try c.a.print("{s}.wavelength", .{base}), g.get("wavelength"), 0.05, 100, 1.1);
        };
    }
}

fn validatePatterns(c: *Check, root: json.Value) Allocator.Error!void {
    const pv = root.get("patterns") orelse return;
    if (pv != .object) return;
    for (pv.object) |m| {
        if (m.key.len > 0 and m.key[0] == '_') continue;
        const fams = m.value.arr() orelse continue;
        for (fams, 0..) |f, i| {
            const arr = f.arr() orelse continue;
            if (arr.len < 5) continue;
            const base = try c.a.print("patterns.{s}[{d}]", .{ m.key, i });
            try c.range(try c.a.print("{s} angle", .{base}), arr[0], -3600, 3600, 0);
            try c.range(try c.a.print("{s} x0", .{base}), arr[1], -1e4, 1e4, 0);
            try c.range(try c.a.print("{s} y0", .{base}), arr[2], -1e4, 1e4, 0);
            try c.range(try c.a.print("{s} dx", .{base}), arr[3], -1e4, 1e4, 0);
            // dy is the line spacing: |dy| >= 1e-4 keeps a hatch run finite (the 150,000-line cap would otherwise truncate silently)
            const dy = arr[4];
            if (dy == .number and (@abs(dy.number) < 1e-4 or @abs(dy.number) > 1e4)) {
                try c.bad("style key '{s} dy' must be a number with 0.0001 <= |dy| <= 10000 (got {s})", .{ base, try c.gotText(dy) });
            } else if (dy != .number) {
                try c.bad("style key '{s} dy' must be a number (got {s})", .{ base, try c.gotText(dy) });
            }
            for (arr[5..], 0..) |dd, k| try c.range(try c.a.print("{s} dash[{d}]", .{ base, k }), dd, -1e4, 1e4, 0);
        }
    }
}

pub fn fromValue(a: Allocator, root: json.Value) StyleError!Style {
    var s: Style = undefined;
    s.id = strOr(root.get("id"), "custom");

    // pens
    {
        const pv = root.get("pens") orelse return error.BadStyle;
        if (pv != .object) return error.BadStyle;
        const out = try a.alloc(PenDef, pv.object.len);
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
            var mat = Material{ .name = m.key, .role = defaultRole(m.key) };
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
            if (m.value.get("pen")) |p| if (p.str()) |t| {
                mat.pen = std.meta.stringToEnum(Pen, t);
            };
            if (m.value.get("role")) |r| if (r.str()) |t| {
                if (std.meta.stringToEnum(Role, t)) |role| mat.role = role;
            };
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
    inline for (@typeInfo(Style).@"struct".field_names) |name| {
        if (comptime !(std.mem.eql(u8, name, "id") or std.mem.eql(u8, name, "pens") or std.mem.eql(u8, name, "materials") or std.mem.eql(u8, name, "patterns") or std.mem.eql(u8, name, "layers"))) {
            @field(s, name) = @field(d, name);
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

test "the minified embedded style is the same document as spec/styles/kerf-standard.kerfstyle.json" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    var err: json.ParseError = undefined;
    const raw = (try json.parse(a, @embedFile("kerf_style_json"), &err)).?;
    const min = (try json.parse(a, default_json, &err)).?;
    var x: std.ArrayList(u8) = .empty;
    var y: std.ArrayList(u8) = .empty;
    try json.writeCompact(&x, a, raw);
    try json.writeCompact(&y, a, min);
    try std.testing.expectEqualStrings(x.items, y.items);
    try std.testing.expect(default_json.len < @embedFile("kerf_style_json").len * 6 / 10);
}

test "default style loads" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const s = try load(arena.allocator(), null);
    try std.testing.expectEqualStrings("kerf-standard", s.id);
    try std.testing.expectEqual(@as(f64, 0.5), s.penWidthMm(.cut));
    try std.testing.expect(s.pen(.hidden).?.dash_mm.?.len == 2);
    try std.testing.expectEqual(CutMark.x, s.material("wood").?.cut_mark);
    try std.testing.expectEqualStrings("S-DETL-CUT", s.layerForPen(.cut));
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
    try std.testing.expectEqual(@as(f64, 0.7), s.penWidthMm(.cut));
    try std.testing.expectEqual(@as(f64, 0.35), s.penWidthMm(.profile));
}

test "a material pen must be one of the engine's pens" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    var err: json.ParseError = undefined;
    const u = (try json.parse(a, "{\"materials\":{\"membrane\":{\"pen\":\"vapor\"},\"wrb\":{\"pen\":\"sketchy\"}}}", &err)).?;
    var why: []const u8 = "";
    try std.testing.expectError(error.BadStyle, loadWhy(a, u, &why));
    try std.testing.expect(std.mem.indexOf(u8, why, "materials.wrb.pen") != null);
    const ok = (try json.parse(a, "{\"materials\":{\"wrb\":{\"pen\":\"vapor\"}}}", &err)).?;
    const s = try load(a, ok);
    try std.testing.expectEqual(Pen.vapor, s.material("wrb").?.pen.?);
}

// The name lists the engine used before roles existed (section.isFillMaterial/isMetal, validate.isMasonry/isUntreatedWood and the
// literal comparisons in the renderers). Kept here only to prove the role table reproduces them for every embedded material.
fn oldIsSoil(m: []const u8) bool {
    const eq = std.mem.eql;
    return eq(u8, m, "earth") or eq(u8, m, "gravel") or eq(u8, m, "sand") or eq(u8, m, "compacted_fill");
}
fn oldIsMetal(m: []const u8) bool {
    const eq = std.mem.eql;
    return eq(u8, m, "steel") or eq(u8, m, "aluminum") or eq(u8, m, "flashing_membrane");
}
fn oldIsMasonry(m: []const u8) bool {
    const eq = std.mem.eql;
    return eq(u8, m, "concrete") or eq(u8, m, "grout") or eq(u8, m, "cmu") or eq(u8, m, "mortar");
}
fn oldIsUntreatedWood(m: []const u8) bool {
    const eq = std.mem.eql;
    return eq(u8, m, "wood") or eq(u8, m, "wood_engineered") or eq(u8, m, "wood_board");
}

test "the role table equals the old name predicates for every embedded material" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const s = try load(arena.allocator(), null);
    try std.testing.expect(s.materials.len > 20);
    for (s.materials) |m| {
        const eq = std.mem.eql;
        try std.testing.expectEqual(oldIsSoil(m.name), m.role == .soil);
        try std.testing.expectEqual(oldIsMetal(m.name), m.role.isMetal());
        try std.testing.expectEqual(oldIsMasonry(m.name), m.role.isMasonry());
        try std.testing.expectEqual(oldIsUntreatedWood(m.name), m.role == .wood);
        try std.testing.expectEqual(eq(u8, m.name, "vapor_retarder"), m.role == .vapor_retarder);
        try std.testing.expectEqual(eq(u8, m.name, "void"), m.role == .void);
        try std.testing.expectEqual(eq(u8, m.name, "grout"), m.role == .grout);
        try std.testing.expectEqual(eq(u8, m.name, "steel"), m.role == .steel);
        try std.testing.expectEqual(eq(u8, m.name, "rebar"), m.role == .rebar);
        // a material absent from the style falls back to the same table
        try std.testing.expectEqual(m.role, s.roleOf(m.name));
    }
    try std.testing.expectEqual(Role.generic, s.roleOf("no_such_material"));
}

test "a style can give a material a role" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    var err: json.ParseError = undefined;
    const u = (try json.parse(a, "{\"materials\":{\"stainless\":{\"role\":\"steel\"},\"fines\":{\"role\":\"soil\"}}}", &err)).?;
    const s = try load(a, u);
    try std.testing.expectEqual(Role.steel, s.roleOf("stainless"));
    try std.testing.expectEqual(Role.soil, s.roleOf("fines"));
    const bad = (try json.parse(a, "{\"materials\":{\"x\":{\"role\":\"plastic\"}}}", &err)).?;
    var why: []const u8 = "";
    try std.testing.expectError(error.BadStyle, loadWhy(a, bad, &why));
    try std.testing.expect(std.mem.indexOf(u8, why, "materials.x.role") != null);
}
