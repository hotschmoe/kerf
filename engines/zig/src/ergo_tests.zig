//! Tests of the v0.1.3 agent ergonomics (SPEC 19): schema vs catalog, lints, acknowledge, barrier, coupled roof geometry.

const std = @import("std");
const json = @import("json.zig");
const geom = @import("geom.zig");
const model = @import("model.zig");
const style_mod = @import("style.zig");
const compile_mod = @import("compile.zig");
const load_mod = @import("load.zig");
const catalog = @import("catalog.zig");
const schema = @import("schema.zig");
const scene_mod = @import("scene.zig");
const V2 = geom.V2;

fn loadSrc(a: std.mem.Allocator, src: []const u8, with_views: bool) !load_mod.Loaded {
    var err: json.ParseError = undefined;
    const doc = (try json.parse(a, src, &err)).?;
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

test "the example document checks with no errors and no warnings; every catalog example compiles" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const l = try loadSrc(a, schema.example_doc, true);
    for (l.diags.list.items) |d| try std.testing.expect(d.level == .info);
    for (catalog.entries) |e| {
        const src = try std.fmt.allocPrint(a, "{{\"kerf\":\"0.1\",\"id\":\"x\",\"components\":[{s}],\"views\":[]}}", .{e.example});
        const lx = try loadSrc(a, src, false);
        errdefer std.debug.print("example of {s} failed: {s}\n", .{ e.name(), if (lx.diags.list.items.len > 0) lx.diags.list.items[0].message else "" });
        try std.testing.expectEqual(@as(usize, 0), lx.diags.errCount());
        try std.testing.expect(lx.scene.comps.len == 1 and lx.scene.comps[0].state == .ok);
    }
}

test "schema is the single source: every field the canonical writer, lint and parsers use is a schema field" {
    // view fields the parser reads
    for ([_][]const u8{ "id", "kind", "number", "title", "scale", "cut_z", "crop", "from", "cutaway", "notes_side", "omit", "annotations" }) |k| {
        try std.testing.expect(schema.hasField(&schema.view, k));
    }
    for ([_][]const u8{ "id", "type", "text", "target", "at", "place", "cite" }) |k| try std.testing.expect(schema.hasField(&schema.note, k));
    for ([_][]const u8{ "id", "type", "from", "to", "dir", "offset", "text" }) |k| try std.testing.expect(schema.hasField(&schema.dim, k));
    for ([_][]const u8{ "id", "type", "text", "at", "offset" }) |k| try std.testing.expect(schema.hasField(&schema.label, k));
    for ([_][]const u8{ "code", "edition", "section", "title", "status" }) |k| try std.testing.expect(schema.hasField(&schema.cite, k));
}

test "unknown keys warn with suggestions and are kept; reference docs warn about none" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const src =
        \\{"kerf":"0.1","id":"t","components":[{"id":"s","type":"lumber","size":"2x4","run":"y","length":10}],"views":[
        \\{"id":"A","scale":"1\"=1'-0\"","crop":{"x":[-5,10],"y":[-2,12]},"side":"left","annotations":[
        \\{"id":"n","type":"note","text":"2X4 STUD","target":"s","citations":[],"point":[1,1],"kind":"note","bogus":1}]}]}
    ;
    const l = try loadSrc(a, src, true);
    try std.testing.expectEqual(@as(usize, 5), count(l, "W_UNKNOWN_KEY"));
    var saw_cite = false;
    var saw_at = false;
    for (l.diags.list.items) |d| {
        if (!std.mem.eql(u8, d.code, "W_UNKNOWN_KEY")) continue;
        if (std.mem.indexOf(u8, d.message, "\"citations\"") != null and std.mem.indexOf(u8, d.message, "Did you mean \"cite\"") != null) saw_cite = true;
        if (std.mem.indexOf(u8, d.message, "\"point\"") != null and std.mem.indexOf(u8, d.message, "Did you mean \"at\"") != null) saw_at = true;
    }
    try std.testing.expect(saw_cite and saw_at);
    // preserved
    try std.testing.expect(l.doc.get("views").?.arr().?[0].get("annotations").?.arr().?[0].get("bogus") != null);
}

