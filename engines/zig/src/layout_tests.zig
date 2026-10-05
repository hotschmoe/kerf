//! Layout quality tests (SPEC 18): note case transform, leader routing (no crossings, no W_LEADER_HIT),
//! auto landing away from crop edges, W_VIEW_FIT detail, determinism.

const std = @import("std");
const json = @import("json.zig");
const geom = @import("geom.zig");
const model = @import("model.zig");
const style_mod = @import("style.zig");
const drawview = @import("drawview.zig");
const drawing = @import("drawing.zig");
const view_mod = @import("view.zig");
const route = @import("route.zig");
const testdocs = @import("testdocs.zig");
const V2 = geom.V2;

fn build(a: std.mem.Allocator, src: []const u8, view_id: []const u8) !drawing.Drawing {
    var err: json.ParseError = undefined;
    const doc = (try json.parse(a, src, &err)).?;
    const st = try a.create(style_mod.Style);
    st.* = try style_mod.load(a, null);
    var diags = model.Diags.init(a);
    return (try drawview.build(a, doc, st, view_id, &diags)).?;
}

fn findDiag(dr: drawing.Drawing, code: []const u8) ?model.Diag {
    for (dr.diagnostics) |d| if (std.mem.eql(u8, d.code, code)) return d;
    return null;
}

const Leader = struct { src: []const u8, l: [3]V2 };

fn leadersOf(a: std.mem.Allocator, dr: drawing.Drawing) ![]Leader {
    var out: std.ArrayList(Leader) = .empty;
    for (dr.items) |it| {
        if (it != .path) continue;
        const p = it.path;
        if (p.closed or p.pts.len != 3 or !std.mem.eql(u8, p.pen, "anno")) continue;
        if (std.mem.startsWith(u8, p.src, "title:") or std.mem.eql(u8, p.src, "sheet")) continue;
        try out.append(a, .{ .src = p.src, .l = .{ p.pts[0].v(), p.pts[1].v(), p.pts[2].v() } });
    }
    return out.items;
}

/// Number of leader pairs that cross (distance 0) or come within one text height.
fn leaderConflicts(a: std.mem.Allocator, dr: drawing.Drawing, h: f64) !struct { cross: usize, near: usize } {
    const ls = try leadersOf(a, dr);
    var cross: usize = 0;
    var near: usize = 0;
    for (ls, 0..) |x, i| for (ls[i + 1 ..]) |y| {
        const d = route.polyPolyDist(x.l, y.l);
        if (d <= 1e-9) cross += 1 else if (d < h) near += 1;
    };
    return .{ .cross = cross, .near = near };
}

test "every reference view (and the PSL / Palmer details) has no crossing leaders and no layout warnings" {
    for (testdocs.layout_docs) |src| {
        var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
        defer arena.deinit();
        const a = arena.allocator();
        var err: json.ParseError = undefined;
        const doc = (try json.parse(a, src, &err)).?;
        const st = try style_mod.load(a, null);
        for (try view_mod.viewIds(a, doc)) |vid| {
            const dr = try build(a, src, vid);
            if (findDiag(dr, "W_LEADER_HIT")) |d| {
                std.debug.print("view {s}: {s}\n", .{ vid, d.message });
                return error.TestUnexpectedResult;
            }
            if (findDiag(dr, "W_VIEW_FIT")) |d| {
                std.debug.print("view {s}: {s}\n", .{ vid, d.message });
                return error.TestUnexpectedResult;
            }
            const c = try leaderConflicts(a, dr, st.text_height_in * dr.scale);
            try std.testing.expectEqual(@as(usize, 0), c.cross);
            try std.testing.expectEqual(@as(usize, 0), c.near);
        }
    }
}

test "layout is deterministic (byte-identical drawing JSON on repeated builds)" {
    const gpa = std.testing.allocator;
    const api = @import("api.zig");
    const doc = std.mem.trim(u8, testdocs.psl, " \n\r\t");
    const input = try std.fmt.allocPrint(gpa, "{{\"doc\":{s},\"view\":\"A\"}}", .{doc});
    defer gpa.free(input);
    const r1 = try api.call(gpa, "drawing", input);
    defer gpa.free(r1.bytes);
    const r2 = try api.call(gpa, "drawing", input);
    defer gpa.free(r2.bytes);
    try std.testing.expect(r1.ok);
    try std.testing.expectEqualSlices(u8, r1.bytes, r2.bytes);
}

