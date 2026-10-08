//! Typed component parameters (REVIEW IDM-1, ARC-4).
//!
//! A component type declares its parameters once, as a struct:
//!
//!     pub const Params = struct {
//!         size: []const u8,                       // no default: required
//!         product: enum { sawn, lvl } = .sawn,   // string choice, the tags are the accepted values
//!         plies: u8 = 1,                          // integer: range in `spec`
//!         thickness: f64,                         // number: `spec` says whether it is a length
//!         until: ?json.Value = null,              // `?` = may be absent; json.Value = raw, validated by the builder
//!         pub const spec = .{
//!             .size = .{ .desc = "what the catalog shows" },
//!             .plies = .{ .min = 1, .max = 8, .desc = "..." },
//!             .thickness = .{ .len = .pos, .hint = "e.g. 0.4375", .desc = "..." },
//!             ...
//!         };
//!     };
//!
//! `parse(Params, p)` reads every field with the same E_PARAM messages the hand-written `Params.str/num/len/int/choice/boolean`
//! calls produce (all problems are collected in one pass; null means at least one was reported). `rows(Params)` generates the
//! catalog table (`kerf catalog`, `kerf schema <type>`, the accepted-key list and the canonical key order of `kerf fmt`) from
//! the same struct, so the parser, the catalog and the unknown-key check cannot drift apart.
//!
//! `spec` entries (all optional except `desc`; a field without an entry is a compile error, and so is an entry without a field):
//!   .desc    catalog text for the parameter.
//!   .def     catalog default text when it is not simply the field default ("required", "null", "true", the number, the tag).
//!   .len     `.any` or `.pos` on an f64 field: parse as a length (`4`, `"3'-4"`), `.pos` also demands > 0. Without it an f64 is a plain number.
//!   .min/.max  range of an integer field (required for integer fields).
//!   .hint    appended to the "is required" message of a length field.
//!   .also    further key names that share this catalog row (`width, height`); they need their own fields with `.row = false`.
//!   .row     false: accepted and parsed, but listed only in the row of the field that names it in `.also`.

const std = @import("std");
const json = @import("json.zig");
const model = @import("model.zig");
const Params = model.Params;

pub const LenKind = enum { any, pos };

/// One catalog row: the parameter names that share it (first = canonical), its default text and its description.
pub const Row = struct {
    names: []const []const u8,
    def: []const u8,
    desc: []const u8,

    /// The names joined as the tables print them: `width, height`.
    pub fn nameText(self: Row, a: std.mem.Allocator) std.mem.Allocator.Error![]const u8 {
        if (self.names.len == 1) return self.names[0];
        var out: std.ArrayList(u8) = .empty;
        for (self.names, 0..) |n, i| {
            if (i > 0) try out.appendSlice(a, ", ");
            try out.appendSlice(a, n);
        }
        return out.items;
    }
};

/// `s.<key>` when the spec entry has it, else `default`.
fn opt(comptime s: anytype, comptime key: []const u8, comptime default: anytype) if (@hasField(@TypeOf(s), key)) @TypeOf(@field(s, key)) else @TypeOf(default) {
    return if (@hasField(@TypeOf(s), key)) @field(s, key) else default;
}

fn hasSpec(comptime T: type, comptime name: []const u8) bool {
    return @hasDecl(T, "spec") and @hasField(@TypeOf(T.spec), name);
}

/// The `spec` entry of field `name` (an empty struct when there is none).
fn specOf(comptime T: type, comptime name: []const u8) if (hasSpec(T, name)) @TypeOf(@field(T.spec, name)) else @TypeOf(.{}) {
    return if (comptime hasSpec(T, name)) @field(T.spec, name) else .{};
}

fn Unwrap(comptime T: type) type {
    return switch (@typeInfo(T)) {
        .optional => |o| o.child,
        else => T,
    };
}

fn isOptional(comptime T: type) bool {
    return @typeInfo(T) == .optional;
}

fn isString(comptime T: type) bool {
    return switch (@typeInfo(T)) {
        .pointer => |ptr| ptr.size == .slice and ptr.child == u8,
        else => false,
    };
}

fn tagNames(comptime E: type) []const []const u8 {
    return comptime blk: {
        const names = @typeInfo(E).@"enum".field_names;
        var out: [names.len][]const u8 = undefined;
        for (names, 0..) |n, i| out[i] = n;
        const final = out;
        break :blk &final;
    };
}

/// The declared default of field number `i` of struct `T`, if it has one.
fn defaultOf(comptime T: type, comptime i: usize) ?@typeInfo(T).@"struct".field_types[i] {
    const info = @typeInfo(T).@"struct";
    return info.field_attrs[i].defaultValue(info.field_types[i]);
}