test "dim dir defaults to the dominant axis and W_DIM_ZERO fires under 1/16 inch" {
    const lint = @import("lint.zig");
    try std.testing.expectEqualStrings("h", lint.dominantDir(V2.init(0, 0), V2.init(5, 5)));
    try std.testing.expectEqualStrings("v", lint.dominantDir(V2.init(0, 0), V2.init(1, -5)));
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const src =
        \\{"kerf":"0.1","id":"t","components":[{"id":"s","type":"lumber","size":"2x4","run":"y","length":10}],"views":[
        \\{"id":"A","scale":"1\"=1'-0\"","crop":{"x":[-5,10],"y":[-2,12]},"annotations":[
        \\{"id":"d1","type":"dim","from":"s@bottom_left","to":"s@top_left","offset":-3},
        \\{"id":"d2","type":"dim","from":"s@bottom_left","to":"s@top_left","dir":"h","offset":-3}]}]}
    ;
    const l = try loadSrc(a, src, true);
    try std.testing.expectEqual(@as(usize, 1), count(l, "W_DIM_ZERO"));
    for (l.diags.list.items) |d| if (std.mem.eql(u8, d.code, "W_DIM_ZERO")) try std.testing.expectEqualStrings("d2", d.id.?);
}

test "coupled roof geometry: slope @truss, until along the slope, truss anchors, mirrored truss" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    for ([_][]const u8{ "left", "right" }) |ext| {
        const src = try std.fmt.allocPrint(a,
            \\{{"kerf":"0.1","id":"t","components":[
            \\{{"id":"truss","type":"truss","pitch":"6:12","exterior":"{s}","span_shown":40,"at":{{"anchor":"bearing_outer","to":[0,0]}}}},
            \\{{"id":"deck","type":"panel","thickness":0.5,"slope":"@truss","until":"truss@top_chord_end","at":{{"anchor":"{s}","to":"truss@tail_top"}}}},
            \\{{"id":"roofing","type":"membrane","material":"shingles","slope":"@truss","points":[[0,0],[{s}10,0]],"until":"truss@top_chord_end","at":{{"to":"truss@tail_top"}}}}
            \\],"views":[]}}
        , .{ ext, if (std.mem.eql(u8, ext, "left")) "bottom_left" else "bottom_right", if (std.mem.eql(u8, ext, "left")) "" else "-" });
        const l = try loadSrc(a, src, false);
        try std.testing.expectEqual(@as(usize, 0), l.diags.errCount());
        const truss = l.scene.find("truss").?;
        const deck = l.scene.find("deck").?;
        const end = scene_mod.Scene.anchorPoint(truss, 0, null, "top_chord_end").?;
        const th: f64 = std.math.atan(@as(f64, 0.5)) * (if (std.mem.eql(u8, ext, "left")) @as(f64, 1) else -1);
        try std.testing.expectApproxEqAbs(th, deck.angle, 1e-9);
        // the far end of the deck's run reaches the truss end measured along the slope
        const d = V2.init(@cos(th), @sin(th));
        const far = scene_mod.Scene.anchorPoint(deck, 0, null, if (std.mem.eql(u8, ext, "left")) "bottom_right" else "bottom_left").?;
        try std.testing.expectApproxEqAbs(end.x * d.x + end.y * d.y, far.x * d.x + far.y * d.y, 1e-6);
        // heel anchors sit on the bearing plane at x = 0 (mirror-independent)
        const heel = scene_mod.Scene.anchorPoint(truss, 0, null, "heel_outer").?;
        const low = scene_mod.Scene.anchorPoint(truss, 0, null, "top_chord_bottom_at_bearing").?;
        const hi = scene_mod.Scene.anchorPoint(truss, 0, null, "top_chord_at_bearing").?;
        try std.testing.expectApproxEqAbs(@as(f64, 0), heel.x, 1e-9);
        try std.testing.expectApproxEqAbs(@as(f64, 3.5), low.y, 1e-9);
        try std.testing.expect(heel.y > 3.5 and heel.y < hi.y);
        // the membrane's last segment ends at the same projection
        const roof = l.scene.find("roofing").?;
        const mend = roof.built.prisms[0].centerline[1];
        const mw = roof.xfs[0].apply(V2.init(mend.x, mend.y));
        try std.testing.expectApproxEqAbs(end.x * d.x + end.y * d.y, mw.x * d.x + mw.y * d.y, 1e-6);
    }
    // errors: unknown component, until behind the start
    const bad = try loadSrc(a, "{\"kerf\":\"0.1\",\"id\":\"t\",\"components\":[{\"id\":\"deck\",\"type\":\"panel\",\"thickness\":0.5,\"length\":10,\"slope\":\"@trus\"}],\"views\":[]}", false);
    try std.testing.expect(bad.diags.errCount() >= 1 and count(bad, "E_REF_UNKNOWN") >= 1);
}

