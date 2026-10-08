//! Hostile-input regression table (REVIEW appendix B). Every row substitutes one hostile value into a small document or
//! style and runs `check`, `drawing`, `mesh` and an SVG `export`. The engine must neither panic, hang nor loop, must not emit
//! `nan`/`inf` into a drawing, and must answer with the stated diagnostic code (checked on the concatenated outputs).
//! The same values seed `tools/zig-engine/fuzz.py`.

const std = @import("std");
const api = @import("api.zig");

const base_doc =
    \\{"kerf":"0.1","id":"hostile","title":"HOSTILE","meta":{"jurisdiction":{"code":"IRC","edition":@@EDITION@@}},
    \\ "run":@@RUN@@,
    \\ "components":[
    \\  {"id":"poly","type":"concrete","shape":"polygon","points":[[0,0],[10,0],[10,10,@@BULGE@@],[0,10]],"at":{"anchor":"bottom_left","to":[0,0]}},
    \\  {"id":"stud","type":"lumber","size":"2x4","orient":"upright","at":{"anchor":"bottom_left","to":[20,0]},
    \\   "array":{"axis":"z","count":3,"spacing":@@SPACING@@}},
    \\  {"id":"rafter","type":"panel","material":"plywood","thickness":0.5,"length":@@LEN@@,"run":"x","slope":@@SLOPE@@,"at":{"anchor":"bottom_left","to":[40,0]}},
    \\  {"id":"strap","type":"connector","model":"H2.5A","lay":"face","gauge":@@GAUGE@@,
    \\   "points":[{"ref":"stud@top_left","offset":@@OFFSET@@},{"ref":"stud@bottom_left","offset":[1.5,1]}]}
    \\ ],
    \\ "views":[{"id":"A","kind":"section","scale":@@SCALE@@,"cut_z":0,"crop":{"x":@@CROPX@@,"y":[-4,16]},"annotations":[
    \\   {"id":"n1","type":"note","text":"CONCRETE POLYGON","target":"poly"}]}]}
;

const Case = struct {
    name: []const u8,
    /// marker -> replacement text (all other markers take their benign value)
    key: []const u8,
    val: []const u8,
    style: []const u8 = "{}",
    /// substring that must appear in the outputs (an E_ code), or "" when only "survives" is required
    expect: []const u8,
};

const benign = [_][2][]const u8{
    .{ "@@EDITION@@", "2021" },
    .{ "@@RUN@@", "[-12,12]" },
    .{ "@@BULGE@@", "0.5" },
    .{ "@@LEN@@", "24" },
    .{ "@@SPACING@@", "2" },
    .{ "@@SLOPE@@", "\"4:12\"" },
    .{ "@@GAUGE@@", "18" },
    .{ "@@OFFSET@@", "[1.5,2]" },
    .{ "@@SCALE@@", "\"1\\\"=1'-0\\\"\"" },
    .{ "@@CROPX@@", "[-4,60]" },
};

