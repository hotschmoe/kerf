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

// ---- SPEC 20: repair loop, column hint, place alignment, dimension stacking, outside text -----------------------------------

fn replaceAnn(a: std.mem.Allocator, src: []const u8, anns: []const u8) ![]u8 {
    // swap the annotations of `two_posts_free` (a view with two notes) for `anns`
    const old =
        \\ {"id":"n1","type":"note","text":"ONE","target":"a"},
        \\ {"id":"n2","type":"note","text":"TWO","target":"b"}
    ;
    return std.mem.replaceOwned(u8, a, src, old, anns);
}

fn textsOf(a: std.mem.Allocator, dr: drawing.Drawing, src: []const u8) ![]drawing.TextItem {
    var out: std.ArrayList(drawing.TextItem) = .empty;
    for (dr.items) |it| if (it == .text and std.mem.eql(u8, it.text.src, src)) try out.append(a, it.text);
    return out.items;
}

/// x of every vertical dimension line (2-point `dim` path with equal x) of dimension `id`.
fn vdimLineX(a: std.mem.Allocator, dr: drawing.Drawing, id: []const u8) !f64 {
    _ = a;
    for (dr.items) |it| {
        if (it != .path or !std.mem.eql(u8, it.path.src, id) or !std.mem.eql(u8, it.path.pen, "dim") or it.path.pts.len != 2) continue;
        const p = it.path.pts;
        if (@abs(p[0].x - p[1].x) < 1e-9 and @abs(p[0].y - p[1].y) > 1e-6) return p[0].x;
    }
    return error.TestUnexpectedResult;
}

test "SPEC 20 stacking: vertical dims with the same offset are stacked 0.25 paper inch apart, shortest nearest" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const src = try replaceAnn(a, two_posts_free,
        \\ {"id":"d_long","type":"dim","from":[0,0],"to":[0,40],"dir":"v","offset":-6},
        \\ {"id":"d_mid","type":"dim","from":[0,0],"to":[0,24],"dir":"v","offset":-6},
        \\ {"id":"d_short","type":"dim","from":[0,8],"to":[0,16],"dir":"v","offset":-6}
    );
    const dr = try build(a, src, "A");
    try std.testing.expect(findDiag(dr, "W_LEADER_HIT") == null);
    const xs = [3]f64{ try vdimLineX(a, dr, "d_short"), try vdimLineX(a, dr, "d_mid"), try vdimLineX(a, dr, "d_long") };
    // all dims are left of the post (x = 0); the authored -6 stays for the shortest, longer ones move out by whole 0.25" steps
    try std.testing.expectApproxEqAbs(@as(f64, -6), xs[0], 1e-6);
    const step = 0.25 * dr.scale;
    try std.testing.expect(xs[1] <= xs[0] - step + 1e-6);
    try std.testing.expect(xs[2] <= xs[1] - step + 1e-6);
    // no two dimension texts overprint each other
    const ids = [3][]const u8{ "d_short", "d_mid", "d_long" };
    const st = try style_mod.load(a, null);
    var polys: [3][4]V2 = undefined;
    for (ids, 0..) |id, i| {
        const ts = try textsOf(a, dr, id);
        try std.testing.expectEqual(@as(usize, 1), ts.len);
        polys[i] = @import("annot.zig").textPoly(&(try @import("font.zig").Font.parse(a, @import("font.zig").embedded)), ts[0], 0);
        _ = st;
    }
    for (0..3) |i| for (i + 1..3) |j| {
        try std.testing.expect(!@import("annot.zig").polysOverlap(&polys[i], &polys[j]));
    };
}

test "SPEC 20 stacking is deterministic and keeps the authored offsets of dims that do not conflict" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const src = try replaceAnn(a, two_posts_free,
        \\ {"id":"d1","type":"dim","from":[0,0],"to":[0,40],"dir":"v","offset":-6},
        \\ {"id":"d2","type":"dim","from":[0,0],"to":[0,24],"dir":"v","offset":-14}
    );
    const d1 = try build(a, src, "A");
    const d2 = try build(a, src, "A");
    try std.testing.expectApproxEqAbs(@as(f64, -6), try vdimLineX(a, d1, "d1"), 1e-9);
    try std.testing.expectApproxEqAbs(@as(f64, -14), try vdimLineX(a, d1, "d2"), 1e-9);
    try std.testing.expectEqual(try vdimLineX(a, d1, "d2"), try vdimLineX(a, d2, "d2"));
}

test "SPEC 20 outside text: a dim whose text does not fit keeps full size, is not dropped, and gets a short leader" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const src = try replaceAnn(a, two_posts_free,
        \\ {"id":"d_small","type":"dim","from":[5,-3],"to":[7,-3],"dir":"h","offset":-4}
    );
    const dr = try build(a, src, "A");
    const st = try style_mod.load(a, null);
    const ts = try textsOf(a, dr, "d_small");
    try std.testing.expectEqual(@as(usize, 1), ts.len);
    try std.testing.expectApproxEqAbs(st.text_height_in * dr.scale, ts[0].h, 1e-9);
    // outside the span between the extension lines (x 5..7)
    try std.testing.expect(ts[0].x < 5 or ts[0].x > 7);
    // ext lines (2) + dimension line (1) + leader (1)
    var n_dim: usize = 0;
    for (dr.items) |it| if (it == .path and std.mem.eql(u8, it.path.src, "d_small") and std.mem.eql(u8, it.path.pen, "dim")) {
        n_dim += 1;
    };
    try std.testing.expectEqual(@as(usize, 4), n_dim);
}

