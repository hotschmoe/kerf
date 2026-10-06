//! Core model types shared by builders, the scene compiler, views and exporters.

const std = @import("std");
const cast = @import("num.zig");
const json = @import("json.zig");
const geom = @import("geom.zig");
const units = @import("units.zig");
const limits = @import("limits.zig");
const Pen = @import("pen.zig").Pen;
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Pt = geom.Pt;

// ---- diagnostics -----------------------------------------------------------------------------------

pub const Level = enum {
    @"error",
    warning,
    info,

    pub fn name(l: Level) []const u8 {
        return switch (l) {
            .@"error" => "error",
            .warning => "warning",
            .info => "info",
        };
    }
};

pub const Diag = struct {
    level: Level,
    code: []const u8,
    id: ?[]const u8 = null,
    path: ?[]const u8 = null,
    message: []const u8,
    fix: ?[]const u8 = null,
};

pub const Diags = struct {
    a: Allocator,
    list: std.ArrayList(Diag) = .empty,

    pub fn init(a: Allocator) Diags {
        return .{ .a = a };
    }

    pub fn add(self: *Diags, level: Level, code: []const u8, id: ?[]const u8, path: ?[]const u8, comptime fmt: []const u8, args: anytype) void {
        const msg = std.fmt.allocPrint(self.a, fmt, args) catch return;
        self.list.append(self.a, .{ .level = level, .code = code, .id = id, .path = path, .message = msg }) catch {};
    }
    pub fn addFix(self: *Diags, level: Level, code: []const u8, id: ?[]const u8, path: ?[]const u8, comptime fmt: []const u8, args: anytype, fix: []const u8) void {
        const msg = std.fmt.allocPrint(self.a, fmt, args) catch return;
        self.list.append(self.a, .{ .level = level, .code = code, .id = id, .path = path, .message = msg, .fix = fix }) catch {};
    }
    pub fn errCount(self: *const Diags) usize {
        var n: usize = 0;
        for (self.list.items) |d| if (d.level == .@"error") {
            n += 1;
        };
        return n;
    }
    pub fn warnCount(self: *const Diags) usize {
        var n: usize = 0;
        for (self.list.items) |d| if (d.level == .warning) {
            n += 1;
        };
        return n;
    }
    pub fn hasCode(self: *const Diags, code: []const u8) bool {
        for (self.list.items) |d| if (std.mem.eql(u8, d.code, code)) return true;
        return false;
    }
};

pub fn diagToJson(a: Allocator, d: Diag) Allocator.Error!json.Value {
    var m: std.ArrayList(json.Member) = .empty;
    try m.append(a, .{ .key = "level", .value = .{ .string = d.level.name() } });
    try m.append(a, .{ .key = "code", .value = .{ .string = d.code } });
    if (d.id) |x| try m.append(a, .{ .key = "id", .value = .{ .string = x } });
    if (d.path) |x| try m.append(a, .{ .key = "path", .value = .{ .string = x } });
    try m.append(a, .{ .key = "message", .value = .{ .string = d.message } });
    if (d.fix) |x| try m.append(a, .{ .key = "fix", .value = .{ .string = x } });
    return .{ .object = m.items };
}

// ---- prisms ---------------------------------------------------------------------------------------

pub const Kind = enum {
    /// A solid body: outline, hatch, occludes.
    body,
    /// A thin layer drawn as a line (membranes). Never occludes or hatches.
    line,
    /// Batting symbol (sinusoidal loop line) fitted to its loop.
    batt,
    /// Outline only, never occludes (hidden truss plate, CMU cross web beyond line).
    ghost,
};

pub const OutlineMode = enum { full, top, none };