const cases = [_]Case{
    // 1 meta.jurisdiction.edition (drawview.codeBasis)
    .{ .name = "edition 1e30", .key = "@@EDITION@@", .val = "1e30", .expect = "E_JSON" },
    .{ .name = "edition 1e999", .key = "@@EDITION@@", .val = "1e999", .expect = "E_JSON" },
    .{ .name = "edition as string 1e999", .key = "@@EDITION@@", .val = "\"1e999\"", .expect = "" },
    // 2 connector gauge
    .{ .name = "gauge 1e20", .key = "@@GAUGE@@", .val = "1e20", .expect = "E_JSON" },
    .{ .name = "gauge 1e15", .key = "@@GAUGE@@", .val = "1e15", .expect = "gauge" },
    .{ .name = "gauge -5", .key = "@@GAUGE@@", .val = "-5", .expect = "gauge" },
    .{ .name = "gauge nan string", .key = "@@GAUGE@@", .val = "\"nan\"", .expect = "E_PARAM" },
    // 3 style: wrap_chars / heights (LAY-2)
    .{ .name = "wrap_chars -5", .key = "@@BULGE@@", .val = "0.5", .style = "{\"notes\":{\"wrap_chars\":-5}}", .expect = "E_STYLE" },
    .{ .name = "wrap_chars 0 (used to hang)", .key = "@@BULGE@@", .val = "0.5", .style = "{\"notes\":{\"wrap_chars\":0}}", .expect = "E_STYLE" },
    .{ .name = "wrap_chars 1e30", .key = "@@BULGE@@", .val = "0.5", .style = "{\"notes\":{\"wrap_chars\":1e30}}", .expect = "E_JSON" },
    .{ .name = "wrap_chars wide", .key = "@@BULGE@@", .val = "0.5", .style = "{\"notes\":{\"wrap_chars\":\"wide\"}}", .expect = "E_STYLE" },
    .{ .name = "text height 0 (used to hang)", .key = "@@BULGE@@", .val = "0.5", .style = "{\"text\":{\"height_in\":0}}", .expect = "E_STYLE" },
    .{ .name = "hatch dy 0", .key = "@@BULGE@@", .val = "0.5", .style = "{\"patterns\":{\"ANSI31\":[[45,0,0,0,0]]}}", .expect = "E_STYLE" },
    .{ .name = "pen dash 0", .key = "@@BULGE@@", .val = "0.5", .style = "{\"pens\":{\"hidden\":{\"dash_mm\":[0,0]}}}", .expect = "E_STYLE" },
    .{ .name = "sheet leaves no area", .key = "@@BULGE@@", .val = "0.5", .style = "{\"sheet\":{\"margin_in\":5}}", .expect = "E_STYLE" },
    // 4 bulge (geom.arcSteps)
    .{ .name = "bulge 1e-14", .key = "@@BULGE@@", .val = "1e-14", .expect = "bulge" },
    .{ .name = "bulge 1e14", .key = "@@BULGE@@", .val = "1e14", .expect = "bulge" },
    .{ .name = "bulge 1e300", .key = "@@BULGE@@", .val = "1e300", .expect = "E_JSON" },
    .{ .name = "bulge nan string", .key = "@@BULGE@@", .val = "\"nan\"", .expect = "bulge" },
    .{ .name = "bulge 1e-6 (smallest accepted)", .key = "@@BULGE@@", .val = "1e-6", .expect = "" },
    // 5 slope
    .{ .name = "slope nan", .key = "@@SLOPE@@", .val = "\"nan\"", .expect = "slope" },
    .{ .name = "slope inf", .key = "@@SLOPE@@", .val = "\"inf\"", .expect = "slope" },
    .{ .name = "slope 1e999 string", .key = "@@SLOPE@@", .val = "\"1e999\"", .expect = "slope" },
    .{ .name = "slope 1:0", .key = "@@SLOPE@@", .val = "\"1:0\"", .expect = "slope" },
    // 6 lengths
    .{ .name = "19-digit length", .key = "@@LEN@@", .val = "\"9999999999999999999\"", .expect = "out of range" },
    .{ .name = "length 1e300", .key = "@@LEN@@", .val = "1e300", .expect = "E_JSON" },
    .{ .name = "length 1e7 inches", .key = "@@LEN@@", .val = "10000000", .expect = "out of range" },
    // 7 scale
    .{ .name = "scale 1e300:1", .key = "@@SCALE@@", .val = "\"1e300:1\"", .expect = "scale" },
    .{ .name = "scale 1:1e-300", .key = "@@SCALE@@", .val = "\"1:1e-300\"", .expect = "scale" },
    .{ .name = "scale nan:1", .key = "@@SCALE@@", .val = "\"nan:1\"", .expect = "scale" },
    .{ .name = "scale 1:inf", .key = "@@SCALE@@", .val = "\"1:inf\"", .expect = "scale" },
    .{ .name = "scale 1:1e300", .key = "@@SCALE@@", .val = "\"1:1e300\"", .expect = "scale" },
    .{ .name = "scale 1'=1\" (12x enlargement)", .key = "@@SCALE@@", .val = "\"1'=1\\\"\"", .expect = "scale" },
    // 8 connector offsets (section.drawFaceTie)
    .{ .name = "offset 1e300", .key = "@@OFFSET@@", .val = "[1e300,0]", .expect = "E_JSON" },
    .{ .name = "offset 1e9 string", .key = "@@OFFSET@@", .val = "[\"1000000000\",0]", .expect = "offset" },
    .{ .name = "offset typo", .key = "@@OFFSET@@", .val = "[\"1/2x\",3]", .expect = "offset" },
    // 9 run
    .{ .name = "run +-1e300", .key = "@@RUN@@", .val = "[-1e300,1e300]", .expect = "E_JSON" },
    .{ .name = "run +-1e9 strings", .key = "@@RUN@@", .val = "[\"-1000000000\",\"1000000000\"]", .expect = "run" },
    // array spacing, crop
    .{ .name = "array spacing 1e300", .key = "@@SPACING@@", .val = "1e300", .expect = "E_JSON" },
    .{ .name = "array spacing 1e9 string", .key = "@@SPACING@@", .val = "\"1000000000\"", .expect = "spacing" },
    .{ .name = "crop +-1e300", .key = "@@CROPX@@", .val = "[-1e300,1e300]", .expect = "E_JSON" },
    .{ .name = "crop +-1e9 strings", .key = "@@CROPX@@", .val = "[\"-1000000000\",\"1000000000\"]", .expect = "crop" },
};

