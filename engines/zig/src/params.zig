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
        const fields = @typeInfo(E).@"enum".fields;
        var out: [fields.len][]const u8 = undefined;
        for (fields, 0..) |f, i| out[i] = f.name;
        const final = out;
        break :blk &final;
    };
}

/// Check the struct and its `spec` agree (compile errors that name the offender).
fn checkSpec(comptime T: type) void {
    comptime {
        const fields = @typeInfo(T).@"struct".fields;
        if (@hasDecl(T, "spec")) {
            for (@typeInfo(@TypeOf(T.spec)).@"struct".fields) |sf| {
                if (!@hasField(T, sf.name)) @compileError(@typeName(T) ++ ".spec names '" ++ sf.name ++ "', which is not a field");
            }
        }
        for (fields) |f| {
            const s = specOf(T, f.name);
            if (!@hasField(@TypeOf(s), "desc")) @compileError(@typeName(T) ++ "." ++ f.name ++ " has no `spec` entry with a .desc (it would be missing from the catalog)");
            const Base = Unwrap(f.type);
            if (@typeInfo(Base) == .int) {
                if (!@hasField(@TypeOf(s), "min") or !@hasField(@TypeOf(s), "max")) @compileError(@typeName(T) ++ "." ++ f.name ++ " is an integer: its spec needs .min and .max");
                if (s.min < std.math.minInt(Base) or s.max > std.math.maxInt(Base)) @compileError(@typeName(T) ++ "." ++ f.name ++ ": .min/.max do not fit the field type");
            }
            if (Base == json.Value and f.defaultValue() == null and !isOptional(f.type) and !@hasField(@TypeOf(s), "def")) {
                @compileError(@typeName(T) ++ "." ++ f.name ++ ": a raw json.Value field needs a default, `?json.Value`, or an explicit .def");
            }
        }
    }
}

/// Read every field of `T` from the component's JSON. Returns null when any parameter was reported as an E_PARAM.
pub fn parse(comptime T: type, p: *Params) ?T {
    comptime checkSpec(T);
    var out: T = undefined;
    inline for (@typeInfo(T).@"struct".fields) |f| readField(T, f, p, &@field(out, f.name));
    return if (p.ok) out else null;
}

fn readField(comptime T: type, comptime f: std.builtin.Type.StructField, p: *Params, dst: *f.type) void {
    const s = comptime specOf(T, f.name);
    const Base = comptime Unwrap(f.type);
    const default: ?Base = comptime if (f.defaultValue()) |d| (if (isOptional(f.type)) d else d) else null;
    if (comptime isOptional(f.type)) {
        // `?X`: absent (or null) is fine; present must be a valid X. A default other than null is allowed too.
        if (!p.has(f.name)) {
            dst.* = default;
        } else if (readValue(Base, f.name, s, p, null)) |v| {
            dst.* = v;
        }
    } else if (readValue(Base, f.name, s, p, default)) |v| {
        dst.* = v;
    }
}

/// One value of type `Base`; `default == null` means required. Null result: reported (or, for json.Value, required and absent).
fn readValue(comptime Base: type, comptime name: []const u8, comptime s: anytype, p: *Params, comptime default: ?Base) ?Base {
    switch (@typeInfo(Base)) {
        .bool => return p.boolean(name, default orelse @compileError("bool param '" ++ name ++ "' needs a default")),
        .float => {
            const hint = comptime opt(s, "hint", @as([]const u8, ""));
            if (comptime @hasField(@TypeOf(s), "len")) {
                const d: ?f64 = default;
                return if (s.len == .pos) p.lenPos(name, d, hint) else p.len(name, d, hint);
            }
            return p.num(name, default);
        },
        .int => {
            const d: ?i64 = if (default) |x| @as(i64, x) else null;
            const v = p.int(name, d, s.min, s.max) orelse return null;
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
            return p.str(name, default);
        },
        .@"union" => {
            if (comptime Base != json.Value) @compileError("unsupported param type " ++ @typeName(Base));
            if (p.raw(name)) |v| return v;
            if (default) |d| return d;
            p.fail(name, "param '{s}' is required for type {s}", .{ name, p.ty });
            return null;
        },
        else => @compileError("unsupported param type " ++ @typeName(Base)),
    }
}

// ---- the catalog table --------------------------------------------------------------------------------

fn defaultText(comptime T: type, comptime f: std.builtin.Type.StructField) []const u8 {
    comptime {
        const s = specOf(T, f.name);
        if (@hasField(@TypeOf(s), "def")) return s.def;
        const Base = Unwrap(f.type);
        const d = f.defaultValue() orelse return if (isOptional(f.type)) "null" else "required";
        if (isOptional(f.type)) {
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
        for (@typeInfo(T).@"struct".fields) |f| {
            if (opt(specOf(T, f.name), "row", true)) n += 1;
        }
        return n;
    }
}

/// The catalog rows of `T`, in field order (comptime; the result is a constant slice).
pub fn rows(comptime T: type) []const Row {
    return comptime blk: {
        checkSpec(T);
        var out: [rowCount(T)]Row = undefined;
        var i: usize = 0;
        for (@typeInfo(T).@"struct".fields) |f| {
            const s = specOf(T, f.name);
            if (!opt(s, "row", true)) continue;
            const also = opt(s, "also", &[_][]const u8{});
            var names: [1 + also.len][]const u8 = undefined;
            names[0] = f.name;
            for (also, 0..) |nm, k| names[1 + k] = nm;
            const final_names = names;
            out[i] = .{ .names = &final_names, .def = defaultText(T, f), .desc = s.desc };
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