const two_posts =
    \\{"kerf":"0.1","id":"t","run":[-3.5,0],"components":[
    \\ {"id":"a","type":"lumber","size":"2x4","run":"y","face":"narrow","length":40,"at":{"anchor":"bottom_left","to":[0,0]}},
    \\ {"id":"b","type":"lumber","size":"2x4","run":"y","face":"narrow","length":10,"at":{"anchor":"bottom_left","to":[20,0]}}],
    \\"views":[{"id":"A","kind":"section","number":"1","title":"T","scale":"1\"=1'-0\"","cut_z":0,
    \\ "crop":{"x":[-5,30],"y":[-5,45]},"notes_side":"right","annotations":[
    \\ {"id":"n1","type":"note","text":"ONE","target":"a","place":[40,0]},
    \\ {"id":"n2","type":"note","text":"TWO","target":"b","place":[40,40]}
    \\ ]}]}
;

const two_posts_free =
    \\{"kerf":"0.1","id":"t","run":[-3.5,0],"components":[
    \\ {"id":"a","type":"lumber","size":"2x4","run":"y","face":"narrow","length":40,"at":{"anchor":"bottom_left","to":[0,0]}},
    \\ {"id":"b","type":"lumber","size":"2x4","run":"y","face":"narrow","length":10,"at":{"anchor":"bottom_left","to":[20,0]}}],
    \\"views":[{"id":"A","kind":"section","number":"1","title":"T","scale":"1\"=1'-0\"","cut_z":0,
    \\ "crop":{"x":[-5,30],"y":[-5,45]},"notes_side":"right","annotations":[
    \\ {"id":"n1","type":"note","text":"ONE","target":"a"},
    \\ {"id":"n2","type":"note","text":"TWO","target":"b"}
    \\ ]}]}
;

test "W_LEADER_HIT: crossing leaders of designer-placed notes name both notes and propose a fix" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const dr = try build(a, two_posts, "A");
    const d = findDiag(dr, "W_LEADER_HIT") orelse return error.TestUnexpectedResult;
    try std.testing.expect(std.mem.indexOf(u8, d.message, "'n1'") != null);
    try std.testing.expect(std.mem.indexOf(u8, d.message, "'n2'") != null);
    try std.testing.expect(std.mem.indexOf(u8, d.message, "crosses") != null);
    const fix = d.fix orelse return error.TestUnexpectedResult;
    try std.testing.expect(std.mem.indexOf(u8, fix, "\"place\"") != null or std.mem.indexOf(u8, fix, "\"at\"") != null);
}

test "W_LEADER_HIT: not raised when the router can place the notes freely" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const dr = try build(a, two_posts_free, "A");
    try std.testing.expect(findDiag(dr, "W_LEADER_HIT") == null);
    const st = try style_mod.load(a, null);
    const c = try leaderConflicts(a, dr, st.text_height_in * dr.scale);
    try std.testing.expectEqual(@as(usize, 0), c.cross + c.near);
}

test "SPEC 20 repair: a label sitting on a leader is moved off it (no W_LEADER_HIT)" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    // n1's leader runs from (40,0) to post a; a label sits right on it
    const src = try std.mem.replaceOwned(u8, a, two_posts,
        \\{"id":"n2","type":"note","text":"TWO","target":"b","place":[40,40]}
    ,
        \\{"id":"l1","type":"label","text":"MID","at":[20,10]}
    );
    const dr = try build(a, src, "A");
    try std.testing.expect(findDiag(dr, "W_LEADER_HIT") == null);
    var moved = false;
    for (dr.items) |it| if (it == .text and std.mem.eql(u8, it.text.src, "l1")) {
        moved = @abs(it.text.x - 20) > 1e-6 or @abs(it.text.y - 10) > 1e-6;
    };
    try std.testing.expect(moved);
}

