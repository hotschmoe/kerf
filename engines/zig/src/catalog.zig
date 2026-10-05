//! Component catalog (SPEC 5): the single source of truth for param names (validation of unknown
//! keys), defaults, parts and anchors, rendered as JSON and markdown for the LLM system prompt.

const std = @import("std");
const json = @import("json.zig");
const Allocator = std.mem.Allocator;

pub const Param = struct {
    name: []const u8,
    def: []const u8,
    desc: []const u8,
};

pub const Entry = struct {
    name: []const u8,
    summary: []const u8,
    params: []const Param,
    parts: []const u8,
    anchors: []const u8,
    draws: []const u8,
    /// One complete component object (valid on its own); `kerf schema <type>` prints it.
    example: []const u8 = "",
};

pub const Hardware = struct { model: []const u8, width: f64, gauge: u32, length: f64, kind: []const u8 };

/// Schematic hardware table: `connector.model` fills width and gauge from here.
pub const hardware = [_]Hardware{
    .{ .model = "H2.5A", .width = 1.375, .gauge = 18, .length = 5.5, .kind = "hurricane tie" },
    .{ .model = "H1", .width = 1.375, .gauge = 18, .length = 5.5, .kind = "hurricane tie" },
    .{ .model = "H10A", .width = 1.5, .gauge = 18, .length = 8, .kind = "hurricane tie" },
    .{ .model = "MSTA9", .width = 1.25, .gauge = 12, .length = 9, .kind = "strap" },
    .{ .model = "MSTA12", .width = 1.25, .gauge = 12, .length = 12, .kind = "strap" },
    .{ .model = "MSTA15", .width = 1.25, .gauge = 12, .length = 15, .kind = "strap" },
    .{ .model = "MSTA18", .width = 1.25, .gauge = 12, .length = 18, .kind = "strap" },
    .{ .model = "MSTA21", .width = 1.25, .gauge = 12, .length = 21, .kind = "strap" },
    .{ .model = "MSTA24", .width = 1.25, .gauge = 12, .length = 24, .kind = "strap" },
    .{ .model = "MSTA30", .width = 1.25, .gauge = 12, .length = 30, .kind = "strap" },
    .{ .model = "MSTA36", .width = 1.25, .gauge = 12, .length = 36, .kind = "strap" },
    .{ .model = "MSTA49", .width = 1.25, .gauge = 12, .length = 49, .kind = "strap" },
    .{ .model = "LSTA9", .width = 1.25, .gauge = 20, .length = 9, .kind = "strap" },
    .{ .model = "LSTA12", .width = 1.25, .gauge = 20, .length = 12, .kind = "strap" },
    .{ .model = "LSTA15", .width = 1.25, .gauge = 20, .length = 15, .kind = "strap" },
    .{ .model = "LSTA18", .width = 1.25, .gauge = 20, .length = 18, .kind = "strap" },
    .{ .model = "LSTA21", .width = 1.25, .gauge = 20, .length = 21, .kind = "strap" },
    .{ .model = "LSTA24", .width = 1.25, .gauge = 20, .length = 24, .kind = "strap" },
    .{ .model = "LSTA36", .width = 1.25, .gauge = 20, .length = 36, .kind = "strap" },
    .{ .model = "CS14", .width = 1.25, .gauge = 14, .length = 0, .kind = "coil strap" },
    .{ .model = "CS16", .width = 1.25, .gauge = 16, .length = 0, .kind = "coil strap" },
    .{ .model = "CS18", .width = 1.25, .gauge = 18, .length = 0, .kind = "coil strap" },
    .{ .model = "CS20", .width = 1.25, .gauge = 20, .length = 0, .kind = "coil strap" },
    .{ .model = "CMST14", .width = 3, .gauge = 14, .length = 0, .kind = "coil strap" },
    .{ .model = "CMST12", .width = 3, .gauge = 12, .length = 0, .kind = "coil strap" },
    .{ .model = "CMSTC16", .width = 3, .gauge = 16, .length = 0, .kind = "coil strap" },
    .{ .model = "MST37", .width = 3, .gauge = 12, .length = 37.5, .kind = "strap" },
    .{ .model = "MST48", .width = 3, .gauge = 12, .length = 48, .kind = "strap" },
    .{ .model = "META12", .width = 1.25, .gauge = 18, .length = 12, .kind = "embedded truss anchor" },
    .{ .model = "META16", .width = 1.25, .gauge = 18, .length = 16, .kind = "embedded truss anchor" },
    .{ .model = "META20", .width = 1.25, .gauge = 18, .length = 20, .kind = "embedded truss anchor" },
    .{ .model = "META24", .width = 1.25, .gauge = 18, .length = 24, .kind = "embedded truss anchor" },
    .{ .model = "HETA12", .width = 1.25, .gauge = 16, .length = 12, .kind = "embedded truss anchor" },
    .{ .model = "HETA16", .width = 1.25, .gauge = 16, .length = 16, .kind = "embedded truss anchor" },
    .{ .model = "HETA20", .width = 1.25, .gauge = 16, .length = 20, .kind = "embedded truss anchor" },
    .{ .model = "HETA24", .width = 1.25, .gauge = 16, .length = 24, .kind = "embedded truss anchor" },
    .{ .model = "HETA40", .width = 1.25, .gauge = 16, .length = 40, .kind = "embedded truss anchor" },
    .{ .model = "HHETA16", .width = 1.25, .gauge = 14, .length = 16, .kind = "embedded truss anchor" },
    .{ .model = "HHETA20", .width = 1.25, .gauge = 14, .length = 20, .kind = "embedded truss anchor" },
    .{ .model = "HETAL20", .width = 1.25, .gauge = 16, .length = 20, .kind = "embedded truss anchor" },
    .{ .model = "DETAL20", .width = 2.5, .gauge = 16, .length = 20, .kind = "embedded truss anchor" },
    .{ .model = "RSP4", .width = 2.125, .gauge = 20, .length = 4.5, .kind = "rafter/stud plate tie" },
    .{ .model = "A34", .width = 2.5, .gauge = 18, .length = 2.875, .kind = "framing angle, 1-7/16 legs" },
    .{ .model = "A35", .width = 4.5, .gauge = 18, .length = 2.875, .kind = "framing angle, 1-7/16 legs" },
    .{ .model = "LTP4", .width = 3, .gauge = 20, .length = 4.25, .kind = "lateral tie plate" },
    .{ .model = "LSTHD8", .width = 3, .gauge = 14, .length = 18.625, .kind = "strap-tie holdown (embedded)" },
    .{ .model = "STHD14", .width = 3, .gauge = 12, .length = 26.125, .kind = "strap-tie holdown (embedded)" },
    .{ .model = "LUS26", .width = 1.5625, .gauge = 18, .length = 4.75, .kind = "face-mount joist hanger, 2x6 (length = height)" },
    .{ .model = "MUS26", .width = 1.5625, .gauge = 18, .length = 5.1875, .kind = "face-mount joist hanger, 2x6 (length = height)" },
    .{ .model = "HUS26", .width = 1.625, .gauge = 16, .length = 5.375, .kind = "heavy face-mount hanger, 2x6 (length = height)" },
    .{ .model = "HHUS26-2", .width = 3.3125, .gauge = 14, .length = 5.375, .kind = "heavy face-mount hanger, double 2x6 (length = height)" },
    .{ .model = "ST2215", .width = 2.0625, .gauge = 20, .length = 16.3125, .kind = "strap tie" },
    .{ .model = "ST6224", .width = 2.0625, .gauge = 16, .length = 23.3125, .kind = "strap tie" },
    .{ .model = "FHA18", .width = 1.4375, .gauge = 12, .length = 17.75, .kind = "strap tie" },
    .{ .model = "LTS12", .width = 1.25, .gauge = 18, .length = 12, .kind = "twist strap" },
};