/// Check the struct and its `spec` agree (compile errors that name the offender).
fn checkSpec(comptime T: type) void {
    comptime {
        const info = @typeInfo(T).@"struct";
        if (@hasDecl(T, "spec")) {
            for (@typeInfo(@TypeOf(T.spec)).@"struct".field_names) |sname| {
                if (!@hasField(T, sname)) @compileError(@typeName(T) ++ ".spec names '" ++ sname ++ "', which is not a field");
            }
        }
        for (info.field_names, info.field_types, 0..) |name, FieldType, i| {
            const s = specOf(T, name);
            if (!@hasField(@TypeOf(s), "desc")) @compileError(@typeName(T) ++ "." ++ name ++ " has no `spec` entry with a .desc (it would be missing from the catalog)");
            const Base = Unwrap(FieldType);
            if (@typeInfo(Base) == .int) {
                if (!@hasField(@TypeOf(s), "min") or !@hasField(@TypeOf(s), "max")) @compileError(@typeName(T) ++ "." ++ name ++ " is an integer: its spec needs .min and .max");
                if (s.min < std.math.minInt(Base) or s.max > std.math.maxInt(Base)) @compileError(@typeName(T) ++ "." ++ name ++ ": .min/.max do not fit the field type");
            }
            if (Base == json.Value and defaultOf(T, i) == null and !isOptional(FieldType) and !@hasField(@TypeOf(s), "def")) {
                @compileError(@typeName(T) ++ "." ++ name ++ ": a raw json.Value field needs a default, `?json.Value`, or an explicit .def");
            }
        }
    }
}

/// Read every field of `T` from the component's JSON. Returns null when any parameter was reported as an E_PARAM.
pub fn parse(comptime T: type, p: *Params) ?T {
    const out = parseAll(T, p);
    return if (p.ok) out else null;
}

/// Like `parse` but total: a field that was reported keeps its default (a required one a zero value), so the caller can go on
/// collecting problems that do not depend on it. The result is only meaningful if `p.ok` is still true.
pub fn parseAll(comptime T: type, p: *Params) T {
    comptime checkSpec(T);
    var out: T = undefined;
    const info = @typeInfo(T).@"struct";
    inline for (info.field_names, info.field_types, 0..) |name, FieldType, i| {
        @field(out, name) = comptime (defaultOf(T, i) orelse zeroOf(FieldType));
        readField(T, i, p, &@field(out, name));
    }
    return out;
}

/// A harmless value for a required field that failed to parse.
fn zeroOf(comptime T: type) T {
    return switch (@typeInfo(T)) {
        .optional => null,
        .bool => false,
        .float => 0,
        .int => 0,
        .@"enum" => @fromBackingInt(0),
        .pointer => "",
        .@"union" => if (T == json.Value) .null else @compileError("unsupported param type " ++ @typeName(T)),
        else => @compileError("unsupported param type " ++ @typeName(T)),
    };
}

fn readField(comptime T: type, comptime i: usize, p: *Params, dst: *@typeInfo(T).@"struct".field_types[i]) void {
    const info = @typeInfo(T).@"struct";
    const name = comptime info.field_names[i];
    const FieldType = comptime info.field_types[i];
    const s = comptime specOf(T, name);
    const Base = comptime Unwrap(FieldType);
    const default: ?Base = comptime if (defaultOf(T, i)) |d| d else null;
    if (comptime isOptional(FieldType)) {
        // `?X`: absent (or null) is fine; present must be a valid X. A default other than null is allowed too.
        if (!p.has(name)) {
            dst.* = default;
        } else if (readValue(Base, name, s, p, null)) |v| {
            dst.* = v;
        }
    } else if (readValue(Base, name, s, p, default)) |v| {
        dst.* = v;
    }
}

/// The scalar kinds the reader knows. One non-generic function (`readScalar`) serves every field of every struct, so the
/// per-field code in a `parse` instantiation is a call with constants, not a copy of the checks.
const Kind = enum { boolean, num, len, len_pos, int, str, raw };

const Scalar = union { b: bool, f: f64, i: i64, s: []const u8, v: json.Value };

fn readScalar(p: *Params, kind: Kind, name: []const u8, hint: []const u8, min: i64, max: i64, default: ?Scalar) ?Scalar {
    switch (kind) {
        .boolean => return .{ .b = p.boolean(name, default.?.b) },
        .num => return .{ .f = p.num(name, if (default) |d| d.f else null) orelse return null },
        .len => return .{ .f = p.len(name, if (default) |d| d.f else null, hint) orelse return null },
        .len_pos => return .{ .f = p.lenPos(name, if (default) |d| d.f else null, hint) orelse return null },
        .int => return .{ .i = p.int(name, if (default) |d| d.i else null, min, max) orelse return null },
        .str => return .{ .s = p.str(name, if (default) |d| d.s else null) orelse return null },
        .raw => {
            if (p.raw(name)) |v| return .{ .v = v };
            if (default) |d| return d;
            p.fail(name, "param '{s}' is required for type {s}", .{ name, p.ty });
            return null;
        },
    }
}