test "removing a component that another follows with slope @ is refused by ops" {
    const ops = @import("ops.zig");
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    var err: json.ParseError = undefined;
    const doc = (try json.parse(a, "{\"kerf\":\"0.1\",\"id\":\"t\",\"components\":[{\"id\":\"truss\",\"type\":\"truss\"},{\"id\":\"deck\",\"type\":\"panel\",\"thickness\":0.5,\"length\":10,\"slope\":\"@truss\"}],\"views\":[]}", &err)).?;
    const opv = (try json.parse(a, "[{\"op\":\"remove\",\"path\":\"components/truss\"}]", &err)).?;
    var diags = model.Diags.init(a);
    const r = try ops.apply(a, doc, opv, .llm, &diags);
    try std.testing.expect(r == null and diags.hasCode("E_REF_UNKNOWN"));
}

test "acknowledge replaces the warning with I_ACK; barrier clears the contact warning" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const base =
        \\{{"kerf":"0.1","id":"t","components":[
        \\{{"id":"cmu","type":"cmu_wall","width":8,"courses":2,"at":{{"to":[0,0]}}}},
        \\{{"id":"sill","type":"lumber","size":"2x6","orient":"flat"{s},"at":{{"anchor":"bottom_left","to":"cmu@top_left"}}}}],"views":[]}}
    ;
    const plain = try loadSrc(a, try std.fmt.allocPrint(a, base, .{""}), false);
    try std.testing.expectEqual(@as(usize, 1), count(plain, "W_UNTREATED_CONTACT"));
    const acked = try loadSrc(a, try std.fmt.allocPrint(a, base, .{",\"acknowledge\":[{\"code\":\"W_UNTREATED_CONTACT\",\"reason\":\"mfr barrier\"}]"}), false);
    try std.testing.expectEqual(@as(usize, 0), count(acked, "W_UNTREATED_CONTACT"));
    try std.testing.expectEqual(@as(usize, 1), count(acked, "I_ACK"));
    const sealed = try loadSrc(a, try std.fmt.allocPrint(a, base, .{",\"barrier\":\"sill_seal\""}), false);
    try std.testing.expectEqual(@as(usize, 0), count(sealed, "W_UNTREATED_CONTACT"));
    try std.testing.expectEqual(@as(usize, 0), sealed.diags.errCount());
    const sill = sealed.scene.find("sill").?;
    try std.testing.expectApproxEqAbs(@as(f64, 1.625), sill.built.box.y1 - sill.built.box.y0, 1e-9);
    try std.testing.expectEqualStrings("barrier", sill.built.prisms[0].part);
    // only warnings can be acknowledged
    const bad = try loadSrc(a, try std.fmt.allocPrint(a, base, .{",\"acknowledge\":[{\"code\":\"E_PARAM\",\"reason\":\"x\"}]"}), false);
    try std.testing.expect(bad.diags.errCount() >= 1);
}

fn replaceAll(a: std.mem.Allocator, text: []const u8, from: []const u8, to: []const u8) ![]u8 {
    return std.mem.replaceOwned(u8, a, text, from, to);
}