pub fn hardwareLine(a: Allocator, h: Hardware) Allocator.Error![]const u8 {
    var wb: [40]u8 = undefined;
    var lb: [40]u8 = undefined;
    if (h.length == 0) return std.fmt.allocPrint(a, "{s}: {s}, {s}\" wide, {d} ga, cut to length", .{ h.model, h.kind, json.fmtNumber(&wb, h.width), h.gauge });
    return std.fmt.allocPrint(a, "{s}: {s}, {s}\" wide, {d} ga, {s}\" long", .{ h.model, h.kind, json.fmtNumber(&wb, h.width), h.gauge, json.fmtNumber(&lb, h.length) });
}

pub const common: []const Param = &.{
    .{ .name = "id", .def = "required", .desc = "unique slug [a-z][a-z0-9_]*; annotations and refs name it" },
    .{ .name = "type", .def = "required", .desc = "one of the catalog types" },
    .{ .name = "label", .def = "null", .desc = "short human label for summaries" },
    .{ .name = "at", .def = "origin", .desc = "{anchor (default bottom_left), to: Ref | {ref, offset} | [x,y], offset: [dx,dy]}: translates the part so its anchor lands on `to`+offset. Point-list types (connector, membrane, fill, rebar path, concrete polygon, solid points) ignore `anchor`: literal [x,y] entries are relative to `to`+offset, Refs are absolute" },
    .{ .name = "rotate", .def = "0", .desc = "degrees CCW about the placement point" },
    .{ .name = "slope", .def = "null", .desc = "\"4:12\" rise:run (rotation about the placement point; adds to rotate)" },
    .{ .name = "mirror", .def = "false", .desc = "mirror the profile about its vertical centerline before placement" },
    .{ .name = "z", .def = "null", .desc = "[z0,z1] absolute, or a number = centered there with the member's natural z thickness. Default: spans doc `run` (members along Z) or centered in `run` with natural thickness" },
    .{ .name = "array", .def = "null", .desc = "{axis: x|y|z, count, spacing}: instance k offset by k*spacing; instances are id#0..id#n-1, refs to `id` mean instance 0, `id#k@anchor` addresses instance k" },
    .{ .name = "embedded", .def = "type default", .desc = "drawn over cut solids and never occluded (rebar, anchor bolts)" },
    .{ .name = "visible", .def = "true", .desc = "false hides the component from views and mesh" },
    .{ .name = "shown", .def = "solid", .desc = "dashed = \"where occurs\" graphics: all edges in the hidden (dashed) pen, no hatch or cut mark, never hides anything, exempt from W_FLOATING / W_NEAR_MISS / W_OVERLAP; notes targeting it get \" (WHERE OCCURS)\" appended. Section views only (iso omits it)" },
    .{ .name = "acknowledge", .def = "null", .desc = "[{code, reason}]: suppress that warning (e.g. W_UNTREATED_CONTACT) for this component; the reason prints as an I_ACK line in the summary and is logged. Errors cannot be acknowledged (`kerf schema acknowledge`)" },
};