test "note and citation text share the style case transform" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const src = try std.mem.replaceOwned(u8, a, two_posts_free,
        \\{"id":"n1","type":"note","text":"ONE","target":"a"},
    ,
        \\{"id":"n1","type":"note","text":"Stud wall","target":"a","cite":[{"code":"IRC","edition":2021,"section":"Table R602.3(5)","status":"suggested"}]},
    );
    const dr = try build(a, src, "A");
    var joined: std.ArrayList(u8) = .empty;
    for (dr.items) |it| if (it == .text and std.mem.eql(u8, it.text.src, "n1")) {
        try joined.appendSlice(a, it.text.s);
        try joined.append(a, ' ');
    };
    try std.testing.expect(std.mem.indexOf(u8, joined.items, "TABLE R602.3(5)") != null);
    for (joined.items) |c| try std.testing.expect(!(c >= 'a' and c <= 'z'));
}

test "W_VIEW_FIT reports overflow per edge, the culprit and the smallest fix first" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const src = try std.mem.replaceOwned(u8, a, testdocs.beam, "\"scale\": \"1\\\"=1'-0\\\"\"", "\"scale\": \"3\\\"=1'-0\\\"\"");
    const dr = try build(a, src, "A");
    const d = findDiag(dr, "W_VIEW_FIT") orelse return error.TestUnexpectedResult;
    for ([_][]const u8{ "left ", "right ", "top ", "bottom ", "Overflow by edge", "notes column", "Width:" }) |needle| {
        if (std.mem.indexOf(u8, d.message, needle) == null) {
            std.debug.print("missing '{s}' in: {s}\n", .{ needle, d.message });
            return error.TestUnexpectedResult;
        }
    }
    const fix = d.fix.?;
    // smallest fix first: the crop, and the scale only because the overflow is > 15% of the frame
    const crop_at = std.mem.indexOf(u8, fix, "narrow crop.x") orelse return error.TestUnexpectedResult;
    const scale_at = std.mem.indexOf(u8, fix, "smaller scale") orelse return error.TestUnexpectedResult;
    try std.testing.expect(crop_at < scale_at);
}

test "W_VIEW_FIT: a small overflow suggests the crop, not a scale change" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const src = try std.mem.replaceOwned(u8, a, testdocs.beam, "-28,\n          24", "-28,\n          40");
    const dr = try build(a, src, "A");
    const d = findDiag(dr, "W_VIEW_FIT") orelse return error.TestUnexpectedResult;
    try std.testing.expect(std.mem.indexOf(u8, d.message, "left 0.20\"") != null);
    try std.testing.expect(std.mem.indexOf(u8, d.message, "bottom 0.00\"") != null);
    try std.testing.expect(std.mem.indexOf(u8, d.fix.?, "narrow crop.x by 4.9 in") != null);
    try std.testing.expect(std.mem.indexOf(u8, d.fix.?, "smaller scale") == null);
}

const long_member =
    \\{"kerf":"0.1","id":"t","run":[-3.5,0],"components":[
    \\ {"id":"m","type":"lumber","size":"2x6","run":"x","face":"wide","length":100,"at":{"anchor":"bottom_left","to":[0,0]}}],
    \\"views":[{"id":"A","kind":"section","number":"1","title":"T","scale":"1\"=1'-0\"","cut_z":0,
    \\ "crop":{"x":[-30,4],"y":[-10,10]},"notes_side":"left","annotations":[
    \\ {"id":"n1","type":"note","text":"LONG MEMBER RUNNING PAST THE CROP","target":"m"}
    \\ ]}]}
;

test "auto landing keeps the arrow away from the crop edge (break line)" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const st = try style_mod.load(a, null);
    const dr = try build(a, long_member, "A");
    const band = 2.0 * st.text_height_in * dr.scale;
    // the member is visible for x in [0, 4]; its centroid (x = 2) is inside the 2-text-height band of the crop edge
    var tip: ?V2 = null;
    for (dr.items) |it| {
        if (it == .fill and std.mem.eql(u8, it.fill.src, "n1")) tip = it.fill.loops[0][0].v();
    }
    const t = tip orelse return error.TestUnexpectedResult;
    // balanced between the member end (x = 0) and the break line (x = 4): at least 1.5 in from both
    try std.testing.expect(t.x >= 1.5 and dr.crop.x1 - t.x >= 1.5);
    _ = band;
}