/// One value of type `Base`; `default == null` means required. Null result: reported (or, for json.Value, required and absent).
fn readValue(comptime Base: type, comptime name: []const u8, comptime s: anytype, p: *Params, comptime default: ?Base) ?Base {
    const hint = comptime opt(s, "hint", @as([]const u8, ""));
    switch (@typeInfo(Base)) {
        .bool => {
            const d = default orelse @compileError("bool param '" ++ name ++ "' needs a default");
            return (readScalar(p, .boolean, name, "", 0, 0, .{ .b = d }) orelse return null).b;
        },
        .float => {
            const kind: Kind = comptime if (@hasField(@TypeOf(s), "len")) (if (s.len == .pos) .len_pos else .len) else .num;
            const d: ?Scalar = if (default) |x| .{ .f = x } else null;
            return (readScalar(p, kind, name, hint, 0, 0, d) orelse return null).f;
        },
        .int => {
            const d: ?Scalar = if (default) |x| .{ .i = @as(i64, x) } else null;
            const v = (readScalar(p, .int, name, "", s.min, s.max, d) orelse return null).i;
            return @intCast(v);
        },
        .@"enum" => {
            const names = comptime tagNames(Base);
            const d: ?[]const u8 = if (default) |x| @tagName(x) else null;
            const str = p.choice(name, d, names) orelse return null;
            return std.meta.stringToEnum(Base, str);
        },
        .pointer => {
            if (comptime !isString(Base)) @compileError("unsupported param type " ++ @typeName(Base));
            const d: ?Scalar = if (default) |x| .{ .s = x } else null;
            return (readScalar(p, .str, name, "", 0, 0, d) orelse return null).s;
        },
        .@"union" => {
            if (comptime Base != json.Value) @compileError("unsupported param type " ++ @typeName(Base));
            const d: ?Scalar = if (default) |x| .{ .v = x } else null;
            return (readScalar(p, .raw, name, "", 0, 0, d) orelse return null).v;
        },
        else => @compileError("unsupported param type " ++ @typeName(Base)),
    }
}

// ---- the catalog table --------------------------------------------------------------------------------

fn defaultText(comptime T: type, comptime i: usize) []const u8 {
    comptime {
        const info = @typeInfo(T).@"struct";
        const s = specOf(T, info.field_names[i]);
        if (@hasField(@TypeOf(s), "def")) return s.def;
        const FieldType = info.field_types[i];
        const Base = Unwrap(FieldType);
        const d = defaultOf(T, i) orelse return if (isOptional(FieldType)) "null" else "required";
        if (isOptional(FieldType)) {
            if (d == null) return "null";
            return text(Base, d.?);
        }
        return text(Base, d);
    }
}

fn text(comptime Base: type, comptime v: Base) []const u8 {
    comptime {
        return switch (@typeInfo(Base)) {
            .bool => if (v) "true" else "false",
            .@"enum" => @tagName(v),
            .float, .int => std.fmt.comptimePrint("{d}", .{v}),
            .pointer => v,
            else => @compileError("give " ++ @typeName(Base) ++ " fields an explicit .def"),
        };
    }
}

fn rowCount(comptime T: type) usize {
    comptime {
        var n: usize = 0;
        for (@typeInfo(T).@"struct".field_names) |name| {
            if (opt(specOf(T, name), "row", true)) n += 1;
        }
        return n;
    }
}

/// The catalog rows of `T`, in field order (comptime; the result is a constant slice).
pub fn rows(comptime T: type) []const Row {
    return comptime blk: {
        @setEvalBranchQuota(200_000); // number formatting for the default texts
        checkSpec(T);
        var out: [rowCount(T)]Row = undefined;
        var i: usize = 0;
        for (@typeInfo(T).@"struct".field_names, 0..) |name, fi| {
            const s = specOf(T, name);
            if (!opt(s, "row", true)) continue;
            const also = opt(s, "also", &[_][]const u8{});
            var names: [1 + also.len][]const u8 = undefined;
            names[0] = name;
            for (also, 0..) |nm, k| names[1 + k] = nm;
            const final_names = names;
            out[i] = .{ .names = &final_names, .def = defaultText(T, fi), .desc = s.desc };
            i += 1;
        }
        const final = out;
        break :blk &final;
    };
}

// ---- tests ------------------------------------------------------------------------------------------