/// A 2D profile extruded over [z0, z1] plus the flags the views need.
pub const Prism = struct {
    comp: u32 = 0,
    part: []const u8 = "",
    instance: u32 = 0,
    material: []const u8 = "generic",
    /// Region: loop 0 outer, remaining loops are holes.
    loops: []const []const Pt,
    z0: f64 = 0,
    z1: f64 = 0,
    embedded: bool = false,
    kind: Kind = .body,
    outline: OutlineMode = .full,
    /// Pen used for the outline when cut/beyond logic does not decide (ghost, line).
    pen: ?Pen = null,
    /// Cross-section marks (one quad per ply, corners CCW from bottom-left) for members seen
    /// end-on (lumber run z, panels). Drawn per the material's `cut_mark`.
    quads: []const [4]V2 = &.{},
    blocking: bool = false,
    /// Ply separation lines (segments) drawn with the outline.
    ply_lines: []const [2]V2 = &.{},
    /// For `line` prisms: the polyline to draw.
    line_pts: []const Pt = &.{},
    /// Centreline of path-like members (rebar, anchor bolts, straps) for 3D sweeps and iso.
    centerline: []const Pt = &.{},
    /// Radius for swept circles (centerline members), 0 when unused.
    sweep_r: f64 = 0,
    /// CMU units subdivide along Z in 3D.
    cmu_unit: bool = false,
    /// Course index for running-bond offsets (cmu units).
    course: u32 = 0,
    /// Shingle tick marks.
    ticks: bool = false,
    /// When > 0 and the component is centered in z, this prism spans +-zhalf instead of the natural thickness.
    zhalf: f64 = 0,
    /// `shown: "dashed"` ("where occurs"): a ghost prism (hidden pen, no hatch, never occludes) that notes can still target.
    dashed: bool = false,
    /// Face-on hardware (connector `lay:"face"`): a schematic symbol drawn outline-only in the `steel` pen with nail-hole
    /// dots along its centerline, on top of everything (never occluded, never filled, never a hatch hole). SPEC 20.
    face_tie: bool = false,

    pub fn transform(p: Prism, a: Allocator, xf: geom.Xf) Allocator.Error!Prism {
        var q = p;
        const loops = try a.alloc([]const Pt, p.loops.len);
        for (p.loops, 0..) |l, i| loops[i] = try xf.applyLoop(a, l);
        q.loops = loops;
        if (p.quads.len > 0) {
            const qs = try a.alloc([4]V2, p.quads.len);
            for (p.quads, 0..) |quad, i| for (quad, 0..) |c, k| {
                qs[i][k] = xf.apply(c);
            };
            q.quads = qs;
        }
        if (p.ply_lines.len > 0) {
            const ls = try a.alloc([2]V2, p.ply_lines.len);
            for (p.ply_lines, 0..) |s, i| {
                ls[i] = .{ xf.apply(s[0]), xf.apply(s[1]) };
            }
            q.ply_lines = ls;
        }
        if (p.line_pts.len > 0) q.line_pts = try xf.applyLoop(a, p.line_pts);
        if (p.centerline.len > 0) q.centerline = try xf.applyLoop(a, p.centerline);
        return q;
    }
};

pub const NamedAnchor = struct { name: []const u8, p: V2 };

/// A named region of a component (its parts, for placement and cover).
pub const Zone = struct {
    name: []const u8,
    loops: []const []const Pt,
    box: geom.Box,
};

pub const Cover = struct {
    bottom: f64 = 3,
    sides: f64 = 3,
    top: f64 = 1.5,
};

pub const PartCover = struct { part: []const u8, cover: Cover };

pub const HostInfo = struct {
    /// Outline used for cover checks (loop with bulges).
    outline: []const Pt,
    cover: Cover,
    part_cover: []const PartCover = &.{},
};

pub const Built = struct {
    prisms: []Prism,
    anchors: []const NamedAnchor = &.{},
    zones: []const Zone = &.{},
    /// Box used for the nine anchors (local coordinates).
    box: geom.Box,
    /// Natural z thickness for members that do not span the run; null spans the document run.
    nat_z: ?f64 = null,
    /// Builder placed points in the pre-transform world frame (anchor placement does not apply).
    points_mode: bool = false,
    host: ?HostInfo = null,
    /// Explicit instance centres (cover-based rebar placement), world coordinates.
    centers: []const V2 = &.{},
    /// Short parameter text for summaries, e.g. `2x8 PT flat run z`.
    info: []const u8 = "",
    /// Rebar facts for checks.
    bar_d: f64 = 0,
};