pub const box_anchors = "top_left top_center top_right middle_left center middle_right bottom_left bottom_center bottom_right (of the profile box; rotate with the member)";

pub const entries: []const Entry = &.{
    .{
        .name = "lumber",
        .summary = "Sawn or engineered wood member (stud, plate, joist, beam, blocking, post).",
        .params = &.{
            .{ .name = "size", .def = "required", .desc = "sawn nominal \"2x4\"..\"2x12\", \"4x4\"..\"4x12\", \"6x6\"..\"6x12\" (also 1x4..1x12); or actual \"1.75x11.875\" (thickness x depth) for lvl/psl/lsl/glulam" },
            .{ .name = "product", .def = "sawn", .desc = "sawn | lvl | psl | lsl | glulam" },
            .{ .name = "run", .def = "z", .desc = "axis the length runs along: z (seen end-on in section), x or y" },
            .{ .name = "orient", .def = "upright", .desc = "run z only: upright (depth vertical) or flat (depth horizontal)" },
            .{ .name = "face", .def = "wide", .desc = "run x/y only: face seen by the viewer: wide (depth in-plane) or narrow (thickness in-plane)" },
            .{ .name = "length", .def = "required for run x/y", .desc = "member length (inches or ft-in string); alternative: `until`" },
            .{ .name = "until", .def = "null", .desc = "run x/y alternative to length: a Ref (or {ref, offset}); the member grows from its placement anchor along its run axis until its far end reaches the Ref's coordinate on that axis (anchor *_left grows right, *_right left, top_* down, bottom_* up; center anchors are an error; length and until together are E_PARAM). Example jack stud: \"at\": {\"anchor\": \"top_left\", \"to\": \"beam@bottom_left\"}, \"until\": \"bottom_plate@top_left\"" },
            .{ .name = "plies", .def = "1", .desc = "built-up members; plies stack along X for run z, along Z otherwise; draws ply lines" },
            .{ .name = "treated", .def = "false", .desc = "preservative treated (material wood_treated; W_UNTREATED_CONTACT checks)" },
            .{ .name = "blocking", .def = "false", .desc = "discontinuous member: section mark is one diagonal instead of an X" },
            .{ .name = "grade", .def = "null", .desc = "free text e.g. \"#2 DF-L\" for notes" },
            .{ .name = "barrier", .def = "null", .desc = "sill_seal | membrane: draws a 1/8\" sealer strip under the member (part `barrier`, material sill_seal) and the member sits on top of it, so bottom_* anchors are the strip underside and the member top is 1/8\" higher than without. Clears W_UNTREATED_CONTACT for untreated wood on concrete/CMU (put it on the wood member that bears on the masonry; add a note, e.g. `SILL SEALER`)" },
        },
        .parts = "barrier (when set)",
        .anchors = "the 9 box anchors",
        .draws = "Section: run z cut => outline + wood X mark per ply (blocking: one diagonal); run x/y cut lengthwise => outline only; beyond => outline. Actual sizes: 2x 1.5 thick; 4x 3.5; 6x 5.5; depths x4 3.5, x6 5.5, x8 7.25, x10 9.25, x12 11.25 (6x8 7.5, 6x10 9.5, 6x12 11.5). Natural z thickness for run x/y.",
        .example = "{\"id\":\"stud\",\"type\":\"lumber\",\"size\":\"2x4\",\"run\":\"y\",\"face\":\"narrow\",\"length\":92.625,\"at\":{\"anchor\":\"bottom_left\",\"to\":[0,1.5]}}",
    },
    .{
        .name = "panel",
        .summary = "Sheathing, boards, gypsum, soffit, fascia/trim boards as a thin rectangle.",
        .params = &.{
            .{ .name = "material", .def = "osb", .desc = "osb | plywood | gypsum | fiber_cement | wood_board" },
            .{ .name = "thickness", .def = "required", .desc = "e.g. 0.4375 (7/16\"), 0.46875 (15/32), 0.5, 0.625, 0.75" },
            .{ .name = "length", .def = "required", .desc = "in-plane extent; alternative: `until` (same rule as lumber)" },
            .{ .name = "until", .def = "null", .desc = "alternative to length: a Ref; the panel grows from its placement anchor along its run axis until its far end reaches the Ref's coordinate" },
            .{ .name = "run", .def = "x", .desc = "in-plane direction of length before rotation: x (length x thickness) or y (thickness x length)" },
        },
        .parts = "none",
        .anchors = "the 9 box anchors",
        .draws = "Cut rectangle with the material's hatch / cut mark (wood_board: one diagonal). Spans the document run along Z. Use `slope` for roof sheathing (rotates about the placement anchor).",
        .example = "{\"id\":\"roof_sheathing\",\"type\":\"panel\",\"material\":\"osb\",\"thickness\":0.4375,\"length\":66,\"run\":\"x\",\"slope\":\"4:12\",\"at\":{\"anchor\":\"bottom_left\",\"to\":[0,0]}}",
    },
    .{
        .name = "cmu_wall",
        .summary = "Concrete masonry wall in section: face shells, grouted cells, mortar joints, bond beam.",
        .params = &.{
            .{ .name = "width", .def = "8", .desc = "nominal 6, 8, 10, 12 => actual 5.625, 7.625, 9.625, 11.625" },
            .{ .name = "courses", .def = "required", .desc = "number of 8\" courses (7.625 unit + 0.375 mortar joint)" },
            .{ .name = "bond_beam_courses", .def = "0", .desc = "top N courses are bond-beam units (always grouted)" },
            .{ .name = "grout", .def = "reinforced", .desc = "solid | reinforced (bond beams + the cut cell) | none" },
            .{ .name = "face_shell", .def = "1.25", .desc = "face shell thickness drawn in section" },
            .{ .name = "top_joint", .def = "false", .desc = "mortar joint above the top course" },
            .{ .name = "cover", .def = "{sides:1.5, top:1.5, bottom:0.5}", .desc = "required clear cover for rebar (W_COVER); supports cover.parts.<part> overrides" },
        },
        .parts = "course_1..course_n (1 = bottom), bond_beam, grout",
        .anchors = "9 box anchors + bond_beam_center, top_center, cell_center_top",
        .draws = "Box height = courses*8 - 0.375 (+0.375 with top_joint); the lowest course sits on the box bottom. Per course: two face shells (cut, cmu hatch), grouted cell (grout hatch) or an empty cell with the cross web as a beyond line, mortar joints as cut lines. 3D: 15.625\" units with 0.375\" head joints, running bond.",
        .example = "{\"id\":\"cmu\",\"type\":\"cmu_wall\",\"width\":8,\"courses\":3,\"bond_beam_courses\":1,\"at\":{\"anchor\":\"bottom_left\",\"to\":[0,0]}}",
    },
    .{
        .name = "concrete",
        .summary = "Cast-in-place concrete: rect/footing, free polygon, or a monolithic slab with turned-down edge.",
        .params = &.{
            .{ .name = "shape", .def = "required", .desc = "rect | footing | polygon | slab_edge" },
            .{ .name = "material", .def = "concrete", .desc = "any style material" },
            .{ .name = "cover", .def = "{bottom:3, sides:3, top:1.5}", .desc = "REQUIRED clear cover for bars in this host (W_COVER); cover.parts.<part> overrides per part zone, e.g. {\"parts\":{\"slab\":{\"bottom\":0.75}}}" },
            .{ .name = "width, height", .def = "rect/footing: required", .desc = "box size" },
            .{ .name = "points", .def = "polygon: required", .desc = "polyline [x,y(,bulge)] relative to the placement point, or absolute Refs" },
            .{ .name = "exterior", .def = "left", .desc = "slab_edge: which side is the exterior edge (right mirrors)" },
            .{ .name = "slab_thickness", .def = "4", .desc = "slab_edge" },
            .{ .name = "slab_length", .def = "48", .desc = "slab_edge: slab drawn from exterior face inward" },
            .{ .name = "footing_width", .def = "12", .desc = "slab_edge: bottom width of the turndown" },
            .{ .name = "footing_depth", .def = "18", .desc = "slab_edge: top of slab to bottom of footing" },
            .{ .name = "haunch", .def = "45", .desc = "slab_edge: inner face slope from horizontal in degrees; 90 = vertical" },
            .{ .name = "recess", .def = "null", .desc = "slab_edge: {width, depth, from_edge} depression at the top exterior edge (door sill); from_edge 0 = at the exterior face" },
            .{ .name = "recess_slope", .def = "0", .desc = "slab_edge: recess floor falls this many inches toward the exterior over its width" },
            .{ .name = "base", .def = "null", .desc = "slab_edge: {material: gravel|sand|compacted_fill, thickness} uniform base course under the slab soffit and along the haunch (soil side), stopping at the footing bottom; part `base` with the fill hatch. Replaces hand-drawn fill polygons under the slab" },
        },
        .parts = "footing (turndown zone), slab (slab zone), base (when base is set) for slab_edge; footing for shape footing",
        .anchors = "9 box anchors; slab_edge adds top_exterior (datum (0,0) at the exterior face), slab_top, footing_bottom_exterior, footing_bottom_interior, slab_bottom_interior, haunch_top, recess_bottom_exterior, recess_bottom_interior, recess_top_interior, base_bottom_interior and base_bottom_footing (with base)",
        .draws = "slab_edge local origin: exterior face at x=0, top of slab at y=0, footing bottom at y=-footing_depth. Cut region with the material hatch; parts are zones (rebar place.in, cover), not separate outlines.",
        .example = "{\"id\":\"slab\",\"type\":\"concrete\",\"shape\":\"slab_edge\",\"slab_thickness\":4,\"footing_width\":12,\"footing_depth\":18,\"at\":{\"anchor\":\"top_exterior\",\"to\":[0,0]}}",
    },
    .{
        .name = "rebar",
        .summary = "Reinforcing bar: a dot in section (along_z) or a line in the XY plane (path).",
        .params = &.{
            .{ .name = "size", .def = "#4", .desc = "#3 .375, #4 .5, #5 .625, #6 .75, #7 .875, #8 1.0 (diameter in)" },
            .{ .name = "mode", .def = "along_z", .desc = "along_z (continuous bar seen as a dot) or path (bar in the XY plane)" },
            .{ .name = "place", .def = "null", .desc = "cover-based placement (preferred): {in: \"comp[.part]\", face: bottom|top|left|right|center, cover: 3, count: 2, side_cover: cover, axis: x|y, station: in}. bottom/top/left/right: bars at clear `cover` from that face, spread evenly between the zone's adjacent faces at `side_cover` (count 1 centers). center: bars centered in the zone on both axes (e.g. a single #4 in the middle of a stem wall); count > 1 spreads along axis x (default) or y at side_cover. station: ONE bar at that offset from the zone's left face (bottom face with axis y; for bottom/top/left/right faces it sets the along-face position)" },
            .{ .name = "points", .def = "path: required", .desc = "polyline [x,y] or Refs; bends get radius bend_radius, drawn as fillets" },
            .{ .name = "bend_radius", .def = "3*d_b", .desc = "inside bend radius for path bars" },
            .{ .name = "spacing_note", .def = "null", .desc = "e.g. \"#4 @ 16\\\" O.C.\" for summaries and notes" },
        },
        .parts = "none",
        .anchors = "9 box anchors of the bar (center = bar center for along_z)",
        .draws = "Default embedded:true. Cut dots draw solid; path bars draw as two parallel lines (bar outline) in pen rebar, filled solid when cut. 3D: swept circle. Natural z thickness = bar diameter. W_COVER is checked against the host concrete/cmu.",
        .example = "{\"id\":\"top_bar\",\"type\":\"rebar\",\"size\":\"#4\",\"at\":{\"anchor\":\"center\",\"to\":[6,-2.5]}}",
    },
    .{
        .name = "anchor_bolt",
        .summary = "Anchor bolt in the XY plane at a given z (shank, hook, nut and washer).",
        .params = &.{
            .{ .name = "diameter", .def = "0.5", .desc = "0.5 or 0.625 typical" },
            .{ .name = "embed", .def = "7 (4 for wedge/screw)", .desc = "length below the placement point (top of concrete); effective embedment for wedge/screw" },
            .{ .name = "projection", .def = "2.5", .desc = "length above the placement point" },
            .{ .name = "hook", .def = "J", .desc = "J: 180 degree bend toward +x, inside radius 1.5*d, returning up hook_len from the lowest point; L: 90 degree bend toward +x, horizontal leg ends hook_len from the shaft centerline; headed: square head 2*d wide, 0.5*d thick; none; wedge: post-installed expansion anchor (straight shaft, expansion clip 1.15*d wide x 0.6*embed long at the embedded end, nut+washer); screw: Titen HD style concrete screw (thread ticks along the embedment, hex washer head at the top, no nut)" },
            .{ .name = "hook_len", .def = "J 2, L 3", .desc = "hook leg length in inches (see hook)" },
            .{ .name = "nut_washer", .def = "true", .desc = "draw nut (1.5*d wide, 0.875*d tall, top at projection - 0.25*d) and washer (2.25*d wide, 0.125 thick) under it" },
        },
        .parts = "shank, nut, washer (wedge adds clip; screw has threads, washer, head instead of nut)",
        .anchors = "9 box anchors + top_of_concrete (where the bolt meets the host top surface, local (0,0))",
        .draws = "Embedded steel, natural z thickness = diameter (set `z` to the bolt's z). Placement anchor named top_of_concrete.",
        .example = "{\"id\":\"anchor_bolt\",\"type\":\"anchor_bolt\",\"diameter\":0.5,\"embed\":7,\"projection\":2.75,\"hook\":\"J\",\"at\":{\"to\":[3,0]}}",
    },
    .{
        .name = "connector",
        .summary = "Schematic steel hardware: straps, ties, embedded anchors, drawn as a thickened polyline.",
        .params = &.{
            .{ .name = "model", .def = "null", .desc = "e.g. MSTA36, H2.5A, HETA20, CS16, CS14: fills width/gauge from the hardware table" },
            .{ .name = "points", .def = "required", .desc = "polyline [x,y] or Refs of the bearing face (lay edge) or centerline (lay face)" },
            .{ .name = "lay", .def = "edge", .desc = "edge: seen edge-on, gauge in-plane growing to `side`, width along Z; face: seen face-on, `width` in-plane centered on the polyline, gauge along Z" },
            .{ .name = "side", .def = "left", .desc = "lay edge: left of the polyline direction (left of a left-to-right line = up) or right" },
            .{ .name = "gauge", .def = "18", .desc = "12 .1046, 14 .0747, 16 .0598, 18 .0478, 20 .0359" },
            .{ .name = "width", .def = "1.25", .desc = "extent along Z (lay edge) or in-plane (lay face)" },
            .{ .name = "fasteners", .def = "null", .desc = "text for notes, e.g. \"(10) 10d EA. END\"" },
        },
        .parts = "none",
        .anchors = "9 box anchors of the resolved profile",
        .draws = "Pen steel; filled solid when cut, outline when beyond. Schematic: the note carries the model, the geometry shows location and path.",
        .example = "{\"id\":\"tie\",\"type\":\"connector\",\"model\":\"H2.5A\",\"lay\":\"face\",\"points\":[[0,0],[0,6]]}",
    },
    .{
        .name = "truss",
        .summary = "Prefab wood truss heel and tail in side view.",
        .params = &.{
            .{ .name = "exterior", .def = "left", .desc = "side of the heel/overhang (right mirrors)" },
            .{ .name = "pitch", .def = "4:12", .desc = "rise:run" },
            .{ .name = "top_chord", .def = "2x4", .desc = "sawn nominal size, depth in-plane" },
            .{ .name = "bottom_chord", .def = "2x4", .desc = "sawn nominal size, depth in-plane" },
            .{ .name = "heel", .def = "standard", .desc = "standard | raised" },
            .{ .name = "heel_height", .def = "null", .desc = "raised heel: vertical height at the bearing outer edge from top of bottom chord to top of top chord" },
            .{ .name = "bearing_width", .def = "3.5", .desc = "width of the support under the heel" },
            .{ .name = "overhang", .def = "12", .desc = "horizontal distance from outer face of bearing to the tail end" },
            .{ .name = "tail", .def = "plumb", .desc = "plumb | square cut" },
            .{ .name = "span_shown", .def = "48", .desc = "how far into the building to draw (crop/break at the end)" },
            .{ .name = "plate", .def = "true", .desc = "draw the heel truss plate outline (dashed hidden pen)" },
        },
        .parts = "top_chord, bottom_chord, heel_web (raised only), plate, tail",
        .anchors = "9 box anchors + bearing_outer (local origin: outer edge of bearing at bottom of bottom chord), bearing_inner, tail_bottom, tail_top, top_chord_at_bearing, top_chord_end, bottom_chord_top_inner",
        .draws = "Standard heel: bottom chord from the bearing outer edge inward; top chord lower edge passes through (bearing_outer.x, top of bottom chord) at the pitch and extends to the plumb tail at x=-overhang. Members are in-plane, z thickness 1.5: set `z` and `array` for spacing.",
        .example = "{\"id\":\"truss\",\"type\":\"truss\",\"pitch\":\"4:12\",\"top_chord\":\"2x4\",\"bottom_chord\":\"2x4\",\"bearing_width\":7.25,\"overhang\":18,\"at\":{\"anchor\":\"bearing_outer\",\"to\":[0,0]}}",
    },
    .{
        .name = "membrane",
        .summary = "Thin layers: underlayment, vapor retarder, WRB, roofing, flashing.",
        .params = &.{
            .{ .name = "material", .def = "membrane", .desc = "underlayment | vapor_retarder | wrb | shingles | flashing_membrane | membrane" },
            .{ .name = "points", .def = "required", .desc = "polyline [x,y] or Refs" },
            .{ .name = "thickness", .def = "per material", .desc = "draw thickness (vapor retarder 0.04, shingles 0.25 typical)" },
            .{ .name = "side", .def = "left", .desc = "which side of the polyline direction the thickness grows: left of dx,dy is (-dy,dx)" },
        },
        .parts = "none",
        .anchors = "9 box anchors of the resolved profile",
        .draws = "Drawn as a line per the material pen (vapor retarder: dashed heavy; shingles: heavy line with tick marks). Never occludes or hatches.",
        .example = "{\"id\":\"vapor\",\"type\":\"membrane\",\"material\":\"vapor_retarder\",\"points\":[[0,0],[48,0]]}",
    },
    .{
        .name = "fill",
        .summary = "Earth, gravel, sand, compacted fill as a hatched polygon.",
        .params = &.{
            .{ .name = "material", .def = "earth", .desc = "earth | gravel | sand | compacted_fill" },
            .{ .name = "points", .def = "required", .desc = "polygon [x,y] or Refs" },
            .{ .name = "outline", .def = "top", .desc = "top (stroke only edges with outward normal up: the grade line) | full | none" },
            .{ .name = "grade_label", .def = "null", .desc = "optional text for annotations" },
        },
        .parts = "none",
        .anchors = "9 box anchors of the polygon",
        .draws = "Hatched cut region; the hatch stops at the crop and fills never get break lines.",
        .example = "{\"id\":\"gravel\",\"type\":\"fill\",\"material\":\"gravel\",\"points\":[[0,0],[48,0],[48,-4],[0,-4]]}",
    },
    .{
        .name = "insulation",
        .summary = "Rigid (hatched) or batt (loop symbol) insulation.",
        .params = &.{
            .{ .name = "form", .def = "rigid", .desc = "rigid | batt" },
            .{ .name = "width, height", .def = "rect: required unless points", .desc = "box size" },
            .{ .name = "points", .def = "null", .desc = "polygon alternative to width/height" },
        },
        .parts = "none",
        .anchors = "9 box anchors",
        .draws = "Rigid: hatched region. Batt: sinusoidal loop line fitted to the rectangle.",
        .example = "{\"id\":\"foam\",\"type\":\"insulation\",\"form\":\"rigid\",\"width\":2,\"height\":24}",
    },
    .{
        .name = "flashing",
        .summary = "Sheet-metal flashing in section: Z, L, drip edge, weep screed or free polyline.",
        .params = &.{
            .{ .name = "profile", .def = "z", .desc = "z: back flange up the wall, horizontal leg out, drop at the nose; l: flange + horizontal leg; drip: flange on the deck, drop, outward kick; weep_screed: nailing flange up the wall, ledge, small drip drop; points: free centerline polyline" },
            .{ .name = "flange", .def = "2 (weep_screed 3.5)", .desc = "vertical back/nailing flange length (drip: horizontal flange on the deck)" },
            .{ .name = "leg", .def = "1 (l 2)", .desc = "horizontal leg length toward the exterior" },
            .{ .name = "drop", .def = "2 (drip 1.5, weep_screed 0.5)", .desc = "downturned leg at the nose" },
            .{ .name = "kick", .def = "0.5", .desc = "drip only: outward kick at the bottom of the drop" },
            .{ .name = "gauge", .def = "26", .desc = "20 .0359, 22 .0299, 24 .0239, 26 .0179, 28 .0149" },
            .{ .name = "exterior", .def = "left", .desc = "side the nose faces (right mirrors); presets only" },
            .{ .name = "points", .def = "profile points: required", .desc = "centerline polyline [x,y] relative to the placement point, or Refs" },
        },
        .parts = "none",
        .anchors = "9 box anchors + corner (first bend, local (0,0) for presets), start, end",
        .draws = "Presets: corner at (0,0), wall surface x=0, exterior -x. Spans the document run along Z. Thin metal: solid fill + steel outline (SPEC 16). Embedded: drawn over hatch, exempt from W_OVERLAP. Place with at.anchor \"corner\".",
        .example = "{\"id\":\"pan\",\"type\":\"flashing\",\"profile\":\"z\",\"at\":{\"anchor\":\"corner\",\"to\":[0,0]}}",
    },
    .{
        .name = "joint",
        .summary = "Concrete joints: expansion filler strip, control (saw-cut) notch, tooled edge radius, sealant bead on backer rod.",
        .params = &.{
            .{ .name = "kind", .def = "required", .desc = "expansion | control | tooled_edge | sealant" },
            .{ .name = "width", .def = "0.5 (control 0.25)", .desc = "expansion: filler thickness; control: notch width at the top; sealant: joint gap width" },
            .{ .name = "depth", .def = "expansion 4, control 1, sealant 0.25", .desc = "expansion: filler depth below the top (set to the slab thickness, or give `in`); control: notch depth (default 1/4 of the `in` zone height); sealant: bead depth" },
            .{ .name = "in", .def = "null", .desc = "optional host zone \"comp[.part]\" whose height sets the default depth (expansion: full height; control: 1/4)" },
            .{ .name = "cap", .def = "0", .desc = "expansion: depth of a sealant cap at the top of the filler (part `sealant`)" },
            .{ .name = "radius", .def = "0.25", .desc = "tooled_edge: radius of the rounded corner" },
            .{ .name = "corner", .def = "top_right", .desc = "tooled_edge: which corner of the concrete the point is: top_right (concrete lies left and below), top_left, bottom_right, bottom_left" },
            .{ .name = "backer_rod", .def = "true", .desc = "sealant: draw the backer rod circle (diameter 1.25*width) below the bead" },
        },
        .parts = "expansion: filler (+ sealant with cap); control: notch; tooled_edge: radius; sealant: bead, rod",
        .anchors = "9 box anchors + joint_top (local (0,0): top surface at the joint centerline); tooled_edge adds corner",
        .draws = "Local origin: top surface, joint centerline (tooled_edge: the sharp corner). Expansion: filler strip width x depth with the joint_filler hatch (place it in the gap between two concrete pieces). Control and tooled_edge are void shapes (embedded: they cut a hole in the host hatch; not in 3D/iso). Sealant: bead (filled) over a backer rod circle. Spans the document run along Z.",
        .example = "{\"id\":\"ej\",\"type\":\"joint\",\"kind\":\"expansion\",\"width\":0.5,\"depth\":4,\"at\":{\"anchor\":\"joint_top\",\"to\":[0,0]}}",
    },
    .{
        .name = "solid",
        .summary = "Escape hatch: any extruded profile with an explicit material (flagged I_SOLID_USED).",
        .params = &.{
            .{ .name = "profile", .def = "required", .desc = "{rect:[w,h]} | {circle:d} | {points:[...]}" },
            .{ .name = "material", .def = "required", .desc = "any style material (aluminum, steel, ...)" },
        },
        .parts = "none",
        .anchors = "9 box anchors",
        .draws = "Use only when no typed component fits.",
        .example = "{\"id\":\"angle\",\"type\":\"solid\",\"material\":\"steel\",\"profile\":{\"rect\":[3,0.25]}}",
    },
};