const Sample = struct {
    size: []const u8,
    product: enum { sawn, lvl, psl } = .sawn,
    run: enum { z, x, y } = .z,
    plies: u8 = 1,
    treated: bool = false,
    thickness: f64,
    gap: f64 = 0.5,
    length: ?f64 = null,
    grade: ?[]const u8 = null,
    width: ?json.Value = null,
    height: ?json.Value = null,

    pub const spec = .{
        .size = .{ .desc = "nominal size" },
        .product = .{ .desc = "sawn | lvl | psl" },
        .run = .{ .desc = "axis" },
        .plies = .{ .min = 1, .max = 8, .desc = "built-up members" },
        .treated = .{ .desc = "PT" },
        .thickness = .{ .len = .pos, .hint = "e.g. 0.4375", .desc = "thickness" },
        .gap = .{ .len = .any, .desc = "gap" },
        .length = .{ .len = .pos, .def = "required for run x/y", .desc = "length" },
        .grade = .{ .desc = "free text" },
        .width = .{ .also = &.{"height"}, .def = "rect: required", .desc = "box size" },
        .height = .{ .row = false, .desc = "box size" },
    };
};

fn testParams(a: std.mem.Allocator, diags: *model.Diags, src: []const u8) !Params {
    var err: json.ParseError = undefined;
    const node = (try json.parse(a, src, &err)).?;
    return .{ .a = a, .diags = diags, .node = node, .id = "c1", .base = "components", .ty = "sample" };
}

test "parse reads typed fields and applies defaults" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    var diags = model.Diags.init(a);
    var p = try testParams(a, &diags, "{\"size\":\"2x4\",\"run\":\"x\",\"plies\":3,\"thickness\":\"1/2\",\"length\":\"2'\"}");
    const v = parse(Sample, &p).?;
    try std.testing.expectEqualStrings("2x4", v.size);
    try std.testing.expectEqual(.sawn, v.product);
    try std.testing.expectEqual(.x, v.run);
    try std.testing.expectEqual(@as(u8, 3), v.plies);
    try std.testing.expectEqual(@as(f64, 0.5), v.thickness);
    try std.testing.expectEqual(@as(f64, 0.5), v.gap);
    try std.testing.expectEqual(@as(f64, 24), v.length.?);
    try std.testing.expect(v.grade == null);
    try std.testing.expectEqual(@as(usize, 0), diags.list.items.len);
}

test "parse collects every problem with the hand-written messages" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    var diags = model.Diags.init(a);
    var p = try testParams(a, &diags, "{\"run\":\"w\",\"plies\":9,\"treated\":1,\"thickness\":0,\"grade\":4}");
    try std.testing.expect(parse(Sample, &p) == null);
    const want = [_][]const u8{
        "param 'size' is required for type sample",
        "param 'run' must be one of \"z\", \"x\", \"y\" (got \"w\")",
        "param 'plies' must be an integer from 1 to 8 (got 9)",
        "param 'treated' must be true or false (got 1)",
        "param 'thickness' must be greater than 0 (got 0)",
        "param 'grade' must be a string (got 4)",
    };
    try std.testing.expectEqual(want.len, diags.list.items.len);
    for (want, diags.list.items) |w, d| try std.testing.expectEqualStrings(w, d.message);
    try std.testing.expectEqualStrings("components/c1/size", diags.list.items[0].path.?);
}

test "a required length reports its hint" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    var diags = model.Diags.init(a);
    var p = try testParams(a, &diags, "{\"size\":\"2x4\"}");
    try std.testing.expect(parse(Sample, &p) == null);
    try std.testing.expectEqualStrings("param 'thickness' is required for type sample: e.g. 0.4375", diags.list.items[0].message);
}

test "rows are generated from the struct" {
    const r = rows(Sample);
    try std.testing.expectEqual(@as(usize, 10), r.len);
    try std.testing.expectEqualStrings("size", r[0].names[0]);
    try std.testing.expectEqualStrings("required", r[0].def);
    try std.testing.expectEqualStrings("sawn", r[1].def);
    try std.testing.expectEqualStrings("1", r[3].def);
    try std.testing.expectEqualStrings("false", r[4].def);
    try std.testing.expectEqualStrings("0.5", r[6].def);
    try std.testing.expectEqualStrings("required for run x/y", r[7].def);
    try std.testing.expectEqualStrings("null", r[8].def);
    try std.testing.expectEqual(@as(usize, 2), r[9].names.len);
    try std.testing.expectEqualStrings("height", r[9].names[1]);
}

test "parseAll keeps going: reported fields keep defaults" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    var diags = model.Diags.init(a);
    var p = try testParams(a, &diags, "{\"run\":\"w\",\"length\":3}");
    const v = parseAll(Sample, &p);
    try std.testing.expect(!p.ok);
    try std.testing.expectEqual(.z, v.run);
    try std.testing.expectEqualStrings("", v.size);
    try std.testing.expectEqual(@as(f64, 3), v.length.?);
}
