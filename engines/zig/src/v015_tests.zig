//! Tests of the v0.1.5 additions (SPEC 21): lenient ops input, requested-elements coverage, W_CROP_STALE,
//! thin-layer note landing, note text lint.

const std = @import("std");
const json = @import("json.zig");
const model = @import("model.zig");
const ops_mod = @import("ops.zig");

fn parse(a: std.mem.Allocator, src: []const u8) !json.Value {
    var err: json.ParseError = undefined;
    return (try json.parse(a, src, &err)).?;
}

test "ops input: array, single op, {ops} and {ops, why} are accepted; anything else is E_PARAM naming what it got" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    var diags = model.Diags.init(a);
    const arr = (try ops_mod.normalize(a, try parse(a, "[{\"op\":\"remove\",\"path\":\"components/x\"}]"), &diags)).?;
    try std.testing.expectEqual(@as(usize, 1), arr.ops.arr().?.len);
    try std.testing.expect(arr.why == null);
    const one = (try ops_mod.normalize(a, try parse(a, "{\"op\":\"remove\",\"path\":\"components/x\"}"), &diags)).?;
    try std.testing.expectEqual(@as(usize, 1), one.ops.arr().?.len);
    const env = (try ops_mod.normalize(a, try parse(a, "{\"ops\":[{\"op\":\"remove\",\"path\":\"a\"},{\"op\":\"remove\",\"path\":\"b\"}],\"why\":\"tidy\"}"), &diags)).?;
    try std.testing.expectEqual(@as(usize, 2), env.ops.arr().?.len);
    try std.testing.expectEqualStrings("tidy", env.why.?);
    try std.testing.expectEqual(@as(usize, 0), diags.list.items.len);
    try std.testing.expect((try ops_mod.normalize(a, try parse(a, "{\"path\":\"components\",\"value\":1}"), &diags)) == null);
    try std.testing.expect((try ops_mod.normalize(a, try parse(a, "\"nope\""), &diags)) == null);
    try std.testing.expectEqual(@as(usize, 2), diags.list.items.len);
    try std.testing.expectEqualStrings("E_PARAM", diags.list.items[0].code);
    try std.testing.expect(std.mem.indexOf(u8, diags.list.items[0].message, "an object with keys path, value") != null);
    try std.testing.expect(std.mem.indexOf(u8, diags.list.items[0].message, "single op object") != null);
    try std.testing.expect(std.mem.indexOf(u8, diags.list.items[1].message, "got a string") != null);
}

test "api apply takes a single op object and the {ops, why} envelope" {
    const api = @import("api.zig");
    const gpa = std.testing.allocator;
    const doc = "{\"kerf\":\"0.1\",\"id\":\"t\",\"components\":[],\"views\":[]}";
    const op = "{\"op\":\"add\",\"path\":\"components\",\"value\":{\"id\":\"s\",\"type\":\"lumber\",\"size\":\"2x4\"}}";
    inline for (.{ op, "{\"ops\":[" ++ op ++ "],\"why\":\"x\"}" }) |ops_src| {
        const input = try std.fmt.allocPrint(gpa, "{{\"doc\":{s},\"ops\":{s}}}", .{ doc, ops_src });
        defer gpa.free(input);
        const r = try api.call(gpa, "apply", input);
        defer gpa.free(r.bytes);
        try std.testing.expect(r.ok);
        try std.testing.expect(std.mem.startsWith(u8, r.bytes, "{\"ok\":true"));
    }
    const bad = try api.call(gpa, "apply", "{\"doc\":{\"kerf\":\"0.1\",\"id\":\"t\"},\"ops\":{\"foo\":1}}");
    defer gpa.free(bad.bytes);
    try std.testing.expect(std.mem.indexOf(u8, bad.bytes, "\"ok\":false") != null and std.mem.indexOf(u8, bad.bytes, "E_PARAM") != null);
}