pub fn find(name: []const u8) ?*const Entry {
    for (entries) |*e| if (std.mem.eql(u8, e.name, name)) return e;
    return null;
}

pub fn typeNames(a: Allocator) Allocator.Error![]const []const u8 {
    const out = try a.alloc([]const u8, entries.len);
    for (entries, 0..) |e, i| out[i] = e.name;
    return out;
}

/// Is `key` an accepted top-level key of a component of type `ty`?
pub fn allowedKey(ty: *const Entry, key: []const u8) bool {
    for (common) |c| if (std.mem.eql(u8, c.name, key)) return true;
    for (ty.params) |p| {
        // "width, height" style entries list several names.
        var it = std.mem.splitSequence(u8, p.name, ", ");
        while (it.next()) |n| if (std.mem.eql(u8, n, key)) return true;
    }
    return false;
}

/// Every accepted top-level key of a component of type `ty` (type params, then common fields).
pub fn allowedKeyNames(a: Allocator, ty: *const Entry) Allocator.Error![]const []const u8 {
    var out: std.ArrayList([]const u8) = .empty;
    for (ty.params) |p| {
        var it = std.mem.splitSequence(u8, p.name, ", ");
        while (it.next()) |n| try out.append(a, n);
    }
    for (common) |c| try out.append(a, c.name);
    return out.items;
}

