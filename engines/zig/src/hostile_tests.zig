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
    return std.fmt.allocPrint(a, "{{\"doc\":{s},\"view\":\"A\",\"format\":\"svg\",\"style\":{s}}}", .{ doc, c.style });
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