fn build(a: std.mem.Allocator, c: Case) ![]u8 {
    var doc = try a.dupe(u8, base_doc);
    for (benign) |b| {
        const v = if (std.mem.eql(u8, b[0], c.key)) c.val else b[1];
        doc = try std.mem.replaceOwned(u8, a, doc, b[0], v);
    }
    // the document goes in as a JSON object; the style is spliced in as raw JSON
    return a.print("{{\"doc\":{s},\"view\":\"A\",\"format\":\"svg\",\"style\":{s}}}", .{ doc, c.style });
}

test "hostile inputs: no panic, no hang, no nan in drawings, precise diagnostics" {
    for (cases) |c| {
        var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
        defer arena.deinit();
        const a = arena.allocator();
        const input = try build(a, c);
        var all: std.ArrayList(u8) = .empty;
        for ([_][]const u8{ "check", "drawing", "mesh", "export" }) |fn_name| {
            const r = try api.call(std.testing.allocator, fn_name, input);
            defer std.testing.allocator.free(r.bytes);
            try all.appendSlice(a, r.bytes);
            if (r.ok and std.mem.eql(u8, fn_name, "export")) {
                // the SVG is paths only: "nan"/"inf" there can only come from a bad number (text is stroked, never printed)
                if (std.mem.indexOf(u8, r.bytes, "nan") != null or std.mem.indexOf(u8, r.bytes, "inf") != null) {
                    std.debug.print("case '{s}': {s} output contains nan/inf\n", .{ c.name, fn_name });
                    return error.TestUnexpectedResult;
                }
            }
        }
        if (c.expect.len > 0 and std.mem.indexOf(u8, all.items, c.expect) == null) {
            std.debug.print("case '{s}': expected \"{s}\" in the output; got:\n{s}\n", .{ c.name, c.expect, all.items[0..@min(all.items.len, 1500)] });
            return error.TestUnexpectedResult;
        }
    }
}

test "benign base document is clean" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const input = try build(a, .{ .name = "base", .key = "@@EDITION@@", .val = "2021", .expect = "" });
    const r = try api.call(std.testing.allocator, "check", input);
    defer std.testing.allocator.free(r.bytes);
    try std.testing.expect(std.mem.indexOf(u8, r.bytes, "\"level\":\"error\"") == null);
    const d = try api.call(std.testing.allocator, "export", input);
    defer std.testing.allocator.free(d.bytes);
    try std.testing.expect(d.ok);
}

test "out of memory is reported, never swallowed (SAF-6)" {
    // For every allocation index of a call: either error.OutOfMemory or exactly the result of the unconstrained call.
    // (Before, `Diags.add` and ~60 more sites ate the error and returned a result with a diagnostic missing.)
    const testdocs = @import("testdocs.zig");
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const doc_input = try a.print("{{\"doc\":{s},\"view\":\"A\",\"format\":\"svg\"}}", .{testdocs.beam});
    const apply_input = try a.print("{{\"doc\":{s},\"ops\":[{{\"op\":\"update\",\"path\":\"components/strap\",\"value\":{{\"z\":0}}}}]}}", .{testdocs.beam});
    for ([_]struct { f: []const u8, input: []const u8 }{
        .{ .f = "check", .input = doc_input },
        .{ .f = "drawing", .input = doc_input },
        .{ .f = "apply", .input = apply_input },
    }) |c| {
        const base = try api.call(std.testing.allocator, c.f, c.input);
        defer std.testing.allocator.free(base.bytes);
        var counter = std.testing.FailingAllocator.init(std.testing.allocator, .{});
        const r0 = try api.call(counter.allocator(), c.f, c.input);
        std.testing.allocator.free(r0.bytes);
        const total = counter.alloc_index;
        try std.testing.expect(total > 0);
        var idx: usize = 0;
        while (idx <= total) : (idx += 1) {
            var fa = std.testing.FailingAllocator.init(std.testing.allocator, .{ .fail_index = idx });
            const r = api.call(fa.allocator(), c.f, c.input) catch |e| {
                try std.testing.expectEqual(error.OutOfMemory, e);
                continue;
            };
            defer std.testing.allocator.free(r.bytes);
            try std.testing.expectEqual(base.ok, r.ok);
            try std.testing.expectEqualStrings(base.bytes, r.bytes);
        }
    }
}