pub fn allowedKeysText(a: Allocator, ty: *const Entry) Allocator.Error![]const u8 {
    var out: std.ArrayList(u8) = .empty;
    var first = true;
    for (ty.params) |p| {
        var it = std.mem.splitSequence(u8, p.name, ", ");
        while (it.next()) |n| {
            if (!first) try out.appendSlice(a, ", ");
            first = false;
            try out.appendSlice(a, n);
        }
    }
    try out.appendSlice(a, ", + common: ");
    for (common, 0..) |c, i| {
        if (i > 0) try out.appendSlice(a, ", ");
        try out.appendSlice(a, c.name);
    }
    return out.items;
}

// ---- rendering -----------------------------------------------------------------------------------------

fn paramJson(a: Allocator, p: Param) Allocator.Error!json.Value {
    return json.obj(a, &.{
        .{ .key = "name", .value = .{ .string = p.name } },
        .{ .key = "default", .value = .{ .string = p.def } },
        .{ .key = "desc", .value = .{ .string = p.desc } },
    });
}

pub fn entryJson(a: Allocator, e: *const Entry) Allocator.Error!json.Value {
    const ps = try a.alloc(json.Value, e.params.len);
    for (e.params, 0..) |p, i| ps[i] = try paramJson(a, p);
    if (std.mem.eql(u8, e.name, "connector")) {
        const ms = try a.alloc(json.Value, hardware.len);
        for (hardware, 0..) |h, i| ms[i] = .{ .string = try hardwareLine(a, h) };
        return json.obj(a, &.{
            .{ .key = "type", .value = .{ .string = e.name } },
            .{ .key = "summary", .value = .{ .string = e.summary } },
            .{ .key = "params", .value = .{ .array = ps } },
            .{ .key = "models", .value = .{ .array = ms } },
            .{ .key = "parts", .value = .{ .string = e.parts } },
            .{ .key = "anchors", .value = .{ .string = e.anchors } },
            .{ .key = "draws", .value = .{ .string = e.draws } },
        });
    }
    return json.obj(a, &.{
        .{ .key = "type", .value = .{ .string = e.name } },
        .{ .key = "summary", .value = .{ .string = e.summary } },
        .{ .key = "params", .value = .{ .array = ps } },
        .{ .key = "parts", .value = .{ .string = e.parts } },
        .{ .key = "anchors", .value = .{ .string = e.anchors } },
        .{ .key = "draws", .value = .{ .string = e.draws } },
    });
}