pub const box_anchor_names = [_][]const u8{
    "top_left",    "top_center",    "top_right",
    "middle_left", "center",        "middle_right",
    "bottom_left", "bottom_center", "bottom_right",
};

pub fn boxAnchor(b: geom.Box, name: []const u8) ?V2 {
    const cx = (b.x0 + b.x1) / 2;
    const cy = (b.y0 + b.y1) / 2;
    const eq = std.mem.eql;
    if (eq(u8, name, "top_left")) return V2.init(b.x0, b.y1);
    if (eq(u8, name, "top_center")) return V2.init(cx, b.y1);
    if (eq(u8, name, "top_right")) return V2.init(b.x1, b.y1);
    if (eq(u8, name, "middle_left")) return V2.init(b.x0, cy);
    if (eq(u8, name, "center")) return V2.init(cx, cy);
    if (eq(u8, name, "middle_right")) return V2.init(b.x1, cy);
    if (eq(u8, name, "bottom_left")) return V2.init(b.x0, b.y0);
    if (eq(u8, name, "bottom_center")) return V2.init(cx, b.y0);
    if (eq(u8, name, "bottom_right")) return V2.init(b.x1, b.y0);
    return null;
}

pub fn rectLoop(a: Allocator, x0: f64, y0: f64, x1: f64, y1: f64) Allocator.Error![]Pt {
    return a.dupe(Pt, &.{ .{ .x = x0, .y = y0 }, .{ .x = x1, .y = y0 }, .{ .x = x1, .y = y1 }, .{ .x = x0, .y = y1 } });
}

pub fn circleLoop(a: Allocator, cx: f64, cy: f64, r: f64) Allocator.Error![]Pt {
    return a.dupe(Pt, &.{ .{ .x = cx + r, .y = cy, .b = 1 }, .{ .x = cx - r, .y = cy, .b = 1 } });
}

pub fn oneLoop(a: Allocator, l: []const Pt) Allocator.Error![]const []const Pt {
    const out = try a.alloc([]const Pt, 1);
    out[0] = l;
    return out;
}

/// `[dx, dy]`: two lengths in inches. Reports E_PARAM (path = where it was written) and returns null on any other shape, so a
/// typo like ["1/2x", 3] is an error with a fix hint instead of an offset of 0.
pub fn offsetPairOrDiag(a: Allocator, diags: *Diags, id: ?[]const u8, path: []const u8, what: []const u8, v: json.Value) ?V2 {
    const fix = std.fmt.allocPrint(a, "write {s} as [x, y], two lengths in inches (numbers or strings such as \"1 1/2\"), e.g. [1.5, -2]", .{what}) catch "write it as [x, y], two lengths in inches";
    const arr = v.arr() orelse {
        diags.addFix(.@"error", "E_PARAM", id, path, "{s} must be an array [x, y] (got {s})", .{ what, kindOrText(a, v) }, fix);
        return null;
    };
    if (arr.len != 2) {
        diags.addFix(.@"error", "E_PARAM", id, path, "{s} must have exactly 2 values [x, y] (got {d})", .{ what, arr.len }, fix);
        return null;
    }
    var out: [2]f64 = undefined;
    for (arr, 0..) |e, i| {
        out[i] = units.parseLength(e) orelse {
            if (units.parseLengthAny(e)) |x| {
                diags.addFix(.@"error", "E_PARAM", id, path, "{s}[{d}] is out of range: a length must be within +-{d} inches (got {s}, which is {s} inches)", .{ what, i, limits.max_coord_in, kindOrText(a, e), numText(a, x) }, fix);
            } else {
                diags.addFix(.@"error", "E_PARAM", id, path, "{s}[{d}] must be a length (got {s}); lengths are {s}", .{ what, i, kindOrText(a, e), units.length_forms }, fix);
            }
            return null;
        };
    }
    return V2.init(out[0], out[1]);
}

