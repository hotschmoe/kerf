//! Tests of the v0.1.5 additions (SPEC 21): lenient ops input, requested-elements coverage, W_CROP_STALE,
//! thin-layer note landing, note text lint.

const std = @import("std");
const json = @import("json.zig");
const model = @import("model.zig");
const ops_mod = @import("ops.zig");
const style_mod = @import("style.zig");
const load_mod = @import("load.zig");

fn loadSrc(a: std.mem.Allocator, src: []const u8, with_views: bool) !load_mod.Loaded {
    const doc = try parse(a, src);
    const st = try a.create(style_mod.Style);
    st.* = try style_mod.load(a, null);
    return load_mod.load(a, doc, st, with_views);
}

fn count(l: load_mod.Loaded, code: []const u8) usize {
    var n: usize = 0;
    for (l.diags.list.items) |d| if (std.mem.eql(u8, d.code, code)) {
        n += 1;
    };
    return n;
}

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

test "coverage: items match component id/type/label/model and note text; the summary prints COVERAGE and missing items warn" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const src =
        \\{"kerf":"0.1","id":"t","meta":{"requested":["cmu wall","Bond Beam","H2.5A ties","2x6 sill plates","vapor retarder","(2) #5 bott"]},
        \\"components":[
        \\{"id":"wall","type":"cmu_wall","width":8,"courses":3},
        \\{"id":"bb","type":"lumber","size":"2x6","orient":"flat","label":"BOND BEAM","at":{"anchor":"bottom_left","to":"wall@top_left"}},
        \\{"id":"tie","type":"connector","model":"H2.5A","at":{"anchor":"bottom_left","to":"bb@top_left"}}],
        \\"views":[{"id":"A","annotations":[
        \\{"id":"n1","type":"note","text":"2X6 PT SILL PLATE","target":"bb"},
        \\{"id":"n2","type":"note","text":"(2) #5 CONT. BOTT.","target":"wall.course1"}]}]}
    ;
    const l = try loadSrc(a, src, false);
    const items = try @import("coverage.zig").compute(a, l.doc);
    try std.testing.expectEqual(@as(usize, 6), items.len);
    try std.testing.expectEqualStrings("wall", items[0].found[0]); // type cmu_wall
    try std.testing.expectEqualStrings("bb", items[1].found[0]); // label, case-insensitive
    try std.testing.expectEqualStrings("tie", items[2].found[0]); // model H2.5A ("ties" vs the connector: only "h2.5a" words, "tie" is the id)
    try std.testing.expectEqualStrings("bb", items[3].found[0]); // note text, plural ignored; resolves to the note target
    try std.testing.expectEqual(@as(usize, 0), items[4].found.len); // vapor retarder: nothing
    try std.testing.expectEqualStrings("wall", items[5].found[0]); // note target "wall.course1" -> component wall
    try std.testing.expectEqual(@as(usize, 1), count(l, "W_REQUESTED_MISSING"));
    const sum = try load_mod.summary(&l);
    try std.testing.expect(std.mem.indexOf(u8, sum, "COVERAGE  5/6 requested elements covered") != null);
    try std.testing.expect(std.mem.indexOf(u8, sum, " MISSING  vapor retarder\n") != null);
    try std.testing.expect(std.mem.indexOf(u8, sum, " ok       cmu wall") != null);
    try std.testing.expect(std.mem.indexOf(u8, sum, "WARN W_REQUESTED_MISSING") != null);
    // full coverage: no block warning, the header says so
    const full = try loadSrc(a, "{\"kerf\":\"0.1\",\"id\":\"t\",\"meta\":{\"requested\":[\"stud\"]},\"components\":[{\"id\":\"stud\",\"type\":\"lumber\",\"size\":\"2x4\"}],\"views\":[]}", false);
    try std.testing.expectEqual(@as(usize, 0), full.diags.warnCount());
    try std.testing.expect(std.mem.indexOf(u8, try load_mod.summary(&full), "COVERAGE  1/1 requested elements covered\n") != null);
    // no meta.requested: no block (the reference documents are unchanged)
    const none = try loadSrc(a, "{\"kerf\":\"0.1\",\"id\":\"t\",\"components\":[],\"views\":[]}", false);
    try std.testing.expect(std.mem.indexOf(u8, try load_mod.summary(&none), "COVERAGE") == null);
}

test "meta.requested must be an array of non-empty strings" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const l1 = try loadSrc(a, "{\"kerf\":\"0.1\",\"id\":\"t\",\"meta\":{\"requested\":\"cmu wall, bond beam\"},\"components\":[],\"views\":[]}", false);
    try std.testing.expectEqual(@as(usize, 1), count(l1, "E_PARAM"));
    const l2 = try loadSrc(a, "{\"kerf\":\"0.1\",\"id\":\"t\",\"meta\":{\"requested\":[\"x\",\"\"]},\"components\":[],\"views\":[]}", false);
    try std.testing.expectEqual(@as(usize, 1), count(l2, "E_PARAM"));
}