pub fn catalogJson(a: Allocator) Allocator.Error!json.Value {
    const cs = try a.alloc(json.Value, common.len);
    for (common, 0..) |p, i| cs[i] = try paramJson(a, p);
    const ts = try a.alloc(json.Value, entries.len);
    for (entries, 0..) |*e, i| ts[i] = try entryJson(a, e);
    return json.obj(a, &.{
        .{ .key = "kerf_catalog", .value = .{ .string = "0.1" } },
        .{ .key = "box_anchors", .value = .{ .string = box_anchors } },
        .{ .key = "common", .value = .{ .array = cs } },
        .{ .key = "types", .value = .{ .array = ts } },
    });
}

pub fn appendEntryMarkdown(out: *std.ArrayList(u8), a: Allocator, e: *const Entry) Allocator.Error!void {
    try out.print(a, "### `{s}`: {s}\n\n", .{ e.name, e.summary });
    try out.appendSlice(a, "| param | default | notes |\n|---|---|---|\n");
    for (e.params) |p| try out.print(a, "| `{s}` | {s} | {s} |\n", .{ p.name, p.def, p.desc });
    if (std.mem.eql(u8, e.name, "connector")) {
        try out.appendSlice(a, "\nHardware models (auto-fill width and gauge):\n");
        for (hardware) |h| try out.print(a, "- {s}\n", .{try hardwareLine(a, h)});
    }
    try out.print(a, "\n- Parts: {s}\n- Anchors: {s}\n- Draws: {s}\n\n", .{ e.parts, e.anchors, e.draws });
}