/// A scalar length member (`offset` of a dim) with the same reporting; absent or null gives `default`.
pub fn lengthOrDiag(a: Allocator, diags: *Diags, id: ?[]const u8, path: []const u8, v: ?json.Value, what: []const u8, default: f64) ?f64 {
    const x = v orelse return default;
    if (x == .null) return default;
    if (units.parseLength(x)) |n| return n;
    const fix = "write it as a number of inches or a length string such as \"1 1/2\"";
    if (units.parseLengthAny(x)) |n| {
        diags.addFix(.@"error", "E_PARAM", id, path, "{s} is out of range: a length must be within +-{d} inches (got {s}, which is {s} inches)", .{ what, limits.max_coord_in, kindOrText(a, x), numText(a, n) }, fix);
    } else {
        diags.addFix(.@"error", "E_PARAM", id, path, "{s} must be a length (got {s}); lengths are {s}", .{ what, kindOrText(a, x), units.length_forms }, fix);
    }
    return null;
}

// ---- param extraction -------------------------------------------------------------------------------

/// Typed access to a component's (or annotation's) JSON params with E_PARAM diagnostics.
pub const Params = struct {
    a: Allocator,
    diags: *Diags,
    node: json.Value,
    id: []const u8,
    /// "components" or "views/A/annotations"
    base: []const u8,
    ty: []const u8 = "",
    ok: bool = true,

    pub fn fail(self: *Params, key: []const u8, comptime fmt: []const u8, args: anytype) void {
        self.failCode("E_PARAM", key, fmt, args);
    }

    /// `fail` with another diagnostic code (E_LIMIT, E_ANCHOR_UNKNOWN, ...); the code is part of the diagnostic from the start.
    pub fn failCode(self: *Params, code: []const u8, key: []const u8, comptime fmt: []const u8, args: anytype) void {
        self.ok = false;
        const path = std.fmt.allocPrint(self.a, "{s}/{s}/{s}", .{ self.base, self.id, key }) catch return;
        self.diags.add(.@"error", code, self.id, path, fmt, args);
    }

    pub fn raw(self: *const Params, key: []const u8) ?json.Value {
        const v = self.node.get(key) orelse return null;
        if (v == .null) return null;
        return v;
    }

    pub fn has(self: *const Params, key: []const u8) bool {
        return self.raw(key) != null;
    }

    /// Length in inches; `default == null` means required.
    pub fn len(self: *Params, key: []const u8, default: ?f64, hint: []const u8) ?f64 {
        const v = self.raw(key) orelse {
            if (default) |d| return d;
            self.fail(key, "param '{s}' is required for type {s}{s}{s}", .{ key, self.ty, if (hint.len > 0) ": " else "", hint });
            return null;
        };
        if (units.parseLength(v)) |x| return x;
        if (units.parseLengthAny(v)) |x| {
            self.fail(key, "param '{s}' is out of range: a length must be within +-{d} inches (got {s}, which is {s} inches)", .{ key, limits.max_coord_in, kindOrText(self.a, v), numText(self.a, x) });
            return null;
        }
        self.fail(key, "param '{s}' must be a length: {s} (got {s})", .{ key, units.length_forms, kindOrText(self.a, v) });
        return null;
    }

    pub fn lenPos(self: *Params, key: []const u8, default: ?f64, hint: []const u8) ?f64 {
        const x = self.len(key, default, hint) orelse return null;
        if (x <= 0) {
            self.fail(key, "param '{s}' must be greater than 0 (got {s})", .{ key, kindOrText(self.a, .{ .number = x }) });
            return null;
        }
        return x;
    }

    pub fn num(self: *Params, key: []const u8, default: ?f64) ?f64 {
        const v = self.raw(key) orelse {
            if (default) |d| return d;
            self.fail(key, "param '{s}' is required for type {s}", .{ key, self.ty });
            return null;
        };
        if (v == .number) return v.number;
        self.fail(key, "param '{s}' must be a number (got {s})", .{ key, kindOrText(self.a, v) });
        return null;
    }

    pub fn int(self: *Params, key: []const u8, default: ?i64, min: i64, max: i64) ?i64 {
        const x = self.num(key, if (default) |d| @floatFromInt(d) else null) orelse return null;
        if (x != @round(x) or x < @as(f64, @floatFromInt(min)) or x > @as(f64, @floatFromInt(max))) {
            self.fail(key, "param '{s}' must be an integer from {d} to {d} (got {s})", .{ key, min, max, kindOrText(self.a, .{ .number = x }) });
            return null;
        }
        return cast.toInt(i64, x);
    }

    // ---- members of object-valued params (place.cover, array.count, recess.from_edge ...) -------------------------------
    // Absent or null gives the default; anything else must have the right type and range, else E_PARAM at `<key>/<field>`
    // and null is returned. Formerly these read `... orelse default`, so a typo such as "cover": "1 1/2x" silently became 1.5.

    fn fieldPath(self: *Params, key: []const u8, field: []const u8) []const u8 {
        return std.fmt.allocPrint(self.a, "{s}/{s}", .{ key, field }) catch key;
    }

    pub fn fieldLen(self: *Params, key: []const u8, obj: json.Value, field: []const u8, default: f64) ?f64 {
        const v = obj.get(field) orelse return default;
        if (v == .null) return default;
        if (units.parseLength(v)) |x| return x;
        const path = self.fieldPath(key, field);
        if (units.parseLengthAny(v)) |x| {
            self.failCode("E_PARAM", path, "{s}.{s} is out of range: a length must be within +-{d} inches (got {s}, which is {s} inches)", .{ key, field, limits.max_coord_in, kindOrText(self.a, v), numText(self.a, x) });
        } else {
            self.failCode("E_PARAM", path, "{s}.{s} must be a length: {s} (got {s})", .{ key, field, units.length_forms, kindOrText(self.a, v) });
        }
        return null;
    }

    pub fn fieldInt(self: *Params, key: []const u8, obj: json.Value, field: []const u8, default: i64, min: i64, max: i64) ?i64 {
        const v = obj.get(field) orelse return default;
        if (v == .null) return default;
        if (v == .number and v.number == @round(v.number)) if (cast.toInt(i64, v.number)) |n| if (n >= min and n <= max) return n;
        self.failCode("E_PARAM", self.fieldPath(key, field), "{s}.{s} must be an integer from {d} to {d} (got {s})", .{ key, field, min, max, kindOrText(self.a, v) });
        return null;
    }

    /// A string member that must be one of `choices` (the first is the default).
    pub fn fieldChoice(self: *Params, key: []const u8, obj: json.Value, field: []const u8, choices: []const []const u8) ?[]const u8 {
        const v = obj.get(field) orelse return choices[0];
        if (v == .null) return choices[0];
        if (v == .string) for (choices) |c| if (std.mem.eql(u8, v.string, c)) return c;
        self.failCode("E_PARAM", self.fieldPath(key, field), "{s}.{s} must be one of {s} (got {s})", .{ key, field, joinQuoted(self.a, choices), kindOrText(self.a, v) });
        return null;
    }

    /// An `[dx, dy]` pair of lengths (offset of a Ref) at param `key`; any other shape is E_PARAM, never "0".
    pub fn offsetPair(self: *Params, key: []const u8, v: json.Value) ?V2 {
        const r = offsetPairOrDiag(self.a, self.diags, self.id, std.fmt.allocPrint(self.a, "{s}/{s}/{s}", .{ self.base, self.id, key }) catch key, "offset", v);
        if (r == null) self.ok = false;
        return r;
    }

    pub fn boolean(self: *Params, key: []const u8, default: bool) bool {
        const v = self.raw(key) orelse return default;
        if (v == .bool) return v.bool;
        self.fail(key, "param '{s}' must be true or false (got {s})", .{ key, kindOrText(self.a, v) });
        return default;
    }

    pub fn str(self: *Params, key: []const u8, default: ?[]const u8) ?[]const u8 {
        const v = self.raw(key) orelse {
            if (default) |d| return d;
            self.fail(key, "param '{s}' is required for type {s}", .{ key, self.ty });
            return null;
        };
        if (v == .string) return v.string;
        self.fail(key, "param '{s}' must be a string (got {s})", .{ key, kindOrText(self.a, v) });
        return null;
    }

    /// One of `allowed`.
    pub fn choice(self: *Params, key: []const u8, default: ?[]const u8, allowed: []const []const u8) ?[]const u8 {
        const s = self.str(key, default) orelse return null;
        for (allowed) |al| if (std.mem.eql(u8, al, s)) return al;
        const list = joinQuoted(self.a, allowed);
        self.fail(key, "param '{s}' must be one of {s} (got \"{s}\")", .{ key, list, s });
        return null;
    }
};