test "SPEC 20 repair: a dimension whose text a leader runs into is pushed out (no W_LEADER_HIT)" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    // n1's leader runs from (40,0) to post a, passing the text of the dimension above y = 10
    const src = try std.mem.replaceOwned(u8, a, two_posts,
        \\{"id":"n2","type":"note","text":"TWO","target":"b","place":[40,40]}
    ,
        \\{"id":"d1","type":"dim","from":[5,10],"to":[15,10],"dir":"h","offset":5}
    );
    const dr = try build(a, src, "A");
    try std.testing.expect(findDiag(dr, "W_LEADER_HIT") == null);
    var line_y: f64 = 0;
    for (dr.items) |it| {
        if (it != .path or !std.mem.eql(u8, it.path.src, "d1") or !std.mem.eql(u8, it.path.pen, "dim") or it.path.pts.len != 2) continue;
        const p = it.path.pts;
        if (@abs(p[0].y - p[1].y) < 1e-9 and @abs(p[0].x - p[1].x) > 1e-6) line_y = p[0].y;
    }
    try std.testing.expect(line_y > 15.0 + 1e-6);
    // pushed by whole 0.25" steps
    const k = (line_y - 15.0) / (0.25 * dr.scale);
    try std.testing.expectApproxEqAbs(@round(k), k, 1e-6);
}

test "note column hint puts the note in that column; a bad value warns" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const src = try replaceAnn(a, two_posts_free,
        \\ {"id":"n1","type":"note","text":"ONE","target":"a","column":"left"},
        \\ {"id":"n2","type":"note","text":"TWO","target":"b"}
    );
    const dr = try build(a, src, "A");
    const t1 = try textsOf(a, dr, "n1");
    const t2 = try textsOf(a, dr, "n2");
    try std.testing.expect(t1[0].x + 5 < dr.crop.x0); // n1 left of the crop although notes_side is right
    try std.testing.expect(t2[0].x > dr.crop.x1);
    try std.testing.expect(findDiag(dr, "W_LEADER_HIT") == null);
    const bad = try replaceAnn(a, two_posts_free,
        \\ {"id":"n1","type":"note","text":"ONE","target":"a","column":"middle"},
        \\ {"id":"n2","type":"note","text":"TWO","target":"b"}
    );
    const dr2 = try build(a, bad, "A");
    const d = findDiag(dr2, "W_PARAM") orelse return error.TestUnexpectedResult;
    try std.testing.expect(std.mem.indexOf(u8, d.message, "column") != null);
}

test "a designer-placed note left of its arrow right-aligns its text block to place.x + width" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const src = try replaceAnn(a, two_posts_free,
        \\ {"id":"n1","type":"note","text":"SHORT LINE AND A LONGER SECOND LINE OF TEXT HERE","target":"a","place":[-40,20]},
        \\ {"id":"n2","type":"note","text":"TWO","target":"b"}
    );
    const dr = try build(a, src, "A");
    const ts = try textsOf(a, dr, "n1");
    try std.testing.expect(ts.len >= 2);
    const x0 = ts[0].x;
    for (ts) |t| {
        try std.testing.expect(t.align_ == .right);
        try std.testing.expectApproxEqAbs(x0, t.x, 1e-9);
    }
    // the block's top-left is `place`: the right edge is place.x + the widest line
    const font = try @import("font.zig").Font.parse(a, @import("font.zig").embedded);
    var w: f64 = 0;
    for (ts) |t| w = @max(w, font.width(t.s, t.h));
    try std.testing.expectApproxEqAbs(@as(f64, -40) + w, x0, 1e-6);
}

test "dense small detail: repair never makes it worse and is deterministic" {
    const gpa = std.testing.allocator;
    const api = @import("api.zig");
    // the truss reference squeezed into one column with extra dims and labels on the leaders' way
    var arena = std.heap.ArenaAllocator.init(gpa);
    defer arena.deinit();
    const a = arena.allocator();
    var src = try std.mem.replaceOwned(u8, a, testdocs.truss, "\"notes_side\": \"both\"", "\"notes_side\": \"right\"");
    src = try std.mem.replaceOwned(u8, a, src, "\"annotations\": [",
        \\"annotations": [
        \\ {"id":"x1","type":"dim","from":"cmu@bottom_left","to":"cmu@bottom_center","dir":"h","offset":-3},
        \\ {"id":"x2","type":"dim","from":"cmu@top_left","to":"cmu@middle_left","dir":"v","offset":-4},
        \\ {"id":"x3","type":"label","text":"INTERIOR","at":"cmu@middle_right","offset":[5,5]},
    );
    const dr1 = try build(a, src, "A");
    const dr2 = try build(a, src, "A");
    try std.testing.expectEqual(dr1.items.len, dr2.items.len);
    for (dr1.items, dr2.items) |x, y| {
        if (x == .text) {
            try std.testing.expectEqual(x.text.x, y.text.x);
            try std.testing.expectEqual(x.text.y, y.text.y);
        }
    }
    // dimension and label texts never overprint each other
    const font = try @import("font.zig").Font.parse(a, @import("font.zig").embedded);
    var polys: std.ArrayList([4]V2) = .empty;
    for (dr1.items) |it| if (it == .text and (std.mem.eql(u8, it.text.src, "x1") or std.mem.eql(u8, it.text.src, "x2") or std.mem.eql(u8, it.text.src, "x3") or std.mem.eql(u8, it.text.src, "d_wall") or std.mem.eql(u8, it.text.src, "d_ovh") or std.mem.eql(u8, it.text.src, "l_ext") or std.mem.eql(u8, it.text.src, "l_int"))) {
        try polys.append(a, @import("annot.zig").textPoly(&font, it.text, 0));
    };
    try std.testing.expect(polys.items.len >= 7);
    for (polys.items, 0..) |p, i| for (polys.items[i + 1 ..]) |q| {
        try std.testing.expect(!@import("annot.zig").polysOverlap(&p, &q));
    };
    _ = api;
}