pub fn catalogMarkdown(a: Allocator) Allocator.Error![]const u8 {
    var out: std.ArrayList(u8) = .empty;
    try out.appendSlice(a, "# Kerf component catalog\n\n");
    try out.appendSlice(a, "Coordinates are inches, X right, Y up, Z toward the viewer; sections look along -Z. Lengths accept numbers or strings (\"7 5/8\", \"3'-4\\\"\").\n\n");
    try out.appendSlice(a, "## Common fields\n\n| field | default | notes |\n|---|---|---|\n");
    for (common) |p| try out.print(a, "| `{s}` | {s} | {s} |\n", .{ p.name, p.def, p.desc });
    try out.print(a, "\nBox anchors: {s}.\n\n## Types\n\n", .{box_anchors});
    for (entries) |*e| try appendEntryMarkdown(&out, a, e);
    return out.items;
}

test "catalog renders" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const md = try catalogMarkdown(a);
    try std.testing.expect(std.mem.indexOf(u8, md, "### `truss`") != null);
    const j = try catalogJson(a);
    try std.testing.expect(j.get("types").?.array.len == entries.len);
    try std.testing.expect(allowedKey(find("lumber").?, "plies"));
    try std.testing.expect(allowedKey(find("insulation").?, "height"));
    try std.testing.expect(!allowedKey(find("lumber").?, "bogus"));
}