pub fn joinQuoted(a: Allocator, items: []const []const u8) []const u8 {
    var out: std.ArrayList(u8) = .empty;
    for (items, 0..) |it, i| {
        if (i > 0) out.appendSlice(a, ", ") catch {};
        out.append(a, '"') catch {};
        out.appendSlice(a, it) catch {};
        out.append(a, '"') catch {};
    }
    return out.items;
}

/// A number for a message: plain up to 1e9 and down to 1e-4, scientific beyond (so 1e300 does not print 300 digits).
pub fn numText(a: Allocator, x: f64) []const u8 {
    if (!std.math.isFinite(x)) return "a non-finite number";
    const m = @abs(x);
    const s = if (m != 0 and (m >= 1e9 or m < 1e-4)) std.fmt.allocPrint(a, "{e}", .{x}) else json.fmtNumberAlloc(a, x);
    return s catch "?";
}

pub fn kindOrText(a: Allocator, v: json.Value) []const u8 {
    var out: std.ArrayList(u8) = .empty;
    switch (v) {
        .string => |s| {
            out.append(a, '"') catch {};
            out.appendSlice(a, s) catch {};
            out.append(a, '"') catch {};
        },
        .number, .bool, .null => json.writeCompact(&out, a, v) catch {},
        else => return v.kindName(),
    }
    return out.items;
}