test "W_SHORT_SLOPE: e07 repro warns with the until fix; reference truss and the until fix stay at 0 warnings" {
    const testdocs = @import("testdocs.zig");
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    // negative: the reference document
    const ref = try loadSrc(a, testdocs.truss, true);
    try std.testing.expectEqual(@as(usize, 0), count(ref, "W_SHORT_SLOPE"));
    try std.testing.expectEqual(@as(usize, 0), ref.diags.errCount());
    // e07 repro: pitch changed to 6:12, sheathing follows with slope "@truss" but keeps a literal length that no longer reaches the truss end
    var src = try replaceAll(a, testdocs.truss, "\"pitch\": \"4:12\"", "\"pitch\": \"6:12\"");
    src = try replaceAll(a, src, "\"slope\": \"4:12\"", "\"slope\": \"@truss\"");
    src = try replaceAll(a, src, "\"length\": 66", "\"length\": 40");
    const bad = try loadSrc(a, src, true);
    try std.testing.expectEqual(@as(usize, 1), count(bad, "W_SHORT_SLOPE"));
    for (bad.diags.list.items) |d| if (std.mem.eql(u8, d.code, "W_SHORT_SLOPE")) {
        try std.testing.expectEqualStrings("roof_sheathing", d.id.?);
        try std.testing.expect(std.mem.indexOf(u8, d.fix.?, "\"until\": \"truss@top_chord_end\"") != null);
        try std.testing.expect(std.mem.indexOf(u8, d.message, "short") != null);
    };
    // the literal length that is long enough, and `until` itself, do not warn
    const long = try loadSrc(a, try replaceAll(a, src, "\"length\": 40", "\"length\": 66"), true);
    try std.testing.expectEqual(@as(usize, 0), count(long, "W_SHORT_SLOPE"));
    const fixed = try loadSrc(a, try replaceAll(a, src, "\"length\": 40,", "\"until\": \"truss@top_chord_end\","), true);
    try std.testing.expectEqual(@as(usize, 0), fixed.diags.errCount());
    try std.testing.expectEqual(@as(usize, 0), count(fixed, "W_SHORT_SLOPE"));
    // it can be acknowledged
    const acked = try loadSrc(a, try replaceAll(a, src, "\"slope\": \"@truss\"", "\"slope\": \"@truss\", \"acknowledge\": [{\"code\": \"W_SHORT_SLOPE\", \"reason\": \"stops at the ridge block\"}]"), true);
    try std.testing.expectEqual(@as(usize, 0), count(acked, "W_SHORT_SLOPE"));
    try std.testing.expectEqual(@as(usize, 1), count(acked, "I_ACK"));
    // a roofing membrane that stops short of its sloped sheathing warns too
    const mem = try loadSrc(a,
        \\{"kerf":"0.1","id":"t","components":[
        \\{"id":"rafter","type":"lumber","size":"2x6","run":"x","length":120,"slope":"4:12","at":{"anchor":"bottom_left","to":[0,0]}},
        \\{"id":"deck","type":"panel","thickness":0.5,"length":120,"slope":"4:12","at":{"anchor":"bottom_left","to":"rafter@top_left"}},
        \\{"id":"roofing","type":"membrane","material":"shingles","slope":"4:12","points":[[0,0],[60,0]],"at":{"to":"deck@top_left"}}
        \\],"views":[]}
    , false);
    try std.testing.expectEqual(@as(usize, 0), mem.diags.errCount());
    try std.testing.expectEqual(@as(usize, 1), count(mem, "W_SHORT_SLOPE"));
}

test "acknowledge accepts I_* codes as a no-op" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const l = try loadSrc(a,
        \\{"kerf":"0.1","id":"t","components":[
        \\{"id":"blk","type":"solid","material":"steel","profile":{"rect":[3,0.25]},"acknowledge":[{"code":"I_SOLID_USED","reason":"no typed component fits"}]}
        \\],"views":[]}
    , false);
    try std.testing.expectEqual(@as(usize, 0), l.diags.errCount());
}