// ---- SAF-4: silent defaults are errors with a precise message ---------------------------------------------------------

const conc = "{\"id\":\"c\",\"type\":\"concrete\",\"shape\":\"rect\",\"width\":12,\"height\":12,\"at\":{\"anchor\":\"bottom_left\",\"to\":[0,0]}}";

const DiagCase = struct {
    name: []const u8,
    /// components array body after the concrete `c`
    comps: []const u8,
    /// annotations array body of view A (may be empty)
    anns: []const u8 = "",
    /// every one of these must occur in the check output
    expect: []const []const u8,
};

const diag_cases = [_]DiagCase{
    .{ .name = "at.to offset typo", .comps = "{\"id\":\"a\",\"type\":\"lumber\",\"size\":\"2x4\",\"at\":{\"to\":{\"ref\":\"c@top_left\",\"offset\":[\"1/2x\",3]}}}", .expect = &.{ "E_PARAM", "components/a/at/to/offset", "offset[0] must be a length", "\\\"1/2x\\\"", "write offset as [x, y]" } },
    .{ .name = "at.to offset not an array", .comps = "{\"id\":\"a\",\"type\":\"lumber\",\"size\":\"2x4\",\"at\":{\"to\":{\"ref\":\"c@top_left\",\"offset\":5}}}", .expect = &.{ "E_PARAM", "offset must be an array [x, y] (got 5)" } },
    .{ .name = "at.to offset with three values", .comps = "{\"id\":\"a\",\"type\":\"lumber\",\"size\":\"2x4\",\"at\":{\"to\":{\"ref\":\"c@top_left\",\"offset\":[1,2,3]}}}", .expect = &.{ "E_PARAM", "exactly 2 values [x, y] (got 3)" } },
    .{ .name = "at.to offset beyond the range", .comps = "{\"id\":\"a\",\"type\":\"lumber\",\"size\":\"2x4\",\"at\":{\"to\":{\"ref\":\"c@top_left\",\"offset\":[\"99999999\",0]}}}", .expect = &.{ "E_PARAM", "out of range" } },
    .{ .name = "point offset typo in a connector", .comps = "{\"id\":\"s\",\"type\":\"connector\",\"model\":\"H2.5A\",\"lay\":\"face\",\"points\":[{\"ref\":\"c@top_left\",\"offset\":[\"x\",0]},{\"ref\":\"c@bottom_left\",\"offset\":[1,1]}]}", .expect = &.{ "E_PARAM", "components/s/points/0/offset", "offset[0] must be a length" } },
    .{ .name = "polygon bulge as text", .comps = "{\"id\":\"p\",\"type\":\"concrete\",\"shape\":\"polygon\",\"points\":[[0,0],[10,0,\"big\"],[0,10]],\"at\":{\"to\":[30,0]}}", .expect = &.{ "E_PARAM", "must be a number", "bulge" } },
    .{ .name = "place.cover typo", .comps = "{\"id\":\"r\",\"type\":\"rebar\",\"size\":\"#4\",\"mode\":\"along_z\",\"place\":{\"in\":\"c\",\"face\":\"bottom\",\"cover\":\"1 1/2x\",\"count\":2}}", .expect = &.{ "E_PARAM", "components/r/place/cover", "place.cover must be a length", "\\\"1 1/2x\\\"" } },
    .{ .name = "place.side_cover typo", .comps = "{\"id\":\"r\",\"type\":\"rebar\",\"size\":\"#4\",\"mode\":\"along_z\",\"place\":{\"in\":\"c\",\"face\":\"bottom\",\"side_cover\":true,\"count\":2}}", .expect = &.{ "E_PARAM", "place.side_cover must be a length" } },
    .{ .name = "place.count as text", .comps = "{\"id\":\"r\",\"type\":\"rebar\",\"size\":\"#4\",\"mode\":\"along_z\",\"place\":{\"in\":\"c\",\"face\":\"bottom\",\"count\":\"3\"}}", .expect = &.{ "E_PARAM", "place.count must be an integer from 1 to 200 (got \\\"3\\\")" } },
    .{ .name = "array.count as text", .comps = "{\"id\":\"a\",\"type\":\"lumber\",\"size\":\"2x4\",\"at\":{\"to\":[20,0]},\"array\":{\"axis\":\"z\",\"count\":\"3\",\"spacing\":2}}", .expect = &.{ "E_PARAM", "array.count must be an integer from 1 to 500 (got \\\"3\\\")" } },
    .{ .name = "array.axis not a string", .comps = "{\"id\":\"a\",\"type\":\"lumber\",\"size\":\"2x4\",\"at\":{\"to\":[20,0]},\"array\":{\"axis\":5,\"count\":3,\"spacing\":2}}", .expect = &.{ "E_PARAM", "array.axis must be one of" } },
    .{ .name = "cover bottom typo", .comps = "{\"id\":\"w\",\"type\":\"concrete\",\"shape\":\"rect\",\"width\":8,\"height\":8,\"cover\":{\"bottom\":\"deep\"},\"at\":{\"to\":[30,0]}}", .expect = &.{ "E_PARAM", "cover.bottom must be a length" } },
    .{ .name = "cover.parts value typo", .comps = "{\"id\":\"w\",\"type\":\"concrete\",\"shape\":\"slab_edge\",\"cover\":{\"parts\":{\"slab\":{\"bottom\":\"x\"}}},\"at\":{\"anchor\":\"top_exterior\",\"to\":[30,0]}}", .expect = &.{ "E_PARAM", "cover/parts/slab/bottom", "must be a length" } },
    .{ .name = "recess.from_edge typo", .comps = "{\"id\":\"w\",\"type\":\"concrete\",\"shape\":\"slab_edge\",\"recess\":{\"width\":8,\"depth\":1,\"from_edge\":\"far\"},\"at\":{\"anchor\":\"top_exterior\",\"to\":[30,0]}}", .expect = &.{ "E_PARAM", "recess.from_edge must be a length" } },
    .{ .name = "dim offset typo", .comps = "", .anns = "{\"id\":\"d1\",\"type\":\"dim\",\"from\":\"c@bottom_left\",\"to\":\"c@bottom_right\",\"dir\":\"h\",\"offset\":\"abc\"}", .expect = &.{ "E_PARAM", "views/A/annotations/d1/offset", "dim offset must be a length", "write it as a number of inches" } },
    .{ .name = "label offset typo", .comps = "", .anns = "{\"id\":\"l1\",\"type\":\"label\",\"text\":\"EXT\",\"at\":\"c@center\",\"offset\":[\"a\",1]}", .expect = &.{ "E_PARAM", "views/A/annotations/l1/offset", "label offset[0] must be a length" } },
    .{ .name = "note place typo", .comps = "", .anns = "{\"id\":\"n1\",\"type\":\"note\",\"text\":\"CONC. BLOCK\",\"target\":\"c\",\"place\":[1]}", .expect = &.{ "E_PARAM", "views/A/annotations/n1/place", "place must have exactly 2 values" } },
};

test "silent defaults are now E_PARAM errors naming the key and giving a fix (SAF-4)" {
    for (diag_cases) |c| {
        var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
        defer arena.deinit();
        const a = arena.allocator();
        const comps = if (c.comps.len > 0) try a.print("{s},{s}", .{ conc, c.comps }) else conc;
        const input = try a.print("{{\"doc\":{{\"kerf\":\"0.1\",\"id\":\"d\",\"run\":[-12,12],\"components\":[{s}],\"views\":[{{\"id\":\"A\",\"kind\":\"section\",\"scale\":\"1\\\"=1'-0\\\"\",\"crop\":{{\"x\":[-4,60],\"y\":[-4,16]}},\"annotations\":[{s}]}}]}}}}", .{ comps, c.anns });
        const r = try api.call(std.testing.allocator, "check", input);
        defer std.testing.allocator.free(r.bytes);
        for (c.expect) |want| {
            if (std.mem.indexOf(u8, r.bytes, want) == null) {
                std.debug.print("case '{s}': expected \"{s}\" in:\n{s}\n", .{ c.name, want, r.bytes[0..@min(r.bytes.len, 2500)] });
                return error.TestUnexpectedResult;
            }
        }
    }
}