// ---- text helpers ------------------------------------------------------------------------------------

pub fn editDistance(a: Allocator, x: []const u8, y: []const u8) usize {
    const n = y.len;
    const prev = a.alloc(usize, n + 1) catch return 999;
    const cur = a.alloc(usize, n + 1) catch return 999;
    for (prev, 0..) |*p, j| p.* = j;
    for (x, 0..) |cx, i| {
        cur[0] = i + 1;
        for (y, 0..) |cy, j| {
            const sub = prev[j] + @as(usize, if (cx == cy) 0 else 1);
            cur[j + 1] = @min(sub, @min(prev[j + 1] + 1, cur[j] + 1));
        }
        @memcpy(prev, cur);
    }
    return prev[n];
}

/// Closest of `names` to `x` (null when nothing is reasonably close or `names` is empty).
pub fn nearest(a: Allocator, x: []const u8, names: []const []const u8) ?[]const u8 {
    var best: ?[]const u8 = null;
    var bd: usize = std.math.maxInt(usize);
    for (names) |n| {
        const d = editDistance(a, x, n);
        if (d < bd) {
            bd = d;
            best = n;
        }
    }
    if (best != null and bd <= @max(3, x.len / 2)) return best;
    return null;
}

test "edit distance" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    try std.testing.expectEqual(@as(usize, 1), editDistance(arena.allocator(), "sill_plate", "sill_plat"));
    try std.testing.expectEqual(@as(usize, 3), editDistance(arena.allocator(), "kitten", "sitting"));
}

test "box anchors" {
    const b = geom.Box{ .x0 = 0, .y0 = 0, .x1 = 4, .y1 = 2 };
    try std.testing.expectEqual(V2.init(2, 1), boxAnchor(b, "center").?);
    try std.testing.expectEqual(V2.init(4, 2), boxAnchor(b, "top_right").?);
    try std.testing.expect(boxAnchor(b, "nope") == null);
}
